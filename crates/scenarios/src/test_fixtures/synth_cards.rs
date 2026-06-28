//! Synthetic test cards used by Phase-4's integration tests.
//!
//! These don't exist in any printed pack — they're vehicles for
//! proving engine wiring end-to-end without depending on real corpus
//! cards. The card codes use an underscore prefix (`_synth_*`) to
//! guarantee no collision with `ArkhamDB`'s digit-prefixed codes.
//!
//! Exposed alongside [`TEST_REGISTRY`] — integration tests install
//! this registry so the on-draw path resolves against synthetic cards
//! that are guaranteed not to collide with real `ArkhamDB` codes
//! (underscore-prefix), rather than depending on a specific corpus
//! card existing. The `cards` crate is still compiled in as a
//! workspace dep — what `TEST_REGISTRY` isolates is the *runtime*
//! registry lookup, not the compile-time footprint.

use std::sync::OnceLock;

use game_core::card_data::{
    CardKind, CardMetadata, Class, HealthValue, Prey, SkillIcons, Spawn, SpawnLocation,
};
use game_core::card_registry::{CardRegistry, EligibilityFn, NativeEffectFn};
use game_core::dsl::{
    choose_one, forced_on_event, gain_resources, native, on_play, reaction_on_event, revelation,
    Ability, Effect, EventPattern, EventTiming, InvestigatorTarget,
};
use game_core::engine::{Cx, EngineOutcome, EvalContext};
use game_core::event::{Event, TraumaKind};
use game_core::state::CardCode;

/// Code for the synthetic location used by the synth-enemy's spawn
/// rule. Underscore prefix guarantees no collision with
/// `ArkhamDB`'s digit-prefixed real codes. Referenced from
/// [`crate::test_fixtures::synthetic::setup`] when stamping the demo
/// location's `code` field.
pub const SYNTH_LOC_CODE: &str = "_synth_loc";

/// Code for the synthetic spawn-bearing enemy.
///
/// Carries `SpawnLocation::Specific(SYNTH_LOC_CODE)` so the on-draw
/// path's enemy arm has something to spawn during the integration
/// test in `crates/scenarios/tests/encounter_spawn.rs`. No abilities
/// (no Revelation, no Activated triggers) — the proof we need is
/// "enemy spawns at the right location, engages the right
/// investigator," not anything ability-driven.
pub const SYNTH_ENEMY_CODE: &str = "_synth_enemy";

/// Code for the synthetic treachery. Underscore prefix guarantees no
/// collision with `ArkhamDB`'s digit-prefixed five-char codes.
pub const SYNTH_TREACHERY_CODE: &str = "_synth_treachery";

/// Code for the synthetic surge-bearing treachery. Its Revelation
/// is the same trivial "gain 1 resource" as [`SYNTH_TREACHERY_CODE`];
/// the load-bearing difference is `surge: true` on the metadata,
/// which drives the surge re-draw path in the per-card sub-sequence
/// (Rules Reference p.19, p.24 1.4 step 5).
pub const SYNTH_SURGE_TREACHERY_CODE: &str = "_synth_surge_treachery";

/// Code for a synthetic treachery whose Revelation is a top-level
/// [`Effect::ChooseOne`] (gain 2 vs gain 5 resources) — i.e. it suspends
/// **directly** into a choice, *not* nested inside a skill test (the Crypt
/// Chill 01167 shape). The #380 motivating case: before the `EncounterCard`
/// frame, its disposal was stranded because the `pending_revelation_discard`
/// slot was only flushed by the skill-test driver.
pub const SYNTH_CHOICE_TREACHERY_CODE: &str = "_synth_choice_treachery";

/// Code for the synthetic Fast event. Used to test the `MythosAfterDraws`
/// window's push-then-scan ordering fix: a Fast event in hand during
/// Mythos must keep the window open (not auto-skip) and must be
/// closeable via `ResolveInput::Skip` after playing (or without
/// playing, per the player's choice).
///
/// The card's `OnPlay` effect is trivially "gain 1 resource" — the
/// effect itself is unimportant; what matters is `is_fast: true` and
/// `card_type: Event` so `check_play_card`'s timing gate allows it
/// inside a permissive window.
pub const SYNTH_FAST_EVENT_CODE: &str = "_synth_fast_event";

