//! Card-related dispatch handlers: deck management, drawing, mulligan,
//! resource grants, and card play.

use crate::action::InputResponse;
use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::{Effect, Trigger};
use crate::event::Event;
use crate::state::{CardCode, CardInstanceId, InvestigatorId, Zone};

use super::super::evaluator::{push_effect, EvalContext};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

/// Starting hand size at scenario setup. Per the Rules Reference,
/// each investigator draws 5 cards before mulligan.
pub(super) const INITIAL_HAND_SIZE: u8 = 5;

/// Whether `code` is a weakness per the installed registry. Returns `false`
/// when no registry is installed or the card code has no metadata —
/// the engine's registry-free unit tests behave as if no card is a weakness.
///
/// Two consumers: the opening-hand set-aside below, and the defeated-enemy
/// disposal in [`combat`](super::combat) (a defeated weakness enemy goes to its
/// owner's discard pile rather than the encounter discard, #632).
pub(super) fn is_weakness_code(code: &CardCode) -> bool {
    crate::card_registry::current()
        .and_then(|reg| (reg.metadata_for)(code))
        .is_some_and(card_dsl::CardMetadata::is_weakness)
}

/// Replace weaknesses currently in `investigator`'s hand per Rules Reference
/// setup step 8: move each to `setaside` (emitting
/// [`Event::WeaknessSetAside`](crate::event::Event::WeaknessSetAside))
/// and draw replacements, looping until the hand holds no weakness or the
/// deck is exhausted.
///
/// # Contract
///
/// - Caller guarantees `investigator` exists in `cx.state.investigators`.
/// - The loop terminates: if the deck runs out mid-replacement, the call
///   returns without panic. The hand then holds no weakness — UNLESS the deck
///   exhausted on a replacement draw that was itself a weakness (RR's "replaced
///   by drawing another card" has no card left to draw). That needs a deck of
///   almost entirely weaknesses, which a legal deck never is; the guard exists
///   only so a pathological deck can't spin the loop forever.
/// - Registry-free (no registry installed): `is_weakness_code` always returns
///   `false`, so this function is a no-op, preserving the engine's
///   registry-free unit test behavior.
pub(super) fn replace_opening_hand_weaknesses(cx: &mut Cx, investigator: InvestigatorId) {
    loop {
        // Scan the hand for weakness indices. Collect before mutating.
        let weakness_indices: Vec<usize> = cx
            .state
            .investigators
            .get(&investigator)
            .expect("replace_opening_hand_weaknesses: investigator exists")
            .hand
            .iter()
            .enumerate()
            .filter(|(_, code)| is_weakness_code(code))
            .map(|(i, _)| i)
            .collect();

        if weakness_indices.is_empty() {
            break;
        }

        let n = weakness_indices.len();

        // Remove weaknesses high-to-low so earlier indices remain valid.
        // Codes are collected in reverse-removal order; reverse at the end
        // to restore hand-order before emitting events.
        let mut removed_codes: Vec<CardCode> = Vec::with_capacity(n);
        {
            let inv = cx
                .state
                .investigators
                .get_mut(&investigator)
                .expect("replace_opening_hand_weaknesses: investigator exists");
            for &i in weakness_indices.iter().rev() {
                let code = inv.hand.remove(i);
                inv.setaside.push(code.clone());
                removed_codes.push(code);
            }
        }
        removed_codes.reverse(); // restore hand order (lowest index first)

        for code in &removed_codes {
            cx.events.push(Event::WeaknessSetAside {
                investigator,
                code: code.clone(),
            });
        }

        // Draw replacements: draw_cards stops at deck end, so n_drawn ≤ n.
        draw_cards(
            cx,
            investigator,
            u8::try_from(n).expect("hand size fits u8 (≤ INITIAL_HAND_SIZE ≤ 8)"),
        );

        // Guard: if the deck is now empty we stop. Without this break the
        // loop could spin forever when replacements are themselves weaknesses
        // and the deck is exhausted.
        let deck_empty = cx
            .state
            .investigators
            .get(&investigator)
            .expect("replace_opening_hand_weaknesses: investigator exists")
            .deck
            .is_empty();
        if deck_empty {
            break;
        }
    }
}

