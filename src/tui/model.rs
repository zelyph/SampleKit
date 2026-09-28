//! The workbench's state and what each key does to it — no terminal here, so
//! that every interaction is a test that draws nothing. Everything
//! it changes or computes goes through the modules the command line and Python
//! call: `editing`, `explanation`, `tagging`, `discovery`, `opening`.
//!
//! A prototype (proto/tui): its keys and screens are tried in use before
//! they are specified.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::collection::computation;
use crate::collection::editing::{self, Became};
use crate::collection::explanation::{self, Explanation, InputState, Made, Origin};
use crate::collection::exports;
use crate::collection::sample_list::{self as list, SampleList};
use crate::collection::tagging;
use crate::config::configuration_edit::{self, ConfigurationEdit, Kind as Named};
use crate::config::discovery;
use crate::config::model_runtime as runtime;
use crate::config::profiles;
use crate::config::project_config::{
    ColumnSpec, DeclarationKind, DeclaredIn, FigureKind, Profile, ProjectConfig,
};
use crate::config::project_setup::{self, Step};
use crate::config::version_control;
use crate::core::identifier::Identifier;
use crate::core::sample::Sample;
use crate::format::document::{self, Destination};
use crate::format::fingerprint::Freshness;
use crate::format::schema::PrecisionSchema;
use crate::presentation::export_formats;
use crate::presentation::plotting;
use crate::presentation::terminal_rendering as render;
use crate::query::field_addressing::{self as fields, Subject};
use crate::query::filter_language as filter;
use crate::query::ordering;

pub use super::typing::{Typed, Written};

/// A key, as the workbench reads it: its own, so that no test needs a
/// terminal library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Delete,
    /// A mouse click, at a column and a row of the screen.
    Click(u16, u16),
    /// The wheel turned, down or up, with the pointer at a column and a row:
    /// the note under it scrolls, and elsewhere it moves as the arrows do.
    Wheel {
        down: bool,
        x: u16,
        y: u16,
    },
    /// Shift+Tab.
    BackTab,
    /// Shift+↑ and Shift+↓: the item under the cursor moved.
    MoveUp,
    MoveDown,
    /// A key held with Ctrl or Alt, or Shift with a key that moves: what a line
    /// typed or the note reads — a word, a selection, undo. Elsewhere it means
    /// nothing, and Shift alone is the key without it.
    Chord(crossterm::event::KeyEvent),
    /// The mouse dragged or let go, or clicked with a key held: what a line
    /// typed or the note reads to select.
    Pointer(crossterm::event::MouseEvent),
}

/// Which screen a binding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Collection,
    Sample,
    Table,
    Control,
    History,
    Setup,
    Configure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Top,
    Bottom,
    PageDown,
    PageUp,
    Open,
    Back,
    Filter,
    ClearFilter,
    Sort,
    Reverse,
    Columns,
    Basket,
    BasketOnly,
    TagBasket,
    Files,
    Edit,
    Explain,
    Editor,
    Help,
    Quit,
    GoTo,
    Unfold,
    NextColumn,
    PreviousColumn,
    Produce,
    Configure,
    /// The project's model in $EDITOR.
    Model,
    /// The sample, or the basket's, removed.
    RemoveSample,
    /// What is shown declared in `.samplekitrc`: a query, a profile, an export.
    Declare,
    /// The note shown scrolled, a few lines at a time.
    NoteDown,
    NoteUp,
    /// The project's history, or the sample's.
    History,
    Remove,
    Write,
    Reread,
    Summary,
    ScrollLeft,
    ScrollRight,
    Control,
    NewSample,
    AddRow,
    Undo,
    ComputeValue,
    ComputeSample,
    EditNote,
    /// The note shown beside the values, or hidden, remembered.
    ToggleNote,
    Select,
    SelectAll,
    ClearSelection,
    /// The collection grouped by fields, or ungrouped.
    Group,
}

/// Every binding, once: what a key does and what help says it does, from the
/// one table, so that the two cannot drift.
pub const BINDINGS: &[(Place, Key, Action, &str)] = &[
    (
        Place::Collection,
        Key::Char('j'),
        Action::Down,
        "next sample",
    ),
    (Place::Collection, Key::Down, Action::Down, "next sample"),
    (
        Place::Collection,
        Key::Char('k'),
        Action::Up,
        "previous sample",
    ),
    (Place::Collection, Key::Up, Action::Up, "previous sample"),
    (Place::Collection, Key::Home, Action::Top, "first sample"),
    (Place::Collection, Key::End, Action::Bottom, "last sample"),
    (
        Place::Collection,
        Key::PageDown,
        Action::PageDown,
        "a page down",
    ),
    (Place::Collection, Key::PageUp, Action::PageUp, "a page up"),
    (
        Place::Collection,
        Key::Enter,
        Action::Open,
        "open the sample",
    ),
    (
        Place::Collection,
        Key::Char('l'),
        Action::Open,
        "open the sample",
    ),
    (
        Place::Collection,
        Key::Right,
        Action::Open,
        "open the sample",
    ),
    (
        Place::Collection,
        Key::Char('f'),
        Action::GoTo,
        "apply a saved query or a profile",
    ),
    (
        Place::Collection,
        Key::Char('g'),
        Action::Group,
        "group by fields, a heading above each group",
    ),
    (
        Place::Collection,
        Key::Char('/'),
        Action::Filter,
        "filter, Tab completes",
    ),
    (
        Place::Collection,
        Key::Esc,
        Action::ClearFilter,
        "clear the filter",
    ),
    (
        Place::Collection,
        Key::Char('s'),
        Action::Sort,
        "sort by a column",
    ),
    (
        Place::Collection,
        Key::Char('r'),
        Action::Reverse,
        "reverse the sort",
    ),
    (
        Place::Collection,
        Key::Char('c'),
        Action::Columns,
        "choose the columns",
    ),
    (
        Place::Collection,
        Key::Char(' '),
        Action::Basket,
        "put in or take out of the basket",
    ),
    (
        Place::Collection,
        Key::Char('b'),
        Action::BasketOnly,
        "the basket only, or everything",
    ),
    (
        Place::Collection,
        Key::Char('t'),
        Action::TagBasket,
        "tag the basket",
    ),
    (
        Place::Collection,
        Key::Char('a'),
        Action::SelectAll,
        "put every sample shown in the basket",
    ),
    (
        Place::Collection,
        Key::Char('x'),
        Action::ClearSelection,
        "empty the basket",
    ),
    (
        Place::Collection,
        Key::Char('C'),
        Action::ComputeSample,
        "compute the selected values, or what is not current: the basket, or the sample",
    ),
    (
        Place::Collection,
        Key::Char('o'),
        Action::Files,
        "the sample's own files",
    ),
    (
        Place::Collection,
        Key::Char('E'),
        Action::Editor,
        "the sample in $EDITOR",
    ),
    (
        Place::Collection,
        Key::Char('p'),
        Action::Produce,
        "draw a figure, or write an export: of the basket, or of what is shown",
    ),
    (
        Place::Collection,
        Key::Char('N'),
        Action::NewSample,
        "a new sample, shaped like this one",
    ),
    (
        Place::Collection,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (
        Place::Collection,
        Key::Char('v'),
        Action::Control,
        "the collection's state: what is not current, what is defective",
    ),
    (
        Place::Collection,
        Key::Char('H'),
        Action::History,
        "the project's history: each change kept, and what it changed",
    ),
    (
        Place::Sample,
        Key::Char('H'),
        Action::History,
        "this sample's history: each change kept, and what it changed",
    ),
    (
        Place::Table,
        Key::Char('H'),
        Action::History,
        "the sample's history: each change kept, and what it changed",
    ),
    (Place::History, Key::Char('j'), Action::Down, "next"),
    (Place::History, Key::Down, Action::Down, "next"),
    (Place::History, Key::Char('k'), Action::Up, "previous"),
    (Place::History, Key::Up, Action::Up, "previous"),
    (Place::History, Key::Home, Action::Top, "the newest"),
    (Place::History, Key::End, Action::Bottom, "the oldest"),
    (
        Place::History,
        Key::PageDown,
        Action::PageDown,
        "a page down",
    ),
    (Place::History, Key::PageUp, Action::PageUp, "a page up"),
    (
        Place::History,
        Key::Char('J'),
        Action::NoteDown,
        "scroll what it changed down",
    ),
    (
        Place::History,
        Key::Char('K'),
        Action::NoteUp,
        "scroll what it changed up",
    ),
    (Place::History, Key::Esc, Action::Back, "back"),
    (Place::History, Key::Char('q'), Action::Back, "back"),
    (Place::History, Key::Left, Action::Back, "back"),
    (Place::History, Key::Char('h'), Action::Back, "back"),
    (Place::History, Key::Char('?'), Action::Help, "this help"),
    (Place::Control, Key::Char('j'), Action::Down, "next"),
    (Place::Control, Key::Down, Action::Down, "next"),
    (Place::Control, Key::Char('k'), Action::Up, "previous"),
    (Place::Control, Key::Up, Action::Up, "previous"),
    (Place::Control, Key::Home, Action::Top, "first"),
    (Place::Control, Key::End, Action::Bottom, "last"),
    (
        Place::Control,
        Key::PageDown,
        Action::PageDown,
        "a page down",
    ),
    (Place::Control, Key::PageUp, Action::PageUp, "a page up"),
    (
        Place::Control,
        Key::Enter,
        Action::Open,
        "open the sample, on its value",
    ),
    (
        Place::Control,
        Key::Char('C'),
        Action::ComputeSample,
        "compute every sample with a value outdated, failed or never computed",
    ),
    (
        Place::Control,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (
        Place::Control,
        Key::Esc,
        Action::Back,
        "back to the collection",
    ),
    (
        Place::Control,
        Key::Char('q'),
        Action::Back,
        "back to the collection",
    ),
    (Place::Control, Key::Char('?'), Action::Help, "this help"),
    (
        Place::Collection,
        Key::Char('P'),
        Action::Configure,
        "the project's configuration, section by section",
    ),
    (
        Place::Setup,
        Key::Char('j'),
        Action::Down,
        "the next answer",
    ),
    (Place::Setup, Key::Down, Action::Down, "the next answer"),
    (
        Place::Setup,
        Key::Char('k'),
        Action::Up,
        "the previous answer",
    ),
    (Place::Setup, Key::Up, Action::Up, "the previous answer"),
    (
        Place::Setup,
        Key::Enter,
        Action::Open,
        "this answer, then the next question; at the end, set the project up",
    ),
    (
        Place::Setup,
        Key::Esc,
        Action::Back,
        "the previous question; from the first, back to the start page or the collection",
    ),
    (
        Place::Setup,
        Key::Char('q'),
        Action::Back,
        "the previous question; from the first, back to the start page or the collection",
    ),
    (Place::Setup, Key::Char('?'), Action::Help, "this help"),
    (
        Place::Collection,
        Key::Char('M'),
        Action::Model,
        "the project's model in $EDITOR",
    ),
    (
        Place::Collection,
        Key::Char('W'),
        Action::Declare,
        "save what is shown in .samplekitrc: the filter, the columns, an export",
    ),
    (
        Place::Collection,
        Key::Char('D'),
        Action::RemoveSample,
        "remove the sample, or the basket's: its file, asked first, u gives it back",
    ),
    (
        Place::Sample,
        Key::Char('D'),
        Action::RemoveSample,
        "remove this sample: its file, asked first, u gives it back",
    ),
    (Place::Configure, Key::Char('j'), Action::Down, "next"),
    (Place::Configure, Key::Down, Action::Down, "next"),
    (Place::Configure, Key::Char('k'), Action::Up, "previous"),
    (Place::Configure, Key::Up, Action::Up, "previous"),
    (
        Place::Configure,
        Key::Right,
        Action::NextColumn,
        "into the section",
    ),
    (
        Place::Configure,
        Key::Char('l'),
        Action::NextColumn,
        "into the section",
    ),
    (
        Place::Configure,
        Key::Tab,
        Action::NextColumn,
        "into the section",
    ),
    (
        Place::Configure,
        Key::Left,
        Action::PreviousColumn,
        "back to the sections",
    ),
    (
        Place::Configure,
        Key::Char('h'),
        Action::PreviousColumn,
        "back to the sections",
    ),
    (
        Place::Configure,
        Key::Enter,
        Action::Open,
        "change the setting; on an entry, add a key to it",
    ),
    (
        Place::Configure,
        Key::Char('e'),
        Action::Edit,
        "change the setting; on an entry, add a key to it",
    ),
    (
        Place::Configure,
        Key::Char('a'),
        Action::AddRow,
        "add a setting: key = value, or name.key = value",
    ),
    (
        Place::Configure,
        Key::Char('d'),
        Action::Remove,
        "remove the setting, or the whole entry",
    ),
    (
        Place::Configure,
        Key::Char('w'),
        Action::Write,
        "write the file, its changes previewed",
    ),
    (
        Place::Configure,
        Key::Char('r'),
        Action::Reread,
        "read the file again, dropping the changes not written",
    ),
    (
        Place::Configure,
        Key::Esc,
        Action::Back,
        "back to the collection, keeping the changes not written",
    ),
    (
        Place::Configure,
        Key::Char('q'),
        Action::Back,
        "back to the collection, keeping the changes not written",
    ),
    (Place::Configure, Key::Char('?'), Action::Help, "this help"),
    (
        Place::Collection,
        Key::Char('S'),
        Action::Summary,
        "the columns summarised, or the samples again",
    ),
    (
        Place::Collection,
        Key::Char('<'),
        Action::ScrollLeft,
        "the columns further left",
    ),
    (
        Place::Collection,
        Key::Char('>'),
        Action::ScrollRight,
        "the columns further right",
    ),
    (Place::Collection, Key::Char('?'), Action::Help, "this help"),
    (Place::Collection, Key::Char('q'), Action::Quit, "quit"),
    (Place::Sample, Key::Char('j'), Action::Down, "next value"),
    (Place::Sample, Key::Down, Action::Down, "next value"),
    (Place::Sample, Key::Char('k'), Action::Up, "previous value"),
    (Place::Sample, Key::Up, Action::Up, "previous value"),
    (Place::Sample, Key::Home, Action::Top, "first value"),
    (Place::Sample, Key::End, Action::Bottom, "last value"),
    (
        Place::Sample,
        Key::PageDown,
        Action::PageDown,
        "a page down",
    ),
    (Place::Sample, Key::PageUp, Action::PageUp, "a page up"),
    (
        Place::Sample,
        Key::Char('e'),
        Action::Edit,
        "change the value, previewed",
    ),
    (
        Place::Sample,
        Key::Enter,
        Action::Explain,
        "where the value came from",
    ),
    (
        Place::Sample,
        Key::Char(' '),
        Action::Select,
        "select the value, for c",
    ),
    (
        Place::Sample,
        Key::Char('a'),
        Action::SelectAll,
        "select every value a formula gives",
    ),
    (
        Place::Sample,
        Key::Char('x'),
        Action::ClearSelection,
        "clear the selection",
    ),
    (Place::Sample, Key::Right, Action::Unfold, "unfold a table"),
    (
        Place::Sample,
        Key::Char('l'),
        Action::Unfold,
        "unfold a table",
    ),
    (
        Place::Sample,
        Key::Char('c'),
        Action::ComputeValue,
        "compute the selected values, or this one, even if current or edited",
    ),
    (
        Place::Sample,
        Key::Char('C'),
        Action::ComputeSample,
        "compute what is not current in the sample",
    ),
    (
        Place::Sample,
        Key::Char('n'),
        Action::ToggleNote,
        "show or hide the note, remembered",
    ),
    (
        Place::Sample,
        Key::Char('N'),
        Action::EditNote,
        "edit the note",
    ),
    (
        Place::Sample,
        Key::Char('o'),
        Action::Files,
        "the sample's own files",
    ),
    (
        Place::Sample,
        Key::Char('E'),
        Action::Editor,
        "the sample in $EDITOR",
    ),
    (
        Place::Sample,
        Key::Esc,
        Action::Back,
        "back to the collection, or to the control screen it was opened from",
    ),
    (
        Place::Sample,
        Key::Char('h'),
        Action::Back,
        "back to the collection, or to the control screen it was opened from",
    ),
    (
        Place::Sample,
        Key::Left,
        Action::Back,
        "back to the collection, or to the control screen it was opened from",
    ),
    (
        Place::Sample,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (
        Place::Sample,
        Key::Char('J'),
        Action::NoteDown,
        "scroll the note down; the wheel over it too",
    ),
    (
        Place::Sample,
        Key::Char('K'),
        Action::NoteUp,
        "scroll the note up",
    ),
    (Place::Sample, Key::Char('?'), Action::Help, "this help"),
    (
        Place::Sample,
        Key::Char('q'),
        Action::Back,
        "back to the collection, or to the control screen it was opened from",
    ),
    (Place::Table, Key::Char('j'), Action::Down, "next row"),
    (Place::Table, Key::Down, Action::Down, "next row"),
    (Place::Table, Key::Char('k'), Action::Up, "previous row"),
    (Place::Table, Key::Up, Action::Up, "previous row"),
    (Place::Table, Key::Home, Action::Top, "first row"),
    (Place::Table, Key::End, Action::Bottom, "last row"),
    (Place::Table, Key::PageDown, Action::PageDown, "a page down"),
    (Place::Table, Key::PageUp, Action::PageUp, "a page up"),
    (Place::Table, Key::Right, Action::NextColumn, "next column"),
    (
        Place::Table,
        Key::Char('l'),
        Action::NextColumn,
        "next column",
    ),
    (
        Place::Table,
        Key::Left,
        Action::PreviousColumn,
        "previous column; from the first, back to the sample",
    ),
    (
        Place::Table,
        Key::Char('h'),
        Action::PreviousColumn,
        "previous column; from the first, back to the sample",
    ),
    (
        Place::Table,
        Key::Char('e'),
        Action::Edit,
        "change the cell, previewed",
    ),
    (
        Place::Table,
        Key::Enter,
        Action::Explain,
        "where the cell came from",
    ),
    (
        Place::Table,
        Key::Char('c'),
        Action::ComputeValue,
        "compute the selected columns, or this one, even if current or edited",
    ),
    (
        Place::Table,
        Key::Char('C'),
        Action::ComputeSample,
        "compute what is not current in the sample",
    ),
    (
        Place::Table,
        Key::Char(' '),
        Action::Select,
        "select the column, for c",
    ),
    (
        Place::Table,
        Key::Char('x'),
        Action::ClearSelection,
        "clear the selection",
    ),
    (
        Place::Table,
        Key::Char('o'),
        Action::Files,
        "the sample's own files",
    ),
    (
        Place::Table,
        Key::Char('E'),
        Action::Editor,
        "the sample in $EDITOR",
    ),
    (Place::Table, Key::Esc, Action::Back, "back to the sample"),
    (
        Place::Table,
        Key::Char('p'),
        Action::Produce,
        "a figure of this table: a column against another, the selected first",
    ),
    (
        Place::Table,
        Key::Char('q'),
        Action::Back,
        "back to the sample",
    ),
    (
        Place::Table,
        Key::Char('+'),
        Action::AddRow,
        "add a row, column=value, …",
    ),
    (
        Place::Table,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (Place::Table, Key::Char('?'), Action::Help, "this help"),
];

/// An action in a word, for the line of hints at the bottom, where the help's
/// sentence does not fit.
pub fn short(action: Action) -> &'static str {
    match action {
        Action::Down | Action::Up => "move",
        Action::Top => "first",
        Action::Bottom => "last",
        Action::PageDown | Action::PageUp => "page",
        Action::Open => "open",
        Action::Back => "back",
        Action::Filter => "filter",
        Action::ClearFilter => "clear filter",
        Action::Sort => "sort",
        Action::Reverse => "reverse",
        Action::Columns => "columns",
        Action::Basket => "basket",
        Action::BasketOnly => "basket only",
        Action::TagBasket => "tag",
        Action::Files => "files",
        Action::Edit => "edit",
        Action::Explain => "explain",
        Action::Editor => "editor",
        Action::Help => "help",
        Action::Quit => "quit",
        Action::GoTo => "apply",
        Action::Unfold => "unfold",
        Action::ComputeValue => "compute",
        Action::ComputeSample => "compute all",
        Action::EditNote => "edit note",
        Action::ToggleNote => "note",
        Action::Select => "select",
        Action::NextColumn | Action::PreviousColumn => "column",
        Action::Produce => "figures, exports",
        Action::Control => "state",
        Action::History => "history",
        Action::Configure => "configuration",
        Action::Model => "model",
        Action::RemoveSample => "remove",
        Action::Declare => "save",
        Action::NoteDown => "note down",
        Action::NoteUp => "note up",
        Action::Remove => "remove",
        Action::Write => "write",
        Action::Reread => "read again",
        Action::Summary => "summary",
        Action::ScrollLeft | Action::ScrollRight => "more columns",
        Action::NewSample => "new",
        Action::AddRow => "add a row",
        Action::Undo => "undo",
        Action::SelectAll => "all",
        Action::ClearSelection => "clear",
        Action::Group => "group",
    }
}

/// The actions the line of hints offers, most wanted first: as many as the
/// terminal's width holds are shown, and help always.
pub fn hinted(place: Place) -> &'static [Action] {
    // The line at the foot: moving about and what concerns the whole screen.
    // What acts on the thing under the cursor is on its own frame instead
    // (`Workbench::element_hints`).
    match place {
        // `N` and `u` before what is used less each day — the summary, the
        // configuration, `g`, `f` — so that an 80-column terminal shows them.
        Place::Collection => &[
            Action::Filter,
            Action::Sort,
            Action::Columns,
            Action::NewSample,
            Action::Undo,
            Action::Produce,
            Action::Control,
            Action::History,
            Action::Summary,
            Action::Configure,
            Action::Group,
            Action::GoTo,
            Action::Declare,
            Action::RemoveSample,
            Action::Model,
            Action::ComputeSample,
            Action::SelectAll,
            Action::ClearSelection,
            Action::TagBasket,
            Action::Reverse,
            Action::BasketOnly,
            Action::Quit,
        ],
        Place::Sample => &[
            Action::Back,
            Action::History,
            Action::Undo,
            Action::ComputeSample,
            Action::SelectAll,
            Action::ClearSelection,
            Action::Files,
            Action::Editor,
        ],
        Place::Control => &[Action::Back, Action::ComputeSample, Action::Undo],
        Place::History => &[Action::Back],
        Place::Setup => &[Action::Open, Action::Back],
        Place::Configure => &[Action::Write, Action::Reread, Action::Back],
        Place::Table => &[
            Action::NextColumn,
            Action::Back,
            Action::History,
            Action::AddRow,
            Action::Undo,
            Action::ComputeSample,
            Action::ClearSelection,
            Action::Files,
            Action::Editor,
        ],
    }
}

/// What the runtime does that the model cannot: a program started, the
/// terminal given to an editor, the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    Open(PathBuf),
    Navigate(PathBuf),
    Editor(PathBuf),
}

#[derive(Clone)]
pub enum Screen {
    /// Setting the project up: its steps, to tick.
    Setup {
        cursor: usize,
    },
    /// The project's configuration, section by section.
    Configure,
    Collection,
    /// A sample, by its position in the view, and the value the cursor is on.
    Sample {
        at: usize,
        cursor: usize,
    },
    /// The collection's state: what is not current, what is defective.
    Control {
        cursor: usize,
    },
    /// The project's history, newest first, or one sample's — `sample` its
    /// place in the view. `scroll` is how far what the snapshot changed is
    /// scrolled; `back` is the screen `H` was pressed on, where `Back` returns
    /// as it was left — its cursor, a table's column: rebuilt from `sample`, it
    /// came back on the sample's first line.
    History {
        cursor: usize,
        scroll: usize,
        sample: Option<usize>,
        back: Box<Screen>,
    },
    /// One of a sample's tables unfolded, `back` the sample screen's cursor;
    /// `cursor` its row and `column` among its columns, the index not counted.
    Table {
        at: usize,
        back: usize,
        name: Identifier,
        cursor: usize,
        column: usize,
    },
}

pub enum Mode {
    Normal,
    /// The filter typed, in its line: the completions are those of what
    /// precedes its cursor, the rest kept after them.
    Filter {
        text: Typed,
        candidates: Vec<String>,
        chosen: Option<usize>,
        error: Option<String>,
    },
    /// A line typed for `purpose`.
    Prompt {
        purpose: Purpose,
        text: Typed,
    },
    /// A list to choose from; `/` narrows it to the items holding `query`,
    /// and `cursor` counts among those. `typing` while the query is typed.
    Picker {
        purpose: Choosing,
        items: Vec<(String, bool)>,
        cursor: usize,
        query: Typed,
        typing: bool,
    },
    Confirm {
        title: String,
        lines: Vec<String>,
        pending: Box<Pending>,
    },
    Panel {
        title: String,
        lines: Vec<String>,
        scroll: usize,
    },
    Files {
        files: Vec<PathBuf>,
        cursor: usize,
    },
    /// A quantity's value, its uncertainty and its readings, edited together;
    /// `on` is the slot typed into.
    Quantity {
        path: PathBuf,
        field: String,
        slots: Vec<Slot>,
        on: usize,
    },
    /// The keys of the screen it was asked on, by section, scrolled.
    Help {
        scroll: usize,
    },
    /// A figure of one's own set up in one window: what it draws and how, a row
    /// each; the form is the workbench's `figure`, kept for next time.
    Figure,
    /// The note edited in place, in `rat-text`'s area with `rat-markdown`'s
    /// keys.
    Note {
        path: PathBuf,
        text: Written,
        original: String,
    },
}

impl Mode {
    /// A prompt with `text` written, the cursor after it.
    fn prompt(purpose: Purpose, text: String) -> Mode {
        Mode::Prompt {
            purpose,
            text: Typed::new(&text),
        }
    }

    /// Whether something is being typed: a key held with Ctrl or Alt, and the
    /// mouse dragged, mean something only then.
    pub fn typing(&self) -> bool {
        matches!(
            self,
            Mode::Filter { .. }
                | Mode::Prompt { .. }
                | Mode::Quantity { .. }
                | Mode::Note { .. }
                | Mode::Picker { typing: true, .. }
        )
    }
}

#[derive(Clone)]
pub enum Purpose {
    /// A text row of the figure being set up: its title or an axis's label.
    FigureText(FigureRow),
    /// The name what is shown is declared under.
    DeclareName(Declare),
    Edit {
        path: PathBuf,
        field: String,
    },
    Tag,
    /// One sample's tags, written whole.
    Tags {
        path: PathBuf,
    },
    /// A configuration setting's value, in a section of `SECTIONS`.
    Setting {
        section: usize,
        name: Option<String>,
        key: String,
    },
    /// A new setting, `key = value` or `name.key = value`.
    AddSetting {
        section: usize,
    },
    /// A new key of one named entry, `key = value`.
    AddKey {
        section: usize,
        name: String,
    },
    /// A new sample's name, shaped like `like`, or from nothing.
    New {
        like: Option<PathBuf>,
    },
    /// A row for a table, `column=value, …`.
    Row {
        path: PathBuf,
        table: String,
    },
    /// The model file one already has, asked by the setup.
    ModelPath,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Choosing {
    Sort,
    Columns,
    /// Where `f` goes, an item's target at its position.
    GoTo(Vec<Target>),
    /// The fields `g` groups by.
    Group,
    /// What `p` draws or writes, an item's at its position.
    Produce(Vec<Output>),
    /// A field for a row of the figure being set up.
    FigureField(FigureRow),
    /// What `W` declares, an item's at its position.
    Declare(Vec<Declare>),
}

/// What `W`, and the figure's `w`, declare in `.samplekitrc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declare {
    /// The filter as a query.
    Query,
    /// The columns and the sort as a profile.
    Profile,
    /// The columns as an export in this format, its profile beside it, and
    /// the filter as its query where there is one.
    Export(&'static str),
    /// The figure set up in its window.
    Figure,
}

/// A row of the figure's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureRow {
    Y,
    X,
    Kind,
    Group,
    Style,
    Title,
    YLabel,
    XLabel,
    YScale,
    XScale,
    Legend,
}

impl FigureRow {
    pub fn label(self) -> &'static str {
        match self {
            FigureRow::Y => "y, up",
            FigureRow::X => "x, across",
            FigureRow::Kind => "kind",
            FigureRow::Group => "grouped by",
            FigureRow::Style => "labels' style",
            FigureRow::Title => "title",
            FigureRow::YLabel => "y label",
            FigureRow::XLabel => "x label",
            FigureRow::YScale => "y scale",
            FigureRow::XScale => "x scale",
            FigureRow::Legend => "legend",
        }
    }
}

const SCALES: [&str; 3] = ["linear", "log", "symlog"];
const LEGENDS: [&str; 3] = ["best", "outside", "none"];

/// A figure of one's own as it is being set up: what it draws, then how. An
/// empty text is the one the field gives.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureForm {
    pub y: Option<String>,
    pub x: Option<String>,
    pub kind: FigureKind,
    pub group: Option<String>,
    pub style: String,
    pub title: String,
    pub y_label: String,
    pub x_label: String,
    pub y_scale: &'static str,
    pub x_scale: &'static str,
    pub legend: &'static str,
    /// The row the cursor is on, among `rows`.
    pub row: usize,
    /// One sample's table, when `p` was pressed over it: its columns the
    /// axes, that sample alone drawn, nothing to group.
    pub of: Option<(PathBuf, Identifier)>,
}

impl FigureForm {
    pub fn new(style: String, of: Option<(PathBuf, Identifier)>) -> FigureForm {
        FigureForm {
            y: None,
            x: None,
            kind: FigureKind::Scatter,
            group: None,
            style,
            title: String::new(),
            y_label: String::new(),
            x_label: String::new(),
            y_scale: "linear",
            x_scale: "linear",
            legend: "best",
            row: 0,
            of,
        }
    }

    /// The rows shown: a group only where there are samples to group.
    pub fn rows(&self) -> Vec<FigureRow> {
        use FigureRow::*;
        [
            Y, X, Kind, Group, Style, Title, YLabel, XLabel, YScale, XScale, Legend,
        ]
        .into_iter()
        .filter(|row| *row != Group || self.of.is_none())
        .collect()
    }

    /// A row's value as the window shows it.
    pub fn shown(&self, row: FigureRow) -> String {
        let or = |text: &str, otherwise: &str| {
            if text.is_empty() {
                otherwise.to_string()
            } else {
                text.to_string()
            }
        };
        match row {
            FigureRow::Y => self
                .y
                .clone()
                .unwrap_or_else(|| "— Enter chooses".to_string()),
            FigureRow::X => self
                .x
                .clone()
                .unwrap_or_else(|| "— Enter chooses".to_string()),
            FigureRow::Kind => self.kind.as_str().to_string(),
            FigureRow::Group => self.group.clone().unwrap_or_else(|| "none".to_string()),
            FigureRow::Style => self.style.clone(),
            FigureRow::Title => or(&self.title, "— none"),
            FigureRow::YLabel => or(&self.y_label, "— the field's symbol and unit"),
            FigureRow::XLabel => or(&self.x_label, "— the field's symbol and unit"),
            FigureRow::YScale => self.y_scale.to_string(),
            FigureRow::XScale => self.x_scale.to_string(),
            FigureRow::Legend => self.legend.to_string(),
        }
    }

    /// A text row's text.
    pub fn text(&self, row: FigureRow) -> &str {
        match row {
            FigureRow::Title => &self.title,
            FigureRow::YLabel => &self.y_label,
            FigureRow::XLabel => &self.x_label,
            _ => "",
        }
    }

    pub fn set_text(&mut self, row: FigureRow, text: &str) {
        let text = text.to_string();
        match row {
            FigureRow::Title => self.title = text,
            FigureRow::YLabel => self.y_label = text,
            FigureRow::XLabel => self.x_label = text,
            _ => {}
        }
    }

    /// A field chosen for an axis or the group; `none` for no group.
    pub fn set_field(&mut self, row: FigureRow, field: String) {
        match row {
            FigureRow::Y => self.y = Some(field),
            FigureRow::X => self.x = Some(field),
            FigureRow::Group => self.group = (field != "none").then_some(field),
            _ => {}
        }
    }

