// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The moderation rules a message meets before anyone sees it: pure, no I/O, so each rule is
//! tested on its own. The chat service feeds them the room's modes, the sender's standing and
//! recent posts, and the draft; they answer refuse, hold or post.
//!
//! The defaults are ADR-0036's: the word filter (rustrict only, ADR-0037), a rate limit, a
//! 10-minute account age and links for trusted chatters only are on; slow mode and the hold
//! for new accounts are off but one key away; influx turns on accounts only, a 24-hour age,
//! slow mode, the hold for new chatters and trusted-only links (ADR-0068), with the same
//! numbers the screener's toast states ("slow mode 10 s", "24 h old").
//!
//! The order matters and is fixed: who you are (banned, timed out, a guest where accounts are
//! required, too new), then what you sent (empty, too long, a reply in comments-only mode,
//! links), then how fast (the rate limit, slow mode, a repeat), then the hold (the word filter
//! holds for a moderator; new accounts and guests are held for a few seconds).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// The longest message, in characters.
pub const MAX_CHARS: usize = 500;
/// The fewest milliseconds between two posts from one chatter (Owncast's floor, about 0.67 s).
pub const MIN_GAP_MS: u64 = 670;
/// At most this many posts in [`BURST_WINDOW_MS`].
pub const BURST: usize = 5;
pub const BURST_WINDOW_MS: u64 = 10_000;
/// The same words again within this long is refused as a repeat.
pub const REPEAT_WINDOW_MS: u64 = 30_000;
/// One character this many times in a row is refused.
pub const MAX_RUN: usize = 12;
/// Slow mode: one message every this many seconds (the screener's "Slow 10 s").
pub const SLOW_S: u64 = 10;
/// The account age needed to post (ADR-0036), and in influx (D35: 24 hours).
pub const MIN_AGE_MINUTES: u64 = 10;
pub const INFLUX_AGE_MINUTES: u64 = 24 * 60;
/// An account younger than this is "new" for the hold (the screener's "Hold new accounts").
pub const NEW_ACCOUNT_HOURS: u64 = 24;
/// How long a held message waits for a moderator before it releases by itself (the
/// synthesis advises 4 or 5 seconds; the design's countdown reads "Releases ... in 4 s").
pub const HOLD_MS: u64 = 5_000;

/// The room's modes as the screener sets them (the control plane keeps them, ADR-0068), and
/// the station's own two settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modes {
    pub slow: bool,
    pub hold_new: bool,
    pub links_trusted: bool,
    pub accounts_only: bool,
    pub min_age_24h: bool,
    pub influx: bool,
    /// Comments and reactions only (ADR-0035): no replies between chatters.
    pub comments_only: bool,
    /// The station lets people post as guests, held every time (ADR-0034).
    pub guests: bool,
}

impl Default for Modes {
    /// ADR-0036: links for trusted chatters on; slow mode and the hold off.
    fn default() -> Self {
        Modes {
            slow: false,
            hold_new: false,
            links_trusted: true,
            accounts_only: false,
            min_age_24h: false,
            influx: false,
            comments_only: false,
            guests: false,
        }
    }
}

