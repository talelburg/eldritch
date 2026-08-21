#!/usr/bin/env python3
"""Vendor Fantasy Flight's official FAQ document into the repo as verbatim markdown.

Two subcommands:

    convert [--pdf PATH]   convert the committed page geometry into markdown
    verify                 re-convert and byte-compare against what is on disk

Standard library only, by design. ``convert`` reads the committed
``raw/faq.bbox.xml`` and needs nothing else installed; ``--pdf`` regenerates
that file from a PDF and is the only path that shells out to poppler's
``pdftotext``.

The refresh procedure lives in ``data/official-faq/SOURCE.md``; read that
before running anything in this file.
"""

from __future__ import annotations

import argparse
import collections
import glob
import hashlib
import json
import re
import subprocess
import sys
import unicodedata
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from difflib import SequenceMatcher
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FAQ_DIR = REPO_ROOT / "data" / "official-faq"
RAW_XML = FAQ_DIR / "raw" / "faq.bbox.xml"
SNAPSHOT_DIR = REPO_ROOT / "data" / "arkhamdb-snapshot"

XHTML = "{http://www.w3.org/1999/xhtml}"

# (name, product code, cycle release order) for every card, keyed by the number
# the card prints beside its product symbol.
SnapshotCards = dict[int, list[tuple[str, str, int]]]

# Poppler emits every coordinate in PostScript points. The thresholds below are
# in the same unit, and all of them were measured against the v2.5 document
# rather than guessed -- see SOURCE.md, "How the conversion works".

# Running furniture (page number, footer) sits below this; no body text does.
FURNITURE_Y = 745.0
FOOTER = "FREQUENTLY ASKED QUESTIONS"

# Inter-word gaps are sharply bimodal: a real space is 1.71-1.73pt, and a gap
# left by a glyph poppler could not map is 9.9-13.5pt. Headings track wider,
# up to 5.9pt, so the threshold sits between the two populations rather than
# just clear of the heading tracking.
GLYPH_GAP = 8.0

# Column gutters. The body grid is two columns; page 30 uses three.
GUTTER_WIDTH = 9.0

# Two line fragments belong to the same visual line when they share a baseline.
# Leading is tighter than the line box in places -- consecutive lines can
# overlap vertically by more than a point -- so overlap is not the test; a
# common top edge is.
SAME_LINE_Y = 2.0

# Line heights are quantised by design: body text is 11.2-12.6pt, the page
# number 13.6pt and the footer 10.5pt. Headings are 18.6pt (a section) and
# 20.9pt or more (a part). The title page's 17.0pt version line sits between
# the two populations and is body, which is why the section threshold is 17.5
# rather than 17.0.
H1_HEIGHT = 20.0
H2_HEIGHT = 17.5

# How closely the words before a reference must match a card's name. Below 1.0
# because the document misspells names; high enough that two different cards at
# the same number never both clear it.
NAME_MATCH = 0.82
NAME_MATCH_SLACK = 0.02
NAME_SLACK = 4

# How far back an inline product name may sit from its symbol, in characters.
INLINE_WINDOW = 60

# Which product a symbol names, at the granularity the document's own icon
# legend uses. Most products carry their own symbol -- each standalone, each
# Return To box, each starter deck, and separately the investigator and
# campaign halves of every chapter-2 expansion -- so the pack is the unit. The
# cycles below are the exception: every pack in them prints one shared cycle
# symbol, and Parallel products all print the single "Parallel" promo icon.
SHARED_ICON_CYCLES = {
    "core",
    "core_ch2",
    "dwl",
    "ptc",
    "tfa",
    "tcu",
    "tde",
    "tic",
    "parallel",
}

# The sentinel standing in for a glyph poppler dropped, before resolution.
GLYPH = "\ue000"


class ConversionError(RuntimeError):
    """Raised when the document does not look the way the converter expects."""


# --------------------------------------------------------------------------
# Icons
# --------------------------------------------------------------------------

# The document's icon font maps into the private-use area. Every codepoint
# below was pinned by rendering the glyph and reading it back against a card
# whose text the snapshot already carries -- the derivations are recorded in
# SOURCE.md, "Identifying the icons". An unmapped codepoint aborts the run.
PUA_ICONS = {
    "\uf250": "[willpower]",
    "\uf251": "[agility]",
    "\uf252": "[intellect]",
    "\uf253": "[combat]",
    "\uf254": "[rogue]",
    "\uf255": "[survivor]",
    "\uf256": "[guardian]",
    "\uf257": "[mystic]",
    "\uf258": "[seeker]",
    "\uf259": "[action]",
    "\uf25a": "[fast]",
    "\uf25b": "[skull]",
    "\uf25c": "[cultist]",
    "\uf25d": "[auto_fail]",
    "\uf25e": "[elder_thing]",
    "\uf25f": "[elder_sign]",
    "\uf260": "[tablet]",
    "\uf263": "[per_investigator]",
    "\uf26d": "[reaction]",
    "\uf26e": "[bless]",
    "\uf26f": "[curse]",
}

