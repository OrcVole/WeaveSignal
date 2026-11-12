// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The legal toolset's words (ADR-0032: release one ships all eight; every sentence terse; no
//! screen or document says a station is compliant): the report reasons, the statement of
//! reasons a chatter receives (DSA Article 17), the mode and retention sentences, and the
//! risk-assessment template a station starts from. None of it is legal advice.

use serde::{Deserialize, Serialize};

use crate::config::{Keep, REVIEW_DAYS, Retention};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Abuse,
    Spam,
    OffTopic,
    Illegal,
    IntimateImage,
    ThreatToLife,
    ChildSafety,
    Other,
    /// Suspected child sexual abuse material in an uploaded picture (ADR-0257): a moderator's
    /// reason for a picture only, never offered on the report form.
    SuspectedCsam,
}

impl Reason {
    /// The reasons a chatter or the public can give on a report or a notice.
    pub const ALL: [Reason; 8] = [
        Reason::Abuse,
        Reason::Spam,
        Reason::OffTopic,
        Reason::Illegal,
        Reason::IntimateImage,
        Reason::ThreatToLife,
        Reason::ChildSafety,
        Reason::Other,
    ];

    /// The reasons only a moderator gives, and only about a picture.
    pub const STAFF: [Reason; 1] = [Reason::SuspectedCsam];

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::SuspectedCsam => "suspected_csam",
            Reason::Abuse => "abuse",
            Reason::Spam => "spam",
            Reason::OffTopic => "off_topic",
            Reason::Illegal => "illegal",
            Reason::IntimateImage => "intimate_image",
            Reason::ThreatToLife => "threat_to_life",
            Reason::ChildSafety => "child_safety",
            Reason::Other => "other",
        }
    }

    /// A reason the public may give (the report form, a notice).
    pub fn parse(s: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// Whether only a moderator may give it, and only for a picture (ADR-0257).
    pub fn staff_only(self) -> bool {
        Reason::STAFF.contains(&self)
    }

    /// Whether a picture refused or removed for it is kept, with the original and the uploader's
    /// details, for the year the law asks (ADR-0257), viewable by one named person.
    pub fn keeps_evidence(self) -> bool {
        self == Reason::SuspectedCsam
    }

    /// The reason as a chatter and a moderator read it.
    pub fn words(self) -> &'static str {
        match self {
            Reason::Abuse => "Abuse or harassment",
            Reason::Spam => "Spam or a scam",
            Reason::OffTopic => "Off topic or repeated",
            Reason::Illegal => "Illegal content",
            Reason::IntimateImage => "An intimate image shared without consent",
            Reason::ThreatToLife => "A threat to someone's life or safety",
            Reason::ChildSafety => "Sexual content involving a child",
            Reason::Other => "Something else",
            Reason::SuspectedCsam => "Suspected child sexual abuse material",
        }
    }

    /// One line under the reason on the report form.
    pub fn hint(self) -> &'static str {
        match self {
            Reason::Abuse => "Insults, threats or hate aimed at someone.",
            Reason::Spam => "Adverts, links to scams, the same thing again and again.",
            Reason::OffTopic => "Nothing to do with the show, or flooding the chat.",
            Reason::Illegal => "Something you believe breaks the law. Say which law if you can.",
            Reason::IntimateImage => {
                "A private sexual image or video of someone. Taken down within 48 hours."
            }
            Reason::ThreatToLife => {
                "Someone is in danger now. If it is an emergency, call the police too."
            }
            Reason::ChildSafety => "Sexual content involving anyone under 18.",
            Reason::Other => "Tell the station what is wrong.",
            Reason::SuspectedCsam => {
                "Keeps the picture and the uploader's details for a year, for a report to the authorities."
            }
        }
    }

    /// Whether the station's answer is that it is unlawful, not only against its terms.
    pub fn unlawful(self) -> bool {
        matches!(
            self,
            Reason::Illegal
                | Reason::IntimateImage
                | Reason::ThreatToLife
                | Reason::ChildSafety
                | Reason::SuspectedCsam
        )
    }

    /// A clock the law fixes: an intimate image comes down within 48 hours (the US TAKE IT DOWN
    /// Act; report 20).
    pub fn due_hours(self) -> Option<i32> {
        (self == Reason::IntimateImage).then_some(48)
    }

    /// Reasons whose reports keep their evidence at once, without waiting for a moderator.
    pub fn preserve_at_once(self) -> bool {
        matches!(
            self,
            Reason::IntimateImage
                | Reason::ThreatToLife
                | Reason::ChildSafety
                | Reason::SuspectedCsam
        )
    }
}

