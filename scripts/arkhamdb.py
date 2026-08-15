#!/usr/bin/env python3
"""Vendor ArkhamDB's rules reference and card FAQ into the repo as verbatim markdown.

Three subcommands:

    fetch-rules [--offline]   fetch (or re-convert) the rules reference
    fetch-faq   [--refresh]   crawl the per-card FAQ for every code in the snapshot
    verify      [rules|faq|all]

Standard library only, by design: no virtualenv, no lockfile, no install step
between refreshes. Nothing here runs in CI.

The refresh procedure lives in ``data/rules-reference/SOURCE.md``; read that
before running anything in this file.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import posixpath
import re
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from html.parser import HTMLParser
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SNAPSHOT_DIR = REPO_ROOT / "data" / "arkhamdb-snapshot"
RULES_DIR = REPO_ROOT / "data" / "rules-reference"
RAW_RULES = RULES_DIR / "raw" / "rules.html"
RULES_OUT = RULES_DIR / "rules"
FAQ_DIR = REPO_ROOT / "data" / "arkhamdb-faq"
NO_RULINGS = FAQ_DIR / "no-rulings.txt"
REFRESH_STAMP = FAQ_DIR / ".refresh-stamp"

RULES_URL = "https://arkhamdb.com/rules"
FAQ_URL = "https://arkhamdb.com/api/public/faq/{code}.json"
USER_AGENT = "eldritch-arkhamdb-ingest/1.0 (+https://github.com/talelburg/eldritch)"

# ArkhamDB throttles. A probe of fifteen rapid requests succeeded eight times
# then timed out six consecutive times; at this pacing a full sweep of the
# snapshot ran clean.
FAQ_DELAY_SECONDS = 0.75
FAQ_TIMEOUT_SECONDS = 30
FAQ_ATTEMPTS = 3


# --------------------------------------------------------------------------
# HTML parsing
# --------------------------------------------------------------------------

# The upstream tag set is small and fully enumerated. Anything outside it
# aborts the fetch rather than being silently dropped.
INLINE_TAGS = {"a", "b", "br", "cite", "em", "i", "span", "strong"}
BLOCK_TAGS = {
    "blockquote",
    "dd",
    "div",
    "h1",
    "h2",
    "h3",
    "li",
    "ol",
    "p",
    "table",
    "td",
    "th",
    "tr",
    "ul",
}
KNOWN_TAGS = INLINE_TAGS | BLOCK_TAGS
VOID_TAGS = {"br"}

# Upstream markup leaves <p> and <li> unclosed in places. Opening one of these
# implicitly closes whatever is listed for it.
AUTO_CLOSE = {
    "p": {"p"},
    "h1": {"p"},
    "h2": {"p"},
    "h3": {"p"},
    "ul": {"p"},
    "ol": {"p"},
    "table": {"p"},
    "blockquote": {"p"},
    "div": {"p"},
    "li": {"p", "li", "dd"},
    "dd": {"p", "li", "dd"},
    "tr": {"p", "td", "th", "tr"},
    "td": {"p", "td", "th"},
    "th": {"p", "td", "th"},
}

HEADING_TAGS = ("h1", "h2", "h3")


class ConversionError(RuntimeError):
    """The upstream page no longer looks like the page we know how to convert."""


@dataclass
class Node:
    tag: str
    attrs: dict[str, str] = field(default_factory=dict)
    children: list = field(default_factory=list)


class RulesHTMLParser(HTMLParser):
    """Builds a small DOM from the rules page's (sloppy) markup."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = Node("#root")
        self.stack = [self.root]

    def handle_starttag(self, tag: str, attrs) -> None:
        if tag not in KNOWN_TAGS:
            raise ConversionError(
                f"unknown tag <{tag}> in the rules body; upstream markup has changed"
            )
        reopen: list[Node] = []
        if tag in BLOCK_TAGS:
            # Upstream writes `<strong><li>text</strong></li>` in Appendix III.
            # An inline element cannot contain a block, so close the open inline
            # elements and re-open them inside the block, which restores the
            # intended `<li><strong>text</strong></li>`.
            while len(self.stack) > 1 and self.stack[-1].tag in INLINE_TAGS:
                inline_node = self.stack.pop()
                if not inline_node.children:
                    self.stack[-1].children.remove(inline_node)
                reopen.insert(0, inline_node)
        closable = AUTO_CLOSE.get(tag, set())
        while len(self.stack) > 1 and self.stack[-1].tag in closable:
            self.stack.pop()
        node = Node(tag, {k: (v or "") for k, v in attrs})
        self.stack[-1].children.append(node)
        if tag in VOID_TAGS:
            return
        self.stack.append(node)
        for original in reopen:
            clone = Node(original.tag, dict(original.attrs))
            self.stack[-1].children.append(clone)
            self.stack.append(clone)

    def handle_startendtag(self, tag: str, attrs) -> None:
        self.handle_starttag(tag, attrs)

    def handle_endtag(self, tag: str) -> None:
        if tag not in KNOWN_TAGS:
            raise ConversionError(
                f"unknown closing tag </{tag}> in the rules body; upstream markup has changed"
            )
        for depth in range(len(self.stack) - 1, 0, -1):
            if self.stack[depth].tag == tag:
                del self.stack[depth:]
                return
        # A close with no opener (upstream sloppiness); ignore it.

    def handle_data(self, data: str) -> None:
        self.stack[-1].children.append(data)


