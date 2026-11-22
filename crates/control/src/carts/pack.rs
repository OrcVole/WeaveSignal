// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Loading the default pack again (1.2.1). At its first start a station loads the pack
//! (`carts.demo_pack`) into its banks, and only then: a station that already has banks never got
//! a later release's better sounds. Here a person asks for one of the pack's banks to be loaded
//! into a station bank of their choosing.
//!
//! Into an empty bank it loads directly. A bank that holds carts is only touched when the request
//! says `replace`, and then only where the cart in the way is still a default pack's cart as it
//! was loaded ([`untouched`]): the same pack id, name, slot, mode and licence, and nothing added
//! to it. A
//! cart a person added, renamed, moved or changed in any way stays where it is, and the pack's
//! sound for its slot is left out. Default carts the pack no longer has a sound for go, so a
//! bank of the old pack does not keep half of it.
//!
//! "As it was loaded" needs the pack a station loaded, which may be an older release's: the
//! pack shipped from 1.0.0 to 1.2.0 is listed here ([`PACK_1_2_0`]), since its files are no
//! longer in the tree once a newer pack takes their place.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use weavesignal_model::carts::{self, Bank, Cart, CartMode};

use super::{CartMeta, CartProblem, Owner, Pack, PackCart};
use crate::hub::Hub;

