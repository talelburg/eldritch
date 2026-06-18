# Deck-search tutors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable `Effect::SearchDeck` deck-search/tutor primitive plus an `EnteredPlay` reaction trigger, and implement Old Book of Lore 01031 and Research Librarian 01032 on them.

**Architecture:** `Effect::SearchDeck` is a typed, inspectable effect (like `Fight`/`Investigate`/`Heal`) that enumerates eligible cards in a deck region (top-N or entire deck) ∩ a `CardFilter`, takes one to hand via the existing Axis-A choice machinery (RR p.18: obligated-if-any, no decline), and shuffles. `EnteredPlay` is a new reaction `EventPattern` + `WindowKind` riding the existing reaction-window pipeline, emitted from the asset play path. A small reentrancy fix lets a choice fired *from* a reaction window resume correctly.

**Tech Stack:** Rust workspace; `card-dsl` (DSL types), `game-core` (kernel/evaluator/dispatch), `cards` (content + registry). Tests: `cargo test`, per-card `#[cfg(test)]`, integration tests in `crates/cards/tests/`.

## Global Constraints

- CI runs `fmt`, `clippy`, `test`, `doc`, `wasm-build`, `wasm-test`, `wasm-clippy`, all warnings-as-errors. Match strict flags locally before pushing:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- `card-dsl` has no I/O/state and sits below `game-core`; types on `Effect` must derive `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize` (so **no borrowed `&'static str` fields** — use owned `String`).
- Handler contract: validate-first / mutate-second. `SearchDeck`'s only mutations (take + shuffle) run after every choice resolves.
- Card text is verbatim from `data/arkhamdb-snapshot/pack/core/core.json`; rules from `data/rules-reference/ahc01_rules_reference_web.pdf` p.18 ("Search").
- Branch: `engine/deck-search-tutors` (already created). One PR. Commit subjects `scope: description`, bodies end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

### Task 1: DSL — `SearchScope`, `CardFilter`, `Effect::SearchDeck`, `search_deck` builder

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (Effect enum ~line 735; builders ~line 1267 near `draw_cards`)
- Test: `crates/card-dsl/src/dsl.rs` (existing `#[cfg(test)]` module)

**Interfaces:**
- Produces:
  - `pub enum SearchScope { Top(u8), EntireDeck }`
  - `pub struct CardFilter { pub trait_: Option<String>, pub kind: Option<crate::card_data::CardType> }`
  - `Effect::SearchDeck { target: InvestigatorTarget, scope: SearchScope, filter: Option<CardFilter> }`
  - `pub fn search_deck(target: InvestigatorTarget, scope: SearchScope, filter: Option<CardFilter>) -> Effect`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module of `crates/card-dsl/src/dsl.rs`, add:

```rust
#[test]
fn search_deck_builder_and_serde_round_trip() {
    let e = search_deck(
        InvestigatorTarget::chosen_at_your_location(),
        SearchScope::Top(3),
        None,
    );
    assert!(matches!(
        e,
        Effect::SearchDeck {
            target: InvestigatorTarget::Chosen(_),
            scope: SearchScope::Top(3),
            filter: None,
        }
    ));
    let json = serde_json::to_string(&e).expect("serialize");
    let back: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(e, back);

    let filtered = search_deck(
        InvestigatorTarget::You,
        SearchScope::EntireDeck,
        Some(CardFilter {
            trait_: Some("Tome".into()),
            kind: Some(crate::card_data::CardType::Asset),
        }),
    );
    let json = serde_json::to_string(&filtered).expect("serialize");
    assert_eq!(filtered, serde_json::from_str::<Effect>(&json).expect("de"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p card-dsl search_deck_builder_and_serde_round_trip`
Expected: FAIL — `SearchScope` / `CardFilter` / `Effect::SearchDeck` / `search_deck` not found.

- [ ] **Step 3: Add the types and the enum variant**

In `crates/card-dsl/src/dsl.rs`, immediately after the `Effect::Investigate { … }` variant (before the closing `}` of `pub enum Effect` at ~line 735), add:

```rust
    /// Search a region of an investigator's deck for one card matching
    /// `filter`, move it to that investigator's hand, then shuffle the deck.
    /// Old Book of Lore 01031 (top 3, any card, chosen investigator) and
    /// Research Librarian 01032 (entire deck, a `Tome` asset, you).
    ///
    /// RR p.18 ("Search"): the searcher is *obligated to find* a card if one
    /// or more eligible options exist (no decline) — so 0 eligible ⇒ find
    /// nothing, 1 ⇒ auto-take, 2+ ⇒ the controller picks. An entire-deck
    /// search must shuffle on completion; top-N shuffles too (Old Book
    /// "shuffles the remaining cards into the deck"). Both "draws it" (Old
    /// Book) and "add to your hand" (Librarian) are modeled as a move to
    /// hand — the only rules difference (on-draw triggers) has no Core
    /// consumer.
    SearchDeck {
        /// Whose deck is searched.
        target: InvestigatorTarget,
        /// Which region of the deck to look at.
        scope: SearchScope,
        /// Which cards are eligible. `None` matches every card.
        filter: Option<CardFilter>,
    },
```

After the `pub enum Effect { … }` block (e.g. just before the `// ---- stats` section comment at ~line 737), add:

