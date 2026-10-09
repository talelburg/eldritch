//! The trigger scan: which triggered abilities a timing event reaches (#962).
//!
//! Three pieces, each written once so the forced and reaction paths cannot
//! disagree about them:
//!
//! - [`board_walk`] — **every** card an ability could be printed on, in one
//!   fixed order, at every condition. A condition never picks zones.
//! - [`pattern_matches`] — whether a pattern hears an event, from the card's
//!   own scoping words and the condition's own data. Exhaustive in both
//!   directions: [`EventPattern::condition`] and [`TimingEvent::condition`]
//!   must agree first, then a `match` on the event with no wildcard arm.
//! - [`collect_forced`] — the forced filter over the two: one candidate per
//!   hit, bound to one controller by the forced-binding rule, not filtered by
//!   reachability.
//!
//! Why the walk is the whole board, rather than a table of zones per
//! condition, is `docs/adr/0018-one-trigger-scan-walks-the-whole-board.md`.

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    self, AttackerScope, EventPattern, EventTiming, TargetScope, TestedLocationScope, Trigger,
    TriggerKind,
};

use crate::card_registry;
use crate::engine::abilities_in_effect;
use crate::engine::dispatch::cursor;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::initiation::{self, InitiationKind};
use crate::state::{
    self, AbilitySource, CandidateSource, CardCode, CardInPlay, CardInstanceId, DamageSource,
    GameState, InvestigatorId, LocationId, ResolutionCandidate, Status,
};

/// One card the walk visits: where its abilities are, its code, and the
/// investigator who controls it, if anyone does.
#[derive(Debug, Clone, Copy)]
pub(super) struct BoardSource<'a> {
    /// Where the card is — a board source, or [`CandidateSource::Hand`] for a
    /// Fast event in an investigator's hand.
    pub(super) source: CandidateSource,
    /// The card's code, which its abilities are looked up by.
    pub(super) code: &'a CardCode,
    /// The investigator who controls the card: for a controlled instance its
    /// controller, for a hand card the investigator holding it. `None` for an
    /// uncontrolled card — a location, an enemy, an attachment on either, a
    /// card put into play at a location, the act, the agenda.
    pub(super) controller: Option<InvestigatorId>,
}

/// Every card an ability reachable by `event` could be printed on, in this
/// order:
///
/// 1. for each investigator — the active investigator first, then the rest of
///    `turn_order`, then anyone else by id — their controlled card instances
///    ([`Investigator::controlled_card_instances`]: the investigator card,
///    cards in play, the threat area), then the Fast events in their hand;
/// 2. each location by [`LocationId`] — the location, its attachments, the
///    cards put into play at it;
/// 3. each enemy by its id, then its attachments;
/// 4. the current act, then the current agenda — or, at an advance, the
///    advancing card the event names in that slot.
///
/// **Eliminated investigators are skipped** — Rules Reference p.10 removes
/// their cards from play, all but the investigator card, which only this
/// filter keeps out of the scan (#567). The one exception is the investigator
/// [`TimingEvent::EliminationGameEnd`] names, who has already been flipped off
/// `Active` when Elimination step 0 fires for their weaknesses.
///
/// The walk does not filter by kind, timing or pattern. The forced collector
/// ignores the hand slot (no corpus card prints a forced ability that works
/// from hand); the reaction path uses it for Fast events (#966).
///
/// [`Investigator::controlled_card_instances`]: crate::state::Investigator::controlled_card_instances
pub(super) fn board_walk<'a>(state: &'a GameState, event: &'a TimingEvent) -> Vec<BoardSource<'a>> {
    let exempt = match event {
        TimingEvent::EliminationGameEnd { investigator } => Some(*investigator),
        _ => None,
    };
    let reg = card_registry::current();
    let mut walked = Vec::new();
    for id in investigator_order(state) {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        if inv.status != Status::Active && Some(id) != exempt {
            continue;
        }
        walked.extend(
            inv.controlled_card_instances()
                .map(|card| in_play(card, Some(id))),
        );
        let Some(reg) = reg else {
            continue;
        };
        walked.extend(
            inv.hand
                .iter()
                .filter(|code| {
                    (reg.metadata_for)(code)
                        .is_some_and(|meta| meta.is_fast() && meta.card_type() == CardType::Event)
                })
                .map(|code| BoardSource {
                    source: CandidateSource::Hand,
                    code,
                    controller: Some(id),
                }),
        );
    }
    for (id, location) in &state.locations {
        walked.push(BoardSource {
            source: CandidateSource::Ability(AbilitySource::Location(*id)),
            code: &location.code,
            controller: None,
        });
        walked.extend(location.attachments.iter().map(|card| in_play(card, None)));
        walked.extend(
            location
                .cards_at_location
                .iter()
                .map(|card| in_play(card, None)),
        );
    }
    for (id, enemy) in &state.enemies {
        walked.push(BoardSource {
            source: CandidateSource::Ability(AbilitySource::Enemy(*id)),
            code: &enemy.code,
            controller: None,
        });
        walked.extend(enemy.attachments.iter().map(|card| in_play(card, None)));
    }
    // An advance names the card whose reverse is resolving. It is still the
    // current one while its reverse resolves (the cursor moves on at the
    // advance's finalize step), and the event's code is the authority for
    // which card that is.
    let act = match event {
        TimingEvent::ActAdvanced { code } => Some(code),
        _ => state.act_deck.get(state.act_index).map(|act| &act.code),
    };
    let agenda = match event {
        TimingEvent::AgendaAdvanced { code } => Some(code),
        _ => state
            .agenda_deck
            .get(state.agenda_index)
            .map(|agenda| &agenda.code),
    };
    walked.extend(act.map(|code| BoardSource {
        source: CandidateSource::Ability(AbilitySource::Act),
        code,
        controller: None,
    }));
    walked.extend(agenda.map(|code| BoardSource {
        source: CandidateSource::Ability(AbilitySource::Agenda),
        code,
        controller: None,
    }));
    walked
}

