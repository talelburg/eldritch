# Arkham Horror LCG Official FAQ

Fantasy Flight's *Notes, Errata, and Frequently Asked Questions* — the
publisher's own FAQ document, converted from the pinned PDF into verbatim
markdown. Start at [`README.md`](README.md), which indexes every section and
every question the Q&A asks.

This is the **third** thing we vendor from the game's paper trail, and the
three do not overlap the way their names suggest:

- **Rules text** ([`../rules-reference/`](../rules-reference/)) is ArkhamDB's
  replica of the Rules Reference. It already folds in this document's numbered
  rules sections in full — Game Play 1.1–1.39 and Card Ability Interpretation
  2.1–2.29 — so those are duplicated here rather than new.
- **Card FAQ** ([`../arkhamdb-faq/`](../arkhamdb-faq/)) is community-collated
  per-card rulings, one file per card.
- **Official FAQ** (here) is the publisher's document itself. Its unique
  content is the **Frequently Asked Questions** section — 145 question-and-
  answer pairs that appear in neither of the other two — plus the errata
  sections, the ultimatums and boons, and the Refractions variants.

See the project-level directive in [`CLAUDE.md`](../../CLAUDE.md) for when to
consult this material, how to cite it, and which source wins when two of them
disagree.

## Source

- **File:** `ahc_faq_v25_february_2026-web.pdf`
- **Version:** V.2.5, February 2026 — "The Legacy Edition"
- **Source URL:**
  <https://images-cdn.fantasyflightgames.com/filer_public/c1/d0/c1d0fab6-7fa6-4ce2-af6a-16416381a19b/ahc_faq_v25_february_2026-web.pdf>
- **Publisher:** Fantasy Flight Games
- **Pulled:** 2026-08-21
- **PDF sha256:** `35c82dc070332acd7863eb6d464641ea79e4657b68ff4f1873538e0bcac6b2e7`

Vendored in-repo rather than referenced by URL for the same reason as the
Rules Reference PDF: Fantasy Flight's `filer_public` CDN has restructured
several times, and the URL embeds the version, so it will not survive the next
revision.

Unlike the card snapshot there is no upstream commit to pin. Provenance is the
URL, the pull date, and the hash above.

## What is committed, and why there are two of them

```
ahc_faq_v25_february_2026-web.pdf   the publisher's artifact, pinned
raw/faq.bbox.xml                    the page geometry the converter reads
*.md                                the conversion, one file per section
```

**`raw/faq.bbox.xml` is the converter's actual input**, in the same way
`../rules-reference/raw/rules.html` is for the rules text. It is
`pdftotext -bbox-layout` output: every word on every page with its bounding
box. Committing it is what makes `convert` reproducible — the conversion
re-runs, and `verify` byte-compares, on a machine with no poppler installed at
all. Without it the guarantee would be much weaker than it looks, because
`pdftotext` output is not stable across poppler versions, so a reviewer on a
different poppler would get a different byte stream and never know.

The PDF is kept as the pinned publisher original. Nobody should be reading it
in normal work — that is what the markdown is for — but it is what any future
question about the fidelity of this conversion gets settled against, and it is
what a refresh converts from.

## Layout

One file per top-level section, named for the section:

```
Intro.md                            the title page: version, and what the document is
Twisting_Warping_Changing.md        the epigraph facing the first section
Notes_and_Errata.md                 rulebook, campaign guide, and card errata
Definitions_and_Terms.md
Rulings_and_Clarifications.md       the numbered rules points, 1.x and 2.x
Frequently_Asked_Questions.md       145 Q&A pairs -- the part that is only here
The_List_of_Taboos.md
Ultimatums_and_Boons.md
Refractions.md
Campaign_Product_Icons.md           the document's own icon legend
Standalone_Product_Icons.md
Starter_Deck_Product_Icons.md
README.md                           generated index of sections and questions
```

Deliberately **not** one file per question. The rules reference splits its
glossary per entry because those filenames are upstream anchor ids, so an
existing `#Skill_Test_Timing` reference maps across with no translation. This
document has no ids at all, so per-question files would mean inventing 145
filenames — which is the translation step that naming rule exists to remove.
The largest file is the Q&A at around 70KB, which is readable whole, and
`README.md` covers the one thing a single flat file is better at: seeing what
is in there.

## How the conversion works

`scripts/ffg-faq.py`, standard library only. `convert` reads the committed
geometry; `--pdf` regenerates it and is the only path that needs poppler.

**Reading order is the whole problem.** The document is two columns (three on
the icon-legend page), and `pdftotext` on its own gets the Q&A pages wrong in
the worst possible way: it emits question, question, answer, answer, so every
answer ends up under the wrong question. Nothing about the result *looks*
broken. So the converter works from geometry rather than from poppler's
reading order:

- **Columns** are found by projecting every word onto the x axis and looking
  for a band no word occupies. That handles the two-column body, the
  three-column legend page and the single-column title page without any of
  them being special-cased.
- **Paragraphs** are poppler's blocks, with two repairs. A line containing an
  icon poppler could not map gets split into two blocks at the gap, so blocks
  sharing a baseline are reassembled. And a bullet whose text hangs to the
  right of its marker is emitted as two blocks — first line, then the rest —
  so a paragraph opening mid-sentence is joined to the one before it.
