// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio's rows (migration 0070, provisional) against a real PostgreSQL: one open
//! session per station, a session read back as a restart reads it, the move written on the row,
//! the end, a deleted show taking its sessions with it, and the bookings table's own check.
//! Ignored unless asked for (`just test-db`).

use uuid::Uuid;
use weavesignal_model::rehearsal::{self, Ended, Opened, Opening, ShowRef, Switches};
use weavesignal_model::{Station, StationId, ensure_station};

async fn pool() -> sqlx::PgPool {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    sqlx::PgPool::connect(&url).await.expect("connect")
}

async fn station(pool: &sqlx::PgPool) -> StationId {
    ensure_station(
        pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap()
    .id
}

async fn show(pool: &sqlx::PgPool, station: StationId, slug: &str, name: &str) -> ShowRef {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO show (id, station_id, slug, name, colour_slot) VALUES ($1, $2, $3, $4, 2)",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(slug)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    ShowRef {
        id,
        slug: slug.into(),
        name: name.into(),
        presenter_account: None,
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn one_session_is_open_at_a_time_and_a_restart_reads_it_back() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    let kettle = show(&pool, s, "the-morning-kettle", "The Morning Kettle").await;

    assert_eq!(rehearsal::open_session(&pool, s).await.unwrap(), None);
    let Opened::New(first) = rehearsal::open(&pool, s, &steeped, "Finlay").await.unwrap() else {
        panic!("a new session")
    };
    assert_eq!(first.show_slug, "steeped");
    assert_eq!(first.show_name, "Steeped");
    assert_eq!(first.opened_by, "Finlay");
    assert!(first.ended.is_none() && first.moved.is_empty() && first.opening.is_none());
    assert!(first.opened_at_ms > 1_700_000_000_000);

    // The same show again is the same session; another show is refused.
    let Opened::Already(again) = rehearsal::open(&pool, s, &steeped, "Kirsty").await.unwrap()
    else {
        panic!("the open session")
    };
    assert_eq!(again.id, first.id);
    let Opened::Busy(busy) = rehearsal::open(&pool, s, &kettle, "Kirsty").await.unwrap() else {
        panic!("busy")
    };
    assert_eq!(busy.show_name, "Steeped");

    // Opens at once: one wins, the others see it.
    let (a, b, c) = tokio::join!(
        rehearsal::open(&pool, s, &kettle, "Kirsty"),
        rehearsal::open(&pool, s, &kettle, "Kirsty"),
        rehearsal::open(&pool, s, &kettle, "Kirsty"),
    );
    for o in [a, b, c] {
        assert!(matches!(o.unwrap(), Opened::Busy(_)));
    }

    // A restart reads it back.
    assert_eq!(
        rehearsal::open_session(&pool, s)
            .await
            .unwrap()
            .map(|x| x.id),
        Some(first.id)
    );

    // The move is written on the row before anything else.
    let opening = Opening::from_people(
        1_763_700_000_000,
        rehearsal::standby_until_ms(1_763_700_000_000, 120),
        Some("eilidh"),
        [Switches {
            identity: "eilidh",
            present: true,
            on: true,
            mute: false,
        }],
    );
    let moved = vec!["eilidh".to_owned(), "gregor".to_owned()];
    assert!(
        rehearsal::record_move(&pool, s, first.id, &moved, &opening)
            .await
            .unwrap()
    );
    let read = rehearsal::session(&pool, s, first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read.moved, moved);
    assert_eq!(read.opening.as_ref(), Some(&opening));
    assert!(read.moved_at_ms.is_some());

    // Ended once; then the studio is free, for any show.
    assert!(
        rehearsal::end(&pool, s, first.id, Ended::Moved)
            .await
            .unwrap()
    );
    assert!(
        !rehearsal::end(&pool, s, first.id, Ended::Ended)
            .await
            .unwrap()
    );
    let ended = rehearsal::session(&pool, s, first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.ended, Some(Ended::Moved));
    assert!(ended.ended_at_ms.unwrap() >= ended.opened_at_ms);
    assert!(
        !rehearsal::record_move(&pool, s, first.id, &moved, &opening)
            .await
            .unwrap(),
        "an ended session takes no move"
    );
    assert_eq!(rehearsal::open_session(&pool, s).await.unwrap(), None);
    let Opened::New(second) = rehearsal::open(&pool, s, &kettle, "Kirsty").await.unwrap() else {
        panic!("a new session")
    };
    assert_ne!(second.id, first.id);

    // Another station's sessions are its own.
    let other = station(&pool).await;
    assert_eq!(rehearsal::open_session(&pool, other).await.unwrap(), None);
    assert!(
        !rehearsal::end(&pool, other, second.id, Ended::Ended)
            .await
            .unwrap()
    );

    // Deleting the show takes its sessions with it.
    sqlx::query("DELETE FROM show WHERE id = $1")
        .bind(kettle.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(rehearsal::open_session(&pool, s).await.unwrap(), None);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_tables_refuse_rows_that_make_no_sense() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    // A booking that ends before it starts.
    let bad = sqlx::query(
        "INSERT INTO rehearsal_booking (id, station_id, show_id, starts_at, ends_at, booked_by)
         VALUES ($1, $2, $3, now(), now() - interval '1 hour', 'Finlay')",
    )
    .bind(Uuid::new_v4())
    .bind(s.as_uuid())
    .bind(steeped.id)
    .execute(&pool)
    .await;
    assert!(bad.is_err());
    // An end without how, and a move without its opening.
    for sql in [
        "INSERT INTO rehearsal_session (id, station_id, show_id, opened_by, ended_at)
         VALUES ($1, $2, $3, 'Finlay', now())",
        "INSERT INTO rehearsal_session (id, station_id, show_id, opened_by, moved_at, moved)
         VALUES ($1, $2, $3, 'Finlay', now(), '[]')",
        "INSERT INTO rehearsal_session (id, station_id, show_id, opened_by, ended_at, ended_how)
         VALUES ($1, $2, $3, 'Finlay', now(), 'lost')",
        "INSERT INTO rehearsal_session (id, station_id, show_id, opened_by) VALUES ($1, $2, $3, '')",
    ] {
        let r = sqlx::query(sql)
            .bind(Uuid::new_v4())
            .bind(s.as_uuid())
            .bind(steeped.id)
            .execute(&pool)
            .await;
        assert!(r.is_err(), "{sql}");
    }
    let good = sqlx::query(
        "INSERT INTO rehearsal_booking (id, station_id, show_id, starts_at, ends_at, booked_by)
         VALUES ($1, $2, $3, now(), now() + interval '1 hour', 'Finlay')",
    )
    .bind(Uuid::new_v4())
    .bind(s.as_uuid())
    .bind(steeped.id)
    .execute(&pool)
    .await;
    assert!(good.is_ok());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_moved_sessions_opening_is_read_back_applied_and_replaced() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    let now = 1_763_700_000_000_i64;
    let mut opening = Opening::from_people(
        now,
        rehearsal::standby_until_ms(now, 120),
        Some("eilidh"),
        [Switches {
            identity: "eilidh",
            present: true,
            on: true,
            mute: false,
        }],
    );
    opening.airing_end_ms = Some(now + 3_600_000);
    let move_one = |opening: Opening| {
        let pool = pool.clone();
        let steeped = steeped.clone();
        async move {
            let Opened::New(x) = rehearsal::open(&pool, s, &steeped, "Finlay").await.unwrap()
            else {
                panic!("a new session")
            };
            let moved = vec!["eilidh".to_owned(), "gregor".to_owned()];
            assert!(
                rehearsal::record_move(&pool, s, x.id, &moved, &opening)
                    .await
                    .unwrap()
            );
            assert!(rehearsal::end(&pool, s, x.id, Ended::Moved).await.unwrap());
            x.id
        }
    };
    let first = move_one(opening.clone()).await;
    // Read back as a restart reads it: the ended, moved session with its opening.
    let back = rehearsal::moved_sessions(&pool, s, 36 * 3_600_000)
        .await
        .unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].id, first);
    assert_eq!(back[0].opening.as_ref(), Some(&opening));
    assert_eq!(back[0].moved, ["eilidh", "gregor"]);
    // Applied: written, and still read back (a missed page is offered the move again).
    let mut applied = opening.clone();
    applied.applied_at_ms = Some(now + 5);
    assert!(
        rehearsal::set_opening(&pool, s, first, &applied)
            .await
            .unwrap()
    );
    let back = rehearsal::moved_sessions(&pool, s, 36 * 3_600_000)
        .await
        .unwrap();
    assert_eq!(
        back[0].opening.as_ref().unwrap().applied_at_ms,
        Some(now + 5)
    );
    // A second move for the same show replaces the first: its opening is cleared.
    let second = move_one(opening.clone()).await;
    assert_eq!(
        rehearsal::clear_other_openings(&pool, s, steeped.id, second, now + 9)
            .await
            .unwrap(),
        1
    );
    let back = rehearsal::moved_sessions(&pool, s, 36 * 3_600_000)
        .await
        .unwrap();
    assert_eq!(back.iter().map(|x| x.id).collect::<Vec<_>>(), [second]);
    let first_row = rehearsal::session(&pool, s, first).await.unwrap().unwrap();
    assert_eq!(first_row.opening.unwrap().cleared_at_ms, Some(now + 9));
    // Another station sees none of it.
    let other = station(&pool).await;
    assert!(
        rehearsal::moved_sessions(&pool, other, 36 * 3_600_000)
            .await
            .unwrap()
            .is_empty()
    );
}