/// Reveal-on-draw for a persistent treachery weakness drawn from the player
/// deck during play (RR Weakness keyword: a drawn weakness resolves its
/// Revelation immediately rather than staying a normal hand card). Scope:
/// **persistent treachery weaknesses** (Cover Up 01007). Each matching card is
/// removed from `investigator`'s hand, a [`Event::CardRevealed`] is emitted, and
/// its `Trigger::Revelation` effects are pushed for the drive loop (Cover Up's
/// `PutIntoThreatArea` then places it in the threat area).
///
/// Removal precedes the push deliberately: `PutIntoThreatArea` spawns a fresh
/// instance by code, so a copy left in hand would duplicate the card.
///
/// Non-persistent treachery weaknesses and weakness enemies/assets are **left in
/// hand untouched** — deferred to #514 (none reachable in the corpus draw path).
/// No-op without an installed registry (registry-free engine unit tests).
///
/// **The shape — draw everything, then resolve every Revelation — is the rule,
/// not a convenience.** This function is called once the whole draw has landed
/// in hand, and it collects *all* matching cards before resolving any.
/// `data/official-faq/Frequently_Asked_Questions.md`:
///
/// > Anytime you draw one or more cards, the card draw occurs simultaneously
/// > unless the effect uses the phrase "one at a time." Then, once all of the
/// > cards have been drawn, you must resolve all Revelation abilities on those
/// > cards (in an order of your choosing).
///
/// Resolving each card's Revelation as it was drawn would be wrong for a draw
/// of two, because the second card is already in hand when the first one's
/// Revelation resolves. One narrowing is deliberate and recorded: the FAQ gives
/// the player *"an order of your choosing"* over the Revelations, and this
/// resolves them in draw order instead. Unobservable while at most one weakness
/// is reachable per draw in the corpus; the choice belongs with #514, which
/// widens which weaknesses resolve here at all.
///
/// MUST NOT be called from the setup opening-hand / mulligan path — those *set
/// aside* weaknesses (#508), they do not resolve them.
pub(in crate::engine) fn resolve_drawn_weaknesses(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(reg) = card_registry::current() else {
        return;
    };
    // Collect indices of drawn persistent treachery weaknesses, in hand order.
    let matches: Vec<(usize, CardCode)> = {
        let Some(inv) = cx.state.investigators.get(&investigator) else {
            return;
        };
        inv.hand
            .iter()
            .enumerate()
            .filter(|(_, code)| {
                (reg.metadata_for)(code)
                    .is_some_and(|m| m.is_weakness() && m.card_type() == CardType::Treachery)
                    && super::encounter::treachery_is_persistent(
                        &(reg.abilities_for)(code).unwrap_or_default(),
                    )
            })
            .map(|(i, code)| (i, code.clone()))
            .collect()
    };
    if matches.is_empty() {
        return;
    }
    // Remove from hand high-index-to-low so earlier indices stay valid.
    {
        let inv = cx
            .state
            .investigators
            .get_mut(&investigator)
            .expect("resolve_drawn_weaknesses: investigator exists");
        for &(i, _) in matches.iter().rev() {
            inv.hand.remove(i);
        }
    }
    // Reveal + push each Revelation, in original draw order.
    for (_, code) in &matches {
        cx.events.push(Event::CardRevealed {
            investigator,
            code: code.clone(),
            card_type: CardType::Treachery,
        });
        let effects: Vec<Effect> = (reg.abilities_for)(code)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.trigger == Trigger::Revelation)
            .map(|a| a.effect)
            .collect();
        if !effects.is_empty() {
            push_effect(
                cx,
                &Effect::Seq(effects),
                EvalContext::for_controller(investigator),
            );
        }
    }
}

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
/// penalty logic for an empty deck lives in [`draw_with_deckout`],
/// which every in-play draw (the Draw action, Upkeep step 4.4, and
/// [`Effect::DrawCards`](crate::dsl::Effect::DrawCards)) goes through.
/// Direct callers are the setup / mulligan paths, where the deck
/// cannot empty and the deck-out rule does not apply.
///
/// Emits a single [`Event::CardsDrawn`] with the actually-drawn
/// count, even if that's zero. A zero-count draw is informative for
/// consumers tracking the attempt.
pub(in crate::engine) fn draw_cards(cx: &mut Cx, investigator: InvestigatorId, count: u8) {
    let drawn = move_deck_top_to_hand(cx, investigator, count);
    cx.events.push(Event::CardsDrawn {
        investigator,
        count: drawn,
    });
}