# The bullet and sub-bullet ornaments arrive as these characters because the
# ornament font is read through WinAnsi. They are list markers, not letters.
BULLET = "Æ"
SUB_BULLET = "="

# Product symbols carry no ToUnicode mapping at all, so they leave a gap in the
# text layer with nothing in it -- the converter cannot see *which* symbol was
# printed, only that one was. Campaign Guide Errata entries name the scenario
# they amend, so the symbol is recovered from the entry's own text. Every entry
# below was confirmed by rendering the page and reading the symbol against the
# document's own "Product Icons" legend on page 30.
SCENARIO_PRODUCTS = {
    "Blood on the Altar": "[dwl]",
    "Echoes of the Past": "[ptc]",
    "The Last King": "[ptc]",
    "The Unspeakable Oath": "[ptc]",
    "Interlude I: Lunacy's Reward": "[ptc]",
    "The Depths of Yoth": "[tfa]",
    "The Boundary Beyond": "[tfa]",
    "Interlude V: The Darkness": "[tfa]",
    "Return to Threads of Fate": "[rttfa]",
    "The Wages of Sin": "[tcu]",
    "Interlude IV: Twist of Fate": "[tcu]",
    "The Witching Hour": "[tcu]",
    "Return to Before the Black Throne": "[rttcu]",
    "The Blob That Ate Everything": "[blob]",
    "Red Tide Rising": "[parallel]",
    "Sanguine Shadows": "[tskc]",
    "Dogs of War": "[tskc]",
    "On Thin Ice": "[tskc]",
    "Shades of Suffering": "[tskc]",
    "Congress of the Keys": "[tskc]",
    "The Feast of Hemlock Vale Campaign Setup": "[fhvc]",
    "Written in Rock": "[fhvc]",
    "Hemlock House": "[fhvc]",
    "The Twisted Hollow": "[fhvc]",
    "The Longest Night": "[fhvc]",
    "Prelude: Dawn of the Final Day": "[fhvc]",
    "Prelude: The Final Evening": "[fhvc]",
    "The Apiary": "[tdcc]",
    "The Doom of Arkham": "[tdcc]",
    "Achievement List": "[tdcc]",
}

# References the snapshot cannot settle on its own: a name it does not carry
# (the document's "Parallel Agnes" is the snapshot's Agnes Baker), or a number
# the snapshot gives to a different card entirely. Each was read off the page
# against the Product Icons legend. The numbers are listed so a reference that
# moves in a later revision fails loudly rather than resolving to the old card.
REFERENCE_OVERRIDES: dict[str, tuple[set[int], str]] = {
    "act 1b-a sacrifice made": ({277}, "[dwl]"),
    "act 1b-palace of the old ones": ({329}, "[tcu]"),
    "act 2d-in shadowed talons": ({127}, "[tfa]"),
    "maniac": ({95}, "[ptc]"),
    "the last king scenario reference card": ({61}, "[ptc]"),
    "agenda 1b-the patients": ({160}, "[ptc]"),
    "young psychopath": ({96}, "[ptc]"),
    "mad patient": ({184}, "[ptc]"),
    "act 2b-nucleus of the universe": ({330}, "[tcu]"),
    "hydra (deep in slumber)": ({330}, "[tic]"),
    "agenda 1b-the risen dead": ({21}, "[tskc]"),
    "agenda 1b-bamboozled!": ({46}, "[tskc]"),
    "special agenda-seeing red": ({62}, "[tskc]"),
    "agenda 1b-in a shadow of voidlight": ({67}, "[tskc]"),
    "act 2b-talisman discovered": ({69}, "[tskc]"),
    "agenda 2b-truths untold": ({98}, "[tskc]"),
    "parallel scenario relics of the past": ({71}, "[parallel]"),
    "close the circle's": ({63}, "[eoep]"),
    "parallel agnes's": ({17}, "[parallel]"),
    "grand chamber": ({64}, "[tfa]"),
    "the painted world": ({24}, "[ptc]"),
    'subject 5u-21/"suzi"': ({1}, "[blbe]"),
    "ritual candles": ({5}, "[jac]"),
    "hypnotic gaze": ({14, 23}, "[jac]"),
    "dark prophecy": ({17}, "[jac]"),
    "cyclopean hammer": ({187}, "[eoep]"),
    "agenda 2a": ({242}, "[ptc]"),
    "enraged side": ({202}, "[tdcc]"),
}

# The deckbuilding-environment bullets on page 30 name products and starter
# decks inline. Same story as above: the symbol is unreadable, the words next
# to it are not.
INLINE_PRODUCTS = {
    "core set": "[core]",
    "Nathaniel Cho": "[nat]",
    "Harvey Walters": "[har]",
    "Winifred Habbamock": "[win]",
    # The name breaks across a line, so only the surname reaches the symbol.
    "Habbamock": "[win]",
    "Jacqueline Fine": "[jac]",
    "Stella Clark": "[ste]",
}