def extract_body(html: str) -> str:
    """Slice out the rules content column, `<div class="col-md-8" id="rules">`."""
    marker = '<div class="col-md-8" id="rules">'
    start = html.find(marker)
    if start == -1:
        raise ConversionError(
            "could not find the rules content column; upstream page structure has changed"
        )
    depth = 0
    for match in re.finditer(r"<div\b|</div>", html[start:]):
        depth += -1 if match.group(0) == "</div>" else 1
        if depth == 0:
            return html[start : start + match.end()]
    raise ConversionError("rules content column is unterminated")


# --------------------------------------------------------------------------
# Rendering
# --------------------------------------------------------------------------

# Two anchors on the upstream page point at ids that do not exist. Both are
# unambiguous typos for a neighbouring id. Any *other* dangling anchor aborts
# the fetch rather than being papered over here.
ANCHOR_FIXUPS = {
    "Evade": "Evade_Action",
    "Per Investigator": "Per_Investigator",
}


def collapse(text: str) -> str:
    return re.sub(r"\s+", " ", text)


def slugify(heading: str) -> str:
    """GitHub's heading-anchor algorithm, applied to the rendered heading text."""
    slug = heading.strip().lower()
    slug = re.sub(r"[^\w\s-]", "", slug, flags=re.UNICODE)
    return re.sub(r"\s+", "-", slug)


def filename_for(node: Node, heading_text: str) -> str:
    """Filenames are ArkhamDB anchor ids verbatim, so `#Anchor_Id` maps onto a
    filename with no translation step. Six glossary entries carry no id
    upstream; those fall back to the same underscore convention ArkhamDB uses
    for the ids it does supply."""
    anchor = node.attrs.get("id", "").strip()
    if anchor:
        return f"{anchor}.md"
    return re.sub(r"[^\w-]", "", heading_text.strip().replace(" ", "_")) + ".md"


@dataclass
class OutputFile:
    """One markdown file: a slice of the document, rendered standalone."""

    path: str  # relative to RULES_OUT, posix
    base_level: int
    nodes: list = field(default_factory=list)
    top_heading: str = ""


@dataclass
class AnchorTarget:
    path: str
    slug: str
    is_top: bool


