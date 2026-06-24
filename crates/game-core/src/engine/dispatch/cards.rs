//! Card-related dispatch handlers: deck management, drawing, mulligan,
//! resource grants, and card play.

use crate::action::InputResponse;
use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::Trigger;
use crate::event::Event;
use crate::state::{CardCode, InvestigatorId, Zone};

use super::super::evaluator::{push_effect, EvalContext};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
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
pub(in crate::engine) fn shuffle_player_deck(cx: &mut Cx, investigator: InvestigatorId) {
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
pub(in crate::engine) fn draw_cards(cx: &mut Cx, investigator: InvestigatorId, count: u8) {
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

/// Discard one card chosen at random from `investigator`'s hand, emitting
/// [`Event::CardDiscarded`] (`from: Zone::Hand`) and returning the discarded
/// code. A no-op returning `None` if the hand is empty.
///
/// The random index is drawn through the engine RNG ([`RngState`](crate::rng::RngState)),
/// so it replays deterministically from `(seed, draws)` — no `EngineRecord` is
/// needed (see `EngineRecord`'s doc-comment). Exposed `pub` (re-exported at the
/// crate root) so card-local natives can drive "discard at random from hand"
/// without reaching into the crate-private RNG (agenda 01105's random-discard
/// branch, Axis A #334).
pub fn discard_random_from_hand(cx: &mut Cx, investigator: InvestigatorId) -> Option<CardCode> {
    let inv = cx.state.investigators.get_mut(&investigator)?;
    if inv.hand.is_empty() {
        return None;
    }
    let idx = cx.state.rng.next_index(
        cx.state
            .investigators
            .get(&investigator)
            .expect("present")
            .hand
            .len(),
    );
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("present");
    let card = inv.hand.remove(idx);
    inv.discard.push(card.clone());
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card.clone(),
        from: Zone::Hand,
    });
    Some(card)
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
/// The draw logic itself is delegated to [`draw_primary_effect`] after
/// the attack-of-opportunity loop runs as an
/// [`ActionResolution`](crate::state::Continuation::ActionResolution) frame (#293).
pub(super) fn draw(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if let Err(rejection) = super::actions::validate_basic_action(cx.state, "Draw", investigator) {
        return rejection;
    }

    // Mutate-second: spend the action, then park the draw over its
    // attack-of-opportunity loop (#293). Push the resume frame, then
    // drive the AoO. Draw is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Resign are), so each ready engaged enemy attacks
    // before the card is drawn (RR p.5).
    super::actions::spend_one_action(cx, investigator);
    cx.state
        .continuations
        .push(crate::state::Continuation::ActionResolution {
            investigator,
            resume: crate::state::ActionResume::Draw,
        });
    super::combat::drive_aoo(cx, investigator)
}

/// The draw half of a Draw action, run after its `AoO` loop (#293).
///
/// Draw has no target precondition (unlike Move or Investigate), so
/// there is no secondary precondition re-check here. The `resume_action_resolution`
/// `Status::Active` gate upstream already guarantees the investigator is
/// present and Active; a missing map entry here is therefore a
/// state-corruption invariant violation — it must panic (via
/// `draw_one_with_deckout`'s `expect`), never silently return `Done`.
pub(super) fn draw_primary_effect(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    draw_one_with_deckout(cx, investigator);
    EngineOutcome::Done
}

/// Push a [`Continuation::Mulligan`](crate::state::Continuation::Mulligan)
/// frame over `remaining` and return the [`EngineOutcome::AwaitingInput`] that
/// prompts `remaining[0]` to mulligan. Used by `start_scenario` (first prompt)
/// and [`resume_mulligan`] (re-prompt after a queue pop). `remaining` must be
/// non-empty; callers ensure this.
pub(super) fn prompt_mulligan(cx: &mut Cx, remaining: Vec<InvestigatorId>) -> EngineOutcome {
    let next = remaining[0];
    cx.state
        .continuations
        .push(crate::state::Continuation::Mulligan { remaining });
    EngineOutcome::AwaitingInput {
        request: InputRequest::prompt(format!(
            "Setup mulligan: {next:?} may mulligan; submit InputResponse::PickMultiple with the \
             hand indices (as option ids) to redraw (an empty selection keeps the hand).",
        )),
        resume_token: ResumeToken(0),
    }
}

/// Resume the setup mulligan loop (#348), driving the top
/// [`Continuation::Mulligan`](crate::state::Continuation::Mulligan) frame.
///
/// The acting investigator is the frame's `remaining[0]` (Rules Reference p.16
/// player order) — the response carries no investigator. Validates the
/// `PickMultiple` redraw indices (each [`OptionId`](crate::engine::OptionId) is
/// a hand index) are in bounds and unique. On success: move named hand cards
/// directly back into the deck (not via the discard pile, per the rules),
/// shuffle, draw the same count back, emit [`Event::MulliganPerformed`], then
/// pop the queue front. When the queue drains, setup ends — "the game begins"
/// (Rules Reference p.27): round 1 skips Mythos (p.24), so
/// [`investigation_phase`](super::phases::investigation_phase) begins here.
/// Otherwise re-prompt the next investigator. Rejections leave state and events
/// untouched.
pub(super) fn resume_mulligan(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let Some(crate::state::Continuation::Mulligan { remaining }) = cx.state.continuations.last()
    else {
        unreachable!("resume_mulligan: no Mulligan frame on top of the stack")
    };
    let remaining = remaining.clone();
    let investigator = remaining[0];

    let InputResponse::PickMultiple { selected } = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: setup mulligan expects InputResponse::PickMultiple, got {response:?}",
            )
            .into(),
        };
    };
    // Each OptionId is a hand index to redraw.
    let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();

    // ---- validate (state untouched on any failure) ----
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
            "resume_mulligan: prompted investigator {investigator:?} is not in the investigators \
             map; this is a state-corruption invariant violation"
        )
        });
    let hand_len = inv.hand.len();
    for &idx in &indices {
        if idx as usize >= hand_len {
            return EngineOutcome::Rejected {
                reason: format!("Mulligan: hand_index {idx} out of bounds (hand size {hand_len})")
                    .into(),
            };
        }
    }
    let mut sorted: Vec<u32> = indices.clone();
    sorted.sort_unstable();
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: duplicate index in {indices:?}").into(),
        };
    }

    // ---- mutate ----
    let redrawn_count =
        u8::try_from(indices.len()).expect("indices.len() <= hand.len() <= u8::MAX in practice");
    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("checked");
    // Walk indices high-to-low so smaller positions remain valid as
    // we remove. Move named cards directly into the deck — they
    // shuffle back in per the rules, not through the discard pile.
    for &i in sorted.iter().rev() {
        let card = inv_mut.hand.remove(i as usize);
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

    // ---- advance the queue ----
    let mut remaining = remaining;
    remaining.remove(0);
    // Pop the current Mulligan frame (validated above; it is the top frame).
    cx.state.continuations.pop();
    if remaining.is_empty() {
        // Setup complete — "the game begins" (Rules Reference p.27). Round 1
        // skips Mythos (p.24), so the first phase to begin is Investigation.
        // Begin it HERE (the kickoff moved off `apply_player_action`): setup has
        // "no action windows" (p.27), so the post-2.1 player window only opens
        // now that mulligans are done. `investigation_phase` may leave an
        // `InvestigationBegins` window open (a Fast-eligible play exists); we
        // still return `Done`, so this is one of the few paths where `Done`
        // accompanies a non-empty continuation stack — hosts present
        // `ResolveInput::Skip` to close it, as for any phase-transition window.
        super::phases::investigation_phase(cx);
        EngineOutcome::Done
    } else {
        prompt_mulligan(cx, remaining)
    }
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
/// effects' own events in the stream). Then the [`Trigger::OnPlay`]
/// abilities are `push_effect`'d for the global `drive` loop beneath a
/// [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// (Slice D #423); when that effect pops, the frame's disposal moves the
/// card out of `hand` — into `cards_in_play` for assets / investigators,
/// or into `discard` (with an emitted [`Event::CardDiscarded`]) for
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
        abilities: _,
        is_fast,
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

    // Mutate. A non-fast play costs one action (validated in `check_play_card`),
    // spent before the card is announced — RR p.5 / the Dynamite Blast FAQ
    // ("spend an action and pay the cost, then … attack of opportunity"). Fast
    // plays are not actions (#378).
    if !is_fast {
        super::actions::spend_one_action(cx, investigator);
    }
    // The card is announced (`CardPlayed`): an event commences play via the
    // shared `begin_event_play` (leaves hand, stashed for discard-on-completion);
    // an asset emits `CardPlayed` now and enters play after its `OnPlay` effect
    // (in `complete_play`).
    match destination {
        super::PlayDestination::Discard => begin_event_play(cx, investigator, idx),
        super::PlayDestination::InPlay => cx.events.push(Event::CardPlayed {
            investigator,
            code: code.clone(),
        }),
    }

    // RR p.5: playing a card is an action, so a non-fast play provokes an AoO
    // from each engaged ready enemy — fired *after* the card is announced + cost
    // paid and *before* its effect resolves (Dynamite Blast 01024 FAQ). Park the
    // rest of the play on an `ActionResolution` frame and drive the AoO loop
    // (which may open the Dodge cancel / Guard Dog soak windows); `complete_play`
    // runs on resume. Fast plays are not actions and resolve immediately. (#378.)
    if !is_fast {
        cx.state
            .continuations
            .push(crate::state::Continuation::ActionResolution {
                investigator,
                resume: crate::state::ActionResume::PlayCard { hand_index, code },
            });
        return super::combat::drive_aoo(cx, investigator);
    }
    complete_play(cx, investigator, idx, &code)
}