# --------------------------------------------------------------------------
# Page geometry
# --------------------------------------------------------------------------


@dataclass
class Word:
    x0: float
    x1: float
    text: str


@dataclass
class Line:
    y0: float
    y1: float
    block: int
    words: list[Word]

    @property
    def x0(self) -> float:
        return self.words[0].x0

    @property
    def x1(self) -> float:
        return self.words[-1].x1

    @property
    def height(self) -> float:
        return self.y1 - self.y0


@dataclass
class Para:
    lines: list[Line]
    page: int

    @property
    def y0(self) -> float:
        return min(line.y0 for line in self.lines)

    @property
    def tallest_line(self) -> float:
        """A paragraph's type size, which is its tallest line's height."""
        return max(line.height for line in self.lines)


def read_pages(xml_path: Path) -> list[list[Line]]:
    """Parse poppler's bbox XML into one list of lines per page."""
    root = ET.parse(xml_path).getroot()
    pages: list[list[Line]] = []
    for page in root.iter(XHTML + "page"):
        lines: list[Line] = []
        for index, block in enumerate(page.iter(XHTML + "block")):
            for line in block.iter(XHTML + "line"):
                words = [
                    Word(float(w.get("xMin")), float(w.get("xMax")), w.text)
                    for w in line
                    if w.tag == XHTML + "word" and w.text
                ]
                if not words:
                    continue
                lines.append(
                    Line(
                        y0=float(line.get("yMin")),
                        y1=float(line.get("yMax")),
                        block=index,
                        words=words,
                    )
                )
        pages.append(lines)
    return pages


def drop_furniture(lines: list[Line], page: int) -> list[Line]:
    """Remove the page number and the running footer, and only those.

    The threshold has 2.3pt of clearance over the lowest body line in the
    document, which is not much to trust silently, so what it drops is checked
    rather than assumed: anything below the line must be the footer or the
    page number.
    """
    kept, dropped = [], []
    for line in lines:
        (dropped if line.y0 >= FURNITURE_Y else kept).append(line)
    for line in dropped:
        text = " ".join(word.text for word in line.words)
        if text != FOOTER and not re.fullmatch(r"\d{1,2}", text):
            raise ConversionError(
                f"page {page}: {text!r} sits below the body text but is "
                "neither the running footer nor a page number"
            )
    return kept


def column_edges(lines: list[Line]) -> list[float]:
    """Find each column's left edge by locating the vertical gutters.

    Works from where ink actually falls rather than from an assumed grid: a
    gutter is a band of x that no word on the page occupies. That handles the
    two-column body, page 30's three columns, and the single-column cover
    without any of them being special-cased.
    """
    if not lines:
        return []
    left = min(line.x0 for line in lines)
    right = max(line.x1 for line in lines)
    occupied = [False] * (int(right - left) + 2)
    for line in lines:
        for word in line.words:
            for bin_ in range(int(word.x0 - left), int(word.x1 - left) + 1):
                occupied[bin_] = True

    edges = [left]
    run = 0
    for offset, ink in enumerate(occupied):
        if ink:
            if run >= GUTTER_WIDTH:
                edges.append(left + offset)
            run = 0
        else:
            run += 1
    return edges


def columns(lines: list[Line]) -> list[list[Line]]:
    """Split a page's lines into columns, each ordered top to bottom."""
    edges = column_edges(lines)
    buckets: list[list[Line]] = [[] for _ in edges]
    for line in lines:
        index = max(i for i, edge in enumerate(edges) if edge <= line.x0 + 0.5)
        buckets[index].append(line)
    for index, bucket in enumerate(buckets):
        bucket.sort(key=lambda line: (line.y0, line.x0))
        if index + 1 < len(edges):
            for line in bucket:
                if line.x1 > edges[index + 1]:
                    raise ConversionError(
                        f"a line runs from column {index} across the gutter at "
                        f"x={edges[index + 1]:.0f}: "
                        f"{' '.join(word.text for word in line.words)[:70]!r}. "
                        "The columns were not split where the page splits them."
                    )
    return [bucket for bucket in buckets if bucket]


def paragraphs(column: list[Line], page: int) -> list[Para]:
    """Group a column's lines into paragraphs.

    Poppler's blocks are paragraphs, with one wrinkle: a line containing an
    icon it could not map is split into two blocks at the gap. Blocks whose
    vertical extents overlap are therefore the same paragraph, reassembled.
    """
    groups: list[list[Line]] = []
    for line in column:
        placed = False
        for group in groups:
            same_block = any(other.block == line.block for other in group)
            same_line = any(abs(line.y0 - other.y0) < SAME_LINE_Y for other in group)
            if same_block or same_line:
                group.append(line)
                placed = True
                break
        if not placed:
            groups.append([line])

    merged: list[Para] = []
    for group in groups:
        merged.append(Para(lines=merge_lines(group), page=page))
    merged.sort(key=lambda para: para.y0)
    return merged


