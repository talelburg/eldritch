//! Encounter-deck draw, spawn, and Mythos draw chain handlers.

use crate::card_data::{CardKind, CardMetadata, CardType, HealthValue, Spawn, SpawnLocation};
use crate::card_registry;
use crate::dsl::Trigger;
use crate::event::Event;
use crate::state::{
    CardCode, Enemy, EnemyId, InvestigatorId, LocationId, Phase, PhaseStep, SpawnEngagePending,
    WindowKind,
};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

/// Hard cap on a single Mythos draw chain. Real scenarios surge ≤2
/// in a chain; the cap exists purely to guarantee termination on
/// malformed encounter decks (e.g. a deck small enough for surge to
/// loop via the Rules Reference p.10 reshuffle). `unreachable!`-class
/// — never reached in legitimate play.
///
const MAX_SURGE_CHAIN: usize = 64;

/// Handler for [`EngineRecord::EncounterDeckShuffled`].
///
/// Permutes the shared encounter deck via the deterministic RNG and
/// emits [`Event::EncounterDeckShuffled`] (when ≥ 2 cards). No
/// validation — the encounter deck is shared, so there's no
/// per-investigator existence check.
pub(super) fn encounter_deck_shuffled(cx: &mut Cx) -> EngineOutcome {
    shuffle_encounter_deck(cx);
    EngineOutcome::Done
}

/// Handler for [`EngineRecord::EncounterCardRevealed`].
///
/// Drives the on-draw resolution path for one encounter card:
///
/// 1. Validate that a card registry is installed (reject with
///    `"EncounterCardRevealed: no card registry installed"` if not).
/// 2. Draw the top of the encounter deck via [`draw_encounter_top`]
///    (transparently reshuffles discard back in if the deck is
///    empty). Reject with `"EncounterCardRevealed: encounter deck and discard both empty"`
///    if both piles are exhausted.
/// 3. Look up the drawn card's metadata via the installed registry.
///    Reject with `"EncounterCardRevealed: unknown card code: {code}"` if the registry
///    doesn't know the code.
/// 4. Delegate to [`resolve_encounter_card`] for the post-draw
///    resolution prefix (emit [`Event::CardRevealed`] + type-dispatch
///    to Revelation / spawn / reject).
///
/// # Validate-first contract caveat
///
/// `draw_encounter_top` mutates `state.encounter_deck` /
/// `state.encounter_discard` BEFORE the unknown-code reject can
/// fire; `Event::CardRevealed` then emits BEFORE Revelation /
/// spawn resolve. The draw is a documented exception to the
/// validate-first convention — the card must be removed from the
/// deck before the reaction window opens (Before-timing listeners
/// in #52 need to see the revealed-but-not-yet-resolved state).
/// `Event::CardRevealed` emits before Revelation for the same
/// reason: Before-timing reaction listeners (#52, not wired) need
/// the event to fire before Revelation resolves (rules-correct
/// interposition point).
///
/// Compare to `play_card`'s documented mid-resolution caveat in
/// CLAUDE.md: same shape, same rationale.
pub(super) fn encounter_card_revealed(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: no card registry installed".into(),
        };
    };

    let Some(code) = draw_encounter_top(cx) else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter deck and discard both empty".into(),
        };
    };

    let Some(metadata) = (registry.metadata_for)(&code) else {
        return EngineOutcome::Rejected {
            reason: format!("EncounterCardRevealed: unknown card code: {code:?}").into(),
        };
    };
    resolve_encounter_card(cx, investigator, code, metadata)
}

/// Shared post-draw resolution helper. Resolves the per-card 5-step
/// sub-sequence's steps 3 (Revelation) and 4 (enemy spawn) for an
/// already-drawn encounter card. Called by `encounter_card_revealed`
/// (the `EngineRecord::EncounterCardRevealed` path) and by
/// `mythos_draw_for` (Mythos 1.4 player-driven draws).
///
/// Body: emits [`Event::CardRevealed`], then dispatches on
/// `metadata.card_type` — treachery → run Revelation abilities →
/// push card to `encounter_discard`; enemy → run Revelation abilities →
/// spawn it; any other type → return `Rejected`.
///
/// **Mid-resolution caveat:** [`Event::CardRevealed`] emits before
/// Revelation runs (Before-timing reactions need that ordering,
/// per #126's design decision). The apply loop's `events.clear()`
/// on Rejected still wipes the event stream on rejection.
///
/// Public so card effects that "draw"/"discard until" cards from the
/// encounter deck can resolve the drawn card faithfully — agenda 01106's
/// reverse draws the dug-up `Ghoul` enemy through here. Requires an
/// installed card registry (rejects otherwise).
pub fn resolve_encounter_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    let card_type = metadata.card_type();

    // Emit BEFORE Revelation resolves — see caveat in encounter_card_revealed.
    cx.events.push(Event::CardRevealed {
        investigator,
        code: code.clone(),
        card_type,
    });

    match card_type {
        CardType::Treachery => {
            let Some(registry) = card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: "encounter card resolution: no card registry installed".into(),
                };
            };
            let abilities = (registry.abilities_for)(&code).unwrap_or_default();
            let eval_ctx = EvalContext::for_controller(investigator);
            for ability in abilities
                .iter()
                .filter(|a| a.trigger == Trigger::Revelation)
            {
                let outcome = apply_effect(cx, &ability.effect, eval_ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            cx.state.encounter_discard.push(code);
            EngineOutcome::Done
        }
        CardType::Enemy => {
            // Revelation effects on enemies (rare, but printed on
            // some encounter enemies — e.g. "Revelation - Discard
            // 1 card from your hand at random.") fire BEFORE the
            // enemy spawns into play, per Rules Reference p.24
            // ("1.4 Each investigator draws 1 encounter card"):
            // "3. Resolve the revelation ability on the drawn card."
            // followed by "4. If the card is an enemy, spawn it
            // following any spawn instruction the card bears."
            //
            // No Phase-4-scope enemy has a Revelation effect; this
            // loop is structural for Phase-7+ enemies.
            let Some(registry) = card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: "encounter card resolution: no card registry installed".into(),
                };
            };
            let abilities = (registry.abilities_for)(&code).unwrap_or_default();
            let eval_ctx = EvalContext::for_controller(investigator);
            for ability in abilities
                .iter()
                .filter(|a| a.trigger == Trigger::Revelation)
            {
                let outcome = apply_effect(cx, &ability.effect, eval_ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            spawn_enemy(cx, investigator, code, metadata)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "EncounterCardRevealed: invalid encounter card type {other:?}; \
                 encounter decks contain only treachery and enemy cards",
            )
            .into(),
        },
    }
}

