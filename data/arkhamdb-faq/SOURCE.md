# ArkhamDB card FAQ

Official rulings for Arkham Horror LCG cards, vendored one file per card as
verbatim markdown.

`<pack>/<code>.md` mirrors the card snapshot's own pack layout, so
`data/arkhamdb-snapshot/pack/core/…` and `data/arkhamdb-faq/core/…` line up.

**A card with no file has no rulings.** That is the point of the split: the
absence of a file is an answer, not a gap. `no-rulings.txt` lists every code
confirmed to have none, so "we never fetched this" and "there is nothing to
fetch" stay distinguishable.

See the project-level directive in [`CLAUDE.md`](../../CLAUDE.md) for when to
consult these files and how to cite them.

## Source

- **Endpoint:** `https://arkhamdb.com/api/public/faq/<code>.json`
- **Card page a ruling is quotable from:** `https://arkhamdb.com/card/<code>`
- **Fetched:** 2026-08-15 (each file records its own fetch date)

There is no upstream commit to pin and no conditional-GET shortcut: the
endpoint's `last-modified` header reflects a deploy timestamp rather than
per-card content, and it ignores `If-Modified-Since`. Provenance is therefore
the per-file metadata line, which records both when ArkhamDB last updated the
ruling and when we fetched it. A ruling untouched since 2017 is reassuring in
a way "we crawled in August" is not.

## Coverage

**The whole card snapshot**, not just the corpus — roughly 5,900 codes.
Rulings are most valuable exactly where the snapshot is: planning against cards
we have not implemented, where a ruling can reveal a mechanic the printed text
hides. Limiting this to Core and Dunwich would leave most card lookups pointing
back at the network, which is the problem the vendoring exists to solve.

Nothing here makes a card playable. The corpus is still Core plus Dunwich.

## File contents

Each file carries the card's name and code, a metadata line, and the rulings as
ArkhamDB returned them:

```markdown
# Roland Banks (01001)

Rulings last updated on ArkhamDB 2017-04-17; fetched 2026-08-15. Source: <https://arkhamdb.com/card/01001>

- You can trigger Roland's [reaction] reaction after you use any card under your control to defeat an enemy (e.g. [Guard Dog](https://arkhamdb.com/card/01021)).
```

**Card text is not duplicated into these files.** It lives in the snapshot and
would immediately drift.

The endpoint's `text` field is *mostly* markdown, so ingestion is close to
passthrough. Not all the way, though — about a fifth of the rulings have inline
HTML written into them by contributors, and every ruling links with
site-relative hrefs. No word is added, removed or reordered; what changes is:

- **Icon spans** arrive as empty HTML elements
  (`<span class="icon-reaction"></span>`) and become the same `[token]` form the
  card corpus and the ingested rules text already use. Left as HTML they would
  be invisible; dropped, the icon would be lost entirely.
- **Inline HTML** becomes its markdown equivalent: `<i>`/`<em>` → `*`,
  `<b>`/`<strong>` → `**`, `<s>`/`<del>` → `~~`, `<code>` → backticks, `<br>`,
  `<p>`, `<ul>`/`<li>` → the corresponding markdown breaks and bullets. `<u>`
  is dropped, markdown having no underline — the words are what carry the
  ruling. The point of vendoring is that a ruling can be quoted verbatim, and a
  quote with `<i>` in it is not that. `verify faq` fails on any tag outside this
  set, so a new one upstream is loud rather than silent.
- **Links are made to resolve.** `/card/01021` becomes an absolute
  `https://arkhamdb.com/card/01021`, since card text lives in the snapshot
  rather than in a local file. `/rules#Exhaust` becomes the vendored rules file
  it names, so following a ruling's citation never needs the network — which is
  the whole point. A rules anchor the page no longer has falls back to the
  absolute URL rather than a dead path.

Raw JSON is **not** committed. Its markdown field is already near-raw, and
5,900 raw files would be a large addition for little gain. The cost, taken
deliberately, is that FAQ verification is structural only — there is no
round-trip comparison available the way there is for the rules text.

## Refreshing

A refresh is an unavoidable full sweep of every code in the snapshot, because
there is no conditional-GET shortcut. It takes **several hours** — ArkhamDB is
paced politely, at roughly a card every three seconds. It is resumable; that is
not a nicety, it is what makes the sweep practical.

Assume you last thought about this six months ago. In the repo root:

1. **Start the sweep.**

   ```sh
   python3 scripts/arkhamdb.py fetch-faq --refresh
   ```

   Standard library only — there is nothing to install. A live progress line
   reports position, percentage, per-outcome counts, rate and estimated time
   remaining. Run it under `nohup`/`tmux` if your terminal might not survive it.

2. **If it is interrupted, run the same command again.** It resumes. A refresh
   writes a `.refresh-stamp` file (gitignored) holding the sweep's start date
   plus the codes it has already confirmed have no rulings; cards that *do*
   have rulings record their own fetch date in their file. Between the two,
   a resumed refresh skips everything the sweep already covered. The stamp is
   deleted on clean completion, so the next refresh starts a new sweep.

   If the run reports errored codes, re-run it — an errored code is deliberately
   left untouched on disk rather than recorded as having no rulings, so a
   transient timeout can never permanently mislabel a card.

3. **Verify.**

   ```sh
   python3 scripts/arkhamdb.py verify faq
   ```

   Structural checks only: every file sits at a pack-and-code path whose code is
   in the snapshot, no code appears both as a file and in `no-rulings.txt`,
   every file carries its metadata line, and every snapshot code is accounted
   for one way or the other. Nothing here runs in CI, so do not skip it.

4. **Update the Fetched date above.**

5. **Review with git.** `git status` and `git diff` are the change report:
   which rulings changed, which cards gained a file, which lost one. A summary
   held in the script's memory would not survive an interruption; version
   control gives an exact account however many times the sweep was resumed.
   Thousands of near-passthrough files cannot be read end to end — spot-check.

To *finish* an interrupted first crawl rather than start a fresh sweep, drop
the flag:

```sh
python3 scripts/arkhamdb.py fetch-faq
```

That skips anything already on disk or already listed as having no rulings.

## Attribution

The FAQ content here is **ArkhamDB's**, at <https://arkhamdb.com>. It is
community-authored collation of official rulings, a different rights situation
from the publisher's own rules text, and it is vendored for offline verbatim
citation in a friends-scale hobby project. Quote it, cite it, and link back to
the card page — the same "their content, we point at it" posture that governs
never re-hosting card art. See
[`docs/product-decisions.md`](../../docs/product-decisions.md).
