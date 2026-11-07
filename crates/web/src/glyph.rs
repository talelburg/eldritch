//! The game's symbols as glyphs of the vendored `arkhamicons` font.
//!
//! [`GLYPHS`] maps a card-markup token to its codepoint in the font and its
//! readable name, and [`view()`] renders one as a span. The font file, its
//! upstream commit and the config the codepoints were checked against are in
//! `crates/web/assets/fonts/SOURCE.md`; `style.css` loads it with `@font-face`.

use card_dsl::card_data::SkillKind;
use game_core::state::ChaosToken;
use leptos::prelude::*;

/// One game symbol: the token card markup writes it as (`combat` for
/// `[combat]`), its codepoint in `arkhamicons`, and its readable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    /// The bare markup token, without brackets.
    pub token: &'static str,
    /// The character the `arkhamicons` font draws as this symbol.
    pub codepoint: char,
    /// What a player calls the symbol, used as the span's title and label.
    pub name: &'static str,
}

/// Every symbol the client draws as a glyph. A bracketed token missing from
/// here renders as a red unknown-token.
///
/// `fast` and `free` are one icon: card text writes it `[fast]`, and the rules
/// write it both ways (`GLOSSARY.md` → **Fast**).
pub const GLYPHS: &[Glyph] = &[
    WILLPOWER,
    INTELLECT,
    COMBAT,
    AGILITY,
    WILD,
    glyph("action", 't', "action"),
    glyph("reaction", 'u', "reaction"),
    glyph("fast", 'v', "free triggered ability"),
    FREE,
    ELDER_SIGN,
    SKULL,
    CULTIST,
    TABLET,
    ELDER_THING,
    AUTO_FAIL,
    glyph("per_investigator", 'r', "per investigator"),
    glyph("guardian", ')', "guardian"),
    glyph("seeker", 'b', "seeker"),
    glyph("mystic", 'c', "mystic"),
    glyph("rogue", 'd', "rogue"),
    glyph("survivor", 'e', "survivor"),
    glyph("curse", 'y', "curse"),
    glyph("bless", 'z', "bless"),
    glyph("codex", '\u{e901}', "codex"),
];

// The rows code reaches by name, so [`for_skill`], [`wild`], [`free`] and
// [`for_chaos_token`] reach them without a lookup that could miss.
const WILLPOWER: Glyph = glyph("willpower", 'f', "willpower");
const INTELLECT: Glyph = glyph("intellect", 'g', "intellect");
const COMBAT: Glyph = glyph("combat", 'h', "combat");
const AGILITY: Glyph = glyph("agility", 'i', "agility");
const WILD: Glyph = glyph("wild", 'j', "wild");
const FREE: Glyph = glyph("free", 'v', "free triggered ability");
const ELDER_SIGN: Glyph = glyph("elder_sign", 'k', "elder sign");
const SKULL: Glyph = glyph("skull", 'm', "skull");
const CULTIST: Glyph = glyph("cultist", 'n', "cultist");
const TABLET: Glyph = glyph("tablet", 'o', "tablet");
const ELDER_THING: Glyph = glyph("elder_thing", 'p', "elder thing");
const AUTO_FAIL: Glyph = glyph("auto_fail", 'q', "auto-fail");

/// A [`Glyph`] row, so the table reads one symbol per line.
const fn glyph(token: &'static str, codepoint: char, name: &'static str) -> Glyph {
    Glyph {
        token,
        codepoint,
        name,
    }
}

/// The glyph a markup token names, if the table maps it.
#[must_use]
pub fn for_token(token: &str) -> Option<&'static Glyph> {
    GLYPHS.iter().find(|g| g.token == token)
}

/// The glyph of a skill's icon.
#[must_use]
pub fn for_skill(skill: SkillKind) -> &'static Glyph {
    match skill {
        SkillKind::Willpower => &WILLPOWER,
        SkillKind::Intellect => &INTELLECT,
        SkillKind::Combat => &COMBAT,
        SkillKind::Agility => &AGILITY,
    }
}

/// The glyph of the wild skill icon, which no [`SkillKind`] names.
#[must_use]
pub fn wild() -> &'static Glyph {
    &WILD
}

/// The glyph of the free-triggered-ability icon, which the Rules Reference's
/// timing chart prints on each player window.
#[must_use]
pub fn free() -> &'static Glyph {
    &FREE
}

/// The glyph a chaos token is drawn as, or `None` for a numeric token and for
/// a `ChaosToken` variant this table doesn't map yet.
#[must_use]
pub fn for_chaos_token(token: ChaosToken) -> Option<&'static Glyph> {
    match token {
        ChaosToken::Skull => Some(&SKULL),
        ChaosToken::Cultist => Some(&CULTIST),
        ChaosToken::Tablet => Some(&TABLET),
        ChaosToken::ElderThing => Some(&ELDER_THING),
        ChaosToken::AutoFail => Some(&AUTO_FAIL),
        ChaosToken::ElderSign => Some(&ELDER_SIGN),
        // `ChaosToken` is #[non_exhaustive]: a future variant has no glyph.
        _ => None,
    }
}

/// A glyph as a `glyph glyph--<token>` span drawing its codepoint, titled and
/// labelled with its readable name.
pub fn view(glyph: &'static Glyph) -> impl IntoView {
    let class = format!("glyph glyph--{}", glyph.token);
    let name = glyph.name;
    view! {
        <span class=class title=name aria-label=name>{glyph.codepoint.to_string()}</span>
    }
}
