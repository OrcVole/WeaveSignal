// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows (1.3.2): what the manage routes ([`crate::access_keys`]) and the listener routes
//! ([`crate::access_listen`]) share: making a key and its text, sealing it, the limits on
//! guessing, and the sweep that ends keys and prunes use records.
//!
//! **A key's text** is 12 characters from a 31-letter alphabet (about 59 bits), written
//! `KETM-7QMP-2D4X`. It is found by a keyed hash and kept sealed (ChaCha20-Poly1305 under a key
//! derived from the station's token key, the key's id as associated data) so that staff can send
//! it again. The station's token key (`WEAVESIGNAL_CONTROL_TOKEN_KEY`, else the station key's
//! derivation) therefore also protects every key: **changing it makes every sealed text
//! unreadable and no typed text find its key**, so a station that rotates the station key and
//! wants its keys to survive sets `WEAVESIGNAL_CONTROL_TOKEN_KEY` first. Without a key at all
//! the feature fails closed: no key can be made and none redeemed.
//!
//! **Guessing** is limited three ways: redeems per address per minute, wrong keys per address
//! per ten minutes, and the key's own width. A wrong key from one address does not tell it
//! anything about another's.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::listener_keys::{Key, Kind, Media, TEXT_ALPHABET, TEXT_LEN, format_text};
use weavesignal_model::listener_keys_db as db;

use crate::access_token::{MAX_SHOWS, P256Signer, TokenSigner};
use crate::apikey::ApiKey;
use crate::config::KeysConfig;

// ---- the runtime state ----

/// Keyed shows' state in memory: the settings, the signer, the limits on guessing, and what
/// the routes read of the windows and the revoked keys ([`crate::keyed::Live`]).
#[derive(Default)]
pub struct Runtime {
    pub config: KeysConfig,
    signer: OnceLock<Box<dyn TokenSigner>>,
    /// The keyed windows, the recorded shut times and the revoked keys.
    pub live: std::sync::Arc<crate::keyed::Live>,
    /// The listener process on loopback, whose keyed mounts the origin carries.
    pub listeners_addr: OnceLock<std::net::SocketAddr>,
    /// Redeems and renewals, per address.
    pub redeems: Counter,
    /// Wrong keys, per address.
    pub failures: Counter,
    /// Lost-key requests, per address and per email hash.
    pub lost_by_address: Counter,
    pub lost_by_email: Counter,
}

impl Runtime {
    pub fn new(config: KeysConfig) -> Self {
        let live = std::sync::Arc::new(crate::keyed::Live::new(config.margins()));
        Self {
            config,
            live,
            ..Default::default()
        }
    }

    /// The signer for tokens, made once from the station's token key (ECDSA P-256).
    pub fn signer(&self, key: &ApiKey) -> &dyn TokenSigner {
        self.signer
            .get_or_init(|| Box::new(P256Signer::from_secret(&key.listener_token_key())))
            .as_ref()
    }
}

/// Counts under a key within a window, bounded in memory like [`crate::api`]'s limiter.
#[derive(Default)]
pub struct Counter {
    windows: Mutex<HashMap<String, (u64, u32)>>,
}

const MAX_KEYS: usize = 16_384;

impl Counter {
    /// Whether `key` is under `limit` in its window at `now`; `Err` is the seconds until the
    /// window turns over. A limit of 0 is no limit. Counts nothing.
    pub fn peek(&self, key: &str, now: u64, window_ms: u64, limit: u32) -> Result<(), u64> {
        if limit == 0 {
            return Ok(());
        }
        let w = self.windows.lock().expect("key limits");
        match w.get(key) {
            Some(&(start, n)) if now.saturating_sub(start) < window_ms && n >= limit => {
                Err((window_ms - now.saturating_sub(start)).div_ceil(1000))
            }
            _ => Ok(()),
        }
    }

    /// Count one under `key`.
    pub fn add(&self, key: &str, now: u64, window_ms: u64) {
        let mut w = self.windows.lock().expect("key limits");
        if w.len() >= MAX_KEYS && !w.contains_key(key) {
            w.retain(|_, (start, _)| now.saturating_sub(*start) < window_ms);
            if w.len() >= MAX_KEYS {
                w.clear();
            }
        }
        let e = w.entry(key.to_owned()).or_insert((now, 0));
        if now.saturating_sub(e.0) >= window_ms {
            *e = (now, 0);
        }
        e.1 += 1;
    }

