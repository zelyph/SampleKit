//! Draws the workbench. Everything it shows is the model's; nothing here
//! decides anything.

use crate::config::project_setup;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table,
    TableState, Wrap,
};

use super::model::{
    Action, BINDINGS, EntryKind, Mode, NoteView, Place, Screen, Workbench, hinted, key_name, short,
};
use super::theme::{colors, is_key, key_style, keyed, selection_style};
use super::typing::{Typed, Written};
use crate::collection::editing;
use crate::config::profiles;
use crate::config::project_config::ColumnSpec;
use crate::core::identifier::Identifier;
use crate::format::fingerprint;
use crate::presentation::terminal_rendering as render;
use crate::query::field_addressing::Subject;
use rat_markdown::styles::MDStyle;
use rat_text::text_area::TextArea;
use rat_text::text_input::TextInput;

pub fn draw(frame: &mut Frame, workbench: &mut Workbench) {
    super::theme::set(workbench.theme);
    // A short terminal gives its rows to the samples: the title, the status
    // and the hints go first, `?` still there — at six rows the collection
    // drew its header and no sample.
    let short = frame.area().height < 10;
    let chrome = u16::from(!short);
    // The keys at the foot, on a second line where one cannot hold them, the
    // main frame a line shorter: cut to one, they lost their end.
    let room = frame.area().width.saturating_sub(2) as usize;
    let foot = if matches!(workbench.mode, Mode::Note { .. }) {
        wrap_hints(NOTE_KEYS, room.saturating_sub(1), 2)
            .into_iter()
            .map(|line| format!(" {line}"))
            .collect()
    } else {
        foot_lines(
            workbench.place(),
            !workbench.filter.is_empty(),
            workbench.from_start,
            room,
        )
    };
    // Past the setup's last question Enter sets the project up, and the foot
    // says so as the screen does.
    let setting_up = workbench
        .setup
        .as_ref()
        .is_some_and(|setup| setup.question >= setup.questions(&workbench.root).len());
    let foot: Vec<String> = if matches!(workbench.screen, Screen::Setup { .. }) && setting_up {
        foot.into_iter()
            .map(|line| line.replace("Enter answer", "Enter set up"))
            .collect()
    } else {
        foot
    };
    let foot_rows = if short { 0 } else { foot.len() as u16 };
    let [top, main, status, hints] = Layout::vertical([
        Constraint::Length(chrome),
        Constraint::Min(3),
        Constraint::Length(chrome),
        Constraint::Length(foot_rows),
    ])
    .horizontal_margin(1)
    .areas(frame.area());
    workbench.page = (main.height.saturating_sub(3) as usize).max(1);
    MAIN_AREA.with(|cell| cell.set(main));

    // What the title says after the name: where the workbench is.
    let place = match workbench.screen {
        Screen::Collection => workbench.root.display().to_string(),
        Screen::Control { .. } => format!("{} · its state", workbench.root.display()),
        Screen::History { sample, .. } => {
            let named = sample
                .and_then(|at| workbench.view.get(at))
                .and_then(|entry| entry.sample.borrow().name().map(str::to_string));
            match named {
                Some(name) => format!("{name} · its history"),
                None => format!("{} · its history", workbench.root.display()),
            }
        }
        Screen::Setup { .. } => format!("{} · setting it up", workbench.root.display()),
        Screen::Configure => workbench
            .workspace
            .as_ref()
            .map(|workspace| workspace.edit.path().display().to_string())
            .unwrap_or_default(),
        Screen::Sample { .. } | Screen::Table { .. } => match workbench.current() {
            Some((path, sample)) => sample
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| path.display().to_string()),
            None => String::new(),
        },
    };
    // On the terminal's own background, the name in the accent colour: a
    // reversed bar hid the computation drawn on it.
    let theme = colors();
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "SampleKit",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
    ];
    let run = run_said(workbench);
    if !place.is_empty() {
        // Where it is, cut from its start to leave the computation its room
        // and a separator before it: the two ran into each other,
        // `/tmp/…-Doc⟳ [░░░]`.
        let taken = run.as_ref().map_or(0, |(said, _)| said.chars().count() + 4);
        let room = (top.width as usize).saturating_sub(" SampleKit · ".chars().count() + taken);
        let shown = ellipsized_start(&place, room);
        let cut = shown != place;
        spans.push(Span::styled(
            format!(" · {shown}"),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        if cut && run.is_some() {
            spans.push(Span::styled(" ·", Style::new().fg(theme.muted)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), top);
    // The computation, at the right of the title bar, whatever the screen:
    // its progress in values, then its outcome, until the next run.
    if let Some((said, finished)) = run {
        let colour = match (finished, said.contains("failed")) {
            (false, _) => theme.accent,
            (true, false) => theme.current,
            (true, true) => theme.failed,
        };
        frame.render_widget(
            Paragraph::new(
                Line::from(Span::styled(
                    format!("{said} "),
                    Style::new().fg(colour).add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            ),
            top,
        );
    }
    match workbench.screen {
        Screen::Collection => {
            let furthest = collection(frame, workbench, main);
            workbench.hscroll = workbench.hscroll.min(furthest);
        }
        Screen::Sample { .. } => {
            NOTE_SHOWN.with(|shown| shown.set(None));
            NOTE_EDITED.with(|edited| edited.set(None));
            workbench.note_view = sample(frame, workbench, main);
            if let (Some(area), Mode::Note { text, .. }) =
                (NOTE_EDITED.with(std::cell::Cell::get), &mut workbench.mode)
            {
                workbench.note_view = Some(written(frame, area, text));
            }
            let shown = NOTE_SHOWN.with(std::cell::Cell::get);
            workbench.note_area = shown.map(|(area, _)| area);
            if let Some((_, scroll)) = shown {
                workbench.note_scroll.1 = scroll;
            }
        }
        Screen::Table { .. } => table(frame, workbench, main),
        Screen::Control { .. } => control(frame, workbench, main),
        // Scrolled no further than what it changed shows: held down past
        // it, going back up waited through the rows scrolled for nothing.
        Screen::History { .. } => {
            let furthest = history(frame, workbench, main);
            if let Screen::History { scroll, .. } = &mut workbench.screen {
                *scroll = (*scroll).min(furthest);
            }
        }
        Screen::Setup { .. } => setup(frame, workbench, main),
        Screen::Configure => configure(frame, workbench, main),
    }
    frame.render_widget(Paragraph::new(status_line(workbench)), status);
    // The note being edited has keys of its own, which the foot says: the
    // sample's said what its keys no longer did while it was typed in.
    let foot: Vec<Line<'static>> = foot.iter().map(|line| hint_spans(line)).collect();
    frame.render_widget(Paragraph::new(foot), hints);
    // A window's scroll kept within what it shows, as the history's is.
    if let Some(scroll) = overlay(frame, workbench) {
        match &mut workbench.mode {
            Mode::Panel { scroll: held, .. } => *held = scroll,
            _ => workbench.confirm_scroll = scroll,
        }
    }
    // The help scrolled no further than its last row.
    if let (Mode::Help { scroll }, Some(furthest)) = (
        &mut workbench.mode,
        HELP_FURTHEST.with(std::cell::Cell::get),
    ) {
        *scroll = (*scroll).min(furthest);
    }
}

thread_local! {
    /// How far the help drawn last may scroll, which `draw` keeps it within.
    static HELP_FURTHEST: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// The screen's main frame, drawn last, which the help lies within: laid
    /// on the whole screen two cells in, its borders ran one cell inside the
    /// frame's, each edge drawn twice.
    static MAIN_AREA: std::cell::Cell<Rect> = const { std::cell::Cell::new(Rect::ZERO) };
    /// The last window's lines, where they are on the screen and how far
    /// they are scrolled.
    static MODAL_SHOWN: std::cell::Cell<(Rect, usize)> =
        const { std::cell::Cell::new((Rect::ZERO, 0)) };
}

/// Where the window's line `row` is on the screen, `from` columns in: none
/// where it is scrolled out of sight.
fn modal_row(row: usize, from: u16) -> Option<Rect> {
    let (inner, scroll) = MODAL_SHOWN.with(std::cell::Cell::get);
    let shown = row.checked_sub(scroll)?;
    (shown < inner.height as usize && from < inner.width).then_some(Rect {
        x: inner.x + from,
        y: inner.y + shown as u16,
        width: inner.width - from,
        height: 1,
    })
}

/// The lines typed in the window open, in the order the view places them.
fn typed_lines(mode: &mut Mode) -> Vec<&mut Typed> {
    match mode {
        Mode::Filter { text, .. } | Mode::Prompt { text, .. } => vec![text],
        Mode::Picker { query, typing, .. } if *typing || !query.is_empty() => vec![query],
        Mode::Quantity { slots, .. } => slots.iter_mut().map(|slot| &mut slot.text).collect(),
        _ => Vec::new(),
    }
}

/// The keys of the screen, by section: two columns where the terminal has
/// the width, the sections kept whole and in order, each description wrapped
/// under itself.
fn help_window(frame: &mut Frame, workbench: &Workbench, scroll: usize) {
    let theme = colors();
    let main = MAIN_AREA.with(std::cell::Cell::get);
    let area = if main.width > 0 { main } else { frame.area() };
    let width = area.width.min(124);
    let inner = width.saturating_sub(4) as usize;
    let columns = if inner >= 88 { 2 } else { 1 };
    let column_width = if columns == 2 { (inner - 3) / 2 } else { inner };
    let sections = workbench.help_sections();
    let keys_width = sections
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|(keys, _)| keys.chars().count()))
        .max()
        .unwrap_or(4)
        .min(14);
    let said_width = column_width.saturating_sub(keys_width + 2).max(10);
    // Each section as the lines it takes: its title, then a line per key,
    // a long description wrapped beneath itself.
    let blocks: Vec<Vec<Line<'static>>> = sections
        .iter()
        .map(|(title, rows)| {
            let mut lines = vec![Line::styled(
                title.to_string(),
                Style::new().fg(theme.table).add_modifier(Modifier::BOLD),
            )];
            for (keys, said) in rows {
                // What it does may name another key: *u gives it back*,
                // read whole, a key and its verb on two lines kept one.
                for (at, part) in keyed_lines(said, said_width, Style::new())
                    .into_iter()
                    .enumerate()
                {
                    let shown_keys = if at == 0 {
                        format!("{keys:<keys_width$}  ")
                    } else {
                        " ".repeat(keys_width + 2)
                    };
                    let mut spans = vec![Span::styled(shown_keys, key_style(&theme))];
                    spans.extend(part);
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::raw(""));
            lines
        })
        .collect();
    let total: usize = blocks.iter().map(Vec::len).sum();
    // The sections in order, the left column taking them until it holds half.
    let (mut left, mut right): (Vec<Line>, Vec<Line>) = (Vec::new(), Vec::new());
    for block in blocks {
        if columns == 1 || left.len() + block.len() / 2 <= total / 2 || left.is_empty() {
            left.extend(block);
        } else {
            right.extend(block);
        }
    }
    let lines: Vec<Line> = if columns == 1 {
        left
    } else {
        (0..left.len().max(right.len()))
            .map(|at| {
                let mut spans = left.get(at).cloned().unwrap_or_default().spans;
                let used: usize = spans.iter().map(|span| span.width()).sum();
                spans.push(Span::raw(" ".repeat(column_width.saturating_sub(used) + 3)));
                spans.extend(right.get(at).cloned().unwrap_or_default().spans);
                Line::from(spans)
            })
            .collect()
    };
    // As tall as the frame only where it is as wide, laid on it; narrower,
    // inside its borders, never on them.
    let tallest = if width < area.width {
        area.height.saturating_sub(2)
    } else {
        area.height
    };
    let height = (lines.len() + 2).min(tallest as usize) as u16;
    let shown_rows = height.saturating_sub(2) as usize;
    let furthest = lines.len().saturating_sub(shown_rows);
    HELP_FURTHEST.with(|cell| cell.set(Some(furthest)));
    let at = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, at);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll.min(furthest) as u16, 0))
            .block(
                framed()
                    .title(Line::from(" keys ").style(Style::new().add_modifier(Modifier::BOLD)))
                    .title_bottom({
                        let mut line = hint_spans(if furthest > 0 {
                            "j k scroll · any other key closes"
                        } else {
                            "any key closes"
                        });
                        line.spans.push(Span::raw(" "));
                        line.right_aligned()
                    }),
            ),
        at,
    );
}

