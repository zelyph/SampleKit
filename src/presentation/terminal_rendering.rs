//! Turning quantities into text laid out for a terminal. It formats *layout*:
//! a single quantity is rendered by [`crate::core::formatting`], and this
//! module decides where the resulting strings go and how much room they get.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// What a table too wide for the terminal says of the columns it left out.
/// One constant, because `compute` recognises a narrowed table by it: a copy
/// of the words in two files disables that fallback silently the day one of
/// them is reworded.
pub const HIDDEN: &str = "hidden — --width shows them";

/// What a table whose columns can be named adds to [`HIDDEN`]: a command with
/// no `-c` was sent to name fewer columns.
pub const FEWER: &str = ", or -c names fewer";

use crate::config::project_config::{ColumnSpec, Profile, ProjectConfig};
use crate::core::formatting::{self, Precision, Presentation, Resolved};
use crate::core::identifier::Identifier;
use crate::core::uncertainty::Uncertainty;
use crate::core::value::Value;
use crate::query::field_addressing::{
    self as fields, Channel, Field, Resolution, Statistic, Subject,
};

// ------------------------------------------------------------------- types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    /// Numbers line up on their decimal point. A column mixing `12.5` and
    /// `1284` reads as magnitudes only when the points align; right alignment
    /// alone makes the eye compare digit counts.
    Decimal,
}

/// A terminal and a pipe want different output, so they are different targets
/// rather than one target and a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Terminal { width: usize },
    Pipe,
}

/// How a table is drawn — a reader's preference, never a statement about the
/// data. A style may add a border; it may not add a digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Plain,
    Boxed,
}

impl Style {
    /// The name a `[render] table` setting or a `--table-style` flag uses.
    pub fn parse(name: &str) -> Option<Style> {
        match name {
            "plain" => Some(Style::Plain),
            "boxed" => Some(Style::Boxed),
            _ => None,
        }
    }

    pub fn names() -> [&'static str; 2] {
        crate::config::project_config::TABLE_STYLES
    }
}

/// A table's geometry, decided before anything is drawn: which columns are
/// shown, how wide each one is, how its cells align, how many leading columns
/// the terminal added itself, and how many were dropped to fit.
///
/// The decision is a value so that it has one owner. *A number is never cut*
///  and *a text column never narrows past its longest word* are
/// properties of this value rather than of the function that prints it:
/// [`table`] draws it into a string, and a surface that places its own columns
/// reads the same widths instead of deciding them again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The shown columns' headers, in order.
    pub headers: Vec<String>,
    /// The shown columns' cells, already padded for decimal alignment.
    pub cells: Vec<Vec<String>>,
    /// Display columns each shown column occupies, separators excluded.
    pub widths: Vec<usize>,
    /// How each shown column's cells sit in their width.
    pub alignment: Vec<Alignment>,
    /// How many columns were dropped from the right to fit the width.
    pub hidden: usize,
    /// How many leading columns the terminal added, and greys with it.
    pub greyed: usize,
}

/// Whose declaration a cell is rendered under: a view's entry or a profile's
/// column.
///
/// The two compose presentation identically — one function, in `views` — and
/// this is what lets one renderer serve both. `cell` always claimed to render
/// *a view or a profile*; taking a `ViewDefinition` was the signature not
/// saying so.
#[derive(Debug, Clone, Copy)]
pub enum Declaration<'a> {
    Profile(&'a Profile),
}

impl Declaration<'_> {
    fn presentation_for(
        &self,
        entry: &ColumnSpec,
        property: &formatting::Presentation,
    ) -> formatting::Presentation {
        match self {
            Declaration::Profile(profile) => profile.presentation_for(entry, property),
        }
    }
}

/// The placeholder a terminal shows for a value that is not there. **Never** in
/// a pipe: a dash in a CSV is a *value*, and one that poisons a numeric column.
pub const ABSENT: &str = "—";

// ------------------------------------------------------------------- widths

/// How many terminal columns a string occupies.
///
/// Not bytes, not `char` count. `é` is one column and two bytes; a CJK
/// character is two columns; a combining accent is zero. Byte-based width
/// produces ragged tables for exactly the users this program has.
/// What a terminal draws of `text`: its colours taken out. A painted cell is
/// judged by what it says, a number or a unit, never by its escapes.
fn drawn(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\u{1b}') {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut parser = anstyle_parse::Parser::<anstyle_parse::DefaultCharAccumulator>::default();
    let mut drawn = Drawn(String::new());
    for byte in text.as_bytes() {
        parser.advance(&mut drawn, *byte);
    }
    std::borrow::Cow::Owned(drawn.0)
}

pub fn width(text: &str) -> usize {
    // A style takes no room on the screen, and must take none in the layout
    // either: a cell that carries colour would otherwise widen its column by the
    // length of an escape nobody sees, and truncation could cut one in half.
    // The parser is `anstyle`'s, so that what is measured is what is drawn.
    if !text.contains('\u{1b}') {
        return UnicodeWidthStr::width(text);
    }
    let mut parser = anstyle_parse::Parser::<anstyle_parse::DefaultCharAccumulator>::default();
    let mut drawn = Drawn(String::new());
    for byte in text.as_bytes() {
        parser.advance(&mut drawn, *byte);
    }
    UnicodeWidthStr::width(drawn.0.as_str())
}

/// What a terminal would actually draw, collected from a styled string.
struct Drawn(String);

impl anstyle_parse::Perform for Drawn {
    fn print(&mut self, character: char) {
        self.0.push(character);
    }
}

/// The mark a value's state gives it in a terminal: `✎` edited by hand, `⚠`
/// outdated, `✗` failed. Current and entered values carry none.
pub fn mark_of(state: &crate::format::fingerprint::Freshness) -> Option<&'static str> {
    use crate::format::fingerprint::Freshness;
    match state {
        Freshness::Edited | Freshness::RecordMissing => Some("✎"),
        // Not known to be current is marked as what is known not to be.
        Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Unjudged { .. } => {
            Some("⚠")
        }
        Freshness::Failed { .. } => Some("✗"),
        Freshness::Source | Freshness::Current => None,
    }
}

/// The style a state carries where colour is allowed: yellow reads *out of
/// date*, red *failed*, cyan *a hand touched this*. Current and entered values
/// carry none, as they carry no mark.
///
/// One vocabulary for both: the mark in a table and the words `status` writes
/// take the same colour for the same state, or the two teach different things.
pub fn style_of(state: &crate::format::fingerprint::Freshness) -> Option<anstyle::Style> {
    use crate::format::fingerprint::Freshness;
    let palette = palette();
    match state {
        Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Unjudged { .. } => {
            Some(palette.outdated)
        }
        Freshness::Failed { .. } => Some(palette.failed),
        Freshness::Edited | Freshness::RecordMissing => Some(palette.edited),
        Freshness::Source | Freshness::Current => None,
    }
}

/// The command line's colours by role, `[colors]`'s or the defaults the TUI
/// has too, and whether they are drawn at all. Off until a command whose
/// output is a terminal turns it on: the TUI, the exports, Python and the
/// tests share this rendering, and draw plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// A value and its uncertainty: bold, on the role's colour.
    pub value: anstyle::Style,
    pub attribute: anstyle::Style,
    /// A link: underlined, in the role's colour.
    pub link: anstyle::Style,
    pub unit: anstyle::Style,
    pub failed: anstyle::Style,
    pub outdated: anstyle::Style,
    pub edited: anstyle::Style,
    /// A value current, `✓` and `written:`.
    pub current: anstyle::Style,
    /// What says nothing happened, and advice.
    pub muted: anstyle::Style,
    /// `error:` before what went wrong.
    pub error: anstyle::Style,
    /// `warning:` before what may be wrong.
    pub message: anstyle::Style,
    pub enabled: bool,
}

impl Palette {
    /// The project's colours, each role it leaves out the default one.
    pub fn of(config: Option<&ProjectConfig>, enabled: bool) -> Palette {
        let role = |name: &str| {
            let written = config
                .and_then(|config| config.color(name))
                .or_else(|| {
                    crate::config::project_config::COLOR_ROLES
                        .iter()
                        .find(|(role, _)| *role == name)
                        .map(|(_, color)| *color)
                })
                .unwrap_or("default");
            anstyle::Style::new().fg_color(ansi_color(written))
        };
        Palette {
            value: role("value").bold(),
            attribute: role("attribute"),
            link: role("link").underline(),
            unit: role("unit"),
            failed: role("failed"),
            outdated: role("outdated"),
            edited: role("edited"),
            current: role("current"),
            muted: role("muted"),
            error: role("error"),
            message: role("message"),
            enabled,
        }
    }
}

impl Default for Palette {
    fn default() -> Palette {
        Palette::of(None, false)
    }
}

/// A colour as `[colors]` writes it, which the configuration has already
/// checked, for the terminal: `None` for its own foreground.
fn ansi_color(written: &str) -> Option<anstyle::Color> {
    use anstyle::AnsiColor as Ansi;
    if let Some(hex) = written.strip_prefix('#') {
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        return Some(anstyle::RgbColor(channel(0), channel(2), channel(4)).into());
    }
    let ansi = match written {
        "black" => Ansi::Black,
        "red" => Ansi::Red,
        "green" => Ansi::Green,
        "yellow" => Ansi::Yellow,
        "blue" => Ansi::Blue,
        "magenta" => Ansi::Magenta,
        "cyan" => Ansi::Cyan,
        "grey" | "gray" => Ansi::White,
        "dark-grey" | "dark-gray" => Ansi::BrightBlack,
        "light-red" => Ansi::BrightRed,
        "light-green" => Ansi::BrightGreen,
        "light-yellow" => Ansi::BrightYellow,
        "light-blue" => Ansi::BrightBlue,
        "light-magenta" => Ansi::BrightMagenta,
        "light-cyan" => Ansi::BrightCyan,
        "white" => Ansi::BrightWhite,
        _ => return None,
    };
    Some(ansi.into())
}

thread_local! {
    /// The palette of the command running: set once, read wherever its output
    /// is painted, rather than handed down every call.
    static PALETTE: std::cell::Cell<Palette> = std::cell::Cell::new(Palette::default());
}

/// The palette this command draws with, plain until it is set.
pub fn set_palette(palette: Palette) {
    PALETTE.with(|held| held.set(palette));
}

pub fn palette() -> Palette {
    PALETTE.with(std::cell::Cell::get)
}

/// Whether stderr is painted: a terminal reads it and nothing turned colour
/// off. Decided by the command line, read where a Python side is started.
static STDERR_PAINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_stderr_painted(painted: bool) {
    STDERR_PAINTED.store(painted, std::sync::atomic::Ordering::Relaxed);
}