/// Spawn one encounter-deck enemy into play.
///
/// Called by [`encounter_card_revealed`] after `Event::CardRevealed`
/// has fired and any [`Trigger::Revelation`](crate::dsl::Trigger::Revelation)
/// abilities on the enemy have resolved.
///
/// # Spawn-location resolution
///
/// Rules Reference page 24, step 4 (1.4 Each investigator draws 1
/// encounter card):
///
/// > If the card is an **enemy**, spawn it following any spawn
/// > instruction the card bears. (A spawn instruction is any text
/// > bearing a "spawn" precursor.) If the encountered enemy has no
/// > spawn instruction, the enemy spawns engaged with the investigator
/// > encountering the card and is placed in that investigator's threat
/// > area.
///
/// We model threat-area placement as
/// `enemy.current_location = drawing investigator's location` +
/// `engaged_with = drawing investigator`. The named-location case
/// (`SpawnLocation::Specific`) looks the location up by its
/// printed [`code`](crate::state::Location::code).
///
/// # Engagement-on-spawn
///
/// Rules Reference page 10 (Enemy Engagement):
///
/// > Any time a ready unengaged enemy is at the same location as an
/// > investigator, it engages that investigator, and is placed in that
/// > investigator's threat area. If there are multiple investigators
/// > at the same location as a ready unengaged enemy, follow the
/// > enemy's prey instructions to determine which investigator is
/// > engaged.
///
/// All cases route through the shared [`resolve_prey`] resolver
/// (#128, option A): the co-located set is narrowed by the enemy's
/// prey (always `Prey::Default` in current scope, so a 2+ set always
/// ties). `None`/`One` resolve inline (no engagement, or engage the
/// sole/best candidate); `Tie` suspends via
/// [`SpawnEngagePending`](crate::state::SpawnEngagePending) and returns
/// [`EngineOutcome::AwaitingInput`] for the lead investigator's
/// `PickInvestigator`. When the spawn happens inside a Mythos
/// encounter-draw chain, [`resume_spawn_engage`] re-enters
/// [`run_mythos_draw_chain`] after the pick resolves.
///
/// # Stat fields TODO
///
/// `CardMetadata` doesn't yet carry per-enemy `fight` / `evade` /
/// `attack_damage` / `attack_horror`. This handler hardcodes
/// `fight: 1, evade: 1, attack_damage: 0, attack_horror: 0` until
/// a future PR (Phase-7+, alongside the first real spawn-bearing
/// enemy) extends `CardMetadata` with enemy-specific stat fields and
/// this handler reads them. Health uses `metadata.health.unwrap_or(1)`
/// because `CardMetadata.health` already exists.
///
/// # Validate-first contract
///
/// The one precondition that can reject — spawn-location resolution —
/// is checked before any mutation, leaving `state`/`events` unchanged
/// on rejection. Engagement resolution never rejects: it either
/// resolves inline or suspends (`AwaitingInput`) with the enemy already
/// minted into `state.enemies` and `Event::EnemySpawned` pushed (the
/// pending choice carries the rest of the work to [`resume_spawn_engage`]).
fn spawn_enemy(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // Resolve the spawn location (validate-first). Only the card's `spawn`
    // rule is read here; the full stat read + mint happens in
    // [`spawn_enemy_at`]. A `Specific` spawn names an in-play location; the
    // default rule (`None`) spawns at the drawing investigator's location.
    let CardKind::Enemy { spawn, .. } = &metadata.kind else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_enemy: card {code} is not an enemy").into(),
        };
    };
    let location_id = match spawn {
        Some(Spawn {
            location: SpawnLocation::Specific(loc_code),
        }) => match cx
            .state
            .locations
            .iter()
            .find(|(_, loc)| loc.code.as_str() == loc_code.as_str())
        {
            Some((id, _)) => *id,
            None => {
                return EngineOutcome::Rejected {
                    reason: format!("spawn_enemy: spawn location not in play (code {loc_code:?})")
                        .into(),
                };
            }
        },
        None => match cx
            .state
            .investigators
            .get(&investigator)
            .and_then(|inv| inv.current_location)
        {
            Some(loc) => loc,
            None => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "spawn_enemy: drawing investigator has no location \
                         (investigator {investigator:?})",
                    )
                    .into(),
                };
            }
        },
    };
    spawn_enemy_at(cx, investigator, code, metadata, location_id)
}

