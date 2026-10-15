//! Asset slot limits (Rules Reference p.19, #498).
//!
//! Slots cap how many asset cards of a given type an investigator may have in
//! play. A full slot does **not** block a play: per RR the player "must choose
//! and discard other assets under his or her control simultaneously with the new
//! asset entering the slot." This module owns the capacity query, the shortfall
//! math, and the interactive make-room driver invoked when an asset enters play.
//!
//! **Both of the investigator's areas hold slots.** `glossary/Slots.md`: *"Each
//! asset in an investigator's play area or threat area with a slot symbol is
//! held in a slot of that type."* The Necronomicon 01009 in a threat area takes
//! a hand slot.

use std::collections::{BTreeMap, BTreeSet};

use card_dsl::card_data::Slot;
use card_dsl::dsl::{CardFilter, Effect};

use crate::action::InputResponse;
use crate::card_registry::{self, CardRegistry};
use crate::engine::dispatch::{cards, choice, threat_area};
use crate::engine::outcome::{EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken};
use crate::engine::{abilities_in_effect, board, Cx};
use crate::state::{
    AssetEntry, CardCode, CardInPlay, CardInstanceId, GameState, InvestigatorId, SlotDiscardFrame,
};

/// Per-type slot counts (a multiset). `BTreeMap` keeps iteration deterministic.
pub(super) type SlotCounts = BTreeMap<Slot, u8>;

/// The slots normally available to an investigator (Rules Reference p.19):
/// "1 accessory slot · 1 body slot · 1 ally slot · 2 hand slots · 2 arcane
/// slots". `Tarot` is not in the original Core Rules Reference (a later-product
/// slot) and no Core/Dunwich card uses it; we default it to 1 and treat it as
/// unreachable in scope. [`slot_grants`] adds what the investigator's cards
/// grant on top.
pub(super) fn default_slot_capacity(slot: Slot) -> u8 {
    match slot {
        Slot::Accessory | Slot::Body | Slot::Ally | Slot::Tarot => 1,
        Slot::Hand | Slot::Arcane => 2,
    }
}

/// Tally a slot multiset (e.g. a two-handed weapon → `{Hand: 2}`).
pub(super) fn count_slots(slots: &[Slot]) -> SlotCounts {
    let mut counts = SlotCounts::new();
    for &slot in slots {
        *counts.entry(slot).or_insert(0) += 1;
    }
    counts
}

/// One [`Effect::AdditionalSlots`] grant in effect: `count` more slots of type
/// `slot`, holding only what `only` admits ([`SlotPools`]).
#[derive(Debug, Clone)]
struct SlotGrant {
    /// The card the grant is printed on, so a make-room discard of that card
    /// is counted as losing the grant too.
    source: CardInstanceId,
    slot: Slot,
    count: u8,
    only: Option<CardFilter>,
}

/// The [`Effect::AdditionalSlots`] grants on the cards `investigator`
/// controls — their investigator card, play area and threat area, through
/// [`abilities_in_effect::constants_of`]. Daisy's Tote Bag 01008: *"You have 2
/// additional hand slots, which can only be used to hold \[\[Tome\]\] assets."*
///
/// An investigator's slot capacity is [`default_slot_capacity`] for each type
/// plus these pools. **A layout is checked by counting** ([`shortfall`]): it
/// is valid if the assets can be packed into the pools, restricted pools taking
/// only the assets they admit. That deliberately approximates FAQ 1.24
/// ("Shifting Slots"), under which *"the investigator must decide which slot
/// is holding which asset at the moment it is played. This cannot be adjusted
/// later unless the contents or quantity of the investigator's slots changes,
/// at which point the investigator may switch which slots are holding any of
/// their assets."* The engine records no assignment. Slot pressure is only
/// checked as an asset enters, which changes the slots' contents, so the
/// player may re-decide every assignment at each check, and the best
/// assignment exists exactly when the assets pack.
fn slot_grants(
    state: &GameState,
    reg: &CardRegistry,
    investigator: InvestigatorId,
) -> Vec<SlotGrant> {
    abilities_in_effect::constants_of(state, reg, investigator)
        .into_iter()
        .filter_map(|(instance, ability)| match ability.effect {
            Effect::AdditionalSlots { slot, count, only } => Some(SlotGrant {
                source: instance,
                slot,
                count,
                only,
            }),
            _ => None,
        })
        .collect()
}