/// What happened to the chatter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Hidden,
    HiddenAndTimedOut {
        minutes: u32,
    },
    HiddenAndBanned,
    TimedOut {
        minutes: u32,
    },
    Banned,
    /// A held message rejected before anyone saw it.
    Rejected,
    /// An uploaded picture refused before anyone else saw it (ADR-0235).
    AvatarRefused,
    /// An approved picture taken down later.
    AvatarRemoved,
    /// A picture or document posted in the chat, refused before anyone else saw it (ADR-0262).
    AttachmentRefused {
        document: bool,
    },
    /// A picture or document posted in the chat, taken down after it was shown.
    AttachmentRemoved {
        document: bool,
    },
}

impl Outcome {
    fn about_a_picture(self) -> bool {
        matches!(self, Outcome::AvatarRefused | Outcome::AvatarRemoved)
    }

    /// The thing posted, for an attachment's outcome.
    fn attachment(self) -> Option<&'static str> {
        match self {
            Outcome::AttachmentRefused { document } | Outcome::AttachmentRemoved { document } => {
                Some(if document { "document" } else { "picture" })
            }
            _ => None,
        }
    }
}

/// The statement of reasons a chatter receives at once (DSA Article 17: the decision, the
/// facts, the ground, whether automation was used, and how to seek redress).
pub fn statement(
    station: &str,
    show: &str,
    quoted: Option<&str>,
    outcome: Outcome,
    reason: Reason,
    automated: bool,
    retention: &Retention,
) -> String {
    // Suspected child sexual abuse material: the uploader is told less (the operator,
    // 2026-11-10): that it was removed under the rules on illegal content, and how to ask for a
    // review; nothing about the reason's words, keeping it or reporting it.
    if reason.keeps_evidence() {
        let thing = match outcome {
            Outcome::AvatarRefused => "the picture you uploaded".to_owned(),
            Outcome::AttachmentRefused { document } | Outcome::AttachmentRemoved { document } => {
                format!(
                    "the {} you posted",
                    if document { "document" } else { "picture" }
                )
            }
            _ => "your picture".to_owned(),
        };
        return format!(
            "{station} removed {thing} for breaking the station's rules on illegal content. A moderator decided. \
             You can ask for a review within {REVIEW_DAYS} days. This notice is kept until the review period ends, then deleted."
        );
    }
    let what = match quoted {
        Some(q) => format!("your message “{}”", excerpt(q, 60)),
        None => "your account".to_owned(),
    };
    let in_show = if show.trim().is_empty() {
        String::new()
    } else {
        format!(" in {}", show.trim())
    };
    let done = match outcome {
        Outcome::Hidden => format!("{station} hid {what}{in_show}"),
        Outcome::Rejected => format!("{station} did not show {what}{in_show}"),
        Outcome::HiddenAndTimedOut { minutes } => {
            format!("{station} hid {what}{in_show} and stopped you posting for {minutes} minutes")
        }
        Outcome::HiddenAndBanned => {
            format!("{station} hid {what}{in_show} and banned you from its chat")
        }
        Outcome::TimedOut { minutes } => {
            format!("{station} stopped you posting for {minutes} minutes")
        }
        Outcome::Banned => format!("{station} banned you from its chat"),
        Outcome::AvatarRefused => format!("{station} did not approve the picture you uploaded"),
        Outcome::AvatarRemoved => format!("{station} removed your picture from its chat"),
        Outcome::AttachmentRefused { document } => format!(
            "{station} did not show the {} you posted{in_show}",
            if document { "document" } else { "picture" }
        ),
        Outcome::AttachmentRemoved { document } => format!(
            "{station} removed the {} you posted{in_show}, and the message with it",
            if document { "document" } else { "picture" }
        ),
    };
    let ground = if reason.unlawful() {
        "we believe it is unlawful"
    } else {
        "it broke our chat rules"
    };
    let who = if automated {
        "An automatic filter flagged it and a moderator decided."
    } else {
        "A moderator decided."
    };
    let kept = match retention.moderation {
        _ if outcome.attachment().is_some() => format!(
            "Nobody else can see the {} now. This notice is kept until the review period ends, then deleted.",
            outcome.attachment().unwrap_or("picture")
        ),
        _ if outcome.about_a_picture() => {
            "Nobody else can see the picture now; your messages show the avatar you chose from our set. This notice is kept until the review period ends, then deleted."
                .to_owned()
        }
        Keep::None => "This notice is kept until the review period ends, then deleted.".to_owned(),
        k => format!(
            "The message, this notice and any review are kept for {}, then deleted.",
            k.words()
        ),
    };
    format!(
        "{done} because {ground} ({}). {who} You can ask for a review within {REVIEW_DAYS} days. {kept}",
        reason.words().to_lowercase()
    )
}

