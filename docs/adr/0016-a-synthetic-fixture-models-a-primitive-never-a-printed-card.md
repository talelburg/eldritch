# A synthetic fixture models a primitive, never a printed card

`crates/scenarios/src/test_fixtures/synth_cards.rs` defines `_synth_cover_up`, a
hand-written treachery carrying a `DiscoverClues` reaction and a `GameEnd`
forced. Cover Up 01007 is a real card, it is in the pinned snapshot, and it is
implemented at `crates/cards/src/impls/cover_up.rs`. The fixture's eligibility
predicate `synth_cover_up_has_clues` (`synth_cards.rs:406-416`) is a byte-for-byte
copy of the real `has_clues` (`cover_up.rs:109-119`) — and the fixture drops the
card's first printed line. 01007
(`data/arkhamdb-snapshot/pack/core/core.json` — it is a player weakness, not
encounter-deck content) reads:

> **Revelation** - Put Cover Up into play in your threat area, with 3 clues on it.
> \[reaction\] When you would discover 1 or more clues at your location: Discard that many clues from Cover Up instead.
> **Forced** - When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma.

The real impl opens with `revelation(put_into_threat_area_with_clues(CODE, 3))`
(`cover_up.rs:71`). The synthetic has no `Trigger::Revelation` at all. That gap was
found, and documented rather than closed, by a later test author
(`crates/cards/tests/upkeep_draw_revelation.rs:14-17`):

> Uses the real `cards::REGISTRY` because the synthetic Cover Up fixture has no
> `Trigger::Revelation` (it would push nothing and never trip the bug); only the
> real 01007 self-places into the threat area with 3 clues.

`docs/agents/standards.md` already forbids this on the production side —
*"Never silently approximate a card"* — for the reason that the playability gate
would hand a player a card the simulator resolves incorrectly. A fixture does
something narrower and harder to see: it hands a *reviewer* a test that reads as
evidence about a card and is evidence about a hand-written approximation of one.
Nothing checks a synthetic against the snapshot, because there is nothing to
check it against.

Two more instances, in two other crates. `crates/scenarios/tests/opening_hand_weaknesses.rs:60-63`
pads a player deck with `01001`–`01004` — Roland Banks, Daisy Walker, "Skids"
O'Toole and Agnes Baker, four real *investigator* cards, in a zone none of them
can legally occupy — because the test needs four opaque tokens and reached for
real codes. And `crates/web/tests/board.rs:161` asserts
`html.contains("_synth_fast_event")` — over a hand seeded at `:119-122` — while
the file installs `install_test_registry()` (`:33`),
which knows only `TEST_INV` and the terminal act/agenda cards: the assertion
passes **because** the code fails to resolve to a name. A test that appears to
pin card rendering pins the unresolved-code fallback instead.

**A synthetic fixture may model an engine primitive. It may not impersonate a
printed card.** The line is checkable against a diff, which is the property that
made it the rule rather than the criterion behind it.

That criterion is worth stating once, because it decides the cases the line does
not reach: **a test's substrate is chosen by what its failure should mean.** A
test asking *does the hunter-movement rule work* wants an enemy whose only
interesting property is `Hunter`, and a real Ghoul Priest drags in stats that can
mask the bug. A test asking *does The Gathering play* wants The Gathering. This
is not an argument that realism wins — realism as the axis would delete tests
that catch real engine bugs.

## The three categories

**Primitive builders are shared, unconditional, and mandatory.**
`game_core::test_support::fixtures` — `test_investigator`, `test_location`,
`test_enemy` — is not a shortcut around real cards. `Investigator` and `Location`
are `#[non_exhaustive]`, so as the module's own doc says, *"downstream test
crates cannot construct them via struct literal — they MUST go through these
fixtures"* (`fixtures.rs:8-18`). roughly 1,300 call sites across 168 files depend on that,
and the layering is why: `game-core` cannot reach `cards` by crate direction, so
there is no real-card alternative at the kernel. These build entities, not cards.

