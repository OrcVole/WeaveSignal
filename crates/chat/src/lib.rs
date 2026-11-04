// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's live chat (M6): its own Axum process in the workspace (ADR-0031), on the
//! station's database, beside the control plane.
//!
//! | Module | What |
//! |---|---|
//! | [`rules`] | the moderation rules a message meets before anyone sees it (pure) |
//! | [`wire`] | the socket's frames, with epoch and offset recovery and two timestamps |
//! | [`room`] | the room in memory: fan-out through bounded queues, the recovery ring, holds |
//! | [`service`] | posting, the hold, moderation, statements of reasons, reports, erasure |
//! | [`legal`] | the legal toolset's words: reasons, statements, the risk-assessment template |
//! | [`key`] | the secret that keys the hash of chatters' email addresses (ADR-0125) |
//! | [`store`] | the rows (migration 0006) and the retention sweep |
//! | [`control`] | the control plane's studio view, followed for the modes, show and delay |
//! | [`http`], [`ws`] | the routes and the socket |

pub mod config;
pub mod control;
pub mod http;
pub mod key;
pub mod legal;
pub mod limits;
pub mod mail;
pub mod room;
pub mod rules;
pub mod service;
pub mod store;
pub mod wire;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::watch;

use crate::config::Config;
use crate::room::Audience;
use crate::service::Chat;
use crate::wire::ServerFrame;

/// Build the service: connect, migrate, load the room, follow the control plane.
pub async fn start(mut config: Config) -> anyhow::Result<Arc<http::AppState>> {
    let station = config.station()?;
    // Fail closed, before anything else: without the key an address could only be kept as a
    // plain hash, which the ADR forbids, so there is no fallback.
    let email_key = config.email_key.take().with_context(|| {
        format!(
            "the chat keeps email addresses as a keyed hash: set {} to a secret of at least {} characters, e.g. `openssl rand -hex 32` (keep it with the database's backups; a different key finds none of the existing accounts)",
            key::EmailKey::ENV,
            key::EmailKey::MIN_LEN
        )
    })?;
    let url = config.database_url.clone().with_context(|| {
        format!(
            "the chat keeps accounts and reports: set {}",
            config::DATABASE_URL_ENV
        )
    })?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .connect(&url)
        .await
        .context("cannot connect to the database")?;
    let station = weavesignal_model::ensure_station(&pool, &station)
        .await
        .context("cannot apply the migrations")?;
    let (tx, rx) = watch::channel(control::Studio::default());
    if !config.control.url.trim().is_empty() {
        control::follow(config.control.url.clone(), config.admin_token.clone(), tx);
    } else {
        tracing::warn!("no control plane set ([control] url); the chat uses its default modes");
        std::mem::forget(tx);
    }
    let admin_token = config.admin_token.clone();
    if admin_token.is_none() {
        tracing::warn!(
            "{} is not set: the studio's chat routes are off",
            config::ADMIN_TOKEN_ENV
        );
    }
    match store::legacy_email_hashes(&pool, station.id).await {
        Ok(0) => {}
        Ok(n) => tracing::warn!(
            accounts = n,
            "accounts made before ADR-0125 still hold an unkeyed email hash; each is re-keyed at its owner's next sign-in"
        ),
        Err(e) => tracing::warn!("cannot count unkeyed email hashes: {e}"),
    }
    let chat = Chat::new(station, pool, config, email_key, rx).await?;
    spawn_tasks(&chat);
    let limits = http::Limiters::new(&chat.config.limits);
    let state = Arc::new(http::AppState {
        chat,
        admin_token,
        sockets: Default::default(),
        codes: Default::default(),
        limits,
    });
    // Addresses are kept only while their window lasts (ADR-0120): forget passed ones each minute.
    let s = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tick.tick().await;
            let now = service::now_ms();
            s.limits.forget_old(now);
            s.chat.forget_old_limits(now);
        }
    });
    Ok(state)
}

/// The service's background work: the rules when the studio's view changes, the reader count,
/// and the retention sweep.
fn spawn_tasks(chat: &Arc<Chat>) {
    let c = chat.clone();
    tokio::spawn(async move {
        let mut rx = c.studio.clone();
        while rx.changed().await.is_ok() {
            c.announce_rules();
        }
    });
    let c = chat.clone();
    tokio::spawn(async move {
        let mut last = usize::MAX;
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        loop {
            tick.tick().await;
            let mut room = c.room.lock().expect("room");
            let n = room.count();
            if n != last {
                last = n;
                room.send(Audience::All, &ServerFrame::Count { n });
            }
        }
    });
    let c = chat.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        loop {
            tick.tick().await;
            // The station may have changed its Jurisdiction since the last sweep.
            let r = match c.reload_retention().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "cannot read the retention setting; nothing is swept");
                    continue;
                }
            };
            let secs = |d: Duration| (d != Duration::MAX).then_some(d.as_secs() as i64);
            match store::sweep(
                &c.db,
                c.station.id,
                secs(r.messages.messages()),
                r.messages == config::Keep::None,
                secs(r.moderation.moderation()),
                r.addresses.addresses().map(|d| secs(d).unwrap_or(i64::MAX)),
            )
            .await
            {
                Ok(s) if s != store::Swept::default() => tracing::info!(?s, "retention sweep"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "the retention sweep failed"),
            }
        }
    });
}

/// Run until SIGINT or SIGTERM.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let addr = SocketAddr::new(config.listen.address, config.listen.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr} (ports.chat)"))?;
    let state = start(config).await?;
    tracing::info!(%addr, station = %state.chat.station.name, "chat listening");
    let app = http::router(state).into_make_service_with_connect_info::<SocketAddr>();
    // Small frames, sent at once: without TCP_NODELAY a chat line can wait 40 ms on a delayed
    // acknowledgement.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
        let _ = tcp.set_nodelay(true);
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let ctrl_c = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            #[cfg(unix)]
            let term = async {
                if let Ok(mut s) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                {
                    s.recv().await;
                }
            };
            #[cfg(not(unix))]
            let term = std::future::pending::<()>();
            tokio::select! {
                () = ctrl_c => {},
                () = term => {},
            }
        })
        .await?;
    tracing::info!("chat stopped");
    Ok(())
}
