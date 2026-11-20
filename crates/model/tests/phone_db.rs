// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line's rows (migrations 0046 and 0047) against a real PostgreSQL. Ignored unless asked for
//! (`just test-db`).

use weavesignal_model::caller_id::{BanKeys, livekit_identity};
use weavesignal_model::phone::{
    PhoneBanRecord, PhoneLine, PhoneSettingsRecord, PhoneWordsRecord, bans, consents_by_ban_keys,
    delete_words, lift_ban, put_ban, put_settings, put_words, record_phone_consent, settings,
    words,
};
use weavesignal_model::retention::erase_consent;
use weavesignal_model::{ConsentRecord, Station, StationId, ensure_station};

async fn pool() -> sqlx::PgPool {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    sqlx::PgPool::connect(&url).await.expect("connect")
}

fn consent(identity: &str, given_by: &'static str) -> ConsentRecord {
    ConsentRecord {
        identity: identity.into(),
        name: String::new(),
        topic: String::new(),
        given_by,
        words: "You're through to the studio. Press any key to carry on.".into(),
        words_version: "0123456789abcdef".into(),
        accepted_at_ms: 1_763_600_000_000,
        show_id: None,
        presenter: String::new(),
        contact: String::new(),
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_phone_consent_is_found_by_number_and_erasure_drops_the_key() {
    let pool = pool().await;
    let s = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let keys = BanKeys::new([3; 32]);
    let key = keys.of_identity(&livekit_identity("+447700900123"));
    let id = record_phone_consent(
        &pool,
        s.id,
        &consent("phone-Xk3pQ9mZr2Ta", "caller"),
        &PhoneLine {
            ban_key: Some(key.clone()),
            number_tail: "0123".into(),
        },
    )
    .await
    .unwrap();
    // A withheld caller: no ban key; words off: not asked.
    record_phone_consent(
        &pool,
        s.id,
        &consent("phone-Yk3pQ9mZr2Tb", "not_asked"),
        &PhoneLine {
            ban_key: None,
            number_tail: "mous".into(),
        },
    )
    .await
    .unwrap();
    let typed = keys.of_typed_number("07700 900123", "44");
    assert_eq!(
        consents_by_ban_keys(&pool, s.id, &typed).await.unwrap(),
        vec![id]
    );
    // A browser consent may not carry phone fields.
    let bad = sqlx::query(
        "INSERT INTO caller_consent (station_id, identity, name, topic, given_by, words,
           words_version, accepted_at, ban_key) VALUES ($1, 'caller-1', '', '', 'caller', 'w',
           '0123456789abcdef', now(), $2)",
    )
    .bind(s.id.as_uuid())
    .bind(&key)
    .execute(&pool)
    .await;
    assert!(bad.is_err());
    // Erased at the caller's request: the number no longer finds it.
    assert_eq!(erase_consent(&pool, s.id, id, "admin").await.unwrap(), 1);
    assert!(
        consents_by_ban_keys(&pool, s.id, &typed)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn bans_by_key_lapse_and_lift() {
    let pool = pool().await;
    let s = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let b = PhoneBanRecord {
        ban_key: "pb_0123456789abcdef0123456789abcdef".into(),
        number_tail: "4471".into(),
        reason: "abusive".into(),
        made_by: "Morag".into(),
        made_at_ms: 1_763_600_000_000,
        expires_at_ms: Some(1_763_600_100_000),
    };
    put_ban(&pool, s.id, &b).await.unwrap();
    assert_eq!(
        bans(&pool, s.id, 1_763_600_000_000).await.unwrap(),
        vec![b.clone()]
    );
    assert!(
        bans(&pool, s.id, 1_763_600_100_000)
            .await
            .unwrap()
            .is_empty()
    );
    // Renewed without an end.
    let renewed = PhoneBanRecord {
        expires_at_ms: None,
        ..b.clone()
    };
    put_ban(&pool, s.id, &renewed).await.unwrap();
    assert_eq!(bans(&pool, s.id, 4_102_444_800_000).await.unwrap().len(), 1);
    assert!(lift_ban(&pool, s.id, &b.ban_key).await.unwrap());
    assert!(!lift_ban(&pool, s.id, &b.ban_key).await.unwrap());
    // Not a ban key: refused by the table.
    let bad = PhoneBanRecord {
        ban_key: "sip_0123456789abcdef".into(),
        ..b
    };
    assert!(put_ban(&pool, s.id, &bad).await.is_err());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_stations_recording_replaces_the_one_before() {
    let pool = pool().await;
    let s = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(words(&pool, s.id).await.unwrap(), None);
    let file = |n: u128| format!("{}.flac", uuid::Uuid::from_u128(n).hyphenated());
    let w = PhoneWordsRecord {
        words: "Live on Kettle FM. Press any key to carry on.".into(),
        words_version: "0123456789abcdef".into(),
        file: file(1),
        duration_ms: 6_000,
        gain_db: -3.5,
        recorded_by: "Morag".into(),
        recorded_at_ms: 1_763_600_000_000,
    };
    assert_eq!(put_words(&pool, s.id, &w).await.unwrap(), None);
    let w2 = PhoneWordsRecord {
        file: file(2),
        ..w.clone()
    };
    assert_eq!(put_words(&pool, s.id, &w2).await.unwrap(), Some(file(1)));
    assert_eq!(words(&pool, s.id).await.unwrap(), Some(w2));
    assert_eq!(delete_words(&pool, s.id).await.unwrap(), Some(file(2)));
    assert_eq!(words(&pool, s.id).await.unwrap(), None);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_settings_changed_in_the_studio_keep_what_is_left_out() {
    let pool = pool().await;
    let s = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        settings(&pool, s.id).await.unwrap(),
        PhoneSettingsRecord::default()
    );
    let voice = PhoneSettingsRecord {
        consent_words: None,
        default_voice: Some("edinburgh".into()),
    };
    assert_eq!(
        put_settings(&pool, s.id, &voice, "Morag").await.unwrap(),
        voice
    );
    let off = PhoneSettingsRecord {
        consent_words: Some(false),
        default_voice: None,
    };
    let both = PhoneSettingsRecord {
        consent_words: Some(false),
        default_voice: Some("edinburgh".into()),
    };
    assert_eq!(
        put_settings(&pool, s.id, &off, "Morag").await.unwrap(),
        both
    );
    assert_eq!(settings(&pool, s.id).await.unwrap(), both);
    // Only the three voices are kept.
    let bad = PhoneSettingsRecord {
        consent_words: None,
        default_voice: Some("glasgow".into()),
    };
    assert!(put_settings(&pool, s.id, &bad, "Morag").await.is_err());
}
