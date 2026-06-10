# Arkham Horror LCG Campaign Guides

This directory holds official campaign guides — the canonical source for
**scenario setup data** that is *not* on the cards themselves: chaos-bag
compositions per difficulty, encounter-set lists, location layout/placement,
set-aside cards, and resolution branching.

Card text lives in [`../arkhamdb-snapshot`](../arkhamdb-snapshot); procedural
rules live in [`../rules-reference`](../rules-reference). Campaign guides are
the third leg: the per-scenario "how to set up the table" data. Consult this
when implementing a scenario's `setup()` (bag, encounter deck, starting board)
or its resolutions.

## Pinned files

### `night_of_the_zealot_campaign_guide.pdf`

- **Source URL:** <https://images-cdn.fantasyflightgames.com/filer_public/8d/30/8d308b73-92f1-4b1e-aa7f-ce39e8d79786/night_of_the_zealot_campaign_guide.pdf>
- **Publisher:** Fantasy Flight Games
- **Pulled:** 2026-06-11
- **Covers:** The Gathering, The Midnight Masks, The Devourer Below; the
  Night of the Zealot campaign setup (incl. the Easy/Standard/Hard/Expert
  chaos-bag compositions on page 1).

Vendored in-repo rather than referenced by URL because Fantasy Flight's
`filer_public` CDN has restructured several times; a stable local copy beats a
link that may rot. Same rationale as `../rules-reference`.

## Standard-difficulty chaos bag (Night of the Zealot)

Recorded here for quick reference (verbatim from page 1, "Campaign Setup →
Assemble the campaign chaos bag", Standard):

`+1, 0, 0, −1, −1, −1, −2, −2, −3, −4, [skull], [skull], [cultist], [tablet],
[auto-fail], [elder sign]`

— +1×1, 0×2, −1×3, −2×2, −3×1, −4×1, Skull×2, Cultist×1, Tablet×1,
AutoFail×1, ElderSign×1 (16 tokens; no elder-thing token in the core set).