/// The active investigator, then the rest of `turn_order`, then every other
/// investigator by id — so an investigator a fixture never seated in
/// `turn_order` is still walked, last.
fn investigator_order(state: &GameState) -> Vec<InvestigatorId> {
    let mut order: Vec<InvestigatorId> = state.active_investigator.into_iter().collect();
    for id in state.turn_order.iter().chain(state.investigators.keys()) {
        if !order.contains(id) {
            order.push(*id);
        }
    }
    order
}

fn in_play(card: &CardInPlay, controller: Option<InvestigatorId>) -> BoardSource<'_> {
    BoardSource {
        source: CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
        code: &card.code,
        controller,
    }
}

/// Every forced ability `event` reaches in the `bucket` cell, as candidates
/// that pass the initiation gate (ADR 0017) as
/// [`Forced`](InitiationKind::Forced), in [`board_walk`] order.
///
/// Each hit is **one** candidate. It is not filtered by reachability — ADR
/// 0010: *"a forced ability is not restricted to the sources its controller
/// could legally use"* — and its controller is bound by the forced-binding
/// rule:
///
/// - a **controlled** card → its controller;
/// - the current **act** or **agenda** → the lead proxy, the first Active
///   investigator in `turn_order` (GLOSSARY "Lead investigator");
/// - any other **uncontrolled** card → the condition's
///   [`subject`](TimingEvent::subject), or the lead proxy when it has none.
///
/// A hit with no one to bind (no lead and no subject) is not collected.
pub(super) fn collect_forced(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    if card_registry::current().is_none() {
        return Vec::new();
    }
    let lead = cursor::first_active_investigator(state);
    let mut hits = Vec::new();
    for walked in board_walk(state, event) {
        // Forced abilities on cards out of play are not scanned: no corpus
        // card prints one.
        let CandidateSource::Ability(source) = walked.source else {
            continue;
        };
        let bound = walked.controller.or(match source {
            AbilitySource::Act | AbilitySource::Agenda => lead,
            AbilitySource::InPlay(_) | AbilitySource::Location(_) | AbilitySource::Enemy(_) => {
                event.subject().or(lead)
            }
        });
        let Some(controller) = bound else {
            continue;
        };
        let Some(abilities) = abilities_in_effect::for_source(state, source, walked.code) else {
            continue;
        };
        for (address, ability) in abilities {
            let Trigger::OnEvent {
                pattern,
                timing,
                kind,
            } = &ability.trigger
            else {
                continue;
            };
            // Kind and cell are load-bearing: the coordinator scans the same
            // (event, bucket) for both kinds (#434), and act 01109 carries a
            // `when`-`RoundEnded` *reaction* this scan must not collect.
            if *kind == TriggerKind::Forced
                && *timing == bucket
                && pattern_matches(
                    state,
                    event,
                    pattern,
                    walked.source,
                    walked.code,
                    controller,
                )
            {
                hits.push(ResolutionCandidate {
                    code: walked.code.clone(),
                    controller,
                    address,
                    source: walked.source,
                });
            }
        }
    }
    // The initiation gate (ADR 0017), at the one chokepoint feeding both the
    // lone-hit path and the 2+ ordered run, so a forced ability that cannot
    // initiate neither resolves nor prompts. As `Forced` it gets the
    // change-state and eligibility checks — Cover Up 01007's *"if there are any
    // clues on Cover Up"* is an eligibility tag, and without it a clueless
    // Cover Up prompted at game end (#786).
    hits.retain(|hit| initiation::check(state, hit, InitiationKind::Forced).is_ok());
    hits
}

