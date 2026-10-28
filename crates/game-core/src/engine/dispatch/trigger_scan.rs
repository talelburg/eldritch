//! The trigger scan: which triggered abilities a timing event reaches.
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
//!   reachability, plus the forced abilities live in hand
//!   ([`AbilityZone::Hand`]) of the cards in each investigator's hand.
//! - [`collect_reactions`] — the reaction filter: one candidate per
//!   investigator who can reach the hit's source (ADR 0010), plus the Fast
//!   events in each investigator's hand.
//!
//! Why the walk is the whole board, rather than a table of zones per
//! condition, is `docs/adr/0018-one-trigger-scan-walks-the-whole-board.md`.

use card_dsl::card_data::{CardMetadata, CardType};
use card_dsl::dsl::{
    self, AbilityZone, AttackerScope, EventPattern, EventTiming, HarmKind, HarmPlacedScope,
    HarmTakenScope, TargetScope, TestOutcome, TestedLocationScope, Trigger, TriggerKind,
};

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, Placement};
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::initiation::{self, InitiationKind};
use crate::engine::dispatch::{cursor, legality};
use crate::engine::enumerate::PlaySource;
use crate::engine::{abilities_in_effect, ability_source};
use crate::state::{
    self, AbilityAddress, AbilitySource, Assignment, CandidateSource, CardCode, CardInstanceId,
    DamageSource, GameState, InvestigatorId, LocationId, ResolutionCandidate, Status,
};