/// The default pack of 1.0.0 to 1.2.0 (its `carts.json` did not change between them), as it
/// named each cart: its id, its label, its slot, how it played and its licence. A cart loaded from it carries `demo:<id>` as its origin.
pub(crate) const PACK_1_2_0: &[(&str, &str, u8, CartMode, &str)] = &[
    (
        "applause-1",
        "Applause after a concert",
        1,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "applause-2",
        "Applause, short",
        2,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "cheer-1",
        "Crowd hurray",
        3,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "cheer-2",
        "Clapping and hurray",
        4,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "cheer-3",
        "Crowd ooh-aah",
        5,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "laugh-1",
        "Audience laugh, short",
        6,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "laugh-2",
        "Audience laughter",
        7,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "laugh-3",
        "Laughter and clapping",
        8,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    ("laugh-4", "Cartoon laugh", 9, CartMode::OneShot, "CC0 1.0"),
    (
        "rimshot-1",
        "Rimshot, ba-dum",
        10,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "rimshot-2",
        "Rimshot, bu-doo",
        11,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "rimshot-3",
        "Rimshot and cymbal, ba-dum-tss",
        12,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    ("boing-1", "Boing", 13, CartMode::OneShot, "CC BY 4.0"),
    (
        "sad-trombone-1",
        "Sad trombone",
        14,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "ding-1",
        "Right answer, bright ding",
        1,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "ding-2",
        "Right answer, confirmation",
        2,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "ding-3",
        "Doorbell ding-dong",
        3,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "buzzer-1",
        "Wrong answer buzzer",
        4,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "buzzer-2",
        "Wrong answer, error tone",
        5,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "bleep-1",
        "Censor bleep, 1 kHz",
        6,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "foghorn-1",
        "Foghorn, lighthouse diaphone",
        7,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "foghorn-2",
        "Foghorn, fog signal",
        8,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "foghorn-3",
        "Ship horn, cruise ship",
        9,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "airhorn-1",
        "Air horn, chime horn blast",
        10,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "airhorn-2",
        "Air horn, short blast",
        11,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "phone-ring-1",
        "Old telephone ringing",
        12,
        CartMode::OneShot,
        "CC BY 4.0",
    ),
    (
        "phone-ring-2",
        "Modern phone ringtone",
        13,
        CartMode::OneShot,
        "CC BY 4.0",
    ),
    ("drumroll-1", "Drum roll", 1, CartMode::OneShot, "CC0 1.0"),
    (
        "drumroll-2",
        "Drum roll, 4 seconds",
        2,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "whoosh-1",
        "Whoosh sweeper",
        3,
        CartMode::Sweeper,
        "CC0 1.0",
    ),
    ("gong-1", "Gong", 4, CartMode::OneShot, "CC0 1.0"),
    ("gong-2", "Gong, short", 5, CartMode::OneShot, "CC0 1.0"),
    (
        "fanfare-1",
        "Ceremonial fanfare, concert band",
        6,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "fanfare-2",
        "Fanfare, finale",
        7,
        CartMode::OneShot,
        "CC BY 4.0",
    ),
    (
        "jingle-1",
        "Upbeat jingle, steel",
        1,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "jingle-2",
        "Comedy jingle, sax",
        2,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "jingle-3",
        "Game-show jingle, 8-bit",
        3,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "jingle-4",
        "Cheeky jingle, pizzicato",
        4,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "jingle-5",
        "Winner hit, brass",
        5,
        CartMode::OneShot,
        "CC0 1.0",
    ),
    (
        "news-sting-1",
        "News sting",
        6,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "news-sting-2",
        "Light sting",
        7,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "bagpipe-1",
        "Highland bagpipe (electronic chanter)",
        8,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "bagpipe-2",
        "Pipe band, Scotland the Brave",
        9,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "thunder-1",
        "Thunder storm",
        10,
        CartMode::OneShot,
        "CC BY 3.0",
    ),
    (
        "rain-1",
        "Rain bed (loop)",
        11,
        CartMode::Bed,
        "Public domain",
    ),
    ("tick-1", "Clock tick", 12, CartMode::OneShot, "CC0 1.0"),
    (
        "tick-2",
        "Clock ticking, old",
        13,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "pips-1",
        "Time signal pips",
        14,
        CartMode::OneShot,
        "Public domain",
    ),
    (
        "bed-1",
        "Music bed, wholesome (loop)",
        15,
        CartMode::Bed,
        "CC BY 4.0",
    ),
    (
        "bed-2",
        "Music bed, chill (loop)",
        16,
        CartMode::Bed,
        "CC BY 3.0",
    ),
];

/// Whether a station's cart is still a default pack's cart as it was loaded: it came from a
/// pack (`demo:<id>`), and has the name, slot, mode and licence that pack gave it, with no
/// second line and no ramp, which no pack sets. The bank is not part of it (a pack's bank may be
/// loaded into another), so a default moved to the same slot of another bank still counts. `pack` is the pack the station has now; the pack of 1.0.0
/// to 1.2.0 is always known. A show's own cart is never one.
pub(crate) fn untouched(cart: &Cart, pack: Option<&Pack>) -> bool {
    if cart.show.is_some() || !cart.detail.is_empty() || cart.ramp_ms.is_some() {
        return false;
    }
    let Some(id) = cart.origin.strip_prefix("demo:") else {
        return false;
    };
    // A licence is cut to 80 characters when a cart is loaded.
    let cut = |l: &str| l.chars().take(80).collect::<String>();
    let was = |label: &str, slot: u8, mode: CartMode, licence: &str| {
        cart.label == label.trim()
            && cart.slot == slot
            && cart.mode == mode
            && cart.licence == cut(licence)
    };
    PACK_1_2_0
        .iter()
        .any(|(i, label, slot, mode, licence)| *i == id && was(label, *slot, *mode, licence))
        || pack.is_some_and(|p| {
            p.carts
                .iter()
                .any(|c| c.id == id && was(&c.label, c.suggested.slot, c.mode(), &c.licence))
        })
}

/// One of the pack's banks to load, and the station's bank it goes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PackInto {
    /// The pack's bank, by its number in the pack.
    pub bank: u8,
    /// The station's bank it is loaded into (1 to 5).
    pub into: u8,
}

/// `POST /api/v1/carts/pack`: which of the pack's banks to load, and where.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PackLoad {
    /// At least one; no station bank twice.
    pub loads: Vec<PackInto>,
    /// Go on where a bank holds carts: replace the default carts nobody changed, keep every
    /// other cart. Left out or false, a bank that holds carts is refused (409) with what would
    /// happen to it.
    #[serde(default)]
    pub replace: bool,
}

/// What loading one of the pack's banks into a station bank does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PackPlan {
    /// The pack's bank, and its name.
    pub bank: u8,
    pub name: String,
    /// The station's bank it goes into, and whether that bank has been made.
    pub into: u8,
    pub made: bool,
    /// The pack's sounds that are loaded.
    pub load: usize,
    /// Of those, how many take the place of a default cart nobody changed.
    pub replace: usize,
    /// Default carts nobody changed that go because the pack has no sound for their slot.
    pub remove: usize,
    /// Carts that are a person's own and stay, by slot.
    pub keep: Vec<u8>,
    /// The pack's sounds left out because a cart that stays is in their slot.
    pub left_out: usize,
}

