// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Retention: how long each kind of record is kept, set by one Jurisdiction drop-down
//! (ADR-0113), from fourteen presets whose periods each carry the rule they follow
//! (ADR-0114, recce 27's proposed presets table as of 2026-10-25), with anonymous counts kept
//! under every preset (ADR-0115), holds on complaint (ADR-0116), and consent kept as long as
//! the recordings it covers plus 42 days (ADR-0107).
//!
//! The presets are data in this file, not rows: they change with the law, and a change of law
//! changes a preset in a release note, never silently (ADR-0114). A station's choice is a row
//! in `retention_setting` (migration 0005); no row means No retention, the default.
//!
//! The sweeper ([`sweep`]) deletes what has expired and is not held: recordings (the file,
//! then the row once no consent needs it) and callers' consent. The other record kinds have
//! periods here but no table in this release (the chat, the per-play log and the access logs
//! arrive with their own features); each joins the sweep as one more step when it lands.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{ConsentRecord, StationId};

/// The date of recce 27's presets table these presets follow.
pub const PRESETS_AS_OF: &str = "2026-10-25";

/// The longest period a station can set, in days (100 years).
pub const DAYS_MAX: u32 = 36_500;

/// How long a caller's pass can live, so the latest a call can end after consent (a day).
pub const CALL_LONGEST_DAYS: u32 = 1;

/// The kinds of record a preset sets a period for (ADR-0113's list, recce 27's columns).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Programme recordings and the broadcast logger ("Logger").
    ProgrammeRecordings,
    /// The per-play music log (PPL, SoundExchange, Re:Sound).
    PerPlayLog,
    ChatMessages,
    /// Reports, the moderation log and statements of reasons (ADR-0039).
    ModerationRecords,
    /// Callers' consent to record (ADR-0107).
    Consent,
    /// The screener's off-air call audio: 0 in every preset (recce 27).
    CallRecordings,
    /// Anonymous listening totals: listener hours and sessions (ADR-0115).
    ListenerStatistics,
    /// Raw server access logs with IP addresses.
    AccessLogs,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::ProgrammeRecordings,
        Kind::PerPlayLog,
        Kind::ChatMessages,
        Kind::ModerationRecords,
        Kind::Consent,
        Kind::CallRecordings,
        Kind::ListenerStatistics,
        Kind::AccessLogs,
    ];

    fn index(self) -> usize {
        Kind::ALL.iter().position(|k| *k == self).expect("in ALL")
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::ProgrammeRecordings => "programme_recordings",
            Kind::PerPlayLog => "per_play_log",
            Kind::ChatMessages => "chat_messages",
            Kind::ModerationRecords => "moderation_records",
            Kind::Consent => "consent",
            Kind::CallRecordings => "call_recordings",
            Kind::ListenerStatistics => "listener_statistics",
            Kind::AccessLogs => "access_logs",
        }
    }

    /// The row's name on the settings screen.
    pub fn label(self) -> &'static str {
        match self {
            Kind::ProgrammeRecordings => "Programme recordings and logger",
            Kind::PerPlayLog => "Per-play music log",
            Kind::ChatMessages => "Chat messages",
            Kind::ModerationRecords => "Moderation records",
            Kind::Consent => "Callers’ consent",
            Kind::CallRecordings => "Off-air call audio",
            Kind::ListenerStatistics => "Listener statistics",
            Kind::AccessLogs => "Server access logs (IP addresses)",
        }
    }

    /// Whether this release keeps records of the kind, so the sweeper acts on it.
    pub fn swept(self) -> bool {
        matches!(
            self,
            Kind::ProgrammeRecordings | Kind::Consent | Kind::CallRecordings
        )
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// How long one kind of record is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "keep", rename_all = "snake_case", deny_unknown_fields)]
pub enum Period {
    /// A number of days after the record is complete (a recording ended, a call over); 0 is
    /// "not kept after the show".
    Days { days: u32 },
    /// Kept with its programme's archive and removed with it (ADR-0039's "A").
    WithArchive,
    /// Consent: kept while any recording it covers exists, then `days` more (ADR-0107's
    /// "R+42"); until a recording is linked, it follows the logger's period.
    AfterRecordings { days: u32 },
    /// Consent under No retention: it goes when the call's last trace does (ADR-0113).
    WithCall,
    /// Anonymous counts only, with nothing that identifies a person (ADR-0115).
    NoLimit,
}

impl Period {
    pub const fn days(days: u32) -> Self {
        Period::Days { days }
    }

    /// The period as days for comparing with a floor: the archive's for "with the archive",
    /// and the longest possible for what has no end.
    pub fn floor_days(self, logger: Period) -> u32 {
        match self {
            Period::Days { days } => days,
            Period::WithArchive => match logger {
                Period::Days { days } => days,
                _ => u32::MAX,
            },
            Period::AfterRecordings { .. } | Period::NoLimit => u32::MAX,
            Period::WithCall => 0,
        }
    }

    /// Whether a station may give this kind this period under Custom.
    pub fn allowed_for(self, kind: Kind) -> Result<(), String> {
        let ok = match (kind, self) {
            (_, Period::Days { days }) | (_, Period::AfterRecordings { days })
                if days > DAYS_MAX =>
            {
                return Err(format!("{}: at most {DAYS_MAX} days", kind.label()));
            }
            (Kind::Consent, Period::AfterRecordings { .. } | Period::WithCall) => true,
            // Consent proof is needed while the recording exists (GDPR Art 7(1)), so no period
            // can cut it short of the recordings it covers.
            (Kind::Consent, _) => false,
            // Anonymous counts stay under every preset (ADR-0115).
            (Kind::ListenerStatistics, Period::NoLimit) => true,
            (Kind::ListenerStatistics, _) => false,
            (Kind::ChatMessages, Period::WithArchive | Period::Days { .. }) => true,
            (_, Period::Days { .. }) => true,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(format!(
                "{} cannot be kept that way ({self:?})",
                kind.label()
            ))
        }
    }
}

/// A period for each kind of record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Periods {
    pub programme_recordings: Period,
    pub per_play_log: Period,
    pub chat_messages: Period,
    pub moderation_records: Period,
    pub consent: Period,
    pub call_recordings: Period,
    pub listener_statistics: Period,
    pub access_logs: Period,
}

impl Periods {
    pub fn get(&self, kind: Kind) -> Period {
        match kind {
            Kind::ProgrammeRecordings => self.programme_recordings,
            Kind::PerPlayLog => self.per_play_log,
            Kind::ChatMessages => self.chat_messages,
            Kind::ModerationRecords => self.moderation_records,
            Kind::Consent => self.consent,
            Kind::CallRecordings => self.call_recordings,
            Kind::ListenerStatistics => self.listener_statistics,
            Kind::AccessLogs => self.access_logs,
        }
    }

    fn from_cells(cells: &[Cell; 8]) -> Self {
        let p = |k: Kind| cells[k.index()].period;
        Periods {
            programme_recordings: p(Kind::ProgrammeRecordings),
            per_play_log: p(Kind::PerPlayLog),
            chat_messages: p(Kind::ChatMessages),
            moderation_records: p(Kind::ModerationRecords),
            consent: p(Kind::Consent),
            call_recordings: p(Kind::CallRecordings),
            listener_statistics: p(Kind::ListenerStatistics),
            access_logs: p(Kind::AccessLogs),
        }
    }

    /// Check every period is one the kind can take under Custom.
    pub fn check(&self) -> Result<(), String> {
        for k in Kind::ALL {
            self.get(k).allowed_for(k)?;
        }
        Ok(())
    }
}

/// The least a rule requires, in days, and the rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Floor {
    pub days: u32,
    pub rule: &'static str,
    /// When the floor applies, where it applies only in some cases (ADR-0155); `None` when it
    /// always does.
    pub condition: Option<&'static str>,
}

/// One period of a preset, with the rule it follows and, where a rule sets one, its floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Cell {
    pub period: Period,
    pub source: &'static str,
    pub floor: Option<Floor>,
}

/// What a rule says about keeping records longer once a complaint arrives (ADR-0116).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ComplaintHold {
    /// The extended period from the broadcast, where the rule fixes one.
    pub days: Option<u32>,
    pub rule: &'static str,
}

/// A Jurisdiction preset (ADR-0114).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Preset {
    pub key: &'static str,
    pub name: &'static str,
    /// The rule each period follows, in a line (recce 27's last column).
    pub summary: &'static str,
    /// For a station that holds a broadcast licence.
    pub licensed: bool,
    #[serde(skip)]
    pub cells: [Cell; 8],
    pub complaint_hold: Option<ComplaintHold>,
    /// The age a chatter declares they are at least, for this jurisdiction (ADR-0148, recce 29):
    /// the age below which a person cannot consent alone to the chat processing their data. 13
    /// in the UK, the US and Canada (UK GDPR Art. 8, COPPA, the OPC's guidance), 16 in Ireland,
    /// Germany and the EU preset (GDPR Art. 8's default, safe in every member state), 15 in
    /// Australia (the OAIC's presumption); 13 under No retention and Custom. A station can
    /// set its own (13 to 21) in the chat's settings.
    pub chat_min_age: u8,
}

impl Preset {
    pub fn cell(&self, kind: Kind) -> &Cell {
        &self.cells[kind.index()]
    }

    pub fn periods(&self) -> Periods {
        Periods::from_cells(&self.cells)
    }
}

/// The key of the default preset.
pub const NO_RETENTION: &str = "none";
/// The key of Custom: every period set by the station.
pub const CUSTOM: &str = "custom";

const fn c(period: Period, source: &'static str) -> Cell {
    Cell {
        period,
        source,
        floor: None,
    }
}

const fn f(period: Period, source: &'static str, days: u32, rule: &'static str) -> Cell {
    Cell {
        period,
        source,
        floor: Some(Floor {
            days,
            rule,
            condition: None,
        }),
    }
}

/// A cell whose floor applies only in some cases (ADR-0155); its warning says when.
const fn fc(
    period: Period,
    source: &'static str,
    days: u32,
    rule: &'static str,
    condition: &'static str,
) -> Cell {
    Cell {
        period,
        source,
        floor: Some(Floor {
            days,
            rule,
            condition: Some(condition),
        }),
    }
}

