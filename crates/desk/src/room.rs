// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Following who is in the room.
//!
//! The desk lists the room's participants through LiveKit's server API with a room-admin
//! token it signs itself, and reconciles the desk with what it finds: a newcomer with a role
//! gets a mix-minus, and a subscription once they publish audio; someone who leaves is
//! dropped at once and their mix-minus kept for a grace period, so a dropped link that rejoins
//! finds it again; a rejoin (a new session) is subscribed afresh; a subscription that routes
//! nothing in time is made again, and so is one that brings no packets for a while, unless
//! the person's audio is muted in the room (packets, not sound: digital silence arrives like
//! any other, issue 13). Participants without a role in their token (the desk's own
//! identities, recorders, anything not issued by the control plane) are ignored, and so are
//! listeners (the `listener` role: monitors, test drivers' ears), who only listen.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use weavesignal_proto::{Keys, ROLE_ATTRIBUTE, Role};

use crate::pipeline::Desk;

/// A participant as the room lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub identity: String,
    /// The name others see (the token's name).
    pub name: String,
    pub sid: String,
    pub role: Role,
    /// Whether they publish an audio track.
    pub has_audio: bool,
    /// Whether every audio track they publish is muted in the room. LiveKit then forwards
    /// nothing of it, so a voice that brings no packets is not a fault while it lasts.
    pub muted: bool,
    /// Their booth, for a caller (ADR-0193); `None` in the studio room.
    pub room: Option<String>,
}

#[derive(Deserialize)]
struct ListReply {
    #[serde(default)]
    participants: Vec<ParticipantInfo>,
}

#[derive(Deserialize)]
struct ParticipantInfo {
    #[serde(default)]
    sid: String,
    #[serde(default)]
    identity: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    tracks: Vec<TrackInfo>,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct TrackInfo {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    muted: bool,
}

/// Parse a `ListParticipants` reply into the people the desk mixes: those with a role, who
/// are not the desk itself and have not disconnected.
pub fn parse(reply: &str, desk_prefix: &str) -> Result<Vec<Seen>> {
    let reply: ListReply =
        serde_json::from_str(reply).context("LiveKit's participant list is not JSON")?;
    let own = format!("{desk_prefix}-");
    Ok(reply
        .participants
        .into_iter()
        .filter(|p| p.identity != desk_prefix && !p.identity.starts_with(&own))
        .filter(|p| p.state != "DISCONNECTED")
        .filter_map(|p| {
            let role: Role = p.attributes.get(ROLE_ATTRIBUTE)?.parse().ok()?;
            if !role.at_desk() {
                return None;
            }
            let mut audio = p.tracks.iter().filter(|t| t.kind == "AUDIO").peekable();
            let has_audio = audio.peek().is_some();
            Some(Seen {
                has_audio,
                muted: has_audio && audio.all(|t| t.muted),
                identity: p.identity,
                name: p.name,
                sid: p.sid,
                role,
                room: None,
            })
        })
        .collect())
}

/// As [`parse`], for a caller's booth (ADR-0193): only the caller it belongs to counts, so a
/// token that names someone else's booth, or any other role, mixes nobody.
pub fn parse_booth(reply: &str, desk_prefix: &str, booth: &str, owner: &str) -> Result<Vec<Seen>> {
    Ok(parse(reply, desk_prefix)?
        .into_iter()
        .filter(|s| s.role == Role::Caller && s.identity == owner)
        .map(|s| Seen {
            room: Some(booth.to_owned()),
            ..s
        })
        .collect())
}

#[derive(Deserialize)]
struct RoomsReply {
    #[serde(default)]
    rooms: Vec<RoomInfo>,
}

#[derive(Deserialize)]
struct RoomInfo {
    name: String,
    #[serde(default, rename = "numParticipants", alias = "num_participants")]
    num_participants: u32,
}

/// The booths of a studio room that anyone is in, from a `ListRooms` reply, with their owners.
pub fn parse_booths(reply: &str, studio_room: &str) -> Result<Vec<(String, String)>> {
    let reply: RoomsReply =
        serde_json::from_str(reply).context("LiveKit's room list is not JSON")?;
    Ok(reply
        .rooms
        .into_iter()
        .filter(|r| r.num_participants > 0)
        .filter_map(|r| {
            let owner = weavesignal_proto::booth_owner(studio_room, &r.name)?.to_owned();
            Some((r.name, owner))
        })
        .collect())
}

/// LiveKit's server API for the studio room and its booths.
pub struct RoomApi {
    agent: ureq::Agent,
    url: String,
    keys: Keys,
    room: String,
    desk_prefix: String,
}

impl RoomApi {
    pub fn new(url: &str, keys: Keys, room: &str, desk_prefix: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            url: url.trim_end_matches('/').to_owned(),
            keys,
            room: room.to_owned(),
            desk_prefix: desk_prefix.to_owned(),
        }
    }

