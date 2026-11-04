// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Playout's rows (migration 0004) against a real PostgreSQL; `just test-db` runs it.

use uuid::Uuid;
use weavesignal_model::playout::{self as rows, ClockRow, LogItem, Play, PlayoutSettings, Track};
use weavesignal_model::{Station, StationId, ensure_station};

const HOUR: i64 = 3_600_000;

fn track(n: u8, artist: &str) -> Track {
    Track {
        id: Uuid::new_v4(),
        title: format!("Song {n}"),
        artist: artist.into(),
        album: "Hats".into(),
        label: "Linn".into(),
        isrc: Some(format!("GBAAA26000{n:02}")),
        category: "a".into(),
        file: format!("{}.flac", Uuid::new_v4()),
        duration_ms: 200_000,
        gain_db: -3.0,
        loudness_lufs: Some(-20.0),
        true_peak_dbtp: Some(-1.5),
        enabled: true,
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_configured_zone_holds_until_the_studio_sets_one() {
    // Review 0025, S1: automation reads the settings first and creates the row with the
    // column's UTC; that UTC was never chosen, so the configuration's zone replaces it at start.
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    weavesignal_model::MIGRATOR.run(&pool).await.unwrap();
    let id = StationId::new_random();
    ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
        .await
        .unwrap();
    assert_eq!(rows::settings(&pool, id).await.unwrap().time_zone, "UTC");
    // Nobody has chosen it, so the studio asks (ADR-0189).
    assert!(!rows::zone_is_set(&pool, id).await.unwrap());
    assert_eq!(
        rows::seed_time_zone(&pool, id, "Europe/London")
            .await
            .unwrap(),
        "Europe/London"
    );
    assert!(!rows::zone_is_set(&pool, id).await.unwrap());
    // A zone the studio saves stays, UTC included, whatever the configuration says.
    let chosen = PlayoutSettings {
        time_zone: "UTC".into(),
        ..rows::settings(&pool, id).await.unwrap()
    };
    rows::put_settings(&pool, id, &chosen).await.unwrap();
    assert!(rows::zone_is_set(&pool, id).await.unwrap());
    assert_eq!(
        rows::seed_time_zone(&pool, id, "Europe/London")
            .await
            .unwrap(),
        "UTC"
    );
    // A new station takes the configuration's zone straight away.
    let fresh = StationId::new_random();
    ensure_station(&pool, &Station::new(fresh, "Radio Tay").unwrap())
        .await
        .unwrap();
    assert_eq!(
        rows::seed_time_zone(&pool, fresh, "Europe/Dublin")
            .await
            .unwrap(),
        "Europe/Dublin"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn playout_rows_round_trip() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let id = StationId::new_random();
    ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
        .await
        .unwrap();

    // Settings start at the defaults: no retention, live, not a US webcaster.
    let s = rows::settings(&pool, id).await.unwrap();
    assert_eq!(s, PlayoutSettings::default());
    let changed = PlayoutSettings {
        time_zone: "Europe/London".into(),
        mode: "auto".into(),
        ..s
    };
    rows::put_settings(&pool, id, &changed).await.unwrap();
    assert_eq!(rows::settings(&pool, id).await.unwrap(), changed);

    // The library, a clock and the week.
    let a = track(1, "The Blue Nile");
    let b = track(2, "Cocteau Twins");
    rows::put_track(&pool, id, &a).await.unwrap();
    rows::put_track(&pool, id, &b).await.unwrap();
    assert_eq!(rows::tracks(&pool, id).await.unwrap().len(), 2);
    let clock = ClockRow {
        id: Uuid::new_v4(),
        name: "Music".into(),
        slots_json: r#"[{"category":"a"}]"#.into(),
    };
    rows::put_clock(&pool, id, &clock).await.unwrap();
    rows::put_week(&pool, id, &[(0, 7, clock.id), (6, 23, clock.id)])
        .await
        .unwrap();
    assert_eq!(rows::week(&pool, id).await.unwrap().len(), 2);
    assert!(
        rows::clocks(&pool, id).await.unwrap()[0]
            .slots_json
            .contains("category")
    );

    // The log: two items, the first queued then played.
    let t0 = 1_793_000_000_000i64 - 1_793_000_000_000i64 % HOUR;
    let item = |t: &Track, at: i64| LogItem {
        id: 0,
        planned_at_ms: at,
        hour_ms: t0,
        clock: Some(clock.id),
        slot: 0,
        kind: "track".into(),
        track: Some(t.id),
        cart: None,
        category: "a".into(),
        duration_ms: t.duration_ms,
        status: "planned".into(),
        note: String::new(),
    };
    let ids = rows::insert_items(&pool, id, &[item(&a, t0), item(&b, t0 + 200_000)])
        .await
        .unwrap();
    assert_eq!(rows::log_end(&pool, id).await.unwrap(), Some(t0 + 400_000));
    let next = rows::next_to_queue(&pool, id, t0, 5).await.unwrap();
    assert_eq!(next.iter().map(|i| i.id).collect::<Vec<_>>(), ids);
    // Long after both ended, an unplayed item is skipped rather than queued.
    assert!(
        rows::next_to_queue(&pool, id, t0 + 10 * HOUR, 5)
            .await
            .unwrap()
            .is_empty()
    );
    let all = rows::items(&pool, id, t0, t0 + HOUR).await.unwrap();
    assert!(all.iter().all(|i| i.status == "skipped"));
    assert_eq!(
        rows::planned_tracks(&pool, id, t0, t0 + HOUR)
            .await
            .unwrap()
            .len(),
        0
    );
    rows::update_item(&pool, id, ids[0], None, Some("played"), Some("by hand"))
        .await
        .unwrap();
    assert_eq!(
        rows::item(&pool, id, ids[0]).await.unwrap().unwrap().note,
        "by hand"
    );

    // The per-play log: an event read twice is kept once; the next play ends the one before.
    let play = |t: &Track, at: i64| Play {
        id: 0,
        started_at_ms: at,
        ended_at_ms: None,
        kind: "track".into(),
        track: Some(t.id),
        cart: None,
        item: None,
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        label: t.label.clone(),
        isrc: t.isrc.clone(),
        duration_ms: t.duration_ms,
        listeners: 7,
        aired: true,
    };
    assert!(
        rows::record_play(&pool, id, &play(&a, t0), Some("boot:1"), true)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        rows::record_play(&pool, id, &play(&a, t0), Some("boot:1"), true)
            .await
            .unwrap()
            .is_none()
    );
    rows::record_play(&pool, id, &play(&b, t0 + 150_000), Some("boot:2"), true)
        .await
        .unwrap();
    let plays = rows::plays(&pool, id, t0, t0 + HOUR).await.unwrap();
    assert_eq!(plays.len(), 2);
    assert_eq!(plays[0].ended_at_ms, Some(t0 + 150_000));
    assert_eq!(plays[1].ended_at_ms, None);
    assert_eq!(plays[0].isrc.as_deref(), Some("GBAAA2600001"));
    rows::end_open_plays(&pool, id, t0 + 400_000).await.unwrap();
    // It ended by its own length, not later.
    assert_eq!(
        rows::plays(&pool, id, t0, t0 + HOUR).await.unwrap()[1].ended_at_ms,
        Some(t0 + 350_000)
    );
    let used = rows::last_used(&pool, id).await.unwrap();
    assert_eq!(used.len(), 2);

    // Anonymous totals add up; the peak only rises.
    rows::add_listening(&pool, id, t0, 120, 1, 3).await.unwrap();
    rows::add_listening(&pool, id, t0, 30, 0, 2).await.unwrap();
    let h = rows::listening(&pool, id, t0, t0 + HOUR).await.unwrap();
    assert_eq!(
        (h[0].listener_seconds, h[0].sessions, h[0].peak),
        (150, 1, 3)
    );

    // Retention's per-play period: a hold on one play keeps it; the other goes.
    let held: (i64,) = sqlx::query_as(
        "INSERT INTO retention_hold (station_id, reason, placed_by) VALUES ($1, 'complaint', 'test') RETURNING id",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO retention_hold_record (hold_id, record_kind, record_id) VALUES ($1, 'per_play', $2)")
        .bind(held.0)
        .bind(plays[1].id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(rows::sweep_plays(&pool, id, t0 + HOUR).await.unwrap(), 1);
    let left = rows::plays(&pool, id, t0, t0 + HOUR).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, plays[1].id);

    // A track removed leaves the log's words in the per-play log.
    assert!(rows::delete_track(&pool, id, b.id).await.unwrap().is_some());
    assert_eq!(
        rows::plays(&pool, id, t0, t0 + HOUR).await.unwrap()[0].title,
        "Song 2"
    );
}