impl PackPlan {
    /// The station bank holds carts, so the load must be asked for with `replace`.
    pub fn asks(&self) -> bool {
        self.replace + self.remove + self.keep.len() > 0
    }

    /// What would happen, in words, for the refusal that asks.
    fn words(&self) -> String {
        let n =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        format!(
            "bank {} holds {}: {} would be replaced or removed, and {} would stay",
            self.into,
            n(
                self.replace + self.remove + self.keep.len(),
                "cart",
                "carts"
            ),
            n(
                self.replace + self.remove,
                "default cart nobody changed",
                "default carts nobody changed"
            ),
            n(self.keep.len(), "cart of yours", "carts of yours"),
        )
    }
}

/// A plan with what carrying it out needs: the pack's carts to load, and the carts that go.
#[derive(Clone, Debug)]
pub(crate) struct Planned {
    pub plan: PackPlan,
    /// Indexes into the pack's carts.
    pub load: Vec<usize>,
    /// The default carts that may be replaced or removed, by id: nothing else is ever touched.
    pub replaceable: BTreeSet<Uuid>,
    /// The slots whose default cart goes with nothing in its place.
    pub remove: Vec<(u8, Uuid)>,
}

/// Work out what loading the pack's bank `load.bank` into the station's bank `load.into` does,
/// from the station's banks and carts as they are. Nothing is changed here.
pub(crate) fn plan(pack: &Pack, load: PackInto, banks: &[Bank], carts: &[Cart]) -> Planned {
    let there: Vec<&Cart> = carts
        .iter()
        .filter(|c| c.show.is_none() && c.bank == load.into)
        .collect();
    let (defaults, own): (Vec<&Cart>, Vec<&Cart>) =
        there.iter().partition(|c| untouched(c, Some(pack)));
    let kept: BTreeSet<u8> = own.iter().map(|c| c.slot).collect();
    let of_bank = |c: &&PackCart| c.suggested.bank == load.bank;
    let loaded: Vec<usize> = pack
        .carts
        .iter()
        .enumerate()
        .filter(|(_, c)| of_bank(c) && !kept.contains(&c.suggested.slot))
        .map(|(i, _)| i)
        .collect();
    let slots: BTreeSet<u8> = loaded
        .iter()
        .map(|i| pack.carts[*i].suggested.slot)
        .collect();
    let remove: Vec<(u8, Uuid)> = defaults
        .iter()
        .filter(|c| !slots.contains(&c.slot))
        .map(|c| (c.slot, c.id))
        .collect();
    Planned {
        plan: PackPlan {
            bank: load.bank,
            name: pack
                .banks
                .get(&load.bank.to_string())
                .cloned()
                .unwrap_or_default(),
            into: load.into,
            made: banks
                .iter()
                .any(|b| b.show.is_none() && b.number == load.into),
            load: loaded.len(),
            replace: defaults.iter().filter(|c| slots.contains(&c.slot)).count(),
            remove: remove.len(),
            keep: kept.iter().copied().collect(),
            left_out: pack.carts.iter().filter(of_bank).count() - loaded.len(),
        },
        load: loaded,
        replaceable: defaults.iter().map(|c| c.id).collect(),
        remove,
    }
}

/// One of the pack's sounds, as the wall's dialog lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PackCartView {
    pub slot: u8,
    pub label: String,
    pub mode: CartMode,
}

/// One of the pack's banks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PackBankView {
    /// Its number in the pack.
    pub number: u8,
    pub name: String,
    pub carts: Vec<PackCartView>,
}

/// `GET /api/v1/carts/pack`: the station's default pack, and which of the station's carts are
/// still a default pack's as they were loaded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PackView {
    /// The station has a pack to load (`carts.demo_pack`) and it reads.
    pub available: bool,
    /// Why not, when it is not.
    pub problem: Option<String>,
    /// A pack is loading now: another load is refused until it ends.
    pub loading: bool,
    pub banks: Vec<PackBankView>,
    /// The station's carts that a load with `replace` may replace or remove, by id: default
    /// carts nobody changed. Every other cart stays.
    pub untouched: Vec<Uuid>,
}

