# Phase 9 — Campaign + Night of the Zealot

## Status

📐 Architecture only. No issues filed.

## Goal

Full Night of the Zealot campaign with deck import + progression.

## Decisions made

From the 2026-05-01/02 strategy phase:

- **Campaign log** is a typed `enum Fact { LitaJoined, GhoulPriestKilled, PrisonersSaved(u8), … }` — compile-time safety; scenarios can't read or write a fact that doesn't exist or has the wrong shape.
- **A campaign module is the top-level orchestrator:** scenario sequence + branching rules (`next_scenario(prev, resolution, log) -> Option<ScenarioId>`), between-scenario flows (deck upgrades, weakness draws, side stories), investigator persistence (decks, XP, trauma, alive/dead/insane).
- **Campaign state stored as snapshots** — XP, decks, campaign log, trauma, surviving investigators. Changes slowly, between scenarios. The fine-grained action log lives at the scenario level. Each campaign record links to N scenario records.
- **Deckbuilder: don't build one. Import + own.** Players build decks on arkham.build (or ArkhamDB) — already comfortable for the user's group, and deckbuilding rules (faction restrictions, level caps, taboo, customizable, parallel investigators) are arkham.build's problem.
- **Eldritch fetches deck data at import**, validates every card has an implementation (this is the gate that enforces "unimplemented = unplayable"), and stores a snapshot.
- **From import onward, Eldritch's copy is canonical** — story modifications (Lita joins, story weakness added) are applied to Eldritch's snapshot directly.
- **Between scenarios, players re-import** an updated arkham.build URL; Eldritch diffs the new deck against (old + persistent story mods) and validates.
- **Skip XP-cost validation at friends-only scale** — trust players to upgrade honestly, same as at a physical table.

## Open questions

⏳ **Scoping TBD.** When Phase 8 closes, file:

- **`Fact` enum scoping.** Every Night of the Zealot fact needs an enum variant. Audit the campaign rules to enumerate them.
- **Campaign module structure.** Trait + impl, or function-table, or plain Rust module per campaign?
- **Deck importer.** Read arkham.build / ArkhamDB JSON; parse into `Vec<CardCode>`; validate every code has an implementation; reject otherwise. UX for "your deck has unimplemented cards" — list them by name.
- **Story modifications.** When Lita joins, she's added as a permanent ally to the investigator's deck snapshot. Storage model: a "persistent story-asset" list per investigator-per-campaign.
- **Weakness draws between scenarios.** Some campaign step adds a basic weakness; the player draws one (or the engine assigns one). Determinism via the campaign-level RNG.
- **Trauma model.** Physical / mental trauma carry over between scenarios; affect max-health / max-sanity at scenario setup. Storage.
- **Per-scenario XP awards.** Each Night of the Zealot scenario's resolution carries an XP delta.

## Dependencies

- Phase 7 (The Gathering) — the first scenario.
- The other two Night of the Zealot scenarios (The Midnight Masks, The Devourer Below) need scenario modules implementing the same patterns.
- Phase 8 (multiplayer + auth) — campaigns are a group concept, so the auth/group model is in place.

## What "done" looks like

A friend group plays through the full Night of the Zealot campaign — three scenarios, with deck upgrades between, weakness draws, story branching based on resolutions. The campaign log records every load-bearing fact; the post-campaign state is queryable.
