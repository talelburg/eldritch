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

use std::collections::BTreeMap;

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

/// One [`Effect::AdditionalSlots`] grant in effect: its own pool of `count`
/// slots of type `slot`, holding only what `only` admits.
#[derive(Debug, Clone)]
struct SlotGrant {
    /// The card the grant is printed on, so a make-room discard of that card
    /// is counted as losing the grant too.
    source: CardInstanceId,
    slot: Slot,
    count: u8,
    only: Option<CardFilter>,
}

/// One asset's claim on slots: the units it takes of each type and, for each
/// grant (index-aligned with the grants it was built against), whether that
/// grant's pool may hold it.
#[derive(Debug, Clone)]
struct SlotClaim {
    units: SlotCounts,
    admitted: Vec<bool>,
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

/// `code`'s claim on slots against `grants`. Empty units when the code is
/// unknown, or it is a non-asset / slot-less asset.
fn claim_of(reg: &CardRegistry, grants: &[SlotGrant], code: &CardCode) -> SlotClaim {
    SlotClaim {
        units: card_slot_need(reg, code),
        admitted: grants
            .iter()
            .map(|grant| {
                grant
                    .only
                    .as_ref()
                    .is_none_or(|filter| card_registry::card_matches(reg, code, filter))
            })
            .collect(),
    }
}

/// For each slot type: how many slot units `claims` hold beyond what the
/// default slots plus `grants` can pack. Only types with a positive shortfall
/// are present.
///
/// The packing is greedy: each restricted pool (in grant order) fills with the
/// units it admits, then the default slots and the unrestricted pools take
/// the rest. With at most one restricted pool per slot type that is exact. With
/// several, a unit is placed in the first pool that admits it, which can miss
/// a packing two overlapping filters would allow. No card in scope grants a
/// second restricted pool.
fn shortfall(grants: &[SlotGrant], claims: &[SlotClaim]) -> SlotCounts {
    let slot_types: std::collections::BTreeSet<Slot> = claims
        .iter()
        .flat_map(|claim| claim.units.keys().copied())
        .collect();
    let mut short = SlotCounts::new();
    for slot in slot_types {
        let mut remaining: Vec<u8> = claims
            .iter()
            .map(|claim| claim.units.get(&slot).copied().unwrap_or(0))
            .collect();
        let mut open = default_slot_capacity(slot);
        for (g, grant) in grants.iter().enumerate() {
            if grant.slot != slot {
                continue;
            }
            if grant.only.is_none() {
                open = open.saturating_add(grant.count);
                continue;
            }
            let mut room = grant.count;
            for (claim, left) in claims.iter().zip(remaining.iter_mut()) {
                if claim.admitted[g] {
                    let take = room.min(*left);
                    room -= take;
                    *left -= take;
                }
            }
        }
        let held = remaining.iter().fold(0u8, |sum, &n| sum.saturating_add(n));
        let over = held.saturating_sub(open);
        if over > 0 {
            short.insert(slot, over);
        }
    }
    short
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
/// always return `None`. Unreachable in the current corpus (max need is
/// `Hand×2` = default cap 2); exists for no-silent-approximation.
pub(super) fn unsatisfiable_slot(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
) -> Option<Slot> {
    let grants = slot_grants(state, cards, investigator);
    let alone = claim_of(cards, &grants, code);
    let need = alone.units.clone();
    shortfall(&grants, &[alone])
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
    inv.cards_in_play
        .iter()
        .chain(inv.threat_area.iter())
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
    let mut claims: Vec<SlotClaim> = occupants(state, investigator)
        .iter()
        .filter(|(inst, _)| Some(*inst) != removed)
        .map(|(_, occupant)| claim_of(cards, &grants, occupant))
        .collect();
    claims.push(claim_of(cards, &grants, code));
    shortfall(&grants, &claims)
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
    use super::*;

    /// A claim on `slots` that the grants admit per `admitted`.
    fn claim(slots: &[Slot], admitted: &[bool]) -> SlotClaim {
        SlotClaim {
            units: count_slots(slots),
            admitted: admitted.to_vec(),
        }
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
        assert!(shortfall(&[], &[claim(&[Slot::Ally], &[])]).is_empty());
        // Hand cap 2, two single-Hand assets → fits.
        let hands = [claim(&[Slot::Hand], &[]), claim(&[Slot::Hand], &[])];
        assert!(shortfall(&[], &hands).is_empty());
    }

    #[test]
    fn shortfall_one_when_cap_one_slot_full() {
        let allies = [claim(&[Slot::Ally], &[]), claim(&[Slot::Ally], &[])];
        assert_eq!(shortfall(&[], &allies).get(&Slot::Ally), Some(&1));
    }

    #[test]
    fn shortfall_for_two_handed_over_full_hands() {
        let one_hand = claim(&[Slot::Hand], &[]);
        let two_handed = claim(&[Slot::Hand, Slot::Hand], &[]);
        let claims = [one_hand.clone(), one_hand.clone(), two_handed];
        assert_eq!(shortfall(&[], &claims).get(&Slot::Hand), Some(&2));
        let claims = [one_hand.clone(), one_hand.clone(), one_hand];
        assert_eq!(shortfall(&[], &claims).get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn a_lone_need_over_capacity_is_short() {
        // need Hand:2 == cap 2 on its own → satisfiable.
        assert!(shortfall(&[], &[claim(&[Slot::Hand, Slot::Hand], &[])]).is_empty());
        // need Ally:2 > cap 1 on its own → unsatisfiable.
        assert_eq!(
            shortfall(&[], &[claim(&[Slot::Ally, Slot::Ally], &[])]).get(&Slot::Ally),
            Some(&1)
        );
    }
}
