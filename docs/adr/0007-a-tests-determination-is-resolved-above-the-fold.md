# A test's determination is resolved above the fold, not inside it

ADR 0005 placed automatic failure and success at stage 5 of the modified-value fold, as the substitution that follows the clamp, and deferred the timing to #630. Implementing it showed that stage 5 cannot own the decision. We decided that **a skill test's determination is a query over the recorded rows, resolved once for the whole test, which both quantity reads then consult** — and that the ST.3/ST.4 skip falls out of asking that query before the token is drawn rather than after.

The obstacle is that the two substitutions land on different quantities. `glossary/Automatic_Failure_Success.md`:

> - If a skill test automatically fails, the investigator's total skill value for that test is considered 0.
> - If a skill test automatically succeeds, the total difficulty of that test is considered 0.

An automatic failure substitutes the investigator's skill value; an automatic success substitutes the test's difficulty. Automatic failure takes precedence over automatic success, and that is a rule *across* the two — a fold evaluating either quantity cannot see the other's rows, so two independent stage-5 substitutions both yield 0 and compare as a success, which is the wrong answer. Rex Murphy 02002 (*"[elder_sign] effect: +2. You may instead choose to automatically fail this skill test to draw 3 cards."*) and Stroke of Luck 02271 (*"…this test is automatically successful (unless a [auto_fail] token was revealed)."*) are both Dunwich, both corpus, so the collision is reachable. The precedence is a general rule from the official FAQ, settled in ADR 0005 and not reopened here. Stroke of Luck's ruling is the nearest *local* witness to it — *"You can exile Stroke of Luck even if you draw the [auto_fail] symbol. The test is still considered a failure"* (<https://arkhamdb.com/card/02271>) — but it is one instance of the rule rather than a statement of it, and it is confounded by that card printing its own parenthetical, so it cannot serve as the citation. The general rule is now cited locally, in the official FAQ's Q&A: *"If a skill test both automatically succeeds and automatically fails, the automatic failure takes precedence, and the test automatically fails."* (`data/official-faq/Frequently_Asked_Questions.md`, vendored by #672).

The determination is **stored** as a recorded row, so it inherits `Lifetime::SkillTest`'s identity check, the ST.8 expiry sweep and the abandonment path without new bookkeeping, and it carries its source card for attribution. A card declares one with a single new DSL primitive, `Effect::AutoResolve`, carrying the two-valued determination; the window it fires in comes from the card's own trigger, exactly as `ModifierScope::ThisSkillTest` already works, and a determination resolved with no test in flight rejects for want of a `SkillTestId` to stamp. That range is wider than the ticket assumed — Possession 03340 latches on commit at ST.2, Delusory Evils 52065 reacts at ST.6 — which is the argument against enumerating legal latch windows in the engine.

The skip is then a question asked at the `Resolving` step instead of a mechanism:

> If it is known that an investigator automatically succeeds or fails at a skill test before Step 3 ("Reveal chaos token") occurs, that step is skipped, along with Step 4. No chaos token(s) are revealed from the chaos bag, and the investigator immediately moves to Step 5. All other steps of the skill test resolve as normal.
>
> If a chaos token effect causes an investigator to automatically succeed or fail at a skill test, continue with Steps 3 and 4, as normal.

A determination already latched when the driver reaches `Resolving` means no draw, no `ChaosTokenRevealed`, no ST.4 push, and a straight advance to `DetermineOutcome`; a determination *caused* by the token cannot skip anything, because the reveal has already happened. The two clauses need no separate machinery — they are the same query at two different moments. Three Aces 06199 prints the skip on the card (*"that test automatically succeeds <i>(do not reveal chaos tokens from the chaos bag)</i>"*), confirming it is a distinct axis from the substitution rather than an optimisation of it.

Skipping the draw does **not** threaten replay. `action.rs` records no chaos draw at all — *"Chaos token draws and inline-during-handler deck shuffles do NOT use this channel — they happen as side effects of the action that triggered them"* — so determinism comes from replaying the same actions against the same `RngState`, and a skip that is a pure function of game state reproduces identically. The ticket listed "a skippable token draw that preserves the replay contract" as one of its three problems; it is not one.

Finally, the revealed token's own contribution becomes a recorded row, which ADR 0005 already specified and #677 did not build. Without it the fold's stage 5 would be a comment describing something that happens in the driver: the token's ±N is added after the query returns, which is why `ModifierBreakdown` still exposes an unclamped `raw_total()`. With it, the whole ST.5 total — base, additive rows, clamp, substitution — is one query, and `raw_total()` retires. The elder sign's bonus moves the same way, with its `Trigger::ElderSign` `IntExpr` **copied into the row unevaluated**.

## Considered options

**A pair of flags on `InFlightSkillTest`** was the obvious shape and reads cheaply at the skip check. It is also precisely the stored-snapshot field ADR 0005 removed, and it would be the second override path #628 was filed to prevent. The row buys the lifetime machinery and the attribution for free.

**Suppressing at write time** — an auto-fail row overwriting or blocking an auto-success row — keeps precedence out of the read path, but makes the *order* of latching load-bearing when the rule is order-independent, and destroys the fact that an automatic success was ever determined.

**Substituting in the driver and leaving the token out of the fold** is the smaller change and was seriously considered. It leaves the clamp in the driver, the substitution in the driver, and the fold's documented stage 5 describing neither — code contradicting a shipped ADR, which is worse than a larger diff.

**Resolving the elder sign's bonus to a literal when the token is revealed** was recommended during design on the reasoning that an elder sign is a printed constant with no live inputs. A sweep of all 106 investigators in the snapshot falsified it: twelve have a state-contingent modifier, and one of them, **Roland Banks 01001** (*"[elder_sign] effect: +1 for each clue on your location."*), is Core, in the corpus, and the only elder sign with a card impl today. Freezing it would have pinned his clue count across the ST.4 window — the staleness bug ADR 0005 exists to kill, reintroduced by the ticket completing it. Others: Agnes Baker 01004, Jenny Barnes 02003, Mark Harrigan 03001, Finn Edwards 04003, Norman Withers 08004, Bob Jenkins 08016, Lucius Galloway 11004, and two parallel investigators.

## Consequences

`InFlightSkillTest::token_resolution` goes away. Its sole reader was the total computation, which moves into the fold; `symbol_on_fail` is already a separate field, the `ChaosTokenRevealed` event already carries the token for display, and "was this test auto-resolved" is the determination query's answer rather than a field's absence.

`FailureReason` keeps its two variants and gains no rules meaning. `AutoFail`'s doc-comment stops saying a *chaos token* forced the total to 0, since a card can now produce the same reason with no token drawn, but the distinction stays display attribution: no Core + Dunwich card keys off a failure being automatic rather than ordinary, and the web client's `" (auto-fail)"` note reads correctly for either cause. The *cause* reaches the client through the event emitted when a determination is latched — without it an automatic success renders as an unexplained win with a `"—"` where the token would be.

`ModifierTarget::Test`'s doc-comment — *"No modifier targets it"* — stops being true. An automatic success substitutes the test's difficulty, not the enemy's fight or the location's shroud, and is the first row to target the test itself.

An elder sign that carries a determination rather than a number is still out of reach. Rex Murphy 02002 is the corpus consumer this ADR is written for, and it is blocked on the pure-modifier elder-sign scope note (#118, sunset #448) independently of anything here. Father Mateo 04004 (*"[elder_sign] effect: You automatically succeed. …"*) is out of corpus but shows the end state: an elder-sign trigger whose modifier is absent rather than zero.