def merge_lines(lines: list[Line]) -> list[Line]:
    """Rejoin line fragments that a dropped icon split across two blocks."""
    out: list[Line] = []
    for line in sorted(lines, key=lambda line: (line.y0, line.x0)):
        if out and abs(line.y0 - out[-1].y0) < SAME_LINE_Y:
            previous = out[-1]
            previous.words = sorted(previous.words + line.words, key=lambda w: w.x0)
            previous.y1 = max(previous.y1, line.y1)
        else:
            out.append(
                Line(y0=line.y0, y1=line.y1, block=line.block, words=list(line.words))
            )
    return out


def line_text(line: Line) -> str:
    """Render one line, marking every gap a dropped glyph left behind."""
    parts = [line.words[0].text]
    for previous, word in zip(line.words, line.words[1:]):
        if word.x0 - previous.x1 >= GLYPH_GAP:
            parts.append(GLYPH)
        parts.append(word.text)
    return " ".join(parts)


def para_text(para: Para, vocabulary: set[str]) -> str:
    """Render a paragraph, rejoining words the page broke across two lines."""
    text = ""
    for line in para.lines:
        rendered = line_text(line)
        if text.endswith("-") and rendered[:1].islower():
            if not keeps_hyphen(text, rendered, vocabulary):
                text = text[:-1]
            text += rendered
        else:
            text = f"{text} {rendered}" if text else rendered
    return text


def keeps_hyphen(before: str, after: str, vocabulary: set[str]) -> bool:
    """Decide whether a hyphen at a line break belongs to the word.

    The document breaks lines at real hyphens -- "Witch-Haunted", "out-of-play"
    -- but also hyphenates a word that is not hyphenated anywhere else. Its own
    usage settles which: if the unhyphenated spelling appears elsewhere in the
    document and the hyphenated one never does, the hyphen was the typesetter's.
    """
    stem = before.split()[-1][:-1]
    tail = after.split()[0]
    plain = inflections(stem + tail)
    hyphenated = inflections(stem + "-" + tail)
    return not (plain & vocabulary and not hyphenated & vocabulary)


def inflections(word: str) -> set[str]:
    """A word and the endings the document might have written it with.

    "repurchased" is broken across a line, and the document spells the word
    unhyphenated elsewhere -- but as "repurchase". Matching only the exact
    form would miss the evidence that is plainly there.
    """
    stem = normalise(word).lower().strip(".,;:!?\"')")
    forms = {stem}
    for ending in ("d", "s", "es", "ed", "ing"):
        forms.add(stem + ending)
        if stem.endswith(ending):
            forms.add(stem[: -len(ending)])
    return forms


def document_vocabulary(pages: list[list[Line]]) -> set[str]:
    """Every word the document spells out on a line of its own accord."""
    words = set()
    for lines in pages:
        for line in lines:
            for word in line.words:
                words.add(normalise(word.text).lower().strip(".,;:!?\"')"))
    return words


# --------------------------------------------------------------------------
# Icon and reference resolution
# --------------------------------------------------------------------------


def snapshot_cards() -> SnapshotCards:
    """Map a card's printed number to the (name, product, cycle order) it names.

    The number the FAQ prints beside a product symbol is the card's position
    within its cycle, which is exactly what the snapshot stores.
    """
    packs = json.loads((SNAPSHOT_DIR / "packs.json").read_text(encoding="utf-8"))
    cycles = json.loads((SNAPSHOT_DIR / "cycles.json").read_text(encoding="utf-8"))
    order = {cycle["code"]: cycle["position"] for cycle in cycles}
    cycle_of = {pack["code"]: pack["cycle_code"] for pack in packs}

    by_position: SnapshotCards = {}
    for path in sorted(glob.glob(str(SNAPSHOT_DIR / "pack" / "*" / "*.json"))):
        for card in json.loads(Path(path).read_text(encoding="utf-8")):
            if "name" not in card or "position" not in card:
                continue
            cycle = cycle_of.get(card["pack_code"])
            if cycle is None or cycle not in order:
                continue
            product = cycle if cycle in SHARED_ICON_CYCLES else card["pack_code"]
            entry = (card["name"], product, order[cycle])
            by_position.setdefault(card["position"], []).append(entry)
    return by_position


def normalise(text: str) -> str:
    """Fold the typographic variation that stops two spellings matching."""
    text = unicodedata.normalize("NFKC", text)
    text = text.replace("\u2019", "'").replace("\u2018", "'")
    text = text.replace("\u201c", '"').replace("\u201d", '"')
    text = text.replace("\u2013", "-").replace("\u2014", "-").replace("\u2011", "-")
    return re.sub(r"\s+", " ", text).strip()


def resolve_icons(text: str, cards: SnapshotCards, where: str) -> str:
    """Turn private-use codepoints into icon tokens and fill the dropped gaps."""
    for char in text:
        if 0xE000 <= ord(char) <= 0xF8FF and char != GLYPH and char not in PUA_ICONS:
            raise ConversionError(
                f"{where}: unmapped private-use codepoint U+{ord(char):04X}. "
                "The icon font has changed; extend PUA_ICONS rather than "
                "dropping the glyph."
            )
    for char, token in PUA_ICONS.items():
        text = text.replace(char, token)
    return resolve_glyph_gaps(mark_wrapped_symbols(text), cards, where)