/// The slots of one type an investigator has (`glossary/Slots.md`): a general
/// pool any asset may take, and one restricted pool per restricted grant.
struct SlotPools<'g> {
    /// Default capacity plus every unrestricted grant: holds any asset.
    general: u8,
    /// One per restricted grant: holds only what its filter admits.
    restricted: Vec<RestrictedPool<'g>>,
}

/// A restricted grant's pool: `room` slots left, holding only what `only`
/// admits.
struct RestrictedPool<'g> {
    room: u8,
    only: &'g CardFilter,
}

impl<'g> SlotPools<'g> {
    /// The pools of type `slot`: [`default_slot_capacity`] plus `grants`.
    fn of(slot: Slot, grants: &'g [SlotGrant]) -> Self {
        let mut pools = SlotPools {
            general: default_slot_capacity(slot),
            restricted: Vec::new(),
        };
        for grant in grants.iter().filter(|grant| grant.slot == slot) {
            match &grant.only {
                None => pools.general = pools.general.saturating_add(grant.count),
                Some(only) => pools.restricted.push(RestrictedPool {
                    room: grant.count,
                    only,
                }),
            }
        }
        pools
    }

    /// Place `units` of `code`'s claim, restricted pools that admit it first
    /// (in grant order), then the general pool. Returns the units no pool
    /// could hold.
    ///
    /// With at most one restricted pool per slot type this packing is exact.
    /// With several, a unit goes to the first pool that admits it, which can
    /// miss a packing two overlapping filters would allow. No card in scope
    /// grants a second restricted pool.
    fn place(&mut self, reg: &CardRegistry, code: &CardCode, units: u8) -> u8 {
        let mut left = units;
        for pool in &mut self.restricted {
            if card_registry::card_matches(reg, code, pool.only) {
                left = fill(&mut pool.room, left);
            }
        }
        fill(&mut self.general, left)
    }
}

/// Put as many of `units` into `room` as fit; return the rest.
fn fill(room: &mut u8, units: u8) -> u8 {
    let held = units.min(*room);
    *room -= held;
    units - held
}

/// For each slot type: how many slot units `assets` hold beyond what the
/// default slots plus `grants` can pack. Only types with a positive shortfall
/// are present.
///
/// Each asset is placed in turn ([`SlotPools::place`]); the units nothing
/// could hold are the overflow, summed per slot type.
fn shortfall(reg: &CardRegistry, grants: &[SlotGrant], assets: &[CardCode]) -> SlotCounts {
    let needs: Vec<(&CardCode, SlotCounts)> = assets
        .iter()
        .map(|code| (code, card_slot_need(reg, code)))
        .collect();
    let slot_types: BTreeSet<Slot> = needs
        .iter()
        .flat_map(|(_, need)| need.keys().copied())
        .collect();
    slot_types
        .into_iter()
        .filter_map(|slot| {
            let mut pools = SlotPools::of(slot, grants);
            let overflow = needs
                .iter()
                .filter_map(|(code, need)| need.get(&slot).map(|&units| (code, units)))
                .map(|(code, units)| pools.place(reg, code, units))
                .fold(0u8, u8::saturating_add);
            (overflow > 0).then_some((slot, overflow))
        })
        .collect()
}

/// The slot multiset `code` needs to enter play, read from `cards`. Empty
/// when the code is unknown, or it is a non-asset / slot-less asset.
pub(super) fn card_slot_need(cards: &CardRegistry, code: &CardCode) -> SlotCounts {
    (cards.metadata_for)(code)
        .map(|meta| count_slots(meta.slots()))
        .unwrap_or_default()
}

