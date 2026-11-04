// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Events out (ADR-0195): what happens at the station, as one small set of events that webhooks
//! ([`crate::webhooks`]) carry to other systems.
//!
//! | Event | When | `data` |
//! |---|---|---|
//! | `now_playing.changed` | what listeners are told is playing changes | the now-playing document, as `GET /api/v1/now-playing` gives it |
//! | `on_air.changed` | the source on air changes (`desk`, `automation`, `fallback`, `silence`, `unknown`) | `on_air`, `previous`, `live` (the desk is on air) |
//! | `episode.started` | a scheduled episode's air time arrives | `show`, `series`, `episode` (id, number, title, `air_at_ms`, `minutes`, `kind`) |
//! | `episode.ended` | its length has run out | as `episode.started` |
//! | `caller.queued` | a caller joins the screener's queue | `identity`, `ordinal`, `queue` (callers waiting) |
//!
//! `caller.queued` carries no name, topic or consent: a caller agreed to be live and kept in the
//! archive (ADR-0124's words), not to have what they typed sent to another system. An
//! integration that needs the name reads the studio's view with a `studio:read` token.
//!
//! The sources only *watch*: they follow the hub's view and playout's now-playing through the
//! channels the studio app already uses, and look at the schedule in the database once in a
//! while. Nothing here sits in the hub's path, and nothing waits for a webhook: [`Events::publish`]
//! drops an event on a bounded broadcast channel and returns, and a receiver that falls behind
//! loses the oldest events, not the hub's time.
//!
//! `episode.started` and `episode.ended` follow the schedule (the air time and the length the
//! episode has, or its slot's, or an hour), not a live switch: no one presses "end" on an
//! episode today. They fire for automation and for the desk alike.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::shows as model;

use crate::hub::{Hub, now_ms};

pub const NOW_PLAYING_CHANGED: &str = "now_playing.changed";
pub const ON_AIR_CHANGED: &str = "on_air.changed";
pub const EPISODE_STARTED: &str = "episode.started";
pub const EPISODE_ENDED: &str = "episode.ended";
pub const CALLER_QUEUED: &str = "caller.queued";

/// Every event kind a webhook can ask for.
pub const KINDS: &[(&str, &str)] = &[
    (
        NOW_PLAYING_CHANGED,
        "What listeners are told is playing changed.",
    ),
    (
        ON_AIR_CHANGED,
        "The source on air changed: the desk (a live show), automation, the fallback, or silence.",
    ),
    (EPISODE_STARTED, "A scheduled episode's air time arrived."),
    (EPISODE_ENDED, "A scheduled episode's length ran out."),
    (CALLER_QUEUED, "A caller joined the screener's queue."),
];

pub fn is_kind(name: &str) -> bool {
    KINDS.iter().any(|(k, _)| *k == name)
}

/// One event, as a webhook delivers it (the body is this, as JSON).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Event {
    /// Unique per event, so a receiver can drop a delivery it has already handled.
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub at_ms: u64,
    pub data: Value,
}

/// The bus the sources publish on and the webhook worker (and tests) read.
pub struct Events {
    tx: broadcast::Sender<Arc<Event>>,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(256).0,
        }
    }
}

impl Events {
    /// Tell everyone listening; never waits.
    pub fn publish(&self, kind: &'static str, data: Value) {
        debug_assert!(is_kind(kind));
        let _ = self.tx.send(Arc::new(Event {
            id: Uuid::new_v4(),
            kind,
            at_ms: now_ms(),
            data,
        }));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.tx.subscribe()
    }
}