/// One card the walk visits: where its abilities are, its code, and the
/// investigator who controls it, if anyone does.
#[derive(Debug, Clone, Copy)]
pub(super) struct BoardSource<'a> {
    /// Where the card is — a board source, [`CandidateSource::Hand`] for a
    /// card in an investigator's hand, or [`CandidateSource::TopOfDiscardPile`]
    /// for a Fast event a permission lets them play from their discard pile.
    pub(super) source: CandidateSource,
    /// The card's code, which its abilities are looked up by.
    pub(super) code: &'a CardCode,
    /// The investigator who controls the card: for a controlled instance its
    /// controller, for a hand card the investigator holding it (for the top of
    /// a discard pile, the pile's owner). `None` for an
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
///   from there, and a forced ability live in hand works from there (Dark
///   Memory 01013's *"If Dark Memory is in your hand at the end of your
///   turn"*). Which hand cards depends on `kind`: for a
///   [`Reaction`](TriggerKind::Reaction) scan only the Fast events, for a
///   [`Forced`](TriggerKind::Forced) scan every card, of whose abilities
///   [`triggers_at`] reads only those live in hand.
/// - **For a reaction scan, so is the top of a discard pile** that a card its
///   owner controls lets them play from (Wendy's Amulet 01014: *"You may play
///   the topmost event in your discard pile as if it were in your hand."*),
///   after that investigator's hand, if it is a Fast event. Whether it may be
///   played from there is [`legality::play_source_available`], the check the
///   turn menu and the lapse probe share. A forced scan never reads it:
///   `glossary/As_If.md` alters the game state for the indicated play alone,
///   so the card is not in hand for any other ability.
/// - **At an advance, the act or agenda slot holds the card the event names.**
///   It is still the current one while its reverse resolves (the cursor moves
///   on at the advance's finalize step), and the event's code is the authority
///   for which card that is, so the scan fills those two slots — the walk's
///   last — itself.
///
/// **Eliminated investigators are skipped**, their cards and their hands —
/// Rules Reference p.10 removes their cards from play, all but the
/// investigator card, which only this filter keeps out of the scan.
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
        let discard_top = match kind {
            TriggerKind::Forced => None,
            TriggerKind::Reaction => abilities_in_effect::discard_play_permission(state, cards, id)
                .and_then(|permission| {
                    legality::play_source_available(
                        state,
                        cards,
                        id,
                        PlaySource::TopOfDiscardPile { permission },
                    )
                    .filter(|code| (cards.metadata_for)(code).is_some_and(CardMetadata::is_fast))
                    .map(|code| (CandidateSource::TopOfDiscardPile { permission }, code))
                }),
        };
        held.into_iter()
            .map(|code| (CandidateSource::Hand, code))
            .chain(discard_top)
            .map(move |(source, code)| BoardSource {
                source,
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
/// - the current **act** or **agenda** → the lead investigator
///   ([`cursor::the_lead`]);
/// - any other **uncontrolled** card → the condition's
///   [`subject`](TimingEvent::subject), or the lead investigator when it has none;
/// - a card in **hand** → the investigator holding it.
///
/// A hit with no one to bind (no lead investigator and no subject) is not collected.
///
/// A card's forced abilities are read where they are live
/// ([`Ability::active_in`](dsl::Ability::active_in)) — `glossary/Ability.md`:
/// *"Card abilities only interact with the game if the card bearing the ability
/// is in play, unless the ability (or rules for the cardtype) specifically
/// references its use from an out-of-play area."* A card in hand contributes
/// only the abilities marked live in hand, Dark Memory 01013's alone in the
/// corpus, so Arcane Initiate 01063's *"After Arcane Initiate enters play"*
/// stays inert there.
pub(super) fn collect_forced(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let lead = cursor::the_lead(state);
    let mut hits = Vec::new();
    for walked in board_walk(state, cards, event, TriggerKind::Forced) {
        // A hand card's controller is the investigator holding it.
        let bound = walked.controller.or(match walked.source.ability() {
            None => None,
            Some(AbilitySource::Act | AbilitySource::Agenda) => lead,
            Some(
                AbilitySource::InPlay(_) | AbilitySource::Location(_) | AbilitySource::Enemy(_),
            ) => event.subject().or(lead),
        });
        let Some(controller) = bound else {
            continue;
        };
        // Kind and cell are load-bearing: the coordinator scans the same
        // (event, bucket) for both kinds, and act 01109 carries a
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
    // Cover Up would prompt at game end.
    hits.retain(|hit| initiation::check(state, cards, hit, InitiationKind::Forced).is_ok());
    hits
}

/// Every reaction `event` reaches in the `bucket` cell, as candidates that pass
/// the initiation gate (ADR 0017), in [`board_walk`] order — the one reaction
/// scan, which the coordinator, the window's re-validation and its fire-time
/// check all ask.
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
///   investigator ([`cursor::the_lead`]): every investigator reaches it, and its corpus reaction (The Barrier
///   01109's round-end advance) is the group's one offer, not one per seat;
/// - a **Fast event in hand**, or on top of a discard pile a permission lets
///   its owner play from → that investigator, gated as a
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
    let lead = cursor::the_lead(state);
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
        let (reachers, kind) = match walked.source.ability() {
            None => (
                walked.controller.into_iter().collect(),
                InitiationKind::Play,
            ),
            Some(AbilitySource::Act | AbilitySource::Agenda) => {
                (lead.into_iter().collect(), InitiationKind::Reaction)
            }
            Some(source) => {
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
                // (Evidence! 01022 at a 0-clue location), no "cannot
                // play" may forbid it (Dissonant Voices 01165), and its
                // cost must be payable. Filtering keeps the offer honest;
                // it is not the binding check — the wallet is shared, so a
                // sibling option can empty it after this ran, and initiation
                // re-asks.
                if initiation::check(state, cards, &candidate, kind).is_err() {
                    continue;
                }
                hits.push(candidate);
                // One option per copy of a Fast event: a card with two matching
                // abilities is still played once. No corpus card has two.
                if walked.source.is_play() {
                    break;
                }
            }
        }
    }
    hits
}

/// The `kind` triggered abilities of the card at `source` in the `bucket` cell,
/// with their addresses: through the side-in-effect / grant funnel for a board
/// source, the printed front for a card in hand or on top of a discard pile
/// (nothing grants to a card out of play).
///
/// Only the abilities live where the card is
/// ([`Ability::active_in`](dsl::Ability::active_in)): in play for a board
/// source, in hand for a forced ability on a hand card. A Fast event's
/// reaction in hand, or on top of a discard pile, is not gated this way — the
/// event is played from there, a rule of its cardtype, and [`board_walk`] has
/// already narrowed both to Fast events.
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
        .filter(|(_, ability)| match (source, kind) {
            (
                CandidateSource::Hand | CandidateSource::TopOfDiscardPile { .. },
                TriggerKind::Reaction,
            ) => true,
            (CandidateSource::Hand, TriggerKind::Forced) => ability.active_in == AbilityZone::Hand,
            // `board_walk` never yields it for a forced scan.
            (CandidateSource::TopOfDiscardPile { .. }, TriggerKind::Forced) => false,
            (CandidateSource::Ability(_), _) => ability.active_in == AbilityZone::InPlay,
        })
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

/// Whether an ability declaring `pattern`, on the card at `source`, bound to
/// `controller`, hears `event`. The one matcher both kinds share:
/// [`trigger_matches`] for what the event says about the controller, then
/// [`scope_matches`] for where the card is.
pub(super) fn pattern_matches(
    state: &GameState,
    cards: &CardRegistry,
    event: &TimingEvent,
    pattern: &EventPattern,
    source: CandidateSource,
    code: &CardCode,
    controller: InvestigatorId,
) -> bool {
    trigger_matches(event, pattern, controller)
        && scope_matches(state, cards, event, pattern, source, code, controller)
}

/// Whether an ability declaring `pattern`, owned by `controller`, matches
/// `event` — the half of the matcher that reads only the event, the pattern and
/// the controller.
///
/// Two steps, both exhaustive:
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
/// **Timing is not consulted here.** Which cell an ability resolves in is
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
        // "At the end of **your** turn" (Frozen in Fear 01164), "…**you**
        // discover clues" (Cover Up 01007; its "at your location" half reads
        // the board, in [`scope_matches`]), "After **you** play a [[Spell]]
        // card" (Heirloom of Hyperborea 01012) and "Play when **you** draw a
        // non-weakness treachery card" (Ward of Protection 01065). The last
        // two's card filters read metadata, in [`scope_matches`]. And "After
        // **you** evade an enemy" (Pickpocketing 01046), "Play after
        // **your** turn begins" (On the Lam 01010), and "When **you** reveal
        // a chaos token" (Wendy Adams 01005).
        TimingEvent::EndOfTurn { investigator }
        | TimingEvent::DiscoverClues { investigator, .. }
        | TimingEvent::CardPlayed { investigator, .. }
        | TimingEvent::EncounterCardDrawn { investigator, .. }
        | TimingEvent::EnemyEvaded { investigator, .. }
        | TimingEvent::TurnBegan { investigator }
        | TimingEvent::ChaosTokenRevealed { investigator, .. } => *investigator == controller,
        // Two patterns share the deal's condition, so each reads its own
        // narrowing, in [`damage_dealt_matches`].
        TimingEvent::DamageDealt {
            source,
            investigator,
            assignment,
        } => damage_dealt_matches(pattern, *source, *investigator, assignment, controller),
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
                margin: _,
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
        // "After 1 or more horror is **placed on Agnes Baker**" (01004): the
        // controller's own investigator card took at least 1 of the kind. Only
        // the investigator card's share counts, so horror soaked by an asset
        // does not — her ruling, *"You cannot use Agnes' ability if you assign
        // horror to an asset."* (<https://arkhamdb.com/card/01004>). The event
        // is emitted even when every point was soaked, which is why the share
        // is read rather than the event's presence.
        TimingEvent::DamagePlaced {
            investigator,
            assignment,
            ..
        } => {
            let EventPattern::HarmPlaced { kind, scope } = pattern else {
                unreachable!(
                    "trigger_matches: the condition check paired {pattern:?} with DamagePlaced, \
                     but only EventPattern::HarmPlaced has that condition"
                )
            };
            match scope {
                HarmPlacedScope::YourInvestigator => {
                    let placed = match kind {
                        HarmKind::Damage => assignment.investigator_damage,
                        HarmKind::Horror => assignment.investigator_horror,
                    };
                    *investigator == controller && placed >= 1
                }
            }
        }
        // The condition is the whole of what the event says. Every scope these
        // patterns carry is about where the card is, in [`scope_matches`]:
        // the attacker and the target, the entered and the left location, the
        // advancing card, elimination's weaknesses.
        TimingEvent::EnemyAttacks { .. }
        | TimingEvent::RoundEnded
        | TimingEvent::EnteredLocation { .. }
        | TimingEvent::ActAdvanced { .. }
        | TimingEvent::AgendaAdvanced { .. }
        | TimingEvent::GameEnd
        | TimingEvent::EliminationGameEnd { .. }
        | TimingEvent::LeftLocation { .. } => true,
    }
}