/// `text` wrapped to `width` columns, each of its lines `indent` spaces in —
/// the first as the others — its spaces kept.
fn hanging(text: &str, indent: usize, width: usize, style: Style) -> Vec<Line<'static>> {
    let options = textwrap::Options::new(width.saturating_sub(indent).max(4))
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|part| {
            Line::from(vec![
                Span::styled(" ".repeat(indent), style),
                Span::styled(part.into_owned(), style),
            ])
        })
        .collect()
}

/// A sentence wrapped to `width` as `wrapped` wraps it, each key it names
/// coloured as `keyed` finds them in the whole sentence: a key at a line's end
/// and its verb on the next are still one.
fn keyed_lines(text: &str, width: usize, base: Style) -> Vec<Vec<Span<'static>>> {
    let parts = wrapped(text, width);
    let whole: Vec<(char, Style)> = keyed(&parts.join(" "), base)
        .iter()
        .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
        .collect();
    let mut at = 0;
    parts
        .iter()
        .map(|part| {
            let count = part.chars().count();
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (character, style) in whole.iter().skip(at).take(count) {
                match spans.last_mut() {
                    Some(last) if last.style == *style => last.content.to_mut().push(*character),
                    _ => spans.push(Span::styled(character.to_string(), *style)),
                }
            }
            // The space the line was broken at.
            at += count + 1;
            spans
        })
        .collect()
}

/// `text` cut at spaces into lines no wider than `width`; a word longer than
/// the line is a line of its own. By `textwrap`.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let options = textwrap::Options::new(width.max(1))
        .break_words(false)
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    textwrap::wrap(&text, options)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}

/// The computation as the title bar says it: `⟳ [███░░░] 3/6 · B-01 · abv`
/// under way, then `✓ 12 computed, 1 failed`; and whether it is over.
fn run_said(workbench: &Workbench) -> Option<(String, bool)> {
    let run = workbench.run.as_ref()?;
    Some(match &run.outcome {
        Some(outcome) => (format!("✓ {outcome}"), true),
        None => {
            let width = 12;
            let filled = (run.done * width)
                .checked_div(run.total)
                .unwrap_or(0)
                .min(width);
            let said = format!(
                "⟳ [{}{}] {}/{}{}",
                "█".repeat(filled),
                "░".repeat(width - filled),
                run.done,
                run.total,
                if run.now.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", run.now)
                }
            );
            (said, false)
        }
    })
}

fn status_line(workbench: &Workbench) -> Line<'static> {
    let mut parts = vec![format!(
        "{} of {} {}",
        workbench.view.len(),
        workbench.collection.len(),
        if workbench.collection.len() == 1 {
            "sample"
        } else {
            "samples"
        }
    )];
    if !workbench.filter.is_empty() {
        parts.push(format!("filter: {}", workbench.filter));
    }
    if !workbench.sort.is_empty() {
        // Each key as it runs, `↓` for one reversed; `r` turns them all.
        let keys: Vec<String> = workbench
            .sort
            .iter()
            .map(|key| {
                let (field, descending) = match key.strip_prefix('-') {
                    Some(field) => (field, true),
                    None => (key.as_str(), false),
                };
                format!(
                    "{field} {}",
                    if descending != workbench.reverse {
                        "↓"
                    } else {
                        "↑"
                    }
                )
            })
            .collect();
        parts.push(format!("sort: {}", keys.join(", ")));
    }
    if !workbench.basket.is_empty() {
        parts.push(format!(
            "basket: {}{}",
            workbench.basket.len(),
            if workbench.basket_only { " (only)" } else { "" }
        ));
    }
    if !workbench.selected.is_empty() {
        parts.push(format!("values: {} selected", workbench.selected.len()));
    }
    Line::raw(format!(" {}", parts.join(" · ")))
}

/// The keys of the note being edited, for the foot of the screen.
const NOTE_KEYS: &str =
    "Esc done, previewed · * _ ~ around a selection · Alt+1…6 heading · Alt+L link · Ctrl+Z undo";

/// What acts on the thing under the cursor, for the foot of its frame: as
/// many as a frame `width` wide holds, the first most wanted.
fn element_line(workbench: &Workbench, width: u16) -> Line<'static> {
    let hints = workbench.element_hints();
    if hints.is_empty() {
        return Line::raw("");
    }
    let mut said: Vec<String> = hints
        .iter()
        .map(|(key, does)| format!("{key} {does}"))
        .collect();
    while said.len() > 1 && said.join(" · ").chars().count() + 6 > width as usize {
        said.pop();
    }
    // One still too long is cut at its end: right-aligned, it lost its
    // start, `└ll what it changed ┘`.
    let said = ellipsized(&said.join(" · "), (width as usize).saturating_sub(6));
    let mut line = hint_spans(&said);
    line.spans.push(Span::raw(" "));
    line.right_aligned()
}

/// `text` cut to `width` columns, an ellipsis where it was cut.
pub fn ellipsized(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= width {
        return text.to_string();
    }
    let mut kept = String::new();
    for character in text.chars() {
        if (kept.clone() + &character.to_string()).width() + 1 > width {
            break;
        }
        kept.push(character);
    }
    kept.push('…');
    kept
}

/// `text` cut to `width` columns from its start, an ellipsis where it was
/// cut: a path keeps its end, where its name is.
pub fn ellipsized_start(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= width {
        return text.to_string();
    }
    let mut kept: Vec<char> = Vec::new();
    for character in text.chars().rev() {
        let next: String = std::iter::once(character)
            .chain(kept.iter().copied())
            .collect();
        if next.width() + 1 > width {
            break;
        }
        kept.insert(0, character);
    }
    format!("…{}", kept.into_iter().collect::<String>())
}

/// Hints cut to `width` columns, those that confirm or leave — `Enter`,
/// `Esc`, `y`, `n` — kept to the last: the others go first, from the end.
/// At 40 columns every window lost `Enter … · Esc cancel`, the one way out.
/// One hint still too long is cut at its end, never at its start.
pub fn fit_hints(line: &str, width: usize) -> String {
    let essential = |hint: &str| {
        ["Enter", "Esc", "y ", "n "]
            .iter()
            .any(|key| hint.starts_with(key))
    };
    let mut kept: Vec<String> = line.split(" · ").map(str::to_string).collect();
    let fits = |kept: &[String]| kept.join(" · ").chars().count() <= width;
    while !fits(&kept) && kept.len() > 1 {
        match kept.iter().rposition(|hint| !essential(hint)) {
            Some(at) => {
                kept.remove(at);
            }
            None => {
                // Those that confirm and leave, too long together: the first
                // cut short, so that the way out is still said after it.
                let rest = kept[1..].join(" · ").chars().count() + 3;
                let room = width.saturating_sub(rest);
                if room >= 8 {
                    kept[0] = ellipsized(&kept[0], room);
                } else {
                    kept.pop();
                }
            }
        }
    }
    ellipsized(&kept.join(" · "), width)
}

/// Hints on as few lines of `width` columns as hold them, `rows` at most, each
/// line broken between two hints, never inside one. Where `rows` lines cannot
/// hold them all, those that confirm or leave — `Enter`, `Esc`, `y`, `n` — and
/// `? help` are kept to the last, the others given up from the end; those left
/// still too many for the rows are `fit_hints`'s, on one line. The first line
/// is filled first: the foot's most wanted.
pub fn wrap_hints(line: &str, width: usize, rows: usize) -> Vec<String> {
    wrapped_hints(line, width, rows, false)
}

/// `wrap_hints`, the last line filled first: a window's border, where
/// `Enter` and `Esc` stay together — laid from the start, `Esc cancel` was
/// left alone on it.
pub fn wrap_window_hints(line: &str, width: usize, rows: usize) -> Vec<String> {
    wrapped_hints(line, width, rows, true)
}

fn wrapped_hints(line: &str, width: usize, rows: usize, from_end: bool) -> Vec<String> {
    let essential = |hint: &str| {
        ["Enter", "Esc", "y ", "n ", "? "]
            .iter()
            .any(|key| hint.starts_with(key))
    };
    let mut kept: Vec<&str> = line.trim_start().split(" · ").collect();
    loop {
        let laid = lay_hints(&kept, width, from_end);
        if laid.len() <= rows.max(1) {
            return laid;
        }
        match kept.iter().rposition(|hint| !essential(hint)) {
            Some(at) => {
                kept.remove(at);
            }
            None => return vec![fit_hints(&kept.join(" · "), width)],
        }
    }
}

/// Hints laid on lines of `width` columns in their order, a line broken
/// between the two hints where the next would pass its end — the lines
/// filled from the first, or `from_end` from the last; one too long for a
/// line alone cut at its end.
fn lay_hints(hints: &[&str], width: usize, from_end: bool) -> Vec<String> {
    let mut lines: Vec<Vec<String>> = Vec::new();
    let fits = |line: &[String], hint: &str| {
        line.iter()
            .map(|said| said.chars().count() + 3)
            .sum::<usize>()
            + hint.chars().count()
            <= width
    };
    let order: Vec<&&str> = if from_end {
        hints.iter().rev().collect()
    } else {
        hints.iter().collect()
    };
    for hint in order {
        let hint = ellipsized(hint, width);
        match lines.last_mut() {
            Some(line) if fits(line, &hint) => line.push(hint),
            _ => lines.push(vec![hint]),
        }
    }
    if from_end {
        lines.reverse();
        for line in &mut lines {
            line.reverse();
        }
    }
    lines.into_iter().map(|line| line.join(" · ")).collect()
}

/// The line of hints coloured: each key in its colour, what it does muted.
fn hint_spans(line: &str) -> Line<'static> {
    let theme = colors();
    let mut spans = Vec::new();
    for (at, hint) in line.trim_start().split(" · ").enumerate() {
        spans.push(Span::raw(if at == 0 { " " } else { "" }));
        if at > 0 {
            spans.push(Span::styled(" · ", Style::new().fg(theme.muted)));
        }
        // The keys are the words it begins with that name one — `j k
        // scroll`, `Shift+Tab back` — and a hint with none is said muted:
        // `any other key closes`.
        let words: Vec<&str> = hint.split(' ').collect();
        let keys = words
            .iter()
            .take(words.len().saturating_sub(1))
            .take_while(|word| is_key(word))
            .count();
        let key = words[..keys].join(" ");
        let said = words[keys..].join(" ");
        if !key.is_empty() {
            spans.push(Span::styled(key, key_style(&theme)));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(said, Style::new().fg(theme.muted)));
    }
    Line::from(spans)
}

