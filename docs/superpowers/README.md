# Superpowers plans and specs — closed archive

**Closed to new entries as of 2026-08-13.** Nothing new is written here. See [ADR-0001](../adr/0001-workflow-runs-on-mattpocock-skills.md).

These 253 files (138 in `plans/`, 115 in `specs/`) are the design specs and implementation plans produced between **2026-05-21** and **2026-07-17**, while the repo's workflow ran on the `superpowers` skills. That plugin is no longer enabled; work now runs on the `mattpocock-skills` suite, where specs come from `/to-spec` and tickets from `/to-tickets`.

## Why this is kept rather than deleted

These are **primary sources**, not residue. Phase docs 5, 6 and 7 cite 29 of them — two as live markdown links — and **phase 7 is the active phase**, so a good number of these describe work still in progress. Deleting the tree would break those links and remove context from current work.

The directory name refers to the tool that produced the files, not to any doctrine the repo still follows.

## Where these things live now

| Then | Now |
|---|---|
| `specs/*-design.md` | `/to-spec` output; design decisions become ADRs in `docs/adr/` |
| `plans/*.md` | `/to-tickets` output — one GitHub issue per ticket, with blocking edges |
| Phase-level context | `docs/phases/phase-N-<slug>.md` (unchanged) |