def mark_wrapped_symbols(text: str) -> str:
    """Mark a dropped symbol that a line break hid from the gap detector.

    A gap is only visible between two words on one line. Where a reference
    wraps -- "(" ending one line, the symbol and number opening the next --
    the symbol has no predecessor to be measured against, and three references
    reached the output with their product silently missing.

    Joining the lines makes it visible again: poppler emits "(39)" as a single
    word when nothing sits between them, so an opening bracket that is its own
    word, followed by a number, is a bracket something was dropped out of.
    """
    return re.sub(r"\(\s+(?=\d)", f"({GLYPH} ", text)


def resolve_glyph_gaps(text: str, cards: SnapshotCards, where: str) -> str:
    """Name each product symbol poppler dropped, from the words around it."""
    while GLYPH in text:
        index = text.index(GLYPH)
        before, after = text[:index], text[index + 1 :]
        token = (
            override_reference(before, after)
            or card_reference(before, after, cards)
            or scenario_reference(before, after)
            or inline_product(before)
        )
        if token is None:
            context = normalise(before[-70:] + " <<HERE>> " + after[:40])
            raise ConversionError(
                f"{where}: a dropped product symbol could not be named.\n"
                f"    {context}\n"
                "Render the page, read the symbol against the document's own "
                "Product Icons legend on page 30, and add the reference to "
                "REFERENCE_OVERRIDES (or to SCENARIO_PRODUCTS if it names a "
                "scenario)."
            )
        text = before + token + after
    # The sentinel is joined to its neighbours like a word, but the page
    # prints the symbol tight against the bracket: "(&#x1f772; 17)", not "( &#x1f772; 17)".
    return re.sub(r"\(\s+(?=\[)", "(", text)


def reference_number(after: str) -> int | None:
    """Read the card number printed after the symbol, if there is one."""
    match = re.match(r"\s*\)?\s*(\d+)", after)
    return int(match.group(1)) if match else None


def override_reference(before: str, after: str) -> str | None:
    """Look the reference up in the hand-read table."""
    number = reference_number(after)
    if number is None:
        return None
    tails = [tail.lower() for tail in reference_tails(before)]
    for name, (numbers, token) in REFERENCE_OVERRIDES.items():
        if number in numbers and any(tail.endswith(name.lower()) for tail in tails):
            return token
    return None


def card_reference(before: str, after: str, cards: SnapshotCards) -> str | None:
    """Resolve ``<name> (<symbol> <number>)`` against the card snapshot.

    The FAQ's own preamble says an entry applies to "the original English
    product printing" of a card, so where a name and number land in more than
    one cycle -- a reprint, a Return To box -- the earliest cycle is the one
    the printed symbol names.
    """
    number = reference_number(after)
    if number is None:
        return None

    tails = reference_tails(before)
    scored = [
        (max(name_similarity(name, tail) for tail in tails), order, product, name)
        for name, product, order in cards.get(number, [])
    ]
    scored = [entry for entry in scored if entry[0] >= NAME_MATCH]
    if not scored:
        return None

    best = max(score for score, _, _, _ in scored)
    closest = [entry for entry in scored if best - entry[0] <= NAME_MATCH_SLACK]
    if len({normalise(entry[3]).lower() for entry in closest}) > 1:
        return None
    return "[" + min(closest, key=lambda entry: entry[1])[2] + "]"


def reference_tails(before: str) -> list[str]:
    """The spellings of the name a reference might be leading up to.

    A reference trails its card name, but not always directly: the name may
    carry a possessive, a subtitle in parentheses, or an earlier number from
    the same reference when one card is printed in two products.
    """
    tail = normalise(before).rstrip("(,/ ")
    tails = [tail]
    while True:
        stripped = re.sub(
            r"\s*(?:\[[a-z_0-9]+\]\s*)?[0-9]+[a-z]?\s*[(,/]*$", "", tails[-1]
        ).rstrip("(,/ ")
        if stripped == tails[-1] or not stripped:
            break
        tails.append(stripped)
    for candidate in list(tails):
        without_subtitle = re.sub(r"\s*\([^()]*\)$", "", candidate)
        if without_subtitle != candidate:
            tails.append(without_subtitle)
    for candidate in list(tails):
        without_nickname = re.sub(r'\s*/\s*"[^"]*"$', "", candidate)
        if without_nickname != candidate:
            tails.append(without_nickname)
    for candidate in list(tails):
        if candidate.endswith("'s"):
            tails.append(candidate[:-2])
    return tails