/// [`trigger_matches`]'s narrowing of the `DamageDealt` condition, which two
/// patterns share: Guard Dog 01021's enemy-attack scope, and the "taken by
/// you" of [`EventPattern::HarmTaken`].
fn damage_dealt_matches(
    pattern: &EventPattern,
    source: DamageSource,
    investigator: InvestigatorId,
    assignment: &Assignment,
    controller: InvestigatorId,
) -> bool {
    match pattern {
        // "**When an enemy attack** deals damage to Guard Dog" (01021). The
        // self-binding half is the source's, in [`scope_matches`]; this
        // half is the card's narrowing to an enemy attack, which
        // `Effect::Deal` harm (Dynamite Blast, a treachery) does not
        // satisfy.
        EventPattern::EnemyAttackDamagedSelf => {
            matches!(source, DamageSource::EnemyAttack { .. })
        }
        // "After **you** take 1 or more horror" (Psychosis 01099): at least
        // 1 of the kind on the controller's investigator card or on any
        // asset they control. `glossary/You_Your.md` (FAQ 2.12): *"it also
        // includes any assets you control."*
        //
        // The scope is the controller of the cards placed on, read here as
        // "the dealt investigator is the controller". The two coincide
        // because every asset share in an assignment is a soak by the dealt
        // investigator's own asset: the soakers are built from their
        // controlled cards only, and nothing deals to an asset directly. It
        // is read off the assignment rather than the board, so a soaking
        // asset the harm has just defeated still counts in the `after`
        // cell. Beat Cop (2) 01028, not implemented, deals straight to an asset,
        // and its ruling (<https://arkhamdb.com/card/02261>) makes that
        // harm its controller's: that is when the controller has to be
        // carried on the assignment.
        EventPattern::HarmTaken { kind, scope } => match scope {
            HarmTakenScope::You => investigator == controller && assignment.total(*kind) >= 1,
        },
        _ => unreachable!(
            "trigger_matches: the condition check paired {pattern:?} with DamageDealt, but \
             only EnemyAttackDamagedSelf and HarmTaken have that condition"
        ),
    }
}