/// What a Python side writing to the same stderr is given to paint its own
/// `warning:`, `error:` and advice as the command line does: each style's
/// escape, by name, or nothing where stderr is not painted.
pub fn child_colours() -> Vec<(&'static str, String)> {
    if !STDERR_PAINTED.load(std::sync::atomic::Ordering::Relaxed) {
        return Vec::new();
    }
    let palette = palette();
    vec![
        (
            "SAMPLEKIT_PAINT_WARNING",
            palette.message.render().to_string(),
        ),
        ("SAMPLEKIT_PAINT_ERROR", palette.error.render().to_string()),
        ("SAMPLEKIT_PAINT_MUTED", palette.muted.render().to_string()),
        ("SAMPLEKIT_PAINT_RESET", anstyle::Reset.render().to_string()),
    ]
}

/// `text` in `style` where the palette is on, each word apart so that a line
/// wrapped between words never cuts a colour in two.
pub fn paint(text: &str, style: anstyle::Style) -> String {
    if !palette().enabled || text.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    for (at, word) in text.split(' ').enumerate() {
        if at > 0 {
            out.push(' ');
        }
        if !word.is_empty() {
            out.push_str(&painted(word, style, true));
        }
    }
    out
}

/// A state as a table says it, `never computed`, `outdated — notch`, in the
/// colour the TUI gives the same words: what has to be computed in the
/// outdated colour, a failure in the failed one, a hand's value in the edited
/// one. `None` for a word that says nothing is wrong.
pub fn state_style(said: &str) -> Option<anstyle::Style> {
    let palette = palette();
    let said = said.trim();
    if said.is_empty() || matches!(said, "entered" | "—" | "-") {
        None
    } else if said == "current" {
        Some(palette.current)
    } else if said.starts_with("failed") {
        Some(palette.failed)
    } else if said.starts_with("edited") || said.starts_with("record missing") {
        Some(palette.edited)
    } else {
        // Outdated, never computed, waiting, its formula changed: a value to
        // compute again, said in the outdated colour whatever its words.
        Some(palette.outdated)
    }
}

/// A state cell painted, where the palette is on and nothing painted it yet.
pub fn painted_state(said: &str) -> String {
    if !palette().enabled || said.contains('\u{1b}') {
        return said.to_string();
    }
    match state_style(said) {
        Some(style) => paint(said, style),
        None => said.to_string(),
    }
}

/// A value as a report says it, `12.513 ± 0.010 mm`, painted where the palette
/// is on: its number bold and the unit after it in the unit's colour. What is
/// no number, `unchanged`, `13 new`, is left as it is.
pub fn painted_said(text: &str) -> String {
    if !palette().enabled {
        return text.to_string();
    }
    let words: Vec<&str> = text.split(' ').collect();
    let number = |word: &str| word.parse::<f64>().is_ok();
    // A list of readings, `1.995, 2.004, 2.008`, is values too.
    if words.len() > 1
        && words
            .iter()
            .all(|word| number(word.strip_suffix(',').unwrap_or(word)))
        && words[..words.len() - 1]
            .iter()
            .all(|word| word.ends_with(','))
    {
        return paint(text, palette().value);
    }
    if !words.first().is_some_and(|first| number(first)) {
        return text.to_string();
    }
    // A count, `13 new`, `20 changed`, `21 cells`, says how many, not a value.
    if words.len() == 2 && matches!(words[1], "new" | "changed" | "cells" | "cell" | "rows") {
        return text.to_string();
    }
    match words.as_slice() {
        [.., last] if words.len() > 1 && !number(last) && *last != "±" => {
            let unit = *last;
            let number = &text[..text.len() - unit.len() - 1];
            painted_quantity(&format!("{number} {unit}"), Some(unit))
        }
        _ => paint(text, palette().value),
    }
}

/// A table's cells painted by what their column holds, where the palette is
/// on: a property's value bold, an attribute's in its colour, a unit in the
/// unit's. A cell standing for nothing, and a column of names or paths, stay
/// as they are. Called once the rows are laid out, before any mark is added.
pub fn paint_cells(profile: &Profile, subjects: &[Subject], rows: &mut [Vec<String>]) {
    let palette = palette();
    if !palette.enabled {
        return;
    }
    for (at, column) in profile.columns().iter().enumerate() {
        let Ok(field) = fields::parse(&column.field) else {
            continue;
        };
        let style = match &field {
            Field::Named {
                channel: Channel::Unit,
                ..
            }
            | Field::Cell {
                channel: Channel::Unit,
                ..
            } => Some(palette.unit),
            Field::Named {
                channel: Channel::Symbol,
                ..
            }
            | Field::Cell {
                channel: Channel::Symbol,
                ..
            } => None,
            Field::Named { name, .. } => {
                if subjects
                    .iter()
                    .any(|subject| subject.sample.property(name).is_ok())
                {
                    Some(palette.value)
                } else if subjects
                    .iter()
                    .any(|subject| subject.sample.attribute(name).is_ok())
                {
                    Some(palette.attribute)
                } else {
                    None
                }
            }
            Field::Cell { .. } => Some(palette.value),
            Field::ListItem { .. } => Some(palette.attribute),
            Field::Reserved(_) => None,
        };
        let Some(style) = style else {
            continue;
        };
        for row in rows.iter_mut() {
            if let Some(cell) = row.get_mut(at)
                && !matches!(cell.as_str(), "" | ABSENT | "-" | "n/a")
            {
                *cell = paint(cell, style);
            }
        }
    }
}

/// The links of a table drawn in a terminal shown as Markdown shows them,
/// their text alone, marked where they leave the project or lead nowhere,
/// underlined in the link's colour where the palette is on. Each sample's
/// wikilinks are found among `samples`, each its path and its `name:`.
pub fn show_links(
    profile: &Profile,
    subjects: &[Subject],
    rows: &mut [Vec<String>],
    samples: &[(std::path::PathBuf, Option<String>)],
) {
    use crate::presentation::links::{self, Leads};
    let palette = palette();
    for (subject, row) in subjects.iter().zip(rows.iter_mut()) {
        for (cell, link) in row.iter_mut().zip(links::row(profile, subject, samples)) {
            let Some(link) = link else {
                continue;
            };
            *cell = if palette.enabled {
                let style = match link.leads {
                    Leads::Nowhere => palette.error.underline(),
                    _ => palette.link,
                };
                paint(&link.text, style)
            } else {
                link.text
            };
        }
    }
}

/// A header, `width [mm]`: its label bold, its unit in the unit's colour.
pub fn painted_header(header: &str) -> String {
    if !palette().enabled {
        return header.to_string();
    }
    match header.rsplit_once(" [") {
        Some((label, unit)) if unit.ends_with(']') => format!(
            "{} {}",
            paint(label, anstyle::Style::new().bold()),
            paint(&format!("[{unit}"), palette().unit)
        ),
        _ => paint(header, anstyle::Style::new().bold()),
    }
}

/// A quantity as a line writes it, `12.513 ± 0.010 mm`: the number bold, the
/// unit after it in the unit's colour. Text that is no number is an attribute's.
pub fn painted_quantity(text: &str, unit: Option<&str>) -> String {
    let palette = palette();
    if !palette.enabled || text.is_empty() || text == ABSENT {
        return text.to_string();
    }
    let (number, unit) = match unit.filter(|unit| !unit.is_empty()) {
        Some(unit) => match text
            .strip_suffix(unit)
            .and_then(|rest| rest.strip_suffix(' '))
        {
            Some(number) => (number, Some(unit)),
            None => (text, None),
        },
        None => (text, None),
    };
    let looks_numeric = number.split(' ').next().is_some_and(|first| {
        first
            .trim_start_matches(['-', '+', '('])
            .parse::<f64>()
            .is_ok()
    });
    let style = if looks_numeric {
        palette.value
    } else {
        palette.attribute
    };
    match unit {
        Some(unit) => format!("{} {}", paint(number, style), paint(unit, palette.unit)),
        None => paint(number, style),
    }
}

/// The mark that goes *beside this text*, where one does.
///
/// A value that failed is marked even where no value stands, because the
/// absence is the thing being reported; any other state leaves an absent cell
/// unmarked, since the mark would be the whole of what the reader sees.
pub fn mark_beside(
    text: &str,
    state: &crate::format::fingerprint::Freshness,
) -> Option<&'static str> {
    let mark = mark_of(state)?;
    if mark != "✗" && matches!(text, "" | ABSENT | "-") {
        return None;
    }
    Some(mark)
}

/// A value with its state's mark after it, painted where colour is allowed:
/// `13.00 ✎`, `1.93 ⚠`.
///
/// The composition of [`mark_beside`], [`style_of`] and [`painted`] lives here
/// rather than at each call site, so that a table, a report and a screen show
/// the same state the same way instead of each assembling it again.
pub fn with_mark(
    text: &str,
    state: &crate::format::fingerprint::Freshness,
    colour: bool,
) -> String {
    let Some(mark) = mark_beside(text, state) else {
        return text.to_string();
    };
    match style_of(state) {
        Some(style) => format!("{text} {}", painted(mark, style, colour)),
        None => format!("{text} {mark}"),
    }
}

/// Text wearing the colour of a state, where colour is allowed: the words
/// `status` writes take the colour the mark in a table takes, or the two teach
/// different things.
pub fn painted_as(
    text: &str,
    state: &crate::format::fingerprint::Freshness,
    colour: bool,
) -> String {
    match style_of(state) {
        Some(style) => painted(text, style, colour),
        // A value current says so in the current colour, as the TUI does.
        None if matches!(state, crate::format::fingerprint::Freshness::Current) => {
            painted(text, palette().current, colour)
        }
        None => text.to_string(),
    }
}

/// A command the reader is invited to run, at the foot of a long report.
pub fn action() -> anstyle::Style {
    anstyle::Style::new().bold()
}

/// A column the terminal added rather than one that was asked for.
pub fn added() -> anstyle::Style {
    anstyle::Style::new().dimmed()
}

/// `text` wearing `style`, or unchanged where colour is not allowed — which is
/// decided once, from the target, and never guessed at a call site.
pub fn painted(text: &str, style: anstyle::Style, allowed: bool) -> String {
    if allowed {
        format!("{}{text}{}", style.render(), style.render_reset())
    } else {
        text.to_string()
    }
}

/// Cut in the middle, with an ellipsis, where two long names sharing a beginning
/// and an end still differ.
pub fn truncate_middle(text: &str, columns: usize) -> String {
    if width(text) <= columns {
        return text.to_string();
    }
    if columns < 3 {
        return truncate(text, columns);
    }
    let room = columns - 1;
    let head_room = room.div_ceil(2);
    let tail_room = room - head_room;
    let mut head = String::new();
    for character in text.chars() {
        let mut next = head.clone();
        next.push(character);
        if width(&next) > head_room {
            break;
        }
        head = next;
    }
    let mut tail = String::new();
    for character in text.chars().rev() {
        let next = format!("{character}{tail}");
        if width(&next) > tail_room {
            break;
        }
        tail = next;
    }
    format!("{head}\u{2026}{tail}")
}

