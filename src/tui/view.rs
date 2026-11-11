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
use super::theme::{colors, is_key, key_style, keyed, selection_style, value_spans, value_style};
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
    drawn(frame, workbench);
    // What was left to the terminal's colours, the theme's.
    super::theme::paint(frame.buffer_mut(), &workbench.theme);
}

fn drawn(frame: &mut Frame, workbench: &mut Workbench) {
    // A short terminal gives its rows to the samples: the title, the status
    // and the hints go first, `?` still there — at six rows the collection
    // drew its header and no sample.
    let short = frame.area().height < 10;
    let chrome = u16::from(!short);
    // The keys at the foot, on a second line where one cannot hold them, the
    // main frame a line shorter: cut to one, they lost their end.
    let room = frame.area().width.saturating_sub(2) as usize;
    let typing_here = workbench.typing
        && matches!(
            workbench.screen,
            Screen::Collection | Screen::Sample { .. } | Screen::Table { .. }
        );
    let foot = if typing_here {
        wrap_hints(TYPING_KEYS, room.saturating_sub(10), 2)
            .into_iter()
            .map(|line| format!(" {line}"))
            .collect()
    } else if matches!(workbench.mode, Mode::Note { .. }) {
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
        Screen::Sample { .. } | Screen::Table { .. } | Screen::Items { .. } => {
            match workbench.current() {
                Some((path, sample)) => {
                    let name = sample
                        .name()
                        .map(str::to_string)
                        .unwrap_or_else(|| path.display().to_string());
                    // The way taken by links, each sample left, then this one.
                    workbench
                        .trail
                        .iter()
                        .map(|step| step.name.as_str())
                        .chain(std::iter::once(name.as_str()))
                        .collect::<Vec<_>>()
                        .join(" › ")
                }
                None => String::new(),
            }
        }
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
        // Cut where the bar ends, and said cut: `— C com` read as a word.
        let room = (top.width as usize).saturating_sub(" SampleKit ".chars().count() + 1);
        let said = if said.chars().count() > room {
            let kept: String = said.chars().take(room.saturating_sub(1)).collect();
            format!("{}…", kept.trim_end())
        } else {
            said
        };
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
        // A table or a list in a window over its sample.
        Screen::Table { .. } | Screen::Items { .. } => {
            NOTE_SHOWN.with(|shown| shown.set(None));
            NOTE_EDITED.with(|edited| edited.set(None));
            workbench.note_view = sample(frame, workbench, main);
            let window = window_area(main, table_extent(workbench));
            frame.render_widget(Clear, window);
            match workbench.screen {
                Screen::Table { .. } => table(frame, workbench, window),
                _ => items_window(frame, workbench, window),
            }
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
    frame.render_widget(Paragraph::new(status_line(workbench, status.width)), status);
    // A value's card over its screen.
    if workbench.card.is_some()
        && matches!(
            workbench.screen,
            Screen::Sample { .. } | Screen::Table { .. }
        )
    {
        card_window(frame, workbench, main);
    }
    // The cell typed in the typing mode, typed where it is: it never widens.
    if let Some(area) = CELL_TYPED.with(|cell| cell.take())
        && let Mode::Prompt {
            purpose: super::model::Purpose::Cell(_),
            text,
        } = &mut workbench.mode
    {
        draw_typed(
            frame,
            area,
            text,
            Style::new().fg(colors().value).add_modifier(Modifier::BOLD),
        );
    }
    // The note being edited has keys of its own, which the foot says: the
    // sample's said what its keys no longer did while it was typed in.
    let mut foot: Vec<Line<'static>> = foot.iter().map(|line| hint_spans(line)).collect();
    // The typing mode said before its keys, as an editor says its mode.
    if typing_here && let Some(first) = foot.first_mut() {
        first.spans.insert(
            0,
            Span::styled(
                " TYPING ",
                Style::new()
                    // The theme's background on its accent; black where the
                    // terminal's is left to it.
                    .fg(match colors().background {
                        Color::Reset => Color::Black,
                        background => background,
                    })
                    .bg(colors().accent)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }
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
        Mode::Script { line } => vec![line],
        Mode::Picker { query, typing, .. } if *typing || !query.is_empty() => vec![query],
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
                        // `j` and `k` scroll whether or not there is more to
                        // see: said so, where *any key closes* was not true.
                        let mut line = hint_spans("j k scroll · any other key closes");
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

/// The computation as the title bar says it: `⟳ [███░░░] 3/6 · B-01 · modulus`
/// under way, then `✓ 12 computed, 1 failed`; and whether it is over.
fn run_said(workbench: &Workbench) -> Option<(String, bool)> {
    let run = workbench.run.as_ref()?;
    Some(match &run.outcome {
        // A tick for what was computed; nothing computed is no success.
        Some(outcome) if outcome.starts_with("nothing") || outcome.contains(" failed") => {
            (format!("· {outcome}"), true)
        }
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

/// What the collection shows, on the line under it: cut with an ellipsis
/// where it is wider, `values: 1 s` read at 60 columns.
fn status_line(workbench: &Workbench, width: u16) -> Line<'static> {
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
    // A file set aside unread shows on no line of the collection: counted
    // here, where it was said nowhere but on `v`.
    let unread = workbench
        .collection
        .skipped()
        .iter()
        .filter(|skipped| {
            !matches!(
                skipped.reason,
                crate::config::discovery::SkipReason::Excluded { .. }
                    | crate::config::discovery::SkipReason::NoFrontmatter
                    | crate::config::discovery::SkipReason::NoVersion
            )
        })
        .count();
    if unread > 0 {
        parts.push(format!(
            "{unread} {} not read — v says why",
            if unread == 1 { "file" } else { "files" }
        ));
    }
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
    let waiting = workbench.waiting_count();
    if waiting > 0 {
        parts.push(format!(
            "{waiting} {} not written",
            if waiting == 1 { "file" } else { "files" }
        ));
    }
    // The value under the cursor, every digit of it, at the line's end:
    // every screen shows values rounded.
    let raw = workbench.raw_here().unwrap_or_default();
    let room = (width as usize).saturating_sub(1).max(1);
    let said = ellipsized(&parts.join(" · "), room);
    let left = said.chars().count();
    let raw_room = room.saturating_sub(left + 4);
    if raw.is_empty() || raw_room < 12 {
        return Line::raw(format!(" {said}"));
    }
    let raw = ellipsized(&raw, raw_room);
    let gap = room.saturating_sub(left + raw.chars().count());
    Line::from(vec![
        Span::raw(format!(" {said}{}", " ".repeat(gap))),
        Span::styled(raw, Style::new().fg(colors().muted)),
    ])
}

/// The keys of the typing mode, for the foot of the screen.
const TYPING_KEYS: &str = "type to replace the cell · Enter keep, down · Tab keep, right · ↑ ↓ ← → cells · F2 change · Del take out · Esc leave";

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
/// `? all keys` are kept to the last, the others given up from the end; those left
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
    /// (`tempering.` in `tempering.nitridation[50]`), then the name, then the unit.
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
            // bracket: `tempering.nitridation[50]`, never `diameter.u` alone.
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
/// `? all keys` always, so that a narrow terminal still says where the rest are.
pub fn hint_line(place: Place, filtered: bool, width: usize) -> String {
    hints_of(place, filtered, false, width)
}

/// The foot of a screen `width` wide: its hints on one line where it holds them
/// all, on two where it does not, broken between whole hints; the two still too
/// few, the least wanted given up from the end, `? all keys` kept. Each line begins
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
    // Over several projects, a summary per project, as `--summary` writes
    // one table per project: one pooled the projects' samples, their units
    // and precisions declared apart.
    let roots: std::collections::BTreeSet<std::path::PathBuf> = workbench
        .view
        .iter()
        .filter_map(|entry| entry.path.as_deref())
        .filter_map(|path| workbench.collection.config_for(path))
        .map(|config| config.root().to_path_buf())
        .collect();
    let mut groups: Vec<String> = Vec::new();
    if roots.len() > 1 {
        groups.push("project".to_string());
    }
    groups.extend(workbench.group.iter().cloned());
    // Grouped, a summary per group, its values in first columns on its first
    // row, as `--summary --group` writes it: one module makes both.
    let grouped = !groups.is_empty();
    let made = if grouped {
        groups
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
    let mixed = summaries::weighting_mixed(parts.iter().flat_map(|(_, rows)| rows));
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
                cells.extend(summaries::written(row, mixed));
                cells
            })
        })
        .collect();
    // The mean's heading says how it was taken, as `--summary`'s does.
    let mean = summaries::mean_header(parts.iter().flat_map(|(_, rows)| rows));
    let mut header: Vec<String> = if grouped { groups.clone() } else { Vec::new() };
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
    let labels = if grouped { groups.len() + 1 } else { 1 };
    let mut body = body;
    let mut widths: Vec<usize> = (0..header.len())
        .map(|at| {
            body.iter()
                .map(|cells| cells[at].chars().count())
                .max()
                .unwrap_or(0)
                .max(header[at].chars().count())
        })
        .collect();
    // **What does not fit is left out whole, never cut**: a count, a mean
    // cut from the left read as another number — `2` for `11/12`, `053` for
    // `1.053`. The labels give way first, cut with `…`; then the columns from
    // the right, named in the title.
    // Inside its borders and the padding `framed_saying` gives it, a column
    // each side: counted without, the widths fitted two columns too many, and
    // the terminal's layout cut `11/12` to `/12`.
    let inside = usize::from(area.width).saturating_sub(4);
    // Each rule between two columns takes three: itself and a space each side.
    let needed =
        |widths: &[usize]| widths.iter().sum::<usize>() + 3 * widths.len().saturating_sub(1);
    if needed(&widths) > inside {
        let label = labels - 1;
        let over = needed(&widths) - inside;
        widths[label] = widths[label].saturating_sub(over).max(8.min(widths[label]));
    }
    let mut hidden: Vec<String> = Vec::new();
    while needed(&widths) > inside && widths.len() > labels + 1 {
        widths.pop();
        if let Some(name) = header.pop() {
            hidden.insert(0, name);
        }
        for cells in &mut body {
            cells.pop();
        }
    }
    for cells in &mut body {
        for (at, cell) in cells.iter_mut().enumerate().take(labels) {
            if cell.chars().count() > widths[at] {
                let kept: String = cell.chars().take(widths[at].saturating_sub(1)).collect();
                *cell = format!("{kept}…");
            }
        }
    }
    let widths: Vec<Constraint> = widths
        .iter()
        .map(|width| Constraint::Length(*width as u16))
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
    // What the width leaves out first: the text left out follows, and a long
    // title is cut at its end.
    if !hidden.is_empty() {
        title = format!("{title}· {} past the edge ", hidden.join(", "));
    }
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
                .title_bottom({
                    let mut line = hint_spans("S the samples again");
                    line.spans.push(Span::raw(" "));
                    line.right_aligned()
                }),
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
    // The names in place, first, where the columns shown hold `name`: never a
    // column scrolled away with the rest, and gone when the picker takes it
    // away.
    let named = workbench
        .profile
        .columns()
        .iter()
        .any(|column| column.field == "name");
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
    // The names headed as the profile heads them — `Piece` — as the summary
    // says them: the collection said `name` where `S` said `Piece`.
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
    // Each column in the style of what it holds, as the command line paints
    // it: a property's values bold, an attribute's in its colour.
    let styles: Vec<Style> = data[first.min(data.len())..]
        .iter()
        .zip(&columns)
        .map(|(&at, cells)| column_style(workbench, at, cells))
        .collect();
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
            // A sample with a change waiting: named as a change, or as an
            // addition where it is made, until written.
            let waiting = path.and_then(crate::format::stage::entry);
            let name_style = if waiting.is_some() {
                Style::new()
                    .fg(colors().waiting)
                    .add_modifier(Modifier::ITALIC)
            } else {
                Style::new()
            };
            if named {
                cells.push(Cell::from(rendered.name.clone()).style(name_style));
            }
            cells.extend(columns.iter().zip(&styles).take(fits).enumerate().map(
                |(shown, (column, style))| {
                    // A link in the link's style, the column's otherwise.
                    let link = data
                        .get(first + shown)
                        .and_then(|at| rendered.links.get(*at).copied().flatten());
                    let style = match link {
                        Some(leads) => attribute_style(Some(leads)),
                        None => *style,
                    };
                    Cell::from(column[row].clone()).style(style)
                },
            ));
            // The rows `Shift+↑` `Shift+↓` took in the typing mode: what is
            // typed goes to each.
            let taken = workbench.typing
                && workbench.rows_from.is_some_and(|from| {
                    (from.min(workbench.cursor)..=from.max(workbench.cursor)).contains(&row)
                });
            let row_style = if taken {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            Row::new(separated(cells, separator(), 1)).style(row_style)
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
    let collection_lengths = separated(widths, Constraint::Length(1), 1);
    // The column typed in, in the typing mode: the profile's column, among
    // those shown — the mark first, then the names, then the data's.
    let typed_column = workbench.typing.then(|| {
        let field = workbench
            .profile
            .columns()
            .get(workbench.column)
            .map(|column| column.field.clone())?;
        let item = if field == "name" {
            1
        } else {
            let at = data.iter().position(|&at| at == workbench.column)?;
            let shown = at.checked_sub(hidden.0).filter(|shown| *shown < fits)?;
            1 + usize::from(named) + shown
        };
        Some(if item <= 1 { item } else { 2 * item - 1 })
    });
    let typed_column = typed_column.flatten();
    let table = Table::new(rows, collection_lengths.clone())
        .header(
            Row::new(separated(header, separator(), 1))
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        // In the typing mode, the line on its own background and the cell
        // typed in reversed on it; else the line reversed, as every list.
        .cell_highlight_style(Style::new().reversed())
        .row_highlight_style(line_style(workbench))
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
        .with_selected_column(typed_column)
        .with_offset(offset);
    frame.render_stateful_widget(table, area, &mut state);
    keep_offset(workbench, "collection", state.offset());
    if matches!(
        workbench.mode,
        Mode::Prompt {
            purpose: super::model::Purpose::Cell(_),
            ..
        }
    ) && let Some(column) = typed_column
    {
        let rect = cell_rect(
            area,
            &lengths(&collection_lengths),
            column,
            selected.saturating_sub(state.offset()),
        );
        CELL_TYPED.with(|cell| cell.set(rect));
    }
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
        // No count of them: how many follow hangs on this one's answer.
        title = format!(
            " setting {} up · question {} ",
            root.display(),
            setup.question + 1
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
            // What a step is for goes on under itself, not under the file's
            // name, where the screen is wide enough for the two columns.
            let columned = |said: &str, style: Style| -> Vec<Line<'static>> {
                let file = proposal.step.name(root);
                let what = said.get(29..).filter(|_| said.is_char_boundary(29));
                match what {
                    Some(what) if file.chars().count() <= 28 && width >= 4 + 29 + 24 => {
                        textwrap::wrap(what, width - 4 - 29)
                            .into_iter()
                            .enumerate()
                            .map(|(at, part)| {
                                // The indent a span of its own, which `+`
                                // replaces.
                                let head = if at == 0 {
                                    vec![
                                        Span::styled("    ", style),
                                        Span::styled(format!("{file:<28} "), style),
                                    ]
                                } else {
                                    vec![Span::styled(" ".repeat(4 + 29), style)]
                                };
                                let mut spans = head;
                                spans.push(Span::styled(part.into_owned(), style));
                                Line::from(spans)
                            })
                            .collect()
                    }
                    _ => hanging(said, 4, width, style),
                }
            };
            if proposal.present {
                lines.extend(columned(
                    &format!("{name}   already here, kept"),
                    Style::new().fg(theme.muted),
                ));
            } else {
                let mut named = columned(&name, Style::new());
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
    // Wide enough for its title once changed, ` sections · not written `,
    // which was cut.
    let [left, right] =
        Layout::horizontal([Constraint::Length(26), Constraint::Min(20)]).areas(area);
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
                // Cut with an ellipsis at the pane's edge: at 40 columns
                // `exclude = ["RE` read as the whole value.
                ConfigRow::Key { name, key, value } => ListItem::new(Line::from(vec![
                    Span::raw(if name.is_some() { "  " } else { "" }),
                    Span::styled(
                        format!("{key:<key_width$}"),
                        Style::new().fg(theme.attribute),
                    ),
                    Span::styled(" = ", Style::new().fg(theme.muted)),
                    Span::raw(ellipsized(
                        value,
                        (right.width as usize)
                            .saturating_sub(4 + if name.is_some() { 2 } else { 0 } + key_width + 3)
                            .max(1),
                    )),
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
                        Line::from(format!(
                            " {} ",
                            ellipsized(
                                &format!("keys: {keys}"),
                                (right.width as usize).saturating_sub(4).max(1)
                            )
                        ))
                        .style(Style::new().fg(theme.muted)),
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
    // By cells, as `status` counts them: a column's line counts its
    // rows; a defect or a note counts once.
    let count = |kind: Concern, waits: bool| {
        workbench
            .control
            .iter()
            .filter(|item| item.kind == kind && (kind != Concern::Owed || waiting(item) == waits))
            .map(|item| match item.kind {
                Concern::Defect | Concern::Note => 1,
                _ => crate::presentation::changes::cells_in(&item.said),
            })
            .sum::<usize>()
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
/// Where a window over the sample is drawn: most of the screen for a table,
/// less for a property or a list, centred.
fn window_area(main: Rect, wanted: Option<(u16, u16)>) -> Rect {
    // Every window alike, a table's grown only as far as it needs: a table
    // of two short columns was drawn over nearly the whole screen.
    let (width, height) = (main.width * 70 / 100, main.height * 50 / 100);
    let (width, height) = match wanted {
        Some((wide, high)) => (
            width.max(wide.min(main.width * 95 / 100)),
            height.max(high.min(main.height * 85 / 100)),
        ),
        None => (width, height),
    };
    // Inside the main frame's borders, never on them.
    let width = width
        .max(40.min(main.width))
        .min(main.width.saturating_sub(2));
    let height = height
        .max(8.min(main.height))
        .min(main.height.saturating_sub(2));
    Rect {
        x: main.x + (main.width.saturating_sub(width)) / 2,
        y: main.y + (main.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// What a table's window needs to show it whole — its columns side by side,
/// its rows, its header and its empty row — roughly, in cells; `None` for
/// any other window.
fn table_extent(workbench: &Workbench) -> Option<(u16, u16)> {
    let Screen::Table { ref name, .. } = workbench.screen else {
        return None;
    };
    let (path, sample) = workbench.current()?;
    let Ok(held) = sample.table(name) else {
        return Some((0, 0));
    };
    let config = workbench.collection.config_for(&path);
    let vocabulary = workbench.collection.vocabulary();
    let subject = Subject {
        sample: &sample,
        path: Some(&path),
        vocabulary: &vocabulary,
        states: None,
    };
    let columns: Vec<Identifier> = held.column_names().into_iter().cloned().collect();
    let mut width = 4usize;
    for column in &columns {
        // As the window writes a cell: the digits a file holds,
        // `15.999999999999794`, asked for a window twice as wide as `16.0`.
        let widest = held
            .rows()
            .map(|row| {
                let values: Vec<crate::core::value::Value> =
                    row.index().into_iter().cloned().collect();
                let field = super::model::cell_address(name, column, values);
                let profile = profiles::anonymous(vec![ColumnSpec {
                    field,
                    label: None,
                    header: None,
                    precision: None,
                    template: None,
                }]);
                render::bare_cell(
                    &profile.columns()[0],
                    &subject,
                    render::Declaration::Profile(&profile),
                    config,
                    None,
                )
                .chars()
                .count()
            })
            .max()
            .unwrap_or(1);
        // Its header with its unit, `temperature [°C]`, and a cell with its
        // mark: what the window draws, where a margin of ten asked for a
        // window wider than the table needed.
        let unit = held
            .column(column)
            .ok()
            .and_then(|meta| meta.presentation().unit.clone())
            .filter(|unit| !unit.is_empty())
            .map(|unit| {
                config
                    .and_then(|config| config.unit(&unit))
                    .and_then(|variants| variants.get("plain").cloned())
                    .unwrap_or(unit)
            });
        let header =
            column.as_str().chars().count() + unit.map_or(0, |unit| unit.chars().count() + 3);
        width += header.max(widest + 2) + 3;
    }
    let rows = held.rows().count() + 1;
    Some((width as u16, (rows + 5) as u16))
}

/// A list in its window: a table of one column, an item a row, the empty
/// row at the foot adding one.
fn items_window(frame: &mut Frame, workbench: &Workbench, area: Rect) {
    let Screen::Items {
        ref field,
        readings,
        cursor,
        ..
    } = workbench.screen
    else {
        return;
    };
    let Some((_, items, list)) = workbench.items() else {
        return;
    };
    // Titled as a table is, `ageing · 8 rows`; headed as its column.
    let what = if readings {
        format!("{field} · readings · {}", items.len())
    } else if list {
        format!(
            "{field} · {}",
            if items.len() == 1 {
                "1 item".to_string()
            } else {
                format!("{} items", items.len())
            }
        )
    } else {
        format!("{field} · one value")
    };
    let (path, sample) = match workbench.current() {
        Some(current) => current,
        None => return,
    };
    let config = workbench.collection.config_for(&path);
    let header = if readings {
        let unit = Identifier::new(field)
            .ok()
            .and_then(|name| sample.property_if(&name))
            .and_then(|handle| handle.presentation().unit)
            .filter(|unit| !unit.is_empty())
            .map(|unit| {
                config
                    .and_then(|config| config.unit(&unit))
                    .and_then(|variants| variants.get("plain").cloned())
                    .unwrap_or(unit)
            });
        match unit {
            Some(unit) => format!("reading [{unit}]"),
            None => "reading".to_string(),
        }
    } else if list {
        "item".to_string()
    } else {
        "value".to_string()
    };
    drop(sample);
    let known = workbench.known_samples();
    let link = |item: &String| crate::presentation::links::shown(item, &path, &known);
    // Readings at the precision their quantity declares; a link as Markdown
    // shows it.
    let shown = |item: &String| -> String {
        if !readings {
            return link(item).map_or_else(|| item.clone(), |link| link.text);
        }
        item.parse::<f64>()
            .ok()
            .and_then(|number| crate::core::value::Value::number(number).ok())
            .map_or_else(
                || item.clone(),
                |value| editing::previewed(&value, field, config),
            )
    };
    let mut rows: Vec<Row> = items
        .iter()
        .map(|item| {
            let style = match link(item).filter(|_| !readings) {
                Some(link) => attribute_style(Some(link.leads)),
                None => Style::new(),
            };
            Row::new(vec![Cell::from(shown(item)).style(style)])
        })
        .collect();
    rows.push(Row::new(vec![
        Cell::from("+").style(Style::new().fg(colors().muted)),
    ]));
    let mut state = TableState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Min(10)])
            .header(
                Row::new(vec![Cell::from(header)]).style(Style::new().add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(Style::new().reversed())
            .block(
                framed_saying(workbench, area.width)
                    .title(format!(" {what} "))
                    .title_bottom(element_line(workbench, area.width)),
            ),
        area,
        &mut state,
    );
}

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
    let config = workbench.collection.config_for(&path);
    let Some(layout) = super::model::table_layout(&sample, name, config) else {
        return;
    };
    // A table the model declares and the file holds no row of: its columns,
    // drawn from the declaration, with no row but the empty one to fill.
    let stand_in;
    let held = match sample.table(name) {
        Ok(held) => held,
        Err(_) => {
            let described = config
                .and_then(crate::config::model_runtime::current_description)
                .and_then(|described| described.tables.get(name.as_str()).cloned());
            let columns: indexmap::IndexMap<Identifier, crate::core::table::ColumnMeta> = layout
                .cursor_columns()
                .into_iter()
                .map(|column| {
                    let unit = described.as_ref().and_then(|table| {
                        table
                            .columns
                            .iter()
                            .find(|declared| declared.name == column.as_str())
                            .and_then(|declared| declared.unit.clone())
                    });
                    let mut meta = crate::core::table::ColumnMeta::default();
                    meta.presentation.unit = unit;
                    (column, meta)
                })
                .collect();
            let Ok(table) = crate::core::table::Table::new(
                name.clone(),
                layout.index.clone(),
                columns,
                Vec::new(),
            ) else {
                return;
            };
            stand_in = table;
            &stand_in
        }
    };
    let vocabulary = workbench.collection.vocabulary();
    let subject = Subject {
        sample: &sample,
        path: Some(&path),
        vocabulary: &vocabulary,
        states: None,
    };
    let index: Vec<String> = held
        .index_columns()
        .iter()
        .map(ToString::to_string)
        .collect();
    let columns: Vec<String> = super::model::value_columns(held)
        .iter()
        .map(ToString::to_string)
        .collect();
    // Each column headed with its unit, as `view` heads it, its cells bare:
    // the file's unit, or the one the project declares, in its plain form.
    let headed = |column: &str| {
        let written = Identifier::new(column)
            .ok()
            .and_then(|id| {
                held.column(&id)
                    .ok()
                    .and_then(|meta| meta.presentation().unit.clone())
            })
            .or_else(|| {
                config
                    .and_then(|config| config.property(&format!("{name}.{column}")))
                    .and_then(|declared| declared.unit.clone())
            })
            .filter(|unit| !unit.is_empty());
        match written {
            Some(unit) => {
                let plain = config
                    .and_then(|config| config.unit(&unit))
                    .and_then(|variants| variants.get("plain").cloned())
                    .unwrap_or(unit);
                format!("{column} [{plain}]")
            }
            None => column.to_string(),
        }
    };
    let index_titles: Vec<String> = index.iter().map(|column| headed(column)).collect();
    // A column selected for `c` is marked, as a value is.
    let marked: Vec<String> = columns
        .iter()
        .map(|column| {
            if workbench.selected.contains(&format!("{name}.{column}")) {
                format!("● {}", headed(column))
            } else {
                headed(column)
            }
        })
        .collect();
    // The columns the model derives: a cell of one nobody filled is owed,
    // `∅`, where it was left unmarked.
    let derived_by_model: Vec<String> = layout.derived.clone();
    // A column whose formula changed is outdated in every cell, as `status`
    // and `explain` say it: its cells read current here, their records
    // matching their inputs.
    let planned =
        crate::collection::computation::planned_states(config, &path, &sample).unwrap_or_default();
    let formula_changed = |column: &Identifier| {
        planned
            .get(&format!("{name}.{column}"))
            .filter(|said| said.contains("formula changed"))
            .cloned()
    };
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
                            if let Some(failure) = row
                                .cell(column)
                                .ok()
                                .and_then(|cell| cell.records().failure.clone())
                            {
                                return format!("failed — {failure}");
                            }
                            let owed = derived_by_model.contains(&column.to_string())
                                && row.cell(column).is_ok_and(|cell| {
                                    cell.records().failure.is_none()
                                        && cell.peek_value().is_none_or(|value| value.is_absent())
                                });
                            return if owed {
                                "never computed".to_string()
                            } else {
                                String::new()
                            };
                        }
                        let word = fingerprint::check_cell_among(&sample, name, column, &values)
                            .map(|state| super::model::word_of(&sample, &state))
                            .unwrap_or_default();
                        match formula_changed(column) {
                            Some(said) if word == "current" => format!("outdated — {said}"),
                            _ => word,
                        }
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
                let text = render::bare_cell(
                    &profile.columns()[0],
                    &subject,
                    render::Declaration::Profile(&profile),
                    config,
                    None,
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
    // The empty row at the foot, where a row is typed cell by cell: what was
    // typed kept in it, a column the model computes marked as owed.
    let draft = workbench
        .draft
        .as_ref()
        .filter(|draft| draft.path == path && draft.table == name.as_str());
    let typed = |column: &str| draft.and_then(|draft| draft.at(column)).map(str::to_string);
    let mut foot: Vec<String> = index
        .iter()
        .map(|column| typed(column).unwrap_or_default())
        .collect();
    if foot.iter().all(String::is_empty)
        && let Some(first) = foot.first_mut()
    {
        *first = "+".to_string();
    }
    foot.extend(
        columns
            .iter()
            .map(|column| typed(column).unwrap_or_default()),
    );
    body.push(foot);
    states.push(
        columns
            .iter()
            .map(|column| {
                if derived_by_model.contains(column) {
                    "never computed".to_string()
                } else {
                    String::new()
                }
            })
            .collect(),
    );
    // Each column's values lined up on their `±`.
    for at in index.len()..index.len() + columns.len() {
        let column: Vec<String> = body.iter().map(|cells| cells[at].clone()).collect();
        for (cells, cell) in body.iter_mut().zip(aligned(&column)) {
            cells[at] = cell;
        }
    }
    // Each cell's mark before it, as a sample's lines carry theirs: a cell
    // outdated or never computed was only a colour, which a reader of the
    // table could not tell from a current one.
    for column in 0..columns.len() {
        let marks: Vec<&'static str> = states
            .iter()
            .map(|row| state_mark(row.get(column).map_or("", String::as_str)))
            .collect();
        if marks.iter().all(|mark| mark.is_empty()) {
            continue;
        }
        for (cells, mark) in body.iter_mut().zip(&marks) {
            let at = index.len() + column;
            cells[at] = format!("{} {}", if mark.is_empty() { " " } else { mark }, cells[at]);
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
    let index_widths: Vec<usize> = index_titles
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
    // The cursor goes over the index too: a value column's place among the
    // value columns, and whether it is on the index.
    let on_index = *on < index.len();
    let on_all = *on;
    let on = on
        .saturating_sub(index.len())
        .min(columns.len().saturating_sub(1));
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
    let mut header = index_titles.clone();
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
                // A value bold, in its state's colour where it has one.
                let state = states.get(at - index.len()).map_or("", String::as_str);
                let style = match state_colour(state) {
                    Color::Reset => value_style(&colors()),
                    colour => Style::new().fg(colour).add_modifier(Modifier::BOLD),
                };
                Cell::from(cell).style(style)
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
    let table_lengths = separated(widths, Constraint::Length(1), 0);
    let table = Table::new(rows, table_lengths.clone())
        .header(
            Row::new(separated(header, separator(), 0))
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        // The row on its own background, the cell under the cursor reversed.
        .row_highlight_style(line_style(workbench))
        .cell_highlight_style(Style::new().reversed())
        .block(
            framed_saying(workbench, area.width)
                .title(format!(
                    " {name} · {} {edges}",
                    super::model::counted(count, "row", "rows")
                ))
                .title_bottom(element_line(workbench, area.width)),
        );
    let rows_shown = (area.height as usize).saturating_sub(3 + said_rows(workbench, area.width));
    let mut state = TableState::default()
        .with_selected(Some(*cursor))
        // Every column but the first has a separator before it.
        .with_selected_column(Some(if on_index {
            2 * on_all
        } else {
            2 * (index.len() + on - start)
        }))
        .with_offset(offset_within(
            workbench,
            "table",
            &vec![1; count],
            rows_shown,
        ));
    let typed_at = state.selected_column();
    let offset_before = state.offset();
    frame.render_stateful_widget(table, area, &mut state);
    keep_offset(workbench, "table", state.offset());
    let _ = offset_before;
    if matches!(
        workbench.mode,
        Mode::Prompt {
            purpose: super::model::Purpose::Cell(_),
            ..
        }
    ) && let Some(column) = typed_at
    {
        let rect = cell_rect(
            area,
            &lengths(&table_lengths),
            column,
            cursor.saturating_sub(state.offset()),
        );
        CELL_TYPED.with(|cell| cell.set(rect));
    }
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
    // The quantities' values lined up on their `±`; one without an
    // uncertainty shows where it goes, `± —`.
    let mut entries = entries;
    for entry in entries.iter_mut() {
        if entry.kind == EntryKind::Quantity
            && !entry.shown.contains('±')
            && entry.shown != "—"
            && !entry.shown.is_empty()
        {
            entry.shown = match entry.shown.split_once(' ') {
                Some((number, unit)) => format!("{number} ± — {unit}"),
                None => format!("{} ± —", entry.shown),
            };
        }
    }
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
    // A line shows its state's mark alone: its words are the value's card's.
    let state_width = 0;
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
    let [left, right] = if hidden {
        [area, Rect::default()]
    } else if stacked {
        let note_rows = (area.height / 3).max(3);
        Layout::vertical([Constraint::Min(3), Constraint::Length(note_rows)]).areas(area)
    } else {
        Layout::horizontal([Constraint::Length(values_width), Constraint::Min(20)]).areas(area)
    };
    // What waits for this sample, read once for every line.
    let fields: Vec<String> = entries.iter().map(|entry| entry.field.clone()).collect();
    let waiting_fields = workbench
        .current()
        .map(|(path, _)| path)
        .map(|path| workbench.waiting_fields(&path, &fields))
        .unwrap_or_default();
    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            // A change waiting for it: drawn as a change, until written.
            let waiting = waiting_fields.contains(&entry.field);
            let colour = match entry.kind {
                EntryKind::Quantity => Color::Reset,
                EntryKind::Attribute => colors().attribute,
                EntryKind::Table => colors().table,
                EntryKind::Backlink => colors().link,
            };
            let state_colour = state_colour(&entry.state);
            let selected = workbench.selected.contains(&entry.field);
            let mark = state_mark(&entry.state);
            // Cut with an ellipsis where the pane ends, never silently — its
            // frame and padding, the selection, the mark and the name before
            // it: `8.6 ± 1.5 pt/da` read as a unit. The value in its role's
            // style, its unit apart; an attribute's in the attribute's colour.
            let cut = ellipsized(
                &entry.shown,
                (left.width as usize)
                    .saturating_sub(2 + 2 + 3 + widest + 2)
                    .max(1),
            );
            let padding = shown_width.saturating_sub(cut.chars().count()) + 2;
            let mut spans = vec![
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
            ];
            let changed = Style::new()
                .fg(colors().waiting)
                .add_modifier(Modifier::ITALIC);
            let value: Vec<Span> = match entry.kind {
                EntryKind::Quantity => value_spans(&cut, Style::new()),
                EntryKind::Attribute => vec![Span::styled(cut, attribute_style(entry.link))],
                EntryKind::Table => vec![Span::raw(cut)],
                EntryKind::Backlink => vec![Span::styled(cut, Style::new().fg(colors().muted))],
            };
            // `± —`, where an uncertainty goes, faint; the number before it
            // as any value.
            spans.extend(value.into_iter().flat_map(|span| {
                let parts: Vec<Span> = match span.content.split_once("± —") {
                    Some((before, after)) if !waiting => vec![
                        Span::styled(before.to_string(), span.style),
                        Span::styled("± —".to_string(), span.style.fg(colors().muted)),
                        Span::styled(after.to_string(), span.style),
                    ],
                    _ => vec![span],
                };
                parts.into_iter().map(|span| {
                    if waiting {
                        span.patch_style(changed)
                    } else {
                        span
                    }
                })
            }));
            spans.push(Span::raw(" ".repeat(padding)));
            ListItem::new(Line::from(spans))
        })
        .collect();
    // Under a window, the line it was opened from.
    let cursor = match &workbench.screen {
        Screen::Sample { cursor, .. } | Screen::Table { back: cursor, .. } => *cursor,
        Screen::Items { up, .. } => match **up {
            Screen::Sample { cursor, .. } => cursor,
            _ => 0,
        },
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
            .highlight_style(line_style(workbench))
            .block(
                framed_saying(workbench, left.width)
                    .title(" values ")
                    .title_bottom(element_line(workbench, left.width)),
            ),
        left,
        &mut state,
    );
    keep_offset(workbench, "sample", state.offset());
    // The typing mode's cell on the cursor's line: the name's or the
    // value's, marked; typed, its editor drawn over it.
    if workbench.typing && matches!(workbench.screen, Screen::Sample { .. }) {
        let row = cursor.saturating_sub(state.offset()) as u16;
        let y = left.y + 1 + row;
        let name_x = left.x + 1 + 4;
        let value_x = name_x + widest as u16 + 2;
        let right_edge = (left.x + left.width).saturating_sub(1);
        let rect = if workbench.cell == 0 {
            Rect::new(name_x, y, widest as u16, 1)
        } else {
            Rect::new(value_x, y, right_edge.saturating_sub(value_x).max(1), 1)
        };
        if y + 1 < left.y + left.height {
            if matches!(
                workbench.mode,
                Mode::Prompt {
                    purpose: super::model::Purpose::Cell(_),
                    ..
                }
            ) {
                CELL_TYPED.with(|cell| cell.set(Some(rect)));
            } else {
                let marked = if workbench.cell == 0 {
                    rect
                } else {
                    Rect::new(value_x, y, (shown_width as u16).max(1).min(rect.width), 1)
                };
                frame
                    .buffer_mut()
                    .set_style(marked, Style::new().reversed());
            }
        }
    }
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

/// Where a cell of row `row` and table column `column` lies in a table drawn
/// in `area`, its columns `lens` wide one column apart, under its frame and
/// its header.
fn cell_rect(area: Rect, lens: &[u16], column: usize, row: usize) -> Option<Rect> {
    let x = area.x + 2 + lens.iter().take(column).map(|len| len + 1).sum::<u16>();
    let y = area.y + 2 + row as u16;
    let width = (*lens.get(column)?)
        .max(8)
        .min((area.x + area.width).saturating_sub(x + 1));
    (y + 1 < area.y + area.height && width > 0).then_some(Rect::new(x, y, width, 1))
}

/// The lengths of a table's columns, its separators among them.
fn lengths(widths: &[Constraint]) -> Vec<u16> {
    widths
        .iter()
        .map(|width| match width {
            Constraint::Length(length) => *length,
            _ => 0,
        })
        .collect()
}

thread_local! {
    /// Where the cell typed in the typing mode lies, which its editor draws.
    static CELL_TYPED: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
    /// Where the note being edited goes, which its editor draws.
    static NOTE_EDITED: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
    /// Where the note was last drawn, and the scroll it was drawn at, which
    /// `draw` gives the model: the wheel over it scrolls it, and a scroll
    /// past its end is brought back.
    static NOTE_SHOWN: std::cell::Cell<Shown> =
        const { std::cell::Cell::new(None) };
}

/// The note being edited, drawn by its editor in `area`: the text as typed,
/// wrapped at words as it is shown out of editing — scrolled sideways, each
/// line's beginning went out of sight — its Markdown marked without changing a character as
/// `rat-markdown` reads it — a heading in the key colour and bold, a list's
/// marker coloured, code muted, emphasis, strength and a link as they are — the
/// selection reversed, the terminal's cursor at the editor's.
fn written(frame: &mut Frame, area: Rect, text: &mut Written) -> NoteView {
    let theme = colors();
    let heading = Style::new().fg(theme.key).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let widget = TextArea::new()
        .text_wrap(rat_text::text_area::TextWrap::Word(8))
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
/// The style of a column of the samples table, by what it holds: a unit in
/// the unit's colour, numbers as values, other text as an attribute's; a
/// sample's name, path or state as they are.
fn column_style(workbench: &Workbench, at: usize, cells: &[String]) -> Style {
    use crate::query::field_addressing::{self as fields, Channel, Field};
    let theme = colors();
    let Some(column) = workbench.profile.columns().get(at) else {
        return Style::new();
    };
    match fields::parse(&column.field) {
        Ok(Field::Reserved(_)) | Err(_) => Style::new(),
        Ok(
            Field::Named {
                channel: Channel::Unit,
                ..
            }
            | Field::Cell {
                channel: Channel::Unit,
                ..
            },
        ) => Style::new().fg(theme.unit),
        Ok(_) => {
            let numeric = cells
                .iter()
                .filter(|cell| !matches!(cell.as_str(), "" | "—" | "-" | "n/a"))
                .all(|cell| {
                    cell.trim_start()
                        .split(' ')
                        .next()
                        .is_some_and(|word| word.trim_start_matches('(').parse::<f64>().is_ok())
                });
            if numeric {
                value_style(&theme)
            } else {
                Style::new().fg(theme.attribute)
            }
        }
    }
}

fn state_colour(state: &str) -> Color {
    let theme = colors();
    if state.starts_with("failed") {
        theme.failed
    } else if state.starts_with("outdated")
        || state.starts_with("not judged")
        || state.starts_with("broken")
    {
        theme.outdated
    } else if state.starts_with("edited") || state.starts_with("record missing") {
        theme.edited
    } else if state == "current" {
        theme.current
    } else if state == super::model::NO_STATISTIC {
        theme.muted
    } else if (state.contains("never computed")
        || state.contains(crate::collection::exports::NEVER_COMPUTED))
        || state.starts_with("waiting")
        || state.starts_with("waits for")
    {
        theme.outdated
    } else if state.is_empty() || state == "entered" {
        Color::Reset
    } else {
        // Its formula changed, a statistic to take again: a value to compute
        // again, in the outdated colour whatever its words, as the command
        // line says it.
        theme.outdated
    }
}

/// `line` wrapped at `width`, its rest under where its text begins: past its
/// leading spaces, and past a field's name where two spaces or more set the
/// name apart from what follows it. Each character keeps its style; words
/// are not cut unless one alone is wider than the room.
fn hung(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let chars: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
        .collect();
    if chars.len() <= width || width < 8 {
        return vec![line];
    }
    let text: String = chars.iter().map(|(c, _)| *c).collect();
    let leading = text.chars().take_while(|c| *c == ' ').count();
    // Past `name   ` where a gap of two spaces follows the first word.
    let indent = text[leading..]
        .find("  ")
        .map(|gap| {
            let after = leading + gap;
            after + text[after..].chars().take_while(|c| *c == ' ').count()
        })
        .filter(|at| *at < width / 2)
        .unwrap_or(leading)
        .min(width / 2);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut at = 0;
    let mut first = true;
    while at < chars.len() {
        let room = if first { width } else { width - indent };
        let end = (at + room).min(chars.len());
        // Break at the last space within the room, unless the line ends here.
        let end = if end < chars.len() {
            (at + 1..end)
                .rev()
                .find(|cut| chars[*cut].0 == ' ')
                .map_or(end, |cut| cut + 1)
        } else {
            end
        };
        let mut spans: Vec<Span<'static>> = Vec::new();
        if !first {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        let piece: Vec<(char, Style)> = chars[at..end]
            .iter()
            .copied()
            .skip_while(|(c, _)| !first && *c == ' ')
            .collect();
        // The indent is a span of its own, never grown by the text after it.
        let kept = usize::from(!first);
        for (c, style) in piece {
            let grows = spans.len() > kept;
            match spans.last_mut() {
                Some(last) if grows && last.style == style => last.content.to_mut().push(c),
                _ => spans.push(Span::styled(c.to_string(), style)),
            }
        }
        lines.push(Line::from(spans).style(line.style));
        at = end;
        first = false;
    }
    lines
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
    // Wrapped here, each line's rest under where its text begins — a change
    // under its value, `→ 1.052, 1.054, 1.05, / 1.051` — where the terminal
    // wrapped it at the window's left edge.
    let lines: Vec<Line<'static>> = if list {
        lines
    } else {
        lines
            .into_iter()
            .flat_map(|line| hung(line, inner))
            .collect()
    };
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
                    // Cut with its mark where the border cannot hold it.
                    Line::from(format!(
                        " {} ",
                        // Its middle cut, where a title ends on a path: the
                        // file's name was what it lost.
                        crate::presentation::terminal_rendering::truncate_middle(
                            title,
                            width.saturating_sub(4)
                        )
                    ))
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
    // More below than the window holds is marked on its last row, in the
    // padding: at 40×16 the quantity's window lost its advice, cut with no
    // sign that it went on.
    if !list && wrapped > scroll + shown_rows && at.width > 2 {
        frame.render_widget(
            Paragraph::new(Span::styled("…", Style::new().fg(colors().muted))),
            Rect {
                x: at.x + at.width - 2,
                y: at.y + shown_rows as u16,
                width: 1,
                height: 1,
            },
        );
    }
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
    // As wide as `modal` makes a list's window: an item longer than its
    // inside is cut with its mark, where the window cut it without one.
    let main = MAIN_AREA.with(std::cell::Cell::get);
    let area = if main.width > 0 && main.height >= 3 {
        main
    } else {
        frame.area()
    };
    let inner = (area.width as usize * 2 / 3)
        .clamp(60, 100)
        .min(area.width.saturating_sub(4) as usize)
        .saturating_sub(4)
        .max(1);
    let lines = items
        .into_iter()
        .map(|item| crate::presentation::terminal_rendering::truncate(&item, inner))
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
        // A cell of the typing mode is typed where it is, drawn by its screen.
        Mode::Prompt {
            purpose: super::model::Purpose::Cell(_),
            ..
        } => {}
        Mode::Prompt { purpose, .. } => {
            let (title, actions) = match purpose {
                super::model::Purpose::Edit { field, .. } => {
                    (format!("{field} ="), "Enter preview · Esc cancel")
                }
                super::model::Purpose::Rename { .. } => (
                    "rename to".to_string(),
                    "its file's name · Enter preview · Esc cancel",
                ),
                super::model::Purpose::Tags { .. } => (
                    "tags =".to_string(),
                    "by commas or spaces · Enter preview · Esc cancel",
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
                super::model::Purpose::ScriptOptions { script, .. } => (
                    format!("{}'s options, NAME=VALUE", script.name),
                    "Enter run · Esc cancel",
                ),
                super::model::Purpose::DeclareName(_) => {
                    ("its name".to_string(), "Enter preview · Esc cancel")
                }
                super::model::Purpose::FigureText(row) => (
                    format!("your figure: {}", row.label()),
                    "Enter keep · empty gives the field's · Esc back",
                ),
                super::model::Purpose::New { .. } => (
                    "a new sample: its name, then field=value … and keep:field,field from the one under the cursor".to_string(),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::AttributeName { .. } => (
                    "a new attribute's name".to_string(),
                    "Enter then its value · Esc cancel",
                ),
                // A line typed is titled by its address, as a value's is.
                super::model::Purpose::Item {
                    field,
                    at,
                    readings,
                    ..
                } => {
                    let list = if *readings {
                        format!("{field}.readings")
                    } else {
                        field.clone()
                    };
                    match at {
                        Some(at) => (
                            format!("{list}[{}] =", at + 1),
                            "Enter preview · empty takes it out · Esc cancel",
                        ),
                        None if *readings => (
                            format!("{list}[new] ="),
                            "several, 11, 13 · Enter preview · Esc cancel",
                        ),
                        None => (format!("{list}[new] ="), "Enter preview · Esc cancel"),
                    }
                }
                super::model::Purpose::DraftCell { table, column, .. } => (
                    format!("{table}.{column}[new] ="),
                    "Enter keep · Esc cancel",
                ),
                super::model::Purpose::Tag => (
                    "tag the basket with, -tag to take it off, or old=new to rename one"
                        .to_string(),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::ShownFile { several } => (
                    if *several {
                        "the file, from the folder opened — what is shown spans several \
                         projects: its extension says its format"
                    } else {
                        "the file, from the project's folder: its extension says its format"
                    }
                    .to_string(),
                    "Enter preview · Esc cancel",
                ),
                super::model::Purpose::FigureFile(_) => (
                    "the figure's file, from the project's folder: .pdf, .png or .svg".to_string(),
                    "Enter write it · Esc cancel",
                ),
                super::model::Purpose::NameMachine { host, .. } => (
                    format!("{host} wrote here, a computer this one does not know: its name"),
                    "Enter keep this name · Esc keep the host's",
                ),
                // The setup's model one already has.
                super::model::Purpose::Cell(_) => (String::new(), ""),
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
            // The theme the project names, which a choice here is over.
            let theme_title = match workbench
                .collection
                .config()
                .and_then(|config| config.theme())
            {
                Some(named) => format!("theme · this project names {named}"),
                None => "theme".to_string(),
            };
            let (title, actions, ticks) = match purpose {
                super::model::Choosing::Theme { .. } => (
                    theme_title.as_str(),
                    "↑↓ try · / narrow · Enter keep · Esc the one before",
                    false,
                ),
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
                super::model::Choosing::Produce(_, title) => (
                    title.as_str(),
                    "Enter run · w a figure to a file · / narrow · Tab next group · Esc cancel",
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
                    } else if matches!(purpose, super::model::Choosing::Theme { .. })
                        && workbench.themes.own(label)
                    {
                        // One's own, after those SampleKit has: said so.
                        format!("{label}  yours")
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
            // One's own themes left out, and why: the list does not show them.
            if matches!(purpose, super::model::Choosing::Theme { .. }) {
                for said in workbench.themes.said() {
                    footer.push(Line::styled(said.clone(), Style::new().fg(colors().muted)));
                }
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
        // A running script's window: what it printed, its last line the one
        // being written, where what is typed for its `input()` goes on.
        Mode::Script { .. } => {
            let (name, printed) = workbench
                .scripting
                .as_ref()
                .map(|running| (running.name.clone(), running.printed.clone()))
                .unwrap_or_default();
            let lines: Vec<String> = printed.split('\n').map(str::to_string).collect();
            let last = lines.len().saturating_sub(1);
            let from = lines.last().map_or(0, |line| {
                unicode_width::UnicodeWidthStr::width(line.as_str())
            }) as u16;
            modal(
                frame,
                &format!("run {name}"),
                "Enter answers · Esc puts it away, the script runs on",
                lines.into_iter().map(Line::raw).collect(),
                Focus::Cursor(last),
                None,
                status,
            );
            typed.push((last, from));
        }
        Mode::Note { .. } => {}
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
pub fn state_mark(state: &str) -> &'static str {
    // The file's name standing for a name nobody wrote is a note, no state
    // that needs attention: marked ⚠, every sample named by its file read
    // outdated.
    // A value or a table to fill in is nobody's omission yet: marked ⚠, a
    // new sample read as outdated from its first line.
    if state.is_empty()
        || state == "current"
        || state == "entered"
        || state == "the file's name"
        || state == super::model::TO_FILL
    {
        ""
    } else if state == super::model::NO_STATISTIC {
        "·"
    } else if state.starts_with("waits for") || state.contains("waiting") {
        // Before failed: *waits for young_modulus, which failed* waits, as
        // `v` says it, and was marked ✗.
        "∅"
    } else if state.contains("failed") {
        "✗"
    } else if state.contains("never computed")
        || state.contains(crate::collection::exports::NEVER_COMPUTED)
    {
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
            // The machine after *when*, as `log` writes it, once two have
            // written in the history.
            let when = match &row.machine {
                Some(machine) => format!("{}  {machine}  ", row.when),
                None => format!("{}  ", row.when),
            };
            let when = Span::styled(when, Style::new().fg(theme.muted));
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

/// An attribute's value: in the attribute's colour, a link underlined in the
/// link's, one leading nowhere in the error's.
fn attribute_style(link: Option<crate::presentation::links::Leads>) -> Style {
    use crate::presentation::links::Leads;
    match link {
        None => Style::new().fg(colors().attribute),
        Some(Leads::Nowhere) => Style::new().fg(colors().error).underlined(),
        Some(_) => Style::new().fg(colors().link).underlined(),
    }
}

/// A value's card: the value, its readings, what it reads and what reads it,
/// the line under the cursor lit; a line typed in, its editor at its value.
fn card_window(frame: &mut Frame, workbench: &Workbench, main: Rect) {
    use super::model::CardGroup;
    let lines = workbench.card_lines();
    let Some((field, cursor)) = workbench.card.clone() else {
        return;
    };
    let named = lines
        .iter()
        .map(|line| line.field.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    let shown = lines
        .iter()
        .map(|line| line.shown.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(12, 36);
    // A heading before each group that has lines.
    let mut rows: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    let mut group = None;
    for (at, line) in lines.iter().enumerate() {
        if group != Some(line.group) {
            group = Some(line.group);
            let heading = match line.group {
                CardGroup::Itself | CardGroup::Readings => None,
                CardGroup::Reads => Some("reads"),
                CardGroup::ReadBy => Some("read by"),
            };
            if let Some(heading) = heading {
                rows.push((None, Line::raw("")));
                rows.push((
                    None,
                    Line::styled(
                        format!(" {heading}"),
                        Style::new().fg(colors().muted).add_modifier(Modifier::BOLD),
                    ),
                ));
            }
        }
        let label = match line.group {
            CardGroup::Readings => "readings".to_string(),
            _ => line.field.clone(),
        };
        let indent = if matches!(line.group, CardGroup::Reads | CardGroup::ReadBy) {
            "   "
        } else {
            " "
        };
        let mark = state_mark(&line.state);
        let style = if line.group == CardGroup::Itself {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        // The value as a line shows it: its number in the value's style, its
        // unit in the unit's; text as text.
        let padded = format!("{:<shown$}", ellipsized(&line.shown, shown));
        let mut spans = vec![
            Span::styled(
                format!(
                    "{indent}{}",
                    ellipsized(&format!("{label:<named$}"), named + 2)
                ),
                style,
            ),
            Span::raw("  "),
        ];
        spans.extend(
            value_spans(&padded, style)
                .into_iter()
                .map(|span| span.patch_style(style)),
        );
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(
                "{} {}",
                if mark.is_empty() { " " } else { mark },
                line.state
            ),
            Style::new().fg(state_colour(&line.state)),
        ));
        rows.push((Some(at), Line::from(spans)));
    }
    let width = (named + shown + 40) as u16;
    let height = rows.len() as u16 + 2;
    let area = window_area(main, Some((width, height)));
    frame.render_widget(Clear, area);
    let block = framed()
        .title(format!(" {field} · its card "))
        .title_bottom(hint_spans(&card_keys(area.width as usize)).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let at_row = rows
        .iter()
        .position(|(at, _)| *at == Some(cursor))
        .unwrap_or(0);
    let first = (at_row + 1).saturating_sub(inner.height as usize);
    for (shown_at, (at, line)) in rows
        .iter()
        .enumerate()
        .skip(first)
        .take(inner.height as usize)
    {
        let y = inner.y + (shown_at - first) as u16;
        let rect = Rect::new(inner.x, y, inner.width, 1);
        // What a narrow card cannot hold of a line, its state's words, is cut
        // with its mark.
        let mut line = line.clone();
        let width = line.width();
        if width > usize::from(inner.width)
            && let Some(last) = line.spans.last_mut()
        {
            let before = width - last.width();
            let room = usize::from(inner.width).saturating_sub(before);
            last.content = ellipsized(&last.content, room).into();
        }
        frame.render_widget(Paragraph::new(line), rect);
        if *at == Some(cursor) {
            // On the `line` colour, as the screens' cursor line: reversed,
            // a unit and a state became blocks of their colours.
            frame
                .buffer_mut()
                .set_style(rect, Style::new().bg(colors().line));
            if matches!(
                workbench.mode,
                Mode::Prompt {
                    purpose: super::model::Purpose::Cell(_),
                    ..
                }
            ) {
                let indent = if lines
                    .get(cursor)
                    .is_some_and(|line| matches!(line.group, CardGroup::Reads | CardGroup::ReadBy))
                {
                    3
                } else {
                    1
                };
                let x = inner.x + (indent + named + 2) as u16;
                let typed = Rect::new(x, y, inner.width.saturating_sub(x - inner.x), 1);
                CELL_TYPED.with(|cell| cell.set(Some(typed)));
            }
        }
    }
}

/// The card's keys at its foot, as many as its width holds, the first most
/// wanted: cut, a key's words were cut in their middle.
fn card_keys(width: usize) -> String {
    let mut keys = vec![
        "↑↓ line",
        "e type in it",
        "Esc close",
        "w write",
        "u take back",
    ];
    while keys.len() > 1 && keys.join(" · ").chars().count() + 6 > width {
        keys.pop();
    }
    format!(" {} ", keys.join(" · "))
}

/// The cursor's line on the collection, a sample and a table: on the `line`
/// colour, its colours kept — reversed, a mark became a block of its colour,
/// and the line a dark bar on a light theme — the cell typed in, in the typing
/// mode, reversed on it.
fn line_style(_workbench: &Workbench) -> Style {
    Style::new().bg(colors().line)
}