/// Code for the synthetic Cover-Up-shaped treachery (C5a #236). Carries a
/// `WouldDiscoverClues` before-timing interrupt + a `GameEnd` forced
/// trauma, both backed by Native effects on [`TEST_REGISTRY`]. Underscore
/// prefix guarantees no collision with real `ArkhamDB` codes.
pub const SYNTH_COVER_UP_CODE: &str = "_synth_cover_up";

/// Native-effect tag: discard the replaced clue count from the synthetic
/// Cover Up (C5a #236).
pub const SYNTH_COVER_UP_DISCARD_TAG: &str = "_synth_cover_up:discard_clues";

/// Native-effect tag: suffer 1 mental trauma at game end if the synthetic
/// Cover Up still holds clues (C5a #236).
pub const SYNTH_COVER_UP_TRAUMA_TAG: &str = "_synth_cover_up:trauma";

/// Eligibility tag: the synthetic Cover Up's discover-replacement reaction may
/// be offered only while it still holds clues (RR p.2 potential gate; #368).
/// Mirrors the real Cover Up's `01007:has_clues`.
pub const SYNTH_COVER_UP_HAS_CLUES_TAG: &str = "_synth_cover_up:has_clues";

/// Static metadata for the synthetic treachery. Only `code`/`name`/the
/// `Treachery` kind carry meaning for the tests.
fn synth_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_TREACHERY_CODE.to_owned(),
        name: "Synthetic Treachery".to_owned(),
        text: Some("Revelation - You gain 1 resource. (Synthetic; not a printed card.)".to_owned()),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_treachery_metadata)
}

/// Metadata for the choice-Revelation treachery (#380). A one-shot treachery
/// shell; the load-bearing part is its `Effect::ChooseOne` Revelation in
/// [`abilities_for`].
fn synth_choice_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_CHOICE_TREACHERY_CODE.to_owned(),
        name: "Synthetic Choice Treachery".to_owned(),
        text: Some(
            "Revelation - Choose one: gain 2 resources; or gain 5 resources. \
             (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        ..synth_treachery_metadata()
    }
}

fn synth_choice_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_choice_treachery_metadata)
}

fn synth_enemy_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_ENEMY_CODE.to_owned(),
        name: "Synthetic Enemy".to_owned(),
        text: Some("Spawn: Synthetic Location. (Synthetic; not a printed card.)".to_owned()),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        weakness: false,
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: Some(HealthValue::Fixed(1)),
            victory: None,
            spawn: Some(Spawn {
                location: SpawnLocation::Specific(SYNTH_LOC_CODE.to_owned()),
            }),
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    }
}

fn synth_enemy_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_enemy_metadata)
}

fn synth_surge_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_SURGE_TREACHERY_CODE.to_owned(),
        name: "Synthetic Surge Treachery".to_owned(),
        text: Some(
            "Revelation - You gain 1 resource. Surge. \
             (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: true,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_surge_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_surge_treachery_metadata)
}

fn synth_fast_event_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_FAST_EVENT_CODE.to_owned(),
        name: "Synthetic Fast Event".to_owned(),
        text: Some(
            "Fast. Play at any player window. \
             You gain 1 resource. (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 3,
            play_only_during_turn: false,
        },
    }
}

fn synth_fast_event_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_fast_event_metadata)
}

fn synth_cover_up_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_COVER_UP_CODE.to_owned(),
        name: "Synthetic Cover Up".to_owned(),
        text: Some(
            "Reaction: when you would discover clues at your location, \
             discard that many from this card instead. Forced: at game end, \
             if any clues remain, suffer 1 mental trauma. (Synthetic.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        weakness: true,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_cover_up_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_cover_up_metadata)
}