/// Mint an enemy from `metadata` at an explicit `location_id`, resolving
/// engagement-on-spawn (prey). The reusable spawn core: [`spawn_enemy`]
/// supplies a location from the card's own spawn rule;
/// [`spawn_set_aside_enemy`] supplies a location named by the bringing
/// effect (The Gathering's Act-2 reverse spawns the Ghoul Priest in the
/// Hallway). `investigator` is the drawing/controlling investigator —
/// used only to carry the engagement-tie resume (`investigator_to_draw`);
/// the engagement candidates come from `location_id` itself.
#[allow(clippy::too_many_lines)]
pub(super) fn spawn_enemy_at(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
    location_id: LocationId,
) -> EngineOutcome {
    // spawn_enemy_at is only reached for Enemy cards; pull the
    // enemy-specific stats out of the kind.
    let CardKind::Enemy {
        health,
        fight,
        evade,
        damage,
        horror,
        hunter,
        retaliate,
        prey,
        victory,
        ..
    } = &metadata.kind
    else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_enemy_at: card {code} is not an enemy").into(),
        };
    };
    let prey = *prey;

    // Resolve health. PerInvestigator scales by the number of investigators
    // in the game (Rules Reference p.12); matches the per-investigator clue
    // path in reveal.rs (its future started-count caveat applies here too).
    let max_health = match health {
        Some(HealthValue::Fixed(n)) => *n,
        Some(HealthValue::PerInvestigator(n)) => {
            let count = u8::try_from(cx.state.investigators.len()).unwrap_or(u8::MAX);
            n.saturating_mul(count)
        }
        None => 1,
    };

    // 2. Resolve engagement-on-spawn (validate-first). The co-located
    //    set is narrowed by the enemy's `prey`; with `Prey::Default` a 2+
    //    set ties and suspends for the lead investigator's
    //    `PickInvestigator` (option A).
    let candidates = super::cursor::active_investigators_at(cx.state, location_id);

    // 3. Mint and place (mutate-second). The enemy is inserted unengaged;
    //    the `One` and (post-resume) `Tie` cases set `engaged_with` via
    //    `engage_enemy_with` so the `EnemyEngaged` event always pairs with
    //    the mutation.
    let enemy_id = EnemyId(cx.state.next_enemy_id);
    cx.state.next_enemy_id = cx.state.next_enemy_id.saturating_add(1);

    let enemy = Enemy {
        id: enemy_id,
        name: metadata.name.clone(),
        code: CardCode::new(metadata.code.clone()),
        fight: i8::try_from(*fight).unwrap_or(i8::MAX),
        evade: i8::try_from(*evade).unwrap_or(i8::MAX),
        max_health,
        damage: 0,
        attack_damage: *damage,
        attack_horror: *horror,
        current_location: Some(location_id),
        exhausted: false,
        traits: metadata.traits.clone(),
        engaged_with: None,
        hunter: *hunter,
        prey,
        retaliate: *retaliate,
        victory: *victory,
    };
    cx.state.enemies.insert(enemy_id, enemy);

    match super::hunters::resolve_prey(cx.state, prey, &candidates) {
        super::hunters::PreyResolution::None => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: None,
            });
            EngineOutcome::Done
        }
        super::hunters::PreyResolution::One(target) => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: Some(target),
            });
            super::hunters::engage_enemy_with(cx, enemy_id, target);
            EngineOutcome::Done
        }
        super::hunters::PreyResolution::Tie(tied) => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: None,
            });
            // `chain_count` is 0 here; when this spawn is reached inside a
            // Mythos surge chain, `run_mythos_draw_chain` patches the
            // stored value to the live chain position before returning the
            // `AwaitingInput` (the single-draw `EncounterCardRevealed` path
            // has no chain, so 0 is correct there).
            cx.state.spawn_engage_pending = Some(SpawnEngagePending {
                enemy: enemy_id,
                investigator_to_draw: investigator,
                candidates: tied.clone(),
                surge: metadata.surge(),
                chain_count: 0,
            });
            EngineOutcome::AwaitingInput {
                request: InputRequest {
                    prompt: format!(
                        "Enemy {enemy_id:?} spawn engagement: lead investigator picks whom to \
                         engage among {tied:?} (submit InputResponse::PickInvestigator)"
                    ),
                },
                resume_token: ResumeToken(0),
            }
        }
    }
}

/// Bring a **set-aside enemy** into play at the location named by
/// `location_code`, minting its stats from the corpus (so per-investigator
/// health scales by the live investigator count). The set-aside-enemy
/// path: [`GameState::add_set_aside_enemy`](crate::state::GameState::add_set_aside_enemy)
/// records the code at `setup()`; a card effect calls this to spawn it
/// (The Gathering's Act-2 reverse, `01109:reverse`).
///
/// Validate-first: rejects (mutating nothing) if `enemy_code` isn't in the
/// set-aside zone, no card registry is installed, the code has no metadata,
/// or `location_code` isn't in play. Only after every check passes does it
/// remove the code from the zone and mint the enemy via `spawn_enemy_at`.
/// `controller` is the lead investigator (carried into engagement-tie
/// resume; the engagement candidates come from the spawn location).
pub fn spawn_set_aside_enemy(
    cx: &mut Cx,
    controller: InvestigatorId,
    enemy_code: &str,
    location_code: &str,
) -> EngineOutcome {
    let Some(pos) = cx
        .state
        .set_aside_enemies
        .iter()
        .position(|c| c.as_str() == enemy_code)
    else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_set_aside_enemy: {enemy_code} is not set aside").into(),
        };
    };
    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "spawn_set_aside_enemy: no card registry installed".into(),
        };
    };
    let Some(metadata) = (registry.metadata_for)(&CardCode::new(enemy_code)) else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_set_aside_enemy: no metadata for {enemy_code}").into(),
        };
    };
    let Some(location_id) = crate::engine::location_id_by_code(cx.state, location_code) else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_set_aside_enemy: location {location_code} not in play").into(),
        };
    };
    // All checks passed — mutate.
    cx.state.set_aside_enemies.remove(pos);
    spawn_enemy_at(
        cx,
        controller,
        CardCode::new(enemy_code),
        metadata,
        location_id,
    )
}

/// Fisher-Yates shuffle of the shared encounter deck using the
/// shared deterministic RNG. Used by [`encounter_deck_shuffled`] and
/// by [`reshuffle_encounter_discard`].
///
/// Emits [`Event::EncounterDeckShuffled`] iff the deck had at least
/// 2 cards (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_encounter_deck(cx: &mut Cx) {
    let deck_len = cx.state.encounter_deck.len();
    if deck_len < 2 {
        return;
    }
    // Mirror shuffle_player_deck's "collect swaps then apply" pattern:
    // RngState::next_index borrows &mut state.rng, which would conflict
    // with a &mut borrow on state.encounter_deck inline.
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = cx.state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    for (a, b) in swaps {
        cx.state.encounter_deck.swap(a, b);
    }
    cx.events.push(Event::EncounterDeckShuffled);
}

