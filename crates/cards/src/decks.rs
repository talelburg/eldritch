//! Deck-level card lists: the **weakness pool** a random basic weakness is
//! drawn from, and the **default decks** the picker seats an investigator with
//! when the player brings none of their own. See **Default deck** in
//! `GLOSSARY.md`.
//!
//! Every default deck has the structure of the 2016 Learn to Play's
//! *"Investigator Starter Decks"* sidebar (p. 4, transcribed verbatim in
//! `data/learn-to-play/starter_decks.md`): one copy of each of the ten
//! primary-class level-0 titles, one of each of the ten secondary-class
//! level-0 titles, two each of Knife 01086, Flashlight 01087 and Emergency
//! Cache 01088, and two each of two neutral skills — 10 + 10 + 6 + 4 = 30.
//! Every card is level 0, as that page's note requires: *"When building these
//! starter decks, do not include these upgraded versions."*

use std::{iter, sync::OnceLock};

use card_dsl::card_data::{CardKind, CardMetadata, WeaknessKind};
use game_core::action::RosterEntry;
use game_core::state::CardCode;

/// The pack whose basic weaknesses make up the pool.
const POOL_PACK: &str = "core";

/// 01000 "Random Basic Weakness": the deckbuilder's placeholder for a drawn
/// weakness, printed as a `"basicweakness"` treachery with quantity 15. It is
/// not a card a player can be dealt, so the pool leaves it out by code.
const RANDOM_BASIC_WEAKNESS_PLACEHOLDER: &str = "01000";

/// The weakness pool: the codes a random basic weakness is drawn from, one
/// entry per physical copy, so a title printed twice is twice as likely.
///
/// It is every [`WeaknessKind::Basic`] card whose pack is exactly `"core"`,
/// less the 01000 placeholder, repeated by its printed quantity: the ten
/// weaknesses of one core set, the pool the Official FAQ describes
/// (`data/official-faq/Frequently_Asked_Questions.md` lines 59-61). Standing
/// in for the FAQ's *"1 copy of each product they own"* until players have a
/// collection setting. See **Basic weakness / weakness pool** in
/// `GLOSSARY.md`.
///
/// Seating draws from it through
/// [`CardRegistry::weakness_pool`](game_core::card_registry::CardRegistry::weakness_pool).
///
/// # Panics
///
/// On first call, if a pool card is neither an Enemy nor a Treachery, so its
/// kind carries no quantity. No core card does; a later pack's asset or event
/// basic weakness would, and widening the pool to it needs a quantity source
/// first.
pub fn weakness_pool() -> &'static [CardCode] {
    static POOL: OnceLock<Vec<CardCode>> = OnceLock::new();
    POOL.get_or_init(|| build_weakness_pool(crate::all()))
        .as_slice()
}

/// Build the pool from `corpus`. See [`weakness_pool`].
fn build_weakness_pool(corpus: &[CardMetadata]) -> Vec<CardCode> {
    corpus
        .iter()
        .filter(|meta| {
            meta.weakness == Some(WeaknessKind::Basic)
                && meta.pack_code == POOL_PACK
                && meta.code != RANDOM_BASIC_WEAKNESS_PLACEHOLDER
        })
        .flat_map(|meta| {
            let (CardKind::Enemy { quantity, .. } | CardKind::Treachery { quantity, .. }) =
                meta.kind
            else {
                panic!(
                    "basic weakness {} ({}) has no printed quantity on its kind",
                    meta.code, meta.name
                );
            };
            iter::repeat_n(CardCode::new(&meta.code), usize::from(quantity))
        })
        .collect()
}

/// The investigator's deck as the picker seats it: the counted cards and
/// signatures, plus one basic weakness either fixed in the list or drawn at
/// seating. See **Default deck** in `GLOSSARY.md`.
pub struct DefaultDeck {
    /// The investigator card's code.
    pub investigator: &'static str,
    /// One copy of each signature card the investigator's *"Deckbuilding
    /// Requirements"* name, outside the deck size.
    pub signatures: &'static [&'static str],
    /// The 30 cards counted toward the deck size, a second copy written out as
    /// a repeated code.
    pub cards: &'static [&'static str],
    /// The **fixed weakness**: the basic weakness the deck holds in place of
    /// *"1 random basic weakness"*, as the Learn to Play decks do.
    pub weakness: &'static str,
}