impl Modes {
    pub fn min_age_minutes(&self) -> u64 {
        if self.min_age_24h || self.influx {
            INFLUX_AGE_MINUTES
        } else {
            MIN_AGE_MINUTES
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Email,
    Guest,
    Staff,
}

/// What the station has granted a chatter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Member,
    /// May post links (`chat.post.links`).
    Trusted,
    /// May act on the room: hide, time out, ban, release, pin.
    Moderator,
    /// The station's own people, from the studio app.
    Staff,
}

impl Standing {
    pub fn parse(s: &str) -> Option<Standing> {
        Some(match s {
            "member" => Standing::Member,
            "trusted" => Standing::Trusted,
            "moderator" => Standing::Moderator,
            "staff" => Standing::Staff,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Member => "member",
            Standing::Trusted => "trusted",
            Standing::Moderator => "moderator",
            Standing::Staff => "staff",
        }
    }
    pub fn moderates(self) -> bool {
        self >= Standing::Moderator
    }
    pub fn links(self) -> bool {
        self >= Standing::Trusted
    }
}

/// A time-out until a time, or a ban.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restriction {
    TimedOut { until_ms: u64 },
    Banned,
}

/// What the rules need to know about the sender.
#[derive(Clone, Debug)]
pub struct Poster {
    pub kind: Kind,
    pub standing: Standing,
    pub created_ms: u64,
    pub restriction: Option<Restriction>,
    /// Their recent posts, oldest first: when, and a hash of the words.
    pub recent: VecDeque<(u64, u64)>,
}

impl Poster {
    /// Remember a post that was accepted (posted or held), keeping only what the rules read.
    pub fn remember(&mut self, now: u64, words_hash: u64) {
        self.recent.push_back((now, words_hash));
        let keep = REPEAT_WINDOW_MS.max(BURST_WINDOW_MS).max(SLOW_S * 1000);
        while let Some(&(t, _)) = self.recent.front() {
            if now.saturating_sub(t) > keep || self.recent.len() > 16 {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Chat,
    Request,
    /// Private to the screener and the presenter (ADR-0040).
    Studio,
    /// A staff announcement.
    Announcement,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Chat => "chat",
            MessageKind::Request => "request",
            MessageKind::Studio => "studio",
            MessageKind::Announcement => "announcement",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "chat" => MessageKind::Chat,
            "request" => MessageKind::Request,
            "studio" => MessageKind::Studio,
            "announcement" => MessageKind::Announcement,
            _ => return None,
        })
    }
}

/// What a sender typed.
#[derive(Clone, Debug)]
pub struct Draft<'a> {
    pub kind: MessageKind,
    pub body: &'a str,
    pub reply: bool,
}

/// Why the word filter stopped a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterHit {
    /// rustrict judged it (ADR-0037).
    Rustrict,
    /// One of the station's own blocked words.
    StationWord,
}

impl FilterHit {
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterHit::Rustrict => "rustrict",
            FilterHit::StationWord => "station word list",
        }
    }
}

/// Why a message was refused; each has the sentence the sender sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Banned,
    TimedOut { until_ms: u64 },
    AccountsOnly,
    GuestsOff,
    TooNew { wait_ms: u64, minutes: u64 },
    Empty,
    TooLong,
    RepliesOff,
    StaffOnly,
    Links,
    TooFast { retry_ms: u64 },
    Slow { retry_ms: u64 },
    Repeat,
    Run,
}