/// Move up to `count` cards from the deck top to the hand, returning how
/// many actually moved. The structural half of [`draw_cards`], factored
/// out so [`draw_with_deckout`] can span a mid-draw reshuffle with a
/// single [`Event::CardsDrawn`] (RR: "when a player draws two or more
/// cards as the result of a single ability or game step, those cards are
/// drawn simultaneously").
fn move_deck_top_to_hand(cx: &mut Cx, investigator: InvestigatorId, count: u8) -> u8 {
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
    u8::try_from(drawn).expect("drawn <= count <= u8::MAX")
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
///
/// # Panics
///
/// Panics if `investigator` vanishes from the state mid-call — the index draw
/// re-`get`s the investigator already checked present above, so this is a
/// state-corruption invariant, not a reachable input error (a missing
/// investigator returns `None` at the top).
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

/// Discard `instance_id` from `investigator`'s `cards_in_play` to their discard
/// pile, emitting [`Event::CardDiscarded`] `{ from: Zone::InPlay }`. Shared by
/// [`Cost::DiscardSelf`](crate::dsl::Cost::DiscardSelf) payment, uses-depletion
/// auto-discard, soak-defeat asset removal, and slot make-room (#498/#119). A
/// missing instance is a state-corruption invariant violation (callers locate it
/// first).
pub(in crate::engine) fn discard_card_from_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("discard_card_from_play: investigator present");
    let pos = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance_id)
        .unwrap_or_else(|| {
            unreachable!("discard_card_from_play: instance {instance_id:?} not in cards_in_play")
        });
    let card = inv.cards_in_play.remove(pos);
    inv.discard.push(card.code.clone());
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card.code,
        from: crate::state::Zone::InPlay,
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

