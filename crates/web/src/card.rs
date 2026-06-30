//! Visual card rendering for the web client. Pure helpers (`parse_card_text`,
//! field formatters, `card_face`) are native-testable; the `Card` component
//! assembles them into a faithful mini-card rectangle. Display-only (no click
//! handlers) — interactivity is a later slice.

use game_core::card_data::{CardKind, Class, SkillIcons, Slot};
use game_core::state::CardCode;
use leptos::prelude::*;

/// A parsed run of card text. `Symbol`/`Unknown` carry the bare token without
/// brackets; the renderer re-adds brackets for `Unknown` so unmapped tokens are
/// visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    LineBreak,
    Symbol(String),
    Trait(String),
    Bold(String),
    Italic(String),
    Unknown(String),
}

/// `ArkhamDB` game symbols we render as chips. Anything else in `[..]` is left
/// verbatim (with brackets) so it pops out for us to add a mapping.
const KNOWN_SYMBOLS: &[&str] = &[
    "willpower",
    "intellect",
    "combat",
    "agility",
    "wild",
    "action",
    "reaction",
    "fast",
    "free",
    "elder_sign",
    "skull",
    "cultist",
    "tablet",
    "auto_fail",
    "bless",
    "curse",
];

/// Translate `ArkhamDB` card-text markup into renderable segments.
///
/// Handles `\n`, `<b>..</b>`, `<i>..</i>`, `[[Trait]]`, and `[symbol]` tokens.
/// Bold/italic content is taken as literal text (not recursively parsed) — card
/// emphasis runs are short labels. An unterminated marker (no closing bracket /
/// tag) degrades to literal text.
#[must_use]
pub fn parse_card_text(text: &str) -> Vec<TextSegment> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            flush(&mut buf, &mut out);
            out.push(TextSegment::LineBreak);
            i += 1;
        } else if c == '[' && chars.get(i + 1) == Some(&'[') {
            if let Some((inner, next)) = read_until(&chars, i + 2, "]]") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Trait(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if c == '[' {
            if let Some((inner, next)) = read_until(&chars, i + 1, "]") {
                flush(&mut buf, &mut out);
                out.push(symbol_or_unknown(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if starts_with(&chars, i, "<b>") {
            if let Some((inner, next)) = read_until(&chars, i + 3, "</b>") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Bold(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else if starts_with(&chars, i, "<i>") {
            if let Some((inner, next)) = read_until(&chars, i + 3, "</i>") {
                flush(&mut buf, &mut out);
                out.push(TextSegment::Italic(inner));
                i = next;
            } else {
                buf.push(c);
                i += 1;
            }
        } else {
            buf.push(c);
            i += 1;
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// Push the accumulated text buffer as a `Text` segment, if non-empty.
fn flush(buf: &mut String, out: &mut Vec<TextSegment>) {
    if !buf.is_empty() {
        out.push(TextSegment::Text(std::mem::take(buf)));
    }
}

/// Whether `chars[i..]` starts with `pat`.
fn starts_with(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(k, pc)| chars.get(i + k) == Some(&pc))
}

/// Read from `start` up to (not including) the first `pat`; return the inner
/// string and the index just past `pat`. `None` if `pat` never occurs.
fn read_until(chars: &[char], start: usize, pat: &str) -> Option<(String, usize)> {
    let pat_len = pat.chars().count();
    let mut j = start;
    while j < chars.len() {
        if starts_with(chars, j, pat) {
            let inner: String = chars[start..j].iter().collect();
            return Some((inner, j + pat_len));
        }
        j += 1;
    }
    None
}

/// Classify a single-bracket token as a known `Symbol` or an `Unknown`.
fn symbol_or_unknown(inner: String) -> TextSegment {
    if KNOWN_SYMBOLS.contains(&inner.as_str()) {
        TextSegment::Symbol(inner)
    } else {
        TextSegment::Unknown(inner)
    }
}

/// Resource-cost label: the number, or `"X"` for an X-cost card.
#[must_use]
pub fn cost_label(cost: Option<i8>) -> String {
    cost.map_or_else(|| "X".to_string(), |c| c.to_string())
}

/// CSS modifier class for a card's class colour.
#[must_use]
pub fn class_css(class: Class) -> &'static str {
    match class {
        Class::Guardian => "card--guardian",
        Class::Seeker => "card--seeker",
        Class::Rogue => "card--rogue",
        Class::Mystic => "card--mystic",
        Class::Survivor => "card--survivor",
        Class::Neutral => "card--neutral",
        Class::Mythos => "card--mythos",
    }
}

/// Human slot name.
fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Hand => "Hand",
        Slot::Accessory => "Accessory",
        Slot::Ally => "Ally",
        Slot::Arcane => "Arcane",
        Slot::Body => "Body",
        Slot::Tarot => "Tarot",
    }
}

/// Slot chips in first-seen order, collapsing duplicates to `"Name ×N"`.
#[must_use]
pub fn slot_chips(slots: &[Slot]) -> Vec<String> {
    let mut counts: Vec<(Slot, usize)> = Vec::new();
    for &s in slots {
        if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == s) {
            entry.1 += 1;
        } else {
            counts.push((s, 1));
        }
    }
    counts
        .into_iter()
        .map(|(s, n)| {
            let name = slot_name(s);
            if n > 1 {
                format!("{name} ×{n}")
            } else {
                name.to_string()
            }
        })
        .collect()
}

/// `(name, count)` for each non-zero skill icon, in canonical order.
#[must_use]
pub fn skill_chips(icons: &SkillIcons) -> Vec<(String, u8)> {
    [
        ("willpower", icons.willpower),
        ("intellect", icons.intellect),
        ("combat", icons.combat),
        ("agility", icons.agility),
        ("wild", icons.wild),
    ]
    .into_iter()
    .filter(|&(_, n)| n > 0)
    .map(|(name, n)| (name.to_string(), n))
    .collect()
}

/// Render-ready normalization of a hand-eligible card's type data.
pub struct CardFace {
    pub class_css: &'static str,
    /// `None` ⇒ no cost corner (Skill cards are committed, not played).
    pub cost_corner: Option<String>,
    pub slot_chips: Vec<String>,
    pub skill_chips: Vec<(String, u8)>,
    pub is_fast: bool,
}

/// Map a hand-eligible `CardKind` (Asset/Event/Skill) to a `CardFace`; `None`
/// for other kinds (rendered as a generic rectangle in this slice).
#[must_use]
pub fn card_face(kind: &CardKind) -> Option<CardFace> {
    match kind {
        CardKind::Asset {
            class,
            cost,
            slots,
            skill_icons,
            is_fast,
            ..
        } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: Some(cost_label(*cost)),
            slot_chips: slot_chips(slots),
            skill_chips: skill_chips(skill_icons),
            is_fast: *is_fast,
        }),
        CardKind::Event {
            class,
            cost,
            skill_icons,
            is_fast,
            ..
        } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: Some(cost_label(*cost)),
            slot_chips: Vec::new(),
            skill_chips: skill_chips(skill_icons),
            is_fast: *is_fast,
        }),
        CardKind::Skill {
            class, skill_icons, ..
        } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: None,
            slot_chips: Vec::new(),
            skill_chips: skill_chips(skill_icons),
            is_fast: false,
        }),
        _ => None,
    }
}

