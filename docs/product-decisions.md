# Product decisions

The standing product, legal, and hosting posture. `CLAUDE.md` covers how the code is built; this covers what the project *is*, which is what makes a decision here expensive to reverse.

Everything below was settled during the 2026-05 strategy phase and has held since. Proposing something that crosses one of these is fine — noticing that it crosses one is the point.

## A full rules engine, not a virtual tabletop

The server understands the game — phases, skill tests, card effects, scenario advancement — and enforces it. It is not a shared table where players move pieces and adjudicate among themselves.

Chosen with the cost in view: *"playing the game should be as smooth as possible, regardless of the size of the work for us."* That is a standing tie-breaker — **where player experience and implementation cost trade off, player experience wins.** The price is a forever-commitment to implementing each new card pack, a structured effect DSL rather than free text, and serious test infrastructure for rules edge cases. All three are visible in the codebase; only the reason for paying them lives here.

The hard consequence is the playability bar: **a card is either fully implemented or unavailable.** There is no manual-resolution fallback, no "resolve this one yourselves" escape hatch. Unimplemented cards still exist in the corpus so deckbuilding can see them (`cards::is_playable`; see `CLAUDE.md` → Hybrid card-effect DSL).

## Audience and scale

A hobby project for the author and a small group of friends. Not commercial, not a public service, no monetization, no SLA. The bar is "make game night work" — quality matters because the game deserves it, not because anyone depends on it.

This is the constraint the rest of the page inherits. It licenses the single-VM hosting, the invite-only auth, and the unvalidated XP costs below, each of which is under-built *deliberately*. If the audience ever expands materially, revisit them together rather than one at a time.

**Multiplayer is synchronous only** — everyone online at once, in a room or on a call. Solo play (one player running one or two investigators) is a first-class mode, not degraded multiplayer, and the session model accommodates a session of one without special-casing. Don't invest in async-turn UX: turn deadlines, per-turn push notifications, play-by-email flows. The event log already gives resume, undo, and debugging for free; async simply isn't a target.

## Legal posture

Exposure is effectively zero at friends-only scale. These are what keep it there:

- **The repo is public** ([talelburg/eldritch](https://github.com/talelburg/eldritch)), **MIT-licensed**, carrying the unofficial-fan-tool disclaimer in `README.md`. It went public on 2026-05-02, when GitHub Free's branch protection turned out to require Pro on private repos — a state strategy had already pre-approved.
- **Never re-host card art.** Link to ArkhamDB's image URLs — their CDN, their bandwidth, their takedown surface — with a text-only fallback when an image is missing.
- **"Eldritch" is a deliberately non-infringing name.** Fantasy Flight's marks — "Arkham Horror" among them — stay out of the product name, the domain, and the UI chrome. Naming packs, scenarios, and cards by their real titles is unavoidable and fine; naming the *product* after them is not.
- No embedded secrets, and no ownership-declaration UX — players are not asked to attest that they own the physical cards.

## Hosting and auth (v1)

The smallest thing that works: one small VM ($5–10/month) or self-hosting on home hardware. No Kubernetes, no autoscaling, no CDN. A `Dockerfile` is cheap portability insurance, not a requirement.

Auth is **invite-only** — GitHub or Google OAuth against a hand-maintained email allowlist. No signup flow, no password reset, no email verification, no abuse handling. Those are the first things to build if scope ever expands, and deliberately the first things skipped until then. This lands in Phase 8.

## Decks are imported, not built

**Don't build a deckbuilder.** Deckbuilding rules — class restrictions, level caps, taboo, customizable cards, parallel investigators — are [arkham.build](https://arkham.build)'s problem, and the group already uses it.

Eldritch imports a deck by URL, validates that **every card in it has an implementation** (this is where the playability bar is enforced), and stores a snapshot. From import onward **Eldritch's snapshot is canonical**: story modifications — Lita joining, a story weakness added — are applied to it directly and never round-tripped back out. Between scenarios players re-import an updated URL, and Eldritch diffs it against the old deck plus its persistent story modifications.

**XP costs are not validated.** Players are trusted to upgrade honestly, exactly as at a physical table. An audience-and-scale decision, not an oversight.

## The frontend has an exit ramp; the engine doesn't

Rust + WASM (Leptos) was chosen for language cohesion, with the trade-offs understood and accepted: a smaller UI ecosystem than React, larger bundles, slower iteration on UI work. **A pivot to React is an acknowledged exit ramp** should Rust/WASM prove to hold the project back.

That leaves a standing architectural obligation: keep the seam between `game-core` — which outlives any frontend, and which both `server` and `web` consume — and the UI layer clean enough that taking the ramp would not touch the engine. Anything that pushes Leptos-shaped assumptions below `crates/web` spends an option the project is deliberately holding open.
