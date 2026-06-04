//! Per-action dispatch handlers.
//!
//! Each function applies a single action variant to the state, mutating
//! the state in place and pushing the resulting events onto the events
//! buffer. Returns the [`EngineOutcome`] for the action.
//!
//! Handlers are split by `Action` bucket: [`apply_player_action`] for
//! human-initiated actions, [`apply_engine_record`] for engine-emitted
//! ones.

use crate::action::{EngineRecord, InputResponse, PlayerAction};
use crate::card_data::{CardMetadata, CardType, Spawn, SpawnLocation};
use crate::card_registry;
use crate::dsl::{Cost, Trigger};
use crate::event::Event;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, GameState, Investigator, InvestigatorId,
    Phase, SpawnEngagePending, Status, WindowKind, Zone,
};

use super::evaluator::{apply_effect, EvalContext};
use super::outcome::{EngineOutcome, InputRequest, ResumeToken};

mod actions;
mod combat;
mod cursor;
mod elimination;
mod hunters;
mod reaction_windows;
mod skill_test;

/// Action points granted to an investigator at the start of their
/// turn during the Investigation phase. Per the Arkham Horror LCG
/// rulebook.
const ACTIONS_PER_TURN: u8 = 3;

/// Starting hand size at scenario setup. Per the Rules Reference,
/// each investigator draws 5 cards before mulligan.
const INITIAL_HAND_SIZE: u8 = 5;

/// Hard cap on a single Mythos draw chain. Real scenarios surge ≤2
/// in a chain; the cap exists purely to guarantee termination on
/// malformed encounter decks (e.g. a deck small enough for surge to
/// loop via the Rules Reference p.10 reshuffle). `unreachable!`-class
/// — never reached in legitimate play.
///
const MAX_SURGE_CHAIN: usize = 64;

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// Phase-1 minimal coverage: [`StartScenario`](PlayerAction::StartScenario)
/// and [`EndTurn`](PlayerAction::EndTurn) are implemented end-to-end;
/// other variants return [`EngineOutcome::Rejected`] with a TODO message
/// so callers and tests get a useful signal rather than a silent no-op.
#[allow(clippy::too_many_lines)] // dispatcher: a guard ladder + one match arm per PlayerAction
pub fn apply_player_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    action: &PlayerAction,
) -> EngineOutcome {
    // While a mulligan is pending (the setup mulligan cursor is `Some`),
    // only Mulligan (and the already-rejected re-StartScenario) is valid.
    // Per the Rules Reference, "after all players have completed their
    // mulligans, the game begins" — the engine enforces that by gating
    // other actions until every investigator has signaled their mulligan
    // choice.
    if state.mulligan_pending.is_some()
        && !matches!(
            action,
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario
        )
    {
        return EngineOutcome::Rejected {
            reason: "a setup mulligan is pending; investigators must submit \
                     PlayerAction::Mulligan (with an empty indices_to_redraw to \
                     keep their hand) in player order before any other action"
                .into(),
        };
    }

    // Reaction-window guard runs BEFORE the skill-test guard: when a
    // window opens mid-skill-test (e.g. Roland's "after you defeat an
    // enemy" firing during a Fight that defeats), both
    // `in_flight_skill_test` and the open reaction window on
    // `state.open_windows` are populated — the test is mid-resolution,
    // parked at the window boundary inside `drive_skill_test`. The
    // reaction-window message is the one the client needs.
    if state.top_reaction_window().is_some() && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a reaction window is open; submit a \
                     PlayerAction::ResolveInput with an InputResponse::PickIndex \
                     to fire a pending trigger, or InputResponse::Skip to close \
                     the window (rejected if forced triggers remain) before any \
                     other action"
                .into(),
        };
    }

    // While a skill test is paused at its commit window (no reaction
    // window open yet), only `ResolveInput` can advance the engine.
    // Mirrors the `mulligan_pending` guard above.
    if state.in_flight_skill_test.is_some() && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a skill test is paused at its commit window; submit a \
                     PlayerAction::ResolveInput with an InputResponse::CommitCards \
                     (empty indices commits no cards) before any other action"
                .into(),
        };
    }

    // Hunter movement is Enemy-phase only; it can't coexist with an open
    // reaction window or an in-flight skill test, so order among the guards
    // is immaterial — but a pending hunter choice still blocks other actions.
    if state.hunter_move_pending.is_some() && !matches!(action, PlayerAction::ResolveInput { .. }) {
        return EngineOutcome::Rejected {
            reason: "a hunter-movement choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::PickLocation (movement) or \
                     InputResponse::PickInvestigator (engagement) before any other action"
                .into(),
        };
    }

    // A pending engagement-on-spawn choice (#128) likewise blocks every
    // action but `ResolveInput`. Mirrors the hunter guard above; the two
    // never coexist (different phases), so guard order is immaterial.
    if state.spawn_engage_pending.is_some() && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "an engagement-on-spawn choice is pending; submit a \
                     PlayerAction::ResolveInput with InputResponse::PickInvestigator \
                     before any other action"
                .into(),
        };
    }

    let outcome = match action {
        PlayerAction::StartScenario => start_scenario(state, events),
        PlayerAction::EndTurn => end_turn(state, events),
        PlayerAction::PerformSkillTest {
            investigator,
            skill,
            difficulty,
        } => skill_test::perform_skill_test(state, events, *investigator, *skill, *difficulty),
        PlayerAction::Investigate { investigator } => {
            actions::investigate(state, events, *investigator)
        }
        PlayerAction::Move {
            investigator,
            destination,
        } => actions::move_action(state, events, *investigator, *destination),
        PlayerAction::Draw { investigator } => draw(state, events, *investigator),
        PlayerAction::Mulligan {
            investigator,
            indices_to_redraw,
        } => mulligan(state, events, *investigator, indices_to_redraw),
        PlayerAction::Fight {
            investigator,
            enemy,
        } => actions::fight(state, events, *investigator, *enemy),
        PlayerAction::Evade {
            investigator,
            enemy,
        } => actions::evade(state, events, *investigator, *enemy),
        PlayerAction::PlayCard {
            investigator,
            hand_index,
        } => play_card(state, events, *investigator, *hand_index),
        PlayerAction::ActivateAbility {
            investigator,
            instance_id,
            ability_index,
        } => activate_ability(state, events, *investigator, *instance_id, *ability_index),
        PlayerAction::DrawEncounterCard => match state.mythos_draw_pending {
            // DrawEncounterCard carries no investigator payload — the
            // acting investigator IS the pending cursor.
            Some(actor) => draw_encounter_card(state, events, actor),
            None => EngineOutcome::Rejected {
                reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
            },
        },
        PlayerAction::ResolveInput { response } => resolve_input(state, events, response),
        PlayerAction::AdvanceAct { investigator } => {
            advance_act_action(state, events, *investigator)
        }
    };

    // After a successful Mulligan, check whether every investigator
    // has now mulliganed. If so, the cursor reaches `None` and normal
    // play begins. Assumes `mulligan()` only ever returns `Done` or
    // `Rejected` (never `AwaitingInput`) — if it ever grows an
    // input-prompt path, this gate must be revisited so the cursor
    // doesn't silently stay set across a partial mulligan.
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && state.mulligan_pending.is_none()
    {
        // Setup complete — "the game begins" (Rules Reference p.27).
        // Round 1 skips the Mythos phase (p.24), so the first phase to
        // begin is Investigation. Kick off its driver HERE, not in
        // start_scenario: setup has "no action windows" (p.27), so the
        // post-2.1 player window must not open until mulligans are done.
        //
        // NOTE: investigation_phase may leave an InvestigationBegins
        // window open (when a Fast-eligible play exists); this function
        // still returns the Mulligan's `Done`. So this is one of the few
        // paths where `Done` can accompany a non-empty `state.open_windows`
        // — hosts check `open_windows` and present `ResolveInput::Skip`
        // to close it, exactly as for the phase-transition windows the
        // void `*_phase` drivers open.
        investigation_phase(state, events);
    }

    // Reaction windows open at the step boundary inside the handler
    // that queued them (see `drive_skill_test`), not at this outer
    // boundary — the Rules Reference clause "after… may be used
    // immediately after that triggering condition's impact upon the
    // game state has resolved" is mid-action, not post-action. Any
    // future action that queues a window outside the skill-test
    // driver must add its own boundary check; there's no fallback
    // here.

    outcome
}

/// Apply an [`EngineRecord`] to the state, pushing events.
pub fn apply_engine_record(
    state: &mut GameState,
    events: &mut Vec<Event>,
    record: &EngineRecord,
) -> EngineOutcome {
    match record {
        EngineRecord::DeckShuffled { investigator } => deck_shuffled(state, events, *investigator),
        EngineRecord::EncounterDeckShuffled => encounter_deck_shuffled(state, events),
        EngineRecord::EncounterCardRevealed { investigator } => {
            encounter_card_revealed(state, events, *investigator)
        }
    }
}

/// Handler for [`EngineRecord::DeckShuffled`].
///
/// Permutes the named investigator's player deck via the deterministic
/// RNG and emits [`Event::DeckShuffled`]. Empty decks are a silent
/// no-op (no event emitted) — there's nothing to shuffle.
fn deck_shuffled(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if !state.investigators.contains_key(&investigator) {
        return EngineOutcome::Rejected {
            reason: format!("DeckShuffled: investigator {investigator:?} is not in state").into(),
        };
    }
    shuffle_player_deck(state, events, investigator);
    EngineOutcome::Done
}

/// Handler for [`EngineRecord::EncounterDeckShuffled`].
///
/// Permutes the shared encounter deck via the deterministic RNG and
/// emits [`Event::EncounterDeckShuffled`] (when ≥ 2 cards). No
/// validation — the encounter deck is shared, so there's no
/// per-investigator existence check.
fn encounter_deck_shuffled(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    shuffle_encounter_deck(state, events);
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
fn encounter_card_revealed(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: no card registry installed".into(),
        };
    };

    let Some(code) = draw_encounter_top(state, events) else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter deck and discard both empty".into(),
        };
    };

    let Some(metadata) = (registry.metadata_for)(&code) else {
        return EngineOutcome::Rejected {
            reason: format!("EncounterCardRevealed: unknown card code: {code:?}").into(),
        };
    };
    resolve_encounter_card(state, events, investigator, code, metadata)
}