/// `202` from `POST /api/v1/carts/pack`: what is being done. The studio's view counts the
/// sounds as they go through the media worker (`carts.importing`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PackStarted {
    pub plans: Vec<PackPlan>,
    /// The sounds to load, over every bank asked for.
    pub loading: usize,
}

impl Hub {
    /// The pack's directory and its `carts.json`, read now.
    async fn read_pack(&self) -> Result<(PathBuf, Pack), CartProblem> {
        let Some(dir) = self.carts.config.demo_pack.clone() else {
            return Err(CartProblem::NoPack(
                "`carts.demo_pack` is not set".to_owned(),
            ));
        };
        let text = tokio::fs::read_to_string(dir.join("carts.json"))
            .await
            .map_err(|e| CartProblem::NoPack(format!("its carts.json cannot be read: {e}")))?;
        let pack: Pack = serde_json::from_str(&text)
            .map_err(|e| CartProblem::NoPack(format!("its carts.json: {e}")))?;
        Ok((dir, pack))
    }

    /// The default pack as the wall's dialog needs it.
    pub async fn pack_view(&self) -> PackView {
        let (_, pack) = match self.read_pack().await {
            Ok(p) => p,
            Err(e) => {
                return PackView {
                    problem: Some(e.to_string()),
                    ..PackView::default()
                };
            }
        };
        let mut banks: Vec<PackBankView> = pack
            .banks
            .iter()
            .filter_map(|(n, name)| Some((n.parse::<u8>().ok()?, name)))
            .map(|(number, name)| {
                let mut carts: Vec<PackCartView> = pack
                    .carts
                    .iter()
                    .filter(|c| c.suggested.bank == number)
                    .map(|c| PackCartView {
                        slot: c.suggested.slot,
                        label: c.label.clone(),
                        mode: c.mode(),
                    })
                    .collect();
                carts.sort_by_key(|c| c.slot);
                PackBankView {
                    number,
                    name: name.clone(),
                    carts,
                }
            })
            .collect();
        banks.sort_by_key(|b| b.number);
        let (loading, untouched) = self.read_inner(|i| {
            (
                i.carts.importing.is_some(),
                i.carts
                    .carts
                    .iter()
                    .filter(|c| untouched(c, Some(&pack)))
                    .map(|c| c.id)
                    .collect(),
            )
        });
        PackView {
            available: true,
            problem: None,
            loading,
            banks,
            untouched,
        }
    }

    /// Start loading the pack's banks asked for; the sounds go through the media worker one by
    /// one after this answers. A bank that holds carts is refused unless `replace` is set, with
    /// what would happen to it.
    pub async fn start_pack_load(
        self: &Arc<Self>,
        req: &PackLoad,
    ) -> Result<PackStarted, CartProblem> {
        self.db()?;
        let (dir, pack) = self.read_pack().await?;
        if req.loads.is_empty() {
            return Err(CartProblem::Invalid(
                "name at least one of the pack's banks to load".into(),
            ));
        }
        let mut into = BTreeSet::new();
        for l in &req.loads {
            if !pack.banks.contains_key(&l.bank.to_string()) {
                return Err(CartProblem::Invalid(format!(
                    "the pack has no bank {}",
                    l.bank
                )));
            }
            if !(1..=carts::BANKS).contains(&l.into) {
                return Err(carts::CartError::Bank(l.into).into());
            }
            if !into.insert(l.into) {
                return Err(CartProblem::Invalid(format!(
                    "bank {} is named twice; one of the pack's banks goes into it at a time",
                    l.into
                )));
            }
        }
        // Planned and claimed in one look at the wall, so two loads never start together.
        let planned = self.with_inner(|i| {
            if i.carts.importing.is_some() {
                return Err(CartProblem::PackLoading);
            }
            let planned: Vec<Planned> = req
                .loads
                .iter()
                .map(|l| plan(&pack, *l, &i.carts.banks, &i.carts.carts))
                .collect();
            if !req.replace
                && let Some(p) = planned.iter().find(|p| p.plan.asks())
            {
                return Err(CartProblem::PackAsks(p.plan.words()));
            }
            let total = planned.iter().map(|p| p.load.len()).sum();
            i.carts.importing = Some((0, total));
            Ok(planned)
        })?;
        let started = PackStarted {
            loading: planned.iter().map(|p| p.load.len()).sum(),
            plans: planned.iter().map(|p| p.plan.clone()).collect(),
        };
        let hub = self.clone();
        tokio::spawn(async move { hub.run_pack_load(dir, pack, planned).await });
        Ok(started)
    }