def name_similarity(name: str, tail: str) -> float:
    """How well a card's name matches the words leading up to a reference.

    Not an equality test: the document spells names its own way -- "Ursula
    Down" for Ursula Downs, "Hyberborea" for Hyperborea -- and a reference
    whose name is merely misspelled is still that card's reference. Compared
    character by character rather than word by word, because the punctuation
    around a name varies as much as the spelling does.
    """
    candidate = normalise(name).lower()
    tail = tail.lower()
    best = 0.0
    # Windows within four characters of the name's own length: enough slack for
    # the punctuation and misspellings above, too little to reach a neighbouring
    # word and score on that instead.
    for length in range(max(1, len(candidate) - NAME_SLACK), len(candidate) + NAME_SLACK + 1):
        if length > len(tail):
            break
        best = max(best, SequenceMatcher(None, candidate, tail[-length:]).ratio())
    return best


def scenario_reference(before: str, after: str) -> str | None:
    """Resolve a Campaign Guide Errata entry from the scenario it names.

    Only the numberless case, which is what a campaign guide entry is. A card
    reference that failed to resolve must not fall through to here: it would
    scan a whole paragraph for any scenario name and answer confidently with
    whichever was mentioned last.
    """
    if reference_number(after) is not None:
        return None
    return nearest_named(before, SCENARIO_PRODUCTS)


def inline_product(before: str) -> str | None:
    """Resolve a product or starter deck named inline in running text.

    Windowed to the words just before the symbol, because these appear in
    lists -- "Nathaniel Cho &#x2423;, Harvey Walters &#x2423;" -- where the name several
    words back is a different product's, not this symbol's.
    """
    return nearest_named(before[-INLINE_WINDOW:], INLINE_PRODUCTS)


def nearest_named(haystack: str, products: dict[str, str]) -> str | None:
    """The product named closest to the end of the text before the symbol."""
    text = normalise(haystack).lower()
    matches = [
        (text.rindex(normalise(name).lower()), token)
        for name, token in products.items()
        if normalise(name).lower() in text
    ]
    return max(matches)[1] if matches else None


# --------------------------------------------------------------------------
# Markdown assembly
# --------------------------------------------------------------------------


@dataclass
class Section:
    title: str
    file: str
    blocks: list[str] = field(default_factory=list)


def new_section(title: str, first: bool) -> Section:
    """Open a section. The document's own title page becomes the intro."""
    return Section(title=title, file="Intro.md" if first else slug(title) + ".md")


def classify(para: Para) -> str:
    if para.tallest_line >= H1_HEIGHT:
        return "h1"
    if para.tallest_line >= H2_HEIGHT:
        return "h2"
    return "body"


def render_body(text: str) -> list[str]:
    """Turn one paragraph of body text into markdown blocks.

    Usually one block out for one in. But where a whole list sits in a single
    block -- the taboo list does -- its markers arrive inline, mid-paragraph,
    and the list has to be cut back apart at them. The two ornaments are safe
    to split on: neither is ever a letter in this document.
    """
    text = re.sub(r"\s+", " ", text).strip()
    blocks = []
    for item in re.split(rf"\s*(?=[{BULLET}{SUB_BULLET}] )", text):
        if item.startswith(BULLET + " "):
            blocks.append("- " + item[len(BULLET) + 1 :])
        elif item.startswith(SUB_BULLET + " "):
            blocks.append("  - " + item[len(SUB_BULLET) + 1 :])
        elif item:
            blocks.append(item)
    return blocks


# Legend entries the snapshot's pack names do not cover: the two promo icons,
# which name no product, and the core set, which the legend names by campaign.
LEGEND_OVERRIDES = {
    "Core Set (Night of the Zealot)": "[core]",
    "Novella": "[novella]",
    "Parallel": "[parallel]",
}

# A legend entry is a product name and nothing else; the sentence introducing
# each list is longer than any product name in it.
LEGEND_ENTRY_LENGTH = 60


def legend_products() -> dict[str, str]:
    """Name every product the icon legend can list, from the snapshot's packs."""
    packs = json.loads((SNAPSHOT_DIR / "packs.json").read_text(encoding="utf-8"))
    products = {}
    for pack in packs:
        cycle = pack["cycle_code"]
        code = cycle if cycle in SHARED_ICON_CYCLES else pack["code"]
        products[pack["name"]] = f"[{code}]"
    return products | LEGEND_OVERRIDES


def legend_entry(
    section: Section, text: str, products: dict[str, str]
) -> list[str]:
    """Restore the icon a Product Icons entry leads with.

    These lists exist to say which symbol means which product, and the symbol
    is the one thing the text layer does not carry: it leads the line, so it
    leaves no gap between words to detect. What identifies it is the entry
    itself -- the product it names.
    """
    if not section.title.endswith("Icons") or len(text) > LEGEND_ENTRY_LENGTH:
        return render_body(text)

    entry = normalise(text)
    scored = [
        (name_similarity(name, entry), token) for name, token in products.items()
    ]
    best = max(score for score, _ in scored)
    token = min(token for score, token in scored if score == best)
    if best < NAME_MATCH:
        raise ConversionError(
            f"{section.title}: no product matches the legend entry {entry!r}. "
            "The legend lists a product the card snapshot does not; add it to "
            "LEGEND_OVERRIDES."
        )
    return [f"{token} {text}"]


