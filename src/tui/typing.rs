//! What is typed in the TUI, in the editors of `rat-text`: a line — the filter,
//! a prompt, a quantity's fields, a list's `/`, the start page's name and
//! narrowing — and the note, whose editor takes `rat-markdown`'s keys besides.
//! The model hands them its keys and reads their text back; the view draws them
//! where they are typed.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rat_markdown::MarkDown;
use rat_markdown::styles::parse_md_styles;
use rat_text::HasScreenCursor;
use rat_text::event::{HandleEvent, Regular, TextOutcome};
use rat_text::text_area::TextAreaState;
use rat_text::text_input::TextInputState;

use super::model::Key;

/// The keys of every line typed, and of the note: `rat-text`'s own, as the
/// reference page and the help say them. A window's `Enter`, `Esc` and `Tab`
/// stay the window's.
pub const TYPING: &[(&str, &str)] = &[
    ("← → Home End", "move; Ctrl+← Ctrl+→ by word"),
    (
        "Shift+← Shift+→ Shift+Home Shift+End",
        "select; Ctrl+Shift+← Ctrl+Shift+→ by word; Ctrl+A all",
    ),
    (
        "Backspace Delete",
        "take a character out, or what is selected; what is typed replaces it",
    ),
    ("Ctrl+Backspace Ctrl+Delete", "take a word out"),
    ("Ctrl+Z", "undo"),
    ("Ctrl+Shift+Z", "redo"),
    ("Ctrl+X Ctrl+V", "cut, paste"),
    (
        "Ctrl+C",
        "copy what is selected; with nothing selected, close SampleKit",
    ),
    ("Ctrl+D", "empty the line; in the note, the line doubled"),
    (
        "click",
        "place the cursor; a drag selects, a double click a word",
    ),
];

/// The note's own keys besides: `rat-markdown`'s, nothing added over them.
pub const WRITING: &[(&str, &str)] = &[
    ("* _ ~", "put around what is selected"),
    ("Alt+1…6", "a heading of that level; the same again, none"),
    ("Alt+L", "a link around what is selected"),
    ("Alt+I", "an image"),
    ("Alt+C", "a code block"),
    ("Alt+K", "a reference link"),
    ("Alt+R", "a link's definition"),
    ("Alt+F", "a footnote"),
    (
        "Enter",
        "a new line, indented as this one; in a table, a new row",
    ),
    (
        "Tab",
        "under a list item's text, to a table's next cell, or four columns in",
    ),
    (
        "Shift+Tab",
        "a table's previous cell, or the selection's indentation taken back",
    ),
    (
        "↑ ↓ PgUp PgDn",
        "move; Ctrl+Home Ctrl+End to its start, its end",
    ),
    ("Ctrl+Y", "the line deleted"),
    ("Esc", "done: the change previewed"),
];