/// Shared post-draw resolution helper. Resolves the per-card 5-step
/// sub-sequence's steps 3 (Revelation) and 4 (enemy spawn) for an
/// already-drawn encounter card. Called by [`encounter_card_revealed`]
/// (the `EngineRecord::EncounterCardRevealed` path) and by
/// `mythos_draw_for` (Mythos 1.4 player-driven draws, lands in T11).
///
/// Body: emits [`Event::CardRevealed`], then dispatches on
/// `metadata.card_type` — treachery → run Revelation abilities →
/// push card to `encounter_discard`;
/// enemy → run Revelation abilities → call [`spawn_enemy`];
/// any other type → return `Rejected`.
///
/// **Mid-resolution caveat:** [`Event::CardRevealed`] emits before
/// Revelation runs (Before-timing reactions need that ordering,
/// per #126's design decision). The apply loop's `events.clear()`
/// on Rejected still wipes the event stream on rejection.
fn resolve_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    let card_type = metadata.card_type;

    // Emit BEFORE Revelation resolves — see caveat in encounter_card_revealed.
    events.push(Event::CardRevealed {
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
            let ctx = EvalContext::for_controller(investigator);
            for ability in abilities
                .iter()
                .filter(|a| a.trigger == Trigger::Revelation)
            {
                let outcome = apply_effect(state, events, &ability.effect, ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            state.encounter_discard.push(code);
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
            let ctx = EvalContext::for_controller(investigator);
            for ability in abilities
                .iter()
                .filter(|a| a.trigger == Trigger::Revelation)
            {
                let outcome = apply_effect(state, events, &ability.effect, ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            spawn_enemy(state, events, investigator, code, metadata)
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
#[allow(clippy::too_many_lines)]
fn spawn_enemy(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // 1. Resolve spawn location (validate-first).
    let location_id = match &metadata.spawn {
        Some(Spawn {
            location: SpawnLocation::Specific(loc_code),
        }) => match state
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
        None => match state
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

    // 2. Resolve engagement-on-spawn (validate-first). The co-located
    //    set is narrowed by the enemy's prey — every spawn uses
    //    `Prey::Default` (Task 2), so a 2+ set always ties and suspends
    //    for the lead investigator's `PickInvestigator` (option A).
    let candidates = cursor::active_investigators_at(state, location_id);

    // 3. Mint and place (mutate-second). The enemy is inserted unengaged;
    //    the `One` and (post-resume) `Tie` cases set `engaged_with` via
    //    `engage_enemy_with` so the `EnemyEngaged` event always pairs with
    //    the mutation.
    let enemy_id = EnemyId(state.next_enemy_id);
    state.next_enemy_id = state.next_enemy_id.saturating_add(1);

    let enemy = Enemy {
        id: enemy_id,
        name: metadata.name.clone(),
        fight: 1,
        evade: 1,
        max_health: metadata.health.unwrap_or(1),
        damage: 0,
        attack_damage: 0,
        attack_horror: 0,
        current_location: Some(location_id),
        exhausted: false,
        traits: metadata.traits.clone(),
        engaged_with: None,
        hunter: false,
        prey: crate::card_data::Prey::Default,
    };
    state.enemies.insert(enemy_id, enemy);

    match hunters::resolve_prey(state, crate::card_data::Prey::Default, &candidates) {
        hunters::PreyResolution::None => {
            events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: None,
            });
            EngineOutcome::Done
        }
        hunters::PreyResolution::One(target) => {
            events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: Some(target),
            });
            hunters::engage_enemy_with(state, events, enemy_id, target);
            EngineOutcome::Done
        }
        hunters::PreyResolution::Tie(tied) => {
            events.push(Event::EnemySpawned {
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
            state.spawn_engage_pending = Some(SpawnEngagePending {
                enemy: enemy_id,
                investigator_to_draw: investigator,
                candidates: tied.clone(),
                surge: metadata.surge,
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

/// Fisher-Yates shuffle of the named investigator's deck using the
/// shared deterministic RNG. Used by [`deck_shuffled`] and by
/// scenario setup (initial-hand draw).
///
/// Emits [`Event::DeckShuffled`] iff the deck had at least 2 cards
/// (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_player_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
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
        let j = state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    let inv = state.investigators.get_mut(&investigator).expect("checked");
    for (a, b) in swaps {
        inv.deck.swap(a, b);
    }
    events.push(Event::DeckShuffled { investigator });
}

/// Fisher-Yates shuffle of the shared encounter deck using the
/// shared deterministic RNG. Used by [`encounter_deck_shuffled`] and
/// by [`reshuffle_encounter_discard`].
///
/// Emits [`Event::EncounterDeckShuffled`] iff the deck had at least
/// 2 cards (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_encounter_deck(state: &mut GameState, events: &mut Vec<Event>) {
    let deck_len = state.encounter_deck.len();
    if deck_len < 2 {
        return;
    }
    // Mirror shuffle_player_deck's "collect swaps then apply" pattern:
    // RngState::next_index borrows &mut state.rng, which would conflict
    // with a &mut borrow on state.encounter_deck inline.
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    for (a, b) in swaps {
        state.encounter_deck.swap(a, b);
    }
    events.push(Event::EncounterDeckShuffled);
}

/// Drain `state.encounter_discard` into `state.encounter_deck` and
/// shuffle the resulting deck. Called by
/// [`draw_encounter_top`] when the deck runs empty.
///
/// Does NOT push an `EngineRecord::EncounterDeckShuffled` to the
/// action log — mid-handler reshuffles rely on RNG determinism for
/// replay rather than log entries, mirroring the existing
/// player-deck pattern. The `EngineRecord` variant is reserved for
/// explicit shuffle actions (future "shuffle X into the encounter
/// deck" effects).
pub(super) fn reshuffle_encounter_discard(state: &mut GameState, events: &mut Vec<Event>) {
    state
        .encounter_deck
        .extend(state.encounter_discard.drain(..));
    shuffle_encounter_deck(state, events);
}

/// Draw the top card of the encounter deck, transparently reshuffling
/// the discard back in if the deck is empty.
///
/// Returns `Some(code)` when a card was available (either from the
/// deck directly or after the reshuffle). Returns `None` when both
/// the deck and the discard are empty — callers decide how to
/// interpret this (#69's Mythos loop treats it as a scenario
/// condition rather than an engine error).
pub(super) fn draw_encounter_top(
    state: &mut GameState,
    events: &mut Vec<Event>,
) -> Option<CardCode> {
    if state.encounter_deck.is_empty() {
        if state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(state, events);
    }
    state.encounter_deck.pop_front()
}

/// Draw up to `count` cards from the named investigator's deck top
/// into their hand. Stops early (without panic) if the deck runs out
/// — this helper is just the structural move; reshuffle / horror
/// penalty logic for an empty deck lives in [`draw`].
///
/// Emits a single [`Event::CardsDrawn`] with the actually-drawn
/// count, even if that's zero. A zero-count draw is informative for
/// consumers tracking the attempt.
pub(super) fn draw_cards(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    count: u8,
) {
    let inv = state
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
    events.push(Event::CardsDrawn {
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
pub(super) fn grant_resources(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if amount == 0 {
        return;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .expect("grant_resources: caller guarantees investigator exists");
    inv.resources = inv.resources.saturating_add(amount);
    events.push(Event::ResourcesGained {
        investigator,
        amount,
    });
}

fn start_scenario(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    // The GameState constructor places the world in its initial shape;
    // this action is the explicit "session has begun" marker that lands
    // in the action log. Replaying it on an already-started state is a
    // bug, not a no-op — reject so callers notice rather than silently
    // double-emitting `ScenarioStarted`.
    if state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "StartScenario applied to a state that is already in progress".into(),
        };
    }
    // Round 1: scenario starts directly in Investigation phase —
    // Mythos is skipped entirely per Rules Reference p.24 "During
    // the first round of the game, skip the mythos phase." No
    // PhaseStarted(Mythos) / PhaseEnded(Mythos) fire — the phase
    // doesn't happen.
    state.round = 1;
    state.phase = Phase::Investigation;
    events.push(Event::ScenarioStarted);

    // For each investigator (sorted by id for determinism), shuffle
    // their deck and deal an initial hand of up to 5.
    let inv_ids: Vec<InvestigatorId> = state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        shuffle_player_deck(state, events, inv_id);
        draw_cards(state, events, inv_id, INITIAL_HAND_SIZE);
    }

    // Seed the mulligan cursor to the first Active investigator in
    // player order. Each investigator submits a single
    // `PlayerAction::Mulligan` in turn; the cursor advances after each
    // and reaches `None` once all have gone (see `apply_player_action`),
    // at which point setup ends. Other player actions are rejected while
    // the cursor is `Some`. An empty/all-eliminated `turn_order` seeds
    // `None` — the same degenerate no-op as the Mythos draw cursor.
    state.mulligan_pending = cursor::first_active_investigator(state);

    // Round-1 action seed: round 1 skips Mythos, so there's no Upkeep 4.2
    // to grant the first round's actions. Every Active investigator → ACTIONS_PER_TURN.
    reset_actions(state, events);

    // NOTE: the Investigation phase is NOT begun here. Setup has no
    // action windows (Rules Reference p.27); the phase begins after the
    // mulligan cursor reaches `None` — see the kickoff in apply_player_action.
    EngineOutcome::Done
}

fn end_turn(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: "EndTurn is only valid during the Investigation phase".into(),
        };
    }
    let Some(active_id) = state.active_investigator else {
        return EngineOutcome::Rejected {
            reason: "EndTurn requires an active investigator".into(),
        };
    };
    // The Some(active_investigator) invariant is paired with that ID
    // existing in the investigators map; a missing entry would be state
    // corruption, not a normal rejection. Surface it loudly rather than
    // hiding behind Rejected.
    let active = state.investigators.get_mut(&active_id).unwrap_or_else(|| {
        unreachable!(
            "active_investigator {active_id:?} is not in the investigators map; \
                 this is a state-corruption invariant violation"
        )
    });

    // Drain remaining actions and announce the turn ended.
    if active.actions_remaining != 0 {
        active.actions_remaining = 0;
        events.push(Event::ActionsRemainingChanged {
            investigator: active_id,
            new_count: 0,
        });
    }
    events.push(Event::TurnEnded {
        investigator: active_id,
    });

    // 2.2.2 decision: "return to 2.2" for the next investigator, or
    // proceed to 2.3. next_active_investigator_after skips eliminated
    // investigators (Rules Reference p.10) — the same shared helper the
    // Enemy phase uses.
    if let Some(next_id) = cursor::next_active_investigator_after(state, active_id) {
        begin_investigator_turn(state, events, next_id);
        EngineOutcome::Done
    } else {
        state.active_investigator = None;
        // 2.3 → Enemy. The cascade may suspend on a hunter-movement tie
        // (Enemy 3.2); propagate its outcome rather than swallowing it.
        investigation_phase_end(state, events)
    }
}

/// Entered by [`step_phase`] on any-to-Investigation transition, and by
/// the mulligan-completion site in [`apply_player_action`] for round 1.
/// Owns the `PhaseStarted(Investigation)` emit (Rules Reference p.24
/// step 2.1) and opens the post-2.1 player window. Rotation to the
/// first active investigator (step 2.2) runs in the
/// [`WindowKind::InvestigationBegins`] continuation via
/// [`begin_investigator_turn`], lead-first by default; explicit
/// player-pick within this window is deferred to #146.
///
/// The window auto-skips inline when nothing is Fast-eligible
/// ([`any_fast_play_eligible`] returns `false` — e.g. no Fast card in any
/// hand, which is always the case in unit tests with no card registry
/// installed), so single-investigator entry still lands the lead active
/// within the same `apply()` call.
fn investigation_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 2.1 Investigation phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });
    // PLAYER WINDOW (post-2.1). Rotation to the first investigator
    // (step 2.2) runs in this window's continuation
    // (`run_window_continuation` → `InvestigationBegins`), so the printed
    // order 2.1 → window → 2.2 holds. Auto-skips inline when nothing is
    // Fast-eligible, so single-investigator entry still lands the lead
    // active within the same apply() call.
    reaction_windows::open_fast_window(state, events, WindowKind::InvestigationBegins);
}

/// 2.2 Next investigator's turn begins. Rotates the active cursor to
/// `who` (the chosen/default investigator) and opens the post-2.2
/// player window. Called from the `InvestigationBegins` continuation
/// (first turn of the phase) and from `end_turn` (each subsequent turn,
/// the rules' "return to 2.2"). Step
/// 2.2.1 (the active investigator's actions) follows as player-driven
/// inputs while `InvestigatorTurnBegins` is the "previous player window."
///
/// `who` must be an `Active` investigator in `turn_order`; callers
/// resolve it via `first_active_investigator` / `next_active_investigator_after`.
pub(super) fn begin_investigator_turn(
    state: &mut GameState,
    events: &mut Vec<Event>,
    who: InvestigatorId,
) {
    rotate_to_active(state, events, who);
    reaction_windows::open_fast_window(state, events, WindowKind::InvestigatorTurnBegins);
}

/// 2.3 Investigation phase ends. Owns the `PhaseEnded(Investigation)`
/// emit — lifted out of `step_phase`, mirroring `mythos_phase_end` /
/// `enemy_phase_end` / `upkeep_phase_end` — then transitions to the
/// Enemy phase. Called only from `end_turn`'s terminal branch (the last
/// investigator has taken a turn this round).
fn investigation_phase_end(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    events.push(Event::PhaseEnded {
        phase: Phase::Investigation,
    });
    step_phase(state, events) // Investigation → Enemy; calls enemy_phase
}

/// Entered by [`step_phase`] on the Upkeep→Mythos transition. Lays
/// out the Rules Reference p.24 sub-steps as discrete named call
/// sites so the rule structure is grep-able and #73 / future-peril-PR
/// fills in TODO bodies without changing the driver shape.
fn mythos_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.1 Round begins. Mythos phase begins.
    //     Rules Reference p.24: "As this is the first framework event
    //     of the round, it [1.1] also formalizes the beginning of a new
    //     game round." The round-counter increment lives HERE (not in
    //     step_phase) so the rule's round-begin point has explicit
    //     driver ownership, mirroring PhaseStarted(Mythos). Round 1 is
    //     bypassed: start_scenario sets round = 1 directly (Mythos
    //     skipped). This is also the future home for a RoundStarted
    //     event when a consumer lands.
    state.round = state.round.saturating_add(1);
    events.push(Event::PhaseStarted {
        phase: Phase::Mythos,
    });

    // 1.2 Place 1 doom on the current agenda.
    place_doom_on_agenda(state, events);

    // 1.3 Check doom threshold.
    check_doom_threshold(state, events);

    // 1.4 Each investigator draws 1 encounter card.
    //     Seed the cursor; the actual draws are player-driven via
    //     PlayerAction::DrawEncounterCard (lands in T12). The
    //     dispatch handler advances the cursor after each chain.
    //     Per Rules Reference p.10 (Elimination), eliminated
    //     investigators (Killed, Insane, Resigned) do not draw
    //     encounter cards — skip to the first Active investigator.
    state.mythos_draw_pending = cursor::first_active_investigator(state);
    if state.mythos_draw_pending.is_none() {
        // No Active investigators to draw (turn_order is empty or all
        // investigators are eliminated). Open the post-1.4 window
        // immediately; open_fast_window's auto-skip path triggers
        // because nothing is eligible, runs the MythosAfterDraws
        // continuation (mythos_phase_end), which transitions to
        // Investigation. All in this same apply.
        reaction_windows::open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}

/// Mythos step 1.2 (Rules Reference p.24): "Take 1 doom from the token
/// pool, and place it on the current agenda card." No-op when no agenda
/// deck is modeled (tests/fixtures without an agenda).
fn place_doom_on_agenda(state: &mut GameState, _events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    state.agenda_doom = state.agenda_doom.saturating_add(1);
}

/// Mythos step 1.3 (Rules Reference p.24): compare doom in play with the
/// current agenda's threshold; if met, the agenda advances. We model
/// doom only on the agenda (no corpus card carries doom yet — summing
/// "doom on each other card in play" would add zero).
///
/// TODO(#73 follow-up): sum doom on other cards in play once a
/// doom-bearing card exists.
///
/// If the current agenda is terminal (carries a `resolution`), advancing
/// it ends the scenario: set the resolution latch instead of moving the
/// cursor. Otherwise emit [`Event::AgendaAdvanced`], reset doom, and make
/// the next agenda current.
fn check_doom_threshold(state: &mut GameState, events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    let agenda = &state.agenda_deck[state.agenda_index];
    if state.agenda_doom < agenda.doom_threshold {
        return;
    }
    match agenda.resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_agenda(state, events),
    }
}

/// Advance the agenda deck one step: emit [`Event::AgendaAdvanced`],
/// reset doom (Rules Reference p.24: "remove all doom from play"), and
/// move the cursor to the next agenda.
///
/// Only ever called for a *non-terminal* agenda (one whose `resolution`
/// is `None`). A non-terminal agenda must have a successor; reaching the
/// end of the deck without a resolution firing is malformed scenario
/// data (the final agenda must carry a `(→R#)` resolution point), so the
/// missing-successor case is `unreachable!()` — mirrors the surge-chain
/// malformation guards from #69.
fn advance_agenda(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.agenda_index;
    events.push(Event::AgendaAdvanced { from });
    state.agenda_doom = 0;
    state.agenda_index += 1;
    if state.agenda_index >= state.agenda_deck.len() {
        unreachable!(
            "advance_agenda: agenda {from} advanced past the end of the deck without a \
             resolution firing — a terminal agenda must carry a resolution point; this is \
             malformed scenario data"
        );
    }
}

/// The investigators who may contribute clues to advance the act, in the
/// deterministic spend order: the acting investigator first, then the rest
/// of `turn_order`. Shared by [`advance_act_action`]'s clue-sufficiency
/// check and [`spend_clues`] so the validation domain and the spend domain
/// can never diverge.
fn clue_contributors(state: &GameState, acting: InvestigatorId) -> Vec<InvestigatorId> {
    std::iter::once(acting)
        .chain(state.turn_order.iter().copied().filter(|id| *id != acting))
        .collect()
}

/// Handler for [`PlayerAction::AdvanceAct`] — a prototype clue-spend to
/// advance the current act (see the action's doc comment and the design
/// spec). Validate-first: reject outside the Investigation phase, when no
/// act deck is modeled, or when the group holds fewer clues than the
/// current act's `clue_threshold`. On success spend exactly the threshold
/// (acting investigator first, then the rest in `turn_order`) and either
/// set the resolution latch (terminal act) or emit [`Event::ActAdvanced`]
/// and advance the cursor.
fn advance_act_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.act_deck.is_empty() {
        return EngineOutcome::Rejected {
            reason: "AdvanceAct: no act deck is modeled for this scenario".into(),
        };
    }
    let threshold = state.act_deck[state.act_index].clue_threshold;
    let total_clues: u32 = clue_contributors(state, investigator)
        .into_iter()
        .filter_map(|id| state.investigators.get(&id))
        .map(|i| u32::from(i.clues))
        .sum();
    if total_clues < u32::from(threshold) {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
            )
            .into(),
        };
    }

    // All validations passed — mutate.
    spend_clues(state, investigator, threshold);
    match state.act_deck[state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_act(state, events),
    }
    EngineOutcome::Done
}