/// The half of the matcher that reads where the card is: its source, and the
/// board around it. Every narrowing is stated here rather than by choosing
/// where to look (ADR 0018) — as the card's
/// own scoping word when the pattern carries one (*"this"*, *"attached"*, *"at
/// your location"*), and as the condition's definition otherwise.
///
/// Exhaustive on the event, like [`trigger_matches`]; it assumes that function
/// has already paired the pattern with the event's condition.
fn scope_matches(
    state: &GameState,
    cards: &CardRegistry,
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
        // (Obscuring Fog 01168), and "by 2 or more" / "by 2 or less"
        // (Scavenging 01073, "Look what I found!" 01079), when the pattern
        // says so.
        TimingEvent::SkillTestResolved { .. } => {
            let EventPattern::SkillTestResolved {
                outcome,
                tested_location,
                margin,
                ..
            } = pattern
            else {
                unreachable!(
                    "scope_matches: trigger_matches paired {pattern:?} with SkillTestResolved, \
                     but only EventPattern::SkillTestResolved has that condition"
                )
            };
            let test = state.current_skill_test();
            let location_ok = match tested_location {
                TestedLocationScope::Attached => test
                    .and_then(|test| test.tested_location)
                    .is_some_and(|tested| attached_to(state, source, tested)),
                TestedLocationScope::Any => true,
            };
            // Read live off the stored result, so a re-total that moves only
            // the margin is what the match sees — and read in the pattern's
            // direction, so a stored result a re-total has flipped the other
            // way matches no margin bound at all.
            let margin_ok = margin.is_none_or(|bound| {
                test.and_then(|test| test.resolved)
                    .and_then(|resolved| match outcome {
                        TestOutcome::Success => resolved.succeeded_by(),
                        TestOutcome::Failure => resolved.failed_by(),
                    })
                    .is_some_and(|by| bound.admits(by))
            });
            location_ok && margin_ok
        }
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
        // the cells after it (ADR 0009). "After **you** take" (Psychosis 01099)
        // names no card: its whole scope is the controller, in
        // [`trigger_matches`].
        TimingEvent::DamageDealt { assignment, .. } => match pattern {
            EventPattern::HarmTaken { .. } => true,
            _ => self_scoped(source, |instance| {
                assignment.asset_damage.contains_key(&instance)
            }),
        },
        // "After **Research Librarian** enters play" (01032): the entered
        // instance itself.
        TimingEvent::EnteredPlay { instance, .. } => {
            self_scoped(source, |candidate| candidate == *instance)
        }
        // "When you would discover 1 or more clues **at your location**" (Cover
        // Up 01007).
        TimingEvent::DiscoverClues { location, .. } => {
            location_of(state, controller) == Some(*location)
        }
        // "After you play a **[[Spell]]** card" (Heirloom of Hyperborea 01012)
        // and "When you draw a **non-weakness treachery** card" (Ward of
        // Protection 01065): the played or drawn card's printed metadata passes
        // the pattern's filter. Not a narrowing of where the ability's card is,
        // but it needs the registry, which [`trigger_matches`] does not take.
        TimingEvent::CardPlayed { code, .. } | TimingEvent::EncounterCardDrawn { code, .. } => {
            let Some(filter) = pattern.card_filter() else {
                unreachable!(
                    "scope_matches: trigger_matches paired {pattern:?} with {event:?}, but only \
                     a pattern carrying a card filter has that condition"
                )
            };
            (cards.metadata_for)(code).is_some_and(|meta| filter.matches(meta))
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
        | TimingEvent::DamagePlaced { .. }
        | TimingEvent::EnemyEvaded { .. }
        | TimingEvent::TurnBegan { .. }
        | TimingEvent::ChaosTokenRevealed { .. } => true,
    }
}

/// A self-binding narrowing — *"Guard Dog"*, *"Research Librarian"* — read
/// against `source`: a card instance must pass `is_self`, and a board source
/// with no instance (a location, an enemy, the act, the agenda) never does.
///
/// A card in hand, or on top of a discard pile, has no instance on the board
/// for the self to be, and passes unread: whether its ability is read at all
/// is [`triggers_at`]'s zone gate.
/// So a Fast event declaring the pattern is played in that condition's window
/// unscoped by it, as `trigger_initiation.rs` relies on.
fn self_scoped(source: CandidateSource, is_self: impl Fn(CardInstanceId) -> bool) -> bool {
    source.is_play() || source.instance().is_some_and(is_self)
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
        && (cards.metadata_for)(&card.code).is_some_and(CardMetadata::is_weakness)
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