/// Headers shortened until their columns fit `room` — the columns' widths
/// with the spaces between them. A header gives way before a value does: its
/// name is cut from the end first, a few characters always kept, then its
/// unit goes; a column is never narrower than its values.
pub fn fitted(headers: &[String], values: &[usize], room: usize) -> Vec<String> {
    const KEPT: usize = 3;
    /// A run of text cut from its end, a few characters always kept.
    struct Part {
        text: Vec<char>,
        keep: usize,
    }
    impl Part {
        fn of(text: &str) -> Part {
            let text: Vec<char> = text.chars().collect();
            Part {
                keep: text.len(),
                text,
            }
        }
        fn said(&self) -> String {
            let mut said: String = self.text.iter().take(self.keep).collect();
            if self.keep < self.text.len() {
                said.push('…');
            }
            said
        }
        fn can_cut(&self) -> bool {
            self.keep > KEPT
        }
        /// A character fewer; the first cut trades one for the ellipsis, so
        /// it takes two.
        fn cut(&mut self) {
            self.keep -= 1;
            if self.keep + 1 == self.text.len() && self.keep > KEPT {
                self.keep -= 1;
            }
        }
    }
    /// A header as three parts that give way in turn: a table cell's table
    /// (`conditioning.` in `conditioning.carbonation[50]`), then the name, then the unit.
    struct Header {
        table: Option<Part>,
        name: Part,
        unit: Option<String>,
        with_unit: bool,
    }
    impl Header {
        fn said(&self) -> String {
            let mut said = String::new();
            if let Some(table) = &self.table {
                said.push_str(&table.said());
                said.push('.');
            }
            said.push_str(&self.name.said());
            if let (true, Some(unit)) = (self.with_unit, &self.unit) {
                said.push(' ');
                said.push_str(unit);
            }
            said
        }
        fn width(&self) -> usize {
            self.said().chars().count()
        }
    }
    let mut headers: Vec<Header> = headers
        .iter()
        .map(|header| {
            let (name, unit) = match header.rsplit_once(" [") {
                Some((name, unit)) => (name, Some(format!("[{unit}"))),
                None => (header.as_str(), None),
            };
            // A table's name is what comes before the first dot, ahead of any
            // bracket: `conditioning.carbonation[50]`, never `malt.u` alone.
            let dot = name.find('.');
            let table = match dot {
                Some(dot) if name[dot..].contains('[') && !name[..dot].contains('[') => Some(dot),
                _ => None,
            };
            match table {
                Some(dot) => Header {
                    table: Some(Part::of(&name[..dot])),
                    name: Part::of(&name[dot + 1..]),
                    unit,
                    with_unit: true,
                },
                None => Header {
                    table: None,
                    name: Part::of(name),
                    unit,
                    with_unit: true,
                },
            }
        })
        .collect();
    let value = |at: usize| values.get(at).copied().unwrap_or(0);
    let total = |headers: &[Header]| -> usize {
        headers
            .iter()
            .enumerate()
            .map(|(at, header)| header.width().max(value(at)))
            .sum::<usize>()
            + headers.len().saturating_sub(1)
    };
    // Every column's table first, then every name, the widest beyond its
    // values giving a character at a time; then the units, if that was not
    // enough.
    #[derive(Clone, Copy, PartialEq)]
    enum Step {
        Table,
        Name,
        Unit,
    }
    for step in [Step::Table, Step::Name, Step::Unit] {
        while total(&headers) > room {
            let Some(at) = (0..headers.len())
                .filter(|at| {
                    let header = &headers[*at];
                    header.width() > value(*at)
                        && match step {
                            Step::Table => header.table.as_ref().is_some_and(Part::can_cut),
                            Step::Name => header.name.can_cut(),
                            Step::Unit => header.with_unit && header.unit.is_some(),
                        }
                })
                .max_by_key(|at| headers[*at].width())
            else {
                break;
            };
            let header = &mut headers[at];
            match step {
                Step::Table => {
                    if let Some(table) = header.table.as_mut() {
                        table.cut();
                    }
                }
                Step::Name => header.name.cut(),
                Step::Unit => header.with_unit = false,
            }
        }
    }
    headers.iter().map(Header::said).collect()
}

/// As many hints as `width` holds, in the order they are most wanted, and
/// `? help` always, so that a narrow terminal still says where the rest are.
pub fn hint_line(place: Place, filtered: bool, width: usize) -> String {
    hints_of(place, filtered, false, width)
}

/// The foot of a screen `width` wide: its hints on one line where it holds them
/// all, on two where it does not, broken between whole hints; the two still too
/// few, the least wanted given up from the end, `? help` kept. Each line begins
/// with a space, as `hints_of`'s does.
pub fn foot_lines(place: Place, filtered: bool, from_start: bool, width: usize) -> Vec<String> {
    let all = hints_of(place, filtered, from_start, usize::MAX);
    wrap_hints(&all, width.saturating_sub(1), 2)
        .into_iter()
        .map(|line| format!(" {line}"))
        .collect()
}

/// `hint_line`, `q` saying where it goes: back to the start page when the
/// workbench was opened from it, where it said *quit*.
pub fn hints_of(place: Place, filtered: bool, from_start: bool, width: usize) -> String {
    let said = |action: Action| {
        BINDINGS
            .iter()
            .find(|(at, _, bound, _)| *at == place && *bound == action)
            .map(|(_, key, _, _)| {
                // Said as the setup's frame says them: a step is ticked, and
                // what is ticked previewed, where the hint said select, open.
                let word = match (place, action) {
                    (Place::Setup, Action::Open) => "answer",
                    (_, Action::Quit) if from_start => "start page",
                    _ => short(action),
                };
                format!("{} {word}", key_name(*key))
            })
    };
    let help = said(Action::Help).unwrap_or_default();
    let separator = " · ";
    let mut line = String::from(" ");
    // A filter in force says first how it is cleared.
    let clear = [Action::ClearFilter];
    let first: &[Action] = if filtered && place == Place::Collection {
        &clear
    } else {
        &[]
    };
    for hint in first
        .iter()
        .chain(hinted(place))
        .filter_map(|action| said(*action))
    {
        let after = line.chars().count()
            + hint.chars().count()
            + separator.chars().count()
            + separator.chars().count()
            + help.chars().count();
        if after > width {
            break;
        }
        line.push_str(&hint);
        line.push_str(separator);
    }
    line.push_str(&help);
    line
}

/// The collection's table, from the rows the model rendered when they last
/// changed: a frame only lays them out. The columns shown, summarised over what
/// is shown: a row per column, as `--summary` writes it.
fn summary(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    use crate::presentation::summaries;
    let theme = colors();
    // Grouped, a summary per group, its values in first columns on its first
    // row, as `--summary --group` writes it: one module makes both.
    let grouped = !workbench.group.is_empty();
    let made = if grouped {
        workbench
            .group
            .iter()
            .map(|field| {
                crate::query::field_addressing::parse(field).map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|parsed| {
                summaries::grouped(
                    &workbench.view,
                    &workbench.profile,
                    &workbench.collection,
                    None,
                    true,
                    &parsed,
                    workbench.group_order(),
                )
            })
    } else {
        summaries::rows(
            &workbench.view,
            &workbench.profile,
            &workbench.collection,
            None,
            true,
        )
        .map(|(rows, left_out)| (vec![(Vec::new(), rows)], left_out))
    };
    let (parts, left_out) = match made {
        Ok(made) => made,
        Err(error) => (Vec::new(), vec![error]),
    };
    let body: Vec<Vec<String>> = parts
        .iter()
        .flat_map(|(key, rows)| {
            rows.iter().enumerate().map(move |(at, row)| {
                // The values once, on the group's first row.
                let mut cells: Vec<String> = if at == 0 {
                    key.iter().map(summaries::group_value).collect()
                } else {
                    vec![String::new(); key.len()]
                };
                cells.push(match &row.unit {
                    Some(unit) => format!("{} [{unit}]", row.label),
                    None => row.label.clone(),
                });
                cells.extend(summaries::written(row));
                cells
            })
        })
        .collect();
    // The mean's heading says how it was taken, as `--summary`'s does.
    let mean = summaries::mean_header(parts.iter().flat_map(|(_, rows)| rows));
    let mut header: Vec<String> = if grouped {
        workbench.group.clone()
    } else {
        Vec::new()
    };
    header.extend(
        [
            "column",
            "n",
            mean.as_str(),
            "s",
            "sem",
            "median",
            "min",
            "max",
        ]
        .iter()
        .map(ToString::to_string),
    );
    let labels = if grouped {
        workbench.group.len() + 1
    } else {
        1
    };
    let widths: Vec<Constraint> = (0..header.len())
        .map(|at| {
            let widest = body
                .iter()
                .map(|cells| cells[at].chars().count())
                .max()
                .unwrap_or(0)
                .max(header[at].chars().count());
            Constraint::Length(widest as u16)
        })
        .collect();
    let table_rows = body.into_iter().map(|cells| {
        let cells: Vec<Cell> = cells
            .into_iter()
            .enumerate()
            .map(|(at, cell)| {
                if at < labels {
                    Cell::from(cell)
                } else {
                    // Numbers right-aligned, so that their places line up.
                    Cell::from(Line::from(cell).right_aligned())
                }
            })
            .collect();
        Row::new(separated(cells, separator(), 0))
    });
    let header: Vec<Cell> = header.into_iter().map(Cell::from).collect();
    let mut title = format!(
        " summary of {} {} ",
        workbench.view.len(),
        if workbench.view.len() == 1 {
            "sample"
        } else {
            "samples"
        }
    );
    if !left_out.is_empty() {
        title = format!("{title}· left out, text: {} ", left_out.join(", "));
    }
    let table = Table::new(table_rows, separated(widths, Constraint::Length(1), 0))
        .header(
            Row::new(separated(header, separator(), 0))
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        .block(
            framed_saying(workbench, area.width)
                .title(Line::from(title).style(Style::new().fg(theme.selected)))
                .title_bottom(hint_spans("S the samples again").right_aligned()),
        );
    frame.render_widget(table, area);
    say_inside(
        frame,
        area,
        &said_on_screen(workbench),
        said_rows(workbench, area.width),
    );
}

/// Draws the collection; the furthest `>` may scroll, where the columns left
/// fit, which the model keeps its scroll within.
fn collection(frame: &mut Frame, workbench: &Workbench, area: Rect) -> usize {
    if workbench.summarised {
        summary(frame, workbench, area);
        return 0;
    }
    // The names always in place, first: a profile's `name` column is that one,
    // never a column scrolled away with the rest.
    let named = true;
    let data: Vec<usize> = workbench
        .profile
        .columns()
        .iter()
        .enumerate()
        .filter(|(_, column)| column.field != "name")
        .map(|(at, _)| at)
        .collect();
    let paths: Vec<Option<&std::path::Path>> = workbench
        .view
        .iter()
        .map(|entry| entry.path.as_deref())
        .collect();
    // Each column's values lined up on their `±`.
    let every: Vec<Vec<String>> = data
        .iter()
        .map(|&at| {
            let cells: Vec<String> = workbench
                .rendered
                .iter()
                .map(|rendered| rendered.cells.get(at).cloned().unwrap_or_default())
                .collect();
            aligned(&cells)
        })
        .collect();
    // The names headed as the profile heads them — `Brew` — as the summary
    // says them: the collection said `name` where `S` said `Brew`.
    let name_header = workbench
        .profile
        .columns()
        .iter()
        .position(|column| column.field == "name")
        .and_then(|at| workbench.header.get(at).cloned())
        .unwrap_or_else(|| "name".to_string());
    let name_width = workbench
        .rendered
        .iter()
        .map(|rendered| rendered.name.chars().count())
        .max()
        .unwrap_or(4)
        .max(name_header.chars().count().min(12))
        .max(4);
    // Scrolled to the side: the columns from the first `<` and `>` chose, the
    // marks and the names always in place — and never further than where the
    // columns left all fit, so that `>` stops once nothing waits past the edge.
    let inner = (area.width as usize).saturating_sub(4);
    let base = if named { 3 + name_width } else { 0 };
    let widths: Vec<usize> = every
        .iter()
        .zip(&data)
        .map(|(column, &at)| {
            let value = column
                .iter()
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0);
            let head = workbench
                .header
                .get(at)
                .map_or(0, |head| head.chars().count());
            value.max(head.min(4)) + 3
        })
        .collect();
    let mut furthest = 0;
    let mut used = base;
    for at in (0..widths.len()).rev() {
        if at + 1 < widths.len() && used + widths[at] > inner {
            furthest = at + 1;
            break;
        }
        used += widths[at];
    }
    let first = workbench.hscroll.min(furthest);
    let columns: Vec<Vec<String>> = every[first..].to_vec();
    let heads: Vec<String> = data[first.min(data.len())..]
        .iter()
        .map(|&at| workbench.header.get(at).cloned().unwrap_or_default())
        .collect();
    let values: Vec<usize> = columns
        .iter()
        .map(|column| {
            column
                .iter()
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    // The room the data columns have: the inside of the frame, less the
    // marks, the names, the rules and the spaces beside each.
    let count = values.len();
    let rules = if named {
        count
    } else {
        count.saturating_sub(1)
    };
    let all = 1 + usize::from(named) + count + rules;
    let fixed = 2 + if named { name_width } else { 0 } + rules + all.saturating_sub(1);
    let room = (area.width as usize)
        .saturating_sub(4)
        .saturating_sub(fixed)
        + count.saturating_sub(1);
    let _ = room;
    // Which columns are shown is decided first, each at the narrowest its
    // header may be cut to; their headers are then fitted to the room they
    // have. What does not fit waits past the edge, `>` away, rather than every
    // column squeezed: the marks and the names, then each column with its rule
    // and the spaces beside it.
    let narrowest = |at: usize| values[at].max(heads[at].chars().count().min(4));
    let mut used = base;
    let mut fits = 0;
    for at in 0..count {
        let width = narrowest(at) + 3;
        if fits > 0 && used + width > inner {
            break;
        }
        used += width;
        fits += 1;
    }
    let fits = fits.max(1).min(count);
    let room_shown = inner.saturating_sub(base + 3 * fits) + fits.saturating_sub(1);
    let titles = fitted(&heads[..fits], &values[..fits], room_shown);
    let values = &values[..fits];
    let hidden = (first, every.len().saturating_sub(first + fits));
    // Several projects: a line where each begins, its title on it; grouped,
    // where each group begins, its values on it: a row of the table left blank
    // and drawn over once the table is.
    let headings = workbench.headings();
    let mut titled: Vec<(usize, String)> = Vec::new();
    let mut selected = workbench.cursor;
    let rows: Vec<Row> = workbench
        .rendered
        .iter()
        .zip(&paths)
        .enumerate()
        .map(|(row, (rendered, path))| {
            let in_basket = path.is_some_and(|path| workbench.basket.contains(path));
            let mut cells = vec![Cell::from(Line::from(vec![
                Span::styled(
                    if in_basket { "●" } else { " " },
                    Style::new().fg(colors().selected),
                ),
                Span::styled(
                    rendered.mark,
                    Style::new().fg(match rendered.mark {
                        "✗" => colors().failed,
                        "⚠" | "∅" => colors().outdated,
                        _ => colors().edited,
                    }),
                ),
            ]))];
            if named {
                cells.push(Cell::from(rendered.name.clone()));
            }
            cells.extend(
                columns
                    .iter()
                    .take(fits)
                    .map(|column| Cell::from(column[row].clone())),
            );
            Row::new(separated(cells, separator(), 1))
        })
        .collect();
    let rows: Vec<Row> = rows
        .into_iter()
        .enumerate()
        .flat_map(|(at, row)| {
            let heading = headings.get(at).cloned().flatten();
            let before = heading.map(|title| {
                titled.push((at + titled.len(), title));
                if at <= workbench.cursor {
                    selected += 1;
                }
                Row::new(Vec::<Cell>::new())
            });
            before.into_iter().chain(std::iter::once(row))
        })
        .collect();
    let lines_of_rows = rows.len();
    let mut header = vec![String::new()];
    if named {
        header.push(name_header);
    }
    header.extend(titles.iter().cloned());
    let mut widths = vec![Constraint::Length(2)];
    if named {
        widths.push(Constraint::Length(name_width as u16));
    }
    for (title, value) in titles.iter().zip(values) {
        // The column spacing on either side of a rule is the room between.
        widths.push(Constraint::Length(title.chars().count().max(*value) as u16));
    }
    // Columns past either edge are said, with the keys that reach them.
    let edges = match hidden {
        (0, 0) => String::new(),
        (before, after) => format!(
            " {}columns {}–{} of {}{} ",
            if before > 0 { "◂ < · " } else { "" },
            before + 1,
            before + fits,
            every.len(),
            if after > 0 { " · > ▸" } else { "" }
        ),
    };
    let header: Vec<Cell> = header.into_iter().map(Cell::from).collect();
    let table = Table::new(rows, separated(widths, Constraint::Length(1), 1))
        .header(
            Row::new(separated(header, separator(), 1))
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(Style::new().reversed())
        .block(
            framed_saying(workbench, area.width)
                .title(Line::from(edges).style(Style::new().fg(colors().muted)))
                .title_bottom(element_line(workbench, area.width)),
        );
    let rows_shown = (area.height as usize).saturating_sub(3 + said_rows(workbench, area.width));
    let mut offset = offset_within(workbench, "collection", &vec![1; lines_of_rows], rows_shown);
    // A group's first sample under the cursor brings its heading with it.
    if offset == selected && selected > 0 && titled.iter().any(|(at, _)| *at + 1 == selected) {
        offset -= 1;
    }
    let mut state = TableState::default()
        .with_selected(Some(selected))
        .with_offset(offset);
    frame.render_stateful_widget(table, area, &mut state);
    keep_offset(workbench, "collection", state.offset());
    // Each heading drawn across the table: `── more ───…`.
    let inside = area.width.saturating_sub(4);
    for (at, title) in &titled {
        let Some(line) = at.checked_sub(state.offset()) else {
            continue;
        };
        if line >= rows_shown {
            continue;
        }
        let y = area.y + 2 + line as u16;
        let text = ellipsized(&format!("── {title} "), inside as usize);
        let rule = "─".repeat((inside as usize).saturating_sub(text.chars().count()));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    text,
                    Style::new()
                        .fg(colors().accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(rule, Style::new().fg(colors().muted)),
            ])),
            Rect::new(area.x + 2, y, inside, 1),
        );
    }
    say_inside(
        frame,
        area,
        &said_on_screen(workbench),
        said_rows(workbench, area.width),
    );
    furthest
}

/// The setup's keys, said one way under its question, on its frame and at the
/// foot: `Enter answer` for a question, `Enter set up` for what the answers
/// will do, `Esc back` for both.
pub const SETUP_ASKING: &str = "j k choose · Enter answer · Esc back";
pub const SETUP_DONE: &str = "Enter set up · Esc back";

/// The setup's keys under its question, as a foot says them — each key in its
/// colour — broken between whole keys, two columns in.
fn setup_keys(said: &str, width: usize) -> Vec<Line<'static>> {
    wrap_hints(said, width.saturating_sub(2), 3)
        .iter()
        .map(|line| {
            let mut line = hint_spans(line);
            line.spans.insert(0, Span::raw(" "));
            line
        })
        .collect()
}

