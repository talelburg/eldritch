//! Encounter-deck draw, spawn, and Mythos draw chain handlers.

use std::mem;

use card_dsl::card_data::{CardKind, CardMetadata, CardType, HealthValue, Spawn, SpawnLocation};
use card_dsl::dsl::{Ability, Effect, Trigger};

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::dispatch::hunters::PreyResolution;
use crate::engine::dispatch::{choice, cursor, hunters, reaction_windows, skill_test};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::{EngineOutcome, InputRequest, OptionTarget, ResumeToken};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{
    CardCode, Continuation, EncounterCardFrame, EncounterDisposition, EncounterDrawFrame, Enemy,
    FastWindowKind, InvestigatorId, LocationId, Owner, PhaseStep, PlayerDrawFrame,
    SpawnEngagePending, Status,
};

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
/// # Validate-first ordering note
///
/// `draw_encounter_top` mutates `state.encounter_deck` /
/// `state.encounter_discard` BEFORE the unknown-code reject can
/// fire; `Event::CardRevealed` then emits BEFORE Revelation /
/// spawn resolve. The ordering is deliberate — the card must be
/// removed from the deck before the reaction window opens
/// (Before-timing listeners need to see the revealed-but-not-yet-
/// resolved state), and `CardRevealed` emits before Revelation for
/// the same rules-correct interposition point. A mid-handler
/// `Rejected` is NOT a hazard here: `apply_via` snapshot-restores
/// the whole state (deck, discard, and RNG included) on rejection,
/// so the ordering only matters on non-rejecting paths.
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

/// A treachery is **persistent** (stays in play after its Revelation,
/// owning its own disposition) iff it has at least one ability whose
/// trigger is not [`Trigger::Revelation`] — the ongoing `Constant`
/// restriction / `OnEvent` forced-discard abilities the three C4c
/// treacheries carry. One-shot treacheries have only a `Revelation`, so
/// they auto-discard after it resolves.
///
/// TODO: assumes every persistent treachery carries an ongoing ability
/// and every one-shot carries none (holds for all Core+Dunwich
/// treacheries). Revisit with an explicit persistence marker only if a
/// treachery must persist with no ongoing ability, or auto-discard
/// despite carrying one.
pub(crate) fn treachery_is_persistent(abilities: &[Ability]) -> bool {
    abilities.iter().any(|a| a.trigger != Trigger::Revelation)
}