/// Native: discard the replaced clue count from the interrupting card
/// instance (Cover Up 01007's "discard that many from Cover Up instead").
fn synth_cover_up_discard(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    // The seam threads the replaced count via `clue_discovery_count`; a
    // missing value means a wiring regression, not a legal 0-clue discard.
    debug_assert!(
        ctx.clue_discovery_count().is_some(),
        "synth_cover_up_discard: clue_discovery_count not threaded"
    );
    let count = ctx.clue_discovery_count().unwrap_or(0);
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "synth_cover_up_discard: no source instance".into(),
        };
    };
    if let Some(inv) = cx.state.investigators.get_mut(&ctx.controller) {
        for card in inv
            .threat_area
            .iter_mut()
            .chain(inv.cards_in_play.iter_mut())
        {
            if card.instance_id == source {
                let take = count.min(card.clues);
                card.clues -= take;
                break;
            }
        }
    }
    EngineOutcome::Done
}

/// Native: at game end, if the source card holds any clues, suffer 1
/// mental trauma (Cover Up 01007's Forced).
fn synth_cover_up_trauma(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "synth_cover_up_trauma: no source instance".into(),
        };
    };
    let has_clues = cx
        .state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| {
            inv.controlled_card_instances()
                .any(|c| c.instance_id == source && c.clues > 0)
        });
    if has_clues {
        cx.events.push(Event::TraumaSuffered {
            investigator: ctx.controller,
            kind: TraumaKind::Mental,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

/// `metadata_for` function pointer used by [`TEST_REGISTRY`].
///
/// Falls through to `game_core::test_support::metadata_for_test_inv` for
/// the synthetic investigator code (`TEST_INV`) used by `test_investigator()`.
/// After cp2a `max_health()`/`max_sanity()` read from the registry; the
/// fallthrough makes capacity reads work without installing a separate registry.
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_metadata_static()),
        SYNTH_SURGE_TREACHERY_CODE => Some(synth_surge_treachery_metadata_static()),
        SYNTH_CHOICE_TREACHERY_CODE => Some(synth_choice_treachery_metadata_static()),
        SYNTH_FAST_EVENT_CODE => Some(synth_fast_event_metadata_static()),
        SYNTH_COVER_UP_CODE => Some(synth_cover_up_metadata_static()),
        _ => game_core::test_support::metadata_for_test_inv(code),
    }
}

/// `abilities_for` function pointer used by [`TEST_REGISTRY`].
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE | SYNTH_SURGE_TREACHERY_CODE => {
            Some(vec![revelation(gain_resources(InvestigatorTarget::You, 1))])
        }
        // #380: a Revelation that suspends *directly* into a choice (two
        // resource-gain branches), unlike Crypt Chill's choice nested inside a
        // skill test.
        SYNTH_CHOICE_TREACHERY_CODE => Some(vec![revelation(choose_one([
            gain_resources(InvestigatorTarget::You, 2),
            gain_resources(InvestigatorTarget::You, 5),
        ]))]),
        SYNTH_FAST_EVENT_CODE => Some(vec![on_play(gain_resources(InvestigatorTarget::You, 1))]),
        SYNTH_COVER_UP_CODE => Some(vec![
            reaction_on_event(
                EventPattern::WouldDiscoverClues,
                EventTiming::When,
                // Discard from self, then cancel the discovery (Axis D #336) —
                // mirrors the real Cover Up 01007 (`cover_up`).
                Effect::Seq(vec![native(SYNTH_COVER_UP_DISCARD_TAG), Effect::Cancel]),
            )
            .with_eligibility(SYNTH_COVER_UP_HAS_CLUES_TAG),
            forced_on_event(
                EventPattern::GameEnd,
                EventTiming::After,
                native(SYNTH_COVER_UP_TRAUMA_TAG),
            ),
        ]),
        // SYNTH_ENEMY_CODE intentionally returns None — the synthetic
        // enemy has no Revelation effect; the spawn handler is the
        // only thing exercised by the integration test.
        _ => None,
    }
}

/// `native_effect_for` function pointer used by [`TEST_REGISTRY`].
fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        SYNTH_COVER_UP_DISCARD_TAG => Some(synth_cover_up_discard),
        SYNTH_COVER_UP_TRAUMA_TAG => Some(synth_cover_up_trauma),
        _ => None,
    }
}