    /// Count one and say whether it was allowed.
    pub fn hit(&self, key: &str, now: u64, window_ms: u64, limit: u32) -> Result<(), u64> {
        self.peek(key, now, window_ms, limit)?;
        self.add(key, now, window_ms);
        Ok(())
    }
}

// ---- a key's text ----

/// A fresh key text, normalised: 12 characters, unbiased (rejection sampling over the bytes).
pub fn generate_text() -> String {
    const N: u8 = TEXT_ALPHABET.len() as u8; // 31
    const LIMIT: u8 = N * 8; // 248: the largest multiple of 31 in a byte
    let rng = SystemRandom::new();
    let mut out = String::with_capacity(TEXT_LEN);
    let mut buf = [0u8; 32];
    while out.len() < TEXT_LEN {
        rng.fill(&mut buf).expect("the system's random source");
        for b in buf {
            if b < LIMIT && out.len() < TEXT_LEN {
                out.push(TEXT_ALPHABET[(b % N) as usize] as char);
            }
        }
    }
    out
}

/// A key's link: the player opens, keeps the key in this browser and cleans the address. The
/// key is in the fragment, which a browser never sends to a server (the same as an invitation's
/// pass), so it is in no server's log.
pub fn link(public_url: &str, norm_text: &str) -> String {
    format!(
        "{}/listen/#key={}",
        public_url.trim_end_matches('/'),
        format_text(norm_text)
    )
}

// ---- sealing ----

const TEXT_AAD: &[u8] = b"weavesignal listener key text ";
const EMAIL_AAD: &[u8] = b"weavesignal listener key email ";

fn aead(key: &ApiKey) -> LessSafeKey {
    LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, &key.listener_seal_key()).expect("a 32-byte key"),
    )
}

fn aad(label: &[u8], id: Uuid) -> Vec<u8> {
    let mut v = label.to_vec();
    v.extend_from_slice(id.as_bytes());
    v
}

fn seal(key: &ApiKey, label: &[u8], id: Uuid, plain: &str) -> ([u8; 12], Vec<u8>) {
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .expect("the system's random source");
    let mut data = plain.as_bytes().to_vec();
    aead(key)
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(label, id)),
            &mut data,
        )
        .expect("sealing a short text");
    (nonce, data)
}

fn open(key: &ApiKey, label: &[u8], id: Uuid, nonce: &[u8], sealed: &[u8]) -> Option<String> {
    let nonce: [u8; 12] = nonce.try_into().ok()?;
    let mut data = sealed.to_vec();
    let plain = aead(key)
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(label, id)),
            &mut data,
        )
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

pub fn seal_text(key: &ApiKey, id: Uuid, norm_text: &str) -> ([u8; 12], Vec<u8>) {
    seal(key, TEXT_AAD, id, norm_text)
}

pub fn open_text(key: &ApiKey, id: Uuid, nonce: &[u8], sealed: &[u8]) -> Option<String> {
    open(key, TEXT_AAD, id, nonce, sealed)
}

pub fn seal_email(key: &ApiKey, id: Uuid, email: &str) -> ([u8; 12], Vec<u8>) {
    seal(key, EMAIL_AAD, id, email)
}

pub fn open_email(key: &ApiKey, id: Uuid, nonce: &[u8], sealed: &[u8]) -> Option<String> {
    open(key, EMAIL_AAD, id, nonce, sealed)
}

// ---- making a key ----

/// A key as staff describe it, before it exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: Kind,
    pub label: String,
    pub note: String,
    pub email: Option<String>,
    pub media: Media,
    pub all_shows: bool,
    pub shows: Vec<Uuid>,
    pub valid_from_ms: Option<i64>,
    pub valid_until_ms: Option<i64>,
    pub max_devices: Option<u32>,
    pub max_listens: Option<u32>,
    pub max_people: Option<u32>,
}

/// A refused field and why, in a sentence for staff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecError {
    pub field: &'static str,
    pub message: String,
}

fn err(field: &'static str, message: impl Into<String>) -> SpecError {
    SpecError {
        field,
        message: message.into(),
    }
}

