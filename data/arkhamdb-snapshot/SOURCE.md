# ArkhamDB JSON data snapshot

This directory is a manually-pinned subset of the upstream
[arkhamdb-json-data](https://github.com/Kamalisk/arkhamdb-json-data) repo.
Eldritch's `card-data-pipeline` reads it as the source of truth for card
metadata.

## Pinned commit

- **Upstream:** `https://github.com/Kamalisk/arkhamdb-json-data`
- **Commit:** `9a9c965b4872d780cb9a3a93e5b612f7c3487972`
- **Date:** 2026-05-05
- **Snapshot pulled:** 2026-05-08

## Scope: a rule, not a list

In scope is **everything upstream except the Asmodee Chapter 2 line** —
cycles `core_ch2` and `investigator_decks_ch2`. Stated as a rule so that a
future snapshot bump adding a Chapter 1 side story is picked up without
anyone re-deciding scope.

Note that **the chapter split does not fall on directory boundaries**:
`pack/investigator/` holds the five Chapter 1 starter decks (`nat`, `har`,
`win`, `jac`, `ste`) alongside the five `investigator_decks_ch2` files
(`tom`, `car`, `and`, `mar`, `mig`). We vendor whole upstream directories
anyway and classify inside them, so re-pinning stays a straight directory
copy. Upstream directory names are mirrored **verbatim**, which means a
couple of them do not match their cycle codes: `pack/side/` is the
`side_stories` cycle and `pack/promo/` is `promotional`.

## Snapshot vs. corpus

These are different sets, and the distinction is load-bearing (see
`CONTEXT.md`):

- The **snapshot** is everything in this directory — all of Chapter 1.
  Most of it is **planning input**: pinned so decisions about the DSL and
  the engine can be made against the full set of cards Eldritch will
  eventually support.
- The **corpus** is what the pipeline ingests and the build compiles —
  Core + Dunwich, emitted as `crates/cards/src/generated/cards.rs`.

Vendoring a pack does **not** make it playable, or even visible to the
engine. Promotion is a deliberate edit to `PACK_FILES`.

## What's included

- `pack/` — every Chapter 1 pack directory: `core`, `dwl`, `ptc`, `tfa`,
  `tcu`, `tde`, `tic`, `eoe`, `tsk`, `fhv`, `tdc`, plus the non-cycle
  groupings `return`, `side`, `promo`, `parallel` and `investigator`.
  Both player files and their `*_encounter.json` companions — encounter
  cards are where forward-compatibility risk concentrates (treachery
  effects, enemy keywords, act/agenda structure), and the four new-format
  cycles ship a single `<code>c.json` that cannot be split anyway.
- `schema/` — JSON schemas the upstream uses for validation. Kept for
  reference when diagnosing a malformed entry; the pipeline does **not**
  read or enforce them.
- Top-level metadata: `cycles.json`, `encounters.json`, `factions.json`,
  `packs.json`, `subtypes.json`, `types.json`.
- `taboos.json` — official taboo list. Carries errata (text changes that
  affect how a card functions in play) plus XP / copy adjustments used at
  deckbuilding time. Errata are gameplay-relevant before the deckbuilder
  exists, so the data lives here even though the deckbuilding side
  doesn't land until Phase 9. Players opt into a specific taboo version;
  the engine applies whichever version a campaign was started under.
- This `SOURCE.md`.

### Two file formats

The old format (FFG, ~2016–2021) is a deluxe expansion plus six mythos
packs — 7 player files and 7 encounter files per cycle, which is what
`core`, `dwl`, `ptc`, `tfa`, `tcu`, `tde` and `tic` look like. The new
format (Asmodee, ~2022+) repackages a cycle into an Investigator
Expansion and a Campaign Expansion, so `eoe`, `tsk`, `fhv` and `tdc` are
two files each: `<code>p.json` (player) and `<code>c.json` (campaign).

For the old-format cycles, `packs.json` *also* lists new-format reprint
packs (`dwlp`/`dwlc`, `ptcp`/`ptcc`, and so on) whose `reprint_packs`
arrays name the seven old-format packs — but **no such file exists
upstream**, because ArkhamDB never catalogued content it already has.
They are on the pipeline's `PACKS_WITHOUT_FILES` exception list.

### How files are classified

Every vendored pack file falls into exactly one of three lists in
`crates/card-data-pipeline/src/main.rs`, and the pipeline **fails** on
any file in none of them:

1. `PACK_FILES` — the corpus. Core + Dunwich, player and encounter.
2. `REFERENCE_FILES` — in scope, vendored as planning input, not
   compiled.
3. `OUT_OF_SCOPE_FILES` — present but deliberately not ours (below).

A fourth check runs the other way: every pack in `packs.json` whose cycle
is in scope must have a vendored file, unless it is on
`PACKS_WITHOUT_FILES`. That is what catches a *forgotten* directory, as
opposed to an unclassified one.

`REFERENCE_FILES` is kept distinct from `OUT_OF_SCOPE_FILES` rather than
lumping both under "not compiled", so that anything sampling "every
Chapter 1 card" cannot silently draw from Chapter 2.

### What's in `OUT_OF_SCOPE_FILES`, and why

Chapter 2 content in two groups, plus one Chapter 1 oddity:

- **`core_2026.json` / `core_2026_encounter.json` — Core Set (2026)**, the
  Asmodee "Chapter 2" set (codes `120xx`, `cycle_code: core_ch2`). **New
  content, not a reprint** — new investigators (Daniela Reyes, Joe
  Diamond, Trish Scarborough, Dexter Drake, Isabelle Barnes), 74 of 104
  entries populated at the pinned commit. Excluded by scope.
- **The five `investigator_decks_ch2` decks** — `tom`, `car`, `and`,
  `mar`, `mig`. Excluded by scope; present only because they share
  `pack/investigator/` with the Chapter 1 starter decks.
- **`rcore.json` — Revised Core Set** (FFG, 2020; codes `015xx`). In a
  Chapter 1 cycle, but excluded for a different reason: all 116 entries
  are **skeletons** — `code`, `position`, `quantity`, `illustrator`, and
  an `alternate_of` / `duplicate_of` pointer at the matching `010xx`
  card, with no name, type or class. Upstream never populated them,
  because the revised product was a repackage whose gameplay content *is*
  `core.json`. There is nothing to plan against.

**A physical Revised Core + new-format Dunwich collection plays
identically to the ingested data.** The codes printed on those cards
differ from the ones the simulator uses, which is cosmetic. So don't
reach for `rcore` / `dwlp` / `dwlc` to "match" a physical collection —
there is no gameplay difference to capture, and no usable data upstream
to capture it with.

## What's deliberately excluded

- The Chapter 2 line — see the scope rule above.
- `translations/` — Eldritch is English-only for now.
- Upstream tooling (`replace.php`, `update_locales.coffee`,
  `validate.py`, `package.json`, etc.) and CI / editor config files
  (`.travis.yml`, `.prettierrc`, `.editorconfig`, `.github/`).
- Upstream's `README.md` and `illustrator_aliases.json` (decorative).

## Updating

Bumping the snapshot is intentionally manual. To refresh:

1. Clone the upstream repo at the desired commit.
2. Replace `pack/`, `schema/`, and the top-level metadata JSONs from the
   new clone — whole directories, per the scope rule above, skipping any
   directory belonging solely to an out-of-scope cycle.
3. Update the **Pinned commit** section above with the new SHA and date.
4. Classify anything new. `cargo test -p card-data-pipeline` will name
   every file that landed in no list and every in-scope pack that arrived
   without one; put each into `PACK_FILES`, `REFERENCE_FILES` or
   `OUT_OF_SCOPE_FILES`.
5. Run the card-data-pipeline (`cargo run -p card-data-pipeline`) and
   review the diff in `crates/cards/src/generated/`. If `PACK_FILES` did
   not change, expect no diff at all.
6. Open a PR; the CI doc/lint/test gates plus reviewer eyes catch any
   schema drift.