**Probe cards are test-local.** A degenerate code that isolates one mechanism has
one reader, and belongs in that reader's file. `crates/cards/tests/` already does
this at scale and correctly: 11 of its 81 binaries install `CardRegistry::EMPTY`
plus hand-written probes — `timing_cells.rs` (`_tc_when` / `_tc_at` / `_tc_after`),
`clue_discovery_cells.rs`, `enemy_attack_cells.rs`, `ability_source_*.rs`
(`SRCCTL01`…), `zero_action_ability_sources.rs` — and never touch the corpus. Each
probe exists to make one cell fire in isolation, which no printed card does
cleanly. The remaining 70 install the real `cards::REGISTRY`. That split is the
posture already working; this ADR names it rather than inventing it.

**The toy scenario splits at the seam where sharing is real.** The
`ScenarioModule` shell — a no-op `apply_resolution`, `resolve_symbol: None`,
`layout: &[]`, and the local registry install — is identical across every reader
and stays shared. The `setup()` *state* is local, composed per test with
`GameStateBuilder`. `crates/server/tests/common/mod.rs:19-42` already demonstrates
this: a ten-line mock scenario, with a comment naming exactly where the shared
fixture's shape was wrong for it — *"seating runs at creation via `seat_and_open`,
so the pre-seeded investigator and turn order are NOT injected here."*

## The `test_fixtures` feature is deleted, not narrowed

`crates/scenarios/Cargo.toml:24-30` declares the feature `default`-on and says it
*"may be enabled by downstream crates (server / web integration tests) that want
the fixture without writing their own."* Neither ever did. `TEST_REGISTRY` has
eleven readers, all inside `crates/scenarios/tests/`. The one crate the feature was
built for looked at the fixture, wanted a different shape, and wrote its own.

The feature existed because cargo compiles integration-test binaries without
`cfg(test)` on their own crate, so a `#[cfg(any(test, feature = "test_fixtures"))]`
gate is inactive there. A `tests/common/` module — the pattern `crates/server/tests/`
already uses — sidesteps that entirely. **The fixture moves to
`crates/scenarios/tests/common/`, and the feature goes.**

This makes one property structural rather than maintained. `module_for`
(`crates/scenarios/src/lib.rs:37-38`) currently routes `"synthetic"` whenever the
feature is on, and `crates/server/src/lifecycle.rs:22` takes the scenario id
straight off the HTTP request — so the toy scenario is startable in production by
POSTing `{"scenario_id":"synthetic"}`, and whether it is depends on Cargo.toml
discipline. A fixture under `tests/` cannot be referenced from `src/` at all.
`synthetic_resolution.rs` and `closing_demo.rs` install a locally-built
`ScenarioRegistry` in place of the crate's, which is the more honest thing for
them to do — they test the engine's resolution hook, not the crate's routing table.

## One synthetic is transitional, and carries its terminal condition