/// A key as the terminal gives it, for an editor: what the model reads as
/// its own key made an event again.
pub fn event_of(key: Key) -> Option<Event> {
    let pressed =
        |code: KeyCode, modifiers: KeyModifiers| Some(Event::Key(KeyEvent::new(code, modifiers)));
    let mouse = |kind: MouseEventKind, column: u16, row: u16| {
        Some(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
    };
    match key {
        Key::Char(character) => pressed(KeyCode::Char(character), KeyModifiers::NONE),
        Key::Enter => pressed(KeyCode::Enter, KeyModifiers::NONE),
        Key::Esc => pressed(KeyCode::Esc, KeyModifiers::NONE),
        Key::Backspace => pressed(KeyCode::Backspace, KeyModifiers::NONE),
        Key::Tab => pressed(KeyCode::Tab, KeyModifiers::NONE),
        Key::BackTab => pressed(KeyCode::BackTab, KeyModifiers::SHIFT),
        Key::Up => pressed(KeyCode::Up, KeyModifiers::NONE),
        Key::Down => pressed(KeyCode::Down, KeyModifiers::NONE),
        Key::MoveUp => pressed(KeyCode::Up, KeyModifiers::SHIFT),
        Key::MoveDown => pressed(KeyCode::Down, KeyModifiers::SHIFT),
        Key::PageUp => pressed(KeyCode::PageUp, KeyModifiers::NONE),
        Key::PageDown => pressed(KeyCode::PageDown, KeyModifiers::NONE),
        Key::Home => pressed(KeyCode::Home, KeyModifiers::NONE),
        Key::End => pressed(KeyCode::End, KeyModifiers::NONE),
        Key::Left => pressed(KeyCode::Left, KeyModifiers::NONE),
        Key::Right => pressed(KeyCode::Right, KeyModifiers::NONE),
        Key::Delete => pressed(KeyCode::Delete, KeyModifiers::NONE),
        Key::Click(x, y) => mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        Key::Wheel { down, x, y } => mouse(
            if down {
                MouseEventKind::ScrollDown
            } else {
                MouseEventKind::ScrollUp
            },
            x,
            y,
        ),
        Key::Chord(pressed) => Some(Event::Key(pressed)),
        Key::Pointer(event) => Some(Event::Mouse(event)),
    }
}

/// A line being typed: `rat-text`'s single-line editor, focused. Its
/// cursor, its selection, its undo and the mouse are the editor's own.
#[derive(Debug)]
pub struct Typed {
    state: Box<TextInputState>,
}

impl Typed {
    /// `text` written, the cursor after it.
    pub fn new(text: &str) -> Typed {
        let mut state = TextInputState::new();
        state.set_text(text);
        let end = state.len();
        state.set_cursor(end, false);
        let mut typed = Typed {
            state: Box::new(state),
        };
        typed.focus(true);
        typed
    }

    /// `text` written, taking no key until it is focused: a list's `/`
    /// before it is pressed.
    pub fn unfocused(text: &str) -> Typed {
        let mut typed = Typed::new(text);
        typed.focus(false);
        typed
    }

    /// Whether the line takes the keys: a picker's `/` is shown unfocused
    /// once it is accepted.
    pub fn focus(&mut self, on: bool) {
        self.state.focus.set(on);
        self.state.focus.set_mouse_focus(on);
    }

    pub fn focused(&self) -> bool {
        self.state.focus.get()
    }

    pub fn text(&self) -> &str {
        self.state.text()
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// The cursor, a byte offset in `text`.
    pub fn at(&self) -> usize {
        self.state
            .try_byte_at(self.state.cursor())
            .map_or(self.state.len_bytes(), |range| range.start)
    }

    /// `text` written instead, the cursor at the byte `at`.
    pub fn set(&mut self, text: &str, at: usize) {
        self.state.set_text(text);
        let at = self
            .state
            .try_byte_pos(at.min(text.len()))
            .unwrap_or(self.state.len());
        self.state.set_cursor(at, false);
    }

    /// What precedes the cursor made `with`, what follows kept: a
    /// completion, which `Ctrl+Z` takes back as any edit.
    pub fn complete(&mut self, with: &str) {
        let cursor = self.state.cursor();
        self.state.set_selection(0, cursor);
        self.state.insert_str(with);
    }

    /// `text` put in where the cursor is, over what is selected: a paste.
    pub fn insert(&mut self, text: &str) -> bool {
        // One line: a paste of several is joined by spaces.
        let line = text.lines().collect::<Vec<_>>().join(" ");
        self.state.insert_str(line)
    }

    /// A key given to the editor; whether it did something with it —
    /// moved, selected, edited — rather than leaving it to the window.
    pub fn key(&mut self, key: Key) -> bool {
        event_of(key).is_some_and(|event| self.handle(&event))
    }

    pub fn handle(&mut self, event: &Event) -> bool {
        self.state.handle(event, Regular) != TextOutcome::Continue
    }

    /// What is selected copied to the clipboard; whether anything was.
    pub fn copy(&mut self) -> bool {
        if !self.state.has_selection() {
            return false;
        }
        // The crate's copy says it changed nothing, having copied.
        self.state.copy_to_clip();
        true
    }

    pub fn has_selection(&self) -> bool {
        self.state.has_selection()
    }

    pub fn selected(&self) -> &str {
        self.state.selected_text()
    }

    /// The editor's state, which the view draws and where it was drawn —
    /// what the mouse is read against.
    pub fn state(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    /// Where the cursor falls on the screen, once drawn: the editor's own,
    /// or at the end of a selection, where it is too.
    pub fn screen_cursor(&self) -> Option<(u16, u16)> {
        if !self.focused() {
            return None;
        }
        self.state.screen_cursor().or_else(|| {
            let column = self.state.col_to_screen(self.state.cursor())?;
            let inner = self.state.inner;
            (column < inner.width.max(1)).then_some((inner.x + column, inner.y))
        })
    }
}

impl Clone for Typed {
    fn clone(&self) -> Typed {
        let mut typed = Typed {
            state: self.state.clone(),
        };
        typed.focus(self.focused());
        typed
    }
}

/// Two lines are the same when their text and their cursor are.
impl PartialEq for Typed {
    fn eq(&self, other: &Typed) -> bool {
        self.text() == other.text() && self.at() == other.at()
    }
}

impl Eq for Typed {}

/// A line is shown as what is typed in it.
impl std::fmt::Display for Typed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.text())
    }
}