class Renderer:
    def __init__(self, anchors: dict[str, AnchorTarget], current_path: str, base_level: int):
        self.anchors = anchors
        self.current_path = current_path
        self.base_level = base_level

    # -- inline ----------------------------------------------------------
    def inline(self, node_or_children) -> str:
        children = (
            node_or_children.children if isinstance(node_or_children, Node) else node_or_children
        )
        out = []
        for child in children:
            if isinstance(child, str):
                out.append(collapse(child))
                continue
            tag = child.tag
            if tag == "br":
                out.append("\n")
            elif tag in ("em", "i"):
                inner = self.inline(child).strip()
                out.append(f"*{inner}*" if inner else "")
            elif tag in ("strong", "b"):
                inner = self.inline(child).strip()
                out.append(f"**{inner}**" if inner else "")
            elif tag in ("span", "cite", "div"):
                # `span` carries only colour, whose meaning is already stated in
                # the text it wraps. `cite` appears once, around text that
                # already contains emphasis, so wrapping it again would nest
                # badly. Both render as their contents.
                out.append(self.inline(child))
            elif tag == "a":
                out.append(self.link(child))
            else:
                raise ConversionError(f"<{tag}> in inline position; upstream markup has changed")
        return "".join(out)

    def link(self, node: Node) -> str:
        text = self.inline(node).strip()
        href = node.attrs.get("href", "")
        if not href.startswith("#"):
            return f"[{text}]({href})"
        anchor = ANCHOR_FIXUPS.get(href[1:], href[1:])
        target = self.anchors.get(anchor)
        if target is None:
            raise ConversionError(
                f"internal link #{href[1:]} resolves to no heading; upstream page has changed"
            )
        rel = posixpath.relpath(target.path, posixpath.dirname(self.current_path)) or "."
        dest = rel if target.is_top else f"{rel}#{target.slug}"
        if " " in dest:
            dest = f"<{dest}>"
        return f"[{text}]({dest})"

    # -- blocks ----------------------------------------------------------
    def blocks(self, children) -> list[str]:
        out: list[str] = []
        pending: list = []

        def flush() -> None:
            if pending:
                text = finish_inline(self.inline(pending))
                if text:
                    out.append(text)
                pending.clear()

        for child in children:
            if isinstance(child, str) or (isinstance(child, Node) and child.tag in INLINE_TAGS):
                pending.append(child)
                continue
            flush()
            out.extend(self.block(child))
        flush()
        return out

    def block(self, node: Node) -> list[str]:
        tag = node.tag
        if tag == "p":
            text = finish_inline(self.inline(node))
            return [text] if text else []
        if tag in HEADING_TAGS:
            level = int(tag[1]) - self.base_level + 1
            return ["#" * max(level, 1) + " " + finish_inline(self.inline(node))]
        if tag in ("ul", "ol"):
            return [self.list_block(node)]
        if tag == "blockquote":
            inner = "\n\n".join(self.blocks(node.children))
            return ["\n".join("> " + line if line else ">" for line in inner.split("\n"))]
        if tag == "table":
            return [self.table(node)]
        if tag == "div":
            return self.blocks(node.children)
        raise ConversionError(f"<{tag}> in block position; upstream markup has changed")

    def list_block(self, node: Node, depth: int = 0) -> str:
        ordered = node.tag == "ol"
        counter = int(node.attrs.get("start", "1")) if ordered else 0
        lines: list[str] = []
        # Upstream writes sub-lists and notes as *siblings* of the item they
        # belong under, so continuations are indented to the last marker's width.
        indent = "  "
        for item in node.children:
            if isinstance(item, str):
                if item.strip():
                    raise ConversionError("bare text inside a list; upstream markup has changed")
                continue
            if item.tag in ("ul", "ol"):
                # A sub-list written as a sibling of the item it belongs under.
                if not lines:
                    raise ConversionError("nested list with no list item above it")
                nested = self.list_block(item, depth + 1)
                lines.extend(indent + line if line else "" for line in nested.split("\n"))
                continue
            if item.tag == "dd":
                # Upstream uses <dd> inside <ul> as an indented note hanging off
                # the item above it. Rendered as a continuation paragraph.
                if not lines:
                    raise ConversionError("<dd> with no list item above it")
                note = finish_inline(self.inline(item))
                lines.append("")
                lines.extend(indent + line if line else "" for line in note.split("\n"))
                continue
            if item.tag in INLINE_TAGS:
                # Inline markup between list items. Upstream leaves a couple of
                # these holding nothing but a newline; anything with real text
                # hangs off the item above it, like <dd>.
                note = finish_inline(self.inline(item))
                if not note:
                    continue
                if not lines:
                    raise ConversionError("inline text inside a list with no item above it")
                lines.append("")
                lines.extend(indent + line if line else "" for line in note.split("\n"))
                continue
            if item.tag in ("p", "blockquote", "table"):
                # A block written as a sibling of the item it belongs under.
                if not lines:
                    raise ConversionError(f"<{item.tag}> inside a list with no item above it")
                block = "\n\n".join(self.block(item))
                lines.append("")
                lines.extend(indent + line if line else "" for line in block.split("\n"))
                continue
            if item.tag != "li":
                raise ConversionError(f"<{item.tag}> inside a list; upstream markup has changed")
            marker = f"{counter}. " if ordered else "- "
            if ordered:
                counter += 1
            body = self.list_item(item, depth)
            indent = " " * len(marker)
            first, *rest = body.split("\n")
            lines.append(marker + first)
            lines.extend((indent + line) if line else "" for line in rest)
        return "\n".join(lines)

    def list_item(self, node: Node, depth: int) -> str:
        parts: list[str] = []
        pending: list = []

        def flush() -> None:
            if pending:
                text = finish_inline(self.inline(pending))
                if text:
                    parts.append(text)
                pending.clear()

        for child in node.children:
            if isinstance(child, str) or (isinstance(child, Node) and child.tag in INLINE_TAGS):
                pending.append(child)
                continue
            flush()
            if child.tag in ("ul", "ol"):
                parts.append(self.list_block(child, depth + 1))
            else:
                parts.extend(self.block(child))
        flush()
        return "\n\n".join(parts)

    def table(self, node: Node) -> str:
        rows: list[list[str]] = []
        header: list[str] | None = None
        for row in node.children:
            if isinstance(row, str):
                continue
            if row.tag != "tr":
                raise ConversionError(f"<{row.tag}> inside a table; upstream markup has changed")
            cells: list[str] = []
            is_header = False
            for cell in row.children:
                if isinstance(cell, str):
                    continue
                if cell.tag not in ("td", "th"):
                    raise ConversionError(
                        f"<{cell.tag}> inside a table row; upstream markup has changed"
                    )
                is_header = is_header or cell.tag == "th"
                text = finish_inline(self.inline(cell)).replace("\n", " ").replace("|", r"\|")
                cells.append(text)
                # Markdown has no colspan; the extra columns are padded empty.
                cells.extend([""] * (int(cell.attrs.get("colspan", "1")) - 1))
            if is_header and header is None:
                header = cells
            else:
                rows.append(cells)
        width = max([len(r) for r in rows] + [len(header or [])] + [1])
        if header is None:
            header = [""] * width

        def line(cells: list[str]) -> str:
            padded = cells + [""] * (width - len(cells))
            return "| " + " | ".join(padded) + " |"

        return "\n".join([line(header), line(["---"] * width)] + [line(r) for r in rows])


