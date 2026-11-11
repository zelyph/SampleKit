//! The workbench's colours, one for each role project config names in
//! `[colors]`, given by a theme: SampleKit's own, dark or light, one of the
//! others built in, or one of one's own; the project's roles laid over it.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::config::project_config::{self, COLOR_ROLES, ProjectConfig};

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
    /// What waits to be written, drawn in italics.
    pub waiting: Color,
    /// The background of the cursor's line in the typing mode.
    pub line: Color,
    /// The whole screen's background, and its text where nothing else
    /// colours it: `Reset`, the terminal's own, under `terminal`.
    pub background: Color,
    pub foreground: Color,
}

impl Theme {
    /// The terminal's sixteen, `terminal`, the project's roles over them:
    /// what the TUI drew before it had themes.
    pub fn of(config: Option<&ProjectConfig>) -> Theme {
        Theme::from_roles(&terminal_roles(), config)
    }

    /// Each role as `roles` writes it, the project's over it where it names
    /// one.
    fn from_roles(roles: &IndexMap<String, String>, config: Option<&ProjectConfig>) -> Theme {
        let role = |name: &str| {
            let written = config
                .and_then(|config| config.colors_named().get(name))
                .or_else(|| roles.get(name))
                .map_or("default", String::as_str);
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
            waiting: role("waiting"),
            line: role("line"),
            background: role("background"),
            foreground: role("foreground"),
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::of(None)
    }
}

/// The themes SampleKit is built with, by name, in the order its list offers
/// them: its own two, the sixteen colours, then the nine of the colours page.
pub const BUILT_IN: &[&str] = &[
    "samplekit-dark",
    "samplekit-light",
    "terminal",
    "catppuccin-mocha",
    "catppuccin-latte",
    "dracula",
    "nord",
    "tokyo-night",
    "gruvbox-dark",
    "gruvbox-light",
    "solarized-dark",
    "solarized-light",
];

/// Each built-in theme's file, as the documentation offers it to copy: the
/// page and the TUI read the same file, so that neither drifts from the other.
const FILES: &[(&str, &str)] = &[
    (
        "samplekit-dark",
        include_str!("../../docs/templates/colours/samplekit-dark.toml"),
    ),
    (
        "samplekit-light",
        include_str!("../../docs/templates/colours/samplekit-light.toml"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../../docs/templates/colours/catppuccin-mocha.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../../docs/templates/colours/catppuccin-latte.toml"),
    ),
    (
        "dracula",
        include_str!("../../docs/templates/colours/dracula.toml"),
    ),
    (
        "nord",
        include_str!("../../docs/templates/colours/nord.toml"),
    ),
    (
        "tokyo-night",
        include_str!("../../docs/templates/colours/tokyo-night.toml"),
    ),
    (
        "gruvbox-dark",
        include_str!("../../docs/templates/colours/gruvbox-dark.toml"),
    ),
    (
        "gruvbox-light",
        include_str!("../../docs/templates/colours/gruvbox-light.toml"),
    ),
    (
        "solarized-dark",
        include_str!("../../docs/templates/colours/solarized-dark.toml"),
    ),
    (
        "solarized-light",
        include_str!("../../docs/templates/colours/solarized-light.toml"),
    ),
];

/// The theme a role left out of one's own falls back to, and the default on
/// a dark terminal.
const DARK: &str = "samplekit-dark";

/// `terminal`: every role as [`COLOR_ROLES`] gives it.
fn terminal_roles() -> IndexMap<String, String> {
    COLOR_ROLES
        .iter()
        .map(|(role, color)| (role.to_string(), color.to_string()))
        .collect()
}

/// The default theme, by the terminal's background as `COLORFGBG` says it,
/// `fg;bg` or `fg;default;bg`, the sixteen colours numbered: light where it
/// is 7 or above 8, dark otherwise, and dark where nothing says.
pub fn default_theme(colorfgbg: Option<&str>) -> &'static str {
    let light = colorfgbg
        .and_then(|value| value.rsplit(';').next())
        .and_then(|background| background.trim().parse::<u8>().ok())
        .is_some_and(|background| background == 7 || background > 8);
    if light { "samplekit-light" } else { DARK }
}

/// Whether the terminal draws a colour given by its red, green and blue, as
/// a theme's are: `COLORTERM` `truecolor` or `24bit`, as the terminals that
/// can say, or Windows Terminal, which can without saying.
pub fn draws_every_colour(colorterm: Option<&str>, windows_terminal: bool) -> bool {
    windows_terminal
        || colorterm.is_some_and(|value| {
            let value = value.trim();
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        })
}

/// The default of this terminal, read once: [`default_theme`] where it draws
/// every colour, its own sixteen, `terminal`, where it does not say so.
pub fn terminal_default() -> &'static str {
    static READ: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    READ.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM").ok();
        let windows_terminal =
            std::env::var_os("WT_SESSION").is_some_and(|value| !value.is_empty());
        if draws_every_colour(colorterm.as_deref(), windows_terminal) {
            default_theme(std::env::var("COLORFGBG").ok().as_deref())
        } else {
            "terminal"
        }
    })
}