- **Line fragments** are merged on a shared top edge rather than on
  overlapping extents. Leading is tighter than the line box in places, so
  consecutive lines *do* overlap vertically; treating that as "same line"
  interleaved four paragraphs on page 19 word by word.
- **Words broken across a line at a hyphen** are rejoined, and whether the
  hyphen stays is settled by the document's own usage: if the unhyphenated
  spelling appears elsewhere and the hyphenated one never does, the hyphen was
  the typesetter's. Eleven words are broken this way; ten keep their hyphen
  (`Witch-Haunted`, `out-of-play`, `mini-cards`) and one does not
  (`repurchased`, which the document spells unhyphenated three times).
- **Headings** are found by line height, which is quantised by design: body
  text is 11.2–12.6pt, the page number 13.6pt, the running footer 10.5pt, and
  headings 17.0pt and up. The page number and footer are dropped.

## Identifying the icons

The document's icon font maps into the Unicode private-use area, so poppler
carries the glyphs through as codepoints with no meaning attached. There are
21 of them. Each was pinned by rendering the glyph *and* reading it back
against a card whose text the snapshot already carries, so the mapping rests
on evidence rather than on recognising a small black shape:

| Codepoint | Token | How it was settled |
| --- | --- | --- |
| U+F250 | `[willpower]` | "You get –1 ␣ and –1 sanity" is Dreams of R'lyeh 01182, whose snapshot text reads `-1 [willpower] and -1 sanity` |
| U+F251 | `[agility]` | "base ␣ value of 5" is Hope 06031, `base [agility] value` |
| U+F252 | `[intellect]` | "performing an investigate action … she must perform an ␣ test" |
| U+F253 | `[combat]` | ".45 Automatic 03190 … +2 ␣ and +2 damage", whose text reads `+2 [combat]` |
| U+F254–F258 | `[rogue]` `[survivor]` `[guardian]` `[mystic]` `[seeker]` | The deckbuilding errata list each investigator's *other* four classes; Zoey Samaras 02001 is `guardian` and Rex Murphy 02002 is `seeker`, and only the first icon differs between their two lines |
| U+F259 | `[action]` | ".45 Automatic's ␣ ability", which is `[action] Spend 1 ammo` |
| U+F25A | `[fast]` | "␣ Discard Stray Cat: Automatically evade", which is `[fast]` on 01076 |
| U+F25B–F260 | `[skull]` `[cultist]` `[auto_fail]` `[elder_thing]` `[elder_sign]` `[tablet]` | Enumerated together in one line on page 26 and read off in order; `[auto_fail]` confirmed separately by Lucky Dice 02230, `non-[auto_fail] chaos token, spend 2 resources` |
| U+F263 | `[per_investigator]` | "clue threshold should be 2␣", and "1␣ kindling" against Fate of the Vale 10659 |
| U+F26D | `[reaction]` | Rendered as the reaction arrow, and Lucky Dice 02230's errata'd ability is `[reaction]` |
| U+F26E, U+F26F | `[bless]`, `[curse]` | "Do ␣ and ␣ tokens have a modifier" |

The vocabulary is the one the card corpus and the rules text already use.
**An unmapped private-use codepoint aborts the conversion** rather than being
dropped, so a font change in a later revision is loud.

## Naming the product symbols

Every card reference in the document is `<name> (<product symbol> <number>)`,
and the product symbols are a harder case than the icons: they carry **no
Unicode mapping at all**, so poppler drops them silently and the page's
`George Barnaby (🜲 17)` arrives as `George Barnaby ( 17)` — a citation with a
hole in it. The symbol is not merely unnamed, it is *absent*; nothing in the
text layer says which one was printed.

What is left is the gap, and the gap is unambiguous: inter-word spacing in
this document is sharply bimodal, 1.71–1.73pt for a real space against
9.9–13.5pt where a glyph was dropped. There are 439 such sites. Each is named
from the words around it:

1. **From the card snapshot**, for the ~400 references that carry a number.
   The number the document prints is the card's position within its cycle,
   which is what the snapshot stores, so name plus number identifies the card.
   The name is matched with a similarity test rather than an equality test,
   because the document spells names its own way — "Ursula Down" for Ursula
   Downs, "Hyberborea" for Hyperborea. Where a name and number land in more
   than one cycle, the earliest is taken, on the document's own authority:
   errata apply to *"the original English product printing"*.
2. **From the scenario named**, for the 33 Campaign Guide Errata entries that
   have no number at all — `(v1.1) Blood on the Altar, resolutions section (␣)`.
3. **From a hand-read table** (`REFERENCE_OVERRIDES`), for the 27 the first
   two cannot settle: act and agenda cards the snapshot does not carry at that
   number, "Parallel Agnes" where the snapshot says Agnes Baker, and a handful
   where the number sits outside the bracket the symbol is in. Every entry was
   read off the rendered page against the document's own Product Icons legend.