impl Refusal {
    /// A short code the pages key their wording and countdowns on.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Banned => "banned",
            Refusal::TimedOut { .. } => "timed_out",
            Refusal::AccountsOnly => "accounts_only",
            Refusal::GuestsOff => "guests_off",
            Refusal::TooNew { .. } => "too_new",
            Refusal::Empty => "empty",
            Refusal::TooLong => "too_long",
            Refusal::RepliesOff => "replies_off",
            Refusal::StaffOnly => "staff_only",
            Refusal::Links => "links",
            Refusal::TooFast { .. } => "too_fast",
            Refusal::Slow { .. } => "slow",
            Refusal::Repeat => "repeat",
            Refusal::Run => "run",
        }
    }

    /// When the sender may try again, if waiting helps.
    pub fn retry_ms(&self) -> Option<u64> {
        match self {
            Refusal::TooNew { wait_ms, .. } => Some(*wait_ms),
            Refusal::TooFast { retry_ms } | Refusal::Slow { retry_ms } => Some(*retry_ms),
            _ => None,
        }
    }

    /// What the sender reads, in plain words (ADR-0032: terse).
    pub fn words(&self) -> String {
        match self {
            Refusal::Banned => "You can't post here: this account is banned.".into(),
            Refusal::TimedOut { .. } => "You're timed out. You can post again when it ends.".into(),
            Refusal::AccountsOnly => {
                "Only signed-in chatters can post right now. Sign in with your email to join in."
                    .into()
            }
            Refusal::GuestsOff => {
                "Sign in with your email to post. Reading needs no account.".into()
            }
            Refusal::TooNew { minutes, .. } => {
                if *minutes >= 60 {
                    format!(
                        "New accounts can post once they're {} hours old.",
                        minutes / 60
                    )
                } else {
                    format!("New accounts can post once they're {minutes} minutes old.")
                }
            }
            Refusal::Empty => "Type a message first.".into(),
            Refusal::TooLong => format!("Keep it under {MAX_CHARS} characters."),
            Refusal::RepliesOff => {
                "Replies are off: this chat takes comments on the show only.".into()
            }
            Refusal::StaffOnly => "Only the station posts announcements.".into(),
            Refusal::Links => {
                "Links are for trusted chatters only. Send it without the link.".into()
            }
            Refusal::TooFast { .. } => "Slow down a little.".into(),
            Refusal::Slow { .. } => format!("Slow mode is on: one message every {SLOW_S} seconds."),
            Refusal::Repeat => "You just sent that.".into(),
            Refusal::Run => "Too many repeated characters.".into(),
        }
    }
}

/// Why a message is held, and whether it releases by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hold {
    /// The word filter flagged it: it waits for a moderator, however long.
    Flagged(FilterHit),
    /// A guest (always, ADR-0034) or a new account while the hold is on: it releases by itself
    /// after [`HOLD_MS`] unless a moderator hides it.
    New { guest: bool },
}

impl Hold {
    pub fn auto_release_ms(&self) -> Option<u64> {
        match self {
            Hold::Flagged(_) => None,
            Hold::New { .. } => Some(HOLD_MS),
        }
    }
    /// The words the moderator reads on the held item.
    pub fn why(&self, account_age_ms: u64) -> String {
        match self {
            Hold::Flagged(hit) => format!("Held: the word filter ({}) flagged it.", hit.as_str()),
            Hold::New { guest: true } => "Held: a guest, without an account.".into(),
            Hold::New { guest: false } => {
                format!("Held: new account, {} old.", age_words(account_age_ms))
            }
        }
    }
}