/// Setting the project up: the question on screen and its answers, then what
/// the answers will do, and what is already here, kept.
fn setup(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    let theme = colors();
    let (Some(setup), Screen::Setup { cursor }) = (&workbench.setup, &workbench.screen) else {
        return;
    };
    let root = &workbench.root;
    let questions = setup.questions(root);
    // Each line wrapped under its own indent: wrapped by the frame, a detail's
    // second line fell to the edge, `│ files`. At the reading width: the
    // questions are prose.
    let width = (area.width.min(READING_WIDTH) as usize)
        .saturating_sub(4)
        .max(8);
    let mut lines: Vec<Line> = Vec::new();
    let title;
    if let Some(question) = questions.get(setup.question) {
        title = format!(
            " setting {} up · question {} of {} ",
            root.display(),
            setup.question + 1,
            questions.len()
        );
        lines.push(Line::raw(""));
        lines.extend(hanging(
            question.ask(),
            2,
            width,
            Style::new().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(""));
        for (index, (label, detail)) in question.choices(root).iter().enumerate() {
            let on = index == *cursor;
            let style = if on {
                Style::new().fg(theme.selected).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            let mut label = hanging(label, 4, width, style);
            if on && let Some(first) = label.first_mut() {
                first.spans[0] = Span::styled("  ▸ ", style);
            }
            lines.extend(label);
            lines.extend(hanging(detail, 6, width, Style::new().fg(theme.muted)));
            lines.push(Line::raw(""));
        }
        lines.extend(setup_keys(SETUP_ASKING, width));
    } else {
        let plan = setup.plan(root);
        title = format!(" setting {} up · what will be done ", root.display());
        lines.push(Line::raw(""));
        for proposal in &plan.proposals {
            let name = super::model::step_name(&proposal.step, root);
            if proposal.present {
                lines.extend(hanging(
                    &format!("{name}   already here, kept"),
                    4,
                    width,
                    Style::new().fg(theme.muted),
                ));
            } else {
                let mut named = hanging(&name, 4, width, Style::new());
                if let Some(first) = named.first_mut() {
                    first.spans[0] = Span::styled("  + ", Style::new().fg(theme.selected));
                }
                lines.extend(named);
            }
            if let Some(note) = &proposal.note {
                lines.extend(hanging(note, 6, width, Style::new().fg(theme.error)));
            }
        }
        if let project_setup::Environment::Ready(venv) = project_setup::environment(root) {
            lines.extend(hanging(
                &format!(
                    "computing runs in {}, which holds samplekit",
                    venv.display()
                ),
                4,
                width,
                Style::new().fg(theme.muted),
            ));
        }
        lines.push(Line::raw(""));
        if let Some(directory) = &plan.blocked {
            lines.extend(hanging(
                &format!(
                    "{} is a file where a directory must go: nothing can be written until it moves",
                    directory.strip_prefix(root).unwrap_or(directory).display()
                ),
                2,
                width,
                Style::new().fg(theme.error),
            ));
        } else if plan.is_complete() {
            lines.extend(hanging(
                "this project is set up: there is nothing to do",
                2,
                width,
                Style::new(),
            ));
        } else {
            lines.extend(setup_keys(SETUP_DONE, width));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            framed_saying(workbench, area.width)
                .title(title)
                .title_bottom(element_line(workbench, area.width)),
        ),
        area,
    );
    say_inside(
        frame,
        area,
        &said_on_screen(workbench),
        said_rows(workbench, area.width),
    );
}

/// The project's configuration: its sections on the left, the one chosen's
/// settings on the right, the keys it takes below them.
fn configure(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    use super::model::{ConfigRow, SECTIONS};
    let theme = colors();
    let Some(workspace) = &workbench.workspace else {
        return;
    };
    let [left, right] =
        Layout::horizontal([Constraint::Length(24), Constraint::Min(20)]).areas(area);
    let sections: Vec<ListItem> = SECTIONS
        .iter()
        .map(|(label, _, _)| ListItem::new(Line::raw(label.to_string())))
        .collect();
    let mut state = ListState::default()
        .with_selected(Some(workspace.section))
        .with_offset(offset_of(workbench, "sections"));
    let focus = |on: bool| {
        if on {
            Style::new().fg(theme.selected)
        } else {
            Style::new()
        }
    };
    frame.render_stateful_widget(
        List::new(sections)
            .highlight_style(if workspace.on_rows {
                Style::new().fg(theme.selected)
            } else {
                Style::new().reversed()
            })
            .block(
                framed()
                    .border_style(focus(!workspace.on_rows))
                    .title(if workspace.dirty {
                        " sections · not written "
                    } else {
                        " sections "
                    }),
            ),
        left,
        &mut state,
    );
    keep_offset(workbench, "sections", state.offset());
    let rows = workbench.workspace_rows();
    let key_width = rows
        .iter()
        .map(|row| match row {
            ConfigRow::Key { key, .. } => key.chars().count(),
            ConfigRow::Name(_) => 0,
        })
        .max()
        .unwrap_or(0);
    let items: Vec<ListItem> = if rows.is_empty() {
        vec![ListItem::new(Line::from(keyed(
            "nothing declared here: a adds a setting",
            Style::new().fg(theme.muted),
        )))]
    } else {
        rows.iter()
            .map(|row| match row {
                ConfigRow::Name(name) => ListItem::new(Line::styled(
                    name.clone(),
                    Style::new().fg(theme.key).add_modifier(Modifier::BOLD),
                )),
                ConfigRow::Key { name, key, value } => ListItem::new(Line::from(vec![
                    Span::raw(if name.is_some() { "  " } else { "" }),
                    Span::styled(
                        format!("{key:<key_width$}"),
                        Style::new().fg(theme.attribute),
                    ),
                    Span::styled(" = ", Style::new().fg(theme.muted)),
                    Span::raw(value.clone()),
                ])),
            })
            .collect()
    };
    let (label, _, keys) = SECTIONS[workspace.section];
    let mut state = ListState::default()
        .with_selected(workspace.on_rows.then_some(workspace.row))
        .with_offset(offset_of(workbench, "settings"));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::new().reversed())
            .block(
                framed_saying(workbench, right.width)
                    .border_style(focus(workspace.on_rows))
                    .title(format!(" {label} "))
                    .title_bottom(
                        Line::from(format!(" keys: {keys} ")).style(Style::new().fg(theme.muted)),
                    ),
            ),
        right,
        &mut state,
    );
    keep_offset(workbench, "settings", state.offset());
    say_inside(
        frame,
        right,
        &said_on_screen(workbench),
        said_rows(workbench, right.width),
    );
}