/// The slot type `code` needs more of than `investigator` has capacity for
/// (default plus [`slot_grants`]), or `None` if it can fit (possibly after
/// discarding occupiers). Empty-need cards (slot-less, non-asset, unknown)
/// always return `None`.
///
/// An occupier that [cannot leave play](abilities_in_effect::cannot_leave_play)
/// is fixed, since make-room may not discard it, so `code` must fit beside
/// those. The Necronomicon 01009 holding a hand slot leaves one for a new
/// asset, so a two-handed one cannot be played until its horror is gone.
pub(super) fn unsatisfiable_slot(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
) -> Option<Slot> {
    let grants = slot_grants(state, cards, investigator);
    let mut assets: Vec<CardCode> = occupants(state, investigator)
        .into_iter()
        .filter(|(inst, _)| abilities_in_effect::cannot_leave_play(state, cards, *inst))
        .map(|(_, fixed)| fixed)
        .collect();
    assets.push(code.clone());
    let need = card_slot_need(cards, code);
    shortfall(cards, &grants, &assets)
        .into_keys()
        .find(|slot| need.contains_key(slot))
}

/// `investigator`'s slot-holding cards: their play area, then their threat
/// area (`glossary/Slots.md`). The investigator card is in neither, so it is
/// correctly excluded; slot-less and non-asset cards claim nothing.
fn occupants(state: &GameState, investigator: InvestigatorId) -> Vec<(CardInstanceId, CardCode)> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    inv.play_and_threat_area()
        .map(|card| (card.instance_id, card.code.clone()))
        .collect()
}

/// The shortfall, over the slot types `code` needs, if `investigator` held
/// their [`occupants`] minus `removed`, plus `code`. A `removed` card's own
/// grants go with it.
fn shortfall_without(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
    removed: Option<CardInstanceId>,
) -> SlotCounts {
    let need = card_slot_need(cards, code);
    if need.is_empty() {
        return SlotCounts::new();
    }
    let grants: Vec<SlotGrant> = slot_grants(state, cards, investigator)
        .into_iter()
        .filter(|grant| Some(grant.source) != removed)
        .collect();
    let mut assets: Vec<CardCode> = occupants(state, investigator)
        .into_iter()
        .filter(|(inst, _)| Some(*inst) != removed)
        .map(|(_, occupant)| occupant)
        .collect();
    assets.push(code.clone());
    shortfall(cards, &grants, &assets)
        .into_iter()
        .filter(|(slot, _)| need.contains_key(slot))
        .collect()
}

/// Per-type shortfall for playing `code` now, over the slot types it needs.
/// Empty when the asset fits without discarding (or the code is unknown).
pub(super) fn slot_deficit(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
) -> SlotCounts {
    shortfall_without(state, cards, investigator, code, None)
}

/// The assets eligible to be discarded to make room for `code`: each of
/// `investigator`'s slot-holding cards whose discard reduces `deficit` (the
/// current [`slot_deficit`]) in some slot type. A Tome in a Tome-only slot is
/// not a candidate when a non-Tome needs the room, since its slot is one the
/// new asset could not use. With no grants in play this is every asset
/// occupying a deficit slot type.
///
/// A card that [cannot leave play](abilities_in_effect::cannot_leave_play) is
/// never a candidate: Crypt Chill 01167's ruling reads a chosen discard that
/// way (<https://arkhamdb.com/card/01167>), *"You cannot choose and discard The
/// Necronomicon or Baron Samedi because they 'cannot leave play'."*
///
/// Returned in play-area order, then threat-area order, so an `OptionId` index
/// is stable between the prompt and its resume.
pub(super) fn make_room_candidates(
    state: &GameState,
    reg: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
    deficit: &SlotCounts,
) -> Vec<(CardInstanceId, CardCode)> {
    occupants(state, investigator)
        .into_iter()
        .filter(|(inst, _)| !abilities_in_effect::cannot_leave_play(state, reg, *inst))
        .filter(|(inst, _)| {
            let after = shortfall_without(state, reg, investigator, code, Some(*inst));
            deficit
                .iter()
                .any(|(slot, &short)| after.get(slot).copied().unwrap_or(0) < short)
        })
        .collect()
}