const D0: Period = Period::days(0);
const D7: Period = Period::days(7);
const D28: Period = Period::days(28);
const D30: Period = Period::days(30);
const D42: Period = Period::days(42);
const D90: Period = Period::days(90);
const D365: Period = Period::days(365);
const D1096: Period = Period::days(1096);
const D1461: Period = Period::days(1461);
const D2192: Period = Period::days(2192);
const R42: Period = Period::AfterRecordings { days: 42 };

// The cells most presets share.
const CHAT_A: Cell = c(
    Period::WithArchive,
    "Kept with the programme's archive and removed with it (ADR-0039); no law sets a period for chat messages.",
);
const MOD_365: Cell = c(
    D365,
    "12 months (ADR-0039), which clears the EU DSA's six months for complaint decisions (Art 20).",
);
const CONSENT_R42: Cell = c(
    R42,
    "While any recording of the call exists, plus 42 days (ADR-0107); GDPR Art 7(1) needs consent to be demonstrable while the recording exists.",
);
const CALL_0: Cell = c(
    D0,
    "Never kept: nothing requires the screener's off-air audio, and German law (StGB § 201) makes it a risk.",
);
const STATS: Cell = c(
    Period::NoLimit,
    "Anonymous totals only, with no IP addresses or names (ADR-0115); PPL, SoundExchange and Re:Sound ask for listener hours.",
);
const IP_30: Cell = c(
    D30,
    "30 days, as ADR-0039 keeps chat IP addresses; no law sets a minimum, and storage limitation sets the ceiling.",
);
const PLAY_1096: Cell = c(
    D1096,
    "3 years (1,096 days, with a leap day) for music-licence queries; no statutory period found.",
);
const US_MOD: Cell = fc(
    D365,
    "12 months (ADR-0039); a report made to the CyberTipline is preserved 1 year.",
    365,
    "18 U.S.C. 2258A(h)(1): a CyberTipline report's contents, 1 year after submission.",
    "the station has made a report to the CyberTipline",
);
// The Australian presets (ADR-0151): 365 days like the rest, with the note.
const AU_MOD: Cell = c(
    D365,
    "12 months (ADR-0039). eSafety's Basic Online Safety Expectations ask 5 years for reports; that is an expectation, not a duty, and its reach to a station chat is untested. Choose Custom to keep reports longer.",
);
const US_PLAY_RULE: &str = "37 CFR 370.4(d)(6): each Report of Use kept 3 years; payment records the prior 3 calendar years (380.4(c)).";

