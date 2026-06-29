//! Visual card rendering for the web client. Pure helpers (`parse_card_text`,
//! field formatters, `card_face`) are native-testable; the `Card` component
//! assembles them into a faithful mini-card rectangle. Display-only (no click
//! handlers) — interactivity is a later slice.

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

#[cfg(test)]
mod tests {
    use super::*;

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