/// Shared post-draw resolution helper. Frames the per-card 5-step
/// sub-sequence's steps 3 (Revelation) and 4 (disposition: treachery discard /
/// enemy spawn) for an already-drawn encounter card. Called by
/// `encounter_card_revealed` (the `EngineRecord::EncounterCardRevealed` path),
/// by the Mythos draw chain (`draw_encounter_card_into_frame`, driven by the
/// [`PlayerDraw`](Continuation::PlayerDraw) frame), and by card effects that
/// draw from the encounter deck (agenda 01106's reverse).
///
/// Body (#423): emits [`Event::CardRevealed`], then pushes a
/// [`Continuation::EncounterCard`] disposition frame (treachery → `Discard`,
/// enemy → `Spawn`) and the card's [`Trigger::Revelation`] effects (combined
/// into one `Seq`, via `push_effect`), returning [`EngineOutcome::Done`] for
/// the global `drive` loop to step. The loop resolves the Revelation, then
/// disposes of the card via `dispose_encounter_card_if_top` — discarding a
/// one-shot treachery or spawning the enemy (which may itself suspend on an
/// engagement tie). Any other card type rejects.
///
/// **Mid-resolution caveat:** [`Event::CardRevealed`] emits before Revelation
/// resolves (Before-timing reactions need that ordering, per #126's design
/// decision). The apply loop's `events.clear()` on Rejected still wipes the
/// event stream on rejection.
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

    // Treachery and enemy both: push the disposition frame BEFORE the
    // Revelation, then push the Revelation effects for the `drive` loop to own
    // (#423). The framework disposes of the card via
    // `dispose_encounter_card_if_top` once the Revelation's whole
    // sub-resolution completes — even if it suspends into a skill test or a
    // choice (#380). A mid-Revelation `Rejected` is rolled back by the apply
    // loop's transactional snapshot, this frame included.
    let disposition = match card_type {
        CardType::Treachery => EncounterDisposition::Discard,
        CardType::Enemy => EncounterDisposition::Spawn { investigator },
        other => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "EncounterCardRevealed: invalid encounter card type {other:?}; \
                     encounter decks contain only treachery and enemy cards",
                )
                .into(),
            };
        }
    };

    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "encounter card resolution: no card registry installed".into(),
        };
    };
    let abilities = (registry.abilities_for)(&code).unwrap_or_default();

    // Revelation effects on enemies (rare, but printed on some encounter
    // enemies — e.g. "Revelation - Discard 1 card from your hand at random.")
    // fire BEFORE the enemy spawns into play, per Rules Reference p.24 ("1.4
    // Each investigator draws 1 encounter card"): "3. Resolve the revelation
    // ability on the drawn card." then "4. If the card is an enemy, spawn it
    // following any spawn instruction the card bears." The spawn happens at
    // disposal, after the Revelation frames the loop drives have all resolved.
    let revelation_effects: Vec<Effect> = abilities
        .into_iter()
        .filter(|a| a.trigger == Trigger::Revelation)
        .map(|a| a.effect)
        .collect();

    cx.state.continuations.push(EncounterCardFrame {
        card: code,
        disposition,
    });

    // Push the Revelation effects (combined into one `Seq`) for the global
    // `drive` loop to step; push nothing when there are none (the disposal
    // frame is then top and the loop disposes immediately). The drawing
    // investigator controls the Revelation; the card is the encounter deck's.
    if !revelation_effects.is_empty() {
        let eval_ctx = EvalContext::for_revelation(investigator, Owner::EncounterDeck);
        evaluator::push_effect(cx, &Effect::Seq(revelation_effects), eval_ctx);
    }
    EngineOutcome::Done
}

