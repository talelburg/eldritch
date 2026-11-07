//! Visual card rendering for the web client.
//!
//! Pure helpers (`parse_card_text`, field formatters, `card_face`) are
//! native-testable, and the `Card` component assembles them into a faithful
//! mini-card rectangle. `Card` itself is display-only. The dock shows a card
//! as a compact tile instead: the wrappers [`HandCardView`] and
//! [`InPlayCardView`] render the tile, add the click handling and the action
//! menus, and show the whole `Card` in a hover popover.

use std::mem;

use card_dsl::card_data::{CardKind, Class, SkillIcons, SkillKind, Skills, Slot};
use game_core::card_registry::CardRegistry;
use game_core::engine::OptionTarget;
use game_core::state::{CardCode, CardInPlay, InvestigatorId, UseKind};
use leptos::prelude::*;

use crate::glyph::{self, Glyph};
use crate::interaction::{self, MultiSelect, PendingOptions};
use crate::popover::{Hover, Popover};
use crate::registry;

/// A parsed run of card text. `Symbol` carries the glyph its token maps to;
/// `Unknown` carries the bare token without brackets, which the renderer
/// re-adds so unmapped tokens are visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    LineBreak,
    Symbol(&'static Glyph),
    Trait(String),
    Bold(String),
    Italic(String),
    Unknown(String),
}

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
        out.push(TextSegment::Text(mem::take(buf)));
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

/// Classify a single-bracket token: a `Symbol` if [`glyph::GLYPHS`] maps it,
/// otherwise an `Unknown`, left to render with its brackets so it pops out for
/// us to add a mapping.
fn symbol_or_unknown(inner: String) -> TextSegment {
    match glyph::for_token(&inner) {
        Some(g) => TextSegment::Symbol(g),
        None => TextSegment::Unknown(inner),
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

/// Render-ready normalization of a hand-eligible card's type data.
pub struct CardFace {
    pub class_css: &'static str,
    /// `None` ⇒ no cost corner (Skill cards are committed, not played).
    pub cost_corner: Option<String>,
    pub slot_chips: Vec<String>,
    /// The card's skill icons, drawn on its tile and in its footer.
    pub skill_icons: SkillIcons,
    pub is_fast: bool,
}

/// Map a hand-eligible `CardKind` (Asset/Event/Skill) to a `CardFace`; `None`
/// for other kinds (rendered as a generic rectangle).
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
            skill_icons: *skill_icons,
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
            skill_icons: *skill_icons,
            is_fast: *is_fast,
        }),
        CardKind::Skill {
            class, skill_icons, ..
        } => Some(CardFace {
            class_css: class_css(*class),
            cost_corner: None,
            slot_chips: Vec::new(),
            skill_icons: *skill_icons,
            is_fast: false,
        }),
        _ => None,
    }
}

/// Display label for a uses pool kind. `UseKind` is `#[non_exhaustive]`, so a
/// future variant falls back to the generic `"uses"`.
fn use_kind_label(kind: UseKind) -> &'static str {
    match kind {
        UseKind::Charges => "charges",
        UseKind::Ammo => "ammo",
        UseKind::Secrets => "secrets",
        UseKind::Supplies => "supplies",
        _ => "uses",
    }
}

/// Live per-instance state of an in-play card as chip strings.
///
/// Uses pools come first (`"2 ammo"`), then soak (`"dmg 1/2"` / `"hor 0/2"`)
/// for an `Asset` that has that capacity, then clues, doom and resources on the
/// card when non-zero. Empty for a plain asset with no uses, no soak capacity
/// and no tokens.
#[must_use]
pub fn live_state_chips(inst: &CardInPlay, kind: &CardKind) -> Vec<String> {
    let mut chips = Vec::new();
    for (use_kind, n) in &inst.uses {
        chips.push(format!("{n} {}", use_kind_label(*use_kind)));
    }
    if let CardKind::Asset { health, sanity, .. } = kind {
        if let Some(cap) = health {
            chips.push(format!("dmg {}/{cap}", inst.accumulated_damage));
        }
        if let Some(cap) = sanity {
            chips.push(format!("hor {}/{cap}", inst.accumulated_horror));
        }
    }
    if inst.clues > 0 {
        chips.push(format!("clues {}", inst.clues));
    }
    if inst.doom > 0 {
        chips.push(format!("doom {}", inst.doom));
    }
    if inst.resources > 0 {
        chips.push(format!("resources {}", inst.resources));
    }
    chips
}

