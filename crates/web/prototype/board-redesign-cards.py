"""PROTOTYPE helper for board-redesign.html (#863). Throwaway.

Pulls every card the prototype shows out of the pinned ArkhamDB snapshot,
verbatim, and splices it into the HTML between the CARDS-BEGIN/CARDS-END
markers. Encounter cards are looked up by name, player cards by code.

    python3 -I crates/web/prototype/board-redesign-cards.py \
        data/arkhamdb-snapshot/pack crates/web/prototype/board-redesign.html
"""

import json
import pathlib
import sys

# Player cards, by code. Roland's hand is from the Learn to Play starter deck
# (data/learn-to-play/starter_decks.md); Agnes's and Daisy's from their default
# decks in crates/cards/src/decks.rs, since the book prints no deck for them.
PLAYER = [
    "01001", "01004", "01002",  # Roland Banks, Agnes Baker, Daisy Walker
    # Roland: in play, threat area, hand
    "01087", "01007",
    "01006", "01016", "01017", "01018", "01022", "01023", "01030", "01037", "01089",
    # Agnes: in play, hand
    "01012", "01060", "01013", "01072", "01080", "01090",
    # Daisy: in play
    "01008",
    # discard piles: Roland, Agnes, Daisy
    "01088", "01019", "01025", "01067", "01039", "01086",
]

# Encounter cards, by name, from The Gathering's sets in the core encounter file.
ENCOUNTER = [
    "Study", "Hallway", "Attic", "Cellar", "Parlor",
    "The Barrier", "What's Going On?!", "Ghoul Minion",
]

KEEP = [
    "code", "name", "subname", "type_code", "subtype_code", "faction_code", "cost",
    "traits", "text", "back_name", "back_text", "slot", "health", "sanity",
    "skill_willpower", "skill_intellect", "skill_combat", "skill_agility", "skill_wild",
    "shroud", "clues", "clues_fixed", "doom", "stage", "enemy_fight", "enemy_evade",
    "enemy_damage", "enemy_horror", "victory", "is_unique", "encounter_code",
]


def main(pack_dir, html_path):
    pack = pathlib.Path(pack_dir)
    player = json.loads((pack / "core" / "core.json").read_text())
    encounter = json.loads((pack / "core" / "core_encounter.json").read_text())
    by_code = {c["code"]: c for c in player + encounter}
    gathering_sets = {"torch", "rats", "ghouls", "striking_fear", "ancient_evils", "chilling_cold"}

    out = {}
    missing = []
    for code in PLAYER:
        card = by_code.get(code)
        if card is None:
            missing.append(code)
            continue
        out[code] = {k: card[k] for k in KEEP if k in card}
    for name in ENCOUNTER:
        hits = [c for c in encounter if c["name"] == name and c.get("encounter_code") in gathering_sets]
        if len(hits) != 1:
            missing.append(f"{name} ({len(hits)} hits)")
            continue
        card = hits[0]
        out[card["code"]] = {k: card[k] for k in KEEP if k in card}

    html = pathlib.Path(html_path)
    text = html.read_text()
    begin, end = "/*CARDS-BEGIN*/", "/*CARDS-END*/"
    head, rest = text.split(begin, 1)
    _, tail = rest.split(end, 1)
    payload = json.dumps(out, ensure_ascii=False, indent=0, sort_keys=True)
    html.write_text(head + begin + payload + end + tail)
    print(f"{len(out)} cards spliced; missing: {missing or 'none'}")
    for code, card in sorted(out.items()):
        print(code, card["name"])


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