```rust
/// Which region of a deck an [`Effect::SearchDeck`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchScope {
    /// The top `n` cards (Old Book of Lore: 3). Fewer if the deck is shorter.
    Top(u8),
    /// The whole deck (Research Librarian). Must be shuffled on completion.
    EntireDeck,
}

/// Eligibility predicate for an [`Effect::SearchDeck`]. Both fields, when
/// `Some`, must hold (trait AND type). `trait_` is owned (the `Effect` enum is
/// serde-serializable, so no borrowed `&'static str`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardFilter {
    /// Required trait (e.g. `"Tome"`). `None` = any trait.
    pub trait_: Option<String>,
    /// Required card type (e.g. [`CardType::Asset`](crate::card_data::CardType::Asset)).
    /// `None` = any type.
    pub kind: Option<crate::card_data::CardType>,
}
```

- [ ] **Step 4: Add the builder**

In `crates/card-dsl/src/dsl.rs`, after the `draw_cards` builder (~line 1271), add:

```rust
/// Build an [`Effect::SearchDeck`].
#[must_use]
pub fn search_deck(
    target: InvestigatorTarget,
    scope: SearchScope,
    filter: Option<CardFilter>,
) -> Effect {
    Effect::SearchDeck {
        target,
        scope,
        filter,
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p card-dsl search_deck_builder_and_serde_round_trip`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "card-dsl: Effect::SearchDeck + SearchScope/CardFilter + builder

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Evaluator — `apply_search_deck` handler + dispatch wiring + `CardSearchedToHand` event

**Files:**
- Modify: `crates/game-core/src/event.rs` (add `CardSearchedToHand` near `DeckShuffled` ~line 303)
- Modify: `crates/game-core/src/engine/evaluator.rs` (dispatch arm ~line 378; `ground_chosen_targets` arm ~line 1242; new handler fn)
- Test: `crates/game-core/src/engine/evaluator.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Consumes: `Effect::SearchDeck`, `SearchScope`, `CardFilter` (Task 1); `crate::engine::dispatch::cards::shuffle_player_deck(cx, InvestigatorId)`; `crate::engine::dispatch::choice::{resolve_choice_count, suspend_for_choice, ChoiceResolution}`; `DecisionCursor::{take, root, recorded_so_far}`; `EvalContext.chosen_investigator`.
- Produces: `Event::CardSearchedToHand { investigator, code }`; the `Effect::SearchDeck` evaluator behavior.

- [ ] **Step 1: Add the event variant**

In `crates/game-core/src/event.rs`, after the `DeckShuffled { … }` variant (~line 303–311), add:

```rust
    /// A card was found by a deck search and moved to an investigator's hand
    /// ([`Effect::SearchDeck`](crate::dsl::Effect::SearchDeck): Old Book of
    /// Lore 01031, Research Librarian 01032). Distinct from
    /// [`CardsDrawn`](Self::CardsDrawn) — a search is not a "draw" (no on-draw
    /// triggers key off it), and it names the specific card.
    CardSearchedToHand {
        /// The investigator who searched and now holds the card.
        investigator: InvestigatorId,
        /// The card moved to hand.
        code: CardCode,
    },
```

(`InvestigatorId` / `CardCode` are already imported in `event.rs` — they're used by neighboring variants.)

- [ ] **Step 2: Write the failing tests**

In the `#[cfg(test)]` module of `crates/game-core/src/engine/evaluator.rs`, add these tests. They use the existing `TestGame` builder and don't need a registry (filter is `None`, so no `metadata_for` lookup). Match the existing test helpers in that module for constructing a game with one investigator and a seeded deck; if the local helper differs, adapt the setup lines but keep the assertions.

```rust
#[test]
fn search_deck_top_n_auto_takes_single_eligible() {
    // One investigator; deck top has exactly one card. No filter ⇒ that card
    // is the sole eligible candidate ⇒ auto-take (no suspend).
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    game.investigators.get_mut(&InvestigatorId(1)).unwrap().deck =
        vec![CardCode::new("90001")];
    let mut cx = Cx::new(&mut game);
    let outcome = apply_effect(
        &mut cx,
        &Effect::SearchDeck {
            target: InvestigatorTarget::You,
            scope: SearchScope::Top(3),
            filter: None,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    let inv = game.investigators.get(&InvestigatorId(1)).unwrap();
    assert!(inv.hand.contains(&CardCode::new("90001")));
    assert!(inv.deck.is_empty());
}

#[test]
fn search_deck_top_n_with_no_cards_is_find_nothing_not_reject() {
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    // Empty deck: 0 eligible ⇒ find nothing, still Done (RR p.18 — search may
    // legally find nothing; it is NOT a rejection).
    let mut cx = Cx::new(&mut game);
    let outcome = apply_effect(
        &mut cx,
        &Effect::SearchDeck {
            target: InvestigatorTarget::You,
            scope: SearchScope::Top(3),
            filter: None,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    assert!(game.investigators.get(&InvestigatorId(1)).unwrap().hand.is_empty());
}

#[test]
fn search_deck_top_n_suspends_on_two_eligible_then_takes_pick() {
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    game.investigators.get_mut(&InvestigatorId(1)).unwrap().deck =
        vec![CardCode::new("90001"), CardCode::new("90002"), CardCode::new("90003")];
    let mut cx = Cx::new(&mut game);
    let outcome = apply_effect(
        &mut cx,
        &Effect::SearchDeck {
            target: InvestigatorTarget::You,
            scope: SearchScope::Top(3),
            filter: None,
        },
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));

    // Resume picking option 1 (the second eligible, "90002").
    let resumed = crate::engine::apply_choice_input(&mut game, OptionId(1));
    assert!(matches!(resumed, EngineOutcome::Done));
    let inv = game.investigators.get(&InvestigatorId(1)).unwrap();
    assert!(inv.hand.contains(&CardCode::new("90002")));
    assert!(!inv.deck.contains(&CardCode::new("90002")));
    assert_eq!(inv.deck.len(), 2);
}
```

> Note: use whatever the module's canonical resume entry point is for a `Continuation::Choice` (the existing Axis-A choice tests in this file call into `apply` with an `InputResponse::PickSingle`, or a `resume_choice` test helper). Replace `crate::engine::apply_choice_input(&mut game, OptionId(1))` with the same call those tests use (e.g. building a `PlayerAction::ResolveInput { response: InputResponse::PickSingle(OptionId(1)) }` and calling `apply`). The assertion content stays identical.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p game-core search_deck_`
Expected: FAIL — `Effect::SearchDeck` not handled (non-exhaustive match / unreachable arm).

- [ ] **Step 4: Add the dispatch arm and the handler**

In `crates/game-core/src/engine/evaluator.rs`, add the dispatch arm right after the `Effect::Investigate { … }` arm (~line 378):

```rust
        Effect::SearchDeck {
            target,
            scope,
            filter,
        } => apply_search_deck(cx, eval_ctx, *target, *scope, filter.as_ref(), cursor),
```

Add `Effect::SearchDeck` to the `ground_chosen_targets` investigator-target match (~line 1242), so Old Book's `Chosen` target grounds before the handler runs:

```rust
    let inv_target = match effect {
        Effect::GainResources { target, .. }
        | Effect::Deal { target, .. }
        | Effect::Heal { target, .. }
        | Effect::DrawCards { target, .. }
        | Effect::SearchDeck { target, .. } => Some(target),
        _ => None,
    };
```

Add the handler function (place it near `draw_cards_effect`, ~line 384):

```rust
/// Resolve [`Effect::SearchDeck`]: the resolved investigator looks at a deck
/// region (`scope`) ∩ `filter`, takes one eligible card to hand (RR p.18:
/// obligated if any exist; 0 ⇒ find nothing), then shuffles the deck. The
/// select reuses the Axis-A choice machinery (cursor replay / suspend on 2+),
/// exactly like [`apply_choose_one`]. A `Chosen` target is already bound by
/// [`ground_chosen_targets`]; the take + shuffle are the only mutations and
/// run after the pick resolves.
fn apply_search_deck(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: crate::dsl::InvestigatorTarget,
    scope: crate::dsl::SearchScope,
    filter: Option<&crate::dsl::CardFilter>,
    cursor: &mut DecisionCursor<'_>,
) -> EngineOutcome {
    use crate::dsl::{InvestigatorTarget, SearchScope};
    use crate::engine::dispatch::cards::shuffle_player_deck;
    use crate::engine::dispatch::choice::{resolve_choice_count, suspend_for_choice, ChoiceResolution};
    use crate::engine::OptionId;

    // 1. Whose deck. `Chosen` is bound by ground_chosen_targets.
    let who = match target {
        InvestigatorTarget::You => eval_ctx.controller,
        InvestigatorTarget::Active => match cx.state.active_investigator {
            Some(id) => id,
            None => {
                return EngineOutcome::Rejected {
                    reason: "SearchDeck: no active investigator".into(),
                }
            }
        },
        InvestigatorTarget::Chosen(_) => match eval_ctx.chosen_investigator {
            Some(id) => id,
            None => {
                return EngineOutcome::Rejected {
                    reason: "SearchDeck: chosen investigator was not bound".into(),
                }
            }
        },
    };
    let Some(inv) = cx.state.investigators.get(&who) else {
        return EngineOutcome::Rejected {
            reason: format!("SearchDeck: investigator {who:?} is not in the state").into(),
        };
    };

    // 2. Enumerate eligible (deck-index, code) in deck order — deterministic,
    //    so OptionId indices replay across suspend/resume (the deck is not
    //    mutated until step 4).
    let region = match scope {
        SearchScope::Top(n) => usize::from(n).min(inv.deck.len()),
        SearchScope::EntireDeck => inv.deck.len(),
    };
    let eligible: Vec<(usize, CardCode)> = inv.deck[..region]
        .iter()
        .enumerate()
        .filter(|(_, code)| match filter {
            None => true,
            Some(f) => filter_matches(f, code),
        })
        .map(|(i, code)| (i, code.clone()))
        .collect();

    // 3. Choice convention — but 0 ⇒ find nothing (not reject).
    let chosen_deck_index: Option<usize> = match resolve_choice_count(eligible.len()) {
        ChoiceResolution::Empty => None,
        ChoiceResolution::Auto(i) => Some(eligible[i].0),
        ChoiceResolution::Suspend => {
            if let Some(OptionId(i)) = cursor.take() {
                Some(eligible[i as usize].0)
            } else {
                let labels = eligible.iter().map(|(_, c)| c.0.clone()).collect();
                return suspend_for_choice(
                    cx,
                    "Search: choose a card to take",
                    labels,
                    cursor.recorded_so_far(),
                    cursor.root(),
                    eval_ctx,
                );
            }
        }
    };

    // 4. Take chosen → hand.
    if let Some(idx) = chosen_deck_index {
        let inv = cx
            .state
            .investigators
            .get_mut(&who)
            .expect("checked above");
        let code = inv.deck.remove(idx);
        inv.hand.push(code.clone());
        cx.events.push(Event::CardSearchedToHand {
            investigator: who,
            code,
        });
    }

    // 5. Shuffle (RR p.18 entire-deck mandatory; Old Book "shuffle the
    //    remaining cards into the deck"). RNG-replayable; no-op on <2 cards.
    shuffle_player_deck(cx, who);
    EngineOutcome::Done
}

/// Whether a deck card `code` matches a [`CardFilter`]: both `trait_` and
/// `kind` (when `Some`) must hold, read from the installed registry's
/// metadata. Returns `false` with no registry (a filtered search finds
/// nothing rather than panicking — only the registry-less test paths, which
/// never use a filter, hit this).
fn filter_matches(f: &crate::dsl::CardFilter, code: &CardCode) -> bool {
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    let Some(meta) = (reg.metadata_for)(code) else {
        return false;
    };
    if let Some(t) = &f.trait_ {
        if !meta.traits.iter().any(|x| x == t) {
            return false;
        }
    }
    if let Some(k) = f.kind {
        if meta.card_type() != k {
            return false;
        }
    }
    true
}
```

> The handler uses `shuffle_player_deck`, currently `pub(super)` in `dispatch/cards.rs`. The evaluator is `crate::engine::evaluator`, a sibling of `crate::engine::dispatch`, so widen its visibility to `pub(in crate::engine)` (matching `draw_cards`). Make that one-line change in `dispatch/cards.rs` as part of this step.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core search_deck_`
Expected: PASS (all three).

- [ ] **Step 6: Run the broader suite + clippy**

Run: `cargo test -p game-core && cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS (exhaustive `Effect` matches now include `SearchDeck`).

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/event.rs crates/game-core/src/engine/evaluator.rs crates/game-core/src/engine/dispatch/cards.rs
git commit -m "engine: Effect::SearchDeck evaluator handler + CardSearchedToHand

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `EnteredPlay` reaction trigger (pattern + window + emit + scan)

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (`EventPattern` enum ~line 372)
- Modify: `crates/game-core/src/state/game_state.rs` (`WindowKind` ~line 925)
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` (`TimingEvent` + its three match methods)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`scan_pending_triggers` instance-filter ~line 182; `trigger_matches` ~line 364 + the false-arm exhaustive list; `run_window_continuation`)
- Modify: `crates/game-core/src/engine/dispatch/cards.rs` (`play_card` asset tail ~line 558)
- Test: `crates/game-core/src/engine/dispatch/cards.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `EventPattern::EnteredPlay`; `WindowKind::AfterEnteredPlay { instance: CardInstanceId, controller: InvestigatorId }`; `TimingEvent::EnteredPlay { instance, controller }`; `play_card` opening the window after an asset enters play.

- [ ] **Step 1: Add the DSL pattern**

In `crates/card-dsl/src/dsl.rs`, before the closing `}` of `pub enum EventPattern` (after `EnemyAttacks`, ~line 372), add:

```rust
    /// The card this ability is printed on entered play (Research Librarian
    /// 01032: "[reaction] After Research Librarian enters play: …"). Bare and
    /// **self-referential** — the engine fires it only for the just-entered
    /// instance (the reaction-window scan filters to that instance), binding
    /// *you* = the controller. A general "after any card enters play" reaction
    /// is out of scope; the pattern is reaction-only (`EventTiming::After`).
    EnteredPlay,
```

- [ ] **Step 2: Add the WindowKind**

In `crates/game-core/src/state/game_state.rs`, before the closing `}` of `pub enum WindowKind` (~line 925+), add:

```rust
    /// Fires after a card entered play, scanning only the entered instance's
    /// own `EnteredPlay` reactions (Research Librarian 01032). Pairs with
    /// [`EventPattern::EnteredPlay`](crate::dsl::EventPattern::EnteredPlay) /
    /// [`EventTiming::After`](crate::dsl::EventTiming::After). `instance` is
    /// the entered card; `controller` its owner.
    AfterEnteredPlay {
        /// The card instance that just entered play (self-binding scope).
        instance: CardInstanceId,
        /// The investigator who controls it.
        controller: InvestigatorId,
    },
```

(`CardInstanceId` / `InvestigatorId` are already in scope in this enum — used by sibling variants.)

- [ ] **Step 3: Add the TimingEvent + wire its three methods**

In `crates/game-core/src/engine/dispatch/emit.rs`, add to `pub(crate) enum TimingEvent` (after `WouldDiscoverClues`, ~line 102):

```rust
    /// A card entered play (reaction-only, After). Opens the
    /// `AfterEnteredPlay` window — Research Librarian 01032's tutor.
    EnteredPlay {
        instance: CardInstanceId,
        controller: InvestigatorId,
    },
```

Add `CardInstanceId` to the `use` imports at the top of `emit.rs` if not already present.

In `fn forced_point` — `EnteredPlay` fires no forced abilities, so add it to the `None` arm:

```rust
            TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
```

In `fn reaction_window`, add (before the `_ => None` arm):

```rust
            TimingEvent::EnteredPlay {
                instance,
                controller,
            } => Some(WindowKind::AfterEnteredPlay {
                instance: *instance,
                controller: *controller,
            }),
```

In `fn forced_continuation`, add `EnteredPlay` to the reaction-only `None` group:

```rust
            // Reaction-only Before-timing points: no forced phase (Axis D).
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
```

- [ ] **Step 4: Self-binding scan filter + trigger_matches + continuation**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, `scan_pending_triggers` — add the instance filter alongside the soaked-asset one (~line 182, inside `for card in inv.controlled_card_instances()`):

```rust
            // Self-binding: `AfterEnteredPlay` fires only for the instance that
            // entered play (Research Librarian 01032). Mirrors the soaked-asset
            // filter above.
            if let WindowKind::AfterEnteredPlay { instance, .. } = kind {
                if card.instance_id != instance {
                    continue;
                }
            }
```

In `trigger_matches`, add a positive arm (next to the `AfterSuccessfulInvestigate` arm, ~line 364):

```rust
        // `AfterEnteredPlay` matches `EnteredPlay`, scoped to the controller
        // (the entered card's owner). The self-instance scoping is in the scan.
        (
            WindowKind::AfterEnteredPlay { controller, .. },
            EventPattern::EnteredPlay,
        ) => controller == controller_,
```

> Rename the binding to avoid shadowing the function's `controller` parameter: bind the window field as `controller: window_controller` and compare `window_controller == controller`. Adjust the exhaustive false-arm tuple lists at the bottom of `trigger_matches` to add `WindowKind::AfterEnteredPlay { .. }` to the window list and `EventPattern::EnteredPlay` to the pattern list, so the match stays exhaustive.

In `run_window_continuation`, add an arm (the window does no framework follow-up — the asset is already in play):

```rust
        // EnteredPlay window has no continuation: the asset entered play before
        // the window opened; closing it just finishes the play action.
        WindowKind::AfterEnteredPlay { .. } => EngineOutcome::Done,
```

- [ ] **Step 5: Emit from play_card after an asset enters play**

In `crates/game-core/src/engine/dispatch/cards.rs`, `play_card`, replace the `InPlay` block tail (~line 558–577) so it captures the instance id, emits the timing event, and opens the queued window:

```rust
    if let super::PlayDestination::InPlay = destination {
        let played = cx
            .state
            .investigators
            .get_mut(&investigator)
            .expect("checked")
            .hand
            .remove(idx);
        let in_play = super::threat_area::new_in_play_instance(cx, played);
        let instance = in_play.instance_id;
        cx.state
            .investigators
            .get_mut(&investigator)
            .expect("checked")
            .cards_in_play
            .push(in_play);
        // "[reaction] After … enters play" (Research Librarian 01032): emit the
        // timing event (queues the AfterEnteredPlay window iff a matching
        // reaction exists), then open the window so the player can act. No
        // forced phase, so emit_event returns Done; we only need to drive an
        // opened window.
        let _ = super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::EnteredPlay {
                instance,
                controller: investigator,
            },
        );
        if cx.state.top_reaction_window().is_some() {
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
    }
    EngineOutcome::Done
```

> `emit_event` and `TimingEvent` are `pub(crate)` in `dispatch::emit`; `play_card` is in `dispatch::cards`, so `super::emit::…` resolves. If `TimingEvent`'s variants aren't visible cross-module, they are (`pub(crate) enum`). `open_queued_reaction_window` is already `pub(crate)`.

- [ ] **Step 6: Write the test (asset with no enters-play reaction is unaffected; one with it opens a window)**

In `crates/game-core/src/engine/dispatch/cards.rs` `#[cfg(test)]`, add a test that an asset *without* an `EnteredPlay` reaction still plays to `Done` (no behavior change), using the existing `play_card` test scaffolding in that module / `engine/mod.rs`:

```rust
#[test]
fn play_asset_without_entered_play_reaction_is_done() {
    // Reuse the module's play_card harness; play an asset whose abilities have
    // no EnteredPlay reaction (e.g. a Constant-only asset). The outcome is Done
    // and no window is left open.
    // (Mirror the existing `play_card_*` tests' setup; assert EngineOutcome::Done
    // and cx.state.open_windows / top_reaction_window() is None.)
}
```

> Fill the body by copying the nearest existing `play_card_*` test's setup (they live in `engine/mod.rs` ~line 3654 and use `play_card_state`), substituting an asset code whose `abilities()` are Constant-only. Assert `matches!(outcome, EngineOutcome::Done)` and `game.top_reaction_window().is_none()`. The full enters-play-opens-a-window path is covered end-to-end by Research Librarian's integration test (Task 6), which has real registry metadata; this unit test just guards the no-reaction path.

- [ ] **Step 7: Run tests + clippy**

Run: `cargo test -p game-core && cargo clippy -p game-core --all-targets --all-features -- -D warnings`
Expected: PASS. Exhaustive matches over `EventPattern` / `WindowKind` / `TimingEvent` now include the new variants (the `doc` job also checks the new intra-doc links).

- [ ] **Step 8: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/emit.rs crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/src/engine/dispatch/cards.rs
git commit -m "engine: EnteredPlay reaction trigger + AfterEnteredPlay window

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Choice-from-reaction reentrancy (`resume_choice` re-drives a window)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/choice.rs` (`resume_choice` tail)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (widen `advance_resolution` visibility if needed)
- Test: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`#[cfg(test)]`) — or fold into Task 6's integration test if a synth reaction-with-choice is awkward in `game-core`.

**Interfaces:**
- Consumes: `GameState::top_reaction_window_index() -> Option<usize>`; `reaction_windows::advance_resolution(cx, idx)`.
- Produces: a `Continuation::Choice` that completes while a reaction-window `Resolution` frame sits below it re-drives that window (closes it / advances to the next pending trigger).

- [ ] **Step 1: Write the failing test**

The scenario: a reaction window fires an effect that suspends for a choice (2+ eligible), and on resume the window must close. The cleanest in-`game-core` reproduction uses a synthetic reaction card whose effect is `Effect::SearchDeck` with 2+ eligible cards. If `game-core`'s test fixtures (`scenarios::test_fixtures::synth_cards` / a local `TEST_REGISTRY`) can register such a card, write it there; otherwise mark this test `#[ignore]` with a comment pointing at Task 6's Research Librarian integration test (which exercises this exact path with the real registry) and rely on that for coverage.

If writing it in `game-core` with a synth registry:

```rust
#[test]
fn choice_fired_from_reaction_window_closes_window_on_resume() {
    // Build a state with an open AfterEnteredPlay window whose sole pending
    // trigger is a card whose reaction effect is SearchDeck over a 2-eligible
    // deck. Fire it (PickSingle the trigger) → AwaitingInput (the card choice).
    // Resume the card choice (PickSingle) → the SearchDeck completes AND the
    // window closes (open_windows empties), outcome Done.
    // Assert: after the second PickSingle, EngineOutcome::Done and
    // state.top_reaction_window().is_none().
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p game-core choice_fired_from_reaction_window`
Expected: FAIL — after resuming the choice, the window frame is stranded (outcome `Done` but `top_reaction_window()` still `Some`), or the second `PickSingle` is misrouted.

- [ ] **Step 3: Extend `resume_choice`**

In `crates/game-core/src/engine/dispatch/choice.rs`, replace the tail of `resume_choice`:

```rust
    // If the choice completed an effect that was suspended *inside* a skill
    // test (Crypt Chill 01167's on_fail discard), re-enter the driver to run
    // the test's teardown — its continuation is parked at `PostFollowUp`.
    if matches!(outcome, EngineOutcome::Done) && cx.state.in_flight_skill_test.is_some() {
        return super::skill_test::drive_skill_test(cx);
    }
    // If the choice completed an effect fired *from a reaction window*
    // (Research Librarian 01032's SearchDeck suspending on 2+ eligible Tomes),
    // the window frame is still on the stack below; re-drive it so it closes /
    // advances to the next pending trigger. Mirrors the skill-test reentrancy
    // above. (Old Book's choice is fired from an activated ability — no window
    // frame — and falls through to `outcome`.)
    if matches!(outcome, EngineOutcome::Done) {
        if let Some(idx) = cx.state.top_reaction_window_index() {
            return super::reaction_windows::advance_resolution(cx, idx);
        }
    }
    outcome
```

If `advance_resolution` is private (`fn`) in `reaction_windows.rs`, widen it to `pub(super)` so `dispatch::choice` can call it. (`top_reaction_window_index` is already used by `reaction_windows`; confirm it's `pub(crate)` or in-`engine` reachable from `choice.rs`; widen if needed.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p game-core choice_fired_from_reaction_window`
Expected: PASS (or, if deferred to Task 6, confirm Task 6's Research-Librarian-with-2-Tomes test passes).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch/choice.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: resume_choice re-drives a reaction window on completion

A choice fired from inside a reaction window (Research Librarian's
SearchDeck on 2+ eligible cards) left the window frame stranded on
resume. Mirror the existing skill-test reentrancy.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Old Book of Lore 01031 (card + test + register)

**Files:**
- Create: `crates/cards/src/impls/old_book_of_lore.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm)
- Test: `crates/cards/src/impls/old_book_of_lore.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `card_dsl::dsl::{activated, search_deck, Cost, InvestigatorTarget, SearchScope, Ability}`.
- Produces: `old_book_of_lore::CODE = "01031"`, `old_book_of_lore::abilities()`.

- [ ] **Step 1: Write the card module + failing test**

Create `crates/cards/src/impls/old_book_of_lore.rs`:

```rust
//! Old Book of Lore (Seeker item asset, 01031).
//!
//! ```text
//! Item. Tome.
//! [action] Exhaust Old Book of Lore: Choose an investigator at your
//!   location. That investigator searches the top 3 cards of his or her deck
//!   for a card, draws it, and shuffles the remaining cards into his or her
//!   deck.
//! ```
//!
//! One `[action]` ability with an exhaust cost: an
//! [`Effect::SearchDeck`](card_dsl::dsl::Effect::SearchDeck) over the top 3 of
//! a chosen co-located investigator's deck, no filter. "draws it" is modeled
//! as a move to hand (the search primitive's destination); the search shuffles
//! on completion. In solo with one investigator the target auto-binds.

use card_dsl::dsl::{
    activated, search_deck, Ability, Cost, InvestigatorTarget, SearchScope,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01031";

/// `[action]`, exhaust: a chosen co-located investigator searches the top 3 of
/// their deck for a card, takes it, and shuffles.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::Exhaust],
        search_deck(
            InvestigatorTarget::chosen_at_your_location(),
            SearchScope::Top(3),
            None,
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Cost, Effect, InvestigatorTarget, SearchScope, Trigger};

    #[test]
    fn abilities_are_action_exhaust_search_top_3() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(abilities[0].costs, vec![Cost::Exhaust]);
        assert!(matches!(
            abilities[0].effect,
            Effect::SearchDeck {
                target: InvestigatorTarget::Chosen(_),
                scope: SearchScope::Top(3),
                filter: None,
            }
        ));
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/cards/src/impls/mod.rs`, add `pub mod old_book_of_lore;` in module-name order (after `medical_texts` / before `overpower`), and add the `abilities_for` arm in the same position:

```rust
        old_book_of_lore::CODE => Some(old_book_of_lore::abilities()),
```

- [ ] **Step 3: Run the card test + verify playable**

Run: `cargo test -p cards old_book_of_lore`
Expected: PASS.

- [ ] **Step 4: Add an end-to-end activation integration test**

In `crates/cards/tests/`, add `search_deck_old_book.rs` (its own binary, so it can `install(cards::REGISTRY)`):

```rust
//! Old Book of Lore 01031: activating its [action] searches the top 3, takes a
//! card to hand, and shuffles. Solo (single investigator ⇒ target auto-binds).

use cards::REGISTRY;
// + the test scaffolding this crate's other integration tests use
// (game_core::test_support, StartScenario seating, etc. — mirror
// crates/cards/tests/play_card.rs).

#[test]
fn old_book_action_searches_top_three_into_hand() {
    let _ = game_core::card_registry::install(REGISTRY);
    // Seat one investigator with Old Book of Lore in play (exhaustable) and a
    // deck whose top 3 are known. Activate Old Book's ability. With one
    // co-located investigator the target auto-binds; the top 3 give a 3-option
    // search ⇒ AwaitingInput. Resume PickSingle(OptionId(0)).
    // Assert: the picked card is in hand, removed from the deck, the deck was
    // shuffled (Event::DeckShuffled emitted), and Old Book is exhausted.
}
```

> Fill the setup by mirroring an existing integration test that activates an ability (`crates/cards/tests/` — e.g. the .38 Special / Medical Texts test if present; otherwise follow `play_card.rs`'s seating pattern and `PlayerAction::ActivateAbility`). Keep the four assertions (hand gained, deck lost, `DeckShuffled`, exhausted).

- [ ] **Step 5: Run + commit**

Run: `cargo test -p cards old_book && cargo test -p cards --test search_deck_old_book`
Expected: PASS.

```bash
git add crates/cards/src/impls/old_book_of_lore.rs crates/cards/src/impls/mod.rs crates/cards/tests/search_deck_old_book.rs
git commit -m "cards: Old Book of Lore 01031 (search top 3 → hand)

Closes #319.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Research Librarian 01032 (card + integration test + register)

**Files:**
- Create: `crates/cards/src/impls/research_librarian.rs`
- Modify: `crates/cards/src/impls/mod.rs` (`pub mod` + `abilities_for` arm)
- Test: `crates/cards/src/impls/research_librarian.rs` (`#[cfg(test)]`) + `crates/cards/tests/search_deck_research_librarian.rs`

**Interfaces:**
- Consumes: `card_dsl::dsl::{reaction_on_event, search_deck, EventPattern, EventTiming, InvestigatorTarget, SearchScope, CardFilter, Ability}`; `card_dsl::card_data::CardType`.
- Produces: `research_librarian::CODE = "01032"`, `research_librarian::abilities()`.

- [ ] **Step 1: Write the card module + failing unit test**

Create `crates/cards/src/impls/research_librarian.rs`:

```rust
//! Research Librarian (Seeker ally asset, 01032).
//!
//! ```text
//! Ally. Miskatonic.
//! [reaction] After Research Librarian enters play: Search your deck for a
//!   Tome asset and add it to your hand. Shuffle your deck.
//! ```
//!
//! One `EnteredPlay` reaction (self-referential — the engine fires it only for
//! this just-entered instance): an
//! [`Effect::SearchDeck`](card_dsl::dsl::Effect::SearchDeck) over the entire
//! deck, filtered to `Tome` assets, into the controller's hand, then shuffle.
//!
//! # Ally-soak gap
//!
//! Metadata gives Research Librarian `health: 1, sanity: 1` (ally soak, not a
//! stat boost). The DSL doesn't model soak yet (#44), so this impl ships only
//! the reaction; the card is mechanically weaker than printed until soak lands.

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    reaction_on_event, search_deck, Ability, CardFilter, EventPattern, EventTiming,
    InvestigatorTarget, SearchScope,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01032";

/// "[reaction] After Research Librarian enters play: Search your deck for a
/// Tome asset, add it to your hand, shuffle."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnteredPlay,
        EventTiming::After,
        search_deck(
            InvestigatorTarget::You,
            SearchScope::EntireDeck,
            Some(CardFilter {
                trait_: Some("Tome".into()),
                kind: Some(CardType::Asset),
            }),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::card_data::CardType;
    use card_dsl::dsl::{
        CardFilter, Effect, EventPattern, EventTiming, InvestigatorTarget, SearchScope,
        Trigger, TriggerKind,
    };

    #[test]
    fn ability_is_entered_play_reaction_search_tome_asset() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredPlay,
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert_eq!(
            abilities[0].effect,
            Effect::SearchDeck {
                target: InvestigatorTarget::You,
                scope: SearchScope::EntireDeck,
                filter: Some(CardFilter {
                    trait_: Some("Tome".into()),
                    kind: Some(CardType::Asset),
                }),
            },
        );
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/cards/src/impls/mod.rs`, add `pub mod research_librarian;` in name order and the `abilities_for` arm:

```rust
        research_librarian::CODE => Some(research_librarian::abilities()),
```

- [ ] **Step 3: Run the unit test**

Run: `cargo test -p cards research_librarian`
Expected: PASS.

- [ ] **Step 4: Integration test — enters-play reaction tutors a Tome (covers Tasks 3 + 4)**

Create `crates/cards/tests/search_deck_research_librarian.rs`:

```rust
//! Research Librarian 01032: entering play opens its reaction window; firing it
//! searches the deck for a Tome asset and adds it to hand, then shuffles.
//! Exercises EnteredPlay (Task 3) and — with 2 Tomes — the choice-from-reaction
//! reentrancy (Task 4).

use cards::REGISTRY;

#[test]
fn entering_play_tutors_the_only_tome_asset() {
    let _ = game_core::card_registry::install(REGISTRY);
    // Seat one investigator. Deck contains exactly one Tome asset (Old Book of
    // Lore 01031) plus several non-Tome cards. Hand contains Research Librarian.
    // Play Research Librarian (PlayCard) → it enters play → AfterEnteredPlay
    // window opens with one pending trigger ⇒ AwaitingInput.
    // Resume PickSingle(OptionId(0)) to fire the reaction. Exactly one eligible
    // Tome ⇒ auto-take (no second prompt) ⇒ Done.
    // Assert: Old Book is in hand, removed from deck, Event::DeckShuffled
    // emitted, window closed.
}

#[test]
fn two_tome_assets_prompt_a_choice_then_tutor_the_pick() {
    let _ = game_core::card_registry::install(REGISTRY);
    // Same, but seed TWO Tome assets in the deck (e.g. Old Book 01031 + Medical
    // Texts 01035 is an Item not a Tome — use two genuine Tome assets from the
    // corpus; verify traits against the snapshot). Play Research Librarian →
    // window opens (AwaitingInput). PickSingle to fire the reaction → SearchDeck
    // sees 2 eligible ⇒ suspends with a card choice (AwaitingInput).
    // PickSingle(OptionId(1)) → that Tome lands in hand, window closes, Done.
    // Assert: the picked Tome in hand; deck shuffled; window closed (Task 4).
}
```

> **Verify the two Tome assets against the snapshot before writing the test** (per CLAUDE.md). In original Core the Seeker Tomes are Old Book of Lore 01031, Medical Texts 01035 (trait `Item. Tome.` — confirm), and others; grep `data/arkhamdb-snapshot/pack/core/core.json` for `"Tome"` and pick two that are `type_code: "asset"`. If only one in-corpus Tome asset is available to the seeded deck, drop the second test to `#[ignore]` and rely on the Task-4 `game-core` synth test for the 2+ path. Fill setup by mirroring `crates/cards/tests/play_card.rs`.

- [ ] **Step 5: Run + commit**

Run: `cargo test -p cards research_librarian && cargo test -p cards --test search_deck_research_librarian`
Expected: PASS.

```bash
git add crates/cards/src/impls/research_librarian.rs crates/cards/src/impls/mod.rs crates/cards/tests/search_deck_research_librarian.rs
git commit -m "cards: Research Librarian 01032 (enters-play Tome tutor)

Closes #320.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Full gauntlet + phase-doc update (final commit)

**Files:**
- Modify: `docs/phases/phase-7-the-gathering.md`

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. Fix any failures before proceeding.

- [ ] **Step 2: Open the PR (before the phase-doc commit)**

```bash
git push -u origin engine/deck-search-tutors
gh pr create --fill
```

PR body: describe the `SearchDeck` primitive, the `EnteredPlay` reaction, and the choice-from-reaction reentrancy fix; cite RR p.18 verbatim for the no-decline + mandatory-shuffle behavior; note "Closes #319" and "Closes #320". Watch CI: `gh pr checks <PR#> --watch`.

- [ ] **Step 3: Update the phase doc once CI is green**

In `docs/phases/phase-7-the-gathering.md`, in the C6b breakdown row and the Axis-E note: flip Old Book of Lore (#319) and Research Librarian (#320) to `✅ PR #<n>`, and add **one** Decisions entry (only if load-bearing for a future PR), e.g.:

> **`Effect::SearchDeck` is the deck-search/tutor primitive (#319/#320, PR #<n>).** Typed (not Native): enumerate a deck region (`Top(n)`/`EntireDeck`) ∩ `CardFilter`, take one to hand via the Axis-A choice machinery (RR p.18 — obligated-if-any, 0 ⇒ find-nothing, mandatory shuffle), reusing the cursor exactly like `ChooseOne`. `EnteredPlay` is a new self-referential reaction pattern/window on the existing pipeline. A choice fired *from* a reaction window now re-drives the window on resume (mirrors the skill-test reentrancy). A future deck-tutor lands as data on `SearchDeck` + corpus traits — no new engine work.

Remove the #319/#320 mentions from the open-Axis-E list. Do **not** add entries a future author would re-derive by grepping.

- [ ] **Step 4: Commit + push the doc update**

```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — close #319/#320 (deck-search tutors)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push
```

- [ ] **Step 5: Confirm CI green; await user approval to merge.** Do not merge without explicit approval (`gh pr merge <PR#> --squash --delete-branch`).

---

## Self-Review

**Spec coverage:** SearchDeck primitive (Tasks 1–2) ✓; RR no-decline/mandatory-shuffle (Task 2 step 4, find-nothing + always-shuffle) ✓; nested target+select for Old Book (Task 2 grounding arm + Task 5) ✓; EnteredPlay reaction (Task 3) ✓; the move-to-hand-models-both decision (`CardSearchedToHand`, no draw flag) ✓; both cards as data (Tasks 5–6) ✓; one PR (Task 7) ✓. **Discovered scope beyond the spec:** the `resume_choice` reaction-window reentrancy (Task 4) — the spec assumed Axis A "just worked"; verification found `resume_choice` re-drives only skill tests, so Research Librarian's 2+-Tome path needs the window re-drive. Documented in Task 4.

**Placeholder scan:** The integration-test bodies (Task 5 step 4, Task 6 step 4) and the two `game-core` test setups (Task 2 step 2's resume call, Task 3 step 6, Task 4 step 1) intentionally point at the nearest existing test to copy setup boilerplate from rather than re-deriving seating/`apply` plumbing that varies by harness version — the assertions (the behavior under test) are fully specified. This is deliberate: the exact `TestGame`/`StartScenario` setup lines are mechanical and best copied from a known-good neighbor at implementation time.

**Type consistency:** `CardFilter { trait_: Option<String>, kind: Option<CardType> }` (owned `String`, serde-safe) is consistent across Tasks 1/2/6. `SearchScope::{Top(u8), EntireDeck}` consistent. `WindowKind::AfterEnteredPlay { instance, controller }` / `TimingEvent::EnteredPlay { instance, controller }` / `EventPattern::EnteredPlay` consistent across Task 3. `apply_search_deck(cx, eval_ctx, target, scope, filter: Option<&CardFilter>, cursor)` matches its dispatch-arm call. `shuffle_player_deck` / `advance_resolution` visibility widenings noted where used.
