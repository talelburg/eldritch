//! Test-only support: fixtures, event-assertion macros, and a
//! convenience re-export of the production [`GameStateBuilder`].
//!
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless
//! of where they import the supporting types from.
//!
//! The state builder itself lives in [`crate::state`] (it constructs
//! production `GameState`s, not just test ones); it is re-exported here
//! so the existing test imports keep working.

pub mod assertions;
pub mod fixtures;
pub mod resolver;

/// Synthetic investigator-card code for unit tests. Registered by
/// [`install_test_registry`] with 8 health / 8 sanity (mirroring the legacy
/// `test_investigator` capacity).
pub const TEST_INV: &str = "TEST_INV";

fn test_inv_metadata() -> &'static crate::card_data::CardMetadata {
    use crate::card_data::{CardKind, CardMetadata, Class, Skills};
    static M: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: TEST_INV.to_owned(),
        name: "Test Investigator".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind: CardKind::Investigator {
            class: Class::Neutral,
            skills: Skills {
                willpower: 3,
                intellect: 3,
                combat: 3,
                agility: 3,
            },
            health: 8,
            sanity: 8,
        },
    })
}

/// Printed-code prefix for the synthetic **terminal** act/agenda cards.
///
/// A card is terminal because it is last in its deck (ADR 0013), and a terminal
/// card ends the scenario by *running an effect on its reverse* — so a fixture
/// that wants "advancing this act/agenda ends the scenario" needs a card whose
/// abilities the registry can serve. [`terminal_code`] mints one per printed
/// resolution number and [`abilities_for_terminal`] serves its reverse.
///
/// Synthetic rather than a real corpus code (01107 / 01110): a unit test that is
/// about *terminality* should not break when a snapshot bump moves the content
/// it was never asserting on.
pub const TEST_TERMINAL_PREFIX: &str = "_TEST_TERM_R";

/// The synthetic terminal card that reaches printed resolution point `n`, e.g.
/// `terminal_code(1)` → `_TEST_TERM_R1`. Put it last in an act or agenda deck
/// and advancing it ends the scenario at `Resolution(n)`.
#[must_use]
pub fn terminal_code(n: u8) -> crate::state::CardCode {
    crate::state::CardCode::new(format!("{TEST_TERMINAL_PREFIX}{n}"))
}

/// Abilities lookup for the synthetic terminal cards ([`terminal_code`]).
///
/// The reverse is declared on both the act-advanced and the agenda-advanced
/// condition, in the `after` cell — the cell every real on-advance reverse uses
/// (01105, 01106, 01108, 01109), because the flip is step 2 of the Rules
/// Reference's advance procedure rather than a triggered ability. One card
/// serving both decks keeps the fixture surface to a single code family.
///
/// Composed into [`install_test_registry`], and into out-of-crate mocks the way
/// [`metadata_for_test_inv`] is:
///
/// ```ignore
/// fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
///     game_core::test_support::abilities_for_terminal(code)
///         .or_else(|| /* mock-specific lookups */)
/// }
/// ```
#[must_use]
pub fn abilities_for_terminal(code: &crate::state::CardCode) -> Option<Vec<crate::dsl::Ability>> {
    use crate::dsl::{forced_on_event, reach_resolution, EventPattern, EventTiming};
    let n: u8 = code
        .as_str()
        .strip_prefix(TEST_TERMINAL_PREFIX)?
        .parse()
        .ok()?;
    Some(vec![
        forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            reach_resolution(n),
        ),
        forced_on_event(
            EventPattern::AgendaAdvanced,
            EventTiming::After,
            reach_resolution(n),
        ),
    ])
}

