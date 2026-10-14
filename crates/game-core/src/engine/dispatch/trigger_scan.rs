//! The trigger scan: which triggered abilities a timing event reaches (#962).
//!
//! Four pieces, each written once so the forced and reaction paths cannot
//! disagree about them:
//!
//! - [`board_walk`] — **every** card an ability could be printed on, in the
//!   [board walk](board::walk)'s order, at every condition. A condition never
//!   picks zones.
//! - [`pattern_matches`] — whether a pattern hears an event, from the card's
//!   own scoping words and the condition's own data. Exhaustive in both
//!   directions: [`EventPattern::condition`] and [`TimingEvent::condition`]
//!   must agree first, then a `match` on the event with no wildcard arm.
//! - [`collect_forced`] — the forced filter over the two: one candidate per
//!   hit, bound to one controller by the forced-binding rule, not filtered by
//!   reachability, plus the forced abilities of the cards in each
//!   investigator's hand.
//! - [`collect_reactions`] — the reaction filter: one candidate per
//!   investigator who can reach the hit's source (ADR 0010), plus the Fast
//!   events in each investigator's hand.
//!
//! Why the walk is the whole board, rather than a table of zones per
//! condition, is `docs/adr/0018-one-trigger-scan-walks-the-whole-board.md`.

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    self, AttackerScope, EventPattern, EventTiming, TargetScope, TestedLocationScope, Trigger,
    TriggerKind,
};

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, Placement};
use crate::engine::dispatch::cursor;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::initiation::{self, InitiationKind};
use crate::engine::{abilities_in_effect, ability_source};
use crate::state::{
    self, AbilityAddress, AbilitySource, CandidateSource, CardCode, CardInstanceId, DamageSource,
    GameState, InvestigatorId, LocationId, ResolutionCandidate, Status,
};

/// One card the walk visits: where its abilities are, its code, and the
/// investigator who controls it, if anyone does.
#[derive(Debug, Clone, Copy)]
pub(super) struct BoardSource<'a> {
    /// Where the card is — a board source, or [`CandidateSource::Hand`] for a
    /// card in an investigator's hand.
    pub(super) source: CandidateSource,
    /// The card's code, which its abilities are looked up by.
    pub(super) code: &'a CardCode,
    /// The investigator who controls the card: for a controlled instance its
    /// controller, for a hand card the investigator holding it. `None` for an
    /// uncontrolled card — a location, an enemy, an attachment on either, a
    /// card put into play at a location, the act, the agenda.
    pub(super) controller: Option<InvestigatorId>,
}