/// True while the synthetic Cover Up instance (the firing source) still holds
/// clues to discard — read-only mirror of [`synth_cover_up_discard`]'s lookup.
fn synth_cover_up_has_clues(state: &game_core::state::GameState, ctx: &EvalContext) -> bool {
    let Some(source) = ctx.source else {
        return false;
    };
    state.investigators.get(&ctx.controller).is_some_and(|inv| {
        inv.threat_area
            .iter()
            .chain(inv.cards_in_play.iter())
            .any(|c| c.instance_id == source && c.clues > 0)
    })
}

/// `native_eligibility_for` function pointer used by [`TEST_REGISTRY`].
fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        SYNTH_COVER_UP_HAS_CLUES_TAG => Some(synth_cover_up_has_clues as EligibilityFn),
        _ => None,
    }
}

/// Ready-made [`CardRegistry`] backed by this module's synthetic
/// cards. Integration tests install this via
/// [`game_core::card_registry::install`] instead of `cards::REGISTRY`
/// so they don't pull in the full corpus.
///
/// Process-isolated: each `cargo test --test` binary gets its own
/// process, so this install doesn't collide with `cards::REGISTRY`
/// installs in other test binaries.
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
    native_effect_for,
    native_eligibility_for,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_resolves_synth_treachery() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let meta = metadata_for(&code).expect("synth treachery must resolve");
        assert_eq!(meta.code, SYNTH_TREACHERY_CODE);
        assert_eq!(meta.card_type(), game_core::card_data::CardType::Treachery);
    }

    #[test]
    fn metadata_for_returns_none_for_unknown_code() {
        let code = CardCode("not_in_synth_registry".into());
        assert!(metadata_for(&code).is_none());
    }

    #[test]
    fn abilities_for_returns_one_revelation_ability() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let abilities = abilities_for(&code).expect("synth treachery must have abilities");
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, game_core::dsl::Trigger::Revelation,);
    }

    #[test]
    fn test_registry_dispatches_to_module_functions() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        assert!((TEST_REGISTRY.metadata_for)(&code).is_some());
        assert!((TEST_REGISTRY.abilities_for)(&code).is_some());
    }

    #[test]
    fn cover_up_fixture_has_interrupt_and_gameend_abilities() {
        let code = CardCode(SYNTH_COVER_UP_CODE.into());
        let abilities = abilities_for(&code).expect("cover up abilities");
        assert_eq!(abilities.len(), 2);
        assert!(matches!(
            abilities[0].trigger,
            game_core::dsl::Trigger::OnEvent {
                pattern: game_core::dsl::EventPattern::WouldDiscoverClues,
                timing: game_core::dsl::EventTiming::When,
                ..
            }
        ));
        assert!(matches!(
            abilities[1].trigger,
            game_core::dsl::Trigger::OnEvent {
                pattern: game_core::dsl::EventPattern::GameEnd,
                ..
            }
        ));
    }

    #[test]
    fn native_effect_for_resolves_cover_up_tags() {
        assert!(native_effect_for(SYNTH_COVER_UP_DISCARD_TAG).is_some());
        assert!(native_effect_for(SYNTH_COVER_UP_TRAUMA_TAG).is_some());
        assert!(native_effect_for("nope").is_none());
    }

    #[test]
    fn metadata_for_resolves_synth_enemy() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        let meta = metadata_for(&code).expect("synth enemy must resolve");
        assert_eq!(meta.code, SYNTH_ENEMY_CODE);
        assert_eq!(meta.card_type(), game_core::card_data::CardType::Enemy);
        let CardKind::Enemy { spawn, .. } = &meta.kind else {
            panic!("synth enemy must be an Enemy kind");
        };
        let spawn = spawn.as_ref().expect("synth enemy must carry a spawn rule");
        match &spawn.location {
            game_core::card_data::SpawnLocation::Specific(code) => {
                assert_eq!(code, SYNTH_LOC_CODE);
            }
        }
    }

    #[test]
    fn abilities_for_synth_enemy_returns_none() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        assert!(abilities_for(&code).is_none());
    }
}