/// How a default deck meets its card back's *"1 random basic weakness"*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaknessChoice {
    /// The deck's [fixed weakness](DefaultDeck::weakness), seated as an
    /// ordinary deck card.
    Fixed,
    /// A **random basic weakness**, drawn from the [`weakness_pool`] at
    /// seating, in place of the fixed one.
    Random,
}

impl DefaultDeck {
    /// The one-investigator roster entry seating this deck: its cards, then its
    /// signatures, then the fixed weakness under [`WeaknessChoice::Fixed`], or a
    /// request for one random basic weakness under [`WeaknessChoice::Random`].
    #[must_use]
    pub fn roster_entry(&self, choice: WeaknessChoice) -> RosterEntry {
        let (fixed, random_basic_weaknesses): (&[&str], u8) = match choice {
            WeaknessChoice::Fixed => (&[self.weakness], 0),
            WeaknessChoice::Random => (&[], 1),
        };
        RosterEntry {
            investigator: CardCode::new(self.investigator),
            deck: self
                .cards
                .iter()
                .chain(self.signatures)
                .chain(fixed)
                .map(|code| CardCode::new(*code))
                .collect(),
            random_basic_weaknesses,
        }
    }
}

/// The five default decks, in the picker's order.
pub const DEFAULT_DECKS: &[DefaultDeck] = &[ROLAND, DAISY, SKIDS, AGNES, WENDY];

/// Roland Banks 01001's default deck.
///
/// **Source:** inspired by the 2016 Learn to Play's Investigator Starter Decks,
/// p. 4 (`data/learn-to-play/starter_decks.md`), whose Roland deck it is.
///
/// **Differences:** Silver Twilight Acolyte 01102 is the fixed weakness in
/// place of the book's Paranoia 01097, for a weakness that bites in The
/// Gathering.
///
/// **Skills:** the book's Guts 01089 and Manual Dexterity 01092: agility 2 is
/// his lowest skill, and the book takes willpower over the tied intellect 3.
///
/// **Requirements**, from his card back: *"Deck Size: 30. Deckbuilding
/// Options: Guardian cards (\[guardian\]) level 0-5, Seeker cards (\[seeker\])
/// level 0-2, Neutral cards level 0-5. Deckbuilding Requirements (do not count
/// toward deck size): Roland's .38 Special, Cover Up, 1 random basic
/// weakness."*
pub const ROLAND: DefaultDeck = DefaultDeck {
    investigator: "01001",
    signatures: &[
        "01006", // Roland's .38 Special
        "01007", // Cover Up
    ],
    cards: &[
        "01016", // .45 Automatic
        "01017", // Physical Training
        "01018", // Beat Cop
        "01019", // First Aid
        "01020", // Machete
        "01021", // Guard Dog
        "01022", // Evidence!
        "01023", // Dodge
        "01024", // Dynamite Blast
        "01025", // Vicious Blow
        "01030", // Magnifying Glass
        "01031", // Old Book of Lore
        "01032", // Research Librarian
        "01033", // Dr. Milan Christopher
        "01034", // Hyperawareness
        "01035", // Medical Texts
        "01036", // Mind over Matter
        "01037", // Working a Hunch
        "01038", // Barricade
        "01039", // Deduction
        "01086", "01086", // Knife
        "01087", "01087", // Flashlight
        "01088", "01088", // Emergency Cache
        "01089", "01089", // Guts
        "01092", "01092", // Manual Dexterity
    ],
    weakness: "01102", // Silver Twilight Acolyte
};