**An unresolvable symbol aborts the conversion.** The failure names the
reference and says where to look, because a silently unnamed product is a
citation that has quietly lost the thing it cites.

The token is the **product** the symbol names, at the granularity the legend
itself uses: each standalone, each Return To box, each starter deck, and
separately the investigator and campaign halves of every chapter-2 expansion
carry their own symbol, so those are named by pack code (`[blbe]`, `[rttfa]`,
`[jac]`, `[tskc]`). The Core Set and the seven chapter-1 cycles print one
shared symbol across every pack in them, as do all Parallel products, so those
are named by cycle code (`[core]`, `[dwl]`, `[parallel]`).

The lookup doubles as a consistency check between this document and the card
snapshot: a reference that stops resolving after a snapshot refresh is telling
you something.

## What is preserved, and what is not

Preserved:

- **Every word.** Verified by comparing the word multiset of the conversion
  against a plain `pdftotext` extraction of the same PDF: 30,267 words, with
  no word lost and none duplicated. The only differences are the eleven
  line-break hyphens, where the plain extraction is the one that is wrong.
- **Icons**, as `[token]`s, including in the legend lists whose whole purpose
  is to say which symbol means which product.
- **Bullets and sub-bullets**, which arrive as `Æ` and `=` because the ornament
  font is read through WinAnsi, and become markdown list markers.

Not preserved, deliberately:

- **The red marking on new content.** The document colour-codes what changed
  since the previous version, and colour is not in the text layer at all —
  recovering it would mean a Python PDF library, which nothing else in this
  repo needs. It buys nothing here: only one version is vendored and it is
  authoritative, so "what changed" is not a question asked of this data. The
  rules text made the same call for the same reason.
- **Bold and italic.** Also absent from the geometry. The cost is visible in
  the sections that print a run-in heading in bold — an ultimatum's name, a
  numbered rule's title — which merge into the paragraph that follows them:
  *"Ultimatum of Agony When assigning damage or horror…"*. The words are all
  there and in the right order; only the emphasis that set them apart is gone.

## Verification

```sh
python3 scripts/ffg-faq.py verify
```

Re-runs the conversion from the committed geometry and byte-compares it with
what is on disk, then checks that no glyph sentinel and no private-use
codepoint survived into the output.

That proves the converter is **deterministic**, which is not the same as
correct — a converter that paired every answer with the wrong question would
pass it perfectly. So the conversion itself asserts the invariants a
mis-ordered read would break, and aborts rather than writing:

- **Every question is followed by its answer** before the next question, across
  all 145 pairs. This is the check that catches the failure poppler's own
  reading order actually has.
- **The numbered rules points do not go backwards**, which covers the rules
  sections the way the Q&A alternation covers the Q&A. It allows a repeat
  rather than requiring a strict increase, because the document numbers two
  different points 2.29 — its own error, not something to paper over here.
- **Every dropped product symbol is named**, and every private-use codepoint is
  mapped.

Between them, every page of the document has an ordering guarantee. Nothing
here runs in CI; this is the whole safety net.

## Refreshing

Rare — a new FAQ version ships roughly yearly, and there is no stable URL to
poll, because the version is in the filename. Assume you last thought about
this a year ago.

1. **Find the new PDF** on Fantasy Flight's Arkham Horror LCG support page and
   download it into this directory.

2. **Convert.**

   ```sh
   python3 scripts/ffg-faq.py convert --pdf data/official-faq/<new file>.pdf
   ```

   This regenerates `raw/faq.bbox.xml` and rewrites the markdown. It needs
   `pdftotext` (poppler-utils) on PATH; the run prints the PDF's sha256.

   A revision that moves things will abort rather than write: an unmapped
   icon, an unnamed product symbol, a question with no answer, a rules point
   out of sequence. Read the message — it names what it could not place. That
   is a signal to update `scripts/ffg-faq.py`, not to work around it.

3. **Verify.**

   ```sh
   python3 scripts/ffg-faq.py verify
   ```

4. **Update this file:** the filename, version, URL, pull date and sha256.

5. **Review the diff, and delete the superseded PDF.** Only one version is
   vendored, on purpose; the diff against the previous conversion is the
   change report, and it is what makes dropping the red change-marking free.

To re-run the conversion after changing `scripts/ffg-faq.py` without a PDF or
poppler at all, drop the flag:

```sh
python3 scripts/ffg-faq.py convert
```

That is what makes the committed geometry worth its size: a converter change
can be re-run and diffed offline, and the conversion is verifiable without
trusting the converter.

## Attribution

The text is Fantasy Flight Games'. Vendored here for offline, verbatim
citation in a friends-scale hobby project; see
[`docs/product-decisions.md`](../../docs/product-decisions.md) for the standing
posture on third-party content.

## What's NOT here

- **The Rules Reference.** [`../rules-reference/`](../rules-reference/) — and
  note that it already contains this document's numbered rules sections.
- **Per-card rulings.** [`../arkhamdb-faq/`](../arkhamdb-faq/), one file per
  card that has any.
- **The taboo list as data.** The list is reproduced here as the document
  prints it, but the machine-readable version is
  `../arkhamdb-snapshot/taboos.json`, which is what anything should be built
  against.