    /// Carry the plans out. A cart that came into a slot since the plan was made is a person's:
    /// it stays, and the pack's sound for that slot is left out.
    async fn run_pack_load(&self, dir: PathBuf, pack: Pack, planned: Vec<Planned>) {
        let total: usize = planned.iter().map(|p| p.load.len()).sum();
        let (mut done, mut failed, mut left) = (0usize, 0usize, 0usize);
        for p in &planned {
            let into = p.plan.into;
            if !p.plan.made
                && let Err(e) = self
                    .put_bank(
                        &Owner::Station,
                        Bank {
                            number: into,
                            name: p.plan.name.clone(),
                            // Banks take palette slots in order (ADR-0066).
                            colour_slot: into,
                            show: None,
                        },
                    )
                    .await
            {
                tracing::warn!(bank = into, error = %e, "the pack's bank was not made");
                failed += p.load.len();
                self.wall(|w| w.importing = Some((done + failed + left, total)));
                continue;
            }
            for c in p.load.iter().map(|i| &pack.carts[*i]) {
                let slot = c.suggested.slot;
                let in_the_way = self.read_inner(|i| {
                    i.carts
                        .carts
                        .iter()
                        .find(|x| x.show.is_none() && x.bank == into && x.slot == slot)
                        .map(|x| x.id)
                });
                // A cart that is sounding is left to play: the load never stops what is on air.
                let free = in_the_way.is_none_or(|id| {
                    p.replaceable.contains(&id)
                        && !self.read_inner(|i| i.carts.sounding.iter().any(|s| s.cart == id))
                });
                // The pack names files beside it; nothing reaches outside it.
                if !free || c.file.contains('/') || c.file.starts_with('.') {
                    left += 1;
                } else {
                    let meta = CartMeta {
                        label: c.label.clone(),
                        detail: String::new(),
                        mode: c.mode(),
                        ramp_ms: None,
                        origin: format!("demo:{}", c.id),
                        licence: c.licence.clone(),
                    };
                    let result = match tokio::fs::read(dir.join(&c.file)).await {
                        Ok(bytes) => self
                            .import_cart(&Owner::Station, into, slot, meta, bytes)
                            .await
                            .map(|_| ()),
                        Err(e) => Err(CartProblem::Invalid(format!("cannot read {}: {e}", c.file))),
                    };
                    match result {
                        Ok(()) => done += 1,
                        Err(e) => {
                            tracing::warn!(cart = %c.id, bank = into, slot, error = %e, "a pack cart was not loaded");
                            failed += 1;
                        }
                    }
                }
                self.wall(|w| w.importing = Some((done + failed + left, total)));
                self.publish_now();
            }
            // The default carts the pack has nothing for: gone, if they are still the same carts.
            for (slot, id) in &p.remove {
                let still = self.read_inner(|i| {
                    i.carts
                        .carts
                        .iter()
                        .any(|x| x.id == *id && x.bank == into && x.slot == *slot)
                        && !i.carts.sounding.iter().any(|s| s.cart == *id)
                });
                if still && let Err(e) = self.delete_cart(&Owner::Station, into, *slot).await {
                    tracing::warn!(bank = into, slot, error = %e, "an old default cart was not removed");
                }
            }
        }
        self.wall(|w| w.importing = None);
        self.publish_now();
        tracing::info!(
            loaded = done, failed, left_out = left, banks = planned.len(), pack = %dir.display(),
            "the default pack was loaded again"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pack made for the test: bank 1 has slots 1, 2 and 3; bank 2 has slot 1.
    fn pack() -> Pack {
        serde_json::from_str(
            r#"{"banks": {"1": "Reactions", "2": "Idents"}, "carts": [
                {"id": "clap", "label": "Clap", "file": "clap.wav", "category": "applause", "suggested": {"bank": 1, "slot": 1}, "loop": false, "licence": "CC0 1.0"},
                {"id": "swish", "label": "Swish", "file": "swish.wav", "category": "whoosh", "suggested": {"bank": 1, "slot": 2}, "loop": false, "licence": "CC0 1.0"},
                {"id": "rain", "label": "Rain (loop)", "file": "rain.wav", "category": "weather", "suggested": {"bank": 1, "slot": 3}, "loop": true, "licence": "CC0 1.0"},
                {"id": "ident", "label": "Ident", "file": "ident.wav", "category": "jingle", "suggested": {"bank": 2, "slot": 1}, "loop": false, "licence": "CC0 1.0"}
            ]}"#,
        )
        .unwrap()
    }