/// The collection's state: each value not current and each finding, worst
/// first, the sample it is about beside it.
fn control(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    use super::model::Concern;
    let Screen::Control { cursor } = workbench.screen else {
        return;
    };
    let theme = colors();
    let widest = workbench
        .control
        .iter()
        .map(|item| item.sample.chars().count())
        .max()
        .unwrap_or(0);
    let colour_of = |kind: Concern| match kind {
        Concern::Failed => theme.failed,
        Concern::Defect => theme.error,
        Concern::Stale => theme.outdated,
        Concern::Owed => theme.outdated,
        Concern::Edited => theme.edited,
        Concern::Note => theme.muted,
    };
    // What the model owes is said one way wherever `∅` stands: never computed,
    // or waiting for an input.
    let waiting = |item: &super::model::ControlItem| item.said.contains("waits for");
    let word_of = |item: &super::model::ControlItem| match item.kind {
        Concern::Failed => "failed",
        Concern::Defect => "defect",
        Concern::Stale => "outdated",
        Concern::Owed if waiting(item) => "waiting",
        Concern::Owed => "never computed",
        Concern::Edited => "edited",
        Concern::Note => "note",
    };
    let words = workbench
        .control
        .iter()
        .map(|item| word_of(item).chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    // A line wraps under what it says, never cut at the frame: a note of
    // `validate` runs past 80 columns.
    let lead = 2 + words + 1 + widest + 2;
    let room = (area.width as usize).saturating_sub(4 + lead).max(12);
    let heights: Vec<usize> = workbench
        .control
        .iter()
        .map(|item| wrapped(&item.said, room).len().max(1))
        .collect();
    let items: Vec<ListItem> = workbench
        .control
        .iter()
        .map(|item| {
            let colour = colour_of(item.kind);
            let said_style = if item.kind == Concern::Note {
                Style::new().fg(theme.muted)
            } else {
                Style::new()
            };
            let mut lines: Vec<Line> = Vec::new();
            for (at, part) in wrapped(&item.said, room).into_iter().enumerate() {
                if at == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!(
                                "{} {:<words$}",
                                super::model::concern_mark(item.kind),
                                word_of(item)
                            ),
                            Style::new().fg(colour),
                        ),
                        Span::raw(format!(" {:<widest$}  ", item.sample)),
                        Span::styled(part, said_style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw(" ".repeat(lead)),
                        Span::styled(part, said_style),
                    ]));
                }
            }
            ListItem::new(lines)
        })
        .collect();
    // What the model owes counted as it is said: never computed, and waiting.
    let count = |kind: Concern, waits: bool| {
        workbench
            .control
            .iter()
            .filter(|item| item.kind == kind && (kind != Concern::Owed || waiting(item) == waits))
            .count()
    };
    // Each mark beside its count, as the legend of the marks says it (the
    // help's `MARKS`), and only those there are: six counts ran past 80
    // columns.
    let counted: Vec<String> = [
        (Concern::Failed, false, "failed", "failed"),
        (Concern::Defect, false, "defective", "defective"),
        (Concern::Stale, false, "outdated", "outdated"),
        (Concern::Owed, false, "never computed", "never computed"),
        (Concern::Owed, true, "waiting", "waiting"),
        (Concern::Edited, false, "edited", "edited"),
        (Concern::Note, false, "note", "notes"),
    ]
    .into_iter()
    .filter(|(kind, waits, ..)| count(*kind, *waits) > 0)
    .map(|(kind, waits, one, many)| {
        let count = count(kind, waits);
        format!(
            "{} {count} {}",
            super::model::concern_mark(kind),
            if count == 1 { one } else { many }
        )
    })
    .collect();
    // Nothing to report said once, in the title.
    let title = if counted.is_empty() {
        " everything is current, and nothing is defective ".to_string()
    } else {
        format!(" {} ", counted.join(" · "))
    };
    // What the model would compute is listed only where it is read: said,
    // as `status` says it, rather than a list that looks complete.
    let unread = workbench
        .model_unread
        .as_ref()
        .map(|reason| format!("values the model would compute are not listed: {reason}"));
    let block = framed_saying(workbench, area.width)
        .title(title)
        .title_bottom(element_line(workbench, area.width));
    let inside = block.inner(area);
    let list_area = match &unread {
        Some(unread) => {
            let said: Vec<Line> = wrapped(unread, inside.width.max(1) as usize)
                .into_iter()
                .map(|line| Line::styled(line, Style::new().fg(theme.muted)))
                .collect();
            let rows = (said.len() as u16).min(inside.height.saturating_sub(1));
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new(said),
                Rect {
                    height: rows,
                    ..inside
                },
            );
            Rect {
                y: inside.y + rows,
                height: inside.height - rows,
                ..inside
            }
        }
        None => {
            frame.render_widget(block, area);
            inside
        }
    };
    let mut state = ListState::default()
        .with_selected(Some(cursor))
        .with_offset(offset_within(
            workbench,
            "control",
            &heights,
            list_area.height as usize,
        ));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().reversed()),
        list_area,
        &mut state,
    );
    keep_offset(workbench, "control", state.offset());
    say_inside(
        frame,
        area,
        &said_on_screen(workbench),
        said_rows(workbench, area.width),
    );
}

/// A table of the sample unfolded: its index, then its columns, a row each.
fn table(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    let Screen::Table {
        name,
        cursor,
        column: on,
        ..
    } = &workbench.screen
    else {
        return;
    };
    let Some((path, sample)) = workbench.current() else {
        return;
    };
    let Ok(held) = sample.table(name) else {
        return;
    };
    let vocabulary = workbench.collection.vocabulary();
    let subject = Subject {
        sample: &sample,
        path: Some(&path),
        vocabulary: &vocabulary,
        states: None,
    };
    let config = workbench.collection.config_for(&path);
    let index: Vec<String> = held
        .index_columns()
        .iter()
        .map(ToString::to_string)
        .collect();
    let columns: Vec<String> = super::model::value_columns(held)
        .iter()
        .map(ToString::to_string)
        .collect();
    // A column selected for `c` is marked, as a value is.
    let marked: Vec<String> = columns
        .iter()
        .map(|column| {
            if workbench.selected.contains(&format!("{name}.{column}")) {
                format!("● {column}")
            } else {
                column.clone()
            }
        })
        .collect();
    // Each derived cell's state, for its colour.
    let mut states: Vec<Vec<String>> = Vec::new();
    let mut body: Vec<Vec<String>> = held
        .rows()
        .map(|row| {
            let keys: Vec<String> = row.index().into_iter().map(editing::shown_value).collect();
            let values: Vec<crate::core::value::Value> = row.index().into_iter().cloned().collect();
            states.push(
                super::model::value_columns(held)
                    .iter()
                    .map(|column| {
                        let derived = row
                            .cell(column)
                            .is_ok_and(|cell| cell.records().computed.is_some());
                        if !derived {
                            return String::new();
                        }
                        fingerprint::check_cell_among(&sample, name, column, &values)
                            .map(|state| super::model::word(&state))
                            .unwrap_or_default()
                    })
                    .collect(),
            );
            let mut cells = keys.clone();
            for column in &columns {
                // A cell as a table column shows it, by its address.
                let field = Identifier::new(column)
                    .map(|column| super::model::cell_address(name, &column, values.clone()))
                    .unwrap_or_default();
                let profile = profiles::anonymous(vec![ColumnSpec {
                    field: field.clone(),
                    label: None,
                    header: None,
                    precision: None,
                    template: None,
                }]);
                let text = render::cell(
                    &profile.columns()[0],
                    &subject,
                    render::Declaration::Profile(&profile),
                    config,
                    None,
                    true,
                );
                cells.push(if text.is_empty() {
                    "—".to_string()
                } else {
                    text
                });
            }
            cells
        })
        .collect();
    // Each column's values lined up on their `±`.
    for at in index.len()..index.len() + columns.len() {
        let column: Vec<String> = body.iter().map(|cells| cells[at].clone()).collect();
        for (cells, cell) in body.iter_mut().zip(aligned(&column)) {
            cells[at] = cell;
        }
    }
    let widest = |at: usize| {
        body.iter()
            .map(|cells| cells.get(at).map_or(0, |cell| cell.chars().count()))
            .max()
            .unwrap_or(0)
    };
    // The index keeps its width; the columns' names give way as the
    // collection's headers do.
    let index_widths: Vec<usize> = index
        .iter()
        .enumerate()
        .map(|(at, title)| widest(at).max(title.chars().count()))
        .collect();
    let values: Vec<usize> = (0..columns.len())
        .map(|at| widest(index.len() + at))
        .collect();
    // The columns shown follow the cursor: from the first that lets it be
    // seen, as many as fit, each at the narrowest its name may be cut to; the
    // index always in place. Their names are then fitted to the room they have.
    let on = (*on).min(columns.len().saturating_sub(1));
    let inner = (area.width as usize).saturating_sub(4);
    let base = index_widths.iter().map(|width| width + 3).sum::<usize>();
    let narrowest = |at: usize| values[at].max(marked[at].chars().count().min(4));
    let span = |from: usize, to: usize| (from..=to).map(|at| narrowest(at) + 3).sum::<usize>();
    let mut start = 0;
    while start < on && base + span(start, on) > inner {
        start += 1;
    }
    let mut end = on;
    while end + 1 < columns.len() && base + span(start, end + 1) <= inner {
        end += 1;
    }
    // A table of its index alone has no column to show beside it.
    let past = if columns.is_empty() { 0 } else { end + 1 };
    let shown = past - start.min(past);
    let room = inner.saturating_sub(base + 3 * shown) + shown.saturating_sub(1);
    let titles = fitted(&marked[start..past], &values[start..past], room);
    let values = &values[start..past];
    let mut header = index.clone();
    header.extend(titles.iter().cloned());
    let widths: Vec<Constraint> = index_widths
        .iter()
        .copied()
        .chain(
            titles
                .iter()
                .zip(values)
                .map(|(title, value)| title.chars().count().max(*value)),
        )
        .map(|width| Constraint::Length(width as u16))
        .collect();
    let rows = body.into_iter().zip(states).map(|(cells, states)| {
        let cells: Vec<Cell> = cells
            .into_iter()
            .enumerate()
            .filter(|(at, _)| {
                *at < index.len() || (index.len() + start..index.len() + past).contains(at)
            })
            .map(|(at, cell)| {
                if at < index.len() {
                    return Cell::from(cell).style(Style::new().fg(colors().index));
                }
                let state = states.get(at - index.len()).map_or("", String::as_str);
                Cell::from(cell).style(Style::new().fg(state_colour(state)))
            })
            .collect();
        Row::new(separated(cells, separator(), 0))
    });
    let edges = if start > 0 || past < columns.len() {
        format!("· columns {}–{} of {} ", start + 1, end + 1, columns.len())
    } else {
        String::new()
    };
    let count = held.rows().count();
    let header: Vec<Cell> = header.into_iter().map(Cell::from).collect();
    let table = Table::new(rows, separated(widths, Constraint::Length(1), 0))
        .header(
            Row::new(separated(header, separator(), 0))
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        // The row underlined, the cell under the cursor reversed.
        .row_highlight_style(Style::new().add_modifier(Modifier::UNDERLINED))
        .cell_highlight_style(Style::new().reversed())
        .block(
            framed_saying(workbench, area.width)
                .title(format!(" {name} · {count} rows {edges}"))
                .title_bottom(element_line(workbench, area.width)),
        );
    let rows_shown = (area.height as usize).saturating_sub(3 + said_rows(workbench, area.width));
    let mut state = TableState::default()
        .with_selected(Some(*cursor))
        // Every column but the first has a separator before it.
        .with_selected_column(Some(2 * (index.len() + on - start)))
        .with_offset(offset_within(
            workbench,
            "table",
            &vec![1; count],
            rows_shown,
        ));
    frame.render_stateful_widget(table, area, &mut state);
    keep_offset(workbench, "table", state.offset());
    say_inside(
        frame,
        area,
        &said_on_screen(workbench),
        said_rows(workbench, area.width),
    );
}

/// A column's values lined up on their `±`: each number right-aligned to the
/// widest before it, so that the signs, and the uncertainties after them, fall
/// in one column. A column with no `±` is left as it is.
pub fn aligned(cells: &[String]) -> Vec<String> {
    if !cells.iter().any(|cell| cell.contains(" ± ")) {
        return cells.to_vec();
    }
    // The number, and what follows it: `± 0.01 mg`, or a unit alone.
    let split = |cell: &str| -> (String, String) {
        match cell.split_once(' ') {
            Some((number, rest)) => (number.to_string(), format!(" {rest}")),
            None => (cell.to_string(), String::new()),
        }
    };
    let widest = cells
        .iter()
        .map(|cell| split(cell).0.chars().count())
        .max()
        .unwrap_or(0);
    cells
        .iter()
        .map(|cell| {
            let (number, rest) = split(cell);
            format!("{number:>widest$}{rest}")
        })
        .collect()
}

/// A column between columns, drawn as a thin rule.
fn separator() -> Cell<'static> {
    Cell::from("│").style(Style::new().fg(colors().muted))
}

/// `items` with `separator` between each, from the one after `first` on: the
/// collection keeps its marks and its names together.
fn separated<T: Clone>(items: Vec<T>, separator: T, first: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(items.len() * 2);
    for (at, item) in items.into_iter().enumerate() {
        if at > first {
            out.push(separator.clone());
        }
        out.push(item);
    }
    out
}