/// Pay `code`'s printed resource cost for `investigator` (RR p.22, Initiation
/// Sequence): saturating-subtract the cost from the wallet and emit
/// [`Event::ResourcesPaid`]. A 0-cost card is a no-op (no event), mirroring
/// [`grant_resources`]'s zero-amount behavior. The cost is read from the
/// registry by `code`; absent metadata (registry-free unit tests) or a 0 cost
/// pays nothing.
///
/// The caller has already validated affordability
/// (`check_play_resource_cost_payable`), so the `saturating_sub` never silently
/// underflows a real shortfall — it's belt-and-suspenders. That check is also
/// what keeps the `u8::try_from(...).unwrap_or(0)` below honest: the two
/// non-numeric costs — a `"–"` (`None`) and an X (`Some(-2)`) — are both
/// rejected there, so neither reaches this function to be charged as free.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(super) fn pay_play_cost(cx: &mut Cx, investigator: InvestigatorId, code: &CardCode) {
    let cost = card_registry::current()
        .and_then(|reg| (reg.metadata_for)(code))
        .and_then(crate::card_data::CardMetadata::play_cost)
        .and_then(|c| u8::try_from(c).ok())
        .unwrap_or(0);
    if cost == 0 {
        return;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("pay_play_cost: caller guarantees investigator exists");
    inv.resources = inv.resources.saturating_sub(cost);
    cx.events.push(Event::ResourcesPaid {
        investigator,
        amount: cost,
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

/// Draw one card for `investigator`, applying the empty-deck rule.
/// Thin wrapper over [`draw_with_deckout`] for the single-card callers
/// (the `Draw` action and Upkeep step 4.4).
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(super) fn draw_one_with_deckout(cx: &mut Cx, investigator: InvestigatorId) {
    draw_with_deckout(cx, investigator, 1);
}

/// Draw `count` cards for `investigator`, applying the empty-deck rule
/// (`data/rules-reference/rules/glossary/Drawing_Cards.md`):
///
/// > If a deck empties middraw, reset the deck and complete the draw.
///
/// > If an investigator with an empty investigator deck needs to draw a
/// > card, that investigator shuffles his or her discard pile back into
/// > his or her deck, then draws the card, and upon completion of the
/// > entire draw takes one horror.
///
/// So the horror is **once per draw**, on completion, however many
/// times the deck emptied along the way — not once per card. The
/// whole draw emits a single [`Event::CardsDrawn`] with the total,
/// matching the rule that the cards are drawn simultaneously.
///
/// The deck-out reading inherited from the `Draw` action is unchanged:
/// horror fires on any would-draw-from-empty, and a zero-card discard
/// is not shuffled back (`glossary/Discard_Piles.md`: "any ability that
/// would shuffle a discard pile of zero cards back into a deck does not
/// shuffle the deck"), so deck and discard both empty means no card,
/// no shuffle, but still the horror.
///
/// Every in-play draw routes here: the `Draw` action, Upkeep step 4.4,
/// and [`Effect::DrawCards`](crate::dsl::Effect::DrawCards) (Guts 01089
/// and the other three Core skills, #636). The setup / mulligan paths
/// deliberately do not — they use [`draw_cards`].
///
/// Caller guarantees `investigator` exists in `state.investigators`.
pub(in crate::engine) fn draw_with_deckout(cx: &mut Cx, investigator: InvestigatorId, count: u8) {
    let mut drawn: u8 = 0;
    let mut deck_ran_out = false;
    while drawn < count {
        let inv = cx
            .state
            .investigators
            .get(&investigator)
            .expect("draw_with_deckout: caller guarantees investigator exists");
        if inv.deck.is_empty() {
            // "Needs to draw a card" with an empty deck: the horror is
            // owed regardless of whether the reshuffle can supply cards.
            deck_ran_out = true;
            if inv.discard.is_empty() {
                // Nothing left anywhere — stop short of `count`.
                break;
            }
            reshuffle_discard_into_deck(cx, investigator);
        }
        let moved = move_deck_top_to_hand(cx, investigator, count - drawn);
        if moved == 0 {
            // The deck is non-empty by here, so this cannot happen. It is a
            // `break` rather than a panic only because the alternative failure
            // mode is an infinite loop: a wrong draw count is recoverable,
            // a hung engine is not.
            break;
        }
        drawn += moved;
    }
    cx.events.push(Event::CardsDrawn {
        investigator,
        count: drawn,
    });
    if deck_ran_out {
        super::elimination::take_horror(cx, investigator, 1);
    }
    // RR Weakness keyword: a weakness drawn during play reveals + resolves its
    // Revelation (#509). Setup's opening-hand draw uses `draw_cards` directly,
    // so it is unaffected (it sets aside instead, #508).
    //
    // Last, after the deck-out horror: the horror is part of "completion of
    // the entire draw", and a Revelation that follows it therefore sees the
    // post-horror state. This is the order the Draw action and Upkeep 4.4
    // already used; it now also governs the `Effect::DrawCards` path, which
    // previously ran the revelation with no horror in between.
    resolve_drawn_weaknesses(cx, investigator);
}

/// Handler for `TurnAction::Draw`.
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
    cx.state
        .continuations
        .push(crate::state::Continuation::Mulligan { remaining });
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(
            "Mulligan: choose cards to redraw (an empty selection keeps your hand).",
        ),
        resume_token: ResumeToken(0),
    }
}

/// Swap the hand cards at `sorted` (ascending, in-bounds, unique hand indices)
/// for fresh ones, per the three ordered steps of the Rules Reference `Mulligan`
/// glossary entry:
///
/// > These cards are set aside, and an equivalent number of cards are drawn and
/// > added to the player's starting hand. The set-aside cards are then shuffled
/// > back into the player's deck.
///
/// The set-aside step is what makes the redraw honest: the named cards are held
/// out of every zone while the replacements are drawn, so a card cannot be
/// redrawn as its own replacement (#637). They return to the **deck**, never
/// through the discard pile.
///
/// The replacements come off the deck unshuffled because setup already shuffled
/// it (`start_scenario` shuffles before dealing the opening hand); the only
/// shuffle the mulligan itself owes is the one returning the set-aside cards.
/// [`draw_cards`] clamps to the deck size, so an investigator whose deck holds
/// fewer cards than they mulliganed ends with a smaller hand rather than a
/// panic — unreachable for a legal 30-card deck, and the rules have no card to
/// offer in that case either.
///
/// The mulligan-redraw weakness sweep ([`replace_opening_hand_weaknesses`]) runs
/// **after** the set-aside cards are back, matching the glossary's ordering: the
/// mulligan ends once the replacements are in hand, and step 8's replacement
/// draw is a later draw off the whole deck. Two consequences, both wanted — it
/// may draw a card this investigator just mulliganed (legal: that card is back
/// in the deck and this is no longer the mulligan draw), and it still has a deck
/// to draw from, where a sweep run before the cards returned could strand the
/// investigator a card short.
///
/// An empty `sorted` is a no-op: nothing is set aside, so there is nothing to
/// draw and nothing to shuffle back (a "keep my hand" mulligan leaves the deck
/// untouched, emitting no [`Event::DeckShuffled`]).
///
/// The cards are held in a local rather than in
/// [`investigator.setaside`](crate::state::Investigator::setaside), which the
/// opening-hand weakness path uses: that pile is flushed once the whole mulligan
/// loop drains (RR setup step 8, "upon completion of this step"), whereas the
/// glossary entry returns each player's cards within their own mulligan. Keeping
/// them local also leaves every zone consistent at the handler boundary.
///
/// Caller guarantees `investigator` exists in `cx.state.investigators` and that
/// every index in `sorted` is a valid hand position.
fn perform_mulligan_redraw(cx: &mut Cx, investigator: InvestigatorId, sorted: &[u32]) {
    if sorted.is_empty() {
        return;
    }
    let count = u8::try_from(sorted.len()).expect("sorted.len() <= hand.len() <= u8::MAX");

    // Set aside. Walk indices high-to-low so smaller positions stay valid as we
    // remove. The collected order doesn't matter: these cards go back to the
    // deck and are shuffled before anything can observe where they landed.
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("perform_mulligan_redraw: caller guarantees investigator exists");
    let mut set_aside: Vec<CardCode> = Vec::with_capacity(sorted.len());
    for &i in sorted.iter().rev() {
        set_aside.push(inv.hand.remove(i as usize));
    }

    // Draw the replacements off the deck, which setup already shuffled.
    draw_cards(cx, investigator, count);

    // Shuffle the set-aside cards back in. This closes the mulligan proper, so
    // it happens before the step-8 sweep below rather than after: the glossary
    // returns the cards as soon as the replacements are in hand, and any later
    // draw comes off the whole deck again.
    cx.state
        .investigators
        .get_mut(&investigator)
        .expect("perform_mulligan_redraw: caller guarantees investigator exists")
        .deck
        .extend(set_aside);
    shuffle_player_deck(cx, investigator);

    // RR setup step 8 applies to the mulligan redraw too: any weakness drawn as
    // a replacement is set aside and replaced again. A separate, later draw off
    // the restored deck — so it may legally turn up a card this investigator
    // just mulliganed, and (unlike a sweep run before the cards return) it still
    // has a deck to draw from.
    replace_opening_hand_weaknesses(cx, investigator);
}

/// Resume the setup mulligan loop (#348), driving the top
/// [`Continuation::Mulligan`](crate::state::Continuation::Mulligan) frame.
///
/// The acting investigator is the frame's `remaining[0]` (Rules Reference p.16
/// player order) — the response carries no investigator. Validates the
/// `PickMultiple` redraw indices (each [`OptionId`](crate::engine::OptionId) is
/// a hand index) are in bounds and unique. On success the three steps of the
/// Rules Reference `Mulligan` glossary entry run in order — "These cards are
/// **set aside**, and an equivalent number of cards are **drawn** and added to
/// the player's starting hand. The set-aside cards are **then shuffled back**
/// into the player's deck." — so a mulliganed card can never turn up among its
/// own replacements (#637). The cards go back to the deck, never through the
/// discard pile. Then emit [`Event::MulliganPerformed`] and pop the queue
/// front. When the queue drains, setup ends — "the game begins"
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
    perform_mulligan_redraw(cx, investigator, &sorted);
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
        // All mulligans complete. Flush every investigator's set-aside
        // weaknesses back into their deck (RR setup step 8: "Upon completion
        // of this step, shuffle each of these weakness cards back into its
        // owner's deck."). Drain `setaside` into `deck` and reshuffle.
        // Process in deterministic id order.
        let mut ids_with_setaside: Vec<crate::state::InvestigatorId> = cx
            .state
            .investigators
            .iter()
            .filter(|(_, inv)| !inv.setaside.is_empty())
            .map(|(id, _)| *id)
            .collect();
        ids_with_setaside.sort_unstable();
        for id in ids_with_setaside {
            let cards: Vec<crate::state::CardCode> = cx
                .state
                .investigators
                .get_mut(&id)
                .expect("id from investigators")
                .setaside
                .drain(..)
                .collect();
            cx.state
                .investigators
                .get_mut(&id)
                .expect("id from investigators")
                .deck
                .extend(cards);
            shuffle_player_deck(cx, id);
        }

        // Setup complete — "the game begins" (Rules Reference p.27). Round 1
        // skips Mythos (p.24), so the first phase to begin is Investigation.
        // Begin it HERE (the kickoff moved off `apply_player_action`): setup has
        // "no action windows" (p.27), so the post-2.1 player window only opens
        // now that mulligans are done. `investigation_phase` may leave an
        // `InvestigationBegins` window open (a Fast-eligible play exists); we
        // still return `Done`, so this is one of the few paths where `Done`
        // accompanies a non-empty continuation stack — hosts present
        // `ResolveInput::Skip` to close it, as for any phase-transition window.
        super::phases::investigation_phase(cx)
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

/// Handler for `TurnAction::PlayCard`.
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
/// effects' own events in the stream), and the card leaves hand with it
/// (RR Appendix I step 3). Then the [`Trigger::OnPlay`]
/// abilities are `push_effect`'d for the global `drive` loop beneath a
/// [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// (Slice D #423) that holds the card; when that effect pops, the frame's
/// disposal places it — into `cards_in_play` for assets, or into `discard`
/// (with an emitted [`Event::CardDiscarded`]) for events.
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
    // Pay the resource cost (RR p.22): both Fast and non-Fast plays pay it —
    // Fast only skips the *action* cost. Affordability was validated in
    // `check_play_card`; the deduction happens before the card is announced and
    // before any attack of opportunity resolves (#501).
    pay_play_cost(cx, investigator, &code);
    // The card is announced (`CardPlayed`) and commences being played — asset or
    // event alike it leaves hand here and rides the frames below until it is
    // placed (RR Appendix I step 3 → 4).
    let card = commence_play(cx, investigator, idx);

    // RR p.5: playing a card is an action, so a non-fast play provokes an AoO
    // from each engaged ready enemy — fired *after* the card is announced + cost
    // paid and *before* its effect resolves (Dynamite Blast 01024 FAQ). Park the
    // rest of the play — the card itself included — on an `ActionResolution`
    // frame and drive the AoO loop (which may open the Dodge cancel / Guard Dog
    // soak windows); `complete_play` runs on resume. Fast plays are not actions
    // and resolve immediately. (#378.)
    if !is_fast {
        cx.state
            .continuations
            .push(crate::state::Continuation::ActionResolution {
                investigator,
                resume: crate::state::ActionResume::PlayCard { card: Some(card) },
            });
        return super::combat::drive_aoo(cx, investigator);
    }
    complete_play(cx, investigator, card)
}