    fn cart(bank: u8, slot: u8, label: &str, mode: CartMode, origin: &str) -> Cart {
        Cart {
            id: Uuid::new_v4(),
            bank,
            slot,
            label: label.into(),
            detail: String::new(),
            mode,
            file: Some(format!("{}.flac", Uuid::new_v4())),
            duration_ms: 2_000,
            gain_db: -7.0,
            loudness_lufs: Some(-16.0),
            true_peak_dbtp: Some(-1.0),
            ramp_ms: None,
            origin: origin.into(),
            licence: "CC0 1.0".into(),
            show: None,
        }
    }

    #[test]
    fn a_default_cart_is_one_nobody_changed() {
        let p = pack();
        // As the pack the station has now loaded it.
        assert!(untouched(
            &cart(1, 1, "Clap", CartMode::OneShot, "demo:clap"),
            Some(&p)
        ));
        assert!(untouched(
            &cart(1, 2, "Swish", CartMode::Sweeper, "demo:swish"),
            Some(&p)
        ));
        assert!(untouched(
            &cart(1, 3, "Rain (loop)", CartMode::Bed, "demo:rain"),
            Some(&p)
        ));
        // As the pack of 1.0.0 to 1.2.0 loaded it, whatever pack the station has now, or none.
        let old = cart(
            1,
            1,
            "Applause after a concert",
            CartMode::OneShot,
            "demo:applause-1",
        );
        assert!(untouched(&old, Some(&p)) && untouched(&old, None));
        // The bank it sits in is not the test: a pack's bank may be loaded into another.
        assert!(untouched(
            &cart(5, 1, "Clap", CartMode::OneShot, "demo:clap"),
            Some(&p)
        ));
        // Renamed, moved to another key, played another way, given a second line or a ramp: a
        // person's now.
        assert!(!untouched(
            &cart(1, 1, "Clap, mine", CartMode::OneShot, "demo:clap"),
            Some(&p)
        ));
        assert!(!untouched(
            &cart(1, 9, "Clap", CartMode::OneShot, "demo:clap"),
            Some(&p)
        ));
        assert!(!untouched(
            &cart(1, 1, "Clap", CartMode::Bed, "demo:clap"),
            Some(&p)
        ));
        let mut c = cart(1, 1, "Clap", CartMode::OneShot, "demo:clap");
        c.detail = "after the news".into();
        assert!(!untouched(&c, Some(&p)));
        let mut c = cart(1, 1, "Clap", CartMode::OneShot, "demo:clap");
        c.ramp_ms = Some(500);
        assert!(!untouched(&c, Some(&p)));
        // Its credit changed: a person's too.
        let mut c = cart(1, 1, "Clap", CartMode::OneShot, "demo:clap");
        c.licence = "CC BY 4.0, Kettle FM".into();
        assert!(!untouched(&c, Some(&p)));
        // A sound a person sent, whatever it is called; a show's own cart.
        assert!(!untouched(
            &cart(1, 1, "Clap", CartMode::OneShot, "clap.wav"),
            Some(&p)
        ));
        assert!(!untouched(
            &cart(1, 1, "Clap", CartMode::OneShot, ""),
            Some(&p)
        ));
        let mut c = cart(1, 1, "Clap", CartMode::OneShot, "demo:clap");
        c.show = Some("steeped".into());
        assert!(!untouched(&c, Some(&p)));
        // A pack id nobody knows, under a known name.
        assert!(!untouched(
            &cart(1, 1, "Clap", CartMode::OneShot, "demo:other"),
            Some(&p)
        ));
    }