def finish_inline(text: str) -> str:
    """Trim the whitespace HTML would have collapsed, and turn `<br>` newlines
    into markdown hard breaks."""
    text = re.sub(r" *\n *", "\n", text).strip()
    return re.sub(r"(?<!\n)\n(?!\n)", "  \n", text)


# --------------------------------------------------------------------------
# Splitting the rules document into files
# --------------------------------------------------------------------------

GLOSSARY_ID = "Glossary"
REQUIRED_SECTION_IDS = [
    "Intro",
    "The_Thing_That_Should_Not_Be",
    "Glossary",
    "Appendix_I_Initiation_Sequence",
    "Appendix_II_Timing_and_Gameplay",
    "Appendix_III_Setting_Up_The_Game",
    "Appendix_IV_Card_Anatomy",
]
GLOSSARY_ENTRY_TOLERANCE = (150, 250)


def heading_text(node: Node) -> str:
    """Plain text of a heading, for filenames, slugs and the index."""
    out = []
    for child in node.children:
        if isinstance(child, str):
            out.append(collapse(child))
        else:
            out.append(heading_text(child))
    return "".join(out).strip()


def split_document(root: Node) -> tuple[list[OutputFile], list, dict[str, AnchorTarget]]:
    """Flatten the body and cut it into one file per section and glossary entry."""
    flat: list = []

    def flatten(node: Node) -> None:
        for child in node.children:
            if isinstance(child, Node) and child.tag == "div":
                flatten(child)
            else:
                flat.append(child)

    flatten(root)

    files: list[OutputFile] = []
    glossary_lead: list = []
    current: OutputFile | None = None
    in_glossary = False

    for node in flat:
        if isinstance(node, str):
            if node.strip() and current is not None:
                current.nodes.append(node)
            continue
        if node.tag == "h1":
            anchor = node.attrs.get("id", "")
            if anchor == GLOSSARY_ID:
                in_glossary = True
                current = None
                continue
            in_glossary = False
            current = OutputFile(filename_for(node, heading_text(node)), 1, [node], heading_text(node))
            files.append(current)
            continue
        if node.tag == "h2" and (in_glossary or current is None):
            name = filename_for(node, heading_text(node))
            path = f"glossary/{name}" if in_glossary else name
            current = OutputFile(path, 2, [node], heading_text(node))
            files.append(current)
            continue
        if current is None:
            if in_glossary:
                glossary_lead.append(node)
            continue
        current.nodes.append(node)

    anchors = build_anchor_map(files)
    # The Glossary is a directory rather than a file; its own anchor resolves to
    # the generated index, which is what a reader following `#Glossary` wants.
    anchors[GLOSSARY_ID] = AnchorTarget("README.md", slugify(GLOSSARY_ID), False)
    return files, glossary_lead, anchors


def build_anchor_map(files: list[OutputFile]) -> dict[str, AnchorTarget]:
    anchors: dict[str, AnchorTarget] = {}
    for out in files:
        for index, node in enumerate(out.nodes):
            if not isinstance(node, Node) or node.tag not in HEADING_TAGS:
                continue
            anchor = node.attrs.get("id", "").strip()
            if not anchor:
                continue
            if anchor in anchors:
                raise ConversionError(f"duplicate anchor id {anchor!r}")
            anchors[anchor] = AnchorTarget(out.path, slugify(heading_text(node)), index == 0)
    return anchors