/// Every theme there is, each its every role: the built-in ones, then one's
/// own, and what was said of a file of one's own left out.
#[derive(Debug, Clone, Default)]
pub struct Themes {
    themes: Vec<(String, IndexMap<String, String>, bool)>,
    said: Vec<String>,
}

/// Where the theme in force comes from, first to last: the folder's choice,
/// the project's `theme` (read from the configuration), the person's last
/// choice, then the default.
#[derive(Debug, Clone, Copy)]
pub struct Choices<'a> {
    pub folder: Option<&'a str>,
    pub last: Option<&'a str>,
    pub default: &'a str,
}

impl Themes {
    /// The built-in themes, then each `.toml` of `folder`, by its name, in
    /// the order of their names. A file that does not read, names a base no
    /// theme bears or one leading back to it, or takes a built-in's name is
    /// left out, and said.
    pub fn read(folder: Option<&Path>) -> Themes {
        let mut themes: Vec<(String, IndexMap<String, String>, bool)> = BUILT_IN
            .iter()
            .map(|name| {
                let roles = match FILES.iter().find(|(file, _)| file == name) {
                    Some((_, text)) => {
                        let mut roles = terminal_roles();
                        let file =
                            project_config::theme_file(Path::new(&format!("{name}.toml")), text)
                                .unwrap_or_else(|error| panic!("a built-in theme reads: {error}"));
                        roles.extend(file.colors);
                        roles
                    }
                    None => terminal_roles(),
                };
                (name.to_string(), roles, false)
            })
            .collect();
        let mut said = Vec::new();
        let mut own: IndexMap<String, project_config::ThemeFile> = IndexMap::new();
        if let Some(folder) = folder
            && folder.exists()
        {
            let mut files: Vec<PathBuf> = match std::fs::read_dir(folder) {
                Ok(entries) => entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|extension| extension == "toml")
                    })
                    .collect(),
                Err(error) => {
                    said.push(format!(
                        "the themes of {} are not read: {error}",
                        folder.display()
                    ));
                    Vec::new()
                }
            };
            files.sort();
            for path in files {
                let Some(name) = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
                else {
                    continue;
                };
                if BUILT_IN.contains(&name.as_str()) {
                    said.push(format!(
                        "theme {}: {name} is a theme SampleKit has, left as it is: give yours \
                         another name, with base = \"{name}\" to start from it",
                        path.display()
                    ));
                    continue;
                }
                let read = std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))
                    .and_then(|text| {
                        project_config::theme_file(&path, &text).map_err(|error| error.to_string())
                    });
                match read {
                    Ok(file) => {
                        own.insert(name, file);
                    }
                    Err(error) => said.push(format!("theme left out: {error}")),
                }
            }
        }
        // Each of one's own on its base, a base of one's own first: followed
        // from one to the next, a loop said.
        let names: Vec<String> = own.keys().cloned().collect();
        for name in names {
            let mut chain: Vec<String> = vec![name.clone()];
            let base_roles = loop {
                let last = chain.last().expect("the chain holds the theme");
                let base = own[last.as_str()]
                    .base
                    .clone()
                    .unwrap_or_else(|| DARK.to_string());
                if let Some((_, roles, _)) = themes.iter().find(|(held, ..)| *held == base) {
                    break Ok(roles.clone());
                }
                if chain.contains(&base) {
                    chain.push(base);
                    break Err(format!(
                        "theme {name} left out: its bases lead back to it, {}",
                        chain.join(" → ")
                    ));
                }
                if !own.contains_key(&base) {
                    let known: Vec<String> = themes
                        .iter()
                        .map(|(held, ..)| held.clone())
                        .chain(own.keys().cloned())
                        .collect();
                    let near =
                        crate::core::identifier::nearest(&base, known.iter().map(String::as_str))
                            .map(|near| format!(" — did you mean {near}?"))
                            .unwrap_or_default();
                    break Err(format!(
                        "theme {name} left out: base = \"{base}\" is no theme here{near}"
                    ));
                }
                chain.push(base);
            };
            match base_roles {
                Ok(mut roles) => {
                    // From the base up, each theme's roles over the one below.
                    for link in chain.iter().rev() {
                        roles.extend(own[link.as_str()].colors.clone());
                    }
                    themes.push((name, roles, true));
                }
                Err(error) => said.push(error),
            }
        }
        Themes { themes, said }
    }

    pub fn names(&self) -> Vec<&str> {
        self.themes.iter().map(|(name, ..)| name.as_str()).collect()
    }

    /// Whether `name` is one's own, of the themes folder.
    pub fn own(&self, name: &str) -> bool {
        self.themes
            .iter()
            .any(|(held, _, own)| held == name && *own)
    }

    /// What was said of one's own themes left out.
    pub fn said(&self) -> &[String] {
        &self.said
    }

    /// A theme by name, the project's roles laid over it.
    pub fn theme(&self, name: &str, config: Option<&ProjectConfig>) -> Option<Theme> {
        self.themes
            .iter()
            .find(|(held, ..)| held == name)
            .map(|(_, roles, _)| Theme::from_roles(roles, config))
    }

    /// The theme in force: the first of `choices` and the project's `theme`
    /// that names a theme there is, in their order, the default otherwise;
    /// its name, the theme with the project's roles over it, and what was
    /// said of a name passed over.
    pub fn resolve(
        &self,
        choices: Choices,
        config: Option<&ProjectConfig>,
    ) -> (String, Theme, Vec<String>) {
        let named = [
            (choices.folder, "the theme chosen in this folder"),
            (
                config.and_then(ProjectConfig::theme),
                "the project's [colors] theme",
            ),
            (choices.last, "the theme last chosen"),
            (Some(choices.default), "the default theme"),
        ];
        let mut unknown: Vec<String> = Vec::new();
        for (name, whose) in named {
            let Some(name) = name else { continue };
            if let Some(theme) = self.theme(name, config) {
                let said = unknown
                    .into_iter()
                    .map(|passed| format!("{passed}: {name} instead"))
                    .collect();
                return (name.to_string(), theme, said);
            }
            let near = crate::core::identifier::nearest(name, self.names())
                .map(|near| format!(" — did you mean {near}?"))
                .unwrap_or_default();
            unknown.push(format!("{whose}, {name}, is no theme here{near}"));
        }
        // No default unknown: SampleKit's own are built in.
        let theme = self
            .theme(DARK, config)
            .unwrap_or_else(|| Theme::of(config));
        (DARK.to_string(), theme, unknown)
    }
}

/// Where the person's last choice of a theme is kept: `theme` in the state
/// folder, for every project.
pub fn last_choice_file() -> Option<PathBuf> {
    crate::config::model_runtime::state_directory().map(|directory| directory.join("theme"))
}

/// The theme `file` keeps, if it keeps one.
pub fn last_chosen(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `name` kept in `file` as the person's last choice.
pub fn keep_last(file: &Path, name: &str) -> Result<(), String> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("the theme is not kept: {}: {error}", folder.display()))?;
    }
    std::fs::write(file, format!("{name}\n"))
        .map_err(|error| format!("the theme is not kept: {}: {error}", file.display()))
}

/// The frame painted in its theme: each colour left to the terminal the
/// theme's text or background, wherever it was drawn — a pane, a window, a
/// cell a window cleared. `terminal` leaves them to it.
pub fn paint(buffer: &mut Buffer, theme: &Theme) {
    if theme.background == Color::Reset && theme.foreground == Color::Reset {
        return;
    }
    for cell in &mut buffer.content {
        if cell.fg == Color::Reset {
            cell.fg = theme.foreground;
        }
        if cell.bg == Color::Reset {
            cell.bg = theme.background;
        }
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