    /// Everyone the desk should mix: the studio room, then each caller's booth (ADR-0193).
    /// A room nobody has opened yet is empty.
    pub fn list(&self) -> Result<Vec<Seen>> {
        let mut seen = parse(
            &self.call(
                "ListParticipants",
                &self.room,
                serde_json::json!({ "room": self.room }),
            )?,
            &self.desk_prefix,
        )?;
        let rooms = self.call("ListRooms", &self.room, serde_json::json!({}))?;
        for (booth, owner) in parse_booths(&rooms, &self.room)? {
            let reply = self.call(
                "ListParticipants",
                &booth,
                serde_json::json!({ "room": booth }),
            )?;
            seen.extend(parse_booth(&reply, &self.desk_prefix, &booth, &owner)?);
        }
        Ok(seen)
    }

    /// One call to LiveKit's room service, as an admin of `room`; a room that is not there
    /// answers an empty reply.
    fn call(&self, method: &str, room: &str, body: serde_json::Value) -> Result<String> {
        let token = self.keys.room_admin(room, 60);
        let mut res = self
            .agent
            .post(&format!("{}/twirp/livekit.RoomService/{method}", self.url))
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .context("cannot reach LiveKit's server API")?;
        let status = res.status().as_u16();
        let text = res
            .body_mut()
            .read_to_string()
            .context("cannot read LiveKit's reply")?;
        match status {
            200 => Ok(text),
            404 => Ok("{}".to_owned()),
            _ => anyhow::bail!("LiveKit's server API answered {status}: {text}"),
        }
    }
}

/// What to do to bring the desk in line with the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Someone to mix, in their room (`None`: the studio room).
    Add(String, Role, Option<String>),
    Subscribe(String, String),
    Resubscribe(String, String),
    Away(String),
    Remove(String),
}

/// The desk as the room watch needs to see it.
#[derive(Clone, Debug, Default)]
pub struct DeskView {
    /// Each person: their role, and since when they have been away.
    pub people: BTreeMap<String, (Role, Option<Duration>)>,
    /// Each subscription: its session, whether it routed audio, its age, and how long its
    /// voice has brought no audio.
    pub subs: BTreeMap<String, (String, bool, Duration, Option<Duration>)>,
    /// How many times in a row each person's subscription has been made again for a fault
    /// (never routed, or routed and silent); kept by the room watch, empty in a fresh view.
    pub retries: BTreeMap<String, u32>,
    /// Each person's room as the desk holds it (`None`: the studio room); someone the desk does
    /// not list here is taken to be where they are seen.
    pub rooms: BTreeMap<String, Option<String>>,
    /// How long since each person was last seen with their audio muted (kept by the room watch,
    /// empty in a fresh view): nothing arrived while they were muted, so their voice is given
    /// a full wait from then.
    pub unmuted: BTreeMap<String, Duration>,
}

