# A synthetic fixture models a primitive, never a printed card

**Every `file:line` in this Context section, down to "The three categories",
records the tree as it stood when the decision was taken, before the decision
deleted `crates/scenarios/src/test_fixtures/` (below), so read them against history
rather than the working tree.**

`crates/scenarios/src/test_fixtures/synth_cards.rs` defined `_synth_cover_up`, a
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
real codes. And `crates/web/tests/board.rs:161` asserted
`html.contains("_synth_fast_event")` — over a hand seeded at `:119-122` — while
the file's test registry (`:33`)
knew only `TEST_INV` and the terminal act/agenda cards: the assertion
passed **because** the code failed to resolve to a name. A test that appears to
pin card rendering pins the unresolved-code fallback instead. That last one was
cited here as evidence of the drift rather than fixed by this posture, since it
needed a rendering decision about what an unresolved code should display. That
decision was taken in [#868](https://github.com/talelburg/eldritch/issues/868),
and came out as neither option the issue proposed: rather than registering probe
cards or asserting the fallback, the binary moved wholesale onto the real
`cards::REGISTRY`, so no code it seeds can fail to resolve, and its
`render_state` asserts `card--unknown` never renders. **Where a test's subject is
rendering rather than an isolated mechanism, the real corpus is the substrate** —
the probe exists to strip a mechanism down, and a renderer has none to strip.

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
fixtures"* (`fixtures.rs:8-18`). Hundreds of call sites across `game-core`,
`cards`, `scenarios` and `web` depend on that, and the layering is why:
`game-core` cannot reach `cards` by crate direction, so there is no real-card
alternative at the kernel. These build entities, not cards.

**Probe cards are test-local.** A degenerate code that isolates one mechanism has
one reader, and belongs in that reader's file. `crates/cards/tests/` already does
this at scale and correctly: its probe binaries build a `MockRegistry` of
hand-written probes — `timing_cells.rs` (`_tc_when` / `_tc_at` / `_tc_after`),
`clue_discovery_cells.rs`, `enemy_attack_cells.rs`, `ability_source_*.rs`
(`SRCCTL01`…), `zero_action_ability_sources.rs` — and never touch the corpus. Each
probe exists to make one cell fire in isolation, which no printed card does
cleanly. Most of the rest name the real corpus (`cards::TEST_REGISTRIES`), and
`enemies.rs` and `registry_smoke.rs` read it directly through `cards::by_code`
without a registry. That split is the posture
already working; this ADR names it rather than inventing it.

**The toy scenario splits at the seam where sharing is real.** The
`ScenarioModule` shell — a no-op `apply_resolution`, `resolve_symbol: None`,
`layout: &[]`, and the local scenario registry — is identical across every reader,
so where several tests in one crate need it, it is shared *there*, in that crate's
own `tests/common/`. The `setup()` *state* is local, composed per test with
`GameStateBuilder`. `crates/server/tests/common/mod.rs` demonstrates this: a
ten-line mock scenario, with a comment naming exactly where the shared
fixture's shape was wrong for it — *"seating runs at creation via `seat_and_open`,
so the pre-seeded investigator and turn order are NOT injected here."*

## The `test_fixtures` feature is deleted, not narrowed

`crates/scenarios/Cargo.toml` declared the feature `default`-on and said it
*"may be enabled by downstream crates (server / web integration tests) that want
the fixture without writing their own."* Neither ever did. The fixture's
`TEST_REGISTRY` had eleven readers, all inside `crates/scenarios/tests/`. The one crate the feature was
built for looked at the fixture, wanted a different shape, and wrote its own.

The feature existed because cargo compiles integration-test binaries without
`cfg(test)` on their own crate, so a `#[cfg(any(test, feature = "test_fixtures"))]`
gate is inactive there. Every reader instead builds the `ScenarioModule` shell it
needs locally — the pattern `crates/server/tests/common/mod.rs` already uses — which
is the more honest thing for them to do: they test the engine's resolution hook,
not the crate's routing table. **The fixture is deleted, and the feature with it.**

This makes one property structural rather than maintained. `module_for`
in `crates/scenarios/src/lib.rs` routed `"synthetic"` whenever the feature was
on, and `crates/server/src/lifecycle.rs` takes the scenario id straight off the
HTTP request — so the toy scenario was startable in production by POSTing
`{"scenario_id":"synthetic"}`, and whether it was depended on Cargo.toml
discipline. With nothing to route to, it cannot be. `crates/scenarios/src/lib.rs`
carries a `None`-for-`"synthetic"` test as insurance against a future re-export.

**The fixture is deleted rather than relocated to `crates/scenarios/tests/common/`.**
Every reader builds its own shell, so `tests/common/` would hold a module no test
binary declares `mod common;` for, and cargo compiles nothing under a `tests/`
subdirectory on its own. It would be invisible to `cargo test`, `clippy` and
`rustfmt` alike: dead code that rots silently, in place of the guarantee the
relocation existed to buy.

## One probe is transitional, and carries its terminal condition

The surge probe, `_mpp_surge_treachery` in `crates/cards/tests/mythos_phase_probes.rs`,
is scaffolding. The corpus has no Surge card because
`crates/card-data-pipeline/src/main.rs` emits `surge: false` for every card that
can carry it, a stub its `TODO(#138)` names. The snapshot has 156 cards whose text
carries Surge, including False Lead 01136 in the Core set. That parsing is
[#138](https://github.com/talelburg/eldritch/issues/138), milestoned to phase 9.
**When #138 lands, the surge probe and its two tests flip to a real card unless
the probe is better tailored to what the test asks** — the criterion above, not an
automatic sweep. `peril` is stubbed at the same site and inherits the same clause,
and has no probe to retire because nothing tests it.

The choice-Revelation probe, `_mpp_choice_treachery` in the same file, is not
transitional. No implemented card has a top-level choice Revelation: Crypt Chill
01167's choice sits under a skill test, and stripping that wrapper is the whole
point of the probe.

## Considered options

**Real cards everywhere.** The strongest form of the realism argument, and it
fails on the probe binaries in `crates/cards/tests/`: no printed card fires one
timing cell in isolation, so `timing_cells.rs` would lose the property it exists
to pin. It also cannot be followed at the kernel, where `#[non_exhaustive]` and
crate direction make primitive builders the only way to build a state.

**Keep the shared toy scenario as-is.** Rejected because it served three different
needs under one name — *a scenario that will not resolve mid-round*
(`upkeep_phase.rs`), *a two-card act deck that ends in a couple of clicks*
(`synthetic_resolution.rs`), *a full four-phase walk in ten actions*
(`closing_demo.rs`) — and the consumers already bent it: `with_encounter_deck`
existed purely as an override hook, `upkeep_hand_size.rs` padded with an
unregistered `01999`, and `hunter_movement.rs`'s second test abandoned the fixture
to hand-build a diamond map.

**Scope the rule to `crates/scenarios/`.** The narrow, shippable version, and it
leaves the largest cluster undocumented: `game_core::test_support` is
unconditionally `pub`, ships in the released rlib, and is where the next author
reaches first.

## Consequences

**`docs/agents/standards.md` § Test layering owns the operative rule and cites
this ADR**; the ADR owns the reasoning. That section names
`crates/scenarios/tests/` as a layer, and names the state builder by its type,
`GameStateBuilder`, a production type re-exported from `test_support`.

The per-file disposition behind the consequences below is the table in
[#864](https://github.com/talelburg/eldritch/issues/864); what follows is the shape
of the result, not a second copy to keep in sync.

**`crates/scenarios/tests/` is scenario-level end-to-end only.** Its tests
exercise scenario data in situ: the `the_gathering*` files, and
`issue_476_fast_window.rs` and `issue_482_advance.rs` beside them — `issue_482`
drives the doom cascade through The Gathering's own agenda 01105 — and
`test_registries.rs`, which checks that `scenarios::TEST_REGISTRIES` serves The
Gathering's module and seats a real investigator beside a synthetic one. The line is *does it
exercise scenario data*, not *is it a full walk*; a full-walk rule would evict
`the_gathering_symbols.rs` and its tests against reference card 01104.

**A test with no card dependency lives in `crates/game-core/tests/`.** The upkeep,
hand-size, resolution and hunter-movement tests need a scenario module, not a card,
and build that shell locally. A scenario test that duplicated a corpus test is
deleted rather than kept: `crates/cards/tests/cover_up.rs` tests the real 01007,
`issue_476_fast_window.rs` covers the Fast window with Magnifying Glass 01030, and
`the_gathering_resolutions.rs` covers the full walks, its replay-determinism walk
included.

**Naming stops implying a shared fixture that is not there.** Test-local codes take
a per-binary prefix, as `crates/cards/tests/` does with `_tc_*` / `_cd_*` /
`_ea_*` / `SRC*` / `_mpp_*`. Under the shared `_synth_` prefix, `_synth_enemy` had
four independent definitions with independently hand-written stats, and three
`_synth_*` codes were defined nowhere at all.

**The rule is enforced by review, not by a lint.** A regex banning ArkhamDB's
`^\d{5}[a-z]?$` shape in test code would have caught the `01001`–`01004` filler,
and would fire constantly on the many binaries that legitimately name real codes.
Distinguishing *names 01007 because it is testing Cover Up* from *names 01001 as
filler* needs the surrounding intent. `standards.md` is already how this repo
binds rules the compiler cannot check, and `code-review`'s Standards axis reads
it.

**The production `server` binary does not compile `scenarios::test_fixtures`**,
#864's original acceptance criterion, because the deletion above leaves nothing to
compile rather than because of `default-features = false`.

---

*Folded: #868, #878, #1038.*