/// Spawn one encounter-deck enemy into play.
///
/// Called by [`encounter_card_revealed`] after `Event::CardRevealed`
/// has fired and any [`Trigger::Revelation`](card_dsl::dsl::Trigger::Revelation)
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
/// `PickSingle`. When the spawn happens inside a Mythos
/// encounter-draw chain, [`resume_spawn_engage`] continues the drawer's
/// [`PlayerDraw`](crate::state::Continuation::PlayerDraw) chain after the pick
/// resolves.
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
/// Spawn-location resolution is checked before any mutation. It has four
/// outcomes: (1) a legal location → mint + engage (below); (2) a `Specific`
/// location not in play → the enemy does **not** spawn and its card is placed
/// in `encounter_discard`, returning `Done` (RR p.24 / Flesh-Eater 01118 FAQ,
/// #517) — this is the only mutating non-spawn path; (3) a
/// [`SpawnLocation::Unrepresented`] clause → `Rejected` with `state`/`events`
/// unchanged, because the enemy prints a spawn instruction we cannot model and
/// silently falling back to the no-instruction rule would apply a *different*
/// rule (#635); (4) the default spawn with
/// no drawing-investigator location → `Rejected` with `state`/`events`
/// unchanged (a state-corruption invariant, not the RR "no legal location"
/// case). Engagement resolution never rejects: it either resolves inline or
/// suspends (`AwaitingInput`) with the enemy already minted into `state.enemies`
/// and `Event::EnemySpawned` pushed (the pending choice carries the rest of the
/// work to [`resume_spawn_engage`]).
///
/// # Unmodelled spawn instructions block the draw
///
/// Outcome (3) is a hard stop, not a skip, and that is worth stating plainly:
/// `apply_via` restores the pristine state on `Rejected`, so the offending card
/// stays on top of the encounter deck and the *same* draw re-rejects rather
/// than failing once and moving on. Ten corpus enemies are consequently
/// undrawable — Acolyte 01169, Wizard of the Order 01170 and Servant of Many
/// Mouths 02224 ("Any empty location"), The Masked Hunter 01121b, Thrall 02086,
/// Lupine Thrall 02095, Emergent Monstrosity 02183, Crazed Shoggoth 02295 and
/// Interstellar Traveler 02329 (one distinct clause each), plus Ruth Turner
/// 01141, whose clause is a plain fixed-location spawn the *parser* truncates
/// (#575, coordinated with #578 — not a missing shape).
///
/// No shipped scenario seeds any of them today, and #670 removes the condition
/// by modelling the shapes. The alternative — treating an unmodelled clause as
/// the RR "no legal location" discard — was rejected deliberately: discarding
/// is itself a rule with an observable effect on the encounter deck, and
/// applying it because *we* cannot read the card would be the same
/// wrong-rule-silently mistake in a new place (#635).
fn spawn_enemy(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // Resolve the spawn location (validate-first). Only the card's `spawn`
    // rule is read here; the full stat read + mint happens in
    // [`spawn_enemy_at`]. A `Specific` spawn names an in-play location; an
    // `Unrepresented` one refuses; the default rule (`None`) spawns at the
    // drawing investigator's location.
    let CardKind::Enemy { spawn, .. } = &metadata.kind else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_enemy: card {code} is not an enemy").into(),
        };
    };
    let location_id = match spawn {
        Some(Spawn {
            location: SpawnLocation::Specific(loc_code),
        }) => {
            if let Some((id, _)) = cx
                .state
                .locations
                .iter()
                .find(|(_, loc)| loc.code.as_str() == loc_code.as_str())
            {
                *id
            } else {
                // Rules Reference p.24: "If an enemy has no legal location to
                // spawn at (for example, if its spawn instruction directs it to
                // a specific location that is not in play …), it does not spawn,
                // and is discarded instead." Flesh-Eater 01118 FAQ: "place that
                // enemy card into the encounter discard pile without any further
                // effects." So the draw resolves (`Done`) rather than rejecting:
                // the enemy never enters play and its card goes to the encounter
                // discard. Silent, matching the treachery `Discard` disposal
                // arm. (#517.)
                cx.state.encounter_discard.push(code);
                return EngineOutcome::Done;
            }
        }
        Some(Spawn {
            location: SpawnLocation::Unrepresented(clause),
        }) => {
            // The card prints a spawn instruction we cannot model. Refusing
            // here is the point of the variant (#635): the alternative — the
            // `None` arm below — is the *positive* rule for an enemy with no
            // Spawn line at all, and applying it to, say, Acolyte 01169
            // ("Spawn - Any empty location.") would place the enemy at the one
            // location guaranteed not to be empty and engage the drawer. Loud
            // refusal, matching how `PlayCard` treats an unimplemented card.
            return EngineOutcome::Rejected {
                reason: format!(
                    "TODO(#670): card {code} prints spawn instruction {clause:?}, which \
                     needs a modelled SpawnLocation variant and the spawning \
                     investigator's choice among valid locations",
                )
                .into(),
            };
        }
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
    spawn_enemy_at(cx, code, metadata, location_id, Owner::EncounterDeck)
}

