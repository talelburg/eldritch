//! Encounter-deck draw, spawn, and Mythos draw chain handlers.

use std::mem;

use card_dsl::card_data::{CardKind, CardMetadata, CardType, Spawn, SpawnLocation};
use card_dsl::dsl::{Ability, Effect, Trigger};

use crate::action::InputResponse;
use crate::card_registry::CardRegistry;
use crate::engine::board;
use crate::engine::dispatch::emit::{self, TimingEvent};
use crate::engine::dispatch::hunters::PreyResolution;
use crate::engine::dispatch::{
    choice, cursor, engagement, hunters, prompt, reaction_windows, skill_test,
};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::{EngineOutcome, InputRequest, OptionTarget};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{
    CardCode, Continuation, EncounterCardFrame, EncounterDisposition, EncounterDrawFrame, Enemy,
    FastWindowKind, GameState, InvestigatorId, LocationId, Owner, PhaseStep, PlayerDrawFrame,
    SpawnEngagePending, Status, Zone,
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
/// 1. Draw the top of the encounter deck via [`draw_encounter_top`]
///    (transparently reshuffles discard back in if the deck is
///    empty). Reject with `"EncounterCardRevealed: encounter deck and discard both empty"`
///    if both piles are exhausted.
/// 2. Look up the drawn card's metadata via the card registry.
///    Reject with `"EncounterCardRevealed: unknown card code: {code}"` if the registry
///    doesn't know the code.
/// 3. Delegate to [`resolve_encounter_card`] for the post-draw
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
    let registry = cx.cards;

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
    resolve_encounter_card(cx, investigator, code, metadata, Owner::EncounterDeck)
}

/// A treachery is **persistent** (stays in play after its Revelation,
/// owning its own disposition) iff it has at least one ability whose
/// trigger is not [`Trigger::Revelation`] — the ongoing `Constant`
/// restriction / `OnEvent` forced-discard abilities a persistent treachery
/// carries. One-shot treacheries have only a `Revelation`, so
/// they auto-discard after it resolves.
///
/// This assumes every persistent treachery carries an ongoing ability
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
/// draw from the encounter deck (agenda 01106's reverse, Drawn to the Flame
/// 01064).
///
/// Body: emits [`Event::CardRevealed`], pushes a
/// [`Continuation::EncounterCard`] disposition frame (treachery → `Discard`,
/// enemy → `Spawn`), then announces the draw,
/// [`TimingEvent::EncounterCardDrawn`], in tail position (ADR 0003). The
/// revelation is that condition's resolve step ([`resolve_revelation`]),
/// so a `when` ability can cancel it (Ward of Protection 01065); the disposal
/// frame beneath the condition still runs once the sequence has drained,
/// through `dispose_encounter_card_if_top` — discarding a one-shot treachery
/// or spawning the enemy (which may itself suspend on an engagement tie). Any
/// other card type rejects.
///
/// **Mid-resolution caveat:** [`Event::CardRevealed`] emits before Revelation
/// resolves (Before-timing reactions need that ordering). The apply loop's
/// `events.clear()` on Rejected still wipes the
/// event stream on rejection.
///
/// Card effects that "draw"/"discard until" cards from the encounter deck
/// resolve the drawn card through here, as
/// [`NativeCx::resolve_encounter_card`](crate::engine::NativeCx::resolve_encounter_card)
/// — agenda 01106's reverse draws the dug-up `Ghoul` enemy this way.
///
/// # The owner
///
/// `owner` is the card's [`Owner`]: [`Owner::EncounterDeck`] for every card
/// drawn from the encounter deck, and the bearer for an encounter-cardtype
/// weakness drawn from a player deck — `glossary/Weakness.md`: *"When an
/// investigator draws a weakness with an encounter cardtype (for example, an
/// enemy or a treachery weakness), resolve that card as if it were just drawn
/// from the encounter deck."* It rides the frame to the three readers that
/// need it: the Revelation's binding, the spawned enemy, and a one-shot's
/// discard pile. The `EncounterCardDrawn` timing point doesn't carry it: no
/// trigger reads it, and Ward of Protection 01065's *"non-weakness"* filter
/// reads the card itself.
pub(crate) fn resolve_encounter_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
    owner: Owner,
) -> EngineOutcome {
    // Emit BEFORE Revelation resolves — see caveat in encounter_card_revealed.
    cx.events.push(Event::CardRevealed {
        investigator,
        code: code.clone(),
        card_type: metadata.card_type(),
    });
    begin_encounter_card(cx, investigator, code, metadata.card_type(), owner)
}

