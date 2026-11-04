//! The workbench's colours, one for each role project config names in
//! `[colors]`: the terminal's sixteen unless the project says
//! otherwise, so that the terminal's theme decides the shades.

use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::config::project_config::{COLOR_ROLES, ProjectConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub failed: Color,
    pub outdated: Color,
    pub edited: Color,
    pub current: Color,
    pub selected: Color,
    pub attribute: Color,
    /// A link, underlined in this colour.
    pub link: Color,
    /// A value and its uncertainty, drawn bold in this colour.
    pub value: Color,
    pub unit: Color,
    pub table: Color,
    pub index: Color,
    pub key: Color,
    pub muted: Color,
    pub message: Color,
    pub error: Color,
    /// The name on the title bar, and a computation under way beside it.
    pub accent: Color,
}

impl Theme {
    /// The project's colours, each role it leaves out the default one.
    pub fn of(config: Option<&ProjectConfig>) -> Theme {
        let role = |name: &str| {
            let written = config
                .and_then(|config| config.color(name))
                .or_else(|| {
                    COLOR_ROLES
                        .iter()
                        .find(|(role, _)| *role == name)
                        .map(|(_, color)| *color)
                })
                .unwrap_or("default");
            color(written)
        };
        Theme {
            failed: role("failed"),
            outdated: role("outdated"),
            edited: role("edited"),
            current: role("current"),
            selected: role("selected"),
            attribute: role("attribute"),
            link: role("link"),
            value: role("value"),
            unit: role("unit"),
            table: role("table"),
            index: role("index"),
            key: role("key"),
            muted: role("muted"),
            message: role("message"),
            error: role("error"),
            accent: role("accent"),
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::of(None)
    }
}

/// A colour as `[colors]` writes it, which the configuration has
/// already checked: a name, `default`, or `#rrggbb`.
pub fn color(written: &str) -> Color {
    if let Some(hex) = written.strip_prefix('#') {
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        return Color::Rgb(channel(0), channel(2), channel(4));
    }
    match written {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "grey" | "gray" => Color::Gray,
        "dark-grey" | "dark-gray" => Color::DarkGray,
        "light-red" => Color::LightRed,
        "light-green" => Color::LightGreen,
        "light-yellow" => Color::LightYellow,
        "light-blue" => Color::LightBlue,
        "light-magenta" => Color::LightMagenta,
        "light-cyan" => Color::LightCyan,
        "white" => Color::White,
        _ => Color::Reset,
    }
}

thread_local! {
    /// The theme of the frame being drawn: set once per frame, read wherever
    /// a colour is chosen, rather than handed to every function that draws.
    static DRAWING: Cell<Theme> = Cell::new(Theme::default());
}

pub fn set(theme: Theme) {
    DRAWING.with(|drawing| drawing.set(theme));
}

pub fn colors() -> Theme {
    DRAWING.with(Cell::get)
}

/// A key, wherever one is named to the user — a screen's foot, a window's
/// border, the help, the start page, a sentence: the theme's `key` colour,
/// bold. The one style every surface draws a key in.
pub fn key_style(theme: &Theme) -> Style {
    Style::new().fg(theme.key).add_modifier(Modifier::BOLD)
}

/// A value and its uncertainty, wherever one is drawn: bold, in the theme's
/// `value` colour, as the command line draws it.
pub fn value_style(theme: &Theme) -> Style {
    Style::new().fg(theme.value).add_modifier(Modifier::BOLD)
}

/// A value as a line shows it, `12.513 ± 0.010 mm`: its number in the value's
/// style and the unit after it in the unit's colour; text that is no number in
/// `other`.
pub fn value_spans(shown: &str, other: Style) -> Vec<Span<'static>> {
    let theme = colors();
    let number = |word: &str| word.trim_start_matches('(').parse::<f64>().is_ok();
    // A value aligned on its decimal point begins with spaces: its first
    // word is the number after them, not the empty one before.
    let words: Vec<&str> = shown.trim_start().split(' ').collect();
    if !words.first().is_some_and(|first| number(first)) {
        return vec![Span::styled(shown.to_string(), other)];
    }
    match words.as_slice() {
        [.., last] if words.len() > 1 && !number(last) && *last != "±" => {
            let cut = shown.len() - last.len();
            vec![
                Span::styled(shown[..cut].to_string(), value_style(&theme)),
                Span::styled((*last).to_string(), Style::new().fg(theme.unit)),
            ]
        }
        _ => vec![Span::styled(shown.to_string(), value_style(&theme))],
    }
}

