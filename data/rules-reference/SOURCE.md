# Arkham Horror LCG Rules Reference

This directory holds the official Rules Reference PDF. It is the
canonical source for procedural-rules behavior — ability timing,
trigger windows, framework events, skill-test resolution sequence,
action structure, anything that says "how the game runs."

See the project-level directive in [`CLAUDE.md`](../../CLAUDE.md) for
when to consult this document.

## Pinned file

- **File:** `ahc01_rules_reference_web.pdf`
- **Source URL:** <https://images-cdn.fantasyflightgames.com/filer_public/c4/b0/c4b0d66c-d79e-411b-bdb5-b5d8c457d4bc/ahc01_rules_reference_web.pdf>
- **Publisher:** Fantasy Flight Games
- **Pulled:** 2026-05-19

Vendored in-repo rather than referenced by URL because Fantasy Flight's
`filer_public` CDN has restructured several times in recent years; a
stable local copy beats a link that may rot.

## Updating

When FFG publishes a new edition (rare — rule revisions are usually
captured as FAQ entries), pull the new PDF, replace
`ahc01_rules_reference_web.pdf` here, update the **Source URL** and
**Pulled** fields above, and note the revision in the PR description.
The CLAUDE.md directive doesn't need to change unless the filename
does.

## What's NOT here

- **FAQ / rulings sheets.** Lives upstream at
  <https://www.fantasyflightgames.com/en/products/arkham-horror-the-card-game/>.
  When a ruling clarifies or supersedes the Rules Reference, prefer
  the FAQ; cite both in PR descriptions where the question is
  load-bearing.
- **Designer commentary, ArkhamDB rule discussion threads.** Useful
  context, but secondary to the Rules Reference itself.
