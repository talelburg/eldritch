//! The game's symbols as glyphs of the vendored `arkhamicons` font.
//!
//! [`GLYPHS`] maps a card-markup token to its codepoint in the font and its
//! readable name, and [`view`] renders one as a span. The font file, its
//! upstream commit and the config the codepoints were checked against are in
//! `crates/web/assets/fonts/SOURCE.md`; `style.css` loads it with `@font-face`.

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
    glyph("willpower", 'f', "willpower"),
    glyph("intellect", 'g', "intellect"),
    glyph("combat", 'h', "combat"),
    glyph("agility", 'i', "agility"),
    glyph("wild", 'j', "wild"),
    glyph("action", 't', "action"),
    glyph("reaction", 'u', "reaction"),
    glyph("fast", 'v', "free triggered ability"),
    glyph("free", 'v', "free triggered ability"),
    glyph("elder_sign", 'k', "elder sign"),
    glyph("skull", 'm', "skull"),
    glyph("cultist", 'n', "cultist"),
    glyph("tablet", 'o', "tablet"),
    glyph("elder_thing", 'p', "elder thing"),
    glyph("auto_fail", 'q', "auto-fail"),
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

/// A glyph as a `glyph glyph--<token>` span drawing its codepoint, titled and
/// labelled with its readable name.
pub fn view(glyph: &'static Glyph) -> impl IntoView {
    let class = format!("glyph glyph--{}", glyph.token);
    let name = glyph.name;
    view! {
        <span class=class title=name aria-label=name>{glyph.codepoint.to_string()}</span>
    }
}