/// Mint an enemy from `metadata` at an explicit `location_id`, resolving
/// engagement-on-spawn (prey). The reusable spawn core: [`spawn_enemy`]
/// supplies a location from the card's own spawn rule;
/// [`put_set_aside_card_into_play`](super::set_aside::put_set_aside_card_into_play)
/// supplies a location named by the bringing effect (The Gathering's Act-2
/// reverse spawns the Ghoul Priest in the Hallway). The engagement
/// candidates come from `location_id` itself.
///
/// `owner` is the enemy's [`Owner`], stated by the caller: the encounter deck
/// for an enemy drawn from it, and whichever owner a set-aside enemy has.
#[allow(clippy::too_many_lines)]
pub(super) fn spawn_enemy_at(
    cx: &mut Cx,
    code: CardCode,
    metadata: &CardMetadata,
    location_id: LocationId,
    owner: Owner,
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
    //    `PickSingle` (option A).
    let candidates = cursor::active_investigators_at(cx.state, location_id);

    // 3. Mint and place (mutate-second). The enemy is inserted unengaged;
    //    the `One` and (post-resume) `Tie` cases set `engaged_with` via
    //    `engage_enemy_with` so the `EnemyEngaged` event always pairs with
    //    the mutation.
    let enemy_id = cx.state.enemy_ids.mint();

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
        attachments: Vec::new(),
        owner,
    };
    cx.state.enemies.insert(enemy_id, enemy);

    match hunters::resolve_prey(cx.state, prey, &candidates) {
        PreyResolution::None => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: None,
            });
            EngineOutcome::Done
        }
        PreyResolution::One(target) => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: Some(target),
            });
            hunters::engage_enemy_with(cx, enemy_id, target);
            EngineOutcome::Done
        }
        PreyResolution::Tie(tied) => {
            cx.events.push(Event::EnemySpawned {
                enemy: enemy_id,
                code,
                location: location_id,
                engaged_with: None,
            });
            // The surge/chain state lives on the drawer's `PlayerDraw` frame
            // beneath (callsite-migration); this frame holds only the engagement
            // pick. `resume_spawn_engage` engages + pops, and the loop continues
            // the chain through the exposed `PlayerDraw`.
            cx.state
                .continuations
                .push(Continuation::SpawnEngage(SpawnEngagePending {
                    enemy: enemy_id,
                    candidates: tied.clone(),
                }));
            EngineOutcome::AwaitingInput {
                request: InputRequest::pick_single(
                    format!(
                        "Enemy {enemy_id:?} spawn engagement: lead investigator picks whom to \
                         engage among {tied:?}"
                    ),
                    choice::candidate_options(cx.state, &tied, |i| {
                        cx.state.investigators[i].card_anchor()
                    }),
                ),
                resume_token: ResumeToken(0),
            }
        }
    }
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
/// the deck and the discard are empty, a state no rule defines: the
/// Rules Reference covers only the empty deck ("shuffle the encounter
/// discard pile back into the encounter deck"). Callers therefore
/// reject it or treat it as malformed scenario data; each documents
/// which.
pub(super) fn draw_encounter_top(cx: &mut Cx) -> Option<CardCode> {
    if cx.state.encounter_deck.is_empty() {
        if cx.state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(cx);
    }
    cx.state.encounter_deck.pop_front()
}

/// Push the prompt for the topmost [`Continuation::EncounterDraw`] frame's
/// current drawer (`remaining[0]`): an [`EngineOutcome::AwaitingInput`] whose
/// response is a binary [`Confirm`](InputResponse::Confirm) (the draw carries
/// no choice). Used by `mythos_phase` (first prompt) and
/// [`advance_encounter_draw`] (re-prompt after a queue pop). The frame must
/// already be on the stack; callers ensure this.
pub(super) fn prompt_encounter_draw(cx: &Cx) -> EngineOutcome {
    let drawer = cx
        .state
        .current_encounter_drawer()
        .expect("prompt_encounter_draw: no EncounterDraw frame on the stack");
    EngineOutcome::AwaitingInput {
        // Anchored to the encounter deck, which is what distinguishes this prompt
        // from the cosmetic skill-test acknowledge on the wire — both are
        // option-less `Confirm`s otherwise (ADR 0011, #541).
        request: InputRequest::confirm(format!(
            "Mythos step 1.4: {drawer:?} draws an encounter card; submit InputResponse::Confirm.",
        ))
        .at(OptionTarget::EncounterDeck),
        resume_token: ResumeToken(0),
    }
}