/// Whether an ability declaring `pattern`, on the card at `source`, bound to
/// `controller`, hears `event`. The one matcher both kinds share:
/// [`trigger_matches`] for what the event says about the controller, then
/// [`scope_matches`] for where the card is.
pub(super) fn pattern_matches(
    state: &GameState,
    event: &TimingEvent,
    pattern: &EventPattern,
    source: CandidateSource,
    code: &CardCode,
    controller: InvestigatorId,
) -> bool {
    trigger_matches(event, pattern, controller)
        && scope_matches(state, event, pattern, source, code, controller)
}

/// Whether an ability declaring `pattern`, owned by `controller`, matches
/// `event` — the half of the matcher that reads only the event, the pattern and
/// the controller.
///
/// Two steps, both exhaustive (#963):
///
/// 1. **The condition.** The pattern's [`EventPattern::condition`] must equal
///    the event's [`TimingEvent::condition`]. Both maps are exhaustive, so a new
///    pattern or a new event cannot compile without naming its condition, and
///    no pairing is left to a wildcard.
/// 2. **The narrowing.** A `match` on the event, one arm per variant, applies
///    what that condition's pattern and the event say beyond the condition
///    itself: Roland Banks 01001's *"you defeat"*, a test's outcome and kind,
///    Frozen in Fear 01164's *"your turn"*.
///
/// **Timing is not consulted here** (#704). Which cell an ability resolves in is
/// the coordinator's business — it scans one cell at a time and the scans
/// filter on `timing == bucket` — so a pattern pairs with its condition
/// identically in all three cells.
pub(super) fn trigger_matches(
    event: &TimingEvent,
    pattern: &EventPattern,
    controller: InvestigatorId,
) -> bool {
    if pattern.condition() != event.condition() {
        return false;
    }
    match event {
        // "after **you** defeat an enemy" (Roland Banks 01001) and "if the
        // Ghoul Priest is defeated" (What Have You Done? 01110), for both kinds.
        TimingEvent::EnemyDefeated { by, code, .. } => {
            // Same condition, so this is the one pattern that has it.
            let EventPattern::EnemyDefeated {
                by_controller,
                code: narrow,
            } = pattern
            else {
                return false;
            };
            (!*by_controller || *by == Some(controller))
                && narrow.as_deref().is_none_or(|c| c == code.as_str())
        }
        TimingEvent::PhaseStarted { phase } => {
            matches!(pattern, EventPattern::PhaseStarted { phase: p } if *p == dsl_phase(*phase))
        }
        TimingEvent::PhaseEnded { phase } => {
            matches!(pattern, EventPattern::PhaseEnded { phase: p } if *p == dsl_phase(*phase))
        }
        // "At the end of **your** turn" (Frozen in Fear 01164) and "…**you**
        // discover clues" (Cover Up 01007; its "at your location" half reads
        // the board, in [`scope_matches`]).
        TimingEvent::EndOfTurn { investigator }
        | TimingEvent::DiscoverClues { investigator, .. } => *investigator == controller,
        // "**When an enemy attack** deals damage to Guard Dog" (01021). The
        // self-binding half is the source's, in [`scope_matches`]; this half is
        // the card's narrowing to an enemy attack, which `Effect::Deal` harm
        // (Dynamite Blast, a treachery) does not satisfy.
        TimingEvent::DamageAssigned { source, .. } => {
            matches!(source, DamageSource::EnemyAttack { .. })
        }
        // "after you succeed/fail a skill test" — narrowed by outcome,
        // (optionally) test kind, and whether the card says *"**you**"*. Dr.
        // Milan 01033 is `{ Success, Some(Investigate), by_controller: true }`.
        //
        // `by_controller` is checked **here** rather than in an eligibility
        // predicate because this matcher runs before a candidate is minted: a
        // hard `*investigator == controller` would have made a reaction on
        // somebody else's test unreachable, whatever the card said afterwards.
        TimingEvent::SkillTestResolved {
            investigator,
            kind,
            outcome,
        } => {
            // Same condition, so this is the one pattern that has it.
            let EventPattern::SkillTestResolved {
                outcome: p_out,
                kind: p_kind,
                by_controller,
                tested_location: _,
            } = pattern
            else {
                return false;
            };
            (!*by_controller || *investigator == controller)
                && outcome == p_out
                && (p_kind.is_none() || *p_kind == Some(*kind))
        }
        // Scoped to the entered card's controller; the self-instance half is in
        // [`scope_matches`] (Research Librarian 01032).
        TimingEvent::EnteredPlay {
            controller: window_controller,
            ..
        } => *window_controller == controller,
        // The condition is the whole of what the event says. Every scope these
        // patterns carry is about where the card is, in [`scope_matches`]:
        // the attacker and the target, the entered and the left location, the
        // advancing card, elimination's weaknesses. `DamagePlaced` has no
        // pattern yet, so the condition check above has refused every one.
        TimingEvent::EnemyAttacks { .. }
        | TimingEvent::RoundEnded
        | TimingEvent::EnteredLocation { .. }
        | TimingEvent::ActAdvanced { .. }
        | TimingEvent::AgendaAdvanced { .. }
        | TimingEvent::GameEnd
        | TimingEvent::EliminationGameEnd { .. }
        | TimingEvent::LeftLocation { .. }
        | TimingEvent::DamagePlaced { .. } => true,
    }
}