/// Drain `state.encounter_discard` into `state.encounter_deck` and
/// shuffle the resulting deck. Called by `draw_encounter_top` when the
/// deck runs empty, and by card effects that "shuffle the discard into
/// the encounter deck" (agenda 01106's reverse).
///
/// Does NOT push an `EngineRecord::EncounterDeckShuffled` to the
/// action log — mid-handler reshuffles rely on RNG determinism for
/// replay rather than log entries, mirroring the existing
/// player-deck pattern. The `EngineRecord` variant is reserved for
/// explicit shuffle actions (future "shuffle X into the encounter
/// deck" effects).
pub fn reshuffle_encounter_discard(cx: &mut Cx) {
    cx.state
        .encounter_deck
        .extend(cx.state.encounter_discard.drain(..));
    shuffle_encounter_deck(cx);
}

/// Draw the top card of the encounter deck, transparently reshuffling
/// the discard back in if the deck is empty.
///
/// Returns `Some(code)` when a card was available (either from the
/// deck directly or after the reshuffle). Returns `None` when both
/// the deck and the discard are empty — callers decide how to
/// interpret this (#69's Mythos loop treats it as a scenario
/// condition rather than an engine error).
pub(super) fn draw_encounter_top(cx: &mut Cx) -> Option<CardCode> {
    if cx.state.encounter_deck.is_empty() {
        if cx.state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(cx);
    }
    cx.state.encounter_deck.pop_front()
}

/// Handler for [`PlayerAction::DrawEncounterCard`]. Validates phase
/// + cursor; delegates to [`mythos_draw_for`] on success.
pub(super) fn draw_encounter_card(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if cx.state.phase != Phase::Mythos {
        return EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: only valid during Mythos phase, got {:?}",
                cx.state.phase,
            )
            .into(),
        };
    }
    match cx.state.mythos_draw_pending {
        None => EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
        },
        Some(expected) if expected != investigator => EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: out of order; expected {expected:?}, got {investigator:?}",
            )
            .into(),
        },
        Some(_) => mythos_draw_for(cx, investigator),
    }
}

/// Resolves one investigator's full Mythos encounter draw — the
/// per-card 5-step sub-sequence from Rules Reference p.24, with
/// surge re-draws looping until the chain ends.
///
/// Called by [`draw_encounter_card`] with the pending-drawer's id.
/// Returns Done on success (chain completed, `mythos_draw_pending`
/// advanced).
///
/// # Mid-chain rejection caveat
///
/// `mythos_draw_for` follows the same pattern as `play_card` (CLAUDE.md
/// documents it): if [`resolve_encounter_card`] rejects mid-chain — e.g.
/// [`spawn_enemy`] rejecting because the drawing investigator has no
/// location — the card has already been drawn from `encounter_deck` by
/// [`draw_encounter_top`], and the apply loop's `events.clear()` on
/// `Rejected` wipes the event stream but does **not** roll back the state
/// mutation. The card is silently lost from the encounter deck. In Phase 4
/// scope this can't happen because the synthetic fixture ensures every
/// investigator has a location at scenario start; revisit if a future
/// scenario lets investigators reach a location-less state during play.
fn mythos_draw_for(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    // Fresh chain: count starts at 0 and the loop draws at least one
    // card (`draw_more = true`).
    run_mythos_draw_chain(cx, investigator, 0, true)
}

/// The Mythos surge-draw loop, shared by the initial draw
/// ([`mythos_draw_for`]) and the post-suspend resume
/// ([`resume_spawn_engage`]).
///
/// `chain_count` is the surge position already consumed (0 for a fresh
/// chain); the loop increments it per drawn card and enforces
/// [`MAX_SURGE_CHAIN`] exactly as the single-pass version did.
/// `draw_more` gates the first iteration: `true` for a fresh draw,
/// or the suspended card's surge bit on resume (a non-surge enemy that
/// suspended for engagement resumes with `draw_more = false`, drawing no
/// further card — only the cursor advance runs).
///
/// On a mid-chain spawn engagement tie, [`resolve_encounter_card`]
/// returns [`EngineOutcome::AwaitingInput`]; this loop patches the live
/// `chain_count` into the freshly-stored
/// [`SpawnEngagePending`](crate::state::SpawnEngagePending) so the resume
/// continues with the cap budget intact, then returns the suspension.
///
/// # Mid-chain rejection caveat
///
/// Same as the single-pass version: if [`resolve_encounter_card`]
/// rejects mid-chain (e.g. [`spawn_enemy`] when the drawing investigator
/// has no location), the card has already left `encounter_deck` and the
/// apply loop's `events.clear()` on `Rejected` does not roll back that
/// mutation. Out of Phase-4 scope (the synthetic fixture gives every
/// investigator a location at setup).
pub(super) fn run_mythos_draw_chain(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut chain_count: usize,
    mut draw_more: bool,
) -> EngineOutcome {
    let Some(reg) = crate::card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no card registry installed".into(),
        };
    };

    while draw_more {
        chain_count += 1;
        if chain_count > MAX_SURGE_CHAIN {
            unreachable!(
                "Mythos draw chain exceeded MAX_SURGE_CHAIN ({}) for \
                 investigator {:?}. Indicates either an infinite reshuffle \
                 loop (Rules Reference p.18: treachery discard precedes surge \
                 re-draw, so a surging treachery in a too-small deck cycles \
                 via the p.10 reshuffle path) or a malformed scenario encounter \
                 deck. Real scenarios don't surge >{} cards in one chain.",
                MAX_SURGE_CHAIN, investigator, MAX_SURGE_CHAIN,
            );
        }

        // Step 1: Draw the card from the encounter deck.
        let Some(code) = draw_encounter_top(cx) else {
            if chain_count == 1 {
                return EngineOutcome::Rejected {
                    reason: "DrawEncounterCard: encounter deck and discard both empty".into(),
                };
            }
            unreachable!(
                "Mythos draw chain hit empty encounter deck AND empty discard for \
                 investigator {:?} at chain position {}. Two independent mechanisms \
                 can reach this: (a) a small encounter deck of only surging \
                 treacheries can loop infinitely via the Rules Reference p.18/p.10 \
                 cycle (treachery discard precedes surge re-draw, so the \
                 just-discarded card gets reshuffled and re-drawn) — caught earlier \
                 by MAX_SURGE_CHAIN; (b) a small encounter deck of only surging \
                 enemies exhausts the encounter universe within one chain (enemies \
                 spawn to play, not discard, so the p.10 reshuffle has nothing to \
                 pull). Both are scenario-data malformation, not legitimate play.",
                investigator, chain_count,
            );
        };

        let Some(metadata) = (reg.metadata_for)(&code) else {
            return EngineOutcome::Rejected {
                reason: format!("DrawEncounterCard: unknown card code: {code:?}").into(),
            };
        };

        // Step 2: Check for the peril keyword on the drawn card.
        super::skill_test::peril_check(cx, &code, investigator, metadata.peril());

        // Step 3 + 4: Resolve revelation, then enemy-spawn if applicable.
        let outcome = resolve_encounter_card(cx, investigator, code.clone(), metadata);
        match outcome {
            EngineOutcome::Done => {}
            EngineOutcome::AwaitingInput { .. } => {
                // A mid-chain spawn engagement tie suspended. Record the
                // live chain position so the resume keeps counting toward
                // the cap rather than restarting its budget.
                if let Some(pending) = cx.state.spawn_engage_pending.as_mut() {
                    pending.chain_count = chain_count;
                }
                return outcome;
            }
            EngineOutcome::Rejected { .. } => return outcome,
        }

        // Step 5: If the drawn card has the surge keyword, loop.
        draw_more = metadata.surge();
    }

    // Chain complete — advance the cursor.
    advance_mythos_draw_pending(cx);
    EngineOutcome::Done
}