/// Spend `amount` clues from the group, deterministically: the acting
/// investigator's clues first, then the remaining investigators in
/// `turn_order`. Callers must have already validated the group holds at
/// least `amount` clues, so the spend always completes.
///
/// TODO(#153): let players choose who contributes when the group holds a
/// surplus (an `AwaitingInput` allocation prompt). The fixed order here is
/// outcome-equivalent single-player.
fn spend_clues(state: &mut GameState, acting: InvestigatorId, amount: u8) {
    let mut remaining = amount;
    for id in clue_contributors(state, acting) {
        if remaining == 0 {
            break;
        }
        if let Some(inv) = state.investigators.get_mut(&id) {
            let take = inv.clues.min(remaining);
            inv.clues -= take;
            remaining -= take;
        }
    }
    debug_assert_eq!(
        remaining, 0,
        "spend_clues called without enough clues in the group"
    );
}

/// Advance the act deck one step: emit [`Event::ActAdvanced`] and move the
/// cursor. Only called for a non-terminal act; the missing-successor case
/// is `unreachable!()` (a terminal act must carry a resolution point —
/// malformed scenario data otherwise). Mirrors [`advance_agenda`].
fn advance_act(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.act_index;
    events.push(Event::ActAdvanced { from });
    state.act_index += 1;
    if state.act_index >= state.act_deck.len() {
        unreachable!(
            "advance_act: act {from} advanced past the end of the deck without a resolution \
             firing — a terminal act must carry a resolution point; this is malformed \
             scenario data"
        );
    }
}

/// Set the scenario-resolution latch. First-writer-wins: a resolution
/// already latched this scenario is authoritative and a later request is
/// ignored. The `apply` hook (in `engine::mod`) observes the `None`→`Some`
/// transition to emit [`Event::ScenarioResolved`] and run the scenario
/// module's `apply_resolution` exactly once.
///
/// Call this only after a handler's validations pass: on a `Rejected`
/// outcome `apply` clears events but does not roll back `state`, so a
/// latch set on a doomed path would persist. All current callers latch
/// only on their success branches.
fn request_resolution(state: &mut GameState, resolution: crate::scenario::Resolution) {
    if state.resolution.is_none() {
        state.resolution = Some(resolution);
    }
}

/// Transition to the next phase. Dispatches into phase driver
/// functions when they exist (each driver owns its own
/// `PhaseStarted` emit). For phases without a driver, emits
/// `PhaseStarted` directly.
///
/// **`PhaseEnded` invariant:** `step_phase` emits **no** `PhaseEnded`
/// for any phase. Each phase's `*_end` helper owns its own boundary
/// emit: `mythos_phase_end` (step 1.5), `investigation_phase_end`
/// (step 2.3), `enemy_phase_end` (step 3.4), `upkeep_phase_end`
/// (step 4.6). `start_scenario`'s first-round-skip path bypasses the
/// entire Mythos phase — no `PhaseStarted(Mythos)` /
/// `PhaseEnded(Mythos)` events fire on round 1 — per Rules Reference
/// p.24 ("skip the mythos phase").
///
/// **Round-bump:** the round-counter increment now lives in
/// `mythos_phase` step 1.1 — the rules' "round begins" point —
/// rather than here. `step_phase` no longer touches `state.round`.
///
/// Returns the transition's [`EngineOutcome`]. Only the
/// Investigation→Enemy arm can return [`EngineOutcome::AwaitingInput`]
/// (a hunter-movement tie in [`enemy_phase`]); every other arm runs its
/// driver to completion and returns [`EngineOutcome::Done`]. The
/// Investigation→Enemy suspension is owned by
/// [`investigation_phase_end`], which propagates it up through
/// [`end_turn`].
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let from = state.phase;
    let to = from.next();

    state.phase = to;
    // The round-counter bump moves into mythos_phase (step 1.1).
    // step_phase no longer touches state.round.

    // Dispatch to phase driver if one exists; otherwise emit
    // PhaseStarted directly (for phases without a driver yet).
    match to {
        Phase::Mythos if from != Phase::Mythos => {
            mythos_phase(state, events);
            EngineOutcome::Done
        }
        Phase::Investigation if from != Phase::Investigation => {
            investigation_phase(state, events);
            EngineOutcome::Done
        }
        Phase::Enemy if from != Phase::Enemy => enemy_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => {
            upkeep_phase(state, events);
            EngineOutcome::Done
        }
        _ => unreachable!(
            "step_phase: from == to (from={from:?}, to={to:?}); Phase::next \
             never returns the same phase, so this branch is structurally \
             unreachable. If it ever fires, something has corrupted \
             state.phase between the read and the dispatch."
        ),
    }
}

/// Set `active_investigator` to `id`. Does NOT refresh actions —
/// actions are reset at Upkeep step 4.2 (`reset_actions`) for the whole
/// next round, and seeded for round 1 by `start_scenario`. By the time
/// an investigator becomes active, `actions_remaining` already holds
/// this round's allotment.
///
/// `id` must refer to an investigator in `state.investigators` (a
/// whole-program invariant for ids drawn from `turn_order`).
fn rotate_to_active(state: &mut GameState, _events: &mut Vec<Event>, id: InvestigatorId) {
    debug_assert!(
        state.investigators.contains_key(&id),
        "rotate_to_active: investigator {id:?} not in investigators (state corruption)"
    );
    state.active_investigator = Some(id);
}

/// Dispatch a [`PlayerAction::ResolveInput`].
///
/// Routes to the right resume handler based on which suspension is
/// outstanding: an open reaction window ([`resume_reaction_window`])
/// or the skill-test commit window ([`finish_skill_test`]). Rejects
/// when nothing is outstanding.
///
/// A reaction window on `state.open_windows` and `in_flight_skill_test`
/// may both be present simultaneously — that's the mid-skill-test
/// reaction case: the skill-test driver is parked at a step boundary
/// waiting for the reaction window to close before continuing. The
/// reaction window takes routing priority; once it closes,
/// [`close_reaction_window_at`] re-enters [`drive_skill_test`] to finish
/// the test.
///
/// # Pure-Fast window closing
///
/// A pure-Fast window (pushed by [`open_fast_window`], empty
/// `pending_triggers`) is **not** returned by [`GameState::top_reaction_window`]
/// because that helper filters out empty-`pending_triggers` windows.
/// When such a window is the only entry on the stack (no
/// reaction-driven window below it), `InputResponse::Skip` closes it
/// directly via [`close_reaction_window_at`] on the literal top-of-stack
/// index. This covers the `MythosAfterDraws` window after all Fast
/// plays have been made and the player is done.
fn resolve_input(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    // Hunter-movement suspension is its own mode; route it before the
    // reaction-window and skill-test checks, which are independent
    // suspension modes. (#128)
    debug_assert!(
        !(state.hunter_move_pending.is_some() && state.spawn_engage_pending.is_some()),
        "hunter movement and spawn engagement cannot both be pending: they arise in \
         different phases (Enemy 3.2 vs Mythos 1.4) and each blocks all other actions",
    );
    if state.hunter_move_pending.is_some() {
        return hunters::resume_hunter_choice(state, events, response);
    }

    // Engagement-on-spawn suspension (#128, option A) is a distinct mode
    // from hunter movement: its resume re-enters the Mythos draw chain.
    if state.spawn_engage_pending.is_some() {
        return hunters::resume_spawn_engage(state, events, response);
    }

    if state.top_reaction_window().is_some() {
        return reaction_windows::resume_reaction_window(state, events, response);
    }

    // Pure-Fast window path (Option B): no reaction-driven window is
    // pending, but a window (e.g. MythosAfterDraws) may still be on the
    // stack with empty pending_triggers. Skip is the only valid response
    // here — PickIndex / CommitCards reject below.
    if !state.open_windows.is_empty() {
        if matches!(response, InputResponse::Skip) {
            let idx = state.open_windows.len() - 1;
            return reaction_windows::close_reaction_window_at(state, events, idx);
        }
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a Fast-play window is open (no pending triggers); \
                 submit InputResponse::Skip to close it, got {response:?}",
            )
            .into(),
        };
    }

    if state.in_flight_skill_test.is_none() {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        };
    }
    match response {
        InputResponse::CommitCards { indices } => {
            skill_test::finish_skill_test(state, events, indices)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit window expects InputResponse::CommitCards, \
                 got {other:?}",
            )
            .into(),
        },
    }
}

/// 3.3 Seed the per-investigator attack cursor and open the first
/// attack window — or the final window directly if there is no Active
/// investigator. Called once hunter movement (step 3.2) completes:
/// from [`enemy_phase`] on the no-tie path, and from
/// [`resume_hunter_choice`] once all hunters resolve.
///
/// Seeds the cursor to the first Active investigator in `turn_order`.
/// Eliminated investigators (Killed / Insane / Resigned) are skipped per
/// Rules Reference p.10 (Elimination); [`cursor::first_active_investigator`] is
/// the shared helper used by Mythos 1.4 (#69) for the same semantics.
/// The loop body runs in [`run_window_continuation`]'s arms.
pub(super) fn enemy_attack_kickoff(state: &mut GameState, events: &mut Vec<Event>) {
    state.enemy_attack_pending = cursor::first_active_investigator(state);

    if state.enemy_attack_pending.is_some() {
        reaction_windows::open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
    } else {
        // No Active investigators (turn_order empty or all eliminated).
        // Skip straight to the final window — mirror of mythos_phase's
        // no-drawer path.
        reaction_windows::open_fast_window(
            state,
            events,
            WindowKind::AfterAllInvestigatorsAttacked,
        );
    }
}

/// Entered by [`step_phase`] on the Investigation→Enemy transition.
/// Owns the `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1),
/// runs hunter movement (step 3.2) via [`drive_hunter_moves`], then
/// kicks off the per-investigator attack loop (step 3.3) via
/// [`enemy_attack_kickoff`].
///
/// If hunter movement suspends on a lead-investigator tie, this returns
/// the [`EngineOutcome::AwaitingInput`] unchanged — the attack-loop
/// kickoff is deferred to [`resume_hunter_choice`], which runs it once
/// the last hunter resolves. Otherwise the kickoff runs inline here and
/// this returns [`EngineOutcome::Done`].
fn enemy_phase(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    // 3.1 Enemy phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Enemy,
    });

    // 3.2 Hunter enemies move. Park on a lead-investigator tie; the
    //     attack-loop kickoff then happens on resume.
    match hunters::drive_hunter_moves(state, events) {
        outcome @ EngineOutcome::AwaitingInput { .. } => return outcome,
        // drive_hunter_moves only ever returns Done or AwaitingInput, never Rejected.
        EngineOutcome::Rejected { reason } => {
            unreachable!("enemy_phase: hunter movement rejected unexpectedly: {reason}")
        }
        EngineOutcome::Done => {}
    }

    // 3.3 Kick off the per-investigator attack loop.
    enemy_attack_kickoff(state, events);
    EngineOutcome::Done
}

/// Called from [`run_window_continuation`]'s
/// [`WindowKind::AfterAllInvestigatorsAttacked`] arm. Emits step
/// 3.4's `PhaseEnded(Enemy)` marker, then transitions to Upkeep.
/// Exact analog of [`mythos_phase_end`] / [`upkeep_phase_end`].
pub(super) fn enemy_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 3.4 Enemy phase ends.
    events.push(Event::PhaseEnded {
        phase: Phase::Enemy,
    });
    // Enemy → Upkeep; calls upkeep_phase. Only the Investigation→Enemy
    // transition can suspend (hunter movement), so this never does.
    let outcome = step_phase(state, events);
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "unexpected suspension in Enemy→Upkeep transition"
    );
}

