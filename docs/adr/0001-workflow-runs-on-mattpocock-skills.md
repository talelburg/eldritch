# Workflow doctrine runs on the mattpocock-skills suite

The repo previously mandated the `superpowers` skills (brainstorm → TDD plan → task-by-task execution) in `CLAUDE.md`, and the plugin injected a session-start instruction reinforcing it. We replaced that with the `mattpocock-skills` suite, disabled the `superpowers` plugin, and moved the domain terminology that had accumulated in `CLAUDE.md` into a real glossary at `CONTEXT.md`. The reason is that most of what `CLAUDE.md` carried was never workflow — it was domain vocabulary and verification discipline sharing a file with process instructions, and the suite gives each of those a proper home (`CONTEXT.md`, `docs/adr/`, `docs/agents/`).

## Considered options

**Keeping `superpowers` enabled alongside the suite** was rejected because its plugin-provided `SessionStart` hook mandates invoking superpowers skills before responding — a standing instruction that would actively fight the new doctrine every session. Half-migrating was not available; the hook lives and dies with the plugin toggle.

**Deleting `docs/superpowers/`** (253 tracked plan and spec files) was rejected because phase docs 5, 6 and 7 carry 29 citations into that tree, two of them live markdown links, and **phase 7 is the active phase**. Those files are current working context, not historical residue. The tree is frozen in place with a README marking it closed to new entries.

**Retiring `docs/phases/`** was rejected because the suite has no analogue for the phase-level narrative — status, goal, what shipped, dependencies, done-criteria — or for the "read this before picking up an issue" entry point. The suite does cover the phase docs' other jobs, so their role narrowed instead: ordering lives in ticket blocking edges, and design decisions now become ADRs.

## Consequences

Four superpowers skills had no analogue and were consciously dropped: `verification-before-completion` (covered more strictly by the CI-gauntlet rule in `CLAUDE.md`), `finishing-a-development-branch` (covered by the PR procedure), `using-git-worktrees` and `dispatching-parallel-agents` (tool-level habits, not doctrine).

Every flow entry point in the suite — `grill-with-docs`, `to-spec`, `to-tickets`, `implement`, `wayfinder`, `triage` — is user-invocation-only. An agent cannot start one; it must ask. This is a deliberate shift of the agency boundary toward the user, replacing an earlier preference for running non-trivial issues straight through to an opened PR without pausing.

Closed phases were **not** retro-migrated: their existing "Decisions made" entries stay in the phase docs, and only new decisions become ADRs. Phase 7's drift from the phase-doc template is likewise left alone.