/// [`resolve_encounter_card`] after its [`Event::CardRevealed`]: push the
/// card's disposition frame and announce the draw. Split out for a player
/// draw of several weaknesses, which reveals every card before resolving the
/// first (see `cards::resolve_drawn_weaknesses`).
pub(super) fn begin_encounter_card(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    card_type: CardType,
    owner: Owner,
) -> EngineOutcome {
    // Treachery and enemy both: push the disposition frame BEFORE the draw
    // condition whose resolve step is the Revelation. The
    // framework disposes of the card via `dispose_encounter_card_if_top` once
    // the condition's whole sequence completes — even if the Revelation
    // suspends into a skill test or a choice, and even if a `when`
    // ability cancelled it. A mid-Revelation `Rejected` is rolled back by the
    // apply loop's transactional snapshot, this frame included.
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

    cx.state.continuations.push(EncounterCardFrame {
        card: Some(code.clone()),
        owner,
        disposition,
        revelation_resolved: false,
    });
    emit::queue_event(cx, &TimingEvent::EncounterCardDrawn { investigator, code })
}

/// Resolve the drawn card `code`'s Revelation for `investigator`: the resolve
/// step of [`TimingEvent::EncounterCardDrawn`], reached only when no `when`
/// ability cancelled it.
///
/// Marks the drawn card's [`EncounterCardFrame`] — the topmost one, directly
/// beneath the draw condition's coordinator — as revealed, then pushes the
/// card's [`Trigger::Revelation`] effects, combined into one `Seq`, for the
/// global `drive` loop to step. Nothing is pushed when there are none.
///
/// Revelation effects on enemies (rare, but printed on some encounter enemies
/// — e.g. Coven Initiate 05090's "Revelation - Discard the top 2 cards of the
/// encounter deck. …") fire BEFORE the enemy spawns into play, per Rules
/// Reference p.24 ("1.4 Each investigator draws 1 encounter card"): "3.
/// Resolve the revelation ability on the drawn card." then "4. If the card is
/// an enemy, spawn it following any spawn instruction the card bears." The
/// spawn happens at disposal, after the condition's sequence has drained. The
/// drawing investigator controls the
/// Revelation; the card is the frame's `owner`'s — the encounter deck's, or the
/// bearer's for a drawn weakness.
///
/// Nothing resolves for a weakness elimination has already taken off the frame
/// and removed from the game (its bearer's deck-out horror defeated them
/// before its Revelation came round): the game has ended for its bearer.
pub(super) fn resolve_revelation(cx: &mut Cx, investigator: InvestigatorId, code: &CardCode) {
    let frame = cx
        .state
        .continuations
        .topmost_of_mut::<EncounterCardFrame>()
        .expect("resolve_revelation: the draw condition sits above its EncounterCard frame");
    let Some(card) = &frame.card else {
        return;
    };
    debug_assert_eq!(
        card, code,
        "resolve_revelation: the topmost EncounterCard frame is not the drawn card's"
    );
    frame.revelation_resolved = true;
    let owner = frame.owner;
    push_revelation(cx, investigator, code, owner);
}