    /// A row of choices moved to the next, or the one before.
    pub fn cycle(&mut self, row: FigureRow, by: isize, styles: &[String]) {
        fn next<T: Copy + PartialEq>(among: &[T], now: T, by: isize) -> T {
            let at = among.iter().position(|held| *held == now).unwrap_or(0) as isize;
            among[(at + by).rem_euclid(among.len() as isize) as usize]
        }
        match row {
            FigureRow::Kind => self.kind = next(&FigureKind::ALL, self.kind, by),
            FigureRow::Style if !styles.is_empty() => {
                let at = styles
                    .iter()
                    .position(|name| *name == self.style)
                    .unwrap_or(0) as isize;
                self.style = styles[(at + by).rem_euclid(styles.len() as isize) as usize].clone();
            }
            FigureRow::YScale => self.y_scale = next(&SCALES, self.y_scale, by),
            FigureRow::XScale => self.x_scale = next(&SCALES, self.x_scale, by),
            FigureRow::Legend => self.legend = next(&LEGENDS, self.legend, by),
            _ => {}
        }
    }

    /// The axes, as `plot -x -y` gives them.
    pub fn declaration(
        &self,
        x: &str,
        y: &str,
    ) -> crate::config::project_config::FigureDeclaration {
        crate::config::project_config::FigureDeclaration::ad_hoc(
            x,
            y,
            self.kind,
            self.group.clone().filter(|_| self.of.is_none()),
        )
    }

    /// How it is drawn, as `plot`'s options give it: what travels to Python
    /// beside the axes. Set on the declaration, it was never sent.
    pub fn overrides(&self) -> plotting::FigureOverrides {
        let given = |text: &str| (!text.is_empty()).then(|| text.to_string());
        plotting::FigureOverrides {
            title: given(&self.title),
            x_label: given(&self.x_label),
            y_label: given(&self.y_label),
            style: Some(self.style.clone()),
            x_scale: (self.x_scale != "linear").then(|| self.x_scale.to_string()),
            y_scale: (self.y_scale != "linear").then(|| self.y_scale.to_string()),
            legend: (self.legend != "best").then(|| self.legend.to_string()),
            ..Default::default()
        }
    }
}

/// One thing a quantity's window edits: its text, and what it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub label: &'static str,
    /// The unit the value is in, said beside its label: a value typed with
    /// its unit, `21 L`, is refused.
    pub unit: String,
    /// What is typed, in its line.
    pub text: Typed,
    pub before: String,
}

impl Slot {
    fn of(label: &'static str, text: String) -> Slot {
        Slot {
            label,
            unit: String::new(),
            text: Typed::unfocused(&text),
            before: text,
        }
    }

    fn changed(&self) -> bool {
        self.text.text() != self.before
    }
}

/// The project's setup, asked as questions: the answers so far, and which
/// question is on screen — past the last, what will be done.
pub struct Setup {
    pub answers: project_setup::Answers,
    pub question: usize,
}

impl Setup {
    /// The questions that apply as the answers stand.
    pub fn questions(&self, root: &Path) -> Vec<project_setup::Question> {
        project_setup::questions(root, &self.answers)
    }

    /// What the answers would do.
    pub fn plan(&self, root: &Path) -> project_setup::Plan {
        project_setup::plan(root, &self.answers)
    }
}

/// The configuration being edited, and where the cursor is in it.
pub struct Workspace {
    pub edit: ConfigurationEdit,
    pub section: usize,
    pub row: usize,
    /// On a section's settings, rather than on the list of sections.
    pub on_rows: bool,
    /// Changed and not written.
    pub dirty: bool,
}

/// A section of the configuration: one of its own, or named entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Setting(&'static str),
    Named(Named),
}

/// Every section the configuration screen shows, in order, each with the keys
/// it takes, as project config reads them.
pub const SECTIONS: &[(&str, Section, &str)] = &[
    (
        "collection",
        Section::Setting("collection"),
        "recursive, include, exclude, files",
    ),
    ("model", Section::Setting("model"), "path, class, python"),
    (
        "render",
        Section::Setting("render"),
        "style, precision, table, identify, figure_style",
    ),
    (
        "units",
        Section::Named(Named::Unit),
        "a spelling per style: plain, math, figure…",
    ),
    (
        "properties",
        Section::Named(Named::Property),
        "unit, symbol, precision, symbol_<style>",
    ),
    ("queries", Section::Named(Named::Query), "filter, directory"),
    (
        "profiles",
        Section::Named(Named::Profile),
        "columns, sort, group",
    ),
    (
        "exports",
        Section::Named(Named::Export),
        "profile, format, output, filename, path, query",
    ),
    (
        "figures",
        Section::Named(Named::Figure),
        "kind, x, y, group, query, title, x_label, y_label, style, x_limits, y_limits, x_scale, y_scale, aspect, legend, figsize",
    ),
    ("styles", Section::Named(Named::Style), "separator"),
    (
        "matplotlib",
        Section::Setting("matplotlib"),
        "matplotlib's own names: font.size = 13",
    ),
    (
        "TUI colours",
        Section::Setting("tui.colors"),
        "failed, outdated, edited, current, selected, attribute, table, index, key, muted, message, error",
    ),
];

/// One line of a configuration section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRow {
    /// A named entry: `[query.approved]`.
    Name(String),
    Key {
        name: Option<String>,
        key: String,
        value: String,
    },
}

/// Why a quantity's value, or its uncertainty, as typed is not a number:
/// written with a unit, `21 L`, or a decimal comma, `21,5`. `None` for a
/// number, an empty text, or a text that is none of these.
fn number_refused(text: &str, unit: &str) -> Option<String> {
    let number = |text: &str| text.trim().parse::<f64>().is_ok();
    if let Some((whole, decimals)) = text.split_once(',')
        && number(whole)
        && !decimals.is_empty()
        && decimals.chars().all(|c| c.is_ascii_digit())
    {
        return Some(format!(
            "'{text}' has a decimal comma: use a point for decimals — {whole}.{decimals}"
        ));
    }
    let digits = text
        .find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c)))
        .unwrap_or(text.len());
    let (figure, rest) = text.split_at(digits);
    if !figure.is_empty() && number(figure) && !rest.trim().is_empty() {
        let unit = if unit.is_empty() {
            String::new()
        } else {
            format!(": it is in {unit}")
        };
        return Some(format!(
            "'{text}': write the number alone, {}{unit}",
            figure.trim()
        ));
    }
    None
}

/// A number written with a decimal comma in a filter — `1,05` outside
/// quotes, digits on both sides — said with the number it meant; `None`
/// where there is none. Asked only of a filter that does not read, where
/// the comma is what the parser stopped at.
/// The values a filter compares a field with by `==`, or tests a list with
/// by `has`: for each, what precedes it through its operator, the clause it
/// ends, and the value as written. Read from the text, quotes kept whole;
/// never failing, as completion does not.
fn compared_values(text: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut quote: Option<char> = None;
    let mut at = 0;
    while at < characters.len() {
        let (byte, character) = characters[at];
        if let Some(open) = quote {
            if character == open {
                quote = None;
            }
            at += 1;
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
            at += 1;
            continue;
        }
        let rest = &text[byte..];
        let starts_word = byte == 0 || text[..byte].ends_with([' ', '(']);
        let operator = if rest.starts_with("==") {
            2
        } else if starts_word && rest.starts_with("has ") {
            3
        } else {
            0
        };
        if operator == 0 {
            at += 1;
            continue;
        }
        let after = byte + operator;
        let value_at = after + (text[after..].len() - text[after..].trim_start().len());
        let tail = &text[value_at..];
        let length = match tail.chars().next() {
            Some(open @ ('"' | '\'')) => tail[1..].find(open).map_or(tail.len(), |end| end + 2),
            Some(_) => tail
                .find(|c: char| c.is_whitespace() || c == ')' || c == '&' || c == '|')
                .unwrap_or(tail.len()),
            None => 0,
        };
        let value = tail[..length].to_string();
        let before = &text[..byte];
        let clause_at = ["&&", "||", "("]
            .iter()
            .filter_map(|connective| before.rfind(connective).map(|at| at + connective.len()))
            .max()
            .unwrap_or(0);
        let clause = format!("{}{}", text[clause_at..value_at].trim_start(), value);
        found.push((format!("{} ", text[..after].trim_end()), clause, value));
        at = characters
            .iter()
            .position(|(index, _)| *index >= value_at + length)
            .unwrap_or(characters.len());
    }
    found
}

fn decimal_comma(text: &str) -> Option<String> {
    let characters: Vec<char> = text.chars().collect();
    let mut quote: Option<char> = None;
    for (at, character) in characters.iter().enumerate() {
        match (quote, *character) {
            (Some(open), close) if close == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(*character),
            (None, ',') => {
                let digit = |at: usize| characters.get(at).is_some_and(char::is_ascii_digit);
                if at == 0 || !digit(at - 1) || !digit(at + 1) {
                    continue;
                }
                let start = (0..at)
                    .rev()
                    .take_while(|at| characters[*at].is_ascii_digit() || characters[*at] == '.')
                    .last()
                    .unwrap_or(at);
                // A digit at the end of a name — `load2,3` — is no number.
                if start > 0
                    && (characters[start - 1].is_alphanumeric() || characters[start - 1] == '_')
                {
                    continue;
                }
                let end = (at + 1..characters.len())
                    .take_while(|at| characters[*at].is_ascii_digit())
                    .last()
                    .map_or(at + 1, |last| last + 1);
                let whole: String = characters[start..end].iter().collect();
                return Some(format!(
                    "'{whole}' has a decimal comma: use a point for decimals — {}",
                    whole.replace(',', ".")
                ));
            }
            _ => {}
        }
    }
    None
}

/// A count and its noun, singular for one.
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The marks a sample or a value is shown with, and what each says: the
/// help's last section, and the control screen's title and lines. Each mark
/// says one thing: `✗` was a failure and a defect, and `·` both readings with
/// no statistic and, on the control screen, a note — beside ` · `, the
/// separator of the title that counted them.
pub const MARKS: [(&str, &str); 8] = [
    ("●", "in the basket"),
    (
        "⚠",
        "outdated: an input changed, or its record cannot be checked — C computes it",
    ),
    (
        "✎",
        "edited: a value typed over its formula, or its record missing",
    ),
    ("✗", "failed: its formula raised an error when last run"),
    (
        "∅",
        "never computed, or waits for an input: what the model owes",
    ),
    (
        "·",
        "readings with no statistic chosen: nothing computes the value",
    ),
    (
        "⊘",
        "defective, on the control screen: what validate refuses",
    ),
    (
        "¶",
        "a note, on the control screen: what validate remarks on",
    ),
];

/// The mark of a line of the control screen, from `MARKS`.
pub fn concern_mark(concern: Concern) -> &'static str {
    match concern {
        Concern::Failed => "✗",
        Concern::Defect => "⊘",
        Concern::Stale => "⚠",
        Concern::Owed => "∅",
        Concern::Edited => "✎",
        Concern::Note => "¶",
    }
}

/// The help's sections, in the order it shows them.
pub const HELP_SECTIONS: [&str; 8] = [
    "moving",
    "finding and showing",
    "choosing",
    "changing",
    "computing and checking",
    "figures and exports",
    "the project",
    "the TUI",
];

/// The keys of one screen, by section and in the order they are bound: each
/// action once, with every key bound to it, and no section left empty. What
/// `?` shows there before the marks, and what the reference page of the
/// documentation is written from, so that neither can drift from the keys.
pub fn keys_of(place: Place) -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    let mut sections: Vec<(&'static str, Vec<(String, &'static str)>)> = HELP_SECTIONS
        .iter()
        .map(|section| (*section, Vec::new()))
        .collect();
    let mut seen: Vec<Action> = Vec::new();
    for (at, _, action, said) in BINDINGS {
        if *at != place || seen.contains(action) {
            continue;
        }
        seen.push(*action);
        let keys: Vec<String> = BINDINGS
            .iter()
            .filter(|(at, _, bound, _)| *at == place && bound == action)
            .map(|(_, key, _, _)| key_name(*key))
            .collect();
        let section = section_of(*action);
        if let Some((_, rows)) = sections.iter_mut().find(|(name, _)| *name == section) {
            rows.push((keys.join(" "), *said));
        }
    }
    sections.retain(|(_, rows)| !rows.is_empty());
    sections
}

/// The section of the help an action is said in.
fn section_of(action: Action) -> &'static str {
    use Action::*;
    match action {
        Down | Up | Top | Bottom | PageDown | PageUp | Open | Back | Unfold | NextColumn
        | PreviousColumn | ScrollLeft | ScrollRight | NoteDown | NoteUp => "moving",
        // `f` applies what is saved to what is shown, and goes nowhere.
        Filter | ClearFilter | Sort | Reverse | Columns | Summary | BasketOnly | Group | GoTo
        | ToggleNote => "finding and showing",
        Select | SelectAll | ClearSelection | Basket | TagBasket => "choosing",
        Declare => "the project",
        Edit | EditNote | NewSample | RemoveSample | AddRow | Undo | Remove | Write | Reread => {
            "changing"
        }
        ComputeValue | ComputeSample | Explain | Control | History => "computing and checking",
        Produce => "figures and exports",
        Configure | Model | Editor | Files => "the project",
        Help | Quit => "the TUI",
    }
}

/// A setup step as a preview names it.
pub fn step_name(step: &Step, root: &Path) -> String {
    format!("{:<28} {}", step.name(root), step.what())
}

/// What a line of the control screen is about, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Concern {
    Failed,
    Defect,
    Stale,
    /// What the model owes: never computed, or waiting for an input.
    Owed,
    Edited,
    Note,
}

/// One line of the control screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlItem {
    pub path: Option<PathBuf>,
    pub sample: String,
    /// The value it is about, where it is one: what the sample opens on.
    pub field: Option<String>,
    pub said: String,
    pub kind: Concern,
}

/// A figure or an export the project declares.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// A figure of one's own: a field against another.
    Own,
    Figure(String),
    /// A figure the model declares: its code runs, once allowed.
    ModelFigure(String),
    Export(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A saved query's filter.
    Query(String),
    /// A profile, as the project declaring it reads it: over several projects
    /// the view has no one configuration to look its name up in.
    Profile(Box<Profile>),
}

/// What `y` does in a confirmation, as its hint says it: every window said
/// *y write*, over an undo and a removal too.
pub fn confirm_action(pending: &Pending) -> &'static str {
    match pending {
        Pending::Undo(_) => "y restore · n cancel · j k scroll",
        Pending::Remove(_) => "y delete · n cancel · j k scroll",
        Pending::Quit => "y leave · n stay",
        Pending::Compute(_) => "y compute · n cancel · j k scroll",
        Pending::Setup => "y set up · n cancel · j k scroll",
        _ => "y write · n cancel · j k scroll",
    }
}

pub enum Pending {
    Write {
        path: PathBuf,
        sample: Box<Sample>,
        origin: document::Origin,
    },
    Tag(tagging::Plan),
    /// Quitting, the configuration's changes not written.
    Quit,
    /// A computation forcing values over the basket, confirmed first.
    Compute(Computing),
    /// The ticked setup steps.
    Setup,
    /// The configuration being edited, written.
    Configuration,
    /// A new sample, where it goes.
    New {
        path: PathBuf,
        sample: Box<Sample>,
    },
    /// Samples' files removed.
    Remove(Vec<PathBuf>),
    /// Declarations added to `.samplekitrc`, what they are said as.
    Declare {
        edit: Box<ConfigurationEdit>,
        said: String,
    },
    /// Files as they were before the last change: restored, or removed.
    Undo(Vec<Undone>),
    /// A declared export, its text ready.
    Export {
        text: String,
        path: PathBuf,
        rows: usize,
        /// The export's name, and the samples it is made from, for the
        /// history's record of the file.
        name: String,
        samples: Vec<PathBuf>,
    },
}

/// A computation asked for: these samples, these values of them (every value
/// not current when empty), overrides given back or kept.
#[derive(Debug, Clone)]
pub struct Computing {
    pub paths: Vec<PathBuf>,
    pub names: Vec<String>,
    pub force: bool,
    /// Current values run again too: a value chosen by hand is run whatever
    /// its state, which the screen shows beside it.
    pub rerun: bool,
}

/// A computation running beside the screen: what it says as it goes.
pub struct Running {
    pub receiver: mpsc::Receiver<Progress>,
    /// The files a computation may write, as they were: what it did write is
    /// one change for `u` once it has finished.
    pub before: Option<Vec<(PathBuf, Option<String>)>>,
}

pub enum Progress {
    Said(String),
    Done(String),
    /// Where a computation is: values done of those planned, and the value
    /// under way, for the title bar rather than the message.
    At {
        done: usize,
        total: usize,
        now: String,
    },
}

/// A computation as the title bar shows it: values done of those planned and
/// the one under way, then its outcome, which stays until the next run — a key
/// pressed meanwhile, a sample opened, no longer takes it away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStatus {
    pub done: usize,
    pub total: usize,
    pub now: String,
    /// What the run came to, once it has.
    pub outcome: Option<String>,
}

/// A row of the collection's table, rendered once per change rather than at
/// every frame: a held key redrew the whole collection at each step.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub name: String,
    pub mark: &'static str,
    pub cells: Vec<String>,
}

/// One line of the sample screen: a field, as it is shown, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub field: String,
    pub shown: String,
    pub state: String,
    /// A property's value may be changed and explained; an attribute
    /// changed; a table only explained.
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Quantity,
    Attribute,
    Table,
}

/// The first item of `g`'s picker when the collection is grouped: no field
/// is named with a space, so it cannot be one.
pub const NO_GROUPING: &str = "no grouping";

/// A column as it is remembered: what a profile's column says of itself.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RememberedColumn {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// One specifier, or the value's and the uncertainty's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precision: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

impl RememberedColumn {
    fn of(column: &ColumnSpec) -> RememberedColumn {
        RememberedColumn {
            field: column.field.clone(),
            label: column.label.clone(),
            header: column.header.clone(),
            precision: match &column.precision {
                None => Vec::new(),
                Some(PrecisionSchema::Both(both)) => vec![both.clone()],
                Some(PrecisionSchema::Split(value, uncertainty)) => {
                    vec![value.clone(), uncertainty.clone()]
                }
            },
            template: column.template.clone(),
        }
    }

    fn spec(&self) -> ColumnSpec {
        ColumnSpec {
            field: self.field.clone(),
            label: self.label.clone(),
            header: self.header.clone(),
            precision: match self.precision.as_slice() {
                [both] => Some(PrecisionSchema::Both(both.clone())),
                [value, uncertainty] => {
                    Some(PrecisionSchema::Split(value.clone(), uncertainty.clone()))
                }
                _ => None,
            },
            template: self.template.clone(),
        }
    }
}

/// What is saved per directory, and read back when the workbench opens it
/// again: never in `.samplekitrc`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Remembered {
    pub directory: String,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub sort: Vec<String>,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub columns: Vec<String>,
    /// The columns whole, as a profile applied made them: the fields alone
    /// came back under their own names, and a profile's labels were lost on
    /// reopening. `columns` is still read from a state written before.
    #[serde(default)]
    pub shown: Vec<RememberedColumn>,
    /// The fields the collection is grouped by, in order.
    #[serde(default)]
    pub group: Vec<String>,
    /// The basket, by paths relative to the directory.
    #[serde(default)]
    pub basket: Vec<String>,
    /// The values selected, by field.
    #[serde(default)]
    pub selected: Vec<String>,
    /// Whether a sample's note is hidden, the values taking the width.
    #[serde(default)]
    pub note_hidden: bool,
}

pub struct Workbench {
    pub root: PathBuf,
    pub collection: SampleList,
    /// The collection filtered, sorted, and narrowed to the basket when asked.
    pub view: SampleList,
    pub profile: Profile,
    pub filter: String,
    pub sort: Vec<String>,
    pub reverse: bool,
    /// The fields the collection is grouped by, in order; none, ungrouped.
    pub group: Vec<String>,
    pub cursor: usize,
    pub basket: BTreeSet<PathBuf>,
    pub basket_only: bool,
    pub screen: Screen,
    pub mode: Mode,
    /// What the last action said.
    pub message: String,
    /// Rows a page holds, set by whoever draws.
    pub page: usize,
    /// The view's rows as the table shows them, and its header.
    pub rendered: Vec<Rendered>,
    pub header: Vec<String>,
    pub running: Option<Running>,
    /// What the projects a computation reaches held before it, for its one
    /// snapshot once it has finished.
    pub computing_history: Option<version_control::Writing>,
    /// The last computation, under way or finished, as the status line says it.
    pub run: Option<RunStatus>,
    /// A figure's window open beside the screen, what it says when closed.
    pub drawing: Option<Running>,
    /// Each change written, as the files it touched were before it: the last
    /// one is what `u` offers back.
    pub undo: Vec<Vec<Undone>>,
    /// The control screen's lines, built when it opens.
    pub control: Vec<ControlItem>,
    /// The history the history screen shows, each snapshot with what it
    /// changed.
    pub history: Vec<HistoryRow>,
    /// How far a confirmation's lines are scrolled.
    pub confirm_scroll: usize,
    /// The last change's undo was refused, its file changed since: asked
    /// again, it is let go.
    pub undo_refused: bool,
    /// The collection shown as its columns summarised, rather than a sample
    /// a row.
    pub summarised: bool,
    /// The first data column shown, where the columns are wider than the
    /// screen.
    pub hscroll: usize,
    /// Where each list and table was scrolled to when last drawn, by name:
    /// kept from frame to frame, the cursor moves inside what is shown, and
    /// the rows move only when it leaves them. Rebuilt each frame from the
    /// top, the selected row stuck to the bottom of a long list.
    pub offsets: std::cell::RefCell<std::collections::HashMap<&'static str, usize>>,
    /// The project's setup, its questions and the answers so far.
    pub setup: Option<Setup>,
    /// Opened on its configuration from the start page: leaving the
    /// configuration goes back there.
    pub back_to_start: bool,
    /// Opened from the start page, where `q` goes back: its help and hint say
    /// so. Set by whoever opens it there.
    pub from_start: bool,
    /// Quit while something was under way beside the screen: the workbench
    /// ends once it has, its snapshot taken — `left` says when.
    pub leaving: bool,
    /// Ctrl+C was pressed while something ran, and asked as `q` asks: a second
    /// ends the workbench at once, and however it ends, it ends as an
    /// interrupt.
    pub interrupted: bool,
    /// The folder the start page's `+` made for this project: `Esc` on the
    /// setup's first question goes back to the page, removing it while it is
    /// still empty.
    pub made: Option<PathBuf>,
    /// Every field of the collection, described: read once per load for the
    /// filter's completion and the pickers, rather than at every key.
    pub described: std::cell::OnceCell<Vec<String>>,
    /// The environment being made beside the screen, what it says when done.
    pub installing: Option<mpsc::Receiver<Result<String, String>>>,
    /// The configuration being edited, its changes kept until written.
    pub workspace: Option<Workspace>,
    /// Values selected for `c`, by field: kept from one sample to the next,
    /// so that the same values are computed over the basket.
    pub selected: BTreeSet<String>,
    /// The figure of one's own being set up, kept from one `p` to the next.
    pub figure: Option<FigureForm>,
    /// Where the note is drawn while it is edited — its inside, and the line
    /// and column shown first — set by whoever draws, for a click.
    pub note_view: Option<NoteView>,
    /// How far the note shown is scrolled, and whose it is: another sample's
    /// opens at its top.
    pub note_scroll: (Option<PathBuf>, usize),
    /// A sample's note hidden by `n`, the values taking the whole width;
    /// remembered.
    pub note_hidden: bool,
    /// Where the note is drawn, as the view last drew it: the wheel over it
    /// scrolls it.
    pub note_area: Option<(u16, u16, u16, u16)>,
    /// The project's colours.
    pub theme: super::theme::Theme,
    /// The control screen's line a sample was opened from, where `Back`
    /// returns rather than to the collection.
    pub from_control: Option<usize>,
    /// What the model owes, by sample: each value it would compute that no file
    /// records — never computed, or waiting for an input — as `samplekit
    /// status` lists them. Only a model can name them, planned beside the
    /// screen, nothing asked. A sample its model read owing nothing is held,
    /// empty.
    pub owed: std::collections::BTreeMap<PathBuf, Vec<Owed>>,
    /// Why the model was not read for `owed`, where it was not: what the
    /// control screen says it does not list.
    pub model_unread: Option<String>,
    /// The model's plan being read beside the screen.
    pub planning: Option<mpsc::Receiver<Planning>>,
}

/// A value the model would compute that no file records: its name, and
/// what `status` says of it — `never computed`, `uncertainty never
/// computed`, `waits for fg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owed {
    pub value: String,
    pub said: String,
}

impl Owed {
    /// Whether it waits for an input, rather than for a computation: `C`
    /// computes the one, and only an input given moves the other.
    pub fn waits(&self) -> bool {
        self.said.starts_with("waits")
    }

    /// As the TUI says it, one way wherever `∅` stands, and as `status` says
    /// it: `never computed`, or `waits for fg`.
    pub fn shown(&self) -> String {
        self.said.clone()
    }
}

/// The model's plan, as its worker gave it: each sample's values, and why
/// the samples it could not plan were not.
pub struct Planning {
    pub plans: Vec<(PathBuf, Vec<runtime::Planned>)>,
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoteView {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub top: usize,
    pub left: usize,
}

impl Workbench {
    /// The collection at `root`, as the last session left it there.
    pub fn open(root: &Path) -> Result<Workbench, String> {
        let collection = list::from_directory(root).map_err(|error| error.to_string())?;
        let remembered = remembered(root);
        let profile = match remembered.as_ref() {
            Some(state) if !state.shown.is_empty() => {
                profiles::anonymous(state.shown.iter().map(RememberedColumn::spec).collect())
            }
            Some(state) if !state.columns.is_empty() => profile_of(&state.columns),
            _ => default_profile(&collection),
        };
        let mut workbench = Workbench {
            root: root.to_path_buf(),
            view: collection.filter_by(|_| true),
            collection,
            profile,
            filter: String::new(),
            sort: Vec::new(),
            reverse: false,
            group: Vec::new(),
            cursor: 0,
            basket: BTreeSet::new(),
            basket_only: false,
            screen: Screen::Collection,
            mode: Mode::Normal,
            message: String::new(),
            page: 20,
            rendered: Vec::new(),
            header: Vec::new(),
            running: None,
            run: None,
            drawing: None,
            undo: Vec::new(),
            control: Vec::new(),
            history: Vec::new(),
            confirm_scroll: 0,
            undo_refused: false,
            summarised: false,
            hscroll: 0,
            offsets: Default::default(),
            computing_history: None,
            setup: None,
            installing: None,
            back_to_start: false,
            from_start: false,
            leaving: false,
            interrupted: false,
            made: None,
            described: std::cell::OnceCell::new(),
            workspace: None,
            selected: BTreeSet::new(),
            figure: None,
            note_view: None,
            note_scroll: (None, 0),
            note_hidden: false,
            note_area: None,
            theme: super::theme::Theme::default(),
            from_control: None,
            owed: Default::default(),
            model_unread: None,
            planning: None,
        };
        workbench.theme = super::theme::Theme::of(workbench.collection.config());
        if let Some(state) = remembered {
            workbench.filter = state.filter;
            workbench.sort = state.sort;
            workbench.reverse = state.reverse;
            workbench.group = state.group;
            // A sample moved or removed since leaves the basket quietly.
            workbench.basket = state
                .basket
                .iter()
                .map(|relative| root.join(relative))
                .filter(|path| path.is_file())
                .collect();
            workbench.selected = state.selected.into_iter().collect();
            workbench.note_hidden = state.note_hidden;
        }
        // A configuration that does not load is said, not passed over: the
        // collection opens without it.
        if workbench.collection.config().is_none()
            && let Err(error) = crate::config::project_config::load_for(root)
        {
            workbench.message = format!("the configuration is not read: {error}");
        }
        // A directory with nothing of a project in it opens on its setup.
        if workbench.collection.is_empty() && workbench.collection.config().is_none() {
            workbench.open_setup();
        }
        if let Err(error) = workbench.refresh() {
            // A remembered filter or sort the collection no longer answers is
            // dropped, and said, rather than opening on nothing.
            if error.starts_with("the sort was set aside")
                || error.starts_with("the grouping was set aside")
            {
                workbench.message = format!("remembered: {error}");
            } else {
                workbench.message = format!("the remembered filter was set aside: {error}");
                workbench.filter.clear();
            }
            workbench.refresh().ok();
        }
        // On the first row shown: the refresh followed the sample the
        // unsorted list began with, which projects set apart moved down.
        workbench.cursor = 0;
        workbench.plan_owed();
        Ok(workbench)
    }

    /// The order the groups come in: the sort's where one is chosen — each
    /// group where its first sample comes — else their values'.
    pub fn group_order(&self) -> list::GroupOrder {
        if self.sort.is_empty() {
            list::GroupOrder::Values
        } else {
            list::GroupOrder::Listed
        }
    }

    /// What is saved for this directory.
    pub fn remembered(&self) -> Remembered {
        Remembered {
            // Whole, so that the start page can list it.
            directory: dunce::canonicalize(&self.root)
                .unwrap_or_else(|_| self.root.clone())
                .display()
                .to_string(),
            filter: self.filter.clone(),
            sort: self.sort.clone(),
            reverse: self.reverse,
            group: self.group.clone(),
            columns: self
                .profile
                .columns()
                .iter()
                .map(|column| column.field.clone())
                .collect(),
            shown: self
                .profile
                .columns()
                .iter()
                .map(RememberedColumn::of)
                .collect(),
            basket: self
                .basket
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.root)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect(),
            selected: self.selected.iter().cloned().collect(),
            note_hidden: self.note_hidden,
        }
    }