/// How long a subscription is given before it is made again: `link_timeout` the first time,
/// then doubling to eight times it (8, 16, 32, 64 s at the default). A source that never
/// sends would otherwise cost a new `livekitwebrtcsrc` (signalling, ICE, a participant in the
/// room) every `link_timeout` for as long as its person stays (review 0025, D1).
pub fn retry_wait(link_timeout: Duration, retries: u32) -> Duration {
    link_timeout * (1u32 << retries.min(3))
}

/// The actions that reconcile the desk with the room. Pure, so it is tested on its own.
pub fn reconcile(
    seen: &[Seen],
    desk: &DeskView,
    leave_grace: Duration,
    link_timeout: Duration,
) -> Vec<Action> {
    let mut out = Vec::new();
    let here: BTreeSet<&str> = seen.iter().map(|s| s.identity.as_str()).collect();
    for s in seen {
        let wait = retry_wait(
            link_timeout,
            desk.retries.get(&s.identity).copied().unwrap_or(0),
        );
        let known = desk.people.get(&s.identity);
        let moved = desk.rooms.get(&s.identity).is_some_and(|r| *r != s.room);
        if moved || known.is_none_or(|(role, away)| *role != s.role || away.is_some()) {
            out.push(Action::Add(s.identity.clone(), s.role, s.room.clone()));
        }
        if !s.has_audio {
            continue;
        }
        // Nothing arrives from a muted track (LiveKit stops forwarding it), so the time it was
        // muted does not count against the voice: it is waited for afresh from its unmuting.
        let since_muted = if s.muted {
            Some(Duration::ZERO)
        } else {
            desk.unmuted.get(&s.identity).copied()
        };
        let counted = |d: Duration| since_muted.map_or(d, |m| d.min(m));
        match desk.subs.get(&s.identity) {
            None => out.push(Action::Subscribe(s.identity.clone(), s.sid.clone())),
            Some((sid, ..)) if *sid != s.sid => {
                out.push(Action::Resubscribe(s.identity.clone(), s.sid.clone()));
            }
            Some((_, false, age, _)) if counted(*age) > wait => {
                out.push(Action::Resubscribe(s.identity.clone(), s.sid.clone()));
            }
            // Routed, but no packets have come for a while: a rejoin under the same identity
            // whose subscription found the departed session's track (the join pages' rejoin,
            // M3), or a link that died quietly. This counts packets, not sound: digital
            // silence (a microphone muted on the page, its track disabled; a guest who has
            // not spoken) keeps arriving and is no fault (issue 13). The pages publish without
            // DTX, so a microphone that gives samples, zeros included, sends a packet every
            // 20 ms; only a track that gives no samples at all (a Web Audio graph with nothing
            // playing) sends nothing, and that cannot be told from a dead link. A track muted
            // in the room is not a fault.
            Some((_, true, _, Some(silent))) if counted(*silent) > wait => {
                tracing::warn!(participant = %s.identity, silent_ms = silent.as_millis() as u64, retries = desk.retries.get(&s.identity).copied().unwrap_or(0), "a routed voice has brought no audio; subscribing again");
                out.push(Action::Resubscribe(s.identity.clone(), s.sid.clone()));
            }
            Some(_) => {}
        }
    }
    for (id, (_, away)) in &desk.people {
        if here.contains(id.as_str()) {
            continue;
        }
        match away {
            None => out.push(Action::Away(id.clone())),
            Some(d) if *d > leave_grace => out.push(Action::Remove(id.clone())),
            Some(_) => {}
        }
    }
    out
}

impl Desk {
    /// The desk as the room watch sees it.
    pub fn view(&self) -> DeskView {
        let mut v = DeskView::default();
        for p in self.status() {
            let away = self
                .person(&p.identity)
                .and_then(|x| x.away_since)
                .map(|t| t.elapsed());
            if let Some(s) = self.subscription(&p.identity) {
                v.subs
                    .insert(p.identity.clone(), (s.sid, s.routed, s.age, s.silent));
            }
            v.rooms.insert(
                p.identity.clone(),
                self.person(&p.identity).and_then(|x| x.room),
            );
            v.people.insert(p.identity, (p.role, away));
        }
        v
    }