/// The fourteen presets in drop-down order, Custom last (ADR-0114).
pub static PRESETS: [Preset; 14] = [
    Preset {
        key: NO_RETENTION,
        name: "No retention",
        summary: "Nothing is kept after a show except anonymous counts. Does not meet a licensed broadcaster's logger duty (42, 90, 28 or 60 days), a US statutory licensee's 3 years of Reports of Use, or a noncommercial educational webcaster's 3 calendar years of server logs, unless the station keeps those outside WeaveSignal.",
        licensed: false,
        cells: [
            c(D0, "Not kept after the show (ADR-0113)."),
            c(
                D0,
                "Not kept (ADR-0113). A US statutory licensee must keep its Reports of Use 3 years outside WeaveSignal (37 CFR 370.4(d)(6)).",
            ),
            c(D0, "Not kept after the show (ADR-0113)."),
            c(D0, "Not kept after the show (ADR-0113)."),
            c(
                Period::WithCall,
                "Goes when the call's last trace does (ADR-0113).",
            ),
            CALL_0,
            STATS,
            c(D0, "Not kept: raw IP logs follow the preset (ADR-0115)."),
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
    Preset {
        key: "uk_ofcom",
        name: "UK, Ofcom-licensed",
        summary: "Licence condition 7(2)(a) or 8(2)(a): 42 days of every programme. Per-play log 3 years for PPL and PRS queries.",
        licensed: true,
        cells: [
            f(
                D42,
                "42 days of every programme: Ofcom licence condition 7(2)(a) (DAB) or 8(2)(a) (community radio).",
                42,
                "Ofcom licence condition 7(2)(a) or 8(2)(a): 42 days (Ofcom bulletins 468 and 553).",
            ),
            c(
                D1096,
                "3 years for PPL and PRS queries; PPL's webcast terms set no period.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
    Preset {
        key: "uk_internet",
        name: "UK, internet-only",
        summary: "No logger duty; 42 days matches Ofcom practice and consent's 42 days. Online Safety Act risk assessments are kept 3 years as documents.",
        licensed: false,
        cells: [
            c(
                D42,
                "No logger duty for an internet-only station; 42 days matches Ofcom practice and ADR-0107. The ICO: keep nothing \"just in case\".",
            ),
            c(
                D1096,
                "3 years for PPL and PRS queries; PPL's webcast terms set no period.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
    Preset {
        key: "us_webcaster",
        name: "US webcaster (statutory licence)",
        summary: "Reports of Use and payment records 3 years (37 CFR 370.4(d)(6), 380.4(c)). Recordings kept short in case the 6-month cap on ephemeral copies applies (open).",
        licensed: false,
        cells: [
            c(
                D42,
                "No FCC rule keeps programme audio; kept short in case 17 U.S.C. 112(e)'s 6-month cap on ephemeral copies applies (an open question).",
            ),
            f(
                D1096,
                "3 years: Reports of Use (37 CFR 370.4(d)(6)) and payment records (380.4(c)).",
                1096,
                US_PLAY_RULE,
            ),
            CHAT_A,
            US_MOD,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 13,
        complaint_hold: Some(ComplaintHold {
            days: None,
            rule: "FCC-licensed stations keep station logs until a complaint is fully satisfied or an investigation ends (47 CFR 73.1840(a)).",
        }),
    },
    Preset {
        key: "us_nce",
        name: "US noncommercial educational webcaster",
        summary: "Server logs 3 full calendar years (37 CFR 380.22(e)), so up to 4 years from a year's first entry; Reports of Use 3 years.",
        licensed: false,
        cells: [
            c(
                D42,
                "No FCC rule keeps programme audio; kept short in case 17 U.S.C. 112(e)'s 6-month cap on ephemeral copies applies (an open question).",
            ),
            f(
                D1461,
                "4 years (1,461 days): Reports of Use 3 years, kept as long as the server logs that support them.",
                1096,
                US_PLAY_RULE,
            ),
            CHAT_A,
            US_MOD,
            CONSENT_R42,
            CALL_0,
            STATS,
            f(
                D1461,
                "3 full calendar years after the year a log entry falls in, so up to 4 years (1,461 days).",
                1461,
                "37 CFR 380.22(e): server logs kept no less than three full calendar years.",
            ),
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
    Preset {
        key: "eu_gdpr",
        name: "EU, GDPR only",
        summary: "No floors; storage limitation (GDPR Art 5(1)(e)). The DSA's Art 20, where it applies, needs 6 months of complaint decisions.",
        licensed: false,
        cells: [
            c(
                D42,
                "No logger duty; storage limitation (GDPR Art 5(1)(e)); 42 days matches consent's 42 days.",
            ),
            PLAY_1096,
            CHAT_A,
            fc(
                D365,
                "12 months (ADR-0039); the DSA's complaint decisions need at least 6 months where Art 20 applies.",
                183,
                "DSA Art 20: complaint decisions kept at least 6 months (not for micro or small platforms, Art 19).",
                "the platform is larger than a small enterprise",
            ),
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 16,
        complaint_hold: None,
    },
    Preset {
        key: "ie_licensed",
        name: "Ireland, licensed",
        summary: "Broadcasting Act 2009 s.46P and Coimisiún na Meán's contracts: 90 days of programme material. Per-play 6 years, as IMRO keeps licensee data.",
        licensed: true,
        cells: [
            f(
                D90,
                "90 days of all programme material: Broadcasting Act 2009 s.46P; Coimisiún na Meán's broadcasting contracts.",
                90,
                "Broadcasting Act 2009 s.46P; Coimisiún na Meán: \"The current retention period … is 90 days\".",
            ),
            c(
                D2192,
                "6 years (2,192 days): IMRO keeps licensee data for the term plus six years.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 16,
        complaint_hold: Some(ComplaintHold {
            days: None,
            rule: "Coimisiún na Meán keeps complaint records 2 years; keep what a complaint concerns until it is closed.",
        }),
    },
    Preset {
        key: "ie_internet",
        name: "Ireland, internet-only (audio)",
        summary: "No logger duty for internet audio (Broadcasting Act 2009 s.2, as amended in 2022). Choose Ireland, licensed, if the station runs scheduled video.",
        licensed: false,
        cells: [
            c(
                D42,
                "No logger duty: internet audio is not a \"broadcasting service\" (Broadcasting Act 2009 s.2, as amended in 2022).",
            ),
            c(
                D2192,
                "6 years (2,192 days): IMRO keeps licensee data for the term plus six years.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 16,
        complaint_hold: None,
    },
    Preset {
        key: "de",
        name: "Germany",
        summary: "State media laws: Bavaria 2 months, North Rhine-Westphalia 3 months (the longest found), applied to licence-free stations as a precaution. IP addresses 7 days.",
        licensed: true,
        cells: [
            f(
                D90,
                "90 days: North Rhine-Westphalia's 3 months (LMG NRW § 43), the longest found; Bavaria 2 months (BayMG Art. 29(2)).",
                90,
                "LMG NRW § 43: three months after the day of broadcast; BayMG Art. 29(2): two months.",
            ),
            c(
                D1096,
                "3 years; no statutory period (VGG §§ 41–42; GEMA's Tarif Radio has no retention clause).",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            c(
                D7,
                "7 days, as the Federal Court of Justice accepted for an access provider (BGH III ZR 391/13); IP addresses are personal data (CJEU C-582/14).",
            ),
        ],
        chat_min_age: 16,
        complaint_hold: Some(ComplaintHold {
            days: None,
            rule: "The state laws keep a recording longer while a complaint is open (LMG NRW § 43; BayMG Art. 29(2)).",
        }),
    },
    Preset {
        key: "ca_licensed",
        name: "Canada, licensed",
        summary: "Radio Regulations, 1986 s.8: 28 days, 56 on a CRTC complaint. Re:Sound's Tariff 8 records 6 years.",
        licensed: true,
        cells: [
            f(
                D28,
                "28 days (\"four weeks\"): Radio Regulations, 1986 s.8(1)(b) and 8(5)(a); the program log too.",
                28,
                "Radio Regulations, 1986 s.8(5)(a): four weeks.",
            ),
            f(
                D2192,
                "6 years (2,192 days): Re:Sound's Tariff 8 keeps records six years after the year they relate to.",
                2192,
                "Re:Sound Tariff 8: records kept six years after the end of the year to which they relate.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 13,
        complaint_hold: Some(ComplaintHold {
            days: Some(56),
            rule: "Radio Regulations, 1986 s.8(5)(b): 56 days from broadcast when the CRTC notifies a complaint within the four weeks.",
        }),
    },
    Preset {
        key: "ca_internet",
        name: "Canada, internet-only",
        summary: "No logger duty: an online undertaking runs without a licence (Broadcasting Act s.31.1(2)). Re:Sound six years if licensed by it.",
        licensed: false,
        cells: [
            c(
                D28,
                "No logger duty: an online undertaking runs without a licence (Broadcasting Act s.31.1(2)); 28 days as licensed stations keep.",
            ),
            f(
                D2192,
                "6 years (2,192 days), if Re:Sound licenses the station (Tariff 8).",
                2192,
                "Re:Sound Tariff 8, if it licenses the station: records kept six years.",
            ),
            CHAT_A,
            MOD_365,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
    Preset {
        key: "au_licensed",
        name: "Australia, licensed",
        summary: "Broadcasting Services Act 1992 Sch 2 cl 5: 42 days for political or current-affairs matter, 60 on complaint. Reports 365 days; eSafety expects 5 years, and Custom can extend it.",
        licensed: true,
        cells: [
            f(
                D42,
                "42 days for political or current-affairs matter: Broadcasting Services Act 1992 Sch 2 cl 5.",
                42,
                "Broadcasting Services Act 1992 Sch 2 cl 5: 42 days.",
            ),
            c(
                D1096,
                "3 years; no number found (APRA AMCOS asks for music reports each quarter).",
            ),
            CHAT_A,
            AU_MOD,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 15,
        complaint_hold: Some(ComplaintHold {
            days: Some(60),
            rule: "Broadcasting Services Act 1992 Sch 2 cl 5(3)(b) and the Community Radio Codes cl 10.6: 60 days on complaint.",
        }),
    },
    Preset {
        key: "au_internet",
        name: "Australia, internet-only",
        summary: "No logger duty: internet radio is excluded from the Broadcasting Services Act until 2027-09-14 (Exclusion Determination 2022). Reports 365 days; eSafety expects 5 years, and Custom can extend it.",
        licensed: false,
        cells: [
            c(
                D42,
                "No logger duty until 2027-09-14: radio using the internet is excluded (Exclusion Determination 2022 s.6).",
            ),
            c(
                D1096,
                "3 years; no number found (APRA AMCOS asks for music reports each quarter).",
            ),
            CHAT_A,
            AU_MOD,
            CONSENT_R42,
            CALL_0,
            STATS,
            IP_30,
        ],
        chat_min_age: 15,
        complaint_hold: None,
    },
    Preset {
        key: CUSTOM,
        name: "Custom",
        summary: "Every period set by the station. The last preset's floors become warnings; nothing is blocked.",
        licensed: false,
        // Custom's own cells are placeholders: a Custom setting carries its periods. They keep
        // everything as long as a station can set, so a Custom row that cannot be read never
        // reads as "delete it all" (the sweep also refuses to run on one).
        cells: [
            c(Period::days(DAYS_MAX), "Set by the station."),
            c(Period::days(DAYS_MAX), "Set by the station."),
            c(Period::days(DAYS_MAX), "Set by the station."),
            c(Period::days(DAYS_MAX), "Set by the station."),
            c(
                Period::AfterRecordings { days: DAYS_MAX },
                "Set by the station.",
            ),
            c(Period::days(DAYS_MAX), "Set by the station."),
            c(Period::NoLimit, "Set by the station."),
            c(Period::days(DAYS_MAX), "Set by the station."),
        ],
        chat_min_age: 13,
        complaint_hold: None,
    },
];

/// A preset by its key.
pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == key)
}

/// A station's retention setting, as stored (no row: No retention).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Setting {
    pub preset: String,
    /// Under Custom, the preset whose floors are kept as warnings.
    pub based_on: Option<String>,
    /// Under Custom, every period.
    pub custom: Option<Periods>,
    pub broadcast_licence: Option<bool>,
    pub us_statutory_licence: Option<bool>,
    pub updated_by: Option<String>,
    pub updated_at_ms: Option<u64>,
}

impl Default for Setting {
    fn default() -> Self {
        Self {
            preset: NO_RETENTION.to_owned(),
            based_on: None,
            custom: None,
            broadcast_licence: None,
            us_statutory_licence: None,
            updated_by: None,
            updated_at_ms: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingError {
    #[error("`{0}` is not a Jurisdiction preset")]
    UnknownPreset(String),
    #[error("Custom needs a period for every kind of record")]
    CustomWithoutPeriods,
    #[error("only Custom takes periods; a preset's periods are its own")]
    PeriodsWithPreset,
    #[error("{0}")]
    BadPeriod(String),
    #[error("say who is making the change")]
    NoWho,
}

impl Setting {
    /// Check the setting can be stored.
    pub fn check(&self) -> Result<(), SettingError> {
        if preset(&self.preset).is_none() {
            return Err(SettingError::UnknownPreset(self.preset.clone()));
        }
        if let Some(b) = &self.based_on
            && (b == CUSTOM || preset(b).is_none())
        {
            return Err(SettingError::UnknownPreset(b.clone()));
        }
        match (&*self.preset, &self.custom) {
            (CUSTOM, None) => Err(SettingError::CustomWithoutPeriods),
            (CUSTOM, Some(p)) => p.check().map_err(SettingError::BadPeriod),
            (_, Some(_)) => Err(SettingError::PeriodsWithPreset),
            _ => Ok(()),
        }
    }

    /// The periods in force.
    pub fn periods(&self) -> Periods {
        match (&self.custom, preset(&self.preset)) {
            (Some(p), _) if self.preset == CUSTOM => *p,
            (_, Some(p)) => p.periods(),
            _ => PRESETS[0].periods(),
        }
    }

    /// The chat's minimum age for this setting (ADR-0148): the preset's own, so No retention and
    /// Custom give 13 whatever Custom is based on.
    pub fn chat_min_age(&self) -> u8 {
        preset(&self.preset).map_or(13, |p| p.chat_min_age)
    }

    /// The preset whose rules apply: the preset itself, or under Custom the one it was based on.
    pub fn rules(&self) -> &'static Preset {
        let key = if self.preset == CUSTOM {
            self.based_on.as_deref().unwrap_or(NO_RETENTION)
        } else {
            &self.preset
        };
        preset(key).unwrap_or(&PRESETS[0])
    }

    /// What the station should know about this setting: No retention's duties, a licence
    /// answered yes on a preset that does not meet it, and Custom's periods under a floor.
    pub fn warnings(&self) -> Vec<Warning> {
        let mut out = Vec::new();
        let periods = self.periods();
        if self.preset == NO_RETENTION {
            out.push(Warning {
                kind: None,
                level: Level::Serious,
                title: "No retention does not meet a broadcast or music licence".into(),
                message: "A licensed broadcaster must keep a recording of its output (UK 42 days, Ireland 90, Canada 28, Germany up to 90, Australia 42). A US statutory licensee must keep Reports of Use for 3 years, and a noncommercial educational webcaster its server logs for 3 calendar years. Choose your jurisdiction, or keep these outside WeaveSignal.".into(),
                source: "ADR-0113 and ADR-0114; recce 27".into(),
            });
        }
        if self.broadcast_licence == Some(true) && !self.rules().licensed {
            out.push(Warning {
                kind: Some(Kind::ProgrammeRecordings),
                level: Level::Serious,
                title: "This setting is not for a licensed broadcaster".into(),
                message: "You said the station holds a broadcast licence. Choose your licensed jurisdiction, so recordings are kept as long as the licence requires.".into(),
                source: "ADR-0113".into(),
            });
        }
        if self.us_statutory_licence == Some(true) {
            let days = periods
                .per_play_log
                .floor_days(periods.programme_recordings);
            if days < 1096 {
                out.push(Warning {
                    kind: Some(Kind::PerPlayLog),
                    level: Level::Serious,
                    title: "Reports of Use must be kept 3 years".into(),
                    message: format!(
                        "You said the station webcasts under the US statutory licence. The per-play log is kept {}, and SoundExchange's Reports of Use must be kept 3 years. Keep them outside WeaveSignal, or choose a US preset.",
                        days_words(days)
                    ),
                    source: US_PLAY_RULE.into(),
                });
            }
        }
        if self.preset == CUSTOM {
            let rules = self.rules();
            for k in Kind::ALL {
                let Some(floor) = rules.cell(k).floor else {
                    continue;
                };
                let days = periods.get(k).floor_days(periods.programme_recordings);
                if days < floor.days {
                    let when = floor
                        .condition
                        .map(|c| format!(" where {c}"))
                        .unwrap_or_default();
                    out.push(Warning {
                        kind: Some(k),
                        level: Level::Warning,
                        title: format!("{}: under the floor for {}", k.label(), rules.name),
                        message: format!(
                            "Kept {}; {} requires at least {}{when}.",
                            days_words(days),
                            rules.name,
                            days_words(floor.days)
                        ),
                        source: floor.rule.into(),
                    });
                }
            }
        }
        out
    }
}

/// "42 days", "1 day", "not at all".
pub fn days_words(days: u32) -> String {
    match days {
        0 => "not at all after the show".to_owned(),
        1 => "1 day".to_owned(),
        u32::MAX => "without limit".to_owned(),
        n => format!("{n} days"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Warning,
    Serious,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Warning {
    pub kind: Option<Kind>,
    pub level: Level,
    pub title: String,
    pub message: String,
    pub source: String,
}

// ---- the database ----

fn ms(at: Option<f64>) -> Option<u64> {
    at.map(|v| v.max(0.0) as u64)
}

/// The station's setting; No retention when it has none.
pub async fn load_setting(pool: &sqlx::PgPool, station: StationId) -> Result<Setting, sqlx::Error> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<bool>,
        Option<bool>,
        String,
        f64,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT preset, based_on, periods::text, broadcast_licence, us_statutory_licence, updated_by,
                extract(epoch FROM updated_at)::double precision * 1000
         FROM retention_setting WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        None => Setting::default(),
        Some((preset, based_on, periods, broadcast, us, by, at)) => Setting {
            preset,
            based_on,
            // A row the CHECK let in but this release cannot read is taken as Custom with
            // nothing: `check` refuses it, and the screen asks for the periods again.
            custom: periods.and_then(|p| serde_json::from_str(&p).ok()),
            broadcast_licence: broadcast,
            us_statutory_licence: us,
            updated_by: Some(by),
            updated_at_ms: ms(Some(at)),
        },
    })
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Setting(#[from] SettingError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// Store the station's setting, and log the change with who made it.
pub async fn save_setting(
    pool: &sqlx::PgPool,
    station: StationId,
    setting: &Setting,
    by: &str,
) -> Result<Setting, StoreError> {
    setting.check()?;
    let by = by.trim();
    if by.is_empty() || by.chars().count() > 120 {
        return Err(SettingError::NoWho.into());
    }
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO retention_setting
           (station_id, preset, based_on, periods, broadcast_licence, us_statutory_licence, updated_by, updated_at)
         VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7, now())
         ON CONFLICT (station_id) DO UPDATE SET
           preset = EXCLUDED.preset, based_on = EXCLUDED.based_on, periods = EXCLUDED.periods,
           broadcast_licence = EXCLUDED.broadcast_licence,
           us_statutory_licence = EXCLUDED.us_statutory_licence,
           updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(station.as_uuid())
    .bind(&setting.preset)
    .bind(&setting.based_on)
    .bind(
        setting
            .custom
            .map(|p| serde_json::to_string(&p).expect("periods serialise")),
    )
    .bind(setting.broadcast_licence)
    .bind(setting.us_statutory_licence)
    .bind(by)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO retention_change (station_id, preset, periods, changed_by) VALUES ($1, $2, $3::jsonb, $4)",
    )
    .bind(station.as_uuid())
    .bind(&setting.preset)
    .bind(serde_json::to_string(&setting.periods()).expect("periods serialise"))
    .bind(by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(load_setting(pool, station).await?)
}

// ---- recordings and consent ----

/// What kind of recording: the programme (logger, archive) or the screener's off-air audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordingKind {
    Programme,
    Call,
}

impl RecordingKind {
    fn key(self) -> &'static str {
        match self {
            RecordingKind::Programme => "programme",
            RecordingKind::Call => "call",
        }
    }

    fn period(self, p: &Periods) -> Period {
        match self {
            RecordingKind::Programme => p.programme_recordings,
            RecordingKind::Call => p.call_recordings,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewRecording {
    pub kind: RecordingKind,
    #[serde(default)]
    pub label: String,
    /// Relative to the recordings directory.
    #[serde(default)]
    pub file: Option<String>,
    pub started_at_ms: u64,
    #[serde(default)]
    pub ended_at_ms: Option<u64>,
}

/// Whether a path names a file inside the recordings directory and nothing else.
pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 300
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|p| p == ".." || p.is_empty() || p == ".")
}

/// Link every consent whose caller was on air while the recording ran to it (ADR-0153,
/// narrowing ADR-0107): a call that only waited or was screened is not in the recording, so
/// the recording does not keep its consent. A stretch still open runs to the end of the call
/// (a day after consent at the latest, as everywhere else).
const LINK_ON_AIR: &str = "
    INSERT INTO consent_recording (consent_id, recording_id)
    SELECT DISTINCT cc.id, r.id FROM recording r
    JOIN caller_consent cc ON cc.station_id = r.station_id
    JOIN consent_on_air a ON a.consent_id = cc.id
    WHERE r.id = $1 AND r.station_id = $2 AND r.kind = 'programme' AND r.deleted_at IS NULL
      AND a.from_at < coalesce(r.ended_at, 'infinity'::timestamptz)
      AND coalesce(a.to_at, cc.call_ended_at, cc.accepted_at + interval '1 day') > r.started_at
    ON CONFLICT DO NOTHING";

/// Register a recording for retention; a programme recording takes the consent of every caller
/// who was on air during it (when it is added and again when it ends). Returns its identifier.
pub async fn add_recording(
    pool: &sqlx::PgPool,
    station: StationId,
    r: &NewRecording,
) -> Result<i64, sqlx::Error> {
    if let Some(f) = &r.file
        && !safe_relative(f)
    {
        return Err(sqlx::Error::Protocol(format!(
            "`{f}` is not a path inside the recordings directory"
        )));
    }
    let mut tx = pool.begin().await?;
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO recording (station_id, kind, label, file, started_at, ended_at)
         VALUES ($1, $2, $3, $4, to_timestamp($5::double precision / 1000.0),
                 to_timestamp($6::double precision / 1000.0))
         RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(r.kind.key())
    .bind(&r.label)
    .bind(&r.file)
    .bind(r.started_at_ms as f64)
    .bind(r.ended_at_ms.map(|v| v as f64))
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(LINK_ON_AIR)
        .bind(id)
        .bind(station.as_uuid())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// Close an open recording; its period starts now. Returns false if no such open recording.
pub async fn end_recording(
    pool: &sqlx::PgPool,
    station: StationId,
    id: i64,
    ended_at_ms: u64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let done = sqlx::query(
        "UPDATE recording SET ended_at = to_timestamp($3::double precision / 1000.0)
         WHERE id = $1 AND station_id = $2 AND ended_at IS NULL AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(ended_at_ms as f64)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if done {
        sqlx::query(LINK_ON_AIR)
            .bind(id)
            .bind(station.as_uuid())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(done)
}

/// Link a consent to a recording it covers (a podcast episode cut from the call, say).
/// Returns false when either is not the station's or the recording is gone.
pub async fn link_consent(
    pool: &sqlx::PgPool,
    station: StationId,
    consent_id: i64,
    recording_id: i64,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "INSERT INTO consent_recording (consent_id, recording_id)
         SELECT cc.id, r.id FROM caller_consent cc, recording r
         WHERE cc.id = $1 AND r.id = $2 AND cc.station_id = $3 AND r.station_id = $3
           AND r.deleted_at IS NULL
         ON CONFLICT DO NOTHING",
    )
    .bind(consent_id)
    .bind(recording_id)
    .bind(station.as_uuid())
    .execute(pool)
    .await?
    .rows_affected();
    if n == 1 {
        return Ok(true);
    }
    // Already linked counts as linked.
    let (linked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM consent_recording cr JOIN recording r ON r.id = cr.recording_id
                        WHERE cr.consent_id = $1 AND cr.recording_id = $2 AND r.station_id = $3)",
    )
    .bind(consent_id)
    .bind(recording_id)
    .bind(station.as_uuid())
    .fetch_one(pool)
    .await?;
    Ok(linked)
}

/// The call a consent was given for has ended (the caller hung up, or was dropped). A stretch
/// on air still open ends with it.
pub async fn mark_call_ended(
    pool: &sqlx::PgPool,
    station: StationId,
    identity: &str,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE consent_on_air a SET to_at = now() FROM caller_consent cc
         WHERE a.consent_id = cc.id AND a.to_at IS NULL
           AND cc.station_id = $1 AND cc.identity = $2 AND cc.call_ended_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(identity)
    .execute(&mut *tx)
    .await?;
    let n = sqlx::query(
        "UPDATE caller_consent SET call_ended_at = now()
         WHERE station_id = $1 AND identity = $2 AND call_ended_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(identity)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// What [`sync_on_air`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnAirSync {
    /// Stretches on air begun.
    pub opened: u64,
    /// Stretches on air ended.
    pub closed: u64,
    /// Identities on air with no consent of a call still going: nothing to record them
    /// against.
    pub no_consent: Vec<String>,
}

/// Make the record of who is on air match the desk's: `on_air` lists the callers on air now.
/// A caller on air with no open stretch gets one (against the consent of the call still
/// going), begun `lag_ms` before now: the desk is polled, so they may have gone on air up to
/// one poll earlier, and a stretch errs on the side of linking. An open stretch of anyone not
/// listed ends now, but never later than the call's end or a day after consent (the same bound
/// an open stretch has in `LINK_ON_AIR`), so a stretch a crash left open for days does not
/// claim them days on air. Writing the same state again changes nothing, so a restarted control
/// plane can write it at its first poll, which also ends any stretch a crash left open
/// (ADR-0153).
pub async fn sync_on_air(
    pool: &sqlx::PgPool,
    station: StationId,
    on_air: &[String],
    lag_ms: u64,
) -> Result<OnAirSync, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let closed = sqlx::query(
        "UPDATE consent_on_air a
         SET to_at = greatest(a.from_at, least(now(), coalesce(cc.call_ended_at, 'infinity'::timestamptz),
                                               cc.accepted_at + interval '1 day'))
         FROM caller_consent cc
         WHERE a.consent_id = cc.id AND a.to_at IS NULL AND cc.station_id = $1
           AND NOT (cc.identity = ANY($2))",
    )
    .bind(station.as_uuid())
    .bind(on_air)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let opened = sqlx::query(
        "INSERT INTO consent_on_air (consent_id, from_at)
         SELECT DISTINCT ON (cc.identity) cc.id, now() - make_interval(secs => $3::double precision / 1000.0)
         FROM caller_consent cc
         WHERE cc.station_id = $1 AND cc.identity = ANY($2) AND cc.call_ended_at IS NULL
         ORDER BY cc.identity, cc.accepted_at DESC, cc.id DESC
         ON CONFLICT DO NOTHING",
    )
    .bind(station.as_uuid())
    .bind(on_air)
    .bind(lag_ms as f64)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let no_consent: Vec<(String,)> = sqlx::query_as(
        "SELECT i FROM unnest($2::text[]) AS i
         WHERE NOT EXISTS (SELECT 1 FROM caller_consent cc
                           WHERE cc.station_id = $1 AND cc.identity = i AND cc.call_ended_at IS NULL)",
    )
    .bind(station.as_uuid())
    .bind(on_air)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(OnAirSync {
        opened,
        closed,
        no_consent: no_consent.into_iter().map(|(i,)| i).collect(),
    })
}

/// A caller is being put on air (ADR-0153): open a stretch for them now, against the consent of
/// the call still going, unless one is open already. The control plane writes it before it
/// tells the desk, so every switch made through it leaves a stretch that begins no later than
/// the switch, whatever the polls see; the next poll ends it if they are not on air. Returns
/// whether a stretch was opened (false for a stretch already open, or no call going).
pub async fn open_on_air(
    pool: &sqlx::PgPool,
    station: StationId,
    identity: &str,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "INSERT INTO consent_on_air (consent_id)
         SELECT cc.id FROM caller_consent cc
         WHERE cc.station_id = $1 AND cc.identity = $2 AND cc.call_ended_at IS NULL
         ORDER BY cc.accepted_at DESC, cc.id DESC LIMIT 1
         ON CONFLICT DO NOTHING",
    )
    .bind(station.as_uuid())
    .bind(identity)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
}

type ConsentRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    f64,
    Option<uuid::Uuid>,
    String,
    String,
);

fn consent_of_row(r: ConsentRow) -> ConsentRecord {
    let (identity, name, topic, by, words, words_version, at, show_id, presenter, contact) = r;
    ConsentRecord {
        identity,
        name,
        topic,
        given_by: if by == "station" { "station" } else { "caller" },
        words,
        words_version,
        accepted_at_ms: ms(Some(at)).unwrap_or(0),
        show_id,
        presenter,
        contact,
    }
}

const CONSENT_COLUMNS: &str = "identity, name, topic, given_by, words, words_version,
    extract(epoch FROM accepted_at)::double precision * 1000, show_id, presenter, contact";

/// The consents of calls that may still be going: given within the longest a pass lives, and
/// the call not ended. A restarted control plane reads these back, so a waiting caller's
/// rejoin still finds their consent.
pub async fn open_consents(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<ConsentRecord>, sqlx::Error> {
    let rows: Vec<ConsentRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CONSENT_COLUMNS} FROM caller_consent
         WHERE station_id = $1 AND call_ended_at IS NULL
           AND accepted_at > now() - make_interval(days => $2)
         ORDER BY accepted_at"
    )))
    .bind(station.as_uuid())
    .bind(CALL_LONGEST_DAYS as i32)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(consent_of_row).collect())
}

/// The latest consent on file for an identity whose call has not ended.
pub async fn open_consent_of(
    pool: &sqlx::PgPool,
    station: StationId,
    identity: &str,
) -> Result<Option<ConsentRecord>, sqlx::Error> {
    let row: Option<ConsentRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CONSENT_COLUMNS} FROM caller_consent
         WHERE station_id = $1 AND identity = $2 AND call_ended_at IS NULL
           AND accepted_at > now() - make_interval(days => $3)
         ORDER BY accepted_at DESC LIMIT 1"
    )))
    .bind(station.as_uuid())
    .bind(identity)
    .bind(CALL_LONGEST_DAYS as i32)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(consent_of_row))
}

// ---- holds ----

/// What a hold can name one by one in this release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HoldTarget {
    Recording,
    Consent,
}

impl HoldTarget {
    fn key(self) -> &'static str {
        match self {
            HoldTarget::Recording => "recording",
            HoldTarget::Consent => "consent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HoldRecord {
    pub kind: HoldTarget,
    pub id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewHold {
    /// Why: the complaint or report, in a line.
    pub reason: String,
    /// Who places it.
    pub by: String,
    /// A stretch of broadcast time: every recording and consent overlapping it is held.
    #[serde(default)]
    pub from_ms: Option<u64>,
    #[serde(default)]
    pub to_ms: Option<u64>,
    /// Records named one by one.
    #[serde(default)]
    pub records: Vec<HoldRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Hold {
    pub id: i64,
    pub reason: String,
    pub placed_by: String,
    pub placed_at_ms: u64,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub records: Vec<HoldRecord>,
    pub released_by: Option<String>,
    pub released_at_ms: Option<u64>,
    pub release_reason: Option<String>,
    /// The date its latest extension fixed, which replaces the date the complaint rule gives
    /// (ADR-0156), and who extended it, when and why.
    pub extended_until_ms: Option<u64>,
    pub extended_by: Option<String>,
    pub extended_at_ms: Option<u64>,
    pub extend_reason: Option<String>,
    /// What it keeps now: recordings not yet deleted, and consents.
    pub recordings: i64,
    pub consents: i64,
}

impl Hold {
    /// The date until which the hold is kept at least: the date its latest extension fixed
    /// (ADR-0156), or else the one a complaint rule gives, the start of the broadcast it
    /// covers plus the rule's days. `None` when it was never extended and the rule fixes no
    /// period or the hold names no stretch of broadcast time.
    pub fn keep_until_ms(&self, rule: Option<&ComplaintHold>) -> Option<u64> {
        self.extended_until_ms.or_else(|| {
            let days = rule?.days?;
            Some(self.from_ms? + u64::from(days) * 86_400_000)
        })
    }

    /// Whether the hold is still open after that date (ADR-0156). It is flagged and never
    /// released automatically.
    pub fn past_date(&self, rule: Option<&ComplaintHold>, now_ms: u64) -> bool {
        self.released_at_ms.is_none() && self.keep_until_ms(rule).is_some_and(|d| d < now_ms)
    }
}

/// The open holds past their "keep until" date under the setting's rules (ADR-0156), oldest
/// date first. Settings flags them and the start-up log names them.
pub async fn holds_past_date(
    pool: &sqlx::PgPool,
    station: StationId,
    setting: &Setting,
    now_ms: u64,
) -> Result<Vec<(Hold, u64)>, sqlx::Error> {
    let rule = setting.rules().complaint_hold;
    let mut out: Vec<(Hold, u64)> = holds(pool, station)
        .await?
        .into_iter()
        .filter(|h| h.past_date(rule.as_ref(), now_ms))
        .filter_map(|h| {
            let d = h.keep_until_ms(rule.as_ref())?;
            Some((h, d))
        })
        .collect();
    out.sort_by_key(|(h, d)| (*d, h.id));
    Ok(out)
}

#[derive(Debug, thiserror::Error)]
pub enum HoldError {
    #[error("{0}")]
    Invalid(String),
    #[error("no hold {0} at this station")]
    NotFound(i64),
    #[error("hold {0} was already released")]
    Released(i64),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn line(s: &str, what: &str, max: usize) -> Result<String, HoldError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(HoldError::Invalid(format!("{what} is empty")));
    }
    if t.chars().count() > max {
        return Err(HoldError::Invalid(format!(
            "{what} is longer than {max} characters"
        )));
    }
    Ok(t.to_owned())
}

/// Place a hold. The records it covers are locked while it is made, so a sweep deleting one at
/// the same moment either finishes first (and the hold says the record is gone) or waits.
pub async fn place_hold(
    pool: &sqlx::PgPool,
    station: StationId,
    h: &NewHold,
) -> Result<Hold, HoldError> {
    let reason = line(&h.reason, "The reason", 1000)?;
    let by = line(&h.by, "Who is placing the hold", 120)?;
    let window = match (h.from_ms, h.to_ms) {
        (Some(a), Some(b)) if b > a => Some((a as f64, b as f64)),
        (None, None) => None,
        _ => {
            return Err(HoldError::Invalid(
                "A time window needs a start and a later end".into(),
            ));
        }
    };
    if window.is_none() && h.records.is_empty() {
        return Err(HoldError::Invalid(
            "Name the records to hold, or a stretch of broadcast time".into(),
        ));
    }
    let ids = |t: HoldTarget| -> Vec<i64> {
        h.records
            .iter()
            .filter(|r| r.kind == t)
            .map(|r| r.id)
            .collect()
    };
    let (recs, cons) = (ids(HoldTarget::Recording), ids(HoldTarget::Consent));
    let mut tx = pool.begin().await?;
    // Every record named must be the station's and still kept.
    let found: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM recording WHERE station_id = $1 AND id = ANY($2) AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(station.as_uuid())
    .bind(&recs)
    .fetch_all(&mut *tx)
    .await?;
    if found.len() != recs.len() {
        return Err(HoldError::Invalid(
            "A recording named is not this station's, or is already deleted".into(),
        ));
    }
    let found: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM caller_consent WHERE station_id = $1 AND id = ANY($2) FOR UPDATE",
    )
    .bind(station.as_uuid())
    .bind(&cons)
    .fetch_all(&mut *tx)
    .await?;
    if found.len() != cons.len() {
        return Err(HoldError::Invalid(
            "A consent named is not this station's, or is already deleted".into(),
        ));
    }
    if let Some((a, b)) = window {
        sqlx::query(
            "SELECT id FROM recording WHERE station_id = $1 AND deleted_at IS NULL
               AND started_at < to_timestamp($3::double precision / 1000.0)
               AND coalesce(ended_at, 'infinity'::timestamptz) > to_timestamp($2::double precision / 1000.0)
             FOR UPDATE",
        )
        .bind(station.as_uuid())
        .bind(a)
        .bind(b)
        .execute(&mut *tx)
        .await?;
    }
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO retention_hold (station_id, reason, placed_by, window_from, window_to)
         VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000.0), to_timestamp($5::double precision / 1000.0))
         RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(&reason)
    .bind(&by)
    .bind(window.map(|w| w.0))
    .bind(window.map(|w| w.1))
    .fetch_one(&mut *tx)
    .await?;
    for r in &h.records {
        sqlx::query(
            "INSERT INTO retention_hold_record (hold_id, record_kind, record_id) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(r.kind.key())
        .bind(r.id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    hold(pool, station, id).await
}

/// Release a hold: the normal period resumes for what it kept.
pub async fn release_hold(
    pool: &sqlx::PgPool,
    station: StationId,
    id: i64,
    by: &str,
    reason: &str,
) -> Result<Hold, HoldError> {
    let by = line(by, "Who is releasing the hold", 120)?;
    let reason = line(reason, "The reason for releasing it", 1000)?;
    let n = sqlx::query(
        "UPDATE retention_hold SET released_by = $3, released_at = now(), release_reason = $4
         WHERE id = $1 AND station_id = $2 AND released_at IS NULL",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(&by)
    .bind(&reason)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        let h = hold(pool, station, id).await?;
        if h.released_at_ms.is_some() {
            return Err(HoldError::Released(id));
        }
    }
    hold(pool, station, id).await
}

/// What extending a hold says (ADR-0156).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Extension {
    /// The new date to keep it until, in milliseconds since the epoch.
    pub until_ms: u64,
    /// Who extends it.
    pub by: String,
    /// Why it has to be kept longer.
    pub reason: String,
}

/// Extend an open hold past its date (ADR-0156): `until_ms` becomes the date it is kept until,
/// and the flag clears until then. The new date must be in the future and later than the date
/// the hold has now, under the station's complaint rule (`rule`); a released hold is refused.
/// The hold, the records it keeps and its other fields are unchanged.
pub async fn extend_hold(
    pool: &sqlx::PgPool,
    station: StationId,
    id: i64,
    ext: &Extension,
    rule: Option<&ComplaintHold>,
    now_ms: u64,
) -> Result<Hold, HoldError> {
    let until_ms = ext.until_ms;
    let by = line(&ext.by, "Who is extending the hold", 120)?;
    let reason = line(&ext.reason, "The reason for extending it", 1000)?;
    let current = hold(pool, station, id).await?;
    if current.released_at_ms.is_some() {
        return Err(HoldError::Released(id));
    }
    if until_ms <= now_ms {
        return Err(HoldError::Invalid(
            "The new date has to be in the future.".into(),
        ));
    }
    if until_ms > now_ms.saturating_add(EXTEND_MAX_MS) {
        return Err(HoldError::Invalid(
            "The new date has to be within a hundred years.".into(),
        ));
    }
    if current.keep_until_ms(rule).is_some_and(|d| until_ms <= d) {
        return Err(HoldError::Invalid(
            "The new date has to be later than the date the hold is kept until now.".into(),
        ));
    }
    // The checks again in the write: two people extending at once cannot move it earlier.
    let n = sqlx::query(
        "UPDATE retention_hold
         SET extended_until = to_timestamp($3::double precision / 1000.0), extended_by = $4,
             extended_at = now(), extend_reason = $5
         WHERE id = $1 AND station_id = $2 AND released_at IS NULL
           AND coalesce(extended_until, '-infinity'::timestamptz) < to_timestamp($3::double precision / 1000.0)",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(until_ms as f64)
    .bind(&by)
    .bind(&reason)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        // Released, or extended further, between the read and the write.
        let now = hold(pool, station, id).await?;
        return Err(if now.released_at_ms.is_some() {
            HoldError::Released(id)
        } else {
            HoldError::Invalid(
                "The hold was extended to a later date meanwhile; read it again.".into(),
            )
        });
    }
    hold(pool, station, id).await
}

/// How far ahead a hold can be extended: a date beyond it is a typing slip, and the database
/// keeps no date past its range.
const EXTEND_MAX_MS: u64 = 100 * 366 * 86_400_000;

type HoldRow = (
    i64,
    String,
    String,
    f64,
    Option<f64>,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<String>,
    i64,
    i64,
);

const HOLD_SELECT: &str = "
    SELECT h.id, h.reason, h.placed_by, extract(epoch FROM h.placed_at)::double precision * 1000,
           extract(epoch FROM h.window_from)::double precision * 1000,
           extract(epoch FROM h.window_to)::double precision * 1000,
           h.released_by, extract(epoch FROM h.released_at)::double precision * 1000, h.release_reason,
           extract(epoch FROM h.extended_until)::double precision * 1000, h.extended_by,
           extract(epoch FROM h.extended_at)::double precision * 1000, h.extend_reason,
           (SELECT count(*) FROM recording r WHERE r.station_id = h.station_id AND r.deleted_at IS NULL
              AND (EXISTS (SELECT 1 FROM retention_hold_record x WHERE x.hold_id = h.id
                           AND x.record_kind = 'recording' AND x.record_id = r.id)
                   OR (h.window_from IS NOT NULL AND r.started_at < h.window_to
                       AND coalesce(r.ended_at, 'infinity'::timestamptz) > h.window_from))),
           (SELECT count(*) FROM caller_consent c WHERE c.station_id = h.station_id
              AND (EXISTS (SELECT 1 FROM retention_hold_record x WHERE x.hold_id = h.id
                           AND x.record_kind = 'consent' AND x.record_id = c.id)
                   OR (h.window_from IS NOT NULL AND c.accepted_at < h.window_to
                       AND coalesce(c.call_ended_at, c.accepted_at + interval '1 day') > h.window_from)))
    FROM retention_hold h";

async fn hold_of_row(pool: &sqlx::PgPool, r: HoldRow) -> Result<Hold, sqlx::Error> {
    let records: Vec<(String, i64)> = sqlx::query_as(
        "SELECT record_kind, record_id FROM retention_hold_record WHERE hold_id = $1 ORDER BY record_kind, record_id",
    )
    .bind(r.0)
    .fetch_all(pool)
    .await?;
    Ok(Hold {
        id: r.0,
        reason: r.1,
        placed_by: r.2,
        placed_at_ms: ms(Some(r.3)).unwrap_or(0),
        from_ms: ms(r.4),
        to_ms: ms(r.5),
        records: records
            .into_iter()
            .filter_map(|(k, id)| {
                let kind = match k.as_str() {
                    "recording" => HoldTarget::Recording,
                    "consent" => HoldTarget::Consent,
                    _ => return None,
                };
                Some(HoldRecord { kind, id })
            })
            .collect(),
        released_by: r.6,
        released_at_ms: ms(r.7),
        release_reason: r.8,
        extended_until_ms: ms(r.9),
        extended_by: r.10,
        extended_at_ms: ms(r.11),
        extend_reason: r.12,
        recordings: r.13,
        consents: r.14,
    })
}

/// One hold.
pub async fn hold(pool: &sqlx::PgPool, station: StationId, id: i64) -> Result<Hold, HoldError> {
    let row: Option<HoldRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{HOLD_SELECT} WHERE h.id = $1 AND h.station_id = $2"
    )))
    .bind(id)
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    match row {
        None => Err(HoldError::NotFound(id)),
        Some(r) => Ok(hold_of_row(pool, r).await?),
    }
}

/// Every hold, open ones first, then released ones newest first.
pub async fn holds(pool: &sqlx::PgPool, station: StationId) -> Result<Vec<Hold>, sqlx::Error> {
    let rows: Vec<HoldRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{HOLD_SELECT} WHERE h.station_id = $1
         ORDER BY (h.released_at IS NOT NULL), coalesce(h.released_at, h.placed_at) DESC, h.id DESC"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(hold_of_row(pool, r).await?);
    }
    Ok(out)
}

// ---- the sweep ----

/// What one sweep did, or (dry run) would do.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct SweepReport {
    pub dry_run: bool,
    pub preset: String,
    pub recordings_deleted: u32,
    /// Recordings whose file was already gone; the row is still marked deleted.
    pub files_missing: u32,
    pub consents_deleted: u32,
    /// Released holds whose own record went, the moderation-records period after their release
    /// (ADR-0157).
    pub holds_deleted: u32,
    /// Records past their period that a hold kept.
    pub held: u32,
    pub errors: Vec<String>,
}

/// The last sweep, as logged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct LastSweep {
    pub finished_at_ms: u64,
    pub preset: String,
    pub recordings_deleted: i32,
    pub files_missing: i32,
    pub consents_deleted: i32,
    pub held: i32,
    pub errors: String,
}

pub async fn last_sweep(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Option<LastSweep>, sqlx::Error> {
    let row: Option<(f64, String, i32, i32, i32, i32, String)> = sqlx::query_as(
        "SELECT extract(epoch FROM finished_at)::double precision * 1000, preset,
                recordings_deleted, files_missing, consents_deleted, held, errors
         FROM retention_sweep WHERE station_id = $1 ORDER BY finished_at DESC, id DESC LIMIT 1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| LastSweep {
        finished_at_ms: ms(Some(r.0)).unwrap_or(0),
        preset: r.1,
        recordings_deleted: r.2,
        files_missing: r.3,
        consents_deleted: r.4,
        held: r.5,
        errors: r.6,
    }))
}

/// How long a released hold's own record is kept after the release (ADR-0157): the
/// moderation-records period. Under No retention that period is 0 days, so the hold's record
/// follows the chat's review window (14 days) instead, as the chat's own holds do. `None`
/// when the period has no end this release applies.
pub fn hold_record_days(periods: &Periods) -> Option<u32> {
    match periods.moderation_records {
        Period::Days { days: 0 } | Period::WithCall => Some(HOLD_RECORD_REVIEW_DAYS),
        Period::Days { days } => Some(days),
        _ => None,
    }
}

/// The days a released hold's record is kept under No retention (the chat's review window).
pub const HOLD_RECORD_REVIEW_DAYS: u32 = 14;

fn days_i32(days: u32) -> i32 {
    i32::try_from(days).unwrap_or(i32::MAX)
}

/// Recordings of a kind past their period: (id, held).
const RECORDINGS_DUE: &str = "
    SELECT id, retention_held(station_id, 'recording', id, started_at, ended_at)
    FROM recording
    WHERE station_id = $1 AND kind = $2 AND deleted_at IS NULL AND ended_at IS NOT NULL
      AND ended_at + make_interval(days => $3) <= now()
    ORDER BY ended_at, id";

/// Consents past their period: (id, held). $2 is the days after the last linked recording is
/// deleted; $3 the days after the call for a consent no recording links.
const CONSENTS_DUE: &str = "
    WITH c AS (
        SELECT cc.id, cc.accepted_at,
               coalesce(cc.call_ended_at, cc.accepted_at + interval '1 day') AS call_end,
               count(r.id) AS links,
               count(r.id) FILTER (WHERE r.deleted_at IS NULL) AS live,
               max(r.deleted_at) AS last_gone
        FROM caller_consent cc
        LEFT JOIN consent_recording cr ON cr.consent_id = cc.id
        LEFT JOIN recording r ON r.id = cr.recording_id
        WHERE cc.station_id = $1
        GROUP BY cc.id
    )
    SELECT id, retention_held($1, 'consent', id, accepted_at, call_end)
    FROM c
    WHERE live = 0 AND call_end <= now()
      AND (CASE WHEN links > 0 THEN last_gone + make_interval(days => $2)
                ELSE call_end + make_interval(days => $3) END) <= now()
    ORDER BY id";

/// Delete what has expired and no hold keeps, under the station's setting. Files are removed
/// from `recordings_dir` before their row is marked deleted; a file that cannot be removed
/// keeps its row and is reported. With `dry_run`, nothing changes and the report says what
/// would go (for a confirmation before a shorter preset is chosen).
pub async fn sweep(
    pool: &sqlx::PgPool,
    station: StationId,
    setting: &Setting,
    recordings_dir: Option<&Path>,
    dry_run: bool,
) -> Result<SweepReport, sqlx::Error> {
    let started: (f64,) =
        sqlx::query_as("SELECT extract(epoch FROM now())::double precision * 1000")
            .fetch_one(pool)
            .await?;
    let mut report = SweepReport {
        dry_run,
        preset: setting.preset.clone(),
        ..SweepReport::default()
    };
    // A setting this release cannot read (a Custom row from a newer release, say) deletes
    // nothing: failing safe keeps records a while longer, failing open would delete them.
    if let Err(e) = setting.check() {
        report.errors.push(format!(
            "the retention setting cannot be read ({e}); nothing was deleted. Choose the jurisdiction again in Settings."
        ));
        if !dry_run {
            log_sweep(pool, station, started.0, &report).await?;
        }
        return Ok(report);
    }
    let periods = setting.periods();

    // 1. Recordings: the programme's (the logger's period) and the screener's (0 everywhere).
    for kind in [RecordingKind::Programme, RecordingKind::Call] {
        let Period::Days { days } = kind.period(&periods) else {
            continue;
        };
        let due: Vec<(i64, bool)> = sqlx::query_as(RECORDINGS_DUE)
            .bind(station.as_uuid())
            .bind(kind.key())
            .bind(days_i32(days))
            .fetch_all(pool)
            .await?;
        for (id, held) in due {
            if held {
                report.held += 1;
            } else if dry_run {
                report.recordings_deleted += 1;
            } else {
                delete_recording(pool, station, id, recordings_dir, &mut report).await?;
            }
        }
    }

    // 2. Consent (ADR-0107, ADR-0113).
    let logger_days = match periods.programme_recordings {
        Period::Days { days } => days,
        _ => 0,
    };
    let consent_days = match periods.consent {
        Period::AfterRecordings { days } => Some((days, logger_days)),
        Period::WithCall => Some((0, 0)),
        _ => None,
    };
    if let Some((after, unlinked)) = consent_days {
        let due: Vec<(i64, bool)> = sqlx::query_as(CONSENTS_DUE)
            .bind(station.as_uuid())
            .bind(days_i32(after))
            .bind(days_i32(unlinked))
            .fetch_all(pool)
            .await?;
        for (id, held) in due {
            if held {
                report.held += 1;
            } else if dry_run {
                report.consents_deleted += 1;
            } else {
                // Checked again as it goes: a hold placed since the list was read keeps it.
                let n = sqlx::query(
                    "DELETE FROM caller_consent
                     WHERE id = $1 AND station_id = $2
                       AND NOT retention_held(station_id, 'consent', id, accepted_at,
                             coalesce(call_ended_at, accepted_at + interval '1 day'))",
                )
                .bind(id)
                .bind(station.as_uuid())
                .execute(pool)
                .await?
                .rows_affected();
                if n == 1 {
                    report.consents_deleted += 1;
                } else {
                    report.held += 1;
                }
            }
        }
    }

    // 3. A released hold's own record goes the moderation-records period after its release
    // (ADR-0157). Its named records go with it; open holds are never touched.
    if let Some(days) = hold_record_days(&periods) {
        if dry_run {
            let (n,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM retention_hold WHERE station_id = $1 AND released_at IS NOT NULL
                   AND released_at + make_interval(days => $2) <= now()",
            )
            .bind(station.as_uuid())
            .bind(days_i32(days))
            .fetch_one(pool)
            .await?;
            report.holds_deleted = u32::try_from(n).unwrap_or(u32::MAX);
        } else {
            let n = sqlx::query(
                "DELETE FROM retention_hold WHERE station_id = $1 AND released_at IS NOT NULL
                   AND released_at + make_interval(days => $2) <= now()",
            )
            .bind(station.as_uuid())
            .bind(days_i32(days))
            .execute(pool)
            .await?
            .rows_affected();
            report.holds_deleted = u32::try_from(n).unwrap_or(u32::MAX);
        }
    }

    if !dry_run {
        // 4. A deleted recording's row goes once no consent counts its 42 days from it.
        sqlx::query(
            "DELETE FROM recording r WHERE r.station_id = $1 AND r.deleted_at IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM consent_recording cr WHERE cr.recording_id = r.id)",
        )
        .bind(station.as_uuid())
        .execute(pool)
        .await?;
        log_sweep(pool, station, started.0, &report).await?;
    }
    Ok(report)
}

async fn log_sweep(
    pool: &sqlx::PgPool,
    station: StationId,
    started_ms: f64,
    report: &SweepReport,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO retention_sweep
           (station_id, started_at, preset, recordings_deleted, files_missing, consents_deleted, held, errors)
         VALUES ($1, to_timestamp($2::double precision / 1000.0), $3, $4, $5, $6, $7, $8)",
    )
    .bind(station.as_uuid())
    .bind(started_ms)
    .bind(&report.preset)
    .bind(report.recordings_deleted as i32)
    .bind(report.files_missing as i32)
    .bind(report.consents_deleted as i32)
    .bind(report.held as i32)
    .bind(report.errors.join("\n"))
    .execute(pool)
    .await?;
    Ok(())
}

async fn delete_recording(
    pool: &sqlx::PgPool,
    station: StationId,
    id: i64,
    dir: Option<&Path>,
    report: &mut SweepReport,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Locked and checked again: a hold placed since the list was read keeps it.
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT file FROM recording
         WHERE id = $1 AND station_id = $2 AND deleted_at IS NULL
           AND NOT retention_held(station_id, 'recording', id, started_at, ended_at)
         FOR UPDATE",
    )
    .bind(id)
    .bind(station.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;
    let Some((file,)) = row else {
        report.held += 1;
        return Ok(());
    };
    if let Some(file) = file {
        let Some(dir) = dir.filter(|_| safe_relative(&file)) else {
            report.errors.push(format!(
                "recording {id}: its file `{file}` cannot be deleted (no recordings directory set, or not a path inside it); kept"
            ));
            return Ok(());
        };
        match std::fs::remove_file(dir.join(&file)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => report.files_missing += 1,
            Err(e) => {
                report
                    .errors
                    .push(format!("recording {id}: cannot delete `{file}`: {e}; kept"));
                return Ok(());
            }
        }
    }
    sqlx::query("UPDATE recording SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    report.recordings_deleted += 1;
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn each_preset_carries_its_jurisdictions_chat_age() {
        // ADR-0148, recce 29.
        for (key, age) in [
            (NO_RETENTION, 13),
            (CUSTOM, 13),
            ("uk_ofcom", 13),
            ("uk_internet", 13),
            ("us_webcaster", 13),
            ("us_nce", 13),
            ("eu_gdpr", 16),
            ("ie_licensed", 16),
            ("ie_internet", 16),
            ("de", 16),
            ("ca_licensed", 13),
            ("ca_internet", 13),
            ("au_licensed", 15),
            ("au_internet", 15),
        ] {
            assert_eq!(preset(key).unwrap().chat_min_age, age, "{key}");
        }
        assert_eq!(PRESETS.len(), 14, "a new preset needs its age here");
        // Custom is 13 whatever it is based on.
        let custom = Setting {
            preset: CUSTOM.into(),
            based_on: Some("de".into()),
            ..Default::default()
        };
        assert_eq!(custom.chat_min_age(), 13);
        let de = Setting {
            preset: "de".into(),
            ..Default::default()
        };
        assert_eq!(de.chat_min_age(), 16);
    }

    use super::*;

    #[test]
    fn fourteen_presets_no_retention_first_custom_last() {
        assert_eq!(PRESETS.len(), 14);
        assert_eq!(PRESETS[0].key, NO_RETENTION);
        assert_eq!(PRESETS[13].key, CUSTOM);
        let mut keys: Vec<_> = PRESETS.iter().map(|p| p.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 14, "keys are unique");
    }

    #[test]
    fn no_preset_sets_a_period_below_its_floor() {
        for p in PRESETS.iter().filter(|p| p.key != CUSTOM) {
            let periods = p.periods();
            for k in Kind::ALL {
                let cell = p.cell(k);
                assert!(!cell.source.is_empty(), "{} {k} has a source", p.key);
                if let Some(floor) = cell.floor {
                    assert!(
                        cell.period.floor_days(periods.programme_recordings) >= floor.days,
                        "{} {k} is under its floor",
                        p.key
                    );
                }
                cell.period.allowed_for(k).unwrap();
            }
        }
    }

    #[test]
    fn the_default_is_no_retention_and_it_warns() {
        let s = Setting::default();
        assert_eq!(s.preset, NO_RETENTION);
        assert_eq!(s.periods().programme_recordings, Period::days(0));
        assert_eq!(s.periods().listener_statistics, Period::NoLimit);
        let w = s.warnings();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].level, Level::Serious);
    }

    #[test]
    fn custom_keeps_the_last_presets_floors_as_warnings() {
        let mut periods = preset("ca_licensed").unwrap().periods();
        periods.programme_recordings = Period::days(14);
        let s = Setting {
            preset: CUSTOM.into(),
            based_on: Some("ca_licensed".into()),
            custom: Some(periods),
            ..Setting::default()
        };
        s.check().unwrap();
        let w = s.warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, Some(Kind::ProgrammeRecordings));
        assert!(w[0].source.contains("8(5)(a)"));
    }

    #[test]
    fn the_us_and_eu_moderation_floors_say_when_they_apply() {
        // ADR-0155: the 365 days follows a CyberTipline report, the 183 days a platform larger
        // than a small enterprise; the other floors say nothing of the kind.
        let warn = |key: &str| {
            let mut periods = preset(key).unwrap().periods();
            periods.moderation_records = Period::days(30);
            let s = Setting {
                preset: CUSTOM.into(),
                based_on: Some(key.into()),
                custom: Some(periods),
                ..Setting::default()
            };
            s.check().unwrap();
            s.warnings()
                .into_iter()
                .find(|w| w.kind == Some(Kind::ModerationRecords))
                .expect("a moderation warning")
        };
        let us = warn("us_webcaster");
        assert!(
            us.message.contains(
                "at least 365 days where the station has made a report to the CyberTipline"
            ),
            "{}",
            us.message
        );
        let eu = warn("eu_gdpr");
        assert!(
            eu.message
                .contains("at least 183 days where the platform is larger than a small enterprise"),
            "{}",
            eu.message
        );
        let uk = preset("uk_ofcom").unwrap().cell(Kind::ProgrammeRecordings);
        assert_eq!(uk.floor.unwrap().condition, None);
    }

    #[test]
    fn the_australian_presets_keep_reports_365_days_with_the_note() {
        for key in ["au_licensed", "au_internet"] {
            let p = preset(key).unwrap();
            let c = p.cell(Kind::ModerationRecords);
            assert_eq!(c.period, Period::days(365), "{key}");
            assert!(c.source.contains("5 years"), "{key}: {}", c.source);
            assert!(c.source.contains("expectation, not a duty"), "{key}");
            assert!(c.source.contains("Custom"), "{key}");
            assert!(p.summary.contains("eSafety expects 5 years"), "{key}");
            assert!(!p.summary.contains("stays open"), "{key}");
        }
    }

    fn a_hold(from_ms: Option<u64>, released: bool) -> Hold {
        Hold {
            id: 1,
            reason: "complaint".into(),
            placed_by: "Morag Ross".into(),
            placed_at_ms: 0,
            from_ms,
            to_ms: from_ms.map(|f| f + 3_600_000),
            records: vec![],
            released_by: released.then(|| "Morag Ross".into()),
            released_at_ms: released.then_some(1),
            release_reason: released.then(|| "closed".into()),
            extended_until_ms: None,
            extended_by: None,
            extended_at_ms: None,
            extend_reason: None,
            recordings: 0,
            consents: 0,
        }
    }

    #[test]
    fn an_open_hold_is_flagged_after_its_keep_until_date_and_never_before() {
        let rule = preset("ca_licensed").unwrap().complaint_hold;
        let day = 86_400_000;
        let h = a_hold(Some(10 * day), false);
        assert_eq!(h.keep_until_ms(rule.as_ref()), Some(66 * day));
        assert!(!h.past_date(rule.as_ref(), 66 * day));
        assert!(h.past_date(rule.as_ref(), 66 * day + 1));
        // Released, no window, or a rule with no fixed period: nothing to flag.
        assert!(!a_hold(Some(10 * day), true).past_date(rule.as_ref(), 500 * day));
        assert!(!a_hold(None, false).past_date(rule.as_ref(), 500 * day));
        let us = preset("us_webcaster").unwrap().complaint_hold;
        assert!(!h.past_date(us.as_ref(), 500 * day));
        assert!(!h.past_date(None, 500 * day));
    }

    #[test]
    fn an_extended_date_replaces_the_rules_and_flags_after_itself() {
        let rule = preset("ca_licensed").unwrap().complaint_hold;
        let day = 86_400_000;
        let mut h = a_hold(Some(10 * day), false);
        h.extended_until_ms = Some(200 * day);
        assert_eq!(h.keep_until_ms(rule.as_ref()), Some(200 * day));
        assert!(
            !h.past_date(rule.as_ref(), 100 * day),
            "past the rule's 66, not its own"
        );
        assert!(h.past_date(rule.as_ref(), 200 * day + 1));
        // A hold the rule gives no date (no window, or a rule with no period) has one once
        // extended.
        let us = preset("us_webcaster").unwrap().complaint_hold;
        assert!(!a_hold(Some(10 * day), false).past_date(us.as_ref(), 500 * day));
        assert!(h.past_date(us.as_ref(), 300 * day));
        h.from_ms = None;
        assert!(h.past_date(None, 300 * day));
    }

    #[test]
    fn a_released_holds_record_is_kept_the_moderation_period_or_the_review_window() {
        let mut p = preset("uk_ofcom").unwrap().periods();
        assert_eq!(hold_record_days(&p), Some(365));
        p.moderation_records = Period::days(0);
        assert_eq!(hold_record_days(&p), Some(HOLD_RECORD_REVIEW_DAYS));
        p.moderation_records = Period::days(90);
        assert_eq!(hold_record_days(&p), Some(90));
        p.moderation_records = Period::NoLimit;
        assert_eq!(hold_record_days(&p), None);
    }

    #[test]
    fn custom_cannot_cut_consent_short_of_the_recording_nor_drop_counts() {
        let mut p = preset("uk_internet").unwrap().periods();
        p.consent = Period::days(1);
        assert!(p.check().is_err());
        let mut p = preset("uk_internet").unwrap().periods();
        p.listener_statistics = Period::days(30);
        assert!(p.check().is_err());
        let mut p = preset("uk_internet").unwrap().periods();
        p.access_logs = Period::days(DAYS_MAX + 1);
        assert!(p.check().is_err());
    }

    #[test]
    fn a_preset_takes_no_periods_and_custom_needs_them() {
        let s = Setting {
            preset: "uk_ofcom".into(),
            custom: Some(PRESETS[1].periods()),
            ..Setting::default()
        };
        assert_eq!(s.check(), Err(SettingError::PeriodsWithPreset));
        let s = Setting {
            preset: CUSTOM.into(),
            ..Setting::default()
        };
        assert_eq!(s.check(), Err(SettingError::CustomWithoutPeriods));
        let s = Setting {
            preset: "atlantis".into(),
            ..Setting::default()
        };
        assert!(matches!(s.check(), Err(SettingError::UnknownPreset(_))));
    }

    #[test]
    fn licence_answers_warn_when_the_preset_does_not_meet_them() {
        let s = Setting {
            preset: "uk_internet".into(),
            broadcast_licence: Some(true),
            us_statutory_licence: Some(true),
            ..Setting::default()
        };
        // UK internet-only keeps the per-play log 1,096 days: the US floor is met.
        let w = s.warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, Some(Kind::ProgrammeRecordings));
        let s = Setting {
            preset: NO_RETENTION.into(),
            us_statutory_licence: Some(true),
            ..Setting::default()
        };
        assert!(
            s.warnings()
                .iter()
                .any(|w| w.kind == Some(Kind::PerPlayLog))
        );
    }

    #[test]
    fn periods_round_trip_as_json() {
        let p = preset("us_nce").unwrap().periods();
        let j = serde_json::to_value(p).unwrap();
        assert_eq!(
            j["access_logs"],
            serde_json::json!({"keep": "days", "days": 1461})
        );
        assert_eq!(
            j["consent"],
            serde_json::json!({"keep": "after_recordings", "days": 42})
        );
        assert_eq!(
            j["chat_messages"],
            serde_json::json!({"keep": "with_archive"})
        );
        assert_eq!(serde_json::from_value::<Periods>(j).unwrap(), p);
    }

    #[test]
    fn only_paths_inside_the_directory_are_safe() {
        assert!(safe_relative("2026/10/26/1400.opus"));
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "a\\b",
            "./a",
            "a\n",
        ] {
            assert!(!safe_relative(bad), "{bad:?}");
        }
    }
}