/// Reshuffle the discard pile back into the deck for the named
/// investigator. Used by [`draw`] when the deck runs empty. Drains
/// `discard` into `deck`, then calls [`shuffle_player_deck`] (which
/// emits [`Event::DeckShuffled`] when ≥ 2 cards land in the deck).
fn reshuffle_discard_into_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
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
    shuffle_player_deck(state, events, investigator);
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
fn draw_one_with_deckout(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
        .investigators
        .get(&investigator)
        .expect("draw_one_with_deckout: caller guarantees investigator exists");
    let deck_empty = inv.deck.is_empty();
    let discard_empty = inv.discard.is_empty();
    if deck_empty {
        if !discard_empty {
            reshuffle_discard_into_deck(state, events, investigator);
        }
        // After the (possibly no-op) reshuffle, attempt the draw.
        // draw_cards handles a still-empty deck by emitting
        // CardsDrawn { count: 0 } without moving cards.
        draw_cards(state, events, investigator, 1);
        // Horror penalty fires on any "would-draw-from-empty-deck"
        // (the reshuffle did happen if discard was non-empty; if it
        // was also empty, the rules don't strictly require horror
        // but we apply it as the safer reading).
        elimination::take_horror(state, events, investigator, 1);
    } else {
        draw_cards(state, events, investigator, 1);
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
fn draw(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
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
    actions::spend_one_action(state, events, investigator);
    draw_one_with_deckout(state, events, investigator);
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
fn mulligan(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    indices_to_redraw: &[u8],
) -> EngineOutcome {
    // One check subsumes the three old ones: the cursor only ever holds
    // an Active `turn_order` id, so a mismatch covers setup-over (`None`),
    // wrong-player / too-early, and already-went (cursor moved past you).
    if state.mulligan_pending != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Mulligan: it is not {investigator:?}'s turn to mulligan \
                 (pending: {:?})",
                state.mulligan_pending,
            )
            .into(),
        };
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
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
    let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
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
        shuffle_player_deck(state, events, investigator);
        draw_cards(state, events, investigator, redrawn_count);
    }
    events.push(Event::MulliganPerformed {
        investigator,
        redrawn_count,
    });
    // Advance to the next Active investigator in player order (or `None`
    // when this was the last). The completion check in
    // `apply_player_action` keys off `None` to end setup.
    state.mulligan_pending = cursor::next_active_investigator_after(state, investigator);
    EngineOutcome::Done
}

/// Internal helper: where a played card lands after on-play effects
/// resolve. Mirrors the Arkham rule that assets stay in play while
/// events resolve and go to the discard.
#[derive(Debug)]
pub(super) enum PlayDestination {
    /// Card stays in play (asset).
    InPlay,
    /// Card moves to the discard after on-play effects resolve (event).
    Discard,
}

/// Resolve the card's destination + abilities via the registry, or
/// produce the appropriate rejection.
///
/// Split out so [`play_card`] stays under the function-size lint —
/// and because the registry-side validations are conceptually
/// separate from the state-side prefix.
pub(super) fn resolve_play_target(
    code: &CardCode,
) -> Result<(PlayDestination, Vec<crate::dsl::Ability>, bool, CardType), EngineOutcome> {
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
    let is_fast = metadata.is_fast;
    let card_type = metadata.card_type;
    let destination = match card_type {
        CardType::Asset => PlayDestination::InPlay,
        CardType::Event => PlayDestination::Discard,
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

/// Validated payload returned by [`check_play_card`] on success.
/// Carries the data `play_card`'s mutation step needs without
/// re-running the validation.
///
/// `is_fast` is consumed by [`any_fast_play_eligible`]; `card_type`
/// is currently destructured with `_` in `play_card` but kept for
/// future consumers (e.g. reaction-window dispatch).
///
/// `#[allow(dead_code)]` covers `card_type` (not yet read outside
/// validation) and suppresses the rustc `dead_code` lint on struct fields
/// that are only read by a `pub(super)` function not yet wired up.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) struct PlayCheckResult {
    pub destination: PlayDestination,
    pub abilities: Vec<crate::dsl::Ability>,
    pub is_fast: bool,
    pub card_type: CardType,
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
/// # State-mutation contract caveat
///
/// For the Phase-3-scoped Core cards the on-play effects in scope
/// (`DiscoverClue`, `GainResources`) can't reject after the standard
/// validation prefix passes. If a future on-play effect can reject
/// mid-resolution, the partial mutation between [`Event::CardPlayed`]
/// and the destination move violates the engine's "no state change on
/// rejection" contract. The apply loop's belt-and-suspenders
/// `events.clear()` still clears the event stream on a rejected
/// outcome; the state-rollback hardening is out of scope here.
fn play_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    hand_index: u8,
) -> EngineOutcome {
    let PlayCheckResult {
        destination,
        abilities,
        is_fast: _,
        card_type: _,
    } = match reaction_windows::check_play_card(state, investigator, hand_index) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    // The code is re-read from state here so we don't pass it through
    // the result (avoiding the lifetime question). The validator already
    // confirmed the hand_index is in bounds and the investigator exists.
    let idx = usize::from(hand_index);
    let code: CardCode = state
        .investigators
        .get(&investigator)
        .expect("checked in validator")
        .hand[idx]
        .clone();

    // Mutate.
    events.push(Event::CardPlayed {
        investigator,
        code: code.clone(),
    });
    let ctx = EvalContext::for_controller(investigator);
    for ability in abilities.iter().filter(|a| a.trigger == Trigger::OnPlay) {
        let outcome = apply_effect(state, events, &ability.effect, ctx);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }
    }
    match destination {
        PlayDestination::InPlay => {
            let instance_id = CardInstanceId(state.next_card_instance_id);
            state.next_card_instance_id = state.next_card_instance_id.saturating_add(1);
            let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
            let card = inv_mut.hand.remove(idx);
            inv_mut
                .cards_in_play
                .push(CardInPlay::enter_play(card, instance_id));
        }
        PlayDestination::Discard => {
            let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
            let card = inv_mut.hand.remove(idx);
            inv_mut.discard.push(card.clone());
            events.push(Event::CardDiscarded {
                investigator,
                code: card,
                from: Zone::Hand,
            });
        }
    }
    EngineOutcome::Done
}

/// Validated payload returned by [`check_activate_ability`] on success.
/// Carries the data `activate_ability`'s mutation step needs without
/// re-running the validation.
#[derive(Debug)]
#[allow(dead_code)] // Fields consumed by any_fast_play_eligible in T05.
pub(super) struct ActivateCheckResult {
    /// Position of the source card in the investigator's `cards_in_play`.
    pub in_play_pos: usize,
    /// The card code of the source card.
    pub source_code: CardCode,
    /// Action cost from the ability's `Trigger::Activated`.
    pub action_cost: u8,
    /// Payment costs (beyond the action cost).
    pub costs: Vec<crate::dsl::Cost>,
    /// The effect to dispatch after paying costs.
    pub effect: crate::dsl::Effect,
    /// Whether the source card was exhausted at validation time —
    /// load-bearing for activated abilities whose payment includes
    /// `Cost::Exhaust`.
    pub source_exhausted: bool,
}

/// Called after the post-1.4 window closes. Emits 1.5's
/// `PhaseEnded(Mythos)` marker, then transitions to Investigation.
/// Rotation is owned by `investigation_phase` (step 2.2), not by
/// `mythos_phase_end`. Invoked from `close_reaction_window_at`'s
/// kind-aware tail when a `MythosAfterDraws` window pops, and from
/// `open_fast_window`'s auto-skip path inline.
pub(super) fn mythos_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.5 Mythos phase ends.
    //     The PhaseEnded(Mythos) emit lives HERE rather than in
    //     step_phase so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     mythos_phase. Rules Reference p.24: "This step formalizes
    //     the end of the mythos phase."
    events.push(Event::PhaseEnded {
        phase: Phase::Mythos,
    });
    // Mythos → Investigation; calls investigation_phase. Only the
    // Investigation→Enemy transition can suspend (hunter movement), so
    // this cascade always completes.
    let outcome = step_phase(state, events);
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "unexpected suspension in Mythos→Investigation transition"
    );
}

/// Entered by [`step_phase`] on the Enemy→Upkeep transition. Owns the
/// `PhaseStarted(Upkeep)` emit (step 4.1) and opens the post-4.1 player
/// window. Steps 4.2 onward run as the window's continuation
/// ([`upkeep_resume`]). Mirror of [`mythos_phase`], inverted: Mythos's
/// window sits at the END, so its driver runs content then opens;
/// Upkeep's sits at the START, so the driver opens immediately and the
/// content is the continuation.
fn upkeep_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.1 Upkeep phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Upkeep,
    });
    // PLAYER WINDOW (post-4.1). Auto-skips inline (running upkeep_resume
    // via run_window_continuation) when nothing is Fast-eligible.
    reaction_windows::open_fast_window(state, events, WindowKind::UpkeepBegins);
}

/// The post-4.1 window continuation. Steps 4.2–4.4 run inline as named
/// call sites; step 4.5 is the [`check_hand_size`] stub (TODO #111).
/// Then hands to [`upkeep_phase_end`] for 4.6 + transition.
pub(super) fn upkeep_resume(state: &mut GameState, events: &mut Vec<Event>) {
    reset_actions(state, events); // 4.2
    ready_exhausted_cards(state, events); // 4.3
    upkeep_draw_and_resource(state, events); // 4.4
    check_hand_size(state, events); // 4.5 (TODO #111)
    upkeep_phase_end(state, events); // 4.6 + transition
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit, then transitions to
/// Mythos. Exact analog of [`mythos_phase_end`]. `step_phase` emits no
/// `PhaseEnded` itself — every phase's `*_end` helper owns its own.
fn upkeep_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.6 Upkeep phase ends. Round ends.
    events.push(Event::PhaseEnded {
        phase: Phase::Upkeep,
    });
    // Upkeep → Mythos; calls mythos_phase. Only the Investigation→Enemy
    // transition can suspend (hunter movement), so this never does.
    let outcome = step_phase(state, events);
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "unexpected suspension in Upkeep→Mythos transition"
    );
}

/// 4.3 Ready exhausted cards. Rules Reference p.25: "Simultaneously
/// ready each exhausted card." "Each exhausted card" is every exhausted
/// card in play regardless of controller — investigator in-play cards
/// AND enemies. Simultaneous, so iteration order is immaterial; we
/// iterate deterministically (investigator id then in-play order; then
/// enemy id) for reproducible event streams. Already-ready cards emit
/// nothing.
///
/// After readying, each enemy that became ready while unengaged and
/// co-located with an investigator engages it via [`reengage_at_location`]
/// (Rules Reference p.10: "if an exhausted enemy at the same location as an
/// investigator becomes ready, it engages as soon as it is readied").
fn ready_exhausted_cards(state: &mut GameState, events: &mut Vec<Event>) {
    let inv_ids: Vec<InvestigatorId> = state.investigators.keys().copied().collect();
    for id in inv_ids {
        let inv = state.investigators.get_mut(&id).expect("id from keys");
        for card in &mut inv.cards_in_play {
            if card.exhausted {
                card.exhausted = false;
                events.push(Event::CardReadied {
                    investigator: id,
                    instance_id: card.instance_id,
                    code: card.code.clone(),
                });
            }
        }
    }
    let enemy_ids: Vec<EnemyId> = state.enemies.keys().copied().collect();
    let mut newly_readied: Vec<EnemyId> = Vec::new();
    for eid in enemy_ids {
        let enemy = state.enemies.get_mut(&eid).expect("id from keys");
        if enemy.exhausted {
            enemy.exhausted = false;
            events.push(Event::EnemyReadied { enemy: eid });
            newly_readied.push(eid);
        }
    }
    // RR p.10: "if an exhausted enemy at the same location as an investigator
    // becomes ready, it engages as soon as it is readied." Runs after the
    // (simultaneous, RR p.25) readying pass. Only newly-readied enemies are
    // checked ("becomes ready"), and only those still unengaged —
    // reengage_at_location's precondition is engaged_with == None, so an enemy
    // that readied while still engaged keeps its existing engagement.
    // newly_readied is in ascending EnemyId order (BTreeMap key order).
    for eid in newly_readied {
        if state.enemies[&eid].engaged_with.is_none() {
            hunters::reengage_at_location(state, events, eid);
        }
    }
}

/// 4.5 Each investigator checks hand size.
fn check_hand_size(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#111): in player order, each investigator with more than 8
    //   cards in hand discards down to 8 (Rules Reference p.25 step 4.5).
    //   Needs an AwaitingInput producer for the discard choice. The call
    //   site exists so the rule step is grep-able and #111 plugs in here
    //   without changing the driver shape.
}

/// 4.2 Reset actions. Rules Reference p.25: "Flip each investigator's
/// mini card back to its colored side. This indicates that the
/// investigator's actions have been reset for his or her next turn."
///
/// The canonical action-refresh site. Sets `actions_remaining` to
/// `ACTIONS_PER_TURN` for each Active investigator and emits
/// `ActionsRemainingChanged` when the value changes. `rotate_to_active`
/// no longer refreshes (step 2.2 is just "the turn begins");
/// `start_scenario` seeds round 1. Eliminated investigators are skipped
/// (Rules Reference p.10).
fn reset_actions(state: &mut GameState, events: &mut Vec<Event>) {
    for id in cursor::active_investigators_in_turn_order(state) {
        let inv = state
            .investigators
            .get_mut(&id)
            .expect("id from active_investigators_in_turn_order");
        if inv.actions_remaining != ACTIONS_PER_TURN {
            inv.actions_remaining = ACTIONS_PER_TURN;
            events.push(Event::ActionsRemainingChanged {
                investigator: id,
                new_count: ACTIONS_PER_TURN,
            });
        }
    }
}

/// 4.4 Each investigator draws 1 card and gains 1 resource. Rules
/// Reference p.25: "In player order, each investigator draws 1 card.
/// Once those cards have been drawn, each investigator gains 1
/// resource." Two passes to honor that ordering: all draws first, then
/// all resource gains.
fn upkeep_draw_and_resource(state: &mut GameState, events: &mut Vec<Event>) {
    let ids = cursor::active_investigators_in_turn_order(state);
    for &id in &ids {
        draw_one_with_deckout(state, events, id);
    }
    for &id in &ids {
        grant_resources(state, events, id, 1);
    }
}

