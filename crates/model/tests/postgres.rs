// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Against a real PostgreSQL. Ignored unless asked for:
//! `WEAVESIGNAL_TEST_DATABASE_URL=postgres://... cargo test -p weavesignal-model -- --ignored`
//! (`just test-db` starts a throwaway PostgreSQL 18 container for it).

use weavesignal_model::{ConsentRecord, Station, StationId, ensure_station, record_consent};

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn migrations_apply_and_the_station_row_is_kept() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let id = StationId::new_random();

    let first = ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
        .await
        .expect("first run");
    assert_eq!(first.id, id);

    // A second start with a new name keeps the identifier and takes the name.
    let second = ensure_station(&pool, &Station::new(id, "Kettle FM Fife").unwrap())
        .await
        .expect("second run");
    assert_eq!(second.id, id);
    assert_eq!(second.name, "Kettle FM Fife");

    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM station WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn carts_are_kept_per_station_and_a_slot_holds_one() {
    use weavesignal_model::carts::{self, Bank, Cart, CartMode};
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let a = ensure_station(&pool, &Station::new(StationId::new_random(), "A").unwrap())
        .await
        .unwrap();
    let b = ensure_station(&pool, &Station::new(StationId::new_random(), "B").unwrap())
        .await
        .unwrap();
    let bank = Bank {
        number: 1,
        name: "Reactions".into(),
        colour_slot: 1,
    };
    carts::put_bank(&pool, a.id, &bank).await.unwrap();
    let file = |n: u128| format!("{}.flac", uuid::Uuid::from_u128(n).hyphenated());
    let mut cart = Cart {
        id: uuid::Uuid::new_v4(),
        bank: 1,
        slot: 1,
        label: "Applause".into(),
        detail: String::new(),
        mode: CartMode::OneShot,
        file: Some(file(1)),
        duration_ms: 12_010,
        gain_db: -7.0,
        loudness_lufs: Some(-16.0),
        true_peak_dbtp: Some(-1.2),
        ramp_ms: Some(1200),
        origin: "demo:applause-1".into(),
        licence: "CC0 1.0".into(),
    };
    assert_eq!(carts::put_cart(&pool, a.id, &cart).await.unwrap(), None);
    // A new cart in the same slot replaces it, and hands back the old file to delete.
    let old = cart.clone();
    cart.id = uuid::Uuid::new_v4();
    cart.file = Some(file(2));
    cart.mode = CartMode::Bed;
    assert_eq!(
        carts::put_cart(&pool, a.id, &cart).await.unwrap(),
        old.file.clone()
    );
    let (banks, list) = carts::load(&pool, a.id).await.unwrap();
    assert_eq!(banks, vec![bank.clone()]);
    assert_eq!(list, vec![cart.clone()]);
    // Another station sees none of it, and cannot use a bank it has not made.
    assert_eq!(carts::load(&pool, b.id).await.unwrap(), (vec![], vec![]));
    assert!(carts::put_cart(&pool, b.id, &cart).await.is_err());
    // The table refuses what the checks refuse.
    let mut bad = cart.clone();
    bad.id = uuid::Uuid::new_v4();
    bad.slot = 2;
    bad.file = Some("../x.flac".into());
    assert!(carts::put_cart(&pool, a.id, &bad).await.is_err());
    assert_eq!(
        carts::delete_cart(&pool, a.id, 1, 1).await.unwrap(),
        Some(cart.id)
    );
    assert_eq!(carts::load(&pool, a.id).await.unwrap().1, vec![]);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_callers_consent_is_kept_with_its_words() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let id = StationId::new_random();
    ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
        .await
        .expect("station");
    let record = ConsentRecord {
        identity: "caller-0a1b2c3d4e5f".into(),
        name: "Jessie Brown".into(),
        topic: "No bus after 23:10".into(),
        given_by: "caller",
        words: "Your call may be broadcast live and recorded.".into(),
        words_version: "0123456789abcdef".into(),
        accepted_at_ms: 1_793_000_000_123,
        show_id: None,
        presenter: String::new(),
        contact: String::new(),
    };
    let row = record_consent(&pool, id, &record).await.expect("insert");
    let (words, by, ms): (String, String, f64) = sqlx::query_as(
        "SELECT words, given_by, extract(epoch FROM accepted_at)::double precision * 1000 FROM caller_consent WHERE id = $1",
    )
    .bind(row)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (words.as_str(), by.as_str()),
        (record.words.as_str(), "caller")
    );
    assert!((ms - 1_793_000_000_123.0).abs() < 1.0, "{ms}");
    let mut bad = record.clone();
    bad.given_by = "someone";
    assert!(
        record_consent(&pool, id, &bad).await.is_err(),
        "the CHECK holds"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_ended_pass_is_kept_per_station_for_a_pass_s_life() {
    use weavesignal_model::passes::{PASS_LONGEST_S, end_pass, ended_passes};
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let a = ensure_station(&pool, &Station::new(StationId::new_random(), "A").unwrap())
        .await
        .unwrap();
    let b = ensure_station(&pool, &Station::new(StationId::new_random(), "B").unwrap())
        .await
        .unwrap();
    end_pass(&pool, a.id, "guest-1a2b").await.unwrap();
    let first = ended_passes(&pool, a.id).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, "guest-1a2b");
    assert!(ended_passes(&pool, b.id).await.unwrap().is_empty());

    // Ended again: one row, the later time.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    end_pass(&pool, a.id, "guest-1a2b").await.unwrap();
    let again = ended_passes(&pool, a.id).await.unwrap();
    assert_eq!(again.len(), 1);
    assert!(again[0].1 > first[0].1);

    // An end older than any pass can live is forgotten.
    sqlx::query(
        "UPDATE pass_ended SET ended_at = now() - make_interval(secs => $2) WHERE station_id = $1",
    )
    .bind(a.id.as_uuid())
    .bind((PASS_LONGEST_S + 60) as f64)
    .execute(&pool)
    .await
    .unwrap();
    assert!(ended_passes(&pool, a.id).await.unwrap().is_empty());
    let (left,): (i64,) = sqlx::query_as("SELECT count(*) FROM pass_ended WHERE station_id = $1")
        .bind(a.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "the old row is deleted, not just hidden");
}