/// A line of text: trimmed, no control characters, at most `max` characters.
pub fn clean_line(s: &str, max: usize) -> Option<String> {
    let t = s.trim();
    (t.chars().count() <= max && !t.chars().any(char::is_control)).then(|| t.to_owned())
}

/// An email address, loosely: one `@`, something either side, no spaces, at most 254
/// characters. Whether it is real is the mail server's to say.
pub fn clean_email(s: &str) -> Option<String> {
    let t = s.trim();
    let (local, domain) = t.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && t.chars().count() <= 254
        && !t.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| t.to_owned())
}

impl Spec {
    /// Trim and check every field; the first problem is named.
    pub fn check(&mut self) -> Result<(), SpecError> {
        self.label = clean_line(&self.label, 120)
            .filter(|l| !l.is_empty())
            .ok_or_else(|| {
                err(
                    "label",
                    "Give the key a name of at most 120 characters: who it is for.",
                )
            })?;
        self.note = clean_line(&self.note, 500)
            .ok_or_else(|| err("note", "A note is at most 500 characters on one line."))?;
        if let Some(e) = self.email.take() {
            if e.trim().is_empty() {
                self.email = None;
            } else {
                self.email = Some(
                    clean_email(&e).ok_or_else(|| err("email", "That is not an email address."))?,
                );
            }
        }
        if self.all_shows && !self.shows.is_empty() {
            return Err(err(
                "shows",
                "A key opens every keyed show or the shows named, not both.",
            ));
        }
        if self.shows.len() > MAX_SHOWS {
            return Err(err(
                "shows",
                format!("A key may name at most {MAX_SHOWS} shows; use every keyed show for more."),
            ));
        }
        self.shows.sort();
        self.shows.dedup();
        if !self.all_shows && self.shows.is_empty() {
            return Err(err(
                "shows",
                "Say which shows the key opens: every keyed show, or some.",
            ));
        }
        if let (Some(f), Some(u)) = (self.valid_from_ms, self.valid_until_ms)
            && u <= f
        {
            return Err(err("valid_until", "A key must end after it starts."));
        }
        for (field, v, max) in [
            ("max_devices", self.max_devices, 1_000),
            ("max_listens", self.max_listens, 1_000_000),
            ("max_people", self.max_people, 100_000),
        ] {
            if let Some(v) = v
                && !(1..=max).contains(&v)
            {
                return Err(err(
                    field,
                    format!("A limit is 1 to {max}, or left out for none."),
                ));
            }
        }
        if self.max_people.is_some() && self.kind != Kind::Group {
            return Err(err(
                "max_people",
                "Only a group key has a number of people; a personal key is one.",
            ));
        }
        Ok(())
    }

    /// The spec a key was made from, to make its replacement the same.
    pub fn of_key(k: &Key, email: Option<String>) -> Self {
        Self {
            kind: k.kind,
            label: k.label.clone(),
            note: k.note.clone(),
            email,
            media: k.media,
            all_shows: k.all_shows,
            shows: k.shows.clone(),
            valid_from_ms: k.valid_from_ms,
            valid_until_ms: k.valid_until_ms,
            max_devices: k.max_devices,
            max_listens: k.max_listens,
            max_people: k.max_people,
        }
    }
}

/// Which of these shows exist at the station.
pub async fn missing_shows(
    pool: &sqlx::PgPool,
    station: StationId,
    shows: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    if shows.is_empty() {
        return Ok(vec![]);
    }
    let have: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM show WHERE station_id = $1 AND id = ANY($2)")
            .bind(station.as_uuid())
            .bind(shows)
            .fetch_all(pool)
            .await?;
    let have: Vec<Uuid> = have.into_iter().map(|h| h.0).collect();
    Ok(shows
        .iter()
        .copied()
        .filter(|s| !have.contains(s))
        .collect())
}