def add_body(section: Section, text: str) -> None:
    """Append a paragraph, rejoining one the page layout split in two.

    A bullet whose text hangs to the right of its marker is emitted as two
    blocks -- the first line, then the rest -- so a paragraph that opens
    mid-sentence is a continuation of the one before it rather than a new
    thought.
    """
    if section.blocks and continues(section.blocks[-1], text):
        section.blocks[-1] += " " + text
        return
    section.blocks.append(text)


def continues(previous: str, text: str) -> bool:
    if previous.startswith("#") or not text[:1].islower():
        return False
    return previous.rstrip()[-1:] not in '.!?:"\u201d\u2026'


def slug(title: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9 ]", "", normalise(title))
    return "_".join(cleaned.split())


def convert(xml_path: Path) -> dict[str, str]:
    """Convert the committed page geometry into one markdown file per section."""
    cards = snapshot_cards()
    products = legend_products()
    sections: list[Section] = []

    pages = read_pages(xml_path)
    vocabulary = document_vocabulary(pages)

    for number, lines in enumerate(pages, start=1):
        for column in columns(drop_furniture(lines, number)):
            for para in paragraphs(column, number):
                where = f"page {number}"
                text = resolve_icons(para_text(para, vocabulary), cards, where)
                kind = classify(para)
                if kind == "h1":
                    sections.append(new_section(normalise(text), first=not sections))
                elif not sections:
                    raise ConversionError(
                        f"page {number} opens with body text, before any "
                        f"heading: {normalise(text)[:70]!r}"
                    )
                elif kind == "h2":
                    sections[-1].blocks.append("## " + normalise(text))
                else:
                    for block in legend_entry(sections[-1], text, products):
                        add_body(sections[-1], block)

    check_structure(sections)

    files = {}
    for section in sections:
        body = "\n\n".join(["# " + section.title] + section.blocks)
        files[section.file] = body.rstrip() + "\n"
    files["README.md"] = render_index(sections)
    return files


def render_index(sections: list[Section]) -> str:
    """Index every section, and every question the Q&A section asks."""
    lines = [
        "# Official FAQ",
        "",
        "Fantasy Flight's *Notes, Errata, and Frequently Asked Questions*, "
        "converted from the pinned PDF. See [`SOURCE.md`](SOURCE.md) for "
        "provenance and for how the conversion works.",
        "",
        "## Sections",
        "",
    ]
    for section in sections:
        lines.append(f"- [{section.title}]({section.file})")

    questions = [
        block
        for section in sections
        if section.title == "Frequently Asked Questions"
        for block in section.blocks
        if is_question(block)
    ]
    lines += ["", "## Questions", ""]
    lines += [
        f"- [{question[2:].strip()}](Frequently_Asked_Questions.md)"
        for question in questions
    ]
    return "\n".join(lines).rstrip() + "\n"


def check_numbering(sections: list[Section]) -> None:
    """Assert the numbered rules read in order.

    The Q&A alternation cannot vouch for the rules sections, which are half the
    document. Their numbering can: "Rulings and Clarifications" is two runs of
    numbered points, and a column read out of order puts them out of sequence.
    """
    for section in sections:
        if section.title != "Rulings and Clarifications":
            continue
        seen: dict[int, int] = {}
        for block in section.blocks:
            match = re.match(r"\((\d+)\.(\d+)\)", block.removeprefix("## "))
            if not match:
                continue
            group, point = int(match.group(1)), int(match.group(2))
            # Non-decreasing rather than strictly increasing: the document
            # numbers two different points 2.29, which is its own error and
            # not something to paper over here.
            if point < seen.get(group, 0):
                raise ConversionError(
                    f"rules point {group}.{point} follows "
                    f"{group}.{seen[group]} -- the columns were read out of order"
                )
            seen[group] = point
        if not seen:
            raise ConversionError("no numbered rules points found")


def is_question(block: str) -> bool:
    return bool(re.match(r"Q:", block))


def is_answer(block: str) -> bool:
    # The document writes "A:No." without a space in at least one place, so the
    # colon is the marker, not the space after it.
    return bool(re.match(r"A:", block))


def check_structure(sections: list[Section]) -> None:
    """Assert the invariants that a mis-ordered read would break.

    A byte-comparison proves the converter is deterministic, not that it read
    the columns in the right order -- a converter that paired every answer
    with the wrong question would pass one perfectly. The Q&A's alternation is
    the check that fails loudly when the ordering is wrong.
    """
    faq = [section for section in sections if section.title == "Frequently Asked Questions"]
    if len(faq) != 1:
        raise ConversionError(
            f"expected exactly one Frequently Asked Questions section, found {len(faq)}"
        )

    check_numbering(sections)

    expecting = "Q"
    asked = 0
    for block in faq[0].blocks:
        if is_question(block):
            if expecting != "Q":
                raise ConversionError(
                    "two questions in a row -- the columns were read out of "
                    f"order around: {block[:70]!r}"
                )
            expecting, asked = "A", asked + 1
        elif is_answer(block):
            if expecting != "A":
                raise ConversionError(
                    "an answer with no question before it -- the columns were "
                    f"read out of order around: {block[:70]!r}"
                )
            expecting = "Q"
    if expecting != "Q":
        raise ConversionError("the last question has no answer")
    if asked < 100:
        raise ConversionError(f"only {asked} questions found; expected well over 100")


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------


