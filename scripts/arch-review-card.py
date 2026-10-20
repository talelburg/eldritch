#!/usr/bin/env python3
"""Print one card of an architecture-review report, as plain text.

The reports from ``/improve-codebase-architecture`` live as HTML on the orphan
branch ``architecture-reviews`` (see ``docs/agents/issue-tracker.md``, "HTML
reports"). Each candidate is an ``<article id="c4">`` (or ``c04``). A grill
reads one card at a time, and each one used to build its own regex to cut it
out of the page.

    scripts/arch-review-card.py                      # list the reports
    scripts/arch-review-card.py 2026-10-18           # list a report's cards
    scripts/arch-review-card.py 2026-10-18 c4        # print card c04
    scripts/arch-review-card.py architecture-review-2026-09.html 12

A report is named by its file name or by the date in it. A card id may be
written ``c4``, ``c04`` or ``4``. The branch is fetched first; if the fetch
fails, the last fetched copy is read and a warning goes to stderr.

Standard library only. Nothing here runs in CI.
"""

from __future__ import annotations

import re
import subprocess
import sys
from html.parser import HTMLParser
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BRANCH = "origin/architecture-reviews"

BLOCK = {"p", "div", "h1", "h2", "h3", "h4", "li", "pre", "tr", "ul", "ol", "table", "section"}


def git(*args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(REPO_ROOT), *args], check=True, capture_output=True, text=True
    ).stdout


def reports() -> list[str]:
    names = git("ls-tree", "--name-only", BRANCH).split()
    return sorted(n for n in names if n.startswith("architecture-review-") and n.endswith(".html"))


def resolve_report(name: str) -> str:
    available = reports()
    if name in available:
        return name
    exact = f"architecture-review-{name}.html"
    if exact in available:
        return exact
    sys.exit(f"no report {name!r}; reports: {', '.join(available)}")


class CardExtractor(HTMLParser):
    """Collect each ``<article id="c…">`` as text, keeping block breaks."""

    def __init__(self) -> None:
        super().__init__()
        self.cards: dict[str, list[str]] = {}
        self.titles: dict[str, str] = {}
        self.current: str | None = None
        self.depth = 0
        self.pre = 0
        self.in_h3 = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if self.current is None:
            ident = dict(attrs).get("id") or ""
            if tag == "article" and re.fullmatch(r"c\d+", ident):
                self.current, self.depth = ident, 1
                self.cards[ident] = []
                self.titles[ident] = ""
            return
        if tag == "article":
            self.depth += 1
        if tag == "pre":
            self.pre += 1
        if tag == "h3":
            self.in_h3 = True
        if tag in BLOCK:
            self.cards[self.current].append("\n")
        if tag == "li":
            self.cards[self.current].append("- ")
        if tag == "br":
            self.cards[self.current].append("\n")

    def handle_endtag(self, tag: str) -> None:
        if self.current is None:
            return
        if tag == "article":
            self.depth -= 1
            if self.depth == 0:
                self.current = None
                return
        if tag == "pre":
            self.pre -= 1
        if tag == "h3":
            self.in_h3 = False
        if tag in BLOCK:
            self.cards[self.current].append("\n")
        elif tag in {"span", "td", "th"}:
            self.cards[self.current].append(" ")
            if self.in_h3:
                self.titles[self.current] += " "

    def handle_data(self, data: str) -> None:
        if self.current is None:
            return
        if self.in_h3:
            self.titles[self.current] += data
        self.cards[self.current].append(data if self.pre else re.sub(r"\s+", " ", data))


def text_of(parts: list[str]) -> str:
    lines = [line.rstrip() for line in "".join(parts).splitlines()]
    out: list[str] = []
    for line in lines:
        if line.strip() or (out and out[-1]):
            out.append(line.strip() if not line.startswith(" ") else line)
    return "\n".join(out).strip()


def card_number(ident: str) -> int:
    """The number in a card id: ``c04`` and ``c4`` are both 4."""
    return int(ident.lstrip("c"))


def main(argv: list[str]) -> int:
    if argv[:1] in (["-h"], ["--help"]):
        print(__doc__)
        return 0
    try:
        git("fetch", "--quiet", "origin", "architecture-reviews")
    except subprocess.CalledProcessError:
        print(f"!! fetch failed; reading the last fetched {BRANCH}", file=sys.stderr)
    try:
        available = reports()
    except subprocess.CalledProcessError:
        sys.exit(f"cannot read {BRANCH}; has it ever been fetched?")
    if not argv:
        print("\n".join(available))
        return 0
    report = resolve_report(argv[0])
    parser = CardExtractor()
    parser.feed(git("show", f"{BRANCH}:{report}"))
    if not parser.cards:
        sys.exit(f"{report} has no <article id=\"c…\"> cards")
    if len(argv) == 1:
        for ident in sorted(parser.cards, key=card_number):
            print(f"{ident:4} {re.sub(r'\s+', ' ', parser.titles[ident]).strip()}")
        return 0
    wanted = argv[1].lower().lstrip("c")
    if not wanted.isdigit():
        sys.exit(f"not a card id: {argv[1]!r} (write c4, c04 or 4)")
    match = [i for i in parser.cards if card_number(i) == int(wanted)]
    if not match:
        numbers = [card_number(i) for i in parser.cards]
        sys.exit(f"{report} has no card {argv[1]}; it has c{min(numbers)}–c{max(numbers)}")
    print(f"{report} #{match[0]}\n")
    print(text_of(parser.cards[match[0]]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
