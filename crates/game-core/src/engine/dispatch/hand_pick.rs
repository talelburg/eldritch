//! **A subset pick from one investigator's hand** — the skill-test commit
//! window, the setup mulligan and the step 4.5 hand-size discard.
//!
//! Each prompt offers its *candidates*: the hand indices of the cards it may
//! select, in hand order. Option `i` is `candidates[i]`, anchored to that hand
//! card, so a host renders selection on the responder's own hand and nowhere
//! else. The resume re-derives the same list from the same hand (nothing moves
//! a hand card between the prompt and its answer) and maps each selected
//! [`OptionId`] back to its hand index ([`picked`]), so an id is never read as
//! a hand index and no frame stores the list. `hand_discard_cost` follows the
//! same convention for its single pick.

use std::collections::BTreeSet;

use crate::card_registry::CardRegistry;
use crate::engine::dispatch::choice;
use crate::engine::outcome::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use crate::state::{GameState, InvestigatorId};

/// The offered options for `investigator`'s hand cards at `candidates`: option
/// `i` anchored to the hand card at `candidates[i]` and labelled with its name.
pub(super) fn options(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    candidates: &[u8],
) -> Vec<ChoiceOption> {
    choice::candidate_options(state, cards, candidates, |&hand_index| {
        OptionTarget::HandCard {
            investigator,
            hand_index,
        }
    })
}

/// The hand indices `selected` names, in the order it names them: each id
/// indexes `candidates`, the list the prompt offered. `what` prefixes a
/// rejection, which a repeated id or an id past the offered options earns.
pub(super) fn picked(
    selected: &[OptionId],
    candidates: &[u8],
    what: &str,
) -> Result<Vec<u8>, EngineOutcome> {
    let mut seen = BTreeSet::new();
    selected
        .iter()
        .map(|&OptionId(i)| {
            if !seen.insert(i) {
                return Err(EngineOutcome::Rejected {
                    reason: format!("{what}: duplicate option {i}").into(),
                });
            }
            candidates
                .get(i as usize)
                .copied()
                .ok_or_else(|| EngineOutcome::Rejected {
                    reason: format!(
                        "{what}: option {i} out of bounds ({} offered)",
                        candidates.len()
                    )
                    .into(),
                })
        })
        .collect()
}

/// The hand indices of `investigator`'s cards that pass `eligible`, in hand
/// order. Empty if the investigator is not in `state`.
pub(super) fn candidates(
    state: &GameState,
    investigator: InvestigatorId,
    mut eligible: impl FnMut(&crate::state::CardCode) -> bool,
) -> Vec<u8> {
    state
        .investigators
        .get(&investigator)
        .map(|inv| {
            inv.hand
                .iter()
                .enumerate()
                .filter(|(_, code)| eligible(code))
                .map(|(i, _)| u8::try_from(i).expect("a hand fits in u8"))
                .collect()
        })
        .unwrap_or_default()
}