/// Daisy Walker 01002's default deck.
///
/// **Source:** inspired by the 2016 Learn to Play's Investigator Starter Decks,
/// p. 4 (`data/learn-to-play/starter_decks.md`), which gives no Daisy deck: this
/// one applies the book's structure to her classes.
///
/// **Differences:** the whole list is ours, with Paranoia 01097 as the fixed
/// weakness.
///
/// **Skills:** Overpower 01091 and Manual Dexterity 01092, for her two lowest
/// skills, combat 2 and agility 2.
///
/// **Requirements**, from her card back: *"Deck Size: 30. Deckbuilding
/// Options: Seeker cards (\[seeker\]) level 0-5, Mystic cards (\[mystic\]) level
/// 0-2, Neutral cards level 0-5. Deckbuilding Requirements (do not count toward
/// deck size): Daisy’s Tote Bag, The Necronomicon (John Dee Translation), 1
/// random basic weakness."*
pub const DAISY: DefaultDeck = DefaultDeck {
    investigator: "01002",
    signatures: &[
        "01008", // Daisy's Tote Bag
        "01009", // The Necronomicon
    ],
    cards: &[
        "01030", // Magnifying Glass
        "01031", // Old Book of Lore
        "01032", // Research Librarian
        "01033", // Dr. Milan Christopher
        "01034", // Hyperawareness
        "01035", // Medical Texts
        "01036", // Mind over Matter
        "01037", // Working a Hunch
        "01038", // Barricade
        "01039", // Deduction
        "01058", // Forbidden Knowledge
        "01059", // Holy Rosary
        "01060", // Shrivelling
        "01061", // Scrying
        "01062", // Arcane Studies
        "01063", // Arcane Initiate
        "01064", // Drawn to the Flame
        "01065", // Ward of Protection
        "01066", // Blinding Light
        "01067", // Fearless
        "01086", "01086", // Knife
        "01087", "01087", // Flashlight
        "01088", "01088", // Emergency Cache
        "01091", "01091", // Overpower
        "01092", "01092", // Manual Dexterity
    ],
    weakness: "01097", // Paranoia
};

/// "Skids" O'Toole 01003's default deck.
///
/// **Source:** inspired by the 2016 Learn to Play's Investigator Starter Decks,
/// p. 4 (`data/learn-to-play/starter_decks.md`), which gives no Skids deck: this
/// one applies the book's structure to his classes.
///
/// **Differences:** the whole list is ours, with Mob Enforcer 01101 as the
/// fixed weakness.
///
/// **Skills:** Guts 01089 for his lowest skill, willpower 2, and Perception
/// 01090 for intellect 3 over Overpower 01091 for the tied combat 3, because
/// his weapons cover combat.
///
/// **Requirements**, from his card back: *"Deck Size: 30. Deckbuilding
/// Options: Rogue cards (\[rogue\]) level 0-5, Guardian cards (\[guardian\]) level
/// 0-2, Neutral cards level 0-5. Deckbuilding Requirements (do not count toward
/// deck size): On the Lam, Hospital Debts, 1 random basic weakness."*
pub const SKIDS: DefaultDeck = DefaultDeck {
    investigator: "01003",
    signatures: &[
        "01010", // On the Lam
        "01011", // Hospital Debts
    ],
    cards: &[
        "01044", // Switchblade
        "01045", // Burglary
        "01046", // Pickpocketing
        "01047", // .41 Derringer
        "01048", // Leo De Luca
        "01049", // Hard Knocks
        "01050", // Elusive
        "01051", // Backstab
        "01052", // Sneak Attack
        "01053", // Opportunist
        "01016", // .45 Automatic
        "01017", // Physical Training
        "01018", // Beat Cop
        "01019", // First Aid
        "01020", // Machete
        "01021", // Guard Dog
        "01022", // Evidence!
        "01023", // Dodge
        "01024", // Dynamite Blast
        "01025", // Vicious Blow
        "01086", "01086", // Knife
        "01087", "01087", // Flashlight
        "01088", "01088", // Emergency Cache
        "01089", "01089", // Guts
        "01090", "01090", // Perception
    ],
    weakness: "01101", // Mob Enforcer
};