/// Push `code`'s [`Trigger::Revelation`] effects, combined into one `Seq`,
/// controlled by `investigator` and owned by `owner`, for the global `drive`
/// loop to step. Nothing is pushed when the card prints none.
///
/// The one place a Revelation's effects are collected: an encounter card's
/// ([`resolve_revelation`]) and a player-cardtype weakness's, which resolves
/// from its bearer's hand (`cards::resolve_drawn_weaknesses`).
pub(super) fn push_revelation(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: &CardCode,
    owner: Owner,
) {
    let revelation_effects: Vec<Effect> = (cx.cards.abilities_for)(code)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.trigger == Trigger::Revelation)
        .map(|a| a.effect)
        .collect();
    if !revelation_effects.is_empty() {
        let eval_ctx = EvalContext::for_revelation(investigator, owner);
        evaluator::push_effect(cx, &Effect::Seq(revelation_effects), eval_ctx);
    }
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
/// An enemy with no Spawn instruction engages its drawer outright, without
/// consulting its prey (`glossary/Spawn.md`: *"If the enemy does not have a
/// "Spawn –" instruction, the investigator drawing the enemy spawns it engaged
/// with him/her, unless it is aloof."*).
///
/// A `Specific` spawn routes through the shared [`resolve_prey`] resolver:
/// the co-located set is narrowed by the enemy's
/// prey. `None`/`One` resolve inline (no engagement, or engage the
/// sole/best candidate); `Tie` pushes a
/// [`SpawnEngagePending`](crate::state::SpawnEngagePending) frame and returns
/// `Done`, and the `drive` loop surfaces its tie-break `PickSingle`, which the
/// rules give the lead investigator (**Lead investigator** in `GLOSSARY.md`);
/// the request carries no recipient. When the spawn happens inside a Mythos
/// encounter-draw chain, [`resume_spawn_engage`] continues the drawer's
/// [`PlayerDraw`](crate::state::Continuation::PlayerDraw) chain after the pick
/// resolves.
///
/// # Stats
///
/// The enemy's printed stats come from its `CardKind::Enemy` metadata. A
/// missing health is 1, and a per-investigator health is multiplied by the
/// number of investigators in the game.
///
/// # Validate-first contract
///
/// Spawn-location resolution is checked before any mutation. It has four
/// outcomes: (1) a legal location → mint + engage (below); (2) a `Specific`
/// location not in play → the enemy does **not** spawn and its card is placed
/// in its owner's discard pile, returning `Done` (RR p.24 / Flesh-Eater 01118
/// FAQ) — this is the only mutating non-spawn path; (3) a
/// [`SpawnLocation::Unrepresented`] clause → `Rejected` with `state`/`events`
/// unchanged, because the enemy prints a spawn instruction we cannot model and
/// silently falling back to the no-instruction rule would apply a *different*
/// rule; (4) the default spawn with
/// no drawing-investigator location → `Rejected` with `state`/`events`
/// unchanged (a state-corruption invariant, not the RR "no legal location"
/// case). Engagement resolution never rejects: it either resolves inline or
/// pushes its tie-break prompt and returns `Done`, with the enemy already minted
/// into `state.enemies` and `Event::EnemySpawned` pushed (the pending choice
/// carries the rest of the work to [`resume_spawn_engage`]).
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
/// (not a missing shape; TODO(#575): parse the clause past its first period).
///
/// No shipped scenario seeds any of them today. TODO(#670): model the shapes,
/// which removes the condition. The alternative — treating an unmodelled clause as
/// the RR "no legal location" discard — was rejected deliberately: discarding
/// is itself a rule with an observable effect on the encounter deck, and
/// applying it because *we* cannot read the card would be the same
/// wrong-rule-silently mistake in a new place.
fn spawn_enemy(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
    owner: Owner,
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
    let (location_id, engages) = match spawn {
        Some(Spawn {
            location: SpawnLocation::Specific(loc_code),
        }) => {
            if let Some((id, _)) = cx
                .state
                .locations
                .iter()
                .find(|(_, loc)| loc.code.as_str() == loc_code.as_str())
            {
                (*id, SpawnEngagement::Prey)
            } else {
                // Rules Reference p.24: "If an enemy has no legal location to
                // spawn at (for example, if its spawn instruction directs it to
                // a specific location that is not in play …), it does not spawn,
                // and is discarded instead." Flesh-Eater 01118 FAQ: "place that
                // enemy card into the encounter discard pile without any further
                // effects." So the draw resolves (`Done`) rather than rejecting:
                // the enemy never enters play and its card goes to its owner's
                // discard pile, with its event, like a one-shot treachery's
                // disposal.
                board::discard_drawn_card(cx, code, owner, drawn_from(owner));
                return EngineOutcome::Done;
            }
        }
        Some(Spawn {
            location: SpawnLocation::Unrepresented(clause),
        }) => {
            // The card prints a spawn instruction we cannot model. Refusing
            // here is the point of the variant: the alternative — the
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
        // `glossary/Spawn.md`: *"If the enemy does not have a "Spawn –"
        // instruction, the investigator drawing the enemy spawns it engaged
        // with him/her, unless it is aloof."* The drawer is named outright, so
        // the enemy's prey is not consulted and two investigators at the
        // drawer's location do not tie. TODO(#653): an aloof enemy
        // spawns unengaged instead.
        None => match cx
            .state
            .investigators
            .get(&investigator)
            .and_then(|inv| inv.current_location)
        {
            Some(loc) => (loc, SpawnEngagement::Drawer(investigator)),
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
    spawn_enemy_engaging(cx, code, metadata, location_id, owner, engages)
}

/// How a spawning enemy finds the investigator it engages.
#[derive(Clone, Copy)]
enum SpawnEngagement {
    /// The investigator who drew it: an enemy with no Spawn instruction.
    Drawer(InvestigatorId),
    /// The engagement rule among the investigators at its location, narrowed
    /// by its prey: an enemy spawned *at* a location.
    Prey,
}

/// The deck a drawn card of `owner`'s came from, named as the `from` of its
/// [`Event::CardDiscarded`] once it is disposed of: a card resolving in limbo
/// is in no zone of its own (GLOSSARY **Limbo**), so the event names where it
/// was drawn from. The encounter deck for its own cards, and the bearer's deck
/// for a drawn weakness.
fn drawn_from(owner: Owner) -> Zone {
    match owner {
        Owner::Investigator(_) => Zone::Deck,
        Owner::EncounterDeck | Owner::Scenario => Zone::EncounterDeck,
    }
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
pub(super) fn spawn_enemy_at(
    cx: &mut Cx,
    code: CardCode,
    metadata: &CardMetadata,
    location_id: LocationId,
    owner: Owner,
) -> EngineOutcome {
    spawn_enemy_engaging(
        cx,
        code,
        metadata,
        location_id,
        owner,
        SpawnEngagement::Prey,
    )
}

/// Mint an enemy from `metadata` at `location_id` and engage it as
/// `engages` says: with its drawer, or by prey among the investigators
/// there. The body of [`spawn_enemy`] and [`spawn_enemy_at`].
fn spawn_enemy_engaging(
    cx: &mut Cx,
    code: CardCode,
    metadata: &CardMetadata,
    location_id: LocationId,
    owner: Owner,
    engages: SpawnEngagement,
) -> EngineOutcome {
    // Mint and place. The enemy is inserted unengaged; the engagement
    // below goes through `engagement::engage`, so the `EnemyEngaged` event
    // always pairs with the mutation. Only an enemy card reaches here, and a
    // rejection rolls the minted id back with the rest of the state.
    let enemy_id = cx.state.enemy_ids.mint();
    let count = cursor::investigator_count(cx.state);
    let Some(enemy) = Enemy::from_metadata(enemy_id, metadata, Some(location_id), owner, count)
    else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_enemy_at: card {code} is not an enemy").into(),
        };
    };
    let prey = enemy.prey;
    cx.state.enemies.insert(enemy_id, enemy);

    // Resolve engagement-on-spawn. A drawer is engaged outright. Otherwise the
    // co-located set is narrowed by the enemy's `prey`; with `Prey::Default` a
    // 2+ set ties and suspends for a tie-break `PickSingle`.
    let resolution = match engages {
        SpawnEngagement::Drawer(drawer) => PreyResolution::One(drawer),
        SpawnEngagement::Prey => {
            let candidates = engagement::candidates(cx.state, enemy_id);
            hunters::resolve_prey(cx.state, cx.cards, prey, &candidates)
        }
    };
    // The spawn is logged before the engagement or the tie-break suspension
    // that follows it.
    let engaged_with = match &resolution {
        PreyResolution::One(target) => Some(*target),
        PreyResolution::None | PreyResolution::Tie(_) => None,
    };
    cx.events.push(Event::EnemySpawned {
        enemy: enemy_id,
        code,
        location: location_id,
        engaged_with,
    });
    match resolution {
        PreyResolution::None => EngineOutcome::Done,
        PreyResolution::One(target) => {
            engagement::engage(cx, enemy_id, target);
            EngineOutcome::Done
        }
        PreyResolution::Tie(tied) => {
            // The surge/chain state lives on the drawer's `PlayerDraw` frame
            // beneath; this frame holds only the engagement pick.
            // `resume_spawn_engage` engages + pops, and the loop continues the
            // chain through the exposed `PlayerDraw`. Pushed and returned from:
            // the `drive` loop surfaces it, so a native whose exit spawned the
            // enemy just returns and the prompt follows it.
            cx.state
                .continuations
                .push(Continuation::SpawnEngage(SpawnEngagePending {
                    enemy: enemy_id,
                    candidates: tied,
                }));
            EngineOutcome::Done
        }
    }
}

/// What a [`SpawnEngage`](Continuation::SpawnEngage) frame asks: whom the
/// spawned enemy engages among its tied candidates, each anchored to the
/// investigator's card. One of the [prompt table](prompt::request)'s helpers.
pub(super) fn spawn_engage_request(
    state: &GameState,
    cards: &CardRegistry,
    pending: &SpawnEngagePending,
) -> InputRequest {
    let SpawnEngagePending {
        enemy: enemy_id,
        candidates: tied,
    } = pending;
    InputRequest::pick_single(
        format!(
            "The lead investigator chooses whom {} engages.",
            choice::enemy_name(state, *enemy_id),
        ),
        choice::candidate_options(state, cards, tied, |i| state.investigators[i].card_anchor()),
    )
}

/// Fisher-Yates shuffle of the shared encounter deck using the
/// shared deterministic RNG. Used by [`encounter_deck_shuffled`] and
/// by [`reshuffle_encounter_discard`].
///
/// Emits [`Event::EncounterDeckShuffled`] iff the deck had at least
/// 2 cards (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_encounter_deck(cx: &mut Cx) {
    if cx
        .state
        .rng
        .shuffle(cx.state.encounter_deck.make_contiguous())
    {
        cx.events.push(Event::EncounterDeckShuffled);
    }
}

/// Drain `state.encounter_discard` into `state.encounter_deck` and
/// shuffle the resulting deck. Called by `draw_encounter_top` when the
/// deck runs empty, and by card effects that "Shuffle the encounter discard
/// pile into the encounter deck." (agenda 01106's reverse).
///
/// Does NOT push an `EngineRecord::EncounterDeckShuffled` to the
/// action log — mid-handler reshuffles rely on RNG determinism for
/// replay rather than log entries, mirroring the existing
/// player-deck pattern. The `EngineRecord` variant is reserved for
/// explicit shuffle actions (future "shuffle X into the encounter
/// deck" effects).
pub(crate) fn reshuffle_encounter_discard(cx: &mut Cx) {
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
/// reject it, treat it as malformed scenario data, or draw nothing; each
/// documents which. A card-local native reaches it as
/// [`NativeCx::draw_encounter_top`](crate::engine::NativeCx::draw_encounter_top).
pub(crate) fn draw_encounter_top(cx: &mut Cx) -> Option<CardCode> {
    if cx.state.encounter_deck.is_empty() {
        if cx.state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(cx);
    }
    take_encounter_top(cx)
}

/// Take the top card of the encounter deck, or `None` if the deck is empty.
/// A card-local native reaches it as
/// [`NativeCx::take_encounter_top`](crate::engine::NativeCx::take_encounter_top).
///
/// Unlike the encounter draw (`draw_encounter_top`), this never reshuffles the discard into an
/// empty deck: a deck search (Rise of the Ghouls 01106) discards what it takes,
/// and a reshuffle would feed those discards back into the search. The caller
/// owns the card it takes and must put it somewhere, as
/// [`discard_drawn_card`](crate::engine::board::discard_drawn_card)
/// does for a miss.
pub(crate) fn take_encounter_top(cx: &mut Cx) -> Option<CardCode> {
    cx.state.encounter_deck.pop_front()
}

/// What the topmost [`Continuation::EncounterDraw`] frame asks of its current
/// drawer (`remaining[0]`): a binary [`Confirm`](InputResponse::Confirm) (the
/// draw carries no choice). One of the [prompt table](prompt::request)'s
/// helpers.
pub(super) fn encounter_draw_request(state: &GameState) -> InputRequest {
    let drawer = state
        .current_encounter_drawer()
        .expect("encounter_draw_request: no EncounterDraw frame on the stack");
    // Anchored to the encounter deck, which is what distinguishes this prompt
    // from the cosmetic skill-test acknowledge on the wire — both are
    // option-less `Confirm`s otherwise (ADR 0011).
    InputRequest::confirm(format!(
        "{} draws an encounter card.",
        choice::investigator_name(state, drawer),
    ))
    .at(OptionTarget::EncounterDeck)
}

/// Resume the Mythos step-1.4 encounter-draw loop, driving the topmost
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
    // Surge recursion and the loop advance happen on that frame, not here.
    cx.state.continuations.push(PlayerDrawFrame {
        investigator: drawer,
        chain_count: 0,
        surge_pending: false,
    });
    EngineOutcome::Done
}

/// Drive one step of the topmost [`Continuation::PlayerDraw`] frame (the
/// `drive` loop's `PlayerDraw` arm). The frame owns one drawer's Mythos surge
/// chain:
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
/// `drive` loop to resolve. The shared per-card prelude of
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
    let reg = cx.cards;

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
    resolve_encounter_card(cx, investigator, code, metadata, Owner::EncounterDeck)
}

