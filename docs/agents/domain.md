# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

**This repo is single-context.**

## Before exploring, read these

- **`CONTEXT.md`** at the repo root, and
- **`docs/adr/`** — read ADRs that touch the area you're about to work in.

Both exist. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) adds to them lazily, when terms or decisions actually get resolved.

## File structure

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-....md
│   └── 0002-....md
└── crates/
    ├── card-dsl/
    ├── game-core/
    ├── cards/
    ├── scenarios/
    ├── protocol/
    ├── server/
    ├── web/
    └── card-data-pipeline/
```

Eldritch is a Cargo workspace, but the crates are **layers of one domain model**, not separate bounded contexts — the crate layering and its rationale live in [`architecture.md`](architecture.md). If that ever stops being true, the switch to a multi-context layout is signalled by a root `CONTEXT-MAP.md` pointing at per-crate `CONTEXT.md` files plus crate-scoped `docs/adr/`; until that file exists, treat the repo as single-context.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (event-sourced orders) — but worth reopening because…_