/// Bring the asset **instance** `card` (held by the caller, in no zone) into
/// the area of `investigator`'s that `entry` names, discarding occupying
/// assets to make room per RR p.19 (#498).
///
/// **Three entry paths, one prompt** (#772, #1097). A play from hand mints the
/// instance and hands it over at RR Appendix I step 3; a
/// [`TakeControl`](card_dsl::dsl::Effect::TakeControl) lifts the instance already
/// on the board. Slot pressure applies to both, and `glossary/Slots.md` says so
/// in one breath: *"If playing **or gaining control** of an asset would put an
/// investigator above his or her slot limit for that type of asset, the
/// investigator must choose and discard other assets under his or her control
/// simultaneously with the new asset entering the slot."* The third is a
/// [`PutIntoThreatArea`](card_dsl::dsl::Effect::PutIntoThreatArea) of a card
/// with a slot symbol (The Necronomicon 01009), which lands in the threat
/// area: `glossary/Put_into_Play.md` holds a card put into play to *"the
/// standard game rules associated with playing or drawing (for encounter
/// cards) that card."*
///
/// Recursive:
///
/// - no deficit → enter directly;
/// - a deficit with exactly one candidate → auto-discard it (forced) and recurse;
/// - a deficit with 2+ candidates → suspend for a player `PickSingle`.
///
/// `check_play_card`'s slot gate ([`unsatisfiable_slot`]) means the asset fits
/// on its own, so while a shortfall remains some occupier's discard reduces it
/// and the recursion makes progress and terminates. Should none qualify — the
/// greedy packing missing a layout, or an occupier that may not be discarded —
/// the entry rejects rather than discarding something that frees nothing, and
/// the apply's snapshot rollback undoes the half-made room.
///
/// Nothing here is particular to either area beyond the final landing,
/// [`enter_asset_into_play`](cards::enter_asset_into_play) or
/// [`enter_threat_area`](threat_area::enter_threat_area): the shortfall and
/// candidates are asked of the investigator and the entering card's code. The
/// prompt's frame sits above whatever is driving the entry — a play's frames,
/// or the effect walk of a Revelation under a draw or a deck search — and
/// those resume once it pops.
pub(in crate::engine) fn enter_asset_making_room(
    cx: &mut Cx,
    investigator: InvestigatorId,
    card: CardInPlay,
    entry: AssetEntry,
) -> EngineOutcome {
    let deficit = slot_deficit(cx.state, cx.cards, investigator, &card.code);
    if deficit.is_empty() {
        match entry {
            AssetEntry::PlayedFromHand | AssetEntry::ControlTaken => {
                cards::enter_asset_into_play(cx, investigator, card, entry);
            }
            AssetEntry::PutIntoThreatArea => {
                threat_area::enter_threat_area(cx, investigator, card);
            }
        }
        return EngineOutcome::Done;
    }
    let candidates = make_room_candidates(cx.state, cx.cards, investigator, &card.code, &deficit);
    match candidates.as_slice() {
        [] => EngineOutcome::Rejected {
            reason: format!(
                "{code}: no asset can be discarded to make room for it \
                 (slots needed: {deficit:?}; RR p.19)",
                code = card.code,
            )
            .into(),
        },
        &[(inst, _)] => {
            board::discard_from_play(cx, inst).expect("slot make-room: the candidate is in play");
            enter_asset_making_room(cx, investigator, card, entry)
        }
        _ => {
            // Genuine choice: the player picks which occupier to discard. The
            // asset stays mid-play, riding this frame across the prompt.
            let request = slot_discard_request(cx, investigator, &candidates, &deficit);
            cx.state.continuations.push(SlotDiscardFrame {
                investigator,
                card: Some(card),
                entry,
            });
            EngineOutcome::AwaitingInput {
                request,
                resume_token: ResumeToken(0),
            }
        }
    }
}

/// The `PickSingle` over the make-room `candidates`.
fn slot_discard_request(
    cx: &Cx,
    investigator: InvestigatorId,
    candidates: &[(CardInstanceId, CardCode)],
    deficit: &SlotCounts,
) -> InputRequest {
    let prompt = format!(
        "Investigator {investigator:?}: choose an asset to discard to make room \
         (slots needed: {deficit:?})."
    );
    InputRequest::pick_single(
        prompt,
        choice::candidate_options(cx.state, cx.cards, candidates, |(inst, _)| {
            OptionTarget::CardInstance(*inst)
        }),
    )
}

