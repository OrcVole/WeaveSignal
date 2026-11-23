// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The hub: the studio's state, fed by the desk link and the people's commands, published as
//! one view to every studio app that is watching (the event stream).

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::watch;
use weavesignal_proto::{Keys, Role};

use crate::carts::{CartWall, CartsLink};
use crate::desk::{DeskClient, DeskCommand, DeskError, DeskStatus, StationMode};
use crate::studio::{Studio, View};

/// Milliseconds since the Unix epoch: what the app's clocks and ages are measured from.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// LiveKit's server API, to take a dropped caller out of the room or their booth.
pub struct RoomAdmin {
    pub api_url: String,
    pub room: String,
    pub keys: Keys,
}

impl RoomAdmin {
    /// Remove a participant (`RoomService/RemoveParticipant`) from the studio room and from
    /// their booth, wherever they are: a caller is in their booth (ADR-0193), anyone else in
    /// the studio room. Absent from a room is not an error.
    pub async fn remove(&self, identity: &str) -> anyhow::Result<()> {
        self.remove_from(&self.room, identity).await?;
        self.remove_from(&weavesignal_proto::booth(&self.room, identity), identity)
            .await
    }

    /// Take both of a show key's monitor seats out of the key's monitor room (ADR-0280): a key
    /// that ends takes its monitor link's listeners with it, not only the right to a new token.
    /// Seats that are not there are not an error.
    pub async fn remove_monitor(&self, key_id: &str) -> anyhow::Result<()> {
        let room = weavesignal_proto::monitor_room(&self.room, key_id);
        let mut failed = None;
        for seat in weavesignal_proto::monitor_seats(key_id) {
            if let Err(e) = self.remove_from(&room, &seat).await {
                failed = Some(e);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// Remove a participant from a room of the station's that is not the studio room: the
    /// rehearsal room (1.3.0). Absent is not an error.
    pub async fn remove_in(&self, room: &str, identity: &str) -> anyhow::Result<()> {
        self.remove_from(room, identity).await
    }

    /// Who is in a room now (`RoomService/ListParticipants`), by identity: the rehearsal room's
    /// people when its desk is not answering (1.3.0). A room that does not exist has nobody.
    pub async fn list_in(&self, room: &str) -> anyhow::Result<Vec<String>> {
        let url = format!(
            "{}/twirp/livekit.RoomService/ListParticipants",
            self.api_url.trim_end_matches('/')
        );
        let token = self.keys.room_admin(room, 60);
        let body = serde_json::json!({ "room": room });
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(3)))
                .http_status_as_error(false)
                .build()
                .into();
            let mut res = agent
                .post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .send_json(body)?;
            let status = res.status().as_u16();
            let text = res.body_mut().read_to_string().unwrap_or_default();
            match status {
                200 => Ok(identities(&text)),
                404 => Ok(Vec::new()),
                s => anyhow::bail!("LiveKit answered {s}: {text}"),
            }
        })
        .await?
    }

    async fn remove_from(&self, room: &str, identity: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/twirp/livekit.RoomService/RemoveParticipant",
            self.api_url.trim_end_matches('/')
        );
        let token = self.keys.room_admin(room, 60);
        let body = serde_json::json!({ "room": room, "identity": identity });
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(3)))
                .http_status_as_error(false)
                .build()
                .into();
            let mut res = agent
                .post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .send_json(body)?;
            match res.status().as_u16() {
                200 | 404 => Ok(()),
                s => anyhow::bail!(
                    "LiveKit answered {s}: {}",
                    res.body_mut().read_to_string().unwrap_or_default()
                ),
            }
        })
        .await?
    }
}