/// Handler for [`PlayerAction::ActivateAbility`].
///
/// Validates the named card instance, the indexed ability's trigger,
/// and every cost-payability precondition. On success, pays every cost
/// (emitting cost events per primitive), emits [`Event::AbilityActivated`],
/// and dispatches the ability's effect through the DSL evaluator.
///
/// # Timing gate
///
/// The gate branches on `action_cost` from `Trigger::Activated`:
///
/// - **Action-cost abilities** (`action_cost > 0`): require Investigation
///   phase + active investigator + sufficient actions remaining. These consume
///   one of the investigator's limited per-turn actions.
/// - **Fast abilities** (`action_cost == 0`): per the Rules Reference, "Fast
///   abilities may be used at any player window." This handler permits them
///   when either (a) the acting investigator is the active investigator during
///   the Investigation phase, or (b) an open window's `fast_actors` scope
///   permits the acting investigator. The `open_windows` stack is pushed by
///   callers (scenario/server) when a player window opens.
///
/// # Cost coverage
///
/// - [`Cost::Resources`](crate::dsl::Cost::Resources): validates
///   wallet, deducts on payment, emits [`Event::ResourcesPaid`].
/// - [`Cost::Exhaust`](crate::dsl::Cost::Exhaust): validates source
///   not already exhausted, flips `cards_in_play[i].exhausted`,
///   emits [`Event::CardExhausted`].
/// - [`Cost::DiscardCardFromHand`](crate::dsl::Cost::DiscardCardFromHand):
///   rejects with a TODO — target-card selection needs an engine
///   `AwaitingInput` producer + `ResolveInput` dispatch. No card on
///   the roadmap uses this cost yet, so the consumer hasn't landed.
///   Test-side seam is [`ChoiceResolver`](crate::test_support::ChoiceResolver).
///
/// # State-mutation contract
///
/// Same caveat as `play_card`: costs are paid and `AbilityActivated`
/// is emitted before `apply_effect` runs, so a mid-resolution
/// rejection inside the effect leaves the costs paid. The apply
/// loop's belt-and-suspenders `events.clear()` still wipes the event
/// stream on rejection. Phase-3 in-scope effects (`GainResources`,
/// `DiscoverClue`, `Seq` of those, future `Modify`/`ThisSkillTest`
/// push) can't reject mid-flight once the standard prefix passes.
fn activate_ability(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    let ActivateCheckResult {
        in_play_pos,
        source_code,
        action_cost,
        costs,
        effect,
        source_exhausted: _,
    } = match reaction_windows::check_activate_ability(
        state,
        investigator,
        instance_id,
        ability_index,
    ) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // Mutate.
    pay_activation_costs(
        state,
        events,
        investigator,
        instance_id,
        in_play_pos,
        &source_code,
        action_cost,
        &costs,
    );
    events.push(Event::AbilityActivated {
        investigator,
        instance_id,
        code: source_code,
        ability_index,
    });

    let ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    apply_effect(state, events, &effect, ctx)
}

/// Pay the action cost and every payment cost of an activated
/// ability. Mutates state in place and pushes the matching events.
/// Caller has already validated that every cost is payable.
#[allow(clippy::too_many_arguments)]
fn pay_activation_costs(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    in_play_pos: usize,
    source_code: &CardCode,
    action_cost: u8,
    costs: &[Cost],
) {
    let inv_mut = state
        .investigators
        .get_mut(&investigator)
        .expect("validated above");
    if action_cost > 0 {
        inv_mut.actions_remaining = inv_mut.actions_remaining.saturating_sub(action_cost);
        events.push(Event::ActionsRemainingChanged {
            investigator,
            new_count: inv_mut.actions_remaining,
        });
    }
    for cost in costs {
        match cost {
            Cost::Resources(n) => {
                inv_mut.resources = inv_mut.resources.saturating_sub(*n);
                events.push(Event::ResourcesPaid {
                    investigator,
                    amount: *n,
                });
            }
            Cost::Exhaust => {
                inv_mut.cards_in_play[in_play_pos].exhausted = true;
                events.push(Event::CardExhausted {
                    investigator,
                    instance_id,
                    code: source_code.clone(),
                });
            }
            Cost::DiscardCardFromHand => {
                unreachable!("DiscardCardFromHand rejected earlier in check_cost_payable")
            }
        }
    }
}

/// Resolve the activated ability at `(code, ability_index)` from the
/// installed [`card_registry`], returning its `(action_cost, costs,
/// effect)` triple or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
pub(super) fn resolve_activated_ability(
    code: &CardCode,
    ability_index: u8,
) -> Result<(u8, Vec<Cost>, crate::dsl::Effect), EngineOutcome> {
    let Some(registry) = card_registry::current() else {
        return Err(EngineOutcome::Rejected {
            reason: "ActivateAbility: no card registry installed; engine cannot resolve abilities."
                .into(),
        });
    };
    let Some(abilities) = (registry.abilities_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("ActivateAbility: card {code} has no effect implementation").into(),
        });
    };
    let idx = usize::from(ability_index);
    let Some(ability) = abilities.get(idx) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability_index {ability_index} out of bounds for {code} \
                 (has {} abilities)",
                abilities.len(),
            )
            .into(),
        });
    };
    let Trigger::Activated { action_cost } = ability.trigger else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability {ability_index} on {code} is not an Activated \
                 trigger (got {:?})",
                ability.trigger,
            )
            .into(),
        });
    };
    Ok((action_cost, ability.costs.clone(), ability.effect.clone()))
}

/// Validate a single [`Cost`] is currently payable against `inv` /
/// `source_exhausted`. Returns the reject reason on failure. Does
/// NOT mutate; the caller does the actual deduction after all costs
/// are checked.
pub(super) fn check_cost_payable(
    cost: &Cost,
    inv: &Investigator,
    source_exhausted: bool,
) -> Result<(), String> {
    match cost {
        Cost::Resources(n) => {
            if inv.resources < *n {
                return Err(format!(
                    "ActivateAbility: needs {n} resources; investigator has {}",
                    inv.resources,
                ));
            }
            Ok(())
        }
        Cost::Exhaust => {
            if source_exhausted {
                return Err(
                    "ActivateAbility: source card is already exhausted; Exhaust cost \
                     cannot be paid"
                        .to_string(),
                );
            }
            Ok(())
        }
        Cost::DiscardCardFromHand => Err(
            "TODO: Cost::DiscardCardFromHand requires AwaitingInput + ResolveInput \
             dispatch; no card uses this cost yet so the engine consumer hasn't landed."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod encounter_card_revealed_tests {
    use super::*;
    use crate::state::CardCode;
    use crate::test_support::{test_investigator, TestGame};

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
        let mut state = TestGame::new()
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

        let outcome = apply_engine_record(
            &mut state,
            &mut events,
            &EngineRecord::EncounterCardRevealed {
                investigator: InvestigatorId(1),
            },
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
    use crate::test_support::TestGame;

    #[test]
    fn shuffle_encounter_deck_emits_event_when_two_or_more_cards() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(42);
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();
        shuffle_encounter_deck(&mut state, &mut events);

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
            let mut state = TestGame::new().build();
            for i in 0..n {
                state.encounter_deck.push_back(CardCode(format!("c{i}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
            assert!(events.is_empty(), "expected no event for n={n} deck");
        }
    }

    #[test]
    fn reshuffle_encounter_discard_moves_discard_into_deck_and_shuffles() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(7);
        for i in 0..5 {
            state.encounter_discard.push(CardCode(format!("d{i}")));
        }

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

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
        let mut state = TestGame::new().build();
        state.encounter_discard.push(CardCode("solo".into()));

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

        assert!(state.encounter_discard.is_empty());
        assert_eq!(state.encounter_deck.len(), 1);
        assert!(events.is_empty(), "1-card shuffle emits no event");
    }

    #[test]
    fn draw_encounter_top_drains_deck_then_returns_none() {
        let mut state = TestGame::new().build();
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();

        assert_eq!(
            draw_encounter_top(&mut state, &mut events),
            Some(CardCode("a".into()))
        );
        assert_eq!(
            draw_encounter_top(&mut state, &mut events),
            Some(CardCode("b".into()))
        );
        assert_eq!(
            draw_encounter_top(&mut state, &mut events),
            Some(CardCode("c".into()))
        );
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        assert!(
            events.is_empty(),
            "no events for any draw — discard is always empty, no reshuffle is triggered"
        );
    }

    #[test]
    fn draw_encounter_top_reshuffles_discard_on_empty_deck() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(13);
        state.encounter_discard.push(CardCode("x".into()));
        state.encounter_discard.push(CardCode("y".into()));
        state.encounter_discard.push(CardCode("z".into()));

        let mut events = Vec::new();
        let drawn = draw_encounter_top(&mut state, &mut events);

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
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        assert!(events.is_empty(), "no events on empty-on-both");
    }

    #[test]
    fn engine_record_encounter_deck_shuffled_drives_shuffle() {
        use crate::action::{Action, EngineRecord};
        use crate::engine::apply;

        let mut state = TestGame::new().build();
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
            let mut state = TestGame::new().build();
            state.rng = RngState::new(seed);
            for i in 0..10 {
                state.encounter_deck.push_back(CardCode(format!("c{i:02}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
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
    use crate::state::{CardCode, LocationId};
    use crate::test_support::{test_investigator, test_location, TestGame};
    use crate::{assert_event, assert_event_sequence, assert_no_event};
    use card_dsl::card_data::{CardMetadata, CardType, Class, SkillIcons, Spawn, SpawnLocation};

    fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
        CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            class: Class::Mythos,
            card_type: CardType::Enemy,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: Some(1),
            sanity: None,
            deck_limit: 1,
            quantity: 1,
            pack_code: "_synth".into(),
            position: 1,
            is_fast: false,
            spawn,
            surge: false,
            peril: false,
        }
    }

    #[test]
    fn spawn_at_specific_location_with_one_investigator_engages_them() {
        let mut loc = test_location(10, "Synth Loc");
        loc.code = CardCode("_synth_loc".into());
        let mut state = TestGame::new()
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
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        // Investigator 1 is NOT at location 10 (current_location is None).

        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_synth_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_nonexistent_loc".into()),
        }));
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
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
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        // Investigator has no current_location.
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_phase(Phase::Mythos)
            .with_location(loc)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .build();
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
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
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
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
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert!(state.spawn_engage_pending.is_some());

        // Investigator 3 is not among the co-located candidates.
        let outcome = hunters::resume_spawn_engage(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
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
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        let _ = spawn_enemy(
            &mut state,
            &mut events,
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
mod investigation_phase_tests {
    use super::*;
    use crate::event::Event;
    use crate::state::{InvestigatorId, Phase, Status, WindowKind};
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn mulligan_completion_kicks_off_investigation_phase() {
        // After the last investigator mulligans, setup ends and the
        // Investigation phase begins (Rules Reference p.27: no action
        // windows during setup; the game begins after mulligans).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;
        state.mulligan_pending = Some(InvestigatorId(1));

        let mut events = Vec::new();
        let outcome = apply_player_action(
            &mut state,
            &mut events,
            &PlayerAction::Mulligan {
                investigator: InvestigatorId(1),
                indices_to_redraw: vec![],
            },
        );

        assert!(matches!(outcome, EngineOutcome::Done));
        assert_eq!(
            state.mulligan_pending, None,
            "mulligan cursor clears once every investigator has mulliganed"
        );
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(1)),
            "Investigation phase kicks off and rotates to the lead after mulligan completes"
        );
        // PhaseStarted(Investigation) fires at mulligan completion (not
        // during StartScenario) AND precedes the post-2.1 window — the
        // printed 2.1 → window order.
        let phase_started = events.iter().position(|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )
        });
        let window_opened = events.iter().position(|e| {
            matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::InvestigationBegins
                }
            )
        });
        let phase_started = phase_started.expect("PhaseStarted(Investigation) must fire");
        let window_opened =
            window_opened.expect("WindowOpened(InvestigationBegins) must fire at phase start");
        assert!(
            phase_started < window_opened,
            "PhaseStarted (2.1) must precede the post-2.1 InvestigationBegins window"
        );
    }

    #[test]
    fn investigation_phase_emits_phase_started_and_rotates_to_lead() {
        // Two investigators; investigation_phase should emit
        // PhaseStarted(Investigation), open the post-2.1 InvestigationBegins
        // window (which auto-skips in tests — no card registry installed),
        // and then rotate to the first investigator in turn_order
        // (Rules Reference p.24 step 2.1 → window → step 2.2 lead-first).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(1)),
            "investigation_phase must rotate to the lead (first in turn_order)"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "PhaseStarted(Investigation) must be emitted"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ActionsRemainingChanged { .. })),
            "rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::InvestigationBegins
                }
            )),
            "investigation_phase opens the post-2.1 InvestigationBegins window"
        );
    }

    #[test]
    fn investigation_phase_with_empty_turn_order_parks() {
        // Degenerate (cannot occur in real gameplay): no investigators.
        // The InvestigationBegins continuation finds no active
        // investigator and PARKS — active stays None, no PhaseEnded, no
        // advance. Locks in the cascade-breaker behavior (see spec
        // "All-eliminated / no-active-investigator handling").
        //
        // Phase starts as Investigation (matching the real call-site
        // shape: step_phase sets state.phase before calling
        // investigation_phase).
        let mut state = TestGame::default().with_phase(Phase::Investigation).build();
        state.turn_order.clear();
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(
            state.active_investigator, None,
            "no investigator to rotate to"
        );
        assert_eq!(state.phase, Phase::Investigation, "phase must not advance");
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "parking must NOT end the phase (auto-advancing would loop the round)"
        );
    }

    #[test]
    fn investigation_phase_skips_defeated_lead_and_picks_first_active() {
        // Investigator 1 (lead) is Killed; investigator 2 is Active.
        // investigation_phase must skip Id(1) and rotate to Id(2).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(2)),
            "investigation_phase must skip the Killed lead and rotate to the first Active investigator"
        );
    }

    #[test]
    fn end_turn_for_last_investigator_ends_phase_and_steps_to_enemy() {
        // Single investigator ends their turn: TurnEnded (2.2.2), then
        // PhaseEnded(Investigation) (2.3) from investigation_phase_end,
        // then the cascade enters the Enemy phase.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);

        assert!(matches!(outcome, EngineOutcome::Done));
        assert!(
            events.iter().any(|e| matches!(e, Event::TurnEnded { investigator } if *investigator == InvestigatorId(1))),
            "step 2.2.2 emits TurnEnded"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "step 2.3 emits PhaseEnded(Investigation) via investigation_phase_end"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    Event::PhaseEnded {
                        phase: Phase::Investigation
                    }
                ))
                .count(),
            1,
            "exactly one PhaseEnded(Investigation) — step_phase must not also emit it"
        );
        assert_ne!(
            state.phase,
            Phase::Investigation,
            "phase advanced past Investigation"
        );
    }

    #[test]
    fn end_turn_rotates_to_next_active_and_opens_turn_window() {
        // Two investigators: ending #1's turn returns to 2.2 for #2 and
        // opens the InvestigatorTurnBegins window for them.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];

        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);

        assert!(matches!(outcome, EngineOutcome::Done));
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(2)),
            "rotates to the next active investigator (return to 2.2)"
        );
        assert_eq!(
            state.phase,
            Phase::Investigation,
            "phase does not end mid-round"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "phase must not end while an investigator is still to take a turn"
        );
    }

    #[test]
    fn step_phase_emits_no_phase_ended() {
        // step_phase no longer emits PhaseEnded for any phase — each
        // phase's *_end helper owns it. Direct Investigation→Enemy step:
        // step_phase must NOT emit PhaseEnded(Investigation); the
        // downstream cascade may emit PhaseEnded for Enemy/Upkeep via
        // their own *_end helpers, but that's correct and expected.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        step_phase(&mut state, &mut events);

        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::PhaseEnded { phase: Phase::Investigation }))
                .count(),
            0,
            "step_phase must emit no PhaseEnded(Investigation) — investigation_phase_end owns it. events = {events:?}"
        );
    }

    #[test]
    fn investigation_entry_emits_phase_started_then_windows_then_lead_active() {
        // Round ≥2 entry via step_phase (Mythos→Investigation) auto-skips
        // both windows (no registry → nothing Fast-eligible) and lands
        // the lead active, with no PhaseEnded yet.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Mythos→Investigation

        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(InvestigatorId(1)));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::WindowOpened {
                kind: WindowKind::InvestigationBegins
            }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::WindowOpened {
                kind: WindowKind::InvestigatorTurnBegins
            }
        )));
        assert!(!events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )));
    }
}

