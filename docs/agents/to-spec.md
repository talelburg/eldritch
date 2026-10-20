# Running `/to-spec` and `/to-tickets` here

`/to-spec` turns a grill into a spec, and `/to-tickets` cuts the spec into a ticket graph. Both publish to the tracker, and `/implement-spec` takes what they publish as true. This file is the one step the upstream skills leave out: **before publishing, check every claim the draft makes about current code.** Read it before either skill publishes.

## Check the draft's claims against the code

A spec or ticket that describes the code as it is today is making a claim, and a grep settles most of them. Before publishing:

- **Grep every count.** Call sites, construction sites, test files, cards that use a pattern: the number goes in the draft as the grep returned it.
- **Grep every "unchanged" and "passes without edits".** For a variant that gains a field, find its construction sites. For an order or an output kept as it is, find the tests that pin it. A test that pins incidental behaviour the change moves isn't a reason to promise it stays; the ticket edits it (`standards.md` → A test that pins incidental behaviour is not a contract). Say which tests that is, so the ticket's size is visible.
- **Confirm every acceptance criterion is reachable.** Some code path the ticket builds has to make it true. A criterion that needs a primitive no ticket adds, or a card the corpus doesn't have, is not reachable.
- **Confirm each ticket's test can pass where the ticket order puts it.** The test may use only what the ticket itself and its blockers build. A test that needs a later ticket's code means a missing blocking edge or a wrong cut.

A refuted claim is fixed in the draft before it is published. Where fixing it changes what the user approved, such as a ticket's size or a promised behaviour, it is a gate (`CLAUDE.md` → Gates), with options and a recommendation.

The check is read-only and can be long. Dispatched to a subagent, it is a `general-purpose` one told to stay read-only in the repo and to write its findings to `/tmp/<run>/` (`CLAUDE.md` → Agent skills).

**Why:** every `/implement-spec` run up to #1148 opened with 4 to 11 gates, and many were claims a grep refutes: "passes without edits", "order unchanged", call-site counts. #1000 lost about 3 hours to one: a killed implementer, a re-grill, and the spec and tickets rewritten. #992 was aborted as "too much to design in an implementation branch". [`implement-spec.md`](implement-spec.md) says the same of #962: three of its five gates "were visible from the code while the spec was being written". By then the user was typing `/to-spec verify please` by hand.