/// Dispose of a [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// once its pushed `OnPlay`/`OnEvent` effect has popped (Slice D #423). Pops the
/// frame first, then by destination: an **event** flushes its stashed
/// `pending_played_event` to discard; an **asset** is removed from hand at
/// `hand_index`, minted into play, and announced via `EnteredPlay`. Because the
/// frame is popped *before* `emit_event`, a reaction window the latter queues
/// (Research Librarian 01032) lands on top and the drive loop opens it — no
/// manual window open, no second stage. Returns `Done` (disposal never awaits
/// input); a missing-registry re-derive surfaces as `Rejected`.
pub(super) fn dispose_play_from_hand(cx: &mut Cx) -> EngineOutcome {
    let Some(crate::state::Continuation::PlayFromHand {
        investigator,
        code,
        hand_index,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("dispose_play_from_hand: top frame is not PlayFromHand");
    };
    cx.state.continuations.pop();

    let destination = match resolve_play_target(&code) {
        Ok((destination, _abilities, _is_fast, _card_type)) => destination,
        // Unreachable post-play (this code already resolved at play time); a
        // Rejected here would strand the played card, so surface it loudly.
        Err(outcome) => return outcome,
    };

    match destination {
        super::PlayDestination::Discard => {
            // Event: discard the stashed played event (RR Appendix I step 4),
            // exactly once for a play that resolves — the mid-play-defeat path
            // flushes in `resume_action_resolution` instead; the two sites are
            // mutually exclusive, so the event discards exactly once per play.
            flush_pending_played_event(cx);
        }
        super::PlayDestination::InPlay => {
            // Asset: remove from hand, mint + seed its in-play instance, push it
            // into play, then announce it. The drive loop opens the
            // after-enters-play reaction window (Research Librarian 01032) if
            // `emit_event` queued one — the frame is already popped.
            let played = cx
                .state
                .investigators
                .get_mut(&investigator)
                .expect("dispose_play_from_hand: investigator present")
                .hand
                .remove(usize::from(hand_index));
            let in_play = super::threat_area::new_in_play_instance(cx, played);
            let instance = in_play.instance_id;
            cx.state
                .investigators
                .get_mut(&investigator)
                .expect("dispose_play_from_hand: investigator present")
                .cards_in_play
                .push(in_play);
            let _ = super::emit::emit_event(
                cx,
                &super::emit::TimingEvent::EnteredPlay {
                    instance,
                    controller: investigator,
                },
            );
        }
    }
    EngineOutcome::Done
}

/// Push a [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame and
/// the card's `OnPlay` effects for the drive loop — the tail shared by the fast
/// path (inline) and the non-fast path (parked on an
/// [`ActionResolution`](crate::state::Continuation::ActionResolution) frame and
/// resumed after the `AoO` loop, #378). Re-derives destination + abilities from
/// the registry by `code` (an event has already left hand; `hand_index` locates
/// an asset still in hand). The drive loop resolves the `OnPlay` effect, then
/// disposes of the card via `dispose_play_from_hand` (event → discard; asset →
/// enter play). (Slice D #423 — replaces the synchronous `apply_effect` + asset
/// tail + manual window open.)
///
/// TODO(#417) (richer mid-action invalidation, shared with
/// `resume_activate_ability`, #361): a resumed `OnPlay` effect that returns
/// [`EngineOutcome::Rejected`] on
/// a lapsed precondition rolls back the *whole* play (the `AoO` damage + spent
/// action) via `apply()`'s snapshot, rather than suppressing the primary only
/// (the §D contract). Unreachable in scope — the only non-fast `OnPlay` cards
/// never reject (Emergency Cache 01088's `GainResources`; Machete 01020 has no
/// `OnPlay`); the rejecting `DiscoverClue` cards (Working a Hunch 01037,
/// Evidence! 01022) are all Fast and never park. Give this suppress-on-lapse
/// when a board-changing `AoO` reaction lands.
fn complete_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: usize,
    code: &CardCode,
) -> EngineOutcome {
    let (_destination, abilities, _is_fast, _card_type) = match resolve_play_target(code) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    // Combine the OnPlay effects into one Seq and push it for the drive loop,
    // below a PlayFromHand frame that disposes the card (event → discard; asset
    // → enter play) once the effect pops. (Slice D #423 — replaces the
    // synchronous apply_effect + asset tail + manual window open.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator,
            code: code.clone(),
            hand_index: u8::try_from(hand_index).expect("hand index fits u8 (#111 hand-size cap)"),
        });
    let on_play: Vec<crate::dsl::Effect> = abilities
        .into_iter()
        .filter(|a| a.trigger == Trigger::OnPlay)
        .map(|a| a.effect)
        .collect();
    if !on_play.is_empty() {
        let eval_ctx = EvalContext::for_controller(investigator);
        push_effect(cx, &crate::dsl::Effect::Seq(on_play), eval_ctx);
    }
    EngineOutcome::Done
}

