# Arkham Horror LCG Learn to Play (2016 Core Set)

Fantasy Flight's *Learn to Play* booklet from the 2016 Core Set, pinned as
published. It is vendored for **one thing only**: the *"Investigator Starter
Decks"* sidebar on p. 4, which the default decks in `crates/cards/src/decks.rs`
are inspired by. Read the verbatim transcription in
[`starter_decks.md`](starter_decks.md), not the PDF.

Its rules are **not** a source. The Rules Reference supersedes the Learn to
Play booklet, and the current rules text is
[`../rules-reference/rules/`](../rules-reference/rules/), with the Official FAQ
in [`../official-faq/`](../official-faq/) winning on conflict. See the
project-level directive in [`CLAUDE.md`](../../CLAUDE.md).

## Source

- **File:** `ahc01_learn_to_play_web.pdf`
- **Edition:** 2016 Core Set (PDF created 2016-10-10)
- **Source URL:**
  <https://images-cdn.fantasyflightgames.com/filer_public/88/53/88538d11-5274-4b4a-ac8c-e8d758f71132/ahc01_learn_to_play_web.pdf>
- **Found through:** the Wayback Machine's CDX index, whose first capture of
  that URL is 2016-10-24.
- **Publisher:** Fantasy Flight Games
- **Pulled:** 2026-10-16
- **PDF sha256:** `25acf96fc558959aaf03eeeb5ec24b5641a9a7e0d91584b5e2099a76fbda7582`

Unlike the card snapshot there is no upstream commit to pin. Provenance is the
URL, the pull date, and the hash above.

## Why a transcription

The sidebar sets two decklists side by side, and the PDF's text layer
interleaves the two columns line by line (`pdftotext` emits *"• Roland's .38
Special (6) • Wendy's Amulet (14) • Cover Up (7) …"*), so a plain extraction
reads as one list that belongs to neither investigator. `starter_decks.md` was
transcribed by hand from the rendered page, one column per deck, and is what
a reviewer checks a deck comment against.

## Attribution

The text is Fantasy Flight Games'. Vendored here for offline, verbatim
citation in a friends-scale hobby project; see
[`docs/product-decisions.md`](../../docs/product-decisions.md) for the standing
posture on third-party content.