/// Resume a slot make-room choice: discard the chosen occupier, then continue
/// making room (re-prompt if still contested, auto-discard a forced last
/// candidate, or enter the pending asset once the deficit clears). An invalid
/// pick rejects and keeps the frame (the `DealDamage` / `HunterMove`
/// contract).
pub(super) fn resume_slot_discard(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let SlotDiscardFrame {
        investigator,
        card,
        entry,
    } = cx
        .state
        .continuations
        .top_expect::<SlotDiscardFrame>()
        .clone();
    let Some(card) = card else {
        // Elimination is the only thing that empties a `SlotDiscard` frame (see
        // `Continuation::take_play_in_progress`), and it cannot run while this
        // prompt is outstanding: the frame awaits input, so every action but its
        // own `ResolveInput` is refused.
        unreachable!(
            "resume_slot_discard: the mid-play asset left the frame while its \
             make-room prompt was open"
        );
    };
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: slot make-room expects PickSingle, got {response:?}")
                .into(),
        };
    };
    let deficit = slot_deficit(cx.state, cx.cards, investigator, &card.code);
    let candidates = make_room_candidates(cx.state, cx.cards, investigator, &card.code, &deficit);
    let Some(&(inst, _)) = candidates.get(*i as usize) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: slot make-room option {i} out of range (0..{})",
                candidates.len()
            )
            .into(),
        };
    };
    // Valid: pop the frame we validated against, discard the choice, continue.
    cx.state.continuations.pop_expect::<SlotDiscardFrame>();
    board::discard_from_play(cx, inst).expect("slot make-room: the candidate is in play");
    enter_asset_making_room(cx, investigator, card, entry)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use card_dsl::card_data::{CardKind, CardMetadata, CardType, Class, SkillIcons};

    use super::*;

    /// A synthetic trait the restricted-pool probes filter on.
    const PROBE_TRAIT: &str = "_SlotProbe";
    /// One hand, no traits.
    const HAND: &str = "_slot_hand";
    /// Two hands, no traits.
    const TWO_HANDED: &str = "_slot_two_handed";
    /// One ally slot.
    const ALLY: &str = "_slot_ally";
    /// Two ally slots: needs more than an investigator has on its own.
    const TWO_ALLY: &str = "_slot_two_ally";
    /// One hand, carrying [`PROBE_TRAIT`].
    const TRAITED_HAND: &str = "_slot_traited_hand";

    fn asset(code: &str, slots: &[Slot], traits: &[&str]) -> CardMetadata {
        CardMetadata {
            code: code.to_owned(),
            name: code.to_owned(),
            traits: traits.iter().map(|t| (*t).to_owned()).collect(),
            text: None,
            back_name: None,
            back_text: None,
            pack_code: "_test".to_owned(),
            weakness: false,
            kind: CardKind::Asset {
                class: Class::Neutral,
                cost: Some(0),
                xp: None,
                slots: slots.to_vec(),
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: false,
                deck_limit: 2,
                uses: None,
                play_only_during_turn: false,
            },
        }
    }

    fn probe_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
        static PROBES: LazyLock<Vec<CardMetadata>> = LazyLock::new(|| {
            vec![
                asset(HAND, &[Slot::Hand], &[]),
                asset(TWO_HANDED, &[Slot::Hand, Slot::Hand], &[]),
                asset(ALLY, &[Slot::Ally], &[]),
                asset(TWO_ALLY, &[Slot::Ally, Slot::Ally], &[]),
                asset(TRAITED_HAND, &[Slot::Hand], &[PROBE_TRAIT]),
            ]
        });
        PROBES.iter().find(|meta| meta.code == code.as_str())
    }

    const PROBES: CardRegistry = CardRegistry {
        metadata_for: probe_metadata_for,
        ..CardRegistry::EMPTY
    };

    fn codes(codes: &[&str]) -> Vec<CardCode> {
        codes.iter().copied().map(CardCode::new).collect()
    }

    /// `count` extra hand slots, holding only what `only` admits.
    fn hand_grant(count: u8, only: Option<CardFilter>) -> SlotGrant {
        SlotGrant {
            source: CardInstanceId(1),
            slot: Slot::Hand,
            count,
            only,
        }
    }

    fn probe_only() -> CardFilter {
        CardFilter::with_trait(PROBE_TRAIT).and_kind(CardType::Asset)
    }

    #[test]
    fn capacity_matches_rr_defaults() {
        assert_eq!(default_slot_capacity(Slot::Accessory), 1);
        assert_eq!(default_slot_capacity(Slot::Body), 1);
        assert_eq!(default_slot_capacity(Slot::Ally), 1);
        assert_eq!(default_slot_capacity(Slot::Hand), 2);
        assert_eq!(default_slot_capacity(Slot::Arcane), 2);
        assert_eq!(default_slot_capacity(Slot::Tarot), 1);
    }

    #[test]
    fn count_slots_tallies_multiset() {
        assert!(count_slots(&[]).is_empty());
        assert_eq!(count_slots(&[Slot::Ally]).get(&Slot::Ally), Some(&1));
        assert_eq!(
            count_slots(&[Slot::Hand, Slot::Hand]).get(&Slot::Hand),
            Some(&2)
        );
    }

    #[test]
    fn no_shortfall_when_room_exists() {
        // Ally cap 1, one Ally → fits.
        assert!(shortfall(&PROBES, &[], &codes(&[ALLY])).is_empty());
        // Hand cap 2, two single-Hand assets → fits.
        assert!(shortfall(&PROBES, &[], &codes(&[HAND, HAND])).is_empty());
    }

    #[test]
    fn shortfall_one_when_cap_one_slot_full() {
        let short = shortfall(&PROBES, &[], &codes(&[ALLY, ALLY]));
        assert_eq!(short.get(&Slot::Ally), Some(&1));
    }

    #[test]
    fn shortfall_for_two_handed_over_full_hands() {
        let short = shortfall(&PROBES, &[], &codes(&[HAND, HAND, TWO_HANDED]));
        assert_eq!(short.get(&Slot::Hand), Some(&2));
        let short = shortfall(&PROBES, &[], &codes(&[HAND, HAND, HAND]));
        assert_eq!(short.get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn a_lone_need_over_capacity_is_short() {
        // need Hand:2 == cap 2 on its own → satisfiable.
        assert!(shortfall(&PROBES, &[], &codes(&[TWO_HANDED])).is_empty());
        // need Ally:2 > cap 1 on its own → unsatisfiable.
        let short = shortfall(&PROBES, &[], &codes(&[TWO_ALLY]));
        assert_eq!(short.get(&Slot::Ally), Some(&1));
    }

    #[test]
    fn a_restricted_pool_holds_what_it_admits() {
        let grants = [hand_grant(2, Some(probe_only()))];
        // Two traited hands in the restricted pool, two plain in the defaults.
        let assets = codes(&[HAND, HAND, TRAITED_HAND, TRAITED_HAND]);
        assert!(shortfall(&PROBES, &grants, &assets).is_empty());
        // A third traited hand spills into the general pool, which is full.
        let assets = codes(&[HAND, HAND, TRAITED_HAND, TRAITED_HAND, TRAITED_HAND]);
        let short = shortfall(&PROBES, &grants, &assets);
        assert_eq!(short.get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn a_restricted_pool_refuses_what_it_does_not_admit() {
        let grants = [hand_grant(2, Some(probe_only()))];
        let short = shortfall(&PROBES, &grants, &codes(&[HAND, HAND, HAND]));
        assert_eq!(short.get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn an_unrestricted_grant_widens_the_general_pool() {
        let grants = [hand_grant(1, None)];
        assert!(shortfall(&PROBES, &grants, &codes(&[HAND, HAND, HAND])).is_empty());
        let short = shortfall(&PROBES, &grants, &codes(&[HAND, HAND, TWO_HANDED]));
        assert_eq!(short.get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn place_fills_restricted_pools_first_and_returns_the_overflow() {
        let grants = [hand_grant(1, Some(probe_only()))];
        let mut pools = SlotPools::of(Slot::Hand, &grants);
        // One unit to the restricted pool, two to the general pool.
        assert_eq!(pools.place(&PROBES, &CardCode::new(TRAITED_HAND), 3), 0);
        // Everything is full: a plain hand overflows.
        assert_eq!(pools.place(&PROBES, &CardCode::new(HAND), 1), 1);
    }
}
