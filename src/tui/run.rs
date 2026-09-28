//! The terminal: raw mode, the alternate screen, the keys read, and the
//! effects the model asks for — a program started, an editor given the
//! terminal.

use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::model::{self, Effect, Key, Workbench};
use super::start::{self, Start};
use crate::presentation::opening;

/// Runs the workbench over `root` until it is quit — or, with `start`, the
/// start page first, and the workbench on what it chose.
pub fn run(root: &Path, start: bool) -> Result<(), String> {
    run_until(root, start).map(|_| ())
}

/// `run`, saying how it ended: quit by its keys, or interrupted by Ctrl+C —
/// which the command line answers with its own interrupt status.
pub fn run_until(root: &Path, start: bool) -> Result<Ended, String> {
    // Opened before the terminal is taken, so that what stops it reads as
    // any error does.
    let mut opened = if start {
        None
    } else {
        Some(Workbench::open(root)?)
    };
    // A panic gives the terminal back before it is reported: left raw, in the
    // alternate screen, the report was unreadable and the shell unusable; the
    // cursor hidden, the shell went on without one.
    let reported: std::sync::Arc<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync> =
        std::sync::Arc::from(std::panic::take_hook());
    let previous = std::sync::Arc::clone(&reported);
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        reported(info);
    }));
    let outcome = (|| {
        enable_raw_mode().map_err(|error| error.to_string())?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture)
            .map_err(|error| error.to_string())?;
        // A paste read as one, typed where the cursor is; a terminal that
        // cannot say so types it key by key, as before.
        let _ = execute!(out, EnableBracketedPaste);
        // What is copied goes to the terminal's clipboard too, where it takes
        // it.
        rat_text::clipboard::set_global_clipboard(TerminalClipboard::default());
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
            .map_err(|error| error.to_string())?;
        // What the start page last opened: quitting a step of the demo comes
        // back to the demo's page.
        let mut last = None;
        let ended = (|| {
            loop {
                let mut workbench = match opened.take() {
                    Some(workbench) => workbench,
                    None => match start_page(&mut terminal, root, &mut last)? {
                        Ok(workbench) => workbench,
                        Err(ended) => return Ok(ended),
                    },
                };
                let ended = session(&mut terminal, &mut workbench)?;
                model::remember(&workbench);
                // A project opened from the start page goes back there when it
                // is quit, its configuration when it is left; Ctrl+C closes
                // everything.
                if !start || ended == Ended::Interrupted {
                    return Ok(ended);
                }
            }
        })();
        let _ = terminal.show_cursor();
        ended
    })();
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    // The panic hook this took over given back: past the workbench, a panic
    // has no terminal to restore.
    drop(std::panic::take_hook());
    std::panic::set_hook(Box::new(move |info| previous(info)));
    outcome
}

/// The start page until a folder is chosen, and the workbench on it as the page
/// asked; how it ended when it was left instead — `q`, or Ctrl+C, which ended
/// as a quit and exited 0 where 130 is owed. `last` what it opened before,
/// which it comes back to: the demo's page from a step of it; what it opens
/// now, kept there.
fn start_page(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    here: &Path,
    last: &mut Option<start::Outcome>,
) -> Result<Result<Workbench, Ended>, String> {
    let mut page = Start::new(here);
    if let Some(outcome) = last.take() {
        page.returning(&outcome);
    }
    loop {
        terminal
            .draw(|frame| start::draw(frame, &mut page))
            .map_err(|error| error.to_string())?;
        let event = event::read().map_err(|error| error.to_string())?;
        let outcome = match start_event(&mut page, &event) {
            StartStep::Stay => continue,
            StartStep::Ended(ended) => return Ok(Err(ended)),
            StartStep::Browse(url) => {
                page.message = match opening::open(Path::new(url)) {
                    Ok(()) => format!("opened {url}"),
                    Err(error) => format!("{error} — {url}"),
                };
                continue;
            }
            StartStep::Chosen(outcome) => outcome,
        };
        // A folder that does not open is said on the page, which stays.
        match start::opened(&outcome) {
            None => return Ok(Err(Ended::Quit)),
            Some(Ok(mut workbench)) => {
                // The demo just written: its environment's install goes on
                // beside its first step, which says when it is done.
                page.hand_over(&mut workbench);
                *last = Some(outcome);
                return Ok(Ok(workbench));
            }
            Some(Err(error)) => page.message = error,
        }
    }
}