def render_files(
    files: list[OutputFile], glossary_lead: list, anchors: dict[str, AnchorTarget]
) -> dict[str, str]:
    """Render every output file, keyed by path relative to `rules/`."""
    rendered: dict[str, str] = {}
    for out in files:
        renderer = Renderer(anchors, out.path, out.base_level)
        body = "\n\n".join(renderer.blocks(out.nodes))
        rendered[out.path] = body.rstrip() + "\n"
    rendered["README.md"] = render_index(files, glossary_lead, anchors)
    return rendered


def render_index(
    files: list[OutputFile], glossary_lead: list, anchors: dict[str, AnchorTarget]
) -> str:
    sections = [f for f in files if not f.path.startswith("glossary/")]
    entries = [f for f in files if f.path.startswith("glossary/")]
    lead = "\n\n".join(Renderer(anchors, "README.md", 1).blocks(glossary_lead))

    lines = [
        "# Rules Reference — index",
        "",
        "Generated by `scripts/arkhamdb.py fetch-rules`. Do not hand-edit: "
        "`verify` re-runs the conversion and compares byte-for-byte.",
        "",
        "Source and refresh procedure: [`../SOURCE.md`](../SOURCE.md).",
        "",
        "## Sections",
        "",
    ]
    for out in sections:
        lines.append(f"- [{out.top_heading}]({link_target(out.path)})")
    lines += ["", "## Glossary", ""]
    if lead:
        lines += [lead, ""]
    for out in entries:
        lines.append(f"- [{out.top_heading}]({link_target(out.path)})")
    lines.append("")
    return "\n".join(lines)


def link_target(path: str) -> str:
    return f"<{path}>" if " " in path else path


def rules_anchor_map(html: str) -> dict[str, AnchorTarget]:
    parser = RulesHTMLParser()
    parser.feed(extract_body(html))
    return split_document(parser.root)[2]


def convert_rules(html: str) -> dict[str, str]:
    """Raw rules HTML in, complete file set out. Pure and deterministic."""
    body = extract_body(html)
    check_coloured_elements(body)
    parser = RulesHTMLParser()
    parser.feed(body)
    files, glossary_lead, anchors = split_document(parser.root)

    for required in REQUIRED_SECTION_IDS:
        if required not in anchors:
            raise ConversionError(f"top-level section {required!r} is missing from the page")
    entries = [f for f in files if f.path.startswith("glossary/")]
    low, high = GLOSSARY_ENTRY_TOLERANCE
    if not low <= len(entries) <= high:
        raise ConversionError(
            f"glossary has {len(entries)} entries, outside the expected {low}-{high}"
        )
    for anchor in set(re.findall(r'href="#([^"]*)"', body)):
        if ANCHOR_FIXUPS.get(anchor, anchor) not in anchors:
            raise ConversionError(f"internal link #{anchor} resolves to no heading on the page")
    for anchor in set(re.findall(r'\bid="([^"]*)"', body)):
        if anchor not in anchors and anchor != "rules":
            raise ConversionError(f"id {anchor!r} is not on a heading; page structure has changed")

    return render_files(files, glossary_lead, anchors)


def check_coloured_elements(body: str) -> None:
    """Every coloured element must state its own provenance in text, which is
    what lets the conversion drop the colour without losing information. A
    coloured element that does not fit either shape aborts the fetch."""
    for match in re.finditer(r"<(\w+)([^>]*\bstyle\s*=\s*\"[^\"]*color:[^\"]*\"[^>]*)>", body):
        tag = match.group(1)
        if tag in HEADING_TAGS:
            continue
        if tag == "span":
            close = body.find("</span>", match.end())
            text = re.sub("<[^>]+>", "", body[match.end() : close]).strip()
            if text.startswith("(") and text.endswith(")"):
                continue
        raise ConversionError(
            f"coloured <{tag}> element that is neither a heading nor a parenthetical: "
            f"{body[match.start(): match.start() + 160]!r}"
        )


# --------------------------------------------------------------------------
# Snapshot
# --------------------------------------------------------------------------


def snapshot_codes() -> dict[str, tuple[str, str]]:
    """Every card code in the snapshot, mapped to (pack directory, card name)."""
    codes: dict[str, tuple[str, str]] = {}
    for path in sorted((SNAPSHOT_DIR / "pack").glob("*/*.json")):
        for card in json.loads(path.read_text(encoding="utf-8")):
            codes[card["code"]] = (path.parent.name, card.get("name", ""))
    if not codes:
        raise SystemExit("no card codes found in the snapshot")
    return codes