/// Agnes Baker 01004's default deck.
///
/// **Source:** inspired by the 2016 Learn to Play's Investigator Starter Decks,
/// p. 4 (`data/learn-to-play/starter_decks.md`), which gives no Agnes deck: this
/// one applies the book's structure to her classes.
///
/// **Differences:** the whole list is ours, with Haunted 01098 as the fixed
/// weakness.
///
/// **Skills:** Perception 01090 and Overpower 01091, for her two lowest
/// skills, intellect 2 and combat 2.
///
/// **Requirements**, from her card back: *"Deck Size: 30. Deckbuilding
/// Options: Mystic cards (\[mystic\]) level 0-5, Survivor cards (\[survivor\])
/// level 0-2, Neutral cards level 0-5. Deckbuilding Requirements (do not count
/// toward deck size): Heirloom of Hyperborea, Dark Memory, 1 random basic
/// weakness."*
pub const AGNES: DefaultDeck = DefaultDeck {
    investigator: "01004",
    signatures: &[
        "01012", // Heirloom of Hyperborea
        "01013", // Dark Memory
    ],
    cards: &[
        "01058", // Forbidden Knowledge
        "01059", // Holy Rosary
        "01060", // Shrivelling
        "01061", // Scrying
        "01062", // Arcane Studies
        "01063", // Arcane Initiate
        "01064", // Drawn to the Flame
        "01065", // Ward of Protection
        "01066", // Blinding Light
        "01067", // Fearless
        "01072", // Leather Coat
        "01073", // Scavenging
        "01074", // Baseball Bat
        "01075", // Rabbit's Foot
        "01076", // Stray Cat
        "01077", // Dig Deep
        "01078", // Cunning Distraction
        "01079", // "Look what I found!"
        "01080", // Lucky!
        "01081", // Survival Instinct
        "01086", "01086", // Knife
        "01087", "01087", // Flashlight
        "01088", "01088", // Emergency Cache
        "01090", "01090", // Perception
        "01091", "01091", // Overpower
    ],
    weakness: "01098", // Haunted
};

/// Wendy Adams 01005's default deck.
///
/// **Source:** inspired by the 2016 Learn to Play's Investigator Starter Decks,
/// p. 4 (`data/learn-to-play/starter_decks.md`), whose Wendy deck it is.
///
/// **Differences:** none; Amnesia 01096 is the book's own weakness.
///
/// **Skills:** the book's Overpower 01091, for her lowest skill, combat 1, and
/// Unexpected Courage 01093, whose two wild icons back any test.
///
/// **Requirements**, from her card back: *"Deck Size: 30. Deckbuilding
/// Options: Survivor cards (\[survivor\]) level 0-5, Rogue cards (\[rogue\]) level
/// 0-2, Neutral cards level 0-5. Deckbuilding Requirements (do not count toward
/// deck size): Wendy's Amulet, Abandoned and Alone, 1 random basic weakness."*
pub const WENDY: DefaultDeck = DefaultDeck {
    investigator: "01005",
    signatures: &[
        "01014", // Wendy's Amulet
        "01015", // Abandoned and Alone
    ],
    cards: &[
        "01072", // Leather Coat
        "01073", // Scavenging
        "01074", // Baseball Bat
        "01075", // Rabbit's Foot
        "01076", // Stray Cat
        "01077", // Dig Deep
        "01078", // Cunning Distraction
        "01079", // "Look what I found!"
        "01080", // Lucky!
        "01081", // Survival Instinct
        "01044", // Switchblade
        "01045", // Burglary
        "01046", // Pickpocketing
        "01047", // .41 Derringer
        "01048", // Leo De Luca
        "01049", // Hard Knocks
        "01050", // Elusive
        "01051", // Backstab
        "01052", // Sneak Attack
        "01053", // Opportunist
        "01086", "01086", // Knife
        "01087", "01087", // Flashlight
        "01088", "01088", // Emergency Cache
        "01091", "01091", // Overpower
        "01093", "01093", // Unexpected Courage
    ],
    weakness: "01096", // Amnesia
};

#[cfg(test)]
mod tests {
    use card_dsl::card_data::Class;

    use super::*;