/// What is selected in a line typed or the note: reversed, readable whatever
/// the terminal's colours, dark or light.
pub fn selection_style() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

/// Whether `word` names a key: one character, a key's name, a modified key
/// (`Ctrl+C`, `Shift+Tab`), arrows (`←→`, `⇧↑↓`), or a range of digits
/// (`1…9`). The figure's `←→` and the start page's `Ctrl+C` were drawn as
/// what they do, uncoloured.
pub fn is_key(word: &str) -> bool {
    let arrows = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|character| matches!(character, '←' | '→' | '↑' | '↓' | '⇧'))
    };
    let range = || {
        let mut parts = word.split('…');
        matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(from), Some(to), None)
                if from.chars().count() == 1 && to.chars().count() == 1
        )
    };
    word.chars().count() == 1
        || arrows(word)
        || range()
        || matches!(
            word,
            "Enter"
                | "Esc"
                | "Tab"
                | "Space"
                | "Backspace"
                | "Del"
                | "PgUp"
                | "PgDn"
                | "Home"
                | "End"
        )
        || ["Shift+", "Ctrl+", "Alt+"].iter().any(|modifier| {
            word.strip_prefix(modifier)
                .is_some_and(|rest| !rest.is_empty())
        })
}

/// Whether a word of a sentence names a key by its name alone — a single
/// character is one only by what follows it (`keyed`).
fn named_key(word: &str) -> bool {
    word.chars().count() > 1 && !word.contains('…') && is_key(word)
}

/// A sentence, each key it names in [`key_style`] and the rest in `base` : a
/// key's name wherever it stands — `Enter`, `Esc`, `Ctrl+C`, `←` — and a single
/// character where what follows says what it does, a verb: *u gives it back*,
/// *C computes*, *+ adds a row*, *P, then w, writes*. A digit is never one — *3
/// samples* — nor a mark or a dash.
pub fn keyed(text: &str, base: Style) -> Vec<Span<'static>> {
    let key = key_style(&colors());
    let words: Vec<&str> = text.split(' ').collect();
    // What a word is once what surrounds it — quotes, brackets, a comma — is
    // set aside.
    let core = |word: &str| -> (usize, usize) {
        let start = word.len() - word.trim_start_matches(['(', '"', '\'', '`']).len();
        let rest = &word[start..];
        let kept = rest.trim_end_matches([',', ';', ':', '.', ')', '"', '\'', '`']);
        // A key that is itself a mark — `.`, `,` — is kept whole.
        if kept.is_empty() {
            return (start, word.len());
        }
        (start, start + kept.len())
    };
    // A verb said of a key: a word of letters ending in `s`, as *gives*,
    // *computes*, *adds* — and not in `ss`, `us` or `is`, as *class*,
    // *status*, *basis* are nouns.
    let verb = |word: Option<&&str>| {
        let Some(word) = word else { return false };
        let word = word.trim_end_matches([',', ';', ':', '.', ')']);
        word.len() > 2
            && word.chars().all(|character| character.is_ascii_lowercase())
            && word.ends_with('s')
            && !word.ends_with("ss")
            && !word.ends_with("us")
            && !word.ends_with("is")
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    for (at, word) in words.iter().enumerate() {
        if at > 0 {
            plain.push(' ');
        }
        let (from, to) = core(word);
        let name = &word[from..to];
        let single = name.chars().count() == 1
            && name.chars().next().is_some_and(|character| {
                character.is_ascii_alphabetic()
                    || matches!(
                        character,
                        '/' | '?' | '+' | '<' | '>' | '~' | '←' | '→' | '↑' | '↓'
                    )
            });
        // `P, then w, writes them`: a key before `, then` is one too.
        let then = word[to..].starts_with(',') && words.get(at + 1) == Some(&"then");
        // What follows it, where nothing but a comma stands between.
        let after = if word[to..].starts_with(',') || word.len() == to {
            words.get(at + 1)
        } else {
            None
        };
        let is = named_key(name) || (single && (verb(after) || then));
        if is {
            plain.push_str(&word[..from]);
            if !plain.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut plain), base));
            }
            spans.push(Span::styled(name.to_string(), key));
            plain.push_str(&word[to..]);
        } else {
            plain.push_str(word);
        }
    }
    if !plain.is_empty() || spans.is_empty() {
        spans.push(Span::styled(plain, base));
    }
    spans
}