/// Make a key from a checked spec, with a fresh text, inside the caller's transaction. Returns
/// the key and its text (normalised): shown to staff this once, and again only by the reveal
/// route.
pub async fn make_key(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    api_key: &ApiKey,
    spec: &Spec,
    family: Option<Uuid>,
    replaces: Option<Uuid>,
    by: &str,
) -> Result<(Key, String), sqlx::Error> {
    let id = Uuid::new_v4();
    let text = generate_text();
    let (tn, ts) = seal_text(api_key, id, &text);
    let hash = api_key.listener_text_hash(&text);
    let email = spec.email.as_deref().map(|e| {
        let (n, s) = seal_email(api_key, id, e);
        (api_key.listener_email_hash(e), n, s)
    });
    let key = db::create_key(
        conn,
        station,
        &db::NewKey {
            id,
            family_id: family,
            kind: spec.kind,
            label: &spec.label,
            note: &spec.note,
            email: email.as_ref().map(|(h, n, s)| (h, &n[..], &s[..])),
            media: spec.media,
            all_shows: spec.all_shows,
            shows: &spec.shows,
            valid_from_ms: spec.valid_from_ms,
            valid_until_ms: spec.valid_until_ms,
            max_devices: spec.max_devices,
            max_listens: spec.max_listens,
            max_people: spec.max_people,
            text_hash: &hash,
            text_nonce: &tn,
            text_sealed: &ts,
            replaces_id: replaces,
            created_by: by,
        },
    )
    .await?;
    Ok((key, text))
}

// ---- the sweep ----

/// How often the sweep runs.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(300);

/// Tell webhooks about keys whose window has passed (once each, across restarts), and prune use
/// records past their retention. Run on a timer by [`spawn_sweeper`].
pub async fn sweep(
    pool: &sqlx::PgPool,
    station: StationId,
    events: &crate::events::Events,
    cfg: &KeysConfig,
    now_ms: i64,
) -> Result<(usize, u64), sqlx::Error> {
    let ended = db::take_expired(pool, station, now_ms).await?;
    for k in &ended {
        events.publish(crate::events::KEY_ENDED, ended_data(k, "expired"));
    }
    let pruned = db::prune(pool, station, cfg.use_records_days).await?;
    Ok((ended.len(), pruned))
}

/// What `key.ended` carries: ids and the label, never the text or an email address.
pub fn ended_data(k: &db::Ended, why: &str) -> serde_json::Value {
    serde_json::json!({
        "key": k.id,
        "label": k.label,
        "family": k.family_id,
        "kind": k.kind.as_str(),
        "reason": why,
    })
}

