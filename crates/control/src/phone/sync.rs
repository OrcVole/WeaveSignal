// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line's trunk and dispatch rule kept in LiveKit (1.2.1; 1.2.0 built both bodies in
//! [`super::trunk`] and never sent them, so LiveKit's SIP service answered every call 404).
//!
//! While the line is on, a task looks at LiveKit every [`EVERY`] (every [`RETRY`] until the first
//! success, since LiveKit may start after the control plane): when this station's own trunk and
//! rule are the ones this process made, it does nothing; otherwise it deletes its own (by
//! [`super::trunk::OWNER_NAME`]) and makes both again from the settings. So a restart of the
//! control plane makes them afresh from the settings file, and a restart of Valkey (which keeps
//! nothing on disk) has them back within a minute. Trunks and rules another tool made are left
//! alone. With the line off it deletes its own once, quietly: a station without the phone line
//! has no SIP API at all (Cloudron, `PHONE=false`), and that is not an error.
//!
//! Nothing here logs a request or an answer: the trunk carries the digest password.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use super::config::{PhoneConfig, TRUNK_PASSWORD_ENV};
use super::trunk::{dispatch_rule, inbound_trunk, own_ids};

/// How often a made trunk and rule are looked at again.
pub const EVERY: Duration = Duration::from_secs(30);
/// How soon a failed look is tried again.
pub const RETRY: Duration = Duration::from_secs(5);

/// One call of LiveKit's SIP API (`livekit.SIP/<method>`, Twirp JSON): the answer's body, or
/// what went wrong in words (never the request's body).
pub trait SipApi: Send + Sync {
    fn call(&self, method: &str, body: Value) -> Result<Value, String>;
}

/// The ids of the trunk and rule this process made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub trunk_id: String,
    pub rule_id: String,
}

/// What a look at LiveKit did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The trunk and rule were already the ones made.
    Unchanged,
    /// Both made (again), after deleting this station's old ones.
    Made(Made),
    /// The line is off: this many of this station's trunks and rules deleted.
    Removed(usize),
}

/// What the station wants in LiveKit: the trunk's body and the studio room the rule names.
pub struct Wanted {
    pub trunk: Value,
    pub studio_room: String,
}

impl Wanted {
    /// From the settings, with the digest password from the environment. `Err` when a digest
    /// user is set and no password is: a trunk made without one would refuse every call.
    pub fn from_config(
        c: &PhoneConfig,
        studio_room: &str,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let password = password.filter(|p| !p.is_empty());
        if !c.auth_username.trim().is_empty() && password.is_none() {
            return Err(format!(
                "`phone.auth_username` is set but {TRUNK_PASSWORD_ENV} is empty: set the provider's \
                 password (PHONE_TRUNK_PASSWORD in the settings file) or clear the user"
            ));
        }
        Ok(Self {
            trunk: inbound_trunk(c, password),
            studio_room: studio_room.to_owned(),
        })
    }
}

fn id_of(answer: &Value, field: &str) -> Result<String, String> {
    answer[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("LiveKit's answer has no `{field}`"))
}

/// Delete this station's own rules, then its own trunks; how many.
fn remove_own(api: &dyn SipApi, trunks: &Value, rules: &Value) -> Result<usize, String> {
    let rule_ids = own_ids(rules, "sip_dispatch_rule_id");
    let trunk_ids = own_ids(trunks, "sip_trunk_id");
    for id in &rule_ids {
        api.call(
            "DeleteSIPDispatchRule",
            json!({ "sip_dispatch_rule_id": id }),
        )?;
    }
    for id in &trunk_ids {
        api.call("DeleteSIPTrunk", json!({ "sip_trunk_id": id }))?;
    }
    Ok(rule_ids.len() + trunk_ids.len())
}