/// Resume the Mythos step-1.4 encounter-draw loop (#348), driving the topmost
/// [`Continuation::EncounterDraw`] frame.
///
/// The acting drawer is the frame's `remaining[0]` (Rules Reference p.24
/// player order) — the response is a binary [`Confirm`](InputResponse::Confirm)
/// (the draw carries no choice). On `Confirm`, pushes a fresh
/// [`PlayerDraw`](Continuation::PlayerDraw) frame *above* the loop frame for that
/// drawer's surge chain and returns [`EngineOutcome::Done`]; the `drive` loop's
/// `PlayerDraw` arm then draws the first card. The chain may suspend on a
/// mid-chain spawn-engagement tie (pushing a
/// [`SpawnEngage`](Continuation::SpawnEngage) frame above the `PlayerDraw`),
/// resumed by [`resume_spawn_engage`](super::hunters::resume_spawn_engage). When
/// the chain ends, the `PlayerDraw` frame pops and [`advance_encounter_draw`]
/// re-prompts the next drawer, or — when drained — pops the loop frame and opens
/// the post-1.4 `MythosAfterDraws` window. Rejections leave state untouched.
pub(super) fn resume_encounter_draw(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let drawer = cx
        .state
        .continuations
        .top_expect::<EncounterDrawFrame>()
        .remaining[0];
    if !matches!(response, InputResponse::Confirm) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: Mythos encounter draw expects InputResponse::Confirm, got {response:?}",
            )
            .into(),
        };
    }
    // Push a fresh per-drawer surge-chain frame above the loop frame and let the
    // `drive` loop's `PlayerDraw` arm draw the first card (chain_count == 0).
    // Surge recursion and the loop advance happen on that frame, not here
    // (callsite-migration).
    cx.state.continuations.push(PlayerDrawFrame {
        investigator: drawer,
        chain_count: 0,
        surge_pending: false,
    });
    EngineOutcome::Done
}

/// Drive one step of the topmost [`Continuation::PlayerDraw`] frame (the
/// `drive` loop's `PlayerDraw` arm). The frame owns one drawer's Mythos surge
/// chain (callsite-migration):
///
/// - On the first step (`chain_count == 0`) or when the last-drawn card surged
///   (`surge_pending`), draw the next card via [`draw_encounter_card_into_frame`]
///   — which bumps `chain_count`, enforces [`MAX_SURGE_CHAIN`], runs the peril
///   check, records `surge_pending` for the next step, and pushes the card's
///   disposition + Revelation frames for the loop to resolve. The
///   [`EncounterCard`](Continuation::EncounterCard) disposal re-exposes this
///   `PlayerDraw` frame, so the chain continues here.
/// - Otherwise (resumed, no pending surge) the chain is over: pop this frame and
///   [`advance_encounter_draw`] moves the loop to the next drawer / opens the
///   post-1.4 window.
///
/// Never awaits input itself (mirrors [`Continuation::EncounterCard`]); a draw
/// may suspend on a spawn-engagement tie or reject — propagated to the caller.
pub(super) fn drive_player_draw(cx: &mut Cx) -> EngineOutcome {
    let PlayerDrawFrame {
        investigator,
        chain_count,
        surge_pending,
    } = *cx.state.continuations.top_expect::<PlayerDrawFrame>();
    if chain_count == 0 || surge_pending {
        draw_encounter_card_into_frame(cx, investigator)
    } else {
        // Chain over: drop this drawer's PlayerDraw frame and advance the loop.
        cx.state.continuations.pop_expect::<PlayerDrawFrame>();
        advance_encounter_draw(cx)
    }
}