/// Handler for [`PlayerAction::DrawEncounterCard`]. Validates phase
/// + cursor; delegates to [`mythos_draw_for`] on success.
pub(super) fn draw_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Mythos {
        return EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: only valid during Mythos phase, got {:?}",
                state.phase,
            )
            .into(),
        };
    }
    match state.mythos_draw_pending {
        None => EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
        },
        Some(expected) if expected != investigator => EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: out of order; expected {expected:?}, got {investigator:?}",
            )
            .into(),
        },
        Some(_) => mythos_draw_for(state, events, investigator),
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
fn mythos_draw_for(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Fresh chain: count starts at 0 and the loop draws at least one
    // card (`draw_more = true`).
    run_mythos_draw_chain(state, events, investigator, 0, true)
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
    state: &mut GameState,
    events: &mut Vec<Event>,
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
        let Some(code) = draw_encounter_top(state, events) else {
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
        skill_test::peril_check(state, events, &code, investigator, metadata.peril);

        // Step 3 + 4: Resolve revelation, then enemy-spawn if applicable.
        let outcome = resolve_encounter_card(state, events, investigator, code.clone(), metadata);
        match outcome {
            EngineOutcome::Done => {}
            EngineOutcome::AwaitingInput { .. } => {
                // A mid-chain spawn engagement tie suspended. Record the
                // live chain position so the resume keeps counting toward
                // the cap rather than restarting its budget.
                if let Some(pending) = state.spawn_engage_pending.as_mut() {
                    pending.chain_count = chain_count;
                }
                return outcome;
            }
            EngineOutcome::Rejected { .. } => return outcome,
        }

        // Step 5: If the drawn card has the surge keyword, loop.
        draw_more = metadata.surge;
    }

    // Chain complete — advance the cursor.
    advance_mythos_draw_pending(state, events);
    EngineOutcome::Done
}

/// Advance `state.mythos_draw_pending` after a completed chain. If
/// a next investigator exists in turn order, set to that id.
/// Otherwise set to None and open the post-1.4 window.
fn advance_mythos_draw_pending(state: &mut GameState, events: &mut Vec<Event>) {
    let current = state
        .mythos_draw_pending
        .expect("advance_mythos_draw_pending called only after a successful chain");
    // Eliminated-skip semantics live in `next_active_investigator_after`.
    let next = cursor::next_active_investigator_after(state, current);
    state.mythos_draw_pending = next;
    if next.is_none() {
        reaction_windows::open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}

#[cfg(test)]
mod mythos_draw_for_tests {
    use super::*;
    use crate::state::CardCode;
    use crate::test_support::{test_investigator, TestGame};

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
        let mut state = TestGame::default()
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
        let outcome = mythos_draw_for(&mut state, &mut events, InvestigatorId(1));
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
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn rejects_outside_mythos_phase() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(1));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("only valid during Mythos")
        ));
    }

    #[test]
    fn rejects_when_no_draw_pending() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = None;
        let mut events = Vec::new();
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(1));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("no draw pending")
        ));
    }

    #[test]
    fn rejects_when_out_of_order() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        // Inv2 attempts to draw when inv1 is expected.
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(2));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("out of order")
        ));
    }
}

#[cfg(test)]
mod mythos_phase_tests {
    use super::*;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn mythos_phase_emits_phase_started_and_seeds_draw_pending() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseStarted(Mythos); events = {events:?}"
        );
    }

    #[test]
    fn mythos_phase_with_empty_turn_order_opens_after_draws_window_inline() {
        let mut state = TestGame::default().with_phase(Phase::Mythos).build();
        state.turn_order.clear();
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        // No drawers → open_fast_window runs for MythosAfterDraws,
        // which auto-skips (no Fast eligibility), runs continuation
        // (mythos_phase_end), which steps into Investigation.
        assert_eq!(state.mythos_draw_pending, None);
        assert_eq!(state.phase, Phase::Investigation);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::MythosAfterDraws
                }
            )),
            "must emit WindowOpened(MythosAfterDraws); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::MythosAfterDraws
                }
            )),
            "must emit WindowClosed(MythosAfterDraws); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseEnded(Mythos); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "must emit PhaseStarted(Investigation); events = {events:?}"
        );
    }

    #[test]
    fn mythos_phase_end_emits_phase_ended_and_steps_to_investigation() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        let mut events = Vec::new();

        mythos_phase_end(&mut state, &mut events);

        assert_eq!(state.phase, Phase::Investigation);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseEnded(Mythos); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "must emit PhaseStarted(Investigation); events = {events:?}"
        );
    }

    /// Site 1 fix (Rules Reference p.10): when the lead investigator in
    /// `turn_order` is eliminated, `mythos_phase` must seed
    /// `mythos_draw_pending` with the first Active investigator rather
    /// than blindly taking `turn_order.first()`.
    #[test]
    fn mythos_phase_skips_eliminated_lead_when_seeding_cursor() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        assert_eq!(
            state.mythos_draw_pending,
            Some(InvestigatorId(2)),
            "cursor must point to the first Active investigator, not the Killed lead"
        );
    }

    /// All investigators in `turn_order` are eliminated. `mythos_phase`
    /// must treat this the same as an empty `turn_order`: seed to None
    /// and open `MythosAfterDraws` inline, which auto-skips and drives
    /// `mythos_phase_end`, transitioning to Investigation.
    ///
    /// This is the non-empty-`turn_order` analogue of
    /// `mythos_phase_with_empty_turn_order_opens_after_draws_window_inline`.
    #[test]
    fn mythos_phase_with_all_investigators_eliminated_opens_after_draws_window() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        assert_eq!(state.mythos_draw_pending, None);
        assert_eq!(
            state.phase,
            Phase::Investigation,
            "no Active drawers → MythosAfterDraws fires inline → Investigation"
        );
    }

    /// Site 2 fix (Rules Reference p.10): when advancing the cursor
    /// after a completed draw, eliminated investigators in the middle of
    /// `turn_order` must be skipped. Here inv2 is Killed; the cursor must
    /// advance from inv1 to inv3.
    #[test]
    fn advance_mythos_draw_pending_skips_eliminated_middle_investigator() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)];
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Killed;
        // Simulate: inv1 has just completed their draw chain.
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();

        advance_mythos_draw_pending(&mut state, &mut events);

        assert_eq!(
            state.mythos_draw_pending,
            Some(InvestigatorId(3)),
            "cursor must skip the Killed inv2 and land on Active inv3"
        );
    }

    #[test]
    fn first_active_investigator_finds_first_active_skipping_eliminated() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Insane;

        assert_eq!(
            cursor::first_active_investigator(&state),
            Some(InvestigatorId(3)),
            "first Active in turn_order after skipping eliminated"
        );
    }

    #[test]
    fn first_active_investigator_returns_none_when_all_eliminated() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(cursor::first_active_investigator(&state), None);
    }

    #[test]
    fn first_active_investigator_returns_none_when_turn_order_empty() {
        let state = TestGame::default().build();
        assert_eq!(cursor::first_active_investigator(&state), None);
    }

    #[test]
    fn next_active_investigator_after_skips_eliminated_middle() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_investigator(test_investigator(4))
            .build();
        state.turn_order = vec![
            InvestigatorId(1),
            InvestigatorId(2),
            InvestigatorId(3),
            InvestigatorId(4),
        ];
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(
            cursor::next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(3)),
            "advance from 1 skips Killed 2, lands on 3"
        );
        assert_eq!(
            cursor::next_active_investigator_after(&state, InvestigatorId(3)),
            Some(InvestigatorId(4)),
            "advance from 3 lands on 4"
        );
        assert_eq!(
            cursor::next_active_investigator_after(&state, InvestigatorId(4)),
            None,
            "advance past the last entry returns None"
        );
    }

    #[test]
    fn next_active_investigator_after_returns_none_when_current_not_in_turn_order() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        assert_eq!(
            cursor::next_active_investigator_after(&state, InvestigatorId(99)),
            None
        );
    }

    #[test]
    fn next_active_investigator_after_works_when_current_is_non_active() {
        // Defeated-mid-loop semantics: `current` may be Killed by the
        // time we advance from them. The cursor still finds the right
        // successor.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(
            cursor::next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(2)),
            "current=1 is non-Active but turn_order still anchors the index"
        );
    }
}

#[cfg(test)]
mod grant_resources_tests {
    use super::*;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn grant_resources_adds_to_wallet_and_emits() {
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();

        grant_resources(&mut state, &mut events, id, 2);

        assert_eq!(state.investigators[&id].resources, before + 2);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::ResourcesGained { investigator, amount: 2 } if *investigator == id
        )));
    }

    #[test]
    fn grant_resources_zero_is_silent_noop() {
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();

        grant_resources(&mut state, &mut events, id, 0);

        assert_eq!(state.investigators[&id].resources, before);
        assert!(events.is_empty());
    }
}

#[cfg(test)]
mod draw_one_with_deckout_tests {
    use super::*;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn draw_one_with_deckout_empty_deck_reshuffles_and_takes_horror() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck.clear();
        inv.discard = vec![CardCode::new("01000"), CardCode::new("01001")];
        inv.horror = 0;
        let hand_before = inv.hand.len();
        let mut state = TestGame::default().with_investigator(inv).build();
        let mut events = Vec::new();