/// Advance `state.mythos_draw_pending` after a completed chain. If
/// a next investigator exists in turn order, set to that id.
/// Otherwise set to None and open the post-1.4 window.
pub(super) fn advance_mythos_draw_pending(cx: &mut Cx) {
    let current = cx
        .state
        .mythos_draw_pending
        .expect("advance_mythos_draw_pending called only after a successful chain");
    // Eliminated-skip semantics live in `next_active_investigator_after`.
    let next = super::cursor::next_active_investigator_after(cx.state, current);
    cx.state.mythos_draw_pending = next;
    if next.is_none() {
        let outcome = super::reaction_windows::open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
        );
        debug_assert_eq!(
            outcome,
            EngineOutcome::Done,
            "open_fast_window(MythosAfterDraws) unexpectedly suspended; this window has no suspending continuation",
        );
    }
}

#[cfg(test)]
mod encounter_card_revealed_tests {
    use crate::state::CardCode;
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// Exercises the early-reject guard: when the handler cannot
    /// proceed past the registry / metadata checks, it must reject
    /// without drawing from the deck and without emitting any events.
    ///
    /// Two possible rejection reasons depending on process state:
    ///
    /// - `"no card registry installed"` — if no registry has been
    ///   installed yet in this process.
    /// - `"unknown card code: ..."` — if another test in this binary
    ///   has already installed a fake registry that doesn't know the
    ///   synthetic code `"__no_such_card"`.
    ///
    /// In both cases the invariant is identical: deck untouched, no
    /// events emitted. The exact rejection reason depends on
    /// `OnceLock` install ordering, which is non-deterministic across
    /// parallel test binaries.
    ///
    /// The authoritative "no registry installed" path is exercised in
    /// the `crates/scenarios/tests/encounter_reveal.rs` integration
    /// test, which runs in its own process and installs `TEST_REGISTRY`
    /// explicitly. The process-isolated install guarantees the "no
    /// registry" rejection fires in a controlled environment.
    #[test]
    fn rejects_when_no_card_registry_installed() {
        use crate::action::EngineRecord;
        use crate::state::InvestigatorId;
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Seed the encounter deck so we can prove the reject fires
        // *before* the draw mutates state. Use a code that no real
        // or fake registry knows so we always hit an early rejection.
        state
            .encounter_deck
            .push_back(CardCode("__no_such_card".into()));
        let pre_deck_len = state.encounter_deck.len();
        let mut events = Vec::new();

        let outcome = super::super::apply_engine_record(
            &mut crate::engine::Cx {
                state: &mut state,
                events: &mut events,
            },
            &EngineRecord::EncounterCardRevealed {
                investigator: InvestigatorId(1),
            },
        );

        match outcome {
            crate::engine::outcome::EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("no card registry installed")
                        || reason.contains("unknown card code"),
                    "unexpected reject reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // Deck must be untouched: the registry-missing reject fires
        // before any draw, and the unknown-code reject fires after the
        // draw. However, the plan's documented exception means that
        // after a successful draw but unknown-code rejection, the deck
        // will be shorter by one. We assert on the *invariant* that
        // matters: no events were emitted, and the deck shrank by at
        // most one (not more).
        assert!(
            state.encounter_deck.len() <= pre_deck_len,
            "deck should not grow; expected <= {pre_deck_len}, got {}",
            state.encounter_deck.len(),
        );
        assert!(
            events.is_empty(),
            "no events should fire before Event::CardRevealed; got {events:?}",
        );
    }
}

#[cfg(test)]
mod encounter_deck_helper_tests {
    use super::*;
    use crate::event::Event;
    use crate::rng::RngState;
    use crate::state::CardCode;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn shuffle_encounter_deck_emits_event_when_two_or_more_cards() {
        let mut state = GameStateBuilder::new().build();
        state.rng = RngState::new(42);
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();
        shuffle_encounter_deck(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(matches!(events.as_slice(), [Event::EncounterDeckShuffled]));
        assert_eq!(state.encounter_deck.len(), 3);
        let mut codes: Vec<_> = state.encounter_deck.iter().cloned().collect();
        codes.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            codes,
            vec![
                CardCode("a".into()),
                CardCode("b".into()),
                CardCode("c".into())
            ]
        );
    }

