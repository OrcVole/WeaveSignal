// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Bookings of the rehearsal studio and its sessions over a stretch of time (1.3.0, step R14)
//! against a real PostgreSQL: a booking made, one that would share time refused, one that only
//! touches allowed, a change within its own time, two writes at once of which one wins, the
//! booking holding the studio now, and the sessions the week lists. Ignored unless asked for
//! (`just test-db`).

use uuid::Uuid;
use weavesignal_model::rehearsal::{self, Ended, Opened, ShowRef};
use weavesignal_model::rehearsal_bookings::{self as bookings, Booked};
use weavesignal_model::{Station, StationId, ensure_station};

const H: i64 = 3_600_000;

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
        "INSERT INTO show (id, station_id, slug, name, colour_slot) VALUES ($1, $2, $3, $4, 5)",
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn bookings_never_share_time() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    let kettle = show(&pool, s, "the-morning-kettle", "The Morning Kettle").await;
    // On the hour, two days on, so the times are whole.
    let t0 = (now_ms() / H + 48) * H;

    let Booked::Done(first) = bookings::create(&pool, s, steeped.id, t0, t0 + H, " Finlay ")
        .await
        .unwrap()
    else {
        panic!("a booking")
    };
    assert_eq!(
        (first.booking.starts_ms, first.booking.ends_ms),
        (t0, t0 + H)
    );
    assert_eq!(first.booking.booked_by, "Finlay");
    assert_eq!(
        (first.show_slug.as_str(), first.show_name.as_str()),
        ("steeped", "Steeped")
    );
    assert_eq!(first.colour_slot, 5);

    // Sharing any time is refused, naming the booking in the way.
    let Booked::Clash(hit) =
        bookings::create(&pool, s, kettle.id, t0 + H / 2, t0 + 2 * H, "Kirsty")
            .await
            .unwrap()
    else {
        panic!("a clash")
    };
    assert_eq!(hit.booking.id, first.booking.id);
    // Touching is not sharing.
    let Booked::Done(second) = bookings::create(&pool, s, kettle.id, t0 + H, t0 + 2 * H, "Kirsty")
        .await
        .unwrap()
    else {
        panic!("a booking that starts as the first ends")
    };

    // A booking moved within its own time does not clash with itself; onto the other's, it does.
    assert!(matches!(
        bookings::change(&pool, s, first.booking.id, steeped.id, t0 + H / 4, t0 + H)
            .await
            .unwrap(),
        Booked::Done(b) if b.booking.starts_ms == t0 + H / 4
    ));
    assert!(matches!(
        bookings::change(&pool, s, first.booking.id, steeped.id, t0, t0 + H + 60_000)
            .await
            .unwrap(),
        Booked::Clash(b) if b.booking.id == second.booking.id
    ));
    // The show can change too.
    assert!(matches!(
        bookings::change(&pool, s, first.booking.id, kettle.id, t0, t0 + H)
            .await
            .unwrap(),
        Booked::Done(b) if b.show_slug == "the-morning-kettle"
    ));
    assert_eq!(
        bookings::change(&pool, s, Uuid::new_v4(), kettle.id, t0, t0 + H)
            .await
            .unwrap(),
        Booked::Missing
    );

    // Listed by the time asked for, in start order.
    let listed = bookings::bookings_between(&pool, s, t0 - H, t0 + 3 * H)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|b| b.booking.id).collect::<Vec<_>>(),
        [first.booking.id, second.booking.id]
    );
    assert_eq!(
        bookings::bookings_between(&pool, s, t0 + 2 * H, t0 + 3 * H)
            .await
            .unwrap(),
        []
    );
    // Another station's bookings are not this one's.
    let other = station(&pool).await;
    assert_eq!(
        bookings::bookings_between(&pool, other, t0 - H, t0 + 3 * H)
            .await
            .unwrap(),
        []
    );
    assert!(
        !bookings::delete(&pool, other, first.booking.id)
            .await
            .unwrap()
    );

    assert!(bookings::delete(&pool, s, first.booking.id).await.unwrap());
    assert!(!bookings::delete(&pool, s, first.booking.id).await.unwrap());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn of_two_bookings_made_at_once_for_the_same_time_one_wins() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    let kettle = show(&pool, s, "the-morning-kettle", "The Morning Kettle").await;
    let t0 = (now_ms() / H + 72) * H;
    for round in 0..5 {
        let at = t0 + round * 2 * H;
        let (a, b) = tokio::join!(
            bookings::create(&pool, s, steeped.id, at, at + H, "Finlay"),
            bookings::create(&pool, s, kettle.id, at, at + H, "Kirsty"),
        );
        let done = [a.unwrap(), b.unwrap()]
            .iter()
            .filter(|x| matches!(x, Booked::Done(_)))
            .count();
        assert_eq!(done, 1, "round {round}: exactly one booking is made");
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_booking_holding_now_and_the_sessions_in_a_stretch() {
    let pool = pool().await;
    let s = station(&pool).await;
    let steeped = show(&pool, s, "steeped", "Steeped").await;
    let kettle = show(&pool, s, "the-morning-kettle", "The Morning Kettle").await;
    let now = now_ms();

    assert_eq!(bookings::holding_now(&pool, s, now).await.unwrap(), None);
    let Booked::Done(b) = bookings::create(&pool, s, steeped.id, now - 30_000, now + H, "Finlay")
        .await
        .unwrap()
    else {
        panic!("a booking")
    };
    assert_eq!(
        bookings::holding_now(&pool, s, now)
            .await
            .unwrap()
            .map(|h| h.booking.id),
        Some(b.booking.id)
    );
    assert_eq!(
        bookings::holding_now(&pool, s, now + H).await.unwrap(),
        None,
        "it holds until its end"
    );

    // One ended session, then one open: both are in a stretch that covers now; the open one runs
    // until now.
    let Opened::New(gone) = rehearsal::open(&pool, s, &kettle, "Kirsty").await.unwrap() else {
        panic!("a session")
    };
    assert!(
        rehearsal::end(&pool, s, gone.id, Ended::Ended)
            .await
            .unwrap()
    );
    let Opened::New(open) = rehearsal::open(&pool, s, &steeped, "Finlay").await.unwrap() else {
        panic!("a session")
    };
    let listed = bookings::sessions_between(&pool, s, now - H, now + H)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|r| r.session.id).collect::<Vec<_>>(),
        [gone.id, open.id]
    );
    assert_eq!(listed[0].session.ended, Some(Ended::Ended));
    assert!(listed[0].session.ended_at_ms.is_some());
    assert_eq!(listed[1].session.ended, None);
    assert_eq!(listed[1].colour_slot, 5);
    // A stretch that ended before they opened has neither.
    assert_eq!(
        bookings::sessions_between(&pool, s, now - 3 * H, now - 2 * H)
            .await
            .unwrap(),
        []
    );
    // A stretch in the future has the open one, which is still running.
    assert_eq!(
        bookings::sessions_between(&pool, s, now - 1_000, now + 2 * H)
            .await
            .unwrap()
            .iter()
            .map(|r| r.session.id)
            .collect::<Vec<_>>(),
        [gone.id, open.id]
    );
}
