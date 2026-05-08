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

- `pack/core/` — Core Set printings (original `core.json`, the 2026 reprint
  `core_2026.json`, the revised `rcore.json`, plus the matching encounter
  files).
- `pack/dwl/` — The Dunwich Legacy cycle: scenario packs (`dwl`, `tmm`,
  `bota`, `uau`, `wda`, `litas`, `tece`) and their encounter files.
- `schema/` — JSON schemas the upstream uses for validation; useful for
  the pipeline to assert the shapes it depends on.
- Top-level metadata: `cycles.json`, `encounters.json`, `factions.json`,
  `packs.json`, `subtypes.json`, `types.json`.
- `taboos.json` — official taboo list. Carries errata (text changes that
  affect how a card functions in play) plus XP / copy adjustments used at
  deckbuilding time. Errata are gameplay-relevant before the deckbuilder
  exists, so the data lives here even though the deckbuilding side
  doesn't land until Phase 9. Players opt into a specific taboo version;
  the engine applies whichever version a campaign was started under.
- This `SOURCE.md`.

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