/// Move the mid-play asset `card` into play (RR Appendix I step 4, "placed in
/// play"): mint + seed its in-play instance, push it to `cards_in_play`, and
/// announce it via the `EnteredPlay` timing event. The emit outcome is
/// intentionally discarded — the frame driving this call is already popped, so
/// the `drive` loop opens any after-enters-play reaction window (Research
/// Librarian 01032) itself. Called by `slots::enter_asset_making_room` once the
/// asset's slots are clear, whether that took a discard or not (#498). The caller
/// owns the card (it left hand at step 3), so there is no hand lookup here to go
/// stale (#565).
pub(in crate::engine) fn enter_asset_into_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    card: CardCode,
) {
    let in_play = super::threat_area::new_in_play_instance(cx, card);
    let instance = in_play.instance_id;
    cx.state
        .investigators
        .get_mut(&investigator)
        .expect("enter_asset_into_play: investigator present")
        .cards_in_play
        .push(in_play);
    let _ = super::emit::queue_event(
        cx,
        &super::emit::TimingEvent::EnteredPlay {
            instance,
            controller: investigator,
        },
    );
}

/// Dispose of a [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// once its pushed `OnPlay`/`OnEvent` effect has popped (Slice D #423) — RR
/// Appendix I step 4, where the card "is regarded as played (and placed in play,
/// or in its owner's discard pile if it's an event)". Pops the frame first (which
/// takes the card off it), then places the card by destination: an **event** goes
/// to its owner's discard pile, an **asset** is minted into play and announced via
/// `EnteredPlay`. Because the frame is popped *before* `queue_event`, a reaction
/// window the latter queues (Research Librarian 01032) lands on top and the drive
/// loop opens it — no manual window open, no second stage. Returns `Done`
/// (disposal never awaits input); a missing-registry re-derive surfaces as
/// `Rejected`.
///
/// The frame holds no card when something else already placed it: elimination's
/// sweep removed it from the game (RR p.10 step 1), or
/// [`Effect::AttachSelfToLocation`](crate::dsl::Effect::AttachSelfToLocation)
/// re-homed it (Barricade 01038). Either way there is nothing left to place and
/// the pop is the whole disposal.
pub(super) fn dispose_play_from_hand(cx: &mut Cx) -> EngineOutcome {
    let Some(crate::state::Continuation::PlayFromHand { investigator, card }) =
        cx.state.continuations.pop()
    else {
        unreachable!("dispose_play_from_hand: top frame is not PlayFromHand");
    };
    let Some(card) = card else {
        return EngineOutcome::Done;
    };

    let destination = match resolve_play_target(&card) {
        Ok((destination, _abilities, _is_fast, _card_type)) => destination,
        // Unreachable post-play (this code already resolved at play time); a
        // Rejected here would strand the played card, so surface it loudly.
        Err(outcome) => return outcome,
    };

    match destination {
        super::PlayDestination::Discard => {
            discard_played_card(cx, investigator, card);
            EngineOutcome::Done
        }
        super::PlayDestination::InPlay => {
            super::slots::enter_asset_making_room(cx, investigator, card)
        }
    }
}