/// The sample's values and its note; where the note is drawn while it is
/// edited, for a click to find its place.
fn sample(frame: &mut Frame, workbench: &Workbench, area: Rect) -> Option<NoteView> {
    let entries = workbench.entries();
    let widest = entries
        .iter()
        .map(|entry| entry.field.chars().count())
        .max()
        .unwrap_or(0);
    // The quantities' values lined up on their `±`.
    let mut entries = entries;
    let quantities: Vec<usize> = (0..entries.len())
        .filter(|at| entries[*at].kind == EntryKind::Quantity)
        .collect();
    let shown: Vec<String> = quantities
        .iter()
        .map(|at| entries[*at].shown.clone())
        .collect();
    for (at, shown) in quantities.into_iter().zip(aligned(&shown)) {
        entries[at].shown = shown;
    }
    let shown_width = entries
        .iter()
        .map(|entry| entry.shown.chars().count())
        .max()
        .unwrap_or(0)
        .min(40);
    // The values take what they need — marks, names, values, the frame — and
    // the note the rest, at most 55% for the values, so that prose is never
    // read through a slot; the note no wider than prose is read, the values
    // taking what lies beyond. The same while the note is edited: a window that
    // moved as editing began was read as a bug, and was one.
    let state_width = entries
        .iter()
        .map(|entry| entry.state.chars().count())
        .max()
        .unwrap_or(0);
    // The selection and the mark, the name, the value, the state, the spaces
    // between and the frame.
    let needed = (3 + widest + 2 + shown_width + 2 + state_width + 5) as u16;
    let share = 55;
    // Narrow, the note goes under the values rather than beside them: at 40
    // columns the values' pane was 18 wide, and drew names alone.
    let stacked = area.width < NARROW;
    // Hidden by `n`, the values take the whole width; edited, it is shown.
    let hidden = workbench.note_hidden && !matches!(workbench.mode, Mode::Note { .. });
    let values_width = if stacked || hidden {
        area.width
    } else {
        needed
            .min(area.width * share / 100)
            .max(24.min(area.width / 2))
            .max(area.width.saturating_sub(READING_WIDTH))
    };
    // Where the state does not fit, its mark alone stands for it, rather
    // than the state being cut off the line: at 80 columns an outdated value read
    // as current. The mark has a column of its own, before the name, so that
    // no value too long for the pane pushes it out of sight.
    let marked_only = needed > values_width;
    let [left, right] = if hidden {
        [area, Rect::default()]
    } else if stacked {
        let note_rows = (area.height / 3).max(3);
        Layout::vertical([Constraint::Min(3), Constraint::Length(note_rows)]).areas(area)
    } else {
        Layout::horizontal([Constraint::Length(values_width), Constraint::Min(20)]).areas(area)
    };
    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let colour = match entry.kind {
                EntryKind::Quantity => Color::Reset,
                EntryKind::Attribute => colors().attribute,
                EntryKind::Table => colors().table,
            };
            let state_colour = state_colour(&entry.state);
            let selected = workbench.selected.contains(&entry.field);
            let mark = state_mark(&entry.state);
            ListItem::new(Line::from(vec![
                Span::styled(
                    if selected { "●" } else { " " },
                    Style::new().fg(colors().selected),
                ),
                Span::styled(
                    format!("{} ", if mark.is_empty() { " " } else { mark }),
                    Style::new().fg(state_colour),
                ),
                Span::styled(
                    format!("{:<widest$}  ", entry.field),
                    Style::new().fg(colour),
                ),
                Span::raw(format!("{:<shown_width$}  ", entry.shown)),
                Span::styled(
                    if marked_only {
                        String::new()
                    } else {
                        entry.state.clone()
                    },
                    Style::new().fg(state_colour),
                ),
            ]))
        })
        .collect();
    let cursor = match workbench.screen {
        Screen::Sample { cursor, .. } => cursor,
        _ => 0,
    };
    let rows_shown = (left.height as usize).saturating_sub(2 + said_rows(workbench, left.width));
    let mut state = ListState::default()
        .with_selected(Some(cursor))
        .with_offset(offset_within(
            workbench,
            "sample",
            &vec![1; entries.len()],
            rows_shown,
        ));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::new().reversed())
            .block(
                framed_saying(workbench, left.width)
                    .title(" values ")
                    .title_bottom(element_line(workbench, left.width)),
            ),
        left,
        &mut state,
    );
    keep_offset(workbench, "sample", state.offset());
    say_inside(
        frame,
        left,
        &said_on_screen(workbench),
        said_rows(workbench, left.width),
    );
    if hidden {
        return None;
    }
    // The note alone: the tags are the first of the values.
    let text = workbench
        .current()
        .map(|(_, sample)| sample.note().trim().to_string())
        .unwrap_or_default();
    if matches!(workbench.mode, Mode::Note { .. }) {
        // Drawn by its editor once the screen is: where it goes.
        NOTE_EDITED.with(|edited| edited.set(Some(right)));
        return None;
    }
    // Shown as the Markdown it is: headings, lists, emphasis, code, scrolled as
    // far as J, K and the wheel took it — never past its end.
    let rendered = tui_markdown::from_str(&text);
    let inside = right.width.saturating_sub(4).max(1) as usize;
    let rows: usize = rendered
        .lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inside))
        .sum();
    let height = right.height.saturating_sub(2) as usize;
    let path = workbench.current().map(|(path, _)| path);
    let scrolled = if workbench.note_scroll.0 == path {
        workbench.note_scroll.1
    } else {
        0
    };
    let scroll = scrolled.min(rows.saturating_sub(height));
    let hint = if rows > height {
        "N edit · n hide note · J K scroll"
    } else {
        "N edit · n hide note"
    };
    frame.render_widget(
        Paragraph::new(rendered)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0))
            .block(framed().title(" note ").title_bottom({
                // Said as the values' frame says its keys.
                let mut line = hint_spans(hint);
                line.spans.push(Span::raw(" "));
                line.right_aligned()
            })),
        right,
    );
    NOTE_SHOWN.with(|shown| {
        shown.set(Some((
            (right.x, right.y, right.width, right.height),
            scroll,
        )))
    });
    None
}

/// A frame's place on the screen — x, y, width, height — and a scroll.
type Shown = Option<((u16, u16, u16, u16), usize)>;

thread_local! {
    /// Where the note being edited goes, which its editor draws.
    static NOTE_EDITED: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
    /// Where the note was last drawn, and the scroll it was drawn at, which
    /// `draw` gives the model: the wheel over it scrolls it, and a scroll
    /// past its end is brought back.
    static NOTE_SHOWN: std::cell::Cell<Shown> =
        const { std::cell::Cell::new(None) };
}

/// The note being edited, drawn by its editor in `area`: the text as typed,
/// unwrapped, its Markdown marked without changing a character as
/// `rat-markdown` reads it — a heading in the key colour and bold, a list's
/// marker coloured, code muted, emphasis, strength and a link as they are — the
/// selection reversed, the terminal's cursor at the editor's.
fn written(frame: &mut Frame, area: Rect, text: &mut Written) -> NoteView {
    let theme = colors();
    let heading = Style::new().fg(theme.key).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let widget = TextArea::new()
        .block(
            framed()
                .border_style(Style::new().fg(theme.selected))
                .title(" note · editing ")
                .title_bottom(
                    Line::from(" Esc done, previewed ")
                        .right_aligned()
                        .style(Style::new().fg(theme.muted)),
                ),
        )
        .select_style(selection_style())
        .text_style_idx(MDStyle::Heading1 as usize, heading)
        .text_style_idx(MDStyle::Heading2 as usize, heading)
        .text_style_idx(MDStyle::Heading3 as usize, heading)
        .text_style_idx(MDStyle::Heading4 as usize, heading)
        .text_style_idx(MDStyle::Heading5 as usize, heading)
        .text_style_idx(MDStyle::Heading6 as usize, heading)
        .text_style_idx(MDStyle::ItemTag as usize, Style::new().fg(theme.selected))
        .text_style_idx(MDStyle::CodeBlock as usize, muted)
        .text_style_idx(MDStyle::CodeInline as usize, muted)
        .text_style_idx(
            MDStyle::Emphasis as usize,
            Style::new().add_modifier(Modifier::ITALIC),
        )
        .text_style_idx(
            MDStyle::Strong as usize,
            Style::new().add_modifier(Modifier::BOLD),
        )
        .text_style_idx(
            MDStyle::Strikethrough as usize,
            Style::new().add_modifier(Modifier::CROSSED_OUT),
        )
        .text_style_idx(
            MDStyle::Link as usize,
            Style::new().add_modifier(Modifier::UNDERLINED),
        )
        .text_style_idx(
            MDStyle::Image as usize,
            Style::new().add_modifier(Modifier::UNDERLINED),
        );
    frame.render_stateful_widget(widget, area, text.state());
    if let Some(at) = text.screen_cursor() {
        frame.set_cursor_position(at);
    }
    let state = text.state();
    NoteView {
        x: state.inner.x,
        y: state.inner.y,
        width: state.inner.width,
        height: state.inner.height,
        top: state.vscroll.offset(),
        left: state.hscroll.offset(),
    }
}

/// A line typed, drawn by its editor in `area`: the text as typed, scrolled
/// sideways to keep its cursor in sight, the selection reversed, the terminal's
/// cursor at the editor's where it takes the keys. The start page draws its own
/// the same way.
pub fn draw_typed(frame: &mut Frame, area: Rect, typed: &mut Typed, style: Style) {
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        TextInput::new()
            .style(style)
            .select_style(selection_style()),
        area,
        typed.state(),
    );
    if let Some(at) = typed.screen_cursor() {
        frame.set_cursor_position(at);
    }
}

/// What a modal keeps in sight: a scroll it was given, or a cursor's line.
#[derive(Clone, Copy)]
enum Focus {
    Scroll(usize),
    Cursor(usize),
}

/// Rows a filter's window keeps for what it says as it is typed: the text, a
/// blank, ten completions, and at its foot an error of up to four rows once
/// wrapped. Kept whether used or not, so that the window does not change size
/// at each key.
const FILTER_ROWS: usize = 16;

/// A value's colour by its state, as the sample screen and a table give it.
fn state_colour(state: &str) -> Color {
    let theme = colors();
    if state.starts_with("failed") {
        theme.failed
    } else if state.starts_with("outdated") || state == "not judged" || state == "broken" {
        theme.outdated
    } else if state == "edited" || state == "record missing" {
        theme.edited
    } else if state == "current" {
        theme.current
    } else if state == super::model::NO_STATISTIC {
        theme.muted
    } else if state.contains("never computed") || state.starts_with("waiting") {
        theme.outdated
    } else {
        Color::Reset
    }
}

