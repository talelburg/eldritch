# Writing agent-facing documentation

The house style for the docs an agent reads to work here: `CLAUDE.md`, `docs/agents/*`, `docs/adr/*`, and `docs/phases/README.md`. `code-review`'s **Standards** axis reads this file when a diff touches one of them, and so should you before adding a rule to `CLAUDE.md` or writing an ADR.

Two files are deliberately out of scope. `CONTEXT.md` already has its own stated admission bar and a consistent entry shape. `docs/audits/*` are dated point-in-time records, not living docs — they are not rewritten into a style they were never written in.

What this file is defending against is **accretion**, not length. The citation-heavy prose is the edge: a rule carrying its verbatim Rules Reference quote and the PR where ignoring it broke The Gathering is a rule an agent obeys, where a bare imperative is one it rationalises past. What has to go is content whose only remaining value is history — which git already keeps.

## The `CLAUDE.md` admission bar

`CLAUDE.md` loads into every session, so residency is a recurring tax on all work, including the work it has nothing to say about. Two routes admit a rule, and a candidate meeting neither belongs in a pointed-at doc.

**Route one — would an agent go wrong before it knew to look this up?** A pointer is safe exactly when the agent has already arrived somewhere that names it: an agent with a card module open will find the `EventTiming` convention from `docs/agents/standards.md`, so the convention does not need to ride along in every session. A rule the agent needs *before* it knows what the task is has nowhere else to live.

**Route two — would an agent violate this from confidence rather than ignorance?** The card-text and rules citation mandate is the motivating case. Nothing in a rules question prompts an agent to check whether the sources are vendored; the reflex is to reach for `WebFetch`, and by the time it has fired the answer is a summary, which is a wrong rules clause. A rule that only works if it is read *before* the wrong move is made cannot sit behind a pointer.

**A pointer states what you would get wrong by skipping it, not just its topic.** "See `docs/agents/standards.md`" is a topic heading an agent skips; naming the mistake is what makes the hop worth taking.

## The ADR shape

ADRs follow the `mattpocock-skills` suite's `ADR-FORMAT.md` — the file `/domain-modeling` loads, which is not vendored here; read it from the skill — on **location** (`docs/adr/`), **sequential numbering**, **decision-as-title**, and the **three-part admission test** (hard to reverse, surprising without context, the result of a real trade-off; all three must hold). That test is restated operationally in [`docs/phases/README.md`](../phases/README.md) under "Maintaining these docs".

**This repo overrides the suite on size only, and the override is a licence rather than a mandate.** An ADR here *may* run long where the evidence needs the room — a verbatim rules quote, the `file:line` the old shape broke at, the ruling that settles a card. It is not required to, and the smaller ADR is the default. Sections are optional: **Considered options** appears only where the rejected alternatives carry evidence worth remembering, **Consequences** only where a downstream effect is genuinely non-obvious. Both currently appear in ten of ten ADRs, which is the convention this reverses.

Naming it as an override matters because an agent that has loaded `/domain-modeling` is holding `ADR-FORMAT.md` in context telling it most ADRs need no sections at all. Contradicting that silently produces an agent that follows whichever spec it happened to read last.

## Fold, don't append; reverse by superseding

**A refinement to a settled decision is folded into the ADR's present-tense prose**, with a one-line changelog footer recording which tickets were folded. ADR 0010 is the counter-example this rule was written from: until #745 folded it, it was a decision plus three chronologically-appended amendments, one of which read *"that is the part of this ADR that was wrong"*, so a reader had to diff four sections in their head to learn what the project currently believed.

**A decision that genuinely reverses an earlier one becomes a new ADR**, and the old one gets the suite's optional `superseded by ADR-NNNN` status field. Folding a reversal erases that the project once believed the opposite — which is precisely the thing someone re-proposes.

## Growth control, without a line cap

**There is deliberately no line cap on `CLAUDE.md`.** A hard ceiling makes a necessary rule pay for an unrelated one's bulk: the next rule to be rejected would be whichever one arrived after the budget ran out, not the weakest one. Three mechanisms instead.

1. **A PR adding to `CLAUDE.md` names the admission route in its body** — route one or route two, in a sentence. Growth is argued, not assumed.
2. **A rule written for a transitional state carries its terminal condition inline**, and names which parts go when that condition is met. [ADR 0008](../adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md) is the model: it says outright that its reject *"is scaffolding with a terminal condition"* and that *"the arm, the classification and the reject are deleted together"* when the last condition flips. `CLAUDE.md`'s licensed-mismatch rule was the same rule written without that clause: by the time #744 deleted it, it stated that no card held the licence any more and then spent another eighty words on a mechanism that stays live for other reasons, and nobody could tell from the rule itself which half was supposed to go.
3. **A documentation sweep at each phase close**, as the backstop for whatever the first two missed. The phase-close procedure owns that step — [`docs/phases/README.md`](../phases/README.md), "Maintaining these docs" — and this file is what the sweep reads.

## De-duplication direction

When a rule has landed in two places, the default is: **the ADR owns the *why*; the operational doc owns the *do this*, and cites the ADR.** That way ten dedup calls are not ten coin-flips.

The escape hatch: a duplication that is genuinely both — a decision whose *statement is* the instruction — is argued case by case rather than forced through the default. Say which it is in the PR body.

## Review checklist

A documentation PR is read against these.

- **Does this rule carry its evidence?** The PR or `file:line` where its absence cost something, or the verbatim clause it implements.
- **Does a transitional rule name its terminal condition, and which parts go with it?**
- **Is this an appended amendment that should have been folded?**
- **Does a new `CLAUDE.md` entry name its admission route?**
- **Does a pointer say what you would get wrong by skipping it?**
- **Is anything here now history?** Per-PR narration, migration state whose migration finished, a decision restated in the doc that already owns it.