/// The identities in a `ListParticipants` answer.
fn identities(reply: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(reply)
        .ok()
        .and_then(|v| {
            v.get("participants")?.as_array().map(|ps| {
                ps.iter()
                    .filter_map(|p| p.get("identity")?.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub(crate) struct Inner {
    studio: Studio,
    desk: Option<DeskStatus>,
    desk_error: Option<String>,
    pub(crate) carts: CartWall,
}

pub struct Hub {
    /// The station's name and takedown contact in force (ADR-0237): the setup page's, else the
    /// settings file's. The studio's view and every route that says either read it here.
    pub live: Arc<crate::setup::Live>,
    pub desk: DeskClient,
    pub room: Option<RoomAdmin>,
    /// Playout, the cart settings and the database, for the cart wall.
    pub carts: CartsLink,
    poll: Duration,
    inner: Mutex<Inner>,
    tx: watch::Sender<Arc<View>>,
    /// Playout's schedule, automation, logs and audience (M4), once started.
    pub onair: std::sync::OnceLock<Arc<crate::onair::OnAir>>,
    /// The station's one time zone (review 0025, S1), set at start-up from the database.
    pub zone: Arc<crate::zone::StationZone>,
    /// The record of who is on air (ADR-0153). Held across each write, so two polls, or a
    /// poll and a command, cannot write one after the other out of order.
    callers_on_air: tokio::sync::Mutex<OnAirRecord>,
    /// Hosts in their own studios (ADR-0280): the ingest's bearer and report, wrong keys.
    pub sources: Arc<crate::sources::Live>,
    /// The last Dump the desk took, from any route: the monitor link says DUMPED for a few
    /// seconds after it (design section 3.7). Kept in memory: it is only ever read seconds later.
    last_dump: Mutex<Option<DumpRecord>>,
    /// The telephone line (17f), once started; unset while it is off.
    pub phone: std::sync::OnceLock<Arc<crate::phone::line::Line>>,
    /// The rehearsal studio (1.3.0): its own desk link and session, never this hub's desk or
    /// studio. Unset while the station has none.
    pub rehearsal: std::sync::OnceLock<Arc<crate::rehearsal::Rehearsal>>,
    /// Who was on air, kept across a restart of the live desk, and who a restart took off and
    /// is still to be put back (1.3: guests and invitations arrive standing by).
    air_kept: Mutex<(crate::air_kept::AirKept, Vec<String>)>,
}

/// A Dump the desk took: when, and how much of the programme listeners lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpRecord {
    pub at_ms: i64,
    /// What the profanity delay held when it was dumped, as the last status said.
    pub lost_ms: u64,
}

/// What the hub last wrote of who is on air, and why it last failed to.
#[derive(Default)]
struct OnAirRecord {
    /// The callers on air as last written to the database; `None` before the first write and
    /// after a command wrote a stretch of its own, so the next poll writes the desk's state.
    written: Option<Vec<String>>,
    /// The last error writing it, so a database that stays down is logged once, not each poll.
    failing: Option<String>,
}

/// How long a write of who is on air may hold up the poll loop (or a command) before it is left
/// for the next poll: the pool waits 30 s for a connection by default.
const ON_AIR_WRITE: Duration = Duration::from_secs(2);

impl Hub {
    pub fn new(
        station: &str,
        studio: Studio,
        desk: DeskClient,
        room: Option<RoomAdmin>,
        poll: Duration,
        carts: CartsLink,
    ) -> Arc<Self> {
        let error = (!desk.configured()).then(|| DeskError::NotConfigured.to_string());
        let view = studio.view(station, None, error.clone(), now_ms());
        let (tx, _) = watch::channel(Arc::new(view));
        Arc::new(Hub {
            live: Arc::new(crate::setup::Live::new(station)),
            desk,
            room,
            carts,
            poll,
            inner: Mutex::new(Inner {
                studio,
                desk: None,
                desk_error: error.clone(),
                carts: CartWall::with_desk_error(error),
            }),
            tx,
            onair: std::sync::OnceLock::new(),
            zone: Arc::new(crate::zone::StationZone::default()),
            callers_on_air: tokio::sync::Mutex::new(OnAirRecord::default()),
            sources: Arc::new(crate::sources::Live::from_env()),
            last_dump: Mutex::new(None),
            phone: std::sync::OnceLock::new(),
            rehearsal: std::sync::OnceLock::new(),
            air_kept: Mutex::new(Default::default()),
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<View>> {
        self.tx.subscribe()
    }

    pub fn view(&self) -> Arc<View> {
        self.tx.borrow().clone()
    }

    /// Publish the studio's view again after something outside the studio changed it: the
    /// station's name, set on the setup page (ADR-0237).
    pub fn republish(&self) {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.publish(&inner);
    }

    fn publish(&self, inner: &Inner) {
        let mut v = inner.studio.view(
            &self.live.name(),
            inner.desk.as_ref(),
            inner.desk_error.clone(),
            now_ms(),
        );
        v.carts = inner.carts.view();
        self.tx.send_if_modified(|old| {
            // The clock alone is no change: the app runs its own between views.
            v.now_ms = old.now_ms;
            if **old == v {
                false
            } else {
                v.now_ms = now_ms();
                *old = Arc::new(v);
                true
            }
        });
    }

    /// Change anything in the hub's state and publish the result.
    pub(crate) fn with_inner<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().expect("hub lock");
        let r = f(&mut inner);
        self.publish(&inner);
        r
    }

    /// Publish the view now (after a change made through `with_inner`, which publishes too;
    /// this is for progress that changes nothing else).
    pub(crate) fn publish_now(&self) {
        let inner = self.inner.lock().expect("hub lock");
        self.publish(&inner);
    }

    /// Change the studio and publish the result; `f` gets the studio and the time.
    pub fn with_studio<R>(&self, f: impl FnOnce(&mut Studio, u64) -> R) -> R {
        let mut inner = self.inner.lock().expect("hub lock");
        let r = f(&mut inner.studio, now_ms());
        self.publish(&inner);
        r
    }

    /// Change the studio and publish only if `f` says something changed (a figure counted every
    /// few seconds should not wake every app that follows the view).
    pub fn with_studio_if(&self, f: impl FnOnce(&mut Studio) -> bool) {
        let mut inner = self.inner.lock().expect("hub lock");
        if f(&mut inner.studio) {
            self.publish(&inner);
        }
    }

    /// Read the hub's state without changing it (nothing is published).
    pub(crate) fn read_inner<R>(&self, f: impl FnOnce(&Inner) -> R) -> R {
        let inner = self.inner.lock().expect("hub lock");
        f(&inner)
    }

    /// Read the studio without changing it.
    pub fn read_studio<R>(&self, f: impl FnOnce(&Studio) -> R) -> R {
        let inner = self.inner.lock().expect("hub lock");
        f(&inner.studio)
    }

    /// The desk's last status, if it answered.
    pub fn desk_status(&self) -> Option<DeskStatus> {
        self.inner.lock().expect("hub lock").desk.clone()
    }

    /// The last Dump the desk took, if any since this process started.
    pub fn last_dump(&self) -> Option<DumpRecord> {
        *self.last_dump.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Ask the desk once and fold the answer in: the people, and the carts sounding (with
    /// any new play events read and recorded).
    pub async fn poll_once(&self) {
        let answer = self.desk.status().await;
        self.take_status(answer).await;
    }

    /// Fold in the desk's answer to a status request (what `poll_once` does with it).
    pub async fn take_status(&self, answer: Result<DeskStatus, DeskError>) {
        let carts = answer.as_ref().ok().map(|st| st.carts.clone());
        {
            let mut kept = self.air_kept.lock().unwrap_or_else(|p| p.into_inner());
            match &answer {
                Ok(st) => {
                    let owed = kept.0.take(st, now_ms());
                    kept.1.extend(owed);
                }
                Err(_) => kept.0.lost(),
            }
        }
        self.with_inner(|inner| match answer {
            Ok(st) => {
                if inner.desk_error.take().is_some() {
                    tracing::info!("the desk answers");
                }
                inner.studio.sync(&st, now_ms());
                inner.desk = Some(st);
            }
            Err(e) => {
                let text = e.to_string();
                if inner.desk_error.as_deref() != Some(&text) {
                    tracing::warn!(error = %text, "the desk is not answering");
                }
                inner.desk = None;
                inner.desk_error = Some(text);
            }
        });
        self.record_callers_on_air().await;
        self.run_phone().await;
        match carts {
            Some(c) => {
                if self.take_carts(c.as_ref()) {
                    self.read_plays().await;
                }
            }
            None => {
                let error = self
                    .read_inner(|i| i.desk_error.clone())
                    .unwrap_or_default();
                self.carts_lost(&error);
            }
        }
    }

    /// The phone line's gates (17f): the desk's phone status through each call's gate, and what
    /// the gates say done. A consent that cannot be kept is not given: the call is hung up, as a
    /// browser caller gets no token. Nothing logged here carries a number or a ban key.
    async fn run_phone(&self) {
        use crate::phone::line::{Line, Todo, refusal_words};
        let Some(line) = self.phone.get() else {
            return;
        };
        let Some(phone) = self.read_inner(|i| i.desk.as_ref().and_then(|d| d.phone.clone())) else {
            return;
        };
        let now = now_ms();
        let ctx = line.context();
        let todos = self.read_studio(|s| {
            line.step(
                &phone,
                &Line::moment(&ctx, s.phone_bans(), s.phone_in_queue()),
                now,
            )
        });
        for t in todos {
            match t {
                Todo::Words {
                    person,
                    file,
                    version,
                    gain_mb,
                } => {
                    let gain_db = gain_mb as f32 / 100.0;
                    if let Err(e) = self
                        .desk
                        .phone_words(&person, &file, &version, gain_db)
                        .await
                    {
                        tracing::warn!(error = %e, %person, "the desk cannot play the phone words");
                    }
                }
                Todo::HangUp { person, why } => {
                    match why {
                        crate::phone::gate::CallEnded::Refused(r) => {
                            tracing::info!(%person, why = refusal_words(r), "phone call refused")
                        }
                        other => {
                            tracing::info!(%person, why = ?other, "phone call ended at the gate")
                        }
                    }
                    if let Err(e) = self.desk.phone_hang_up(&person).await {
                        tracing::warn!(error = %e, %person, "the desk cannot hang up");
                    }
                }
                Todo::Admit { call, line: pl } => {
                    let person = call.identity.clone();
                    if let Some((pool, station)) = &self.carts.db {
                        let c = &call.consent;
                        let record = weavesignal_model::ConsentRecord {
                            identity: call.identity.clone(),
                            name: String::new(),
                            topic: String::new(),
                            given_by: match c.by {
                                crate::studio::ConsentBy::Caller => "caller",
                                crate::studio::ConsentBy::Station => "station",
                                crate::studio::ConsentBy::NotAsked => "not_asked",
                            },
                            words: c.text.clone(),
                            words_version: c.version.clone(),
                            accepted_at_ms: c.at_ms,
                            show_id: None,
                            presenter: String::new(),
                            contact: String::new(),
                        };
                        let kept = weavesignal_model::phone::record_phone_consent(
                            pool,
                            *station,
                            &record,
                            &weavesignal_model::phone::PhoneLine {
                                ban_key: pl.ban_key.clone(),
                                number_tail: pl.tail.clone(),
                            },
                        )
                        .await;
                        if let Err(e) = kept {
                            tracing::error!(error = %e, %person, "cannot keep a phone caller's consent: hanging up");
                            let _ = self.desk.phone_hang_up(&person).await;
                            continue;
                        }
                    }
                    tracing::info!(
                        target: "weavesignal::consent",
                        identity = %person,
                        by = ?call.consent.by,
                        version = %call.consent.version,
                        line = "phone",
                        "phone caller through the gate"
                    );
                    self.with_studio(|s, _| s.phone_call(call, pl));
                    if let Err(e) = self.desk.phone_admit(&person).await {
                        tracing::warn!(error = %e, %person, "the desk cannot let the phone caller in");
                    }
                }
            }
        }
    }

    /// Write who is on air to the database when it has changed (ADR-0153). The desk's status
    /// is the one complete record of it, whoever flipped the switch (a command from the studio,
    /// which also writes a stretch of its own before it reaches the desk, or a rejoin within the
    /// desk's grace). A desk that does not answer changes nothing: the stretches open stay open
    /// until it answers again or the call ends, erring on the side of keeping a consent with a
    /// recording that may carry the caller. A write that fails or takes too long is left for
    /// the next poll, as the record still differs.
    async fn record_callers_on_air(&self) {
        let Some((pool, station)) = &self.carts.db else {
            return;
        };
        let mut rec = self.callers_on_air.lock().await;
        let Some(now) = self.read_inner(|i| i.desk.as_ref().map(callers_on_air)) else {
            return;
        };
        if rec.written.as_ref() == Some(&now) {
            return;
        }
        let lag_ms = self.poll.as_millis() as u64;
        let write = weavesignal_model::retention::sync_on_air(pool, *station, &now, lag_ms);
        let error = match tokio::time::timeout(ON_AIR_WRITE, write).await {
            Ok(Ok(done)) => {
                for id in &done.no_consent {
                    tracing::warn!(
                        target: "weavesignal::retention",
                        caller = %id,
                        "a caller is on air with no consent on file for a call still going; their time on air is not recorded"
                    );
                }
                if rec.failing.take().is_some() {
                    tracing::info!(target: "weavesignal::retention", "who is on air is recorded again");
                }
                rec.written = Some(now);
                return;
            }
            Ok(Err(e)) => e.to_string(),
            Err(_) => format!("the database took more than {} s", ON_AIR_WRITE.as_secs()),
        };
        if rec.failing.as_deref() != Some(&error) {
            tracing::warn!(
                target: "weavesignal::retention",
                error = %error,
                "cannot record who is on air; a recording may not keep the consent of a caller it carries (trying again at each poll)"
            );
            rec.failing = Some(error);
        }
    }

    /// A caller is about to be put on air through the control plane: write a stretch for them
    /// first (ADR-0153), so the switch is on record however soon they come off and whatever
    /// the polls see. The next poll writes the desk's state again and ends it if they are off.
    async fn open_on_air(&self, identity: &str) {
        let Some((pool, station)) = &self.carts.db else {
            return;
        };
        let caller = self.desk_status().is_some_and(|d| {
            d.people
                .iter()
                .any(|p| p.identity == identity && p.role == Role::Caller)
        });
        if !caller {
            return;
        }
        let mut rec = self.callers_on_air.lock().await;
        let write = weavesignal_model::retention::open_on_air(pool, *station, identity);
        match tokio::time::timeout(ON_AIR_WRITE, write).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!(
                target: "weavesignal::retention",
                caller = %identity, error = %e,
                "cannot record a caller going on air; the next poll records it if it sees them"
            ),
            Err(_) => tracing::warn!(
                target: "weavesignal::retention",
                caller = %identity,
                "recording a caller going on air took more than {} s; the next poll records it if it sees them",
                ON_AIR_WRITE.as_secs()
            ),
        }
        rec.written = None;
    }

    /// Follow the desk until the process ends.
    pub fn spawn_poller(self: &Arc<Self>) {
        if !self.desk.configured() {
            tracing::warn!(
                "no desk socket configured (`desk.socket`); the studio app shows the desk as not configured"
            );
            return;
        }
        let hub = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(hub.poll);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                hub.poll_once().await;
                hub.put_back_air().await;
            }
        });
    }

    /// Put back on air the people a restart of the live desk took off (1.3), once each, as it
    /// finds them: see [`crate::air_kept`]. At once, with no fade: they were on air a moment ago.
    pub async fn put_back_air(&self) {
        let owed = std::mem::take(&mut self.air_kept.lock().unwrap_or_else(|p| p.into_inner()).1);
        for identity in owed {
            let cmd = DeskCommand::OnAir {
                identity: identity.clone(),
                on: true,
                fade_ms: None,
            };
            match self.command(&cmd).await {
                Ok(()) => tracing::info!(%identity, "put back on air after the desk started again"),
                Err(e) => {
                    tracing::warn!(%identity, error = %e, "cannot put them back on air after the desk started again")
                }
            }
        }
    }

    /// The station's mode (ADR-0045) from the playout settings: into the studio view (for
    /// ADR-0183's AUTO lamp) and to the desk (for the video's sound, ADR-0172).
    pub async fn set_mode(&self, mode: StationMode) {
        self.with_studio_if(|s| s.set_station_mode(mode));
        self.sync_desk_mode().await;
    }

    /// Tell the desk the station's mode when its last status says another. Called every few
    /// seconds (with the settings), so a desk that started again (live, its default) is told
    /// again, and a control plane that started again tells it only once it has read the setting.
    /// A desk too old to report a mode is not sent one.
    pub async fn sync_desk_mode(&self) {
        let (want, has) = self.read_inner(|i| {
            (
                i.studio.station_mode(),
                i.desk.as_ref().and_then(|d| d.mode),
            )
        });
        let (Some(want), Some(has)) = (want, has) else {
            return;
        };
        if want == has {
            return;
        }
        tracing::info!(from = ?has, to = ?want, "telling the desk the station's mode");
        if let Err(e) = self.command(&DeskCommand::Mode { mode: want }).await {
            tracing::warn!(error = %e, "the desk did not take the station's mode; trying again with the settings");
        }
    }

    /// Send the desk a command, then read it back at once so the app sees the change.
    pub async fn command(&self, cmd: &DeskCommand) -> Result<(), DeskError> {
        if let DeskCommand::OnAir {
            identity, on: true, ..
        } = cmd
        {
            self.open_on_air(identity).await;
        }
        // What a Dump costs listeners is what the delay held before it; read first.
        let held_ms = matches!(cmd, DeskCommand::Dump).then(|| {
            self.desk_status()
                .and_then(|d| d.delay)
                .map_or(0, |d| u64::from(d.delay_ms))
        });
        self.desk.send(cmd).await?;
        if let Some(lost_ms) = held_ms {
            *self.last_dump.lock().unwrap_or_else(|p| p.into_inner()) = Some(DumpRecord {
                at_ms: now_ms() as i64,
                lost_ms,
            });
        }
        self.poll_once().await;
        Ok(())
    }
}

/// The callers on air in the desk's status, in order.
fn callers_on_air(desk: &DeskStatus) -> Vec<String> {
    let mut ids: Vec<String> = desk
        .people
        .iter()
        .filter(|p| p.role == Role::Caller && p.on_air)
        .map(|p| p.identity.clone())
        .collect();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with_fake_desk() -> (
        Arc<Hub>,
        Arc<Mutex<crate::desk::fake::FakeDesk>>,
        std::path::PathBuf,
    ) {
        let dir = crate::desk::tests::tempdir();
        let fake = crate::desk::fake::spawn(&dir.join("desk.sock"));
        let hub = Hub::new(
            "Kettle FM",
            Studio::new("The Morning Kettle", vec![]),
            DeskClient::new(Some(&dir.join("desk.sock"))),
            None,
            Duration::from_millis(250),
            CartsLink::new(crate::config::CartsConfig::default(), None),
        );
        (hub, fake, dir)
    }

    #[tokio::test]
    async fn a_live_desk_that_starts_again_gets_back_on_air_the_people_it_had_on() {
        let (hub, fake, dir) = hub_with_fake_desk();
        let p = |id: &str, role: &str, on: bool| {
            serde_json::json!({"identity": id, "name": "", "role": role, "on_air": on,
                "talk": false, "present": true, "linked": true})
        };
        {
            let mut f = fake.lock().unwrap();
            f.started_ms = Some(1);
            f.people = vec![
                p("eilidh", "presenter", true),
                p("gregor", "guest", true),
                p("kirsty", "guest", false),
            ];
        }
        hub.poll_once().await;
        hub.put_back_air().await;
        // The same desk: nothing sent.
        assert!(
            fake.lock()
                .unwrap()
                .lines
                .iter()
                .all(|l| l["op"] == "status")
        );
        // It starts again, and finds everyone standing by (guests and invitations, 1.3).
        {
            let mut f = fake.lock().unwrap();
            f.started_ms = Some(2);
            f.people = vec![
                p("eilidh", "presenter", false),
                p("gregor", "guest", false),
                p("kirsty", "guest", false),
            ];
        }
        hub.poll_once().await;
        hub.put_back_air().await;
        let on: Vec<String> = fake
            .lock()
            .unwrap()
            .lines
            .iter()
            .filter(|l| l["op"] == "on_air" && l["on"] == true)
            .map(|l| l["identity"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(on, ["eilidh", "gregor"], "Kirsty was off air before");
        // Once: no more at the next look.
        hub.poll_once().await;
        hub.put_back_air().await;
        let count = fake
            .lock()
            .unwrap()
            .lines
            .iter()
            .filter(|l| l["op"] == "on_air")
            .count();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn modes_sent(fake: &Mutex<crate::desk::fake::FakeDesk>) -> Vec<String> {
        let f = fake.lock().unwrap();
        f.lines
            .iter()
            .filter(|l| l["op"] == "mode")
            .map(|l| l["mode"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_phone_call_hears_the_words_then_a_key_puts_it_in_the_queue() {
        use crate::phone::config::PhoneConfig;
        use crate::phone::line::{Context, Line};
        use crate::phone::words::{ALBA_FILE, PhoneWords};
        use weavesignal_model::caller_id::{BanKeys, livekit_identity};
        let (hub, fake, dir) = hub_with_fake_desk();
        let line = Line::new(
            PhoneConfig {
                enabled: true,
                numbers: vec!["+441314960000".into()],
                allowed_addresses: vec!["127.0.0.1/32".into()],
                public_ip: "127.0.0.1".into(),
                ..PhoneConfig::default()
            },
            Some(BanKeys::new([9; 32])),
        );
        line.set_context(Context {
            call_in_open: true,
            consent_today: "Live on Kettle FM.".into(),
            station_recording: None,
            default_present: true,
            ..Context::default()
        });
        assert!(hub.phone.set(Arc::new(line)).is_ok());
        let version = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba).version();
        let booth = |admitted: bool| {
            serde_json::json!({"person": "phone-Xk3pQ9mZr2Ta", "tail": "0123",
                "sip_identity": livekit_identity("+447700900123"), "answered": true,
                "admitted": admitted, "words_playing": null, "words_done": 0})
        };
        fake.lock().unwrap().phone =
            Some(serde_json::json!({"booths": [booth(false)], "keys": []}));
        hub.poll_once().await;
        {
            let f = fake.lock().unwrap();
            assert_eq!(f.ops(), ["phone_words"]);
            assert_eq!(f.lines.last().unwrap()["file"], ALBA_FILE);
        }
        fake.lock().unwrap().phone = Some(serde_json::json!({"booths": [booth(false)],
            "keys": [{"seq": 1, "person": "phone-Xk3pQ9mZr2Ta", "code": 11,
                "words_version": version}]}));
        hub.poll_once().await;
        assert_eq!(fake.lock().unwrap().ops(), ["phone_words", "phone_admit"]);
        // The desk now lists them as a caller: they join the queue showing four digits.
        {
            let mut f = fake.lock().unwrap();
            f.phone = Some(serde_json::json!({"booths": [booth(true)], "keys": []}));
            f.people = vec![serde_json::json!({"identity": "phone-Xk3pQ9mZr2Ta",
                "name": "Phone 0123", "role": "caller", "on_air": false, "talk": false,
                "present": true, "linked": true})];
        }
        hub.poll_once().await;
        let v = hub.view();
        assert_eq!(v.callers.len(), 1);
        assert_eq!(v.callers[0].name, "");
        assert_eq!(v.callers[0].phone.as_ref().unwrap().number, "···0123");
        assert!(v.callers[0].consent.is_some());
        assert_eq!(fake.lock().unwrap().ops(), ["phone_words", "phone_admit"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_desk_is_told_the_stations_mode_once_and_again_after_it_restarts() {
        let (hub, fake, dir) = hub_with_fake_desk();
        fake.lock().unwrap().mode = Some("live".into());
        hub.poll_once().await;
        // Not read yet: the desk keeps what it has (a control plane that just started).
        hub.sync_desk_mode().await;
        assert!(modes_sent(&fake).is_empty());
        assert_eq!(hub.view().mode, None);

        hub.set_mode(StationMode::Auto).await;
        assert_eq!(modes_sent(&fake), ["auto"]);
        assert_eq!(hub.view().mode, Some(StationMode::Auto));
        assert_eq!(hub.desk_status().unwrap().mode, Some(StationMode::Auto));
        // Idempotent: the settings read again every few seconds send nothing more.
        hub.set_mode(StationMode::Auto).await;
        hub.poll_once().await;
        hub.sync_desk_mode().await;
        assert_eq!(modes_sent(&fake), ["auto"]);

        // The desk starts again, live: told again at the next look.
        fake.lock().unwrap().mode = Some("live".into());
        hub.poll_once().await;
        hub.sync_desk_mode().await;
        assert_eq!(modes_sent(&fake), ["auto", "auto"]);

        // Back to live from the settings.
        hub.set_mode(StationMode::Live).await;
        assert_eq!(modes_sent(&fake), ["auto", "auto", "live"]);
        assert_eq!(hub.view().mode, Some(StationMode::Live));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_desk_too_old_to_report_a_mode_is_not_sent_one() {
        let (hub, fake, dir) = hub_with_fake_desk();
        hub.poll_once().await;
        hub.set_mode(StationMode::Auto).await;
        assert!(modes_sent(&fake).is_empty());
        assert_eq!(
            hub.view().mode,
            Some(StationMode::Auto),
            "the view has it all the same"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