    /// The filter, the sort and the basket applied again.
    pub fn refresh(&mut self) -> Result<(), String> {
        // Where things are, by file: a view re-filtered or re-sorted moves
        // them, and a position alone would then name another sample.
        let path_at =
            |view: &SampleList, at: usize| view.get(at).and_then(|entry| entry.path.clone());
        let open = match self.screen {
            Screen::Sample { at, .. } | Screen::Table { at, .. } => path_at(&self.view, at),
            _ => None,
        };
        let under = path_at(&self.view, self.cursor);
        // A filter that no longer reads, or a sort that no longer resolves,
        // leaves everything shown, unsorted, and says so — never an empty
        // screen that still counts every sample.
        let mut refused = None;
        // `state` is read only for a filter or a column naming it; the view
        // then carries each sample's states.
        let stated = self.reads_states().then(|| self.with_states());
        let base = stated.as_ref().unwrap_or(&self.collection);
        let mut view = if self.filter.trim().is_empty() {
            base.filter_by(|_| true)
        } else {
            match filter::parse(&self.filter)
                .map_err(|error| {
                    filter::caret(&self.filter, &error).unwrap_or_else(|| error.to_string())
                })
                .and_then(|parsed| base.filter(&parsed).map_err(|error| error.to_string()))
            {
                Ok(view) => view,
                // Set aside, and said: shown beside everything, it named a
                // selection the screen no longer made.
                Err(error) => {
                    refused = Some(format!("the filter was set aside: {error}"));
                    self.filter.clear();
                    base.filter_by(|_| true)
                }
            }
        };
        if self.basket_only {
            let basket = self.basket.clone();
            view = narrowed(&view, |path| basket.contains(path));
        }
        // Several projects are shown one after the other, each sorted within
        // itself, as the command line gives one table each: the view's order is
        // what the cursor moves through, and the screen heads each project
        // where it begins.
        let projects = self.collection.spans_configurations();
        if !self.sort.is_empty() || projects {
            let mut keys: Vec<String> = self
                .sort
                .iter()
                .map(|key| {
                    if self.reverse {
                        key.strip_prefix('-')
                            .map_or_else(|| format!("-{key}"), str::to_string)
                    } else {
                        key.clone()
                    }
                })
                .collect();
            if projects {
                keys.insert(0, "project".to_string());
            }
            let sorted = ordering::parse_spec(&keys)
                .map_err(|error| error.to_string())
                .and_then(|spec| view.sort(&spec).map_err(|error| error.to_string()));
            if let Err(error) = sorted {
                self.sort.clear();
                self.reverse = false;
                refused.get_or_insert(format!("the sort was set aside: {error}"));
                // The projects still apart, the sort that failed aside.
                if projects && let Ok(spec) = ordering::parse_spec(&["project".to_string()]) {
                    let _ = view.sort(&spec);
                }
            }
        }
        // Grouped, each group's samples together, the sort chosen within each,
        // and each project's groups within it: `project` first keeps the
        // projects apart as the sort above did.
        if !self.group.is_empty() {
            let mut keys: Vec<String> = Vec::new();
            if projects {
                keys.push("project".to_string());
            }
            keys.extend(self.group.iter().cloned());
            let grouped = keys
                .iter()
                .map(|key| fields::parse(key).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()
                .and_then(|keys| {
                    view.grouped(&keys, self.group_order())
                        .map_err(|error| error.to_string())
                });
            match grouped {
                Ok(grouped) => view = grouped,
                Err(error) => {
                    self.group.clear();
                    refused.get_or_insert(format!("the grouping was set aside: {error}"));
                }
            }
        }
        // A remembered column naming what no sample holds any more is set
        // aside and said, as a sort is: kept, it was a column of dashes.
        if let Some(first) = self.collection.iter().next() {
            let vocabulary = self.collection.vocabulary();
            let sample = first.sample.borrow();
            let subject = Subject {
                sample: &sample,
                path: first.path.as_deref(),
                vocabulary: &vocabulary,
                states: first.states.as_deref(),
            };
            let unknown: Vec<String> = self
                .profile
                .columns
                .iter()
                .map(|column| column.field.clone())
                .filter(|column| {
                    fields::parse(column).map_or(true, |field| {
                        matches!(
                            fields::resolve(&field, &subject),
                            Err(fields::FieldError::UnknownProperty { .. }
                                | fields::FieldError::UnknownTable { .. }
                                | fields::FieldError::UnknownColumn { .. })
                        )
                    })
                })
                .collect();
            if !unknown.is_empty() {
                drop(sample);
                self.profile
                    .columns
                    .retain(|column| !unknown.contains(&column.field));
                refused.get_or_insert(format!(
                    "{} set aside: no sample holds {}",
                    if unknown.len() == 1 {
                        "a column was"
                    } else {
                        "columns were"
                    },
                    unknown.join(", ")
                ));
            }
        }
        self.view = view;
        let position = |view: &SampleList, path: &Path| {
            view.iter()
                .position(|entry| entry.path.as_deref() == Some(path))
        };
        if let Some(at) = under.as_deref().and_then(|path| position(&self.view, path)) {
            self.cursor = at;
        }
        self.cursor = self.cursor.min(self.view.len().saturating_sub(1));
        if let Some(path) = open {
            match position(&self.view, &path) {
                Some(found) => match &mut self.screen {
                    Screen::Sample { at, .. } | Screen::Table { at, .. } => *at = found,
                    _ => {}
                },
                None => {
                    self.screen = Screen::Collection;
                    self.from_control = None;
                    // A note being written for it ends too, unwritten: typed on
                    // behind the collection, a reflexive y wrote it.
                    let noting = matches!(self.mode, Mode::Note { .. });
                    if noting {
                        self.mode = Mode::Normal;
                    }
                    self.message = format!(
                        "{} is no longer among those shown{}",
                        path.file_stem().unwrap_or_default().to_string_lossy(),
                        if noting {
                            ": its note was not written"
                        } else {
                            ""
                        }
                    );
                }
            }
        }
        self.hscroll = self.hscroll.min(self.header.len().saturating_sub(1));
        // A line gone — an attribute cleared — leaves the cursor on the last.
        if let Screen::Sample { .. } = self.screen {
            let last = self.entries().len().saturating_sub(1);
            if let Screen::Sample { cursor, .. } = &mut self.screen {
                *cursor = (*cursor).min(last);
            }
        }
        self.render();
        match refused {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// The table's rows rendered again, after anything that changes them.
    pub fn render(&mut self) {
        let vocabulary = self.collection.vocabulary();
        let guards: Vec<_> = self
            .view
            .iter()
            .map(|entry| {
                (
                    entry.path.clone(),
                    entry.sample.borrow(),
                    entry.states.clone(),
                )
            })
            .collect();
        // A column of `state` shows the words the view was read with.
        let subjects: Vec<Subject> = guards
            .iter()
            .map(|(path, sample, states)| Subject {
                sample,
                path: path.as_deref(),
                vocabulary: &vocabulary,
                states: states.as_deref(),
            })
            .collect();
        self.header = render::columns(&self.profile, &subjects, self.collection.config(), None);
        self.rendered = subjects
            .iter()
            .map(|subject| {
                let config = subject
                    .path
                    .and_then(|path| self.collection.config_for(path));
                Rendered {
                    // Whose each row is, as a terminal table says it.
                    name: subject
                        .sample
                        .name()
                        .map(str::to_string)
                        .or_else(|| {
                            subject
                                .path
                                .and_then(|path| path.file_stem())
                                .map(|stem| stem.to_string_lossy().into_owned())
                        })
                        .unwrap_or_default(),
                    mark: {
                        // What the model owes is marked where the file says
                        // nothing worse: `status` lists it, and the collection
                        // showed the sample as current.
                        let mark = Workbench::mark(subject.sample);
                        let owes = subject
                            .path
                            .is_some_and(|path| !self.owed_by(path).is_empty());
                        if owes && matches!(mark, " " | "✎") {
                            "∅"
                        } else {
                            mark
                        }
                    },
                    cells: render::row(&self.profile, subject, config, None),
                }
            })
            .collect();
    }

    /// What a running computation said since the last frame; true when it
    /// said anything.
    pub fn tick(&mut self) -> bool {
        let mut said = false;
        if let Some(installing) = &self.installing {
            match installing.try_recv() {
                Ok(Ok(text) | Err(text)) => {
                    said = true;
                    self.message = text;
                    self.installing = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    said = true;
                    self.message = "the environment's thread ended without a word".to_string();
                    self.installing = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(drawing) = &self.drawing {
            match drawing.receiver.try_recv() {
                Ok(Progress::Done(text) | Progress::Said(text)) => {
                    said = true;
                    self.message = text;
                    self.drawing = None;
                }
                // A drawing thread gone without a word is over, not open.
                Err(mpsc::TryRecvError::Disconnected) => {
                    said = true;
                    self.message = "the figure's thread ended without a word".to_string();
                    self.drawing = None;
                }
                Ok(Progress::At { .. }) | Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(planning) = &self.planning {
            match planning.try_recv() {
                Ok(planned) => {
                    said = true;
                    self.planning = None;
                    self.take_plan(planned);
                }
                Err(mpsc::TryRecvError::Disconnected) => self.planning = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(running) = &self.running else {
            return said;
        };
        let mut done = None;
        loop {
            match running.receiver.try_recv() {
                Ok(Progress::Said(text)) => {
                    said = true;
                    self.message = text;
                }
                Ok(Progress::At { done, total, now }) => {
                    said = true;
                    let run = self.run.get_or_insert_with(RunStatus::default);
                    (run.done, run.total, run.now) = (done, total, now);
                }
                Ok(Progress::Done(text)) => {
                    said = true;
                    done = Some(text);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    said = true;
                    done = Some("the computation ended without a word".to_string());
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if let Some(text) = done {
            if let Some(before) = self.running.take().and_then(|running| running.before) {
                let changed = settled(before);
                if !changed.is_empty() {
                    self.undo.push(changed);
                }
            }
            let warning = self.computing_history.take().and_then(|writing| {
                let said = format!("tui · compute: {text}");
                history_warning(version_control::after_writing(writing, &said))
            });
            self.message.clear();
            self.reload();
            if let Some(warning) = warning {
                self.message = warning;
            }
            // The outcome stays in the status line until the next run; what
            // the reload said — a sample no longer shown — is the message.
            let run = self.run.get_or_insert_with(RunStatus::default);
            run.done = run.total;
            run.now.clear();
            run.outcome = Some(text);
        }
        said
    }

    /// The places a change reaches: the workbench's folder, and the files it
    /// writes — whose projects keep the history.
    fn places(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        std::iter::once(self.root.clone())
            .chain(paths.iter().cloned())
            .collect()
    }

    /// The last change the project's history can take back, as an undo's
    /// files, and what that change was, said.
    fn undoable_from_history(&self) -> Option<(Vec<Undone>, String)> {
        let config = version_control::projects_of(std::slice::from_ref(&self.root))
            .into_iter()
            .next()?;
        let undoable = version_control::last_undoable(&config).ok()??;
        let text = |bytes: Option<Vec<u8>>| {
            bytes.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        };
        let group = undoable
            .files
            .into_iter()
            .map(|file| Undone {
                path: file.path,
                before: text(file.before),
                after: text(file.after),
            })
            .collect();
        let when =
            chrono::DateTime::from_timestamp(undoable.seconds + i64::from(undoable.offset), 0)
                .map(|time| time.naive_utc().format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
        Some((
            group,
            format!("kept in the history, {when}: {}", undoable.message),
        ))
    }

    /// The history screen, of the project or of the sample at `sample` in the
    /// view: each snapshot that changed it, newest first, with what it changed.
    fn open_history(&mut self, sample: Option<usize>) {
        use crate::presentation::changes;
        let file = sample.and_then(|at| self.view.get(at)?.path.clone());
        let places = vec![file.clone().unwrap_or_else(|| self.root.clone())];
        let Some(config) = version_control::projects_of(&places).into_iter().next() else {
            self.message = "no .samplekitrc here keeps a history".to_string();
            return;
        };
        let entries = match version_control::entries(&config) {
            Ok(entries) => entries,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        if entries.is_empty() {
            self.message =
                "no history is kept here yet: it begins with the first change SampleKit writes"
                    .to_string();
            return;
        }
        let root = dunce::canonicalize(config.root()).unwrap_or_default();
        let named = file.as_ref().and_then(|file| {
            let file = dunce::canonicalize(file).ok()?;
            let relative = file.strip_prefix(&root).ok()?;
            Some(
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/"),
            )
        });
        let holds = |name: &str| named.as_deref().is_none_or(|named| named == name);
        let machines = changes::machines_of(&entries).len() > 1;
        let mut rows = Vec::new();
        for (at, entry) in entries.iter().enumerate() {
            let changed: Vec<&String> = entry.changed.iter().filter(|name| holds(name)).collect();
            if changed.is_empty() {
                continue;
            }
            let mut lines = changes::of_snapshot(&config, &entries, at, holds)
                .unwrap_or_else(|error| vec![error.to_string()]);
            // The script that made it, kept beside it, said after the change.
            if let Ok(Some((name, source))) = version_control::script_of(&config, &entry.id) {
                lines.push(String::new());
                lines.push(format!("the script that made it: {name}"));
                lines.extend(
                    String::from_utf8_lossy(&source)
                        .lines()
                        .map(|line| format!("  {line}")),
                );
            }
            rows.push(HistoryRow {
                number: at + 1,
                when: changes::when(entry),
                message: entry.message.clone(),
                changed: changes::changed_said(&changed),
                lines,
                machine: machines.then(|| changes::machine_said(entry)),
            });
        }
        if rows.is_empty() {
            self.message = "no change kept in the history touched this sample".to_string();
            return;
        }
        self.history = rows;
        let back = std::mem::replace(&mut self.screen, Screen::Collection);
        self.screen = Screen::History {
            cursor: 0,
            scroll: 0,
            sample,
            back: Box::new(back),
        };
    }

    /// The collection read again from its files: after a write, or an editor.
    pub fn reload(&mut self) {
        match list::from_directory(&self.root) {
            Ok(collection) => {
                self.collection = collection;
                // Its fields read again when next asked: a write, an editor,
                // a computation may have added one.
                self.described = std::cell::OnceCell::new();
                // A sample gone — undone, removed in an editor — leaves the
                // basket, which would otherwise refuse every action on it.
                self.basket.retain(|path| path.is_file());
                // Its colours with it: .samplekitrc may be what was edited.
                self.theme = super::theme::Theme::of(self.collection.config());
                // What the model owes, planned again: a value written may
                // be one it owed, or make another wait.
                self.plan_owed();
                // The control screen says the state as it is now.
                if let Screen::Control { cursor } = self.screen {
                    self.open_control();
                    let last = self.control.len().saturating_sub(1);
                    self.screen = Screen::Control {
                        cursor: cursor.min(last),
                    };
                }
                if let Err(error) = self.refresh() {
                    self.message = error;
                }
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    /// What the model owes, planned beside the screen as `samplekit status`
    /// plans it: from the model's description, nothing asked, no formula run,
    /// and no Python started while the description is current — a worker
    /// writing it again where it is not. Where the model cannot be read, why:
    /// the control screen says what it does not list. Until the plan is in, the
    /// last one stands.
    fn plan_owed(&mut self) {
        self.planning = None;
        self.model_unread = None;
        let Ok(groups) = computation::groups_of(&self.collection, None) else {
            return;
        };
        let mut jobs = Vec::new();
        let mut unread: Vec<String> = Vec::new();
        for group in &groups {
            let Some(config) = group.config.as_ref() else {
                continue;
            };
            let paths: Vec<PathBuf> = group
                .entries
                .iter()
                .filter_map(|at| self.collection.get(*at)?.path.clone())
                .collect();
            let from = paths.first().cloned().unwrap_or_else(|| self.root.clone());
            let reason = match runtime::availability(Some(config), &from) {
                Ok(runtime::Availability::NoTemplate) => continue,
                Ok(runtime::Availability::Ready { .. }) => {
                    jobs.push((config.clone(), from, paths));
                    continue;
                }
                Ok(runtime::Availability::Unavailable { reason }) => reason,
                Err(error) => error.to_string(),
            };
            // Its first line: a reason may go on to say where to fix it.
            let reason = reason.lines().next().unwrap_or_default().to_string();
            if !unread.contains(&reason) {
                unread.push(reason);
            }
        }
        if !unread.is_empty() {
            self.model_unread = Some(unread.join("; "));
        }
        if jobs.is_empty() {
            if !self.owed.is_empty() {
                self.owed.clear();
                self.render();
            }
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut plans = Vec::new();
            let mut unread = Vec::new();
            // What each formula was when a sample was last computed.
            let computed = runtime::ComputedStore::user();
            for (config, from, paths) in jobs {
                let description = match runtime::describe(&config, &from) {
                    Ok(description) => description,
                    Err(error) => {
                        unread.push(error.to_string());
                        continue;
                    }
                };
                for path in paths {
                    // Forced, as `status` plans it, so that a value held for
                    // want of its record is told from an override.
                    let recorded = computed
                        .as_ref()
                        .map(|store| store.recorded_formulas(&path))
                        .unwrap_or_default();
                    match description.plan(&path, &recorded) {
                        Ok(plan) => plans.push((path, plan)),
                        Err(error) => unread.push(format!(
                            "{}: {}",
                            path.file_stem().unwrap_or_default().to_string_lossy(),
                            error.to_string().lines().next().unwrap_or_default()
                        )),
                    }
                }
            }
            let _ = sender.send(Planning { plans, unread });
        });
        self.planning = Some(receiver);
    }

    /// The model's plan taken in: each value it would compute that no file
    /// records — never computed, or waiting — said as `status` says it, a
    /// value the file records as failed or outdated left to the file's own
    /// state. The marks, the sample screen and the control screen read it.
    pub fn take_plan(&mut self, planning: Planning) {
        let mut owed = std::collections::BTreeMap::new();
        for (path, plan) in planning.plans {
            let Some(entry) = self
                .collection
                .iter()
                .find(|entry| entry.path.as_deref() == Some(path.as_path()))
            else {
                continue;
            };
            let sample = entry.sample.borrow();
            let recorded: Vec<String> = editing::not_current(&sample)
                .into_iter()
                .map(|value| value.name)
                .collect();
            let values: Vec<Owed> = plan
                .into_iter()
                .filter(|planned| {
                    matches!(
                        planned.reason,
                        runtime::Reason::NeverComputed
                            | runtime::Reason::Waiting
                            | runtime::Reason::FormulaChanged
                    ) && !recorded.contains(&planned.value)
                })
                .map(|planned| {
                    // An entered value whose uncertainty a formula owes.
                    let entered = Identifier::new(&planned.value).ok().is_some_and(|name| {
                        sample.property(&name).is_ok_and(|handle| {
                            handle
                                .peek(crate::core::property::Property::peek_value)
                                .is_some_and(|value| !value.is_absent())
                        })
                    });
                    let said = if matches!(
                        planned.reason,
                        runtime::Reason::Waiting | runtime::Reason::FormulaChanged
                    ) {
                        planned.said
                    } else if entered {
                        "uncertainty never computed".to_string()
                    } else {
                        "never computed".to_string()
                    };
                    Owed {
                        value: planned.value,
                        said,
                    }
                })
                .collect();
            // Kept empty too: the sample was read by its model, which owes
            // it nothing.
            owed.insert(path, values);
        }
        if !planning.unread.is_empty() {
            self.model_unread = Some(planning.unread.join("; "));
        }
        self.owed = owed;
        // A filter or a column asking `state` is asked again of what the
        // plan says.
        if self.reads_states() {
            self.refresh().ok();
        }
        self.render();
        if let Screen::Control { cursor } = self.screen {
            self.open_control();
            let last = self.control.len().saturating_sub(1);
            self.screen = Screen::Control {
                cursor: cursor.min(last),
            };
        }
    }

    /// Whether the filter or a column names `state`, which is then read.
    fn reads_states(&self) -> bool {
        self.profile
            .columns()
            .iter()
            .any(|column| column.field == "state")
            || filter::parse(&self.filter)
                .is_ok_and(|parsed| !filter::state_words(&parsed).is_empty())
    }

    /// The collection, each sample carrying its states: what its files say, and
    /// what the model's plan owes it once the plan is in — a sample the plan
    /// has not reached answers from its files, a word only the model could deny
    /// left unknown.
    fn with_states(&self) -> SampleList {
        let nothing_owed: Vec<Owed> = Vec::new();
        let mut states = crate::collection::validation::states(&self.collection);
        for (entry, states) in self.collection.iter().zip(states.iter_mut()) {
            let Some(path) = entry.path.as_ref() else {
                continue;
            };
            // A project declaring no model owes nothing, and is read so.
            let no_model = self
                .config_for(path)
                .is_none_or(|config| runtime::template_of(config).is_none());
            let Some(owed) = self.owed.get(path).or(no_model.then_some(&nothing_owed)) else {
                continue;
            };
            let mut held = states.held().to_vec();
            held.extend(owed.iter().map(|value| {
                if value.said.starts_with("waits for") {
                    fields::State::Waiting
                } else if value.said.contains("never computed") {
                    fields::State::NeverComputed
                } else {
                    // A formula changed since it computed the value.
                    fields::State::Stale
                }
            }));
            *states = fields::States::new(held, true);
        }
        self.collection.with_states(states)
    }

    /// What the model owes the sample at `path`.
    fn owed_by(&self, path: &Path) -> &[Owed] {
        self.owed.get(path).map_or(&[], Vec::as_slice)
    }

    pub fn place(&self) -> Place {
        match self.screen {
            Screen::Collection => Place::Collection,
            Screen::Sample { .. } => Place::Sample,
            Screen::Table { .. } => Place::Table,
            Screen::Control { .. } => Place::Control,
            Screen::History { .. } => Place::History,
            Screen::Setup { .. } => Place::Setup,
            Screen::Configure => Place::Configure,
        }
    }

    /// The sample under the cursor, with its file.
    pub fn current(&self) -> Option<(PathBuf, std::cell::Ref<'_, Sample>)> {
        let at = match self.screen {
            Screen::Collection => self.cursor,
            Screen::Sample { at, .. } | Screen::Table { at, .. } => at,
            Screen::Control { .. }
            | Screen::History { .. }
            | Screen::Setup { .. }
            | Screen::Configure => return None,
        };
        let entry = self.view.get(at)?;
        Some((entry.path.clone()?, entry.sample.borrow()))
    }

    fn config_for(&self, path: &Path) -> Option<&ProjectConfig> {
        self.collection.config_for(path)
    }

    /// A key, in whatever mode the workbench is in.
    pub fn key(&mut self, key: Key) -> Effect {
        // Held with Ctrl or Alt, a key means something only to what is typed:
        // Ctrl+Y is not y, nor Alt+U u. Shift with a key that moves is that
        // key, as it was; the mouse dragged is nothing.
        let key = if self.mode.typing() {
            key
        } else {
            match plain(key) {
                Some(key) => key,
                None => return Effect::None,
            }
        };
        // Leaving, waiting for what is under way: a key pressed stays, and
        // does nothing else — the wheel and a click aside.
        if self.leaving && !matches!(key, Key::Wheel { .. } | Key::Click(..)) {
            self.leaving = false;
            self.interrupted = false;
            self.message = "you stay: q asks again".to_string();
            return Effect::None;
        }
        // The wheel: over the note shown, it scrolls it; anywhere else, it is
        // an arrow.
        let key = match key {
            Key::Wheel { down, x, y } => {
                let over_note = self.note_area.is_some_and(|(left, top, width, height)| {
                    (left..left + width).contains(&x) && (top..top + height).contains(&y)
                });
                if over_note
                    && matches!(self.mode, Mode::Normal)
                    && matches!(self.screen, Screen::Sample { .. })
                {
                    self.scroll_note(if down { 3 } else { -3 });
                    return Effect::None;
                }
                // The note being written scrolls under it, as an editor does.
                if matches!(self.mode, Mode::Note { .. }) {
                    key
                } else if down {
                    Key::Down
                } else {
                    Key::Up
                }
            }
            key => key,
        };
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Normal => self.normal(key),
            Mode::Filter {
                text,
                candidates,
                chosen,
                error,
            } => self.filtering(key, text, candidates, chosen, error),
            Mode::Prompt { purpose, text } => self.prompting(key, purpose, text),
            Mode::Figure => self.figuring(key),
            Mode::Help { scroll } => self.helping(key, scroll),
            Mode::Picker {
                purpose,
                items,
                cursor,
                query,
                typing,
            } => self.picking(key, purpose, items, cursor, query, typing),
            Mode::Confirm {
                title,
                lines,
                pending,
            } => self.confirming(key, title, lines, *pending),
            Mode::Panel {
                title,
                lines,
                scroll,
            } => {
                match key {
                    Key::Char('j') | Key::Down => {
                        self.mode = Mode::Panel {
                            title,
                            scroll: (scroll + 1).min(lines.len().saturating_sub(1)),
                            lines,
                        }
                    }
                    Key::Char('k') | Key::Up => {
                        self.mode = Mode::Panel {
                            title,
                            lines,
                            scroll: scroll.saturating_sub(1),
                        }
                    }
                    _ => {}
                }
                Effect::None
            }
            Mode::Files { files, cursor } => self.choosing_file(key, files, cursor),
            Mode::Note {
                path,
                text,
                original,
            } => self.noting(key, path, text, original),
            Mode::Quantity {
                path,
                field,
                slots,
                on,
            } => self.quantity(key, path, field, slots, on),
        }
    }

    fn normal(&mut self, key: Key) -> Effect {
        let place = self.place();
        let Some(action) = BINDINGS
            .iter()
            .find(|(at, bound, _, _)| *at == place && *bound == key)
            .map(|(_, _, action, _)| *action)
        else {
            return Effect::None;
        };
        self.message.clear();
        let last_entry = match &self.screen {
            Screen::Sample { .. } => self.entries().len().saturating_sub(1),
            Screen::Table { name, .. } => self
                .current()
                .and_then(|(_, sample)| sample.table(name).ok().map(|table| table.rows().count()))
                .unwrap_or(0)
                .saturating_sub(1),
            Screen::Collection => 0,
            Screen::Control { .. } => self.control.len().saturating_sub(1),
            Screen::History { .. } => self.history.len().saturating_sub(1),
            Screen::Setup { .. } | Screen::Configure => 0,
        };
        let page = self.page;
        let last_column = match &self.screen {
            Screen::Table { name, .. } => self
                .current()
                .and_then(|(_, sample)| {
                    sample
                        .table(name)
                        .ok()
                        .map(|table| value_columns(table).len())
                })
                .unwrap_or(0)
                .saturating_sub(1),
            _ => 0,
        };
        match (&mut self.screen, action) {
            (_, Action::Quit) => return self.quit(),
            (_, Action::Help) => self.help(),
            // Summarised, the rows are columns: what acts on a sample waits
            // for the samples to be shown again.
            (Screen::Collection, action)
                if self.summarised
                    && !matches!(
                        action,
                        Action::Summary
                            | Action::Filter
                            | Action::ClearFilter
                            | Action::Columns
                            | Action::GoTo
                            | Action::Group
                            | Action::BasketOnly
                            | Action::Control
                            | Action::Configure
                            | Action::Produce
                            | Action::Undo
                    ) =>
            {
                self.message = "S shows the samples again".to_string();
            }
            (Screen::Setup { cursor }, action) => {
                let cursor = *cursor;
                return self.setup_key(action, cursor);
            }
            (Screen::Configure, action) => {
                if self.workspace_key(action) {
                    return self.quit();
                }
            }
            (Screen::Collection, Action::Configure) => {
                // Changes not written wait here; the file is read again only
                // when there are none.
                if self
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.dirty)
                {
                    self.screen = Screen::Configure;
                } else {
                    self.open_workspace();
                }
            }
            (Screen::Collection, Action::Down) => self.move_by(1),
            (Screen::Collection, Action::Up) => self.move_by(-1),
            (Screen::Collection, Action::PageDown) => self.move_by(self.page as isize),
            (Screen::Collection, Action::PageUp) => self.move_by(-(self.page as isize)),
            (Screen::Collection, Action::Top) => self.cursor = 0,
            (Screen::Collection, Action::GoTo) => self.mode = self.goto_mode(),
            (Screen::Collection, Action::Group) => self.mode = self.group_mode(),
            (Screen::Collection, Action::Produce) => self.produce_mode(),
            (Screen::Collection, Action::Declare) => self.declare_mode(),
            // Over a table: a figure of its columns, this sample's curve.
            (Screen::Table { name, .. }, Action::Produce) => {
                let name = name.clone();
                if let Some(path) = self.current().map(|(path, _)| path) {
                    self.open_figure(Some((path, name)));
                }
            }
            (Screen::Collection, Action::Control) => self.open_control(),
            (Screen::Collection, Action::History) => self.open_history(None),
            (Screen::Sample { at, .. } | Screen::Table { at, .. }, Action::History) => {
                let at = *at;
                self.open_history(Some(at));
            }
            (Screen::History { cursor, scroll, .. }, Action::Down) => {
                *cursor = (*cursor + 1).min(last_entry);
                *scroll = 0;
            }
            (Screen::History { cursor, scroll, .. }, Action::Up) => {
                *cursor = cursor.saturating_sub(1);
                *scroll = 0;
            }
            (Screen::History { cursor, scroll, .. }, Action::Top) => (*cursor, *scroll) = (0, 0),
            (Screen::History { cursor, scroll, .. }, Action::Bottom) => {
                (*cursor, *scroll) = (last_entry, 0)
            }
            (Screen::History { cursor, scroll, .. }, Action::PageDown) => {
                *cursor = (*cursor + page).min(last_entry);
                *scroll = 0;
            }
            (Screen::History { cursor, scroll, .. }, Action::PageUp) => {
                *cursor = cursor.saturating_sub(page);
                *scroll = 0;
            }
            (Screen::History { scroll, .. }, Action::NoteDown) => *scroll += 3,
            (Screen::History { scroll, .. }, Action::NoteUp) => *scroll = scroll.saturating_sub(3),
            (Screen::History { back, .. }, Action::Back) => {
                let back = std::mem::replace(back.as_mut(), Screen::Collection);
                self.screen = back;
            }
            (Screen::Collection, Action::Summary) => self.summarised = !self.summarised,
            (Screen::Collection, Action::ScrollLeft) => {
                self.hscroll = self.hscroll.saturating_sub(1)
            }
            (Screen::Collection, Action::ScrollRight) => {
                // The data columns, the names apart: those it can scroll to.
                let data = self
                    .profile
                    .columns()
                    .iter()
                    .filter(|column| column.field != "name")
                    .count();
                self.hscroll = (self.hscroll + 1).min(data.saturating_sub(1))
            }
            (Screen::Control { cursor }, Action::Down) => *cursor = (*cursor + 1).min(last_entry),
            (Screen::Control { cursor }, Action::Up) => *cursor = cursor.saturating_sub(1),
            (Screen::Control { cursor }, Action::Top) => *cursor = 0,
            (Screen::Control { cursor }, Action::Bottom) => *cursor = last_entry,
            (Screen::Control { cursor }, Action::PageDown) => {
                *cursor = (*cursor + page).min(last_entry)
            }
            (Screen::Control { cursor }, Action::PageUp) => *cursor = cursor.saturating_sub(page),
            (Screen::Control { cursor }, Action::Open) => {
                let at = *cursor;
                self.open_concern(at);
            }
            (Screen::Control { .. }, Action::Back) => self.screen = Screen::Collection,
            (Screen::Control { .. }, Action::ComputeSample) => {
                // Every sample with a value outdated, failed or never computed,
                // as one run; one only waiting for an input is left to it.
                let mut paths: Vec<PathBuf> = Vec::new();
                for item in &self.control {
                    let computes = match item.kind {
                        Concern::Stale | Concern::Failed => true,
                        Concern::Owed => item.path.as_ref().is_some_and(|path| {
                            self.owed_by(path).iter().any(|owed| !owed.waits())
                        }),
                        _ => false,
                    };
                    if computes
                        && let Some(path) = &item.path
                        && !paths.contains(path)
                    {
                        paths.push(path.clone());
                    }
                }
                if paths.is_empty() {
                    self.message = "nothing is outdated, failed or never computed".to_string();
                } else {
                    self.compute(Computing {
                        paths,
                        names: Vec::new(),
                        force: false,
                        rerun: false,
                    });
                }
            }
            (_, Action::Undo) => self.begin_undo(),
            // Shaped like the sample under the cursor; the first of a
            // collection, from nothing, into samples/ where there is one.
            (Screen::Collection, Action::NewSample) => {
                let like = self.current().map(|(path, _)| path);
                self.mode = Mode::prompt(Purpose::New { like }, String::new());
            }
            (Screen::Table { name, .. }, Action::AddRow) => {
                let table = name.to_string();
                if let Some(path) = self.current().map(|(path, _)| path) {
                    self.mode = Mode::prompt(Purpose::Row { path, table }, String::new());
                }
            }
            (Screen::Collection, Action::Bottom) => self.cursor = self.view.len().saturating_sub(1),
            (Screen::Collection, Action::Open) => {
                self.from_control = None;
                if self.view.get(self.cursor).is_some() {
                    self.screen = Screen::Sample {
                        at: self.cursor,
                        cursor: 0,
                    };
                }
            }
            (Screen::Collection, Action::Filter) => {
                self.mode = self.filter_mode(Typed::new(&self.filter));
            }
            (Screen::Collection, Action::ClearFilter) => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.refresh().ok();
                    self.message = "filter cleared".to_string();
                }
            }
            (Screen::Collection, Action::Sort) => {
                // The keys sorted by, first and in their order, `-` on one
                // reversed; then the columns shown; then every other field the
                // collection holds, as `-s` takes any.
                let mut items: Vec<(String, bool)> =
                    self.sort.iter().map(|key| (key.clone(), true)).collect();
                let shown = self
                    .profile
                    .columns()
                    .iter()
                    .map(|column| column.field.clone());
                let others = self.offered_fields(false);
                for field in shown.chain(others) {
                    let offered = items
                        .iter()
                        .any(|(key, _)| key.trim_start_matches('-') == field);
                    if !offered {
                        items.push((field, false));
                    }
                }
                self.mode = Mode::Picker {
                    purpose: Choosing::Sort,
                    items,
                    cursor: 0,
                    query: Typed::unfocused(""),
                    typing: false,
                };
            }
            (Screen::Collection, Action::Reverse) => {
                if self.sort.is_empty() {
                    self.message = "nothing is sorted to reverse: s sorts by a column".to_string();
                } else {
                    self.reverse = !self.reverse;
                    self.refresh().ok();
                }
            }
            (Screen::Collection, Action::Columns) => self.mode = self.columns_mode(),
            (Screen::Collection, Action::Basket) => {
                if let Some(path) = self
                    .view
                    .get(self.cursor)
                    .and_then(|entry| entry.path.clone())
                    && !self.basket.remove(&path)
                {
                    self.basket.insert(path);
                }
                self.move_by(1);
                // The basket alone shown: one taken out leaves it.
                if self.basket_only {
                    self.refresh().ok();
                }
            }
            (Screen::Collection, Action::BasketOnly) => {
                self.basket_only = !self.basket_only;
                self.refresh().ok();
            }
            (Screen::Collection, Action::TagBasket) => {
                if self.basket.is_empty() {
                    self.message = "the basket is empty: Space puts a sample in it".to_string();
                } else {
                    self.mode = Mode::prompt(Purpose::Tag, String::new());
                }
            }
            (Screen::Sample { cursor, .. }, Action::Down) => {
                *cursor = (*cursor + 1).min(last_entry);
            }
            (Screen::Sample { cursor, .. }, Action::Up) => *cursor = cursor.saturating_sub(1),
            (Screen::Sample { cursor, .. }, Action::Top) => *cursor = 0,
            (Screen::Sample { cursor, .. }, Action::Bottom) => *cursor = last_entry,
            (Screen::Sample { cursor, .. }, Action::PageDown) => {
                *cursor = (*cursor + page).min(last_entry)
            }
            (Screen::Sample { cursor, .. }, Action::PageUp) => {
                *cursor = cursor.saturating_sub(page)
            }
            // Back where it was opened from: the control screen's line, or
            // the collection.
            (Screen::Sample { .. }, Action::Back) => match self.from_control.take() {
                Some(line) => {
                    self.open_control();
                    let last = self.control.len().saturating_sub(1);
                    self.screen = Screen::Control {
                        cursor: line.min(last),
                    };
                }
                None => self.screen = Screen::Collection,
            },
            (Screen::Sample { at, cursor }, Action::Unfold) => {
                let (at, back) = (*at, *cursor);
                match self.entry() {
                    Some(entry) if entry.kind == EntryKind::Table => {
                        if let Ok(name) = Identifier::new(&entry.field) {
                            self.screen = Screen::Table {
                                at,
                                back,
                                name,
                                cursor: 0,
                                column: 0,
                            };
                        }
                    }
                    _ => self.message = "l unfolds a table; Enter explains a value".to_string(),
                }
            }
            (Screen::Table { cursor, .. }, Action::Down) => *cursor = (*cursor + 1).min(last_entry),
            (Screen::Table { cursor, .. }, Action::Up) => *cursor = cursor.saturating_sub(1),
            (Screen::Table { cursor, .. }, Action::Top) => *cursor = 0,
            (Screen::Table { cursor, .. }, Action::Bottom) => *cursor = last_entry,
            (Screen::Table { cursor, .. }, Action::PageDown) => {
                *cursor = (*cursor + page).min(last_entry)
            }
            (Screen::Table { cursor, .. }, Action::PageUp) => *cursor = cursor.saturating_sub(page),
            (Screen::Table { column, .. }, Action::NextColumn) => {
                *column = (*column + 1).min(last_column)
            }
            (Screen::Table { column, .. }, Action::PreviousColumn) if *column > 0 => *column -= 1,
            (Screen::Table { at, back, .. }, Action::Back | Action::PreviousColumn) => {
                self.screen = Screen::Sample {
                    at: *at,
                    cursor: *back,
                }
            }
            (Screen::Sample { .. }, Action::ComputeValue) => self.compute_chosen(false),
            (Screen::Table { .. }, Action::ComputeValue) => self.compute_columns(),
            (Screen::Table { .. }, Action::Select) => {
                if let Some((_, column)) = self.cell()
                    && !self.selected.remove(&column)
                {
                    self.selected.insert(column);
                }
            }
            (Screen::Table { .. }, Action::ClearSelection) => {
                self.selected.clear();
                self.message = "no value selected".to_string();
            }
            (Screen::Collection, Action::SelectAll) => {
                let shown: Vec<PathBuf> = self
                    .view
                    .iter()
                    .filter_map(|entry| entry.path.clone())
                    .collect();
                self.basket.extend(shown);
                self.message = format!(
                    "{} in the basket",
                    counted(self.basket.len(), "sample", "samples")
                );
            }
            (Screen::Collection, Action::ClearSelection) => {
                self.basket.clear();
                if self.basket_only {
                    self.basket_only = false;
                    self.refresh().ok();
                }
                self.message = "the basket is empty".to_string();
            }
            (Screen::Sample { cursor, .. }, Action::Select) => {
                let at = *cursor;
                *cursor = (*cursor + 1).min(last_entry);
                if let Some(entry) = self.entries().into_iter().nth(at) {
                    self.toggle_selected(entry);
                }
            }
            (Screen::Sample { .. }, Action::SelectAll) => {
                for entry in self.entries() {
                    if self.computable(&entry) {
                        self.selected.insert(entry.field);
                    }
                }
                self.message = format!(
                    "{} selected",
                    counted(self.selected.len(), "value", "values")
                );
            }
            (Screen::Sample { .. }, Action::ClearSelection) => {
                self.selected.clear();
                self.message = "no value selected".to_string();
            }
            (Screen::Sample { .. }, Action::EditNote) => self.begin_note(),
            (Screen::Sample { .. }, Action::ToggleNote) => {
                self.note_hidden = !self.note_hidden;
                self.message = if self.note_hidden {
                    "the note hidden: n shows it, N edits it".to_string()
                } else {
                    "the note shown".to_string()
                };
            }
            // Scrolling a note out of sight would do nothing, silently.
            (Screen::Sample { .. }, Action::NoteDown | Action::NoteUp) if self.note_hidden => {
                self.message = "the note is hidden: n shows it".to_string();
            }
            (Screen::Collection, Action::ComputeSample) if !self.selected.is_empty() => {
                self.compute_chosen(true)
            }
            (_, Action::ComputeSample) => {
                let paths: Vec<PathBuf> =
                    if matches!(self.screen, Screen::Collection) && !self.basket.is_empty() {
                        self.basket.iter().cloned().collect()
                    } else {
                        self.current().map(|(path, _)| path).into_iter().collect()
                    };
                self.compute(Computing {
                    paths,
                    names: Vec::new(),
                    force: false,
                    rerun: false,
                });
            }
            (Screen::Sample { .. } | Screen::Table { .. }, Action::Explain) => self.explain(),
            (Screen::Sample { .. } | Screen::Table { .. }, Action::Edit) => self.begin_edit(),
            (_, Action::Files) => self.files(),
            (_, Action::Editor) if self.running.is_some() => {
                self.message = "a computation is writing: edit once it has finished".to_string();
            }
            (_, Action::Editor) => {
                if let Some((path, _)) = self.current() {
                    return Effect::Editor(path);
                }
            }
            (Screen::Sample { .. }, Action::NoteDown) => self.scroll_note(3),
            (Screen::Sample { .. }, Action::NoteUp) => self.scroll_note(-3),
            (_, Action::RemoveSample) if self.running.is_some() => {
                self.message = "a computation is writing: remove once it has finished".to_string();
            }
            (Screen::Collection | Screen::Sample { .. }, Action::RemoveSample) => {
                self.remove_mode()
            }
            (_, Action::Model) if self.running.is_some() => {
                self.message =
                    "a computation is running the model: edit it once it has finished".to_string();
            }
            // The model the project declares, in the editor the user chose:
            // an editor of their own knows Python as none drawn here could.
            (_, Action::Model) => {
                match self
                    .collection
                    .config()
                    .and_then(runtime::template_of)
                    .map(|template| template.path().to_path_buf())
                {
                    Some(path) => return Effect::Editor(path),
                    None => {
                        self.message = "no model is declared here: [model] path in P".to_string()
                    }
                }
            }
            _ => {}
        }
        Effect::None
    }

    /// What runs beside the screen, as quitting says it: a computation, a
    /// figure's window, the environment being made.
    pub fn under_way(&self) -> Vec<String> {
        let mut under_way = Vec::new();
        if self.running.is_some() {
            under_way.push(match &self.run {
                Some(run) if run.total > 0 => format!(
                    "a computation, {} of {} values done{}",
                    run.done,
                    run.total,
                    if run.now.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", run.now)
                    }
                ),
                _ => "a computation".to_string(),
            });
        }
        if self.drawing.is_some() {
            under_way.push("a figure's window, open until it is closed".to_string());
        }
        if self.installing.is_some() {
            under_way.push("the environment, samplekit being installed in it".to_string());
        }
        under_way
    }

    /// Quitting, or leaving for the start page: asked first when the
    /// configuration's changes are not written, or something is under way —
    /// a computation's snapshot is taken as it ends, and the workbench
    /// dropped before it lost it while its worker went on writing unseen.
    fn quit(&mut self) -> Effect {
        let unwritten = self
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace.dirty);
        let under_way = self.under_way();
        if !unwritten && under_way.is_empty() {
            return Effect::Quit;
        }
        let mut lines = Vec::new();
        if unwritten {
            lines.push("the configuration's changes are not written".to_string());
            lines.push("y leaves without them · n stays — P, then w, writes them".to_string());
        }
        if !under_way.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("under way:".to_string());
            lines.extend(under_way.iter().map(|what| format!("  {what}")));
            lines.push(String::new());
            lines.push("y leaves once it has ended, what it wrote kept · n stays".to_string());
        }
        self.mode = Mode::Confirm {
            title: if self.from_start {
                "back to the start page".to_string()
            } else {
                "quit".to_string()
            },
            lines,
            pending: Box::new(Pending::Quit),
        };
        Effect::None
    }

    /// Text pasted into the terminal, typed where the cursor is in the line or
    /// the note being typed, over what is selected; nowhere else. A line takes
    /// several lines pasted as one, joined by spaces.
    pub fn paste(&mut self, pasted: &str) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        self.mode = match mode {
            Mode::Filter { mut text, .. } => {
                text.insert(pasted);
                self.filter_mode(text)
            }
            Mode::Prompt { purpose, mut text } => {
                text.insert(pasted);
                Mode::Prompt { purpose, text }
            }
            Mode::Picker {
                purpose,
                items,
                mut query,
                typing: true,
                ..
            } => {
                query.insert(pasted);
                Mode::Picker {
                    purpose,
                    items,
                    cursor: 0,
                    query,
                    typing: true,
                }
            }
            Mode::Quantity {
                path,
                field,
                mut slots,
                on,
            } => {
                slots[on].text.insert(pasted);
                Mode::Quantity {
                    path,
                    field,
                    slots,
                    on,
                }
            }
            Mode::Note {
                path,
                mut text,
                original,
            } => {
                text.insert(pasted);
                Mode::Note {
                    path,
                    text,
                    original,
                }
            }
            mode => mode,
        };
    }

    /// Ctrl+C where a line or the note is typed and something in it is
    /// selected: copied, and whether it was — Ctrl+C is then no interrupt.
    pub fn copy(&mut self) -> bool {
        match &mut self.mode {
            Mode::Filter { text, .. } | Mode::Prompt { text, .. } => text.copy(),
            Mode::Picker {
                query,
                typing: true,
                ..
            } => query.copy(),
            Mode::Quantity { slots, on, .. } => slots[*on].text.copy(),
            Mode::Note { text, .. } => text.copy(),
            _ => false,
        }
    }

    /// Ctrl+C: whether the workbench ends now. With nothing under way it does;
    /// with a computation, a figure's window or the environment under way it
    /// asks as `q` does, and waits on `y` for it to end — and a second Ctrl+C
    /// ends it at once, what runs dropped. Ended either way, `interrupted` says
    /// it was an interrupt.
    pub fn interrupt(&mut self) -> bool {
        let first = !self.interrupted;
        self.interrupted = true;
        if !first || self.under_way().is_empty() {
            return true;
        }
        let again = "Ctrl+C again quits at once, what runs dropped".to_string();
        if self.leaving {
            self.message = format!("{} · {again}", self.message);
            return false;
        }
        self.quit();
        if let Mode::Confirm { lines, .. } = &mut self.mode {
            lines.push(again);
        }
        false
    }

    /// A filter that matched nothing, and a value it compares a field with that
    /// the collection does not hold: the nearest the collection holds for that
    /// field, as `style == stuot: did you mean stout?`. The collection's own
    /// values are the candidates, the completion's; a number is not offered
    /// one, near being no likeness there.
    fn nearest_value(&self, text: &str) -> Option<String> {
        let vocabulary = self.collection.vocabulary();
        for (head, clause, value) in compared_values(text) {
            let bare = value.trim_matches(|c| c == '"' || c == '\'');
            if bare.is_empty() || bare.parse::<f64>().is_ok() {
                continue;
            }
            let mut known: Vec<String> = Vec::new();
            for entry in self.collection.iter() {
                let sample = entry.sample.borrow();
                let subject = Subject {
                    sample: &sample,
                    path: entry.path.as_deref(),
                    vocabulary: &vocabulary,
                    states: entry.states.as_deref(),
                };
                for candidate in filter::complete(&head, &subject) {
                    let Some(offered) = candidate.strip_prefix(&head) else {
                        continue;
                    };
                    let offered = offered.trim().trim_matches('"').to_string();
                    if !offered.is_empty() && !known.contains(&offered) {
                        known.push(offered);
                    }
                }
            }
            if known.iter().any(|held| held == bare) {
                continue;
            }
            // Case aside: `Stout` for `stout` is as near as a value gets.
            let lowered: Vec<String> = known.iter().map(|held| held.to_lowercase()).collect();
            let Some(near) = crate::core::identifier::nearest(
                &bare.to_lowercase(),
                lowered.iter().map(String::as_str),
            ) else {
                continue;
            };
            let near = lowered
                .iter()
                .position(|held| *held == near)
                .map_or(near, |at| known[at].clone());
            return Some(format!("{clause}: did you mean {near}?"));
        }
        None
    }

    /// Quit while something was under way, and it has now ended: the
    /// runtime ends the workbench.
    pub fn left(&self) -> bool {
        self.leaving && self.under_way().is_empty()
    }

    fn move_by(&mut self, by: isize) {
        let last = self.view.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + by).clamp(0, last.max(0)) as usize;
    }

    fn help(&mut self) {
        self.mode = Mode::Help { scroll: 0 };
    }

    /// The keys of the screen shown, by section and in the order they are
    /// bound: each action once, with every key bound to it. Drawn from the
    /// bindings, so that the help cannot drift from them.
    pub fn help_sections(&self) -> Vec<(&'static str, Vec<(String, &'static str)>)> {
        let place = self.place();
        let mut sections = keys_of(place);
        // Opened from the start page, `q` goes back there.
        if self.from_start {
            for (_, rows) in &mut sections {
                for (keys, said) in rows.iter_mut() {
                    if keys == "q" && *said == "quit" {
                        *said = "back to the start page, where q quits";
                    }
                }
            }
        }
        // A configuration opened from the start page goes back there, once its
        // changes are written or let go: help said the collection.
        if self.back_to_start && place == Place::Configure {
            for (_, rows) in &mut sections {
                for (keys, said) in rows.iter_mut() {
                    if keys == "Esc q" {
                        *said = "back to the start page, once the changes are written or dropped";
                    }
                }
            }
        }
        // What the marks beside a sample or a value say: nowhere else did.
        if matches!(
            place,
            Place::Collection | Place::Sample | Place::Table | Place::Control
        ) {
            sections.push((
                "marks",
                MARKS
                    .iter()
                    .map(|(mark, said)| (mark.to_string(), *said))
                    .collect(),
            ));
        }
        sections
    }

    fn helping(&mut self, key: Key, scroll: usize) -> Effect {
        match key {
            Key::Char('j') | Key::Down => {
                self.mode = Mode::Help { scroll: scroll + 1 };
            }
            Key::Char('k') | Key::Up => {
                self.mode = Mode::Help {
                    scroll: scroll.saturating_sub(1),
                };
            }
            Key::PageDown => {
                self.mode = Mode::Help {
                    scroll: scroll + self.page,
                };
            }
            Key::PageUp => {
                self.mode = Mode::Help {
                    scroll: scroll.saturating_sub(self.page),
                };
            }
            _ => {}
        }
        Effect::None
    }

    // --------------------------------------------------------------- filter

    /// Every field of the collection, described — a table's cells among
    /// them — read once per load: over a large collection, each key typed in
    /// the filter read them all again.
    pub fn described_fields(&self) -> &[String] {
        self.described.get_or_init(|| {
            self.collection
                .available_fields()
                .iter()
                .map(fields::describe)
                .collect()
        })
    }

    /// The fields a picker offers: a list's items by position — `hops[#0]`,
    /// an address, not a field anyone chose — offered as the list itself
    /// for a column, which shows it whole, and for a sort, which cannot
    /// order a whole list, as its first item alone.
    fn offered_fields(&self, whole_lists: bool) -> Vec<String> {
        let mut offered: Vec<String> = Vec::new();
        for field in self.described_fields() {
            let item = field
                .split_once("[#")
                .filter(|(list, _)| !list.contains('.'));
            let field = match item {
                Some((list, _)) if whole_lists => list.to_string(),
                Some((_, position)) if position != "0]" => continue,
                _ => field.clone(),
            };
            if !offered.contains(&field) {
                offered.push(field);
            }
        }
        offered
    }

    fn filter_mode(&self, text: Typed) -> Mode {
        let (candidates, error) = self.filter_help(text.text(), text.at());
        Mode::Filter {
            text,
            candidates,
            chosen: None,
            error,
        }
    }

    /// What could come next, and what is wrong with what is written: the
    /// command line's own completion and errors.
    fn filter_help(&self, whole: &str, at: usize) -> (Vec<String>, Option<String>) {
        let text = &whole[..at];
        let candidates = self
            .collection
            .get(self.cursor.min(self.collection.len().saturating_sub(1)))
            .map(|entry| {
                let sample = entry.sample.borrow();
                let vocabulary = self.collection.vocabulary();
                let subject = Subject {
                    sample: &sample,
                    path: entry.path.as_deref(),
                    vocabulary: &vocabulary,
                    states: entry.states.as_deref(),
                };
                let offered = |written: &str| {
                    let mut found: Vec<String> = Vec::new();
                    for entry in self.collection.iter() {
                        let other = entry.sample.borrow();
                        let subject = Subject {
                            sample: &other,
                            path: entry.path.as_deref(),
                            vocabulary: &vocabulary,
                            states: entry.states.as_deref(),
                        };
                        for candidate in filter::complete(written, &subject) {
                            if !found.contains(&candidate) {
                                found.push(candidate);
                            }
                        }
                    }
                    found
                };
                let _ = subject;
                let mut found = offered(text);
                // What begins with the word being typed comes first; then what
                // holds its letters in order, closest first: `bx` finds `brix`,
                // and `hz` finds `haze`.
                let cut = text
                    .rfind(|c: char| " .[(\"'!=<>&|,".contains(c))
                    .map_or(0, |at| at + 1);
                let (settled, typed) = text.split_at(cut);
                if !typed.is_empty() {
                    let mut near: Vec<(i64, String)> = offered(settled)
                        .into_iter()
                        .filter(|candidate| !found.contains(candidate))
                        .filter_map(|candidate| {
                            let word = candidate.strip_prefix(settled)?;
                            let word = word.trim_matches(|c: char| c == '"' || c == ' ');
                            fuzzy(word, typed).map(|score| (score, candidate))
                        })
                        .collect();
                    near.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
                    found.extend(near.into_iter().map(|(_, candidate)| candidate));
                }
                // Then every field of the collection, a table's cells among
                // them, holding the letters of the whole word being typed:
                // `ebc20` finds `measurements.ebc[20]`, which the word after
                // the last `.` alone never reached.
                let start = text
                    .rfind(|c: char| " (\"'!=<>&|,".contains(c))
                    .map_or(0, |at| at + 1);
                let (before, word) = text.split_at(start);
                if !word.is_empty() {
                    let mut deep: Vec<(i64, String)> = self
                        .described_fields()
                        .iter()
                        .filter_map(|field| {
                            let candidate = format!("{before}{field}");
                            (!found.contains(&candidate))
                                .then(|| fuzzy(field, word).map(|score| (score, candidate)))
                                .flatten()
                        })
                        .collect();
                    deep.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
                    found.extend(deep.into_iter().map(|(_, candidate)| candidate));
                }
                found
            })
            .unwrap_or_default();
        let error = if whole.trim().is_empty() {
            None
        } else {
            filter::parse(whole)
                .err()
                .map(|error| filter::caret(whole, &error).unwrap_or_else(|| error.to_string()))
        };
        // A decimal comma is said as one, and completes nothing: after `1,`
        // the completions were values of their own, `og > 1,1.0513…`.
        if error.is_some()
            && let Some(comma) = decimal_comma(whole)
        {
            return (Vec::new(), Some(comma));
        }
        let mut shown: Vec<String> = Vec::new();
        for candidate in candidates {
            let candidate = self.completion_shown(candidate);
            if !shown.contains(&candidate) {
                shown.push(candidate);
            }
        }
        (shown.into_iter().take(40).collect(), error)
    }

    /// A completion's number at the precision its field declares where it
    /// is compared by order — `og > 1.0513333333333332` offered as `og >
    /// 1.051` — denoised where none is. Beside `==` or `!=` it stays whole:
    /// only the very number equals the value.
    fn completion_shown(&self, candidate: String) -> String {
        let Some((head, number)) = candidate.rsplit_once(' ') else {
            return candidate;
        };
        let Ok(parsed) = number.parse::<f64>() else {
            return candidate;
        };
        if number.parse::<i64>().is_ok() {
            return candidate;
        }
        let Some((before, operator)) = head.trim_end().rsplit_once(' ') else {
            return candidate;
        };
        if !matches!(operator, ">" | ">=" | "<" | "<=") {
            return candidate;
        }
        let field = before
            .rsplit(|c: char| " (!&|".contains(c))
            .next()
            .unwrap_or(before);
        let precision = self
            .collection
            .config()
            .and_then(|config| config.property(field))
            .and_then(|declared| declared.precision.as_ref())
            .and_then(crate::format::schema::PrecisionSchema::precision);
        format!(
            "{head} {}",
            at_precision(&crate::core::value::Value::Number(parsed), precision, false)
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn filtering(
        &mut self,
        key: Key,
        mut line: Typed,
        candidates: Vec<String>,
        chosen: Option<usize>,
        _error: Option<String>,
    ) -> Effect {
        match key {
            Key::Esc => return Effect::None,
            Key::Enter => {
                let text = line.text().to_string();
                let before = std::mem::replace(&mut self.filter, text.clone());
                match self.refresh() {
                    Ok(()) => {
                        self.cursor = 0;
                        self.message = format!(
                            "{} of {}",
                            self.view.len(),
                            counted(self.collection.len(), "sample", "samples")
                        );
                        // Nothing matched: a value mistyped is offered its
                        // nearest among those the collection holds.
                        if self.view.is_empty()
                            && let Some(near) = self.nearest_value(&text)
                        {
                            self.message = format!("{} · {near}", self.message);
                        }
                    }
                    Err(error) => {
                        // Said as what is wrong with what was typed, and what
                        // is still applied: "set aside" read as the filter
                        // in force having gone, which it had not.
                        let error = decimal_comma(&text).unwrap_or_else(|| {
                            error
                                .strip_prefix("the filter was set aside: ")
                                .unwrap_or(&error)
                                .to_string()
                        });
                        let in_force = if before.trim().is_empty() {
                            "no filter is in force: every sample is shown".to_string()
                        } else {
                            format!("the filter in force is still {before}")
                        };
                        self.filter = before;
                        self.refresh().ok();
                        let (candidates, _) = self.filter_help(&text, line.at());
                        self.mode = Mode::Filter {
                            text: line,
                            candidates,
                            chosen: None,
                            error: Some(format!("{error}\n{in_force}")),
                        };
                    }
                }
                return Effect::None;
            }
            // Tab goes through the completions, Shift+Tab back.
            Key::Tab | Key::BackTab if !candidates.is_empty() => {
                let count = candidates.len();
                let next = match key {
                    Key::Tab => chosen.map_or(0, |at| (at + 1) % count),
                    _ => chosen.map_or(count - 1, |at| (at + count - 1) % count),
                };
                // What follows the cursor stays after the completion.
                line.complete(&candidates[next]);
                self.mode = Mode::Filter {
                    text: line,
                    chosen: Some(next),
                    candidates,
                    error: None,
                };
                return Effect::None;
            }
            // The cursor moves through what is written: a parenthesis can be
            // put back before a clause already typed; words, a selection and
            // undo are the line's own.
            _ => {
                line.key(key);
            }
        }
        self.mode = self.filter_mode(line);
        Effect::None
    }

    // --------------------------------------------------------------- pickers

    fn columns_mode(&self) -> Mode {
        let shown: Vec<String> = self
            .profile
            .columns()
            .iter()
            .map(|column| column.field.clone())
            .collect();
        let mut items: Vec<(String, bool)> =
            shown.iter().map(|field| (field.clone(), true)).collect();
        for field in self.offered_fields(true) {
            if !shown.contains(&field) {
                items.push((field, false));
            }
        }
        Mode::Picker {
            purpose: Choosing::Columns,
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        }
    }

    /// The fields `g` offers to group by: those grouped by first, ticked, in
    /// their order, then the columns' others; grouped, a first item that
    /// ungroups.
    fn group_mode(&self) -> Mode {
        let mut items: Vec<(String, bool)> = Vec::new();
        if !self.group.is_empty() {
            items.push((NO_GROUPING.to_string(), false));
        }
        items.extend(self.group.iter().map(|field| (field.clone(), true)));
        for field in self.offered_fields(true) {
            if !self.group.contains(&field) {
                items.push((field, false));
            }
        }
        Mode::Picker {
            purpose: Choosing::Group,
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        }
    }

    /// What `f` applies: the saved queries and the profiles of the folder
    /// opened, with what its configuration imports — at the root of several
    /// collections, what is common, never each one's. Another project is
    /// reached from the start page, not from here. A folder with no
    /// configuration of its own over several projects offers each one's, named
    /// by its project: it has nothing else to offer.
    fn goto_mode(&self) -> Mode {
        let mut items = Vec::new();
        let mut targets = Vec::new();
        let mut configs: Vec<&ProjectConfig> = Vec::new();
        match self.collection.config() {
            Some(config) => configs.push(config),
            None => {
                for entry in self.collection.iter() {
                    let Some(config) = entry
                        .path
                        .as_deref()
                        .and_then(|path| self.collection.config_for(path))
                    else {
                        continue;
                    };
                    if !configs.iter().any(|held| held.root() == config.root()) {
                        configs.push(config);
                    }
                }
            }
        }
        let several = configs.len() > 1;
        for config in configs {
            let project = if several {
                format!("{} · ", self.project_title(config.root()))
            } else {
                String::new()
            };
            // Where each comes from, where the configuration imports another.
            let origin = |kind: DeclarationKind, name: &str| {
                if config.imports().is_empty() {
                    return String::new();
                }
                match config.origin(kind, name) {
                    DeclaredIn::Local => "   · local".to_string(),
                    DeclaredIn::Imported(file) => format!("   · {}", self.shown_file(&file)),
                    DeclaredIn::Over(file) => format!("   · local over {}", self.shown_file(&file)),
                }
            };
            for name in config.query_names() {
                if let Ok(query) = config.query(name) {
                    items.push((
                        format!(
                            "query      {project}{name}   {}{}",
                            query.filter,
                            origin(DeclarationKind::Query, name)
                        ),
                        false,
                    ));
                    targets.push(Target::Query(query.filter.clone()));
                }
            }
            for name in config.profile_names() {
                if let Ok(profile) = config.profile(name) {
                    items.push((
                        format!(
                            "profile    {project}{name}{}",
                            origin(DeclarationKind::Profile, name)
                        ),
                        false,
                    ));
                    targets.push(Target::Profile(Box::new(profile.clone())));
                }
            }
        }
        Mode::Picker {
            purpose: Choosing::GoTo(targets),
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        }
    }

    /// What `p` offers: the figures and the exports the project declares.
    fn produce_mode(&mut self) {
        // A figure of one's own first: it needs nothing declared.
        let mut items = vec![(
            "figure  your own: a field against another".to_string(),
            false,
        )];
        let mut outputs = vec![Output::Own];
        let Some(config) = self.collection.config() else {
            self.mode = Mode::Picker {
                purpose: Choosing::Produce(outputs),
                items,
                cursor: 0,
                query: Typed::unfocused(""),
                typing: false,
            };
            return;
        };
        for name in config.figure_names() {
            let title = config
                .figure(name)
                .and_then(|figure| figure.title.clone())
                .unwrap_or_default();
            items.push((format!("figure  {name}   {title}"), false));
            outputs.push(Output::Figure(name.to_string()));
        }
        // The model's own figures, read from its source without running it.
        let (of_the_model, _) = runtime::model_declarations(config);
        for name in of_the_model {
            if config.figure(&name).is_none() {
                items.push((format!("figure  {name}   the model's"), false));
                outputs.push(Output::ModelFigure(name));
            }
        }
        for name in config.export_names() {
            let output = config
                .export(name)
                .map(|export| export.output.display().to_string())
                .unwrap_or_default();
            items.push((format!("export  {name} → {output}"), false));
            outputs.push(Output::Export(name.to_string()));
        }
        if items.is_empty() {
            self.message = "the project declares no figure and no export".to_string();
            return;
        }
        self.mode = Mode::Picker {
            purpose: Choosing::Produce(outputs),
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        };
    }

    /// The samples a figure or an export is of: the basket, or what is shown.
    fn chosen_samples(&self) -> Result<SampleList, String> {
        if self.basket.is_empty() {
            return Ok(self.view.filter_by(|_| true));
        }
        let paths: Vec<PathBuf> = self.basket.iter().cloned().collect();
        list::from_files(&paths).map_err(|error| error.to_string())
    }

    fn produce(&mut self, output: Output) {
        let chosen = match self.chosen_samples() {
            Ok(chosen) => chosen,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        match output {
            Output::Own => self.open_figure(None),
            Output::Figure(name) => self.draw_figure(&name, chosen),
            Output::ModelFigure(name) => self.draw_model_figure(&name),
            Output::Export(name) => self.preview_export(&name, chosen),
        }
    }

    /// The figure's window, opened on what was set up last — its axes kept
    /// while the collection still holds them — or over one sample's table.
    fn open_figure(&mut self, of: Option<(PathBuf, Identifier)>) {
        let style = self
            .figure_styles()
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "plain".to_string());
        let kept = self.figure.take().filter(|form| form.of == of);
        let mut form = kept.unwrap_or_else(|| FigureForm::new(style, of));
        form.row = 0;
        self.figure = Some(form);
        self.mode = Mode::Figure;
    }

    /// The fields an axis or a group may name: values first, then a table's
    /// whole columns, which a figure draws against one another, a curve per
    /// sample, then its cells, a point per sample — or, over one sample's
    /// table, its columns alone, those selected there first.
    fn figure_fields(&self) -> Vec<String> {
        if let Some((path, table)) = self.figure.as_ref().and_then(|form| form.of.as_ref()) {
            let held: Vec<String> = self
                .view
                .iter()
                .find(|entry| entry.path.as_deref() == Some(path.as_path()))
                .and_then(|entry| {
                    let sample = entry.sample.borrow();
                    sample.table(table).ok().map(|held| {
                        held.column_names()
                            .into_iter()
                            .map(|column| format!("{table}.{column}"))
                            .collect()
                    })
                })
                .unwrap_or_default();
            let chosen = |field: &String| {
                self.selected
                    .iter()
                    .any(|selected| selected.split('[').next() == Some(field.as_str()))
            };
            let (first, rest): (Vec<String>, Vec<String>) = held.into_iter().partition(chosen);
            return first.into_iter().chain(rest).collect();
        }
        let vocabulary = self.collection.vocabulary();
        let columns = vocabulary
            .tables_as_strings()
            .into_iter()
            .flat_map(|table| {
                let columns = Identifier::new(&table)
                    .map(|name| vocabulary.columns_as_strings(&name))
                    .unwrap_or_default();
                columns
                    .into_iter()
                    .map(move |column| format!("{table}.{column}"))
            });
        let described: Vec<String> = self
            .collection
            .available_fields()
            .iter()
            .map(fields::describe)
            .collect();
        // `state` among them, a column as `-c name,state` makes it.
        let (cells, values): (Vec<String>, Vec<String>) = described
            .into_iter()
            .partition(|field| field.ends_with(']'));
        values.into_iter().chain(columns).chain(cells).collect()
    }

    /// The styles a figure's labels may be written in: the project's for
    /// figures first, then the others it declares, `plain` last — Unicode, where
    /// the others may be matplotlib's mathtext.
    pub fn figure_styles(&self) -> Vec<(String, &'static str)> {
        let Some(config) = self.collection.config() else {
            return vec![("plain".to_string(), "Unicode")];
        };
        let default = config.render().figure_style.clone();
        let mut styles: Vec<(String, &'static str)> = Vec::new();
        if let Some(default) = &default {
            styles.push((default.clone(), "the project's for figures"));
        }
        for name in config.render_style_names() {
            if name != "plain" && Some(name) != default.as_deref() {
                styles.push((name.to_string(), ""));
            }
        }
        styles.push(("plain".to_string(), "Unicode"));
        styles
    }

    /// A key in the figure's window: a row chosen, changed, or the figure drawn.
    fn figuring(&mut self, key: Key) -> Effect {
        let styles: Vec<String> = self
            .figure_styles()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let Some(form) = &mut self.figure else {
            return Effect::None;
        };
        let rows = form.rows();
        let row = rows.get(form.row).copied().unwrap_or(FigureRow::Y);
        match key {
            Key::Esc | Key::Char('q') => return Effect::None,
            Key::Char('j') | Key::Down => form.row = (form.row + 1).min(rows.len() - 1),
            Key::Char('k') | Key::Up => form.row = form.row.saturating_sub(1),
            Key::Char('h') | Key::Left => form.cycle(row, -1, &styles),
            Key::Char('l') | Key::Right | Key::Char(' ') => form.cycle(row, 1, &styles),
            Key::Char('p') => {
                let form = form.clone();
                self.draw_own(&form);
                return Effect::None;
            }
            // Kept: declared in `.samplekitrc`, drawn again by `plot NAME`.
            Key::Char('w') => {
                if form.y.is_none() || form.x.is_none() {
                    self.message = "choose both axes first: Enter on y, then on x".to_string();
                } else {
                    self.mode = Mode::prompt(Purpose::DeclareName(Declare::Figure), String::new());
                    return Effect::None;
                }
            }
            Key::Enter => match row {
                FigureRow::Y | FigureRow::X | FigureRow::Group => {
                    let mut items: Vec<(String, bool)> = self
                        .figure_fields()
                        .into_iter()
                        .map(|field| (field, false))
                        .collect();
                    if row == FigureRow::Group {
                        items.insert(0, ("none".to_string(), false));
                    }
                    self.mode = Mode::Picker {
                        purpose: Choosing::FigureField(row),
                        items,
                        cursor: 0,
                        query: Typed::unfocused(""),
                        typing: false,
                    };
                    return Effect::None;
                }
                FigureRow::Title | FigureRow::XLabel | FigureRow::YLabel => {
                    let text = form.text(row).to_string();
                    self.mode = Mode::prompt(Purpose::FigureText(row), text);
                    return Effect::None;
                }
                _ => form.cycle(row, 1, &styles),
            },
            _ => {}
        }
        self.mode = Mode::Figure;
        Effect::None
    }

    /// What `W` offers to declare of what is shown.
    fn declare_mode(&mut self) {
        let mut offered = Vec::new();
        let mut items = Vec::new();
        if !self.filter.is_empty() {
            items.push((format!("query     the filter: {}", self.filter), false));
            offered.push(Declare::Query);
        }
        items.push((
            "profile   the columns shown, and the sort".to_string(),
            false,
        ));
        offered.push(Declare::Profile);
        for format in ["csv", "tsv", "json"] {
            items.push((
                format!(
                    "export    the columns as {format}, written to out/NAME.{format}{}",
                    if self.filter.is_empty() {
                        ""
                    } else {
                        ", of the filter"
                    }
                ),
                false,
            ));
            offered.push(Declare::Export(format));
        }
        self.mode = Mode::Picker {
            purpose: Choosing::Declare(offered),
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        };
    }

    /// What is shown, declared under `name` in the project's `.samplekitrc`,
    /// previewed as the lines it adds. A name the file already gives that kind
    /// is refused: another's declaration is not replaced from here.
    fn preview_declare(&mut self, what: &Declare, name: &str) {
        if let Err(error) = Identifier::new(name) {
            self.message = format!("'{name}' is no name: {error}");
            return;
        }
        let path = self
            .collection
            .config()
            .map(|config| config.root().join(".samplekitrc"))
            .unwrap_or_else(|| self.root.join(".samplekitrc"));
        let mut edit = match ConfigurationEdit::open(&path) {
            Ok(edit) => edit,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let taken = |kind: Named| edit.names(kind).iter().any(|held| held == name);
        let kinds: Vec<Named> = match what {
            Declare::Query => vec![Named::Query],
            Declare::Profile => vec![Named::Profile],
            Declare::Export(_) if self.filter.is_empty() => vec![Named::Profile, Named::Export],
            Declare::Export(_) => vec![Named::Query, Named::Profile, Named::Export],
            Declare::Figure => vec![Named::Figure],
        };
        if let Some(kind) = kinds.iter().find(|kind| taken(**kind)) {
            self.message = format!(
                "a {} named {name} is declared already: another name",
                kind.section()
            );
            return;
        }
        let text = |value: &str| toml_edit::Value::from(value.to_string());
        let query = |edit: &mut ConfigurationEdit| {
            edit.set(Named::Query, name, "filter", text(&self.filter));
        };
        let profile = |edit: &mut ConfigurationEdit| {
            let mut columns = toml_edit::Array::new();
            for column in self.profile.columns() {
                let mut entry = toml_edit::InlineTable::new();
                entry.insert("field", text(&column.field));
                if let Some(label) = &column.label {
                    entry.insert("label", text(label));
                }
                columns.push(entry);
            }
            edit.set(Named::Profile, name, "columns", columns);
            if !self.sort.is_empty() {
                // As the screen sorts it: `r` turns every key.
                let keys: toml_edit::Array = self
                    .sort
                    .iter()
                    .map(|key| match (self.reverse, key.strip_prefix('-')) {
                        (false, _) => key.clone(),
                        (true, Some(field)) => field.to_string(),
                        (true, None) => format!("-{key}"),
                    })
                    .collect();
                edit.set(Named::Profile, name, "sort", keys);
            }
            // The grouping shown, which `f` and `--profile` then follow.
            if !self.group.is_empty() {
                let fields: toml_edit::Array = self.group.iter().cloned().collect();
                edit.set(Named::Profile, name, "group", fields);
            }
        };
        let said = match what {
            Declare::Query => {
                query(&mut edit);
                format!("query {name} declared: g goes to it, --query {name} selects with it")
            }
            Declare::Profile => {
                profile(&mut edit);
                format!("profile {name} declared: --profile {name} shows it")
            }
            Declare::Export(format) => {
                if !self.filter.is_empty() {
                    query(&mut edit);
                    edit.set(Named::Export, name, "query", text(name));
                }
                profile(&mut edit);
                edit.set(Named::Export, name, "profile", text(name));
                edit.set(Named::Export, name, "format", text(format));
                edit.set(
                    Named::Export,
                    name,
                    "output",
                    text(&format!("out/{name}.{format}")),
                );
                format!("export {name} declared: p writes it, samplekit export {name} too")
            }
            Declare::Figure => {
                let Some(form) = self.figure.clone() else {
                    return;
                };
                let (Some(y), Some(x)) = (form.y.clone(), form.x.clone()) else {
                    return;
                };
                edit.set(Named::Figure, name, "kind", text(form.kind.as_str()));
                edit.set(Named::Figure, name, "x", text(&x));
                edit.set(Named::Figure, name, "y", text(&y));
                if let Some(group) = form.group.as_ref().filter(|_| form.of.is_none()) {
                    edit.set(Named::Figure, name, "group", text(group));
                }
                edit.set(Named::Figure, name, "style", text(&form.style));
                for (key, value) in [
                    ("title", form.title.as_str()),
                    ("x_label", form.x_label.as_str()),
                    ("y_label", form.y_label.as_str()),
                ] {
                    if !value.is_empty() {
                        edit.set(Named::Figure, name, key, text(value));
                    }
                }
                for (key, value, default) in [
                    ("x_scale", form.x_scale, "linear"),
                    ("y_scale", form.y_scale, "linear"),
                    ("legend", form.legend, "best"),
                ] {
                    if value != default {
                        edit.set(Named::Figure, name, key, text(value));
                    }
                }
                format!("figure {name} declared: p draws it, samplekit plot {name} too")
            }
        };
        if let Err(error) = edit.checked() {
            self.message = error.to_string();
            return;
        }
        self.mode = Mode::Confirm {
            title: format!("save in {}", self.here(&path)),
            lines: edit.changes(),
            pending: Box::new(Pending::Declare {
                edit: Box::new(edit),
                said,
            }),
        };
    }

    /// A figure of one's own, of the basket or the samples shown — or of one
    /// sample's table — drawn as `plot -x -y` draws it: one project's samples.
    fn draw_own(&mut self, form: &FigureForm) {
        self.mode = Mode::Figure;
        let (Some(y), Some(x)) = (form.y.clone(), form.x.clone()) else {
            self.message = "choose both axes first: Enter on y, then on x".to_string();
            return;
        };
        if self.drawing.is_some() {
            self.message = "a figure is open: close its window first".to_string();
            return;
        }
        let chosen = match &form.of {
            Some((path, _)) => match list::from_files(std::slice::from_ref(path)) {
                Ok(one) => one,
                Err(error) => {
                    self.message = error.to_string();
                    return;
                }
            },
            None => match self.chosen_samples() {
                Ok(chosen) => chosen,
                Err(error) => {
                    self.message = error;
                    return;
                }
            },
        };
        let config = self.collection.config();
        let root = config.map(|config| config.root().to_path_buf());
        let (samples, elsewhere): (Vec<PathBuf>, Vec<PathBuf>) = chosen
            .iter()
            .filter_map(|entry| entry.path.clone())
            .partition(|path| {
                self.collection
                    .config_for(path)
                    .map(|theirs| theirs.root().to_path_buf())
                    == root
            });
        let name = format!("{y} against {x}");
        if samples.is_empty() {
            self.message = format!("{name}: no sample to draw");
            return;
        }
        let python = match config {
            Some(config) => {
                runtime::interpreter_for(config, &self.root).map_err(|error| error.to_string())
            }
            None => runtime::find_interpreter(&self.root).ok_or_else(|| {
                "no environment found: a figure is drawn by matplotlib, in the .venv/ beside the project"
                    .to_string()
            }),
        };
        let python = match python {
            Ok(python) => python,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let count = samples.len();
        let request = plotting::FigureRequest {
            choice: plotting::FigureChoice::AdHoc(Box::new(form.declaration(&x, &y))),
            samples,
            output: None,
            overrides: form.overrides(),
            project_style: true,
            said: None,
            snapshot: None,
        };
        self.spawn_drawing(python, request, &name, count);
        self.said_left_out(elsewhere.len());
    }

    /// A declared figure drawn in its window beside the screen, what Python
    /// says kept for when it closes.
    fn draw_figure(&mut self, name: &str, mut chosen: SampleList) {
        let Some(config) = self.collection.config() else {
            return;
        };
        if self.drawing.is_some() {
            self.message = "a figure is open: close its window first".to_string();
            return;
        }
        // Its own query narrows what it draws, and it draws one project's
        // samples.
        if let Some(query) = config.figure(name).and_then(|figure| figure.query.clone()) {
            let narrowed = crate::collection::named_queries::named(config, &query)
                .map_err(|error| error.to_string())
                .and_then(|declared| {
                    filter::parse(&declared.filter).map_err(|error| error.to_string())
                })
                .and_then(|parsed| chosen.filter(&parsed).map_err(|error| error.to_string()));
            match narrowed {
                Ok(narrowed) => chosen = narrowed,
                Err(error) => {
                    self.message = error;
                    return;
                }
            }
        }
        let root = config.root().to_path_buf();
        let (samples, elsewhere): (Vec<PathBuf>, Vec<PathBuf>) = chosen
            .iter()
            .filter_map(|entry| entry.path.clone())
            .partition(|path| {
                self.collection
                    .config_for(path)
                    .is_some_and(|theirs| theirs.root() == root)
            });
        if samples.is_empty() {
            self.message = format!("'{name}' has no sample to draw");
            return;
        }
        let python = match runtime::interpreter_for(config, &self.root) {
            Ok(python) => python,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let count = samples.len();
        let request = plotting::FigureRequest {
            choice: plotting::FigureChoice::Named {
                name: name.to_string(),
                model: false,
            },
            samples,
            output: None,
            overrides: Default::default(),
            project_style: true,
            said: None,
            snapshot: None,
        };
        self.spawn_drawing(python, request, name, count);
        self.said_left_out(elsewhere.len());
    }

    /// A drawing started beside the screen, what Python says kept for when its
    /// window closes.
    fn spawn_drawing(
        &mut self,
        python: PathBuf,
        request: plotting::FigureRequest,
        name: &str,
        count: usize,
    ) {
        let drawn = name.to_string();
        let name = name.to_string();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let said = match plotting::draw_quietly(&python, &request) {
                Ok(()) => format!("'{name}' closed"),
                Err((error, said)) if said.is_empty() => error.to_string(),
                Err((_, said)) => said,
            };
            let _ = sender.send(Progress::Done(said));
        });
        self.drawing = Some(Running {
            receiver,
            before: None,
        });
        self.message = format!("drawing '{drawn}' of {count} samples: its window opens beside");
    }

    /// A figure the model declares, of the basket or the sample under the
    /// cursor: the model's code, drawn as `plot` draws it, nothing asked.
    fn draw_model_figure(&mut self, name: &str) {
        let Some(config) = self.collection.config() else {
            return;
        };
        if self.drawing.is_some() {
            self.message = "a figure is open: close its window first".to_string();
            return;
        }
        let asked: Vec<PathBuf> = if self.basket.is_empty() {
            self.current().map(|(path, _)| path).into_iter().collect()
        } else {
            self.basket.iter().cloned().collect()
        };
        // Python reads each sample with its own project's model: another
        // project's is not this figure's, so it is left out, as `plot` draws
        // one project's samples.
        let root = config.root().to_path_buf();
        let (samples, elsewhere): (Vec<PathBuf>, Vec<PathBuf>) =
            asked.into_iter().partition(|path| {
                self.collection
                    .config_for(path)
                    .is_some_and(|theirs| theirs.root() == root)
            });
        if samples.is_empty() {
            self.message = if elsewhere.is_empty() {
                format!("'{name}' has no sample to draw")
            } else {
                format!(
                    "'{name}' is this project's model's, and draws none of another project's samples"
                )
            };
            return;
        }
        let python = match runtime::availability(Some(config), &self.root) {
            Ok(runtime::Availability::Ready { python, .. }) => python,
            Ok(runtime::Availability::NoTemplate) => {
                self.message = "no model is declared here".to_string();
                return;
            }
            Ok(runtime::Availability::Unavailable { reason }) => {
                self.message = format!("the model cannot run: {reason}");
                return;
            }
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let count = samples.len();
        let request = plotting::FigureRequest {
            choice: plotting::FigureChoice::Named {
                name: name.to_string(),
                model: true,
            },
            samples,
            output: None,
            overrides: Default::default(),
            project_style: true,
            said: None,
            snapshot: None,
        };
        self.spawn_drawing(python, request, name, count);
        self.said_left_out(elsewhere.len());
    }

    /// Samples of another project a figure leaves out, said beside it.
    fn said_left_out(&mut self, left_out: usize) {
        if left_out > 0 {
            self.message = format!("{} · {left_out} of another project left out", self.message);
        }
    }

    /// A declared export previewed as `export` previews it: where it goes, how
    /// many rows, what in it is not current.
    fn preview_export(&mut self, name: &str, chosen: SampleList) {
        let Some(config) = self.collection.config() else {
            return;
        };
        let of = if self.basket.is_empty() {
            format!(
                "{} of {} shown",
                chosen.len(),
                counted(self.collection.len(), "sample", "samples")
            )
        } else {
            format!(
                "the basket's {}",
                counted(chosen.len(), "sample", "samples")
            )
        };
        let samples: Vec<PathBuf> = chosen
            .iter()
            .filter_map(|entry| entry.path.clone())
            .collect();
        let prepared = match exports::prepare(name, config, chosen, &self.collection) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let text = match export_formats::serialize(&prepared.dataset, prepared.format) {
            Ok(text) => text,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        if let Some(refused) = exports::refusal(&prepared.path) {
            self.message = format!("{}: {refused}", self.here(&prepared.path));
            return;
        }
        let rows = prepared.dataset.rows.len();
        let mut lines = vec![
            format!("{name} → {}", self.here(&prepared.path)),
            format!("{}, from {of}", counted(rows, "row", "rows")),
        ];
        if prepared.path.exists() {
            lines.push("the file is there, and y replaces it".to_string());
        }
        let behind = &prepared.behind;
        if behind.stale + behind.held > 0 {
            lines.push(String::new());
            lines.push(format!(
                "written from values that are not current: {} outdated, {} edited or without a record",
                behind.stale, behind.held
            ));
            for value in behind.named.iter().take(6) {
                lines.push(format!("  {value}"));
            }
            if behind.named.len() > 6 {
                lines.push(format!("  and {} more", behind.named.len() - 6));
            }
        }
        self.mode = Mode::Confirm {
            title: format!("export {name}"),
            lines,
            pending: Box::new(Pending::Export {
                text,
                path: prepared.path,
                rows,
                name: name.to_string(),
                samples,
            }),
        };
    }

    fn go(&mut self, target: Target) {
        match target {
            Target::Query(text) => {
                let before = std::mem::replace(&mut self.filter, text);
                match self.refresh() {
                    Ok(()) => self.cursor = 0,
                    Err(error) => {
                        self.message = error;
                        self.filter = before;
                        self.refresh().ok();
                    }
                }
            }
            Target::Profile(profile) => {
                let profile = *profile;
                self.sort = profile.sort.clone();
                self.reverse = false;
                // Its groups too, as its sort: a profile grouping by nothing
                // ungroups, as its empty sort unsorts.
                self.group = profile.group.clone();
                self.profile = profile;
                if let Err(error) = self.refresh() {
                    self.message = error;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn picking(
        &mut self,
        key: Key,
        purpose: Choosing,
        mut items: Vec<(String, bool)>,
        mut cursor: usize,
        mut query: Typed,
        mut typing: bool,
    ) -> Effect {
        let shown = visible(&items, query.text());
        let last = shown.len().saturating_sub(1);
        if typing {
            match key {
                // The narrowing gone, the cursor stays on the item it was on.
                Key::Esc => {
                    query = Typed::unfocused("");
                    typing = false;
                    cursor = shown.get(cursor).copied().unwrap_or(0);
                }
                // What they do in the list: Space ticks, Enter accepts, the
                // narrowing kept. Typed into it, a space found nothing and
                // Enter took a second press.
                Key::Enter | Key::Char(' ') => {
                    query.focus(false);
                    return self.picking(key, purpose, items, cursor, query, false);
                }
                Key::Down => cursor = (cursor + 1).min(last),
                Key::Up => cursor = cursor.saturating_sub(1),
                _ => {
                    let before = query.text().to_string();
                    if query.key(key) && query.text() != before {
                        cursor = 0;
                    }
                }
            }
            self.mode = Mode::Picker {
                purpose,
                items,
                cursor,
                query,
                typing,
            };
            return Effect::None;
        }
        match key {
            Key::Esc if !query.is_empty() => {
                query = Typed::unfocused("");
                cursor = shown.get(cursor).copied().unwrap_or(0);
            }
            Key::Esc => {
                // Back to the figure's window, which asked.
                if matches!(purpose, Choosing::FigureField(_)) {
                    self.mode = Mode::Figure;
                }
                return Effect::None;
            }
            Key::Char('/') => {
                typing = true;
                query.focus(true);
                query.key(Key::End);
            }
            Key::Char('j') | Key::Down => cursor = (cursor + 1).min(last),
            Key::Char('k') | Key::Up => cursor = cursor.saturating_sub(1),
            // Space toggles, Enter accepts what is ticked.
            Key::Char(' ') => {
                if let (Choosing::Sort | Choosing::Columns | Choosing::Group, Some(&chosen)) =
                    (&purpose, shown.get(cursor))
                {
                    // Ungrouping is chosen with Enter, not ticked.
                    if items[chosen].0 != NO_GROUPING {
                        items[chosen].1 = !items[chosen].1;
                    }
                }
            }
            // A sort key reversed, and ticked with it.
            Key::Char('r') if purpose == Choosing::Sort => {
                if let Some(&chosen) = shown.get(cursor) {
                    let (label, ticked) = &mut items[chosen];
                    *label = match label.strip_prefix('-') {
                        Some(field) => field.to_string(),
                        None => format!("-{label}"),
                    };
                    *ticked = true;
                }
            }
            // The item under the cursor moved: the sort's priority, the
            // columns' order. Among those shown when the list is narrowed.
            Key::MoveUp | Key::Char('K')
                if matches!(
                    purpose,
                    Choosing::Sort | Choosing::Columns | Choosing::Group
                ) =>
            {
                if cursor > 0 && cursor < shown.len() {
                    items.swap(shown[cursor], shown[cursor - 1]);
                    cursor -= 1;
                }
            }
            Key::MoveDown | Key::Char('J')
                if matches!(
                    purpose,
                    Choosing::Sort | Choosing::Columns | Choosing::Group
                ) =>
            {
                if cursor < last {
                    items.swap(shown[cursor], shown[cursor + 1]);
                    cursor += 1;
                }
            }
            Key::Enter => {
                let under = shown.get(cursor).copied();
                match purpose {
                    Choosing::Sort => {
                        // Every key ticked, in the order shown; the one under
                        // the cursor when none is.
                        let mut chosen: Vec<String> = items
                            .iter()
                            .filter(|(_, ticked)| *ticked)
                            .map(|(key, _)| key.clone())
                            .collect();
                        if chosen.is_empty() {
                            chosen.extend(
                                under
                                    .and_then(|at| items.get(at))
                                    .map(|(key, _)| key.clone()),
                            );
                        }
                        self.sort = chosen;
                        self.reverse = false;
                    }
                    Choosing::Columns => {
                        let chosen: Vec<String> = items
                            .into_iter()
                            .filter(|(_, ticked)| *ticked)
                            .map(|(field, _)| field)
                            .collect();
                        if chosen.is_empty() {
                            self.message = "a table needs one column".to_string();
                            return Effect::None;
                        }
                        // A column kept keeps its label, header and
                        // precision: rebuilt from its field alone, a profile's
                        // `Original gravity` went back to `og`.
                        let mut profile = profile_of(&chosen);
                        for column in &mut profile.columns {
                            if let Some(held) = self
                                .profile
                                .columns()
                                .iter()
                                .find(|held| held.field == column.field)
                            {
                                *column = held.clone();
                            }
                        }
                        self.profile = profile;
                    }
                    Choosing::Group => {
                        let under = under.and_then(|at| items.get(at)).map(|(field, _)| field);
                        if under.is_some_and(|field| field == NO_GROUPING) {
                            self.group.clear();
                        } else {
                            // Every field ticked, in the order shown; the one
                            // under the cursor when none is.
                            let mut chosen: Vec<String> = items
                                .iter()
                                .filter(|(field, ticked)| *ticked && field != NO_GROUPING)
                                .map(|(field, _)| field.clone())
                                .collect();
                            if chosen.is_empty() {
                                chosen.extend(under.cloned());
                            }
                            self.group = chosen;
                        }
                        self.cursor = 0;
                    }
                    Choosing::GoTo(mut targets) => {
                        if let Some(at) = under {
                            self.go(targets.swap_remove(at));
                        }
                        return Effect::None;
                    }
                    Choosing::Produce(mut outputs) => {
                        if let Some(at) = under {
                            self.produce(outputs.swap_remove(at));
                        }
                        return Effect::None;
                    }
                    Choosing::Declare(mut offered) => {
                        if let Some(at) = under {
                            self.mode = Mode::prompt(
                                Purpose::DeclareName(offered.swap_remove(at)),
                                String::new(),
                            );
                        }
                        return Effect::None;
                    }
                    Choosing::FigureField(row) => {
                        let chosen = under
                            .and_then(|at| items.get(at))
                            .map(|(item, _)| item.clone());
                        if let (Some(chosen), Some(form)) = (chosen, &mut self.figure) {
                            form.set_field(row, chosen);
                        }
                        self.mode = Mode::Figure;
                        return Effect::None;
                    }
                }
                if let Err(error) = self.refresh() {
                    self.message = error;
                }
                return Effect::None;
            }
            _ => {}
        }
        self.mode = Mode::Picker {
            purpose,
            items,
            cursor,
            query,
            typing,
        };
        Effect::None
    }

    // -------------------------------------------------------------- sample

    /// The sample screen's lines: its attributes, its quantities, its tables.
    pub fn entries(&self) -> Vec<Entry> {
        let Some((path, sample)) = self.current() else {
            return Vec::new();
        };
        let vocabulary = self.collection.vocabulary();
        let subject = Subject {
            sample: &sample,
            path: Some(&path),
            vocabulary: &vocabulary,
            states: None,
        };
        let config = self.config_for(&path);
        let states = crate::format::fingerprint::check(&sample).unwrap_or_default();
        let owed = self.owed_by(&path);
        let owed_state = |field: &str| {
            owed.iter()
                .find(|owed| owed.value == field)
                .map(Owed::shown)
        };
        let mut entries = Vec::new();
        // Its name first, as its file writes it, edited as any attribute: the
        // file's name where it writes none.
        entries.push(Entry {
            field: "name".to_string(),
            shown: sample.name().unwrap_or("—").to_string(),
            state: if sample.written_name().is_none() {
                "the file's name".to_string()
            } else {
                String::new()
            },
            kind: EntryKind::Attribute,
        });
        // Its tags, first among what describes it rather than in its note.
        let tags: Vec<String> = sample.tags().iter().map(ToString::to_string).collect();
        entries.push(Entry {
            field: "tags".to_string(),
            shown: if tags.is_empty() {
                "—".to_string()
            } else {
                tags.join(", ")
            },
            state: String::new(),
            kind: EntryKind::Attribute,
        });
        // As the file holds them: its attributes first, then its quantities,
        // then its tables.
        for name in sample.attribute_names() {
            let shown = sample
                .attribute(name)
                .ok()
                .map(|value| match value.as_scalar() {
                    Some(value) => editing::shown_value(value),
                    None => value
                        .as_list()
                        .map(|items| {
                            items
                                .iter()
                                .map(editing::shown_value)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default(),
                })
                .unwrap_or_default();
            entries.push(Entry {
                field: name.to_string(),
                shown,
                state: String::new(),
                kind: EntryKind::Attribute,
            });
        }
        for name in sample.property_names() {
            let field = name.to_string();
            let profile = profile_of(std::slice::from_ref(&field));
            let shown = render::cell(
                &profile.columns()[0],
                &subject,
                render::Declaration::Profile(&profile),
                config,
                None,
                true,
            );
            entries.push(Entry {
                shown: if shown.is_empty() {
                    "—".to_string()
                } else {
                    shown
                },
                // Readings with no statistic chosen say so, where a value
                // entered alone says nothing — unless the model owes it: its
                // statistic is then the model's, never computed yet.
                state: states
                    .get(name)
                    .map(word)
                    .filter(|said| !said.is_empty())
                    .or_else(|| owed_state(&field))
                    .unwrap_or_else(|| {
                        if editing::kept_by(&sample, &field).is_some_and(|kept| !kept.statistic) {
                            NO_STATISTIC.to_string()
                        } else {
                            String::new()
                        }
                    }),
                field,
                kind: EntryKind::Quantity,
            });
        }
        // What the model would compute and the file does not hold yet: a
        // line of its own, so that `c` reaches it and its state is read.
        let held = |value: &str| {
            let head = value.split(['.', '[']).next().unwrap_or(value);
            entries.iter().any(|entry: &Entry| entry.field == head)
                || sample
                    .table_names()
                    .iter()
                    .any(|name| name.as_str() == head)
        };
        let absent: Vec<Entry> = owed
            .iter()
            .filter(|owed| !held(&owed.value))
            .map(|owed| Entry {
                field: owed.value.clone(),
                shown: "—".to_string(),
                state: owed.shown(),
                kind: EntryKind::Quantity,
            })
            .collect();
        entries.extend(absent);
        for name in sample.table_names() {
            let rows = sample
                .table(name)
                .map(|table| table.rows().count())
                .unwrap_or(0);
            let prefix = format!("{name}.");
            entries.push(Entry {
                field: name.to_string(),
                shown: format!("a table of {rows} rows  ▸"),
                // A column the model owes, said on its table's line.
                state: owed
                    .iter()
                    .find(|owed| owed.value.starts_with(&prefix))
                    .map(|owed| format!("{} {}", owed.value, owed.shown()))
                    .unwrap_or_default(),
                kind: EntryKind::Table,
            });
        }
        entries
    }

    /// What acts on the thing under the cursor, as its frame says it: a sample
    /// in the collection, a value — each kind its own keys — or a table's cell.
    /// A sample's note hidden, where to find it again.
    pub fn element_hints(&self) -> Vec<(&'static str, &'static str)> {
        let mut hints = self.hints_under_cursor();
        if self.note_hidden
            && matches!(self.screen, Screen::Sample { .. })
            && !matches!(self.mode, Mode::Note { .. })
        {
            hints.push(("n", "show note"));
        }
        hints
    }

    fn hints_under_cursor(&self) -> Vec<(&'static str, &'static str)> {
        // The note being edited takes every key: the value's said what they
        // no longer did.
        if matches!(self.mode, Mode::Note { .. }) {
            return Vec::new();
        }
        match self.screen {
            Screen::History { .. } => vec![("J K", "scroll what it changed")],
            Screen::Collection => {
                if self.view.is_empty() {
                    return Vec::new();
                }
                vec![
                    ("Enter", "open"),
                    ("Space", "basket"),
                    ("o", "files"),
                    ("E", "editor"),
                ]
            }
            Screen::Sample { .. } => match self.entry() {
                None => Vec::new(),
                Some(entry) => match entry.kind {
                    EntryKind::Attribute => vec![("e", "edit")],
                    EntryKind::Table => {
                        let mut hints = vec![("→", "unfold")];
                        if self.computable(&entry) {
                            hints.extend([("c", "compute"), ("Space", "select")]);
                        }
                        hints
                    }
                    EntryKind::Quantity => {
                        let mut hints = vec![("e", "edit"), ("Enter", "explain")];
                        if self.computable(&entry) {
                            hints.extend([("c", "compute"), ("Space", "select")]);
                        }
                        hints
                    }
                },
            },
            // Said as its body and its foot say them.
            Screen::Setup { .. } => {
                let asking = self
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.question < setup.questions(&self.root).len());
                if asking {
                    vec![("Enter", "answer"), ("Esc", "back")]
                } else {
                    vec![("Enter", "set up"), ("Esc", "back")]
                }
            }
            Screen::Configure => match &self.workspace {
                Some(workspace) if workspace.on_rows => {
                    vec![("Enter", "change"), ("a", "add"), ("d", "remove")]
                }
                Some(_) => vec![("→", "into it"), ("a", "add")],
                None => Vec::new(),
            },
            Screen::Control { .. } => {
                if self.control.is_empty() {
                    Vec::new()
                } else {
                    vec![("Enter", "open it")]
                }
            }
            Screen::Table { .. } => match self.cell() {
                None => Vec::new(),
                Some((cell, _)) => {
                    let mut hints = vec![("e", "edit"), ("Enter", "explain")];
                    if !cell.state.is_empty() {
                        hints.extend([("c", "compute"), ("Space", "select the column")]);
                    }
                    hints
                }
            },
        }
    }

    /// The value under the cursor: a line of the sample screen, or the cell of
    /// an unfolded table — as a value, so that it is explained and edited the
    /// same way.
    fn entry(&self) -> Option<Entry> {
        match self.screen {
            Screen::Sample { cursor, .. } => self.entries().into_iter().nth(cursor),
            Screen::Table { .. } => self.cell().map(|(entry, _)| entry),
            Screen::Collection
            | Screen::Control { .. }
            | Screen::History { .. }
            | Screen::Setup { .. }
            | Screen::Configure => None,
        }
    }

    /// The cell under the cursor of an unfolded table, and its column as
    /// `compute` names it: `conditioning.co2`.
    pub fn cell(&self) -> Option<(Entry, String)> {
        let Screen::Table {
            name,
            cursor,
            column,
            ..
        } = &self.screen
        else {
            return None;
        };
        let (path, sample) = self.current()?;
        let table = sample.table(name).ok()?;
        let column_name = value_columns(table).get(*column)?.clone();
        let row = table.rows().nth(*cursor)?;
        let index: Vec<crate::core::value::Value> = row.index().into_iter().cloned().collect();
        let field = cell_address(name, &column_name, index.clone());
        let derived = row
            .cell(&column_name)
            .is_ok_and(|cell| cell.records().computed.is_some());
        let state = if derived {
            crate::format::fingerprint::check_cell_among(&sample, name, &column_name, &index)
                .map(|state| word(&state))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let vocabulary = self.collection.vocabulary();
        let subject = Subject {
            sample: &sample,
            path: Some(&path),
            vocabulary: &vocabulary,
            states: None,
        };
        let profile = profile_of(std::slice::from_ref(&field));
        let shown = render::cell(
            &profile.columns()[0],
            &subject,
            render::Declaration::Profile(&profile),
            self.config_for(&path),
            None,
            true,
        );
        Some((
            Entry {
                field,
                shown,
                state,
                kind: EntryKind::Quantity,
            },
            format!("{name}.{column_name}"),
        ))
    }

    fn explain(&mut self) {
        let Some(entry) = self.entry() else {
            return;
        };
        let Some((path, sample)) = self.current() else {
            return;
        };
        let vocabulary = self.collection.vocabulary();
        let config = self.config_for(&path);
        let subject = Subject {
            sample: &sample,
            path: Some(&path),
            vocabulary: &vocabulary,
            states: None,
        };
        // A value as the sample screen shows it, and an input's the same way.
        let shown = |field: &str| {
            let profile = profile_of(&[field.to_string()]);
            let text = render::cell(
                &profile.columns()[0],
                &subject,
                render::Declaration::Profile(&profile),
                config,
                None,
                true,
            );
            if text.is_empty() {
                "—".to_string()
            } else {
                text
            }
        };
        let lines =
            match explanation::explain(&sample, Some(&path), config, &entry.field, &vocabulary) {
                Ok(explained) => {
                    let mut lines = Vec::new();
                    if matches!(explained, Explanation::Value(_)) {
                        lines.push(format!("{} = {}", entry.field, shown(&entry.field)));
                        lines.push(String::new());
                    }
                    lines.extend(explained_lines(&explained, &shown));
                    lines
                }
                Err(_) => vec!["nothing to explain here".to_string()],
            };
        drop(sample);
        self.mode = Mode::Panel {
            title: format!("{} — where it came from", entry.field),
            lines,
            scroll: 0,
        };
    }

    fn begin_edit(&mut self) {
        let Some(entry) = self.entry() else {
            return;
        };
        if entry.kind == EntryKind::Table {
            self.message = format!(
                "{} is a table: → opens it, where e changes a cell and + adds a row",
                entry.field
            );
            return;
        }
        let Some((path, sample)) = self.current() else {
            return;
        };
        if entry.field == "tags" && entry.kind == EntryKind::Attribute {
            let text = sample
                .tags()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            drop(sample);
            self.mode = Mode::prompt(Purpose::Tags { path }, text);
            return;
        }
        // Denoised: a computed `14.333333333333307` is typed over as
        // 14.3333333333, every figure typed kept. Left as it is, it is not
        // written, so nothing of the file is rounded by opening the window.
        let text_of = |field: &str| {
            editing::value_at(&sample, field)
                .map(|value| denoised(&value))
                .filter(|shown| shown != "—")
                .unwrap_or_default()
        };
        let text = text_of(&entry.field);
        // A quantity's value and uncertainty in one window, and its readings
        // where it is a property, not a table's cell.
        if entry.kind == EntryKind::Quantity {
            let unit = self.unit_of(&sample, &path, &entry.field);
            let mut slots = vec![
                Slot::of("value", text),
                Slot::of("uncertainty", text_of(&format!("{}.u", entry.field))),
            ];
            for slot in &mut slots {
                slot.unit = unit.clone();
            }
            if !entry.field.contains('[') {
                let readings = Identifier::new(&entry.field)
                    .ok()
                    .and_then(|name| sample.property(&name).ok())
                    .and_then(|handle| handle.readings())
                    .map(|readings| {
                        readings
                            .as_slice()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                slots.push(Slot::of("readings", readings));
            }
            drop(sample);
            slots[0].text.focus(true);
            self.mode = Mode::Quantity {
                path,
                field: entry.field,
                slots,
                on: 0,
            };
            return;
        }
        drop(sample);
        self.mode = Mode::prompt(
            Purpose::Edit {
                path,
                field: entry.field,
            },
            text,
        );
    }

    /// The unit a value is in: its own, its column's for a table's cell, or
    /// the one the project declares for it.
    fn unit_of(&self, sample: &Sample, path: &Path, field: &str) -> String {
        let own = match (field.split_once('.'), &self.screen) {
            (Some((table, column)), Screen::Table { .. }) => {
                let column = column.split('[').next().unwrap_or(column);
                Identifier::new(table)
                    .ok()
                    .zip(Identifier::new(column).ok())
                    .and_then(|(table, column)| {
                        let table = sample.table(&table).ok()?;
                        table.column(&column).ok()?.presentation().unit.clone()
                    })
            }
            _ => Identifier::new(field)
                .ok()
                .and_then(|name| sample.property(&name).ok())
                .and_then(|handle| handle.presentation().unit),
        };
        own.or_else(|| {
            self.config_for(path)
                .and_then(|config| config.property(field))
                .and_then(|declared| declared.unit.clone())
        })
        .unwrap_or_default()
    }

    fn prompting(&mut self, key: Key, purpose: Purpose, mut line: Typed) -> Effect {
        match key {
            Key::Esc if matches!(purpose, Purpose::FigureText(_)) => {
                self.mode = Mode::Figure;
                return Effect::None;
            }
            Key::Esc => return Effect::None,
            Key::Enter => {
                // A line refused keeps its window, its text and what is said,
                // as the quantity's window does: closed, the text was lost,
                // and what was typed again fell on the screen as its keys.
                let again = purpose.clone();
                self.message.clear();
                let text = line.text().to_string();
                let accepted = match purpose {
                    Purpose::DeclareName(what) => {
                        self.preview_declare(&what, text.trim());
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::FigureText(row) => {
                        if let Some(form) = &mut self.figure {
                            form.set_text(row, text.trim());
                        }
                        self.mode = Mode::Figure;
                        true
                    }
                    Purpose::Edit { path, field } => {
                        self.preview_edit(path, &field, text.trim());
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Tag => {
                        self.preview_tag(&text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Tags { path } => {
                        self.preview_tags(path, &text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::New { like } => {
                        self.preview_new(like, &text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Setting { section, name, key } => {
                        self.set_setting(section, name, &key, &text)
                    }
                    Purpose::AddSetting { section } => self.add_setting(section, &text),
                    Purpose::AddKey { section, name } => self.add_key(section, name, &text),
                    Purpose::Row { path, table } => {
                        self.preview_row(path, &table, &text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::ModelPath => self.answer_model_path(&text),
                };
                if !accepted && !self.message.is_empty() && matches!(self.mode, Mode::Normal) {
                    self.mode = Mode::Prompt {
                        purpose: again,
                        text: line,
                    };
                }
                return Effect::None;
            }
            _ => {
                line.key(key);
            }
        }
        self.mode = Mode::Prompt {
            purpose,
            text: line,
        };
        Effect::None
    }

    /// The change as `set` previews it, before anything is written: what it
    /// becomes, and what it makes not current.
    fn preview_edit(&mut self, path: PathBuf, field: &str, text: &str) {
        self.preview_quantity(path, field, None, &[(field.to_string(), text.to_string())]);
    }

    /// Several assignments to one sample — new readings first, as `set
    /// --readings` writes them — previewed together as `set` does.
    fn preview_quantity(
        &mut self,
        path: PathBuf,
        field: &str,
        readings: Option<String>,
        assignments: &[(String, String)],
    ) {
        let outcome = (|| -> Result<(Vec<String>, Pending), String> {
            let (mut sample, origin) =
                document::load_sample(&path).map_err(|error| error.to_string())?;
            let before: Vec<String> = editing::not_current(&sample)
                .into_iter()
                .map(|entry| entry.name)
                .collect();
            // At the precision the project declares, as the screen shows it,
            // and every digit where it declares none.
            let config = self.config_for(&path);
            let shown_of = |field: &str, value: &Option<crate::core::value::Value>| {
                value.as_ref().map_or("—".to_string(), |value| {
                    editing::previewed(value, field, config)
                })
            };
            let mut lines = Vec::new();
            let mut unchanged = Vec::new();
            if let Some(text) = &readings {
                if text.trim().is_empty() {
                    return Err(format!(
                        "{field}'s readings are not taken away here: its file holds them"
                    ));
                }
                let name = Identifier::new(field).map_err(|error| error.to_string())?;
                let parsed = editing::readings_of(field, text).map_err(|error| error.message)?;
                let count = parsed.as_slice().len();
                // A value written by a hand stays beside new readings, and
                // outranks their statistic: said as `set --readings` says it,
                // not as their statistic becoming the value.
                let statistic = sample
                    .property(&name)
                    .is_ok_and(|handle| handle.records().computed.is_some());
                let edited = matches!(
                    crate::format::fingerprint::check_property(&sample, &name),
                    Ok(Freshness::Edited)
                );
                let kept = sample.property(&name).ok().and_then(|handle| {
                    handle.peek(|property| {
                        let written = property
                            .written_value()
                            .filter(|_| !statistic || edited || property.is_edited())
                            .cloned();
                        match (property.readings(), written, property.peek_value()) {
                            (Some(_), Some(written), _) => Some(written),
                            (None, _, Some(value)) if !value.is_absent() => Some(value),
                            _ => None,
                        }
                    })
                });
                editing::set_readings(&mut sample, &name, parsed).map_err(|error| error.message)?;
                lines.push(format!("{field}.readings   {text}"));
                let value_typed = assignments.iter().any(|(assigned, _)| assigned == field);
                lines.push(match kept {
                    Some(kept) if !value_typed => format!(
                        "  {count} readings; the value {} stays, written: it outranks their \
                         statistic — an empty value lets them give it",
                        editing::previewed(&kept, field, config)
                    ),
                    _ if value_typed => {
                        format!("  {count} readings; the value written beside them outranks them")
                    }
                    _ if statistic => format!(
                        "  {count} readings; their statistic is outdated until c computes it again"
                    ),
                    _ => format!("  {count} readings; their statistic is its value"),
                });
            }
            for (field, text) in assignments {
                let value = editing::typed(field, text).map_err(|error| error.message)?;
                // As `set` knows it: `.samplekitrc`, the model's description,
                // then the other samples.
                let known = editing::known_as(
                    &sample,
                    field,
                    self.config_for(&path),
                    Some(&self.collection),
                );
                if let Some(known) = &known
                    && let Some(held) = &known.held
                    && held.numeric
                    && matches!(
                        value,
                        crate::core::value::Value::Text(_) | crate::core::value::Value::Boolean(_)
                    )
                {
                    return Err(editing::not_a_number_for(
                        field,
                        known.declaration.unit.as_deref(),
                        &value,
                    )
                    .message);
                }
                let change = editing::apply_known(&mut sample, field, value, known.as_ref())
                    .map_err(|error| error.message)?;
                let (before, after) = editing::previewed_change(
                    change.before.as_ref(),
                    change.after.as_ref(),
                    field,
                    config,
                );
                match change.became {
                    Became::Unchanged => {
                        unchanged.push(format!(
                            "{field} is already {}",
                            shown_of(field, &change.after)
                        ));
                        continue;
                    }
                    // A name no longer written: the file's stands for it.
                    Became::Cleared if field == "name" => lines.push(format!(
                        "{field}   {before}  →  {after}, the file's name, none written"
                    )),
                    Became::Cleared => lines.push(format!("{field}   cleared")),
                    _ => lines.push(format!("{field}   {before}  →  {after}")),
                }
                match change.became {
                    Became::Override => lines.push("  an override: the formula stays".to_string()),
                    Became::NewQuantity => lines.push(
                        match known.clone().map(|known| known.from) {
                            Some(editing::KnownFrom::Model(_)) => {
                                "  new, a quantity the model declares"
                            }
                            Some(editing::KnownFrom::Samples) => {
                                "  new, a quantity as the other samples hold it"
                            }
                            _ => "  new, declared in .samplekitrc",
                        }
                        .to_string(),
                    ),
                    Became::NewAttribute { .. } => lines.push("  new, an attribute".to_string()),
                    _ => {}
                }
                // Two units for one name: `.samplekitrc`'s is written.
                if change.became == Became::NewQuantity
                    && let Some((configured, modelled)) =
                        known.as_ref().and_then(|known| known.disagreement.clone())
                {
                    lines.push(format!(
                        "  in {configured} by .samplekitrc and in {modelled} by the model: \
                         written in {configured}"
                    ));
                }
                if let Some(kept) = &change.kept {
                    lines.push(format!("  keeps its {} readings", kept.readings.len()));
                }
            }
            if lines.is_empty() {
                return Err(unchanged.join(" · "));
            }
            let newly: Vec<String> = editing::not_current(&sample)
                .into_iter()
                .filter(|entry| !before.contains(&entry.name))
                .map(|entry| format!("  {}   {}", entry.name, word(&entry.state)))
                .collect();
            if newly.is_empty() {
                lines.push(String::new());
                lines.push("nothing more becomes not current".to_string());
            } else {
                lines.push(String::new());
                lines.push(format!("this makes {} not current:", newly.len()));
                lines.extend(newly);
            }
            Ok((
                lines,
                Pending::Write {
                    path: path.clone(),
                    sample: Box::new(sample),
                    origin,
                },
            ))
        })();
        match outcome {
            Ok((lines, pending)) => {
                self.mode = Mode::Confirm {
                    title: "change".to_string(),
                    lines,
                    pending: Box::new(pending),
                }
            }
            Err(message) => self.message = message,
        }
    }

    fn quantity(
        &mut self,
        key: Key,
        path: PathBuf,
        field: String,
        mut slots: Vec<Slot>,
        mut on: usize,
    ) -> Effect {
        match key {
            Key::Esc => return Effect::None,
            Key::Tab | Key::Down => {
                on = (on + 1) % slots.len();
                for (at, slot) in slots.iter_mut().enumerate() {
                    slot.text.focus(at == on);
                }
            }
            Key::BackTab | Key::Up => {
                on = (on + slots.len() - 1) % slots.len();
                for (at, slot) in slots.iter_mut().enumerate() {
                    slot.text.focus(at == on);
                }
            }
            Key::Enter => {
                // Only what changed is written: an untouched value beside
                // readings stays theirs. New readings go first, so that a
                // value written with them is the value.
                let slot = |label: &str| slots.iter().find(|slot| slot.label == label);
                let readings = slot("readings")
                    .filter(|slot| slot.changed())
                    .map(|slot| slot.text.text().to_string());
                let mut assignments = Vec::new();
                if let Some(value) = slot("value").filter(|slot| slot.changed()) {
                    assignments.push((field.clone(), value.text.text().to_string()));
                }
                if let Some(spread) = slot("uncertainty").filter(|slot| slot.changed()) {
                    assignments.push((format!("{field}.u"), spread.text.text().to_string()));
                }
                // A number is written alone, its decimals after a point: `21 L`
                // and `21,5` read as text, refused far from what was meant.
                let refused = slots
                    .iter()
                    .filter(|slot| slot.label != "readings" && slot.changed())
                    .find_map(|slot| number_refused(slot.text.text().trim(), &slot.unit));
                // Said in the window, which stays open to correct it.
                if let Some(refused) = refused {
                    self.message = refused;
                    self.mode = Mode::Quantity {
                        path,
                        field,
                        slots,
                        on,
                    };
                } else if assignments.is_empty() && readings.is_none() {
                    self.message = "nothing changed".to_string();
                } else {
                    self.preview_quantity(path, &field, readings, &assignments);
                }
                return Effect::None;
            }
            // A click on another field types in it.
            Key::Click(x, y) => {
                if let Some(at) = slots
                    .iter_mut()
                    .position(|slot| slot.text.state().inner.contains((x, y).into()))
                {
                    on = at;
                }
                for (at, slot) in slots.iter_mut().enumerate() {
                    slot.text.focus(at == on);
                }
                slots[on].text.key(key);
            }
            _ => {
                slots[on].text.key(key);
            }
        }
        self.mode = Mode::Quantity {
            path,
            field,
            slots,
            on,
        };
        Effect::None
    }

    /// A sample's tags as written — by commas or spaces — previewed: the tags
    /// it loses and those it gains.
    fn preview_tags(&mut self, path: PathBuf, text: &str) {
        let mut tags: Vec<Identifier> = Vec::new();
        for word in text.split([',', ' ']).filter(|word| !word.is_empty()) {
            match Identifier::new(word) {
                Ok(tag) if !tags.contains(&tag) => tags.push(tag),
                Ok(_) => {}
                Err(error) => {
                    self.message = format!("'{word}' is not a usable tag: {error}");
                    return;
                }
            }
        }
        let (mut sample, origin) = match document::load_sample(&path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let before = sample.tags().to_vec();
        let mut lines: Vec<String> = before
            .iter()
            .filter(|tag| !tags.contains(tag))
            .map(|tag| format!("- {tag}"))
            .collect();
        lines.extend(
            tags.iter()
                .filter(|tag| !before.contains(tag))
                .map(|tag| format!("+ {tag}")),
        );
        if lines.is_empty() {
            self.message = "the tags are unchanged".to_string();
            return;
        }
        sample.set_tags(tags);
        self.mode = Mode::Confirm {
            title: "tags".to_string(),
            lines,
            pending: Box::new(Pending::Write {
                path,
                sample: Box::new(sample),
                origin,
            }),
        };
    }

    fn preview_tag(&mut self, text: &str) {
        // `-tag` takes it off.
        let (removing, text) = match text.trim().strip_prefix('-') {
            Some(tag) => (true, tag),
            None => (false, text.trim()),
        };
        let tag = match Identifier::new(text) {
            Ok(tag) => tag,
            Err(error) => {
                self.message = format!("'{text}' is not a usable tag: {error}");
                return;
            }
        };
        let paths: Vec<PathBuf> = self.basket.iter().cloned().collect();
        let selection = match list::from_files(&paths) {
            Ok(selection) => selection,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let edit = if removing {
            tagging::Edit::Remove(tag.clone())
        } else {
            tagging::Edit::Add(tag.clone())
        };
        let plan = tagging::plan(&selection, edit);
        let mut lines = vec![format!(
            "'{tag}' {} {} of the basket's {}",
            if removing { "off" } else { "on" },
            plan.changes.len(),
            counted(paths.len(), "sample", "samples")
        )];
        for change in &plan.changes {
            lines.push(format!("  {}", self.here(&change.path)));
        }
        self.mode = Mode::Confirm {
            title: "tag".to_string(),
            lines,
            pending: Box::new(Pending::Tag(plan)),
        };
    }

    fn confirming(
        &mut self,
        key: Key,
        title: String,
        lines: Vec<String>,
        pending: Pending,
    ) -> Effect {
        match key {
            // Files a computation is writing are written by nothing else
            // meanwhile: its save would refuse theirs, or undo mix the two.
            Key::Char('y') if self.running.is_some() && !matches!(pending, Pending::Quit) => {
                self.confirm_scroll = 0;
                self.message =
                    "a computation is writing: write this once it has finished".to_string();
                if let Pending::Undo(group) = pending {
                    self.undo.push(group);
                }
            }
            Key::Char('y') => {
                self.confirm_scroll = 0;
                // What the files were, for `u`: kept once the change is written.
                let touched: Vec<PathBuf> = match &pending {
                    Pending::Write { path, .. }
                    | Pending::New { path, .. }
                    | Pending::Export { path, .. } => vec![path.clone()],
                    Pending::Tag(plan) => plan
                        .changes
                        .iter()
                        .map(|change| change.path.clone())
                        .collect(),
                    Pending::Remove(paths) => paths.clone(),
                    Pending::Declare { edit, .. } => vec![edit.path().to_path_buf()],
                    // Not kept for `u`: an undo is not undone.
                    Pending::Undo(_) | Pending::Quit | Pending::Compute(_) => Vec::new(),
                    Pending::Configuration => self
                        .workspace
                        .as_ref()
                        .map(|workspace| vec![workspace.edit.path().to_path_buf()])
                        .unwrap_or_default(),
                    // The files it will write; an environment is not undone.
                    Pending::Setup => self
                        .setup
                        .as_ref()
                        .map(|setup| {
                            setup
                                .plan(&self.root)
                                .missing()
                                .filter_map(|proposal| match &proposal.step {
                                    Step::File { path, .. } => Some(path.clone()),
                                    Step::Directory { .. } | Step::Environment { .. } => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                let before = snapshot(&touched);
                // What changed since the history's last snapshot is one of its
                // own, so this change is credited with its own. An undo's files
                // are its places in the history all the same.
                let reached: Vec<PathBuf> = match &pending {
                    Pending::Undo(group) => {
                        group.iter().map(|undone| undone.path.clone()).collect()
                    }
                    _ => touched.clone(),
                };
                let (writing, unread) = version_control::before_writing(&self.places(&reached));
                let mut wrote = false;
                self.message = match pending {
                    Pending::Write {
                        path,
                        mut sample,
                        origin,
                    } => match document::save_sample(&mut sample, &Destination::Origin(origin)) {
                        // As the command line says it, from the folder the
                        // workbench is on: a whole path wrapped over three
                        // lines of the message.
                        Ok(_) => {
                            wrote = true;
                            format!("written: {}", self.here(&path))
                        }
                        Err(error) => error.to_string(),
                    },
                    Pending::Quit => {
                        let under_way = self.under_way();
                        if under_way.is_empty() {
                            return Effect::Quit;
                        }
                        self.leaving = true;
                        self.message = format!(
                            "leaving once it has ended — {} · any key stays",
                            under_way.join(" · ")
                        );
                        return Effect::None;
                    }
                    Pending::Compute(computing) => {
                        self.compute(computing);
                        return Effect::None;
                    }
                    Pending::Setup => {
                        let said = self.apply_setup();
                        wrote = !said.contains("nothing was written");
                        said
                    }
                    Pending::Configuration => match self
                        .workspace
                        .as_mut()
                        .map(|workspace| workspace.edit.write())
                    {
                        Some(Ok(_)) => {
                            wrote = true;
                            let path = self
                                .workspace
                                .as_ref()
                                .map(|workspace| workspace.edit.path().to_path_buf())
                                .unwrap_or_default();
                            // Read again, so that the next change is against what
                            // the file now holds.
                            if let (Some(workspace), Ok(edit)) =
                                (self.workspace.as_mut(), ConfigurationEdit::open(&path))
                            {
                                workspace.edit = edit;
                                workspace.dirty = false;
                            }
                            format!("written: {}", self.here(&path))
                        }
                        Some(Err(error)) => error.to_string(),
                        None => String::new(),
                    },
                    Pending::Declare { edit, said } => match edit.write() {
                        Ok(_) => {
                            wrote = true;
                            format!("{said} · u takes it back")
                        }
                        Err(error) => error.to_string(),
                    },
                    Pending::Remove(paths) => {
                        let mut removed = 0;
                        let mut failed = None;
                        for path in &paths {
                            match std::fs::remove_file(path) {
                                Ok(()) => removed += 1,
                                Err(error) => {
                                    failed = Some(format!("{}: {error}", self.here(path)));
                                    break;
                                }
                            }
                        }
                        wrote = removed > 0;
                        for path in &paths {
                            self.basket.remove(path);
                        }
                        // Removed from its own screen, the collection is where
                        // it goes, said as the collection says a removal: the
                        // reload said it was "no longer among those shown".
                        let open = match self.screen {
                            Screen::Sample { at, .. } | Screen::Table { at, .. } => {
                                self.view.get(at).and_then(|entry| entry.path.clone())
                            }
                            _ => None,
                        };
                        if open.is_some_and(|open| paths.contains(&open)) {
                            self.screen = Screen::Collection;
                        }
                        let said = format!(
                            "removed {} · u gives {} back",
                            counted(removed, "sample", "samples"),
                            if removed == 1 { "it" } else { "them" }
                        );
                        match failed {
                            Some(failed) => format!("{said} · stopped at {failed}"),
                            None => said,
                        }
                    }
                    Pending::New { path, mut sample } => {
                        match document::save_sample(&mut sample, &Destination::Path(path.clone())) {
                            Ok(_) => {
                                wrote = true;
                                format!("created: {}", self.here(&path))
                            }
                            Err(error) => error.to_string(),
                        }
                    }
                    Pending::Undo(group) => {
                        // Checked again now: an edit made while the question
                        // was open is not overwritten either.
                        if let Some(moved) = group.iter().find(|undone| {
                            std::fs::read_to_string(&undone.path).ok() != undone.after
                        }) {
                            let said =
                                format!("{} changed meanwhile: not undone", self.here(&moved.path));
                            self.undo.push(group);
                            self.message = said;
                            return Effect::None;
                        }
                        let mut failed = None;
                        for undone in &group {
                            // Whole or not at all: a file cut halfway by a
                            // failure is worse than the change it undid. And
                            // read again inside the lock that covers the write,
                            // `.samplekitrc` as a sample: an edit made since
                            // the check above is not lost.
                            let done = document::replace_if_unchanged(
                                &undone.path,
                                undone.after.as_deref().map(str::as_bytes),
                                undone.before.as_deref().map(str::as_bytes),
                            );
                            match done {
                                // Kept in the history as an undo, which is
                                // never offered back: unsaid, `log` read it
                                // as a change made outside, and the next `u`
                                // offered the whole snapshot before it.
                                Ok(()) => wrote = true,
                                Err(document::DocumentError::Io { source, .. }) => {
                                    failed = Some(format!("{}: {source}", self.here(&undone.path)))
                                }
                                // Another writer's change names the file itself.
                                Err(error) => failed = Some(error.to_string()),
                            }
                        }
                        failed.unwrap_or_else(|| {
                            format!(
                                "undone: {} as {} {}",
                                counted(group.len(), "file", "files"),
                                if group.len() == 1 { "it" } else { "they" },
                                if group.len() == 1 { "was" } else { "were" }
                            )
                        })
                    }
                    Pending::Export {
                        text,
                        path,
                        rows,
                        name,
                        samples,
                    } => {
                        let made = path
                            .parent()
                            .filter(|parent| !parent.as_os_str().is_empty())
                            .map_or(Ok(()), std::fs::create_dir_all)
                            .map_err(|error| error.to_string())
                            .and_then(|()| {
                                crate::collection::exports::write(&text, &path, true)
                                    .map_err(|error| error.to_string())
                            });
                        match made {
                            Ok(()) => {
                                wrote = true;
                                let said = format!(
                                    "{} written to {}",
                                    counted(rows, "row", "rows"),
                                    self.here(&path)
                                );
                                match keep_output(&path, &format!("tui · export {name}"), &samples)
                                {
                                    Some(warning) => format!("{said} — {warning}"),
                                    None => said,
                                }
                            }
                            Err(error) => error,
                        }
                    }
                    Pending::Tag(plan) => match tagging::apply(&plan) {
                        Ok(written) => {
                            wrote = true;
                            format!(
                                "{} {}",
                                counted(written.len(), "sample", "samples"),
                                match plan.edit {
                                    tagging::Edit::Remove(_) => "untagged",
                                    _ => "tagged",
                                }
                            )
                        }
                        // Those written before the failure are said, and undone
                        // with the rest of what was written.
                        Err(error) => {
                            wrote = true;
                            error.to_string()
                        }
                    },
                };
                if wrote {
                    let changed = settled(before);
                    if !changed.is_empty() {
                        self.undo.push(changed);
                    }
                    // Its paths from the workbench's folder, as the history
                    // names them.
                    // An undo says so as the history reads it: never offered
                    // back as a change of its own.
                    // The history names a change after its title's colon, as
                    // it always has: `change: written C1.md`.
                    let said = if title.starts_with("undo") {
                        format!("tui · {}{}", version_control::UNDO, self.message)
                    } else {
                        let message = self.message.replacen("written: ", "written ", 1).replacen(
                            "created: ",
                            "created ",
                            1,
                        );
                        format!("tui · {title}: {message}")
                    }
                    .replace(&format!("{}/", self.root.display()), "");
                    let failures = version_control::after_writing(writing, &said);
                    if let Some(warning) = history_warning(unread.into_iter().chain(failures)) {
                        self.message = format!("{} — {warning}", self.message);
                    }
                }
                self.reload();
            }
            // An undo declined is still there to take.
            // A confirmation longer than its window is read to its end first.
            Key::Char('j') | Key::Down => {
                // Rows, once wrapped, may outnumber lines: the window stops at
                // the end whatever this says.
                let rows: usize = lines.iter().map(|line| line.chars().count() / 20 + 1).sum();
                self.confirm_scroll = (self.confirm_scroll + 1).min(rows);
                self.mode = Mode::Confirm {
                    title,
                    lines,
                    pending: Box::new(pending),
                };
                return Effect::None;
            }
            Key::Char('k') | Key::Up => {
                self.confirm_scroll = self.confirm_scroll.saturating_sub(1);
                self.mode = Mode::Confirm {
                    title,
                    lines,
                    pending: Box::new(pending),
                };
                return Effect::None;
            }
            Key::Char('n') | Key::Esc => {
                self.confirm_scroll = 0;
                // Staying lets a Ctrl+C go: the next is a first again.
                if matches!(pending, Pending::Quit) {
                    self.interrupted = false;
                }
                if let Pending::Undo(group) = pending {
                    self.undo.push(group);
                }
                self.message = "nothing written".to_string();
            }
            _ => {
                self.mode = Mode::Confirm {
                    title,
                    lines,
                    pending: Box::new(pending),
                }
            }
        }
        Effect::None
    }

    // ---------------------------------------------------------- new, rows

    /// A new sample shaped like the one under the cursor, beside it — as `new
    /// --like` shapes one — previewed before it is written.
    fn preview_new(&mut self, like: Option<PathBuf>, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.message = "a new sample needs a name".to_string();
            return;
        }
        // Checked as `new` checks it, in the project of the sample it is
        // shaped like, beside which it is written — or, the first, in this
        // one's samples/ where there is one.
        let project = match &like {
            Some(like) => self.collection.config_for(like),
            None => self.collection.config(),
        }
        .map(|config| config.root().to_path_buf());
        if let Some(refused) = editing::refuses_name(name, &self.collection, project.as_deref()) {
            self.message = refused.message;
            return;
        }
        let pattern = match like.as_deref().map(document::load_sample) {
            None => None,
            Some(Ok((pattern, _))) => Some(pattern),
            Some(Err(error)) => {
                self.message = error.to_string();
                return;
            }
        };
        let shaped = match editing::shaped_like(name, pattern.as_ref(), &[]) {
            Ok(shaped) => shaped,
            Err(error) => {
                self.message = error.message;
                return;
            }
        };
        let directory = match &like {
            Some(like) => like.parent().unwrap_or(Path::new(".")).to_path_buf(),
            None => {
                let base = project.clone().unwrap_or_else(|| self.root.clone());
                let samples = base.join("samples");
                if samples.is_dir() { samples } else { base }
            }
        };
        let path = directory.join(format!("{name}.md"));
        if path.exists() {
            self.message = format!("{}: already here", self.here(&path));
            return;
        }
        let mut lines = vec![match &like {
            Some(like) => format!(
                "{}, shaped like {}",
                self.here(&path),
                like.file_stem().unwrap_or_default().to_string_lossy()
            ),
            None => format!(
                "{}: named by its file, the values it holds for e and the model to give",
                self.here(&path)
            ),
        }];
        if !shaped.measured.is_empty() {
            lines.push(format!("  to fill in    {}", shaped.measured.join(", ")));
        }
        if !shaped.attributes.is_empty() {
            let carried: Vec<String> = shaped
                .attributes
                .iter()
                .map(|(name, value)| format!("{name} = {value}"))
                .collect();
            lines.push(format!("  carried       {}", carried.join(", ")));
        }
        if !shaped.not_carried.is_empty() {
            lines.push(format!(
                "  not carried   {}: the new sample's own to write",
                shaped.not_carried.join(", ")
            ));
        }
        if !shaped.tables.is_empty() {
            lines.push(format!("  tables, empty {}", shaped.tables.join(", ")));
        }
        if !shaped.left_out.is_empty() {
            lines.push(format!(
                "  left out, a formula gives them: {}",
                shaped.left_out.join(", ")
            ));
        }
        self.mode = Mode::Confirm {
            title: "new sample".to_string(),
            lines,
            pending: Box::new(Pending::New {
                path,
                sample: Box::new(shaped.sample),
            }),
        };
    }

    /// A row added to the unfolded table, from `column=value, …`, as `set
    /// --add-row` adds one: previewed with what it leaves empty and what the
    /// model will fill.
    fn preview_row(&mut self, path: PathBuf, table: &str, text: &str) {
        let outcome = (|| -> Result<(Vec<String>, Pending), String> {
            let mut cells: Vec<(String, crate::core::value::Value)> = Vec::new();
            let mut last: Option<String> = None;
            for written in text
                .split(',')
                .map(str::trim)
                .filter(|written| !written.is_empty())
            {
                // Digits alone after a cell whose value is a number: what
                // follows a decimal comma, `gravity=1,011`, which read as
                // '011', no change of anything.
                if !written.contains('=')
                    && written.chars().all(|c| c.is_ascii_digit())
                    && let Some(previous) = &last
                    && previous
                        .split_once('=')
                        .is_some_and(|(_, value)| value.trim().parse::<f64>().is_ok())
                {
                    return Err(format!(
                        "'{previous},{written}' reads as a decimal comma: a row's cells are \
                         separated by commas and use a point for decimals — {}.{written}",
                        previous.trim()
                    ));
                }
                last = Some(written.to_string());
                let (column, value) =
                    editing::split_assignment(written).map_err(|error| error.message)?;
                if value.contains('=') {
                    return Err(format!(
                        "'{written}' holds two cells: a row is written column=value, separated by commas"
                    ));
                }
                let value = editing::value_of(&value).map_err(|error| error.message)?;
                cells.push((column, value));
            }
            if cells.is_empty() {
                return Err("a row is written column=value, …: its index first".to_string());
            }
            let (mut sample, origin) =
                document::load_sample(&path).map_err(|error| error.to_string())?;
            let added = editing::add_row(&mut sample, table, cells, |name| {
                editing::shape_from(&self.collection, &path, name)
            })
            .map_err(|error| error.message)?;
            let given: Vec<String> = added
                .given
                .iter()
                .map(|(column, value, over)| {
                    format!(
                        "{column} = {}{}",
                        editing::shown_value(value),
                        if *over { " (over the model's)" } else { "" }
                    )
                })
                .collect();
            let mut lines = vec![format!("a row in {table}: {}", given.join(", "))];
            let named = |names: &[Identifier]| {
                names
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if !added.left_absent.is_empty() {
                lines.push(format!(
                    "  left empty          {}",
                    named(&added.left_absent)
                ));
            }
            if !added.by_the_model.is_empty() {
                lines.push(format!(
                    "  the model fills     {}",
                    named(&added.by_the_model)
                ));
            }
            Ok((
                lines,
                Pending::Write {
                    path: path.clone(),
                    sample: Box::new(sample),
                    origin,
                },
            ))
        })();
        match outcome {
            Ok((lines, pending)) => {
                self.mode = Mode::Confirm {
                    title: "row".to_string(),
                    lines,
                    pending: Box::new(pending),
                }
            }
            Err(message) => self.message = message,
        }
    }

    // ---------------------------------------------------------------- undo

    /// The last change the workbench wrote, offered back: each file as it was
    /// before, one it created removed.
    fn begin_undo(&mut self) {
        // A computation still writing would mix its values with the files put
        // back.
        if self.running.is_some() {
            self.message = "a computation is running: undo once it has finished".to_string();
            return;
        }
        // Past this session's changes, the history's: an undo that outlives the
        // workbench.
        let mut from_history = None;
        let group = match self.undo.pop() {
            Some(group) => group,
            None => match self.undoable_from_history() {
                Some((group, said)) => {
                    from_history = Some(said);
                    group
                }
                None => {
                    self.message = "nothing to undo".to_string();
                    return;
                }
            },
        };
        // A file changed since — by hand, in an editor — is not overwritten
        // with an older text: the change is not undone, and stays offered.
        if let Some(moved) = group
            .iter()
            .find(|undone| std::fs::read_to_string(&undone.path).ok() != undone.after)
        {
            // Asked again, it is let go: kept, it barred every older change.
            if self.undo_refused {
                self.undo_refused = false;
                self.message = format!(
                    "let go: {} changed since; u now offers the change before it",
                    self.here(&moved.path)
                );
                return;
            }
            self.undo_refused = true;
            self.message = format!(
                "{} changed since the last change was written: not undone — u again lets it go",
                self.here(&moved.path)
            );
            self.undo.push(group);
            return;
        }
        self.undo_refused = false;
        let mut lines: Vec<String> = from_history.into_iter().collect();
        for undone in &group {
            let name = self.here(&undone.path);
            match (&undone.before, &undone.after) {
                // What goes back, value by value, as the history says a change:
                // now, then as it was.
                (Some(before), Some(after)) => {
                    lines.push(format!("restore {name}"));
                    if let Some(config) = self
                        .config_for(&undone.path)
                        .or_else(|| self.collection.config())
                    {
                        lines.extend(
                            crate::presentation::changes::changes_of(
                                &name,
                                Some(&after.clone().into_bytes()),
                                Some(&before.clone().into_bytes()),
                                config,
                            )
                            .into_iter()
                            .skip(1),
                        );
                    }
                }
                (Some(_), None) => lines.push(format!("restore {name}, which it removed")),
                (None, _) => lines.push(format!("remove {name}, which it created")),
            }
        }
        self.mode = Mode::Confirm {
            title: "undo the last change".to_string(),
            lines,
            pending: Box::new(Pending::Undo(group)),
        };
    }

    // ------------------------------------------------------------- control

    /// The collection's state, as `status` and `validate` say it: each value
    /// not current, then each defect and note, sample by sample.
    fn open_control(&mut self) {
        let mut items = Vec::new();
        for entry in self.collection.iter() {
            let sample = entry.sample.borrow();
            let name = sample.name().unwrap_or("(unnamed)").to_string();
            for value in editing::not_current(&sample) {
                let kind = match &value.state {
                    Freshness::Failed { .. } => Concern::Failed,
                    Freshness::Stale { .. } | Freshness::Broken { .. } => Concern::Stale,
                    _ => Concern::Edited,
                };
                let rows = if value.rows > 0 {
                    format!(" ({})", counted(value.rows, "row", "rows"))
                } else {
                    String::new()
                };
                items.push(ControlItem {
                    path: entry.path.clone(),
                    sample: name.clone(),
                    field: Some(value.name.clone()),
                    said: format!("{}  {}{rows}", value.name, word(&value.state)),
                    kind,
                });
            }
        }
        // What the model owes, as `status` lists it.
        for entry in self.collection.iter() {
            let Some(path) = &entry.path else {
                continue;
            };
            let name = entry
                .sample
                .borrow()
                .name()
                .unwrap_or("(unnamed)")
                .to_string();
            for owed in self.owed_by(path) {
                items.push(ControlItem {
                    path: Some(path.clone()),
                    sample: name.clone(),
                    field: Some(owed.value.clone()),
                    said: format!("{}  {}", owed.value, owed.shown()),
                    kind: Concern::Owed,
                });
            }
        }
        let report = crate::collection::validation::run(&self.collection);
        for finding in &report.findings {
            // Said value by value above: a count of them would say it twice.
            if matches!(
                finding.detail,
                crate::collection::validation::Detail::NotCurrent { .. }
                    | crate::collection::validation::Detail::Overridden { .. }
            ) {
                continue;
            }
            items.push(ControlItem {
                path: finding.path.clone(),
                sample: finding.sample.clone(),
                field: finding.detail.quantity().map(str::to_string),
                said: crate::collection::validation::described(finding)
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join(" — "),
                kind: match finding.severity {
                    crate::collection::validation::Severity::Defect => Concern::Defect,
                    _ => Concern::Note,
                },
            });
        }
        // Worst first, and a sample's together within each.
        items.sort_by_key(|item| (item.kind, item.sample.clone()));
        self.control = items;
        self.screen = Screen::Control { cursor: 0 };
        // Nothing to report is said by the screen's title, once.
    }

    /// The sample a line of the control screen is about, opened on its value.
    fn open_concern(&mut self, at: usize) {
        let Some(item) = self.control.get(at).cloned() else {
            return;
        };
        let Some(path) = item.path else {
            self.message = format!("{} has no file to open", item.sample);
            return;
        };
        // Shown in the view, or the filter set aside to show it.
        let mut found = self
            .view
            .iter()
            .position(|entry| entry.path.as_deref() == Some(&path));
        if found.is_none() && (!self.filter.is_empty() || self.basket_only) {
            self.filter.clear();
            self.basket_only = false;
            self.refresh().ok();
            self.message = "the filter was set aside to show it".to_string();
            found = self
                .view
                .iter()
                .position(|entry| entry.path.as_deref() == Some(&path));
        }
        let Some(position) = found else {
            self.message = format!("{} is not in the collection shown", item.sample);
            return;
        };
        self.cursor = position;
        self.screen = Screen::Sample {
            at: position,
            cursor: 0,
        };
        self.from_control = Some(at);
        if let Some(field) = item.field {
            let stem = field.split(['[', '.']).next().unwrap_or(&field).to_string();
            if let Some(line) = self
                .entries()
                .iter()
                .position(|entry| entry.field == field || entry.field == stem)
                && let Screen::Sample { cursor, .. } = &mut self.screen
            {
                *cursor = line;
            }
        }
    }

    // --------------------------------------------------------------- setup

    /// The project's setup, asked as questions, each answer its default.
    pub fn open_setup(&mut self) {
        let setup = Setup {
            answers: project_setup::Answers::default(),
            question: 0,
        };
        let cursor = setup
            .questions(&self.root)
            .first()
            .map_or(0, |question| question.chosen(&setup.answers));
        self.setup = Some(setup);
        self.screen = Screen::Setup { cursor };
    }

    fn setup_key(&mut self, action: Action, cursor: usize) -> Effect {
        let Some(setup) = &mut self.setup else {
            return Effect::None;
        };
        let questions = setup.questions(&self.root);
        let asking = questions.get(setup.question).copied();
        match action {
            Action::Down | Action::Up if asking.is_some() => {
                let last = asking.map_or(0, |question| {
                    question.choices(&self.root).len().saturating_sub(1)
                });
                self.screen = Screen::Setup {
                    cursor: if action == Action::Down {
                        (cursor + 1).min(last)
                    } else {
                        cursor.saturating_sub(1)
                    },
                }
            }
            // A model one already has: its path asked first, the one given
            // before offered again.
            Action::Open if asking.is_some_and(|question| question.asks_path(cursor)) => {
                let before = match &setup.answers.model {
                    project_setup::ModelAnswer::Existing(path) => path.display().to_string(),
                    _ => String::new(),
                };
                self.mode = Mode::prompt(Purpose::ModelPath, before);
            }
            // The answer, then the next question with its own answer on it.
            Action::Open if asking.is_some() => {
                if let Some(question) = asking {
                    question.choose(&mut setup.answers, cursor);
                }
                self.next_question();
            }
            Action::Open => {
                let plan = setup.plan(&self.root);
                if let Some(directory) = &plan.blocked {
                    self.message = format!(
                        "{} is a file where a directory must go: nothing can be written until it moves",
                        self.here(directory)
                    );
                    return Effect::None;
                }
                let chosen: Vec<String> = plan
                    .missing()
                    .map(|proposal| step_name(&proposal.step, &self.root))
                    .collect();
                if chosen.is_empty() {
                    self.message = "this project is set up: there is nothing to do".to_string();
                    return Effect::None;
                }
                let mut lines = vec![format!("in {}:", self.root.display())];
                lines.extend(chosen.iter().map(|name| format!("  {name}")));
                lines.push(String::new());
                lines.push("nothing already here is touched".to_string());
                self.mode = Mode::Confirm {
                    title: "set the project up".to_string(),
                    lines,
                    pending: Box::new(Pending::Setup),
                };
            }
            // The previous question, its answer kept; from the first, out: back
            // to the start page where it was opened from, the folder its `+`
            // made removed while still empty — the collection of a folder with
            // nothing in it was no way back.
            Action::Back => {
                if setup.question == 0 {
                    self.setup = None;
                    if self.from_start {
                        if let Some(made) = self.made.take() {
                            // Only an empty folder goes: anything put in it
                            // since is someone's.
                            let _ = std::fs::remove_dir(made);
                        }
                        return Effect::Quit;
                    }
                    self.screen = Screen::Collection;
                } else {
                    setup.question = (setup.question - 1).min(questions.len());
                    let cursor = questions
                        .get(setup.question)
                        .map_or(0, |question| question.chosen(&setup.answers));
                    self.screen = Screen::Setup { cursor };
                }
            }
            _ => {}
        }
        Effect::None
    }

    /// The setup's next question, its own answer marked: what applies may
    /// change with the answer — the example asks nothing of a model.
    fn next_question(&mut self) {
        let Some(setup) = &mut self.setup else {
            return;
        };
        setup.question += 1;
        let questions = setup.questions(&self.root);
        self.screen = Screen::Setup {
            cursor: questions
                .get(setup.question)
                .map_or(0, |question| question.chosen(&setup.answers)),
        };
    }

    /// The model file typed, checked: named, the next question; refused, said,
    /// and the window kept.
    fn answer_model_path(&mut self, typed: &str) -> bool {
        match project_setup::model_path(&self.root, typed) {
            Ok(path) => {
                if let Some(setup) = &mut self.setup {
                    setup.answers.model = project_setup::ModelAnswer::Existing(path);
                }
                self.mode = Mode::Normal;
                self.next_question();
                true
            }
            Err(reason) => {
                self.message = reason;
                self.mode = Mode::Normal;
                false
            }
        }
    }

    /// The answers done; the directory read again, and the environment made
    /// beside the screen.
    fn apply_setup(&mut self) -> String {
        let Some(setup) = self.setup.take() else {
            return String::new();
        };
        let plan = setup.plan(&self.root);
        let said = match project_setup::apply(&plan, |_| true) {
            Ok(applied) => {
                // Files alone: `samples/`, a directory made, counted as a
                // third file of two.
                let files = applied.written.iter().filter(|path| !path.is_dir()).count();
                let mut said = format!("{} written", counted(files, "file", "files"));
                if let Some(step) = applied.environment {
                    let (sender, receiver) = mpsc::channel();
                    let root = self.root.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(project_setup::make_environment(&root, &step));
                    });
                    self.installing = Some(receiver);
                    said.push_str(&format!(
                        " · installing samplekit {} in .venv/, a minute or so",
                        crate::config::project_setup::python_spelling(env!("CARGO_PKG_VERSION"))
                    ));
                }
                // What comes first, in the project just made.
                if setup.answers.starter == project_setup::Starter::Empty {
                    said.push_str(if setup.answers.has_model() {
                        " · M opens the model, N makes a first sample, P configures"
                    } else {
                        " · N makes a first sample, P configures"
                    });
                }
                said
            }
            Err(error) => error.to_string(),
        };
        self.screen = Screen::Collection;
        said
    }

    // ------------------------------------------------------- configuration

    /// A path from the folder the workbench is on.
    fn here(&self, path: &Path) -> String {
        here(&self.root, path)
    }

    /// Where the collection shows samples of more than one project, the project
    /// each row of the view begins, by its title — `None` for a row in the same
    /// project as the one above it, and for every row where one project holds
    /// them all. A project's title is its folder, from the workbench's own
    /// where it is inside it, as `project` names the workbench's own; `no
    /// project` for a sample no `.samplekitrc` holds. A project as its heading
    /// names it: its path from the folder the workbench is on, or its folder's
    /// own name where it is that folder. A configuration file as `f` names
    /// where a declaration comes from: from the folder opened,
    /// `../.samplekitrc`.
    fn shown_file(&self, file: &Path) -> String {
        let own = dunce::canonicalize(&self.root).unwrap_or(self.root.clone());
        let mut up = PathBuf::new();
        let mut at = own.as_path();
        loop {
            if let Ok(below) = file.strip_prefix(at) {
                return up.join(below).display().to_string();
            }
            match at.parent() {
                Some(parent) => {
                    up.push("..");
                    at = parent;
                }
                None => return file.display().to_string(),
            }
        }
    }

    fn project_title(&self, root: &Path) -> String {
        let root = dunce::canonicalize(root).unwrap_or(root.to_path_buf());
        let own = dunce::canonicalize(&self.root).unwrap_or(self.root.clone());
        match root.strip_prefix(&own) {
            Ok(inside) if !inside.as_os_str().is_empty() => inside.display().to_string(),
            _ => root.file_name().map_or_else(
                || root.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        }
    }

    pub fn project_headings(&self) -> Vec<Option<String>> {
        if !self.collection.spans_configurations() {
            return vec![None; self.view.len()];
        }
        let mut last: Option<Option<PathBuf>> = None;
        self.view
            .iter()
            .map(|entry| {
                let project = entry
                    .path
                    .as_deref()
                    .and_then(|path| self.collection.config_for(path))
                    .map(|config| config.root().to_path_buf());
                if last.as_ref() == Some(&project) {
                    return None;
                }
                last = Some(project.clone());
                Some(match project {
                    None => "no project".to_string(),
                    Some(root) => self.project_title(&root),
                })
            })
            .collect()
    }

    /// The heading each row of the view begins, where the collection is grouped
    /// or spans several projects: the project's title, then the group's values
    /// as the command line heads a table — `more · beer = schwarz` — and
    /// `None` within a group. Ungrouped, the projects' titles alone.
    pub fn headings(&self) -> Vec<Option<String>> {
        let projects = self.project_headings();
        if self.group.is_empty() {
            return projects;
        }
        let Ok(parsed) = self
            .group
            .iter()
            .map(|field| fields::parse(field))
            .collect::<Result<Vec<_>, _>>()
        else {
            return projects;
        };
        // Each row's group, by the sample: the view is already in groups.
        let mut key_of: std::collections::HashMap<*const std::cell::RefCell<Sample>, Vec<_>> =
            std::collections::HashMap::new();
        // Which group each row is in: the view is in its groups already, so
        // the order they are found in does not matter here.
        if let Ok(groups) = self.view.group_by(&parsed, list::GroupOrder::Values) {
            for group in groups {
                for entry in group.samples.iter() {
                    key_of.insert(std::rc::Rc::as_ptr(&entry.sample), group.key.clone());
                }
            }
        }
        let mut project = None;
        let mut last = None;
        self.view
            .iter()
            .zip(projects)
            .map(|(entry, heading)| {
                if heading.is_some() {
                    project = heading;
                    last = None;
                }
                let key = key_of.get(&std::rc::Rc::as_ptr(&entry.sample)).cloned();
                if key == last {
                    return None;
                }
                last = key.clone();
                let values = crate::presentation::summaries::group_heading(
                    &self.group,
                    &key.unwrap_or_default(),
                );
                Some(match &project {
                    Some(project) => format!("{project} · {values}"),
                    None => values,
                })
            })
            .collect()
    }

    /// The project's configuration, section by section, edited in memory and
    /// written only when asked, whole and checked.
    pub fn open_workspace(&mut self) {
        let path = self
            .collection
            .config()
            .map(|config| config.root().join(".samplekitrc"))
            .unwrap_or_else(|| self.root.join(".samplekitrc"));
        match ConfigurationEdit::open(&path) {
            Ok(edit) => {
                // The colours' former section is read still, and left as it is:
                // `P` edits `[tui.colors]`, which wins a role both write, and
                // says so rather than show a section it does not list.
                if !edit.settings("workbench.colors").is_empty() {
                    self.message = "the colours under [workbench.colors] are read still; \
                                    P edits [tui.colors], which wins a role both write"
                        .to_string();
                }
                self.workspace = Some(Workspace {
                    edit,
                    section: 0,
                    row: 0,
                    on_rows: false,
                    dirty: false,
                });
                self.screen = Screen::Configure;
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    /// The rows a section shows: its keys, or each entry's name and keys.
    pub fn workspace_rows(&self) -> Vec<ConfigRow> {
        let Some(workspace) = &self.workspace else {
            return Vec::new();
        };
        match SECTIONS[workspace.section].1 {
            Section::Setting(section) => workspace
                .edit
                .settings(section)
                .into_iter()
                .map(|(key, value)| ConfigRow::Key {
                    name: None,
                    key,
                    value,
                })
                .collect(),
            Section::Named(kind) => {
                let mut rows = Vec::new();
                for name in workspace.edit.names(kind) {
                    rows.push(ConfigRow::Name(name.clone()));
                    for (key, value) in workspace.edit.entries(kind, &name) {
                        rows.push(ConfigRow::Key {
                            name: Some(name.clone()),
                            key,
                            value,
                        });
                    }
                }
                rows
            }
        }
    }

    /// A key on the configuration screen; whether it leaves for the start page.
    fn workspace_key(&mut self, action: Action) -> bool {
        let rows = self.workspace_rows();
        let back_to_start = self.back_to_start;
        let Some(workspace) = &mut self.workspace else {
            return false;
        };
        let last_row = rows.len().saturating_sub(1);
        // A row taken away leaves the cursor on the one before.
        workspace.row = workspace.row.min(last_row);
        match action {
            Action::Down if workspace.on_rows => workspace.row = (workspace.row + 1).min(last_row),
            Action::Up if workspace.on_rows => workspace.row = workspace.row.saturating_sub(1),
            Action::Down => {
                workspace.section = (workspace.section + 1).min(SECTIONS.len() - 1);
                workspace.row = 0;
            }
            Action::Up => {
                workspace.section = workspace.section.saturating_sub(1);
                workspace.row = 0;
            }
            Action::NextColumn | Action::Open if !workspace.on_rows => workspace.on_rows = true,
            Action::PreviousColumn if workspace.on_rows => workspace.on_rows = false,
            // Acting on a setting needs the cursor among them, not on the
            // list of sections.
            Action::Open | Action::Edit | Action::Remove if !workspace.on_rows => {}
            Action::Open | Action::Edit => {
                let section = workspace.section;
                match rows.get(workspace.row) {
                    Some(ConfigRow::Key { name, key, value }) => {
                        let (name, key) = (name.clone(), key.clone());
                        self.mode =
                            Mode::prompt(Purpose::Setting { section, name, key }, value.clone());
                    }
                    // An entry's name takes a key of its own.
                    Some(ConfigRow::Name(name)) => {
                        let name = name.clone();
                        self.mode = Mode::prompt(Purpose::AddKey { section, name }, String::new());
                    }
                    None => {}
                }
            }
            Action::AddRow => {
                let section = workspace.section;
                self.mode = Mode::prompt(Purpose::AddSetting { section }, String::new());
            }
            Action::Remove => {
                let Some(row) = rows.get(workspace.row) else {
                    return false;
                };
                let section = SECTIONS[workspace.section].1;
                let removed = match (section, row) {
                    (Section::Named(kind), ConfigRow::Name(name)) => {
                        workspace.edit.remove(kind, name)
                    }
                    (
                        Section::Named(kind),
                        ConfigRow::Key {
                            name: Some(name),
                            key,
                            ..
                        },
                    ) => workspace.edit.unset(kind, name, key),
                    (Section::Setting(section), ConfigRow::Key { key, .. }) => {
                        workspace.edit.unset_setting(section, key)
                    }
                    _ => false,
                };
                if removed {
                    workspace.dirty = true;
                    // The last row gone, the cursor on the one before it.
                    let rows = self.workspace_rows().len();
                    if let Some(workspace) = &mut self.workspace {
                        workspace.row = workspace.row.min(rows.saturating_sub(1));
                    }
                }
            }
            Action::Write => {
                let lines = workspace.edit.changes();
                if lines.is_empty() {
                    self.message = "nothing changed".to_string();
                    return false;
                }
                if let Err(error) = workspace.edit.checked() {
                    self.message = error.to_string();
                    return false;
                }
                // Changed underneath since it was read: said now, with the way
                // out, rather than refused once confirmed.
                if std::fs::read_to_string(workspace.edit.path())
                    .ok()
                    .as_deref()
                    != workspace.edit.read_text()
                {
                    self.message = "the file changed since it was read: r reads it again, \
                                    letting these changes go"
                        .to_string();
                    return false;
                }
                self.mode = Mode::Confirm {
                    title: format!("write {}", here(&self.root, workspace.edit.path())),
                    lines,
                    pending: Box::new(Pending::Configuration),
                };
            }
            // The file as it is now, what was not written let go: the way
            // out when it changed underneath.
            Action::Reread => {
                let section = workspace.section;
                self.open_workspace();
                if let Some(workspace) = &mut self.workspace {
                    workspace.section = section;
                    self.message = "read again; the changes not written were dropped".to_string();
                }
            }
            // Back to the start page it came from, the changes not written
            // kept here until they are written or let go.
            Action::Back if back_to_start => {
                if workspace.dirty {
                    self.message = "changes not written: w writes them, r lets them go".to_string();
                    return false;
                }
                return true;
            }
            Action::Back => {
                if workspace.dirty {
                    self.message =
                        "changes not written: w writes them, and they wait here meanwhile"
                            .to_string();
                }
                self.screen = Screen::Collection;
            }
            _ => {}
        }
        false
    }

    /// A setting as typed: TOML where it reads as TOML, text otherwise; an
    /// empty one taken away. Whether it was taken: one that would not load
    /// is taken, and said at once.
    fn set_setting(&mut self, section: usize, name: Option<String>, key: &str, text: &str) -> bool {
        let Some(workspace) = &mut self.workspace else {
            return false;
        };
        let value = match configuration_edit::value_of(text) {
            Ok(value) => value,
            Err(refused) => {
                self.message = refused;
                return false;
            }
        };
        let done = match (SECTIONS[section].1, name) {
            (Section::Named(kind), Some(name)) if text.trim().is_empty() => {
                workspace.edit.unset(kind, &name, key)
            }
            (Section::Named(kind), Some(name)) => workspace.edit.set(kind, &name, key, value),
            (Section::Setting(section), _) if text.trim().is_empty() => {
                workspace.edit.unset_setting(section, key)
            }
            (Section::Setting(section), _) => workspace.edit.set_setting(section, key, value),
            (Section::Named(_), None) => return false,
        };
        // Refused where the file holds something other than a table there.
        if !done {
            self.message = format!("{key}: the file holds something else there, left as it is");
            return false;
        }
        workspace.dirty = true;
        // Checked as it would load, said at once rather than at the write.
        if let Err(error) = workspace.edit.checked() {
            self.message = error.to_string();
        }
        true
    }

    /// `key = value` in the named entry `name`; whether it was taken.
    fn add_key(&mut self, section: usize, name: String, text: &str) -> bool {
        let Some((key, value)) = text.split_once('=') else {
            self.message = format!("write key = value, a key of {name}");
            return false;
        };
        let taken = self.set_setting(section, Some(name.clone()), key.trim(), value);
        self.cursor_on_setting(Some(&name), key.trim());
        taken
    }

    /// The cursor on what was just added, so that `d` or `e` next acts on it:
    /// left where it was, `d` removed another entry.
    fn cursor_on_setting(&mut self, name: Option<&str>, key: &str) {
        let rows = self.workspace_rows();
        let at = rows.iter().position(|row| match row {
            ConfigRow::Key {
                name: held,
                key: written,
                ..
            } => held.as_deref() == name && written == key,
            ConfigRow::Name(_) => false,
        });
        if let (Some(at), Some(workspace)) = (at, &mut self.workspace) {
            workspace.row = at;
            workspace.on_rows = true;
        }
    }

    /// `key = value`, or `name.key = value` in a section of named entries.
    fn add_setting(&mut self, section: usize, text: &str) -> bool {
        let Some((left, value)) = text.split_once('=') else {
            self.message = "write key = value, or name.key = value".to_string();
            return false;
        };
        let left = left.trim();
        match SECTIONS[section].1 {
            // The key is the last part: a property's name may hold dots,
            // `conditioning.co2.symbol`, where its keys never do.
            Section::Named(_) => {
                let Some((name, key)) = left.rsplit_once('.') else {
                    self.message = "an entry is written name.key = value".to_string();
                    return false;
                };
                let taken =
                    self.set_setting(section, Some(name.trim().to_string()), key.trim(), value);
                self.cursor_on_setting(Some(name.trim()), key.trim());
                taken
            }
            // A key written quoted, as TOML would quote it, is that key.
            Section::Setting(_) => {
                let key = left.trim_matches('"');
                let taken = self.set_setting(section, None, key, value);
                self.cursor_on_setting(None, key);
                taken
            }
        }
    }

    // ------------------------------------------------------------- compute

    /// A value selected, or no longer; a table or an attribute is not
    /// computed alone, and says so.
    fn toggle_selected(&mut self, entry: Entry) {
        // A table is selected whole: computing it computes every column.
        if entry.kind == EntryKind::Attribute {
            self.message = format!("{} is an attribute: nothing computes it", entry.field);
            return;
        }
        if !self.selected.remove(&entry.field) {
            self.selected.insert(entry.field);
        }
    }

    /// The selected values computed — the one under the cursor when none is —
    /// with the inputs they need, run again whatever their state and an
    /// override given back: the screen shows each state beside it, so a
    /// value chosen is a value meant (`compute -p NAMES --rerun --force`).
    /// Over the basket from the collection, over this sample otherwise.
    ///
    /// A value no formula gives is left out and said; one beside readings no
    /// statistic is recorded for, chosen alone, says how one is given: no mean
    /// stands in on its own.
    fn compute_chosen(&mut self, over_basket: bool) {
        let paths: Vec<PathBuf> = if over_basket && !self.basket.is_empty() {
            self.basket.iter().cloned().collect()
        } else {
            self.current().map(|(path, _)| path).into_iter().collect()
        };
        if over_basket {
            // A selection kept from an earlier session turns C into forcing
            // those values over many samples: said, and confirmed first.
            let names: Vec<String> = self.selected.iter().cloned().collect();
            self.mode = Mode::Confirm {
                title: "compute the values selected".to_string(),
                lines: vec![
                    format!(
                        "{} over {}",
                        names.join(", "),
                        counted(paths.len(), "sample", "samples")
                    ),
                    "run again, and any value set by hand given back to its formula".to_string(),
                    String::new(),
                    "x on a sample clears the selection; C then computes what is not current"
                        .to_string(),
                ],
                pending: Box::new(Pending::Compute(Computing {
                    paths,
                    names,
                    force: true,
                    rerun: true,
                })),
            };
            return;
        }
        let entries = self.entries();
        let chosen: Vec<Entry> = if self.selected.is_empty() {
            self.entry().into_iter().collect()
        } else {
            entries
                .into_iter()
                .filter(|entry| self.selected.contains(&entry.field))
                .collect()
        };
        // Readings with no statistic chosen: their state, and the ways out.
        if let [entry] = chosen.as_slice()
            && entry.state == NO_STATISTIC
        {
            // Where the model is read, a statistic it declares is written with
            // the first value computed from the readings: said so, where the
            // message said a model chooses one beside the model that did.
            let read = self
                .current()
                .is_some_and(|(path, _)| self.owed.contains_key(&path));
            self.message = if read {
                format!(
                    "{} has readings and no statistic recorded: the model's, where it declares \
                     one, is written with the first value computed from them — C computes \
                     what is not current, or e types a value",
                    entry.field
                )
            } else {
                format!(
                    "{} has readings and no statistic chosen for them, so no value: the \
                     model declares one (sk.stats.mean, the median…), or e types a value",
                    entry.field
                )
            };
            return;
        }
        let (names, left): (Vec<Entry>, Vec<Entry>) =
            chosen.into_iter().partition(|entry| self.computable(entry));
        let left: Vec<String> = left.into_iter().map(|entry| entry.field).collect();
        if names.is_empty() {
            self.message = if left.is_empty() {
                "no value is selected here".to_string()
            } else {
                format!("nothing computes {}: entered, not derived", left.join(", "))
            };
            return;
        }
        // A value typed over its formula is given back to it: asked first,
        // as over the basket, rather than lost to a key.
        let overridden: Vec<String> = names
            .iter()
            .filter(|entry| entry.state == "edited")
            .map(|entry| format!("  {}   {}", entry.field, entry.shown))
            .collect();
        let computing = Computing {
            paths,
            names: names.into_iter().map(|entry| entry.field).collect(),
            force: true,
            rerun: true,
        };
        if !overridden.is_empty() {
            let mut lines = vec![format!(
                "{} typed over {} formula, given back to it:",
                if overridden.len() == 1 {
                    "a value"
                } else {
                    "values"
                },
                if overridden.len() == 1 {
                    "its"
                } else {
                    "their"
                }
            )];
            lines.extend(overridden);
            lines.push(String::new());
            lines.push("the value typed is lost · u takes the computation back".to_string());
            self.mode = Mode::Confirm {
                title: "compute over a value typed".to_string(),
                lines,
                pending: Box::new(Pending::Compute(computing)),
            };
            return;
        }
        self.compute(computing);
        if !left.is_empty() && self.running.is_some() {
            self.message = format!(
                "computing… {} left out: entered, not derived",
                left.join(", ")
            );
        }
    }

    /// The selected columns of this table computed — the cursor's column when
    /// none is — run again, overrides given back, as `c` does for a value.
    /// Whether a formula gives the value, or any column of the table.
    fn computable(&self, entry: &Entry) -> bool {
        match entry.kind {
            EntryKind::Quantity => !entry.state.is_empty() && entry.state != NO_STATISTIC,
            EntryKind::Attribute => false,
            EntryKind::Table => self.current().is_some_and(|(_, sample)| {
                Identifier::new(&entry.field)
                    .ok()
                    .and_then(|name| sample.table(&name).ok().map(table_derived))
                    .unwrap_or(false)
            }),
        }
    }

    fn compute_columns(&mut self) {
        let Screen::Table { name, .. } = &self.screen else {
            return;
        };
        let prefix = format!("{name}.");
        let Some((cell, column)) = self.cell() else {
            return;
        };
        let mut names: Vec<String> = self
            .selected
            .iter()
            .filter(|field| field.starts_with(&prefix))
            .cloned()
            .collect();
        // No column selected: the whole table, as naming it to compute does.
        if names.is_empty() {
            let derived = self
                .current()
                .is_some_and(|(_, sample)| sample.table(name).is_ok_and(table_derived));
            if !derived {
                self.message = format!("nothing computes {name}: entered, not derived");
                return;
            }
            names.push(name.to_string());
        }
        let _ = (cell, column);
        let paths = self.current().map(|(path, _)| path).into_iter().collect();
        self.compute(Computing {
            paths,
            names,
            force: true,
            rerun: true,
        });
    }

    /// A computation started beside the screen: the model runs without a
    /// question, as `samplekit compute --write` runs it, and the message names
    /// the model file, as the command line's first lines do.
    fn compute(&mut self, computing: Computing) {
        if self.running.is_some() {
            self.message = "a computation is already running".to_string();
            return;
        }
        if computing.paths.is_empty() {
            return;
        }
        let selection = match list::from_files(&computing.paths) {
            Ok(selection) => selection,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let groups = match computation::groups_of(&selection, None) {
            Ok(groups) => groups,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let mut jobs = Vec::new();
        let mut models: Vec<String> = Vec::new();
        for group in &groups {
            let from = group
                .entries
                .first()
                .and_then(|at| selection.get(*at))
                .and_then(|entry| entry.path.clone())
                .unwrap_or_else(|| self.root.clone());
            let availability = runtime::availability(group.config.as_ref(), &from);
            match availability {
                Err(error) => {
                    self.message = error.to_string();
                    return;
                }
                Ok(runtime::Availability::NoTemplate) => {
                    self.message = "no model is declared: nothing computes here".to_string();
                    return;
                }
                Ok(runtime::Availability::Unavailable { reason }) => {
                    self.message = format!("the model cannot run: {reason}");
                    return;
                }
                Ok(runtime::Availability::Ready { template, python }) => {
                    let model = self.here(template.path());
                    if !models.contains(&model) {
                        models.push(model);
                    }
                    let planned = computation::requests(
                        &selection,
                        group,
                        &template,
                        &computation::Asked {
                            names: computing.names.clone(),
                            rerun: computing.rerun,
                            force: computing.force,
                        },
                    );
                    jobs.push((python, planned.requests));
                }
            }
        }
        // What a computation writes is undone as a whole, and is one snapshot
        // in the history, after one of what changed before it.
        let before = snapshot(&computing.paths);
        let (writing, unread) = version_control::before_writing(&self.places(&computing.paths));
        self.computing_history = Some(writing);
        if let Some(warning) = history_warning(unread) {
            self.message = warning;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || run_jobs(jobs, &sender));
        self.running = Some(Running {
            receiver,
            before: Some(before),
        });
        self.run = Some(RunStatus {
            now: "planning".to_string(),
            ..RunStatus::default()
        });
        self.message = format!("running the model {}", models.join(", "));
    }

    // ---------------------------------------------------------------- note

    fn begin_note(&mut self) {
        let Some((path, sample)) = self.current() else {
            return;
        };
        let original = sample.note().to_string();
        drop(sample);
        // The last line's end is the file's, put back when it is written.
        let text = Written::new(original.strip_suffix('\n').unwrap_or(&original));
        self.mode = Mode::Note {
            path,
            text,
            original,
        };
    }

    /// A key while the note is written: `Esc` previews it; every other is the
    /// editor's — `rat-markdown`'s keys, then `rat-text`'s. A click puts the
    /// cursor where it fell, the wheel scrolls it.
    fn noting(&mut self, key: Key, path: PathBuf, mut text: Written, original: String) -> Effect {
        if key == Key::Esc {
            self.preview_note(path, &text.text(), &original);
            return Effect::None;
        }
        text.key(key);
        self.mode = Mode::Note {
            path,
            text,
            original,
        };
        Effect::None
    }

    /// The note's change, previewed line by line before it is written.
    fn preview_note(&mut self, path: PathBuf, written: &str, original: &str) {
        let mut text = written.to_string();
        if (original.ends_with('\n') || original.is_empty()) && !text.ends_with('\n') {
            text.push('\n');
        }
        if text.trim_end() == original.trim_end() {
            self.message = "the note is unchanged".to_string();
            return;
        }
        let (mut sample, origin) = match document::load_sample(&path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let before: Vec<&str> = original.lines().collect();
        let after: Vec<&str> = text.lines().collect();
        sample.set_note(text.clone());
        self.mode = Mode::Confirm {
            title: "note".to_string(),
            lines: differences(&before, &after),
            pending: Box::new(Pending::Write {
                path,
                sample: Box::new(sample),
                origin,
            }),
        };
    }

    // --------------------------------------------------------------- files

    /// The note shown scrolled by `by` lines, from its top for another
    /// sample's; the view keeps it within the note.
    fn scroll_note(&mut self, by: isize) {
        let path = self.current().map(|(path, _)| path);
        if self.note_scroll.0 != path {
            self.note_scroll = (path, 0);
        }
        self.note_scroll.1 = (self.note_scroll.1 as isize + by).max(0) as usize;
    }

    /// The sample under the cursor, or the basket's, asked about before their
    /// files go: what goes, and what stays — their own files.
    fn remove_mode(&mut self) {
        let on_sample = matches!(self.screen, Screen::Sample { .. });
        let paths: Vec<PathBuf> = if self.basket.is_empty() || on_sample {
            self.current().map(|(path, _)| path).into_iter().collect()
        } else {
            self.basket.iter().cloned().collect()
        };
        if paths.is_empty() {
            self.message = "no sample to remove".to_string();
            return;
        }
        let mut lines: Vec<String> = paths
            .iter()
            .map(|path| {
                format!(
                    "  {}",
                    path.strip_prefix(&self.root).unwrap_or(path).display()
                )
            })
            .collect();
        lines.insert(
            0,
            format!(
                "{} to delete:",
                counted(paths.len(), "sample file", "sample files")
            ),
        );
        let own: usize = paths
            .iter()
            .filter_map(|path| {
                let config = self.config_for(path)?;
                discovery::sample_files(config, path, None).ok()
            })
            .map(|files| files.len())
            .sum();
        lines.push(String::new());
        if own > 0 {
            lines.push(format!(
                "their own files stay: {} under [collection] files",
                counted(own, "file", "files")
            ));
        }
        lines.push(if paths.len() == 1 {
            "u gives it back, as it was".to_string()
        } else {
            "u gives them back, as they were".to_string()
        });
        self.mode = Mode::Confirm {
            title: "remove".to_string(),
            lines,
            pending: Box::new(Pending::Remove(paths)),
        };
    }

    fn files(&mut self) {
        let Some((path, _)) = self.current() else {
            return;
        };
        let Some(config) = self.config_for(&path) else {
            self.message = "no .samplekitrc says where the samples' files are".to_string();
            return;
        };
        if config.collection().files.is_empty() {
            self.message = "no folder is declared for the samples' own files: add \
                            files = [\"images\"] to [collection] in .samplekitrc"
                .to_string();
            return;
        }
        match discovery::sample_files(config, &path, None) {
            Ok(files) if files.is_empty() => {
                self.message = "this sample has no file of its own".to_string()
            }
            Ok(files) => self.mode = Mode::Files { files, cursor: 0 },
            Err(error) => self.message = error.to_string(),
        }
    }

    fn choosing_file(&mut self, key: Key, files: Vec<PathBuf>, mut cursor: usize) -> Effect {
        match key {
            Key::Esc | Key::Char('q') => return Effect::None,
            Key::Char('j') | Key::Down => cursor = (cursor + 1).min(files.len().saturating_sub(1)),
            Key::Char('k') | Key::Up => cursor = cursor.saturating_sub(1),
            Key::Enter => {
                let chosen = files[cursor].clone();
                self.mode = Mode::Files { files, cursor };
                return Effect::Open(chosen);
            }
            Key::Char('n') => {
                let chosen = files[cursor].clone();
                self.mode = Mode::Files { files, cursor };
                return Effect::Navigate(chosen);
            }
            _ => {}
        }
        self.mode = Mode::Files { files, cursor };
        Effect::None
    }

    /// A row's mark: `✗` failed, `⚠` not current, `✎` a hand's, as a table
    /// marks it on the command line.
    pub fn mark(sample: &Sample) -> &'static str {
        let behind = editing::not_current(sample);
        if behind
            .iter()
            .any(|entry| matches!(entry.state, Freshness::Failed { .. }))
        {
            "✗"
        } else if behind.iter().any(|entry| {
            matches!(
                entry.state,
                Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Unjudged { .. }
            )
        }) {
            "⚠"
        } else if !behind.is_empty() {
            "✎"
        } else {
            " "
        }
    }
}

/// A value as a line typed from it holds it: a number at twelve significant
/// figures, which drops what a sum left behind — `4.375000000000013` is
/// 4.375, `14.333333333333307` is 14.3333333333 — and keeps every figure
/// that was typed; anything else as `editing` shows it.
pub fn denoised(value: &crate::core::value::Value) -> String {
    use crate::core::value::Value;
    match value {
        Value::Number(number) if number.is_finite() => {
            let rounded = format!("{number:.11e}").parse::<f64>().unwrap_or(*number);
            editing::shown_value(&Value::Number(rounded))
        }
        other => editing::shown_value(other),
    }
}

/// A number at `precision`, the uncertainty's where `uncertainty`; denoised
/// where none is declared.
pub fn at_precision(
    value: &crate::core::value::Value,
    precision: Option<crate::core::formatting::Precision>,
    uncertainty: bool,
) -> String {
    use crate::core::value::Value;
    match (value, precision) {
        (Value::Number(_) | Value::Integer(_), Some(precision)) => {
            crate::core::formatting::format_value(
                value,
                &crate::core::formatting::Resolved {
                    unit: None,
                    symbol: None,
                    separator: "±".to_string(),
                    precision: Some(if uncertainty {
                        precision.of_uncertainty()
                    } else {
                        precision
                    }),
                },
            )
        }
        (value, _) => denoised(value),
    }
}

/// A path as a window says it: from the folder the workbench is on, so that
/// a title cut to its width still shows the file's name.
fn here(root: &Path, path: &Path) -> String {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let full = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    full.strip_prefix(&root)
        .or_else(|_| path.strip_prefix(&root))
        .map(|inner| inner.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// Whether a formula fills any of a table's cells.
fn table_derived(table: &crate::core::table::Table) -> bool {
    let columns = value_columns(table);
    table.rows().any(|row| {
        columns.iter().any(|column| {
            row.cell(column)
                .is_ok_and(|cell| cell.records().computed.is_some())
        })
    })
}

/// The state of a quantity whose readings have no statistic chosen: nothing
/// computes its value from them.
pub const NO_STATISTIC: &str = "no statistic";

/// A cell's address as a field path writes it, its index quoted where it
/// must be — `'a,b'`, `'12'` as text — so that it reads back as that cell.
pub fn cell_address(
    table: &Identifier,
    column: &Identifier,
    index: Vec<crate::core::value::Value>,
) -> String {
    fields::describe(&fields::Field::Cell {
        table: table.clone(),
        column: column.clone(),
        row: crate::core::table::RowAddress::index(index),
        channel: fields::Channel::Value,
    })
}

/// A table's columns without its index: those a cell is in.
pub fn value_columns(table: &crate::core::table::Table) -> Vec<Identifier> {
    let index = table.index_columns();
    table
        .column_names()
        .into_iter()
        .filter(|column| !index.contains(column))
        .cloned()
        .collect()
}

/// Each file as it is now — its text, or nothing where there is none — for an
/// undo. A file written from samples, tied in their projects' history to the
/// snapshot it was made from; what went wrong, as a warning.
fn keep_output(written: &Path, said: &str, samples: &[PathBuf]) -> Option<String> {
    version_control::keep_output_from(written, said, samples)
        .into_iter()
        .last()
        .map(|(_, error)| format!("the history does not record it: {error}"))
}

/// One snapshot as the history screen shows it: its number over the whole
/// history, when, what wrote, what it changed in brief, and in full, as `diff`
/// says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub number: usize,
    pub when: String,
    pub message: String,
    pub changed: String,
    pub lines: Vec<String>,
    /// The machine that took it and those it joined, where more than one wrote
    /// the history.
    pub machine: Option<String>,
}

/// A history not kept, said once in the status line, never as the change's
/// failure.
fn history_warning(
    failures: impl IntoIterator<Item = (PathBuf, version_control::VcsError)>,
) -> Option<String> {
    failures
        .into_iter()
        .last()
        .map(|(_, error)| format!("the history was not kept: {error}"))
}

fn snapshot(paths: &[PathBuf]) -> Vec<(PathBuf, Option<String>)> {
    paths
        .iter()
        .map(|path| (path.clone(), std::fs::read_to_string(path).ok()))
        .collect()
}

/// A file a change wrote: what it held before, and what the change left in
/// it — an undo restores the one only while the file still holds the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// The files of a snapshot a change has since written, each with what it
/// holds now; those it left as they were are no part of it.
fn settled(before: Vec<(PathBuf, Option<String>)>) -> Vec<Undone> {
    before
        .into_iter()
        .filter_map(|(path, before)| {
            let after = std::fs::read_to_string(&path).ok();
            (after != before).then_some(Undone {
                path,
                before,
                after,
            })
        })
        .collect()
}

/// A key as a screen or a list reads it, where nothing is typed: Shift with a
/// key that moves is that key, as before a line typed read Shift; held with
/// Ctrl or Alt, or the mouse dragged, it is none.
pub fn plain(key: Key) -> Option<Key> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key {
        Key::Chord(pressed) if pressed.modifiers == KeyModifiers::SHIFT => {
            Some(match pressed.code {
                KeyCode::Left => Key::Left,
                KeyCode::Right => Key::Right,
                KeyCode::Home => Key::Home,
                KeyCode::End => Key::End,
                KeyCode::PageUp => Key::PageUp,
                KeyCode::PageDown => Key::PageDown,
                KeyCode::Delete => Key::Delete,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::Enter => Key::Enter,
                _ => return None,
            })
        }
        Key::Chord(_) | Key::Pointer(_) => None,
        key => Some(key),
    }
}

/// The items of a picker `query` finds, by their positions, the closest
/// first: its letters in order, not necessarily side by side.
pub fn visible(items: &[(String, bool)], query: &str) -> Vec<usize> {
    let mut found: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(at, (label, _))| fuzzy(label, query).map(|score| (score, at)))
        .collect();
    // Stable: equal scores keep the list's own order.
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    found.into_iter().map(|(_, at)| at).collect()
}

/// How closely `text` holds the letters of `query` in order, ignoring case;
/// `None` when it does not hold them all. Letters side by side, at the start of
/// a word, and near the start all score higher: `mw` finds `mash_water`
/// before `mashing_water`. Scored by nucleo, fzf's scheme as Helix keeps it.
pub fn fuzzy(text: &str, query: &str) -> Option<i64> {
    use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
    use nucleo_matcher::{Config, Matcher, Utf32Str};
    if query.is_empty() {
        return Some(0);
    }
    thread_local! {
        // A matcher holds its buffers: one per thread, not one per item.
        static MATCHER: std::cell::RefCell<Matcher> = std::cell::RefCell::new({
            let mut config = Config::DEFAULT;
            // What is typed is the beginning of a name more often than not.
            config.prefer_prefix = true;
            Matcher::new(config)
        });
    }
    let atom = Atom::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
        false,
    );
    let mut characters = Vec::new();
    MATCHER.with(|matcher| {
        atom.score(
            Utf32Str::new(text, &mut characters),
            &mut matcher.borrow_mut(),
        )
        .map(i64::from)
    })
}

/// The lines two texts differ by, `-` the old and `+` the new, around what
/// they share (a longest common subsequence: a note is short).
fn differences(before: &[&str], after: &[&str]) -> Vec<String> {
    let (n, m) = (before.len(), after.len());
    let mut common = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            common[i][j] = if before[i] == after[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut lines) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && before[i] == after[j] {
            i += 1;
            j += 1;
        // What goes before what comes, as a diff reads: a line replaced shows
        // `-` then `+`, where `+` came first.
        } else if i < n && (j == m || common[i + 1][j] >= common[i][j + 1]) {
            lines.push(format!("- {}", before[i]));
            i += 1;
        } else {
            lines.push(format!("+ {}", after[j]));
            j += 1;
        }
    }
    lines
}

/// A computation's requests run on a worker each, what they do sent back as
/// they do it; the files are written as each value finishes.
fn run_jobs(jobs: Vec<(PathBuf, Vec<runtime::Request>)>, sender: &mpsc::Sender<Progress>) {
    let (mut computed, mut failed, mut pending, mut waiting) = (0, 0, 0, 0);
    let mut refused: Vec<String> = Vec::new();
    // Steps are values: each sample's plan asked for first, so that the bar
    // moves by what is computed rather than by samples of any size.
    let mut total = 0;
    let finished = std::cell::Cell::new(0usize);
    for (python, requests) in jobs {
        // The model's digest as the worker will import it, taken before it
        // starts: one taken after the run named a model edited meanwhile.
        let digests: Vec<Option<runtime::TemplateDigest>> = requests
            .iter()
            .map(|request| runtime::digest_of(&request.template).ok())
            .collect();
        let store = runtime::ComputedStore::user();
        let mut worker = match runtime::Worker::start(&python) {
            Ok(worker) => worker,
            Err(error) => {
                let _ = sender.send(Progress::Done(error.to_string()));
                return;
            }
        };
        for request in &requests {
            total += worker.plan(request).map(|plan| plan.len()).unwrap_or(0);
        }
        let _ = sender.send(Progress::At {
            done: finished.get(),
            total,
            now: String::new(),
        });
        for (request, digest) in requests.iter().zip(&digests) {
            let label = request
                .sample
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut on = |event: &runtime::Event| match event {
                runtime::Event::Started { value } => {
                    let _ = sender.send(Progress::At {
                        done: finished.get(),
                        total: total.max(finished.get() + 1),
                        now: format!("{label} · {value}"),
                    });
                }
                runtime::Event::Finished { .. } | runtime::Event::Waiting { .. } => {
                    finished.set(finished.get() + 1);
                    let _ = sender.send(Progress::At {
                        done: finished.get(),
                        total: total.max(finished.get()),
                        now: String::new(),
                    });
                }
                runtime::Event::Failed { value, traceback } => {
                    finished.set(finished.get() + 1);
                    let last = traceback.trim_end().lines().last().unwrap_or_default();
                    let _ = sender.send(Progress::Said(format!("{label} {value} failed: {last}")));
                }
                _ => {}
            };
            match worker.compute(request, true, &mut on) {
                Ok(report) => {
                    computed += report.computed;
                    failed += report.failed;
                    pending += report.pending;
                    waiting += report.waiting;
                    if let (Some(store), Some(digest)) = (&store, digest) {
                        computation::record_computed(store, request, &report, digest);
                    }
                }
                // One sample refusing — a value it does not have, one nothing
                // computes — leaves the others to run.
                Err(error) => {
                    let first = error.to_string();
                    let first = first.lines().next().unwrap_or_default().to_string();
                    let _ = sender.send(Progress::Said(format!("{label}: {first}")));
                    refused.push(format!("{label}: {first}"));
                }
            }
        }
    }
    // Nothing ran because it was refused is not nothing to compute; nor is what
    // waits for its inputs.
    let mut said = if computed + failed == 0 && refused.is_empty() && waiting == 0 {
        "nothing to compute: every value asked for is current".to_string()
    } else if computed + failed == 0 {
        "nothing computed".to_string()
    } else {
        format!("{computed} computed")
    };
    if failed > 0 {
        said.push_str(&format!(", {failed} failed — Enter on it explains"));
    }
    if pending > 0 {
        said.push_str(&format!(", {pending} left outdated — C computes them"));
    }
    if waiting > 0 {
        said.push_str(&format!(
            ", {waiting} {} for {} inputs",
            if waiting == 1 { "waits" } else { "wait" },
            if waiting == 1 { "its" } else { "their" }
        ));
    }
    if let Some(first) = refused.first() {
        said.push_str(&format!(
            " · {} refused, {first}{}",
            refused.len(),
            if refused.len() > 1 { " …" } else { "" }
        ));
    }
    let _ = sender.send(Progress::Done(said));
}

fn narrowed(view: &SampleList, keep: impl Fn(&Path) -> bool) -> SampleList {
    let kept: std::collections::HashSet<*const Sample> = view
        .iter()
        .filter(|entry| entry.path.as_deref().is_some_and(&keep))
        .map(|entry| entry.sample.as_ptr().cast_const())
        .collect();
    view.filter_by(move |sample| kept.contains(&std::ptr::from_ref(sample)))
}

fn profile_of(fields: &[String]) -> Profile {
    profiles::anonymous(
        fields
            .iter()
            .map(|field| ColumnSpec {
                field: field.clone(),
                label: None,
                header: None,
                precision: None,
                template: None,
            })
            .collect(),
    )
}

/// The project's first declared profile, or the name and the first five
/// quantities.
fn default_profile(collection: &SampleList) -> Profile {
    if let Some(config) = collection.config()
        && let Some(name) = config.profile_names().first()
        && let Ok(profile) = config.profile(name)
    {
        return profile.clone();
    }
    let mut fields = vec!["name".to_string()];
    fields.extend(
        collection
            .available_fields()
            .iter()
            .map(fields::describe)
            // SampleKit's own fields are the file's, not the sample's.
            // `project` only where the view spans several: one project's column
            // repeated its folder on every row.
            .filter(|field| {
                !matches!(
                    field.as_str(),
                    "name" | "path" | "filename" | "tags" | "state"
                ) && !field.contains(['.', '['])
                    && (field != "project" || collection.spans_configurations())
            })
            .take(5),
    );
    profile_of(&fields)
}

/// A state as the command line's `status` says it, shorter.
pub fn word(state: &Freshness) -> String {
    match state {
        Freshness::Source => String::new(),
        Freshness::Current => "current".to_string(),
        Freshness::Edited => "edited".to_string(),
        Freshness::RecordMissing => "record missing".to_string(),
        Freshness::Failed { message } => format!("failed — {message}"),
        Freshness::Unjudged { .. } => "not judged".to_string(),
        Freshness::Stale { changed, upstream } => {
            let mut parts: Vec<String> = changed.iter().map(ToString::to_string).collect();
            parts.extend(upstream.iter().map(|name| format!("{name}…")));
            format!("outdated — {}", parts.join(", "))
        }
        Freshness::Broken { .. } => "broken".to_string(),
    }
}

/// An explanation as the panel lists it: what the command line's `explain`
/// says, each input with its value and its state.
fn explained_lines(explained: &Explanation, shown: &dyn Fn(&str) -> String) -> Vec<String> {
    match explained {
        Explanation::Table(table) => {
            let mut lines = vec![counted(table.rows, "row", "rows")];
            for column in &table.columns {
                lines.push(format!(
                    "  {:<16} {:<10} {:?}",
                    column.name.to_string(),
                    column.unit.clone().unwrap_or_default(),
                    column.kind
                ));
            }
            lines
        }
        Explanation::Value(value) => {
            let channel = |made: &Made| match made {
                Made::Value => "its value",
                Made::Uncertainty => "its uncertainty",
                Made::Whole => "it",
            };
            let mut lines = Vec::new();
            match &value.origin {
                Origin::Nothing => lines.push("nothing is stored here".to_string()),
                Origin::Failed { message } => {
                    lines.push("its formula failed:".to_string());
                    lines.extend(message.lines().map(|line| format!("  {line}")));
                }
                Origin::Statistic { readings } => {
                    lines.push(format!("a statistic of {readings} readings"))
                }
                Origin::ByHand => {
                    lines.push("put here by hand, over what a formula or a statistic gave".into())
                }
                Origin::Entered => lines.push("entered or measured; nothing computed it".into()),
                Origin::ReadsNothing(made) => lines.push(format!(
                    "a formula that reads nothing of this sample computed {}",
                    channel(made)
                )),
                Origin::OwnReadings => lines.push(
                    "computed from its own readings, by the statistic its model declares".into(),
                ),
                Origin::Inputs { inputs, .. } => {
                    lines.push("computed from".to_string());
                    let rows: Vec<[String; 3]> = inputs
                        .iter()
                        .map(|input| {
                            let value = match (&input.field, input.cells) {
                                (_, Some(cells)) => format!("{cells} cells"),
                                (Some(field), None) => shown(field),
                                (None, None) => "—".to_string(),
                            };
                            let state = match &input.state {
                                Some(InputState::Of(state)) => word(state),
                                Some(InputState::Column { stale: true }) => "outdated".to_string(),
                                Some(InputState::Column { stale: false }) => "current".to_string(),
                                None => String::new(),
                            };
                            [input.name.clone(), value, state]
                        })
                        .collect();
                    let width = |at: usize| {
                        rows.iter()
                            .map(|row| row[at].chars().count())
                            .max()
                            .unwrap_or(0)
                    };
                    let (names, values) = (width(0), width(1));
                    for [name, value, state] in rows {
                        let pad = |text: &str, to: usize| {
                            format!("{text}{}", " ".repeat(to - text.chars().count()))
                        };
                        lines.push(format!(
                            "  {}  {}  {state}",
                            pad(&name, names),
                            pad(&value, values)
                        ));
                    }
                }
            }
            if let Some(state) = value
                .state
                .as_ref()
                .map(word)
                .filter(|said| !said.is_empty())
            {
                lines.push(String::new());
                lines.push(format!("this value is {state}"));
            }
            if let Some(trace) = &value.trace {
                lines.push(String::new());
                lines.extend(trace.lines().map(str::to_string));
            }
            // A message or a trace may hold several lines; a blank line
            // stays one.
            lines
                .into_iter()
                .flat_map(|line| {
                    if line.is_empty() {
                        vec![line]
                    } else {
                        line.lines().map(str::to_string).collect()
                    }
                })
                .collect()
        }
    }
}

pub fn key_name(key: Key) -> String {
    match key {
        Key::Char(' ') => "Space".to_string(),
        Key::Char(character) => character.to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Esc => "Esc".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Up => "↑".to_string(),
        Key::Down => "↓".to_string(),
        Key::PageUp => "PgUp".to_string(),
        Key::PageDown => "PgDn".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::Left => "←".to_string(),
        Key::Right => "→".to_string(),
        Key::Delete => "Del".to_string(),
        Key::Click(..) => "click".to_string(),
        Key::Wheel { .. } => "wheel".to_string(),
        Key::BackTab => "Shift+Tab".to_string(),
        Key::MoveUp => "Shift+↑".to_string(),
        Key::MoveDown => "Shift+↓".to_string(),
        Key::Chord(pressed) => {
            use crossterm::event::{KeyCode, KeyModifiers};
            let mut name = String::new();
            for (held, said) in [
                (KeyModifiers::CONTROL, "Ctrl+"),
                (KeyModifiers::ALT, "Alt+"),
                (KeyModifiers::SHIFT, "Shift+"),
            ] {
                if pressed.modifiers.contains(held) {
                    name.push_str(said);
                }
            }
            name.push_str(&match pressed.code {
                KeyCode::Char(character) => character.to_string(),
                code => key_name(match code {
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
                    _ => return name + "?",
                }),
            });
            name
        }
        Key::Pointer(_) => "mouse".to_string(),
    }
}

// --------------------------------------------------------------- state

/// `$XDG_STATE_HOME/samplekit/workbench/<digest>.toml`, in the machine's state
/// directory: one file per directory, named by a digest of its path.
pub fn state_file(root: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let directory = runtime::state_directory()?.join("workbench");
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let name: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(directory.join(format!("{name}.toml")))
}

/// The folders the workbench kept a state for, newest first, those still there,
/// nine at most: the start page's recent projects. A state written before the
/// folder was kept whole is not listed.
pub fn recent_projects() -> Vec<PathBuf> {
    let Some(directory) =
        state_file(Path::new(".")).and_then(|file| file.parent().map(Path::to_path_buf))
    else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension()? != "toml" {
                        return None;
                    }
                    let when = entry.metadata().ok()?.modified().ok()?;
                    let state: Remembered =
                        toml::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
                    // Kept by an earlier build as Windows' `\\?\C:\…`: said
                    // as the system says it.
                    let folder = dunce::simplified(Path::new(&state.directory)).to_path_buf();
                    (folder.is_absolute() && folder.is_dir()).then_some((when, folder))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut recent: Vec<PathBuf> = Vec::new();
    for (_, folder) in found {
        if !recent.contains(&folder) {
            recent.push(folder);
        }
    }
    recent.truncate(9);
    recent
}

fn remembered(root: &Path) -> Option<Remembered> {
    let text = std::fs::read_to_string(state_file(root)?).ok()?;
    toml::from_str(&text).ok()
}

/// Saves what a session leaves for the next: best effort, never a failure.
pub fn remember(workbench: &Workbench) {
    // A folder gone — the one `+` made, left from the setup's first question
    // — is no project to come back to.
    if !workbench.root.is_dir() {
        return;
    }
    let Some(file) = state_file(&workbench.root) else {
        return;
    };
    if let (Some(directory), Ok(text)) = (file.parent(), toml::to_string(&workbench.remembered())) {
        let _ = std::fs::create_dir_all(directory);
        let _ = std::fs::write(&file, text);
    }
}