/// Migration 0017 on a database that already holds chat rows (ADR-0125): an account with an
/// address keeps its plain hash marked 1 (re-keyed at its owner's next sign-in), one without an
/// address is marked 2, the pending codes go, and a row written later without a version (by a
/// chat built before 0017) is presumed plain.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn migration_17_marks_existing_chat_accounts_and_drops_pending_codes() {
    use sqlx::Connection;
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let mut db = sqlx::PgConnection::connect(&url).await.expect("connect");
    // A schema of its own, migrated up to 0016 as a station's database was before this branch.
    let schema = format!("m17_{}", uuid::Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema}"
    )))
    .execute(&mut db)
    .await
    .unwrap();
    let mut seventeen = None;
    for m in weavesignal_model::MIGRATOR.iter() {
        if m.version < 17 {
            sqlx::raw_sql(sqlx::AssertSqlSafe(m.sql.as_str().to_owned()))
                .execute(&mut db)
                .await
                .unwrap_or_else(|e| panic!("migration {}: {e}", m.version));
        } else if m.version == 17 {
            seventeen = Some(m.sql.as_str().to_owned());
        }
    }
    let seventeen = seventeen.expect("migration 0017");
    let station = uuid::Uuid::new_v4();
    let plain = vec![7u8; 32];
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO station (id, name) VALUES ('{station}', 'Kettle FM');
         INSERT INTO chat_account (id, station_id, kind, email_hash, name) VALUES
           ('{a}', '{station}', 'email', '\\x{h}', 'Morag Henderson'),
           ('{g}', '{station}', 'guest', NULL, 'Rory MacLean'),
           ('{e}', '{station}', 'email', NULL, 'erased 00000000');
         INSERT INTO chat_signin (id, station_id, email_hash, code_hash, expires_at)
           VALUES ('{c}', '{station}', '\\x{h}', '\\x{h}', now() + interval '10 minutes');",
        a = uuid::Uuid::from_u128(1),
        g = uuid::Uuid::from_u128(2),
        e = uuid::Uuid::from_u128(3),
        c = uuid::Uuid::from_u128(4),
        h = "07".repeat(32),
    )))
    .execute(&mut db)
    .await
    .unwrap();

    sqlx::raw_sql(sqlx::AssertSqlSafe(seventeen))
        .execute(&mut db)
        .await
        .expect("0017 applies over existing rows");

    let rows: Vec<(uuid::Uuid, Option<Vec<u8>>, i16)> =
        sqlx::query_as("SELECT id, email_hash, email_hash_version FROM chat_account ORDER BY id")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (uuid::Uuid::from_u128(1), Some(plain.clone()), 1),
            (uuid::Uuid::from_u128(2), None, 2),
            (uuid::Uuid::from_u128(3), None, 2),
        ]
    );
    let (codes,): (i64,) = sqlx::query_as("SELECT count(*) FROM chat_signin")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(codes, 0, "pending codes in the old form are dropped");
    // A chat from before 0017 names no version: its row is presumed plain.
    sqlx::query(
        "INSERT INTO chat_account (id, station_id, kind, email_hash, name)
         VALUES ($1, $2, 'email', $3, 'Eilidh Campbell')",
    )
    .bind(uuid::Uuid::from_u128(5))
    .bind(station)
    .bind(vec![9u8; 32])
    .execute(&mut db)
    .await
    .unwrap();
    let (v,): (i16,) = sqlx::query_as("SELECT email_hash_version FROM chat_account WHERE id = $1")
        .bind(uuid::Uuid::from_u128(5))
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(v, 1);
    // Only the two versions are allowed.
    assert!(
        sqlx::query("UPDATE chat_account SET email_hash_version = 3 WHERE id = $1")
            .bind(uuid::Uuid::from_u128(5))
            .execute(&mut db)
            .await
            .is_err()
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&mut db)
        .await
        .unwrap();
}
