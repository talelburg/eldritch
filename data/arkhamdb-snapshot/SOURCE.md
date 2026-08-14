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

## What's included

- `pack/core/` — Core Set printings: the original `core.json` +
  `core_encounter.json`, the 2026 reprint (`core_2026.json` +
  `core_2026_encounter.json`), and the revised `rcore.json` (which has no
  separate encounter file upstream).
- `pack/dwl/` — The Dunwich Legacy cycle: scenario packs (`dwl`, `tmm`,
  `bota`, `uau`, `wda`, `litas`, `tece`) and their encounter files.
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

### What the pipeline actually ingests

`PACK_FILES` in `crates/card-data-pipeline/src/main.rs` reads the
**old-format** files only: `core.json` + `core_encounter.json` and the
seven `dwl` packs + their encounter files. `core_2026*.json` and
`rcore.json` are pinned here for reference but are **not** ingested —
adding them to the build requires extending `PACK_FILES` (and reconciling
duplicate codes across printings), not just bumping the snapshot.

### Why the other printings aren't ingested

The two Core printings Eldritch skips are skipped for different reasons,
and neither is a backlog item:

- **`rcore.json` — Revised Core Set** (FFG, 2020; codes `015xx`). All 116
  entries are **skeletons**: `code`, `position`, `quantity`,
  `illustrator`, and an `alternate_of` / `duplicate_of` pointer at the
  matching `010xx` card — no name, type, or class. Upstream never
  populated them, because the revised product was a repackage (one copy
  of each card rather than needing two Cores, and the Limited
  deckbuilding restriction dropped) whose gameplay content *is*
  `core.json`. There is nothing here to ingest.
- **`core_2026.json` — Core Set (2026)**, the Asmodee "Chapter 2" set
  (codes `120xx`, `cycle_code: core_ch2`). **New content, not a
  reprint** — new investigators (Daniela Reyes, Joe Diamond, Trish
  Scarborough, Dexter Drake, Isabelle Barnes), 74 of 104 entries
  populated at the pinned commit. Excluded by scope, not by data quality.

The same old/new split runs through the cycle expansions. The old format
(FFG, ~2016–2021) is a deluxe expansion plus six mythos packs — the seven
`pack/dwl/*.json` files, which is what the pipeline reads. The new format
(Asmodee, ~2022+) repackages a cycle into an Investigator Expansion and a
Campaign Expansion: `packs.json` lists `dwlp` and `dwlc`, each with a
`reprint_packs` array naming all seven old-format packs, but **no
`dwlp.json` or `dwlc.json` exists upstream** — ArkhamDB hasn't catalogued
files whose content is already covered by what it has.

**A physical Revised Core + new-format Dunwich collection plays
identically to the ingested data.** The codes printed on those cards
differ from the ones the simulator uses, which is cosmetic. So don't
reach for `rcore` / `dwlp` / `dwlc` to "match" a physical collection —
there is no gameplay difference to capture, and no usable data upstream
to capture it with.

## What's deliberately excluded

- All packs outside Core + Dunwich (`pack/ptc/`, `pack/tfa/`, etc.) —
  Eldritch's Phase 2/3 scope is Core + Dunwich only. Add the relevant
  pack directory here when widening coverage.
- `translations/` — Eldritch is English-only for now.
- Upstream tooling (`replace.php`, `update_locales.coffee`,
  `validate.py`, `package.json`, etc.) and CI / editor config files
  (`.travis.yml`, `.prettierrc`, `.editorconfig`, `.github/`).
- Upstream's `README.md` and `illustrator_aliases.json` (decorative).

## Updating

Bumping the snapshot is intentionally manual. To refresh:

1. Clone the upstream repo at the desired commit.
2. Replace `pack/core/`, `pack/dwl/`, `schema/`, and the top-level
   metadata JSONs from the new clone.
3. Update the **Pinned commit** section above with the new SHA and date.
4. Run the card-data-pipeline (`cargo run -p card-data-pipeline`) and
   review the diff in `crates/cards/src/generated/`.
5. Open a PR; the CI doc/lint/test gates plus reviewer eyes catch any
   schema drift.