/// One look at LiveKit: with `wanted`, make sure this station's trunk and rule are there (the
/// ones in `made`, else made again); without it, delete them.
pub fn reconcile(
    api: &dyn SipApi,
    wanted: Option<&Wanted>,
    made: Option<&Made>,
) -> Result<Outcome, String> {
    let trunks = api.call("ListSIPInboundTrunk", json!({}))?;
    let rules = api.call("ListSIPDispatchRule", json!({}))?;
    let Some(w) = wanted else {
        return remove_own(api, &trunks, &rules).map(Outcome::Removed);
    };
    if let Some(m) = made
        && own_ids(&trunks, "sip_trunk_id") == [m.trunk_id.as_str()]
        && own_ids(&rules, "sip_dispatch_rule_id") == [m.rule_id.as_str()]
    {
        return Ok(Outcome::Unchanged);
    }
    remove_own(api, &trunks, &rules)?;
    let trunk_id = id_of(
        &api.call("CreateSIPInboundTrunk", w.trunk.clone())?,
        "sip_trunk_id",
    )?;
    let rule_id = id_of(
        &api.call(
            "CreateSIPDispatchRule",
            dispatch_rule(&w.studio_room, &trunk_id),
        )?,
        "sip_dispatch_rule_id",
    )?;
    Ok(Outcome::Made(Made { trunk_id, rule_id }))
}

/// LiveKit's SIP API over HTTP, with a short-lived `sip.admin` token for each call.
pub struct Twirp {
    pub api_url: String,
    pub keys: weavesignal_proto::Keys,
}

impl SipApi for Twirp {
    fn call(&self, method: &str, body: Value) -> Result<Value, String> {
        let url = format!(
            "{}/twirp/livekit.SIP/{method}",
            self.api_url.trim_end_matches('/')
        );
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut res = agent
            .post(&url)
            .header(
                "Authorization",
                &format!("Bearer {}", self.keys.sip_admin(60)),
            )
            .send_json(body)
            .map_err(|e| format!("{method}: LiveKit did not answer ({e})"))?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        if status != 200 {
            // A Twirp error's `msg` only: never the body as sent.
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["msg"].as_str().map(str::to_owned))
                .unwrap_or_default();
            let msg: String = msg.chars().take(300).collect();
            return Err(format!("{method}: LiveKit answered {status} {msg}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("{method}: LiveKit's answer: {e}"))
    }
}

/// Where the trunk stands, as `GET /api/v1/phone` shows it.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct TrunkStatus {
    /// `making` (not made yet: LiveKit not reached, or the first try under way), `made` (the
    /// station's trunk and dispatch rule are in LiveKit, so calls reach the line) or `failed`
    /// (tried and refused; `message` says why, and it is tried again).
    pub state: String,
    /// Why, when it is `failed`.
    pub message: Option<String>,
    /// When it was last made or found as made (ms since the epoch).
    pub checked_ms: Option<u64>,
}

/// The trunk's status, shared between the task and the route.
#[derive(Default)]
pub struct TrunkCell(Mutex<TrunkStatus>);

impl TrunkCell {
    pub fn new() -> Self {
        Self(Mutex::new(TrunkStatus {
            state: "making".into(),
            ..TrunkStatus::default()
        }))
    }

    pub fn get(&self) -> TrunkStatus {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, s: TrunkStatus) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = s;
    }
}

