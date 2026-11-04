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