/// A modal window: its title on the top border, the keys that act in it on
/// the bottom one, centred. Every modal is one width — two thirds of the
/// screen, 60 to 100 columns — its lines wrapped inside; its height is `rows`
/// where given, so that it holds still while it is typed in, and its content's
/// otherwise.
fn modal(
    frame: &mut Frame,
    title: &str,
    actions: &str,
    lines: Vec<Line<'static>>,
    focus: Focus,
    rows: Option<usize>,
    status: &[Line<'static>],
) -> usize {
    // Within the main frame, as the help is: a tall window was drawn over
    // the status line and the hints.
    let main = MAIN_AREA.with(std::cell::Cell::get);
    let area = if main.width > 0 && main.height >= 3 {
        main
    } else {
        frame.area()
    };
    // A list is not wrapped: a line is an item, and the cursor's line the
    // row to keep in sight — a wrapped label pushed it out of the window.
    let list = matches!(focus, Focus::Cursor(_));
    // What wraps is read as prose, and is no wider than prose is read; a list
    // keeps its width.
    let widest = if list { 100 } else { READING_WIDTH as usize };
    let width = (area.width as usize * 2 / 3)
        .clamp(60, widest)
        .min(area.width.saturating_sub(4) as usize);
    // Border and padding: two columns each side.
    let inner = width.saturating_sub(4).max(1);
    let rows_of = |lines: &[Line]| -> usize {
        lines
            .iter()
            .map(|line| line.width().max(1).div_ceil(inner))
            .sum()
    };
    let wrapped = if list { lines.len() } else { rows_of(&lines) };
    // What is said wraps as the rest does, and takes the rows it needs.
    let said = rows_of(status);
    // Its keys on the bottom border, and those it cannot hold on the row inside
    // above it, the window a row taller: cut to the border, the sort's lost `/
    // narrow` at 80 columns.
    let keys = wrap_window_hints(actions, width.saturating_sub(4), 2);
    let above = keys.len().saturating_sub(1);
    // What is said — an error, an outcome — holds the last rows inside the
    // window, under what it shows: `rows` counts them. Inside the main frame's
    // borders, never on them: as tall as the frame, its corners fell on the
    // frame's border. What it cannot hold scrolls.
    let height = (rows.unwrap_or(wrapped + said) + 2 + above)
        .min(area.height.saturating_sub(2) as usize)
        .max(3);
    // At least a row of what the window shows, whatever is said and its
    // keys: on a small screen the error was drawn over the very line being
    // typed.
    let above = above.min(height.saturating_sub(3));
    let said = said.min(height.saturating_sub(3 + above));
    let shown_rows = height.saturating_sub(2 + said + above).max(1);
    // Scrolled no further than the last row at the window's foot: past it,
    // the window emptied to one line.
    let scroll = match focus {
        Focus::Scroll(scroll) => scroll.min(wrapped.saturating_sub(shown_rows)),
        Focus::Cursor(line) => line.saturating_sub(shown_rows.saturating_sub(1)),
    };
    let at = Rect {
        x: area.x + (area.width.saturating_sub(width as u16)) / 2,
        y: area.y + (area.height.saturating_sub(height as u16)) / 2,
        width: width as u16,
        height: height as u16,
    };
    frame.render_widget(Clear, at);
    let paragraph = if list {
        Paragraph::new(lines)
    } else {
        Paragraph::new(lines).wrap(Wrap { trim: false })
    };
    frame.render_widget(
        paragraph.scroll((scroll as u16, 0)).block(
            framed()
                .padding(Padding {
                    left: 1,
                    right: 1,
                    top: 0,
                    bottom: (said + above) as u16,
                })
                .title(
                    Line::from(format!(" {title} "))
                        .style(Style::new().add_modifier(Modifier::BOLD)),
                )
                .title_bottom({
                    // As many of its keys as two rows hold, `Enter` and
                    // `Esc` kept to the last; a window too short for two,
                    // those one holds.
                    let last = if above + 1 == keys.len() {
                        keys.last().cloned().unwrap_or_default()
                    } else {
                        fit_hints(actions, width.saturating_sub(4))
                    };
                    let mut line = hint_spans(&last);
                    line.spans.push(Span::raw(" "));
                    line.right_aligned()
                }),
        ),
        at,
    );
    // Where its lines are, and how far scrolled: a line typed is drawn
    // there by its editor.
    MODAL_SHOWN.with(|shown| {
        shown.set((
            Rect {
                x: at.x + 2,
                y: at.y + 1,
                width: at.width.saturating_sub(4),
                height: shown_rows as u16,
            },
            scroll,
        ))
    });
    if above > 0 {
        // The first of its keys on the row above the border, lined up with
        // those on it.
        let row = Rect {
            x: at.x + 1,
            y: (at.y + at.height).saturating_sub(1 + above as u16),
            width: at.width.saturating_sub(2),
            height: above as u16,
        };
        let lines: Vec<Line<'static>> = keys[..above]
            .iter()
            .map(|line| {
                let mut line = hint_spans(line);
                line.spans.push(Span::raw(" "));
                line.right_aligned()
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), row);
    }
    // What is said above its keys.
    let over_keys = Rect {
        height: at.height.saturating_sub(above as u16),
        ..at
    };
    say_inside(frame, over_keys, status, said);
    scroll
}

/// Below this width the sample's note goes under its values, not beside.
const NARROW: u16 = 60;

/// The widest a pane or a window of prose is drawn, its frame included: 80
/// columns of text inside a frame and its margins — the note, what a snapshot
/// changed, where a value came from. On a wide terminal the note was given two
/// hundred columns for three lines.
const READING_WIDTH: u16 = 84;

/// What is said, on the last rows inside a framed area — over the padding
/// the block kept free for it.
fn say_inside(frame: &mut Frame, area: Rect, status: &[Line<'static>], rows: usize) {
    if status.is_empty() || area.height < 3 {
        return;
    }
    let said = Rect {
        x: area.x + 2,
        y: (area.y + area.height).saturating_sub(1 + rows as u16),
        width: area.width.saturating_sub(4),
        height: rows as u16,
    };
    frame.render_widget(
        Paragraph::new(status.to_vec()).wrap(Wrap { trim: false }),
        said,
    );
}

/// The last action's words, for the screen behind no window: a window open
/// says them itself.
fn said_on_screen(workbench: &Workbench) -> Vec<Line<'static>> {
    if workbench.message.is_empty() || !matches!(workbench.mode, Mode::Normal | Mode::Note { .. }) {
        return Vec::new();
    }
    // Each line of it: the second of an error is often its way out.
    workbench
        .message
        .lines()
        .map(|line| Line::from(keyed(line, Style::new().fg(colors().message))))
        .collect()
}

/// The rows what is said takes inside a frame `width` wide, wrapped: three at
/// most, so that a long message never hides what it is about.
fn said_rows(workbench: &Workbench, width: u16) -> usize {
    let inner = (width as usize).saturating_sub(4).max(1);
    said_on_screen(workbench)
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner))
        .sum::<usize>()
        .min(3)
}

/// A screen's block, the rows kept free at its foot while something is said.
fn framed_saying(workbench: &Workbench, width: u16) -> Block<'static> {
    framed().padding(Padding {
        left: 1,
        right: 1,
        top: 0,
        bottom: said_rows(workbench, width) as u16,
    })
}

/// A list in a modal window, the cursor's line highlighted.
fn modal_list(
    frame: &mut Frame,
    title: &str,
    actions: &str,
    items: Vec<String>,
    cursor: usize,
    rows: Option<usize>,
    status: &[Line<'static>],
) {
    let lines = items
        .into_iter()
        .enumerate()
        .map(|(at, item)| {
            if at == cursor {
                Line::styled(item, Style::new().reversed())
            } else {
                // The figure's `— Enter chooses`, the picker's `W saves…`.
                Line::from(keyed(&item, Style::new()))
            }
        })
        .collect::<Vec<_>>();
    modal(
        frame,
        title,
        actions,
        lines,
        Focus::Cursor(cursor),
        rows,
        status,
    );
}

/// Draws the window over the screen; a confirmation's scroll as it could be
/// drawn, which the model keeps.
fn overlay(frame: &mut Frame, workbench: &mut Workbench) -> Option<usize> {
    // What the last action said, at the foot of the window it was said in.
    let said: Vec<Line<'static>> = if workbench.message.is_empty() {
        Vec::new()
    } else {
        vec![Line::from(keyed(
            &workbench.message,
            Style::new().fg(colors().message),
        ))]
    };
    let status = said.as_slice();
    // Where each line typed goes: its line in the window, and how far in.
    let mut typed: Vec<(usize, u16)> = Vec::new();
    match &workbench.mode {
        Mode::Normal => {}
        Mode::Filter {
            candidates,
            chosen,
            error,
            ..
        } => {
            let mut lines = vec![Line::from(Span::styled("/ ", key_style(&colors())))];
            typed.push((0, 2));
            // An error holds the foot of the window, where the rows for it are
            // kept whether it is there or not.
            let errors: Vec<Line<'static>> = error
                .iter()
                .flat_map(|error| error.lines())
                .take(3)
                .map(|line| Line::styled(line.to_string(), Style::new().fg(colors().error)))
                .collect();
            let status = if errors.is_empty() {
                status
            } else {
                errors.as_slice()
            };
            if !candidates.is_empty() {
                lines.push(Line::raw(""));
            }
            for (at, candidate) in candidates.iter().take(10).enumerate() {
                let style = if Some(at) == *chosen {
                    Style::new().reversed()
                } else {
                    Style::new().fg(colors().muted)
                };
                lines.push(Line::styled(candidate.clone(), style));
            }
            modal(
                frame,
                "filter",
                "Tab complete · ← → move · Enter apply · Esc cancel",
                lines,
                Focus::Scroll(0),
                Some(FILTER_ROWS),
                status,
            );
        }
        Mode::Prompt { purpose, .. } => {
            let (title, actions) = match purpose {
                super::model::Purpose::Edit { field, .. } => {
                    (format!("{field} ="), "Enter preview · Esc cancel")
                }
                super::model::Purpose::Tags { .. } => (
                    "tags, by commas or spaces".to_string(),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::Setting { key, .. } => (
                    format!("{key} ="),
                    "Enter set · empty removes it · Esc cancel",
                ),
                super::model::Purpose::AddKey { section, name } => (
                    format!(
                        "a key of {name} in {}: key = value",
                        super::model::SECTIONS[*section].0
                    ),
                    "Enter add · Esc cancel",
                ),
                super::model::Purpose::AddSetting { section } => (
                    format!(
                        "a setting in {}: {}",
                        super::model::SECTIONS[*section].0,
                        match super::model::SECTIONS[*section].1 {
                            super::model::Section::Named(_) => "name.key = value",
                            super::model::Section::Setting(_) => "key = value",
                        }
                    ),
                    "Enter add · Esc cancel",
                ),
                super::model::Purpose::DeclareName(_) => {
                    ("its name".to_string(), "Enter preview · Esc cancel")
                }
                super::model::Purpose::FigureText(row) => (
                    format!("your figure: {}", row.label()),
                    "Enter keep · empty gives the field's · Esc back",
                ),
                super::model::Purpose::New { .. } => (
                    "a new sample's name".to_string(),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::Row { table, .. } => (
                    format!("a row in {table}: column=value, …"),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::Tag => (
                    "tag the basket with, or -tag to take it off".to_string(),
                    "Enter preview · Esc cancel",
                ),
                // The setup's model one already has.
                super::model::Purpose::ModelPath => (
                    "your model file's path, from the project's folder".to_string(),
                    "Enter use it · Esc back to the question",
                ),
            };
            modal(
                frame,
                &title,
                actions,
                vec![Line::raw("")],
                Focus::Scroll(0),
                None,
                status,
            );
            typed.push((0, 0));
        }
        Mode::Picker {
            purpose,
            items,
            cursor,
            query,
            typing,
        } => {
            let (title, actions, ticks) = match purpose {
                super::model::Choosing::Sort => (
                    "sort by, first to last",
                    "Space tick · r reverse · ⇧↑↓ move · / narrow · Enter sort · Esc cancel",
                    true,
                ),
                super::model::Choosing::Columns => (
                    "columns, left to right",
                    "Space tick · ⇧↑↓ move · / narrow · Enter show · Esc cancel",
                    true,
                ),
                super::model::Choosing::Produce(_) => (
                    "figures and exports",
                    "/ narrow · Enter draw or preview · Esc cancel",
                    false,
                ),
                // It applies to what is shown, and goes nowhere.
                super::model::Choosing::GoTo(_) => (
                    "apply a saved query or a profile",
                    "/ narrow · Enter apply · Esc cancel",
                    false,
                ),
                super::model::Choosing::Group => (
                    "group by, first to last",
                    "Space tick · ⇧↑↓ move · / narrow · Enter group · Esc cancel",
                    true,
                ),
                super::model::Choosing::Declare(_) => (
                    "save what is shown in .samplekitrc",
                    "Enter name it · Esc cancel",
                    false,
                ),
                super::model::Choosing::FigureField(row) => (
                    match row {
                        super::model::FigureRow::Y => "your figure: y, up",
                        super::model::FigureRow::X => "your figure: x, across",
                        _ => "your figure: grouped by, a colour each",
                    },
                    "/ narrow · Enter choose · Esc back",
                    false,
                ),
            };
            let actions = if *typing {
                "type to narrow · Space tick · Enter accept · Esc clear"
            } else {
                actions
            };
            let shown = super::model::visible(items, query.text());
            let mut lines: Vec<String> = shown
                .iter()
                .map(|at| {
                    let (label, ticked) = &items[*at];
                    // Ungrouping is an item to choose, not a field to tick.
                    if ticks && label != super::model::NO_GROUPING {
                        // A sort key's `-` is said as the way it runs.
                        let (label, running) = match label.strip_prefix('-') {
                            Some(field) => (field, "  ↓ descending"),
                            None => (label.as_str(), ""),
                        };
                        // A list sorts by an item: said which, rather than
                        // its address alone.
                        let item = label
                            .strip_suffix("[#0]")
                            .filter(|list| !list.contains('.'))
                            .map(|list| format!("  the first item of {list}"))
                            .unwrap_or_default();
                        format!(
                            "[{}] {label}{running}{item}",
                            if *ticked { "x" } else { " " }
                        )
                    } else {
                        label.clone()
                    }
                })
                .collect();
            let mut at = *cursor;
            if *typing || !query.is_empty() {
                // The narrowing's line, drawn by its editor after the `/`.
                lines.insert(0, "/".to_string());
                typed.push((0, 1));
                at += 1;
            }
            if shown.is_empty() {
                lines.push(
                    match purpose {
                        // Empty before any narrowing: the project declares none.
                        super::model::Choosing::GoTo(_) if query.is_empty() => {
                            "no query or profile is declared here — W saves the filter or the \
                             columns as one"
                        }
                        _ => "nothing holds it",
                    }
                    .to_string(),
                );
            }
            // The whole choice, in its order, at the foot: what a narrowed
            // list no longer shows.
            let mut footer: Vec<Line<'static>> = Vec::new();
            if ticks {
                let chosen: Vec<Span<'static>> = items
                    .iter()
                    .filter(|(_, ticked)| *ticked)
                    .enumerate()
                    .flat_map(|(at, (label, _))| {
                        let (field, arrow) = match label.strip_prefix('-') {
                            Some(field) => (field.to_string(), " ↓"),
                            None => (label.clone(), " ↑"),
                        };
                        let sorting = matches!(purpose, super::model::Choosing::Sort);
                        let mut spans = Vec::new();
                        if at > 0 {
                            spans.push(Span::styled(" · ", Style::new().fg(colors().muted)));
                        }
                        spans.push(Span::styled(field, Style::new().fg(colors().selected)));
                        if sorting {
                            spans.push(Span::styled(arrow, Style::new().fg(colors().selected)));
                        }
                        spans
                    })
                    .collect();
                let mut line = vec![Span::styled(
                    match purpose {
                        super::model::Choosing::Sort => "sorted by  ",
                        super::model::Choosing::Group => "grouped by  ",
                        _ => "shown  ",
                    },
                    Style::new().fg(colors().muted),
                )];
                if chosen.is_empty() {
                    line.extend(keyed(
                        match purpose {
                            super::model::Choosing::Sort => {
                                "nothing ticked: Enter sorts by the one under the cursor"
                            }
                            super::model::Choosing::Group => {
                                "nothing ticked: Enter groups by the one under the cursor"
                            }
                            _ => "nothing ticked: a table needs one column",
                        },
                        Style::new().fg(colors().muted),
                    ));
                } else {
                    line.extend(chosen);
                }
                footer.push(Line::from(line));
            }
            footer.extend(status.iter().cloned());
            let status = footer.as_slice();
            // As tall as the whole list and the line narrowing it, however
            // few items the narrowing leaves.
            modal_list(
                frame,
                title,
                actions,
                lines,
                at,
                Some(items.len() + 1 + status.len()),
                status,
            );
        }
        Mode::Confirm {
            title,
            lines,
            pending,
        } => {
            return Some(modal(
                frame,
                title,
                super::model::confirm_action(pending),
                // A key it names in its colour: *u gives it back*.
                lines
                    .iter()
                    .map(|line| Line::from(keyed(line, Style::new())))
                    .collect(),
                Focus::Scroll(workbench.confirm_scroll),
                None,
                status,
            ));
        }
        Mode::Panel {
            title,
            lines,
            scroll,
        } => {
            return Some(modal(
                frame,
                title,
                "j k scroll · any other key closes",
                lines.iter().cloned().map(Line::raw).collect(),
                Focus::Scroll(*scroll),
                None,
                status,
            ));
        }
        Mode::Note { .. } => {}
        Mode::Quantity {
            field, slots, on, ..
        } => {
            // Each label with its unit, `value (g)`, the texts lined up after
            // the longest.
            let labelled = |slot: &super::model::Slot| {
                if slot.unit.is_empty() {
                    slot.label.to_string()
                } else {
                    format!("{} ({})", slot.label, slot.unit)
                }
            };
            let width = slots
                .iter()
                .map(|slot| labelled(slot).chars().count() + 1)
                .max()
                .unwrap_or(0)
                .max(13);
            let mut lines: Vec<Line<'static>> = slots
                .iter()
                .enumerate()
                .map(|(at, slot)| {
                    let active = at == *on;
                    let style = if active {
                        Style::new().fg(colors().selected)
                    } else {
                        Style::new().fg(colors().muted)
                    };
                    typed.push((at, width as u16));
                    Line::styled(format!("{:<width$}", labelled(slot)), style)
                })
                .collect();
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                if slots.len() > 2 {
                    "the number alone, a point for decimals · an empty uncertainty clears it · readings by commas"
                } else {
                    "the number alone, a point for decimals · an empty uncertainty clears it"
                },
                Style::new().fg(colors().muted),
            ));
            modal(
                frame,
                field,
                "Tab next · Shift+Tab back · Enter preview · Esc cancel",
                lines,
                Focus::Scroll(0),
                None,
                status,
            );
        }
        Mode::Help { scroll } => help_window(frame, workbench, *scroll),
        // The figure's window: what it draws, then how, a row each.
        Mode::Figure => {
            if let Some(form) = &workbench.figure {
                let rows = form.rows();
                let items: Vec<String> = rows
                    .iter()
                    .map(|row| format!("{:<14} {}", row.label(), form.shown(*row)))
                    .collect();
                let title = match &form.of {
                    Some((_, table)) => format!("a figure of {table}"),
                    None => "a figure of your own".to_string(),
                };
                modal_list(
                    frame,
                    &title,
                    "Enter change · ←→ choose · p draw · w save · Esc cancel",
                    items,
                    form.row,
                    Some(rows.len() + 1 + status.len()),
                    status,
                );
            }
        }
        Mode::Files { files, cursor } => {
            let items: Vec<String> = files
                .iter()
                .map(|file| {
                    file.strip_prefix(&workbench.root)
                        .unwrap_or(file)
                        .display()
                        .to_string()
                })
                .collect();
            modal_list(
                frame,
                "its files",
                "Enter open · n folder · Esc cancel",
                items,
                *cursor,
                None,
                status,
            );
        }
    };
    // Each line typed drawn by its editor where the window put it.
    let places: Vec<Option<Rect>> = typed
        .iter()
        .map(|(row, from)| modal_row(*row, *from))
        .collect();
    for (line, place) in typed_lines(&mut workbench.mode).into_iter().zip(places) {
        if let Some(area) = place {
            draw_typed(frame, area, line, Style::new());
        }
    }
    None
}

/// A bordered block with a space between its border and what it holds.
fn framed() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
}