/// Push a [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// holding `card` and the card's `OnPlay` effects for the drive loop — the tail
/// shared by the fast path (inline) and the non-fast path (parked on an
/// [`ActionResolution`](crate::state::Continuation::ActionResolution) frame and
/// resumed after the `AoO` loop, #378). Re-derives the `OnPlay` abilities from
/// the registry by code. The drive loop resolves the `OnPlay` effect, then
/// disposes of the card via `dispose_play_from_hand` (event → discard; asset →
/// enter play). (Slice D #423 — replaces the synchronous `apply_effect` + asset
/// tail + manual window open.)
///
/// **Disposal strictly after the whole `OnPlay` effect is the rule for an
/// attaching event, not just tidiness.** Barricade 01038's `OnPlay` is
/// `Effect::AttachSelfToLocation`, and the official FAQ fixes when such an
/// event finishes: *"An event that attaches to another card is considered
/// 'resolved' when all abilities and effects triggered by it entering play
/// resolve, including its attachment effect."*
/// (`data/official-faq/Frequently_Asked_Questions.md`.) So the attachment is
/// part of resolution rather than something that happens to a card already
/// resolved — which is why the frame holds the played card across the effect
/// walk and only then disposes of it.
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
fn complete_play(cx: &mut Cx, investigator: InvestigatorId, card: CardCode) -> EngineOutcome {
    let (_destination, abilities, _is_fast, _card_type) = match resolve_play_target(&card) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    // Combine the OnPlay effects into one Seq and push it for the drive loop,
    // below a PlayFromHand frame that holds the card and disposes of it (event →
    // discard; asset → enter play) once the effect pops. (Slice D #423 — replaces
    // the synchronous apply_effect + asset tail + manual window open.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator,
            card: Some(card),
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
/// was *announced* (`CardPlayed`, action spent) but does not resolve — and the
/// card itself, still riding the `ActionResolution` frame, was removed from the
/// game by elimination's sweep (RR p.10 step 1). Neither an event nor an asset
/// reaches a discard pile or the play area: you don't gain the asset if you die
/// paying for it, and a dead investigator has no discard pile to place an event
/// in.
pub(super) fn resume_play_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    card: CardCode,
) -> EngineOutcome {
    complete_play(cx, investigator, card)
}