    /// Carry out reconciling actions.
    pub fn apply(&self, actions: &[Action]) {
        for a in actions {
            let r = match a {
                Action::Add(id, role, room) => self.add_person_in(id, *role, room.as_deref()),
                Action::Subscribe(id, sid) => self.subscribe(id, sid),
                Action::Resubscribe(id, sid) => {
                    tracing::info!(participant = %id, sid, "subscribing again");
                    self.drop_subscription(id);
                    self.subscribe(id, sid)
                }
                Action::Away(id) => {
                    self.mark_away(id);
                    Ok(())
                }
                Action::Remove(id) => {
                    self.remove_person(id);
                    Ok(())
                }
            };
            if let Err(e) = r {
                tracing::error!(action = ?a, error = %format!("{e:#}"), "cannot reconcile the desk with the room");
            }
        }
    }
}

/// Forget the retries of anyone gone, anyone whose voice now arrives, and anyone muted (their
/// unmuting starts afresh).
fn note_retries(retries: &mut BTreeMap<String, u32>, seen: &[Seen], view: &DeskView) {
    let here: BTreeSet<&str> = seen
        .iter()
        .filter(|s| !s.muted)
        .map(|s| s.identity.as_str())
        .collect();
    retries.retain(|id, _| {
        here.contains(id.as_str()) && !matches!(view.subs.get(id), Some((_, true, _, None)))
    });
}

/// Note who is muted now, and forget anyone gone.
fn note_muted(last_muted: &mut BTreeMap<String, Instant>, seen: &[Seen], now: Instant) {
    let here: BTreeSet<&str> = seen.iter().map(|s| s.identity.as_str()).collect();
    last_muted.retain(|id, _| here.contains(id.as_str()));
    for s in seen.iter().filter(|s| s.muted) {
        last_muted.insert(s.identity.clone(), now);
    }
}

/// Count each resubscription made for a fault; one made for a new session starts afresh.
fn count_retries(retries: &mut BTreeMap<String, u32>, actions: &[Action], view: &DeskView) {
    for a in actions {
        if let Action::Resubscribe(id, sid) = a {
            match view.subs.get(id) {
                Some((old, ..)) if old == sid => *retries.entry(id.clone()).or_insert(0) += 1,
                _ => {
                    retries.remove(id);
                }
            }
        }
    }
}