/// Start the sweep: first after half a minute, then every five minutes.
pub fn spawn_sweeper(staff: std::sync::Arc<crate::staff::Staff>) {
    let (Some(pool), Some(station)) = (staff.api.db().cloned(), staff.api.station()) else {
        return;
    };
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            let cfg = staff.api.access.config.clone();
            match sweep(
                &pool,
                station,
                &staff.api.events,
                &cfg,
                crate::hub::now_ms() as i64,
            )
            .await
            {
                Ok((ended, pruned)) => {
                    if ended > 0 || pruned > 0 {
                        tracing::info!(ended, pruned, "keyed shows: sweep");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "keyed shows: the sweep failed"),
            }
            tokio::time::sleep(SWEEP_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use weavesignal_model::listener_keys as rules;

    fn key() -> ApiKey {
        ApiKey::parse("0123456789abcdef0123456789abcdef").unwrap()
    }

    fn spec() -> Spec {
        Spec {
            kind: Kind::Personal,
            label: "Ishbel Fraser".into(),
            note: "".into(),
            email: Some("ishbel@example.org".into()),
            media: Media::Both,
            all_shows: true,
            shows: vec![],
            valid_from_ms: None,
            valid_until_ms: None,
            max_devices: Some(2),
            max_listens: None,
            max_people: None,
        }
    }

    #[test]
    fn a_generated_text_is_in_the_alphabet_and_different_each_time() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..500 {
            let t = generate_text();
            assert!(rules::text_is_wellformed(&t), "{t}");
            seen.insert(t);
        }
        assert_eq!(seen.len(), 500);
    }

    #[test]
    fn every_letter_of_the_alphabet_turns_up() {
        // A biased or truncated draw would leave letters out.
        let mut seen = [false; 128];
        for _ in 0..400 {
            for b in generate_text().bytes() {
                seen[b as usize] = true;
            }
        }
        for c in TEXT_ALPHABET {
            assert!(seen[*c as usize], "{}", *c as char);
        }
    }

    #[test]
    fn a_sealed_text_opens_only_for_its_key_and_under_its_secret() {
        let id = Uuid::new_v4();
        let (n, s) = seal_text(&key(), id, "KETM7QMP2D4X");
        assert_eq!(
            open_text(&key(), id, &n, &s).as_deref(),
            Some("KETM7QMP2D4X")
        );
        assert!(open_text(&key(), Uuid::new_v4(), &n, &s).is_none());
        let other = ApiKey::parse("fedcba9876543210fedcba9876543210").unwrap();
        assert!(open_text(&other, id, &n, &s).is_none());
        // A text sealed as an email does not open as a text.
        let (n2, s2) = seal_email(&key(), id, "a@b.c");
        assert!(open_text(&key(), id, &n2, &s2).is_none());
        assert_eq!(open_email(&key(), id, &n2, &s2).as_deref(), Some("a@b.c"));
        // The sealed form is not the text.
        assert!(!String::from_utf8_lossy(&s).contains("KETM"));
        // A changed byte is refused.
        let mut bad = s.clone();
        bad[0] ^= 1;
        assert!(open_text(&key(), id, &n, &bad).is_none());
    }

    #[test]
    fn the_link_keeps_the_key_in_the_fragment() {
        assert_eq!(
            link("https://kettle.example/", "KETM7QMP2D4X"),
            "https://kettle.example/listen/#key=KETM-7QMP-2D4X"
        );
    }

    #[test]
    fn a_spec_is_trimmed_and_checked() {
        let mut s = spec();
        s.label = "  Ishbel Fraser \n".into();
        s.email = Some(" ishbel@example.org ".into());
        s.check().unwrap();
        assert_eq!(s.label, "Ishbel Fraser");
        assert_eq!(s.email.as_deref(), Some("ishbel@example.org"));

        let mut s = spec();
        s.label = "  ".into();
        assert_eq!(s.check().unwrap_err().field, "label");
        let mut s = spec();
        s.email = Some("not an address".into());
        assert_eq!(s.check().unwrap_err().field, "email");
        let mut s = spec();
        s.email = Some("  ".into());
        s.check().unwrap();
        assert_eq!(s.email, None);
        let mut s = spec();
        s.all_shows = false;
        assert_eq!(s.check().unwrap_err().field, "shows");
        let mut s = spec();
        s.shows = vec![Uuid::new_v4()];
        assert_eq!(s.check().unwrap_err().field, "shows");
        let mut s = spec();
        s.valid_from_ms = Some(10);
        s.valid_until_ms = Some(10);
        assert_eq!(s.check().unwrap_err().field, "valid_until");
        let mut s = spec();
        s.max_devices = Some(0);
        assert_eq!(s.check().unwrap_err().field, "max_devices");
        let mut s = spec();
        s.max_people = Some(5);
        assert_eq!(s.check().unwrap_err().field, "max_people");
        s.kind = Kind::Group;
        s.check().unwrap();
        let mut s = spec();
        s.all_shows = false;
        s.shows = (0..=MAX_SHOWS as u128).map(Uuid::from_u128).collect();
        assert_eq!(s.check().unwrap_err().field, "shows");
    }

    #[test]
    fn show_ids_are_sorted_and_deduplicated() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut s = spec();
        s.all_shows = false;
        s.shows = vec![b, a, b];
        s.check().unwrap();
        assert_eq!(s.shows, vec![a, b]);
    }

    #[test]
    fn the_counter_peeks_adds_and_turns_over() {
        let c = Counter::default();
        assert!(c.hit("a", 1_000, 10_000, 2).is_ok());
        assert!(c.hit("a", 2_000, 10_000, 2).is_ok());
        assert_eq!(c.hit("a", 3_000, 10_000, 2), Err(8));
        // A peek counts nothing.
        assert!(c.peek("b", 3_000, 10_000, 1).is_ok());
        assert!(c.peek("b", 3_000, 10_000, 1).is_ok());
        c.add("b", 3_000, 10_000);
        assert!(c.peek("b", 3_500, 10_000, 1).is_err());
        // The window turns over.
        assert!(c.hit("a", 11_001, 10_000, 2).is_ok());
        // A limit of 0 is none.
        for _ in 0..50 {
            assert!(c.hit("z", 1, 10_000, 0).is_ok());
        }
    }

    #[test]
    fn an_email_address_is_checked_loosely() {
        assert!(clean_email("a@b.co").is_some());
        for bad in [
            "", "a", "a@b", "@b.co", "a@.co", "a@b.", "a b@c.de", "a@b@c.de",
        ] {
            assert!(clean_email(bad).is_none(), "{bad}");
        }
    }
}