/// Install `base` with the synthetic terminal cards ([`terminal_code`]) composed
/// into its `abilities_for`, so a fixture whose act/agenda deck ends in one gets
/// its reverse served alongside whatever `base` already knows.
///
/// For **integration tests in other crates**, which install a real registry
/// (`cards::REGISTRY`, `synth_cards::TEST_REGISTRY`) into the process-global
/// `OnceLock` and so cannot compose at the definition site the way
/// [`install_test_registry`] does. Call it exactly where the plain install went:
///
/// ```ignore
/// #[ctor::ctor(unsafe)]
/// fn install() {
///     game_core::test_support::install_registry_with_terminal_cards(cards::REGISTRY);
/// }
/// ```
///
/// Idempotent, and — like [`card_registry::install`](crate::card_registry::install)
/// — first-install-wins.
pub fn install_registry_with_terminal_cards(base: crate::card_registry::CardRegistry) {
    use crate::card_registry::CardRegistry;
    use crate::state::CardCode;
    static BASE: std::sync::OnceLock<CardRegistry> = std::sync::OnceLock::new();
    fn abilities_for(code: &CardCode) -> Option<Vec<crate::dsl::Ability>> {
        abilities_for_terminal(code)
            .or_else(|| BASE.get().and_then(|base| (base.abilities_for)(code)))
    }
    let _ = BASE.set(base);
    // `back_abilities_for` rides `..base` deliberately: the terminal cards are
    // synthetic acts/agendas with no reverse side, so there is nothing to
    // compose in, and overriding the slot would switch the *real* registry's
    // back sides off for every test that installs through here (#774).
    let _ = crate::card_registry::install(CardRegistry {
        abilities_for,
        ..base
    });
}

/// Metadata lookup for the synthetic `TEST_INV` investigator code.
///
/// Integration tests in `crates/game-core/tests/` install their own
/// mock `CardRegistry` instead of calling [`install_test_registry`].
/// When those mocks use `test_investigator`, the investigator's
/// `investigator_card.code` is `TEST_INV`. Any code path that reads
/// `max_health()` / `max_sanity()` (damage/horror application, defeat
/// checks) calls `investigator_capacity(TEST_INV)` and needs the
/// registry to know that code. Compose this into the mock's
/// `metadata_for`:
///
/// ```ignore
/// fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
///     game_core::test_support::metadata_for_test_inv(code)
///         .or_else(|| /* mock-specific lookups */)
/// }
/// ```
pub fn metadata_for_test_inv(
    code: &crate::state::CardCode,
) -> Option<&'static crate::card_data::CardMetadata> {
    (code.as_str() == TEST_INV).then(test_inv_metadata)
}

/// Install a minimal game-core test registry that knows `TEST_INV` and the
/// synthetic terminal cards ([`terminal_code`]), and nothing else. Idempotent;
/// safe to call from any test. Capacity-reading code (`max_health()` /
/// `max_sanity()` / soak / defeat) needs this installed, and so does any fixture
/// whose act/agenda deck ends in a terminal card — without the registry its
/// reverse never fires and the advance finds no ending latched.
///
/// One registry for the whole crate because `OnceLock<CardRegistry>` is
/// process-global: a second per-test install would collide (the same constraint
/// that put [`fire_forced_on_enter`] here).
pub fn install_test_registry() {
    use crate::state::CardCode;
    static INSTALL: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALL.get_or_init(|| {
        fn metadata_for(code: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
            (code.as_str() == TEST_INV).then(test_inv_metadata)
        }
        fn abilities_for(code: &CardCode) -> Option<Vec<crate::dsl::Ability>> {
            abilities_for_terminal(code)
        }
        let _ = crate::card_registry::install(crate::card_registry::CardRegistry {
            metadata_for,
            abilities_for,
            back_abilities_for: |_| None,
            native_effect_for: |_| None,
            native_eligibility_for: |_| None,
            native_condition_for: |_| None,
        });
    });
}

pub use crate::state::GameStateBuilder;
pub use fixtures::{
    awaiting_commit_input, awaiting_pick_single_input, test_enemy, test_investigator,
    test_location, test_skill_test,
};
pub use resolver::{
    apply_no_commits, dispatch_turn_action_unchecked, drive, drive_skill_test, perform_skill_test,
    perform_skill_test_no_commits, take_turn_action, ChoiceResolver, ScriptedResolver,
    TakeOneFastPlay, TestSession,
};

