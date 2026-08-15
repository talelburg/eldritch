# Arkham Horror LCG Rules Reference

This directory holds the canonical source for procedural-rules behavior —
ability timing, trigger windows, framework events, skill-test resolution
sequence, action structure, anything that says "how the game runs."

That canonical source is **[`rules/`](rules/)**: verbatim markdown ingested
from ArkhamDB's rules page. Start at [`rules/README.md`](rules/README.md),
which indexes every section and glossary entry.

The publisher's own PDF is also here, but it is **not** what you read. See
"The PDF" below.

See the project-level directive in [`CLAUDE.md`](../../CLAUDE.md) for when to
consult this material and how to cite it.

## The ingested rules text

- **Source URL:** <https://arkhamdb.com/rules>
- **Fetched:** 2026-08-15
- **Raw HTML:** [`raw/rules.html`](raw/rules.html),
  sha256 `8ba1beddfaed79233edbd85febea87f7b8dc59886bcfc7f14ee81de268f7ca71`
- **Author of the page:** ArkhamDB, reproducing Fantasy Flight Games' rules
  text. See "Attribution" below.

Unlike the card snapshot, there is no upstream commit to pin: ArkhamDB serves
the rules as a page, not a repository. Provenance is therefore the URL, the
fetch date, and the hash above.

ArkhamDB's page is a **superset of the printed Rules Reference**. Its own
preamble ([`rules/Intro.md`](rules/Intro.md)) says so: it is a replica of the
Core Set Rules Reference plus rules added in deluxe expansions and rules added
in the official FAQ. That is why it, rather than the PDF, is canonical here —
the PDF is the 2016 Core Set edition and predates every keyword introduced by
a deluxe expansion (`Alert`, `Bonded`, Bless and Curse tokens, Frost tokens,
`Concealed X`, …) as well as every FAQ amendment, including "The Silver Rule".
The snapshot spans all of Chapter 1, so most of what we plan against is
outside what the PDF can rule on.

### Layout, and why it is shaped this way

```
rules/
  README.md                        generated index of every section and glossary entry
  Intro.md                         ArkhamDB's note on what the page is
  The_Thing_That_Should_Not_Be.md  the golden, grim and silver rules
  Appendix_I_Initiation_Sequence.md
  Appendix_II_Timing_and_Gameplay.md
  Appendix_III_Setting_Up_The_Game.md
  Appendix_IV_Card_Anatomy.md
  glossary/                        one file per glossary entry
```

The document is 86% Glossary. Splitting by top-level section alone would leave
one 200KB file, which is the "grep and hope you got the right slice" failure
mode the split exists to prevent. So:

- **The non-Glossary sections stay whole.** They are procedural sequences meant
  to be read in order, and none exceeds 14KB. Appendix II's skill-test timing
  in particular must never be read in fragments.
- **The Glossary splits one file per entry.** The Glossary is already a
  reference of standalone alphabetical entries, so per-entry files are its own
  structure, not a shredding of it. Entries are small and evenly sized, so
  every one can be read whole. `README.md` covers the one thing a single large
  file is better at: skimming what exists.

**Filenames are ArkhamDB anchor ids verbatim.** A pre-existing reference to
`#Skill_Test_Timing` maps onto a filename with no translation step. Three ids
contain a space upstream (`Swarming X`, `Tarot Slot`,
`Record_in_your_Campaign Log`); those spaces are kept, because normalising them
would reintroduce exactly the translation step the rule exists to remove. Six
glossary entries carry no id upstream (`Enemy Phase`, `Experience`,
`Investigation Phase`, `Mythos Phase`, `Trauma`, `Upkeep Phase` — all of them
pointer entries into Appendix II or `Campaign_Play.md`); those fall back to the
same underscore convention ArkhamDB uses for the ids it does supply. Their
filenames therefore coincide with anchor ids that live *inside* other files;
links resolve by id, so nothing is ambiguous, but do not assume
`glossary/Experience.md` is where `#Experience` points.

### What the conversion does, and does not, change

The conversion is hand-rolled rather than delegated to a generic
HTML-to-markdown library: the tag set is tiny and fully enumerated, and a
generic converter may reflow text, normalise whitespace, or escape the brackets
in `[intellect]` — any of which quietly breaks the verbatim guarantee that is
the point of vendoring at all.

Preserved exactly:

- **Text.** No word is substituted, dropped, or reordered. Markdown special
  characters are *not* escaped, so `[intellect]` and `[hand_2]` survive intact.
- **Icon tokens** such as `[intellect]`, `[reaction]` and `[free]`, which is the
  same convention the card corpus already uses and is more precise than any
  prose substitute.