/// One card rendered as a faithful mini-card rectangle, colour-coded by class.
///
/// Looks metadata up via the installed registry; a card with no metadata
/// (unimplemented stub, or registry absent in a render-only path) falls back to
/// a bare rectangle showing the raw code. Non-hand card kinds render as a
/// generic rectangle (name/traits/text) — their detailed faces are later slices.
/// Display-only: no click handlers.
// Leptos components receive props by value (the macro builds a props struct); a
// reference here would require lifetime annotations the macro can't express.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn Card(code: CardCode) -> impl IntoView {
    let Some(meta) = game_core::card_registry::current().and_then(|r| (r.metadata_for)(&code))
    else {
        return view! {
            <div class="card card--unknown">
                <span class="card-name">{code.to_string()}</span>
            </div>
        }
        .into_any();
    };

    let name = meta.name.clone();
    let traits = if meta.traits.is_empty() {
        String::new()
    } else {
        format!("{}.", meta.traits.join(". "))
    };
    let text_view = meta
        .text
        .as_deref()
        .map(|t| render_segments(parse_card_text(t)));
    let weakness_view = meta
        .weakness
        .then(|| view! { <span class="card-weakness">"Weakness"</span> });

    match card_face(&meta.kind) {
        Some(face) => {
            let CardFace {
                class_css,
                cost_corner,
                slot_chips,
                skill_chips,
                is_fast,
            } = face;
            let cost_view = cost_corner.map(|c| view! { <span class="card-cost">{c}</span> });
            let fast_view = is_fast.then(|| view! { <span class="card-fast">"Fast"</span> });
            let slot_views: Vec<_> = slot_chips
                .into_iter()
                .map(|s| view! { <span class="chip chip--slot">{s}</span> })
                .collect();
            let skill_views: Vec<_> = skill_chips
                .into_iter()
                .map(|(name, n)| {
                    let label = if n > 1 {
                        format!("{name} ×{n}")
                    } else {
                        name.clone()
                    };
                    let cls = format!("chip chip--{name}");
                    view! { <span class=cls>{label}</span> }
                })
                .collect();
            view! {
                <div class=format!("card {class_css}")>
                    <div class="card-head">
                        {cost_view}
                        <span class="card-name">{name}</span>
                        {fast_view}
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-slots">{slot_views}</span>
                        <span class="card-skills">{skill_views}</span>
                    </div>
                </div>
            }
            .into_any()
        }
        None => view! {
            <div class="card card--generic">
                <div class="card-head">
                    <span class="card-name">{name}</span>
                    {weakness_view}
                </div>
                <div class="card-traits">{traits}</div>
                <div class="card-text">{text_view}</div>
            </div>
        }
        .into_any(),
    }
}