/// Draw one card into an [`Continuation::EncounterCard`] frame for the global
/// `drive` loop to resolve (callsite-migration). The shared per-card prelude of
/// the Mythos surge chain: bump the topmost
/// [`Continuation::PlayerDraw`] frame's `chain_count`, enforce
/// [`MAX_SURGE_CHAIN`], [`draw_encounter_top`], run the peril check, record the
/// drawn card's `surge` bit back onto the `PlayerDraw` frame (so the next
/// [`drive_player_draw`] step knows whether to draw again), then push the card's
/// disposition + Revelation frames via [`resolve_encounter_card`]. Returns its
/// outcome (`Done` with frames pushed, or a registry/empty-deck reject).
/// An empty deck and discard rejects only on the chain's first card; past
/// it, the draw panics as malformed scenario data.
///
/// Called only by [`drive_player_draw`] — the first draw and every surge
/// re-draw of a drawer's chain (including after a mid-chain engagement tie
/// resolves and the `PlayerDraw` frame is re-exposed). The `PlayerDraw` frame is
/// on top, with drawer `investigator`.
///
/// # Mid-chain rejection note
///
/// A reject after the draw is fully rolled back: `apply_via` restores the
/// pre-apply snapshot (deck, discard, RNG) on `Rejected`, so the drawn card
/// returns to `encounter_deck` — the draw-before-validate ordering only
/// matters on non-rejecting paths.
fn draw_encounter_card_into_frame(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let Some(reg) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no card registry installed".into(),
        };
    };

    // Bump + cap-check the live chain position. The drawer's `PlayerDraw` frame
    // is on top (its card-resolution frames are pushed above it next).
    let frame = cx.state.continuations.top_mut::<PlayerDrawFrame>();
    frame.chain_count += 1;
    let chain_count = frame.chain_count;
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

    // Record this card's `surge` bit back onto the PlayerDraw frame: the next
    // `drive_player_draw` step reads it to decide whether to draw again (surge)
    // or end the chain. Still the top frame — the draw only mutated the deck,
    // pushing nothing above it.
    let surges = metadata.surge();
    cx.state
        .continuations
        .top_mut::<PlayerDrawFrame>()
        .surge_pending = surges;

    // Step 2: Check for the peril keyword on the drawn card.
    skill_test::peril_check(cx, &code, investigator, metadata.peril());

    // Step 3 + 4: Push the disposition + Revelation frames; the `drive` loop
    // resolves them, then disposes of the card.
    resolve_encounter_card(cx, investigator, code, metadata)
}

/// Advance the encounter-draw loop after a completed chain (#348, replacing the
/// former `advance_mythos_draw_pending` cursor advance): drop the just-finished
/// drawer from the topmost [`Continuation::EncounterDraw`] frame, then skip any
/// now-eliminated investigators (an encounter card may have eliminated a later
/// drawer — Rules Reference p.10: eliminated investigators do not draw,
/// mirroring `next_active_investigator_after`'s skip). When a drawer remains,
/// re-prompt them ([`EngineOutcome::AwaitingInput`]); when the queue drains, pop
/// the frame and open the post-1.4 `MythosAfterDraws` window. Called only after
/// a chain completes, with the just-popped drawer's `PlayerDraw` frame already
/// gone and the `EncounterDraw` frame topmost.
pub(super) fn advance_encounter_draw(cx: &mut Cx) -> EngineOutcome {
    // The finished drawer's `PlayerDraw` frame has just been popped, so the
    // `EncounterDraw` loop frame is on top. Pull the queue out to advance it
    // without aliasing `state.investigators`.
    let mut queue = mem::take(
        &mut cx
            .state
            .continuations
            .top_mut::<EncounterDrawFrame>()
            .remaining,
    );
    queue.remove(0); // drop the finished drawer
    while let Some(&next) = queue.first() {
        if cx
            .state
            .investigators
            .get(&next)
            .is_some_and(|inv| inv.status == Status::Active)
        {
            break;
        }
        queue.remove(0); // skip a now-eliminated investigator (RR p.10)
    }
    if queue.is_empty() {
        cx.state.continuations.pop_expect::<EncounterDrawFrame>(); // the drained frame, on top
        let outcome = reaction_windows::open_fast_window(
            cx,
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        );
        debug_assert_eq!(
            outcome,
            EngineOutcome::Done,
            "open_fast_window(MythosAfterDraws) unexpectedly suspended; this window has no suspending continuation",
        );
        EngineOutcome::Done
    } else {
        // Write the advanced queue back and prompt the next drawer. The surge
        // budget is per-`PlayerDraw` now (a fresh frame is pushed on the next
        // drawer's Confirm), so there is nothing to reset here (callsite-migration).
        cx.state
            .continuations
            .top_mut::<EncounterDrawFrame>()
            .remaining = queue;
        prompt_encounter_draw(cx)
    }
}

