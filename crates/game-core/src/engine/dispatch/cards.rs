//! Card-related dispatch handlers: deck management, drawing, mulligan,
//! resource grants, and card play.

use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::Trigger;
use crate::event::Event;
use crate::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Phase, Status, Zone};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::EngineOutcome;
use super::Cx;

/// Starting hand size at scenario setup. Per the Rules Reference,
/// each investigator draws 5 cards before mulligan.
pub(super) const INITIAL_HAND_SIZE: u8 = 5;

/// Handler for [`EngineRecord::DeckShuffled`].
///
/// Permutes the named investigator's player deck via the deterministic
/// RNG and emits [`Event::DeckShuffled`]. Empty decks are a silent
/// no-op (no event emitted) — there's nothing to shuffle.
pub(super) fn deck_shuffled(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if !cx.state.investigators.contains_key(&investigator) {
        return EngineOutcome::Rejected {
            reason: format!("DeckShuffled: investigator {investigator:?} is not in state").into(),
        };
    }
    shuffle_player_deck(cx, investigator);
    EngineOutcome::Done
}

/// Fisher-Yates shuffle of the named investigator's deck using the
/// shared deterministic RNG. Used by [`deck_shuffled`] and by
/// scenario setup (initial-hand draw).
///
/// Emits [`Event::DeckShuffled`] iff the deck had at least 2 cards
/// (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_player_deck(cx: &mut Cx, investigator: InvestigatorId) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
            "shuffle_player_deck: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
        });
    if inv.deck.len() < 2 {
        return;
    }
    // Fisher-Yates: walk from the end, swap each element with one in
    // [0, i]. `next_index(n)` returns `[0, n)`, so we pass i+1.
    let deck_len = inv.deck.len();
    // Collect swap indices first, then apply — avoids holding a
    // mutable borrow on `inv.deck` across the RNG calls. (next_index
    // takes &mut state.rng, which conflicts with the &mut borrow we
    // already have on the investigator if we did this inline.)
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = cx.state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("checked");
    for (a, b) in swaps {
        inv.deck.swap(a, b);
    }
    cx.events.push(Event::DeckShuffled { investigator });
}

/// Draw up to `count` cards from the named investigator's deck top
/// into their hand. Stops early (without panic) if the deck runs out
/// — this helper is just the structural move; reshuffle / horror
/// penalty logic for an empty deck lives in [`draw`].
///
/// Emits a single [`Event::CardsDrawn`] with the actually-drawn
/// count, even if that's zero. A zero-count draw is informative for
/// consumers tracking the attempt.
pub(super) fn draw_cards(cx: &mut Cx, investigator: InvestigatorId, count: u8) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "draw_cards: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    let drawn = std::cmp::min(count as usize, inv.deck.len());
    // Cards are drawn from the deck front (top). Splice out the first
    // `drawn` cards in order and append to hand.
    let drawn_cards: Vec<_> = inv.deck.drain(..drawn).collect();
    inv.hand.extend(drawn_cards);
    // `drawn` ≤ `count: u8`, so the cast can't overflow.
    let drawn_u8 = u8::try_from(drawn).expect("drawn <= count <= u8::MAX");
    cx.events.push(Event::CardsDrawn {
        investigator,
        count: drawn_u8,
    });
}

/// Grant `amount` resources to `investigator`: saturating-add to the
/// wallet and emit [`Event::ResourcesGained`]. The resource-grant core
/// shared by the DSL `gain_resources` (called after target resolution)
/// and Upkeep step 4.4. No-op (no event) when `amount == 0`, matching
/// the existing `gain_resources` zero-amount behavior.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(crate) fn grant_resources(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    if amount == 0 {
        return;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("grant_resources: caller guarantees investigator exists");
    inv.resources = inv.resources.saturating_add(amount);
    cx.events.push(Event::ResourcesGained {
        investigator,
        amount,
    });
}

/// Reshuffle the discard pile back into the deck for the named
/// investigator. Used by [`draw`] when the deck runs empty. Drains
/// `discard` into `deck`, then calls [`shuffle_player_deck`] (which
/// emits [`Event::DeckShuffled`] when ≥ 2 cards land in the deck).
fn reshuffle_discard_into_deck(cx: &mut Cx, investigator: InvestigatorId) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "reshuffle_discard_into_deck: investigator {investigator:?} is not in the \
             investigators map; this is a state-corruption invariant violation"
            )
        });
    let cards: Vec<_> = inv.discard.drain(..).collect();
    inv.deck.extend(cards);
    shuffle_player_deck(cx, investigator);
}