/// "40 s", "2 min", "3 h", "3 days".
pub fn age_words(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else if s < 48 * 3600 {
        format!("{} h", s / 3600)
    } else {
        format!("{} days", s / 86_400)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Refuse(Refusal),
    Hold(Hold),
    Post,
}

/// Judge a draft. `filter` is the word filter (rustrict and the station's words); it is called
/// only for a message that would otherwise be accepted.
pub fn judge(
    now: u64,
    modes: &Modes,
    poster: &Poster,
    draft: &Draft<'_>,
    filter: impl FnOnce(&str) -> Option<FilterHit>,
) -> Verdict {
    use Verdict::Refuse;
    let staff = poster.standing.moderates();

    // ---- who ----
    match poster.restriction {
        Some(Restriction::Banned) => return Refuse(Refusal::Banned),
        Some(Restriction::TimedOut { until_ms }) if until_ms > now => {
            return Refuse(Refusal::TimedOut { until_ms });
        }
        _ => {}
    }
    let guest = poster.kind == Kind::Guest;
    if guest {
        if !modes.guests {
            return Refuse(Refusal::GuestsOff);
        }
        if modes.accounts_only || modes.influx {
            return Refuse(Refusal::AccountsOnly);
        }
    }
    let age_ms = now.saturating_sub(poster.created_ms);
    if !staff && !guest {
        let minutes = modes.min_age_minutes();
        let need = minutes * 60_000;
        if age_ms < need {
            return Refuse(Refusal::TooNew {
                wait_ms: need - age_ms,
                minutes,
            });
        }
    }

    // ---- what ----
    let body = draft.body.trim();
    if body.is_empty() {
        return Refuse(Refusal::Empty);
    }
    if body.chars().count() > MAX_CHARS {
        return Refuse(Refusal::TooLong);
    }
    if draft.kind == MessageKind::Announcement && !staff {
        return Refuse(Refusal::StaffOnly);
    }
    if draft.reply && modes.comments_only && !staff {
        return Refuse(Refusal::RepliesOff);
    }
    if !staff && (modes.links_trusted || modes.influx) && !poster.standing.links() && has_link(body)
    {
        return Refuse(Refusal::Links);
    }
    if !staff && longest_run(body) >= MAX_RUN {
        return Refuse(Refusal::Run);
    }

    // ---- how fast ----
    if !staff {
        if let Some(&(last, _)) = poster.recent.back() {
            let since = now.saturating_sub(last);
            if since < MIN_GAP_MS {
                return Refuse(Refusal::TooFast {
                    retry_ms: MIN_GAP_MS - since,
                });
            }
            // Slow mode paces the room, not the private lane to the studio.
            if (modes.slow || modes.influx)
                && draft.kind != MessageKind::Studio
                && since < SLOW_S * 1000
            {
                return Refuse(Refusal::Slow {
                    retry_ms: SLOW_S * 1000 - since,
                });
            }
        }
        let in_window: Vec<u64> = poster
            .recent
            .iter()
            .filter(|(t, _)| now.saturating_sub(*t) < BURST_WINDOW_MS)
            .map(|(t, _)| *t)
            .collect();
        if in_window.len() >= BURST {
            let oldest = in_window[in_window.len() - BURST];
            return Refuse(Refusal::TooFast {
                retry_ms: BURST_WINDOW_MS - now.saturating_sub(oldest),
            });
        }
        let fp = words_hash(body);
        if poster
            .recent
            .iter()
            .any(|(t, f)| *f == fp && now.saturating_sub(*t) < REPEAT_WINDOW_MS)
        {
            return Refuse(Refusal::Repeat);
        }
    }

    // ---- the hold ----
    if staff {
        return Verdict::Post;
    }
    if let Some(hit) = filter(body) {
        return Verdict::Hold(Hold::Flagged(hit));
    }
    if guest {
        return Verdict::Hold(Hold::New { guest: true });
    }
    if (modes.hold_new || modes.influx)
        && poster.standing < Standing::Trusted
        && age_ms < NEW_ACCOUNT_HOURS * 3_600_000
    {
        return Verdict::Hold(Hold::New { guest: false });
    }
    Verdict::Post
}

/// A cheap hash of a message's words, for the repeat rule: case, spaces and punctuation
/// do not make a message new.
pub fn words_hash(body: &str) -> u64 {
    // FNV-1a over the lower-cased letters and digits.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in body.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    h
}

/// The longest run of one character, ignoring spaces.
pub fn longest_run(body: &str) -> usize {
    let mut best = 0;
    let mut run = 0;
    let mut last = None;
    for c in body.chars().filter(|c| !c.is_whitespace()) {
        if Some(c) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(c);
        }
        best = best.max(run);
    }
    best
}

/// Whether a message carries a link: a scheme, `www.`, or a word that ends in a dot and a
/// known top-level domain (so "bus.stop" is not a link and "spam.shop/x" is).
pub fn has_link(body: &str) -> bool {
    let lower = body.to_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return true;
    }
    lower
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"' | '\''))
        .any(|word| {
            let word = word.trim_end_matches(['.', ',', '!', '?', ';', ':']);
            let host = word.split(['/', '?', '#']).next().unwrap_or("");
            let Some((name, tld)) = host.rsplit_once('.') else {
                return false;
            };
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
                && TLDS.contains(&tld)
        })
}