/// What an event on the start page comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartStep {
    /// Nothing to do but draw again.
    Stay,
    /// The page left: quit, or interrupted by Ctrl+C.
    Ended(Ended),
    /// A web page to open in the browser.
    Browse(&'static str),
    /// A folder chosen, and what to do with it.
    Chosen(start::Outcome),
}

/// An event on the start page, as the terminal gives it: Ctrl+C closes
/// SampleKit as from the workbench, and is said as an interrupt.
pub fn start_event(page: &mut Start, event: &Event) -> StartStep {
    let pressed = match event {
        Event::Key(pressed) => pressed,
        Event::Paste(pasted) => {
            page.paste(pasted);
            return StartStep::Stay;
        }
        Event::Mouse(mouse) => {
            if let Some(key) = pointer_of(mouse) {
                page.key(key);
            }
            return StartStep::Stay;
        }
        _ => return StartStep::Stay,
    };
    if pressed.kind != KeyEventKind::Press {
        return StartStep::Stay;
    }
    // Over a selection in the line typed Ctrl+C copies it.
    if interrupts(pressed) && !page.copy() {
        return StartStep::Ended(Ended::Interrupted);
    }
    let Some(key) = key_of(pressed) else {
        return StartStep::Stay;
    };
    match page.key(key) {
        start::Effect::None => StartStep::Stay,
        start::Effect::Browse(url) => StartStep::Browse(url),
        start::Effect::Done(start::Outcome::Quit) => StartStep::Ended(Ended::Quit),
        start::Effect::Done(outcome) => StartStep::Chosen(outcome),
    }
}

/// Whether a key pressed is Ctrl+C.
fn interrupts(pressed: &crossterm::event::KeyEvent) -> bool {
    pressed.modifiers.contains(KeyModifiers::CONTROL) && pressed.code == KeyCode::Char('c')
}

/// How a session with the workbench ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Quit by the workbench's own keys: back to the start page, where it was
    /// shown.
    Quit,
    /// Ctrl+C: SampleKit closes, wherever it is.
    Interrupted,
}

fn session(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    workbench: &mut Workbench,
) -> Result<Ended, String> {
    loop {
        terminal
            .draw(|frame| super::view::draw(frame, workbench))
            .map_err(|error| error.to_string())?;
        // A computation running beside the screen is looked at ten times a
        // second, as is the model's plan being read; otherwise the loop waits
        // for a key.
        if (workbench.running.is_some()
            || workbench.drawing.is_some()
            || workbench.installing.is_some()
            || workbench.planning.is_some())
            && !event::poll(Duration::from_millis(100)).map_err(|error| error.to_string())?
        {
            workbench.tick();
            // Quit while a computation ran: it ends once the run has, its
            // snapshot taken.
            if workbench.left() {
                return Ok(ending(workbench));
            }
            continue;
        }
        // Every key waiting is handled before the next frame: a held key
        // queues faster than a frame draws, and went on moving once let go.
        loop {
            let event = event::read().map_err(|error| error.to_string())?;
            if let Some(ended) = handle(terminal, workbench, event)? {
                return Ok(ended);
            }
            if !event::poll(Duration::ZERO).map_err(|error| error.to_string())? {
                break;
            }
        }
        workbench.tick();
        if workbench.left() {
            return Ok(ending(workbench));
        }
    }
}

/// How a workbench quit ends: an interrupt where Ctrl+C asked for it, a quit
/// otherwise.
fn ending(workbench: &Workbench) -> Ended {
    if workbench.interrupted {
        Ended::Interrupted
    } else {
        Ended::Quit
    }
}

/// A key as the model reads it; `None` for one it does not. Held with Ctrl or
/// Alt — AltGr's characters among them — or Shift with a key that moves or
/// deletes, it is a chord, which a line typed reads and nothing else.
fn key_of(pressed: &KeyEvent) -> Option<Key> {
    let (code, modifiers) = (pressed.code, pressed.modifiers);
    let shifted = modifiers.contains(KeyModifiers::SHIFT);
    let held = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let moves = matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Delete
            | KeyCode::Backspace
            | KeyCode::Enter
    );
    if held || (shifted && moves) {
        return Some(Key::Chord(KeyEvent::new(code, modifiers)));
    }
    Some(match code {
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Up if shifted => Key::MoveUp,
        KeyCode::Down if shifted => Key::MoveDown,
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Delete => Key::Delete,
        _ => return None,
    })
}