/// Advance the encounter-draw loop after a completed chain: drop the just-finished
/// drawer from the topmost [`Continuation::EncounterDraw`] frame, then skip any
/// now-eliminated investigators (an encounter card may have eliminated a later
/// drawer — Rules Reference p.10: eliminated investigators do not draw,
/// mirroring `next_in_player_order`'s skip). When a drawer remains,
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
        // budget is per-`PlayerDraw` frame (a fresh frame is pushed on the next
        // drawer's Confirm), so there is nothing to reset here.
        cx.state
            .continuations
            .top_mut::<EncounterDrawFrame>()
            .remaining = queue;
        prompt::suspend(cx.state, cx.cards)
    }
}

/// If the top continuation frame is a [`Continuation::EncounterCard`], dispose
/// of its card per its [`EncounterDisposition`] and pop the frame. A no-op
/// when no such frame is on top; returns
/// [`EngineOutcome::Done`] unless an enemy spawn suspends / rejects (propagated
/// immediately).
///
/// Disposal:
///
/// - `Discard` (treachery, Rules Reference p.18 default): a one-shot treachery
///   is discarded to its owner's discard pile — the encounter discard, or a
///   drawn weakness's bearer's — emitting [`Event::CardDiscarded`]
///   `from` the deck it was drawn from; a **persistent** treachery (one
///   carrying a non-`Revelation` ability) placed itself during its Revelation
///   and owns its own disposition, so it is skipped. Persistence is re-derived
///   from the registry by card code — the frame stays payload-minimal.
/// - `Spawn` (enemy, RR p.24 step 4): re-derive the enemy metadata from the
///   registry and [`spawn_enemy`] at the drawer's location, owned by the
///   frame's owner. The spawn may
///   push an engagement tie, which ends the loop because the tie is then on
///   top, or reject, which is returned immediately. It may also
///   return `Done` *without* minting an enemy — a `Specific` spawn location not
///   in play discards the card to its owner's pile (RR p.24) — in which
///   case the loop continues past the (already-popped) frame as usual.
///
/// A frame whose card elimination has taken (a drawn weakness whose bearer was
/// defeated mid-resolution, [`Continuation::take_resolving_weakness`]) is
/// popped with nothing disposed of: the card is already removed from the game.
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
        let EncounterCardFrame {
            card,
            owner,
            disposition,
            revelation_resolved,
        } = cx.state.continuations.pop_expect::<EncounterCardFrame>();
        // Elimination took a bearer's weakness off the frame and removed it
        // from the game (ADR 0002): there is nothing left to dispose of.
        let Some(card) = card else {
            continue;
        };

        match disposition {
            EncounterDisposition::Discard => {
                // A persistent treachery placed itself during its Revelation.
                // One whose Revelation was cancelled (Ward of Protection 01065)
                // placed nothing, so it is discarded like any other —
                // `glossary/Cancel.md`: *"If the effects of a treachery card
                // are canceled, the card is still regarded as having been
                // drawn, and it is still placed in the encounter discard
                // pile."*
                let placed_itself = revelation_resolved
                    && (cx.cards.abilities_for)(&card)
                        .is_some_and(|abilities| treachery_is_persistent(&abilities));
                if !placed_itself {
                    board::discard_drawn_card(cx, card, owner, drawn_from(owner));
                }
            }
            EncounterDisposition::Spawn { investigator } => {
                let Some(metadata) = (cx.cards.metadata_for)(&card) else {
                    return EngineOutcome::Rejected {
                        reason: format!("encounter enemy disposal: no metadata for card {card:?}")
                            .into(),
                    };
                };
                // An engagement tie pushes its `SpawnEngage` frame and returns
                // `Done`: the loop guard then finds it on top rather than an
                // `EncounterCard`, so the loop ends and the `drive` loop
                // surfaces the tie. A mid-Mythos tie sits above the drawer's
                // `PlayerDraw`, and `resume_spawn_engage` continues the chain
                // after the pick. A reject propagates.
                match spawn_enemy(cx, investigator, card, metadata, owner) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
        }
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests;