/// Test helper: fire forced triggers for an investigator entering a
/// location, returning the `EngineOutcome`. Constructs the internal
/// `ForcedTriggerPoint` so integration tests don't need it public.
///
/// Lives in `test_support` because `queue_forced_triggers` needs a custom
/// `CardRegistry` and `OnceLock<CardRegistry>` is process-global — an
/// in-crate install would collide with `card_registry::tests`. Integration
/// tests in `crates/game-core/tests/` run in separate processes.
/// Wired into `move_action` (`EnteredLocation`); this helper exists for
/// unit-style coverage of the dispatch path in isolation.
pub fn fire_forced_on_enter(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    location: crate::state::LocationId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnteredLocation {
            investigator,
            location,
        },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire one timing cell's forced triggers for a phase ending,
/// returning the `EngineOutcome`. Constructs the internal
/// `ForcedTriggerPoint` so integration tests don't need it public. See
/// `fire_forced_on_enter`.
///
/// The caller names the `cell`, because this helper fires exactly one — the
/// real emit walks all three. A card declaring a different cell than the one
/// asked for here fires nothing, so pass the cell the card under test prints:
/// agenda 01107's *"**Forced** - At the end of the enemy phase"* is
/// [`EventTiming::At`](crate::dsl::EventTiming::At).
pub fn fire_forced_on_phase_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    phase: crate::state::Phase,
    cell: crate::dsl::EventTiming,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::PhaseEnded { phase },
        cell,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire `ForcedTriggerPoint::RoundEnded` against `state`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// round-end Forced abilities (agenda 01107's doom).
pub fn fire_forced_on_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::RoundEnded,
        crate::dsl::EventTiming::At,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: run the Upkeep step-4.6 round-end sequence — `upkeep_phase_end`
/// then the `drive` loop that walks the `RoundEnded` coordinator (#434) —
/// returning the `EngineOutcome`. Suspends on act 01109's "when the round ends"
/// clue-spend reaction window when affordable; resume it with
/// [`resume_round_end_window`]. Requires the `UpkeepPhase` anchor on the stack
/// (the coordinator's teardown pops it).
pub fn run_upkeep_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::upkeep_phase_end(&mut cx);
    crate::engine::drive(&mut cx, out)
}

/// Test helper: run the Enemy step-3.4 phase end — `enemy_phase_end` then the
/// `drive` loop — returning the `EngineOutcome`. The forced abilities keyed to
/// `PhaseEnded { Enemy }` (agenda 01107's Ghoul move) are *queued* by the emit
/// and resolved by the loop, ahead of the Enemy→Upkeep transition (#569).
/// Requires the `EnemyPhase` anchor on the stack (the transition pops it).
pub fn run_enemy_phase_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::enemy_phase_end(&mut cx);
    crate::engine::drive(&mut cx, out)
}

/// Test helper: walk one triggering condition's whole timing sequence — push
/// the [`EmitEvent`](crate::state::Continuation::EmitEvent) coordinator for
/// `event` at the `when` cell and drive it to its next suspension or
/// completion.
///
/// `queue_event` routes only `RoundEnded` through the coordinator until #702, so
/// this is the only way to walk another condition's cells — notably the
/// caller-owned `when`-cell reject (#701). Delete it with the classification.
pub fn run_timing_sequence(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    event: crate::engine::TimingEvent,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    cx.state
        .continuations
        .push(crate::state::Continuation::EmitEvent {
            event,
            step: crate::state::EmitStep::When,
        });
    crate::engine::drive(&mut cx, crate::engine::EngineOutcome::Done)
}

/// Test helper: resume the round-end `when` act-advance reaction window (#434)
/// with `response` (`PickSingle`/`Skip`), driving the coordinator through to its
/// next suspension or completion via the player-action entry.
pub fn resume_round_end_window(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    response: &crate::action::InputResponse,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::apply_player_action(
        &mut cx,
        &crate::action::PlayerAction::ResolveInput {
            response: response.clone(),
        },
    )
}