# --------------------------------------------------------------------------
# HTTP
# --------------------------------------------------------------------------


def http_get(url: str, timeout: int = 60) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


class Progress:
    """One live line: position, percentage, per-outcome counts, rate and ETA."""

    def __init__(self, total: int, label: str):
        self.total = total
        self.label = label
        self.start = time.monotonic()
        self.done = 0
        self.counts = {"fetched": 0, "cached": 0, "none": 0, "error": 0}
        self.tty = sys.stderr.isatty()

    def tick(self, outcome: str) -> None:
        self.done += 1
        self.counts[outcome] += 1
        if self.tty or self.done % 100 == 0 or self.done == self.total:
            self.render()

    def render(self) -> None:
        elapsed = time.monotonic() - self.start
        rate = self.done / elapsed if elapsed > 0 else 0.0
        remaining = (self.total - self.done) / rate if rate > 0 else 0.0
        counts = " ".join(f"{k}={v}" for k, v in self.counts.items())
        line = (
            f"{self.label} {self.done}/{self.total} "
            f"({100 * self.done / self.total:5.1f}%) {counts} "
            f"{rate:.2f}/s eta {format_duration(remaining)}"
        )
        end = "\r" if self.tty else "\n"
        sys.stderr.write(line.ljust(100) + end)
        sys.stderr.flush()

    def finish(self) -> None:
        self.render()
        sys.stderr.write("\n" if self.tty else "")
        sys.stderr.flush()


def format_duration(seconds: float) -> str:
    seconds = int(seconds)
    return f"{seconds // 3600:d}h{(seconds % 3600) // 60:02d}m{seconds % 60:02d}s"


# --------------------------------------------------------------------------
# fetch-rules
# --------------------------------------------------------------------------


def cmd_fetch_rules(args: argparse.Namespace) -> int:
    if args.offline:
        if not RAW_RULES.exists():
            raise SystemExit(f"{RAW_RULES} does not exist; run without --offline first")
        html = RAW_RULES.read_text(encoding="utf-8")
        print(f"converting the committed raw HTML ({len(html)} bytes)")
    else:
        print(f"fetching {RULES_URL}")
        raw = http_get(RULES_URL)
        html = raw.decode("utf-8")
        # Convert before writing anything: a structural change aborts the
        # refresh rather than leaving a half-updated directory behind.
        convert_rules(html)
        RAW_RULES.parent.mkdir(parents=True, exist_ok=True)
        RAW_RULES.write_bytes(raw)
        print(f"wrote {RAW_RULES.relative_to(REPO_ROOT)} ({len(raw)} bytes)")

    rendered = convert_rules(html)
    write_rules_tree(rendered)
    print(f"wrote {len(rendered)} files under {RULES_OUT.relative_to(REPO_ROOT)}")
    print(f"raw sha256: {hashlib.sha256(RAW_RULES.read_bytes()).hexdigest()}")
    print(f"fetched:    {datetime.now(timezone.utc).date().isoformat()}")
    return 0


def write_rules_tree(rendered: dict[str, str]) -> None:
    if RULES_OUT.exists():
        for path in sorted(RULES_OUT.rglob("*.md"), reverse=True):
            path.unlink()
    for relative, text in rendered.items():
        path = RULES_OUT / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


# --------------------------------------------------------------------------
# fetch-faq
# --------------------------------------------------------------------------

ICON_SPAN = re.compile(r'<span class="icon-([a-z0-9_]+)"\s*>\s*</span>')


def faq_markdown(text: str) -> str:
    """ArkhamDB's `text` field is already markdown. The only conversion is icon
    spans, which arrive as empty HTML elements and become the same `[token]`
    form the card corpus and the rules text already use."""
    return ICON_SPAN.sub(r"[\1]", text.replace("\r\n", "\n")).strip()


def faq_document(code: str, name: str, entries: list[dict], fetched: str) -> str:
    updated = sorted(entry.get("updated", {}).get("date", "")[:10] for entry in entries)[-1]
    body = "\n\n".join(faq_markdown(entry.get("text", "")) for entry in entries if entry.get("text"))
    return (
        f"# {name} ({code})\n\n"
        f"Rulings last updated on ArkhamDB {updated or 'unknown'}; "
        f"fetched {fetched}. Source: <https://arkhamdb.com/card/{code}>\n\n"
        f"{body}\n"
    )