/// Text wrapped between words to `columns`; a word wider than that is broken
/// between graphemes, and nothing is cut. By `textwrap`, first fit, widths as a
/// terminal counts them.
pub fn wrap(text: &str, columns: usize) -> Vec<String> {
    if columns == 0 || width(text) <= columns {
        return vec![text.to_string()];
    }
    // No hyphenation: `farmhouse-steel` is one name, and textwrap otherwise
    // broke it at its hyphen while the column still had room for it whole.
    let options = textwrap::Options::new(columns)
        .break_words(true)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}

/// Cut to fit, with an ellipsis so the reader knows something was removed, and
/// never inside a grapheme cluster.
pub fn truncate(text: &str, columns: usize) -> String {
    if width(text) <= columns {
        return text.to_string();
    }
    if columns == 0 {
        return String::new();
    }
    // One column goes to the ellipsis, so what remains is the room for content.
    let room = columns.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    let mut painted = false;
    let mut rest = text;
    // A colour, `ESC [ … m`, is copied whole and takes no room: counted as
    // text, it widened the cut and could end it inside an escape.
    'cut: while !rest.is_empty() {
        if let Some(body) = rest.strip_prefix("\u{1b}[")
            && let Some(end) = body.find('m')
            && body[..end].chars().all(|c| c.is_ascii_digit() || c == ';')
        {
            out.push_str(&rest[..2 + end + 1]);
            painted = true;
            rest = &body[end + 1..];
            continue;
        }
        let cluster = rest.graphemes(true).next().unwrap_or(rest);
        let cluster_width = width(cluster);
        if used + cluster_width > room {
            break 'cut;
        }
        out.push_str(cluster);
        used += cluster_width;
        rest = &rest[cluster.len()..];
    }
    out.push('…');
    if painted {
        out.push_str("\u{1b}[0m");
    }
    out
}

fn pad(text: &str, columns: usize, alignment: Alignment) -> String {
    let padding = columns.saturating_sub(width(text));
    match alignment {
        Alignment::Left => format!("{text}{}", " ".repeat(padding)),
        _ => format!("{}{text}", " ".repeat(padding)),
    }
}

// ------------------------------------------------------------------ tables

/// One table, laid out.
///
/// Rows must be rectangular. Alignment is decided per column from what it
/// holds: a column of numbers aligns on its decimal point, anything else on
/// the left.
pub fn table(headers: Vec<String>, rows: Vec<Vec<String>>, target: Target, style: Style) -> String {
    table_identified(headers, rows, target, style, 0, false)
}

/// A table whose first `keep` columns are what it is for — a sample, a value
/// and its state — and are never hidden: they wrap inside their words where
/// the width cannot hold them whole, and any other column goes first.
/// `status --width 30` hid the state, which was the answer.
pub fn table_keeping(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: Target,
    style: Style,
    keep: usize,
) -> String {
    draw(
        &lay_out_keeping(headers, rows, target, style, 0, false, keep),
        style,
    )
}

/// The same table, its first column greyed, header and cells: a column the
/// terminal added rather than one asked for.
pub fn table_identified(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: Target,
    style: Style,
    added: usize,
    grey: bool,
) -> String {
    draw(&lay_out(headers, rows, target, style, added, grey), style)
}

/// The layout decision alone, without drawing it: which columns fit, how wide,
/// how aligned, how many were dropped.
///
/// `table` is this followed by [`draw`]. They are separate because the decision
/// is the part that has to be the same everywhere a table is shown, and a
/// surface that draws its own cells still has to make it.
pub fn lay_out(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: Target,
    style: Style,
    added: usize,
    grey: bool,
) -> Layout {
    lay_out_keeping(headers, rows, target, style, added, grey, 1)
}

/// [`lay_out`], its first `keep` columns never hidden.
pub fn lay_out_keeping(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: Target,
    style: Style,
    added: usize,
    grey: bool,
    keep: usize,
) -> Layout {
    // A header bold, its unit in the unit's colour, where a terminal reads it
    // and the palette is on: decided here, where the target is known.
    let headers = if matches!(target, Target::Terminal { .. }) && palette().enabled {
        headers
            .iter()
            .map(|header| {
                if header.contains('\u{1b}') {
                    header.clone()
                } else {
                    painted_header(header)
                }
            })
            .collect()
    } else {
        headers
    };
    // **What the terminal added gives way before what was asked for**: where
    // the width hides a column someone asked for, the added ones go if that
    // shows more of it. A `project` column kept whole beside a truncated,
    // then hidden, asked-for column was a table laid out for the terminal's
    // sake rather than its reader's.
    let laid = lay_out_as_given(
        headers.clone(),
        rows.clone(),
        target,
        style,
        added,
        grey,
        keep,
    );
    if added == 0 || laid.hidden == 0 || headers.len() <= added {
        return laid;
    }
    let bare = lay_out_as_given(
        headers.into_iter().skip(added).collect(),
        rows.into_iter()
            .map(|row| row.into_iter().skip(added).collect())
            .collect(),
        target,
        style,
        0,
        false,
        keep,
    );
    if bare.hidden < laid.hidden {
        bare
    } else {
        laid
    }
}