/// Test helper: fire forced triggers for an act advancing, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_act_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::ActAdvanced { code },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire forced triggers for an agenda advancing, returning
/// the `EngineOutcome`. See `fire_forced_on_enter`. Exercises the agenda
/// reverses (01105 discard/horror, 01106 dig-until-Ghoul).
pub fn fire_forced_on_agenda_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::AgendaAdvanced { code },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire one timing cell's forced triggers for an enemy defeat,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`, and
/// `fire_forced_on_phase_end` for why the caller names the `cell`: act
/// 01110's *"**Objective** - If the Ghoul Priest is Defeated, advance."* is
/// [`EventTiming::At`](crate::dsl::EventTiming::At).
pub fn fire_forced_on_enemy_defeat(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
    cell: crate::dsl::EventTiming,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnemyDefeated { code },
        cell,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire one timing cell's forced triggers for an enemy attack,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`, and
/// `fire_forced_on_phase_end` for why the caller names the `cell`: Silver
/// Twilight Acolyte 01102's *"**Forced** - After Silver Twilight Acolyte
/// attacks: Place 1 doom on the current agenda."* is
/// [`EventTiming::After`](crate::dsl::EventTiming::After).
///
/// Fires the point in isolation, without the attack that would carry it — which
/// is what lets a test read the candidate the scan produced (its source, and the
/// anchor of the interactive acknowledge) without staging a whole Enemy phase.
pub fn fire_forced_on_enemy_attack(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    enemy: crate::state::EnemyId,
    investigator: crate::state::InvestigatorId,
    cell: crate::dsl::EventTiming,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnemyAttacks {
            enemy,
            investigator,
        },
        cell,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire one timing cell's forced triggers for `investigator`'s
/// turn ending, returning the `EngineOutcome`. See `fire_forced_on_enter`, and
/// `fire_forced_on_phase_end` for why the caller names the `cell`: Frozen in
/// Fear 01164's *"**Forced** - At the end of your turn: …"* is
/// [`EventTiming::At`](crate::dsl::EventTiming::At), so a corpus test of the
/// threat-area path wants that cell — the mock-registry callers here declare
/// [`After`](crate::dsl::EventTiming::After) and pass it.
pub fn fire_forced_at_end_of_turn(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    cell: crate::dsl::EventTiming,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EndOfTurn { investigator },
        cell,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire the forced phase of
/// `ForcedTriggerPoint::SkillTestResolved` (Investigate + success), returning
/// the `EngineOutcome`. See `fire_forced_on_enter`. Exercises the threat-area
/// "after successfully investigated" forced path via the controlled-instance
/// scan. It sets up no in-flight `SkillTest` frame, so the location-attachment
/// scan is a no-op here — the attachment path (Obscuring Fog 01168) is
/// exercised end-to-end through a real Investigate instead.
pub fn fire_forced_after_location_investigated(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::queue_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::SkillTestResolved {
            investigator,
            kind: crate::dsl::SkillTestKind::Investigate,
            outcome: crate::dsl::TestOutcome::Success,
        },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: eliminate `investigator` by dealing them `damage`, then run the
/// `drive` loop to completion — so Rules Reference p.10 Elimination finishes,
/// including step 0's weakness game-end abilities when they put the sequence on
/// a continuation frame (#638).
///
/// Lives here — unlike its `fire_forced_*` neighbours, whose reason is the
/// process-global `OnceLock<CardRegistry>` — because the three pieces it
/// composes (`Cx`, `take_damage`, `drive`) are not all reachable from an
/// integration test: `drive` is `pub(crate)`.
///
/// `damage` must be lethal for the investigator's capacity; the caller is
/// responsible for that (the registry it installed answers `max_health()`).
pub fn eliminate_by_damage(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    damage: u8,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::take_damage(&mut cx, investigator, damage);
    crate::engine::drive(&mut cx, crate::engine::EngineOutcome::Done)
}