/// Every card an ability reachable by `event` could be printed on: the
/// [board walk](board::walk), in its order (ADR 0018), with two changes the scan
/// alone makes.
///
/// - **Hand cards are added**, after the whole board — the act and the agenda
///   included — one investigator's hand at a time in the walk's investigator
///   order. A hand is not on the board, but a Fast event's reaction is played
///   from there, and a forced ability that names the hand works from there
///   (Dark Memory 01013's *"If Dark Memory is in your hand at the end of your
///   turn"*). Which hand cards depends on `kind`: for a
///   [`Reaction`](TriggerKind::Reaction) scan only the Fast events, for a
///   [`Forced`](TriggerKind::Forced) scan every card, whose forced abilities
///   alone [`collect_forced`] reads.
/// - **At an advance, the act or agenda slot holds the card the event names.**
///   It is still the current one while its reverse resolves (the cursor moves
///   on at the advance's finalize step), and the event's code is the authority
///   for which card that is, so the scan fills those two slots — the walk's
///   last — itself.
///
/// **Eliminated investigators are skipped**, their cards and their hands —
/// Rules Reference p.10 removes their cards from play, all but the
/// investigator card, which only this filter keeps out of the scan (#567).
/// The one exception is the investigator
/// [`TimingEvent::EliminationGameEnd`] names, who has already been flipped off
/// `Active` when Elimination step 0 fires for their weaknesses.
///
/// Beyond the hand slot, the walk does not filter by kind, timing or pattern.
pub(super) fn board_walk<'a>(
    state: &'a GameState,
    cards: &'a CardRegistry,
    event: &'a TimingEvent,
    kind: TriggerKind,
) -> Vec<BoardSource<'a>> {
    let exempt = match event {
        TimingEvent::EliminationGameEnd { investigator } => Some(*investigator),
        _ => None,
    };
    // The cards in `id`'s hand a `kind` scan reads: the Fast events for a
    // reaction scan, every card for a forced one.
    let hand = |id: InvestigatorId| {
        let held = state
            .investigators
            .get(&id)
            .map(|inv| {
                inv.hand
                    .iter()
                    .filter(|code| match kind {
                        TriggerKind::Forced => true,
                        TriggerKind::Reaction => (cards.metadata_for)(code).is_some_and(|meta| {
                            meta.is_fast() && meta.card_type() == CardType::Event
                        }),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        held.into_iter().map(move |code| BoardSource {
            source: CandidateSource::Hand,
            code,
            controller: Some(id),
        })
    };
    let mut walked = Vec::new();
    for card in board::walk(state) {
        let controller = card.placement.investigator();
        if card.placement.in_eliminated_area(state) && controller != exempt {
            continue;
        }
        // The act and agenda slots, the walk's last two, are filled below.
        if matches!(card.placement, Placement::Act | Placement::Agenda) {
            continue;
        }
        walked.push(BoardSource {
            source: CandidateSource::Ability(card.source),
            code: card.code(),
            controller,
        });
    }
    // The event's code fills an advancing slot even when the deck's cursor
    // names no card there, as on a board that fires an advance without loading
    // the deck (`cards/tests/agenda_reverses.rs`): the reverse is still scanned.
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
    for id in board::investigator_order(state) {
        let active = state
            .investigators
            .get(&id)
            .is_some_and(|inv| inv.status == Status::Active);
        if active || Some(id) == exempt {
            walked.extend(hand(id));
        }
    }
    walked
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
///   [`subject`](TimingEvent::subject), or the lead proxy when it has none;
/// - a card in **hand** → the investigator holding it.
///
/// A hit with no one to bind (no lead and no subject) is not collected.
///
/// A forced ability on a card in hand is collected when its pattern matches.
/// The rule is narrower — `glossary/Ability.md`: *"Card abilities only interact
/// with the game if the card bearing the ability is in play, unless the ability
/// (or rules for the cardtype) specifically references its use from an
/// out-of-play area"* — and the scan honours it only for self-scoped patterns,
/// which a card in hand never satisfies ([`self_scoped`]): Arcane Initiate
/// 01063's *"After Arcane Initiate enters play"* stays inert in hand. A forced
/// ability on a hand card whose pattern is not self-scoped would fire from
/// hand; in the corpus that is Dark Memory 01013 alone, which should.
pub(super) fn collect_forced(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let lead = cursor::first_active_investigator(state);
    let mut hits = Vec::new();
    for walked in board_walk(state, cards, event, TriggerKind::Forced) {
        // A hand card's controller is the investigator holding it.
        let bound = walked.controller.or(match walked.source {
            CandidateSource::Hand => None,
            CandidateSource::Ability(AbilitySource::Act | AbilitySource::Agenda) => lead,
            CandidateSource::Ability(
                AbilitySource::InPlay(_) | AbilitySource::Location(_) | AbilitySource::Enemy(_),
            ) => event.subject().or(lead),
        });
        let Some(controller) = bound else {
            continue;
        };
        // Kind and cell are load-bearing: the coordinator scans the same
        // (event, bucket) for both kinds (#434), and act 01109 carries a
        // `when`-`RoundEnded` *reaction* this scan must not collect.
        let forced = triggers_at(
            state,
            cards,
            walked.source,
            walked.code,
            TriggerKind::Forced,
            bucket,
        );
        for (address, pattern) in forced {
            if pattern_matches(
                state,
                cards,
                event,
                &pattern,
                TriggerKind::Forced,
                walked.source,
                walked.code,
                controller,
            ) {
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
    // clues on Cover Up"* is an eligibility condition, and without it a clueless
    // Cover Up prompted at game end (#786).
    hits.retain(|hit| initiation::check(state, cards, hit, InitiationKind::Forced).is_ok());
    hits
}

/// Every reaction `event` reaches in the `bucket` cell, as candidates that pass
/// the initiation gate (ADR 0017), in [`board_walk`] order — the one reaction
/// scan, which the coordinator, the window's re-validation and its fire-time
/// check all ask (#568).
///
/// A reaction is a player's choice, so a hit becomes **one candidate per
/// investigator who can reach its source** — ADR 0010's
/// [`reachable_sources`](ability_source::reachable_sources), the predicate an
/// activation answers to, asked here in the walk's investigator order. A forced
/// ability is not filtered this way ([`collect_forced`]):
///
/// - a controlled card → its controller, and, for an encounter card in a threat
///   area, every other investigator at that location (Haunted 01098's ruling);
/// - a location, a card attached to or put into play at it, an enemy and its
///   attachments → each investigator at that location;
/// - the current **act** or **agenda** → **one** candidate, bound to the lead
///   proxy: every investigator reaches it, and its corpus reaction (The Barrier
///   01109's round-end advance) is the group's one offer, not one per seat;
/// - a **Fast event in hand** → the investigator holding it, gated as a
///   [`Play`](InitiationKind::Play), and offered once per copy however many of
///   its abilities match.
///
/// Every other candidate is gated as a [`Reaction`](InitiationKind::Reaction).
pub(super) fn collect_reactions(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let lead = cursor::first_active_investigator(state);
    // Each investigator's reachable sources, asked once per scan and only if a
    // board hit needs them.
    let mut reach: Option<Vec<(InvestigatorId, Vec<AbilitySource>)>> = None;
    let mut hits = Vec::new();
    for walked in board_walk(state, cards, event, TriggerKind::Reaction) {
        let reactions = triggers_at(
            state,
            cards,
            walked.source,
            walked.code,
            TriggerKind::Reaction,
            bucket,
        );
        if reactions.is_empty() {
            continue;
        }
        let (reachers, kind) = match walked.source {
            CandidateSource::Hand => (
                walked.controller.into_iter().collect(),
                InitiationKind::Play,
            ),
            CandidateSource::Ability(AbilitySource::Act | AbilitySource::Agenda) => {
                (lead.into_iter().collect(), InitiationKind::Reaction)
            }
            CandidateSource::Ability(source) => {
                let reach = reach.get_or_insert_with(|| {
                    board::investigator_order(state)
                        .into_iter()
                        .map(|id| {
                            let sources = ability_source::reachable_sources(state, cards, id)
                                .into_iter()
                                .map(|(source, _)| source)
                                .collect();
                            (id, sources)
                        })
                        .collect()
                });
                let reachers: Vec<InvestigatorId> = reach
                    .iter()
                    .filter(|(_, sources)| sources.contains(&source))
                    .map(|(id, _)| *id)
                    .collect();
                (reachers, InitiationKind::Reaction)
            }
        };
        for controller in reachers {
            for (address, pattern) in &reactions {
                if !pattern_matches(
                    state,
                    cards,
                    event,
                    pattern,
                    TriggerKind::Reaction,
                    walked.source,
                    walked.code,
                    controller,
                ) {
                    continue;
                }
                let candidate = ResolutionCandidate {
                    code: walked.code.clone(),
                    controller,
                    address: address.clone(),
                    source: walked.source,
                };
                // The initiation gate (ADR 0017). For a Fast event this is the
                // play gate — its effect must be able to change the game state
                // (Evidence! 01022 at a 0-clue location, #495), no "cannot
                // play" may forbid it (Dissonant Voices 01165, #917), and its
                // cost must be payable (#501). Filtering keeps the offer honest;
                // it is not the binding check — the wallet is shared, so a
                // sibling option can empty it after this ran, and initiation
                // re-asks (#568).
                if initiation::check(state, cards, &candidate, kind).is_err() {
                    continue;
                }
                hits.push(candidate);
                // One option per copy of a Fast event: a card with two matching
                // abilities is still played once. No in-scope card has two.
                if walked.source == CandidateSource::Hand {
                    break;
                }
            }
        }
    }
    hits
}

/// The `kind` triggered abilities of the card at `source` in the `bucket` cell,
/// with their addresses: through the side-in-effect / grant funnel for a board
/// source, the printed front for a card in hand (nothing grants to a card out
/// of play).
fn triggers_at(
    state: &GameState,
    cards: &CardRegistry,
    source: CandidateSource,
    code: &CardCode,
    kind: TriggerKind,
    bucket: EventTiming,
) -> Vec<(AbilityAddress, EventPattern)> {
    abilities_in_effect::for_candidate_source(state, cards, source, code)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(address, ability)| match ability.trigger {
            Trigger::OnEvent {
                pattern,
                timing,
                kind: k,
            } if k == kind && timing == bucket => Some((address, pattern)),
            _ => None,
        })
        .collect()
}

/// Whether a `kind` ability declaring `pattern`, on the card at `source`,
/// bound to `controller`, hears `event`. The one matcher both kinds share:
/// [`trigger_matches`] for what the event says about the controller, then
/// [`scope_matches`] for where the card is. The kind matters only to a card in
/// hand ([`self_scoped`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn pattern_matches(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    pattern: &EventPattern,
    kind: TriggerKind,
    source: CandidateSource,
    code: &CardCode,
    controller: InvestigatorId,
) -> bool {
    trigger_matches(event, pattern, controller)
        && scope_matches(state, cards, event, pattern, kind, source, code, controller)
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
            let EventPattern::EnemyDefeated {
                by_controller,
                code: narrow,
            } = pattern
            else {
                unreachable!(
                    "trigger_matches: the condition check paired {pattern:?} with EnemyDefeated, \
                     but only EventPattern::EnemyDefeated has that condition"
                )
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
        // "After **you** play a [[Spell]] card" (Heirloom of Hyperborea 01012).
        // The card filter reads metadata, in [`scope_matches`].
        TimingEvent::CardPlayed { investigator, .. } => *investigator == controller,
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
            let EventPattern::SkillTestResolved {
                outcome: p_out,
                kind: p_kind,
                by_controller,
                tested_location: _,
            } = pattern
            else {
                unreachable!(
                    "trigger_matches: the condition check paired {pattern:?} with \
                     SkillTestResolved, but only EventPattern::SkillTestResolved has that condition"
                )
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
#[allow(clippy::too_many_arguments)]
fn scope_matches(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    pattern: &EventPattern,
    kind: TriggerKind,
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
                unreachable!(
                    "scope_matches: trigger_matches paired {pattern:?} with EnemyAttacks, but \
                     only EventPattern::EnemyAttacks has that condition"
                )
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
            controlled_weakness(state, cards, source, *investigator)
        }
        // "When an enemy attack deals damage to **Guard Dog**" (01021): only an
        // asset the assignment gives damage to. It reads the *event's*
        // assignment, which is what makes an edit in a `when` cell visible to
        // the cells after it (ADR 0009).
        TimingEvent::DamageAssigned { assignment, .. } => self_scoped(source, kind, |instance| {
            assignment.asset_damage.contains_key(&instance)
        }),
        // "After **Research Librarian** enters play" (01032): the entered
        // instance itself.
        TimingEvent::EnteredPlay { instance, .. } => {
            self_scoped(source, kind, |candidate| candidate == *instance)
        }
        // "When you would discover 1 or more clues **at your location**" (Cover
        // Up 01007).
        TimingEvent::DiscoverClues { location, .. } => {
            location_of(state, controller) == Some(*location)
        }
        // "After you play a **[[Spell]]** card" (Heirloom of Hyperborea 01012):
        // the played card's printed metadata passes the pattern's filter. Not a
        // narrowing of where the ability's card is, but it needs the registry,
        // which [`trigger_matches`] does not take.
        TimingEvent::CardPlayed { code: played, .. } => {
            let EventPattern::CardPlayed { filter } = pattern else {
                unreachable!(
                    "scope_matches: trigger_matches paired {pattern:?} with CardPlayed, but \
                     only EventPattern::CardPlayed has that condition"
                )
            };
            (cards.metadata_for)(played).is_some_and(|meta| filter.matches(meta))
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
/// A card in hand has no instance on the board for the self to be, and which
/// way that falls depends on `kind`:
///
/// - a **reaction** in hand — a Fast event — passes unread, so a Fast event
///   declaring the pattern is played in that condition's window unscoped by it,
///   as `trigger_initiation.rs` relies on (#964);
/// - a **forced** ability in hand never passes. Its card is out of play, and an
///   ability about the card itself — Arcane Initiate 01063's *"After Arcane
///   Initiate enters play"* — does not reference a use from hand, so
///   `glossary/Ability.md` keeps it from interacting with the game (#1046).
fn self_scoped(
    source: CandidateSource,
    kind: TriggerKind,
    is_self: impl Fn(CardInstanceId) -> bool,
) -> bool {
    match source {
        CandidateSource::Ability(source) => source.instance().is_some_and(is_self),
        CandidateSource::Hand => kind == TriggerKind::Reaction,
    }
}

/// Whether `source` is a card instance attached to `location`.
fn attached_to(state: &GameState, source: CandidateSource, location: LocationId) -> bool {
    source
        .instance()
        .and_then(|instance| board::find_instance(state, instance))
        .is_some_and(|(_, placement)| placement == Placement::LocationAttachment(location))
}

/// Whether `source` is a weakness `investigator` controls.
fn controlled_weakness(
    state: &GameState,
    cards: &CardRegistry,
    source: CandidateSource,
    investigator: InvestigatorId,
) -> bool {
    let Some((card, placement)) = source
        .instance()
        .and_then(|instance| board::find_instance(state, instance))
    else {
        return false;
    };
    placement.investigator() == Some(investigator)
        && (cards.metadata_for)(&card.code).is_some_and(|meta| meta.weakness)
}

/// Whether investigators `a` and `b` share a current location. Two
/// investigators at no location never match.
fn same_location(state: &GameState, a: InvestigatorId, b: InvestigatorId) -> bool {
    location_of(state, a).is_some_and(|la| location_of(state, b) == Some(la))
}

/// The location investigator `id` is at, if any.
fn location_of(state: &GameState, id: InvestigatorId) -> Option<LocationId> {
    state
        .investigators
        .get(&id)
        .and_then(|inv| inv.current_location)
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