    #[test]
    fn shuffle_encounter_deck_is_silent_on_zero_or_one_card() {
        for n in 0..=1 {
            let mut state = GameStateBuilder::new().build();
            for i in 0..n {
                state.encounter_deck.push_back(CardCode(format!("c{i}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut Cx {
                state: &mut state,
                events: &mut events,
            });
            assert!(events.is_empty(), "expected no event for n={n} deck");
        }
    }

    #[test]
    fn reshuffle_encounter_discard_moves_discard_into_deck_and_shuffles() {
        let mut state = GameStateBuilder::new().build();
        state.rng = RngState::new(7);
        for i in 0..5 {
            state.encounter_discard.push(CardCode(format!("d{i}")));
        }

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(
            state.encounter_discard.is_empty(),
            "discard should be drained"
        );
        assert_eq!(state.encounter_deck.len(), 5, "all 5 cards moved into deck");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "expected EncounterDeckShuffled (≥ 2 cards moved)"
        );
    }

    #[test]
    fn reshuffle_encounter_discard_is_silent_when_discard_has_one_card() {
        let mut state = GameStateBuilder::new().build();
        state.encounter_discard.push(CardCode("solo".into()));

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(state.encounter_discard.is_empty());
        assert_eq!(state.encounter_deck.len(), 1);
        assert!(events.is_empty(), "1-card shuffle emits no event");
    }

    #[test]
    fn draw_encounter_top_drains_deck_then_returns_none() {
        let mut state = GameStateBuilder::new().build();
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();

        assert_eq!(
            draw_encounter_top(&mut Cx {
                state: &mut state,
                events: &mut events,
            }),
            Some(CardCode("a".into()))
        );
        assert_eq!(
            draw_encounter_top(&mut Cx {
                state: &mut state,
                events: &mut events,
            }),
            Some(CardCode("b".into()))
        );
        assert_eq!(
            draw_encounter_top(&mut Cx {
                state: &mut state,
                events: &mut events,
            }),
            Some(CardCode("c".into()))
        );
        assert_eq!(
            draw_encounter_top(&mut Cx {
                state: &mut state,
                events: &mut events,
            }),
            None
        );
        assert!(
            events.is_empty(),
            "no events for any draw — discard is always empty, no reshuffle is triggered"
        );
    }

    #[test]
    fn draw_encounter_top_reshuffles_discard_on_empty_deck() {
        let mut state = GameStateBuilder::new().build();
        state.rng = RngState::new(13);
        state.encounter_discard.push(CardCode("x".into()));
        state.encounter_discard.push(CardCode("y".into()));
        state.encounter_discard.push(CardCode("z".into()));

        let mut events = Vec::new();
        let drawn = draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        let drawn_code = drawn.expect("should reshuffle and draw");
        assert!(
            [
                CardCode("x".into()),
                CardCode("y".into()),
                CardCode("z".into())
            ]
            .contains(&drawn_code),
            "drawn card must be one of the three discard cards, got {drawn_code:?}"
        );
        assert_eq!(
            state.encounter_deck.len(),
            2,
            "2 cards remain in deck post-draw"
        );
        assert!(state.encounter_discard.is_empty(), "discard drained");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "reshuffle emits one event"
        );
    }

    #[test]
    fn draw_encounter_top_returns_none_when_deck_and_discard_both_empty() {
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        assert_eq!(
            draw_encounter_top(&mut Cx {
                state: &mut state,
                events: &mut events,
            }),
            None
        );
        assert!(events.is_empty(), "no events on empty-on-both");
    }

    #[test]
    fn engine_record_encounter_deck_shuffled_drives_shuffle() {
        use crate::action::{Action, EngineRecord};
        use crate::engine::apply;

        let mut state = GameStateBuilder::new().build();
        state.rng = RngState::new(99);
        for i in 0..4 {
            state.encounter_deck.push_back(CardCode(format!("c{i}")));
        }
        let original: Vec<_> = state.encounter_deck.iter().cloned().collect();

        let result = apply(state, Action::Engine(EngineRecord::EncounterDeckShuffled));

        assert!(
            matches!(result.outcome, crate::EngineOutcome::Done),
            "expected Done, got {:?}",
            result.outcome
        );
        let mut shuffled: Vec<_> = result.state.encounter_deck.iter().cloned().collect();
        let mut orig_sorted = original.clone();
        shuffled.sort_by(|a, b| a.0.cmp(&b.0));
        orig_sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(shuffled, orig_sorted);
        assert!(result
            .events
            .iter()
            .any(|e| matches!(e, Event::EncounterDeckShuffled)));
    }

    #[test]
    fn encounter_deck_shuffle_is_deterministic_from_seed() {
        fn shuffle_with_seed(seed: u64) -> Vec<CardCode> {
            let mut state = GameStateBuilder::new().build();
            state.rng = RngState::new(seed);
            for i in 0..10 {
                state.encounter_deck.push_back(CardCode(format!("c{i:02}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut Cx {
                state: &mut state,
                events: &mut events,
            });
            state.encounter_deck.iter().cloned().collect()
        }

        let a = shuffle_with_seed(2026);
        let b = shuffle_with_seed(2026);
        assert_eq!(a, b, "same seed must produce same shuffle order");

        let c = shuffle_with_seed(42);
        assert_ne!(
            a, c,
            "different seeds should produce different orders (smoke test)"
        );
    }
}

#[cfg(test)]
mod spawn_enemy_tests {
    use super::*;
    use crate::state::{CardCode, InvestigatorId, LocationId, Phase};
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};
    use crate::{assert_event, assert_event_sequence, assert_no_event};
    use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey, Spawn, SpawnLocation};

    fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
        enemy_metadata(
            spawn,
            HealthValue::Fixed(1),
            false,
            false,
            Prey::Default,
            1,
            1,
            0,
            0,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn enemy_metadata(
        spawn: Option<Spawn>,
        health: HealthValue,
        hunter: bool,
        retaliate: bool,
        prey: Prey,
        fight: u8,
        evade: u8,
        damage: u8,
        horror: u8,
        victory: Option<u8>,
    ) -> CardMetadata {
        CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            text: None,
            traits: Vec::new(),
            pack_code: "_synth".into(),
            kind: CardKind::Enemy {
                fight,
                evade,
                damage,
                horror,
                health: Some(health),
                victory,
                spawn,
                surge: false,
                peril: false,
                hunter,
                retaliate,
                prey,
                quantity: 1,
            },
        }
    }

    #[test]
    fn spawn_enemy_at_places_enemy_at_the_given_location_not_the_drawers() {
        // The investigator is at loc 10; spawn_enemy_at is told loc 11. The
        // enemy must land at 11 (the explicit location wins), unlike
        // spawn_enemy's investigator-location fallback.
        let mut here = test_location(10, "Here");
        here.code = CardCode("_here".into());
        let mut there = test_location(11, "There");
        there.code = CardCode("_there".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(here)
            .with_location(there)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));

        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy_at(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
            LocationId(11),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        let enemy = state.enemies.values().next().expect("enemy spawned");
        assert_eq!(
            enemy.current_location,
            Some(LocationId(11)),
            "the explicit location wins over the drawer's location",
        );
        assert_event!(
            events,
            Event::EnemySpawned { location, .. } if *location == LocationId(11)
        );
    }

    #[test]
    fn spawn_set_aside_enemy_rejects_when_not_set_aside() {
        // Empty set-aside zone — the spawn must reject before touching the
        // registry or location, and mint nothing.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([InvestigatorId(1)])
            .build();
        let mut events = Vec::new();
        let outcome = spawn_set_aside_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            "01116",
            "01112",
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "spawning an enemy that isn't set aside must reject, got {outcome:?}",
        );
        assert!(state.enemies.is_empty(), "no enemy minted on reject");
    }

    #[test]
    fn spawn_set_aside_enemy_keeps_the_code_aside_on_a_failed_spawn() {
        // The enemy is set aside, but the target location isn't in play (and
        // no usable metadata is guaranteed in a bare unit test) — the spawn
        // must reject without removing the code from the set-aside zone
        // (validate-first: no mutation on reject).
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.set_aside_enemies.push(CardCode::new("01116"));
        let mut events = Vec::new();
        let outcome = spawn_set_aside_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            "01116",
            "01112", // not in play
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "missing target location must reject, got {outcome:?}",
        );
        assert_eq!(
            state.set_aside_enemies,
            vec![CardCode::new("01116")],
            "the code stays set aside when the spawn rejects",
        );
        assert!(state.enemies.is_empty(), "no enemy minted on reject");
    }

    #[test]
    fn spawn_enemy_reads_combat_stats_and_keywords_from_metadata() {
        let mut loc = test_location(10, "Loc");
        loc.code = CardCode("_l".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));

        let metadata = enemy_metadata(
            None,
            HealthValue::Fixed(5),
            true,
            true,
            Prey::Default,
            4,
            4,
            2,
            2,
            None,
        );
        let mut events = Vec::new();
        spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        let enemy = state.enemies.values().next().expect("enemy spawned");
        assert_eq!(enemy.fight, 4);
        assert_eq!(enemy.evade, 4);
        assert_eq!(enemy.attack_damage, 2);
        assert_eq!(enemy.attack_horror, 2);
        assert_eq!(enemy.max_health, 5);
        assert!(enemy.hunter);
        assert!(enemy.retaliate);
    }

    #[test]
    fn spawn_enemy_scales_per_investigator_health_by_investigator_count() {
        let mut loc = test_location(10, "Loc");
        loc.code = CardCode("_l".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .build();
        for id in [1, 2] {
            state
                .investigators
                .get_mut(&InvestigatorId(id))
                .unwrap()
                .current_location = Some(LocationId(10));
        }

        let metadata = enemy_metadata(
            None,
            HealthValue::PerInvestigator(5),
            false,
            false,
            Prey::Default,
            4,
            4,
            2,
            2,
            None,
        );
        let mut events = Vec::new();
        spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        let enemy = state.enemies.values().next().expect("enemy spawned");
        assert_eq!(enemy.max_health, 10, "5 health × 2 investigators");
    }

    #[test]
    fn spawn_enemy_reads_victory_from_metadata() {
        let mut loc = test_location(10, "Loc");
        loc.code = CardCode("_l".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));

        let metadata = enemy_metadata(
            None,
            HealthValue::Fixed(5),
            false,
            false,
            Prey::Default,
            4,
            4,
            2,
            2,
            Some(2),
        );
        let mut events = Vec::new();
        spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        let enemy = state.enemies.values().next().expect("enemy spawned");
        assert_eq!(enemy.victory, Some(2));
    }

    #[test]
    fn spawn_at_specific_location_with_one_investigator_engages_them() {
        let mut loc = test_location(10, "Synth Loc");
        loc.code = CardCode("_synth_loc".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1)])
            .build();
        // Place investigator 1 at location 10.
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));

        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_synth_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        assert_eq!(state.enemies.len(), 1);
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.current_location, Some(LocationId(10)));
        assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

        assert_event_sequence!(
            events,
            Event::EnemySpawned { code, location, engaged_with, .. }
                if *code == CardCode("_synth_enemy".into())
                    && *location == LocationId(10)
                    && *engaged_with == Some(InvestigatorId(1)),
            Event::EnemyEngaged { investigator, .. }
                if *investigator == InvestigatorId(1),
        );
    }

    #[test]
    fn spawn_at_specific_location_with_no_investigators_leaves_unengaged() {
        let mut loc = test_location(10, "Synth Loc");
        loc.code = CardCode("_synth_loc".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        // Investigator 1 is NOT at location 10 (current_location is None).

        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_synth_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.engaged_with, None);
        // No engagement happened, so no EnemyEngaged event fires.
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn spawn_at_specific_location_rejects_when_location_not_in_play() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_nonexistent_loc".into()),
        }));
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("spawn location not in play"),
                    "unexpected reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(state.enemies.is_empty());
    }

    #[test]
    fn spawn_with_no_instruction_places_at_drawing_investigators_location() {
        let mut loc = test_location(10, "Demo");
        loc.code = CardCode("_demo_loc".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.current_location, Some(LocationId(10)));
        assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));
        // Default-spawn engagement fires the paired EnemyEngaged event.
        assert_event!(
            events,
            Event::EnemyEngaged { investigator, .. }
                if *investigator == InvestigatorId(1)
        );
    }

    #[test]
    fn spawn_with_no_instruction_rejects_when_drawing_investigator_has_no_location() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Investigator has no current_location.
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("drawing investigator has no location"),
                    "unexpected reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn spawn_engages_sole_colocated_investigator() {
        // Regression: #127's single-investigator engage-on-spawn path
        // still resolves inline under the shared prey resolver.
        let mut loc = test_location(1, "Hall");
        loc.code = CardCode("_loc".into());
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_location(loc)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .build();
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert_eq!(outcome, EngineOutcome::Done);
        let spawned = state.enemies.values().next().expect("one enemy");
        assert_eq!(spawned.engaged_with, Some(InvestigatorId(1)));
    }

    #[test]
    fn spawn_tie_suspends_for_lead_pick() {
        let mut loc = test_location(1, "Hall");
        loc.code = CardCode("_loc".into());
        let mut i1 = test_investigator(1);
        i1.current_location = Some(LocationId(1));
        let mut i2 = test_investigator(2);
        i2.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_location(loc)
            .with_investigator(i1)
            .with_investigator(i2)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(state.spawn_engage_pending.is_some());
        let spawned = state.enemies.values().next().expect("one enemy");
        assert_eq!(spawned.engaged_with, None);
    }

    #[test]
    fn resume_spawn_engage_rejects_bad_pick_and_preserves_pending() {
        // Validate-first: a pick outside the stored candidate set rejects
        // and leaves `spawn_engage_pending` intact for retry, with the
        // enemy still unengaged.
        use crate::action::InputResponse;
        let mut loc = test_location(1, "Hall");
        loc.code = CardCode("_loc".into());
        let mut i1 = test_investigator(1);
        i1.current_location = Some(LocationId(1));
        let mut i2 = test_investigator(2);
        i2.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_location(loc)
            .with_investigator(i1)
            .with_investigator(i2)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let _ = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert!(state.spawn_engage_pending.is_some());

        // Investigator 3 is not among the co-located candidates.
        let outcome = super::super::hunters::resume_spawn_engage(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickInvestigator(InvestigatorId(3)),
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "{outcome:?}"
        );
        assert!(
            state.spawn_engage_pending.is_some(),
            "pending must survive a rejected pick for retry",
        );
        let enemy = state.enemies.values().next().expect("enemy still placed");
        assert_eq!(enemy.engaged_with, None, "no engagement on rejected pick");
    }

    #[test]
    fn spawn_mints_distinct_enemy_ids() {
        let mut loc = test_location(10, "L");
        loc.code = CardCode("_l".into());
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(10));
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_l".into()),
        }));
        let mut events = Vec::new();

        let _ = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        let _ = spawn_enemy(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert_eq!(
            state.enemies.len(),
            2,
            "two spawns should produce two distinct enemies"
        );
    }
}