fn lay_out_as_given(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: Target,
    style: Style,
    added: usize,
    grey: bool,
    keep: usize,
) -> Layout {
    // **Every** column the terminal added is greyed, where colour is on: they
    // are all there because the terminal put them there, and one of them
    // reading as asked-for is the confusion the grey exists to prevent.
    let grey_identity = if grey { added } else { 0 };
    debug_assert!(
        rows.iter().all(|row| row.len() == headers.len()),
        "a table's rows are rectangular"
    );
    let columns = headers.len();
    if columns == 0 {
        return Layout {
            headers,
            cells: rows,
            widths: Vec::new(),
            alignment: Vec::new(),
            hidden: 0,
            greyed: 0,
        };
    }

    // The identity column is the terminal's own, not one that was asked for.
    // Where the width cannot hold it *and* a column that was asked for, it is
    // the one to go: clearing columns from the right keeps the label and hides
    // the data, which renders a table nobody can read — and exits 0 while doing
    // it.
    if added > 0
        && columns > added
        && let Target::Terminal { width: available } = target
    {
        let overhead = match style {
            Style::Boxed => 2 * 3 + 1,
            Style::Plain => 3,
        };
        // A number is never narrowed, so the first asked-for column needs what
        // it holds; the identity needs a label's minimum.
        let asked = rows
            .iter()
            .map(|row| width(&row[added]))
            .chain(std::iter::once(width(&headers[added])))
            .max()
            .unwrap_or(0);
        // What the added columns themselves take, at their narrowest.
        let carried = added * MIN_LABEL + (added.saturating_sub(1)) * 3;
        if available < carried + asked + overhead {
            return lay_out(
                headers.into_iter().skip(added).collect(),
                rows.into_iter()
                    .map(|row| row.into_iter().skip(added).collect())
                    .collect(),
                target,
                style,
                0,
                false,
            );
        }
    }
    let alignment: Vec<Alignment> = (0..columns)
        .map(|at| {
            // A column the terminal added is a name; any other whose cells all
            // look like numbers aligns on the point — the first one too, which
            // `-c radius,name` asked for as a number and got as a name.
            if at < added {
                return Alignment::Left;
            }
            let numeric = rows.iter().all(|row| {
                let cell = &row[at];
                cell == ABSENT || cell.is_empty() || looks_numeric(cell)
            }) && rows.iter().any(|row| looks_numeric(&row[at]));
            // Points line up where the numbers are one quantity's. A column of
            // several — compute's *before* holding `6.8 ± 0.1 %` over `82 %`
            // over `8 cells` — lined up points that mean nothing to each
            // other, and read ragged: its numbers end at its right instead.
            let mut units = rows
                .iter()
                .filter(|row| looks_numeric(&row[at]))
                .map(|row| unit_of(&row[at]));
            let first = units.next();
            if numeric && units.any(|unit| Some(unit) != first) {
                return Alignment::Right;
            }
            if numeric {
                Alignment::Decimal
            } else {
                Alignment::Left
            }
        })
        .collect();

    // Decimal alignment is done by padding the cells themselves, before the
    // column width is measured, so the layout arithmetic stays one rule.
    let mut cells: Vec<Vec<String>> = rows;
    for (at, align) in alignment.iter().enumerate() {
        if *align == Alignment::Decimal {
            align_on_point(&mut cells, at);
        }
    }

    let content: Vec<usize> = (0..columns)
        .map(|at| cells.iter().map(|row| width(&row[at])).max().unwrap_or(0))
        .collect();
    // A text column narrows no further than its longest word, so that its cells
    // wrap between words.
    let words: Vec<usize> = (0..columns)
        .map(|at| {
            cells
                .iter()
                .flat_map(|row| row[at].split(' '))
                .map(width)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut widths: Vec<usize> = headers
        .iter()
        .zip(&content)
        .map(|(header, content)| width(header).max(*content))
        .collect();
    // A header wraps between its words like a cell, and no narrower than its
    // longest word or its unit: `Origina…ravity` and `vo…L]` were read by
    // nobody.
    let header_words: Vec<usize> = headers
        .iter()
        .map(|header| {
            header_tokens(header)
                .iter()
                .map(|token| width(token))
                .max()
                .unwrap_or(0)
        })
        .collect();
    // A terminal may need the table narrowed; a pipe never does, because output
    // truncated for display and then parsed is data loss.
    let mut shown = columns;
    if let Target::Terminal { width: available } = target {
        shown = fit(
            &mut widths,
            &content,
            &words,
            &header_words,
            &alignment,
            available,
            style,
            keep,
        );
    }
    let hidden = columns - shown;
    widths.truncate(shown);
    Layout {
        headers: headers.into_iter().take(shown).collect(),
        cells: cells
            .into_iter()
            .map(|row| row.into_iter().take(shown).collect())
            .collect(),
        widths,
        alignment: alignment.into_iter().take(shown).collect(),
        hidden,
        greyed: grey_identity,
    }
}

/// A laid-out table drawn into text: its borders, its rows, and the closing
/// line naming how many columns the width could not hold.
pub fn draw(layout: &Layout, style: Style) -> String {
    let Layout {
        headers,
        cells,
        widths,
        alignment,
        hidden,
        greyed,
    } = layout;
    if headers.is_empty() {
        return String::new();
    }
    let (widths, alignment, greyed) = (&widths[..], &alignment[..], *greyed);

    let mut out = String::new();
    match style {
        Style::Boxed => {
            out.push_str(&rule(widths, '┌', '┬', '┐'));
            out.push_str(&line(headers, widths, alignment, style, true, greyed));
            out.push_str(&rule(widths, '├', '┼', '┤'));
            for row in cells {
                out.push_str(&line(row, widths, alignment, style, false, greyed));
            }
            out.push_str(&rule(widths, '└', '┴', '┘'));
        }
        Style::Plain => {
            out.push_str(&line(headers, widths, alignment, style, true, greyed));
            let total: usize = widths
                .iter()
                .map(|w| w + 3)
                .sum::<usize>()
                .saturating_sub(3);
            out.push_str(&"─".repeat(total));
            out.push('\n');
            for row in cells {
                out.push_str(&line(row, widths, alignment, style, false, greyed));
            }
        }
    }
    if *hidden > 0 {
        out.push_str(&format!(
            "{} {HIDDEN}\n",
            if *hidden == 1 {
                "1 column".to_string()
            } else {
                format!("{hidden} columns")
            }
        ));
    }
    out
}

/// Whether a cell is a number: a value, then at most an uncertainty after `±`
/// and one unit — and nothing else. The first word alone decided it, and
/// `12 samples, dry-nickel` was a number, aligned on a point it did not have.
fn looks_numeric(cell: &str) -> bool {
    let cell = drawn(cell);
    let cell = cell.as_ref();
    let number = |word: &str| !word.is_empty() && word.parse::<f64>().is_ok();
    // A value that waits is bracketed, `(69 %)`, and is a number still.
    let trimmed = cell.trim();
    let cell = trimmed
        .strip_prefix('(')
        .and_then(|inner| inner.split_once(')'))
        .map_or(trimmed.to_string(), |(inner, mark)| {
            format!("{inner}{mark}")
        });
    let mut words = cell.split_whitespace();
    if !words.next().is_some_and(number) {
        return false;
    }
    let mut rest: Vec<&str> = words.collect();
    // A state's mark closes a value, painted or not: `13.00 g ✎`.
    if rest
        .last()
        .is_some_and(|last| last.contains(['✎', '⚠', '✗']))
    {
        rest.pop();
    }
    if rest.first() == Some(&"±") {
        if !rest.get(1).is_some_and(|word| number(word)) {
            return false;
        }
        rest.drain(..2);
    }
    match rest.as_slice() {
        [] => true,
        // A unit is one word, and none that ends a clause.
        [unit] => !unit.ends_with([',', ';', ':', '.']) && !number(unit),
        _ => false,
    }
}

/// The unit a numeric cell ends with, its state's mark set aside; empty
/// where it has none.
fn unit_of(cell: &str) -> String {
    let cell = drawn(cell);
    let mut words: Vec<&str> = cell.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|last| last.contains(['✎', '⚠', '✗']))
    {
        words.pop();
    }
    match words.as_slice() {
        [_] | [_, "±", _] => String::new(),
        [.., unit] => unit.to_string(),
        [] => String::new(),
    }
}

/// Pads a column's cells so that their decimal points sit in one place: the
/// point **of the value**, the first word. `12 ± 3.5` split at the point of its
/// uncertainty, and stood a column away from `1.5 ± 0.2`. A cell holding no
/// number — the placeholder of an absent value — ends where the column ends,
/// as a number would, rather than standing at the left of a numeric column.
fn align_on_point(rows: &mut [Vec<String>], at: usize) {
    let split = |cell: &str| -> (String, String) {
        let value_end = cell.find(char::is_whitespace).unwrap_or(cell.len());
        let point = cell[..value_end].find('.').unwrap_or(value_end);
        (cell[..point].to_string(), cell[point..].to_string())
    };
    let (mut before, mut after) = (0usize, 0usize);
    for row in rows.iter().filter(|row| looks_numeric(&row[at])) {
        let (head, tail) = split(&row[at]);
        before = before.max(width(&head));
        after = after.max(width(&tail));
    }
    for row in rows.iter_mut() {
        if !looks_numeric(&row[at]) {
            let cell = &row[at];
            row[at] = format!(
                "{}{cell}",
                " ".repeat((before + after).saturating_sub(width(cell)))
            );
            continue;
        }
        let (head, tail) = split(&row[at]);
        row[at] = format!(
            "{}{head}{tail}{}",
            " ".repeat(before.saturating_sub(width(&head))),
            " ".repeat(after.saturating_sub(width(&tail)))
        );
    }
}

/// Takes columns down to fit a window, from the widest first, and never below
/// a floor that keeps a header recognisable.
/// The narrowest a label is cut to, in the middle.
const MIN_LABEL: usize = 3;

/// A table fitted to a terminal without cutting a number or what a cell says :
/// labels longer than their column's content give way first, cut in the middle;
/// then text columns narrow, never below their longest word, their cells
/// wrapping; then columns are hidden from the right, never the first; and a
/// column left still too wide wraps inside its words. How many columns are
/// shown.
#[allow(clippy::too_many_arguments)]
fn fit(
    widths: &mut [usize],
    content: &[usize],
    words: &[usize],
    header_words: &[usize],
    alignment: &[Alignment],
    available: usize,
    style: Style,
    keep: usize,
) -> usize {
    let overhead = |count: usize| match style {
        Style::Boxed => count * 3 + 1,
        Style::Plain => count.saturating_sub(1) * 3,
    };
    let total =
        |widths: &[usize], count: usize| widths[..count].iter().sum::<usize>() + overhead(count);
    let mut shown = widths.len();
    if total(widths, shown) <= available {
        return shown;
    }
    let mut labels: Vec<usize> = (0..widths.len()).collect();
    labels.sort_by_key(|at| std::cmp::Reverse(widths[*at].saturating_sub(content[*at])));
    for at in labels {
        let excess = total(widths, shown).saturating_sub(available);
        if excess == 0 {
            break;
        }
        // A label wraps: it gives way down to its longest word, never cut.
        let floor = content[at].max(header_words[at]).max(MIN_LABEL);
        if widths[at] > floor {
            widths[at] -= (widths[at] - floor).min(excess);
        }
    }
    // Then text columns narrow, the widest first, never below their longest
    // word: their cells wrap between words.
    let texts: Vec<bool> = alignment
        .iter()
        .map(|align| *align == Alignment::Left)
        .collect();
    loop {
        if total(widths, shown) <= available {
            break;
        }
        let Some(widest) = (0..shown)
            .filter(|at| {
                texts[*at] && widths[*at] > words[*at].max(header_words[*at]).max(MIN_LABEL)
            })
            .max_by_key(|at| widths[*at])
        else {
            break;
        };
        widths[widest] -= 1;
    }
    // Before a column asked for is hidden, text wraps inside its words: a
    // name no space breaks — CJK, a long identifier — otherwise hid every
    // column but the first at a width that could show them all.
    if total(widths, shown) > available {
        let mut trial = widths[..shown].to_vec();
        narrow_text(&mut trial, &texts[..shown], available, style);
        // Only a word too long for any sensible column is broken: one that
        // fits in twelve cells — `roughness` — is kept whole, and a column
        // is hidden instead, as a narrow terminal always did.
        let whole = (0..shown).all(|at| !texts[at] || trial[at] >= words[at].min(12));
        // Columns a table is for are not hidden: they wrap inside their words.
        if total(&trial, shown) <= available && (whole || shown <= keep) {
            widths[..shown].copy_from_slice(&trial);
            return shown;
        }
    }
    while shown > keep.max(1) && total(widths, shown) > available {
        shown -= 1;
    }
    if total(widths, shown) > available {
        // A column left too wide wraps inside its words; a number never narrows.
        let mut text = widths[..shown].to_vec();
        narrow_text(&mut text, &texts[..shown], available, style);
        widths[..shown].copy_from_slice(&text);
    }
    shown
}

/// The last resort: the widest text columns shrink, a number never.
fn narrow_text(widths: &mut [usize], texts: &[bool], available: usize, style: Style) {
    let overhead = match style {
        Style::Boxed => widths.len() * 3 + 1,
        Style::Plain => widths.len().saturating_sub(1) * 3,
    };
    loop {
        let total: usize = widths.iter().sum::<usize>() + overhead;
        if total <= available {
            return;
        }
        let Some(widest) = widths
            .iter()
            .enumerate()
            .filter(|(at, w)| texts[*at] && **w > 6)
            .max_by_key(|(_, w)| **w)
            .map(|(at, _)| at)
        else {
            return;
        };
        widths[widest] -= 1;
    }
}

#[allow(dead_code)]
fn narrow(widths: &mut [usize], available: usize, style: Style) {
    let overhead = match style {
        Style::Boxed => widths.len() * 3 + 1,
        Style::Plain => widths.len().saturating_sub(1) * 3,
    };
    loop {
        let total: usize = widths.iter().sum::<usize>() + overhead;
        if total <= available {
            return;
        }
        let Some(widest) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > 6)
            .max_by_key(|(_, w)| **w)
            .map(|(at, _)| at)
        else {
            return;
        };
        widths[widest] -= 1;
    }
}

/// A header's words, a unit in brackets kept as one: `volume [L]` is
/// `volume` and `[L]`, and `[g per L]` is never broken at its spaces.
fn header_tokens(header: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut open = false;
    for word in header.split(' ').filter(|word| !word.is_empty()) {
        if open && let Some(last) = tokens.last_mut() {
            last.push(' ');
            last.push_str(word);
        } else {
            tokens.push(word.to_string());
        }
        if word.contains('[') {
            open = !word.contains(']');
        } else if word.contains(']') {
            open = false;
        }
    }
    tokens
}