/// The half of the matcher that reads where the card is: its source, and the
/// board around it. Every narrowing a per-condition zone table used to make by
/// choosing where to look is stated here instead (ADR 0018) — as the card's
/// own scoping word when the pattern carries one (*"this"*, *"attached"*, *"at
/// your location"*), and as the condition's definition otherwise.
///
/// Exhaustive on the event, like [`trigger_matches`]; it assumes that function
/// has already paired the pattern with the event's condition.
fn scope_matches(
    state: &GameState,
    event: &TimingEvent,
    pattern: &EventPattern,
    source: CandidateSource,
    code: &CardCode,
    controller: InvestigatorId,
) -> bool {
    match event {
        // "After you enter **the Attic**" (01113) — the entered location's own
        // card. Its ruling: *"The Forced ability triggers each time an
        // investigator enters this location."*
        TimingEvent::EnteredLocation { location, .. } => {
            source == CandidateSource::Ability(AbilitySource::Location(*location))
        }
        // "When an investigator leaves **attached location**" (Barricade 01038).
        TimingEvent::LeftLocation { location, .. } => attached_to(state, source, *location),
        // "After **attached location** is successfully investigated"
        // (Obscuring Fog 01168), when the pattern says so.
        TimingEvent::SkillTestResolved { .. } => match pattern {
            EventPattern::SkillTestResolved {
                tested_location: TestedLocationScope::Attached,
                ..
            } => state
                .current_skill_test()
                .and_then(|test| test.tested_location)
                .is_some_and(|tested| attached_to(state, source, tested)),
            _ => true,
        },
        // "After **Silver Twilight Acolyte** attacks" (01102) and "an enemy
        // attacks **an investigator at your location**" (Dodge 01023).
        TimingEvent::EnemyAttacks {
            enemy,
            investigator,
        } => {
            let EventPattern::EnemyAttacks { attacker, target } = pattern else {
                return false;
            };
            let attacker_ok = match attacker {
                AttackerScope::This => {
                    source == CandidateSource::Ability(AbilitySource::Enemy(*enemy))
                }
                AttackerScope::Any => true,
            };
            let target_ok = match target {
                TargetScope::AtYourLocation => same_location(state, controller, *investigator),
                TargetScope::Any => true,
            };
            attacker_ok && target_ok
        }
        // The act or agenda **this ability is printed on** advanced: the event
        // names the advancing card by code.
        TimingEvent::ActAdvanced { code: advancing } => {
            source == CandidateSource::Ability(AbilitySource::Act) && code == advancing
        }
        TimingEvent::AgendaAdvanced { code: advancing } => {
            source == CandidateSource::Ability(AbilitySource::Agenda) && code == advancing
        }
        // Rules Reference p.10 Elimination step 0: *"Trigger any 'when the game
        // ends' abilities on each weakness the eliminated investigator owns that
        // is in play."* The game has not ended for anyone else, so only a
        // weakness that investigator controls hears it. The rule says *owns*;
        // control coincides with ownership for every weakness the engine can
        // represent, since a weakness enters its owner's threat area and nothing
        // models one player controlling another's card.
        TimingEvent::EliminationGameEnd { investigator } => {
            controlled_weakness(state, source, *investigator)
        }
        // "When an enemy attack deals damage to **Guard Dog**" (01021): only an
        // asset the assignment gives damage to. It reads the *event's*
        // assignment, which is what makes an edit in a `when` cell visible to
        // the cells after it (ADR 0009).
        TimingEvent::DamageAssigned { assignment, .. } => self_scoped(source, |instance| {
            assignment.asset_damage.contains_key(&instance)
        }),
        // "After **Research Librarian** enters play" (01032): the entered
        // instance itself.
        TimingEvent::EnteredPlay { instance, .. } => {
            self_scoped(source, |candidate| candidate == *instance)
        }
        // "When you would discover 1 or more clues **at your location**" (Cover
        // Up 01007).
        TimingEvent::DiscoverClues { location, .. } => {
            state
                .investigators
                .get(&controller)
                .and_then(|inv| inv.current_location)
                == Some(*location)
        }
        // Board-wide conditions, and conditions whose only scope is the
        // controller (read in `trigger_matches`): no card's position narrows
        // them.
        TimingEvent::PhaseStarted { .. }
        | TimingEvent::PhaseEnded { .. }
        | TimingEvent::EnemyDefeated { .. }
        | TimingEvent::RoundEnded
        | TimingEvent::EndOfTurn { .. }
        | TimingEvent::GameEnd
        | TimingEvent::DamagePlaced { .. } => true,
    }
}