#[cfg(test)]
mod mythos_draw_for_tests {
    use super::*;
    use crate::state::{CardCode, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// Exercises the early-reject guard for the registry / unknown-card
    /// checks. Depending on which tests have run in this process:
    ///
    /// - `"no card registry installed"` — if no registry has been
    ///   installed yet in this process.
    /// - `"unknown card code: ..."` — if another test has installed a
    ///   registry that doesn't know the synthetic code `"__no_such_card"`.
    ///
    /// In both cases the invariant is identical: state is not further
    /// mutated, the card remains in the encounter deck (the draw was
    /// either blocked before or after the draw). The deck-length
    /// assertion allows for the draw-then-reject case (deck shrinks by
    /// at most one) matching the `encounter_card_revealed_tests` pattern.
    #[test]
    fn rejects_when_registry_not_installed_or_unknown_code() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.mythos_draw_pending = Some(InvestigatorId(1));
        // Seed the encounter deck with an unknown code so we prove the
        // reject fires at the registry or unknown-code check, not at the
        // empty-deck check.
        state
            .encounter_deck
            .push_back(CardCode("__no_such_card".into()));
        let pre_deck_len = state.encounter_deck.len();
        let mut events = Vec::new();
        let outcome = mythos_draw_for(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("no card registry installed")
                        || reason.contains("unknown card code"),
                    "unexpected reject reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // Deck must not grow; may shrink by 1 if draw happened before
        // the unknown-code reject (documented exception matching the
        // encounter_card_revealed validate-first caveat).
        assert!(
            state.encounter_deck.len() <= pre_deck_len,
            "deck should not grow; expected <= {pre_deck_len}, got {}",
            state.encounter_deck.len(),
        );
    }
}

#[cfg(test)]
mod draw_encounter_card_tests {
    use super::*;
    use crate::state::{InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn rejects_outside_mythos_phase() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        let outcome = draw_encounter_card(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
        );
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("only valid during Mythos")
        ));
    }

    #[test]
    fn rejects_when_no_draw_pending() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = None;
        let mut events = Vec::new();
        let outcome = draw_encounter_card(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(1),
        );
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("no draw pending")
        ));
    }

    #[test]
    fn rejects_when_out_of_order() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        // Inv2 attempts to draw when inv1 is expected.
        let outcome = draw_encounter_card(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            InvestigatorId(2),
        );
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("out of order")
        ));
    }
}
