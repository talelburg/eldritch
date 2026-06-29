//! Asset slot limits (Rules Reference p.19, #498).
//!
//! Slots cap how many asset cards of a given type an investigator may have in
//! play. A full slot does **not** block a play: per RR the player "must choose
//! and discard other assets under his or her control simultaneously with the new
//! asset entering the slot." This module owns the capacity table, the deficit
//! math, and the interactive make-room driver invoked when an asset enters play.

use std::collections::BTreeMap;

use crate::card_data::Slot;

/// Per-type slot counts (a multiset). `BTreeMap` keeps iteration deterministic.
#[allow(dead_code)] // wired in Task 2 (#498)
pub(super) type SlotCounts = BTreeMap<Slot, u8>;

/// The slots normally available to an investigator (Rules Reference p.19):
/// "1 accessory slot · 1 body slot · 1 ally slot · 2 hand slots · 2 arcane
/// slots". `Tarot` is not in the original Core Rules Reference (a later-product
/// slot) and no Core/Dunwich card uses it; we default it to 1 and treat it as
/// unreachable in scope.
///
/// TODO: slot-modifying cards (grant/remove a slot) — none in Core/Dunwich.
/// When the first lands, this becomes a per-investigator query reading their
/// in-play modifiers rather than a flat default.
#[allow(dead_code)] // wired in Task 2 (#498)
pub(super) fn default_slot_capacity(slot: Slot) -> u8 {
    match slot {
        Slot::Accessory | Slot::Body | Slot::Ally | Slot::Tarot => 1,
        Slot::Hand | Slot::Arcane => 2,
    }
}

/// Tally a slot multiset (e.g. a two-handed weapon → `{Hand: 2}`).
#[allow(dead_code)] // wired in Task 2 (#498)
pub(super) fn count_slots(slots: &[Slot]) -> SlotCounts {
    let mut counts = SlotCounts::new();
    for &slot in slots {
        *counts.entry(slot).or_insert(0) += 1;
    }
    counts
}

/// For each slot type the new card needs: `max(0, occupied + need - capacity)`.
/// Only types with a positive deficit are present in the result.
#[allow(dead_code)] // wired in Task 2 (#498)
pub(super) fn deficit_from(occupied: &SlotCounts, need: &SlotCounts) -> SlotCounts {
    let mut deficit = SlotCounts::new();
    for (&slot, &n) in need {
        let cap = default_slot_capacity(slot);
        let occ = occupied.get(&slot).copied().unwrap_or(0);
        let d = occ.saturating_add(n).saturating_sub(cap);
        if d > 0 {
            deficit.insert(slot, d);
        }
    }
    deficit
}

/// The first slot type the card needs more of than the investigator has capacity
/// for — i.e. the play is unsatisfiable even after discarding every occupying
/// asset. `None` when every `need[T] <= cap[T]`. Unreachable in the current
/// corpus (max need is `Hand×2` = cap 2); exists for no-silent-approximation.
#[allow(dead_code)] // wired in Task 2 (#498)
pub(super) fn slot_need_exceeds_capacity(need: &SlotCounts) -> Option<Slot> {
    need.iter()
        .find(|(&slot, &n)| n > default_slot_capacity(slot))
        .map(|(&slot, _)| slot)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn deficit_zero_when_room_exists() {
        // Ally cap 1, none occupied, need 1 → fits.
        let occ = count_slots(&[]);
        let need = count_slots(&[Slot::Ally]);
        assert!(deficit_from(&occ, &need).is_empty());
        // Hand cap 2, one occupied, need 1 → fits.
        let occ = count_slots(&[Slot::Hand]);
        let need = count_slots(&[Slot::Hand]);
        assert!(deficit_from(&occ, &need).is_empty());
    }

    #[test]
    fn deficit_one_when_cap_one_slot_full() {
        // Ally cap 1, one occupied, need 1 → deficit Ally:1.
        let occ = count_slots(&[Slot::Ally]);
        let need = count_slots(&[Slot::Ally]);
        let d = deficit_from(&occ, &need);
        assert_eq!(d.get(&Slot::Ally), Some(&1));
    }

    #[test]
    fn deficit_for_two_handed_over_full_hands() {
        // Hand cap 2, two occupied, need 2 (two-handed weapon) → deficit Hand:2.
        let occ = count_slots(&[Slot::Hand, Slot::Hand]);
        let need = count_slots(&[Slot::Hand, Slot::Hand]);
        assert_eq!(deficit_from(&occ, &need).get(&Slot::Hand), Some(&2));
        // Hand cap 2, two occupied, need 1 → deficit Hand:1.
        let need_one = count_slots(&[Slot::Hand]);
        assert_eq!(deficit_from(&occ, &need_one).get(&Slot::Hand), Some(&1));
    }

    #[test]
    fn need_exceeds_capacity_detects_overflow() {
        // need Hand:2 == cap 2 → satisfiable.
        assert!(slot_need_exceeds_capacity(&count_slots(&[Slot::Hand, Slot::Hand])).is_none());
        // need Ally:2 > cap 1 → unsatisfiable.
        assert_eq!(
            slot_need_exceeds_capacity(&count_slots(&[Slot::Ally, Slot::Ally])),
            Some(Slot::Ally)
        );
    }
}