/// Draw one card for `investigator`, applying the empty-deck rule:
/// reshuffle the discard into the deck if the deck is empty, draw,
/// and take 1 horror on any would-draw-from-empty. Extracted verbatim
/// from the `Draw` action body so the action and Upkeep step 4.4 share
/// one code path. The deck-out reading (horror on would-draw-from-empty;
/// no reshuffle of a zero-card discard per Rules Reference p.9) is
/// inherited unchanged.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(super) fn draw_one_with_deckout(cx: &mut Cx, investigator: InvestigatorId) {
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .expect("draw_one_with_deckout: caller guarantees investigator exists");
    let deck_empty = inv.deck.is_empty();
    let discard_empty = inv.discard.is_empty();
    if deck_empty {
        if !discard_empty {
            reshuffle_discard_into_deck(cx, investigator);
        }
        // After the (possibly no-op) reshuffle, attempt the draw.
        // draw_cards handles a still-empty deck by emitting
        // CardsDrawn { count: 0 } without moving cards.
        draw_cards(cx, investigator, 1);
        // Horror penalty fires on any "would-draw-from-empty-deck"
        // (the reshuffle did happen if discard was non-empty; if it
        // was also empty, the rules don't strictly require horror
        // but we apply it as the safer reading).
        super::elimination::take_horror(cx, investigator, 1);
    } else {
        draw_cards(cx, investigator, 1);
    }
}

/// Handler for [`PlayerAction::Draw`].
///
/// Validate-first: Investigation phase, investigator is active and
/// `Status::Active`, has at least 1 action remaining. Then spend the
/// action and resolve the draw per the Rules Reference:
///
/// - **Non-empty deck**: draw 1 to hand.
/// - **Empty deck, non-empty discard**: shuffle discard into deck,
///   draw 1, then take 1 horror — the horror penalty fires when an
///   investigator with an empty deck needs to draw.
/// - **Both empty**: no shuffle (per the Rules Reference's "any
///   ability that would shuffle a discard pile of zero cards back
///   into a deck does not shuffle the deck"), no card drawn — but
///   the 1 horror still applies. The rules don't explicitly address
///   this corner case; we apply the horror as the safer reading
///   ("would-draw-from-empty triggers the penalty"), and the case
///   is rare enough in practice (only high-cycle decks burn through
///   both zones) that the difference is mostly theoretical.
///
/// The draw logic itself is delegated to [`draw_one_with_deckout`].
pub(super) fn draw(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw: {investigator:?} is not the active investigator ({:?})",
                cx.state.active_investigator,
            )
            .into(),
        };
    }
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "Draw: active_investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Draw requires at least 1 action point".into(),
        };
    }

    // Mutate.
    super::actions::spend_one_action(cx, investigator);
    draw_one_with_deckout(cx, investigator);
    EngineOutcome::Done
}

/// Handler for [`PlayerAction::Mulligan`].
///
/// Per the Rules Reference, the redrawn cards shuffle directly back
/// into the deck (not via the discard pile). Validates that it is this
/// investigator's turn to mulligan (`mulligan_pending == Some(investigator)`,
/// Rules Reference p.16 player order) and that the redraw indices are in
/// bounds and unique.
///
/// On success: move named hand cards to the deck, shuffle, draw the
/// same count back, advance `mulligan_pending` to the next investigator
/// in player order, emit `MulliganPerformed`. An empty `indices_to_redraw`
/// is a legal "keep my hand" mulligan that consumes the turn without
/// touching the deck.
pub(super) fn mulligan(
    cx: &mut Cx,
    investigator: InvestigatorId,
    indices_to_redraw: &[u8],
) -> EngineOutcome {
    // One check subsumes the three old ones: the cursor only ever holds
    // an Active `turn_order` id, so a mismatch covers setup-over (`None`),
    // wrong-player / too-early, and already-went (cursor moved past you).
    if cx.state.mulligan_pending != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Mulligan: it is not {investigator:?}'s turn to mulligan \
                 (pending: {:?})",
                cx.state.mulligan_pending,
            )
            .into(),
        };
    }
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "mulligan_pending {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    // Validate indices: each must be in bounds and unique.
    let hand_len = inv.hand.len();
    for &idx in indices_to_redraw {
        if usize::from(idx) >= hand_len {
            return EngineOutcome::Rejected {
                reason: format!("Mulligan: hand_index {idx} out of bounds (hand size {hand_len})")
                    .into(),
            };
        }
    }
    let mut sorted: Vec<usize> = indices_to_redraw.iter().map(|&i| usize::from(i)).collect();
    sorted.sort_unstable();
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: duplicate index in {indices_to_redraw:?}").into(),
        };
    }

    // Mutate.
    let redrawn_count = u8::try_from(indices_to_redraw.len())
        .expect("indices_to_redraw.len() <= hand.len() <= u8::MAX in practice");
    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("checked");
    // Walk indices high-to-low so smaller positions remain valid as
    // we remove. Move named cards directly into the deck — they
    // shuffle back in per the rules, not through the discard pile.
    for &i in sorted.iter().rev() {
        let card = inv_mut.hand.remove(i);
        inv_mut.deck.push(card);
    }
    // If anything actually moved, shuffle the deck (which now contains
    // the redrawn cards mixed with the rest) and draw replacements.
    // For an empty "keep my hand" mulligan, skip both — there's
    // nothing to put back, so no shuffle and no draw.
    if redrawn_count > 0 {
        shuffle_player_deck(cx, investigator);
        draw_cards(cx, investigator, redrawn_count);
    }
    cx.events.push(Event::MulliganPerformed {
        investigator,
        redrawn_count,
    });
    // Advance to the next Active investigator in player order (or `None`
    // when this was the last). The completion check in
    // `apply_player_action` keys off `None` to end setup.
    cx.state.mulligan_pending =
        super::cursor::next_active_investigator_after(cx.state, investigator);
    EngineOutcome::Done
}