`_synth_surge_treachery` stays, and it is scaffolding. The corpus has no Surge card
because `crates/card-data-pipeline/src/main.rs:955,970` hardcodes `surge: false`
for every treachery — the comment at `:897` calls it the *"not-yet-parsed"*
default (#138). The snapshot has 156 cards whose text carries Surge, including
False Lead 01136 in the Core set. That parsing is
[#138](https://github.com/talelburg/eldritch/issues/138), which already names
this fixture in its own Context and is milestoned to phase 9. **When #138 lands,
the surge fixture and its two tests flip to a real card unless the synthetic is
better tailored to what the test asks** — the criterion above, not an automatic
sweep. `peril` is stubbed at the same site and inherits the same clause, and has
no fixture to retire because nothing tests it today.

`_synth_choice_treachery` is not transitional. No implemented card has a
top-level choice Revelation: Crypt Chill 01167's choice sits under a skill test,
and stripping that wrapper is the whole point of the probe.

## Considered options

**Real cards everywhere.** The strongest form of the realism argument, and it
fails on the 11 probe binaries in `crates/cards/tests/`: no printed card fires one
timing cell in isolation, so `timing_cells.rs` would lose the property it exists
to pin. It also cannot be followed at the kernel, where `#[non_exhaustive]` and
crate direction make primitive builders the only way to build a state.

**Keep the shared toy scenario as-is.** Rejected because it serves three different
needs under one name — *a scenario that will not resolve mid-round*
(`upkeep_phase.rs`), *a two-card act deck that ends in a couple of clicks*
(`synthetic_resolution.rs`), *a full four-phase walk in ten actions*
(`closing_demo.rs`) — and the consumers already bend it: `with_encounter_deck`
exists purely as an override hook, `upkeep_hand_size.rs` pads with an unregistered
`01999`, and `hunter_movement.rs`'s second test abandons the fixture to hand-build
a diamond map.

**Scope the rule to `crates/scenarios/`.** The narrow, shippable version, and it
leaves the largest cluster undocumented: `game_core::test_support` is
unconditionally `pub`, ships in the released rlib, and is where the next author
reaches first.

## Consequences

**`docs/agents/standards.md` § Test layering owns the operative rule and cites
this ADR**; the ADR owns the reasoning. That section also gains
`crates/scenarios/tests/`, which it does not currently mention, and loses a
`TestGame` builder that does not exist anywhere in the workspace — the type is
`GameStateBuilder` (`crates/game-core/src/state/builder.rs:52`), a production type
re-exported from `test_support` (`test_support/mod.rs:196`). `TestGame` was real:
PR #253 promoted it to `GameStateBuilder` for #251, and `standards.md` was never
updated. Two dated phase docs still name it and keep it, since they record what
shipped at the time; each gains the rename inline so the pointer does not dangle.
`docs/superpowers/` is a dated archive and is not rewritten.

**`crates/scenarios/tests/` becomes scenario-level end-to-end only.** It goes from
16 files to five: the three `the_gathering*` files plus `issue_476_fast_window.rs`
and `issue_482_advance.rs`, which stay because they exercise scenario data in situ
— `issue_482` drives the doom cascade through The Gathering's own agenda 01105.
The line is *does it exercise scenario data*, not *is it a full walk*; a full-walk
rule would evict `the_gathering_symbols.rs` and its 12 tests against reference
card 01104.

**Four test groups drop a layer to `crates/game-core/tests/`.** `upkeep_phase.rs`,
`upkeep_hand_size.rs`, `synthetic_resolution.rs` and `hunter_movement.rs`'s
diamond-map test have no card dependency at all; they needed a scenario module,
which is now a locally-built shell. Three groups are deleted as duplicates:
`cover_up_interrupt.rs` (superseded by `crates/cards/tests/cover_up.rs`, against
the real 01007 and going further), `mythos_phase.rs`'s Fast-window pair
(superseded by `issue_476_fast_window.rs` with Magnifying Glass 01030), and
`closing_demo.rs`'s walks (superseded by `the_gathering_resolutions.rs`) — whose
`replay_with_roundtrip` determinism check is ported onto a Gathering walk rather
than lost.

**Naming stops implying a shared fixture that is not there.** `_synth_enemy` has
four independent definitions with independently hand-written stats
(`synth_cards.rs:45`, `game-core/src/engine/dispatch/encounter.rs:1274`,
`card-dsl/src/card_data.rs:1037`, `game-core/tests/enemy_encounter_spawn.rs:30`)
plus three bare uses that carry no stats at all (`game-core/src/event.rs:758,771`,
`cards/tests/window_revalidation.rs:314`); `_synth_loc` has three. Test-local codes take a per-binary
prefix, as `crates/cards/tests/` already does with `_tc_*` / `_cd_*` / `_ea_*` /
`SRC*`. Three codes — `_synth_asset`, `_synth_card`, `_synth_reaction` — are
defined nowhere at all.

**The rule is enforced by review, not by a lint.** A regex banning ArkhamDB's
`^\d{5}[a-z]?$` shape in test code would have caught the `01001`–`01004` filler,
and would fire constantly on the 70 binaries that legitimately name real codes.
Distinguishing *names 01007 because it is testing Cover Up* from *names 01001 as
filler* needs the surrounding intent. `standards.md` is already how this repo
binds rules the compiler cannot check, and `code-review`'s Standards axis reads
it.

**#864 is re-scoped from a narrow feature-flag cleanup into the tracking issue for
this work**, with the migration's steps as child issues. Its original
acceptance criterion — that the production `server` binary does not compile
`scenarios::test_fixtures` — is satisfied by the relocation above rather than by
`default-features = false`, and the phase-4 note it was going to update
(`docs/phases/phase-4-scenario-plumbing.md:78`) points here instead.