/// Keep the trunk and rule while the line is on (`wanted`), or delete them once when it is off.
pub fn spawn(
    api: Arc<dyn SipApi>,
    wanted: Option<Result<Wanted, String>>,
    cell: Option<Arc<TrunkCell>>,
) {
    tokio::spawn(async move {
        let wanted = match wanted {
            None => None,
            Some(Ok(w)) => Some(Arc::new(w)),
            Some(Err(why)) => {
                tracing::error!(%why, "the phone line's trunk cannot be made: no call will reach the line");
                if let Some(c) = &cell {
                    c.set(TrunkStatus {
                        state: "failed".into(),
                        message: Some(why),
                        checked_ms: None,
                    });
                }
                return;
            }
        };
        let mut made: Option<Made> = None;
        // Failed looks in a row: the wait grows from 5 s to a minute.
        let mut failures = 0u32;
        // With the line off: a few tries while LiveKit starts, then give up without a word.
        let mut off_tries = 0u8;
        loop {
            let (api2, w2, m2) = (api.clone(), wanted.clone(), made.clone());
            let r = tokio::task::spawn_blocking(move || {
                reconcile(api2.as_ref(), w2.as_deref(), m2.as_ref())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            if wanted.is_none() {
                match r {
                    Ok(Outcome::Removed(n)) if n > 0 => {
                        tracing::info!(
                            removed = n,
                            "the phone line is off: its trunk and dispatch rule removed from LiveKit"
                        );
                        return;
                    }
                    Ok(_) => return,
                    Err(e) => {
                        off_tries += 1;
                        tracing::debug!(error = %e, "the phone line is off; LiveKit's SIP API not reached");
                        if off_tries >= 6 {
                            return;
                        }
                        tokio::time::sleep(RETRY * 2).await;
                        continue;
                    }
                }
            }
            if r.is_ok() {
                failures = 0;
            }
            let pause = match r {
                Ok(Outcome::Unchanged) => {
                    if let Some(c) = &cell {
                        c.set(TrunkStatus {
                            state: "made".into(),
                            message: None,
                            checked_ms: Some(crate::hub::now_ms()),
                        });
                    }
                    EVERY
                }
                Ok(Outcome::Made(m)) => {
                    tracing::info!(trunk = %m.trunk_id, rule = %m.rule_id, "the phone line's trunk and dispatch rule made in LiveKit");
                    made = Some(m);
                    if let Some(c) = &cell {
                        c.set(TrunkStatus {
                            state: "made".into(),
                            message: None,
                            checked_ms: Some(crate::hub::now_ms()),
                        });
                    }
                    EVERY
                }
                Ok(Outcome::Removed(_)) => EVERY,
                Err(e) => {
                    // LiveKit starting after the control plane is normal: quiet, and still
                    // `making`, until it has answered once. A refusal is `failed` at once.
                    let unreached = e.contains("did not answer");
                    let starting =
                        unreached && cell.as_ref().is_none_or(|c| c.get().state == "making");
                    if starting {
                        tracing::info!(error = %e, "the phone line's trunk: LiveKit's SIP API not reached yet");
                    } else {
                        tracing::warn!(error = %e, "the phone line's trunk cannot be made or checked; trying again");
                    }
                    if let Some(c) = &cell {
                        c.set(TrunkStatus {
                            state: if starting { "making" } else { "failed" }.into(),
                            message: Some(e),
                            checked_ms: None,
                        });
                    }
                    // LiveKit not answering says nothing of what it holds: what was made is
                    // looked at again, not made afresh. A refusal makes both again.
                    if !unreached {
                        made = None;
                    }
                    failures = failures.saturating_add(1);
                    (RETRY * 2u32.saturating_pow(failures - 1)).min(Duration::from_secs(60))
                }
            };
            tokio::time::sleep(pause).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::trunk::OWNER_NAME;
    use super::*;

    /// LiveKit's SIP store in memory, with every call written down.
    #[derive(Default)]
    struct Fake {
        trunks: Mutex<Vec<Value>>,
        rules: Mutex<Vec<Value>>,
        calls: Mutex<Vec<String>>,
        next: Mutex<u32>,
        down: Mutex<bool>,
    }

    impl Fake {
        fn writes(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| !c.starts_with("List"))
                .cloned()
                .collect()
        }
        fn id(&self, p: &str) -> String {
            let mut n = self.next.lock().unwrap();
            *n += 1;
            format!("{p}_{n}")
        }
    }

    impl SipApi for Fake {
        fn call(&self, method: &str, body: Value) -> Result<Value, String> {
            if *self.down.lock().unwrap() {
                return Err(format!("{method}: LiveKit did not answer (refused)"));
            }
            self.calls.lock().unwrap().push(method.to_owned());
            match method {
                "ListSIPInboundTrunk" => Ok(json!({ "items": *self.trunks.lock().unwrap() })),
                "ListSIPDispatchRule" => Ok(json!({ "items": *self.rules.lock().unwrap() })),
                "CreateSIPInboundTrunk" => {
                    let mut t = body["trunk"].clone();
                    t["sip_trunk_id"] = json!(self.id("ST"));
                    self.trunks.lock().unwrap().push(t.clone());
                    Ok(t)
                }
                "CreateSIPDispatchRule" => {
                    let mut r = body["dispatch_rule"].clone();
                    r["sip_dispatch_rule_id"] = json!(self.id("SDR"));
                    self.rules.lock().unwrap().push(r.clone());
                    Ok(r)
                }
                "DeleteSIPTrunk" => {
                    let id = body["sip_trunk_id"].clone();
                    self.trunks
                        .lock()
                        .unwrap()
                        .retain(|t| t["sip_trunk_id"] != id);
                    Ok(json!({}))
                }
                "DeleteSIPDispatchRule" => {
                    let id = body["sip_dispatch_rule_id"].clone();
                    self.rules
                        .lock()
                        .unwrap()
                        .retain(|r| r["sip_dispatch_rule_id"] != id);
                    Ok(json!({}))
                }
                other => Err(format!("{other}: not a method")),
            }
        }
    }

    fn cfg() -> PhoneConfig {
        PhoneConfig {
            enabled: true,
            numbers: vec!["+441314960000".into()],
            allowed_addresses: vec!["127.0.0.1/32".into()],
            public_ip: "127.0.0.1".into(),
            ..PhoneConfig::default()
        }
    }

    fn wanted() -> Wanted {
        Wanted::from_config(&cfg(), "studio", None).unwrap()
    }

    #[test]
    fn an_empty_livekit_gets_the_trunk_and_a_rule_that_names_it() {
        let f = Fake::default();
        let Outcome::Made(m) = reconcile(&f, Some(&wanted()), None).unwrap() else {
            panic!("not made")
        };
        assert_eq!(
            f.writes(),
            ["CreateSIPInboundTrunk", "CreateSIPDispatchRule"]
        );
        let rules = f.rules.lock().unwrap();
        assert_eq!(rules[0]["trunk_ids"], json!([m.trunk_id]));
        assert_eq!(rules[0]["name"], OWNER_NAME);
        assert_eq!(
            rules[0]["rule"]["dispatch_rule_individual"]["room_prefix"],
            "studio-phone"
        );
        assert_eq!(
            f.trunks.lock().unwrap()[0]["numbers"],
            json!(["+441314960000"])
        );
    }

    #[test]
    fn made_and_still_there_writes_nothing() {
        let f = Fake::default();
        let Outcome::Made(m) = reconcile(&f, Some(&wanted()), None).unwrap() else {
            panic!()
        };
        f.calls.lock().unwrap().clear();
        assert_eq!(
            reconcile(&f, Some(&wanted()), Some(&m)).unwrap(),
            Outcome::Unchanged
        );
        assert!(f.writes().is_empty(), "{:?}", f.writes());
    }

    #[test]
    fn a_restart_replaces_the_old_ones_and_leaves_anothers_alone() {
        let f = Fake::default();
        f.trunks.lock().unwrap().extend([
            json!({"sip_trunk_id": "ST_old", "name": OWNER_NAME}),
            json!({"sip_trunk_id": "ST_theirs", "name": "someone else's"}),
        ]);
        f.rules.lock().unwrap().extend([
            json!({"sip_dispatch_rule_id": "SDR_old", "name": OWNER_NAME}),
            json!({"sip_dispatch_rule_id": "SDR_theirs", "name": "someone else's"}),
        ]);
        // A new process has made nothing yet: the station's old ones go, both are made again.
        let Outcome::Made(m) = reconcile(&f, Some(&wanted()), None).unwrap() else {
            panic!()
        };
        assert_eq!(
            f.writes(),
            [
                "DeleteSIPDispatchRule",
                "DeleteSIPTrunk",
                "CreateSIPInboundTrunk",
                "CreateSIPDispatchRule"
            ]
        );
        let trunks: Vec<_> = f
            .trunks
            .lock()
            .unwrap()
            .iter()
            .map(|t| t["sip_trunk_id"].clone())
            .collect();
        assert_eq!(trunks, [json!("ST_theirs"), json!(m.trunk_id)]);
        let rules: Vec<_> = f
            .rules
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["sip_dispatch_rule_id"].clone())
            .collect();
        assert_eq!(rules, [json!("SDR_theirs"), json!(m.rule_id)]);
    }

    #[test]
    fn a_store_that_forgot_them_gets_them_back() {
        let f = Fake::default();
        let Outcome::Made(m) = reconcile(&f, Some(&wanted()), None).unwrap() else {
            panic!()
        };
        // Valkey restarted: it keeps nothing on disk.
        f.trunks.lock().unwrap().clear();
        f.rules.lock().unwrap().clear();
        let Outcome::Made(m2) = reconcile(&f, Some(&wanted()), Some(&m)).unwrap() else {
            panic!("not made again")
        };
        assert_ne!(m, m2);
        assert_eq!(f.trunks.lock().unwrap().len(), 1);
        assert_eq!(f.rules.lock().unwrap().len(), 1);
        // A rule lost alone (the trunk kept) is made again too, with a fresh trunk.
        f.rules.lock().unwrap().clear();
        assert!(matches!(
            reconcile(&f, Some(&wanted()), Some(&m2)).unwrap(),
            Outcome::Made(_)
        ));
        assert_eq!(f.trunks.lock().unwrap().len(), 1);
    }

    #[test]
    fn with_the_line_off_its_own_are_deleted_and_nothing_made() {
        let f = Fake::default();
        reconcile(&f, Some(&wanted()), None).unwrap();
        f.trunks
            .lock()
            .unwrap()
            .push(json!({"sip_trunk_id": "ST_theirs", "name": "x"}));
        f.calls.lock().unwrap().clear();
        assert_eq!(reconcile(&f, None, None).unwrap(), Outcome::Removed(2));
        assert_eq!(f.writes(), ["DeleteSIPDispatchRule", "DeleteSIPTrunk"]);
        assert_eq!(f.trunks.lock().unwrap().len(), 1);
        assert!(f.rules.lock().unwrap().is_empty());
        assert_eq!(reconcile(&f, None, None).unwrap(), Outcome::Removed(0));
    }

    #[test]
    fn livekit_down_is_an_error_to_try_again_and_writes_nothing() {
        let f = Fake::default();
        *f.down.lock().unwrap() = true;
        let e = reconcile(&f, Some(&wanted()), None).unwrap_err();
        assert!(e.contains("did not answer"), "{e}");
        *f.down.lock().unwrap() = false;
        assert!(matches!(
            reconcile(&f, Some(&wanted()), None).unwrap(),
            Outcome::Made(_)
        ));
    }

    #[test]
    fn a_digest_user_without_a_password_is_refused_in_words() {
        let c = PhoneConfig {
            auth_username: "kettle".into(),
            ..cfg()
        };
        let e = Wanted::from_config(&c, "studio", Some("")).err().unwrap();
        assert!(e.contains(TRUNK_PASSWORD_ENV), "{e}");
        let w = Wanted::from_config(&c, "studio", Some("pw")).unwrap();
        assert_eq!(w.trunk["trunk"]["auth_password"], "pw");
    }

    /// The HTTP transport against a stand-in LiveKit: the Twirp path, a `sip.admin` bearer, and a
    /// Twirp error's message (not the body sent) in the error.
    #[test]
    fn the_twirp_transport_sends_a_sip_admin_bearer_to_livekits_path() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (n, s) in listener.incoming().take(2).enumerate() {
                let mut s = s.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let (mut auth, mut len) = (String::new(), 0usize);
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if lower.starts_with("authorization:") {
                        auth = h["authorization:".len()..].trim().to_owned();
                    }
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                seen.push((line.trim().to_owned(), auth));
                let (status, text) = if n == 0 {
                    ("200 OK", r#"{"items": []}"#)
                } else {
                    (
                        "409 Conflict",
                        r#"{"code": "already_exists", "msg": "conflicting rule"}"#,
                    )
                };
                write!(
                    s,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
            }
            seen
        });
        let api = Twirp {
            api_url: format!("http://{addr}/"),
            keys: weavesignal_proto::Keys {
                api_key: "k".into(),
                api_secret: "test-only-secret-test-only-secret-0".into(),
            },
        };
        assert_eq!(
            api.call("ListSIPInboundTrunk", json!({})).unwrap(),
            json!({"items": []})
        );
        let e = api
            .call(
                "CreateSIPDispatchRule",
                json!({"secret": "never-in-the-error"}),
            )
            .unwrap_err();
        assert!(e.contains("409") && e.contains("conflicting rule"), "{e}");
        assert!(!e.contains("never-in-the-error"));
        let seen = server.join().unwrap();
        assert_eq!(
            seen[0].0,
            "POST /twirp/livekit.SIP/ListSIPInboundTrunk HTTP/1.1"
        );
        let bearer = seen[0].1.strip_prefix("Bearer ").unwrap();
        use base64::Engine as _;
        let claims: Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(bearer.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["sip"]["admin"], true);
    }
}