    /// The pool's codes and their counts, sorted by code.
    fn counts() -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for code in weakness_pool() {
            match counts.iter_mut().find(|(c, _)| c == code.as_str()) {
                Some((_, n)) => *n += 1,
                None => counts.push((code.as_str().to_string(), 1)),
            }
        }
        counts.sort();
        counts
    }

    /// The core set's ten physical basic weaknesses (`pack/core/core.json`'s
    /// `"basicweakness"` cards and their `quantity`): Amnesia 01096 and
    /// Paranoia 01097 two copies each, Haunted 01098, Psychosis 01099,
    /// Hypochondria 01100, Mob Enforcer 01101, Silver Twilight Acolyte 01102
    /// and Stubborn Detective 01103 one each.
    #[test]
    fn the_pool_is_the_core_sets_ten_basic_weaknesses() {
        let expected: Vec<(String, usize)> = [
            ("01096", 2),
            ("01097", 2),
            ("01098", 1),
            ("01099", 1),
            ("01100", 1),
            ("01101", 1),
            ("01102", 1),
            ("01103", 1),
        ]
        .iter()
        .map(|(c, n)| ((*c).to_string(), *n))
        .collect();
        assert_eq!(counts(), expected);
        assert_eq!(weakness_pool().len(), 10);
    }

    /// 01000 "Random Basic Weakness" is the deckbuilder's placeholder, not a
    /// card a player can be dealt.
    #[test]
    fn the_pool_excludes_the_random_basic_weakness_placeholder() {
        assert!(!weakness_pool().iter().any(|c| c.as_str() == "01000"));
    }

    /// Every pool card is implemented, so seating never deals a card that
    /// does nothing. Drawn codes skip the playable gate; this test guards them
    /// instead.
    #[test]
    fn every_pool_card_is_playable() {
        let unplayable: Vec<&str> = weakness_pool()
            .iter()
            .map(CardCode::as_str)
            .filter(|code| !crate::is_playable(code))
            .collect();
        assert!(
            unplayable.is_empty(),
            "pool cards without an implementation: {unplayable:?}"
        );
    }

    /// The classes a deck's 30 counted cards may come from, by investigator:
    /// the primary and secondary class of each card back's *"Deckbuilding
    /// Options"*, plus Neutral. Hand-written, since nothing reads them from
    /// `back_text`. TODO(#783): read them in the deckbuilding validator.
    fn allowed_classes(investigator: &str) -> [Class; 3] {
        match investigator {
            "01001" => [Class::Guardian, Class::Seeker, Class::Neutral],
            "01002" => [Class::Seeker, Class::Mystic, Class::Neutral],
            "01003" => [Class::Rogue, Class::Guardian, Class::Neutral],
            "01004" => [Class::Mystic, Class::Survivor, Class::Neutral],
            "01005" => [Class::Survivor, Class::Rogue, Class::Neutral],
            other => panic!("no default deck expected for {other}"),
        }
    }

    /// A player card's level, read off its kind.
    fn level(meta: &CardMetadata) -> Option<u8> {
        match meta.kind {
            CardKind::Asset { xp, .. }
            | CardKind::Event { xp, .. }
            | CardKind::Skill { xp, .. } => xp,
            _ => None,
        }
    }

    fn meta(code: &str) -> &'static CardMetadata {
        crate::by_code(code).unwrap_or_else(|| panic!("{code} is not in the corpus"))
    }

    /// The picker offers the five core investigators, in card order.
    #[test]
    fn there_is_one_default_deck_per_core_investigator_in_picker_order() {
        let investigators: Vec<&str> = DEFAULT_DECKS.iter().map(|d| d.investigator).collect();
        assert_eq!(investigators, ["01001", "01002", "01003", "01004", "01005"]);
    }

    /// Each deck's 30 counted cards meet its investigator's card back:
    /// *"Deck Size: 30"*, level 0 only, from the classes its *"Deckbuilding
    /// Options"* name, and `glossary/Deckbuilding.md`'s *"may not include more
    /// than 2 copies (by title) of any given player card."*
    #[test]
    fn each_default_deck_counts_thirty_legal_cards() {
        for deck in DEFAULT_DECKS {
            let who = deck.investigator;
            assert_eq!(deck.cards.len(), 30, "{who}: deck size");
            let mut titles: Vec<(&str, usize)> = Vec::new();
            for code in deck.cards {
                let meta = meta(code);
                assert_eq!(level(meta), Some(0), "{who}: {code} is not level 0");
                assert!(meta.weakness.is_none(), "{who}: {code} is a weakness");
                let class = meta.class().expect("a player card has a class");
                assert!(
                    allowed_classes(who).contains(&class),
                    "{who}: {code} is {class:?}"
                );
                match titles.iter_mut().find(|(t, _)| *t == meta.name) {
                    Some((_, n)) => *n += 1,
                    None => titles.push((&meta.name, 1)),
                }
            }
            let over: Vec<_> = titles.iter().filter(|(_, n)| *n > 2).collect();
            assert!(over.is_empty(), "{who}: more than 2 copies of {over:?}");
        }
    }

    /// Each deck carries its investigator's signatures, the cards named on its
    /// card back's *"Deckbuilding Requirements"*: exactly one copy each, per
    /// `glossary/Signature_Cards.md`'s *"If no number is specified, that number
    /// is 1."*
    #[test]
    fn each_default_deck_carries_its_printed_signatures_once() {
        let expected = [
            ("01001", ["01006", "01007"]),
            ("01002", ["01008", "01009"]),
            ("01003", ["01010", "01011"]),
            ("01004", ["01012", "01013"]),
            ("01005", ["01014", "01015"]),
        ];
        for (deck, (investigator, signatures)) in DEFAULT_DECKS.iter().zip(expected) {
            assert_eq!(deck.investigator, investigator);
            assert_eq!(deck.signatures, signatures, "{investigator}: signatures");
            for mode in [WeaknessChoice::Fixed, WeaknessChoice::Random] {
                let entry = deck.roster_entry(mode);
                for code in signatures {
                    let copies = entry.deck.iter().filter(|c| c.as_str() == code).count();
                    assert_eq!(copies, 1, "{investigator} {mode:?}: copies of {code}");
                }
            }
        }
    }

    /// Each fixed weakness is one of the core set's basic weaknesses: a card
    /// the random draw could have dealt.
    #[test]
    fn each_fixed_weakness_is_a_core_basic_weakness() {
        for deck in DEFAULT_DECKS {
            let meta = meta(deck.weakness);
            assert_eq!(
                meta.weakness,
                Some(WeaknessKind::Basic),
                "{}",
                deck.weakness
            );
            assert!(
                weakness_pool().iter().any(|c| c.as_str() == deck.weakness),
                "{} is not in the weakness pool",
                deck.weakness
            );
        }
    }

    /// Fixed mode seats the 30 cards, the two signatures and the fixed
    /// weakness, and asks the engine for no random draw.
    #[test]
    fn a_fixed_roster_entry_seats_the_fixed_weakness_and_draws_none() {
        for deck in DEFAULT_DECKS {
            let entry = deck.roster_entry(WeaknessChoice::Fixed);
            assert_eq!(entry.investigator, CardCode::new(deck.investigator));
            assert_eq!(entry.deck.len(), 30 + 2 + 1, "{}", deck.investigator);
            assert!(entry.deck.contains(&CardCode::new(deck.weakness)));
            assert_eq!(entry.random_basic_weaknesses, 0);
        }
    }

    /// Random mode leaves the fixed weakness out and asks the engine to draw
    /// the card back's *"1 random basic weakness"*.
    #[test]
    fn a_random_roster_entry_leaves_the_fixed_weakness_out_and_draws_one() {
        for deck in DEFAULT_DECKS {
            let entry = deck.roster_entry(WeaknessChoice::Random);
            assert_eq!(entry.deck.len(), 30 + 2, "{}", deck.investigator);
            assert!(!entry.deck.contains(&CardCode::new(deck.weakness)));
            assert_eq!(entry.random_basic_weaknesses, 1);
        }
    }

    /// Seating refuses an unimplemented card, so every card either mode seats
    /// must be playable.
    #[test]
    fn every_card_a_default_deck_seats_is_playable() {
        for deck in DEFAULT_DECKS {
            let unplayable: Vec<CardCode> = deck
                .roster_entry(WeaknessChoice::Fixed)
                .deck
                .into_iter()
                .filter(|code| !crate::is_playable(code.as_str()))
                .collect();
            assert!(
                unplayable.is_empty(),
                "{}: not playable: {unplayable:?}",
                deck.investigator
            );
        }
    }
}