/// Complete a non-fast card play after its `AoO` loop (#378). The actor-`Active`
/// re-validation gate has already run in
/// [`resume_action_resolution`](super::resume_action_resolution); delegates to
/// [`complete_play`] to run the `OnPlay` effects + asset enter-play.
///
/// On a mid-play defeat the gate suppresses this before it runs, so the card
/// was *announced* (`CardPlayed`, action spent) but does not resolve: an event
/// is flushed to discard by the suppress path in `resume_action_resolution`
/// (the `PlayFromHand` frame is never pushed on the suppressed path); an **asset** never
/// enters play and stays in hand — correct, and swept into the removed pile by
/// elimination cleanup (you don't gain the asset if you die paying for it).
pub(super) fn resume_play_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: u8,
    code: &CardCode,
) -> EngineOutcome {
    complete_play(cx, investigator, usize::from(hand_index), code)
}

/// Commence playing an event from `investigator`'s hand at `hand_index` (RR
/// Appendix I, step 3): emit [`Event::CardPlayed`], remove the card from hand,
/// and stash it in
/// [`pending_played_event`](crate::state::GameState::pending_played_event) so
/// it is placed in discard on *completion* of its effect (step 4), flushed by
/// `dispose_play_from_hand` once the `PlayFromHand` frame is re-exposed after
/// its `OnPlay`/`OnEvent` effect pops (RR Appendix I step 4). Stashing before
/// the effect runs means a suspending effect (Dynamite Blast 01024's location
/// choice) discards the event on resume rather than stranding it in hand.
///
/// Shared by [`play_card`]'s event branch and the Axis-C reaction-event play
/// (`reaction_windows::play_fast_event`). The caller runs the event's
/// effect(s) after this returns; neither path charges a resource cost (Slice 1
/// does not model play-cost resources). The caller guarantees `investigator`
/// exists and `hand_index` is in bounds.
pub(super) fn begin_event_play(cx: &mut Cx, investigator: InvestigatorId, hand_index: usize) {
    let code = cx
        .state
        .investigators
        .get(&investigator)
        .expect("begin_event_play: caller guarantees investigator exists")
        .hand[hand_index]
        .clone();
    cx.events.push(Event::CardPlayed {
        investigator,
        code: code.clone(),
    });
    let card = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("begin_event_play: caller guarantees investigator exists")
        .hand
        .remove(hand_index);
    cx.state.pending_played_event = Some((investigator, card));
}

/// Flush a [`pending_played_event`](crate::state::GameState::pending_played_event)
/// to its owner's discard pile, emitting [`Event::CardDiscarded`] (`from:
/// Zone::Hand`). Called by [`dispose_play_from_hand`] when a play resolves (Slice
/// D #423), and also by the suppress path in `resume_action_resolution` for the
/// mid-play-defeat case (where `PlayFromHand` is never pushed). The two call sites
/// are mutually exclusive per play, so per RR Appendix I step 4 an event is
/// placed in discard exactly once — "simultaneously with the completion" of its
/// effect. A no-op when no event is mid-play.
pub(in crate::engine) fn flush_pending_played_event(cx: &mut Cx) {
    let Some((investigator, code)) = cx.state.pending_played_event.take() else {
        return;
    };
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.discard.push(code.clone());
    }
    cx.events.push(Event::CardDiscarded {
        investigator,
        code,
        from: Zone::Hand,
    });
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
