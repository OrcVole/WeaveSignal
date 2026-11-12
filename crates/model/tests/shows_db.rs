// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Shows, series and episodes against a real PostgreSQL (migration 0007). Ignored unless asked
//! for: `just test-db` starts a throwaway PostgreSQL 18 and runs these.

use jiff::Timestamp;
use jiff::tz::TimeZone;
use weavesignal_model::shows::{
    self, CorrectionInput, EpisodeKind, EpisodePatch, NewEpisodes, SeriesInput, ShowError,
    ShowInput, Slot,
};
use weavesignal_model::{Station, StationId, ensure_station};

async fn pool() -> (sqlx::PgPool, StationId) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let id = StationId::new_random();
    ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
        .await
        .unwrap();
    (pool, id)
}

fn show_input() -> ShowInput {
    ShowInput {
        name: "Saturday Night Kettle".into(),
        description: "Live music, requests and the Forth after dark.".into(),
        colour_slot: 3,
        layout: "poster".into(),
        slot: Some(Slot {
            weekday: 6,
            start: 20 * 60,
            minutes: Some(180),
        }),
        hosts: vec![
            "Shona Campbell".into(),
            "Catriona MacKenzie-Buchanan".into(),
        ],
        tags: vec!["music".into()],
        explicit: false,
        presenter: None,
        contact: None,
        links: None,
        commercial_music: None,
        presenter_account: None,
        step: None,
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_season_drafts_itself_inherits_and_freezes_at_air() {
    let (pool, station) = pool().await;
    let zone = TimeZone::get("Europe/London").unwrap();
    let show = shows::put_show(&pool, station, "saturday-night", &show_input())
        .await
        .unwrap();
    let series = shows::put_series(
        &pool,
        station,
        &show,
        3,
        &SeriesInput {
            starts_on: Some("2026-09-05".parse().unwrap()),
            planned_episodes: Some(10),
            ..SeriesInput::default()
        },
    )
    .await
    .unwrap();

    // "Create 10 episodes": numbered 1 to 10, every Saturday at eight from 5 September.
    let drafted = shows::create_episodes(
        &pool,
        station,
        &show,
        &series,
        &zone,
        &NewEpisodes {
            count: 10,
            title: "Back for the autumn".into(),
            guests: vec!["Firth Lights".into()],
        },
        Timestamp::now(),
        shows::Maker {
            label: "station key",
            account: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(drafted.len(), 10);
    assert_eq!(
        drafted.iter().map(|e| e.number).collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    let seventh = &drafted[6];
    let local = Timestamp::from_second(seventh.air_at.unwrap())
        .unwrap()
        .to_zoned(zone.clone());
    assert_eq!(
        local.strftime("%Y-%m-%d %H:%M %Z").to_string(),
        "2026-10-17 20:00 BST"
    );
    assert!(drafted[0].guests.len() == 1 && drafted[1].guests.is_empty());

    // One more: number 11, the Saturday after the tenth.
    let next = shows::create_episodes(
        &pool,
        station,
        &show,
        &series,
        &zone,
        &NewEpisodes {
            count: 1,
            ..NewEpisodes::default()
        },
        Timestamp::now(),
        shows::Maker {
            label: "station key",
            account: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(next[0].number, 11);
    assert_eq!(
        next[0].air_at.unwrap() - drafted[9].air_at.unwrap(),
        7 * 24 * 3600
    );

    // Inherited by reference: the series takes new hosts, the episode follows.
    let people = shows::people(&pool, station).await.unwrap();
    let all = shows::list_episodes(&pool, series.id).await.unwrap();
    let e7 = shows::get_episode(&pool, series.id, 7)
        .await
        .unwrap()
        .unwrap();
    let r = shows::resolve_episode(&show, &series, &e7, &people, &zone, &all);
    assert_eq!(r.hosts, ["Shona Campbell", "Catriona MacKenzie-Buchanan"]);
    let series = shows::put_series(
        &pool,
        station,
        &show,
        3,
        &SeriesInput {
            hosts: Some(vec!["Hamish Wallace".into()]),
            starts_on: Some("2026-09-05".parse().unwrap()),
            planned_episodes: Some(10),
            ..SeriesInput::default()
        },
    )
    .await
    .unwrap();
    let people = shows::people(&pool, station).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &e7, &people, &zone, &all);
    assert_eq!(r.hosts, ["Hamish Wallace"]);

    // Set on the episode, then reset to inherit.
    let patch: EpisodePatch = serde_json::from_str(
        r#"{"title": "Ghost Train night", "hosts": ["Shona Campbell"], "guests": ["Firth Lights", "Dr Una Robertson"]}"#,
    )
    .unwrap();
    let e7 = shows::patch_episode(&pool, station, &e7, &patch)
        .await
        .unwrap();
    let people = shows::people(&pool, station).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &e7, &people, &zone, &all);
    assert_eq!(r.hosts, ["Shona Campbell"]);
    assert_eq!(r.guests, ["Firth Lights", "Dr Una Robertson"]);
    let reset: EpisodePatch = serde_json::from_str(r#"{"hosts": null}"#).unwrap();
    let e7 = shows::patch_episode(&pool, station, &e7, &reset)
        .await
        .unwrap();
    assert_eq!(e7.hosts, None);
    assert_eq!(e7.title, "Ghost Train night", "left alone");

    // On air: frozen. A later change to the series does not reach it, and it cannot be edited.
    let r = shows::resolve_episode(&show, &series, &e7, &people, &zone, &all);
    assert!(shows::freeze_episode(&pool, e7.id, &r).await.unwrap());
    assert!(
        !shows::freeze_episode(&pool, e7.id, &r).await.unwrap(),
        "the first freeze stands"
    );
    let e7 = shows::get_episode(&pool, series.id, 7)
        .await
        .unwrap()
        .unwrap();
    assert!(e7.aired_at.is_some());
    let series = shows::put_series(
        &pool,
        station,
        &show,
        3,
        &SeriesInput {
            hosts: Some(vec!["Lachlan Scott".into()]),
            ..SeriesInput::default()
        },
    )
    .await
    .unwrap();
    let people = shows::people(&pool, station).await.unwrap();
    let after = shows::resolve_episode(&show, &series, &e7, &people, &zone, &all);
    assert_eq!(after.hosts, ["Hamish Wallace"]);
    assert!(after.frozen);
    assert!(matches!(
        shows::patch_episode(&pool, station, &e7, &reset).await,
        Err(ShowError::Frozen(_))
    ));

    // A feed lists aired episodes with files only.
    assert!(
        shows::published_episodes(&pool, show.id)
            .await
            .unwrap()
            .is_empty()
    );
    shows::set_files(
        &pool,
        e7.id,
        &serde_json::json!({"enclosure": {"file": "x.mp3"}}),
    )
    .await
    .unwrap();
    let published = shows::published_episodes(&pool, show.id).await.unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].1.number, 7);

    // The feed's guid is kept once.
    let g = shows::keep_feed_guid(&pool, show.id, uuid::Uuid::from_u128(7))
        .await
        .unwrap();
    let again = shows::keep_feed_guid(&pool, show.id, uuid::Uuid::from_u128(8))
        .await
        .unwrap();
    assert_eq!(g, again);

    // Guests are remembered: one row per name.
    let ids = shows::people_ids(&pool, station, &["Firth Lights".into()])
        .await
        .unwrap();
    assert_eq!(
        ids,
        shows::people_ids(&pool, station, &["Firth Lights".into()])
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn bad_shows_are_refused_with_the_field_named() {
    let (pool, station) = pool().await;
    let mut bad = show_input();
    bad.colour_slot = 13;
    assert!(matches!(
        shows::put_show(&pool, station, "saturday-night", &bad).await,
        Err(ShowError::Invalid(m)) if m.contains("colour")
    ));
    assert!(matches!(
        shows::put_show(&pool, station, "../etc", &show_input()).await,
        Err(ShowError::Invalid(_))
    ));
    let mut bad = show_input();
    bad.layout = "billboard".into();
    assert!(shows::put_show(&pool, station, "x", &bad).await.is_err());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_erased_guest_leaves_every_list_and_what_aired() {
    // ADR-0122: the record goes; the name leaves the lists, the frozen copy and its mentions
    // are reported for a person to edit.
    let (pool, station) = pool().await;
    let zone = TimeZone::get("Europe/London").unwrap();
    let show = shows::put_show(&pool, station, "kettle-talk", &show_input())
        .await
        .unwrap();
    let series = shows::put_series(
        &pool,
        station,
        &show,
        1,
        &SeriesInput {
            starts_on: Some("2026-09-05".parse().unwrap()),
            ..SeriesInput::default()
        },
    )
    .await
    .unwrap();
    let eps = shows::create_episodes(
        &pool,
        station,
        &show,
        &series,
        &zone,
        &NewEpisodes {
            count: 3,
            title: "Kettle talk".into(),
            guests: vec!["Morag Ross".into(), "Gregor Stewart".into()],
        },
        Timestamp::now(),
        shows::Maker {
            label: "station key",
            account: None,
        },
    )
    .await
    .unwrap();
    // Drafting puts the guests on the first; the second has her too, and names her in its
    // description.
    let patch: EpisodePatch = serde_json::from_str(
        r#"{"description": "With Morag Ross on the ferries.", "guests": ["Morag Ross"]}"#,
    )
    .unwrap();
    shows::patch_episode(&pool, station, &eps[1], &patch)
        .await
        .unwrap();
    // The first has aired: frozen with her name in it.
    let people = shows::people(&pool, station).await.unwrap();
    let all = shows::list_episodes(&pool, series.id).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &all[0], &people, &zone, &all);
    assert!(r.guests.contains(&"Morag Ross".to_owned()));
    assert!(shows::freeze_episode(&pool, all[0].id, &r).await.unwrap());

    let morag = *people
        .iter()
        .find(|(_, n)| n.as_str() == "Morag Ross")
        .unwrap()
        .0;
    let gone = shows::erase_person(&pool, station, morag)
        .await
        .unwrap()
        .expect("she was there");
    assert_eq!(gone.name, "Morag Ross");
    assert_eq!(
        gone.episodes,
        {
            let mut v = vec![eps[0].id, eps[1].id];
            v.sort();
            v
        },
        "the two that had her"
    );
    assert_eq!(
        gone.mentions,
        vec![eps[1].id],
        "the description that names her"
    );

    let people = shows::people(&pool, station).await.unwrap();
    assert!(
        !people.values().any(|n| n == "Morag Ross"),
        "her record is gone"
    );
    for e in shows::list_episodes(&pool, series.id).await.unwrap() {
        assert!(!e.guests.contains(&morag), "episode {}", e.number);
        if let Some(f) = &e.frozen {
            assert_eq!(
                f.guests,
                ["Gregor Stewart"],
                "what aired no longer names her"
            );
        }
    }
    assert!(
        shows::erase_person(&pool, station, morag)
            .await
            .unwrap()
            .is_none(),
        "erased once"
    );
}

/// A show with one drafted episode, in the station's zone.
async fn one_episode(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
) -> (
    shows::Show,
    shows::Series,
    shows::Episode,
    jiff::tz::TimeZone,
) {
    let zone = TimeZone::get("Europe/London").unwrap();
    let show = shows::put_show(pool, station, slug, &show_input())
        .await
        .unwrap();
    let series = shows::put_series(
        pool,
        station,
        &show,
        1,
        &SeriesInput {
            starts_on: Some("2026-09-05".parse().unwrap()),
            ..SeriesInput::default()
        },
    )
    .await
    .unwrap();
    let ep = shows::create_episodes(
        pool,
        station,
        &show,
        &series,
        &zone,
        &NewEpisodes {
            count: 1,
            title: "The ferrys".into(),
            guests: vec!["Morag Ross".into(), "Gregor Stuart".into()],
        },
        Timestamp::now(),
        shows::Maker {
            label: "station key",
            account: None,
        },
    )
    .await
    .unwrap()
    .remove(0);
    (show, series, ep, zone)
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_episode_is_live_until_marked_pre_recorded_and_the_frozen_copy_keeps_the_kind() {
    // ADR-0188 (migration 0015).
    let (pool, station) = pool().await;
    let (show, series, ep, zone) = one_episode(&pool, station, "kettle-kind").await;
    assert_eq!(ep.kind, EpisodeKind::Live, "drafted live");

    let patch: EpisodePatch = serde_json::from_str(r#"{"kind": "prerecorded"}"#).unwrap();
    let ep = shows::patch_episode(&pool, station, &ep, &patch)
        .await
        .unwrap();
    assert_eq!(ep.kind, EpisodeKind::Prerecorded);
    let again = shows::get_episode(&pool, series.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.kind, EpisodeKind::Prerecorded, "kept");
    // A patch that leaves the kind out leaves it alone.
    let title: EpisodePatch = serde_json::from_str(r#"{"title": "The ferries"}"#).unwrap();
    let ep = shows::patch_episode(&pool, station, &ep, &title)
        .await
        .unwrap();
    assert_eq!(ep.kind, EpisodeKind::Prerecorded);
    assert!(serde_json::from_str::<EpisodePatch>(r#"{"kind": "studio-b"}"#).is_err());

    let people = shows::people(&pool, station).await.unwrap();
    let all = shows::list_episodes(&pool, series.id).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &ep, &people, &zone, &all);
    assert_eq!(r.episode.as_ref().unwrap().kind, EpisodeKind::Prerecorded);
    assert!(shows::freeze_episode(&pool, ep.id, &r).await.unwrap());
    let aired = shows::get_episode(&pool, series.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        aired
            .frozen
            .as_ref()
            .unwrap()
            .episode
            .as_ref()
            .unwrap()
            .kind,
        EpisodeKind::Prerecorded
    );
    assert!(
        matches!(
            shows::patch_episode(&pool, station, &aired, &patch).await,
            Err(ShowError::Frozen(_))
        ),
        "the kind aired is the kind broadcast"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_correction_changes_the_aired_text_and_logs_who_and_why() {
    // ADR-0177 (migration 0016): the frozen record is corrected in place, the old values stay
    // in the log, and an erasure redacts the log too.
    let (pool, station) = pool().await;
    let (show, series, ep, zone) = one_episode(&pool, station, "kettle-fix").await;
    let fix = CorrectionInput {
        title: Some("The ferries".into()),
        guests: Some(vec!["Morag Ross".into(), "Gregor Stewart".into()]),
        reason: "Misspelt the title and Gregor's surname.".into(),
        ..CorrectionInput::default()
    };

    // Not aired yet: edit it instead.
    assert!(matches!(
        shows::correct_episode(&pool, station, ep.id, &fix, "morag", None, None).await,
        Err(ShowError::NotAired(_))
    ));

    let people = shows::people(&pool, station).await.unwrap();
    let all = shows::list_episodes(&pool, series.id).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &ep, &people, &zone, &all);
    assert!(shows::freeze_episode(&pool, ep.id, &r).await.unwrap());

    // A reason is needed, and a correction that changes nothing is refused.
    let no_reason = CorrectionInput {
        reason: " ".into(),
        ..fix.clone()
    };
    assert!(matches!(
        shows::correct_episode(&pool, station, ep.id, &no_reason, "morag", None, None).await,
        Err(ShowError::Invalid(m)) if m.contains("reason")
    ));
    let nothing = CorrectionInput {
        title: Some("The ferrys".into()),
        reason: "No change.".into(),
        ..CorrectionInput::default()
    };
    assert!(matches!(
        shows::correct_episode(&pool, station, ep.id, &nothing, "morag", None, None).await,
        Err(ShowError::Invalid(m)) if m.contains("changes nothing")
    ));
    assert!(
        shows::list_corrections(&pool, ep.id)
            .await
            .unwrap()
            .is_empty()
    );

    let line = shows::correct_episode(&pool, station, ep.id, &fix, "morag", None, None)
        .await
        .unwrap();
    assert_eq!(line.made_by, "morag");
    assert_eq!(line.reason, "Misspelt the title and Gregor's surname.");
    assert_eq!(line.old_values["title"], "The ferrys");
    assert_eq!(line.new_values["title"], "The ferries");
    assert_eq!(
        line.old_values["guests"],
        serde_json::json!(["Morag Ross", "Gregor Stuart"])
    );
    assert_eq!(
        line.new_values["guests"],
        serde_json::json!(["Morag Ross", "Gregor Stewart"])
    );
    assert!(
        line.old_values.get("description").is_none(),
        "only what changed is logged"
    );

    let fixed = shows::get_episode(&pool, series.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fixed.id, ep.id, "the same episode, the same id");
    assert!(fixed.aired_at.is_some());
    let frozen = fixed.frozen.as_ref().unwrap();
    assert_eq!(frozen.episode.as_ref().unwrap().title, "The ferries");
    assert_eq!(frozen.guests, ["Morag Ross", "Gregor Stewart"]);
    assert_eq!(
        fixed.title, "The ferries",
        "the column follows the frozen copy"
    );
    // What was broadcast is not text and did not move.
    assert_eq!(frozen.episode.as_ref().unwrap().air_at, ep.air_at);
    assert_eq!(
        frozen.hosts,
        ["Shona Campbell", "Catriona MacKenzie-Buchanan"]
    );
    let people = shows::people(&pool, station).await.unwrap();
    let all = shows::list_episodes(&pool, series.id).await.unwrap();
    let r = shows::resolve_episode(&show, &series, &fixed, &people, &zone, &all);
    assert_eq!(
        &r, frozen,
        "what the cards and the feed read is the corrected copy"
    );

    // A second correction is a second line; the log reads oldest first.
    let again = CorrectionInput {
        description: Some("Ferries, with Gregor Stewart.".into()),
        reason: "Added a line.".into(),
        ..CorrectionInput::default()
    };
    shows::correct_episode(&pool, station, ep.id, &again, "station key", None, None)
        .await
        .unwrap();
    let log = shows::list_corrections(&pool, ep.id).await.unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].reason, fix.reason);
    assert_eq!(log[1].made_by, "station key");

    // Erasing the misspelt guest redacts the log: the old name goes, the rest stays.
    let stuart = *people
        .iter()
        .find(|(_, n)| n.as_str() == "Gregor Stuart")
        .unwrap()
        .0;
    let gone = shows::erase_person(&pool, station, stuart)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gone.corrections_redacted, 1);
    let log = shows::list_corrections(&pool, ep.id).await.unwrap();
    assert!(log[0].redacted && !log[1].redacted);
    assert_eq!(
        log[0].old_values["guests"],
        serde_json::json!(["Morag Ross"])
    );
    assert_eq!(log[0].old_values["title"], "The ferrys");
    assert_eq!(
        log[0].new_values["guests"],
        serde_json::json!(["Morag Ross", "Gregor Stewart"])
    );

    // Erasing someone the corrected record names takes them from the new values too.
    let stewart = shows::people_ids(&pool, station, &["Gregor Stewart".into()])
        .await
        .unwrap()[0];
    shows::erase_person(&pool, station, stewart)
        .await
        .unwrap()
        .unwrap();
    let log = shows::list_corrections(&pool, ep.id).await.unwrap();
    assert_eq!(
        log[0].new_values["guests"],
        serde_json::json!(["Morag Ross"])
    );
    assert_eq!(log[1].new_values["description"], "Ferries, with [erased].");
    assert!(log[1].redacted);
}

// ---- the beginner steps (ADR-0271, migration 0033) ----

use weavesignal_model::shows::{Step, StepBy};

fn by(label: &str) -> StepBy {
    StepBy {
        label: label.into(),
        account: None,
    }
}

/// One episode of `show`, air time set directly (the calendar's rule is not what is tested).
async fn episode_at(pool: &sqlx::PgPool, station: StationId, slug: &str, air_at: i64) {
    let zone = TimeZone::get("Europe/London").unwrap();
    let show = shows::get_show(pool, station, slug).await.unwrap().unwrap();
    let series = shows::put_series(pool, station, &show, 1, &SeriesInput::default())
        .await
        .unwrap();
    let made = shows::create_episodes(
        pool,
        station,
        &show,
        &series,
        &zone,
        &NewEpisodes {
            count: 1,
            ..NewEpisodes::default()
        },
        Timestamp::now(),
        shows::Maker {
            label: "station key",
            account: None,
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE episode SET air_at = to_timestamp($1) WHERE id = $2")
        .bind(air_at as f64)
        .bind(made[0].id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_new_show_starts_at_solo_and_a_save_without_a_step_keeps_the_one_it_has() {
    let (pool, station) = pool().await;
    let made = shows::put_show(&pool, station, "morning-kettle", &show_input())
        .await
        .unwrap();
    assert_eq!(made.step, Step::Solo, "a show made without the field");
    // A save that leaves the step out keeps whatever it has.
    shows::set_step(
        &pool,
        station,
        "morning-kettle",
        Step::Callers,
        &by("eilidh"),
    )
    .await
    .unwrap();
    let saved = shows::put_show(&pool, station, "morning-kettle", &show_input())
        .await
        .unwrap();
    assert_eq!(saved.step, Step::Callers);
    // One that sends it sets it, on a new show too.
    let sent = ShowInput {
        step: Some(Step::Full),
        ..show_input()
    };
    assert_eq!(
        shows::put_show(&pool, station, "morning-kettle", &sent)
            .await
            .unwrap()
            .step,
        Step::Full
    );
    let new = shows::put_show(&pool, station, "steeped", &sent)
        .await
        .unwrap();
    assert_eq!(new.step, Step::Full);
    // A show inserted the way the migration left the column's default: solo.
    let raw: String = sqlx::query_scalar(
        "INSERT INTO show (id, station_id, slug, name, colour_slot, layout)
         VALUES (gen_random_uuid(), $1, 'bare', 'Bare', 1, 'poster') RETURNING step",
    )
    .bind(station.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(raw, "solo");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn changing_a_step_alone_is_logged_with_who_and_a_repeat_is_not() {
    let (pool, station) = pool().await;
    let show = shows::put_show(&pool, station, "morning-kettle", &show_input())
        .await
        .unwrap();
    let (now, before) = shows::set_step(
        &pool,
        station,
        "morning-kettle",
        Step::Guests,
        &by("eilidh"),
    )
    .await
    .unwrap();
    assert_eq!((before, now.step), (Step::Solo, Step::Guests));
    // Nothing changes, nothing is logged.
    let (_, before) = shows::set_step(
        &pool,
        station,
        "morning-kettle",
        Step::Guests,
        &by("eilidh"),
    )
    .await
    .unwrap();
    assert_eq!(before, Step::Guests);
    shows::set_step(
        &pool,
        station,
        "morning-kettle",
        Step::Solo,
        &by("station key"),
    )
    .await
    .unwrap();
    // The editor's save logs through the other door.
    shows::log_step_change(
        &pool,
        station,
        show.id,
        Step::Solo,
        Step::Full,
        &by("shona"),
    )
    .await
    .unwrap();
    let log = shows::step_changes(&pool, show.id).await.unwrap();
    assert_eq!(
        log.iter()
            .map(|c| (c.from, c.to, c.by.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (Step::Solo, Step::Guests, "eilidh"),
            (Step::Guests, Step::Solo, "station key"),
            (Step::Solo, Step::Full, "shona"),
        ]
    );
    assert!(log.iter().all(|c| c.at_ms > 1_700_000_000_000));
    // A show that is not there.
    assert!(matches!(
        shows::set_step(&pool, station, "nope", Step::Full, &by("x")).await,
        Err(ShowError::Missing(_))
    ));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_show_last_on_air_and_the_next_come_from_the_schedule() {
    let (pool, station) = pool().await;
    let now = Timestamp::now().as_second();
    // Nothing has aired: no show last on air, none next.
    assert!(
        shows::last_started(&pool, station, now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        shows::next_start(&pool, station, now)
            .await
            .unwrap()
            .is_none()
    );

    for (slug, step) in [
        ("kettle", Step::Solo),
        ("steeped", Step::Full),
        ("late", Step::Callers),
    ] {
        shows::put_show(
            &pool,
            station,
            slug,
            &ShowInput {
                step: Some(step),
                ..show_input()
            },
        )
        .await
        .unwrap();
    }
    // Kettle aired three days ago and Steeped yesterday; Late is on in an hour.
    episode_at(&pool, station, "kettle", now - 3 * 86_400).await;
    episode_at(&pool, station, "steeped", now - 86_400).await;
    episode_at(&pool, station, "late", now + 3_600).await;

    let (last, at) = shows::last_started(&pool, station, now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (last.slug.as_str(), last.step, at),
        ("steeped", Step::Full, now - 86_400)
    );
    let (next, at) = shows::next_start(&pool, station, now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (next.slug.as_str(), next.step, at),
        ("late", Step::Callers, now + 3_600)
    );
    // An hour on, Late is the show last on air and nothing is next.
    let (last, _) = shows::last_started(&pool, station, now + 3_700)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last.slug, "late");
    assert!(
        shows::next_start(&pool, station, now + 3_700)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_chat_counts_match_what_the_chat_lists() {
    let (pool, station) = pool().await;
    let st = station.as_uuid();
    let none = weavesignal_model::chat::moderation_counts(&pool, station)
        .await
        .unwrap();
    assert_eq!((none.held, none.reports), (0, 0));
    for (id, state) in [(1_i64, "held"), (2, "held"), (3, "visible"), (4, "hidden")] {
        sqlx::query(
            "INSERT INTO chat_message (station_id, id, kind, body, server_at, state)
             VALUES ($1, $2, 'chat', 'hello', now(), $3)",
        )
        .bind(st)
        .bind(id)
        .bind(state)
        .execute(&pool)
        .await
        .unwrap();
    }
    // Two reports of message 1 count once; a notice counts alone; an escalated one counts; a
    // dismissed one does not.
    for (kind, message, status) in [
        ("report", Some(1_i64), "open"),
        ("report", Some(1), "open"),
        ("report", Some(2), "escalated"),
        ("notice", None, "open"),
        ("report", Some(3), "dismissed"),
    ] {
        sqlx::query(
            "INSERT INTO chat_report (id, station_id, kind, message_id, reason, status)
             VALUES (gen_random_uuid(), $1, $2, $3, 'abuse', $4)",
        )
        .bind(st)
        .bind(kind)
        .bind(message)
        .bind(status)
        .execute(&pool)
        .await
        .unwrap();
    }
    let counts = weavesignal_model::chat::moderation_counts(&pool, station)
        .await
        .unwrap();
    assert_eq!((counts.held, counts.reports), (2, 3));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_migration_leaves_existing_shows_at_full_and_new_ones_at_solo() {
    // Run migration 0033's own statements on a stand-in for the table as it was before.
    let sql = include_str!("../migrations/0033_show_step.sql");
    let code: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let alters: Vec<String> = code
        .split(';')
        .map(|s| s.trim().to_owned())
        .filter(|s| s.starts_with("ALTER TABLE show"))
        .collect();
    assert_eq!(
        alters.len(),
        2,
        "the migration adds the column, then sets the default"
    );
    let (pool, _) = pool().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE show (id int PRIMARY KEY, name text NOT NULL)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO show VALUES (1, 'Saturday Night Kettle')")
        .execute(&mut *tx)
        .await
        .unwrap();
    for a in &alters {
        sqlx::query(sqlx::AssertSqlSafe(a.clone()))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO show (id, name) VALUES (2, 'New')")
        .execute(&mut *tx)
        .await
        .unwrap();
    let steps: Vec<(i32, String)> = sqlx::query_as("SELECT id, step FROM show ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(steps, vec![(1, "full".to_owned()), (2, "solo".to_owned())]);
    tx.rollback().await.unwrap();
}

// ---- a cart wall per show (migration 0034) ----

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_shows_cart_banks_are_its_own_and_numbered_apart_from_the_stations() {
    use weavesignal_model::carts::{self, Bank, Cart, CartMode};
    let (pool, station) = pool().await;
    let kettle = shows::put_show(&pool, station, "morning-kettle", &show_input())
        .await
        .unwrap();
    let steeped = shows::put_show(&pool, station, "steeped", &show_input())
        .await
        .unwrap();
    let bank = |name: &str| Bank {
        number: 2,
        name: name.into(),
        colour_slot: 2,
        show: None,
    };
    let cart = |label: &str| Cart {
        id: uuid::Uuid::new_v4(),
        bank: 2,
        slot: 1,
        label: label.into(),
        detail: String::new(),
        mode: CartMode::OneShot,
        file: Some(format!("{}.flac", uuid::Uuid::new_v4())),
        duration_ms: 1_000,
        gain_db: 0.0,
        loudness_lufs: None,
        true_peak_dbtp: None,
        ramp_ms: None,
        origin: String::new(),
        licence: String::new(),
        show: None,
    };
    // Bank 2 three times over: the station's, The Morning Kettle's and Steeped's.
    carts::put_bank(&pool, station, None, &bank("Station effects"))
        .await
        .unwrap();
    // A show cannot fill a bank it has not made, even where the station has one.
    assert!(
        carts::put_cart(&pool, station, Some(kettle.id), &cart("Miaow"))
            .await
            .is_err()
    );
    carts::put_bank(&pool, station, Some(kettle.id), &bank("Cat noises"))
        .await
        .unwrap();
    carts::put_bank(&pool, station, Some(steeped.id), &bank("Dog woofs"))
        .await
        .unwrap();
    // Renaming one changes only that one.
    carts::put_bank(&pool, station, Some(steeped.id), &bank("Woofs"))
        .await
        .unwrap();
    let station_cart = cart("Applause");
    let cat = cart("Miaow");
    let dog = cart("Woof");
    carts::put_cart(&pool, station, None, &station_cart)
        .await
        .unwrap();
    carts::put_cart(&pool, station, Some(kettle.id), &cat)
        .await
        .unwrap();
    carts::put_cart(&pool, station, Some(steeped.id), &dog)
        .await
        .unwrap();
    let (banks, list) = carts::load(&pool, station).await.unwrap();
    let named: Vec<(Option<&str>, &str)> = banks
        .iter()
        .map(|b| (b.show.as_deref(), b.name.as_str()))
        .collect();
    assert_eq!(
        named,
        vec![
            (None, "Station effects"),
            (Some("morning-kettle"), "Cat noises"),
            (Some("steeped"), "Woofs"),
        ]
    );
    let labels: Vec<(Option<&str>, &str)> = list
        .iter()
        .map(|c| (c.show.as_deref(), c.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        vec![
            (None, "Applause"),
            (Some("morning-kettle"), "Miaow"),
            (Some("steeped"), "Woof"),
        ]
    );
    // A new cart in Steeped's slot replaces only Steeped's.
    let bark = cart("Bark");
    assert_eq!(
        carts::put_cart(&pool, station, Some(steeped.id), &bark)
            .await
            .unwrap(),
        dog.file.clone()
    );
    // Emptying the show's slot leaves the station's and the other show's.
    assert_eq!(
        carts::delete_cart(&pool, station, Some(steeped.id), 2, 1)
            .await
            .unwrap(),
        Some((bark.id, bark.file.clone()))
    );
    // Removing a show's bank takes its carts and nothing else.
    assert_eq!(
        carts::delete_show_bank(&pool, station, kettle.id, 2)
            .await
            .unwrap(),
        Some(vec![(cat.id, cat.file.clone())])
    );
    assert_eq!(
        carts::delete_show_bank(&pool, station, kettle.id, 2)
            .await
            .unwrap(),
        None
    );
    let (banks, list) = carts::load(&pool, station).await.unwrap();
    assert_eq!(banks.len(), 2);
    assert_eq!(list, vec![station_cart]);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_cart_migration_makes_every_bank_there_the_stations_and_keeps_its_carts() {
    // Run migration 0003's tables and 0034's statements on stand-ins in a transaction: the
    // banks and carts as a station had them before 1.0.2.
    let strip = |sql: &str| -> Vec<String> {
        sql.lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .split(';')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let before = strip(include_str!("../migrations/0003_carts.sql"));
    let after = strip(include_str!("../migrations/0034_show_cart_banks.sql"));
    let (pool, station) = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let run = |s: String| sqlx::AssertSqlSafe(s);
    // Temporary tables may only point at temporary tables: stand-ins for the station and shows.
    for s in [
        "CREATE TEMP TABLE station (id uuid PRIMARY KEY)",
        "CREATE TEMP TABLE show (id uuid PRIMARY KEY)",
    ] {
        sqlx::query(s).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO station VALUES ($1)")
        .bind(station.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    for s in before {
        sqlx::query(run(s.replace("CREATE TABLE ", "CREATE TEMP TABLE ")))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let st = station.as_uuid();
    for (n, name) in [(1_i16, "Reactions"), (4, "Idents")] {
        sqlx::query("INSERT INTO cart_bank VALUES ($1, $2, $3, $2)")
            .bind(st)
            .bind(n)
            .bind(name)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for (bank, slot, label) in [(1_i16, 1_i16, "Applause"), (4, 3, "Top of the hour")] {
        sqlx::query(
            "INSERT INTO cart (id, station_id, bank, slot, label, mode)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, 'one_shot')",
        )
        .bind(st)
        .bind(bank)
        .bind(slot)
        .bind(label)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    for s in after {
        sqlx::query(run(s)).execute(&mut *tx).await.unwrap();
    }
    // Every bank is the station's; every cart is in the bank of its number.
    let banks: Vec<(i16, Option<uuid::Uuid>)> =
        sqlx::query_as("SELECT number, show_id FROM cart_bank ORDER BY number")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(banks, vec![(1, None), (4, None)]);
    let carts: Vec<(String, i16)> = sqlx::query_as(
        "SELECT c.label, b.number FROM cart c JOIN cart_bank b ON b.id = c.bank_id
         ORDER BY c.label",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        carts,
        vec![
            ("Applause".to_owned(), 1),
            ("Top of the hour".to_owned(), 4)
        ]
    );
    // A show's bank 1 is not the station's bank 1; a second station bank 1 is refused.
    let show = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO show VALUES ($1)")
        .bind(show)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO cart_bank (id, station_id, show_id, number, name, colour_slot)
         VALUES (gen_random_uuid(), $1, $2, 1, 'Cat noises', 2)",
    )
    .bind(st)
    .bind(show)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("SAVEPOINT again")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO cart_bank (id, station_id, number, name, colour_slot)
             VALUES (gen_random_uuid(), $1, 1, 'Again', 2)",
        )
        .bind(st)
        .execute(&mut *tx)
        .await
        .is_err()
    );
    sqlx::query("ROLLBACK TO SAVEPOINT again")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
}