impl PartialEq<str> for Typed {
    fn eq(&self, other: &str) -> bool {
        self.text() == other
    }
}

impl PartialEq<String> for Typed {
    fn eq(&self, other: &String) -> bool {
        self.text() == other
    }
}

impl PartialEq<&str> for Typed {
    fn eq(&self, other: &&str) -> bool {
        self.text() == *other
    }
}

/// The note being written: `rat-text`'s text area, with `rat-markdown`'s keys
/// over its own and nothing added over those, its Markdown coloured as
/// `rat-markdown` reads it.
#[derive(Debug)]
pub struct Written {
    state: Box<TextAreaState>,
}

impl Written {
    /// `text` written, the cursor at its start.
    pub fn new(text: &str) -> Written {
        let mut state = TextAreaState::new();
        // What the file holds, whatever the system: Windows' `\r\n` would be
        // written into a note that has `\n`.
        state.set_newline("\n");
        state.set_tab_width(4);
        state.set_text(text);
        state.set_cursor((0, 0), false);
        // The editor moves up and down through what it last drew: until
        // it is drawn, a page of a terminal's usual size.
        state.inner = ratatui::layout::Rect::new(0, 0, 80, 24);
        state.rendered = state.inner.as_size();
        state.focus.set(true);
        state.focus.set_mouse_focus(true);
        let mut written = Written {
            state: Box::new(state),
        };
        written.restyle();
        written
    }

    pub fn text(&self) -> String {
        self.state.text()
    }

    /// The cursor: its line, and its column in characters.
    pub fn cursor(&self) -> (usize, usize) {
        let at = self.state.cursor();
        (at.y as usize, at.x as usize)
    }

    /// `text` put in where the cursor is, over what is selected: a paste.
    pub fn insert(&mut self, text: &str) -> bool {
        let changed = self.state.insert_str(text);
        self.restyle();
        changed
    }

    /// A key given to the editor, `rat-markdown`'s handling first; whether
    /// it did something with it.
    pub fn key(&mut self, key: Key) -> bool {
        event_of(key).is_some_and(|event| self.handle(&event))
    }

    pub fn handle(&mut self, event: &Event) -> bool {
        let outcome = self.state.handle(event, MarkDown::default());
        if outcome == TextOutcome::TextChanged {
            self.restyle();
        }
        outcome != TextOutcome::Continue
    }

    /// What is selected copied to the clipboard; whether anything was.
    pub fn copy(&mut self) -> bool {
        if !self.state.has_selection() {
            return false;
        }
        // The crate's copy says it changed nothing, having copied.
        self.state.copy_to_clip();
        true
    }

    pub fn state(&mut self) -> &mut TextAreaState {
        &mut self.state
    }

    /// Where the cursor falls on the screen, once drawn.
    pub fn screen_cursor(&self) -> Option<(u16, u16)> {
        self.state.screen_cursor()
    }

    /// The Markdown's parts, as `rat-markdown` finds them, for the view to
    /// colour: headings, a list's markers, emphasis, code.
    fn restyle(&mut self) {
        let text = self.state.text();
        self.state.set_styles(parse_md_styles(&text));
    }
}