def read_no_rulings() -> set[str]:
    if not NO_RULINGS.exists():
        return set()
    return {line.strip() for line in NO_RULINGS.read_text(encoding="utf-8").splitlines() if line.strip()}


def write_no_rulings(codes: set[str]) -> None:
    NO_RULINGS.parent.mkdir(parents=True, exist_ok=True)
    NO_RULINGS.write_text("".join(f"{code}\n" for code in sorted(codes)), encoding="utf-8")


def read_stamp() -> tuple[str, set[str]] | None:
    """The refresh run-stamp: sweep start date, plus the codes this sweep has
    already confirmed have no rulings. Cards *with* rulings record their own
    fetch date in their file, so they need no entry here."""
    if not REFRESH_STAMP.exists():
        return None
    lines = REFRESH_STAMP.read_text(encoding="utf-8").splitlines()
    if not lines:
        return None
    return lines[0].strip(), {line.strip() for line in lines[1:] if line.strip()}


def write_stamp(started: str, confirmed: set[str]) -> None:
    REFRESH_STAMP.write_text(
        "".join([f"{started}\n"] + [f"{code}\n" for code in sorted(confirmed)]), encoding="utf-8"
    )


def file_fetch_date(path: Path) -> str:
    match = re.search(r"fetched (\d{4}-\d{2}-\d{2})", path.read_text(encoding="utf-8"))
    return match.group(1) if match else ""


def cmd_fetch_faq(args: argparse.Namespace) -> int:
    codes = snapshot_codes()
    fetched_on = datetime.now(timezone.utc).date().isoformat()
    no_rulings = read_no_rulings()

    if args.refresh:
        stamp = read_stamp()
        started, confirmed_none = stamp if stamp else (fetched_on, set())
        write_stamp(started, confirmed_none)
        print(f"refresh sweep started {started}; {len(confirmed_none)} codes already re-checked")
    else:
        started, confirmed_none = "", set()

    progress = Progress(len(codes), "faq")
    errors: list[str] = []

    for code, (pack, name) in sorted(codes.items()):
        path = FAQ_DIR / pack / f"{code}.md"
        if args.refresh:
            if code in confirmed_none:
                progress.tick("cached")
                continue
            if path.exists() and file_fetch_date(path) >= started:
                progress.tick("cached")
                continue
        else:
            if path.exists() or code in no_rulings:
                progress.tick("cached")
                continue

        entries = fetch_faq_entries(code)
        if entries is None:
            # An errored code leaves no trace on disk, so the next run retries
            # it. Recording it as having no rulings would mislabel it forever.
            errors.append(code)
            progress.tick("error")
            continue

        if entries:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(faq_document(code, name, entries, fetched_on), encoding="utf-8")
            no_rulings.discard(code)
            progress.tick("fetched")
        else:
            # A card that has lost all its rulings loses its file.
            if path.exists():
                path.unlink()
            no_rulings.add(code)
            if args.refresh:
                confirmed_none.add(code)
                write_stamp(started, confirmed_none)
            progress.tick("none")

        write_no_rulings(no_rulings)

    progress.finish()

    if errors:
        print(f"{len(errors)} codes errored and were left untouched; re-run to retry them")
        print("  " + " ".join(errors[:40]) + (" ..." if len(errors) > 40 else ""))
        return 1

    if args.refresh and REFRESH_STAMP.exists():
        REFRESH_STAMP.unlink()
    print("sweep complete; `git status` and `git diff` are the change report")
    return 0


def fetch_faq_entries(code: str) -> list[dict] | None:
    """The card's FAQ entries, `[]` for a card with no rulings, `None` on error."""
    for attempt in range(FAQ_ATTEMPTS):
        try:
            time.sleep(FAQ_DELAY_SECONDS * (1 + attempt * 3))
            payload = http_get(FAQ_URL.format(code=code), timeout=FAQ_TIMEOUT_SECONDS)
            return json.loads(payload.decode("utf-8"))
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return []
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError):
            pass
    return None


# --------------------------------------------------------------------------
# verify
# --------------------------------------------------------------------------

MD_LINK = re.compile(r"\[[^\]]*\]\(<?([^)>]+)>?\)")
HTML_TAG = re.compile(r"</?[a-zA-Z][a-zA-Z0-9]*(?:\s[^<>]*)?/?>")