/// A header wrapped to its column, word by word, as a cell wraps: a word wider
/// than the column is broken, a unit in brackets never.
fn header_lines(header: &str, columns: usize) -> Vec<String> {
    if width(header) <= columns {
        return vec![header.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for token in header_tokens(header) {
        let joined = if current.is_empty() {
            token.clone()
        } else {
            format!("{current} {token}")
        };
        if width(&joined) <= columns {
            current = joined;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if width(&token) <= columns || token.starts_with('[') {
            current = token;
        } else {
            let mut pieces = wrap(&token, columns);
            current = pieces.pop().unwrap_or_default();
            lines.extend(pieces);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn rule(widths: &[usize], left: char, middle: char, right: char) -> String {
    let mut out = String::new();
    out.push(left);
    for (at, w) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        out.push(if at + 1 == widths.len() {
            right
        } else {
            middle
        });
    }
    out.push('\n');
    out
}

fn line(
    row: &[String],
    widths: &[usize],
    alignment: &[Alignment],
    style: Style,
    header: bool,
    // How many leading columns are the terminal's own, and greyed with it.
    grey_leading: usize,
) -> String {
    // A label is cut in the middle, where two long names still differ; a text
    // cell wraps onto the lines below, and nothing it says is cut.
    let pieces: Vec<Vec<String>> = widths
        .iter()
        .enumerate()
        .map(|(at, w)| {
            let raw = row.get(at).cloned().unwrap_or_default();
            if header {
                header_lines(&raw, *w)
            } else if alignment[at] == Alignment::Left {
                wrap(&raw, *w)
            } else {
                vec![truncate(&raw, *w)]
            }
        })
        .collect();
    let height = pieces.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let mut out = String::new();
    for physical in 0..height {
        let mut text = String::new();
        for (at, w) in widths.iter().enumerate() {
            let shown = pieces[at].get(physical).cloned().unwrap_or_default();
            // A header reads like its column: from the left over text, and
            // ending where the numbers end over a numeric column, where `porosity`
            // hugging the point would look wrong.
            let align = if header {
                match alignment[at] {
                    Alignment::Left => Alignment::Left,
                    Alignment::Right | Alignment::Decimal => Alignment::Right,
                }
            } else {
                alignment[at]
            };
            let mut padded = pad(&shown, *w, align);
            if at < grey_leading {
                // After padding: `width` skips escapes now, but keeping the
                // style outside the padding keeps the two independent.
                padded = painted(&padded, added(), true);
            }
            match style {
                Style::Boxed => text.push_str(&format!("│ {padded} ")),
                Style::Plain => {
                    text.push_str(&padded);
                    if at + 1 < widths.len() {
                        text.push_str("   ");
                    }
                }
            }
        }
        if style == Style::Boxed {
            text.push('│');
        }
        // Trailing spaces are invisible and make a diff noisy.
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------------ a sample

/// One sample shown whole, as the workbench's sample screen shows it: its name;
/// its tags, its attributes and its values, each with its state; then every
/// table unfolded, its index first. The note is left out unless asked for, then
/// written after, as the file holds it. Over a selection, each sample is shown
/// once, one after the other.
#[allow(clippy::too_many_arguments)]
pub fn sheet(
    subject: &Subject,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    precision: Option<&crate::format::schema::PrecisionSchema>,
    target: Target,
    note: bool,
    table_style: Style,
    samples: Option<&[(std::path::PathBuf, Option<String>)]>,
) -> String {
    sheet_of_held(
        subject,
        config,
        style,
        precision,
        target,
        note,
        table_style,
        None,
        samples,
    )
}

/// [`sheet`], leaving out what a model declares and nothing holds: a value
/// with none, and a table with no row, that `held` — the names the sample's
/// file holds — does not name. Python's `to_text` writes the sample as `view`
/// writes its file, which knows nothing of the model. Given the project's
/// `samples`, each its path and its `name:`, a terminal shows the links as a
/// table drawn there does.
#[allow(clippy::too_many_arguments)]
pub fn sheet_of_held(
    subject: &Subject,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    precision: Option<&crate::format::schema::PrecisionSchema>,
    target: Target,
    note: bool,
    table_style: Style,
    held: Option<&[String]>,
    samples: Option<&[(std::path::PathBuf, Option<String>)]>,
) -> String {
    let sample = subject.sample;
    let unheld = |name: &str| held.is_some_and(|held| !held.iter().any(|kept| kept == name));
    let mut out = format!("{}\n", name_of(subject));
    let terminal = matches!(target, Target::Terminal { .. });
    let _pass = crate::format::fingerprint::Pass::begin();
    let states = crate::format::fingerprint::check(sample).unwrap_or_default();
    // Tags first, the attributes that describe the sample, then what was
    // measured and derived.
    let mut fields: Vec<String> = vec!["tags".to_string()];
    fields.extend(
        sample
            .attribute_names()
            .into_iter()
            .map(|name| name.to_string()),
    );
    fields.extend(
        sample
            .property_names()
            .into_iter()
            .filter(|name| {
                !unheld(name.as_str())
                    || sample.property(name).is_ok_and(|handle| {
                        handle
                            .peek(crate::core::property::Property::peek_value)
                            .is_some_and(|value| !value.is_absent())
                    })
            })
            .map(|name| name.to_string()),
    );
    let profile = crate::config::profiles::anonymous(
        fields
            .iter()
            .map(|field| ColumnSpec {
                field: field.clone(),
                label: None,
                header: None,
                precision: precision.cloned(),
                template: None,
            })
            .collect(),
    );
    let label_width = fields.iter().map(|field| width(field)).max().unwrap_or(0);
    let shown: Vec<String> = profile
        .columns()
        .iter()
        .map(|entry| {
            cell(
                entry,
                subject,
                Declaration::Profile(&profile),
                config,
                style,
                true,
            )
        })
        .collect();
    // In a terminal, a link is its text alone, as a table drawn there shows it.
    let linked = match samples {
        Some(samples) if terminal => crate::presentation::links::row(&profile, subject, samples),
        _ => vec![None; shown.len()],
    };
    let shown: Vec<String> = shown
        .into_iter()
        .zip(&linked)
        .map(|(text, link)| link.as_ref().map_or(text, |link| link.text.clone()))
        .collect();
    let value_width = shown.iter().map(|text| width(text)).max().unwrap_or(0);
    // Painted once laid out, on a terminal: a property's number bold and its
    // unit in the unit's colour, an attribute's value in its own.
    let painting = terminal && palette().enabled;
    let shown: Vec<String> = if painting {
        profile
            .columns()
            .iter()
            .zip(&shown)
            .zip(&linked)
            .map(|((entry, text), link)| {
                if let Some(link) = link {
                    let style = match link.leads {
                        crate::presentation::links::Leads::Nowhere => palette().error.underline(),
                        _ => palette().link,
                    };
                    return paint(text, style);
                }
                let property = Identifier::new(&entry.field)
                    .ok()
                    .is_some_and(|name| sample.property(&name).is_ok());
                if property {
                    let unit = presentation(
                        entry,
                        subject,
                        Declaration::Profile(&profile),
                        config,
                        style,
                    )
                    .unit;
                    painted_quantity(text, unit.as_deref())
                } else if matches!(text.as_str(), "" | ABSENT) {
                    text.clone()
                } else {
                    paint(text, palette().attribute)
                }
            })
            .collect()
    } else {
        shown
    };
    for (field, shown) in fields.iter().zip(&shown) {
        let state = Identifier::new(field)
            .ok()
            .and_then(|name| states.get(&name))
            .and_then(said_state);
        let line = match state {
            Some((mark, word)) if terminal => {
                let said = format!("{mark} {word}");
                let said = match Identifier::new(field)
                    .ok()
                    .and_then(|name| states.get(&name))
                    .and_then(style_of)
                {
                    Some(style) if painting => paint(&said, style),
                    _ => said,
                };
                format!(
                    "  {}   {}   {said}",
                    pad(field, label_width, Alignment::Left),
                    pad(shown, value_width, Alignment::Left)
                )
            }
            Some((_, word)) => format!(
                "  {}   {}   {word}",
                pad(field, label_width, Alignment::Left),
                pad(shown, value_width, Alignment::Left)
            ),
            None => format!("  {}   {shown}", pad(field, label_width, Alignment::Left)),
        };
        out.push_str(line.trim_end());
        out.push('\n');
        // A quantity's readings beneath it: a reading typed wrong is seen
        // where its mean is, found otherwise only with `-c name,notch.readings`.
        let readings = Identifier::new(field)
            .ok()
            .and_then(|name| sample.property(&name).ok())
            .and_then(|handle| {
                handle.peek(|property| {
                    property.readings().map(|readings| {
                        readings
                            .as_slice()
                            .iter()
                            .map(|reading| reading.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                })
            });
        if let Some(readings) = readings {
            let readings = if painting {
                paint(&readings, palette().value)
            } else {
                readings
            };
            out.push_str(&format!(
                "  {}   {} {readings}\n",
                pad("", label_width, Alignment::Left),
                if painting {
                    paint("readings", palette().muted)
                } else {
                    "readings".to_string()
                }
            ));
        }
    }
    for name in sample.table_names() {
        if unheld(name.as_str())
            && sample
                .table(name)
                .is_ok_and(|table| table.rows().count() == 0)
        {
            continue;
        }
        if let Some(unfolded) =
            unfolded_table(subject, name, config, style, precision, target, table_style)
        {
            out.push('\n');
            out.push_str(&unfolded);
        }
    }
    // The researcher's text, byte for byte, and only when asked for.
    if note && !sample.note().trim().is_empty() {
        out.push('\n');
        out.push_str(sample.note());
        if !sample.note().ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// One table of a sample unfolded, as `view` shows it: its name and its row
/// count, then its columns headed with their units, its index first. `None`
/// where the sample holds no table of that name.
pub fn unfolded_table(
    subject: &Subject,
    name: &Identifier,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    precision: Option<&crate::format::schema::PrecisionSchema>,
    target: Target,
    table_style: Style,
) -> Option<String> {
    let sample = subject.sample;
    let terminal = matches!(target, Target::Terminal { .. });
    let found = sample.table(name).ok()?;
    // No field of the sample's own: a cell is named whole.
    let profile = crate::config::profiles::anonymous(Vec::new());
    let mut out = String::new();
    let columns: Vec<Identifier> = found.column_names().into_iter().cloned().collect();
    let rows_count = found.rows().count();
    out.push_str(&format!(
        "  {name} · {rows_count} {}\n",
        if rows_count == 1 { "row" } else { "rows" }
    ));
    // Each column headed with its unit, which its cells then leave out.
    let headers: Vec<String> = columns
        .iter()
        .map(|column| {
            // The file's unit, or the one `[property."table.column"]`
            // declares where the file writes none, as `-c` heads it.
            let written = found
                .presentation_of(column, &crate::core::table::RowAddress::ordinal(0))
                .ok()
                .and_then(|presentation| presentation.unit);
            let unit = resolve(
                &formatting::Presentation {
                    unit: written.clone(),
                    ..Default::default()
                },
                &format!("{name}.{column}"),
                config,
                style,
            )
            .unit
            .or(written);
            match unit {
                Some(unit) => format!("{column} [{unit}]"),
                None => column.to_string(),
            }
        })
        .collect();
    let indexes: Vec<Vec<crate::core::value::Value>> = found
        .rows()
        .map(|row| row.index().into_iter().cloned().collect())
        .collect();
    let cell_of = |column: &Identifier, index: &[crate::core::value::Value]| {
        crate::query::field_addressing::describe(&crate::query::field_addressing::Field::Cell {
            table: name.clone(),
            column: column.clone(),
            row: crate::core::table::RowAddress::index(index.to_vec()),
            channel: crate::query::field_addressing::Channel::Value,
        })
    };
    // A column's numbers share their decimals, as a table of samples has them
    // share: where nothing declares a precision, plain numbers padded to the
    // most precise, values with uncertainties written to one number of
    // decimals where one suits them all.
    let mut shared: Vec<Option<usize>> = Vec::new();
    let mut plain: Vec<bool> = Vec::new();
    for column in &columns {
        let mut known = Vec::new();
        let mut numbers = true;
        let mut uncertain = true;
        let mut declared = precision.is_some();
        for index in &indexes {
            let Ok(field) = fields::parse(&cell_of(column, index)) else {
                numbers = false;
                break;
            };
            let entry = ColumnSpec {
                field: cell_of(column, index),
                label: None,
                header: None,
                precision: None,
                template: None,
            };
            declared |= presentation(
                &entry,
                subject,
                Declaration::Profile(&profile),
                config,
                style,
            )
            .precision
            .is_some();
            match (
                read(&field, Channel::Value, subject),
                read(&field, Channel::Uncertainty, subject),
            ) {
                (None | Some(Value::Absent), _) => {}
                (Some(Value::Number(value)), Some(Value::Number(uncertainty))) => {
                    known.push((value, uncertainty));
                }
                (Some(Value::Number(_) | Value::Integer(_)), None) => uncertain = false,
                _ => {
                    numbers = false;
                    uncertain = false;
                }
            }
        }
        let places = (!declared && uncertain)
            .then(|| places_suiting(known))
            .flatten();
        shared.push(places);
        plain.push(!declared && numbers && places.is_none() && !uncertain);
    }
    let mut rows = Vec::new();
    let mut marks = Vec::new();
    for index in &indexes {
        let mut cells = Vec::new();
        let mut marked = Vec::new();
        for (at, column) in columns.iter().enumerate() {
            // The mark in its state's colour, where the palette is on.
            let mark = if terminal {
                crate::format::fingerprint::check_cell_among(sample, name, column, index)
                    .ok()
                    .and_then(|state| {
                        let mark = mark_of(&state)?;
                        Some(match style_of(&state) {
                            Some(style) => paint(mark, style),
                            None => mark.to_string(),
                        })
                    })
            } else {
                None
            };
            // A cell as `-c 'table.column[index]'` shows it, through the
            // same path: its readings' statistic, its uncertainty, and
            // `n/a` as written, never as a cell left empty.
            let entry = ColumnSpec {
                field: cell_of(column, index),
                label: None,
                header: None,
                precision: precision.cloned(),
                template: None,
            };
            // Without its unit, which heads the column.
            let mut shown = render_cell_at(
                &entry,
                subject,
                Declaration::Profile(&profile),
                config,
                style,
                true,
                false,
                shared[at],
            );
            if shown.is_empty() {
                shown = ABSENT.to_string();
            }
            cells.push(shown);
            marked.push(mark);
        }
        rows.push(cells);
        marks.push(marked);
    }
    for (at, even) in plain.iter().enumerate() {
        let cells: Vec<&String> = rows.iter().filter_map(|row| row.get(at)).collect();
        let Some(most) = even.then(|| most_decimals(&cells)).flatten() else {
            continue;
        };
        for (row, index) in rows.iter_mut().zip(&indexes) {
            if row[at].as_str() == ABSENT {
                continue;
            }
            let entry = ColumnSpec {
                field: cell_of(&columns[at], index),
                label: None,
                header: None,
                precision: None,
                template: None,
            };
            row[at] = render_cell_at(
                &entry,
                subject,
                Declaration::Profile(&profile),
                config,
                style,
                true,
                false,
                Some(most),
            );
        }
    }
    for (cells, marked) in rows.iter_mut().zip(marks) {
        for (cell, mark) in cells.iter_mut().zip(marked) {
            if terminal && !matches!(cell.as_str(), "" | ABSENT | "n/a") {
                // A number as a value, a text as an attribute's value is.
                let style = if looks_numeric(cell) {
                    palette().value
                } else {
                    palette().attribute
                };
                *cell = paint(cell, style);
            }
            if let Some(mark) = mark {
                cell.push(' ');
                cell.push_str(&mark);
            }
        }
    }
    let headers: Vec<String> = if terminal && palette().enabled {
        headers
            .iter()
            .map(|header| painted_header(header))
            .collect()
    } else {
        headers
    };
    for line in table(headers, rows, target, table_style).lines() {
        out.push_str(format!("  {line}").trim_end());
        out.push('\n');
    }
    Some(out)
}

/// A value's state as the sample's screen says it: its mark and its word,
/// nothing where it is entered or current.
fn said_state(
    state: &crate::format::fingerprint::Freshness,
) -> Option<(&'static str, &'static str)> {
    use crate::format::fingerprint::Freshness;
    let word = match state {
        Freshness::Source | Freshness::Current => return None,
        Freshness::Stale { .. } => "outdated",
        Freshness::Broken { .. } => "broken",
        Freshness::Edited => "edited",
        Freshness::Failed { .. } => "failed",
        Freshness::RecordMissing => "record missing",
        Freshness::Unjudged { .. } => "unjudged",
    };
    Some((mark_of(state).unwrap_or(" "), word))
}

/// One row of a profile's table: one cell per column, in the profile's order.
///
/// **In column order, not in read order.** The two once disagreed, and a row
/// whose cells are ordered by when they resolved is a table that reads
/// correctly and means something else.
///
/// A quantity is *one* cell here, where an export splits it in two: a terminal
/// has room for `12.50 ± 0.05` and a CSV column has one number in it.
///
/// **The unit is not in the cell.** It is in the header, by [`columns`]: the
/// owner's files write 13 030 units for 50 quantities, and a unit that cannot
/// change down a column belongs where it is written once.
pub fn row(
    profile: &Profile,
    subject: &Subject,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Vec<String> {
    profile
        .columns()
        .iter()
        .map(|column| {
            let shown = render_cell(
                column,
                subject,
                Declaration::Profile(profile),
                config,
                style,
                true,
                false,
            );
            if shown.is_empty() {
                ABSENT.to_string()
            } else {
                shown
            }
        })
        .collect()
}

/// A column's numbers, where nothing says how many digits they have, written
/// to as many decimals as the most precise of them: `2` beside `1.99` is
/// `2.00`, `12.5` beside `12.49` is `12.50`. A column only of numbers without an
/// uncertainty, none of whose cells a precision, a template or a channel
/// writes: a declared precision already writes them alike, and text that looks
/// like a number — a version, `1.10` — is never padded. The digits are added
/// to the text, never rounded again. A column of values with uncertainties is
/// written to one number of decimals where one suits every row:
/// `12.4920 ± 0.0055` above `12.497 ± 0.012` is
/// `12.492 ± 0.006`. `rows` are [`row`]'s, one per subject, each written under
/// its configuration.
pub fn even_decimals(
    profile: &Profile,
    subjects: &[Subject],
    configs: &[Option<&ProjectConfig>],
    style: Option<&str>,
    rows: &mut [Vec<String>],
) {
    for (at, column) in profile.columns().iter().enumerate() {
        let Ok(field) = fields::parse(&column.field) else {
            continue;
        };
        if column.template.is_some() || fields::has_explicit_channel(&column.field) {
            continue;
        }
        if let Some(places) = shared_places(profile, column, &field, subjects, configs, style) {
            for ((row, subject), config) in rows.iter_mut().zip(subjects).zip(configs) {
                let Some(cell) = row.get_mut(at) else {
                    continue;
                };
                if cell.as_str() == ABSENT {
                    continue;
                }
                *cell = render_cell_at(
                    column,
                    subject,
                    Declaration::Profile(profile),
                    *config,
                    style,
                    true,
                    false,
                    Some(places),
                );
            }
            continue;
        }
        let plain = subjects.iter().zip(configs).all(|(subject, config)| {
            let resolved = presentation(
                column,
                subject,
                Declaration::Profile(profile),
                *config,
                style,
            );
            let number = matches!(
                read(&field, Channel::Value, subject),
                None | Some(Value::Number(_) | Value::Integer(_) | Value::Absent)
            );
            resolved.precision.is_none()
                && resolved.property.is_none()
                && number
                && read(&field, Channel::Uncertainty, subject).is_none()
        });
        if !plain {
            continue;
        }
        // Written again to that many decimals, as `.Nf` would: an entered
        // number keeps its digits and gains zeros, and a computed one its own
        // digits, where zeros added to its short form would have been false.
        let cells: Vec<&String> = rows.iter().filter_map(|row| row.get(at)).collect();
        let Some(most) = most_decimals(&cells) else {
            continue;
        };
        for ((row, subject), config) in rows.iter_mut().zip(subjects).zip(configs) {
            let Some(cell) = row.get_mut(at) else {
                continue;
            };
            if cell.as_str() != ABSENT {
                *cell = render_cell_at(
                    column,
                    subject,
                    Declaration::Profile(profile),
                    *config,
                    style,
                    true,
                    false,
                    Some(most),
                );
            }
        }
    }
}

/// The decimals of the most precise of a column's cells, each a number written
/// as text: what the whole column is then written to. `None` where a cell is
/// in exponent form or no number, or none has a decimal: the column is left as
/// it is.
fn most_decimals(cells: &[&String]) -> Option<usize> {
    let mut most = 0;
    for cell in cells {
        if cell.as_str() == ABSENT {
            continue;
        }
        cell.parse::<f64>().ok()?;
        if cell.contains(['e', 'E']) {
            return None;
        }
        most = most.max(
            cell.split_once('.')
                .map_or(0, |(_, decimals)| decimals.len()),
        );
    }
    (most > 0).then_some(most)
}

/// The most decimals that suit every value of a column known to an
/// uncertainty, each keeping one or two significant digits of it, two when
/// the first is a 1. `None` where no number suits them all, or a value is not
/// one: each is then written to its own.
fn places_suiting(values: impl IntoIterator<Item = (f64, f64)>) -> Option<usize> {
    let mut fewest = i32::MIN;
    let mut most = i32::MAX;
    let mut any = false;
    for (value, uncertainty) in values {
        if !value.is_finite() {
            return None;
        }
        // A zero uncertainty has no significant digit to keep: it takes the
        // column's decimals, and chooses none — a curve's first row, `0 ± 0`,
        // unshared the whole column.
        if uncertainty == 0.0 {
            continue;
        }
        let (low, high) = formatting::uncertainty_places(uncertainty)?;
        fewest = fewest.max(low);
        most = most.min(high);
        any = true;
    }
    (any && fewest <= most && most >= 0).then_some(most as usize)
}

/// The decimals a column of values with uncertainties is written to, where
/// nothing is declared: the one number [`even_decimals`] gives the whole
/// column, or else the most any row is written to. `None` for a column that is
/// not only of such values: a summary writes its extremes as it did.
pub fn column_places(
    profile: &Profile,
    column: &ColumnSpec,
    subjects: &[Subject],
    configs: &[Option<&ProjectConfig>],
    style: Option<&str>,
) -> Option<usize> {
    let field = fields::parse(&column.field).ok()?;
    if column.template.is_some() || fields::has_explicit_channel(&column.field) {
        return None;
    }
    if let Some(places) = shared_places(profile, column, &field, subjects, configs, style) {
        return Some(places);
    }
    let mut most: Option<i32> = None;
    for (subject, config) in subjects.iter().zip(configs) {
        let resolved = presentation(
            column,
            subject,
            Declaration::Profile(profile),
            *config,
            style,
        );
        if resolved.precision.is_some() {
            return None;
        }
        match (
            read(&field, Channel::Value, subject),
            read(&field, Channel::Uncertainty, subject),
        ) {
            (None | Some(Value::Absent), _) => continue,
            (Some(Value::Number(_)), Some(Value::Number(uncertainty))) => {
                let (_, high) = formatting::uncertainty_places(uncertainty)?;
                most = Some(most.map_or(high, |most| most.max(high)));
            }
            _ => return None,
        }
    }
    most.filter(|most| *most >= 0).map(|most| most as usize)
}

/// The one number of decimals a column of values with uncertainties can be
/// written to, the most that suits every row: each uncertainty keeps one or
/// two significant digits, and two when the first is a 1. `None` where no
/// number suits them all, or where the column is not only of such values with
/// nothing declared: each row then keeps its own rounding.
fn shared_places(
    profile: &Profile,
    column: &ColumnSpec,
    field: &fields::Field,
    subjects: &[Subject],
    configs: &[Option<&ProjectConfig>],
    style: Option<&str>,
) -> Option<usize> {
    let mut known = Vec::new();
    for (subject, config) in subjects.iter().zip(configs) {
        let resolved = presentation(
            column,
            subject,
            Declaration::Profile(profile),
            *config,
            style,
        );
        if resolved.precision.is_some() {
            return None;
        }
        match (
            read(field, Channel::Value, subject),
            read(field, Channel::Uncertainty, subject),
        ) {
            (None | Some(Value::Absent), _) => continue,
            (Some(Value::Number(value)), Some(Value::Number(uncertainty))) => {
                known.push((value, uncertainty));
            }
            _ => return None,
        }
    }
    places_suiting(known)
}

/// The headers of a profile's table: each column's label, and its unit when
/// every subject that has one agrees.
///
/// **A unit is added only when it cannot be wrong.** Two spellings down one
/// column produce a header with none, and `validation` is what says so out
/// loud — a header claiming `g` over a column holding one `kg` would be the
/// silent wrong answer this project exists to remove.
pub fn columns(
    profile: &Profile,
    subjects: &[Subject],
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Vec<String> {
    profile
        .columns()
        .iter()
        .map(|column| {
            let label = profile.label_for(column).to_string();
            // A style whose form of a property places the unit writes it in
            // each cell, and the header none.
            if style_places_unit(config, style) {
                return label;
            }
            match agreed_unit(column, subjects, profile, &|_| config, style) {
                Some(unit) => format!("{label} [{unit}]"),
                None => label,
            }
        })
        .collect()
}

/// Whether the style in force writes a property's unit itself: its `property`
/// places `{unit}` inside it.
pub fn style_places_unit(config: Option<&ProjectConfig>, style: Option<&str>) -> bool {
    config
        .and_then(|config| config.resolved_as(&Presentation::default(), "", style).ok())
        .is_some_and(|resolved| resolved.property.as_deref().is_some_and(places_unit))
}

/// Whether a form of a property places the unit inside it, `\SI{…}{{unit}}`:
/// one ending on `{unit}`, as `{value} ± {uncertainty} {unit}` does, leaves it
/// to a header that heads a column with it.
fn places_unit(template: &str) -> bool {
    template.contains("{unit}") && !template.trim_end().ends_with("{unit}")
}

/// The unit every subject holding one agrees on for a column, as a header shows
/// it, or none: what an export writes beside the header too.
pub fn column_unit(
    column: &ColumnSpec,
    subjects: &[Subject],
    profile: &Profile,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Option<String> {
    agreed_unit(column, subjects, profile, &|_| config, style)
}

/// [`column_unit`] over samples of several projects, each resolved under its
/// own: `configs` holds each subject's configuration. A unit every project
/// declares alike heads the column whatever order the projects were named in.
pub fn column_unit_each(
    column: &ColumnSpec,
    subjects: &[Subject],
    profile: &Profile,
    configs: &[Option<&ProjectConfig>],
    style: Option<&str>,
) -> Option<String> {
    agreed_unit(
        column,
        subjects,
        profile,
        &|at| configs.get(at).copied().flatten(),
        style,
    )
}

/// Whether a sample holds something for `field` that can be in a unit: one
/// that holds nothing has no unit to disagree with, whatever `[property.*]`
/// would give it, and neither has a text, a date or a truth — `[property.style]
/// unit = "kg"` headed a column of alloy styles `style [kg]`.
fn holds(field: &fields::Field, subject: &Subject) -> bool {
    matches!(
        fields::resolve(field, subject),
        Ok(fields::Resolution::Scalar(Some(ref value)))
            if !matches!(
                value,
                Value::Text(_) | Value::Boolean(_) | Value::Date(_) | Value::DateTime(_)
            )
    )
}

fn agreed_unit<'a>(
    column: &ColumnSpec,
    subjects: &[Subject],
    profile: &Profile,
    config_of: &dyn Fn(usize) -> Option<&'a ProjectConfig>,
    style: Option<&str>,
) -> Option<String> {
    let Ok(field) = fields::parse(&column.field) else {
        return None;
    };
    if fields::has_explicit_channel(&column.field)
        && matches!(
            fields::channel(&field),
            Channel::Unit | Channel::Symbol | Channel::Stat(Statistic::Count)
        )
    {
        return None;
    }
    let mut agreed: Option<String> = None;
    let mut held = false;
    for (at, subject) in subjects
        .iter()
        .enumerate()
        .filter(|(_, subject)| holds(&field, subject))
    {
        held = true;
        let property = presentation_of(&field, subject);
        let composed = profile.presentation_for(column, &property);
        let Some(unit) = resolve(&composed, &address(&field), config_of(at), style).unit else {
            // A value with no unit under a header naming one would read as in
            // that unit: `5` under `ore_mass [kg]`, written in grams for all
            // anyone knows. Only a row with nothing in it is no evidence.
            if matches!(
                fields::resolve(&field, subject),
                Ok(fields::Resolution::Scalar(Some(_)))
            ) {
                return None;
            }
            continue;
        };
        match &agreed {
            None => agreed = Some(unit),
            Some(seen) if *seen == unit => {}
            // Two spellings: the header says nothing rather than something
            // wrong.
            Some(_) => return None,
        }
    }
    // Nothing computed yet: the unit the model declares heads the column, as
    // it will once the values are there.
    if !held && agreed.is_none() {
        return declared_unit(&field, config_of(0)).and_then(|unit| {
            let declared = Presentation {
                unit: Some(unit),
                symbol: None,
                precision: None,
            };
            let composed = profile.presentation_for(column, &declared);
            resolve(&composed, &address(&field), config_of(0), style).unit
        });
    }
    agreed
}

/// The unit the model declares for a field, from its description: none where
/// no model is described, or it declares none.
fn declared_unit(field: &fields::Field, config: Option<&ProjectConfig>) -> Option<String> {
    let description = crate::config::model_runtime::current_description(config?)?;
    match field {
        fields::Field::Named { name, .. } => {
            description.properties.get(name.as_str())?.unit.clone()
        }
        fields::Field::Cell { table, column, .. } => description
            .tables
            .get(table.as_str())?
            .columns
            .iter()
            .find(|described| described.name == column.as_str())?
            .unit
            .clone(),
        _ => None,
    }
}

/// The columns whose samples do not agree on a unit, with the spellings they
/// hold — what a header cannot name without saying something wrong.
///
/// `agreed_unit` answers `None` there, and a header that simply drops its unit
/// prints two incomparable numbers under a word that promises nothing. The
/// caller is the one that can refuse, so this is what it asks.
/// Each column's units over `subjects`, resolved under `config`, as a header
/// would name them: what a selection over several projects compares across
/// their configurations.
pub fn units_of(
    profile: &Profile,
    subjects: &[Subject],
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let mut found = Vec::new();
    for column in profile.columns() {
        let Ok(field) = fields::parse(&column.field) else {
            continue;
        };
        if fields::has_explicit_channel(&column.field)
            && matches!(
                fields::channel(&field),
                Channel::Unit | Channel::Symbol | Channel::Stat(Statistic::Count)
            )
        {
            continue;
        }
        let mut spellings: Vec<String> = Vec::new();
        for subject in subjects.iter().filter(|subject| holds(&field, subject)) {
            let property = presentation_of(&field, subject);
            let composed = profile.presentation_for(column, &property);
            if let Some(unit) = resolve(&composed, &address(&field), config, style).unit
                && !spellings.contains(&unit)
            {
                spellings.push(unit);
            }
        }
        found.push((column.field.clone(), spellings));
    }
    found
}

pub fn disagreeing_units(
    profile: &Profile,
    subjects: &[Subject],
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let mut found = Vec::new();
    for column in profile.columns() {
        let Ok(field) = fields::parse(&column.field) else {
            continue;
        };
        // A column of units, symbols or counts is in none: listing `m.unit`
        // is how a reader finds which files disagree, and it was refused.
        if fields::has_explicit_channel(&column.field)
            && matches!(
                fields::channel(&field),
                Channel::Unit | Channel::Symbol | Channel::Stat(Statistic::Count)
            )
        {
            continue;
        }
        if agreed_unit(column, subjects, profile, &|_| config, style).is_some() {
            continue;
        }
        let mut spellings: Vec<String> = Vec::new();
        for subject in subjects.iter().filter(|subject| holds(&field, subject)) {
            let property = presentation_of(&field, subject);
            let composed = profile.presentation_for(column, &property);
            if let Some(unit) = resolve(&composed, &address(&field), config, style).unit
                && !spellings.contains(&unit)
            {
                spellings.push(unit);
            }
        }
        if spellings.len() > 1 {
            found.push((column.field.clone(), spellings));
        }
    }
    found
}

/// One cell of a view or a profile: the whole quantity when there is room for
/// a pair, and a template's output when one is declared.
pub fn cell(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    quantity: bool,
) -> String {
    render_cell(entry, subject, declaration, config, style, quantity, true)
}

/// The same cell without its unit, which its column's header says.
pub fn bare_cell(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> String {
    render_cell(entry, subject, declaration, config, style, true, false)
}

/// How one column's numbers are presented, resolved once: what the file says,
/// composed with what the column overrides, resolved against the project's
/// declarations and the style in force.
///
/// [`cell`] renders text with this; an export rounds its numbers to the same
/// precision. One resolution, so that a file and a screen can never disagree
/// about how many digits a quantity has — the reason they did is that the
/// export never asked.
pub fn presentation(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Resolved {
    let Ok(field) = fields::parse(&entry.field) else {
        return Resolved::plain(&formatting::Presentation::default());
    };
    let property = presentation_of(&field, subject);
    let composed = declaration.presentation_for(entry, &property);
    resolve(&composed, &address(&field), config, style)
}

/// The text a column's template writes for a subject, as a delimited export
/// writes it: `None` where the column has no template, or the subject no
/// value to fill it with.
pub fn templated(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Option<String> {
    entry.template.as_ref()?;
    let text = render_cell(entry, subject, declaration, config, style, true, true);
    (text != ABSENT).then_some(text)
}

/// `unit` is false for a table cell, whose unit is in the header instead.
fn render_cell(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    quantity: bool,
    unit: bool,
) -> String {
    render_cell_at(
        entry,
        subject,
        declaration,
        config,
        style,
        quantity,
        unit,
        None,
    )
}

/// [`render_cell`], at `places` decimals where given: as a declared `.Nf`
/// writes the cell, through the style's form.
#[allow(clippy::too_many_arguments)]
fn render_cell_at(
    entry: &ColumnSpec,
    subject: &Subject,
    declaration: Declaration<'_>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    quantity: bool,
    unit: bool,
    places: Option<usize>,
) -> String {
    let Ok(field) = fields::parse(&entry.field) else {
        return ABSENT.to_string();
    };
    let mut resolved = presentation(entry, subject, declaration, config, style);
    if let Some(places) = places {
        resolved.precision = Precision::both(&format!(".{places}f")).ok();
    }
    let requested = fields::channel(&field);
    // A template writes `{unit}` where it says: the unit is its to place,
    // not the header's to take — a column's, or the style's form of a
    // property, `\SI{12.1 \pm 0.05}{\gram}`.
    let placed = |template: &Option<String>| template.as_deref().is_some_and(places_unit);
    if !unit
        && requested != Channel::Unit
        && entry.template.is_none()
        && !placed(&resolved.property)
        && !placed(&resolved.unitless)
    {
        resolved.unit = None;
    }
    // A count is a number of readings: the quantity's unit is not its unit, and
    // its precision applies only when the column declares one.
    if requested == Channel::Stat(Statistic::Count) {
        resolved.unit = None;
        if entry.precision.is_none() {
            resolved.precision = None;
        }
    }

    let explicit = fields::has_explicit_channel(&entry.field);
    let value = match requested {
        Channel::Unit => resolved.unit.clone().map(Value::text),
        Channel::Symbol => resolved.symbol.clone().map(Value::text),
        other => read(&field, other, subject),
    };
    let uncertainty = (!explicit && requested == Channel::Value)
        .then(|| read(&field, Channel::Uncertainty, subject))
        .flatten()
        .and_then(|value| match value {
            Value::Number(number) => crate::core::uncertainty::Uncertainty::new(number).ok(),
            _ => None,
        });
    if explicit && requested == Channel::Uncertainty {
        resolved.precision = resolved
            .precision
            .as_ref()
            .and_then(|precision| Precision::both(precision.uncertainty().as_str()).ok());
    }

    // A template replaces the default rendering, and only for the column that
    // declares one. It never alters a stored value: it is given the same
    // resolved literals the default rendering would use.
    // A value absent or not applicable has nothing to fill it with: shown as
    // any cell without one is, not as `\num{ \pm }`.
    if let Some(template) = &entry.template {
        return match &value {
            None | Some(Value::Absent) => ABSENT.to_string(),
            Some(Value::NotApplicable) => "n/a".to_string(),
            Some(_) => fill(template, value.as_ref(), uncertainty.as_ref(), &resolved),
        };
    }
    let Some(value) = value else {
        return ABSENT.to_string();
    };
    // At the declared precision, as an export and a template write it : a
    // screen shows what a file holds.
    if quantity && !explicit {
        return formatting::format_quantity(&value, uncertainty.as_ref(), &resolved);
    }
    match (&value, requested) {
        // A stored uncertainty is never negative, so `new` accepts every one a
        // sample can hold.
        (Value::Number(number), channel) if fields::is_spread(channel) => {
            crate::core::uncertainty::Uncertainty::new(*number)
                .map(|stored| formatting::format_uncertainty(&stored, &resolved))
                .unwrap_or_else(|_| formatting::format_value(&value, &resolved))
        }
        _ => formatting::format_value(&value, &resolved),
    }
}

/// One quantity alone, with its unit beside it: a property printed rather than
/// placed in a table. Its presentation is composed with `precision` as a
/// column's precision composes, and resolved for `address` in the project's
/// style. An absent value is the empty string; the caller picks a placeholder.
pub fn quantity(
    value: &Value,
    uncertainty: Option<&Uncertainty>,
    presentation: &Presentation,
    address: &str,
    config: Option<&ProjectConfig>,
    precision: Option<&Precision>,
) -> String {
    let composed = Presentation {
        precision: precision
            .cloned()
            .or_else(|| presentation.precision.clone()),
        ..presentation.clone()
    };
    quantity_in(value, uncertainty, &composed, address, config, None)
}

/// [`quantity`] in a style named, `[render] style`'s where none is: its units,
/// its symbols, its form of a property.
pub fn quantity_in(
    value: &Value,
    uncertainty: Option<&Uncertainty>,
    presentation: &Presentation,
    address: &str,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> String {
    let resolved = resolve(presentation, address, config, style);
    formatting::format_quantity(value, uncertainty, &resolved)
}

/// `{value:.3f} ± {uncertainty:.1f} {unit}` and the same names bare.
///
/// **A brace that does not open a known name is text.** That is what makes
/// `\num{{value:.2f}}` mean *a LaTeX brace, then a placeholder* rather than
/// *a placeholder called `{value`*: the outer `{` fails to name anything, so it
/// is emitted and the scan resumes one character later, where `{value:.2f}`
/// succeeds. A rule based on recognising the name rather than on counting
/// braces also means a template full of LaTeX survives untouched.
fn fill(
    template: &str,
    value: Option<&Value>,
    uncertainty: Option<&crate::core::uncertainty::Uncertainty>,
    resolved: &Resolved,
) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let placeholder = &after[..close];
        // Not a name this module knows: emit the brace and resume one
        // character later, which is what makes a LaTeX brace survive.
        let named = placeholder.split(':').next().unwrap_or(placeholder);
        if !matches!(named, "value" | "uncertainty" | "u" | "unit" | "symbol") {
            out.push('{');
            rest = after;
            continue;
        }
        let (name, spec) = match placeholder.split_once(':') {
            Some((name, spec)) => (name, Some(spec)),
            None => (placeholder, None),
        };
        let local = match spec {
            Some(spec) => Resolved {
                unit: resolved.unit.clone(),
                symbol: resolved.symbol.clone(),
                property: resolved.property.clone(),
                uncertainty: resolved.uncertainty.clone(),
                unitless: resolved.unitless.clone(),
                unit_alone: resolved.unit_alone.clone(),
                unit_in_form: resolved.unit_in_form.clone(),
                precision: crate::core::formatting::Precision::both(spec).ok(),
            },
            None => resolved.clone(),
        };
        match name {
            "value" => {
                if let Some(value) = value {
                    out.push_str(&formatting::format_value(value, &local));
                }
            }
            // An uncertainty through the uncertainty's specifier, rounded up.
            "uncertainty" | "u" => {
                if let Some(uncertainty) = uncertainty {
                    out.push_str(&formatting::format_uncertainty(uncertainty, &local));
                }
            }
            // The resolved literals, so a template in a math-styled project
            // gets `\Omega` without asking for it.
            "unit" => out.push_str(resolved.unit.as_deref().unwrap_or_default()),
            "symbol" => out.push_str(resolved.symbol.as_deref().unwrap_or_default()),
            // Unreachable: an unknown name was emitted as a brace above.
            _ => out.push_str(&format!("{{{placeholder}}}")),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

// ----------------------------------------------------------------- helpers

fn read(field: &Field, channel: Channel, subject: &Subject) -> Option<Value> {
    match fields::resolve(&fields::with_channel(field, channel), subject) {
        Ok(Resolution::Scalar(value)) => value,
        // A tag set is not a scalar and still has to appear in a cell someone
        // asked for: joined by a space, as `exports` joins it, so one thing has
        // one spelling. A dash here would be the column of dashes this project
        // exists to remove.
        Ok(Resolution::Tags(tags)) if channel == Channel::Value && !tags.is_empty() => {
            let joined: Vec<String> = tags.iter().map(|tag| tag.to_string()).collect();
            Some(Value::text(joined.join(" ")))
        }
        // A list attribute, or a quantity's readings, shows its items, never a
        // dash that reads as absent.
        Ok(Resolution::List(values))
            if matches!(channel, Channel::Value | Channel::Readings) && !values.is_empty() =>
        {
            let items: Vec<String> = values.iter().map(item_text).collect();
            Some(Value::text(items.join(", ")))
        }
        _ => None,
    }
}

fn item_text(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Boolean(flag) => flag.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        Value::Absent => String::new(),
        Value::NotApplicable => "n/a".to_string(),
    }
}

fn presentation_of(field: &Field, subject: &Subject) -> crate::core::formatting::Presentation {
    // A table column carries its own unit and precision.
    if let Field::Cell { table, column, .. } = field {
        return subject
            .sample
            .table(table)
            .ok()
            .and_then(|held| held.column(column).ok())
            .map(|view| view.presentation().clone())
            .unwrap_or_default();
    }
    let Field::Named { name, .. } = field else {
        return crate::core::formatting::Presentation::default();
    };
    subject
        .sample
        .property(name)
        .ok()
        .map(|handle| handle.with(|property| property.presentation().clone()))
        .unwrap_or_default()
}

/// A project's style turns the stored strings into literals, and
/// `project_config` is the **only** place that happens. Without a project, the
/// strings are used as written and `±` is the separator.
///
/// A style naming nothing declared is an error there. Here it is a rendering:
/// the stored strings, plus a warning would be a lie in a table cell, so the
/// declaration's own message reaches the caller through `validate` and a cell
/// falls back to what the file says rather than to a guess.
fn resolve(
    presentation: &crate::core::formatting::Presentation,
    quantity: &str,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Resolved {
    match config {
        None => Resolved::plain(presentation),
        Some(config) => config
            .resolved_as(presentation, quantity, style)
            .unwrap_or_else(|_| Resolved::plain(presentation)),
    }
}

/// The address of a field: the key `[property.*]` is written with.
fn address(field: &Field) -> String {
    match field {
        Field::Named { name, .. } => name.to_string(),
        Field::Cell { table, column, .. } => format!("{table}.{column}"),
        Field::ListItem { name, .. } => name.to_string(),
        Field::Reserved(_) => String::new(),
    }
}

pub fn name_of(subject: &Subject) -> String {
    if let Some(name) = subject.sample.name() {
        return name.to_string();
    }
    subject
        .path
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "—".to_string())
}

/// A computation's progress line: a bar filled to `fraction`, the text, and the
/// time elapsed. It is cut to leave the terminal's last column free: a line that
/// reaches it wraps on some terminals, and a wrapped line cannot be redrawn in
/// place.
pub fn progress_line(fraction: f64, text: &str, elapsed: &str, columns: usize) -> String {
    const BAR: usize = 20;
    let filled = ((fraction.clamp(0.0, 1.0) * BAR as f64).floor() as usize).min(BAR);
    let head = format!("[{}{}]", "█".repeat(filled), "░".repeat(BAR - filled));
    let tail = format!(" · {elapsed}");
    let room = columns.saturating_sub(1);
    let fixed = width(&head) + width(&tail) + 1;
    let line = if text.is_empty() || room <= fixed + 1 {
        format!("{head}{tail}")
    } else {
        format!("{head} {}{tail}", cut_end(text, room - fixed))
    };
    cut_end(&line, room)
}

/// Text cut to `room` columns, ending with `…` when it was cut.
fn cut_end(text: &str, room: usize) -> String {
    if width(text) <= room {
        return text.to_string();
    }
    if room == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let taken = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + taken + 1 > room {
            break;
        }
        out.push(c);
        used += taken;
    }
    out.push('…');
    out
}
