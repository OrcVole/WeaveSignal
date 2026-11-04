// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The log's generation against a real PostgreSQL (ADR-0181, review 0025 section 4): two
//! generations of the same stretch, or automation topping the log up while a regeneration runs,
//! must not leave two interleaved sets in it. Ignored unless asked for: `just test-db`.

use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::CartsConfig;
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::{Hub, now_ms};
use weavesignal_control::onair::{Links, OnAir};
use weavesignal_control::playout::PlayoutClient;
use weavesignal_control::studio::Studio;
use weavesignal_model::playout::{self as rows, ClockRow, LogItem, PlayoutSettings, Track};
use weavesignal_model::{Station, StationId, ensure_station};

const HOUR_MS: i64 = 3_600_000;

/// A station with a library of twenty tracks, one clock of a single category, and no rules to
/// leave a slot empty; and its on-air layer with no playout behind it.
async fn station() -> (sqlx::PgPool, StationId, std::sync::Arc<OnAir>) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    for n in 0..20 {
        let id = Uuid::new_v4();
        rows::put_track(
            &pool,
            station.id,
            &Track {
                id,
                title: format!("Song {n}"),
                artist: format!("Artist {n}"),
                album: String::new(),
                label: String::new(),
                isrc: None,
                category: "a".into(),
                file: format!("{id}.flac"),
                duration_ms: 180_000,
                gain_db: 0.0,
                loudness_lufs: None,
                true_peak_dbtp: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
    }
    let clock = Uuid::new_v4();
    rows::put_clock(
        &pool,
        station.id,
        &ClockRow {
            id: clock,
            name: "all songs".into(),
            slots_json: serde_json::json!([{"category": "a"}]).to_string(),
        },
    )
    .await
    .unwrap();
    rows::put_settings(
        &pool,
        station.id,
        &PlayoutSettings {
            artist_separation_min: 0,
            title_separation_min: 0,
            album_separation_min: 0,
            default_clock: Some(clock),
            ..PlayoutSettings::default()
        },
    )
    .await
    .unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let onair = OnAir::new(
        station.id,
        pool.clone(),
        Links {
            playout: PlayoutClient::new(None),
            listeners: PlayoutClient::new(None),
            status_url: String::new(),
            library_dir: PathBuf::from("/nonexistent"),
            logger_dir: PathBuf::from("/nonexistent"),
            listener_delay: Duration::ZERO,
            library_lufs: -16.0,
            library_max_seconds: 600.0,
        },
        &hub,
    );
    // The hub holds the on-air layer weakly: keep the hub alive with it.
    std::mem::forget(hub);
    (pool, station.id, onair)
}

async fn all_items(pool: &sqlx::PgPool, station: StationId) -> Vec<LogItem> {
    rows::items(pool, station, 0, 4_000_000_000_000)
        .await
        .unwrap()
}

/// No two items start together, and none starts before the one before it has ended.
fn assert_one_set(items: &[LogItem]) {
    assert!(!items.is_empty(), "the log is empty");
    for w in items.windows(2) {
        assert!(
            w[1].planned_at_ms >= w[0].planned_at_ms + i64::from(w[0].duration_ms),
            "two sets are interleaved: item {} at {} (for {} ms) and item {} at {}",
            w[0].id,
            w[0].planned_at_ms,
            w[0].duration_ms,
            w[1].id,
            w[1].planned_at_ms
        );
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn two_regenerations_at_once_leave_one_set() {
    let (pool, station, onair) = station().await;
    let from = now_ms() as i64 / HOUR_MS * HOUR_MS + HOUR_MS;
    let to = from + 3 * HOUR_MS;
    let mut made = 0;
    for _ in 0..5 {
        let (a, b, c, d) = tokio::join!(
            onair.generate(from, to, true),
            onair.generate(from, to, true),
            onair.generate(from, to, true),
            onair.generate(from, to, true)
        );
        made = a.unwrap().0;
        assert!([b, c, d].into_iter().all(|r| r.unwrap().0 == made));
        let items = all_items(&pool, station).await;
        assert_eq!(
            items.len(),
            made,
            "the last to run replaced the others' items"
        );
        assert_one_set(&items);
    }
    assert!(made > 0);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn automation_topping_up_while_the_week_is_regenerated_leaves_one_set() {
    let (pool, station, onair) = station().await;
    let now = now_ms() as i64;
    let from = now / HOUR_MS * HOUR_MS;
    let (a, b, c) = tokio::join!(
        onair.top_up_log(now),
        onair.top_up_log(now),
        onair.generate(from, from + 5 * HOUR_MS, true)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_one_set(&all_items(&pool, station).await);
}