/// Render parsed card text to views (known symbols → chips; unknown tokens keep
/// their brackets in a `.unknown-token` span so they pop out).
fn render_segments(segments: Vec<TextSegment>) -> Vec<AnyView> {
    segments
        .into_iter()
        .map(|seg| match seg {
            TextSegment::Text(s) => view! { {s} }.into_any(),
            TextSegment::LineBreak => view! { <br/> }.into_any(),
            TextSegment::Symbol(tok) => {
                let cls = format!("chip chip--{tok}");
                view! { <span class=cls>{tok}</span> }.into_any()
            }
            TextSegment::Trait(t) => view! { <span class="card-trait-ref">{t}</span> }.into_any(),
            TextSegment::Bold(s) => view! { <b>{s}</b> }.into_any(),
            TextSegment::Italic(s) => view! { <i>{s}</i> }.into_any(),
            TextSegment::Unknown(inner) => {
                view! { <span class="unknown-token">{format!("[{inner}]")}</span> }.into_any()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::card_data::{CardKind, Class, SkillIcons, Slot};

    #[test]
    fn cost_label_handles_value_and_x() {
        assert_eq!(cost_label(Some(3)), "3");
        assert_eq!(cost_label(None), "X");
    }

    #[test]
    fn class_css_maps_each_class() {
        assert_eq!(class_css(Class::Guardian), "card--guardian");
        assert_eq!(class_css(Class::Mystic), "card--mystic");
        assert_eq!(class_css(Class::Neutral), "card--neutral");
    }

    #[test]
    fn slot_chips_collapse_duplicates_with_counts() {
        assert_eq!(slot_chips(&[Slot::Hand]), vec!["Hand".to_string()]);
        assert_eq!(
            slot_chips(&[Slot::Arcane, Slot::Arcane]),
            vec!["Arcane ×2".to_string()]
        );
        assert!(slot_chips(&[]).is_empty());
    }

    #[test]
    fn skill_chips_lists_nonzero_icons_in_order() {
        let icons = SkillIcons {
            willpower: 0,
            intellect: 2,
            combat: 1,
            agility: 0,
            wild: 1,
        };
        assert_eq!(
            skill_chips(&icons),
            vec![
                ("intellect".to_string(), 2),
                ("combat".to_string(), 1),
                ("wild".to_string(), 1)
            ]
        );
    }

    #[test]
    fn card_face_asset_has_cost_slots_icons() {
        let kind = CardKind::Asset {
            class: Class::Guardian,
            cost: Some(3),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons {
                combat: 1,
                ..SkillIcons::default()
            },
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        };
        let face = card_face(&kind).expect("asset has a face");
        assert_eq!(face.class_css, "card--guardian");
        assert_eq!(face.cost_corner.as_deref(), Some("3"));
        assert_eq!(face.slot_chips, vec!["Hand".to_string()]);
        assert_eq!(face.skill_chips, vec![("combat".to_string(), 1)]);
    }

    #[test]
    fn card_face_skill_has_no_cost_corner() {
        let kind = CardKind::Skill {
            class: Class::Seeker,
            xp: None,
            skill_icons: SkillIcons {
                intellect: 1,
                ..SkillIcons::default()
            },
            deck_limit: 2,
            commit_limit: Some(1),
        };
        let face = card_face(&kind).expect("skill has a face");
        assert_eq!(face.cost_corner, None);
        assert!(face.slot_chips.is_empty());
    }

    #[test]
    fn card_face_is_none_for_non_hand_kinds() {
        let kind = CardKind::Agenda { doom_threshold: 3 };
        assert!(card_face(&kind).is_none());
    }

    #[test]
    fn plain_text_is_one_segment() {
        assert_eq!(
            parse_card_text("Deal 1 damage."),
            vec![TextSegment::Text("Deal 1 damage.".into())]
        );
    }

    #[test]
    fn newline_becomes_line_break() {
        assert_eq!(
            parse_card_text("a\nb"),
            vec![
                TextSegment::Text("a".into()),
                TextSegment::LineBreak,
                TextSegment::Text("b".into()),
            ]
        );
    }

    #[test]
    fn known_symbol_becomes_symbol_segment() {
        assert_eq!(
            parse_card_text("+1 [combat]"),
            vec![
                TextSegment::Text("+1 ".into()),
                TextSegment::Symbol("combat".into()),
            ]
        );
    }

    #[test]
    fn unknown_token_is_preserved_without_brackets_in_segment() {
        // The bare token is captured; the renderer re-adds brackets.
        assert_eq!(
            parse_card_text("[mystery]"),
            vec![TextSegment::Unknown("mystery".into())]
        );
    }

    #[test]
    fn double_bracket_is_a_trait() {
        assert_eq!(
            parse_card_text("[[Tome]] only"),
            vec![
                TextSegment::Trait("Tome".into()),
                TextSegment::Text(" only".into()),
            ]
        );
    }

    #[test]
    fn bold_and_italic_runs() {
        assert_eq!(
            parse_card_text("<b>Fight.</b> <i>x</i>"),
            vec![
                TextSegment::Bold("Fight.".into()),
                TextSegment::Text(" ".into()),
                TextSegment::Italic("x".into()),
            ]
        );
    }

    #[test]
    fn unterminated_marker_is_literal_text() {
        // No closing bracket ⇒ the '[' is ordinary text, never panics.
        assert_eq!(
            parse_card_text("a [b"),
            vec![TextSegment::Text("a [b".into())]
        );
    }
}