/// Top-level domains common in chat spam, and the generic and country ones people type.
const TLDS: &[&str] = &[
    "com", "net", "org", "io", "co", "uk", "ie", "de", "fr", "eu", "us", "ca", "au", "nz", "info",
    "biz", "xyz", "top", "shop", "store", "online", "site", "club", "app", "dev", "ly", "gg", "tv",
    "me", "link", "live", "fm", "ru", "cn", "tk", "ml", "ga", "cf", "gq", "click", "win", "vip",
    "pro", "lol", "fun", "art", "ai", "to", "be", "nl", "es", "it", "pl", "se", "no", "dk", "fi",
    "at", "ch", "pt", "in", "br", "mx", "jp", "kr", "gov", "edu", "scot", "wales", "london",
];

/// rustrict's judgement (ADR-0037): profane, offensive, sexual or mean at moderate severity or
/// worse holds a message for a moderator. Mild words (a "damn" in banter) pass.
pub fn rustrict_flags(body: &str) -> bool {
    use rustrict::{CensorStr, Type};
    body.is(Type::MODERATE_OR_HIGHER
        & (Type::PROFANE | Type::OFFENSIVE | Type::SEXUAL | Type::MEAN))
}

/// The station's own blocked words, matched on whole words after lower-casing.
pub fn station_word(body: &str, words: &[String]) -> bool {
    if words.is_empty() {
        return false;
    }
    let lower = body.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().any(|w| {
        let w = w.trim().to_lowercase();
        if w.is_empty() {
            return false;
        }
        if w.contains(' ') {
            lower.contains(&w)
        } else {
            tokens.contains(&w.as_str())
        }
    })
}

