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
        ["plain", "boxed"]
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
    let colour = match state {
        Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Unjudged { .. } => {
            anstyle::AnsiColor::Yellow
        }
        Freshness::Failed { .. } => anstyle::AnsiColor::Red,
        Freshness::Edited | Freshness::RecordMissing => anstyle::AnsiColor::Cyan,
        Freshness::Source | Freshness::Current => return None,
    };
    Some(anstyle::Style::new().fg_color(Some(colour.into())))
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
    // No hyphenation: `farmhouse-saison` is one name, and textwrap otherwise
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
    for cluster in text.graphemes(true) {
        let cluster_width = width(cluster);
        if used + cluster_width > room {
            break;
        }
        out.push_str(cluster);
        used += cluster_width;
    }
    out.push('…');
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
            // `-c og,name` asked for as a number and got as a name.
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
/// `12 samples, dry-stout` was a number, aligned on a point it did not have.
fn looks_numeric(cell: &str) -> bool {
    let number = |word: &str| !word.is_empty() && word.parse::<f64>().is_ok();
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
fn unit_of(cell: &str) -> &str {
    let mut words: Vec<&str> = cell.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|last| last.contains(['✎', '⚠', '✗']))
    {
        words.pop();
    }
    match words.as_slice() {
        [_] | [_, "±", _] => "",
        [.., unit] => unit,
        [] => "",
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
        // fits in twelve cells — `haze` — is kept whole, and a column
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
            // ending where the numbers end over a numeric column, where `brix`
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
pub fn sheet(
    subject: &Subject,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    precision: Option<&crate::format::schema::PrecisionSchema>,
    target: Target,
    note: bool,
) -> String {
    let sample = subject.sample;
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
    let value_width = shown.iter().map(|text| width(text)).max().unwrap_or(0);
    for (field, shown) in fields.iter().zip(&shown) {
        let state = Identifier::new(field)
            .ok()
            .and_then(|name| states.get(&name))
            .and_then(said_state);
        let line = match state {
            Some((mark, word)) if terminal => format!(
                "  {}   {}   {mark} {word}",
                pad(field, label_width, Alignment::Left),
                pad(shown, value_width, Alignment::Left)
            ),
            Some((_, word)) => format!(
                "  {}   {}   {word}",
                pad(field, label_width, Alignment::Left),
                pad(shown, value_width, Alignment::Left)
            ),
            None => format!("  {}   {shown}", pad(field, label_width, Alignment::Left)),
        };
        out.push_str(line.trim_end());
        out.push('\n');
    }
    for name in sample.table_names() {
        let Ok(found) = sample.table(name) else {
            continue;
        };
        let columns: Vec<Identifier> = found.column_names().into_iter().cloned().collect();
        let rows_count = found.rows().count();
        out.push_str(&format!(
            "\n  {name} · {rows_count} {}\n",
            if rows_count == 1 { "row" } else { "rows" }
        ));
        // Each column headed with its unit, which its cells then leave out.
        let headers: Vec<String> = columns
            .iter()
            .map(|column| {
                let unit = found
                    .presentation_of(column, &crate::core::table::RowAddress::ordinal(0))
                    .ok()
                    .and_then(|presentation| presentation.unit)
                    .map(|unit| {
                        resolve(
                            &formatting::Presentation {
                                unit: Some(unit.clone()),
                                ..Default::default()
                            },
                            &format!("{name}.{column}"),
                            config,
                            style,
                        )
                        .unit
                        .unwrap_or(unit)
                    });
                match unit {
                    Some(unit) => format!("{column} [{unit}]"),
                    None => column.to_string(),
                }
            })
            .collect();
        let mut rows = Vec::new();
        for row in found.rows() {
            let index: Vec<crate::core::value::Value> = row.index().into_iter().cloned().collect();
            let mut cells = Vec::new();
            for column in &columns {
                let mark = if terminal {
                    crate::format::fingerprint::check_cell_among(sample, name, column, &index)
                        .ok()
                        .as_ref()
                        .and_then(mark_of)
                } else {
                    None
                };
                let shown = match row.cell(column) {
                    Ok(property) => property
                        .value()
                        .ok()
                        .filter(|value| !value.is_absent())
                        .map(|value| {
                            let mut resolved = resolve(
                                property.presentation(),
                                &format!("{name}.{column}"),
                                config,
                                style,
                            );
                            resolved.unit = None;
                            if let Some(precision) = precision.and_then(|given| given.precision()) {
                                resolved.precision = Some(precision);
                            }
                            formatting::format_value(&value, &resolved)
                        })
                        .unwrap_or_else(|| ABSENT.to_string()),
                    Err(_) => ABSENT.to_string(),
                };
                cells.push(match mark {
                    Some(mark) => format!("{shown} {mark}"),
                    None => shown,
                });
            }
            rows.push(cells);
        }
        for line in table(headers, rows, target, Style::Plain).lines() {
            out.push_str(format!("  {line}").trim_end());
            out.push('\n');
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
            match agreed_unit(column, subjects, profile, config, style) {
                Some(unit) => format!("{label} [{unit}]"),
                None => label,
            }
        })
        .collect()
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
    agreed_unit(column, subjects, profile, config, style)
}

fn agreed_unit(
    column: &ColumnSpec,
    subjects: &[Subject],
    profile: &Profile,
    config: Option<&ProjectConfig>,
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
    for subject in subjects {
        let property = presentation_of(&field, subject);
        let composed = profile.presentation_for(column, &property);
        let Some(unit) = resolve(&composed, &address(&field), config, style).unit else {
            // A value with no unit under a header naming one would read as in
            // that unit: `5` under `grain_mass [kg]`, written in grams for all
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
    agreed
}

/// The columns whose samples do not agree on a unit, with the spellings they
/// hold — what a header cannot name without saying something wrong.
///
/// `agreed_unit` answers `None` there, and a header that simply drops its unit
/// prints two incomparable numbers under a word that promises nothing. The
/// caller is the one that can refuse, so this is what it asks.
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
        if agreed_unit(column, subjects, profile, config, style).is_some() {
            continue;
        }
        let mut spellings: Vec<String> = Vec::new();
        for subject in subjects {
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
    let Ok(field) = fields::parse(&entry.field) else {
        return ABSENT.to_string();
    };
    let mut resolved = presentation(entry, subject, declaration, config, style);
    let requested = fields::channel(&field);
    if !unit && requested != Channel::Unit {
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
    if let Some(template) = &entry.template {
        return fill(template, value.as_ref(), uncertainty.as_ref(), &resolved);
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
        (Value::Number(number), Channel::Uncertainty) => {
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
    let resolved = resolve(&composed, address, config, None);
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
                separator: resolved.separator.clone(),
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
            "uncertainty" | "u" => {
                if let Some(uncertainty) = uncertainty
                    && let Ok(magnitude) = Value::number(uncertainty.magnitude())
                {
                    out.push_str(&formatting::format_value(&magnitude, &local));
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

fn name_of(subject: &Subject) -> String {
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
