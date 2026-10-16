//! Deck-level card lists: the **weakness pool** a random basic weakness is
//! drawn from. The starter decklists join it in #782.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, WeaknessKind};
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
            std::iter::repeat_n(CardCode::new(&meta.code), usize::from(quantity))
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
        // TODO(#781): the basic weaknesses #781 implements. Delete this list
        // when it lands; every pool card must then pass.
        const NOT_YET_IMPLEMENTED: [&str; 7] = [
            "01096", "01097", "01098", "01099", "01100", "01101", "01103",
        ];
        let unplayable: Vec<&str> = weakness_pool()
            .iter()
            .map(CardCode::as_str)
            .filter(|code| !crate::is_playable(code) && !NOT_YET_IMPLEMENTED.contains(code))
            .collect();
        assert!(
            unplayable.is_empty(),
            "pool cards without an implementation: {unplayable:?}"
        );
    }
}