    #[test]
    fn the_old_pack_is_fifty_carts_each_in_a_slot_of_the_wall() {
        assert_eq!(PACK_1_2_0.len(), 50);
        let ids: BTreeSet<&str> = PACK_1_2_0.iter().map(|c| c.0).collect();
        assert_eq!(ids.len(), 50, "no id twice");
        assert!(
            PACK_1_2_0
                .iter()
                .all(|c| (1..=24).contains(&c.2) && !c.4.is_empty())
        );
    }

    #[test]
    fn an_empty_bank_takes_the_packs_bank_whole_and_asks_nothing() {
        let p = pack();
        let none = plan(&p, PackInto { bank: 1, into: 4 }, &[], &[]);
        assert_eq!(
            none.plan,
            PackPlan {
                bank: 1,
                name: "Reactions".into(),
                into: 4,
                made: false,
                load: 3,
                replace: 0,
                remove: 0,
                keep: vec![],
                left_out: 0
            }
        );
        assert!(!none.plan.asks());
        assert_eq!(none.load, vec![0, 1, 2]);
        // A bank that is made and empty is the same, and says it is made. Another show's bank
        // of that number, and carts in other banks, are not this bank's.
        let banks = [
            Bank {
                number: 4,
                name: "Spare".into(),
                colour_slot: 4,
                show: None,
            },
            Bank {
                number: 1,
                name: "Steeped".into(),
                colour_slot: 2,
                show: Some("steeped".into()),
            },
        ];
        let mut theirs = cart(4, 1, "Woof", CartMode::OneShot, "woof.wav");
        theirs.show = Some("steeped".into());
        let elsewhere = cart(2, 1, "Mine", CartMode::OneShot, "mine.wav");
        let made = plan(
            &p,
            PackInto { bank: 1, into: 4 },
            &banks,
            &[theirs, elsewhere],
        );
        assert!(made.plan.made && !made.plan.asks());
        assert_eq!(made.plan.load, 3);
    }

    #[test]
    fn a_bank_that_holds_carts_keeps_every_cart_that_is_a_persons() {
        let p = pack();
        // Bank 1: the old pack's applause on key 1, nobody's; a renamed default on key 2; a
        // person's own sound on key 7; the old pack's laugh on key 6, which the pack no longer
        // has a sound for.
        let old_1 = cart(
            1,
            1,
            "Applause after a concert",
            CartMode::OneShot,
            "demo:applause-1",
        );
        let renamed = cart(1, 2, "Our applause", CartMode::OneShot, "demo:applause-2");
        let own = cart(1, 7, "Station ident", CartMode::OneShot, "ident.wav");
        let mut old_6 = cart(
            1,
            6,
            "Audience laugh, short",
            CartMode::OneShot,
            "demo:laugh-1",
        );
        old_6.licence = "CC BY 3.0".into();
        let carts = [old_1.clone(), renamed.clone(), own.clone(), old_6.clone()];
        let got = plan(&p, PackInto { bank: 1, into: 1 }, &[], &carts);
        assert_eq!(
            got.plan,
            PackPlan {
                bank: 1,
                name: "Reactions".into(),
                into: 1,
                made: false,
                // Clap onto key 1 and Rain onto key 3; Swish is left out, key 2 is taken.
                load: 2,
                replace: 1,
                remove: 1,
                keep: vec![2, 7],
                left_out: 1
            }
        );
        assert!(got.plan.asks());
        assert_eq!(got.load, vec![0, 2]);
        assert_eq!(got.remove, vec![(6, old_6.id)]);
        // Only the two defaults nobody changed may ever be replaced or removed.
        assert_eq!(got.replaceable, BTreeSet::from([old_1.id, old_6.id]));
        assert_eq!(
            got.plan.words(),
            "bank 1 holds 4 carts: 2 default carts nobody changed would be replaced or removed, and 2 carts of yours would stay"
        );
    }
}