/// Resolve the card's destination + abilities via the registry, or
/// produce the appropriate rejection.
///
/// Split out so [`play_card`] stays under the function-size lint —
/// and because the registry-side validations are conceptually
/// separate from the state-side prefix.
pub(super) fn resolve_play_target(
    code: &CardCode,
) -> Result<
    (
        super::PlayDestination,
        Vec<crate::dsl::Ability>,
        bool,
        CardType,
    ),
    EngineOutcome,
> {
    let Some(registry) = card_registry::current() else {
        return Err(EngineOutcome::Rejected {
            reason: "PlayCard: no card registry installed; engine cannot resolve card \
                     metadata or abilities. Install game_core::card_registry before \
                     dispatching PlayCard."
                .into(),
        });
    };
    let Some(metadata) = (registry.metadata_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("PlayCard: unknown card code {code}").into(),
        });
    };
    let is_fast = metadata.is_fast();
    let card_type = metadata.card_type();
    let destination = match card_type {
        CardType::Asset => super::PlayDestination::InPlay,
        CardType::Event => super::PlayDestination::Discard,
        other => {
            return Err(EngineOutcome::Rejected {
                reason: format!(
                    "PlayCard: card_type {other:?} is not playable from hand (card {code})",
                )
                .into(),
            });
        }
    };
    let Some(abilities) = (registry.abilities_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: card {code} has no effect implementation; the deck-import \
                 gate (#73-era) should refuse decks containing unimplemented cards.",
            )
            .into(),
        });
    };
    Ok((destination, abilities, is_fast, card_type))
}