/// If the top continuation frame is a [`Continuation::EncounterCard`], dispose
/// of its card per its [`EncounterDisposition`] and pop the frame (#380 /
/// callsite-migration). A no-op when no such frame is on top; returns
/// [`EngineOutcome::Done`] unless an enemy spawn suspends / rejects (propagated
/// immediately).
///
/// Disposal:
///
/// - `Discard` (treachery, Rules Reference p.18 default): a one-shot treachery
///   is discarded to `encounter_discard`; a **persistent** treachery (one
///   carrying a non-`Revelation` ability) placed itself during its Revelation
///   and owns its own disposition, so it is skipped. Persistence is re-derived
///   from the registry by card code — the frame stays payload-minimal (#380).
///   The discard is eventless.
/// - `Spawn` (enemy, RR p.24 step 4): re-derive the enemy metadata from the
///   registry and [`spawn_enemy`] at the drawer's location. The spawn may
///   suspend on an engagement tie ([`EngineOutcome::AwaitingInput`]) or reject;
///   either is returned immediately (the loop does not continue). It may also
///   return `Done` *without* minting an enemy — a `Specific` spawn location not
///   in play discards the card to `encounter_discard` (RR p.24, #517) — in which
///   case the loop continues past the (already-popped) frame as usual.
///
/// After disposal the frame is gone and the loop re-dispatches whatever is
/// beneath: a [`PlayerDraw`](Continuation::PlayerDraw) frame (Mythos chain →
/// `drive_player_draw` continues / ends it), or nothing / another frame
/// (engine-record reveal, agenda reverse-draw → done). The `while` keeps
/// draining any further stacked `EncounterCard` frames.
///
/// Called from the `drive` loop's [`Continuation::EncounterCard`] arm once a
/// Revelation's whole sub-resolution completes and the frame is top again.
pub(super) fn dispose_encounter_card_if_top(cx: &mut Cx) -> EngineOutcome {
    while cx
        .state
        .continuations
        .top_of::<EncounterCardFrame>()
        .is_some()
    {
        let EncounterCardFrame { card, disposition } =
            cx.state.continuations.pop_expect::<EncounterCardFrame>();

        match disposition {
            EncounterDisposition::Discard => {
                let persistent = card_registry::current()
                    .and_then(|reg| (reg.abilities_for)(&card))
                    .is_some_and(|abilities| treachery_is_persistent(&abilities));
                if !persistent {
                    cx.state.encounter_discard.push(card.clone());
                }
            }
            EncounterDisposition::Spawn { investigator } => {
                let Some(metadata) =
                    card_registry::current().and_then(|reg| (reg.metadata_for)(&card))
                else {
                    return EngineOutcome::Rejected {
                        reason: format!("encounter enemy disposal: no metadata for card {card:?}")
                            .into(),
                    };
                };
                match spawn_enemy(cx, investigator, card.clone(), metadata) {
                    EngineOutcome::Done => {}
                    // An engagement tie suspended (or a reject): propagate
                    // immediately rather than continue the loop. A mid-Mythos
                    // tie leaves a `SpawnEngage` frame above the drawer's
                    // `PlayerDraw`, and `resume_spawn_engage` continues the chain
                    // after the pick.
                    other => return other,
                }
            }
        }
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests;