/// Commence playing the card at `hand_index` in `investigator`'s hand (RR
/// Appendix I, step 3: "The card commences being played"): emit
/// [`Event::CardPlayed`], remove the card from hand, and hand it back to the
/// caller, which parks it on the frame driving the play. Between here and
/// step 4 the card is in no zone at all — see
/// [`Continuation::play_in_progress`](crate::state::Continuation::play_in_progress).
///
/// Asset and event alike: the step-3 wording draws no distinction, and carrying
/// an asset on its frame is what keeps a mid-play hand shuffle (a Fast event
/// played in the attack-of-opportunity window this play provokes) from
/// invalidating the disposal (#565).
///
/// Shared by [`play_card`] and the Axis-C reaction-event play
/// (`reaction_windows::play_fast_event`). Both callers pay the resource cost
/// before calling and run the card's effect(s) after. The caller guarantees
/// `investigator` exists and `hand_index` is in bounds.
pub(super) fn commence_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: usize,
) -> CardCode {
    let card = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("commence_play: caller guarantees investigator exists")
        .hand
        .remove(hand_index);
    cx.events.push(Event::CardPlayed {
        investigator,
        code: card.clone(),
    });
    card
}

/// Place a played event in its owner's discard pile (RR Appendix I step 4:
/// "placed … in its owner's discard pile if it's an event"), emitting
/// [`Event::CardDiscarded`] with `from: Zone::Hand` — hand is the zone the card
/// left when it commenced being played, and it has been in none since.
///
/// The only caller is [`dispose_play_from_hand`], reached exactly once per play
/// that resolves, so an event is placed in discard exactly once —
/// "simultaneously with the completion" of its effect.
fn discard_played_card(cx: &mut Cx, investigator: InvestigatorId, card: CardCode) {
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.discard.push(card.clone());
    }
    cx.events.push(Event::CardDiscarded {
        investigator,
        code: card,
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
mod draw_with_deckout_tests {
    use super::*;
    use crate::state::{CardCode, InvestigatorId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn draw_one_with_deckout_empty_deck_reshuffles_and_takes_horror() {
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck.clear();
        inv.discard = vec![CardCode::new("01000"), CardCode::new("01001")];
        // After #448 cp2a: horror accumulates on investigator_card, accessor reads it.
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
            state.investigators[&id].horror(),
            1,
            "deck-out costs 1 horror"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })));
    }

    /// A multi-card draw that empties the deck partway through completes the
    /// full count — "If a deck empties middraw, reset the deck and complete
    /// the draw" — and takes the horror **once**, on completion of the
    /// entire draw, not once per emptied deck (#636).
    #[test]
    fn draw_with_deckout_completes_the_count_across_a_midway_reshuffle() {
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = vec![CardCode::new("01000")];
        inv.discard = vec![
            CardCode::new("01001"),
            CardCode::new("01002"),
            CardCode::new("01003"),
        ];
        let hand_before = inv.hand.len();
        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        draw_with_deckout(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            3,
        );

        let inv = &state.investigators[&id];
        assert_eq!(inv.hand.len(), hand_before + 3, "drew the full count");
        assert_eq!(inv.deck.len(), 1, "1 + 3 cards, 3 of them drawn");
        assert!(inv.discard.is_empty(), "discard shuffled back in");
        assert_eq!(inv.horror(), 1, "exactly 1 horror for the whole draw");
        // One horror event, not one per card.
        crate::assert_event_count!(events, 1, Event::HorrorTaken { .. });
        // Drawn simultaneously ⇒ one event carrying the whole count, even
        // though the reshuffle split the move into two hops.
        crate::assert_event_count!(events, 1, Event::CardsDrawn { .. });
        crate::assert_event!(events, Event::CardsDrawn { count: 3, .. });
    }

    /// The ordinary case is untouched: enough cards in the deck means no
    /// reshuffle and no horror.
    #[test]
    fn draw_with_deckout_on_a_stocked_deck_neither_reshuffles_nor_takes_horror() {
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = vec![
            CardCode::new("01000"),
            CardCode::new("01001"),
            CardCode::new("01002"),
        ];
        inv.discard = vec![CardCode::new("01003")];
        let hand_before = inv.hand.len();
        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        draw_with_deckout(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            2,
        );

        let inv = &state.investigators[&id];
        assert_eq!(inv.hand.len(), hand_before + 2, "drew 2");
        assert_eq!(inv.deck.len(), 1);
        assert_eq!(inv.discard.len(), 1, "discard untouched");
        assert_eq!(inv.horror(), 0, "no deck-out, no horror");
        crate::assert_no_event!(events, Event::DeckShuffled { .. });
        crate::assert_no_event!(events, Event::HorrorTaken { .. });
        crate::assert_event!(events, Event::CardsDrawn { count: 2, .. });
    }
}