/// Handler for [`PlayerAction::PlayCard`].
///
/// Validates the standard player-action prefix, looks up the card's
/// metadata and abilities via the installed [`card_registry`], routes
/// the card to its destination zone based on its
/// [`CardType`](crate::card_data::CardType), and runs every
/// [`Trigger::OnPlay`] ability through the DSL evaluator.
///
/// # Timing gate
///
/// The gate branches on `is_fast` (from [`CardMetadata`](crate::card_data::CardMetadata))
/// and [`CardType`](crate::card_data::CardType), per Rules Reference p. 11:
///
/// - **Non-Fast cards** (asset or event without the ⚡ icon): require
///   Investigation phase + the active investigator. The standard
///   "your turn, your action" constraint.
///
/// - **Fast events** (Rules Reference p. 11: *"A fast event card may be
///   played from a player's hand any time its play instructions
///   specify"*): permitted when `active_during_investigation` OR when
///   the top open window's `fast_actors` scope permits the acting
///   investigator. Any eligible investigator in a permissive window
///   qualifies — card-level "Play only during your turn" constraints
///   (e.g. Working a Hunch 01037) are a separate per-card concern
///   **not** enforced here.
///
/// - **Fast assets** (Rules Reference p. 11: *"A fast asset may be
///   played by an investigator during any player window on his or her
///   turn"*): the "his or her turn" clause restricts to the **owner**,
///   modeled as the active investigator. Permitted when
///   `active_during_investigation` OR when the owner is the active
///   investigator AND the top open window permits them. Non-owner plays
///   remain illegal even in a permissive window.
///
/// Card-level play constraints (e.g. "Play only during your turn",
/// "Play only if …") are **not** enforced by this gate; they are a
/// future per-card concern.
///
/// # Ordering
///
/// [`Event::CardPlayed`] fires first (the play *causes* any on-play
/// effects, so it's correct for the play event to precede the
/// effects' own events in the stream). Then each [`Trigger::OnPlay`]
/// ability runs through [`apply_effect`]; if any returns non-`Done`,
/// the handler propagates that outcome. Finally the card moves out
/// of `hand` — into `cards_in_play` for assets / investigators, or
/// into `discard` (with an emitted [`Event::CardDiscarded`]) for
/// events.
///
/// # State-mutation contract
///
/// A mid-resolution reject here (an `OnPlay` effect returning non-`Done`
/// after [`Event::CardPlayed`] and earlier effects have committed) is
/// rolled back at the `apply` boundary — see [`apply`](crate::engine::apply)'s
/// "Handler contract". No per-handler rollback is needed.
pub(super) fn play_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: u8,
) -> EngineOutcome {
    let super::PlayCheckResult {
        destination,
        abilities,
        is_fast: _,
        card_type,
    } = match super::reaction_windows::check_play_card(cx.state, investigator, hand_index) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    // Validate-first: a constant restriction may forbid playing this card
    // type (Dissonant Voices 01165: "You cannot play assets or events").
    if let Some(reg) = crate::card_registry::current() {
        if crate::engine::evaluator::play_is_prohibited(cx.state, reg, investigator, card_type) {
            return EngineOutcome::Rejected {
                reason: format!(
                    "PlayCard: {investigator:?} cannot play a {card_type:?} \
                     (a constant restriction forbids it)"
                )
                .into(),
            };
        }
    }
    // The code is re-read from state here so we don't pass it through
    // the result (avoiding the lifetime question). The validator already
    // confirmed the hand_index is in bounds and the investigator exists.
    let idx = usize::from(hand_index);
    let code: CardCode = cx
        .state
        .investigators
        .get(&investigator)
        .expect("checked in validator")
        .hand[idx]
        .clone();

    // Mutate.
    cx.events.push(Event::CardPlayed {
        investigator,
        code: code.clone(),
    });
    let eval_ctx = EvalContext::for_controller(investigator);
    for ability in abilities.iter().filter(|a| a.trigger == Trigger::OnPlay) {
        let outcome = apply_effect(cx, &ability.effect, eval_ctx);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }
    }
    match destination {
        super::PlayDestination::InPlay => {
            let instance_id = CardInstanceId(cx.state.next_card_instance_id);
            cx.state.next_card_instance_id = cx.state.next_card_instance_id.saturating_add(1);
            // Seed the named-uses pool ("ammo") from the asset's printed
            // `uses` before moving the code into the instance.
            let initial_uses = crate::card_registry::current()
                .and_then(|reg| (reg.metadata_for)(&code))
                .and_then(|m| match &m.kind {
                    crate::card_data::CardKind::Asset { uses, .. } => *uses,
                    _ => None,
                });
            let inv_mut = cx
                .state
                .investigators
                .get_mut(&investigator)
                .expect("checked");
            let card = inv_mut.hand.remove(idx);
            let mut in_play = CardInPlay::enter_play(card, instance_id);
            if let Some(u) = initial_uses {
                in_play.uses.insert(u.kind, u.count);
            }
            inv_mut.cards_in_play.push(in_play);
        }
        super::PlayDestination::Discard => {
            let inv_mut = cx
                .state
                .investigators
                .get_mut(&investigator)
                .expect("checked");
            let card = inv_mut.hand.remove(idx);
            inv_mut.discard.push(card.clone());
            cx.events.push(Event::CardDiscarded {
                investigator,
                code: card,
                from: Zone::Hand,
            });
        }
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod grant_resources_tests {
    use super::*;
    use crate::state::InvestigatorId;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn grant_resources_adds_to_wallet_and_emits() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();

        grant_resources(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            2,
        );

        assert_eq!(state.investigators[&id].resources, before + 2);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::ResourcesGained { investigator, amount: 2 } if *investigator == id
        )));
    }

    #[test]
    fn grant_resources_zero_is_silent_noop() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();

        grant_resources(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            0,
        );

        assert_eq!(state.investigators[&id].resources, before);
        assert!(events.is_empty());
    }
}

#[cfg(test)]
mod draw_one_with_deckout_tests {
    use super::*;
    use crate::state::{CardCode, InvestigatorId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn draw_one_with_deckout_empty_deck_reshuffles_and_takes_horror() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck.clear();
        inv.discard = vec![CardCode::new("01000"), CardCode::new("01001")];
        inv.horror = 0;
        let hand_before = inv.hand.len();
        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        draw_one_with_deckout(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
        );

        assert_eq!(
            state.investigators[&id].hand.len(),
            hand_before + 1,
            "drew 1"
        );
        assert_eq!(
            state.investigators[&id].horror, 1,
            "deck-out costs 1 horror"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })));
    }
}