- **Provenance parentheticals.** Upstream colour-codes deluxe additions in red
  and FAQ additions in blue, but every coloured element also states its own
  provenance in text — `Alert (added in The Forgotten Age)`, `"The Silver Rule"
  (added in FAQ, section 'Card Ability Interpretation', point 2.20)`. The colour
  carries no information the text does not, so no marker vocabulary of our own
  is invented; inventing one would create a second thing that can drift from the
  first. The fetch aborts if it meets a coloured element that is neither a
  heading nor such a parenthetical.

Changed, mechanically and deliberately:

- **Internal links** are rewritten to point at the local file and heading.
  External links keep their absolute URL.
- **Heading levels** are shifted so each file's own top heading is `#`. Each
  file is a standalone document.
- **Whitespace** is collapsed the way a browser would collapse it, so each
  paragraph is one line and greppable. `<br>` becomes a markdown hard break.
- **Two upstream typos in link targets** are fixed: `#Evade` → `Evade_Action`
  and `#Per Investigator` → `Per_Investigator`, both unambiguous references to
  a neighbouring id. Any *other* dangling anchor aborts the fetch rather than
  being papered over.
- **Malformed markup is repaired** where the intent is unambiguous: Appendix III
  writes `<strong><li>text</strong></li>`, and upstream writes sub-lists and
  `<dd>` notes as siblings of the list item they belong under. Both are
  reassembled into the nesting they clearly mean.

## The PDF

- **File:** `ahc01_rules_reference_web.pdf`
- **Source URL:** <https://images-cdn.fantasyflightgames.com/filer_public/c4/b0/c4b0d66c-d79e-411b-bdb5-b5d8c457d4bc/ahc01_rules_reference_web.pdf>
- **Publisher:** Fantasy Flight Games
- **Pulled:** 2026-05-19

Retained as **the pinned publisher original**, so that any future question
about the fidelity of ArkhamDB's replica can be settled against the publisher's
own artifact. It is not the working source and nobody should be reading it in
normal work: it is the 2016 Core Set edition, and `rules/` is strictly newer
and strictly larger.

Vendored in-repo rather than referenced by URL because Fantasy Flight's
`filer_public` CDN has restructured several times in recent years.

## Refreshing

Rare. ArkhamDB folds new expansions and FAQ revisions into the page as they
land, so a refresh is worth running when a new deluxe expansion or FAQ version
ships, or when a rules question turns out to be unanswerable from `rules/`.

Assume you last thought about this six months ago. In the repo root:

1. **Fetch and convert.**

   ```sh
   python3 scripts/arkhamdb.py fetch-rules
   ```

   Standard library only — there is nothing to install. The fetch converts
   before it writes anything, so a structural change upstream aborts the run
   and leaves the working tree untouched. If it aborts, read the message: it
   names the tag, section, count or coloured element it could not place. That
   is a signal to update `scripts/arkhamdb.py`, not to work around it.

2. **Verify.**

   ```sh
   python3 scripts/arkhamdb.py verify rules
   ```

   This re-runs the conversion against the committed `raw/rules.html` and
   compares byte-for-byte with what is on disk, then checks that every internal
   link resolves, that no HTML leaked into the markdown, and that every anchor
   on the page has a file. Nothing here runs in CI — this step is the whole
   safety net, so do not skip it.

3. **Update this file:** the **Fetched** date and the **Raw HTML** sha256, both
   printed by step 1.

4. **Review the diff.** It is readable; read it. Pay particular attention to
   entries whose text changed rather than merely moved, and spot-check a few
   against `raw/rules.html`.

To re-run the conversion after changing `scripts/arkhamdb.py` without touching
the network:

```sh
python3 scripts/arkhamdb.py fetch-rules --offline
```

That is what makes the committed raw HTML worth its size: a converter change
can be re-run and diffed offline, and the conversion is verifiable without
trusting the converter.

## Attribution

The rules text is Fantasy Flight Games'. The page it was ingested from — the
replica, the collation of deluxe and FAQ additions, and the anchor scheme these
filenames follow — is **ArkhamDB's** work, at <https://arkhamdb.com/rules>.
Vendored here for offline, verbatim citation in a friends-scale hobby project;
see [`docs/product-decisions.md`](../../docs/product-decisions.md) for the
standing posture on third-party content.

## What's NOT here

- **Per-card rulings.** Those live in [`data/arkhamdb-faq/`](../arkhamdb-faq/),
  one file per card that has any.
- **Designer commentary, ArkhamDB rules-discussion threads, fan wikis.** Useful
  context, but secondary to the Rules Reference itself, and they lag.
- **Translations.** English only, consistent with the card snapshot.