def extract_geometry(pdf: Path) -> None:
    """Regenerate the committed page geometry from a PDF."""
    try:
        subprocess.run(
            ["pdftotext", "-bbox-layout", str(pdf), str(RAW_XML)],
            check=True,
            capture_output=True,
        )
    except FileNotFoundError:
        raise ConversionError(
            "pdftotext is not on PATH. It ships with poppler-utils, and is "
            "needed only when regenerating raw/faq.bbox.xml from a PDF."
        ) from None
    except subprocess.CalledProcessError as error:
        raise ConversionError(f"pdftotext failed: {error.stderr.decode()}") from None
    print(f"wrote {RAW_XML.relative_to(REPO_ROOT)}")
    print(f"  pdf sha256 {hashlib.sha256(pdf.read_bytes()).hexdigest()}")


def cmd_convert(args: argparse.Namespace) -> int:
    if args.pdf:
        extract_geometry(Path(args.pdf))
    files = convert(RAW_XML)
    for name in sorted(path.name for path in FAQ_DIR.glob("*.md")):
        if name not in files and name != "SOURCE.md":
            (FAQ_DIR / name).unlink()
    for name, body in sorted(files.items()):
        (FAQ_DIR / name).write_text(body, encoding="utf-8")
    print(f"wrote {len(files)} files to {FAQ_DIR.relative_to(REPO_ROOT)}")
    return 0


def letters(text: str) -> list[str]:
    """The document reduced to the characters that carry its content.

    Everything the conversion is allowed to change is discarded: whitespace,
    the icon tokens it inserts, the hyphens it decides about at a line break,
    the list markers it rewrites. What is left is what must survive intact.
    """
    text = re.sub(r"\[[a-z_0-9]+\]", "", text).replace(BULLET, "")
    return sorted(char for char in normalise(text).lower() if char.isalnum())


def check_completeness(files: dict[str, str], pages: list[list[Line]]) -> str | None:
    """Check that the conversion says everything the page geometry says.

    Byte-comparing a re-conversion proves the converter is deterministic, and
    the ordering invariants prove the columns were read in the right order.
    Neither would notice text quietly going missing -- a line dropped as
    furniture, a paragraph lost to a bad column split. This counts every
    character on both sides.
    """
    source = "".join(
        word.text
        for number, lines in enumerate(pages, start=1)
        for line in drop_furniture(lines, number)
        for word in line.words
    )
    converted = "".join(
        body for name, body in files.items() if name != "README.md"
    )
    expected, actual = letters(source), letters(converted)
    if expected == actual:
        return None

    missing = collections.Counter(expected) - collections.Counter(actual)
    extra = collections.Counter(actual) - collections.Counter(expected)
    return (
        f"the conversion has {len(actual)} content characters where the page "
        f"geometry has {len(expected)}"
        + (f"; missing {dict(missing.most_common(5))}" if missing else "")
        + (f"; extra {dict(extra.most_common(5))}" if extra else "")
    )


def cmd_verify(args: argparse.Namespace) -> int:
    problems: list[str] = []
    files = convert(RAW_XML)

    incomplete = check_completeness(files, read_pages(RAW_XML))
    if incomplete:
        problems.append(incomplete)

    for name, body in sorted(files.items()):
        path = FAQ_DIR / name
        if not path.exists():
            problems.append(f"{name}: missing; re-run convert")
        elif path.read_text(encoding="utf-8") != body:
            problems.append(f"{name}: on disk does not match a fresh conversion")

    on_disk = {path.name for path in FAQ_DIR.glob("*.md")} - {"SOURCE.md"}
    for name in sorted(on_disk - set(files)):
        problems.append(f"{name}: not produced by a fresh conversion; stale file")

    for name, body in sorted(files.items()):
        if GLYPH in body:
            problems.append(f"{name}: an unresolved glyph sentinel survived")
        if any(0xE000 <= ord(char) <= 0xF8FF for char in body):
            problems.append(f"{name}: a private-use codepoint survived")

    if problems:
        for problem in problems:
            print(f"FAIL {problem}")
        return 1
    print(f"ok: {len(files)} files match a fresh conversion")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    convert_parser = sub.add_parser("convert", help="convert the committed geometry")
    convert_parser.add_argument(
        "--pdf", help="regenerate raw/faq.bbox.xml from this PDF first"
    )
    convert_parser.set_defaults(func=cmd_convert)

    verify_parser = sub.add_parser("verify", help="re-convert and compare with disk")
    verify_parser.set_defaults(func=cmd_verify)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except ConversionError as error:
        print(f"aborted: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