/// What a chatter reads when the picture they uploaded waited so long for a decision that the chat
/// deleted it (ADR-0254). It is not a judgement of the picture.
pub fn avatar_expired(station: &str, days: u32) -> String {
    format!(
        "{station} deleted the picture you uploaded because nobody had decided on it after {days} days. \
         That is not a judgement of the picture: you can upload it again. Your messages show the avatar \
         you chose from our set. An automatic step did this, with no moderator involved. You can ask for \
         a review within {REVIEW_DAYS} days. This notice is kept until the review period ends, then deleted."
    )
}

/// What a chatter reads when a picture or document they posted waited so long for a decision that
/// the chat deleted it, with its message (ADR-0254, ADR-0262).
pub fn attachment_expired(station: &str, days: u32, document: bool) -> String {
    let what = if document { "document" } else { "picture" };
    format!(
        "{station} deleted the {what} you posted, and the message with it, because nobody had decided on it after {days} days. \
         That is not a judgement of the {what}: you can post it again. An automatic step did this, with no moderator \
         involved. You can ask for a review within {REVIEW_DAYS} days. This notice is kept until the review period ends, then deleted."
    )
}

/// The first `n` characters of a message, with an ellipsis when cut.
pub fn excerpt(text: &str, n: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n {
        t
    } else {
        format!("{}…", t.chars().take(n).collect::<String>().trim_end())
    }
}

/// What the chat's mode means, in one sentence (ADR-0035: the setup says it plainly).
pub fn mode_words(comments_only: bool) -> &'static str {
    if comments_only {
        "Comments and reactions only: listeners comment on the show and react, but cannot reply to each other."
    } else {
        "Full chat: listeners talk to the studio and to each other, with replies and mentions."
    }
}

/// How long things are kept, one sentence each, for the chat's rules.
pub fn retention_words(r: &Retention) -> (String, String, String) {
    let messages = match r.messages {
        Keep::None => "Messages are deleted when the show ends.".to_owned(),
        k => format!("Messages are kept for {}.", k.words()),
    };
    let moderation = match r.moderation {
        Keep::None => format!(
            "Reports and moderation records are kept for {REVIEW_DAYS} days, so a decision can be reviewed."
        ),
        k => format!("Reports and moderation records are kept for {}.", k.words()),
    };
    let addresses = match r.addresses {
        Keep::None => "IP addresses are not kept.".to_owned(),
        k => format!("IP addresses are kept for {}.", k.words()),
    };
    (messages, moderation, addresses)
}

/// A template for the station's own risk assessment (the UK Act's illegal-content risk
/// assessment and children's access assessment), to start from, not an answer.
pub const RISK_ASSESSMENT_TEMPLATE: &str = "# Chat risk assessment: {station}

Started from WeaveSignal's template on {date}. This is a starting point, not legal advice.
Review it at least once a year and whenever the chat changes.

## 1. The service
- What the chat is: a live chat beside the station's player. Mode: {mode}
- Who can post: signed-in chatters (light accounts){guests}. Reading needs no account.
- Accountable person: {accountable}
- Contact point: {contact}

## 2. Who uses it
- Expected audience and numbers:
- Minimum age chatters declare: {min_age}. Is the chat likely to be used by children? Why or why not:
- If children are likely users, a children's risk assessment is also needed.

## 3. Illegal content risks
For each kind, rate the likelihood and impact on this chat, and say why.
| Kind | Likelihood | Impact | Why |
|---|---|---|---|
| Threats, harassment, hate | | | |
| Terrorism | | | |
| Child sexual exploitation | | | |
| Intimate images shared without consent | | | |
| Fraud and scams | | | |
| Other: | | | |

## 4. Measures in place
- Word filter (rustrict), a rate limit, a 10-minute account age, links for trusted chatters only.
- Slow mode, the hold for new accounts, and influx mode, one key away.
- Reports on every message and user; notices from people without an account.
- A statement of reasons to each chatter acted on, with {review} days to ask for a review.
- Evidence preserved on escalation; intimate images taken down within 48 hours.
- Retention: {retention}
- Other measures the station takes:

## 5. Who does what
- Who moderates during each show:
- Who answers notices, and how fast:
- Who tells the police about a threat to life:

## 6. Decisions and review
- Residual risks and what the station accepts:
- Next review date:
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_statement_names_the_decision_the_ground_automation_and_redress() {
        let r = Retention::default();
        let s = statement(
            "Kettle FM",
            "The Kettle Phone-In",
            Some("three buses a night, what would that cost, 400k a year? and more words here"),
            Outcome::Hidden,
            Reason::Abuse,
            true,
            &r,
        );
        assert!(s.starts_with("Kettle FM hid your message “three buses a night"));
        assert!(s.contains("in The Kettle Phone-In"));
        assert!(s.contains("because it broke our chat rules (abuse or harassment)"));
        assert!(s.contains("An automatic filter flagged it and a moderator decided."));
        assert!(s.contains("ask for a review within 14 days"));
        // No retention by default (ADR-0113): never claim twelve months.
        assert!(!s.contains("12 months"));
        assert!(!s.to_lowercase().contains("compliant"));
    }

    #[test]
    fn the_retention_sentence_follows_the_setting() {
        let r = Retention {
            moderation: Keep::Days(365),
            ..Retention::default()
        };
        let s = statement(
            "Kettle FM",
            "",
            None,
            Outcome::Banned,
            Reason::Illegal,
            false,
            &r,
        );
        assert_eq!(
            s,
            "Kettle FM banned you from its chat because we believe it is unlawful (illegal content). A moderator decided. You can ask for a review within 14 days. The message, this notice and any review are kept for 12 months, then deleted."
        );
    }

    #[test]
    fn a_posted_picture_or_document_is_named_as_what_it_is() {
        let r = Retention::default();
        let s = statement(
            "Kettle FM",
            "The Kettle Phone-In",
            None,
            Outcome::AttachmentRefused { document: true },
            Reason::Spam,
            false,
            &r,
        );
        assert!(
            s.starts_with("Kettle FM did not show the document you posted in The Kettle Phone-In"),
            "{s}"
        );
        assert!(s.contains("Nobody else can see the document now."));
        assert!(!s.contains("avatar"));
        let s = statement(
            "Kettle FM",
            "",
            None,
            Outcome::AttachmentRemoved { document: false },
            Reason::SuspectedCsam,
            false,
            &r,
        );
        assert_eq!(
            s,
            "Kettle FM removed the picture you posted for breaking the station's rules on illegal content. A moderator decided. You can ask for a review within 14 days. This notice is kept until the review period ends, then deleted."
        );
        let e = attachment_expired("Kettle FM", 30, false);
        assert!(e.contains("deleted the picture you posted") && e.contains("30 days"));
    }

    #[test]
    fn reasons_round_trip_and_only_intimate_images_have_a_clock() {
        for r in Reason::ALL {
            assert_eq!(Reason::parse(r.as_str()), Some(r));
            assert!(!r.words().is_empty() && !r.hint().is_empty());
        }
        assert_eq!(Reason::IntimateImage.due_hours(), Some(48));
        assert_eq!(Reason::Abuse.due_hours(), None);
    }

    #[test]
    fn the_csam_reason_is_a_moderators_for_a_picture_and_keeps_its_evidence() {
        // ADR-0257: not on the report form, not parsed from the public's words, but kept at once.
        let r = Reason::SuspectedCsam;
        assert!(!Reason::ALL.contains(&r));
        assert_eq!(Reason::parse("suspected_csam"), None);
        assert!(r.staff_only() && r.keeps_evidence() && r.preserve_at_once() && r.unlawful());
        assert!(Reason::STAFF.contains(&r));
        assert!(!Reason::ChildSafety.staff_only() && !Reason::ChildSafety.keeps_evidence());
        assert_eq!(r.as_str(), "suspected_csam");
        let said = statement(
            "Kettle FM",
            "",
            None,
            Outcome::AvatarRefused,
            r,
            false,
            &Retention::default(),
        );
        // Told less (the operator, 2026-11-10): the rules on illegal content and the review,
        // nothing about the reason's words, keeping or reporting.
        assert!(
            said.starts_with("Kettle FM removed the picture you uploaded for breaking the station's rules on illegal content."),
            "{said}"
        );
        assert!(said.contains("ask for a review within 14 days"), "{said}");
        for never in [
            "child",
            "keep the picture",
            "at least a year",
            "authorities",
        ] {
            assert!(!said.contains(never), "{never}: {said}");
        }
        let other = statement(
            "Kettle FM",
            "",
            None,
            Outcome::AvatarRefused,
            Reason::Abuse,
            false,
            &Retention::default(),
        );
        assert!(!other.contains("at least a year"), "{other}");
    }

    #[test]
    fn excerpts_cut_on_characters() {
        assert_eq!(excerpt("short", 60), "short");
        assert_eq!(excerpt("ab cd ef", 5), "ab cd…");
        assert_eq!(excerpt("ééééé", 3), "ééé…");
    }
}