        draw_one_with_deckout(&mut state, &mut events, id);

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

#[cfg(test)]
mod upkeep_phase_tests {
    use super::*;
    use crate::action::{Action, PlayerAction};
    use crate::engine::{apply, EngineOutcome};
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase, Status,
    };
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};
    use crate::{assert_event, assert_event_sequence, assert_no_event};

    #[test]
    fn upkeep_phase_emits_phase_started_and_auto_skips_to_mythos() {
        // No Fast-eligible cards / no reactions installed → the post-4.1
        // window auto-skips inline, the continuation runs, and the
        // cascade lands in Mythos.
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Enemy → Upkeep, cascades to Mythos

        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
        let started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseStarted(Upkeep)");
        let w_open = pos(&|e| {
            matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::UpkeepBegins
                }
            )
        })
        .expect("WindowOpened(UpkeepBegins)");
        let w_close = pos(&|e| {
            matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::UpkeepBegins
                }
            )
        })
        .expect("WindowClosed(UpkeepBegins)");
        let ended = pos(&|e| {
            matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseEnded(Upkeep)");
        let mythos = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Mythos
                }
            )
        })
        .expect("PhaseStarted(Mythos)");
        assert!(
            started < w_open && w_open < w_close && w_close < ended && ended < mythos,
            "upkeep sub-step events must be ordered 4.1 → window → 4.6 → Mythos 1.1; \
             events = {events:?}"
        );
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert!(
            state.open_windows.is_empty(),
            "UpkeepBegins must not persist on the stack"
        );
    }

    #[test]
    fn ready_exhausted_cards_readies_investigator_cards_and_enemies() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut inv = test_investigator(1);
        let mut card = CardInPlay::enter_play(CardCode("01000".into()), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play = vec![card];
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true;
        let mut state = TestGame::default()
            .with_investigator(inv)
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(
            !state.investigators[&inv_id].cards_in_play[0].exhausted,
            "card readied"
        );
        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert!(events.iter().any(|e| matches!(
            e, Event::CardReadied { investigator, instance_id, .. }
            if *investigator == inv_id && *instance_id == CardInstanceId(1))));
        assert!(events.iter().any(|e| matches!(
            e, Event::EnemyReadied { enemy } if *enemy == enemy_id)));
    }

    #[test]
    fn ready_exhausted_cards_reengages_co_located_unengaged_enemy() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let loc = test_location(10, "Synth Loc");
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true; // exhausted + disengaged, e.g. survived a successful Evade
        enemy.current_location = Some(LocationId(10));
        let mut state = TestGame::default()
            .with_investigator_at(test_investigator(1), LocationId(10))
            .with_location(loc)
            .with_enemy(enemy)
            .with_turn_order([inv_id])
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with,
            Some(inv_id),
            "readied enemy re-engages the co-located investigator (RR p.10)"
        );
        assert_event!(events, Event::EnemyReadied { enemy } if *enemy == enemy_id);
        assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv_id);
        assert_event_sequence!(
            events,
            Event::EnemyReadied { .. },
            Event::EnemyEngaged { .. },
        );
    }

    #[test]
    fn ready_exhausted_cards_leaves_ready_cards_untouched() {
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = false; // already ready
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(
            events.is_empty(),
            "no readying events for already-ready cards"
        );
    }

    #[test]
    fn ready_exhausted_cards_no_engage_when_no_co_located_investigator() {
        let enemy_id = EnemyId(1);
        let inv_id = InvestigatorId(1);
        let loc = test_location(10, "Synth Loc");
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true;
        enemy.current_location = Some(LocationId(10));
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1)) // current_location stays None — NOT co-located
            .with_location(loc)
            .with_enemy(enemy)
            .with_turn_order([inv_id])
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with, None,
            "no investigator at the enemy's location → no engagement"
        );
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn ready_exhausted_cards_keeps_existing_engagement_no_duplicate() {
        let enemy_id = EnemyId(1);
        let inv_id = InvestigatorId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true; // exhausted but still engaged (e.g. attacked last Enemy phase)
        enemy.engaged_with = Some(inv_id);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut state, &mut events);

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with,
            Some(inv_id),
            "an already-engaged enemy keeps its engagement"
        );
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn upkeep_draw_and_resource_draws_and_grants_per_active_investigator() {
        let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::new("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::new("01001")];
        let mut inv_c = test_investigator(3);
        inv_c.status = Status::Resigned; // eliminated → skipped
        inv_c.deck = vec![CardCode::new("01002")];
        let res_a = inv_a.resources;
        let res_b = inv_b.resources;
        let res_c = inv_c.resources;
        let hand_a = inv_a.hand.len();
        let mut state = TestGame::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .with_investigator(inv_c)
            .build();
        state.turn_order = vec![a, b, c];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut state, &mut events);

        assert_eq!(state.investigators[&a].resources, res_a + 1);
        assert_eq!(state.investigators[&b].resources, res_b + 1);
        assert_eq!(
            state.investigators[&c].resources, res_c,
            "eliminated investigator skipped"
        );
        assert_eq!(state.investigators[&a].hand.len(), hand_a + 1);
        assert_eq!(
            state.investigators[&c].deck.len(),
            1,
            "eliminated investigator did not draw"
        );
    }

    #[test]
    fn upkeep_draw_and_resource_two_pass_ordering() {
        // All CardsDrawn events precede all ResourcesGained events.
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::new("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::new("01001")];
        let mut state = TestGame::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut state, &mut events);

        let last_draw = events
            .iter()
            .rposition(|e| matches!(e, Event::CardsDrawn { .. }))
            .expect("draws");
        let first_gain = events
            .iter()
            .position(|e| matches!(e, Event::ResourcesGained { .. }))
            .expect("gains");
        assert!(
            last_draw < first_gain,
            "all draws must precede all resource gains"
        );
    }

    #[test]
    fn reset_actions_sets_active_to_per_turn_and_skips_eliminated() {
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.actions_remaining = 0;
        let mut inv_b = test_investigator(2);
        inv_b.actions_remaining = 0;
        inv_b.status = Status::Killed;
        let mut state = TestGame::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        reset_actions(&mut state, &mut events);

        assert_eq!(state.investigators[&a].actions_remaining, ACTIONS_PER_TURN);
        assert_eq!(
            state.investigators[&b].actions_remaining, 0,
            "eliminated skipped"
        );
        assert!(events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, new_count }
            if *investigator == a && *new_count == ACTIONS_PER_TURN)));
        assert!(!events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, .. } if *investigator == b)));
    }

    #[test]
    fn reset_actions_emits_nothing_for_already_full() {
        // Emit-on-change semantics: when actions_remaining already equals
        // ACTIONS_PER_TURN, reset_actions makes no state change and emits
        // no ActionsRemainingChanged event.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = ACTIONS_PER_TURN;
        let mut state = TestGame::default().with_investigator(inv).build();
        state.turn_order = vec![id];
        let mut events = Vec::new();

        reset_actions(&mut state, &mut events);

        assert_eq!(state.investigators[&id].actions_remaining, ACTIONS_PER_TURN);
        assert!(events.is_empty(), "no event when value is unchanged");
    }

    #[test]
    fn rotate_to_active_does_not_refresh_actions() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 1;
        let mut state = TestGame::default().with_investigator(inv).build();
        let mut events = Vec::new();

        rotate_to_active(&mut state, &mut events, id);

        assert_eq!(state.active_investigator, Some(id));
        assert_eq!(
            state.investigators[&id].actions_remaining, 1,
            "rotate must not refresh actions"
        );
        assert!(
            events.is_empty(),
            "rotate no longer emits ActionsRemainingChanged"
        );
    }

    #[test]
    fn round_increments_on_mythos_entry_via_driver() {
        // After the Upkeep→Mythos cascade, state.round bumps by 1.
        // The bump now lives in mythos_phase step 1.1 (this task);
        // the test asserts observable behavior, which is unchanged.
        let id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Upkeep)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;
        state.round = 4;

        let mut events = Vec::new();
        step_phase(&mut state, &mut events); // Upkeep → ... → Mythos via the cascade

        assert_eq!(state.round, 5, "round bumps on Mythos entry");
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn end_turn_cascades_through_upkeep_to_mythos_draw_pending() {
        // Single investigator, non-empty deck, an exhausted in-play card.
        // After EndTurn: card readied, hand +1, resources +1, landed in
        // Mythos with draw pending and round bumped.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 0;
        inv.deck = vec![CardCode::new("01000"), CardCode::new("01001")];
        let mut card = CardInPlay::enter_play(CardCode::new("01002"), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play = vec![card];
        let res_before = inv.resources;
        let hand_before = inv.hand.len();
        let mut state = TestGame::default()
            .with_investigator(inv)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = Some(id);
        state.round = 1;

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.phase, Phase::Mythos);
        assert_eq!(result.state.round, 2, "round bumped on Mythos entry");
        assert_eq!(result.state.mythos_draw_pending, Some(id));
        assert_eq!(result.state.active_investigator, None);
        assert!(
            !result.state.investigators[&id].cards_in_play[0].exhausted,
            "readied"
        );
        assert_eq!(
            result.state.investigators[&id].resources,
            res_before + 1,
            "gained 1"
        );
        assert_eq!(
            result.state.investigators[&id].hand.len(),
            hand_before + 1,
            "drew 1"
        );
    }
}

