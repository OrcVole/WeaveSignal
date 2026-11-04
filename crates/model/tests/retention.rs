// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Retention: the fourteen presets against recce 27's table, and (against a real PostgreSQL,
//! ignored unless asked for, `just test-db`) the sweeper, holds, consent linked to recordings,
//! and consent read back after a restart.

use std::time::{SystemTime, UNIX_EPOCH};

use weavesignal_model::retention::{
    self, CUSTOM, HoldRecord, HoldTarget, Kind, NO_RETENTION, NewHold, NewRecording, PRESETS,
    Period, RecordingKind, Setting,
};
use weavesignal_model::{ConsentRecord, Station, StationId, ensure_station, record_consent};

// ---- the presets, transcribed a second time from recce 27's "Proposed presets" table ----

/// One row of recce 27's table: logger, per-play, chat ("A" or 0), moderation, consent
/// ("R+42" or with the call), raw access logs; aggregate stats are "no limit" in every row.
struct Row {
    key: &'static str,
    logger: u32,
    per_play: u32,
    chat_with_archive: bool,
    moderation: u32,
    consent_r42: bool,
    access: u32,
    /// The floor the rule column names for the logger, if any.
    logger_floor: Option<u32>,
    complaint_hold_days: Option<u32>,
}

const TABLE: [Row; 13] = [
    Row {
        key: "none",
        logger: 0,
        per_play: 0,
        chat_with_archive: false,
        moderation: 0,
        consent_r42: false,
        access: 0,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "uk_ofcom",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: Some(42),
        complaint_hold_days: None,
    },
    Row {
        key: "uk_internet",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "us_webcaster",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "us_nce",
        logger: 42,
        per_play: 1461,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 1461,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "eu_gdpr",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "ie_licensed",
        logger: 90,
        per_play: 2192,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: Some(90),
        complaint_hold_days: None,
    },
    Row {
        key: "ie_internet",
        logger: 42,
        per_play: 2192,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "de",
        logger: 90,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 7,
        logger_floor: Some(90),
        complaint_hold_days: None,
    },
    Row {
        key: "ca_licensed",
        logger: 28,
        per_play: 2192,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: Some(28),
        complaint_hold_days: Some(56),
    },
    Row {
        key: "ca_internet",
        logger: 28,
        per_play: 2192,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
    Row {
        key: "au_licensed",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: Some(42),
        complaint_hold_days: Some(60),
    },
    Row {
        key: "au_internet",
        logger: 42,
        per_play: 1096,
        chat_with_archive: true,
        moderation: 365,
        consent_r42: true,
        access: 30,
        logger_floor: None,
        complaint_hold_days: None,
    },
];

#[test]
fn each_preset_has_recce_27s_values_and_a_source_for_each() {
    let names = [
        "No retention",
        "UK, Ofcom-licensed",
        "UK, internet-only",
        "US webcaster (statutory licence)",
        "US noncommercial educational webcaster",
        "EU, GDPR only",
        "Ireland, licensed",
        "Ireland, internet-only (audio)",
        "Germany",
        "Canada, licensed",
        "Canada, internet-only",
        "Australia, licensed",
        "Australia, internet-only",
        "Custom",
    ];
    assert_eq!(
        PRESETS.iter().map(|p| p.name).collect::<Vec<_>>(),
        names,
        "ADR-0114's fourteen, in order"
    );
    for row in &TABLE {
        let p = retention::preset(row.key).unwrap_or_else(|| panic!("{}", row.key));
        let got = p.periods();
        let days = Period::days;
        assert_eq!(
            got.programme_recordings,
            days(row.logger),
            "{} logger",
            row.key
        );
        assert_eq!(got.per_play_log, days(row.per_play), "{} per-play", row.key);
        let chat = if row.chat_with_archive {
            Period::WithArchive
        } else {
            days(0)
        };
        assert_eq!(got.chat_messages, chat, "{} chat", row.key);
        assert_eq!(
            got.moderation_records,
            days(row.moderation),
            "{} moderation",
            row.key
        );
        let consent = if row.consent_r42 {
            Period::AfterRecordings { days: 42 }
        } else {
            Period::WithCall
        };
        assert_eq!(got.consent, consent, "{} consent", row.key);
        assert_eq!(
            got.call_recordings,
            days(0),
            "{}: off-air call audio is 0 everywhere",
            row.key
        );
        assert_eq!(
            got.listener_statistics,
            Period::NoLimit,
            "{}: counts stay (ADR-0115)",
            row.key
        );
        assert_eq!(got.access_logs, days(row.access), "{} access logs", row.key);
        assert_eq!(
            p.cell(Kind::ProgrammeRecordings).floor.map(|f| f.days),
            row.logger_floor,
            "{} logger floor",
            row.key
        );
        assert_eq!(
            p.complaint_hold.and_then(|h| h.days),
            row.complaint_hold_days,
            "{} complaint hold",
            row.key
        );
        for k in Kind::ALL {
            assert!(p.cell(k).source.len() > 10, "{} {k}: a source", row.key);
        }
    }
    // The statutory floors recce 27 names, where a preset carries them.
    let floor = |key: &str, k: Kind| {
        retention::preset(key)
            .unwrap()
            .cell(k)
            .floor
            .map(|f| f.days)
    };
    assert_eq!(floor("us_webcaster", Kind::PerPlayLog), Some(1096));
    assert_eq!(floor("us_nce", Kind::AccessLogs), Some(1461));
    assert_eq!(floor("ca_licensed", Kind::PerPlayLog), Some(2192));
    assert_eq!(floor("uk_internet", Kind::ProgrammeRecordings), None);
}

// ---- against PostgreSQL ----

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

const DAY: u64 = 24 * 3600 * 1000;

async fn station(name: &str) -> (sqlx::PgPool, StationId) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let s = ensure_station(&pool, &Station::new(StationId::new_random(), name).unwrap())
        .await
        .unwrap();
    (pool, s.id)
}