/// A self-binding narrowing — *"Guard Dog"*, *"Research Librarian"* — read
/// against `source`: a card instance must pass `is_self`, and a board source
/// with no instance (a location, an enemy, the act, the agenda) never does.
///
/// A Fast event in hand passes unread. It has no instance on the board for the
/// self to be, and the hand scan has never applied the self filter, so a Fast
/// event declaring the pattern is played in that condition's window unscoped
/// by it — as `trigger_initiation.rs` relies on (#964).
fn self_scoped(source: CandidateSource, is_self: impl Fn(CardInstanceId) -> bool) -> bool {
    match source {
        CandidateSource::Ability(source) => source.instance().is_some_and(is_self),
        CandidateSource::Hand => true,
    }
}

/// Whether `source` is a card instance attached to `location`.
fn attached_to(state: &GameState, source: CandidateSource, location: LocationId) -> bool {
    let Some(instance) = source.instance() else {
        return false;
    };
    state
        .locations
        .get(&location)
        .is_some_and(|loc| loc.attachments.iter().any(|c| c.instance_id == instance))
}

/// Whether `source` is a weakness `investigator` controls.
fn controlled_weakness(
    state: &GameState,
    source: CandidateSource,
    investigator: InvestigatorId,
) -> bool {
    let Some(instance) = source.instance() else {
        return false;
    };
    let Some(card) = controlled_instance(state, investigator, instance) else {
        return false;
    };
    card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&card.code))
        .is_some_and(|meta| meta.weakness)
}

fn controlled_instance(
    state: &GameState,
    investigator: InvestigatorId,
    instance: CardInstanceId,
) -> Option<&CardInPlay> {
    state
        .investigators
        .get(&investigator)?
        .controlled_card_instances()
        .find(|card| card.instance_id == instance)
}

/// Whether investigators `a` and `b` share a current location. Two
/// investigators at no location never match.
fn same_location(state: &GameState, a: InvestigatorId, b: InvestigatorId) -> bool {
    let loc = |id| {
        state
            .investigators
            .get(&id)
            .and_then(|i| i.current_location)
    };
    loc(a).is_some_and(|la| loc(b) == Some(la))
}

/// Map the engine's `state::Phase` to the `card-dsl` mirror so a
/// `PhaseStarted` / `PhaseEnded` pattern can be compared.
fn dsl_phase(phase: state::Phase) -> dsl::Phase {
    match phase {
        state::Phase::Mythos => dsl::Phase::Mythos,
        state::Phase::Investigation => dsl::Phase::Investigation,
        state::Phase::Enemy => dsl::Phase::Enemy,
        state::Phase::Upkeep => dsl::Phase::Upkeep,
    }
}

#[cfg(test)]
mod tests;