/// A caller's consent as the erasure search shows it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct ConsentFound {
    pub id: i64,
    pub identity: String,
    pub name: String,
    pub topic: String,
    /// Unix ms.
    pub accepted_at_ms: i64,
    pub erased: bool,
}

/// The callers' consents whose name holds `name` (any case), newest first, at most 50.
pub async fn find_consents(
    pool: &sqlx::PgPool,
    station: StationId,
    name: &str,
) -> Result<Vec<ConsentFound>, sqlx::Error> {
    let rows: Vec<(i64, String, String, String, f64, bool)> = sqlx::query_as(
        "SELECT id, identity, name, topic, extract(epoch FROM accepted_at)::float8 * 1000,
                erased_at IS NOT NULL
         FROM caller_consent
         WHERE station_id = $1 AND strpos(lower(name), lower($2)) > 0
         ORDER BY accepted_at DESC LIMIT 50",
    )
    .bind(station.as_uuid())
    .bind(name.trim())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, identity, name, topic, ms, erased)| ConsentFound {
            id,
            identity,
            name,
            topic,
            accepted_at_ms: ms as i64,
            erased,
        })
        .collect())
}

/// The name an erased consent shows.
pub const ERASED: &str = "erased";

/// Erase a caller's consent at their request (ADR-0122): the name and topic are redacted; the
/// date, the words, their version and who gave it stay, as the evidence the recording was
/// lawful. Every consent of the same call (its identity) goes with it. The rows erased.
pub async fn erase_consent(
    pool: &sqlx::PgPool,
    station: StationId,
    id: i64,
    by: &str,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE caller_consent SET name = $3, topic = '', erased_at = now(), erased_by = $4
         WHERE station_id = $1 AND erased_at IS NULL
           AND identity = (SELECT identity FROM caller_consent WHERE station_id = $1 AND id = $2)",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(ERASED)
    .bind(by)
    .execute(pool)
    .await?
    .rows_affected())
}
