// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The control plane's studio view, followed: it keeps the room's modes as the screener
//! switches them (S, K, L, influx's G G with its undo, ADR-0068), the show's title, the
//! presenter's name and the listener delay (ADR-0103). The chat reads them from the view's
//! server-sent events with the station's admin bearer and never writes back, so the modes have
//! one home and the screener's keys stay as they are.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::watch;

/// What the chat takes from the studio's view.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Studio {
    /// Whether the control plane's view arrived (false: the chat uses its defaults).
    pub linked: bool,
    pub slow: bool,
    pub hold_new: bool,
    pub links_trusted: bool,
    pub accounts_only: bool,
    pub min_age_24h: bool,
    pub influx: bool,
    pub show: String,
    pub presenter: String,
    pub listener_delay_ms: u64,
}

#[derive(Deserialize)]
struct View {
    #[serde(default)]
    show: Option<ShowView>,
    #[serde(default)]
    seats: Option<Seats>,
    chat: ChatView,
    #[serde(default)]
    listener_delay: Option<Delay>,
}
#[derive(Deserialize)]
struct ShowView {
    #[serde(default)]
    title: String,
}
#[derive(Deserialize)]
struct Seats {
    #[serde(default)]
    presenter: Option<String>,
    #[serde(default)]
    presenter_name: String,
}
#[derive(Deserialize)]
struct ChatView {
    #[serde(default)]
    slow: bool,
    #[serde(default)]
    hold_new: bool,
    #[serde(default)]
    links_trusted: bool,
    #[serde(default)]
    accounts_only: bool,
    #[serde(default)]
    min_age_24h: bool,
    #[serde(default)]
    influx: bool,
}
#[derive(Deserialize)]
struct Delay {
    ms: u64,
}

/// Read one `state` event's JSON into what the chat needs.
pub fn parse(json: &str) -> Option<Studio> {
    let v: View = serde_json::from_str(json).ok()?;
    let presenter = v
        .seats
        .map(|s| {
            if s.presenter_name.trim().is_empty() {
                s.presenter.unwrap_or_default()
            } else {
                s.presenter_name
            }
        })
        .unwrap_or_default();
    Some(Studio {
        linked: true,
        slow: v.chat.slow,
        hold_new: v.chat.hold_new,
        links_trusted: v.chat.links_trusted,
        accounts_only: v.chat.accounts_only,
        min_age_24h: v.chat.min_age_24h,
        influx: v.chat.influx,
        show: v.show.map(|s| s.title).unwrap_or_default(),
        presenter,
        listener_delay_ms: v.listener_delay.map(|d| d.ms).unwrap_or(15_000),
    })
}

/// Follow the control plane on a thread of its own, forever: each view that changes what the
/// chat needs is sent on `tx`; a lost link marks the studio unlinked and retries every 2 s.
pub fn follow(url: String, bearer: Option<String>, tx: watch::Sender<Studio>) {
    std::thread::Builder::new()
        .name("chat-control-link".into())
        .spawn(move || {
            let events = format!("{}/api/v1/studio/events", url.trim_end_matches('/'));
            let mut warned = false;
            loop {
                let mut req = ureq::get(&events).header("Accept", "text/event-stream");
                if let Some(b) = &bearer {
                    req = req.header("Authorization", &format!("Bearer {b}"));
                }
                match req.call() {
                    Ok(resp) => {
                        warned = false;
                        tracing::info!(%events, "following the control plane's studio view");
                        let reader = BufReader::new(resp.into_body().into_reader());
                        let mut data = String::new();
                        for line in reader.lines() {
                            let Ok(line) = line else { break };
                            if let Some(d) = line.strip_prefix("data:") {
                                data.push_str(d.trim_start());
                            } else if line.is_empty() && !data.is_empty() {
                                if let Some(s) = parse(&data) {
                                    tx.send_if_modified(|cur| {
                                        let changed = *cur != s;
                                        if changed {
                                            *cur = s;
                                        }
                                        changed
                                    });
                                }
                                data.clear();
                            }
                        }
                    }
                    Err(e) => {
                        if !warned {
                            tracing::warn!(%events, error = %e, "cannot follow the control plane; the chat uses its default modes until it can");
                            warned = true;
                        }
                    }
                }
                tx.send_if_modified(|cur| {
                    let was = cur.linked;
                    cur.linked = false;
                    was
                });
                if tx.is_closed() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("a thread for the control link");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_studio_view_gives_the_modes_show_and_delay() {
        let s = parse(
            r#"{"station":"Kettle FM","show":{"title":"The Kettle Phone-In","rundown":[]},
                "seats":{"presenter":"presenter-1","screener":null,"presenter_name":"Rory Bruce","screener_name":""},
                "chat":{"slow":true,"hold_new":false,"links_trusted":true,"accounts_only":true,"min_age_24h":true,"influx":true,"influx_since_ms":5},
                "listener_delay":{"ms":15000,"measured":false},"other":1}"#,
        )
        .unwrap();
        assert!(s.linked && s.slow && s.influx && s.accounts_only && s.min_age_24h);
        assert_eq!(s.show, "The Kettle Phone-In");
        assert_eq!(s.presenter, "Rory Bruce");
        assert_eq!(s.listener_delay_ms, 15_000);
        assert!(parse("{}").is_none());
    }
}