fn preset_setting(key: &str) -> Setting {
    Setting {
        preset: key.into(),
        ..Setting::default()
    }
}

async fn recording(
    pool: &sqlx::PgPool,
    st: StationId,
    dir: &std::path::Path,
    kind: RecordingKind,
    name: &str,
    started_ago_days: u64,
    ended_ago_days: Option<u64>,
) -> i64 {
    std::fs::write(dir.join(name), b"audio").unwrap();
    let now = now_ms();
    retention::add_recording(
        pool,
        st,
        &NewRecording {
            kind,
            label: name.into(),
            file: Some(name.into()),
            started_at_ms: now - started_ago_days * DAY - 3_600_000,
            ended_at_ms: ended_ago_days.map(|d| now - d * DAY),
        },
    )
    .await
    .unwrap()
}

async fn deleted(pool: &sqlx::PgPool, id: i64) -> bool {
    let row: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT extract(epoch FROM deleted_at)::double precision FROM recording WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .unwrap();
    // A row purged after deletion counts as deleted.
    row.is_none_or(|(at,)| at.is_some())
}

fn tmpdir(what: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "weavesignal-retention-{what}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_sweeper_deletes_only_expired_unheld_records() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("sweep");
    let setting = retention::save_setting(&pool, st, &preset_setting("uk_internet"), "Morag Ross")
        .await
        .unwrap();
    let p = RecordingKind::Programme;
    let expired = recording(&pool, st, &dir, p, "expired.opus", 50, Some(50)).await;
    let recent = recording(&pool, st, &dir, p, "recent.opus", 10, Some(10)).await;
    let open = recording(&pool, st, &dir, p, "open.opus", 60, None).await;
    let named = recording(&pool, st, &dir, p, "named.opus", 50, Some(50)).await;
    let windowed = recording(&pool, st, &dir, p, "windowed.opus", 45, Some(45)).await;
    let offair = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Call,
        "offair.opus",
        0,
        Some(0),
    )
    .await;
    // Another station's expired recording is not this sweep's to touch.
    let (_, other) = station("Other FM").await;
    let theirs = recording(&pool, other, &dir, p, "theirs.opus", 50, Some(50)).await;

    // A hold naming one record, and one covering the 45-days-ago broadcast.
    let now = now_ms();
    let h1 = retention::place_hold(
        &pool,
        st,
        &NewHold {
            reason: "Complaint about the 14:00 phone-in".into(),
            by: "Morag Ross".into(),
            from_ms: None,
            to_ms: None,
            records: vec![HoldRecord {
                kind: HoldTarget::Recording,
                id: named,
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(h1.recordings, 1);
    let h2 = retention::place_hold(
        &pool,
        st,
        &NewHold {
            reason: "Ofcom fairness complaint".into(),
            by: "Morag Ross".into(),
            from_ms: Some(now - 45 * DAY - 1_800_000),
            to_ms: Some(now - 45 * DAY - 1_200_000),
            records: vec![],
        },
    )
    .await
    .unwrap();
    // The window covers the 45-days-ago recording, and the open one still recording then.
    assert_eq!(h2.recordings, 2);

    // A dry run says what would go and changes nothing.
    let dry = retention::sweep(&pool, st, &setting, Some(&dir), true)
        .await
        .unwrap();
    assert_eq!((dry.recordings_deleted, dry.held), (2, 2), "{dry:?}");
    assert!(!deleted(&pool, expired).await);

    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!(r.recordings_deleted, 2, "{r:?}");
    assert_eq!(r.held, 2, "{r:?}");
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(deleted(&pool, expired).await && !dir.join("expired.opus").exists());
    assert!(
        deleted(&pool, offair).await,
        "off-air call audio is kept 0 days"
    );
    for (id, f) in [
        (recent, "recent.opus"),
        (open, "open.opus"),
        (named, "named.opus"),
        (windowed, "windowed.opus"),
    ] {
        assert!(!deleted(&pool, id).await, "{f} is kept");
        assert!(dir.join(f).exists(), "{f}'s file is kept");
    }
    assert!(!deleted(&pool, theirs).await && dir.join("theirs.opus").exists());
    let last = retention::last_sweep(&pool, st).await.unwrap().unwrap();
    assert_eq!((last.recordings_deleted, last.held), (2, 2));

    // Released, the normal period resumes: both go at the next sweep.
    let released = retention::release_hold(&pool, st, h1.id, "Morag Ross", "Complaint closed")
        .await
        .unwrap();
    assert!(released.released_at_ms.is_some());
    assert!(matches!(
        retention::release_hold(&pool, st, h1.id, "Morag Ross", "again").await,
        Err(retention::HoldError::Released(_))
    ));
    retention::release_hold(&pool, st, h2.id, "Iain Gunn", "Ofcom closed the case")
        .await
        .unwrap();
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!((r.recordings_deleted, r.held), (2, 0), "{r:?}");
    assert!(deleted(&pool, named).await && deleted(&pool, windowed).await);
    // The log keeps both holds, with who released them and why.
    let holds = retention::holds(&pool, st).await.unwrap();
    assert_eq!(holds.len(), 2);
    assert!(
        holds
            .iter()
            .all(|h| h.released_by.is_some() && h.release_reason.is_some())
    );
    // Another station cannot see or release them.
    assert!(matches!(
        retention::hold(&pool, other, h1.id).await,
        Err(retention::HoldError::NotFound(_))
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_missing_file_is_counted_and_a_file_outside_the_directory_is_refused() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("missing");
    let setting = preset_setting(NO_RETENTION);
    let id = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Programme,
        "gone.opus",
        1,
        Some(1),
    )
    .await;
    std::fs::remove_file(dir.join("gone.opus")).unwrap();
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!((r.recordings_deleted, r.files_missing), (1, 1), "{r:?}");
    assert!(deleted(&pool, id).await);
    // Without a directory, a recording with a file is kept and reported.
    let kept = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Programme,
        "kept.opus",
        1,
        Some(1),
    )
    .await;
    let r = retention::sweep(&pool, st, &setting, None, false)
        .await
        .unwrap();
    assert_eq!(r.recordings_deleted, 0);
    assert_eq!(r.errors.len(), 1, "{r:?}");
    assert!(!deleted(&pool, kept).await);
    // The database refuses a path out of the directory, as the model does.
    let bad = retention::add_recording(
        &pool,
        st,
        &NewRecording {
            kind: RecordingKind::Programme,
            label: String::new(),
            file: Some("../../etc/passwd".into()),
            started_at_ms: now_ms() - DAY,
            ended_at_ms: Some(now_ms()),
        },
    )
    .await;
    assert!(bad.is_err());
    let raw = sqlx::query(
        "INSERT INTO recording (station_id, kind, file, started_at) VALUES ($1, 'programme', '/etc/passwd', now())",
    )
    .bind(st.as_uuid())
    .execute(&pool)
    .await;
    assert!(raw.is_err(), "the CHECK holds");
    std::fs::remove_dir_all(&dir).ok();
}

/// A stretch on air, `from` to `to` after the consent was given.
async fn on_air_between(pool: &sqlx::PgPool, consent_id: i64, from: &str, to: &str) {
    sqlx::query(
        "INSERT INTO consent_on_air (consent_id, from_at, to_at)
         SELECT id, accepted_at + $2::interval, accepted_at + $3::interval FROM caller_consent WHERE id = $1",
    )
    .bind(consent_id)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .unwrap();
}

async fn linked_to(pool: &sqlx::PgPool, consent_id: i64, recording_id: i64) -> bool {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM consent_recording WHERE consent_id = $1 AND recording_id = $2",
    )
    .bind(consent_id)
    .bind(recording_id)
    .fetch_one(pool)
    .await
    .unwrap();
    n == 1
}

fn consent(identity: &str, accepted_ms: u64) -> ConsentRecord {
    ConsentRecord {
        identity: identity.into(),
        name: "Jessie Brown".into(),
        topic: "No bus after 23:10".into(),
        given_by: "caller",
        words: "Your call may be broadcast live and recorded.".into(),
        words_version: "0123456789abcdef".into(),
        accepted_at_ms: accepted_ms,
    }
}

async fn consent_exists(pool: &sqlx::PgPool, id: i64) -> bool {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM caller_consent WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    n == 1
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn consent_is_kept_as_long_as_its_recordings_plus_42_days() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("consent");
    let setting = retention::save_setting(&pool, st, &preset_setting("uk_ofcom"), "Morag Ross")
        .await
        .unwrap();
    let now = now_ms();
    // A call 50 days ago, which the 50-days-ago programme recording covers.
    let linked = record_consent(
        &pool,
        st,
        &consent("caller-aaaaaaaaaaaa", now - 50 * DAY - 1_800_000),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE caller_consent SET call_ended_at = accepted_at + interval '10 minutes' WHERE id = $1")
        .bind(linked)
        .execute(&pool)
        .await
        .unwrap();
    // They were put on air for five minutes of it (ADR-0153).
    on_air_between(&pool, linked, "2 minutes", "7 minutes").await;
    let rec = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Programme,
        "hour.opus",
        50,
        Some(50),
    )
    .await;
    let (links,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM consent_recording WHERE consent_id = $1 AND recording_id = $2",
    )
    .bind(linked)
    .bind(rec)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        links, 1,
        "the recording took the consent of the caller who was on air during it"
    );
    // An unlinked consent follows the logger's period from the call: 50 days ago goes, 10 stays.
    let old = record_consent(&pool, st, &consent("caller-bbbbbbbbbbbb", now - 50 * DAY))
        .await
        .unwrap();
    let young = record_consent(&pool, st, &consent("caller-cccccccccccc", now - 10 * DAY))
        .await
        .unwrap();
    sqlx::query("UPDATE caller_consent SET call_ended_at = accepted_at + interval '5 minutes' WHERE id = ANY($1)")
        .bind(vec![old, young])
        .execute(&pool)
        .await
        .unwrap();

    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert!(deleted(&pool, rec).await, "the 42-day logger expired");
    assert!(
        consent_exists(&pool, linked).await,
        "kept 42 days after its last recording went"
    );
    assert!(!consent_exists(&pool, old).await && consent_exists(&pool, young).await);
    assert_eq!(r.consents_deleted, 1, "{r:?}");

    // 43 days after the recording went, the consent goes too, and the recording's row with it.
    sqlx::query("UPDATE recording SET deleted_at = now() - interval '43 days' WHERE id = $1")
        .bind(rec)
        .execute(&pool)
        .await
        .unwrap();
    // But not while a hold names it.
    let h = retention::place_hold(
        &pool,
        st,
        &NewHold {
            reason: "Caller disputes consent".into(),
            by: "Iain Gunn".into(),
            from_ms: None,
            to_ms: None,
            records: vec![HoldRecord {
                kind: HoldTarget::Consent,
                id: linked,
            }],
        },
    )
    .await
    .unwrap();
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!((r.consents_deleted, r.held), (0, 1), "{r:?}");
    assert!(consent_exists(&pool, linked).await);
    retention::release_hold(&pool, st, h.id, "Iain Gunn", "Resolved")
        .await
        .unwrap();
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!(r.consents_deleted, 1, "{r:?}");
    assert!(!consent_exists(&pool, linked).await);
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM recording WHERE id = $1")
        .bind(rec)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        rows, 0,
        "the deleted recording's row goes once no consent needs it"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn under_no_retention_consent_goes_with_the_call() {
    let (pool, st) = station("Kettle FM").await;
    let setting = retention::load_setting(&pool, st).await.unwrap();
    assert_eq!(
        setting.preset, NO_RETENTION,
        "a new station starts on No retention"
    );
    let now = now_ms();
    let waiting = record_consent(&pool, st, &consent("caller-dddddddddddd", now - 60_000))
        .await
        .unwrap();
    let ended = record_consent(&pool, st, &consent("caller-eeeeeeeeeeee", now - 600_000))
        .await
        .unwrap();
    assert_eq!(
        retention::mark_call_ended(&pool, st, "caller-eeeeeeeeeeee")
            .await
            .unwrap(),
        1
    );
    let r = retention::sweep(&pool, st, &setting, None, false)
        .await
        .unwrap();
    assert_eq!(r.consents_deleted, 1, "{r:?}");
    assert!(
        consent_exists(&pool, waiting).await,
        "a call still going keeps its consent"
    );
    assert!(!consent_exists(&pool, ended).await);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn consent_survives_a_restart_until_the_call_ends() {
    let (pool, st) = station("Kettle FM").await;
    let now = now_ms();
    record_consent(&pool, st, &consent("caller-ffffffffffff", now - 60_000))
        .await
        .unwrap();
    record_consent(&pool, st, &consent("caller-000000000000", now - 2 * DAY))
        .await
        .unwrap();
    // What a restarted control plane reads back: calls that may still be going.
    let open = retention::open_consents(&pool, st).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].identity, "caller-ffffffffffff");
    assert_eq!(open[0].given_by, "caller");
    assert!(open[0].accepted_at_ms.abs_diff(now - 60_000) < 5);
    let one = retention::open_consent_of(&pool, st, "caller-ffffffffffff")
        .await
        .unwrap();
    assert_eq!(
        one.as_ref().map(|c| c.words_version.as_str()),
        Some("0123456789abcdef")
    );
    retention::mark_call_ended(&pool, st, "caller-ffffffffffff")
        .await
        .unwrap();
    assert!(
        retention::open_consents(&pool, st)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        retention::open_consent_of(&pool, st, "caller-ffffffffffff")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_setting_is_kept_with_a_log_of_who_changed_it() {
    let (pool, st) = station("Kettle FM").await;
    let mut periods = retention::preset("ca_licensed").unwrap().periods();
    periods.programme_recordings = Period::days(14);
    let custom = Setting {
        preset: CUSTOM.into(),
        based_on: Some("ca_licensed".into()),
        custom: Some(periods),
        broadcast_licence: Some(true),
        ..Setting::default()
    };
    let saved = retention::save_setting(&pool, st, &custom, "Morag Ross")
        .await
        .unwrap();
    assert_eq!(saved.preset, CUSTOM);
    assert_eq!(saved.custom, Some(periods));
    assert_eq!(saved.updated_by.as_deref(), Some("Morag Ross"));
    assert_eq!(
        saved.warnings().len(),
        1,
        "14 days is under Canada's 28: a warning, not a refusal"
    );
    retention::save_setting(&pool, st, &preset_setting("uk_ofcom"), "Iain Gunn")
        .await
        .unwrap();
    let back = retention::load_setting(&pool, st).await.unwrap();
    assert_eq!((back.preset.as_str(), back.custom), ("uk_ofcom", None));
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM retention_change WHERE station_id = $1")
            .bind(st.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 2);
    assert!(
        retention::save_setting(&pool, st, &preset_setting("atlantis"), "x")
            .await
            .is_err()
    );
    assert!(
        retention::save_setting(&pool, st, &preset_setting("uk_ofcom"), "  ")
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_unreadable_custom_setting_deletes_nothing() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("unreadable");
    let id = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Programme,
        "old.opus",
        400,
        Some(400),
    )
    .await;
    // A Custom row whose periods this release cannot read (a newer release's, say).
    sqlx::query(
        "INSERT INTO retention_setting (station_id, preset, periods, updated_by) VALUES ($1, 'custom', '{}'::jsonb, 'x')",
    )
    .bind(st.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let setting = retention::load_setting(&pool, st).await.unwrap();
    assert!(setting.check().is_err());
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!((r.recordings_deleted, r.consents_deleted), (0, 0), "{r:?}");
    assert_eq!(r.errors.len(), 1, "{r:?}");
    assert!(!deleted(&pool, id).await && dir.join("old.opus").exists());
    assert!(
        !retention::last_sweep(&pool, st)
            .await
            .unwrap()
            .unwrap()
            .errors
            .is_empty()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_erased_consent_keeps_its_date_and_terms_and_loses_the_name() {
    use weavesignal_model::retention::{ERASED, erase_consent, find_consents};
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = weavesignal_model::ensure_station(
        &pool,
        &weavesignal_model::Station::new(weavesignal_model::StationId::new_random(), "Kettle FM")
            .unwrap(),
    )
    .await
    .unwrap()
    .id;
    for (identity, name) in [
        ("caller-aa11", "Jessie Brown"),
        ("caller-bb22", "Niall Anderson"),
    ] {
        sqlx::query(
            "INSERT INTO caller_consent (station_id, identity, name, topic, given_by, words, words_version, accepted_at)
             VALUES ($1, $2, $3, 'Buses after the late shift', 'caller', 'Your call may be broadcast and recorded.', '0123456789abcdef', now())",
        )
        .bind(station.as_uuid())
        .bind(identity)
        .bind(name)
        .execute(&pool)
        .await
        .unwrap();
    }
    let found = find_consents(&pool, station, "jessie").await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Jessie Brown");
    assert_eq!(
        erase_consent(&pool, station, found[0].id, "Elspeth Gordon")
            .await
            .unwrap(),
        1
    );
    let (name, topic, words, version, by, erased): (String, String, String, String, String, bool) =
        sqlx::query_as(
            "SELECT name, topic, words, words_version, erased_by, erased_at IS NOT NULL
             FROM caller_consent WHERE id = $1",
        )
        .bind(found[0].id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((name.as_str(), topic.as_str()), (ERASED, ""));
    assert_eq!(
        words, "Your call may be broadcast and recorded.",
        "the terms stay as evidence"
    );
    assert_eq!(version, "0123456789abcdef");
    assert_eq!(by, "Elspeth Gordon");
    assert!(erased);
    assert!(
        find_consents(&pool, station, "jessie")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        find_consents(&pool, station, "niall").await.unwrap().len(),
        1,
        "others untouched"
    );
    assert_eq!(
        erase_consent(&pool, station, found[0].id, "x")
            .await
            .unwrap(),
        0,
        "erased once"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_recording_keeps_the_consent_only_of_callers_who_went_on_air_during_it() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("on-air-link");
    let setting = retention::save_setting(&pool, st, &preset_setting("uk_ofcom"), "Morag Ross")
        .await
        .unwrap();
    let now = now_ms();
    // Four calls in the same hour, 50 days ago, each ten minutes long.
    let at = now - 50 * DAY;
    let mut ids = Vec::new();
    for (n, identity) in [
        "caller-aaaaaaaaaaaa",
        "caller-bbbbbbbbbbbb",
        "caller-cccccccccccc",
        "caller-dddddddddddd",
    ]
    .iter()
    .enumerate()
    {
        let id = record_consent(&pool, st, &consent(identity, at + n as u64 * 1000))
            .await
            .unwrap();
        sqlx::query("UPDATE caller_consent SET call_ended_at = accepted_at + interval '10 minutes' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        ids.push(id);
    }
    let [aired, waited, aired_before, aired_after] = ids[..] else {
        unreachable!()
    };
    // `aired` was on air two minutes in; `waited` never was (lobby or screening only).
    on_air_between(&pool, aired, "2 minutes", "7 minutes").await;
    // The recording runs from half an hour before the calls to half an hour after. One caller
    // came off air before it began and one went on air after it ended.
    on_air_between(&pool, aired_before, "-2 hours", "-1 hour").await;
    on_air_between(&pool, aired_after, "5 hours", "6 hours").await;
    std::fs::write(dir.join("hour.opus"), b"audio").unwrap();
    let rec = retention::add_recording(
        &pool,
        st,
        &NewRecording {
            kind: RecordingKind::Programme,
            label: "hour".into(),
            file: Some("hour.opus".into()),
            started_at_ms: at - 1_800_000,
            ended_at_ms: Some(at + 1_800_000),
        },
    )
    .await
    .unwrap();
    assert!(linked_to(&pool, aired, rec).await, "on air during it");
    for (id, why) in [
        (waited, "never on air"),
        (aired_before, "off air before it began"),
        (aired_after, "on air after it ended"),
    ] {
        assert!(!linked_to(&pool, id, rec).await, "{why}");
    }

    // The three unlinked consents follow the logger period from the call's end (42 days on
    // this preset): 50 days ago, so they go at once; the linked one stays while its recording
    // is kept, and for 42 days after.
    let r = retention::sweep(&pool, st, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!(r.consents_deleted, 3, "{r:?}");
    assert!(consent_exists(&pool, aired).await);
    for id in [waited, aired_before, aired_after] {
        assert!(!consent_exists(&pool, id).await);
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_open_recording_takes_a_caller_on_air_when_it_ends_and_one_still_on_air_counts() {
    let (pool, st) = station("Kettle FM").await;
    let dir = tmpdir("on-air-open");
    let now = now_ms();
    let on_air = record_consent(&pool, st, &consent("caller-eeeeeeeeeeee", now - 600_000))
        .await
        .unwrap();
    let screened = record_consent(&pool, st, &consent("caller-ffffffffffff", now - 600_000))
        .await
        .unwrap();
    // The recording is open and begins before the call; nobody had gone on air yet.
    let rec = recording(
        &pool,
        st,
        &dir,
        RecordingKind::Programme,
        "open.opus",
        0,
        None,
    )
    .await;
    assert!(!linked_to(&pool, on_air, rec).await);
    // They go on air, still on air as the recording ends: an open stretch overlaps it.
    let synced = retention::sync_on_air(&pool, st, &["caller-eeeeeeeeeeee".to_owned()], 0)
        .await
        .unwrap();
    assert_eq!((synced.opened, synced.closed), (1, 0));
    assert!(
        retention::end_recording(&pool, st, rec, now_ms())
            .await
            .unwrap()
    );
    assert!(linked_to(&pool, on_air, rec).await);
    assert!(!linked_to(&pool, screened, rec).await);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_record_of_who_is_on_air_follows_the_desk_and_writing_it_twice_changes_nothing() {
    let (pool, st) = station("Kettle FM").await;
    let now = now_ms();
    let a = "caller-111111111111";
    let b = "caller-222222222222";
    let gone = "caller-333333333333";
    let ida = record_consent(&pool, st, &consent(a, now - 600_000))
        .await
        .unwrap();
    let idb = record_consent(&pool, st, &consent(b, now - 600_000))
        .await
        .unwrap();
    let open_stretches = |id: i64| {
        let pool = pool.clone();
        async move {
            let (n,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM consent_on_air WHERE consent_id = $1 AND to_at IS NULL",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            n
        }
    };
    let both = [a.to_owned(), b.to_owned(), gone.to_owned()];
    let done = retention::sync_on_air(&pool, st, &both, 0).await.unwrap();
    assert_eq!((done.opened, done.closed), (2, 0));
    assert_eq!(
        done.no_consent,
        vec![gone.to_owned()],
        "no consent on file to record it against"
    );
    let again = retention::sync_on_air(&pool, st, &both, 0).await.unwrap();
    assert_eq!(
        (again.opened, again.closed),
        (0, 0),
        "the same state twice changes nothing"
    );
    assert_eq!(
        (open_stretches(ida).await, open_stretches(idb).await),
        (1, 1)
    );

    // `a` comes off; `b` stays on; `a` goes on again later: a second stretch.
    let done = retention::sync_on_air(&pool, st, &[b.to_owned()], 0)
        .await
        .unwrap();
    assert_eq!((done.opened, done.closed), (0, 1));
    assert_eq!(open_stretches(ida).await, 0);
    retention::sync_on_air(&pool, st, &[a.to_owned(), b.to_owned()], 0)
        .await
        .unwrap();
    let (stretches,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM consent_on_air WHERE consent_id = $1")
            .bind(ida)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stretches, 2);

    // A call that ends closes its stretch, and the caller cannot go on air for it again.
    retention::mark_call_ended(&pool, st, b).await.unwrap();
    assert_eq!(open_stretches(idb).await, 0);
    let done = retention::sync_on_air(&pool, st, &[a.to_owned(), b.to_owned()], 0)
        .await
        .unwrap();
    assert_eq!(done.opened, 0);
    assert_eq!(done.no_consent, vec![b.to_owned()]);
    assert_eq!(open_stretches(idb).await, 0);
    // The desk saying nobody is on air ends the rest.
    let done = retention::sync_on_air(&pool, st, &[], 0).await.unwrap();
    assert_eq!(done.closed, 1);
    assert_eq!(open_stretches(ida).await, 0);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_command_opens_a_stretch_a_poll_backdates_one_and_a_late_close_is_bounded() {
    let (pool, st) = station("Kettle FM").await;
    let now = now_ms();
    let a = "caller-444444444444";
    let b = "caller-555555555555";
    let ida = record_consent(&pool, st, &consent(a, now - 600_000))
        .await
        .unwrap();
    // `b` gave consent three days ago and the call never ended in the database (a crash).
    let idb = record_consent(&pool, st, &consent(b, now - 3 * DAY))
        .await
        .unwrap();
    let stretch = |id: i64| {
        let pool = pool.clone();
        async move {
            let r: Vec<(f64, Option<f64>, f64)> = sqlx::query_as(
                "SELECT extract(epoch FROM a.from_at)::double precision * 1000,
                        extract(epoch FROM a.to_at)::double precision * 1000,
                        extract(epoch FROM cc.accepted_at)::double precision * 1000
                 FROM consent_on_air a JOIN caller_consent cc ON cc.id = a.consent_id
                 WHERE a.consent_id = $1 ORDER BY a.id",
            )
            .bind(id)
            .fetch_all(&pool)
            .await
            .unwrap();
            r
        }
    };

    // The command's write: once, then nothing more while it is open; no call, nothing.
    assert!(retention::open_on_air(&pool, st, a).await.unwrap());
    assert!(
        !retention::open_on_air(&pool, st, a).await.unwrap(),
        "already open"
    );
    assert!(
        !retention::open_on_air(&pool, st, "caller-nobody")
            .await
            .unwrap()
    );
    // The poll that follows sees them on air: nothing new.
    let done = retention::sync_on_air(&pool, st, &[a.to_owned()], 250)
        .await
        .unwrap();
    assert_eq!(done.opened, 0);
    assert_eq!(stretch(ida).await.len(), 1);

    // A stretch the poll opens begins one poll earlier than the poll that saw it.
    let before = now_ms() as f64;
    retention::sync_on_air(&pool, st, &[a.to_owned(), b.to_owned()], 5_000)
        .await
        .unwrap();
    let opened = stretch(idb).await;
    assert_eq!(opened.len(), 1);
    assert!(
        opened[0].0 <= before - 4_000.0,
        "backdated by the poll: {} against {before}",
        opened[0].0
    );

    // A crash left `b`'s stretch open; it began two days ago. Closed now, it ends at the day
    // after consent, not three days on.
    sqlx::query("UPDATE consent_on_air SET from_at = now() - interval '2 days 23 hours' WHERE consent_id = $1")
        .bind(idb)
        .execute(&pool)
        .await
        .unwrap();
    retention::sync_on_air(&pool, st, &[a.to_owned()], 250)
        .await
        .unwrap();
    let closed = stretch(idb).await;
    let (from, to, accepted) = (closed[0].0, closed[0].1.unwrap(), closed[0].2);
    assert!(
        (to - (accepted + DAY as f64)).abs() < 1_000.0,
        "bounded by a day after consent"
    );
    assert!(to >= from);
    // One that began after that bound ends where it began, never before.
    sqlx::query("UPDATE consent_on_air SET to_at = NULL, from_at = now() - interval '1 hour' WHERE consent_id = $1")
        .bind(idb)
        .execute(&pool)
        .await
        .unwrap();
    retention::sync_on_air(&pool, st, &[a.to_owned()], 250)
        .await
        .unwrap();
    let late = stretch(idb).await;
    assert_eq!(late[0].1, Some(late[0].0));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_hold_can_be_extended_once_and_again_but_never_to_a_date_not_later() {
    let (pool, st) = station("Kettle FM").await;
    let rule = retention::preset("ca_licensed").unwrap().complaint_hold;
    let now = now_ms();
    let held = retention::place_hold(
        &pool,
        st,
        &NewHold {
            reason: "CRTC complaint about the 14:00 phone-in".into(),
            by: "Iain Gunn".into(),
            from_ms: Some(now - 100 * DAY),
            to_ms: Some(now - 100 * DAY + 3_600_000),
            records: vec![],
        },
    )
    .await
    .unwrap();
    let ext = |until_ms: u64| retention::Extension {
        until_ms,
        by: "Morag Ross".into(),
        reason: "The regulator asked for the tapes again".into(),
    };
    // Past its 56-day date: flagged until it is extended.
    assert!(held.past_date(rule.as_ref(), now));
    assert_eq!(held.extended_until_ms, None);

    // Not later than the date it has (44 days ago), not in the future, or past any date the
    // database keeps: refused, with a sentence and not a database error.
    for bad in [now - 60 * DAY, now - 1000, 200 * 366 * DAY + now, u64::MAX] {
        assert!(matches!(
            retention::extend_hold(&pool, st, held.id, &ext(bad), rule.as_ref(), now).await,
            Err(retention::HoldError::Invalid(_))
        ));
    }
    let until = now + 30 * DAY;
    let h = retention::extend_hold(&pool, st, held.id, &ext(until), rule.as_ref(), now)
        .await
        .unwrap();
    assert!(h.extended_until_ms.is_some_and(|d| d.abs_diff(until) < 5));
    assert_eq!(h.extended_by.as_deref(), Some("Morag Ross"));
    assert!(h.extended_at_ms.is_some() && h.extend_reason.is_some());
    assert!(
        !h.past_date(rule.as_ref(), now),
        "extending clears the flag"
    );
    assert!(
        h.past_date(rule.as_ref(), now + 31 * DAY),
        "and it returns after the new date"
    );
    // The hold is the same: still open, same reason, nothing released.
    assert_eq!((h.id, h.reason.as_str()), (held.id, held.reason.as_str()));
    assert!(h.released_at_ms.is_none());
    // The flag listing agrees.
    let setting = Setting {
        preset: "ca_licensed".into(),
        ..Setting::default()
    };
    assert!(
        retention::holds_past_date(&pool, st, &setting, now)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        retention::holds_past_date(&pool, st, &setting, now + 31 * DAY)
            .await
            .unwrap()
            .len(),
        1
    );

    // Again, later; not to the same date or earlier.
    assert!(matches!(
        retention::extend_hold(&pool, st, held.id, &ext(until), rule.as_ref(), now).await,
        Err(retention::HoldError::Invalid(_))
    ));
    let h = retention::extend_hold(
        &pool,
        st,
        held.id,
        &ext(until + 30 * DAY),
        rule.as_ref(),
        now,
    )
    .await
    .unwrap();
    assert!(
        h.extended_until_ms
            .is_some_and(|d| d.abs_diff(until + 30 * DAY) < 5)
    );

    // Empty words are refused; a released hold cannot be extended; neither can another
    // station's, nor one that is not there.
    let blank = retention::Extension {
        by: "  ".into(),
        ..ext(until + 60 * DAY)
    };
    assert!(matches!(
        retention::extend_hold(&pool, st, held.id, &blank, rule.as_ref(), now).await,
        Err(retention::HoldError::Invalid(_))
    ));
    let (_, other) = station("Other FM").await;
    assert!(matches!(
        retention::extend_hold(
            &pool,
            other,
            held.id,
            &ext(until + 60 * DAY),
            rule.as_ref(),
            now
        )
        .await,
        Err(retention::HoldError::NotFound(_))
    ));
    retention::release_hold(&pool, st, held.id, "Morag Ross", "Complaint closed")
        .await
        .unwrap();
    assert!(matches!(
        retention::extend_hold(
            &pool,
            st,
            held.id,
            &ext(until + 60 * DAY),
            rule.as_ref(),
            now
        )
        .await,
        Err(retention::HoldError::Released(_))
    ));
}
