// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's control plane.

pub mod api;
pub mod apikey;
pub mod app;
pub mod carts;
pub mod challenge;
pub mod config;
pub mod desk;
pub mod events;
pub mod hub;
pub mod join;
pub mod mail;
pub mod mailkey;
pub mod media;
pub mod onair;
pub mod onair_api;
pub mod open;
pub mod openapi;
mod openapi_page;
pub mod playout;
pub mod ports;
pub mod public;
pub mod retention;
pub mod schedule;
pub mod scopes;
pub mod shows;
pub mod staff;
pub mod steering;
pub mod studio;
pub mod telemetry;
pub mod tokens;
pub mod video;
pub mod web;
pub mod webhooks;
pub mod zone;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;

use crate::config::Config;

/// Bind the control port, naming the clash and the next free suggestion if it is taken.
pub async fn bind(config: &Config) -> anyhow::Result<TcpListener> {
    let spec = ports::spec("control").expect("the control port is in the registry");
    let port = config.ports.get("control");
    let addr = SocketAddr::new(config.listen.internal, port);
    match TcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(e) => match ports::check_one(spec, port, config.listen.internal) {
            Err(clash) => Err(anyhow::anyhow!("{clash}")),
            Ok(()) => Err(e).with_context(|| format!("cannot listen on {addr}")),
        },
    }
}

/// The room's settings with LiveKit's keys, when the secret is set.
pub fn room_state(config: &Config) -> Option<app::RoomState> {
    let secret = config.livekit_secret.clone()?;
    Some(app::RoomState {
        config: config.room.clone(),
        keys: weavesignal_proto::Keys {
            api_key: config.room.api_key.clone(),
            api_secret: secret,
        },
    })
}

/// Load a cart pack into the station's banks, then stop (`weavesignal-control carts load-pack`).
pub async fn load_pack(config: Config, dir: &std::path::Path) -> anyhow::Result<usize> {
    let station = config.station()?;
    let url = config.database_url.as_ref().with_context(|| {
        format!(
            "carts are kept in the database: set {}",
            config::DATABASE_URL_ENV
        )
    })?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(url)
        .await
        .context("cannot connect to the database")?;
    let station = weavesignal_model::ensure_station(&pool, &station)
        .await
        .context("cannot apply the migrations")?;
    let hub = hub::Hub::new(
        &station.name,
        studio::Studio::default(),
        desk::DeskClient::new(None),
        None,
        std::time::Duration::from_millis(250),
        carts::CartsLink::new(config.carts.clone(), Some((pool, station.id))),
    );
    hub.reload_carts().await?;
    Ok(hub.load_pack(dir).await?)
}