/// Follow the room until `stop` is set: list it every `poll`, reconcile the desk.
pub fn watch(desk: Desk, api: RoomApi, stop: Arc<AtomicBool>) {
    let c = desk.config().room.clone();
    let grace = Duration::from_millis(c.leave_grace_ms);
    let timeout = Duration::from_millis(c.link_timeout_ms);
    let poll = Duration::from_millis(c.poll_ms);
    let mut failing_since: Option<Instant> = None;
    let mut was_empty = true;
    // Faulted resubscriptions in a row, per person, for the back-off (`retry_wait`).
    let mut retries: BTreeMap<String, u32> = BTreeMap::new();
    // When each person was last seen with their audio muted.
    let mut last_muted: BTreeMap<String, Instant> = BTreeMap::new();
    while !stop.load(Ordering::SeqCst) {
        // Each time the room empties, what the desk still holds: across shows it must come
        // back to the same (the staging run saw about two cores used with the room empty).
        let empty = desk.status().is_empty();
        if empty && !was_empty {
            let c = crate::census::census(desk.pipeline());
            tracing::info!(
                elements = c.elements,
                pads = c.pads,
                threads = crate::census::threads(),
                "the room is empty"
            );
        }
        was_empty = empty;
        match api.list() {
            Ok(seen) => {
                if failing_since.take().is_some() {
                    tracing::info!("LiveKit's server API answers again");
                }
                let mut view = desk.view();
                note_retries(&mut retries, &seen, &view);
                view.retries = retries.clone();
                note_muted(&mut last_muted, &seen, Instant::now());
                view.unmuted = last_muted
                    .iter()
                    .map(|(id, t)| (id.clone(), t.elapsed()))
                    .collect();
                let actions = reconcile(&seen, &view, grace, timeout);
                count_retries(&mut retries, &actions, &view);
                desk.apply(&actions);
                desk.note_names(seen.iter().map(|s| (s.identity.as_str(), s.name.as_str())));
            }
            Err(e) => {
                if failing_since.is_none() {
                    tracing::warn!(error = %format!("{e:#}"), "cannot list the room; the desk keeps mixing who it has");
                    failing_since = Some(Instant::now());
                }
            }
        }
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"{"participants": [
        {"sid": "PA_1", "identity": "hamish", "name": "Hamish Reid", "state": "ACTIVE",
         "tracks": [{"sid": "TR_1", "type": "AUDIO"}], "attributes": {"weavesignal.role": "presenter"}},
        {"sid": "PA_2", "identity": "gregor", "state": "JOINED", "tracks": [],
         "attributes": {"weavesignal.role": "guest"}},
        {"sid": "PA_3", "identity": "desk-mm-hamish", "state": "ACTIVE",
         "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "presenter"}},
        {"sid": "PA_4", "identity": "ear-hamish", "state": "ACTIVE", "tracks": []},
        {"sid": "PA_5", "identity": "mallory", "state": "ACTIVE",
         "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "admin"}},
        {"sid": "PA_6", "identity": "gone", "state": "DISCONNECTED",
         "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "guest"}},
        {"sid": "PA_7", "identity": "ear-gregor", "state": "ACTIVE",
         "tracks": [], "attributes": {"weavesignal.role": "listener"}}
    ]}"#;

    #[test]
    fn only_people_with_a_role_are_mixed() {
        let seen = parse(REPLY, "desk").unwrap();
        assert_eq!(
            seen,
            vec![
                Seen {
                    identity: "hamish".into(),
                    name: "Hamish Reid".into(),
                    sid: "PA_1".into(),
                    role: Role::Presenter,
                    has_audio: true,
                    muted: false,
                    room: None,
                },
                Seen {
                    identity: "gregor".into(),
                    name: String::new(),
                    sid: "PA_2".into(),
                    role: Role::Guest,
                    has_audio: false,
                    muted: false,
                    room: None,
                },
            ]
        );
        assert!(parse("{}", "desk").unwrap().is_empty());
        assert!(parse("not json", "desk").is_err());
    }

    fn seen(id: &str, sid: &str, audio: bool) -> Seen {
        Seen {
            identity: id.into(),
            name: String::new(),
            sid: sid.into(),
            role: Role::Guest,
            has_audio: audio,
            muted: false,
            room: None,
        }
    }

    const GRACE: Duration = Duration::from_secs(30);
    const TIMEOUT: Duration = Duration::from_secs(8);

    #[test]
    fn a_silent_voice_is_tried_again_less_and_less_often() {
        // Review 0025, D1: 8, 16, 32, then 64 s at most.
        assert_eq!(retry_wait(TIMEOUT, 0), TIMEOUT);
        assert_eq!(retry_wait(TIMEOUT, 1), TIMEOUT * 2);
        assert_eq!(retry_wait(TIMEOUT, 3), TIMEOUT * 8);
        assert_eq!(retry_wait(TIMEOUT, 40), TIMEOUT * 8);
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(20),
                Some(Duration::from_secs(12)),
            ),
        );
        let again = vec![Action::Resubscribe("g".into(), "PA_1".into())];
        assert_eq!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT),
            again
        );
        v.retries.insert("g".into(), 1);
        assert!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT).is_empty(),
            "the second try waits 16 s"
        );
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), false, Duration::from_secs(17), None),
        );
        assert_eq!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT),
            again
        );
    }

    #[test]
    fn retries_count_faults_and_start_afresh() {
        let mut v = DeskView::default();
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(20),
                Some(Duration::from_secs(12)),
            ),
        );
        let mut r = BTreeMap::new();
        count_retries(
            &mut r,
            &[Action::Resubscribe("g".into(), "PA_1".into())],
            &v,
        );
        count_retries(
            &mut r,
            &[Action::Resubscribe("g".into(), "PA_1".into())],
            &v,
        );
        assert_eq!(r.get("g"), Some(&2));
        // A new session is not a fault.
        count_retries(
            &mut r,
            &[Action::Resubscribe("g".into(), "PA_2".into())],
            &v,
        );
        assert_eq!(r.get("g"), None);
        // A voice that arrives clears the count; so does leaving.
        r.insert("g".into(), 2);
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), true, Duration::from_secs(20), None),
        );
        note_retries(&mut r, &[seen("g", "PA_1", true)], &v);
        assert_eq!(r.get("g"), None);
        r.insert("g".into(), 2);
        note_retries(&mut r, &[], &DeskView::default());
        assert!(r.is_empty());
    }

    #[test]
    fn a_newcomer_is_added_and_subscribed_once_they_publish() {
        let empty = DeskView::default();
        assert_eq!(
            reconcile(&[seen("g", "PA_1", false)], &empty, GRACE, TIMEOUT),
            vec![Action::Add("g".into(), Role::Guest, None)]
        );
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        assert_eq!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT),
            vec![Action::Subscribe("g".into(), "PA_1".into())]
        );
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), true, Duration::from_secs(1), None),
        );
        assert!(reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT).is_empty());
    }

    #[test]
    fn a_leaver_goes_away_then_after_the_grace_is_removed() {
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), true, Duration::from_secs(9), None),
        );
        assert_eq!(
            reconcile(&[], &v, GRACE, TIMEOUT),
            vec![Action::Away("g".into())]
        );
        v.subs.clear();
        v.people
            .insert("g".into(), (Role::Guest, Some(Duration::from_secs(5))));
        assert!(reconcile(&[], &v, GRACE, TIMEOUT).is_empty());
        v.people
            .insert("g".into(), (Role::Guest, Some(Duration::from_secs(31))));
        assert_eq!(
            reconcile(&[], &v, GRACE, TIMEOUT),
            vec![Action::Remove("g".into())]
        );
    }

    #[test]
    fn a_rejoin_within_the_grace_is_welcomed_back_and_subscribed_afresh() {
        let mut v = DeskView::default();
        v.people
            .insert("g".into(), (Role::Guest, Some(Duration::from_secs(5))));
        assert_eq!(
            reconcile(&[seen("g", "PA_2", true)], &v, GRACE, TIMEOUT),
            vec![
                Action::Add("g".into(), Role::Guest, None),
                Action::Subscribe("g".into(), "PA_2".into())
            ]
        );
        // A rejoin seen before the leave was: a new session under an old subscription.
        v.people.insert("g".into(), (Role::Guest, None));
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), true, Duration::from_secs(60), None),
        );
        assert_eq!(
            reconcile(&[seen("g", "PA_2", true)], &v, GRACE, TIMEOUT),
            vec![Action::Resubscribe("g".into(), "PA_2".into())]
        );
    }

    #[test]
    fn a_routed_voice_that_brings_nothing_is_made_again() {
        // A rejoin under the same identity whose new subscription routed a pad that carries
        // nothing (M3's relay run): heard nothing for longer than the link timeout.
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        let sub = |silent: u64| {
            (
                "PA_2".to_owned(),
                true,
                Duration::from_secs(60),
                Some(Duration::from_secs(silent)),
            )
        };
        v.subs.insert("g".into(), sub(1));
        assert!(reconcile(&[seen("g", "PA_2", true)], &v, GRACE, TIMEOUT).is_empty());
        v.subs.insert("g".into(), sub(9));
        assert_eq!(
            reconcile(&[seen("g", "PA_2", true)], &v, GRACE, TIMEOUT),
            vec![Action::Resubscribe("g".into(), "PA_2".into())]
        );
    }

    #[test]
    fn a_track_muted_in_the_room_is_read_from_the_list() {
        let reply = r#"{"participants": [
            {"sid": "PA_1", "identity": "morag", "state": "ACTIVE",
             "tracks": [{"sid": "TR_1", "type": "AUDIO", "muted": true}],
             "attributes": {"weavesignal.role": "guest"}},
            {"sid": "PA_2", "identity": "hamish", "state": "ACTIVE",
             "tracks": [{"sid": "TR_2", "type": "AUDIO", "muted": true},
                        {"sid": "TR_3", "type": "AUDIO"}],
             "attributes": {"weavesignal.role": "presenter"}},
            {"sid": "PA_3", "identity": "gregor", "state": "ACTIVE",
             "tracks": [{"sid": "TR_4", "type": "VIDEO", "muted": true}],
             "attributes": {"weavesignal.role": "guest"}}
        ]}"#;
        let seen = parse(reply, "desk").unwrap();
        let muted: Vec<(&str, bool)> = seen
            .iter()
            .map(|s| (s.identity.as_str(), s.muted))
            .collect();
        // Muted only when every audio track is: one open microphone still sends; a muted
        // camera is not a muted voice.
        assert_eq!(
            muted,
            vec![("morag", true), ("hamish", false), ("gregor", false)]
        );
    }

    #[test]
    fn a_voice_muted_in_the_room_is_not_a_fault() {
        // Issue 13: LiveKit forwards nothing of a muted track, so no packets is expected.
        let mut s = seen("g", "PA_1", true);
        s.muted = true;
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        // Routed, then muted: no packets for a minute.
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(90),
                Some(Duration::from_secs(60)),
            ),
        );
        assert!(reconcile(std::slice::from_ref(&s), &v, GRACE, TIMEOUT).is_empty());
        // Muted from the start: never routed.
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), false, Duration::from_secs(60), None),
        );
        assert!(reconcile(std::slice::from_ref(&s), &v, GRACE, TIMEOUT).is_empty());
        // A new session is still subscribed afresh, muted or not.
        let mut rejoined = seen("g", "PA_2", true);
        rejoined.muted = true;
        assert_eq!(
            reconcile(&[rejoined], &v, GRACE, TIMEOUT),
            vec![Action::Resubscribe("g".into(), "PA_2".into())]
        );
    }

    #[test]
    fn an_unmuted_voice_is_waited_for_afresh() {
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(90),
                Some(Duration::from_secs(60)),
            ),
        );
        // Unmuted 2 s ago: its packets have had 2 s, not 60, to come back.
        v.unmuted.insert("g".into(), Duration::from_secs(2));
        assert!(reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT).is_empty());
        // Unmuted 9 s ago and still nothing: a fault again.
        v.unmuted.insert("g".into(), Duration::from_secs(9));
        assert_eq!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT),
            vec![Action::Resubscribe("g".into(), "PA_1".into())]
        );
        // Packets that came since the unmuting count from the last of them.
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(90),
                Some(Duration::from_millis(20)),
            ),
        );
        assert!(reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT).is_empty());
    }

    #[test]
    fn muting_clears_the_back_off_and_is_remembered_while_they_stay() {
        let mut s = seen("g", "PA_1", true);
        s.muted = true;
        let mut r = BTreeMap::from([("g".to_owned(), 2)]);
        let mut v = DeskView::default();
        v.subs.insert(
            "g".into(),
            (
                "PA_1".into(),
                true,
                Duration::from_secs(90),
                Some(Duration::from_secs(60)),
            ),
        );
        note_retries(&mut r, std::slice::from_ref(&s), &v);
        assert!(r.is_empty(), "a muted voice's unmuting starts afresh");
        let t0 = Instant::now();
        let mut m = BTreeMap::new();
        note_muted(&mut m, std::slice::from_ref(&s), t0);
        assert_eq!(m.get("g"), Some(&t0));
        // Unmuted: the last time seen muted stays; gone: forgotten.
        let later = t0 + Duration::from_secs(1);
        note_muted(&mut m, &[seen("g", "PA_1", true)], later);
        assert_eq!(m.get("g"), Some(&t0));
        note_muted(&mut m, &[], later);
        assert!(m.is_empty());
    }

    #[test]
    fn a_subscription_that_never_links_is_made_again() {
        let mut v = DeskView::default();
        v.people.insert("g".into(), (Role::Guest, None));
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), false, Duration::from_secs(3), None),
        );
        assert!(reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT).is_empty());
        v.subs.insert(
            "g".into(),
            ("PA_1".into(), false, Duration::from_secs(9), None),
        );
        assert_eq!(
            reconcile(&[seen("g", "PA_1", true)], &v, GRACE, TIMEOUT),
            vec![Action::Resubscribe("g".into(), "PA_1".into())]
        );
    }

    #[test]
    fn booths_with_someone_in_them_are_listed_with_their_owner() {
        let reply = r#"{"rooms": [
            {"name": "studio", "numParticipants": 9},
            {"name": "studio-booth-c7f3", "numParticipants": 2},
            {"name": "studio-booth-empty", "numParticipants": 0},
            {"name": "other-booth-x", "numParticipants": 1}
        ]}"#;
        assert_eq!(
            parse_booths(reply, "studio").unwrap(),
            vec![("studio-booth-c7f3".to_owned(), "c7f3".to_owned())]
        );
        assert!(parse_booths("{}", "studio").unwrap().is_empty());
    }

    #[test]
    fn a_booth_mixes_only_its_own_caller() {
        // ADR-0193: a token for someone else's booth, or another role, mixes nobody.
        let reply = r#"{"participants": [
            {"sid": "PA_1", "identity": "c7f3", "state": "ACTIVE",
             "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "caller"}},
            {"sid": "PA_2", "identity": "intruder", "state": "ACTIVE",
             "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "caller"}},
            {"sid": "PA_3", "identity": "c7f3x", "state": "ACTIVE",
             "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "guest"}},
            {"sid": "PA_4", "identity": "desk-mm-c7f3", "state": "ACTIVE",
             "tracks": [{"type": "AUDIO"}], "attributes": {"weavesignal.role": "caller"}}
        ]}"#;
        let seen = parse_booth(reply, "desk", "studio-booth-c7f3", "c7f3").unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].identity, "c7f3");
        assert_eq!(seen[0].room.as_deref(), Some("studio-booth-c7f3"));
    }

    #[test]
    fn someone_seen_in_another_room_joins_afresh_there() {
        let mut s = seen("c7f3", "PA_1", false);
        s.role = Role::Caller;
        s.room = Some("studio-booth-c7f3".into());
        let mut v = DeskView::default();
        v.people.insert("c7f3".into(), (Role::Caller, None));
        v.rooms.insert("c7f3".into(), None);
        assert_eq!(
            reconcile(&[s.clone()], &v, GRACE, TIMEOUT),
            vec![Action::Add(
                "c7f3".into(),
                Role::Caller,
                Some("studio-booth-c7f3".into())
            )]
        );
        v.rooms
            .insert("c7f3".into(), Some("studio-booth-c7f3".into()));
        assert!(reconcile(&[s], &v, GRACE, TIMEOUT).is_empty());
    }
}