/// One card rendered as a faithful mini-card rectangle, colour-coded by class.
///
/// Looks metadata up in the registry provided at mount; a card with no
/// metadata (an unimplemented stub) falls back to a bare rectangle showing the
/// raw code. Non-hand card kinds render as a
/// generic rectangle (name/traits/text) — their detailed faces are later slices.
/// Display-only: no click handlers.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Leptos `#[component]` takes `code` and `in_play` by value into its owned props struct"
)]
#[component]
pub fn Card(code: CardCode, #[prop(optional)] in_play: Option<CardInPlay>) -> impl IntoView {
    let Some(meta) = (registry::use_cards().metadata_for)(&code) else {
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
        .is_weakness()
        .then(|| view! { <span class="card-weakness">"Weakness"</span> });

    let is_in_play = in_play.is_some();
    let exhausted = in_play.as_ref().is_some_and(|c| c.exhausted);
    let exhausted_badge =
        exhausted.then(|| view! { <span class="card-exhausted">"Exhausted"</span> });
    let live_views: Vec<_> = in_play
        .as_ref()
        .map(|inst| live_state_chips(inst, &meta.kind))
        .unwrap_or_default()
        .into_iter()
        .map(|s| view! { <span class="chip chip--live">{s}</span> })
        .collect();

    match card_face(&meta.kind) {
        Some(face) => {
            let CardFace {
                class_css,
                cost_corner,
                slot_chips,
                skill_icons,
                is_fast,
            } = face;
            let cost_view = if is_in_play {
                None
            } else {
                cost_corner.map(|c| view! { <span class="card-cost">{c}</span> })
            };
            let root_class = exhaustible_class(&format!("card {class_css}"), exhausted);
            let fast_view = is_fast.then(|| view! { <span class="card-fast">"Fast"</span> });
            let slot_views: Vec<_> = slot_chips
                .into_iter()
                .map(|s| view! { <span class="chip chip--slot">{s}</span> })
                .collect();
            let skill_views = icon_views(skill_icons);
            view! {
                <div class=root_class>
                    <div class="card-head">
                        {cost_view}
                        <span class="card-name">{name}</span>
                        {fast_view}
                        {exhausted_badge}
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-slots">{slot_views}</span>
                        <span class="card-skills">{skill_views}</span>
                        <span class="card-live">{live_views}</span>
                    </div>
                </div>
            }
            .into_any()
        }
        None => {
            // The generic face (Treachery / Location / …). Per-instance state
            // surfaces as the live-chip footer (e.g. a threat-area treachery's
            // clues-on-card); the exhausted dim/badge stays Asset-only — no
            // in-scope non-asset card exhausts.
            view! {
                <div class="card card--generic">
                    <div class="card-head">
                        <span class="card-name">{name}</span>
                        {weakness_view}
                    </div>
                    <div class="card-traits">{traits}</div>
                    <div class="card-text">{text_view}</div>
                    <div class="card-footer">
                        <span class="card-live">{live_views}</span>
                    </div>
                </div>
            }
            .into_any()
        }
    }
}

/// A card's root class: `base`, dimmed by `card--exhausted` while it is
/// `exhausted`.
pub(crate) fn exhaustible_class(base: &str, exhausted: bool) -> String {
    if exhausted {
        format!("{base} card--exhausted")
    } else {
        base.to_owned()
    }
}

/// The four skills, in the order a card prints their icons.
const SKILLS: [SkillKind; 4] = [
    SkillKind::Willpower,
    SkillKind::Intellect,
    SkillKind::Combat,
    SkillKind::Agility,
];

/// One icon: the white `glyph` inside its `skill-icon--<token>` colour circle,
/// named in the circle's `title`.
fn icon_view(glyph: &'static Glyph) -> AnyView {
    let class = format!("skill-icon skill-icon--{}", glyph.token);
    view! { <span class=class title=glyph.name>{glyph::view(glyph)}</span> }.into_any()
}

/// One skill's icon.
fn skill_icon(skill: SkillKind) -> AnyView {
    icon_view(glyph::for_skill(skill))
}

/// Every icon in `icons`, one per icon printed (so two `[intellect]` are two
/// circles), wild last, in printed order.
fn icon_views(icons: SkillIcons) -> Vec<AnyView> {
    SKILLS
        .into_iter()
        .flat_map(|skill| (0..icons.matching(skill)).map(move |_| skill_icon(skill)))
        .chain((0..icons.wild).map(|_| icon_view(glyph::wild())))
        .collect()
}

/// A card as a compact tile: its name, its cost, its skill icons, a border in
/// its class colour, a mark when it is a weakness, and, in play, its uses and
/// the damage, horror and tokens on it. Its printed text is left to the hover
/// popover the slot around it hosts, which shows the whole [`Card`].
///
/// `skills` replaces the icons with the investigator's skill values, for the
/// investigator card. A code the registry can't resolve is a `card--unknown`
/// tile showing the raw code, as the full card is.
fn card_tile(code: &CardCode, in_play: Option<&CardInPlay>, skills: Option<Skills>) -> AnyView {
    let Some(meta) = (registry::use_cards().metadata_for)(code) else {
        return view! {
            <div class="tile card--unknown">
                <div class="tile-head"><span class="tile-name">{code.to_string()}</span></div>
            </div>
        }
        .into_any();
    };
    let face = card_face(&meta.kind);
    let class = match (&face, &meta.kind) {
        (Some(face), _) => face.class_css,
        (None, CardKind::Investigator { class, .. }) => class_css(*class),
        // A treachery's metadata carries no class: a weakness is a neutral
        // player card, and any other is an encounter card, a Mythos one.
        (None, CardKind::Treachery { .. }) if meta.is_weakness() => class_css(Class::Neutral),
        (None, CardKind::Treachery { .. }) => class_css(Class::Mythos),
        (None, _) => "card--generic",
    };
    let is_investigator = matches!(meta.kind, CardKind::Investigator { .. });
    // A skill, a treachery and anything else with no cost shows a dash, so
    // the name sits at the same place on every tile.
    let cost =
        (!is_investigator).then(|| match face.as_ref().and_then(|f| f.cost_corner.clone()) {
            Some(cost) => view! { <span class="tile-cost">{cost}</span> }.into_any(),
            None => view! { <span class="tile-cost none">"–"</span> }.into_any(),
        });
    let icons: Vec<AnyView> = match skills {
        None => face.map(|f| icon_views(f.skill_icons)).unwrap_or_default(),
        Some(s) => {
            let values: Vec<_> = SKILLS
                .into_iter()
                .map(|skill| {
                    view! { {skill_icon(skill)}<span class="skill-value">{s.value(skill)}</span> }
                })
                .collect();
            vec![view! { <span class="inv-skills">{values}</span> }.into_any()]
        }
    };
    let weakness = meta.is_weakness();
    let exhausted = in_play.is_some_and(|c| c.exhausted);
    let live: Vec<_> = in_play
        .map(|inst| live_state_chips(inst, &meta.kind))
        .unwrap_or_default()
        .into_iter()
        .map(|s| view! { <span class="chip chip--live">{s}</span> })
        .collect();
    let mut root = exhaustible_class(&format!("tile {class}"), exhausted);
    if weakness {
        root.push_str(" weakness");
    }
    view! {
        <div class=root>
            <div class="tile-head">
                {cost}
                <span class="tile-name">{meta.name.clone()}</span>
            </div>
            <div class="tile-icons">
                {icons}
                {weakness.then(|| view! { <span class="tile-weakness">"Weakness"</span> })}
                {exhausted.then(|| view! { <span class="tile-exhausted">"Exhausted"</span> })}
                {live}
            </div>
        </div>
    }
    .into_any()
}

/// A card's tile and, while `hover` is open, the whole [`Card`] in a popover:
/// the inside of a hand or in-play slot, which hosts the hover.
fn tile_with_popover(
    code: CardCode,
    in_play: Option<CardInPlay>,
    skills: Option<Skills>,
    hover: Hover,
) -> impl IntoView {
    let tile = card_tile(&code, in_play.as_ref(), skills);
    view! {
        {tile}
        <Popover hover=hover>
            {
                let code = code.clone();
                match in_play.clone() {
                    Some(instance) => view! { <Card code=code in_play=instance/> }.into_any(),
                    None => view! { <Card code=code/> }.into_any(),
                }
            }
        </Popover>
    }
}

/// Interactive wrapper for a hand card, shown as a tile with the whole card in
/// its hover popover. Reads `PendingOptions` + `MultiSelect` and either takes
/// part in a live `PickMultiple` or offers a "Play …"
/// [`menu_layer`](crate::interaction::menu_layer) via the card's `HandCard`
/// anchor. The two modes are mutually exclusive.
///
/// While a `PickMultiple` is live, a card the request offers (an option anchored
/// to it) is `.selectable` and a click toggles its option in the selection
/// (`.selected`). A card it does not offer — another investigator's hand, or a
/// card the pick may not take — renders inert.
#[component]
pub fn HandCardView(code: CardCode, investigator: InvestigatorId, index: u8) -> impl IntoView {
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = interaction::options_for_hand_card(&pending, investigator, index, &code);
    if let Some(ms) = use_context::<MultiSelect>() {
        if ms.active.get() {
            let hover = Hover::new();
            let Some(id) = menu_opts.first().map(|opt| opt.id.0) else {
                return view! {
                    <div class="hand-slot" {..hover.handlers()}>
                        {tile_with_popover(code, None, None, hover)}
                    </div>
                }
                .into_any();
            };
            let selected = ms.selected;
            return view! {
                <div
                    class="hand-slot selectable"
                    class:selected=move || selected.get().contains(&id)
                    on:click=move |_| selected.update(|s| {
                        if !s.remove(&id) {
                            s.insert(id);
                        }
                    })
                    {..hover.handlers()}
                >
                    {tile_with_popover(code, None, None, hover)}
                </div>
            }
            .into_any();
        }
    }
    let actionable = !menu_opts.is_empty();
    let open = RwSignal::new(None::<(i32, i32)>);
    let hover = Hover::beside_menu(open);
    view! {
        <div class="hand-slot" class:actionable={actionable} {..hover.handlers()}>
            {tile_with_popover(code, None, None, hover)}
            {
                // wasm-only Play trigger + menu (web_sys / OutboundTx).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
    .into_any()
}

/// Interactive wrapper for an in-play / threat-area card, shown as a tile with
/// the whole card in its hover popover. Reads `PendingOptions` and, via the
/// card's `CardInstance` anchor, offers an Activate (open-turn) / Trigger
/// (reaction-window) menu. No selection mode — that is hand-only.
///
/// `skills`, for the investigator card, puts the investigator's skill values on
/// the tile.
#[component]
pub fn InPlayCardView(
    instance: CardInPlay,
    #[prop(optional)] skills: Option<Skills>,
) -> impl IntoView {
    let code = instance.code.clone();
    let instance_id = instance.instance_id;
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = interaction::options_for(&pending, &OptionTarget::CardInstance(instance_id));
    let actionable = !menu_opts.is_empty();
    let open = RwSignal::new(None::<(i32, i32)>);
    let hover = Hover::beside_menu(open);
    view! {
        <div class="card-slot" class:actionable={actionable} {..hover.handlers()}>
            {tile_with_popover(code, Some(instance), skills, hover)}
            {
                // wasm-only Activate/Trigger trigger + menu (web_sys / OutboundTx).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
}

/// Render parsed card text to views: a mapped symbol becomes its
/// [`glyph::view()`] span, and an unknown token keeps its brackets in a
/// `.unknown-token` span so it pops out.
pub fn render_segments(segments: Vec<TextSegment>) -> Vec<AnyView> {
    segments
        .into_iter()
        .map(|seg| match seg {
            TextSegment::Text(s) => view! { {s} }.into_any(),
            TextSegment::LineBreak => view! { <br/> }.into_any(),
            TextSegment::Symbol(g) => glyph::view(g).into_any(),
            TextSegment::Trait(t) => view! { <span class="card-trait-ref">{t}</span> }.into_any(),
            TextSegment::Bold(s) => view! { <b>{s}</b> }.into_any(),
            TextSegment::Italic(s) => view! { <i>{s}</i> }.into_any(),
            TextSegment::Unknown(inner) => unknown_token(&inner),
        })
        .collect()
}

/// An unmapped `[token]`, brackets restored, in the red `.unknown-token` span.
fn unknown_token(inner: &str) -> AnyView {
    view! { <span class="unknown-token">{format!("[{inner}]")}</span> }.into_any()
}

/// A header card's popover: the name and printed text of the face it is
/// showing. The act, the agenda and a pile's cards show only their name on
/// the board, so this is where their text is read.
pub(crate) fn card_popover(name: &str, text: Option<&str>) -> impl IntoView {
    let text = text.map(|t| render_segments(parse_card_text(t)));
    view! {
        <div class="popover-title">{name.to_owned()}</div>
        {text.map(|t| view! { <div class="card-text">{t}</div> })}
    }
}

/// Which face of a card to show.
///
/// During an advance an act or agenda flips from its front to its reverse
/// (the "1b" side that carries the on-advance effect) once the flip is
/// clicked. `pub` because it rides in the generated component props struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// The printed front (`name`/`text`).
    Front,
    /// The reverse (`back_name`/`back_text`).
    Reverse,
}

/// Name (printed, or the raw code when no metadata) + raw ability text for a
/// card code, on the given [`Face`]. The reverse falls back to the front name
/// if a card carries no `back_name` (defensive — a real advancing act/agenda
/// always prints one).
///
/// `pub(crate)` because the decision modal names the card it is printed on
/// from the very same face the board is showing (ADR 0015) — the
/// front/reverse choice is computed once, here, rather than re-derived per
/// surface.
pub(crate) fn name_and_text_src(
    cards: &CardRegistry,
    code: &CardCode,
    face: Face,
) -> (String, Option<String>) {
    let meta = (cards.metadata_for)(code);
    let (name_src, text_src) = match face {
        Face::Front => (
            meta.map(|m| m.name.clone()),
            meta.and_then(|m| m.text.clone()),
        ),
        Face::Reverse => (
            meta.and_then(|m| m.back_name.clone())
                .or_else(|| meta.map(|m| m.name.clone())),
            meta.and_then(|m| m.back_text.clone()),
        ),
    };
    (name_src.unwrap_or_else(|| code.to_string()), text_src)
}

/// [`name_and_text_src`] read from the registry provided at mount.
pub(crate) fn name_and_text(code: &CardCode, face: Face) -> (String, Option<String>) {
    name_and_text_src(registry::use_cards(), code, face)
}

#[cfg(test)]
mod tests {
    use game_core::state::{CardInPlay as TestCardInPlay, CardInstanceId, Owner as CardOwner};

    use super::*;

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
        assert_eq!(face.skill_icons.combat, 1);
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
            commit_only_to_own_test: false,
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
                TextSegment::Symbol(glyph::for_skill(SkillKind::Combat)),
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

    #[test]
    fn live_state_chips_includes_clues_on_card() {
        // A treachery (Cover Up) in the threat area carries clues on the card.
        let meta = cards::by_code("01007").expect("Cover Up in corpus");
        let mut inst = TestCardInPlay::enter_play(
            CardCode::new("01007"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        inst.clues = 3;
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec!["clues 3".to_string()]
        );
    }

    #[test]
    fn live_state_chips_includes_doom_on_card() {
        // Arcane Initiate (ally, health 1 / sanity 2) with its 1 doom.
        let meta = cards::by_code("01063").expect("Arcane Initiate in corpus");
        let mut inst = TestCardInPlay::enter_play(
            CardCode::new("01063"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        inst.doom = 1;
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec![
                "dmg 0/1".to_string(),
                "hor 0/2".to_string(),
                "doom 1".to_string(),
            ]
        );
    }

    #[test]
    fn live_state_chips_includes_resources_on_card() {
        // Hospital Debts in the threat area, with 4 resources moved onto it.
        let meta = cards::by_code("01011").expect("Hospital Debts in corpus");
        let mut inst = TestCardInPlay::enter_play(
            CardCode::new("01011"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        inst.resources = 4;
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec!["resources 4".to_string()]
        );
    }

    #[test]
    fn live_state_chips_omits_clues_when_zero() {
        let meta = cards::by_code("01007").expect("Cover Up in corpus");
        let inst = TestCardInPlay::enter_play(
            CardCode::new("01007"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        assert!(live_state_chips(&inst, &meta.kind).is_empty());
    }

    #[test]
    fn live_state_chips_soak_uses_real_capacity() {
        // Beat Cop 01018 is an ally asset with health 2 / sanity 2.
        let meta = cards::by_code("01018").expect("Beat Cop in corpus");
        let mut inst = TestCardInPlay::enter_play(
            CardCode::new("01018"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        inst.accumulated_damage = 1;
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec!["dmg 1/2".to_string(), "hor 0/2".to_string()]
        );
    }

    #[test]
    fn live_state_chips_lists_uses_without_soak_for_plain_asset() {
        // Machete 01020 is an asset with no soak capacity.
        let meta = cards::by_code("01020").expect("Machete in corpus");
        let mut inst = TestCardInPlay::enter_play(
            CardCode::new("01020"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        inst.uses.insert(UseKind::Ammo, 2);
        assert_eq!(
            live_state_chips(&inst, &meta.kind),
            vec!["2 ammo".to_string()]
        );
    }

    #[test]
    fn live_state_chips_empty_for_plain_asset_no_uses() {
        let meta = cards::by_code("01020").expect("Machete in corpus");
        let inst = TestCardInPlay::enter_play(
            CardCode::new("01020"),
            CardInstanceId(0),
            CardOwner::Investigator(InvestigatorId(1)),
        );
        assert!(live_state_chips(&inst, &meta.kind).is_empty());
    }
}