/// Start the sources. `pool` and `station` are for the schedule; without a database the
/// schedule's events are not made.
pub fn spawn_sources(
    hub: &Arc<Hub>,
    events: &Arc<crate::staff::Staff>,
    pool: Option<sqlx::PgPool>,
    station: StationId,
) {
    if let Some(onair) = hub.onair.get() {
        let mut rx = onair.subscribe();
        let staff = events.clone();
        tokio::spawn(async move {
            let mut last = rx.borrow_and_update().clone();
            while rx.changed().await.is_ok() {
                let now = rx.borrow_and_update().clone();
                if *now == *last {
                    continue;
                }
                if let Ok(data) = serde_json::to_value(&*now) {
                    staff.api.events.publish(NOW_PLAYING_CHANGED, data);
                }
                if now.on_air != last.on_air {
                    staff.api.events.publish(
                        ON_AIR_CHANGED,
                        json!({
                            "on_air": now.on_air,
                            "previous": last.on_air,
                            "live": now.on_air == "desk",
                        }),
                    );
                }
                last = now;
            }
        });
    }

    let mut rx = hub.subscribe();
    let staff = events.clone();
    tokio::spawn(async move {
        let mut seen: HashSet<String> = rx
            .borrow_and_update()
            .callers
            .iter()
            .map(|c| c.identity.clone())
            .collect();
        while rx.changed().await.is_ok() {
            let view = rx.borrow_and_update().clone();
            for c in &view.callers {
                if seen.insert(c.identity.clone()) {
                    staff.api.events.publish(
                        CALLER_QUEUED,
                        json!({
                            "identity": c.identity,
                            "ordinal": c.ordinal,
                            "queue": view.callers.len(),
                        }),
                    );
                }
            }
            // A caller who has gone is forgotten, so a new call is a new arrival.
            seen.retain(|id| view.callers.iter().any(|c| &c.identity == id));
        }
    });

    if let Some(pool) = pool {
        let staff = events.clone();
        tokio::spawn(async move {
            let mut schedule = Schedule::default();
            loop {
                match look(&pool, station, now_ms()).await {
                    Ok(episodes) => {
                        for (kind, data) in schedule.step(&episodes, now_ms()) {
                            staff.api.events.publish(kind, data);
                        }
                    }
                    Err(e) => tracing::debug!(error = %e, "events: cannot read the schedule"),
                }
                tokio::time::sleep(SCHEDULE_EVERY).await;
            }
        });
    }
}

/// How often the schedule is looked at.
const SCHEDULE_EVERY: Duration = Duration::from_secs(15);

/// An episode as the schedule's events need it.
#[derive(Clone, Debug, PartialEq)]
pub struct Aired {
    pub id: Uuid,
    pub start_ms: u64,
    pub end_ms: u64,
    pub data: Value,
}

/// The episodes airing from a day before `now` to a day after, with their ends.
async fn look(
    pool: &sqlx::PgPool,
    station: StationId,
    now: u64,
) -> Result<Vec<Aired>, sqlx::Error> {
    let day = 86_400;
    let from = (now / 1000).saturating_sub(day) as i64;
    let found = model::episodes_between(pool, station, from, from + 2 * day as i64).await?;
    Ok(found
        .into_iter()
        .filter_map(|(show, series, ep)| {
            let start = u64::try_from(ep.air_at?).ok()? * 1000;
            let minutes = ep
                .minutes
                .or(series.slot.and_then(|s| s.minutes))
                .or(show.slot.and_then(|s| s.minutes))
                .unwrap_or(60);
            Some(Aired {
                id: ep.id,
                start_ms: start,
                end_ms: start + u64::from(minutes) * 60_000,
                data: json!({
                    "show": { "slug": show.slug, "name": show.name },
                    "series": series.number,
                    "episode": {
                        "id": ep.id,
                        "number": ep.number,
                        "title": ep.title,
                        "air_at_ms": start,
                        "minutes": minutes,
                        "kind": ep.kind,
                    },
                }),
            })
        })
        .collect())
}

/// What has started and ended between one look at the schedule and the next.
#[derive(Default)]
pub struct Schedule {
    /// When the schedule was last looked at; none before the first look.
    last_ms: Option<u64>,
    /// Episodes that have started and not yet ended.
    running: HashMap<Uuid, Aired>,
}