/// Run the control plane until SIGINT or SIGTERM.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let listener = bind(&config).await?;
    let mut station = config.station()?;

    let db = match &config.database_url {
        None => {
            tracing::warn!(
                "no database configured ({}); running without one",
                config::DATABASE_URL_ENV
            );
            None
        }
        Some(url) => {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(8)
                .connect(url)
                .await
                .context("cannot connect to the database")?;
            station = weavesignal_model::ensure_station(&pool, &station)
                .await
                .context("cannot apply the migrations")?;
            tracing::info!("database migrated");
            Some(pool)
        }
    };

    let addr = listener.local_addr()?;
    tracing::info!(%addr, station_id = %station.id, station = %station.name, "control plane listening");
    let room = room_state(&config);
    if room.is_none() {
        tracing::warn!(
            "LiveKit's secret is not set ({}); room tokens are off",
            config::LIVEKIT_SECRET_ENV
        );
    }
    let room_admin = config.livekit_secret.clone().map(|secret| hub::RoomAdmin {
        api_url: config.room.api_url.clone(),
        room: config.room.name.clone(),
        keys: weavesignal_proto::Keys {
            api_key: config.room.api_key.clone(),
            api_secret: secret,
        },
    });
    let mut studio = studio::Studio::new(&config.studio.show, config.studio.rundown.clone());
    studio.set_listener_delay(config.studio.listener_delay_ms);
    studio.set_edges(config.steering.enabled());
    for b in &config.studio.banned {
        studio.ban(
            &b.identity,
            studio::Ban {
                date: b.date.clone(),
                reason: b.reason.clone(),
            },
        );
    }
    // Carts sound in the desk (ADR-0117); the desk link fires them.
    let carts = carts::CartsLink::new(
        config.carts.clone(),
        db.clone().map(|pool| (pool, station.id)),
    );
    let hub = hub::Hub::new(
        &station.name,
        studio,
        desk::DeskClient::new(config.desk.socket.as_deref()),
        room_admin,
        std::time::Duration::from_millis(config.desk.poll_ms),
        carts,
    );
    hub.spawn_poller();
    hub.start_carts().await;
    onair_api::LIBRARY_UPLOAD_MB.store(
        config.playout.library_max_upload_mb,
        std::sync::atomic::Ordering::Relaxed,
    );
    // The station's one time zone (review 0025, S1): the one set in the studio's settings, or
    // until someone sets one, `publishing.zone`. Before the playout layer starts, which reads
    // the settings too.
    hub.zone.set(&config.publishing.zone);
    if let Some(pool) = &db {
        match weavesignal_model::playout::seed_time_zone(pool, station.id, &config.publishing.zone)
            .await
        {
            Ok(zone) => {
                if zone != config.publishing.zone {
                    tracing::info!(
                        zone = %zone,
                        configured = %config.publishing.zone,
                        "the station's time zone is the one set in the studio's settings, not `publishing.zone`"
                    );
                }
                hub.zone.set(&zone);
            }
            Err(e) => tracing::error!(error = %e, "cannot read the station's time zone"),
        }
    }
    // The playout layer (M4): automation, the per-play log, audience, the logger, now playing.
    match (&db, config.playout.socket.as_deref()) {
        (Some(pool), Some(socket)) => {
            let links = onair::Links {
                playout: playout::PlayoutClient::new(Some(socket)),
                listeners: playout::PlayoutClient::new(config.playout.listeners_socket.as_deref()),
                status_url: format!(
                    "http://{}/status.json",
                    SocketAddr::new(config.listen.internal, config.ports.get("listeners"))
                ),
                library_dir: config.playout.library_dir.clone(),
                // The logger writes into retention's recordings directory, which the sweeper
                // deletes from (the listener process's `logger.dir`).
                logger_dir: config.retention.recordings_dir.join(onair::LOGGER_SUBDIR),
                listener_delay: std::time::Duration::from_millis(config.studio.listener_delay_ms),
                library_lufs: config.playout.library_lufs,
                library_max_seconds: config.playout.library_max_seconds,
            };
            onair::OnAir::new(station.id, pool.clone(), links, &hub).spawn(&hub);
        }
        _ => tracing::warn!(
            "automation, the per-play log, audience counts and the logger need the database and playout's socket"
        ),
    }
    // Shows, cards and feeds (M8): their own state and routes, merged below.
    // Staff sessions (ADR-0127): one set, shared by every route that checks them.
    let mailer =
        staff::Mailer::from_config(&config, &station.name).map_err(|e| anyhow::anyhow!(e))?;
    if mailer.is_some() {
        tracing::info!("staff sign-in codes by email are on");
    }
    let api = Arc::new(api::Api::new(
        db.clone(),
        station.id,
        config.api_key.clone(),
        config.api.clone(),
        config.join.forwarded_for,
    ));
    let staff = Arc::new(
        staff::Staff::new(db.clone())
            .with_mailer(mailer)
            .with_api(api),
    );
    let restored_tokens = staff.api.restore().await;
    if restored_tokens > 0 {
        tracing::info!(
            tokens = restored_tokens,
            "API tokens read back from the database"
        );
    }
    if staff.api.key().is_none() {
        tracing::warn!(
            "API tokens and webhooks are off: set {} (or the station key, {})",
            apikey::ApiKey::ENV,
            config::ADMIN_TOKEN_ENV
        );
    } else if std::env::var(apikey::ApiKey::ENV).map_or(true, |v| v.trim().is_empty()) {
        // ADR-0202: a station installed before every route made a key keeps working on the
        // derived one, and is told why it should set its own.
        tracing::warn!(
            "API tokens use a key derived from the station key ({}): rotating that revokes them; \
             set {} to keep them across a rotation (setting it revokes the tokens made until now)",
            config::ADMIN_TOKEN_ENV,
            apikey::ApiKey::ENV
        );
    }
    // Events out (ADR-0195): the sources watch the hub, playout and the schedule; the worker
    // delivers to webhooks. Neither is in the hub's path.
    events::spawn_sources(&hub, &staff, db.clone(), station.id);
    webhooks::spawn_worker(staff.clone(), webhooks::BACKOFF.to_vec());
    let gate_staff = staff.clone();
    let restored_sessions = staff.restore(station.id).await;
    if restored_sessions > 0 {
        tracing::info!(
            sessions = restored_sessions,
            "staff sessions read back from the database"
        );
    }
    let shows_state = Arc::new(shows::ShowsState {
        station: station.clone(),
        db: db.clone(),
        admin_token: config.admin_token.clone(),
        staff: staff.clone(),
        publishing: config.publishing.clone(),
        zone: hub.zone.clone(),
    });
    let state = Arc::new(app::AppState {
        station,
        db,
        room,
        admin_token: config.admin_token.clone(),
        staff,
        hub,
        join: config.join.clone(),
        video: config.video.clone(),
        joins: Default::default(),
        turn_tls_port: config.ports.get("turn_tls"),
        steering: config.steering.clone(),
    });
    // Callers waiting when the control plane stopped keep their consent (M3's known fault).
    let restored = retention::restore_consents(&state).await;
    if restored > 0 {
        tracing::info!(
            callers = restored,
            "callers' consent read back from the database"
        );
    }
    let ended = public::restore_ended_passes(&state).await;
    if ended > 0 {
        tracing::info!(passes = ended, "ended passes read back from the database");
    }
    let link = Arc::new(retention::RetentionLink {
        recordings_dir: config.retention.recordings_dir.clone(),
        sweep_every: std::time::Duration::from_secs(config.retention.sweep_every_s),
    });
    // The slate's still follows the show on air (ADR-0200).
    video::spawn_slate_still(state.clone());
    if let Some(pool) = &state.db {
        retention::warn_at_start(pool, state.station.id).await;
        retention::spawn_sweeper(pool.clone(), state.station.id, link.clone());
    }
    if !config.join.forwarded_for {
        tracing::warn!(
            "call-in counts calls per address only for direct connections; behind the station's proxy set `join.forwarded_for = true`, or only the station-wide limit applies"
        );
    }
    // Content steering among the edges (M7): its own state, routes and health checks.
    let steering_state = Arc::new(steering::SteeringState::new(
        config.steering.clone(),
        config.admin_token.clone(),
    ));
    steering::spawn_checker(steering_state.clone());
    // The public pages, when a build is configured (M3's known fault: nothing served them).
    let site = match &config.web.dir {
        Some(dir) => {
            let csp = web::csp_with_hls(
                &config.room.public_url,
                &config.join.stream_url,
                &config.join.hls_url,
            );
            let site = web::Site::load_listed(dir, &csp, config.web.listed)
                .with_context(|| format!("cannot serve the public pages from {}", dir.display()))?;
            tracing::info!(dir = %dir.display(), files = site.len(), "serving the public pages");
            Some(Arc::new(site))
        }
        None => {
            tracing::info!("no `web.dir`: the public pages are not served here");
            None
        }
    };
    let mut router = app::router(state);
    if let Some(site) = site {
        router = router.fallback(web::fallback(site));
    }
    // The peer's address, for call-in's rate limit.
    let routes = router
        .layer(axum::Extension(link))
        .merge(shows::router(shows_state))
        .merge(steering::router(steering_state));
    // API tokens, the open routes' CORS and limits (ADR-0195): over every route.
    let service = api::layer(routes, gate_staff)
        // Search engines: unlisted unless the station says otherwise (ADR-0133).
        .layer(axum::middleware::from_fn_with_state(
            config.web.listed,
            web::robots,
        ))
        .into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, service)
        .with_graceful_shutdown(shutdown())
        .await?;
    tracing::info!("control plane stopped");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
}