def verify_rules(problems: list[str]) -> None:
    if not RAW_RULES.exists():
        problems.append(f"{RAW_RULES.relative_to(REPO_ROOT)} is missing")
        return
    html = RAW_RULES.read_text(encoding="utf-8")
    try:
        expected = convert_rules(html)
    except ConversionError as error:
        problems.append(f"re-converting the raw HTML failed: {error}")
        return

    on_disk = {
        str(path.relative_to(RULES_OUT).as_posix()): path.read_text(encoding="utf-8")
        for path in RULES_OUT.rglob("*.md")
    }

    for missing in sorted(set(expected) - set(on_disk)):
        problems.append(f"rules: {missing} is missing from the vendored tree")
    for extra in sorted(set(on_disk) - set(expected)):
        problems.append(f"rules: {extra} is not produced by the conversion")
    for shared in sorted(set(expected) & set(on_disk)):
        if expected[shared] != on_disk[shared]:
            problems.append(f"rules: {shared} does not match a re-run of the conversion")

    for relative, text in sorted(on_disk.items()):
        tag = HTML_TAG.search(text)
        if tag:
            problems.append(f"rules: {relative} contains an HTML tag {tag.group(0)!r}")
        for destination in MD_LINK.findall(text):
            if "://" in destination or destination.startswith("www."):
                continue
            file_part, _, fragment = destination.partition("#")
            if not file_part:
                continue
            target = (RULES_OUT / posixpath.dirname(relative) / file_part).resolve()
            if not target.exists():
                problems.append(f"rules: {relative} links to missing file {file_part}")
                continue
            if fragment:
                headings = {
                    slugify(line.lstrip("# ").strip())
                    for line in target.read_text(encoding="utf-8").splitlines()
                    if line.startswith("#")
                }
                if fragment not in headings:
                    problems.append(f"rules: {relative} links to missing anchor #{fragment}")

    anchors = rules_anchor_map(html)
    for raw_anchor in sorted(set(re.findall(r'\bid="([^"]*)"', extract_body(html))) - {"rules"}):
        target = anchors.get(raw_anchor)
        if target is None:
            problems.append(f"rules: anchor {raw_anchor} has no corresponding heading")
        elif target.path not in on_disk:
            problems.append(f"rules: anchor {raw_anchor} points at missing file {target.path}")


def verify_faq(problems: list[str]) -> None:
    codes = snapshot_codes()
    no_rulings = read_no_rulings()
    seen: set[str] = set()

    for path in sorted(FAQ_DIR.rglob("*.md")):
        code = path.stem
        pack = path.parent.name
        seen.add(code)
        if code not in codes:
            problems.append(f"faq: {path.name} is not a code in the snapshot")
            continue
        if pack != codes[code][0]:
            problems.append(f"faq: {code} sits under {pack}/, snapshot has it under {codes[code][0]}/")
        text = path.read_text(encoding="utf-8")
        if not re.search(r"Rulings last updated on ArkhamDB .*fetched \d{4}-\d{2}-\d{2}", text):
            problems.append(f"faq: {code} has no metadata line")

    for code in sorted(no_rulings & seen):
        problems.append(f"faq: {code} appears both as a file and in no-rulings.txt")
    for code in sorted(no_rulings - set(codes)):
        problems.append(f"faq: no-rulings.txt lists {code}, which is not in the snapshot")

    unaccounted = set(codes) - seen - no_rulings
    if unaccounted:
        problems.append(
            f"faq: {len(unaccounted)} snapshot codes are neither vendored nor listed as "
            f"having no rulings (e.g. {' '.join(sorted(unaccounted)[:5])})"
        )


def cmd_verify(args: argparse.Namespace) -> int:
    problems: list[str] = []
    if args.what in ("rules", "all"):
        verify_rules(problems)
    if args.what in ("faq", "all"):
        verify_faq(problems)
    for problem in problems:
        print(problem)
    if problems:
        print(f"\n{len(problems)} problem(s)")
        return 1
    print("verified")
    return 0


# --------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    rules = sub.add_parser("fetch-rules", help="fetch and convert the rules reference")
    rules.add_argument(
        "--offline",
        action="store_true",
        help="re-convert the committed raw HTML instead of fetching",
    )
    rules.set_defaults(func=cmd_fetch_rules)

    faq = sub.add_parser("fetch-faq", help="crawl the per-card FAQ for the whole snapshot")
    faq.add_argument(
        "--refresh",
        action="store_true",
        help="re-fetch every code rather than resuming an interrupted crawl",
    )
    faq.set_defaults(func=cmd_fetch_faq)

    verify = sub.add_parser("verify", help="check the vendored text against its raw source")
    verify.add_argument("what", nargs="?", default="all", choices=["rules", "faq", "all"])
    verify.set_defaults(func=cmd_verify)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except ConversionError as error:
        print(f"aborted: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