/// A display name a chatter may take: 1 to 40 characters, letters, digits, spaces and
/// `_ - . '`, not only spaces, and not something the word filter or the station's list of
/// forbidden names catches. Returns the name tidied (outer spaces gone, inner runs single).
pub fn check_name(raw: &str, forbidden: &[String]) -> Result<String, &'static str> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Choose a name to show beside your messages.");
    }
    if name.chars().count() > 40 {
        return Err("Keep your name to 40 characters.");
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '\''))
    {
        return Err("Use letters, numbers, spaces and _ - . ' only.");
    }
    let lower = name.to_lowercase();
    if forbidden.iter().any(|f| {
        let f = f.trim().to_lowercase();
        !f.is_empty() && lower.contains(&f)
    }) || rustrict_flags(&name)
    {
        return Err("Choose another name.");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;
    const NOW: u64 = 1_000_000_000;

    fn member(age_ms: u64) -> Poster {
        Poster {
            kind: Kind::Email,
            standing: Standing::Member,
            created_ms: NOW - age_ms,
            restriction: None,
            recent: VecDeque::new(),
        }
    }
    fn chat(body: &str) -> Draft<'_> {
        Draft {
            kind: MessageKind::Chat,
            body,
            reply: false,
        }
    }
    fn clean(_: &str) -> Option<FilterHit> {
        None
    }
    fn judge_clean(modes: &Modes, p: &Poster, d: &Draft<'_>) -> Verdict {
        judge(NOW, modes, p, d, clean)
    }

    #[test]
    fn the_defaults_are_adr_0036s() {
        let m = Modes::default();
        assert!(m.links_trusted, "links for trusted chatters only, on");
        assert!(
            !m.slow && !m.hold_new && !m.influx,
            "slow mode and the hold off"
        );
        assert_eq!(m.min_age_minutes(), 10);
        assert!(!m.comments_only, "full chat by default (ADR-0035)");
        assert!(!m.guests, "posting needs an account (ADR-0034)");
    }

    #[test]
    fn an_old_enough_member_posts() {
        let m = Modes::default();
        assert_eq!(
            judge_clean(&m, &member(11 * MIN), &chat("hello")),
            Verdict::Post
        );
    }

    #[test]
    fn a_new_account_waits_ten_minutes_and_is_told_how_long() {
        let m = Modes::default();
        let v = judge_clean(&m, &member(3 * MIN), &chat("hello"));
        assert_eq!(
            v,
            Verdict::Refuse(Refusal::TooNew {
                wait_ms: 7 * MIN,
                minutes: 10
            })
        );
        if let Verdict::Refuse(r) = v {
            assert_eq!(r.retry_ms(), Some(7 * MIN));
            assert!(r.words().contains("10 minutes"));
        }
    }

    #[test]
    fn influx_asks_for_a_day_old_account() {
        let m = Modes {
            influx: true,
            ..Modes::default()
        };
        let v = judge_clean(&m, &member(2 * 3600 * 1000), &chat("hello"));
        assert!(matches!(
            v,
            Verdict::Refuse(Refusal::TooNew { minutes: 1440, .. })
        ));
        if let Verdict::Refuse(r) = v {
            assert!(r.words().contains("24 hours"));
        }
    }

    #[test]
    fn banned_and_timed_out_chatters_cannot_post() {
        let m = Modes::default();
        let mut p = member(MIN * 60);
        p.restriction = Some(Restriction::Banned);
        assert_eq!(
            judge_clean(&m, &p, &chat("hi")),
            Verdict::Refuse(Refusal::Banned)
        );
        p.restriction = Some(Restriction::TimedOut { until_ms: NOW + 5 });
        assert_eq!(
            judge_clean(&m, &p, &chat("hi")),
            Verdict::Refuse(Refusal::TimedOut { until_ms: NOW + 5 })
        );
        // A time-out that has ended no longer counts.
        p.restriction = Some(Restriction::TimedOut { until_ms: NOW - 1 });
        assert_eq!(judge_clean(&m, &p, &chat("hi")), Verdict::Post);
    }

    #[test]
    fn guests_post_only_where_the_station_allows_and_are_always_held() {
        let mut g = member(0);
        g.kind = Kind::Guest;
        let off = Modes::default();
        assert_eq!(
            judge_clean(&off, &g, &chat("hi")),
            Verdict::Refuse(Refusal::GuestsOff)
        );
        let on = Modes {
            guests: true,
            ..Modes::default()
        };
        // No account-age wait for a guest; the hold stands in for it (ADR-0034).
        assert_eq!(
            judge_clean(&on, &g, &chat("hi")),
            Verdict::Hold(Hold::New { guest: true })
        );
        let accounts_only = Modes {
            guests: true,
            accounts_only: true,
            ..Modes::default()
        };
        assert_eq!(
            judge_clean(&accounts_only, &g, &chat("hi")),
            Verdict::Refuse(Refusal::AccountsOnly)
        );
        let influx = Modes {
            guests: true,
            influx: true,
            ..Modes::default()
        };
        assert_eq!(
            judge_clean(&influx, &g, &chat("hi")),
            Verdict::Refuse(Refusal::AccountsOnly)
        );
    }

    #[test]
    fn links_are_for_trusted_chatters_while_the_mode_is_on() {
        let m = Modes::default();
        let p = member(MIN * 60);
        for body in [
            "see https://spam.example/x",
            "go to www.free-stuff.example",
            "cheap at bonnie-gifts.shop now",
            "visit spam.shop/deal",
        ] {
            assert_eq!(
                judge_clean(&m, &p, &chat(body)),
                Verdict::Refuse(Refusal::Links),
                "{body}"
            );
        }
        // Dots in ordinary words are not links.
        for body in [
            "the bus.stop is shut",
            "it's 3.50 a ticket",
            "end of story.",
        ] {
            assert_eq!(judge_clean(&m, &p, &chat(body)), Verdict::Post, "{body}");
        }
        let mut t = member(MIN * 60);
        t.standing = Standing::Trusted;
        assert_eq!(
            judge_clean(&m, &t, &chat("https://kettle.example/tickets")),
            Verdict::Post
        );
        let open = Modes {
            links_trusted: false,
            ..Modes::default()
        };
        assert_eq!(
            judge_clean(&open, &p, &chat("https://kettle.example/tickets")),
            Verdict::Post
        );
    }

    #[test]
    fn the_rate_limit_spaces_posts_and_caps_a_burst() {
        let m = Modes::default();
        let mut p = member(MIN * 60);
        p.remember(NOW - 300, words_hash("one"));
        assert_eq!(
            judge_clean(&m, &p, &chat("two")),
            Verdict::Refuse(Refusal::TooFast {
                retry_ms: MIN_GAP_MS - 300
            })
        );
        let mut q = member(MIN * 60);
        for i in 0..BURST as u64 {
            q.remember(NOW - 9_000 + i * 1_000, i);
        }
        assert!(matches!(
            judge_clean(&m, &q, &chat("again")),
            Verdict::Refuse(Refusal::TooFast { .. })
        ));
    }

    #[test]
    fn slow_mode_allows_one_message_in_ten_seconds_but_not_for_the_studio_lane() {
        let m = Modes {
            slow: true,
            ..Modes::default()
        };
        let mut p = member(MIN * 60);
        p.remember(NOW - 4_000, words_hash("first"));
        let v = judge_clean(&m, &p, &chat("second"));
        assert_eq!(v, Verdict::Refuse(Refusal::Slow { retry_ms: 6_000 }));
        let studio = Draft {
            kind: MessageKind::Studio,
            body: "Rory, my dad drove the N22",
            reply: false,
        };
        assert_eq!(judge_clean(&m, &p, &studio), Verdict::Post);
        p.recent.clear();
        p.remember(NOW - 10_500, words_hash("first"));
        assert_eq!(judge_clean(&m, &p, &chat("second")), Verdict::Post);
    }

    #[test]
    fn the_same_words_twice_in_thirty_seconds_are_a_repeat() {
        let m = Modes::default();
        let mut p = member(MIN * 60);
        p.remember(NOW - 5_000, words_hash("Play it again Rory!"));
        assert_eq!(
            judge_clean(&m, &p, &chat("play it again, rory")),
            Verdict::Refuse(Refusal::Repeat)
        );
        assert_eq!(
            judge_clean(&m, &p, &chat("play the other one")),
            Verdict::Post
        );
    }

    #[test]
    fn empty_long_and_shouty_messages_are_refused() {
        let m = Modes::default();
        let p = member(MIN * 60);
        assert_eq!(
            judge_clean(&m, &p, &chat("   ")),
            Verdict::Refuse(Refusal::Empty)
        );
        let long = "a ".repeat(MAX_CHARS);
        assert_eq!(
            judge_clean(&m, &p, &chat(&long)),
            Verdict::Refuse(Refusal::TooLong)
        );
        assert_eq!(
            judge_clean(&m, &p, &chat("FIRST!!!!!!!!!!!!!!")),
            Verdict::Refuse(Refusal::Run)
        );
        assert_eq!(judge_clean(&m, &p, &chat("FIRST!!!")), Verdict::Post);
    }

    #[test]
    fn comments_only_refuses_replies_between_chatters() {
        let m = Modes {
            comments_only: true,
            ..Modes::default()
        };
        let p = member(MIN * 60);
        let reply = Draft {
            kind: MessageKind::Chat,
            body: "agreed",
            reply: true,
        };
        assert_eq!(
            judge_clean(&m, &p, &reply),
            Verdict::Refuse(Refusal::RepliesOff)
        );
        assert_eq!(judge_clean(&m, &p, &chat("great show")), Verdict::Post);
    }

    #[test]
    fn the_word_filter_holds_for_a_moderator_with_no_countdown() {
        let m = Modes::default();
        let p = member(MIN * 60);
        let v = judge(NOW, &m, &p, &chat("anything"), |_| {
            Some(FilterHit::Rustrict)
        });
        assert_eq!(v, Verdict::Hold(Hold::Flagged(FilterHit::Rustrict)));
        if let Verdict::Hold(h) = v {
            assert_eq!(h.auto_release_ms(), None);
            assert!(h.why(0).contains("rustrict"));
        }
    }

    #[test]
    fn hold_new_holds_accounts_under_a_day_for_five_seconds() {
        let m = Modes {
            hold_new: true,
            ..Modes::default()
        };
        let v = judge_clean(&m, &member(2 * MIN * 60), &chat("hiya"));
        assert_eq!(v, Verdict::Hold(Hold::New { guest: false }));
        if let Verdict::Hold(h) = v {
            assert_eq!(h.auto_release_ms(), Some(HOLD_MS));
            assert_eq!(h.why(2 * MIN * 60), "Held: new account, 2 h old.");
        }
        assert_eq!(
            judge_clean(&m, &member(25 * MIN * 60), &chat("hiya")),
            Verdict::Post
        );
        let mut t = member(2 * MIN * 60);
        t.standing = Standing::Trusted;
        assert_eq!(judge_clean(&m, &t, &chat("hiya")), Verdict::Post);
    }

    #[test]
    fn staff_and_moderators_skip_the_limits_but_not_a_ban() {
        let m = Modes {
            influx: true,
            ..Modes::default()
        };
        let mut s = member(0);
        s.standing = Standing::Staff;
        s.remember(NOW - 10, words_hash("welcome"));
        assert_eq!(
            judge(NOW, &m, &s, &chat("welcome https://kettle.example"), |_| {
                Some(FilterHit::Rustrict)
            }),
            Verdict::Post
        );
        let ann = Draft {
            kind: MessageKind::Announcement,
            body: "Slow mode is on for a bit.",
            reply: false,
        };
        assert_eq!(judge_clean(&m, &s, &ann), Verdict::Post);
        assert_eq!(
            judge_clean(&Modes::default(), &member(MIN * 60), &ann),
            Verdict::Refuse(Refusal::StaffOnly)
        );
        s.restriction = Some(Restriction::Banned);
        assert_eq!(
            judge_clean(&m, &s, &chat("x")),
            Verdict::Refuse(Refusal::Banned)
        );
    }

    #[test]
    fn rustrict_catches_abuse_and_lets_banter_through() {
        assert!(rustrict_flags("you are a fucking idiot"));
        assert!(!rustrict_flags(
            "the N22 was a lifeline, two an hour till three"
        ));
        assert!(!rustrict_flags("Scunthorpe United lost again"));
    }

    #[test]
    fn station_words_match_whole_words() {
        let words = vec!["councillor smith".to_owned(), "scab".to_owned()];
        assert!(station_word("Ask Councillor Smith about it", &words));
        assert!(station_word("what a scab!", &words));
        assert!(!station_word("he scabbed his knee", &words));
        assert!(!station_word("anything", &[]));
    }

    #[test]
    fn names_are_tidied_and_checked() {
        assert_eq!(
            check_name("  Elspeth   Gordon ", &[]).unwrap(),
            "Elspeth Gordon"
        );
        assert_eq!(check_name("ceilidh_jean", &[]).unwrap(), "ceilidh_jean");
        assert!(check_name("", &[]).is_err());
        assert!(check_name(&"a".repeat(41), &[]).is_err());
        assert!(check_name("<script>", &[]).is_err());
        assert!(check_name("Kettle FM staff", &["kettle fm".to_owned()]).is_err());
        assert!(check_name("shithead", &[]).is_err());
    }

    #[test]
    fn age_words_read_naturally() {
        assert_eq!(age_words(40_000), "40 s");
        assert_eq!(age_words(2 * MIN), "2 min");
        assert_eq!(age_words(3 * 3600 * 1000), "3 h");
        assert_eq!(age_words(3 * 86_400 * 1000), "3 days");
    }
}