/// A state as the one character a table marks it with, for a pane too narrow
/// to say it in words.
fn state_mark(state: &str) -> &'static str {
    if state.is_empty() || state == "current" || state == "entered" {
        ""
    } else if state == super::model::NO_STATISTIC {
        "·"
    } else if state.contains("failed") {
        "✗"
    } else if state.contains("never computed") || state.contains("waiting") {
        "∅"
    } else if state.contains("edited") || state.contains("record missing") {
        "✎"
    } else {
        "⚠"
    }
}

/// Where the list or table `name` was scrolled to when last drawn.
fn offset_of(workbench: &Workbench, name: &'static str) -> usize {
    workbench.offsets.borrow().get(name).copied().unwrap_or(0)
}

/// Where the list `name` was scrolled to, no further than what fills its
/// `rows` with its last items — each item `heights` rows tall: kept from a
/// smaller terminal, the offset left samples hidden above and rows empty
/// below once it grew.
fn offset_within(
    workbench: &Workbench,
    name: &'static str,
    heights: &[usize],
    rows: usize,
) -> usize {
    let mut furthest = heights.len();
    let mut filled = 0;
    while furthest > 0 && filled + heights[furthest - 1] <= rows {
        furthest -= 1;
        filled += heights[furthest];
    }
    offset_of(workbench, name).min(furthest)
}

/// Keeps where `name` is scrolled to now, for the next frame to start from
/// (see `Workbench::offsets`).
fn keep_offset(workbench: &Workbench, name: &'static str, offset: usize) {
    workbench.offsets.borrow_mut().insert(name, offset);
}

/// The history screen: the snapshots, newest first, and beside them what the
/// one under the cursor changed, value by value, as `diff` says it. The
/// history's two panes; how far what it changed may scroll.
fn history(frame: &mut Frame, workbench: &Workbench, area: Rect) -> usize {
    let Screen::History { cursor, scroll, .. } = workbench.screen else {
        return 0;
    };
    let theme = colors();
    // What it changed is read as prose: at most the reading width, the
    // snapshots taking the rest.
    let changes = (area.width * 55 / 100).min(READING_WIDTH);
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(changes)]).areas(area);
    // Each snapshot on a line where it fits, its message under its number
    // and date where it does not — cut at its end: beside the date, at 80
    // columns, it was cut to `workbenc`.
    let inside = (left.width as usize).saturating_sub(4);
    let items: Vec<ListItem> = workbench
        .history
        .iter()
        .map(|row| {
            let number = Span::styled(format!("{:>3}  ", row.number), Style::new().fg(theme.muted));
            let when = Span::styled(format!("{}  ", row.when), Style::new().fg(theme.muted));
            let lead = number.width() + when.width();
            if lead + row.message.chars().count() <= inside {
                ListItem::new(Line::from(vec![
                    number,
                    when,
                    Span::raw(row.message.clone()),
                ]))
            } else {
                let lead = ellipsized(&format!("{}{}", number.content, when.content), inside);
                ListItem::new(vec![
                    Line::styled(lead, Style::new().fg(theme.muted)),
                    Line::from(vec![
                        Span::raw("     "),
                        Span::raw(ellipsized(&row.message, inside.saturating_sub(5))),
                    ]),
                ])
            }
        })
        .collect();
    let heights: Vec<usize> = items.iter().map(ListItem::height).collect();
    let rows_shown = (left.height as usize).saturating_sub(2 + said_rows(workbench, left.width));
    let mut state = ListState::default()
        .with_selected(Some(cursor))
        .with_offset(offset_within(workbench, "history", &heights, rows_shown));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::new().reversed())
            .block(
                framed_saying(workbench, left.width)
                    .title(format!(" {} kept ", workbench.history.len())),
            ),
        left,
        &mut state,
    );
    keep_offset(workbench, "history", state.offset());
    let Some(row) = workbench.history.get(cursor) else {
        return 0;
    };
    // Wrapped, each line under its own indent: cut at the pane's edge, what
    // a change said was lost past 80 columns.
    let width = (right.width as usize).saturating_sub(4).max(8);
    let mut lines: Vec<Line> = Vec::new();
    let mut push = |text: &str, style: Style| {
        let indent = text
            .chars()
            .take_while(|c| *c == ' ')
            .count()
            .min(width / 2);
        let first = text.trim_start();
        // Its spaces kept: a change's columns line up.
        let options = textwrap::Options::new(width.saturating_sub(indent).max(4))
            .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
        let parts: Vec<String> = if first.is_empty() {
            Vec::new()
        } else {
            textwrap::wrap(first, options)
                .into_iter()
                .map(|part| part.into_owned())
                .collect()
        };
        if parts.is_empty() {
            lines.push(Line::default());
        }
        for part in parts {
            lines.push(Line::from(Span::styled(
                format!("{}{part}", " ".repeat(indent)),
                style,
            )));
        }
    };
    let muted = Style::new().fg(theme.muted);
    match &row.machine {
        Some(machine) => push(
            &format!("#{} · {} · {machine}", row.number, row.when),
            muted,
        ),
        None => push(&format!("#{} · {}", row.number, row.when), muted),
    }
    push(&row.message, Style::new());
    push(&format!("changed: {}", row.changed), muted);
    push("", Style::new());
    for line in &row.lines {
        if line.starts_with("  ") {
            push(line, Style::new());
        } else {
            push(line, Style::new().fg(theme.accent));
        }
    }
    let furthest = lines
        .len()
        .saturating_sub(right.height.saturating_sub(2) as usize);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll.min(furthest) as u16, 0))
            // `J` `K` scroll this pane: said under it, not under the list
            // `j` `k` move in.
            .block(
                framed_saying(workbench, right.width)
                    .title(" what it changed ")
                    .title_bottom(element_line(workbench, right.width)),
            ),
        right,
    );
    furthest
}