impl Schedule {
    /// Take a look at `now`. The first look starts nothing: what is already airing is only
    /// remembered, so a restart does not announce it again.
    pub fn step(&mut self, episodes: &[Aired], now: u64) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        let Some(last) = self.last_ms.replace(now) else {
            for e in episodes
                .iter()
                .filter(|e| e.start_ms <= now && now < e.end_ms)
            {
                self.running.insert(e.id, e.clone());
            }
            return out;
        };
        let mut starting: Vec<&Aired> = episodes
            .iter()
            .filter(|e| e.start_ms > last && e.start_ms <= now && !self.running.contains_key(&e.id))
            .collect();
        starting.sort_by_key(|e| e.start_ms);
        for e in starting {
            out.push((EPISODE_STARTED, e.data.clone()));
            self.running.insert(e.id, e.clone());
        }
        let mut ending: Vec<Aired> = self
            .running
            .values()
            .filter(|e| e.end_ms <= now)
            .cloned()
            .collect();
        ending.sort_by_key(|e| e.end_ms);
        for e in ending {
            self.running.remove(&e.id);
            out.push((EPISODE_ENDED, e.data));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(n: u32, start: u64, end: u64) -> Aired {
        Aired {
            id: Uuid::from_u128(u128::from(n)),
            start_ms: start,
            end_ms: end,
            data: json!({ "n": n }),
        }
    }

    fn kinds(v: &[(&'static str, Value)]) -> Vec<(&'static str, u64)> {
        v.iter()
            .map(|(k, d)| (*k, d["n"].as_u64().unwrap()))
            .collect()
    }

    #[test]
    fn an_episode_starts_when_its_time_arrives_and_ends_when_its_length_runs_out() {
        let mut s = Schedule::default();
        let all = [ep(1, 1_000, 5_000), ep(2, 3_000, 4_000)];
        assert!(
            s.step(&all, 500).is_empty(),
            "the first look starts nothing"
        );
        assert!(s.step(&all, 900).is_empty());
        assert_eq!(kinds(&s.step(&all, 1_200)), [(EPISODE_STARTED, 1)]);
        assert!(s.step(&all, 2_000).is_empty());
        assert_eq!(kinds(&s.step(&all, 3_500)), [(EPISODE_STARTED, 2)]);
        assert_eq!(kinds(&s.step(&all, 4_100)), [(EPISODE_ENDED, 2)]);
        assert_eq!(kinds(&s.step(&all, 6_000)), [(EPISODE_ENDED, 1)]);
        assert!(s.step(&all, 9_000).is_empty(), "each is announced once");
    }

    #[test]
    fn what_is_airing_at_the_first_look_is_not_announced_again_but_does_end() {
        let mut s = Schedule::default();
        let all = [ep(1, 100, 5_000)];
        assert!(s.step(&all, 2_000).is_empty());
        assert!(s.step(&all, 3_000).is_empty());
        assert_eq!(kinds(&s.step(&all, 5_100)), [(EPISODE_ENDED, 1)]);
    }

    #[test]
    fn a_short_episode_between_two_looks_starts_and_ends_together() {
        let mut s = Schedule::default();
        let all = [ep(1, 1_000, 1_500)];
        s.step(&all, 500);
        assert_eq!(
            kinds(&s.step(&all, 2_000)),
            [(EPISODE_STARTED, 1), (EPISODE_ENDED, 1)]
        );
    }

    #[test]
    fn publishing_never_waits_and_a_slow_reader_loses_only_the_old() {
        let e = Events::default();
        // No one is listening: fine.
        e.publish(CALLER_QUEUED, json!({}));
        let mut rx = e.subscribe();
        for i in 0..300 {
            e.publish(ON_AIR_CHANGED, json!({ "i": i }));
        }
        let first = rx.try_recv();
        assert!(
            matches!(first, Err(broadcast::error::TryRecvError::Lagged(_))),
            "{first:?}"
        );
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn the_kinds_are_the_five_the_adr_names() {
        let names: Vec<_> = KINDS.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            names,
            [
                "now_playing.changed",
                "on_air.changed",
                "episode.started",
                "episode.ended",
                "caller.queued"
            ]
        );
    }
}