#[cfg(test)]
mod enemy_phase_tests {
    use super::*;
    use crate::action::Action;
    use crate::assert_event;
    use crate::engine::{apply, EngineOutcome};
    use crate::state::{
        EnemyId, FastActorScope, InvestigatorId, LocationId, OpenWindow, Phase, Status, WindowKind,
    };
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};
    use crate::Event;

    #[test]
    fn enemy_phase_runs_hunters_then_attack_loop_when_no_tie() {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        loc_a.connections = vec![LocationId(2)];
        loc_b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);
        assert_eq!(outcome, EngineOutcome::Done);
        // No registry installed → the attack window auto-skips inline and
        // the cascade runs Enemy→Upkeep→Mythos within this same call (same
        // as `enemy_phase_emits_phase_started_and_cascades_to_mythos...`).
        // The hunter still moved + engaged during step 3.2, and the first
        // attack window still opened — asserted via the event stream below.
        assert_eq!(state.phase, Phase::Mythos);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert_event!(events, Event::EnemyEngaged { enemy, .. } if *enemy == EnemyId(1));
        assert_event!(events, Event::WindowOpened { kind } if *kind == WindowKind::BeforeInvestigatorAttacked);
    }

    #[test]
    fn enemy_phase_suspends_on_hunter_tie_then_resumes_into_attack_loop() {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = end_turn(&mut state, &mut events);
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(state.phase, Phase::Enemy);
        let mut ev2 = Vec::new();
        let resumed = resolve_input(
            &mut state,
            &mut ev2,
            &InputResponse::PickLocation(LocationId(2)),
        );
        assert_eq!(resumed, EngineOutcome::Done);
        assert_event!(ev2, Event::WindowOpened { kind } if *kind == WindowKind::BeforeInvestigatorAttacked);
        // With no registry the attack window auto-skips and the cascade runs
        // Enemy->Upkeep->Mythos within the same resume call (same as the no-tie test).
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn resolve_attacks_for_investigator_fires_engaged_ready_enemy_and_exhausts() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        combat::resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // Damage placed.
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
            )),
            "expected DamageTaken {{ amount: 1 }}; events = {events:?}"
        );

        // Enemy exhausted in state and event.
        assert!(
            state.enemies[&enemy_id].exhausted,
            "enemy must be exhausted"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::EnemyExhausted { enemy } if *enemy == enemy_id
            )),
            "expected EnemyExhausted; events = {events:?}"
        );

        // Ordering: DamageTaken precedes EnemyExhausted (post-attack exhaust).
        let damage_pos = events
            .iter()
            .position(|e| matches!(e, Event::DamageTaken { .. }))
            .unwrap();
        let exhaust_pos = events
            .iter()
            .position(|e| matches!(e, Event::EnemyExhausted { .. }))
            .unwrap();
        assert!(
            damage_pos < exhaust_pos,
            "DamageTaken must precede EnemyExhausted; events = {events:?}"
        );
    }

    #[test]
    fn resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies() {
        let inv_id = InvestigatorId(1);

        // Engaged but exhausted — must NOT attack.
        let mut e1 = test_enemy(1, "Exhausted Engaged");
        e1.engaged_with = Some(inv_id);
        e1.exhausted = true;
        e1.attack_damage = 5;

        // Ready but unengaged — must NOT attack.
        let mut e2 = test_enemy(2, "Ready Unengaged");
        e2.engaged_with = None;
        e2.attack_damage = 5;

        // Ready engaged — the only one that attacks.
        let mut e3 = test_enemy(3, "Ready Engaged");
        e3.engaged_with = Some(inv_id);
        e3.attack_damage = 1;

        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(e1)
            .with_enemy(e2)
            .with_enemy(e3)
            .build();
        let mut events = Vec::new();

        combat::resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // Exactly one DamageTaken (from e3, amount 1).
        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(
            damages.len(),
            1,
            "exactly one attacker should fire; events = {events:?}"
        );
        assert!(matches!(damages[0], Event::DamageTaken { amount: 1, .. }));

        // Only e3 exhausted; e1 already was; e2 must remain ready.
        assert!(
            state.enemies[&EnemyId(1)].exhausted,
            "e1 was already exhausted; still is"
        );
        assert!(
            !state.enemies[&EnemyId(2)].exhausted,
            "e2 must NOT exhaust (didn't attack)"
        );
        assert!(
            state.enemies[&EnemyId(3)].exhausted,
            "e3 attacked and exhausted"
        );

        // Exactly one EnemyExhausted event (e3). e1's prior-state exhausted doesn't re-emit.
        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(3) }
        ));
    }

    #[test]
    fn resolve_attacks_for_investigator_iterates_attackers_in_enemy_id_order() {
        let inv_id = InvestigatorId(1);

        let mut e_lower = test_enemy(2, "Lower id"); // EnemyId(2)
        e_lower.engaged_with = Some(inv_id);
        e_lower.attack_damage = 1;

        let mut e_higher = test_enemy(10, "Higher id"); // EnemyId(10)
        e_higher.engaged_with = Some(inv_id);
        e_higher.attack_damage = 2;

        let mut state = TestGame::default()
            .with_investigator({
                let mut inv = test_investigator(1);
                inv.max_health = 100; // survive both attacks
                inv
            })
            .with_enemy(e_higher) // insert in NON-id order to confirm BTreeMap ordering wins
            .with_enemy(e_lower)
            .build();
        let mut events = Vec::new();

        combat::resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // The two DamageTaken events must appear in EnemyId(2) → EnemyId(10) order
        // (verifiable via their amounts: 1 then 2).
        let damages: Vec<u8> = events
            .iter()
            .filter_map(|e| match e {
                Event::DamageTaken { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(
            damages,
            vec![1, 2],
            "EnemyId order: 2 (dmg 1) before 10 (dmg 2)"
        );
    }

    #[test]
    fn resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop() {
        let inv_id = InvestigatorId(1);

        // EnemyId(1) deals the killing blow on its attack.
        let mut e1 = test_enemy(1, "Killer");
        e1.engaged_with = Some(inv_id);
        e1.attack_damage = 1;

        // EnemyId(2) must NOT attack (active check fails at loop top).
        let mut e2 = test_enemy(2, "Bystander");
        e2.engaged_with = Some(inv_id);
        e2.attack_damage = 5;

        let mut state = TestGame::default()
            .with_investigator({
                let mut inv = test_investigator(1);
                inv.max_health = 1; // e1's attack defeats
                inv
            })
            .with_enemy(e1)
            .with_enemy(e2)
            .build();
        let mut events = Vec::new();

        combat::resolve_attacks_for_investigator(&mut state, &mut events, inv_id);

        // e1 attacked + exhausted.
        assert!(
            state.enemies[&EnemyId(1)].exhausted,
            "e1 attacked, must exhaust"
        );
        // e2 did NOT attack and did NOT exhaust.
        assert!(
            !state.enemies[&EnemyId(2)].exhausted,
            "e2 must not exhaust (early-break)"
        );

        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(
            damages.len(),
            1,
            "only e1's attack lands; events = {events:?}"
        );

        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(1) }
        ));

        // Investigator was defeated.
        assert_eq!(state.investigators[&inv_id].status, Status::Killed);
    }

    #[test]
    fn enemy_phase_emits_phase_started_and_cascades_to_mythos_in_no_eligibility_case() {
        // 1 Active investigator, no engaged enemies. Auto-skip
        // cascades through both windows + enemy_phase_end +
        // Upkeep → Mythos.
        let inv_id = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Positional ordering of the major events.
        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
        let started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Enemy
                }
            )
        })
        .expect("PhaseStarted(Enemy)");
        let w1_open = pos(&|e| {
            matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::BeforeInvestigatorAttacked
                }
            )
        })
        .expect("WindowOpened(Before)");
        let w1_close = pos(&|e| {
            matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::BeforeInvestigatorAttacked
                }
            )
        })
        .expect("WindowClosed(Before)");
        let w2_open = pos(&|e| {
            matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::AfterAllInvestigatorsAttacked
                }
            )
        })
        .expect("WindowOpened(After)");
        let w2_close = pos(&|e| {
            matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::AfterAllInvestigatorsAttacked
                }
            )
        })
        .expect("WindowClosed(After)");
        let ended = pos(&|e| {
            matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .expect("PhaseEnded(Enemy)");
        let upkeep_started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseStarted(Upkeep)");

        assert!(
            started < w1_open
                && w1_open < w1_close
                && w1_close < w2_open
                && w2_open < w2_close
                && w2_close < ended
                && ended < upkeep_started,
            "ordered: 3.1 → BeforeInv window → AfterAll window → 3.4 → Upkeep 4.1; events = {events:?}"
        );
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert_eq!(state.enemy_attack_pending, None, "cursor cleared at end");
    }

    #[test]
    fn enemy_phase_with_two_investigators_iterates_in_turn_order() {
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Two BeforeInvestigatorAttacked windows + one AfterAll.
        let before_opens: Vec<usize> = events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                matches!(
                    e,
                    Event::WindowOpened {
                        kind: WindowKind::BeforeInvestigatorAttacked
                    }
                )
                .then_some(i)
            })
            .collect();
        let after_opens: Vec<usize> = events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                matches!(
                    e,
                    Event::WindowOpened {
                        kind: WindowKind::AfterAllInvestigatorsAttacked
                    }
                )
                .then_some(i)
            })
            .collect();
        assert_eq!(before_opens.len(), 2, "one window per Active investigator");
        assert_eq!(after_opens.len(), 1);
        assert!(before_opens[0] < before_opens[1] && before_opens[1] < after_opens[0]);
    }

    #[test]
    fn enemy_phase_skips_eliminated_investigator_in_advance() {
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let id3 = InvestigatorId(3);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2, id3];
        state.active_investigator = None;
        state.investigators.get_mut(&id2).unwrap().status = Status::Insane;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // Only 2 BeforeInvestigatorAttacked windows (id1 + id3).
        let before_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::WindowOpened {
                        kind: WindowKind::BeforeInvestigatorAttacked
                    }
                )
            })
            .count();
        assert_eq!(before_count, 2, "Insane id2 must be skipped");
    }

    #[test]
    fn enemy_phase_with_all_eliminated_opens_after_all_directly() {
        let id1 = InvestigatorId(1);
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1];
        state.active_investigator = None;
        state.investigators.get_mut(&id1).unwrap().status = Status::Killed;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // No BeforeInvestigatorAttacked windows — straight to AfterAll.
        assert!(
            events.iter().all(|e| !matches!(
                e,
                Event::WindowOpened {
                    kind: WindowKind::BeforeInvestigatorAttacked
                }
            )),
            "no per-investigator window when all are eliminated; events = {events:?}"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            Event::WindowOpened {
                kind: WindowKind::AfterAllInvestigatorsAttacked
            }
        )));
        // With all investigators eliminated, the cascade keeps going:
        // Enemy → Upkeep (no-op steps for empty Active set) → Mythos
        // (mythos_draw_pending = None → auto-skip path) → Investigation.
        // The point of this test is the structural shape — no
        // BeforeInvestigatorAttacked window, AfterAll opens directly —
        // not the terminal phase.
        assert_eq!(state.phase, Phase::Investigation);
    }

    #[test]
    fn enemy_phase_attack_lands_in_full_cascade() {
        // 1 investigator engaged with 1 ready enemy. Full Investigation→Enemy→Upkeep→Mythos
        // cascade; attack lands inside the BeforeInvestigatorAttacked continuation.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Investigation → Enemy

        // The attack landed. Event-stream evidence — state.enemies's
        // `exhausted` flag is reset by Upkeep step 4.3 later in the
        // cascade (ready_exhausted_cards), so checking the post-cascade
        // state directly would race the readying step. The
        // DamageTaken + EnemyExhausted events emitted inside the
        // BeforeInvestigatorAttacked continuation are the authoritative
        // signal that the attack landed.
        assert!(events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )));

        // Cascade landed in Mythos.
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn step_phase_from_enemy_does_not_emit_phase_ended_enemy() {
        // Direct unit-level check: step_phase emits no PhaseEnded itself,
        // so the Enemy→Upkeep step must not emit PhaseEnded(Enemy)
        // (enemy_phase_end owns that emit).
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;
        // Use a state where Upkeep's cascade can complete (Active investigator exists).
        let mut events = Vec::new();

        step_phase(&mut state, &mut events); // Enemy → Upkeep

        // step_phase itself MUST NOT emit PhaseEnded(Enemy); only
        // enemy_phase_end is allowed to (which doesn't run here — we
        // started in Enemy and stepped out, simulating the "phase
        // transition without driver-owned end emit" path).
        let phase_ended_enemy_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::PhaseEnded {
                        phase: Phase::Enemy
                    }
                )
            })
            .count();
        assert_eq!(
            phase_ended_enemy_count, 0,
            "step_phase must NOT emit PhaseEnded(Enemy); only enemy_phase_end may. events = {events:?}"
        );
    }

    #[test]
    fn enemy_phase_resumes_via_skip_input() {
        // Construct the state mid-pause: a BeforeInvestigatorAttacked
        // window is on the stack with empty pending_triggers (the
        // "pure-Fast window" shape that open_fast_window pushes when
        // Fast play is eligible), and the cursor points at inv1.
        //
        // Submitting PlayerAction::ResolveInput(InputResponse::Skip)
        // routes through resolve_input's "open_windows non-empty +
        // no reaction triggers" branch → close_reaction_window_at →
        // run_window_continuation's BeforeInvestigatorAttacked arm →
        // resolve_attacks_for_investigator → cursor advance to None →
        // open AfterAllInvestigatorsAttacked → auto-skip continuation
        // → enemy_phase_end → cascade Upkeep → Mythos.
        //
        // The synthetic OpenWindow push fakes the pause point because
        // a real Fast-eligibility setup would require either a card-
        // registry install (heavyweight integration test) or a Fast
        // event card in hand with resources — neither tractable in
        // the engine layer. The Skip path itself is the load-bearing
        // resume mechanism this test exercises.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        state.enemy_attack_pending = Some(inv_id);
        state.open_windows.push(OpenWindow {
            kind: WindowKind::BeforeInvestigatorAttacked,
            pending_triggers: Vec::new(),
            fast_actors: FastActorScope::Any,
        });

        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
        );

        match result.outcome {
            EngineOutcome::Done => {}
            ref other => panic!(
                "expected Done after Skip; got {other:?}; events = {:?}",
                result.events
            ),
        }
        assert_eq!(
            result.state.phase,
            Phase::Mythos,
            "cascade lands in Mythos after Skip resumed the continuation"
        );
        assert!(
            result.events.iter().any(|e| matches!(
                e,
                Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
            )),
            "attack should have landed during the resumed continuation; events = {:?}",
            result.events
        );
        assert!(
            result.events.iter().any(|e| matches!(
                e,
                Event::EnemyExhausted { enemy } if *enemy == enemy_id
            )),
            "EnemyExhausted should fire during the resumed continuation; events = {:?}",
            result.events
        );
        assert_eq!(
            result.state.enemy_attack_pending, None,
            "cursor must clear after the continuation advances past the last \
             Active investigator and the AfterAll window auto-skips"
        );
    }

    // TODO(#71 follow-up): pause-on-Fast-eligibility test — needs a
    // tractable Fast-eligibility fixture at the engine layer (Fast
    // event card in hand + resources + card-registry install, which
    // would push this into the cards crate's integration tests). The
    // Skip-resume test above proves the resume path is correct; the
    // pause shape is exercised indirectly via the existing
    // any_fast_play_eligible-driven open_fast_window tests at
    // dispatch.rs's open_fast_window_tests block.
}

#[cfg(test)]
mod doom_agenda_tests {
    use super::*;
    use crate::event::Event;
    use crate::test_support::TestGame;
    use crate::{assert_event, assert_no_event};

    #[test]
    fn place_doom_increments_agenda_doom() {
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 2,
            resolution: None,
        }];
        let mut events = Vec::new();
        place_doom_on_agenda(&mut state, &mut events);
        assert_eq!(state.agenda_doom, 1);
        place_doom_on_agenda(&mut state, &mut events);
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn doom_threshold_advances_non_terminal_agenda() {
        use crate::scenario::Resolution;
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![
            Agenda {
                doom_threshold: 2,
                resolution: None,
            },
            Agenda {
                doom_threshold: 2,
                resolution: Some(Resolution::Lost {
                    reason: "agenda".into(),
                }),
            },
        ];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(state.agenda_index, 1);
        assert_eq!(state.agenda_doom, 0, "doom resets on advance");
        assert!(
            state.resolution.is_none(),
            "non-terminal advance does not resolve"
        );
        assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
    }

    #[test]
    fn doom_threshold_on_terminal_agenda_sets_resolution_latch() {
        use crate::scenario::Resolution;
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 2,
            resolution: Some(Resolution::Lost {
                reason: "doom".into(),
            }),
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(
            state.agenda_index, 0,
            "cursor does not move on a terminal agenda"
        );
        assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));
        assert_no_event!(events, Event::AgendaAdvanced { .. });
    }

    #[test]
    fn doom_threshold_not_met_does_nothing() {
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 3,
            resolution: None,
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(state.agenda_index, 0);
        assert_eq!(state.agenda_doom, 2);
        assert!(events.is_empty());
    }

    #[test]
    fn request_resolution_is_first_writer_wins() {
        use crate::scenario::Resolution;
        let mut state = TestGame::new().build();
        request_resolution(
            &mut state,
            Resolution::Lost {
                reason: "first".into(),
            },
        );
        request_resolution(
            &mut state,
            Resolution::Won {
                id: "second".into(),
            },
        );
        assert!(
            matches!(state.resolution, Some(Resolution::Lost { ref reason }) if reason == "first")
        );
    }
}

#[cfg(test)]
mod advance_act_tests {
    use super::*;
    use crate::action::Action;
    use crate::engine::{apply, EngineOutcome};
    use crate::event::Event;
    use crate::state::{InvestigatorId, Phase};
    use crate::test_support::{test_investigator, TestGame};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn advance_act_rejects_when_clues_insufficient() {
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 1;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            clue_threshold: 2,
            resolution: None,
        }];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.act_index, 0);
        assert_eq!(
            result.state.investigators[&inv].clues, 1,
            "no clues spent on reject"
        );
    }

    #[test]
    fn advance_act_spends_clues_and_advances_non_terminal() {
        use crate::scenario::Resolution;
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 3;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![
            Act {
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                clue_threshold: 2,
                resolution: Some(Resolution::Won { id: "demo".into() }),
            },
        ];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.act_index, 1);
        assert_eq!(
            result.state.investigators[&inv].clues, 1,
            "spent exactly 2 of 3"
        );
        assert!(result.state.resolution.is_none());
        assert_event!(result.events, Event::ActAdvanced { from } if *from == 0);
    }

    #[test]
    fn advance_act_on_terminal_act_sets_resolution_latch() {
        use crate::scenario::Resolution;
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 2;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            clue_threshold: 2,
            resolution: Some(Resolution::Won { id: "demo".into() }),
        }];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.act_index, 0,
            "cursor does not move on a terminal act"
        );
        assert!(matches!(
            result.state.resolution,
            Some(Resolution::Won { .. })
        ));
        assert_no_event!(result.events, Event::ActAdvanced { .. });
        assert_eq!(result.state.investigators[&inv].clues, 0);
    }

    #[test]
    fn advance_act_spends_acting_investigator_first_then_turn_order() {
        use crate::state::Act;
        let acting = InvestigatorId(1);
        let other = InvestigatorId(2);
        let mut inv1 = test_investigator(1);
        inv1.clues = 1;
        let mut inv2 = test_investigator(2);
        inv2.clues = 2;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv1)
            .with_investigator(inv2)
            .with_active_investigator(acting)
            .with_turn_order([acting, other])
            .build();
        // Two acts so the non-terminal first act can advance the cursor to 1
        // (a terminal `resolution: None` act at the end would hit the
        // advance-past-end `unreachable!`). The successor's contents are
        // irrelevant to this spend-order test.
        state.act_deck = vec![
            Act {
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                clue_threshold: 2,
                resolution: None,
            },
        ];

        // Threshold 2: acting (1 clue) drained fully first, then 1 from `other`.
        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: acting,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.investigators[&acting].clues, 0,
            "acting drained first"
        );
        assert_eq!(
            result.state.investigators[&other].clues, 1,
            "remainder taken from turn_order"
        );
        assert_eq!(result.state.act_index, 1);
    }
}