/// One event; how it ends the session, if it does.
fn handle(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    workbench: &mut Workbench,
    event: Event,
) -> Result<Option<Ended>, String> {
    // The wheel moves as the arrows do; a click is the model's to place,
    // a drag a line typed's to select.
    let pressed = match event {
        Event::Key(pressed) => pressed,
        Event::Mouse(mouse) => {
            if let Some(key) = pointer_of(&mouse) {
                workbench.key(key);
            }
            return Ok(None);
        }
        Event::Paste(pasted) => {
            workbench.paste(&pasted);
            return Ok(None);
        }
        _ => return Ok(None),
    };
    if pressed.kind != KeyEventKind::Press {
        return Ok(None);
    }
    // Ctrl+C ends at once, unless something runs beside the screen: then it
    // asks as `q` does, and a second ends at once. Over a selection in a line
    // typed or the note, it copies it instead.
    if interrupts(&pressed) && !workbench.copy() {
        return Ok(workbench.interrupt().then_some(Ended::Interrupted));
    }
    if interrupts(&pressed) {
        return Ok(None);
    }
    // Ctrl+Y is not y, nor Alt+U u: a key held with either is a chord, which
    // means something to a line typed alone, rather than confirming a write or
    // starting an undo.
    let Some(key) = key_of(&pressed) else {
        return Ok(None);
    };
    match workbench.key(key) {
        Effect::None => {}
        Effect::Quit => return Ok(Some(ending(workbench))),
        Effect::Open(path) => {
            workbench.message = match opening::open(&path) {
                Ok(()) => format!("opened {}", path.display()),
                Err(error) => error.to_string(),
            }
        }
        Effect::Navigate(path) => {
            workbench.message = match opening::navigate(&path) {
                Ok(folder) => format!("opened {}", folder.display()),
                Err(error) => error.to_string(),
            }
        }
        Effect::Editor(path) => {
            // The terminal is the editor's while it runs.
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableBracketedPaste);
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            let edited = opening::edit(&path);
            let _ = enable_raw_mode();
            let _ = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);
            let _ = execute!(io::stdout(), EnableBracketedPaste);
            let _ = terminal.clear();
            workbench.message = match edited {
                Ok(()) => format!("edited {}", path.display()),
                Err(error) => error.to_string(),
            };
            workbench.reload();
        }
    }
    Ok(None)
}

/// The mouse as the model reads it: the wheel, a click of the left button, and
/// — for a line typed or the note to select — the left button dragged, let go,
/// or pressed with a key held.
fn pointer_of(mouse: &MouseEvent) -> Option<Key> {
    Some(match mouse.kind {
        MouseEventKind::ScrollDown => Key::Wheel {
            down: true,
            x: mouse.column,
            y: mouse.row,
        },
        MouseEventKind::ScrollUp => Key::Wheel {
            down: false,
            x: mouse.column,
            y: mouse.row,
        },
        MouseEventKind::Down(MouseButton::Left) if mouse.modifiers.is_empty() => {
            Key::Click(mouse.column, mouse.row)
        }
        MouseEventKind::Down(MouseButton::Left)
        | MouseEventKind::Drag(MouseButton::Left)
        | MouseEventKind::Up(MouseButton::Left) => Key::Pointer(*mouse),
        _ => return None,
    })
}

/// The clipboard of what is typed: kept here for `Ctrl+V`, and given to the
/// terminal by OSC 52, which puts it on the system's clipboard where the
/// terminal takes it — Windows Terminal, kitty, WezTerm, foot, iTerm2 — and is
/// ignored where it does not. What the system holds comes in by the terminal's
/// own paste.
#[derive(Debug, Clone, Default)]
struct TerminalClipboard {
    text: std::sync::Arc<std::sync::Mutex<String>>,
}

impl rat_text::clipboard::Clipboard for TerminalClipboard {
    fn get_string(&self) -> Result<String, rat_text::clipboard::ClipboardError> {
        self.text
            .lock()
            .map(|text| text.clone())
            .map_err(|_| rat_text::clipboard::ClipboardError)
    }

    fn set_string(&self, copied: &str) -> Result<(), rat_text::clipboard::ClipboardError> {
        use std::io::Write;
        let mut text = self
            .text
            .lock()
            .map_err(|_| rat_text::clipboard::ClipboardError)?;
        *text = copied.to_string();
        let mut out = io::stdout();
        let _ = write!(out, "\x1b]52;c;{}\x07", base64(copied.as_bytes()));
        let _ = out.flush();
        Ok(())
    }
}

/// `bytes` in base64, as OSC 52 carries them.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut written = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let joined = chunk.iter().enumerate().fold(0u32, |joined, (at, byte)| {
            joined | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                written.push(DIGITS[(joined >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                written.push('=');
            }
        }
    }
    written
}
