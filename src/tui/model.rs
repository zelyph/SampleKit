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
    ColumnSpec, DeclarationKind, DeclaredIn, Export, ExportTarget, FigureKind, Format, Profile,
    ProjectConfig,
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
use crate::presentation::scripting;
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
    /// A property in its window.
    Property,
    /// A list in its window: readings, or an attribute's items.
    Items,
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
    RenameSample,
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
    /// An attribute added to the sample: its name asked, then its value.
    AddAttribute,
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
    /// An item moved down its list, or up.
    ItemDown,
    ItemUp,
    /// An attribute turned from a list of one into its single value, or back.
    ListOrSingle,
    /// From the history, the files taken back to a snapshot.
    RestoreAt,
    /// From the history, a snapshot marked, then compared with another.
    Compare,
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
        Key::Char('c'),
        Action::Columns,
        "choose the columns",
    ),
    (
        Place::Collection,
        Key::Char(' '),
        Action::Basket,
        "put in or take out of the basket, then the next row",
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
        "tag the basket, take a tag off it, or rename a tag: old=new, over every sample shown where the basket is empty",
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
        "compute the selected values, or what is not current: the basket, or every sample shown",
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
        "run a script, draw a figure, or write an export: of the basket, or of what is shown; w on a figure writes it to a file",
    ),
    (
        Place::Collection,
        Key::Char('N'),
        Action::NewSample,
        "a new sample beside this one: its name, then field=value …, and keep:field,… from the one under the cursor",
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
        Place::Property,
        Key::Char('l'),
        Action::NextColumn,
        "next column",
    ),
    (
        Place::Property,
        Key::Right,
        Action::NextColumn,
        "next column",
    ),
    (
        Place::Property,
        Key::Char('h'),
        Action::PreviousColumn,
        "previous column; from the first, back to the sample",
    ),
    (
        Place::Property,
        Key::Left,
        Action::PreviousColumn,
        "previous column; from the first, back to the sample",
    ),
    (
        Place::Property,
        Key::Char('e'),
        Action::Edit,
        "change the cell, previewed; on readings, their list",
    ),
    (
        Place::Property,
        Key::Char('c'),
        Action::ComputeValue,
        "compute the property, even if current or edited",
    ),
    (
        Place::Property,
        Key::Enter,
        Action::Explain,
        "where the value came from",
    ),
    (
        Place::Property,
        Key::Esc,
        Action::Back,
        "back to the sample",
    ),
    (
        Place::Property,
        Key::Char('q'),
        Action::Back,
        "back to the sample",
    ),
    (
        Place::Property,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (Place::Property, Key::Char('?'), Action::Help, "this help"),
    (Place::Items, Key::Char('j'), Action::Down, "next item"),
    (Place::Items, Key::Down, Action::Down, "next item"),
    (Place::Items, Key::Char('k'), Action::Up, "previous item"),
    (Place::Items, Key::Up, Action::Up, "previous item"),
    (
        Place::Items,
        Key::Char('e'),
        Action::Edit,
        "change the item, previewed; on the empty row, add one",
    ),
    (
        Place::Items,
        Key::Char('d'),
        Action::Remove,
        "take the item out, previewed",
    ),
    (
        Place::Items,
        Key::Char('J'),
        Action::ItemDown,
        "move the item down, previewed",
    ),
    (
        Place::Items,
        Key::Char('K'),
        Action::ItemUp,
        "move the item up, previewed",
    ),
    (
        Place::Items,
        Key::Char('L'),
        Action::ListOrSingle,
        "an attribute: a list of one to its single value, or back",
    ),
    (
        Place::Items,
        Key::Enter,
        Action::Explain,
        "where the item leads, where it is a link",
    ),
    (
        Place::Items,
        Key::Char('o'),
        Action::Files,
        "where the item leads, where it is a link: its sample, its file or its address",
    ),
    (Place::Items, Key::Esc, Action::Back, "back"),
    (Place::Items, Key::Char('q'), Action::Back, "back"),
    (Place::Items, Key::Left, Action::Back, "back"),
    (Place::Items, Key::Char('h'), Action::Back, "back"),
    (
        Place::Items,
        Key::Char('u'),
        Action::Undo,
        "undo the last change written",
    ),
    (Place::Items, Key::Char('?'), Action::Help, "this help"),
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
    (
        Place::History,
        Key::Char('r'),
        Action::RestoreAt,
        "take the files back to this snapshot, previewed: restore --at",
    ),
    (
        Place::History,
        Key::Char('d'),
        Action::Compare,
        "what this snapshot changed, and it marked; d on another compares the two: diff --from --to",
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
    (
        Place::Collection,
        Key::Char('R'),
        Action::RenameSample,
        "rename the sample, its own files and the links leading to it, asked first",
    ),
    (
        Place::Sample,
        Key::Char('R'),
        Action::RenameSample,
        "rename this sample, its own files and the links leading to it, asked first",
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
        "open the value in its window (value, uncertainty, readings), or an attribute's items; a link, its sample or its address",
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
        Key::Char('p'),
        Action::Produce,
        "run a script, draw a figure, or write an export: of the basket, or of the sample",
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
        Key::Char('A'),
        Action::AddAttribute,
        "add an attribute: its name, then its value",
    ),
    (
        Place::Sample,
        Key::Char('o'),
        Action::Files,
        "the sample's own files, or where the link under the cursor leads",
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
        "change the index or a text cell; on the empty row, add one; a value is changed in its window, Enter",
    ),
    (
        Place::Table,
        Key::Enter,
        Action::Explain,
        "open the cell: its value, its uncertainty, where it came from; e there changes them",
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
        Action::Columns => "columns",
        Action::Basket => "basket",
        Action::BasketOnly => "basket only",
        Action::TagBasket => "tag",
        Action::Files => "files",
        Action::Edit => "edit",
        Action::Explain => "explain",
        Action::Editor => "editor",
        Action::Help => "all keys",
        Action::Quit => "quit",
        Action::GoTo => "apply",
        Action::Unfold => "unfold",
        Action::ComputeValue => "compute",
        Action::ComputeSample => "update",
        Action::EditNote => "edit note",
        Action::ToggleNote => "note",
        Action::Select => "select",
        Action::NextColumn | Action::PreviousColumn => "column",
        Action::Produce => "produce",
        Action::Control => "state",
        Action::History => "history",
        Action::Configure => "configuration",
        Action::Model => "model",
        Action::RemoveSample => "remove",
        Action::RenameSample => "rename",
        Action::Declare => "save",
        Action::NoteDown => "note down",
        Action::NoteUp => "note up",
        Action::Remove => "remove",
        Action::Write => "write",
        Action::Reread => "read again",
        Action::Summary => "summary",
        Action::ScrollLeft | Action::ScrollRight => "more columns",
        Action::NewSample => "new",
        Action::AddAttribute => "add an attribute",
        Action::AddRow => "add a row",
        Action::Undo => "undo",
        Action::SelectAll => "all",
        Action::ClearSelection => "clear",
        Action::Group => "group",
        Action::ItemDown | Action::ItemUp => "move the item",
        Action::ListOrSingle => "list or single",
        Action::RestoreAt => "restore",
        Action::Compare => "compare",
    }
}

/// The actions the line of hints offers, most wanted first: as many as the
/// terminal's width holds are shown, and help always.
pub fn hinted(place: Place) -> &'static [Action] {
    // The line at the foot: moving about and what concerns the whole screen.
    // What acts on the thing under the cursor is on its own frame instead
    // (`Workbench::element_hints`).
    match place {
        // Most used first, the least used given up first where the foot does
        // not hold them all: at 80 columns, or with a filter on, `C`, `M` and
        // `D` must stay, the basket's keys go (their frame says them).
        Place::Collection => &[
            Action::Filter,
            Action::Sort,
            Action::Columns,
            Action::ComputeSample,
            Action::NewSample,
            Action::Undo,
            Action::Produce,
            Action::Control,
            Action::History,
            Action::Model,
            Action::RemoveSample,
            Action::RenameSample,
            Action::Quit,
            Action::Summary,
            Action::Configure,
            Action::Group,
            Action::GoTo,
            Action::Declare,
            Action::SelectAll,
            Action::ClearSelection,
            Action::TagBasket,
            Action::BasketOnly,
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
        Place::History => &[Action::RestoreAt, Action::Compare, Action::Back],
        Place::Setup => &[Action::Open, Action::Back],
        Place::Configure => &[Action::Write, Action::Reread, Action::Back],
        Place::Table => &[
            Action::NextColumn,
            Action::Back,
            Action::History,
            Action::Undo,
            Action::ComputeSample,
            Action::ClearSelection,
            Action::Files,
            Action::Editor,
        ],
        Place::Property => &[Action::NextColumn, Action::Back, Action::Undo],
        Place::Items => &[Action::Back, Action::Undo],
    }
}

/// What the runtime does that the model cannot: a program started, the
/// terminal given to an editor, the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    Open(PathBuf),
    /// A web address an attribute links to, in the browser.
    OpenAddress(String),
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
    /// A property opened in its window: a table of one row — `value`,
    /// `uncertainty`, `readings` where a formula does not give it — `column`
    /// among them, `back` the sample screen's cursor. A table's cell opens in
    /// it too, `name` its address, `up` the table it was opened from.
    Property {
        at: usize,
        back: usize,
        name: String,
        column: usize,
        up: Option<Box<Screen>>,
    },
    /// A list opened as a table of one column: a property's readings, or an
    /// attribute's value or items; `cursor` its row, the empty one at the foot
    /// adding one. `up` the screen it was opened from, where `Back` goes.
    Items {
        at: usize,
        field: String,
        readings: bool,
        cursor: usize,
        up: Box<Screen>,
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
    /// A script's window while it runs: what it prints, as it prints it, and
    /// at its foot the line its `input()` reads, as a terminal's.
    Script {
        line: Typed,
    },
    Files {
        files: Vec<PathBuf>,
        cursor: usize,
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
    /// A line to type, its text there: a sample's value opens selected,
    /// what is typed replacing it; any other line, the cursor after it.
    fn prompt(purpose: Purpose, text: String) -> Mode {
        let text = match purpose {
            Purpose::Edit { .. } | Purpose::Item { .. } | Purpose::DraftCell { .. } => {
                Typed::new_selected(&text)
            }
            _ => Typed::new(&text),
        };
        Mode::Prompt { purpose, text }
    }

    /// Whether something is being typed: a key held with Ctrl or Alt, and the
    /// mouse dragged, mean something only then.
    pub fn typing(&self) -> bool {
        matches!(
            self,
            Mode::Filter { .. }
                | Mode::Prompt { .. }
                | Mode::Note { .. }
                | Mode::Picker { typing: true, .. }
        )
    }
}

#[derive(Clone)]
pub enum Purpose {
    /// A script's options, `NAME=VALUE` apart by spaces, before it runs on
    /// `samples`.
    ScriptOptions {
        script: runtime::DescribedScript,
        samples: Vec<PathBuf>,
    },
    /// A text row of the figure being set up: its title or an axis's label.
    FigureText(FigureRow),
    /// The name what is shown is declared under.
    DeclareName(Declare),
    Edit {
        path: PathBuf,
        field: String,
    },
    /// A sample's new name.
    Rename {
        path: PathBuf,
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
    /// The name of an attribute added to the sample at `path`: its value is
    /// asked next, as `e` asks a value.
    AttributeName {
        path: PathBuf,
    },
    /// A new sample's name: written beside the sample `beside`, or the
    /// first of the collection.
    New {
        beside: Option<PathBuf>,
    },
    /// An item of a list in its window: `at` its place, or `None` for one
    /// added.
    Item {
        path: PathBuf,
        field: String,
        readings: bool,
        at: Option<usize>,
    },
    /// A cell of the row being typed at a table's foot.
    DraftCell {
        path: PathBuf,
        table: String,
        column: String,
    },
    /// The model file one already has, asked by the setup.
    ModelPath,
    /// The file a figure of `p` is written to, `<name>.pdf` offered.
    FigureFile(Output),
    /// The file what is shown is written to: over one project, from its
    /// folder, `out/shown.csv` offered; over several, from the folder opened,
    /// nothing offered.
    ShownFile {
        several: bool,
    },
    /// The name this machine gives another that wrote in the history, asked
    /// on the history screen of `sample`, or of the project.
    NameMachine {
        identifier: String,
        host: String,
        sample: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Choosing {
    Sort,
    Columns,
    /// Where `f` goes, an item's target at its position.
    GoTo(Vec<Target>),
    /// The fields `g` groups by.
    Group,
    /// What `p` runs, draws or writes, an item's at its position, under its
    /// title: what it is of.
    Produce(Vec<Output>, String),
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
        "history",
        Section::Setting("history"),
        "shared: whether git carries the history",
    ),
    (
        "TUI colours",
        Section::Setting("colors"),
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
    // A value and its uncertainty typed together: each has its cell.
    if let Some((value, uncertainty)) = ["±", "+/-", "+-"]
        .iter()
        .find_map(|sign| text.split_once(sign))
        && number(value)
        && number(uncertainty)
    {
        return Some(format!(
            "'{text}': its uncertainty has a cell of its own: {} here, and {} in the \
             uncertainty",
            value.trim(),
            uncertainty.trim().trim_start_matches('-')
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
pub(crate) fn counted(count: usize, one: &str, many: &str) -> String {
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
        "outdated: an input changed, or its record cannot be checked; C computes it",
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
pub const HELP_SECTIONS: [&str; 9] = [
    "moving",
    "finding and showing",
    "choosing",
    "changing",
    "computing and checking",
    "the history",
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
        ItemDown | ItemUp | ListOrSingle => "changing",
        // `f` applies what is saved to what is shown, and goes nowhere.
        Filter | ClearFilter | Sort | Columns | Summary | BasketOnly | Group | GoTo
        | ToggleNote => "finding and showing",
        Select | SelectAll | ClearSelection | Basket | TagBasket => "choosing",
        Declare => "the project",
        Edit | EditNote | NewSample | AddAttribute | RemoveSample | RenameSample | AddRow
        | Undo | Remove | Write | Reread => "changing",
        ComputeValue | ComputeSample | Explain | Control | History => "computing and checking",
        RestoreAt | Compare => "the history",
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

/// What `p` offers: a script of the model, a figure, an export — or the
/// heading of their group, which does nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// A group's heading: scripts, figures, exports.
    Heading,
    /// A script of the model, run on what the menu is of.
    Script(Box<runtime::DescribedScript>),
    /// A figure of one's own: a field against another.
    Own,
    Figure(String),
    /// A figure the model declares: its code runs, once allowed.
    ModelFigure(String),
    Export(String),
    /// An export of a figure: drawn to its declared file.
    FigureExport(String),
    /// What is shown — its columns, sort and groups — written once to a file
    /// one names, its format by its extension.
    Shown,
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
        Pending::Figure { .. } => "y replace · n cancel",
        _ => "y write · n cancel · j k scroll",
    }
}

/// One file of an export, its text ready, and the samples it is made from.
pub struct ExportFile {
    pub text: String,
    pub path: PathBuf,
    pub rows: usize,
    pub samples: Vec<PathBuf>,
    /// What made it, as the history records it: the command that makes it
    /// again where there is one.
    pub said: String,
    /// What the file held when it was previewed, none where it was not
    /// there: a file changed or made since is not written over.
    pub before: Option<Vec<u8>>,
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
    /// A sample renamed, its files and the links leading to it.
    Rename(Box<crate::collection::renaming::Plan>),
    /// Declarations added to `.samplekitrc`, what they are said as.
    Declare {
        edit: Box<ConfigurationEdit>,
        said: String,
    },
    /// Files as they were before the last change: restored, or removed.
    Undo(Vec<Undone>),
    /// A figure drawn to a file there already, which it replaces.
    Figure {
        output: Output,
        path: PathBuf,
    },
    /// What a script changed, held until it is written.
    Run {
        name: String,
        held: Vec<scripting::Held>,
        /// What the script asked with `input()`, answered: kept with the
        /// change in the history.
        answers: Vec<scripting::Answer>,
    },
    /// An export, its files ready: one per project declaring it.
    Export {
        /// The export's name, for the history's record of each file.
        name: String,
        files: Vec<ExportFile>,
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
    pub before: Option<Vec<(PathBuf, Option<Vec<u8>>)>>,
}

/// A script running beside the screen: what it says as it prints it, the line
/// its `input()` reads, and all it printed so far, which its window shows.
pub struct ScriptRunning {
    pub name: String,
    pub events: mpsc::Receiver<scripting::ScriptEvent>,
    pub input: Option<std::process::ChildStdin>,
    pub printed: String,
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
    /// Where each cell's link leads, where it holds one: its text is then
    /// the link's, as Markdown shows it.
    pub links: Vec<Option<crate::presentation::links::Leads>>,
}

/// One line of the sample screen: a field, as it is shown, and how it stands.
/// A screen a link was followed from.
#[derive(Clone)]
pub struct LinkStep {
    pub left: Screen,
    /// The sample's name, as the title bar says the way taken.
    pub name: String,
    /// Its file, to find it again where the view changed meanwhile.
    pub path: PathBuf,
    /// The link opened a guest, which leaves with the step.
    guest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub field: String,
    pub shown: String,
    pub state: String,
    /// A property's value may be changed and explained; an attribute
    /// changed; a table only explained.
    pub kind: EntryKind,
    /// Where an attribute's link leads, which its colour and mark say; its
    /// text is then the link's, as Markdown shows it.
    pub link: Option<crate::presentation::links::Leads>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Quantity,
    Attribute,
    Table,
    /// A sample whose links lead to this one, after everything it holds.
    Backlink,
}

impl Screen {
    /// The sample a screen shows, by its position in the view: the sample's,
    /// a table's of it, a value's window or a list's.
    pub fn sample_at(&self) -> Option<usize> {
        match self {
            Screen::Sample { at, .. }
            | Screen::Table { at, .. }
            | Screen::Property { at, .. }
            | Screen::Items { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// The same sample found at `found` once the view moved, in this screen
    /// and in those it goes back to.
    fn move_sample_to(&mut self, found: usize) {
        match self {
            Screen::Sample { at, .. } | Screen::Table { at, .. } => *at = found,
            Screen::Property { at, up, .. } => {
                *at = found;
                if let Some(up) = up {
                    up.move_sample_to(found);
                }
            }
            Screen::Items { at, up, .. } => {
                *at = found;
                up.move_sample_to(found);
            }
            _ => {}
        }
    }
}

/// A picker's rows, each three columns — a kind or a name, a name or what
/// it acts on, what it is — every column as wide as its widest.
fn aligned_rows(rows: Vec<(String, String, String)>) -> Vec<(String, bool)> {
    let width = |pick: fn(&(String, String, String)) -> &String| {
        rows.iter()
            .map(|row| render::width(pick(row)))
            .max()
            .unwrap_or(0)
    };
    let (first, second) = (width(|row| &row.0), width(|row| &row.1));
    rows.iter()
        .map(|(a, b, c)| {
            let line = format!(
                "{a}{}  {b}{}   {c}",
                " ".repeat(first - render::width(a)),
                " ".repeat(second - render::width(b))
            );
            (line.trim_end().to_string(), false)
        })
        .collect()
}

/// Four columns aligned — `p`'s name, what it acts on, what it is, whence
/// it comes — what it is cut with `…` past a width, so that whence stays in
/// sight.
fn aligned_columns(rows: Vec<(String, String, String, String)>) -> Vec<(String, bool)> {
    const SAID: usize = 44;
    let cut = |said: &str| -> String {
        if render::width(said) <= SAID {
            said.to_string()
        } else {
            let kept: String = said.chars().take(SAID - 1).collect();
            format!("{}…", kept.trim_end())
        }
    };
    let rows: Vec<(String, String, String, String)> = rows
        .into_iter()
        .map(|(a, b, c, d)| (a, b, cut(&c), d))
        .collect();
    let width = |pick: fn(&(String, String, String, String)) -> &String| {
        rows.iter()
            .map(|row| render::width(pick(row)))
            .max()
            .unwrap_or(0)
    };
    let (first, second, third) = (
        width(|row| &row.0),
        width(|row| &row.1),
        width(|row| &row.2),
    );
    rows.iter()
        .map(|(a, b, c, d)| {
            let line = format!(
                "{a}{}  {b}{}   {c}{}   {d}",
                " ".repeat(first - render::width(a)),
                " ".repeat(second - render::width(b)),
                " ".repeat(third - render::width(c))
            );
            (line.trim_end().to_string(), false)
        })
        .collect()
}

/// A confirmation's change lines as the workbench builds them — a field,
/// three spaces, `before  →  after` or what became of it, and its notes
/// indented beneath — said as every surface says a change, whole.
fn confirmed(
    lines: &[String],
    before: Option<&Sample>,
    after: &Sample,
    config: Option<&ProjectConfig>,
) -> Vec<String> {
    use crate::presentation::changes::{FieldChange, change_lines, made_whole};
    let mut changes: Vec<FieldChange> = Vec::new();
    for line in lines {
        if line.starts_with(' ') {
            if let Some(last) = changes.last_mut() {
                last.notes.push(line.trim().to_string());
            }
            continue;
        }
        let (field, rest) = line.split_once("   ").unwrap_or((line.as_str(), ""));
        let rest = rest.trim();
        changes.push(match rest.split_once("  →  ") {
            Some((before, after)) => {
                // What the line said after the value stays said, beneath.
                match after.split_once(", ") {
                    Some((after, aside)) => {
                        let mut change = FieldChange::new(field, before, after);
                        change.notes.push(aside.to_string());
                        change
                    }
                    None => FieldChange::new(field, before, after),
                }
            }
            None => FieldChange::said(field, rest),
        });
    }
    change_lines(&made_whole(changes, before, after, config))
}

/// What a change makes not current, as every surface says it.
fn said_not_current(sample: &Sample, before: &[editing::NotCurrent]) -> Vec<String> {
    let newly: Vec<(String, String)> =
        editing::newly_not_current(before, &editing::not_current(sample))
            .into_iter()
            .map(|entry| {
                let said = word_of(sample, &entry.state);
                let said = match entry.rows {
                    0 => said,
                    rows => format!("{said} ({})", counted(rows, "row", "rows")),
                };
                (entry.name, said)
            })
            .collect();
    let already = before.len();
    crate::presentation::changes::not_current_lines(
        &newly,
        if newly.is_empty() { already } else { 0 },
    )
}

/// What the drawing says where no window can open here — over SSH without a
/// display, or matplotlib choosing no interactive backend.
const NO_WINDOW: [&str; 3] = [
    "would show nothing",
    "opens no window",
    "no display to open it on",
];

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
    /// The columns whole, as a profile applied made them: the fields alone
    /// came back under their own names, and a profile's labels were lost on
    /// reopening.
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
    /// What the files were when the collection was read — their times and
    /// sizes, and their folders' — so that one changed outside is read again.
    read_as: u64,
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
    /// What that window draws, to be written to a file instead where no
    /// window can open here.
    pub windowed: Option<Output>,
    /// A script running beside the screen: its name, and what it printed
    /// and would change, or why it failed.
    pub scripting: Option<ScriptRunning>,
    /// Each change written, as the files it touched were before it: the last
    /// one is what `u` offers back.
    pub undo: Vec<Vec<Undone>>,
    /// The control screen's lines, built when it opens.
    pub control: Vec<ControlItem>,
    /// The history the history screen shows, each snapshot with what it
    /// changed.
    pub history: Vec<HistoryRow>,
    /// The project and the file the history screen shows, and the snapshot
    /// marked to compare.
    pub history_scope: Option<(ProjectConfig, Option<String>)>,
    pub compare_mark: Option<usize>,
    /// The screens a link was followed from, each with its sample's name:
    /// `Esc` on the sample a link opened goes back to the last, and the title
    /// bar says the way taken, `steel-01 › aluminium-01`.
    pub trail: Vec<LinkStep>,
    /// Samples a link opened that the view does not hold, another project's
    /// or one the filter leaves out: shown after the view's own while the
    /// trail runs, in the order they were opened.
    guests: Vec<list::Entry>,
    /// The collections those of another project were read in, which say
    /// their configuration.
    guest_lists: Vec<SampleList>,
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
    /// What was being typed when Ctrl+C asked whether to quit: given back on
    /// `n`, lost on `y`.
    typing_kept: Option<Box<Mode>>,
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
    /// The row being typed at the foot of a table, until its index is given.
    pub draft: Option<Draft>,
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
/// computed`, `waits for notch`.
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
    /// it: `never computed`, or `waits for notch`.
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
            _ => default_profile(&collection),
        };
        let mut workbench = Workbench {
            read_as: files_stamp(&collection, root),
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
            windowed: None,
            scripting: None,
            undo: Vec::new(),
            control: Vec::new(),
            history: Vec::new(),
            history_scope: None,
            compare_mark: None,
            trail: Vec::new(),
            guests: Vec::new(),
            guest_lists: Vec::new(),
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
            typing_kept: None,
            made: None,
            described: std::cell::OnceCell::new(),
            workspace: None,
            selected: BTreeSet::new(),
            figure: None,
            draft: None,
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
        if let Some(said) = UNREAD_STATE.with(|said| said.borrow_mut().take()) {
            workbench.message = said;
        }
        if let Some(state) = remembered {
            workbench.filter = state.filter;
            workbench.sort = state.sort;
            workbench.reverse = state.reverse;
            workbench.group = state.group;
            // A sample moved or removed since leaves the basket, said.
            let kept: Vec<PathBuf> = state
                .basket
                .iter()
                .map(|relative| root.join(relative))
                .collect();
            let gone = kept.iter().filter(|path| !path.is_file()).count();
            workbench.basket = kept.into_iter().filter(|path| path.is_file()).collect();
            if gone > 0 {
                workbench.message = format!(
                    "{gone} sample{} of the basket no longer there, left out of it",
                    if gone == 1 { "" } else { "s" }
                );
            }
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
        // What was remembered and no longer applies — a filter, a sort, a
        // grouping, a column — is set aside, each alone, and each said: the
        // refresh dropped what failed, and kept the rest.
        if let Err(error) = workbench.refresh() {
            workbench.message = format!("remembered: {error}");
        }
        // On the first row shown: the refresh followed the sample the
        // unsorted list began with, which projects set apart moved down.
        workbench.cursor = 0;
        workbench.plan_owed();
        workbench.bring_histories();
        // What the history said as the project was read — a trace that could
        // not be read — where nothing else is said.
        if let Some(said) = version_control::notices().pop()
            && workbench.message.is_empty()
        {
            workbench.message = said;
        }
        Ok(workbench)
    }

    /// Folders made projects, reached as the workbench opens: the snapshots
    /// the project above kept of their files brought into their own
    /// histories, and said.
    fn bring_histories(&mut self) {
        let mut projects: Vec<ProjectConfig> =
            version_control::projects_of(std::slice::from_ref(&self.root));
        for entry in self.collection.iter() {
            if let Some(config) = entry
                .path
                .as_deref()
                .and_then(|path| self.collection.config_for(path))
                && !projects.iter().any(|known| known.root() == config.root())
            {
                projects.push(config.clone());
            }
        }
        let said: Vec<String> = projects
            .iter()
            .filter_map(version_control::bring_from_above)
            .collect();
        if !said.is_empty() {
            self.message = said.join(" · ");
        }
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
        let open = self
            .screen
            .sample_at()
            .and_then(|at| path_at(&self.view, at));
        let under = path_at(&self.view, self.cursor);
        // A filter that no longer reads, or a sort that no longer resolves,
        // leaves everything shown, unsorted, and says so — never an empty
        // screen that still counts every sample. Each is said: the first
        // alone was, the others dropped unsaid.
        let mut refused: Vec<String> = Vec::new();
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
                    refused.push(format!("the filter was set aside: {error}"));
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
                refused.push(format!("the sort was set aside: {error}"));
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
                    refused.push(format!("the grouping was set aside: {error}"));
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
                refused.push(format!(
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
        // The samples links opened stay while they are visited.
        if !self.guests.is_empty() {
            // Read again with the collection where it holds them: a write
            // elsewhere reloads every sample.
            let held: Vec<list::Entry> = self
                .guests
                .iter()
                .filter(|guest| {
                    !self
                        .view
                        .iter()
                        .any(|entry| entry.path.is_some() && entry.path == guest.path)
                })
                .map(|guest| {
                    guest
                        .path
                        .as_deref()
                        .and_then(|path| self.collection.by_path(path))
                        .cloned()
                        .unwrap_or_else(|| guest.clone())
                })
                .collect();
            self.guests = held.clone();
            self.view = self.view.with_more(held);
        }
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
                Some(found) => self.screen.move_sample_to(found),
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
        if refused.is_empty() {
            Ok(())
        } else {
            Err(refused.join(" · "))
        }
    }

    /// [`refresh`](Self::refresh), what it set aside said: dropped unsaid, a
    /// filter or a sort went without a word.
    fn refresh_said(&mut self) {
        if let Err(error) = self.refresh() {
            self.message = error;
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
                        let owed = subject.path.map_or(&[][..], |path| self.owed_by(path));
                        // A formula changed is outdated, `⚠`, as `v` and the
                        // command line say it: every sample read `∅` once a
                        // formula of the model was edited.
                        let changed = owed.iter().any(|owed| {
                            owed.said.contains("formula changed")
                                || owed.said.starts_with("statistic ")
                        });
                        if changed && matches!(mark, " " | "✎" | "∅") {
                            "⚠"
                        } else if !owed.is_empty() && matches!(mark, " " | "✎") {
                            "∅"
                        } else {
                            mark
                        }
                    },
                    cells: render::row(&self.profile, subject, config, None),
                    links: Vec::new(),
                }
            })
            .collect();
        // A column's numbers written to as many decimals as its most precise.
        let configs: Vec<_> = subjects
            .iter()
            .map(|subject| {
                subject
                    .path
                    .and_then(|path| self.collection.config_for(path))
            })
            .collect();
        let mut cells: Vec<Vec<String>> = self
            .rendered
            .iter_mut()
            .map(|rendered| std::mem::take(&mut rendered.cells))
            .collect();
        render::even_decimals(&self.profile, &subjects, &configs, None, &mut cells);
        let known = self.known_samples();
        for ((rendered, mut row), subject) in self.rendered.iter_mut().zip(cells).zip(&subjects) {
            let links = crate::presentation::links::row(&self.profile, subject, &known);
            for (cell, link) in row.iter_mut().zip(&links) {
                if let Some(link) = link {
                    cell.clone_from(&link.text);
                }
            }
            rendered.links = links
                .into_iter()
                .map(|link| link.map(|link| link.leads))
                .collect();
            rendered.cells = row;
        }
    }

    /// The collection read again where a file changed outside since it was
    /// read — an editor, a script, another terminal — so that the screen, and
    /// the preview of the next change, show the file as it is. Not while a
    /// window is open on what it read, nor while the model computes. True when
    /// read again.
    pub fn catch_up(&mut self) -> bool {
        if !matches!(self.mode, Mode::Normal) || self.running.is_some() {
            return false;
        }
        if files_stamp(&self.collection, &self.root) == self.read_as {
            return false;
        }
        self.reload();
        if self.message.is_empty() {
            self.message = "read again: a file changed outside SampleKit".to_string();
        }
        true
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
                    self.drawing = None;
                    // No window can open here: its file is asked, as `w`
                    // asks it of a declared figure. Without a display the
                    // figure of one's own could reach no file undeclared.
                    match self.windowed.take() {
                        Some(output) if NO_WINDOW.iter().any(|said| text.contains(said)) => {
                            self.message = format!("{text} — its file is asked");
                            self.ask_figure_file(output);
                        }
                        _ => self.message = text,
                    }
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
        while let Some(running) = &mut self.scripting {
            match running.events.try_recv() {
                Ok(scripting::ScriptEvent::Printed(text)) => {
                    said = true;
                    running.printed.push_str(&text);
                    // A question asked while its window was put away brings
                    // it back: nothing else can answer it.
                    if !text.ends_with('\n') && matches!(self.mode, Mode::Normal) {
                        self.mode = Mode::Script {
                            line: Typed::new(""),
                        };
                    }
                }
                Ok(scripting::ScriptEvent::Ended(outcome)) => {
                    said = true;
                    let name = running.name.clone();
                    self.scripting = None;
                    if matches!(self.mode, Mode::Script { .. }) {
                        self.mode = Mode::Normal;
                    }
                    let outcome = outcome.map_err(|(error, said)| {
                        if said.is_empty() {
                            error.to_string()
                        } else {
                            said
                        }
                    });
                    self.script_ended(&name, outcome);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    said = true;
                    self.message = "the script's thread ended without a word".to_string();
                    self.scripting = None;
                    if matches!(self.mode, Mode::Script { .. }) {
                        self.mode = Mode::Normal;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
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
                let said = format!("tui · compute: {}", for_history(&text));
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
    fn undoable_from_history(&self) -> Result<Option<(Vec<Undone>, String)>, String> {
        let Some(config) = version_control::projects_of(std::slice::from_ref(&self.root))
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        // A history that cannot be read is said: it is no history without a
        // change.
        let Some(undoable) = version_control::last_undoable(&config)
            .map_err(|error| format!("the history could not be read: {error}"))?
        else {
            return Ok(None);
        };
        // Byte for byte: read as text, a file in another encoding came back
        // altered.
        let group = undoable
            .files
            .into_iter()
            .map(|file| Undone {
                path: file.path,
                before: file.before,
                after: file.after,
            })
            .collect();
        let when =
            chrono::DateTime::from_timestamp(undoable.seconds + i64::from(undoable.offset), 0)
                .map(|time| time.naive_utc().format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
        Ok(Some((
            group,
            format!("kept in the history, {when}: {}", undoable.message),
        )))
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
        // Named where `log` names it: two machines wrote, or one that is not
        // this one.
        let machines = changes::machines_of(&entries).len() > 1
            || entries
                .iter()
                .any(|entry| !entry.machine.is_empty() && entry.machine != version_control::YOU);
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
                id: entry.id.clone(),
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
        self.history_scope = Some((config.clone(), named.clone()));
        self.compare_mark = None;
        let back = std::mem::replace(&mut self.screen, Screen::Collection);
        self.screen = Screen::History {
            cursor: 0,
            scroll: 0,
            sample,
            back: Box::new(back),
        };
        // A computer that wrote here and that this one has not named yet: its
        // name is asked, once, and kept on this computer alone.
        let me = version_control::machine();
        let names = version_control::machine_names();
        if let Some(entry) = entries.iter().find(|entry| {
            entry.machine_id != me
                && entry.host != entry.machine_id
                && !names.contains_key(&entry.machine_id)
        }) {
            self.mode = Mode::prompt(
                Purpose::NameMachine {
                    identifier: entry.machine_id.clone(),
                    host: entry.host.clone(),
                    sample,
                },
                entry.host.clone(),
            );
        }
    }

    /// The files the history screen shows taken back to the snapshot at
    /// `at`, as `restore --at` takes them: each change previewed, `y`
    /// writes, and it is a change kept of its own.
    fn restore_at(&mut self, at: usize) {
        if self.running.is_some() {
            self.message = "a computation is running: restore once it has finished".to_string();
            return;
        }
        let (Some(row), Some((config, named))) = (self.history.get(at), self.history_scope.clone())
        else {
            return;
        };
        let (number, id) = (row.number, row.id.clone());
        let then = match version_control::files_at(&config, &id) {
            Ok(files) => files,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let now = match version_control::files_now(&config) {
            Ok(files) => files,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let holds = |name: &str| named.as_deref().is_none_or(|named| named == name);
        let names: std::collections::BTreeSet<&String> = then
            .keys()
            .chain(now.keys())
            .filter(|name| holds(name))
            .collect();
        let mut group = Vec::new();
        let mut lines = vec![format!("restore · as at #{number}")];
        for name in names {
            let (was, is) = (then.get(name), now.get(name));
            if was == is {
                continue;
            }
            let path = config.root().join(name);
            let shown = self.here(&path);
            match (is, was) {
                (Some(is), Some(was)) => lines.extend(crate::presentation::changes::changes_of(
                    &shown,
                    Some(is),
                    Some(was),
                    &config,
                )),
                (Some(_), None) => {
                    lines.push(format!("{shown}   removed: the snapshot did not hold it"))
                }
                (None, Some(_)) => lines.push(format!("{shown}   made again")),
                (None, None) => {}
            }
            group.push(Undone {
                path,
                before: was.cloned(),
                after: is.cloned(),
            });
        }
        if group.is_empty() {
            self.message = format!("nothing to change — the files are as they were at #{number}");
            return;
        }
        self.mode = Mode::Confirm {
            title: format!("restore #{number}"),
            lines,
            pending: Box::new(Pending::Undo(group)),
        };
    }

    /// What a snapshot changed, from the one before it, in a window, the
    /// snapshot marked; `d` on another then compares the two, as `diff --from
    /// --to` does. The first `d` only marked it, and showed nothing.
    fn compare_with(&mut self, at: usize) {
        let Some(row) = self.history.get(at) else {
            return;
        };
        let number = row.number;
        match self.compare_mark.take() {
            Some(marked) if marked == at => {
                self.message = "the mark is let go".to_string();
            }
            Some(marked) => self.compare_pair(marked, at),
            None => {
                self.compare_mark = Some(at);
                // The one before it is the next row: the list is newest first.
                if at + 1 < self.history.len() {
                    self.compare_pair(at + 1, at);
                    self.compare_mark = Some(at);
                    self.message = format!("#{number} marked: d on another compares the two");
                } else {
                    self.message = format!(
                        "#{number} is the first state kept, and changed nothing before it: \
                         d on another compares the two"
                    );
                }
            }
        }
    }

    /// Two snapshots of the history compared, value by value, in a window.
    fn compare_pair(&mut self, marked: usize, at: usize) {
        let Some((config, named)) = self.history_scope.clone() else {
            return;
        };
        // From the older to the newer, as `diff --from --to` reads them: the
        // older has the higher number.
        let (a, b) = (&self.history[marked], &self.history[at]);
        let (older, newer) = if a.number > b.number { (a, b) } else { (b, a) };
        let read = |id: &str| version_control::files_at(&config, id);
        let (from, to) = match (read(&older.id), read(&newer.id)) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(error), _) | (_, Err(error)) => {
                self.message = error.to_string();
                return;
            }
        };
        let holds = |name: &str| named.as_deref().is_none_or(|named| named == name);
        let mut lines = vec![
            format!(
                "from  #{} · {} · {}",
                older.number, older.when, older.message
            ),
            format!(
                "to    #{} · {} · {}",
                newer.number, newer.when, newer.message
            ),
            String::new(),
        ];
        let names: std::collections::BTreeSet<&String> = from
            .keys()
            .chain(to.keys())
            .filter(|name| holds(name))
            .collect();
        let mut any = false;
        for name in names {
            let (was, is) = (from.get(name), to.get(name));
            if was == is {
                continue;
            }
            any = true;
            let shown = self.here(&config.root().join(name));
            match (was, is) {
                (Some(was), Some(is)) => lines.extend(crate::presentation::changes::changes_of(
                    &shown,
                    Some(was),
                    Some(is),
                    &config,
                )),
                (None, Some(_)) => lines.push(format!("{shown}   new")),
                (Some(_), None) => lines.push(format!("{shown}   removed")),
                (None, None) => {}
            }
        }
        if !any {
            lines.push("nothing changed between them".to_string());
        }
        let (from_number, to_number) = (older.number, newer.number);
        self.mode = Mode::Panel {
            title: format!("diff #{from_number} → #{to_number}"),
            lines,
            scroll: 0,
        };
    }

    /// A name given to another machine, kept, and the history shown with it.
    fn name_machine(&mut self, identifier: &str, name: &str, sample: Option<usize>) {
        if let Err(error) = version_control::name_machine(identifier, name) {
            self.message = error.to_string();
            return;
        }
        self.message = format!("{name} it is, on this computer");
        if let Screen::History { back, .. } =
            std::mem::replace(&mut self.screen, Screen::Collection)
        {
            self.screen = *back;
        }
        self.mode = Mode::Normal;
        self.open_history(sample);
    }

    /// The collection read again from its files: after a write, or an editor.
    pub fn reload(&mut self) {
        match list::from_directory(&self.root) {
            Ok(collection) => {
                self.read_as = files_stamp(&collection, &self.root);
                self.collection = collection;
                // Its fields read again when next asked: a write, an editor,
                // a computation may have added one.
                self.described = std::cell::OnceCell::new();
                // A sample gone — undone, removed in an editor — leaves the
                // basket, which would otherwise refuse every action on it;
                // said.
                let held = self.basket.len();
                self.basket.retain(|path| path.is_file());
                let gone = held - self.basket.len();
                if gone > 0 {
                    self.message = format!(
                        "{gone} sample{} of the basket no longer there, left out of it",
                        if gone == 1 { "" } else { "s" }
                    );
                }
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
            for (config, from, paths) in jobs {
                // What the project's trace knows of the formulas that
                // computed each value.
                let trace = computation::trace_of(Some(&config)).ok().flatten();
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
                    let recorded = match crate::format::document::load_sample(&path) {
                        Ok((sample, _)) => computation::recorded_for(trace.as_deref(), &sample),
                        Err(_) => Default::default(),
                    };
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
                    (matches!(
                        planned.reason,
                        runtime::Reason::NeverComputed
                            | runtime::Reason::Waiting
                            | runtime::Reason::FormulaChanged
                    ) || planned.said.starts_with("statistic "))
                        && !recorded.contains(&planned.value)
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
                        // A value waiting for itself, as `compute` says it:
                        // `volume waits for volume` read as a riddle.
                        if planned.said == format!("waits for {}", planned.value) {
                            "waits for its value, which nobody entered".to_string()
                        } else {
                            planned.said
                        }
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
            self.refresh_said();
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
            Screen::Property { .. } => Place::Property,
            Screen::Items { .. } => Place::Items,
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
            Screen::Sample { at, .. }
            | Screen::Table { at, .. }
            | Screen::Property { at, .. }
            | Screen::Items { at, .. } => at,
            Screen::Control { .. }
            | Screen::History { .. }
            | Screen::Setup { .. }
            | Screen::Configure => return None,
        };
        let entry = self.view.get(at)?;
        Some((entry.path.clone()?, entry.sample.borrow()))
    }

    fn config_for(&self, path: &Path) -> Option<&ProjectConfig> {
        match self
            .guest_lists
            .iter()
            .find(|held| held.by_path(path).is_some())
        {
            Some(held) => held.config_for(path),
            None => self.collection.config_for(path),
        }
    }

    /// The links followed forgotten, and the samples they opened outside the
    /// view gone from it.
    fn leave_trail(&mut self) {
        self.trail.clear();
        if !self.guests.is_empty() {
            let own = self.view.len().saturating_sub(self.guests.len());
            self.view = self.view.truncated(own);
            self.guests.clear();
            self.guest_lists.clear();
        }
        self.cursor = self.cursor.min(self.view.len().saturating_sub(1));
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
            Mode::Script { line } => self.answering(key, line),
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
            // Its rows, and the empty one at its foot.
            Screen::Table { name, .. } => self
                .current()
                .and_then(|(_, sample)| sample.table(name).ok().map(|table| table.rows().count()))
                .unwrap_or(0),
            Screen::Collection => 0,
            Screen::Control { .. } => self.control.len().saturating_sub(1),
            Screen::History { .. } => self.history.len().saturating_sub(1),
            Screen::Setup { .. } | Screen::Configure | Screen::Property { .. } => 0,
            // Its items, and the empty row at its foot.
            Screen::Items { .. } => self.items().map_or(0, |(_, items, _)| items.len()),
        };
        let page = self.page;
        let last_column = match &self.screen {
            Screen::Table { name, .. } => self
                .current()
                .and_then(|(path, sample)| {
                    table_layout(&sample, name, self.config_for(&path))
                        .map(|layout| layout.cursor_columns().len())
                })
                .unwrap_or(0)
                .saturating_sub(1),
            Screen::Property { name, .. } => self.property_columns(name).len().saturating_sub(1),
            _ => 0,
        };
        // `Enter` and `o` on an attribute that links elsewhere follow it, and
        // on a list's item.
        if matches!(self.screen, Screen::Sample { .. } | Screen::Items { .. })
            && matches!(action, Action::Explain | Action::Files)
            && let Some(effect) = self.follow_link()
        {
            return effect;
        }
        // A sample linking here, followed as its link would be.
        if matches!(self.screen, Screen::Sample { .. })
            && matches!(
                action,
                Action::Explain | Action::Files | Action::Edit | Action::Unfold
            )
            && let Some(entry) = self.entry()
            && entry.kind == EntryKind::Backlink
        {
            let at = self
                .entries()
                .iter()
                .filter(|each| each.kind == EntryKind::Backlink)
                .position(|each| each.field == entry.field)
                .unwrap_or(0);
            if let Some((path, _, _)) = self.backlinks().into_iter().nth(at) {
                return self.visit(path, &entry.field);
            }
            return Effect::None;
        }
        // Back from a sample a link opened: to where the link was.
        if matches!(self.screen, Screen::Sample { .. })
            && matches!(action, Action::Back)
            && let Some(step) = self.trail.pop()
        {
            if step.guest && self.guests.pop().is_some() {
                self.view = self.view.truncated(self.view.len().saturating_sub(1));
            }
            self.screen = step.left;
            if let Some(at) = self
                .view
                .iter()
                .position(|entry| entry.path.as_deref() == Some(step.path.as_path()))
            {
                self.screen.move_sample_to(at);
            }
            if self.trail.is_empty() {
                self.leave_trail();
            }
            return Effect::None;
        }
        // On the collection, no sample is visited.
        if matches!(self.screen, Screen::Collection) && !self.guests.is_empty() {
            self.leave_trail();
        }
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
            (Screen::Collection | Screen::Sample { .. }, Action::Produce) => self.produce_mode(),
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
                    self.preview_compute(
                        Computing {
                            paths,
                            names: Vec::new(),
                            force: false,
                            rerun: false,
                        },
                        None,
                    );
                }
            }
            (Screen::History { cursor, .. }, Action::RestoreAt) => {
                let at = *cursor;
                self.restore_at(at);
            }
            (Screen::History { cursor, .. }, Action::Compare) => {
                let at = *cursor;
                self.compare_with(at);
            }
            (_, Action::Undo) => self.begin_undo(),
            // Beside the sample under the cursor, taking nothing of it; the
            // first of a collection into samples/ where there is one.
            (Screen::Collection, Action::NewSample) => {
                let beside = self.current().map(|(path, _)| path);
                self.mode = Mode::prompt(Purpose::New { beside }, String::new());
            }
            (Screen::Collection, Action::Bottom) => self.cursor = self.view.len().saturating_sub(1),
            (Screen::Collection, Action::Open) => {
                self.from_control = None;
                self.leave_trail();
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
                    self.refresh_said();
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
                // What has no order is not offered: a sample's states and its
                // tags are sets, refused once chosen.
                for field in shown
                    .chain(others)
                    .filter(|field| !matches!(field.as_str(), "state" | "tags"))
                {
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
                    self.refresh_said();
                }
            }
            (Screen::Collection, Action::BasketOnly) => {
                self.basket_only = !self.basket_only;
                self.refresh_said();
            }
            (Screen::Collection, Action::TagBasket) => {
                // Empty, the basket leaves a rename to make over what is shown.
                self.mode = Mode::prompt(Purpose::Tag, String::new());
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
                    // A table the model declares and the file holds no row
                    // of opens on its first row, which makes it.
                    // A table the model declares and the file holds no row of
                    // unfolds as any does: its columns, and the empty row to
                    // fill.
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
            (Screen::Property { column, .. }, Action::NextColumn) => {
                *column = (*column + 1).min(last_column)
            }
            (Screen::Property { column, .. }, Action::PreviousColumn) if *column > 0 => {
                *column -= 1
            }
            (Screen::Property { at, back, up, .. }, Action::Back | Action::PreviousColumn) => {
                self.screen = match up.take() {
                    Some(up) => *up,
                    None => Screen::Sample {
                        at: *at,
                        cursor: *back,
                    },
                }
            }
            (Screen::Property { .. }, Action::Edit) => self.edit_property_cell(),
            (Screen::Property { .. }, Action::ComputeValue) => self.compute_chosen(false),
            (Screen::Property { .. }, Action::Explain) => self.explain(),
            (Screen::Items { cursor, .. }, Action::Down) => *cursor = (*cursor + 1).min(last_entry),
            (Screen::Items { cursor, .. }, Action::Up) => *cursor = cursor.saturating_sub(1),
            (Screen::Items { up, .. }, Action::Back) => {
                self.screen = (**up).clone();
            }
            (Screen::Items { .. }, Action::Edit) => self.edit_item(),
            (Screen::Items { .. }, Action::Remove) => self.change_items(ItemChange::Remove),
            (Screen::Items { .. }, Action::ItemDown) => self.change_items(ItemChange::Down),
            (Screen::Items { .. }, Action::ItemUp) => self.change_items(ItemChange::Up),
            (Screen::Items { .. }, Action::ListOrSingle) => self.change_items(ItemChange::Toggle),
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
                    self.refresh_said();
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
                // In the collection, what is not current in the samples shown —
                // the basket's, where it holds any: on a current line, the
                // sample alone answered *nothing to compute* above a line
                // that was not. A sample alone is computed from its own view.
                let paths: Vec<PathBuf> = match &self.screen {
                    Screen::Collection if !self.basket.is_empty() => {
                        self.basket.iter().cloned().collect()
                    }
                    Screen::Collection => self
                        .view
                        .iter()
                        .filter_map(|entry| entry.path.clone())
                        .collect(),
                    _ => self.current().map(|(path, _)| path).into_iter().collect(),
                };
                // The basket is kept from a session to the next: what `C`
                // acts on is said, where it is not every sample shown.
                let of_basket = matches!(self.screen, Screen::Collection)
                    && !self.basket.is_empty()
                    && self.basket.len() < self.view.len();
                let count = paths.len();
                let basket = of_basket.then(|| {
                    format!(
                        "the basket's {} — x empties it",
                        counted(count, "sample", "samples")
                    )
                });
                self.preview_compute(
                    Computing {
                        paths,
                        names: Vec::new(),
                        force: false,
                        rerun: false,
                    },
                    basket,
                );
            }
            (Screen::Sample { .. }, Action::Explain) => self.open_window(),
            (Screen::Table { .. }, Action::Explain) => self.open_cell_window(false),
            (Screen::Sample { .. } | Screen::Table { .. }, Action::Edit) => self.begin_edit(),
            (Screen::Sample { .. }, Action::AddAttribute) => {
                let path = self.current().map(|(path, _)| path);
                if let Some(path) = path {
                    self.mode = Mode::prompt(Purpose::AttributeName { path }, String::new());
                }
            }
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
            (_, Action::RenameSample) if self.running.is_some() => {
                self.message = "a computation is writing: rename once it has finished".to_string();
            }
            // Its name to type, the one it has offered.
            (Screen::Collection | Screen::Sample { .. }, Action::RenameSample) => {
                match self.current().map(|(path, _)| path) {
                    Some(path) => {
                        let stem = path
                            .file_stem()
                            .map(|stem| {
                                stem.to_string_lossy()
                                    .trim_end_matches(".sample")
                                    .to_string()
                            })
                            .unwrap_or_default();
                        self.mode = Mode::Prompt {
                            purpose: Purpose::Rename { path },
                            text: Typed::new_selected(&stem),
                        };
                    }
                    None => self.message = "no sample to rename".to_string(),
                }
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
        let draft = self.draft_unwritten();
        if !unwritten && !draft && under_way.is_empty() {
            return Effect::Quit;
        }
        let mut lines = Vec::new();
        if unwritten {
            lines.push("the configuration's changes are not written".to_string());
            lines.push("y leaves without them · n stays — P, then w, writes them".to_string());
        }
        if draft {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(
                "a row kept at a table's foot is not written: its index is not given".to_string(),
            );
            lines.push("y leaves without it · n stays — its index writes it".to_string());
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
        // A row kept at a table's foot, its index not given yet, is asked
        // about as `q` asks: it was lost at once, every cell typed in it.
        if first && !self.typing_unsaved() && self.draft_unwritten() {
            self.mode = Mode::Confirm {
                title: "quit".to_string(),
                lines: vec![
                    "a row kept at a table's foot is not written: its index is not given"
                        .to_string(),
                    "y quits without it · n goes back to it".to_string(),
                    "Ctrl+C again quits at once".to_string(),
                ],
                pending: Box::new(Pending::Quit),
            };
            return false;
        }
        // The configuration's changes not written are asked about as `q` asks:
        // Ctrl+C left with them, lost.
        if first
            && !self.typing_unsaved()
            && self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.dirty)
        {
            self.mode = Mode::Confirm {
                title: "quit".to_string(),
                lines: vec![
                    "the configuration's changes are not written".to_string(),
                    "y quits without them · n goes back to them".to_string(),
                    "Ctrl+C again quits at once".to_string(),
                ],
                pending: Box::new(Pending::Quit),
            };
            return false;
        }
        // What is being typed and not saved is asked about first, as a
        // computation is: Ctrl+C closed the note, text and all, at once.
        if first && self.typing_unsaved() {
            let typing = std::mem::replace(&mut self.mode, Mode::Normal);
            self.typing_kept = Some(Box::new(typing));
            self.mode = Mode::Confirm {
                title: "quit".to_string(),
                lines: vec![
                    "what you are typing is not saved".to_string(),
                    "y quits without it · n goes back to it".to_string(),
                    "Ctrl+C again quits at once".to_string(),
                ],
                pending: Box::new(Pending::Quit),
            };
            return false;
        }
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

    /// Whether something typed would be lost: a line, a value or the note,
    /// holding text not written.
    fn typing_unsaved(&self) -> bool {
        match &self.mode {
            // The filter and a list's `/` are lines typed too: Ctrl+C quit
            // there at once, what was typed lost.
            Mode::Prompt { text, .. } | Mode::Filter { text, .. } => !text.text().trim().is_empty(),
            Mode::Picker {
                query,
                typing: true,
                ..
            } => !query.text().trim().is_empty(),
            Mode::Note { text, original, .. } => text.text() != *original,
            _ => false,
        }
    }

    /// The row typed at a table's foot and kept until its index is given:
    /// cells measured and not written.
    fn draft_unwritten(&self) -> bool {
        self.draft.as_ref().is_some_and(|draft| {
            draft
                .cells
                .iter()
                .any(|(_, typed)| !typed.trim().is_empty())
        })
    }

    /// A filter that matched nothing, and a value it compares a field with that
    /// the collection does not hold: the nearest the collection holds for that
    /// field, as `style == stuot: did you mean nickel?`. The collection's own
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
            // Case aside: `Nickel` for `nickel` is as near as a value gets.
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

    /// The fields a picker offers: a list's items by position — `carbon[#0]`,
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
                // holds its letters in order, closest first: `bx` finds `porosity`,
                // and `hz` finds `roughness`.
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
                // `hv20` finds `measurements.hv[20]`, which the word after
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
        // the completions were values of their own, `radius > 1,1.0513…`.
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
    /// is compared by order — `radius > 1.0513333333333332` offered as `radius >
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
                        self.refresh_said();
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
            // Kind, name and what it is, each in its column, as `list` says
            // them: a query's filter, a profile's columns.
            let mut rows: Vec<(String, String, String)> = Vec::new();
            for name in config.query_names() {
                if let Ok(query) = config.query(name) {
                    rows.push((
                        "query".to_string(),
                        format!("{project}{name}"),
                        format!("{}{}", query.filter, origin(DeclarationKind::Query, name)),
                    ));
                    targets.push(Target::Query(query.filter.clone()));
                }
            }
            for name in config.profile_names() {
                if let Ok(profile) = config.profile(name) {
                    let columns: Vec<String> = profile
                        .columns()
                        .iter()
                        .map(|column| column.field.clone())
                        .collect();
                    rows.push((
                        "profile".to_string(),
                        format!("{project}{name}"),
                        format!(
                            "{}{}",
                            columns.join(", "),
                            origin(DeclarationKind::Profile, name)
                        ),
                    ));
                    targets.push(Target::Profile(Box::new(profile.clone())));
                }
            }
            items.extend(aligned_rows(rows));
        }
        Mode::Picker {
            purpose: Choosing::GoTo(targets),
            items,
            cursor: 0,
            query: Typed::unfocused(""),
            typing: false,
        }
    }

    /// A key in a running script's window: typed in its line, `Enter`
    /// sending the line to what `input()` reads, echoed as a terminal echoes
    /// it; `Esc` puts the window away, the script running on.
    fn answering(&mut self, key: Key, mut line: Typed) -> Effect {
        let Some(running) = &mut self.scripting else {
            return Effect::None;
        };
        match key {
            Key::Enter => {
                let answer = line.text().to_string();
                let sent = running.input.as_mut().map(|input| {
                    use std::io::Write as _;
                    input
                        .write_all(format!("{answer}\n").as_bytes())
                        .and_then(|()| input.flush())
                });
                if let Some(Err(error)) = sent {
                    self.message = format!("the script reads no more: {error}");
                }
                running.printed.push_str(&format!("{answer}\n"));
                line = Typed::new("");
            }
            Key::Esc => {
                self.message = format!(
                    "'{}' runs on: its window comes back when it asks",
                    running.name
                );
                return Effect::None;
            }
            other => {
                line.key(other);
            }
        }
        self.mode = Mode::Script { line };
        Effect::None
    }

    /// A script started beside the screen, on `samples`.
    fn start_script(
        &mut self,
        name: &str,
        samples: Vec<PathBuf>,
        options: std::collections::BTreeMap<String, String>,
    ) {
        // The model of the menu's project that declares it, or the root's.
        let Some(config) = self.declaring(|config| {
            runtime::model_scripts(config)
                .iter()
                .any(|declared| declared == name)
        }) else {
            self.message = format!("no model of this selection declares the script '{name}'");
            return;
        };
        let config = &config;
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
        let request = scripting::ScriptRequest {
            script: name.to_string(),
            samples,
            options,
            style: None,
            // Ended in the workbench as a computation is: by Ctrl+C.
            timeout: None,
        };
        let (input, events) = match scripting::start(&python, &request) {
            Ok(started) => started,
            Err(error) => {
                self.message = format!("'{name}' failed: {error}");
                return;
            }
        };
        self.scripting = Some(ScriptRunning {
            name: name.to_string(),
            events,
            input: Some(input),
            printed: String::new(),
        });
        // Its window, where it prints and where `input()` is answered.
        self.mode = Mode::Script {
            line: Typed::new(""),
        };
        self.message = format!(
            "running '{name}' on {}",
            counted(count, "sample", "samples")
        );
    }

    /// A script ended: what it changed shown and asked about, what it printed
    /// above; what it printed alone shown in a window.
    fn script_ended(&mut self, name: &str, outcome: Result<scripting::Ran, String>) {
        let scripting::Ran {
            printed,
            held,
            answers,
        } = match outcome {
            Ok(ended) => ended,
            Err(said) => {
                self.message = format!("'{name}' failed: {said}");
                return;
            }
        };
        let held: Vec<scripting::Held> = held
            .into_iter()
            .filter(|held| held.before.as_deref() != Some(held.after.as_slice()))
            .collect();
        let mut lines: Vec<String> = printed.lines().map(str::to_string).collect();
        // What it asked, answered, as the command line says it.
        if !answers.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.extend(
                answers
                    .iter()
                    .map(|asked| format!("asked {}  {}", asked.prompt.trim(), asked.answer)),
            );
        }
        if held.is_empty() {
            self.message = format!("'{name}' ran, and changed nothing");
            if !lines.is_empty() {
                self.mode = Mode::Panel {
                    title: format!("run {name}"),
                    lines,
                    scroll: 0,
                };
            }
            return;
        }
        let Some(config) = self.collection.config() else {
            return;
        };
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let mut newly: Vec<(String, String)> = Vec::new();
        for file in &held {
            let shown = self.here(&file.path);
            lines.extend(crate::presentation::changes::changes_of(
                &shown,
                file.before.as_ref(),
                Some(&file.after),
                config,
            ));
            // What it makes not current, as the command line says it: a
            // script scaling the ore charge left recovery outdated, unsaid.
            let Some(after) = crate::presentation::changes::sample_of(&file.after) else {
                continue;
            };
            let was = file
                .before
                .as_deref()
                .and_then(crate::presentation::changes::sample_of)
                .map(|before| editing::not_current(&before))
                .unwrap_or_default();
            let label = file
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            for entry in editing::newly_not_current(&was, &editing::not_current(&after)) {
                let said = word_of(&after, &entry.state);
                let said = match entry.rows {
                    0 => said,
                    rows => format!("{said} ({})", counted(rows, "row", "rows")),
                };
                let named = if held.len() > 1 {
                    format!("{label} {}", entry.name)
                } else {
                    entry.name
                };
                newly.push((named, said));
            }
        }
        let stale = crate::presentation::changes::not_current_lines(&newly, 0);
        if !stale.is_empty() {
            lines.push(String::new());
            lines.extend(stale);
        }
        // Ended: `running 'scale' on 1 sample` stayed under its preview.
        self.message = format!(
            "'{name}' ran: {}",
            counted(held.len(), "file changed", "files changed")
        );
        self.mode = Mode::Confirm {
            title: format!("run {name}"),
            lines,
            pending: Box::new(Pending::Run {
                name: name.to_string(),
                held,
                answers,
            }),
        };
    }

    /// What `p` offers: the figures and the exports the project declares.
    fn produce_mode(&mut self) {
        // What the selection's projects declare, then the root's: from a root
        // of several projects, `p` offered the root's alone.
        let (scoped, spans) = self.producing_configs();
        let mut configs: Vec<ProjectConfig> = scoped;
        if let Some(root) = self.collection.config()
            && !configs.iter().any(|held| held.root() == root.root())
        {
            configs.push(root.clone());
        }
        let root_dir = self
            .collection
            .config()
            .map(|root| root.root().to_path_buf());
        let origin = |config: &ProjectConfig| -> &'static str {
            if Some(config.root().to_path_buf()) == root_dir {
                "root"
            } else {
                "project"
            }
        };
        // The scripts the model declares, its figures, the declared ones, the
        // exports: each with what it acts on, what it is, whence it comes.
        let mut scripts: Vec<(String, String, String, String, Output)> = Vec::new();
        let mut figures: Vec<(String, String, String, String, Output)> = vec![(
            "your own".to_string(),
            String::new(),
            "a field against another".to_string(),
            String::new(),
            Output::Own,
        )];
        let mut exports: Vec<(String, String, String, String, Output)> = vec![(
            "what is shown".to_string(),
            String::new(),
            "its columns, to a file you name".to_string(),
            String::new(),
            Output::Shown,
        )];
        for config in &configs {
            let (of_the_model, _) = runtime::model_declarations(config);
            // The model read once for its scripts and its figures: what each
            // acts on and its description, as `list scripts` and `list
            // figures` say them.
            let described = (runtime::template_of(config).is_some()
                && (!runtime::model_scripts(config).is_empty() || !of_the_model.is_empty()))
            .then(|| runtime::describe(config, config.root()).ok())
            .flatten();
            if let Some(description) = described.clone() {
                for script in description.scripts {
                    if scripts.iter().any(|(name, ..)| *name == script.name) {
                        continue;
                    }
                    let acts = match script.acts {
                        runtime::Draws::Sample => "each sample",
                        runtime::Draws::Collection => "selection",
                    };
                    scripts.push((
                        script.name.clone(),
                        acts.to_string(),
                        script.description.clone(),
                        "model".to_string(),
                        Output::Script(Box::new(script)),
                    ));
                }
            }
            for name in config.figure_names() {
                if figures.iter().any(|(held, ..)| held == name) {
                    continue;
                }
                // Its title, or what it draws, as `list figures` says it.
                let title = config
                    .figure(name)
                    .map(|figure| {
                        figure.title.clone().unwrap_or_else(|| {
                            let kind = figure.kind.as_str();
                            let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                                "an"
                            } else {
                                "a"
                            };
                            format!("{article} {kind} of {} against {}", figure.y, figure.x)
                        })
                    })
                    .unwrap_or_default();
                figures.push((
                    name.to_string(),
                    "selection".to_string(),
                    title,
                    origin(config).to_string(),
                    Output::Figure(name.to_string()),
                ));
            }
            for name in of_the_model {
                if figures.iter().any(|(held, ..)| *held == name) {
                    continue;
                }
                let figure = described.as_ref().and_then(|described| {
                    described.figures.iter().find(|figure| figure.name == name)
                });
                let acts = match figure.map(|figure| figure.draws) {
                    Some(runtime::Draws::Sample) => "each sample",
                    Some(runtime::Draws::Collection) => "selection",
                    None => "",
                };
                let said = figure
                    .and_then(|figure| figure.description.lines().next())
                    .filter(|line| !line.is_empty())
                    .unwrap_or("drawn by the model");
                figures.push((
                    name.clone(),
                    acts.to_string(),
                    said.to_string(),
                    "model".to_string(),
                    Output::ModelFigure(name),
                ));
            }
            for name in config.export_names() {
                if exports.iter().any(|(held, ..)| held == name) {
                    continue;
                }
                let (said, output) = match config.export(name) {
                    Ok(Export::Table(export)) => (
                        format!("{} · profile {}", export.output.display(), export.profile),
                        Output::Export(name.to_string()),
                    ),
                    Ok(Export::Figure(export)) => (
                        format!("{} · figure {}", export.output.display(), export.figure),
                        Output::FigureExport(name.to_string()),
                    ),
                    Err(_) => (String::new(), Output::Export(name.to_string())),
                };
                exports.push((
                    name.to_string(),
                    String::new(),
                    said,
                    origin(config).to_string(),
                    output,
                ));
            }
        }
        // One table of columns, each group under its heading.
        let mut rows: Vec<(String, String, String, String)> = Vec::new();
        let mut outputs: Vec<Output> = Vec::new();
        let mut headings: Vec<usize> = Vec::new();
        for (heading, group) in [
            ("scripts", scripts),
            ("figures", figures),
            ("exports", exports),
        ] {
            if group.is_empty() {
                continue;
            }
            headings.push(rows.len());
            rows.push((
                heading.to_string(),
                String::new(),
                String::new(),
                String::new(),
            ));
            outputs.push(Output::Heading);
            for (name, acts, said, whence, output) in group {
                rows.push((format!("  {name}"), acts, said, whence));
                outputs.push(output);
            }
        }
        let mut items = aligned_columns(rows);
        for at in headings {
            items[at].0 = items[at].0.trim_end().to_string();
        }
        let title = format!("produce · {}", self.producing_said());
        if let Some(spans) = spans {
            self.message = spans;
        }
        // The cursor on the first thing to run, past the first heading.
        self.mode = Mode::Picker {
            purpose: Choosing::Produce(outputs, title),
            items,
            cursor: 1,
            query: Typed::unfocused(""),
            typing: false,
        };
    }

    /// What `p` is of, as its title says it: the basket, the sample open, or
    /// what is shown — and the project, where it is one.
    fn producing_said(&self) -> String {
        let selection = self.producing_selection();
        let of = if !self.basket.is_empty() {
            format!(
                "the basket's {}",
                counted(selection.len(), "sample", "samples")
            )
        } else if selection.len() == 1 && !matches!(self.screen, Screen::Collection) {
            selection
                .iter()
                .next()
                .and_then(|entry| entry.sample.borrow().name().map(str::to_string))
                .unwrap_or_else(|| "the sample".to_string())
        } else {
            format!(
                "what is shown: {}",
                counted(selection.len(), "sample", "samples")
            )
        };
        match self.collection.scope_of(&selection) {
            Some(list::Scope::One(config))
                if self
                    .collection
                    .config()
                    .is_none_or(|root| root.root() != config.root()) =>
            {
                format!("{of} · {}", self.here(config.root()))
            }
            _ => of,
        }
    }

    /// What a figure or an export is about: the basket; in a sample, that
    /// sample; else what is shown.
    fn producing_selection(&self) -> SampleList {
        if !self.basket.is_empty() {
            return self.view.filter_entries(|entry| {
                entry
                    .path
                    .as_ref()
                    .is_some_and(|path| self.basket.contains(path))
            });
        }
        if !matches!(self.screen, Screen::Collection)
            && let Some(path) = self.current().map(|(path, _)| path)
        {
            return self
                .view
                .filter_entries(|entry| entry.path.as_deref() == Some(path.as_path()));
        }
        self.view.filter_by(|_| true)
    }

    /// The projects the selection belongs to whose figures and exports are
    /// offered — one, or several of one model — and, where they are of
    /// different models, the sentence saying so: only the root's are then.
    fn producing_configs(&self) -> (Vec<ProjectConfig>, Option<String>) {
        let selection = self.producing_selection();
        match self.collection.scope_of(&selection) {
            Some(list::Scope::One(config)) => (vec![config.clone()], None),
            Some(list::Scope::SameModel(configs)) => (configs.into_iter().cloned().collect(), None),
            Some(list::Scope::Mixed(configs)) => {
                let named: Vec<String> = configs
                    .iter()
                    .map(|config| self.here(config.root()))
                    .collect();
                (
                    Vec::new(),
                    Some(format!(
                        "the selection spans several projects of different models — {}: only \
                         the root's figures and exports",
                        named.join(", ")
                    )),
                )
            }
            None => (Vec::new(), None),
        }
    }

    /// The configuration declaring the figure or the export `name`: the
    /// selection's projects first, then the root's.
    fn declaring(&self, has: impl Fn(&ProjectConfig) -> bool) -> Option<ProjectConfig> {
        let (scoped, _) = self.producing_configs();
        scoped
            .into_iter()
            .chain(self.collection.config().cloned())
            .find(|config| has(config))
    }

    /// The samples a figure or an export is of: the basket, the sample open,
    /// or what is shown.
    fn chosen_samples(&self) -> Result<SampleList, String> {
        if self.basket.is_empty() {
            return Ok(self.producing_selection());
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
        // Without a display a window shows nothing: a file is offered.
        if matches!(output, Output::Figure(_) | Output::ModelFigure(_))
            && crate::presentation::opening::refusal(&self.root).is_some()
        {
            self.ask_figure_file(output);
            return;
        }
        match output {
            Output::Heading | Output::Script(_) => {}
            Output::Own => self.open_figure(None),
            Output::Figure(name) => self.draw_figure(&name, chosen, None, None),
            Output::ModelFigure(name) => self.draw_model_figure(&name, None),
            Output::Export(name) => self.preview_export(&name, chosen),
            Output::FigureExport(name) => self.write_figure_export(&name),
            Output::Shown => {
                let several = self.shown_projects() > 1;
                let offered = if several { "" } else { "out/shown.csv" };
                self.mode = Mode::prompt(Purpose::ShownFile { several }, offered.to_string());
            }
        }
    }

    /// How many projects the samples what is shown would write belong to.
    fn shown_projects(&self) -> usize {
        self.collection
            .by_configuration(&self.producing_selection())
            .len()
    }

    /// What is shown, in its order, its columns and its groups, previewed as
    /// written to `written`, from the project's folder: csv, tsv, json, or a
    /// style's document by its extension.
    fn preview_shown(&mut self, written: &str) -> bool {
        let written = written.trim();
        let several = self.shown_projects();
        if several > 1 && written.is_empty() {
            self.message = format!(
                "what is shown spans {several} projects: name its file, from {}",
                self.project_title(&self.root)
            );
            return false;
        }
        let config = self
            .declaring(|_| true)
            .or_else(|| self.collection.config().cloned());
        let config = config.as_ref();
        // The data formats, then each document a style writes.
        let mut formats = vec![Format::Csv, Format::Tsv, Format::Json];
        let (scoped, _) = self.producing_configs();
        for held in scoped.iter().chain(self.collection.config()) {
            for name in held.text_format_names() {
                if let Some(text) = held.text_format(name)
                    && !formats
                        .iter()
                        .any(|format| export_formats::extension(format) == text.extension)
                {
                    formats.push(Format::Text(Box::new(text.clone())));
                }
            }
        }
        let accepted: Vec<String> = formats
            .iter()
            .map(|format| format!(".{}", export_formats::extension(format)))
            .collect();
        let extension = Path::new(written)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let Some(format) = formats
            .into_iter()
            .find(|format| export_formats::extension(format) == extension)
        else {
            self.message = format!(
                "the file's extension says its format: {}",
                accepted.join(", ")
            );
            return false;
        };
        let base = match config {
            Some(config) if several <= 1 => config.root().to_path_buf(),
            _ => self.root.clone(),
        };
        let path = base.join(written);
        let selected = self.producing_selection();
        // Its groups among its columns, and said as groups, which a document
        // marks — `\midrule` between two — as `--group` has a file do.
        let profile = if self.group.is_empty() {
            self.profile.clone()
        } else {
            let mut profile = self.profile.carrying(&self.group);
            profile.group = self.group.clone();
            profile
        };
        // A document in its style's spellings, as `-o t.tex` writes it.
        let style = match &format {
            Format::Text(text) => Some(text.name.clone()),
            _ => None,
        };
        let target = ExportTarget {
            name: "shown".to_string(),
            profile: profile.name.clone(),
            format: format.clone(),
            output: path.clone(),
            filename: false,
            path: false,
            query: None,
            style,
        };
        let dataset =
            match exports::run_within(&target, &profile, &selected, config, &self.collection) {
                Ok(dataset) => dataset,
                Err(error) => {
                    self.message = error.to_string();
                    return false;
                }
            };
        let behind = exports::behind(&selected, &profile, config);
        let prepared = exports::Prepared {
            dataset,
            format,
            path,
            behind,
        };
        let samples: Vec<PathBuf> = selected
            .iter()
            .filter_map(|entry| entry.path.clone())
            .collect();
        let folder = if several > 1 {
            self.root.clone()
        } else {
            base.clone()
        };
        // Made again by the command writing the same table, where the
        // command line can say every column as it is.
        let said = self
            .shown_command(&folder, &samples, &prepared.path, &prepared.format)
            .unwrap_or_else(|| "tui · export what is shown".to_string());
        self.confirm_export(
            "what is shown",
            vec![(prepared, samples, String::new(), said)],
            Vec::new(),
            selected.len(),
        );
        true
    }

    /// The samples a command selects as the menu did — the folder and the
    /// filter, over what is shown; the files themselves, over the basket or a
    /// sample open — as its targets, and the selection written after it.
    fn command_selection(&self, folder: &Path, samples: &[PathBuf]) -> (Vec<String>, Vec<String>) {
        if self.basket.is_empty() && matches!(self.screen, Screen::Collection) {
            let filter = if self.filter.trim().is_empty() {
                Vec::new()
            } else {
                vec!["-f".to_string(), self.filter.clone()]
            };
            (vec![from_here(folder)], filter)
        } else {
            (
                samples.iter().map(|path| from_here(path)).collect(),
                Vec::new(),
            )
        }
    }

    /// `samplekit … export NAME` over a project's part of what `p` exported.
    fn export_command(&self, name: &str, folder: &Path, samples: &[PathBuf]) -> String {
        let (targets, selection) = self.command_selection(folder, samples);
        let mut words = vec!["samplekit".to_string()];
        words.extend(targets);
        words.extend(["export".to_string(), name.to_string()]);
        words.extend(selection);
        shell_words::join(words)
    }

    /// The command writing what is shown to `path`: its columns, sort and
    /// groups. The columns of a profile the project declares are that profile,
    /// `--profile NAME`, whatever its headers, labels and templates; others
    /// are `-c`. `None` where neither says them — a column holding what `-c`
    /// cannot say outside a declared profile, or a profile's sort or groups
    /// left out, which no option says.
    fn shown_command(
        &self,
        folder: &Path,
        samples: &[PathBuf],
        path: &Path,
        format: &Format,
    ) -> Option<String> {
        let declared = self
            .collection
            .config()
            .and_then(|config| config.profile(&self.profile.name).ok())
            .filter(|declared| declared.columns == self.profile.columns);
        let shown_columns = match declared {
            Some(declared) => {
                if (self.sort.is_empty() && !declared.sort.is_empty())
                    || (self.group.is_empty() && !declared.group.is_empty())
                {
                    return None;
                }
                vec!["--profile".to_string(), declared.name.clone()]
            }
            None => vec!["-c".to_string(), self.columns_as_written(format)?],
        };
        let (targets, selection) = self.command_selection(folder, samples);
        let mut words = vec!["samplekit".to_string()];
        words.extend(targets);
        words.extend(selection);
        words.extend(shown_columns);
        if !self.sort.is_empty() {
            let keys: Vec<String> = self
                .sort
                .iter()
                .map(|key| match (self.reverse, key.strip_prefix('-')) {
                    (false, _) => key.clone(),
                    (true, Some(field)) => field.to_string(),
                    (true, None) => format!("-{key}"),
                })
                .collect();
            words.extend(["-s".to_string(), keys.join(",")]);
        }
        if !self.group.is_empty() {
            words.extend(["--group".to_string(), self.group.join(",")]);
        }
        if let Format::Text(text) = format {
            words.extend(["--style".to_string(), text.name.clone()]);
        }
        words.extend(["-o".to_string(), from_here(path), "--write".to_string()]);
        Some(shell_words::join(words))
    }

    /// The columns shown as `-c` writes them; `None` where one holds what it
    /// cannot say — a header, a template, a precision for each half, a label
    /// holding `,` or `=`.
    fn columns_as_written(&self, format: &Format) -> Option<String> {
        let mut columns = Vec::new();
        for column in self.profile.columns() {
            if column.template.is_some() {
                return None;
            }
            let mut written = column.field.clone();
            match &column.precision {
                Some(PrecisionSchema::Both(spec)) => written.push_str(&format!(":{spec}")),
                Some(PrecisionSchema::Split(..)) => return None,
                None => {}
            }
            // `-c field=NAME` names the column in what is written: a data
            // file's header, a document's label.
            let name = match format {
                Format::Text(_) => column.label.as_ref(),
                _ => column.header.as_ref(),
            };
            if let Some(name) = name {
                if name.contains('\'') {
                    return None;
                }
                if name.contains([',', '=']) {
                    written.push_str(&format!("='{name}'"));
                } else {
                    written.push_str(&format!("={name}"));
                }
            }
            columns.push(written);
        }
        Some(columns.join(","))
    }

    /// The file a figure is written to, asked: `<name>.pdf` in the project,
    /// the format its extension says.
    fn ask_figure_file(&mut self, output: Output) {
        let name = match &output {
            Output::Figure(name) | Output::ModelFigure(name) => name.clone(),
            Output::Own => "figure".to_string(),
            _ => return,
        };
        // A figure the model draws for one sample is a file per sample: its
        // name holds `{name}`.
        let offered = match &output {
            Output::ModelFigure(figure) if self.draws_each_sample(figure) => {
                format!("{name}-{{name}}.pdf")
            }
            _ => format!("{name}.pdf"),
        };
        self.mode = Mode::prompt(Purpose::FigureFile(output), offered);
    }

    /// Whether the model's figure `name` draws one sample, as its description
    /// says.
    fn draws_each_sample(&self, name: &str) -> bool {
        self.declaring(|config| {
            runtime::model_declarations(config)
                .0
                .iter()
                .any(|declared| declared == name)
        })
        .and_then(|config| runtime::describe(&config, config.root()).ok())
        .is_some_and(|description| {
            description
                .figures
                .iter()
                .any(|figure| figure.name == name && figure.draws == runtime::Draws::Sample)
        })
    }

    /// A figure of `p` written to `written`, from the project's folder.
    fn write_figure(&mut self, output: Output, written: &str) -> bool {
        let written = written.trim();
        if written.is_empty() {
            self.message = "a figure's file needs a name: <name>.pdf, .png or .svg".to_string();
            return false;
        }
        let base = self
            .collection
            .config()
            .map(|config| config.root().to_path_buf())
            .unwrap_or_else(|| self.root.clone());
        let path = base.join(written);
        // One sample's figure for several samples needs a file each: one
        // file named alone held the sample under the cursor.
        if let Output::ModelFigure(figure) = &output
            && !written.contains("{name}")
            && self.draws_each_sample(figure)
            && self.chosen_samples().is_ok_and(|chosen| chosen.len() > 1)
        {
            self.message = format!(
                "'{figure}' draws one sample a figure: name the file with {{name}}, \
                 {figure}-{{name}}.pdf, for a file each"
            );
            return false;
        }
        // A file there already is replaced once asked: `w` wrote over one
        // without a word.
        if !written.contains("{name}") && path.exists() {
            self.mode = Mode::Confirm {
                title: "replace a figure's file".to_string(),
                lines: vec![format!(
                    "{} is there already: y draws the figure over it",
                    self.here(&path)
                )],
                pending: Box::new(Pending::Figure { output, path }),
            };
            return true;
        }
        self.draw_to(output, path);
        true
    }

    /// The figure `output` names, drawn to `path`.
    fn draw_to(&mut self, output: Output, path: PathBuf) {
        match output {
            Output::Figure(name) => match self.chosen_samples() {
                Ok(chosen) => self.draw_figure(&name, chosen, Some(path), None),
                Err(error) => self.message = error,
            },
            Output::ModelFigure(name) => self.draw_model_figure(&name, Some(path)),
            Output::FigureExport(name) => self.draw_figure_export(&name, path),
            Output::Own => {
                if let Some(form) = self.figure.clone() {
                    self.draw_own(&form, Some(path));
                }
            }
            _ => {}
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
        for name in config.declared_style_names() {
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
                // Without a display a window shows nothing: a file is asked.
                if crate::presentation::opening::refusal(&self.root).is_some() {
                    if form.y.is_none() || form.x.is_none() {
                        self.message = "choose both axes first: Enter on y, then on x".to_string();
                        self.mode = Mode::Figure;
                    } else {
                        self.ask_figure_file(Output::Own);
                    }
                    return Effect::None;
                }
                self.draw_own(&form, None);
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
        let text = |value: &str| toml_edit::Value::from(value.to_string());
        let saved = match what {
            Declare::Query => Some(configuration_edit::Saved::Query),
            Declare::Profile => Some(configuration_edit::Saved::Profile),
            Declare::Export(format) => Some(configuration_edit::Saved::Export(format)),
            Declare::Figure => None,
        };
        let said = match saved {
            Some(saved) => {
                // As the screen sorts it: `r` turns every key.
                let sort: Vec<String> = self
                    .sort
                    .iter()
                    .map(|key| match (self.reverse, key.strip_prefix('-')) {
                        (false, _) => key.clone(),
                        (true, Some(field)) => field.to_string(),
                        (true, None) => format!("-{key}"),
                    })
                    .collect();
                let view = configuration_edit::ShownView {
                    filter: &self.filter,
                    columns: self.profile.columns(),
                    sort: &sort,
                    // The grouping shown, which `f` and `--profile` then follow.
                    group: &self.group,
                };
                if let Err(error) = edit.declare_view(saved, name, &view) {
                    self.message = error;
                    return;
                }
                match saved {
                    configuration_edit::Saved::Query => format!(
                        "query {name} declared — f offers it, --query {name} selects with it"
                    ),
                    configuration_edit::Saved::Profile => {
                        format!("profile {name} declared — --profile {name} shows it")
                    }
                    configuration_edit::Saved::Export(_) => {
                        format!("export {name} declared — p writes it, samplekit export {name} too")
                    }
                }
            }
            None => {
                if name.is_empty() {
                    self.message = "a figure needs a name, to be declared under it".to_string();
                    return;
                }
                if let Err(error) = Identifier::new(name) {
                    self.message = format!("'{name}' is no name: {error}");
                    return;
                }
                if edit.names(Named::Figure).iter().any(|held| held == name) {
                    self.message =
                        format!("a figure named {name} is declared already: another name");
                    return;
                }
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
                format!("figure {name} declared — p draws it, samplekit plot {name} too")
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
    fn draw_own(&mut self, form: &FigureForm, output: Option<PathBuf>) {
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
            None => Err(
                "no interpreter is declared: a figure is drawn by matplotlib, in the Python a \
                 project's .samplekitrc names under [model] python — samplekit init makes one"
                    .to_string(),
            ),
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
            output,
            overrides: form.overrides(),
            project_style: true,
            said: None,
            snapshot: None,
            workbench: true,
            // The workbench says it, and has no exit to give.
            accept_outdated: true,
        };
        self.spawn_drawing(python, request, &name, count);
        self.said_left_out(elsewhere.len());
    }

    /// A declared figure drawn in its window beside the screen, what Python
    /// says kept for when it closes.
    /// An export of a figure, to its declared file: asked first where one is
    /// there already, as `w` asks.
    fn write_figure_export(&mut self, name: &str) {
        let Some(config) =
            self.declaring(|config| matches!(config.export(name), Ok(Export::Figure(_))))
        else {
            self.message = format!("no export '{name}' of a figure is declared for this selection");
            return;
        };
        let Ok(Export::Figure(export)) = config.export(name) else {
            return;
        };
        let path = config.resolve(&export.output.to_string_lossy());
        if path.exists() {
            self.mode = Mode::Confirm {
                title: "replace a figure's file".to_string(),
                lines: vec![format!(
                    "{} is there already: y draws '{}' over it",
                    self.here(&path),
                    export.figure
                )],
                pending: Box::new(Pending::Figure {
                    output: Output::FigureExport(name.to_string()),
                    path,
                }),
            };
            return;
        }
        self.draw_figure_export(name, path);
    }

    /// The figure an export names, its query narrowing the samples and its
    /// style labelling the axes, drawn to `path`.
    fn draw_figure_export(&mut self, name: &str, path: PathBuf) {
        let Some(config) =
            self.declaring(|config| matches!(config.export(name), Ok(Export::Figure(_))))
        else {
            return;
        };
        let Ok(Export::Figure(export)) = config.export(name) else {
            return;
        };
        let export = export.clone();
        if config.figure(&export.figure).is_none() {
            self.draw_model_figure(&export.figure, Some(path));
            return;
        }
        let mut chosen = match self.chosen_samples() {
            Ok(chosen) => chosen,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        if let Some(query) = &export.query {
            let narrowed = crate::collection::named_queries::named(&config, query)
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
        self.draw_figure(&export.figure, chosen, Some(path), export.style);
    }

    fn draw_figure(
        &mut self,
        name: &str,
        mut chosen: SampleList,
        output: Option<PathBuf>,
        style: Option<String>,
    ) {
        // Declared by the selection's project, or by the root.
        let Some(config) = self.declaring(|config| config.figure(name).is_some()) else {
            self.message = format!("no figure '{name}' is declared for this selection");
            return;
        };
        let config = &config;
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
        // The root's figure draws every sample chosen; a project's, those of
        // the projects the selection is about — its own, or those of its
        // model.
        let root = config.root().to_path_buf();
        let from_the_root = self
            .collection
            .config()
            .is_some_and(|held| held.root() == root);
        let (scoped, _) = self.producing_configs();
        let mut allowed: Vec<PathBuf> = scoped
            .iter()
            .map(|held| held.root().to_path_buf())
            .collect();
        allowed.push(root.clone());
        let (samples, elsewhere): (Vec<PathBuf>, Vec<PathBuf>) = chosen
            .iter()
            .filter_map(|entry| entry.path.clone())
            .partition(|path| {
                from_the_root
                    || self
                        .collection
                        .config_for(path)
                        .is_some_and(|theirs| allowed.iter().any(|root| *root == theirs.root()))
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
            output,
            overrides: plotting::FigureOverrides {
                style,
                ..Default::default()
            },
            project_style: true,
            said: None,
            snapshot: None,
            workbench: true,
            // The workbench says it, and has no exit to give.
            accept_outdated: true,
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
        let file = request.output.clone();
        self.windowed = match (&file, &request.choice) {
            (Some(_), _) => None,
            (None, plotting::FigureChoice::Named { name, model: false }) => {
                Some(Output::Figure(name.clone()))
            }
            (None, plotting::FigureChoice::Named { name, model: true }) => {
                Some(Output::ModelFigure(name.clone()))
            }
            (None, _) => Some(Output::Own),
        };
        // Said from the project's folder, as every path the workbench says:
        // whole, it ran past the frame and hid the file's name. A file per
        // sample is counted.
        let written = file.as_ref().map(|path| {
            if path.to_string_lossy().contains("{name}") {
                counted(count, "file", "files")
            } else {
                self.here(path)
            }
        });
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let said = match plotting::draw_quietly(&python, &request) {
                Ok(()) => match &written {
                    Some(path) => format!("'{name}' written: {path}"),
                    None => format!("'{name}' closed"),
                },
                Err((error, said)) if said.is_empty() => error.to_string(),
                Err((_, said)) => said,
            };
            let _ = sender.send(Progress::Done(said));
        });
        self.drawing = Some(Running {
            receiver,
            before: None,
        });
        self.message = match file {
            Some(path) => format!(
                "writing '{drawn}' of {count} samples to {}",
                self.here(&path)
            ),
            None => format!("drawing '{drawn}' of {count} samples: its window opens beside"),
        };
    }

    /// A figure the model declares, of the basket or the sample under the
    /// cursor: the model's code, drawn as `plot` draws it, nothing asked.
    fn draw_model_figure(&mut self, name: &str, output: Option<PathBuf>) {
        // The model of the selection's projects, or the root's.
        let Some(config) = self.declaring(|config| {
            runtime::model_declarations(config)
                .0
                .iter()
                .any(|declared| declared == name)
        }) else {
            self.message = format!("no model of this selection declares the figure '{name}'");
            return;
        };
        let config = &config;
        if self.drawing.is_some() {
            self.message = "a figure is open: close its window first".to_string();
            return;
        }
        // Written to a file, the samples `p` acts on: the basket, or those
        // shown; in a window, the sample open, or the basket.
        let asked: Vec<PathBuf> = match (&output, self.chosen_samples()) {
            (Some(_), Ok(chosen)) => chosen
                .iter()
                .filter_map(|entry| entry.path.clone())
                .collect(),
            _ if self.basket.is_empty() => {
                self.current().map(|(path, _)| path).into_iter().collect()
            }
            _ => self.basket.iter().cloned().collect(),
        };
        // Python reads each sample with its own project's model: another
        // project's is not this figure's, so it is left out, as `plot` draws
        // one project's samples.
        // Projects of one model draw together: each reads its samples with
        // that model.
        let root = config.root().to_path_buf();
        let (scoped, _) = self.producing_configs();
        let mut allowed: Vec<PathBuf> = scoped
            .iter()
            .map(|held| held.root().to_path_buf())
            .collect();
        allowed.push(root.clone());
        let (samples, elsewhere): (Vec<PathBuf>, Vec<PathBuf>) =
            asked.into_iter().partition(|path| {
                self.collection
                    .config_for(path)
                    .is_some_and(|theirs| allowed.iter().any(|root| *root == theirs.root()))
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
            output,
            overrides: Default::default(),
            project_style: true,
            said: None,
            snapshot: None,
            workbench: true,
            // The workbench says it, and has no exit to give.
            accept_outdated: true,
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
    /// A declared export over the samples chosen: each project declaring it
    /// writes its own samples to its own declared file, as `samplekit export`
    /// does; a project declaring none is left out, and said. Every file is
    /// made before any is written: one that cannot be refuses them all.
    fn preview_export(&mut self, name: &str, chosen: SampleList) {
        let all = self.collection.filter_by(|_| true);
        let whole = self.collection.by_configuration(&all);
        let mut files: Vec<(exports::Prepared, Vec<PathBuf>, String, String)> = Vec::new();
        let mut left_out: Vec<String> = Vec::new();
        let mut refused: Option<String> = None;
        for part in self.collection.by_configuration(&chosen) {
            let project = part
                .config
                .map(|config| self.project_title(config.root()))
                .unwrap_or_else(|| "no project".to_string());
            let Some(config) = part.config.filter(|config| config.export(name).is_ok()) else {
                left_out.push(format!(
                    "{} not exported: {project} declares no export '{name}'",
                    counted(part.samples.len(), "sample", "samples")
                ));
                continue;
            };
            let described = whole
                .iter()
                .find(|other| other.file == part.file)
                .map_or(&self.collection, |other| &other.samples);
            let samples: Vec<PathBuf> = part
                .samples
                .iter()
                .filter_map(|entry| entry.path.clone())
                .collect();
            let said = self.export_command(name, config.root(), &samples);
            match exports::prepare(name, config, part.samples, described) {
                Ok(prepared) => files.push((prepared, samples, project, said)),
                Err(error) => {
                    refused = Some(format!("{project}: {error} — nothing written"));
                    break;
                }
            }
        }
        if let Some(refused) = refused {
            self.message = refused;
            return;
        }
        if files.is_empty() {
            self.message = left_out
                .first()
                .cloned()
                .unwrap_or_else(|| format!("no export '{name}' is declared for this selection"));
            return;
        }
        // Two projects writing one file: the second would replace the first.
        for (at, (prepared, _, project, _)) in files.iter().enumerate() {
            if let Some((_, _, other, _)) = files[..at]
                .iter()
                .find(|(held, ..)| normalised(&held.path) == normalised(&prepared.path))
            {
                self.message = format!(
                    "'{name}' would write {} for {other} and for {project}: name the \
                     project in its output, output = \"…/{{project}}/…\"",
                    self.here(&normalised(&prepared.path))
                );
                return;
            }
        }
        self.confirm_export(name, files, left_out, chosen.len());
    }

    /// An export's files prepared, previewed before `y` writes them: what each
    /// holds, whether it replaces a file, what was left out, and what in them
    /// is not current.
    fn confirm_export(
        &mut self,
        name: &str,
        prepared: Vec<(exports::Prepared, Vec<PathBuf>, String, String)>,
        left_out: Vec<String>,
        chosen: usize,
    ) {
        let of = if self.basket.is_empty() {
            format!(
                "{chosen} of {} shown",
                counted(self.collection.len(), "sample", "samples")
            )
        } else {
            format!("the basket's {}", counted(chosen, "sample", "samples"))
        };
        let mut files: Vec<ExportFile> = Vec::new();
        let mut behind = exports::Behind::default();
        let mut left_empty: Vec<String> = Vec::new();
        for (prepared, samples, _, said) in &prepared {
            // An uncertainty no sample holds is left out, and said, as the
            // command line leaves it: a column nobody can fill reads as zero.
            let (dataset, empty) = export_formats::without_empty_uncertainties(&prepared.dataset);
            let columns = match &prepared.format {
                Format::Json => false,
                Format::Text(document) => document.property.is_none(),
                _ => true,
            };
            if columns {
                for header in empty {
                    if !left_empty.contains(&header) {
                        left_empty.push(header);
                    }
                }
            }
            let text = match export_formats::serialize(&dataset, prepared.format.clone()) {
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
            // Every kind, a failed value too, which this left out.
            behind.add(&prepared.behind);
            let path = normalised(&prepared.path);
            files.push(ExportFile {
                before: std::fs::read(&path).ok(),
                text,
                path,
                rows: dataset.rows.len(),
                samples: samples.clone(),
                said: said.clone(),
            });
        }
        // Titled what and where; what it holds first.
        let rows: usize = files.iter().map(|file| file.rows).sum();
        let mut lines = vec![format!("{}, from {of}", counted(rows, "row", "rows"))];
        let title = if let [file] = files.as_slice() {
            if file.path.exists() {
                lines.push("the file is there, and y replaces it".to_string());
            }
            format!("export {name} · {}", self.here(&file.path))
        } else {
            for (file, (_, _, project, _)) in files.iter().zip(&prepared) {
                lines.push(format!(
                    "  {} · {} · {project}{}",
                    self.here(&file.path),
                    counted(file.rows, "row", "rows"),
                    if file.path.exists() {
                        " · there, y replaces it"
                    } else {
                        ""
                    }
                ));
            }
            format!("export {name} · {}", counted(files.len(), "file", "files"))
        };
        lines.extend(left_out);
        for header in left_empty {
            lines.push(format!(
                "{header} is left out: it is empty for every sample"
            ));
        }
        if behind.any() {
            lines.push(String::new());
            let kinds = behind.kinds();
            lines.push(format!(
                "written from values that are not current: {}",
                kinds.join(", ")
            ));
            for value in behind.named.iter().take(6) {
                lines.push(format!("  {value}"));
            }
            if behind.named.len() > 6 {
                lines.push(format!("  and {} more", behind.named.len() - 6));
            }
        }
        self.mode = Mode::Confirm {
            title,
            lines,
            pending: Box::new(Pending::Export {
                name: name.to_string(),
                files,
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
                        self.refresh_said();
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
                Key::PageDown => cursor = (cursor + self.page).min(last),
                Key::PageUp => cursor = cursor.saturating_sub(self.page),
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
            // The next group's first line, from the last back to the first.
            Key::Tab if matches!(purpose, Choosing::Produce(..)) => {
                if let Choosing::Produce(outputs, _) = &purpose {
                    let heading = |at: usize| {
                        shown
                            .get(at)
                            .is_some_and(|&item| outputs.get(item) == Some(&Output::Heading))
                    };
                    let next = (cursor + 1..shown.len())
                        .find(|&at| heading(at))
                        .or_else(|| (0..shown.len()).find(|&at| heading(at)));
                    if let Some(at) = next {
                        cursor = (at + 1).min(last);
                    }
                }
            }
            // A figure of `p` written to a file rather than drawn in a window.
            Key::Char('w') if matches!(purpose, Choosing::Produce(..)) => {
                if let (Choosing::Produce(outputs, _), Some(&at)) = (&purpose, shown.get(cursor))
                    && let Some(output @ (Output::Figure(_) | Output::ModelFigure(_))) =
                        outputs.get(at)
                {
                    self.ask_figure_file(output.clone());
                    return Effect::None;
                }
                self.message = "w writes a figure to a file: move to one".to_string();
            }
            Key::Char('j') | Key::Down => {
                cursor = off_heading(&purpose, &shown, (cursor + 1).min(last), false)
            }
            Key::Char('k') | Key::Up => {
                cursor = off_heading(&purpose, &shown, cursor.saturating_sub(1), true)
            }
            // As everywhere else a list moves: a sort or a column list runs
            // past forty entries.
            Key::Home => cursor = off_heading(&purpose, &shown, 0, false),
            Key::End => cursor = off_heading(&purpose, &shown, last, true),
            Key::PageDown => {
                cursor = off_heading(&purpose, &shown, (cursor + self.page).min(last), false)
            }
            Key::PageUp => {
                cursor = off_heading(&purpose, &shown, cursor.saturating_sub(self.page), true)
            }
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
                        // `Original hardness` went back to `radius`.
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
                    Choosing::Produce(mut outputs, title) => {
                        let Some(at) = under else {
                            return Effect::None;
                        };
                        // A heading runs nothing: the menu stays, whole.
                        let output = if outputs.get(at) == Some(&Output::Heading) {
                            Output::Heading
                        } else {
                            outputs.swap_remove(at)
                        };
                        match output {
                            Output::Heading => {
                                self.mode = Mode::Picker {
                                    purpose: Choosing::Produce(outputs, title),
                                    items,
                                    cursor,
                                    query,
                                    typing,
                                };
                            }
                            Output::Script(script) => {
                                let samples: Vec<PathBuf> = self
                                    .producing_selection()
                                    .iter()
                                    .filter_map(|entry| entry.path.clone())
                                    .collect();
                                if samples.is_empty() {
                                    self.message = "no sample to run a script on".to_string();
                                } else if script.options.is_empty() {
                                    self.start_script(&script.name, samples, Default::default());
                                } else {
                                    let given = script
                                        .options
                                        .iter()
                                        .map(|option| {
                                            format!(
                                                "{}={}",
                                                option.name,
                                                option.default.as_deref().unwrap_or_default()
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    self.mode = Mode::prompt(
                                        Purpose::ScriptOptions {
                                            script: *script,
                                            samples,
                                        },
                                        given,
                                    );
                                }
                            }
                            output => self.produce(output),
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
    /// The samples a wikilink may name, as the collection holds them, each
    /// its path and its `name:`.
    pub fn known_samples(&self) -> Vec<(PathBuf, Option<String>)> {
        self.collection
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.path.clone()?,
                    entry.sample.try_borrow().ok()?.name().map(str::to_string),
                ))
            })
            .collect()
    }

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
            link: None,
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
            link: None,
        });
        let known = self.known_samples();
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
            // A link as Markdown shows it: its text, marked where it leaves
            // the project or leads nowhere.
            let link = sample
                .attribute(name)
                .ok()
                .and_then(|value| crate::presentation::links::attribute(value, &path, &known));
            entries.push(Entry {
                field: name.to_string(),
                shown: link.as_ref().map_or(shown, |link| link.text.clone()),
                state: String::new(),
                kind: EntryKind::Attribute,
                link: link.map(|link| link.leads),
            });
        }
        // What the model computes: a value held there without its record is
        // one typed over the formula — marked, where it read as entered.
        let computed_by_model: Vec<String> = config
            .and_then(crate::config::model_runtime::current_description)
            .map(|described| {
                described
                    .properties
                    .iter()
                    .filter(|(_, declared)| declared.value.is_formula())
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .unwrap_or_default();
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
                // A formula changed since it computed the value first: current
                // by its inputs, it was said current, as `status` did not.
                state: owed_state(&field)
                    .filter(|said| said.contains("formula changed"))
                    .or_else(|| {
                        states
                            .get(name)
                            .map(|state| word_of(&sample, state))
                            .filter(|said| !said.is_empty())
                    })
                    .or_else(|| {
                        let typed_over = computed_by_model.contains(&field)
                            && sample.property(name).is_ok_and(|handle| {
                                handle.records().computed.is_none()
                                    && handle
                                        .peek(crate::core::property::Property::peek_value)
                                        .is_some_and(|value| !value.is_absent())
                            });
                        typed_over.then(|| "record missing".to_string())
                    })
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
                link: None,
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
                link: None,
            })
            .collect();
        entries.extend(absent);
        // What the model declares for a hand to fill and the file does not
        // hold yet — a new sample's `radius`, its first row of `ageing` — so
        // that `e` and `l` reach it: only an editor could before.
        let description = config.and_then(crate::config::model_runtime::current_description);
        if let Some(description) = &description {
            use crate::config::model_runtime::Origin;
            for (name, declared) in &description.properties {
                let entered = matches!(declared.value, Origin::Entered | Origin::Statistic { .. });
                if entered && !entries.iter().any(|entry| entry.field == *name) {
                    entries.push(Entry {
                        field: name.clone(),
                        shown: "—".to_string(),
                        state: TO_FILL.to_string(),
                        kind: EntryKind::Quantity,
                        link: None,
                    });
                }
            }
        }
        let not_current = editing::not_current(&sample);
        for name in sample.table_names() {
            let rows = sample
                .table(name)
                .map(|table| table.rows().count())
                .unwrap_or(0);
            let prefix = format!("{name}.");
            entries.push(Entry {
                field: name.to_string(),
                shown: format!("a table of {}  ▸", counted(rows, "row", "rows")),
                // A column the model owes, said on its table's line; else one
                // whose cells are not current — outdated since an input
                // changed, the line said nothing where its table marked each
                // cell.
                state: owed
                    .iter()
                    .find(|owed| owed.value.starts_with(&prefix))
                    .map(|owed| format!("{} {}", owed.value, owed.shown()))
                    .or_else(|| {
                        not_current
                            .iter()
                            .find(|value| value.name.starts_with(&prefix))
                            .map(|value| {
                                format!("{} {}", value.name, word_of(&sample, &value.state))
                            })
                    })
                    .unwrap_or_default(),
                kind: EntryKind::Table,
                link: None,
            });
        }
        if let Some(description) = &description {
            for name in description.tables.keys() {
                if !sample
                    .table_names()
                    .iter()
                    .any(|held| held.as_str() == name)
                {
                    entries.push(Entry {
                        field: name.clone(),
                        shown: "no row yet  ▸".to_string(),
                        state: TO_FILL.to_string(),
                        kind: EntryKind::Table,
                        link: None,
                    });
                }
            }
            // In the order the model declares them, whatever order the file
            // wrote them in: a new sample's `radius` came after `score`, and a
            // value written moved the lines under the cursor.
            let rank = |entry: &Entry| match entry.kind {
                EntryKind::Quantity => (
                    1,
                    description
                        .properties
                        .get_index_of(&entry.field)
                        .unwrap_or(usize::MAX),
                ),
                EntryKind::Table => (
                    2,
                    description
                        .tables
                        .get_index_of(&entry.field)
                        .unwrap_or(usize::MAX),
                ),
                _ => (0, 0),
            };
            entries.sort_by_key(rank);
        }
        // The samples linking to it, last: each followed as a link is.
        for (_, name, fields) in self.backlinks() {
            entries.push(Entry {
                field: format!("← {name}"),
                shown: fields.join(", "),
                state: "linked from".to_string(),
                kind: EntryKind::Backlink,
                link: None,
            });
        }
        entries
    }

    /// The samples of the collection whose links lead to the one shown: each
    /// its path, its name, and the attributes holding them.
    pub fn backlinks(&self) -> Vec<(PathBuf, String, Vec<String>)> {
        let Some((here, _)) = self.current() else {
            return Vec::new();
        };
        let canonical =
            |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let this = canonical(&here);
        let known = self.known_samples();
        let leads_here = |text: &str, from: &Path| match crate::core::link::parse(text) {
            Some(crate::core::link::Link::Sample { target, .. }) => matches!(
                crate::core::link::resolve(
                    &target,
                    known.iter().map(|(path, name)| (path.as_path(), name.as_deref()))
                ),
                crate::core::link::Resolved::One(path) if canonical(&path) == this
            ),
            Some(crate::core::link::Link::Path { path, .. }) => {
                let folder = from.parent().unwrap_or(Path::new("."));
                canonical(&crate::core::link::path_from(folder, &path)) == this
            }
            _ => false,
        };
        let mut found = Vec::new();
        for entry in self.collection.iter() {
            let Some(path) = entry.path.as_ref() else {
                continue;
            };
            if canonical(path) == this {
                continue;
            }
            let Ok(sample) = entry.sample.try_borrow() else {
                continue;
            };
            let fields: Vec<String> = sample
                .attribute_names()
                .into_iter()
                .filter(|name| {
                    sample.attribute(name).is_ok_and(|value| {
                        let texts: Vec<&crate::core::value::Value> = match value {
                            crate::core::sample::AttributeValue::Scalar(value) => vec![value],
                            crate::core::sample::AttributeValue::List(items) => {
                                items.iter().collect()
                            }
                        };
                        texts.into_iter().any(|value| {
                            matches!(value, crate::core::value::Value::Text(text) if leads_here(text, path))
                        })
                    })
                })
                .map(ToString::to_string)
                .collect();
            if !fields.is_empty() {
                let name = sample.name().map(str::to_string).unwrap_or_else(|| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
                found.push((path.clone(), name, fields));
            }
        }
        found
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
            Screen::Property { .. } => vec![("e", "edit"), ("c", "compute"), ("Enter", "explain")],
            Screen::Items {
                readings,
                ref field,
                ..
            } => {
                let mut hints = vec![("e", "edit"), ("d", "remove"), ("J K", "move")];
                // The tags are a list and stay one: `L` refuses them.
                if !readings && field != "tags" {
                    hints.push(("L", "list or single"));
                }
                hints
            }
            Screen::Sample { .. } => match self.entry() {
                None => Vec::new(),
                Some(entry) => match entry.kind {
                    EntryKind::Attribute => vec![("e", "edit")],
                    EntryKind::Backlink => vec![("Enter", "open")],
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
            // The property itself, as its sample's line says it.
            Screen::Property {
                ref name, ref up, ..
            } => match up.as_deref() {
                Some(table) => self.cell_on(table).map(|(entry, _)| entry),
                None => self
                    .entries()
                    .into_iter()
                    .find(|entry| entry.field == *name),
            },
            Screen::Items { .. }
            | Screen::Collection
            | Screen::Control { .. }
            | Screen::History { .. }
            | Screen::Setup { .. }
            | Screen::Configure => None,
        }
    }

    /// The cell under the cursor of an unfolded table, and its column as
    /// `compute` names it: `tempering.co2`.
    pub fn cell(&self) -> Option<(Entry, String)> {
        self.cell_on(&self.screen)
    }

    /// The cell `screen`, an unfolded table, has its cursor on.
    fn cell_on(&self, screen: &Screen) -> Option<(Entry, String)> {
        let Screen::Table {
            name,
            cursor,
            column,
            ..
        } = screen
        else {
            return None;
        };
        let (path, sample) = self.current()?;
        let table = sample.table(name).ok()?;
        let layout = table_layout(&sample, name, self.config_for(&path))?;
        let column_name = layout.cursor_columns().get(*column)?.clone();
        let row = table.rows().nth(*cursor)?;
        let index: Vec<crate::core::value::Value> = row.index().into_iter().cloned().collect();
        let field = cell_address(name, &column_name, index.clone());
        let derived = row
            .cell(&column_name)
            .is_ok_and(|cell| cell.records().computed.is_some());
        let state = if derived {
            crate::format::fingerprint::check_cell_among(&sample, name, &column_name, &index)
                .map(|state| word_of(&sample, &state))
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
                link: None,
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
                    let stored = explanation::nothing_stored(&sample, &entry.field);
                    lines.extend(explained_lines(&explained, &shown).into_iter().map(|line| {
                        if line == "nothing is stored here" {
                            stored.clone()
                        } else {
                            line
                        }
                    }));
                    // A value the model owes and the sample does not hold:
                    // what it is computed from, and the input nobody entered,
                    // as `explain` says it.
                    if let Explanation::Value(value) = &explained
                        && matches!(value.origin, explanation::Origin::Nothing)
                        && let Some(described) = config.and_then(runtime::current_description)
                        && let Some(property) = described.property(&entry.field)
                    {
                        let shown_path = self.here(&path);
                        lines.extend(explanation::owed_lines(
                            &described,
                            property,
                            &entry.field,
                            &sample,
                            &shown_path,
                        ));
                    }
                    // A formula changed since it computed it: outdated, as
                    // `status` says, though current by its inputs.
                    // A cell is its column's, whose formula it is: one of a
                    // column whose formula changed was said current.
                    let column = match fields::parse(&entry.field) {
                        Ok(fields::Field::Cell { table, column, .. }) => {
                            format!("{table}.{column}")
                        }
                        _ => entry.field.clone(),
                    };
                    if let Some(said) =
                        crate::collection::computation::planned_states(config, &path, &sample)
                            .and_then(|planned| {
                                planned
                                    .get(entry.field.as_str())
                                    .or_else(|| planned.get(&column))
                                    .cloned()
                            })
                            .filter(|said| said.contains("formula changed"))
                    {
                        let outdated =
                            format!("this value is outdated — {said}: C computes it again");
                        // In place of the line that called it current.
                        match lines
                            .iter()
                            .position(|line| line == "this value is current")
                        {
                            Some(at) => lines[at] = outdated,
                            None => lines.push(outdated),
                        }
                    }
                    lines
                }
                Err(_) => vec!["nothing to explain here".to_string()],
            };
        drop(sample);
        // Paths as the rest of the screen writes them, from the folder opened:
        // `./pieces/…` where the folder is `.`.
        let written = path.display().to_string();
        let lines = if written.starts_with("./") {
            let shown = self.here(&path);
            lines
                .into_iter()
                .map(|line| line.replace(&written, &shown))
                .collect()
        } else {
            lines
        };
        self.mode = Mode::Panel {
            title: format!("{} — where it came from", entry.field),
            lines,
            scroll: 0,
        };
    }

    // ------------------------------------------------------------ windows

    /// `Enter` on a sample's line: a property in its window, an attribute's
    /// value or items in theirs, a table unfolded.
    /// An attribute under the cursor that links elsewhere, followed: a sample
    /// it names, on its own screen; a file, in its application; a web
    /// address, in the browser. In a list's window, the item under the
    /// cursor. `None` where the cursor is on no link.
    fn follow_link(&mut self) -> Option<Effect> {
        let (field, item) = match &self.screen {
            Screen::Sample { .. } => {
                let entry = self
                    .entry()
                    .filter(|entry| entry.kind == EntryKind::Attribute)?;
                (entry.field, None)
            }
            Screen::Items {
                field,
                cursor,
                readings: false,
                ..
            } => (field.clone(), Some(*cursor)),
            _ => return None,
        };
        let name = Identifier::new(&field).ok()?;
        let (path, text) = {
            let (path, sample) = self.current()?;
            let text = match (sample.attribute(&name).ok()?, item) {
                (
                    crate::core::sample::AttributeValue::Scalar(crate::core::value::Value::Text(
                        text,
                    )),
                    None | Some(0),
                ) => text.clone(),
                (crate::core::sample::AttributeValue::List(values), Some(at)) => {
                    match values.get(at)? {
                        crate::core::value::Value::Text(text) => text.clone(),
                        _ => return None,
                    }
                }
                _ => return None,
            };
            (path, text)
        };
        let link = crate::core::link::parse(&text)?;
        let config = self.config_for(&path).cloned();
        let known: Vec<(PathBuf, Option<String>)> = self
            .view
            .iter()
            .filter_map(|entry| {
                let name = entry.sample.borrow().name().map(str::to_string);
                Some((entry.path.clone()?, name))
            })
            .collect();
        let leads = discovery::destination(&link, &path, || {
            discovery::linkable(config.as_ref(), &path, &known)
        });
        let written = text.trim();
        match leads {
            discovery::Destination::Address(address) => return Some(Effect::OpenAddress(address)),
            discovery::Destination::File(file) => return Some(Effect::Open(file)),
            discovery::Destination::Sample(found) => {
                let written = written.to_string();
                return Some(self.visit(found, &written));
            }
            discovery::Destination::Nowhere(target) if written.starts_with("[[") => {
                self.message = format!(
                    "{written} leads to no sample: no file of the project is named {target}, \
                     and no sample's name is"
                );
            }
            discovery::Destination::Nowhere(target) => {
                self.message = format!(
                    "{written} leads to nothing: no file is at {target}, from the sample's folder"
                );
            }
            discovery::Destination::Several(found) => {
                self.message = format!(
                    "{written} leads to {} samples: a link names one",
                    found.len()
                );
            }
        }
        Some(Effect::None)
    }

    /// The sample at `found` opened on its screen from the one shown, as a
    /// link `written` leads there: the way taken kept, `Esc` back along it; a
    /// sample the view does not hold shown after it while the way runs.
    fn visit(&mut self, found: PathBuf, written: &str) -> Effect {
        let same =
            |held: &PathBuf| dunce::canonicalize(held).ok() == dunce::canonicalize(&found).ok();
        let (left_path, left) = match self.current() {
            Some((path, sample)) => {
                let name = sample.name().map(str::to_string).unwrap_or_else(|| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
                (path, name)
            }
            None => return Effect::None,
        };
        let shown = self
            .view
            .iter()
            .position(|entry| entry.path.as_ref().is_some_and(same));
        // A sample the view does not hold, the filter's or another
        // project's, is shown after it while the way runs: from the
        // collection where it is there, else read as its own project's.
        let (at, guest) = match shown {
            Some(at) => (at, false),
            None => {
                let entry = match self
                    .collection
                    .iter()
                    .find(|entry| entry.path.as_ref().is_some_and(same))
                {
                    Some(entry) => entry.clone(),
                    None => match list::from_files(std::slice::from_ref(&found)) {
                        Ok(read) => match read.get(0).cloned() {
                            Some(entry) => {
                                self.guest_lists.push(read);
                                entry
                            }
                            None => return Effect::None,
                        },
                        Err(error) => {
                            self.message = format!("{written} → {}: {error}", self.here(&found));
                            return Effect::None;
                        }
                    },
                };
                self.guests.push(entry.clone());
                self.view = self.view.with_more(vec![entry]);
                (self.view.len() - 1, true)
            }
        };
        self.trail.push(LinkStep {
            left: self.screen.clone(),
            name: left.clone(),
            path: left_path,
            guest,
        });
        self.screen = Screen::Sample { at, cursor: 0 };
        self.message = format!(
            "{written} → {}{} — Esc goes back to {left}",
            self.here(&found),
            if guest { ", not among those shown" } else { "" }
        );
        Effect::None
    }

    fn open_window(&mut self) {
        let Screen::Sample { at, cursor } = self.screen else {
            return;
        };
        let Some(entry) = self.entry() else {
            return;
        };
        match entry.kind {
            EntryKind::Backlink => {}
            EntryKind::Quantity if !entry.field.contains('[') => {
                self.screen = Screen::Property {
                    at,
                    back: cursor,
                    name: entry.field,
                    column: 0,
                    up: None,
                }
            }
            // The name is one value, never a list: its line. The tags are a
            // list, in the window every list has.
            EntryKind::Attribute if entry.field == "name" => self.begin_edit(),
            EntryKind::Attribute => {
                self.screen = Screen::Items {
                    at,
                    field: entry.field,
                    readings: false,
                    cursor: 0,
                    up: Box::new(Screen::Sample { at, cursor }),
                }
            }
            EntryKind::Table => {
                if let Ok(name) = Identifier::new(&entry.field) {
                    self.screen = Screen::Table {
                        at,
                        back: cursor,
                        name,
                        cursor: 0,
                        column: 0,
                    };
                }
            }
            EntryKind::Quantity => self.explain(),
        }
    }

    /// The cell under a table's cursor in its window, as a property's value
    /// is; `edit` begins with its value typed. A cell of the index, or of
    /// text, is changed on one line, and explained.
    fn open_cell_window(&mut self, edit: bool) {
        let Screen::Table { at, back, .. } = self.screen else {
            return;
        };
        if let Some((path, field, text)) = self.index_cell() {
            if edit {
                self.mode = Mode::prompt(Purpose::Edit { path, field }, text);
            } else {
                self.explain();
            }
            return;
        }
        let Some((entry, _)) = self.cell() else {
            return;
        };
        let text = self.current().is_some_and(|(_, sample)| {
            matches!(
                editing::value_at(&sample, &entry.field),
                Some(crate::core::value::Value::Text(_))
            )
        });
        if text {
            if edit {
                self.begin_text_edit(entry.field);
            } else {
                self.explain();
            }
            return;
        }
        let up = Box::new(self.screen.clone());
        self.screen = Screen::Property {
            at,
            back,
            name: entry.field,
            column: 0,
            up: Some(up),
        };
        if edit {
            self.edit_property_cell();
        }
    }

    /// The unit of the quantity a window shows, as the screen writes it —
    /// `°C` where `[unit.*]` gives a plain form; empty where it has none.
    pub fn window_unit(&self) -> String {
        let Screen::Property { ref name, .. } = self.screen else {
            return String::new();
        };
        let Some((path, sample)) = self.current() else {
            return String::new();
        };
        let own = match name.split_once('.') {
            Some((table, column)) if name.contains('[') => {
                let column = column.split('[').next().unwrap_or(column);
                Identifier::new(table)
                    .ok()
                    .zip(Identifier::new(column).ok())
                    .and_then(|(table, column)| {
                        let table = sample.table(&table).ok()?;
                        table.column(&column).ok()?.presentation().unit.clone()
                    })
            }
            _ => Identifier::new(name)
                .ok()
                .and_then(|name| sample.property(&name).ok())
                .and_then(|handle| handle.presentation().unit),
        };
        drop(sample);
        let config = self.config_for(&path).or_else(|| self.collection.config());
        let unit = own
            .or_else(|| {
                config
                    .and_then(|config| config.property(name))
                    .and_then(|declared| declared.unit.clone())
            })
            .unwrap_or_default();
        config
            .and_then(|config| config.unit(&unit))
            .and_then(|variants| variants.get("plain").cloned())
            .unwrap_or(unit)
    }

    /// A value changed on one line, as typed.
    fn begin_text_edit(&mut self, field: String) {
        let Some((path, sample)) = self.current() else {
            return;
        };
        let text = editing::value_at(&sample, &field)
            .map(|value| denoised(&value))
            .filter(|shown| shown != "—")
            .unwrap_or_default();
        drop(sample);
        self.mode = Mode::prompt(Purpose::Edit { path, field }, text);
    }

    /// The columns of a property's window: its value and uncertainty, and its
    /// readings where no formula gives its value.
    pub fn property_columns(&self, name: &str) -> Vec<&'static str> {
        use crate::config::model_runtime::Origin;
        use crate::core::property::InputName;
        // A table's cell: its value and its uncertainty.
        if name.contains('[') {
            return vec!["value", "uncertainty"];
        }
        // A formula's, as the model declares it, as the workbench's sample
        // holds it, or as its file records it: a record naming anything but
        // the value's own readings — a sample read without the model's
        // Python holds no formula.
        let current = self.current();
        let described = current
            .as_ref()
            .and_then(|(path, _)| self.config_for(path))
            .and_then(crate::config::model_runtime::current_description)
            .and_then(|described| described.properties.get(name).cloned())
            .is_some_and(|property| {
                matches!(
                    property.value,
                    Origin::Formula { .. } | Origin::Quantity { .. }
                )
            });
        let held = current
            .and_then(|(_, sample)| {
                let name = Identifier::new(name).ok()?;
                let handle = sample.property(&name).ok()?;
                let readings = Identifier::new("readings").ok()?;
                Some(
                    handle.is_computed()
                        || handle.records().computed.is_some_and(|inputs| {
                            inputs
                                .keys()
                                .any(|input| *input != InputName::Named(readings.clone()))
                        }),
                )
            })
            .unwrap_or(false);
        let computed = described || held;
        if computed {
            vec!["value", "uncertainty"]
        } else {
            vec!["value", "uncertainty", "readings"]
        }
    }

    /// What the model declares a property to be, as the window's first line
    /// says it: `the value is the mean of its readings, the uncertainty their
    /// standard error`.
    pub fn declared_as(&self, name: &str) -> String {
        use crate::config::model_runtime::Origin;
        let Some((path, _)) = self.current() else {
            return String::new();
        };
        let Some(described) = self
            .config_for(&path)
            .and_then(crate::config::model_runtime::current_description)
        else {
            return String::new();
        };
        // A cell is its column's: a table's formula, a statistic of its
        // readings, or entered — said of every cell, a computed one included,
        // *entered by hand*.
        if let Ok(fields::Field::Cell { table, column, .. }) = fields::parse(name) {
            let Some(declared) = described.tables.get(table.as_str()).and_then(|held| {
                held.columns
                    .iter()
                    .find(|held| held.name == column.as_str())
            }) else {
                return "the model does not declare its column: entered by hand".to_string();
            };
            let from = |reads: &[String]| {
                if reads.is_empty() {
                    String::new()
                } else {
                    format!(", from {}", reads.join(", "))
                }
            };
            if let Some(Origin::Rows { reads, .. }) = &declared.uncertainty {
                return format!(
                    "its value is entered, a table's formula gives its uncertainty{}",
                    from(reads)
                );
            }
            return match &declared.value {
                Origin::Rows { reads, .. } | Origin::Columns { reads, .. } => {
                    format!("a table's formula gives it{}", from(reads))
                }
                Origin::Statistic { statistic } => format!(
                    "the value is the {} of its readings",
                    statistic.replace('_', " ")
                ),
                _ => "its column is entered".to_string(),
            };
        }
        let Some(property) = described.properties.get(name) else {
            return "the model does not declare it: entered by hand".to_string();
        };
        let reads = |reads: &[String]| {
            if reads.is_empty() {
                String::new()
            } else {
                format!(", from {}", reads.join(", "))
            }
        };
        let value = match &property.value {
            Origin::Entered => "the value is entered".to_string(),
            Origin::Formula { reads: from } => format!("a formula gives the value{}", reads(from)),
            Origin::Quantity { reads: from } => {
                return format!(
                    "one formula gives the value and its uncertainty{}",
                    reads(from)
                );
            }
            Origin::Statistic { statistic } => {
                format!(
                    "the value is the {} of its readings",
                    statistic.replace('_', " ")
                )
            }
            Origin::Rows { .. } | Origin::Columns { .. } => {
                "a table's formula gives it".to_string()
            }
        };
        let uncertainty = match &property.uncertainty {
            None => String::new(),
            Some(Origin::Entered) => ", the uncertainty entered".to_string(),
            Some(Origin::Statistic { statistic }) => {
                format!(", the uncertainty their {}", statistic.replace('_', " "))
            }
            Some(Origin::Formula { reads: from } | Origin::Quantity { reads: from }) => {
                format!(", a formula gives the uncertainty{}", reads(from))
            }
            Some(_) => String::new(),
        };
        format!("{value}{uncertainty}")
    }

    /// `e` in a property's window: its value or uncertainty on one line, its
    /// readings in their own window.
    fn edit_property_cell(&mut self) {
        let Screen::Property {
            at,
            back,
            ref name,
            column,
            ref up,
        } = self.screen
        else {
            return;
        };
        let up = up.clone();
        let name = name.clone();
        let columns = self.property_columns(&name);
        let Some(which) = columns.get(column).copied() else {
            return;
        };
        let Some((path, sample)) = self.current() else {
            return;
        };
        match which {
            "readings" => {
                drop(sample);
                self.screen = Screen::Items {
                    at,
                    field: name.clone(),
                    readings: true,
                    cursor: 0,
                    up: Box::new(Screen::Property {
                        at,
                        back,
                        name,
                        column,
                        up,
                    }),
                };
            }
            _ => {
                let field = if which == "uncertainty" {
                    format!("{name}.u")
                } else {
                    name.clone()
                };
                let text = editing::value_at(&sample, &field)
                    .map(|value| denoised(&value))
                    .filter(|shown| shown != "—")
                    .unwrap_or_default();
                drop(sample);
                self.mode = Mode::prompt(Purpose::Edit { path, field }, text);
            }
        }
    }

    /// The list a window of items shows: its file, its items as typed, and
    /// whether the attribute is a list — readings always are.
    pub fn items(&self) -> Option<(PathBuf, Vec<String>, bool)> {
        let Screen::Items {
            ref field,
            readings,
            ..
        } = self.screen
        else {
            return None;
        };
        let (path, sample) = self.current()?;
        let name = Identifier::new(field).ok()?;
        if readings {
            let held = sample
                .property(&name)
                .ok()
                .and_then(|handle| handle.readings())
                .map(|readings| {
                    readings
                        .as_slice()
                        .iter()
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            return Some((path, held, true));
        }
        // The tags, a list whatever they hold.
        if field == "tags" {
            return Some((
                path,
                sample.tags().iter().map(ToString::to_string).collect(),
                true,
            ));
        }
        match sample.attribute(&name) {
            Ok(crate::core::sample::AttributeValue::List(items)) => {
                Some((path, items.iter().map(editing::shown_value).collect(), true))
            }
            Ok(crate::core::sample::AttributeValue::Scalar(value)) => Some((
                path,
                if value.is_absent() {
                    Vec::new()
                } else {
                    vec![editing::shown_value(value)]
                },
                false,
            )),
            Err(_) => Some((path, Vec::new(), false)),
        }
    }

    /// `e` in a window of items: the item under the cursor, or a new one on
    /// the empty row at the foot.
    fn edit_item(&mut self) {
        let Screen::Items {
            ref field,
            readings,
            cursor,
            ..
        } = self.screen
        else {
            return;
        };
        let field = field.clone();
        let Some((path, items, _)) = self.items() else {
            return;
        };
        let at = (cursor < items.len()).then_some(cursor);
        let text = at.map(|at| items[at].clone()).unwrap_or_default();
        self.mode = Mode::prompt(
            Purpose::Item {
                path,
                field,
                readings,
                at,
            },
            text,
        );
    }

    /// An item typed: the list with it, previewed.
    fn type_item(
        &mut self,
        path: PathBuf,
        field: &str,
        readings: bool,
        at: Option<usize>,
        text: &str,
    ) {
        let Some((_, mut items, list)) = self.items() else {
            return;
        };
        match at {
            Some(at) if text.is_empty() => {
                items.remove(at);
            }
            Some(at) => items[at] = text.to_string(),
            None if text.is_empty() => return,
            // Readings typed on the empty row, several at once: a first one
            // alone is no list of readings.
            None if readings => items.extend(
                text.split(',')
                    .map(str::trim)
                    .filter(|piece| !piece.is_empty())
                    .map(str::to_string),
            ),
            None => items.push(text.to_string()),
        }
        // A second item makes a single value a list; the preview says so.
        let list = list || items.len() > 1;
        self.preview_items(path, field, readings, items, list);
    }

    /// `d`, `J`, `K` and `L` in a window of items.
    fn change_items(&mut self, change: ItemChange) {
        let Screen::Items {
            ref field,
            readings,
            cursor,
            ..
        } = self.screen
        else {
            return;
        };
        let field = field.clone();
        let Some((path, mut items, list)) = self.items() else {
            return;
        };
        let mut list = list;
        let mut moved_to = cursor;
        match change {
            ItemChange::Remove if cursor < items.len() => {
                items.remove(cursor);
            }
            ItemChange::Down if cursor + 1 < items.len() => {
                items.swap(cursor, cursor + 1);
                moved_to = cursor + 1;
            }
            ItemChange::Up if cursor > 0 && cursor < items.len() => {
                items.swap(cursor, cursor - 1);
                moved_to = cursor - 1;
            }
            ItemChange::Toggle if readings => {
                self.message = "readings are a list, and stay one".to_string();
                return;
            }
            ItemChange::Toggle if field == "tags" => {
                self.message = "tags are a list, and stay one".to_string();
                return;
            }
            ItemChange::Toggle if list && items.len() > 1 => {
                self.message = format!(
                    "{field} holds {} items: take out all but one first, d on each",
                    items.len()
                );
                return;
            }
            ItemChange::Toggle => list = !list,
            _ => return,
        }
        if let Screen::Items { cursor, .. } = &mut self.screen {
            *cursor = moved_to;
        }
        self.preview_items(path, &field, readings, items, list);
    }

    /// A list of items previewed as the file would take it: readings as
    /// `set --readings` writes them, an attribute as a list or its single value.
    fn preview_items(
        &mut self,
        path: PathBuf,
        field: &str,
        readings: bool,
        items: Vec<String>,
        list: bool,
    ) {
        // The tags are written as tags, not as an attribute of that name.
        if field == "tags" {
            self.preview_tags(path, &items.join(", "));
            return;
        }
        if readings {
            if items.len() == 1 {
                self.message = "readings are several: type two or more on the empty row, \
                                11, 13 — one number is a value, typed in the value's cell"
                    .to_string();
                return;
            }
            self.preview_quantity(path, field, Some(items.join(", ")), &[]);
            return;
        }
        if !list {
            let text = items.first().cloned().unwrap_or_default();
            self.preview_edit(path, field, &text);
            return;
        }
        let outcome = (|| -> Result<(Vec<String>, Pending), String> {
            let (mut sample, origin) =
                document::load_sample(&path).map_err(|error| error.to_string())?;
            let values = items
                .iter()
                .map(|item| editing::value_of(item).map_err(|error| error.message))
                .collect::<Result<Vec<_>, _>>()?;
            let (before, after) =
                editing::assign_list(&mut sample, field, values).map_err(|error| error.message)?;
            let mut lines = vec![format!("{field}   {before}  →  {after}")];
            if !before.starts_with('[') && before != "—" {
                lines.push("  a single value becomes a list".to_string());
            }
            let original = document::load_sample(&path).ok().map(|(held, _)| held);
            let lines = confirmed(&lines, original.as_ref(), &sample, self.config_for(&path));
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
                    title: format!("change · {}", self.here(&path)),
                    lines,
                    pending: Box::new(pending),
                }
            }
            Err(message) => self.message = message,
        }
    }

    /// The index cell under the cursor of an unfolded table: its file, its
    /// address, and its value as typed.
    fn index_cell(&self) -> Option<(PathBuf, String, String)> {
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
        let index = table.index_columns().to_vec();
        let at = index.get(*column)?.clone();
        let row = table.rows().nth(*cursor)?;
        let values: Vec<crate::core::value::Value> = row.index().into_iter().cloned().collect();
        let text = values.get(*column).map(denoised).unwrap_or_default();
        let field = cell_address(name, &at, values);
        Some((path.clone(), field, text))
    }

    /// Whether the cursor is on the empty row at an unfolded table's foot.
    pub fn on_the_empty_row(&self) -> bool {
        let Screen::Table { name, cursor, .. } = &self.screen else {
            return false;
        };
        let rows = self
            .current()
            .and_then(|(_, sample)| sample.table(name).ok().map(|table| table.rows().count()))
            .unwrap_or(0);
        *cursor == rows
    }

    /// A cell of the row at a table's foot, typed: what was typed there
    /// before offered again.
    fn begin_draft_cell(&mut self) {
        let Screen::Table { name, column, .. } = &self.screen else {
            return;
        };
        let (name, column) = (name.clone(), *column);
        let Some((path, sample)) = self.current() else {
            return;
        };
        let Some(layout) = table_layout(&sample, &name, self.config_for(&path)) else {
            return;
        };
        drop(sample);
        let Some(column) = layout.cursor_columns().get(column).cloned() else {
            return;
        };
        if layout.derived.contains(&column.to_string()) {
            self.message = format!(
                "{name}.{column} is computed by the model: C computes it once the row is written"
            );
            return;
        }
        let table = name.to_string();
        let same = self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.path == path && draft.table == table);
        if !same {
            self.draft = Some(Draft {
                path: path.clone(),
                table: table.clone(),
                cells: Vec::new(),
            });
        }
        let text = self
            .draft
            .as_ref()
            .and_then(|draft| draft.at(column.as_str()))
            .unwrap_or_default()
            .to_string();
        self.mode = Mode::prompt(
            Purpose::DraftCell {
                path,
                table,
                column: column.to_string(),
            },
            text,
        );
    }

    /// A cell of the row at a table's foot, kept; once every column of its
    /// index is given, the row is previewed, to be written.
    fn type_draft_cell(
        &mut self,
        path: PathBuf,
        table: String,
        column: String,
        text: &str,
    ) -> bool {
        // A decimal comma, `4,5`, said at once, as a row written whole says
        // it: kept, the row was refused only once its index was given.
        if text.contains(',') && text.replace(',', ".").parse::<f64>().is_ok() {
            self.message = format!(
                "'{text}' reads as a decimal comma: a point is written for decimals — {}",
                text.replace(',', ".")
            );
            return false;
        }
        let draft = self.draft.get_or_insert_with(|| Draft {
            path: path.clone(),
            table: table.clone(),
            cells: Vec::new(),
        });
        draft.cells.retain(|(held, _)| *held != column);
        if !text.is_empty() {
            draft.cells.push((column, text.to_string()));
        }
        let draft = draft.clone();
        let index: Vec<String> = self
            .current()
            .and_then(|(path, sample)| {
                let name = Identifier::new(&table).ok()?;
                table_layout(&sample, &name, self.config_for(&path))
            })
            .map(|layout| layout.index.iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        if index.is_empty() || index.iter().any(|column| draft.at(column).is_none()) {
            self.message = format!(
                "kept: the row is written once its index — {} — is given",
                index.join(", ")
            );
            self.mode = Mode::Normal;
            return true;
        }
        // The index first, as a row is written.
        let mut cells: Vec<&(String, String)> = draft.cells.iter().collect();
        cells.sort_by_key(|(column, _)| !index.contains(column));
        let text = cells
            .iter()
            .map(|(column, text)| format!("{column}={text}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.preview_row(path, &table, &text);
        !matches!(self.mode, Mode::Normal)
    }

    fn begin_edit(&mut self) {
        if self.on_the_empty_row() {
            self.begin_draft_cell();
            return;
        }
        // A row's index, changed as a cell is: one line, previewed as the row
        // moving.
        if let Some((path, field, text)) = self.index_cell() {
            self.mode = Mode::prompt(Purpose::Edit { path, field }, text);
            return;
        }
        let Some(entry) = self.entry() else {
            return;
        };
        if entry.kind == EntryKind::Table {
            self.message = format!(
                "{} is a table: → opens it, where e changes a cell, the index's too, and the \
                 empty row at its foot adds one",
                entry.field
            );
            return;
        }
        let Some((path, sample)) = self.current() else {
            return;
        };
        // The tags are a list: `e` opens their window, as `Enter` does.
        if entry.field == "tags" && entry.kind == EntryKind::Attribute {
            drop(sample);
            if let Screen::Sample { at, cursor } = self.screen {
                self.screen = Screen::Items {
                    at,
                    field: "tags".to_string(),
                    readings: false,
                    cursor: 0,
                    up: Box::new(Screen::Sample { at, cursor }),
                };
            }
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
        // A quantity is changed in its window, its value first: the same
        // window `Enter` opens, a cell of a table's as a property's. A text
        // cell is one line.
        let numeric = !matches!(
            editing::value_at(&sample, &entry.field),
            Some(crate::core::value::Value::Text(_))
        );
        if entry.kind == EntryKind::Quantity && numeric {
            drop(sample);
            match self.screen {
                // A cell holding a value is changed in its window, opened by
                // `Enter`, where its value and its uncertainty are side by
                // side: typed from the table, a row filled cell by cell sent
                // the next cell's number into the uncertainty.
                Screen::Table { .. } => {
                    self.message = "Enter opens the cell, its value and its uncertainty: e there \
                                    changes them"
                        .to_string();
                }
                Screen::Sample { .. } => {
                    self.open_window();
                    if matches!(self.screen, Screen::Property { .. }) {
                        self.edit_property_cell();
                    }
                }
                _ => {}
            }
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

    fn prompting(&mut self, key: Key, purpose: Purpose, mut line: Typed) -> Effect {
        match key {
            Key::Esc if matches!(purpose, Purpose::FigureText(_)) => {
                self.mode = Mode::Figure;
                return Effect::None;
            }
            Key::Esc => {
                // Not naming a machine keeps its host's name, so that it is
                // not asked again.
                if let Purpose::NameMachine {
                    identifier,
                    host,
                    sample,
                } = &purpose
                {
                    self.name_machine(identifier, host, *sample);
                }
                return Effect::None;
            }
            Key::Enter => {
                // A line refused keeps its window, its text and what is said,
                // as the quantity's window does: closed, the text was lost,
                // and what was typed again fell on the screen as its keys.
                let again = purpose.clone();
                self.message.clear();
                let text = line.text().to_string();
                let accepted = match purpose {
                    Purpose::ScriptOptions { script, samples } => {
                        match typed_options(&text, &script) {
                            Ok(options) => {
                                self.start_script(&script.name, samples, options);
                                true
                            }
                            Err(error) => {
                                self.message = error;
                                false
                            }
                        }
                    }
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
                    // A value or an uncertainty typed in a quantity's window
                    // is a number alone: `21 L` or `21,5` is said at once.
                    Purpose::Edit { .. }
                        if matches!(self.screen, Screen::Property { .. })
                            && number_refused(text.trim(), &self.window_unit()).is_some() =>
                    {
                        self.message =
                            number_refused(text.trim(), &self.window_unit()).unwrap_or_default();
                        false
                    }
                    Purpose::Edit { path, field } => {
                        self.preview_edit(path, &field, text.trim());
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Tag => {
                        self.preview_tag(&text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Rename { path } => {
                        self.preview_rename(&path, text.trim());
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Tags { path } => {
                        self.preview_tags(path, &text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::AttributeName { path } => self.name_attribute(path, text.trim()),
                    Purpose::New { beside } => {
                        self.preview_new(beside, &text);
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::Setting { section, name, key } => {
                        self.set_setting(section, name, &key, &text)
                    }
                    Purpose::AddSetting { section } => self.add_setting(section, &text),
                    Purpose::AddKey { section, name } => self.add_key(section, name, &text),
                    Purpose::DraftCell {
                        path,
                        table,
                        column,
                    } => self.type_draft_cell(path, table, column, text.trim()),
                    Purpose::Item {
                        path,
                        field,
                        readings,
                        at,
                    } => {
                        self.type_item(path, &field, readings, at, text.trim());
                        !matches!(self.mode, Mode::Normal)
                    }
                    Purpose::ModelPath => self.answer_model_path(&text),
                    Purpose::FigureFile(output) => self.write_figure(output, &text),
                    Purpose::ShownFile { .. } => self.preview_shown(&text),
                    Purpose::NameMachine {
                        identifier,
                        host,
                        sample,
                    } => {
                        let name = if text.trim().is_empty() {
                            host
                        } else {
                            text.trim().to_string()
                        };
                        self.name_machine(&identifier, &name, sample);
                        true
                    }
                };
                if !accepted && !self.message.is_empty() && matches!(self.mode, Mode::Normal) {
                    self.mode = Mode::Prompt {
                        purpose: again,
                        text: line,
                    };
                }
                return Effect::None;
            }
            // A link being typed, `[[st`, completed with a sample's name.
            Key::Tab if matches!(purpose, Purpose::Edit { .. }) => self.complete_link(&mut line),
            _ => {
                let before = line.text().to_string();
                line.key(key);
                // A refusal said of what was typed goes once it is typed
                // over: it stood under a new name as if said of it.
                if line.text() != before {
                    self.message.clear();
                }
            }
        }
        self.mode = Mode::Prompt {
            purpose,
            text: line,
        };
        Effect::None
    }

    /// `[[st` before the cursor completed as far as the project's samples
    /// agree, by their files' names; closed, `]]`, where one alone answers;
    /// those that do said where several do.
    fn complete_link(&mut self, line: &mut Typed) {
        let before = &line.text()[..line.at()];
        let Some(open) = before
            .rfind("[[")
            .filter(|open| !before[*open..].contains("]]"))
        else {
            return;
        };
        let typed = before[open + 2..].to_string();
        let mut names: Vec<String> = self
            .collection
            .iter()
            .filter_map(|entry| {
                let stem = entry
                    .path
                    .as_ref()?
                    .file_stem()?
                    .to_string_lossy()
                    .into_owned();
                Some(
                    stem.strip_suffix(".sample")
                        .map(str::to_string)
                        .unwrap_or(stem),
                )
            })
            .filter(|name| name.starts_with(&typed))
            .collect();
        names.sort();
        names.dedup();
        let head = before[..open + 2].to_string();
        match names.as_slice() {
            [] => self.message = format!("no sample's file is named {typed}…"),
            [one] => line.complete(&format!("{head}{one}]]")),
            several => {
                let common = several[1..]
                    .iter()
                    .fold(several[0].clone(), |common, name| {
                        common
                            .chars()
                            .zip(name.chars())
                            .take_while(|(a, b)| a == b)
                            .map(|(a, _)| a)
                            .collect()
                    });
                if common.len() > typed.len() {
                    line.complete(&format!("{head}{common}"));
                }
                let shown: Vec<&str> = several.iter().take(8).map(String::as_str).collect();
                self.message = format!(
                    "{}{}",
                    shown.join(", "),
                    if several.len() > 8 { ", …" } else { "" }
                );
            }
        }
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
            let before = editing::not_current(&sample);
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
                if let Some(refused) = editing::readings_refused_by_model(config, name.as_str()) {
                    return Err(refused.message);
                }
                editing::set_readings(&mut sample, &name, parsed).map_err(|error| error.message)?;
                lines.push(format!("{field}.readings   {text}"));
                let value_typed = assignments
                    .iter()
                    .any(|(assigned, text)| assigned == field && !text.trim().is_empty());
                let emptied = assignments
                    .iter()
                    .any(|(assigned, text)| assigned == field && text.trim().is_empty());
                // A quantity the model declares no statistic of: its readings
                // give no value, which the preview did not say — and said an
                // empty value lets them give one.
                let no_statistic = config
                    .and_then(crate::config::model_runtime::current_description)
                    .and_then(|description| {
                        description.properties.get(field).map(|declared| {
                            !matches!(
                                declared.value,
                                crate::config::model_runtime::Origin::Statistic { .. }
                            )
                        })
                    })
                    .unwrap_or(false);
                lines.push(match kept {
                    _ if no_statistic && (value_typed || (kept.is_some() && !emptied)) => format!(
                        "  {count} readings; the model declares no statistic of them: the value \
                         written beside them stays the value"
                    ),
                    _ if no_statistic => format!(
                        "  {count} readings; the model declares no statistic of them: they give \
                         no value"
                    ),
                    _ if emptied => {
                        format!("  {count} readings; the value emptied, c gives it their statistic")
                    }
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
                    _ => format!(
                        "  {count} readings; no statistic of them is recorded: c takes the one \
                         the model declares, and where it declares none they give no value"
                    ),
                });
            }
            for (field, text) in assignments {
                // Another sample's name is refused, as `set` refuses it.
                if field == "name" && !text.trim().is_empty() && sample.name() != Some(text.trim())
                {
                    let project = config.map(|config| config.root().to_path_buf());
                    if let Some(refused) = editing::refuses_rename(
                        text.trim(),
                        &self.collection,
                        project.as_deref(),
                        &path,
                    ) {
                        return Err(refused.message);
                    }
                }
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
                let mut change = editing::apply_known(&mut sample, field, value, known.as_ref())
                    .map_err(|error| error.message)?;
                editing::over_the_model(&sample, &mut change, config);
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
                    Became::Override => {
                        lines.push("  typed over its formula, which stays".to_string())
                    }
                    // A value the model computes, typed where it was never
                    // computed: an override, as over a value it computed —
                    // said *new*, it was then held without a mark.
                    Became::NewQuantity
                        if config
                            .and_then(crate::config::model_runtime::current_description)
                            .and_then(|described| described.properties.get(field).cloned())
                            .is_some_and(|declared| declared.value.is_formula()) =>
                    {
                        lines.push(
                            "  typed over its formula: C keeps it, and c on it gives it back to \
                             its formula"
                                .to_string(),
                        );
                    }
                    Became::NewQuantity => lines.push(
                        match known.clone().map(|known| known.from) {
                            Some(editing::KnownFrom::Model(_)) => {
                                "  new, a property the model declares"
                            }
                            Some(editing::KnownFrom::Samples) => {
                                "  new, a property as the other samples hold it"
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
            // As every surface says a change: whole, then what it makes
            // not current.
            let original = document::load_sample(&path).ok().map(|(held, _)| held);
            let mut lines = confirmed(&lines, original.as_ref(), &sample, config);
            lines.push(String::new());
            lines.extend(said_not_current(&sample, &before));
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
                    title: format!("change · {}", self.here(&path)),
                    lines,
                    pending: Box::new(pending),
                }
            }
            // Said as the window is typed in, not as `set` is: `volume=1.05`
            // is the command line's.
            Err(message) => self.message = in_the_windows_words(&message),
        }
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
        // As every surface says a change of tags: `+prize -gift` in the
        // value column.
        let mut said: Vec<String> = before
            .iter()
            .filter(|tag| !tags.contains(tag))
            .map(|tag| format!("-{tag}"))
            .collect();
        said.extend(
            tags.iter()
                .filter(|tag| !before.contains(tag))
                .map(|tag| format!("+{tag}")),
        );
        let lines = if said.is_empty() {
            Vec::new()
        } else {
            crate::presentation::changes::change_lines(&[
                crate::presentation::changes::FieldChange::said("tags", said.join(" ")),
            ])
        };
        if lines.is_empty() {
            self.message = "the tags are unchanged".to_string();
            return;
        }
        sample.set_tags(tags);
        self.mode = Mode::Confirm {
            title: format!("tags · {}", self.here(&path)),
            lines,
            pending: Box::new(Pending::Write {
                path,
                sample: Box::new(sample),
                origin,
            }),
        };
    }

    fn preview_tag(&mut self, text: &str) {
        // `old=new` renames a tag, as `tag rename` does: over the basket, or
        // every sample shown where it is empty.
        if let Some((from, to)) = text.split_once('=') {
            let (from, to) = (from.trim(), to.trim());
            let (from, to) = match (Identifier::new(from), Identifier::new(to)) {
                (Ok(from), Ok(to)) => (from, to),
                (Err(error), _) | (_, Err(error)) => {
                    self.message = format!("'{text}' renames no tag: {error}");
                    return;
                }
            };
            let paths: Vec<PathBuf> = if self.basket.is_empty() {
                self.view
                    .iter()
                    .filter_map(|entry| entry.path.clone())
                    .collect()
            } else {
                self.basket.iter().cloned().collect()
            };
            let selection = match list::from_files(&paths) {
                Ok(selection) => selection,
                Err(error) => {
                    self.message = error.to_string();
                    return;
                }
            };
            let plan = tagging::plan(
                &selection,
                tagging::Edit::Rename {
                    from: from.clone(),
                    to: to.clone(),
                },
            );
            if plan.changes.is_empty() {
                self.message = format!("nothing to change — no sample here has the tag '{from}'");
                return;
            }
            let mut lines = vec![format!(
                "'{from}' renamed '{to}' in {}",
                counted(plan.changes.len(), "sample", "samples")
            )];
            for change in &plan.changes {
                lines.push(format!("  {}", self.here(&change.path)));
            }
            self.mode = Mode::Confirm {
                title: "rename a tag".to_string(),
                lines,
                pending: Box::new(Pending::Tag(plan)),
            };
            return;
        }
        if self.basket.is_empty() {
            self.message = "the basket is empty: Space puts a sample in it — old=new renames a \
                            tag over every sample shown"
                .to_string();
            return;
        }
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
            // Files a computation is writing are written by nothing else
            // meanwhile: the question stays, and is answered once it has
            // ended — dropped here, a note typed whole was lost.
            Key::Char('y') if self.running.is_some() && !matches!(pending, Pending::Quit) => {
                self.message = "a computation is writing: y again once it has finished".to_string();
                self.mode = Mode::Confirm {
                    title,
                    lines,
                    pending: Box::new(pending),
                };
            }
            Key::Char('y') => {
                self.confirm_scroll = 0;
                // A computation's outcome, over, leaves the title at the next
                // write: it stayed there, said of values since changed.
                if self.run.as_ref().is_some_and(|run| run.outcome.is_some()) {
                    self.run = None;
                }
                // What a write changes, as its preview's lines `radius 1.048 →
                // 1.050` say it,
                // for the history: kept as the status line's `written
                // pieces/gamma-zinc.md`, the value changed went unnamed.
                let changing = matches!(pending, Pending::Write { .. }).then(|| {
                    lines
                        .iter()
                        .filter(|line| line.contains("  →  ") && !line.starts_with("    "))
                        .take(3)
                        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect::<Vec<_>>()
                        .join("; ")
                });
                // What the files were, for `u`: kept once the change is written.
                let touched: Vec<PathBuf> = match &pending {
                    Pending::Write { path, .. } | Pending::New { path, .. } => {
                        vec![path.clone()]
                    }
                    Pending::Export { files, .. } => {
                        files.iter().map(|file| file.path.clone()).collect()
                    }
                    Pending::Tag(plan) => plan
                        .changes
                        .iter()
                        .map(|change| change.path.clone())
                        .collect(),
                    Pending::Remove(paths) => paths.clone(),
                    Pending::Rename(plan) => {
                        let mut touched = vec![plan.from.clone(), plan.to.clone()];
                        for change in &plan.links {
                            if !touched.contains(&change.file) {
                                touched.push(change.file.clone());
                            }
                        }
                        touched
                    }
                    Pending::Run { held, .. } => {
                        held.iter().map(|held| held.path.clone()).collect()
                    }
                    Pending::Declare { edit, .. } => vec![edit.path().to_path_buf()],
                    // Not kept for `u`: an undo is not undone.
                    Pending::Undo(_)
                    | Pending::Quit
                    | Pending::Compute(_)
                    | Pending::Figure { .. } => Vec::new(),
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
                let mut renamed: Option<PathBuf> = None;
                self.message = match pending {
                    Pending::Write {
                        path,
                        mut sample,
                        origin,
                    } => {
                        // The columns a table written by hand gains carry
                        // the model's units.
                        list::declare_model_units_at(&mut sample, &path);
                        let saved =
                            document::save_sample(&mut sample, &Destination::Origin(origin));
                        match saved {
                            // As the command line says it, from the folder
                            // the workbench is on: a whole path wrapped over
                            // three lines of the message.
                            Ok(_) => {
                                wrote = true;
                                format!("written: {}", self.here(&path))
                            }
                            Err(error) => error.to_string(),
                        }
                    }
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
                    Pending::Figure { output, path } => {
                        self.draw_to(output, path);
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
                            document::note_written(path);
                            match std::fs::remove_file(path) {
                                Ok(()) => removed += 1,
                                Err(error) => {
                                    failed = Some(format!("{}: {error}", self.here(path)));
                                    break;
                                }
                            }
                        }
                        wrote = removed > 0;
                        let held = !self.basket.is_empty();
                        for path in &paths {
                            self.basket.remove(path);
                        }
                        // The basket shown alone and now empty: every sample
                        // is shown again, and said, where the list was left
                        // empty without a word.
                        let emptied = held && self.basket.is_empty() && self.basket_only;
                        if emptied {
                            self.basket_only = false;
                        }
                        // Removed from its own screen, the collection is where
                        // it goes, said as the collection says a removal: the
                        // reload said it was "no longer among those shown".
                        let open = self
                            .screen
                            .sample_at()
                            .and_then(|at| self.view.get(at))
                            .and_then(|entry| entry.path.clone());
                        if open.is_some_and(|open| paths.contains(&open)) {
                            self.screen = Screen::Collection;
                        }
                        let mut said = format!(
                            "removed {} · u gives {} back",
                            counted(removed, "sample", "samples"),
                            if removed == 1 { "it" } else { "them" }
                        );
                        if emptied {
                            said.push_str(" · the basket is empty: every sample is shown");
                        }
                        match failed {
                            Some(failed) => format!("{said} · stopped at {failed}"),
                            None => said,
                        }
                    }
                    Pending::New { path, mut sample } => {
                        list::declare_model_units_at(&mut sample, &path);
                        // A file made there since the preview is not written
                        // over, as `new` refuses it.
                        match document::save_sample(&mut sample, &Destination::New(path.clone())) {
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
                        if let Some(moved) = group
                            .iter()
                            .find(|undone| std::fs::read(&undone.path).ok() != undone.after)
                        {
                            let restoring = title.starts_with("restore");
                            let said = format!(
                                "{} changed meanwhile: {}",
                                self.here(&moved.path),
                                if restoring {
                                    "not restored"
                                } else {
                                    "not undone"
                                }
                            );
                            if !restoring {
                                self.undo.push(group);
                            }
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
                                undone.after.as_deref(),
                                undone.before.as_deref(),
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
                                "{}: {} as {} {}",
                                if title.starts_with("restore") {
                                    "restored"
                                } else {
                                    "undone"
                                },
                                counted(group.len(), "file", "files"),
                                if group.len() == 1 { "it" } else { "they" },
                                if group.len() == 1 { "was" } else { "were" }
                            )
                        })
                    }
                    Pending::Export { files, .. } => {
                        let mut written: Vec<(PathBuf, usize)> = Vec::new();
                        let mut failed = None;
                        let mut warning = None;
                        for file in &files {
                            let made = file
                                .path
                                .parent()
                                .filter(|parent| !parent.as_os_str().is_empty())
                                .map_or(Ok(()), std::fs::create_dir_all)
                                .map_err(|error| error.to_string())
                                .and_then(|()| {
                                    crate::collection::exports::write_replacing(
                                        &file.text,
                                        &file.path,
                                        file.before.as_deref(),
                                    )
                                    .map_err(|error| error.to_string())
                                });
                            if let Err(error) = made {
                                failed = Some(format!("{}: {error}", self.here(&file.path)));
                                break;
                            }
                            written.push((file.path.clone(), file.rows));
                            warning =
                                warning.or(keep_output(&file.path, &file.said, &file.samples));
                        }
                        wrote = !written.is_empty();
                        let rows: usize = written.iter().map(|(_, rows)| rows).sum();
                        let said = match written.as_slice() {
                            [] => String::new(),
                            [(path, _)] => format!(
                                "{} written to {}",
                                counted(rows, "row", "rows"),
                                self.here(path)
                            ),
                            several => format!(
                                "{} written to {}",
                                counted(rows, "row", "rows"),
                                counted(several.len(), "file", "files")
                            ),
                        };
                        match (failed, warning) {
                            (Some(error), _) if said.is_empty() => error,
                            (Some(error), _) => format!("{said}; then {error}"),
                            (None, Some(warning)) => format!("{said} — {warning}"),
                            (None, None) => said,
                        }
                    }
                    Pending::Run {
                        name,
                        held,
                        answers,
                    } => {
                        let mut done = 0usize;
                        let mut failed = None;
                        for file in &held {
                            match document::replace_if_unchanged(
                                &file.path,
                                file.before.as_deref(),
                                Some(file.after.as_slice()),
                            ) {
                                Ok(()) => done += 1,
                                Err(error) => {
                                    failed = Some(error.to_string());
                                    break;
                                }
                            }
                        }
                        wrote = done > 0;
                        // The answers it was given, kept with the change.
                        let answered = if answers.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " — answered {}",
                                answers
                                    .iter()
                                    .map(|asked| format!(
                                        "'{}' {}",
                                        asked.prompt.trim(),
                                        asked.answer
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };
                        let written = format!(
                            "'{name}' written: {}{answered}",
                            counted(done, "file", "files")
                        );
                        match failed {
                            Some(error) if done == 0 => format!("{error} — nothing was written"),
                            Some(error) => format!("{error} — {written} before it"),
                            None => written,
                        }
                    }
                    Pending::Rename(plan) => match crate::collection::renaming::apply(&plan) {
                        Ok(_) => {
                            wrote = true;
                            renamed = Some(plan.to.clone());
                            format!(
                                "renamed {} → {} · u gives the samples back; R again moves \
                                 its own files back",
                                self.here(&plan.from),
                                self.here(&plan.to)
                            )
                        }
                        Err(error) => error.to_string(),
                    },
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
                    // A row typed at a table's foot, written: the empty row is
                    // empty again.
                    if self
                        .draft
                        .as_ref()
                        .is_some_and(|draft| touched.contains(&draft.path))
                    {
                        self.draft = None;
                    }
                    let changed = settled(before);
                    if !changed.is_empty() {
                        self.undo.push(changed);
                    }
                    // Its paths from the workbench's folder, as the history
                    // names them.
                    // An undo says so as the history reads it: never offered
                    // back as a change of its own.
                    // The history names a change after its title's colon, as
                    // it always has, the values changed before the file: `change: diameter 12.0 →
                    // 12.5, written C1.md`.
                    // Titled what and where; the history names what alone.
                    let title = title.split(" · ").next().unwrap_or(&title).to_string();
                    let said = if title.starts_with("undo") {
                        format!("tui · {}{}", version_control::UNDO, self.message)
                    } else {
                        let message = self.message.replacen("written: ", "written ", 1).replacen(
                            "created: ",
                            "created ",
                            1,
                        );
                        match changing.as_deref().filter(|changing| !changing.is_empty()) {
                            Some(changing) => {
                                format!("tui · {title}: {changing}, {}", for_history(&message))
                            }
                            None => format!("tui · {title}: {}", for_history(&message)),
                        }
                    }
                    .replace(&format!("{}/", self.root.display()), "");
                    let failures = version_control::after_writing(writing, &said);
                    if let Some(warning) = history_warning(unread.into_iter().chain(failures)) {
                        self.message = format!("{} — {warning}", self.message);
                    }
                }
                // The screen on a sample renamed follows it to its new file.
                if let Some(to) = renamed {
                    let on_sample = self.screen.sample_at().is_some();
                    let said = self.message.clone();
                    self.reload();
                    self.message = said;
                    if let Some(at) = self
                        .view
                        .iter()
                        .position(|entry| entry.path.as_deref() == Some(to.as_path()))
                    {
                        if on_sample {
                            self.screen = Screen::Sample { at, cursor: 0 };
                        } else {
                            self.cursor = at;
                        }
                    }
                } else {
                    self.reload();
                }
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
                // Staying lets a Ctrl+C go: the next is a first again; what
                // was being typed comes back as it was left.
                if matches!(pending, Pending::Quit) {
                    self.interrupted = false;
                    if let Some(typing) = self.typing_kept.take() {
                        self.mode = *typing;
                        self.message.clear();
                        return Effect::None;
                    }
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

    /// An attribute's name, refused where it is no name or the sample holds
    /// it already; accepted, its value is asked, as `e` asks one.
    fn name_attribute(&mut self, path: PathBuf, name: &str) -> bool {
        let named = match Identifier::new(name) {
            Ok(named) => named,
            Err(error) => {
                self.message = error.to_string();
                return false;
            }
        };
        let held = document::load_sample(&path)
            .map_err(|error| error.to_string())
            .map(|(sample, _)| {
                matches!(name, "name" | "tags")
                    || sample.has_attribute(&named)
                    || sample.has_property(&named)
                    || sample.table(&named).is_ok()
            });
        match held {
            Err(error) => {
                self.message = error;
                false
            }
            Ok(true) => {
                self.message = format!("{name} is held already: e changes it");
                false
            }
            Ok(false) => {
                self.mode = Mode::prompt(
                    Purpose::Edit {
                        path,
                        field: name.to_string(),
                    },
                    String::new(),
                );
                true
            }
        }
    }

    /// A new sample beside the one under the cursor, taking nothing of it,
    /// previewed before it is written.
    fn preview_new(&mut self, beside: Option<PathBuf>, text: &str) {
        // `NAME field=value … keep:field,field`, as `new NAME field=value
        // --keep SAMPLE fields` writes it: what is kept is the sample's under
        // the cursor.
        let mut words = text.split_whitespace();
        let name = words.next().unwrap_or_default();
        if name.is_empty() {
            self.message = "a new sample needs a name".to_string();
            return;
        }
        let mut keep: Vec<String> = Vec::new();
        let mut assignments: Vec<(String, String)> = Vec::new();
        for word in words {
            if let Some(fields) = word.strip_prefix("keep:") {
                keep.extend(
                    fields
                        .split(',')
                        .filter(|field| !field.is_empty())
                        .map(str::to_string),
                );
            } else if let Some((field, value)) = word.split_once('=') {
                assignments.push((field.to_string(), value.to_string()));
            } else {
                self.message = format!(
                    "'{word}' is neither field=value nor keep:fields — a new sample is NAME \
                     field=value … keep:field,field"
                );
                return;
            }
        }
        // Checked as `new` checks it, in the project of the sample under the
        // cursor, beside which it is written — or, the first, in this one's
        // samples/ where there is one.
        let project = match &beside {
            Some(beside) => self.collection.config_for(beside),
            None => self.collection.config(),
        }
        .map(|config| config.root().to_path_buf());
        if let Some(refused) = editing::refuses_name(name, &self.collection, project.as_deref()) {
            self.message = refused.message;
            return;
        }
        let pattern = if keep.is_empty() {
            None
        } else {
            match &beside {
                Some(beside) => match document::load_sample(beside) {
                    Ok((sample, _)) => Some(sample),
                    Err(error) => {
                        self.message = error.to_string();
                        return;
                    }
                },
                None => {
                    self.message =
                        "keep: takes from the sample under the cursor, and none is".to_string();
                    return;
                }
            }
        };
        let shaped = match editing::shaped_like(name, pattern.as_ref(), &keep) {
            Ok(shaped) => shaped,
            Err(error) => {
                self.message = error.message;
                return;
            }
        };
        let mut sample = shaped.sample;
        let directory = match &beside {
            Some(beside) => beside.parent().unwrap_or(Path::new(".")).to_path_buf(),
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
        let config = beside
            .as_ref()
            .and_then(|beside| self.collection.config_for(beside))
            .or_else(|| self.collection.config());
        let mut lines = Vec::new();
        if let Some(beside) = &beside
            && !keep.is_empty()
        {
            lines.push(format!(
                "kept from {}: {}",
                self.here(beside),
                keep.join(", ")
            ));
        }
        for (field, text) in &assignments {
            let value = match editing::typed(field, text) {
                Ok(value) => value,
                Err(error) => {
                    self.message = error.message;
                    return;
                }
            };
            let known = editing::known_as(&sample, field, config, Some(&self.collection));
            if let Err(error) = editing::apply_known(&mut sample, field, value, known.as_ref()) {
                self.message = error.message;
                return;
            }
            lines.push(format!("{field}   {text}"));
        }
        if lines.is_empty() {
            lines.push(format!(
                "{}: named by its file, empty; A adds an attribute, e a value, or the model",
                self.here(&path)
            ));
        }
        self.mode = Mode::Confirm {
            title: format!("new sample · {}", self.here(&path)),
            lines,
            pending: Box::new(Pending::New {
                path,
                sample: Box::new(sample),
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
                // follows a decimal comma, `hardness=1,011`, which read as
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
            let before = editing::not_current(&sample);
            // A table no sample holds yet takes its columns from the model,
            // as `set --add-row` does: the model's first, which says them,
            // then another sample's.
            let config = self.config_for(&path);
            let added = editing::add_row(&mut sample, table, cells, |name| {
                config
                    .and_then(|config| {
                        // Described again where its description is stale:
                        // a project's first sample had no table to copy.
                        if crate::config::model_runtime::current_description(config).is_none() {
                            let _ = crate::config::model_runtime::describe(config, &path);
                        }
                        editing::shape_from_model(config, name)
                    })
                    .or_else(|| editing::shape_from(&self.collection, &path, name))
            })
            .map_err(|error| error.message)?;
            // Each cell given as a change from nothing, as `set --add-row`
            // says it.
            let changes: Vec<crate::presentation::changes::FieldChange> = added
                .given
                .iter()
                .map(|(column, value, over)| {
                    let mut change = crate::presentation::changes::FieldChange::new(
                        format!("{table}.{column}"),
                        "—",
                        editing::shown_value(value),
                    );
                    if *over {
                        change
                            .notes
                            .push("typed over the model's formula".to_string());
                    }
                    change
                })
                .collect();
            let mut lines = crate::presentation::changes::change_lines(&changes);
            let named = |names: &[Identifier]| {
                names
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if !added.left_absent.is_empty() || !added.by_the_model.is_empty() {
                lines.push(String::new());
            }
            if !added.left_absent.is_empty() {
                lines.push(format!(
                    "  left empty              {}",
                    named(&added.left_absent)
                ));
            }
            if !added.by_the_model.is_empty() {
                lines.push(format!(
                    "  computed by the model   {}",
                    named(&added.by_the_model)
                ));
            }
            // What the row makes not current, as every preview says it.
            lines.push(String::new());
            lines.extend(said_not_current(&sample, &before));
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
                    title: format!("row · {}", self.here(&path)),
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
        let mut group = match self.undo.pop() {
            Some(group) => group,
            None => match self.undoable_from_history() {
                Ok(Some((group, said))) => {
                    from_history = Some(said);
                    group
                }
                Ok(None) => {
                    self.message = "nothing to undo".to_string();
                    return;
                }
                Err(error) => {
                    self.message = error;
                    return;
                }
            },
        };
        // A file changed since — a colleague's work pulled, an edit by hand —
        // has the change taken back value by value, as `restore` does, what
        // was changed since kept: refused whole, `u` could take nothing back
        // on a sample a pull had touched.
        let mut conflicts: Vec<String> = Vec::new();
        for undone in group.iter_mut() {
            let now = std::fs::read(&undone.path).ok();
            if now == undone.after {
                continue;
            }
            let (Some(before), Some(after), Some(now)) =
                (undone.before.clone(), undone.after.clone(), now)
            else {
                continue;
            };
            let name = self.here(&undone.path);
            match crate::config::version_control::taken_back(
                &name,
                Some(before.as_slice()),
                Some(after.as_slice()),
                Some(now.as_slice()),
            ) {
                Ok(Some(merged)) => {
                    undone.before = Some(merged);
                    undone.after = Some(now);
                }
                Ok(None) => {}
                Err(found) => conflicts.extend(found),
            }
        }
        if !conflicts.is_empty() {
            // Asked again, it is let go, and the change before it offered.
            if self.undo_refused {
                self.undo_refused = false;
                self.message = "let go: u now offers the change before it".to_string();
                return;
            }
            self.undo_refused = true;
            self.message = format!(
                "not undone: what it changed was changed since — {} — set those values again by \
                 hand; u again leaves it and offers the change before it",
                conflicts.join(", ")
            );
            self.undo.push(group);
            return;
        }
        // A file changed since — by hand, in an editor — is not overwritten
        // with an older text: the change is not undone, and stays offered.
        if let Some(moved) = group
            .iter()
            .find(|undone| std::fs::read(&undone.path).ok() != undone.after)
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
                // A configuration, line by line: what goes and what comes
                // back — `restore .samplekitrc` said nothing of either.
                (Some(before), Some(after))
                    if undone
                        .path
                        .extension()
                        .is_none_or(|extension| extension != "md") =>
                {
                    lines.push(format!("restore {name}"));
                    lines.extend(lines_undone(
                        &String::from_utf8_lossy(after),
                        &String::from_utf8_lossy(before),
                    ));
                }
                (Some(before), Some(after)) => {
                    lines.push(format!("restore {name}"));
                    if let Some(config) = self
                        .config_for(&undone.path)
                        .or_else(|| self.collection.config())
                    {
                        lines.extend(
                            crate::presentation::changes::changes_of(
                                &name,
                                Some(after),
                                Some(before),
                                config,
                            )
                            .into_iter()
                            .skip(1),
                        );
                    }
                    // What taking it back makes not current, as `restore` and
                    // every other preview say it: undoing a computation left
                    // its values outdated again, unsaid.
                    if let (Some(now), Some(then)) = (
                        crate::presentation::changes::sample_of(after),
                        crate::presentation::changes::sample_of(before),
                    ) {
                        lines.push(String::new());
                        lines.extend(said_not_current(&then, &editing::not_current(&now)));
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
                    // Waiting for an input emptied since is owed, as `status`
                    // says it: marked outdated, `C` could do nothing for it.
                    state @ (Freshness::Stale { .. } | Freshness::Broken { .. })
                        if !editing::emptied_inputs(&sample, state).is_empty() =>
                    {
                        Concern::Owed
                    }
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
                    said: format!(
                        "{}  {}{rows}",
                        value.name,
                        said_whole(&sample, &value.state)
                    ),
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
                    // A formula changed since it computed the value is an
                    // outdated value, as `status` counts it: it was counted
                    // among those never computed.
                    kind: if owed.said.contains("formula changed") {
                        Concern::Stale
                    } else {
                        Concern::Owed
                    },
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
                // Paths from the folder opened, as the screen writes them:
                // `./pieces/…` where it is `.`.
                said: crate::collection::validation::described(finding)
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join(" — ")
                    .replace(" ./", " "),
                kind: match finding.severity {
                    crate::collection::validation::Severity::Defect => Concern::Defect,
                    _ => Concern::Note,
                },
            });
        }
        // What only reads values that wait waits too, as `status` says it
        //: it was outdated, among what is to fix.
        loop {
            let waiting: std::collections::HashSet<(String, String)> = items
                .iter()
                .filter(|item| item.kind == Concern::Owed && item.said.contains("waits for"))
                .filter_map(|item| Some((item.sample.clone(), item.field.clone()?)))
                .collect();
            let mut changed = false;
            for item in items.iter_mut().filter(|item| item.kind == Concern::Stale) {
                let Some(field) = item.field.clone() else {
                    continue;
                };
                let Some(state) = item.said.strip_prefix(&format!("{field}  ")) else {
                    continue;
                };
                let names: std::collections::HashSet<String> = waiting
                    .iter()
                    .filter(|(sample, _)| *sample == item.sample)
                    .map(|(_, name)| name.clone())
                    .collect();
                if let Some(said) = crate::presentation::changes::behind_waiting(state, &names) {
                    item.said = format!("{field}  {said}");
                    item.kind = Concern::Owed;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // The values in `status`'s order, sample by sample and by name; then
        // the defects and notes, worst first.
        items.sort_by_key(|item| {
            let found = matches!(item.kind, Concern::Defect | Concern::Note);
            (
                found,
                if found { Some(item.kind) } else { None },
                item.sample.clone(),
                item.field.clone(),
            )
        });
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
            self.refresh_said();
            self.message = "the filter was set aside to show it".to_string();
            found = self
                .view
                .iter()
                .position(|entry| entry.path.as_deref() == Some(&path));
        }
        let Some(position) = found else {
            // A file that could not be read is in no collection: said so, and
            // where it is mended, not as a sample filtered away.
            let unread = self
                .collection
                .skipped()
                .iter()
                .any(|skipped| skipped.path == path);
            self.message = if unread {
                format!(
                    "{} could not be read, so no screen shows it: mend it in an editor, as this line says",
                    item.sample
                )
            } else {
                format!("{} is not in the collection shown", item.sample)
            };
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
                if plan.missing().next().is_none() {
                    self.message = "this project is set up: there is nothing to do".to_string();
                    return Effect::None;
                }
                // The page is the preview: what will be done was just read,
                // and asked a second time it was the same list again.
                return self.confirming(
                    Key::Char('y'),
                    "set the project up".to_string(),
                    Vec::new(),
                    Pending::Setup,
                );
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
    /// as the command line heads a table — `more · alloy = chrome` — and
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
                let values = crate::presentation::summaries::group_heading_in(
                    &self.group,
                    &key.unwrap_or_default(),
                    entry
                        .path
                        .as_deref()
                        .and_then(|path| self.collection.config_for(path)),
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
            // `tempering.co2.symbol`, where its keys never do.
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
            .filter(|entry| entry.state.starts_with("edited"))
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
            EntryKind::Attribute | EntryKind::Backlink => false,
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

    /// What `C` would compute, said as `compute` says it before it asks: each
    /// value not current, the model's owed ones, counted by cells, and what is
    /// kept; `y` computes and writes.
    fn preview_compute(&mut self, computing: Computing, basket: Option<String>) {
        if self.running.is_some() {
            self.message = "a computation is already running".to_string();
            return;
        }
        let mut rows: Vec<(String, String, String)> = Vec::new();
        let mut cells = 0usize;
        let mut samples = std::collections::BTreeSet::new();
        let mut waits = 0usize;
        for path in &computing.paths {
            let Some(entry) = self
                .collection
                .iter()
                .find(|entry| entry.path.as_deref() == Some(path.as_path()))
            else {
                continue;
            };
            let sample = entry.sample.borrow();
            let name = sample.name().unwrap_or("(unnamed)").to_string();
            for value in editing::not_current(&sample) {
                // A value typed over its formula is kept, as `compute` keeps it.
                if matches!(value.state, Freshness::Edited | Freshness::RecordMissing) {
                    continue;
                }
                let said = said_whole(&sample, &value.state);
                if said.starts_with("waits for") {
                    waits += value.rows.max(1);
                } else {
                    cells += value.rows.max(1);
                    samples.insert(name.clone());
                }
                let rows_said = if value.rows > 1 {
                    format!(" ({})", counted(value.rows, "row", "rows"))
                } else {
                    String::new()
                };
                rows.push((
                    name.clone(),
                    value.name.clone(),
                    format!("{said}{rows_said}"),
                ));
            }
            for owed in self.owed_by(path) {
                if rows
                    .iter()
                    .any(|(at, value, _)| *at == name && *value == owed.value)
                {
                    continue;
                }
                if owed.waits() {
                    waits += 1;
                } else {
                    cells += 1;
                    samples.insert(name.clone());
                }
                rows.push((name.clone(), owed.value.clone(), owed.shown()));
            }
        }
        let kept = match list::from_files(&computing.paths) {
            Ok(selection) => computation::edited_kept(&selection, &computing.names),
            Err(_) => 0,
        };
        // Where the model's plan was not read yet, what it never computed is
        // known once it runs: said, and asked all the same.
        let unplanned = computing
            .paths
            .iter()
            .filter(|path| !self.owed.contains_key(*path))
            .count();
        if cells == 0 && unplanned == 0 {
            let within = counted(computing.paths.len(), "sample", "samples");
            self.message = if waits > 0 {
                format!(
                    "nothing would be computed: {} for an input nobody entered",
                    counted(waits, "waits", "wait")
                )
            } else {
                format!("nothing to change — nothing to compute in {within}")
            };
            if kept > 0 {
                self.message.push_str(&format!(
                    " · {} {} kept — c on {} gives it back to its formula",
                    counted(kept, "value typed", "values typed"),
                    if kept == 1 {
                        "over its formula"
                    } else {
                        "over their formulas"
                    },
                    if kept == 1 { "it" } else { "one" }
                ));
            }
            return;
        }
        let sample_width = rows
            .iter()
            .map(|(at, ..)| at.chars().count())
            .max()
            .unwrap_or(0);
        let title = if cells == 0 {
            "compute?".to_string()
        } else {
            format!("compute these {}?", counted(cells, "value", "values"))
        };
        let value_width = rows
            .iter()
            .map(|(_, value, _)| value.chars().count())
            .max()
            .unwrap_or(0);
        let mut lines: Vec<String> = rows
            .iter()
            .map(|(at, value, said)| format!("{at:<sample_width$}  {value:<value_width$}  {said}"))
            .collect();
        if !lines.is_empty() {
            lines.push(String::new());
        }
        if unplanned > 0 {
            lines.push(format!(
                "what the model never computed in {} is known once it runs",
                counted(unplanned, "sample", "samples")
            ));
        }
        if cells > 0 {
            lines.push(format!(
                "{} in {} would be computed{}",
                counted(cells, "value", "values"),
                counted(samples.len(), "sample", "samples"),
                if waits > 0 {
                    format!(
                        ", {} for an input nobody entered",
                        counted(waits, "waits", "wait")
                    )
                } else {
                    String::new()
                }
            ));
        }
        if kept > 0 {
            lines.push(format!(
                "{} {} kept — c on {} gives it back to its formula",
                counted(kept, "value typed", "values typed"),
                if kept == 1 {
                    "over its formula"
                } else {
                    "over their formulas"
                },
                if kept == 1 { "it" } else { "one" }
            ));
        }
        if let Some(basket) = basket {
            lines.push(basket);
        }
        self.mode = Mode::Confirm {
            title,
            lines,
            pending: Box::new(Pending::Compute(computing)),
        };
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
                            timeout: None,
                        },
                    );
                    jobs.push(Job {
                        python,
                        config: group.config.clone(),
                        requests: planned.requests,
                        before: planned.before,
                    });
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
        // A value typed over its formula, kept by a computation not forced:
        // said after it, as the command line says it.
        let kept = if computing.force {
            0
        } else {
            computation::edited_kept(&selection, &computing.names)
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || run_jobs(jobs, kept, &sender));
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
        // Edited with its last line ended: the editor's line keys — Enter at
        // the end, Ctrl+D, Alt+2 — mistook a last line without its end, and
        // wrote `A\nBC` for `ABC` typed after Enter. How the file ended is put
        // back when it is written.
        let ended = if original.ends_with('\n') {
            original.clone()
        } else {
            format!("{original}\n")
        };
        let text = Written::new(&ended);
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
            // The file's note changed since it was opened: the note typed stays
            // open, compared from now on with the file's, so that the preview
            // shows what writing it does to what another hand wrote.
            if let Some(changed) = self.preview_note(path.clone(), &text.text(), &original) {
                self.mode = Mode::Note {
                    path,
                    text,
                    original: changed,
                };
            }
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
    /// The note's preview; the file's note where it changed since the note
    /// was opened, and no preview is made.
    fn preview_note(&mut self, path: PathBuf, written: &str, original: &str) -> Option<String> {
        let mut text = written.to_string();
        if (original.ends_with('\n') || original.is_empty()) && !text.ends_with('\n') {
            text.push('\n');
        }
        let (mut sample, origin) = match document::load_sample(&path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.message = error.to_string();
                return None;
            }
        };
        if sample.note().trim_end() != original.trim_end() {
            self.message = "the note was changed in the file since you opened it: Esc again \
                            shows what yours does to it"
                .to_string();
            return Some(sample.note().to_string());
        }
        if text.trim_end() == original.trim_end() {
            self.message = "the note is unchanged".to_string();
            return None;
        }
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
        None
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
    /// What renaming the sample at `path` to `new` does, asked first.
    fn preview_rename(&mut self, path: &Path, new: &str) {
        let config = self.config_for(path).cloned();
        let plan = match crate::collection::renaming::plan(config.as_ref(), path, new) {
            Ok(plan) => plan,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let mut lines = vec![format!(
            "{}  →  {}",
            self.here(&plan.from),
            self.here(&plan.to)
        )];
        if let Some((before, after)) = &plan.name {
            lines.push(format!("name  {before}  →  {after}"));
        }
        for (from, to) in &plan.own {
            lines.push(format!(
                "own file  {}  →  {}",
                self.here(from),
                self.here(to)
            ));
        }
        lines.push(String::new());
        if plan.links.is_empty() {
            lines.push("no link of the project leads to it".to_string());
        } else {
            lines.push(format!(
                "{} leading to it, written again:",
                counted(plan.links.len(), "link", "links")
            ));
            for change in &plan.links {
                lines.push(format!(
                    "  {}  {}  {}  →  {}",
                    self.here(&change.file),
                    change.field,
                    change.before,
                    change.after
                ));
            }
        }
        self.mode = Mode::Confirm {
            title: "rename".to_string(),
            lines,
            pending: Box::new(Pending::Rename(Box::new(plan))),
        };
    }

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
        // The links leading to them, left as written, lead nowhere after.
        let incoming = crate::collection::renaming::links_leading_to(
            paths.first().and_then(|path| self.config_for(path)),
            &paths,
        );
        if !incoming.is_empty() {
            lines.push(format!(
                "{} to {}, and will lead nowhere:",
                counted(incoming.len(), "link leads", "links lead"),
                if paths.len() == 1 { "it" } else { "them" }
            ));
            let mut file: Option<&Path> = None;
            for link in &incoming {
                if file != Some(link.file.as_path()) {
                    lines.push(format!(
                        "  {}",
                        link.file
                            .strip_prefix(&self.root)
                            .unwrap_or(&link.file)
                            .display()
                    ));
                    file = Some(link.file.as_path());
                }
                lines.push(format!("    {}  {}", link.field, link.text));
            }
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
                    property: None,
                    uncertainty: None,
                    unitless: None,
                    unit_alone: None,
                    unit_in_form: None,
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
    // A file not yet written by its folder, which is: `./pieces/new.md` was
    // said so where every other line says `pieces/…`.
    let full = dunce::canonicalize(path)
        .or_else(|error| match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => dunce::canonicalize(if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            })
            .map(|parent| parent.join(name)),
            _ => Err(error),
        })
        .unwrap_or_else(|_| path.to_path_buf());
    full.strip_prefix(&root)
        .or_else(|_| path.strip_prefix(&root))
        .or_else(|_| path.strip_prefix("."))
        .map(|inner| inner.display().to_string())
        .unwrap_or_else(|_| relative_to(&root, &full).display().to_string())
}

/// `path` from `base`, up through `..` where it is not inside: a file beside
/// the project was said by its whole path, which a window cut before its name.
fn relative_to(base: &Path, path: &Path) -> PathBuf {
    let path = normalised(path);
    let base = normalised(base);
    let shared = base
        .components()
        .zip(path.components())
        .take_while(|(one, other)| one == other)
        .count();
    // Nothing shared but the root: the whole path reads better.
    if shared <= 1 {
        return path;
    }
    let mut relative = PathBuf::new();
    for _ in base.components().skip(shared) {
        relative.push("..");
    }
    relative.extend(path.components().skip(shared));
    relative
}

/// A text's lines as an undo changes them: `- ` what it takes away, `+ `
/// what it brings back, each in the order its version holds it.
fn lines_undone(now: &str, back: &str) -> Vec<String> {
    let mut left: Vec<&str> = back.lines().collect();
    let mut going = Vec::new();
    // A blank line said alone, `  - `, says nothing.
    for line in now.lines().filter(|line| !line.trim().is_empty()) {
        match left.iter().position(|kept| *kept == line) {
            Some(at) => {
                left.remove(at);
            }
            None => going.push(format!("  - {line}")),
        }
    }
    let mut right: Vec<&str> = now.lines().collect();
    let mut coming = Vec::new();
    for line in back.lines().filter(|line| !line.trim().is_empty()) {
        match right.iter().position(|kept| *kept == line) {
            Some(at) => {
                right.remove(at);
            }
            None => coming.push(format!("  + {line}")),
        }
    }
    going.extend(coming);
    going
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

/// The times and sizes of a collection's files and of their folders, as one
/// number: a file edited, added or removed changes it. Its configurations,
/// what they import and the models they declare — each model's folder, where
/// the modules it imports sit — count as its files.
fn files_stamp(collection: &SampleList, root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut folders: BTreeSet<PathBuf> = BTreeSet::from([root.to_path_buf()]);
    let mut declared: Vec<PathBuf> = Vec::new();
    for config in collection.configurations() {
        declared.push(config.root().join(".samplekitrc"));
        declared.extend(config.imports().iter().cloned());
        if let Some(model) = config.model() {
            let model = config.root().join(&model.path);
            let modules = model
                .parent()
                .and_then(|folder| std::fs::read_dir(folder).ok())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "py"));
            declared.extend(modules);
            declared.push(model);
        }
    }
    declared.sort();
    declared.dedup();
    let paths = collection
        .iter()
        .filter_map(|entry| entry.path.clone())
        .chain(
            collection
                .skipped()
                .iter()
                .map(|skipped| skipped.path.clone()),
        );
    for path in &declared {
        path.hash(&mut hasher);
        if let Ok(metadata) = std::fs::metadata(path) {
            metadata.len().hash(&mut hasher);
            metadata.modified().ok().hash(&mut hasher);
        }
    }
    for path in paths {
        if let Some(folder) = path.parent() {
            folders.insert(folder.to_path_buf());
        }
        path.hash(&mut hasher);
        if let Ok(metadata) = std::fs::metadata(&path) {
            metadata.len().hash(&mut hasher);
            metadata.modified().ok().hash(&mut hasher);
        }
    }
    // A folder by the names it holds, not by its time: SampleKit's own hidden
    // folder, made at a project's first opening, changed the time and was
    // said a file changed outside SampleKit.
    for folder in folders {
        let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .filter(|name| {
                        !name.to_string_lossy().starts_with('.') || name == ".samplekitrc"
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        folder.hash(&mut hasher);
        names.hash(&mut hasher);
    }
    hasher.finish()
}

/// An editing refusal in the words of the window it is typed in: `set`'s
/// `volume=1.05`, or `ageing.hardness[…]=1.05`, is the command line's.
fn in_the_windows_words(message: &str) -> String {
    // A value and its uncertainty typed together: each has its cell here.
    const APART: &str = "its uncertainty is a field of its own: ";
    if let Some(at) = message.find(APART) {
        let rest = &message[at + APART.len()..];
        let mut parts = rest.split_whitespace();
        if let (Some(value), Some(uncertainty)) = (parts.next(), parts.next())
            && let (Some((_, value)), Some((_, uncertainty))) =
                (value.split_once('='), uncertainty.split_once('='))
        {
            return format!(
                "{}its uncertainty has a cell of its own: {value} here, and {uncertainty} in \
                 the uncertainty",
                &message[..at]
            );
        }
    }
    // Several numbers where one is: they are readings, the window's own list.
    const READINGS: &str = "readings are given as such: ";
    if let Some(at) = message.find(READINGS) {
        let rest = &message[at + READINGS.len()..];
        if let Some((_, values)) = rest.split_once('=') {
            return format!(
                "{}they are readings: {values} in the readings, Enter on them",
                &message[..at]
            );
        }
    }
    const ADVICE: &str = "write the number alone: ";
    let Some(at) = message.find(ADVICE) else {
        return message.to_string();
    };
    let rest = &message[at + ADVICE.len()..];
    match rest.find('=') {
        Some(equals) if !rest[..equals].contains(' ') => format!(
            "{}write the number alone, as {}",
            &message[..at],
            &rest[equals + 1..]
        ),
        _ => message.to_string(),
    }
}

/// What a change says in the history: the status line's words up to its
/// advice — *C computes them*, *installing … a minute or so* — which was true
/// on the screen for a moment and was kept for ever.
fn for_history(said: &str) -> &str {
    let cut = [" — ", " · "]
        .iter()
        .filter_map(|mark| said.find(mark))
        .min()
        .unwrap_or(said.len());
    said[..cut].trim_end()
}

/// A value or a table the model declares for a hand to fill, and the file
/// does not hold yet.
pub const TO_FILL: &str = "to fill in";

/// The state of a quantity whose readings have no statistic chosen: nothing
/// computes its value from them.
pub const NO_STATISTIC: &str = "no statistic";

/// A cell's address as a field path writes it, its index quoted where it
/// must be — `'a,b'`, `'12'` as text — so that it reads back as that cell.
/// The state of the cell `address` names in `sample`, as its table's window
/// colours it: failed, never computed, outdated — a formula changed counted —
/// or current. `None` for what is no derived cell.
/// A script's options as its box takes them, `NAME=VALUE` apart by spaces:
/// a value runs to the next `NAME=`, its spaces kept — `label=has space` —
/// and quotes around it go. An option the script has not is refused here,
/// naming those it has, the box kept open on what was typed: the command
/// line's syntax, `--set space=…`, was said, and the box closed.
fn typed_options(
    text: &str,
    script: &runtime::DescribedScript,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut options: std::collections::BTreeMap<String, String> = Default::default();
    let mut last: Option<String> = None;
    for word in text.split_whitespace() {
        match word.split_once('=') {
            Some((name, value))
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') =>
            {
                if options.contains_key(name) {
                    return Err(format!("{name} is given twice"));
                }
                options.insert(name.to_string(), value.to_string());
                last = Some(name.to_string());
            }
            _ => match &last {
                Some(name) => {
                    let value = options.get_mut(name).expect("inserted above");
                    value.push(' ');
                    value.push_str(word);
                }
                None => {
                    return Err(format!(
                        "'{word}': an option is NAME=VALUE — {}",
                        script
                            .options
                            .iter()
                            .map(|option| format!("{}=…", option.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            },
        }
    }
    for value in options.values_mut() {
        let trimmed = value.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|rest| rest.strip_suffix('\''))
            })
            .unwrap_or(trimmed);
        *value = unquoted.to_string();
    }
    if let Some(unknown) = options
        .keys()
        .find(|name| !script.options.iter().any(|option| option.name == **name))
    {
        let known: Vec<String> = script
            .options
            .iter()
            .map(|option| option.name.clone())
            .collect();
        return Err(if known.is_empty() {
            format!("'{}' takes no option, and {unknown} is none", script.name)
        } else {
            format!(
                "'{}' has no option {unknown} — its options: {}",
                script.name,
                known.join(", ")
            )
        });
    }
    Ok(options)
}

pub fn cell_state(
    sample: &Sample,
    address: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
    path: &Path,
) -> Option<String> {
    let fields::Field::Cell {
        table, column, row, ..
    } = fields::parse(address).ok()?
    else {
        return None;
    };
    let held = sample.table(&table).ok()?;
    let cell = held.at(&row, &column).ok()?;
    if let Some(failure) = cell.records().failure.clone() {
        return Some(format!("failed — {failure}"));
    }
    if cell.records().computed.is_none() {
        let owed =
            held.is_derived(&column) && cell.peek_value().is_none_or(|value| value.is_absent());
        return owed.then(|| "never computed".to_string());
    }
    let crate::core::table::RowAddress::Index(values) = row else {
        return None;
    };
    let word = crate::format::fingerprint::check_cell_among(sample, &table, &column, &values)
        .map(|state| word_of(sample, &state))
        .unwrap_or_default();
    let changed = crate::collection::computation::planned_states(config, path, sample)
        .unwrap_or_default()
        .get(&format!("{table}.{column}"))
        .filter(|said| said.contains("formula changed"))
        .cloned();
    Some(match changed {
        Some(said) if word == "current" => format!("outdated — {said}"),
        _ => word,
    })
}

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

/// A table as the workbench shows it: its index's columns, then those a cell
/// is in, and the rows it holds. One the file holds no row of is the model's
/// declaration: its columns, no row — unfolded to be filled, where only a
/// prompt for a whole row reached it.
pub struct TableLayout {
    pub index: Vec<Identifier>,
    pub columns: Vec<Identifier>,
    /// Whether the file holds it: a table the model declares may not be yet.
    pub held: bool,
    /// The columns the model computes, which a row's cells never take by hand.
    pub derived: Vec<String>,
}

impl TableLayout {
    /// The columns the cursor goes over: the index's first, as the file
    /// writes them.
    pub fn cursor_columns(&self) -> Vec<Identifier> {
        self.index.iter().chain(&self.columns).cloned().collect()
    }
}

/// [`TableLayout`] of a sample's table, or of the one its model declares.
pub fn table_layout(
    sample: &Sample,
    name: &Identifier,
    config: Option<&ProjectConfig>,
) -> Option<TableLayout> {
    let described = config
        .and_then(crate::config::model_runtime::current_description)
        .and_then(|described| described.tables.get(name.as_str()).cloned());
    let derived: Vec<String> = described
        .as_ref()
        .map(|table| {
            table
                .columns
                .iter()
                .filter(|column| column.formula.is_some())
                .map(|column| column.name.clone())
                .collect()
        })
        .unwrap_or_default();
    if let Ok(table) = sample.table(name) {
        return Some(TableLayout {
            index: table.index_columns().to_vec(),
            columns: value_columns(table),
            held: true,
            derived,
        });
    }
    let described = described?;
    let index: Vec<Identifier> = described
        .index
        .iter()
        .filter_map(|column| Identifier::new(column).ok())
        .collect();
    let columns = described
        .columns
        .iter()
        .filter_map(|column| Identifier::new(&column.name).ok())
        .filter(|column| !index.contains(column))
        .collect();
    Some(TableLayout {
        index,
        columns,
        held: false,
        derived,
    })
}

/// What a key does to a list in its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemChange {
    Remove,
    Down,
    Up,
    Toggle,
}

/// A row being typed at a table's foot, its cells by column, written once its
/// index is given.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub path: PathBuf,
    pub table: String,
    pub cells: Vec<(String, String)>,
}

impl Draft {
    /// What was typed in a column, or nothing.
    pub fn at(&self, column: &str) -> Option<&str> {
        self.cells
            .iter()
            .find(|(held, _)| held == column)
            .map(|(_, text)| text.as_str())
    }
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
/// A path as the command line names it from the folder the workbench runs in,
/// where the history says its command ran.
fn from_here(path: &Path) -> String {
    let here = std::env::current_dir().unwrap_or_default();
    let here = dunce::canonicalize(&here).unwrap_or(here);
    let full = dunce::canonicalize(path).unwrap_or_else(|_| normalised(path));
    let relative = full
        .strip_prefix(&here)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| relative_to(&here, &full));
    match relative.display().to_string() {
        empty if empty.is_empty() => ".".to_string(),
        shown => shown,
    }
}

/// A path made absolute and rid of its `.` and `..`, to compare two files.
fn normalised(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or(path.to_path_buf());
    path.components().fold(PathBuf::new(), |mut place, part| {
        match part {
            std::path::Component::ParentDir => {
                place.pop();
            }
            std::path::Component::CurDir => {}
            part => place.push(part),
        }
        place
    })
}

/// In `p`'s menu, the line nearest `at` that is no group's heading: further
/// up when moving up, down otherwise, and the other way where none is.
fn off_heading(purpose: &Choosing, shown: &[usize], at: usize, up: bool) -> usize {
    let Choosing::Produce(outputs, _) = purpose else {
        return at;
    };
    let line = |at: usize| {
        shown
            .get(at)
            .is_some_and(|&item| outputs.get(item) != Some(&Output::Heading))
    };
    let above = (0..=at).rev().find(|&at| line(at));
    let below = (at..shown.len()).find(|&at| line(at));
    let found = if up { above.or(below) } else { below.or(above) };
    found.unwrap_or(at)
}

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
    /// The snapshot's id, as the history names it.
    pub id: String,
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
    let noticed = version_control::notices().pop();
    failures
        .into_iter()
        .last()
        .map(|(_, error)| format!("the history was not kept: {error}"))
        .or(noticed)
}

/// What each file holds, byte for byte: a file in another encoding than
/// UTF-8 is held as it is, where it was taken for absent, and an undo then
/// removed it as one the change had created.
fn snapshot(paths: &[PathBuf]) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    paths
        .iter()
        .map(|path| (path.clone(), std::fs::read(path).ok()))
        .collect()
}

/// A file a change wrote: what it held before, and what the change left in
/// it — an undo restores the one only while the file still holds the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// The files of a snapshot a change has since written, each with what it
/// holds now; those it left as they were are no part of it.
fn settled(before: Vec<(PathBuf, Option<Vec<u8>>)>) -> Vec<Undone> {
    before
        .into_iter()
        .filter_map(|(path, before)| {
            let after = std::fs::read(&path).ok();
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
/// a word, and near the start all score higher: `mw` finds `preheat_water`
/// before `preheating_water`. Scored by nucleo, fzf's scheme as Helix keeps it.
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
/// The requests of one group, on the interpreter that runs them, with the
/// records each sample held before: what the trace keeps is told by them.
struct Job {
    python: PathBuf,
    config: Option<crate::config::project_config::ProjectConfig>,
    requests: Vec<runtime::Request>,
    before: std::collections::HashMap<PathBuf, Vec<version_control::HeldRecord>>,
}

fn run_jobs(jobs: Vec<Job>, kept: usize, sender: &mpsc::Sender<Progress>) {
    let (mut computed, mut failed, mut pending, mut waiting) = (0, 0, 0, 0);
    let mut refused: Vec<String> = Vec::new();
    // Steps are values: each sample's plan asked for first, so that the bar
    // moves by what is computed rather than by samples of any size.
    let mut total = 0;
    let finished = std::cell::Cell::new(0usize);
    let mut tracing = computation::Tracing::default();
    // What was computed, for the outcome to name: kept over every screen until
    // the next run, `✓ 1 computed` was read on the collection as said of it.
    let mut samples: Vec<String> = Vec::new();
    // Those a value was computed in: what the outcome names, where it named
    // every sample asked about, `8 computed in 12 samples` of two.
    let mut made: Vec<String> = Vec::new();
    // Those a value failed in, counted with them as the command line does.
    let failed_names: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    for Job {
        python,
        config,
        requests,
        before,
    } in jobs
    {
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
        for request in &requests {
            let label = request
                .sample
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            samples.push(label.clone());
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
                    failed_names.borrow_mut().push(label.clone());
                    let last = crate::presentation::changes::error_line(traceback);
                    let _ = sender.send(Progress::Said(format!("{label} {value} failed: {last}")));
                }
                _ => {}
            };
            match worker.compute(request, true, &mut on) {
                Ok(report) => {
                    if report.computed > 0 {
                        made.push(label.clone());
                    }
                    computed += report.computed;
                    failed += report.failed;
                    pending += report.pending;
                    waiting += report.waiting;
                    tracing.wrote(
                        config.as_ref(),
                        before.get(&request.sample).map_or(&[][..], Vec::as_slice),
                        &request.sample,
                        &report,
                    );
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
    // What the writes computed, kept in each project's history; a trace not
    // kept is said as the history's failures are.
    for (root, error) in tracing.keep() {
        refused.push(format!(
            "the trace of {} was not kept: {error}",
            root.display()
        ));
    }
    // Nothing ran because it was refused is not nothing to compute; nor is what
    // waits for its inputs.
    let named = |names: &[String]| match names {
        [one] => format!(" in {one}"),
        [a, b] => format!(" in {a}, {b}"),
        [a, b, c] => format!(" in {a}, {b}, {c}"),
        many => format!(" in {} samples", many.len()),
    };
    let failed_in = failed_names.into_inner();
    let within = named(&samples);
    // The command line's sentence: one way to say how a computation ended.
    let ran: std::collections::BTreeSet<&String> = made.iter().chain(&failed_in).collect();
    let mut said = if computed + failed == 0 && refused.is_empty() && waiting == 0 {
        format!("nothing to change — nothing to compute{within}")
    } else if computed + failed == 0 {
        format!("nothing computed{within}")
    } else {
        format!(
            "computed {} in {}",
            counted(computed, "value", "values"),
            counted(ran.len(), "sample", "samples")
        )
    };
    if failed > 0 {
        said.push_str(&format!(", {failed} failed — Enter on it explains"));
    }
    if pending > 0 {
        // Never computed, as often as outdated: said as the command line
        // says it, where a value never computed was said outdated.
        said.push_str(&format!(
            ", {pending} left outdated or never computed — C computes them"
        ));
    }
    if waiting > 0 {
        said.push_str(&format!(
            ", {} {}",
            counted(waiting, "waits", "wait"),
            if failed > 0 {
                "behind what failed"
            } else {
                "for what nobody entered"
            }
        ));
    }
    if kept > 0 {
        said.push_str(&format!(
            " · {} {} kept — c on {} gives it back to its formula",
            counted(kept, "value typed", "values typed"),
            if kept == 1 {
                "over its formula"
            } else {
                "over their formulas"
            },
            if kept == 1 { "it" } else { "one" }
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
/// quantities. A row names its sample: a profile holding no `name` — one made
/// for a figure's export, its quantity alone — opens with the name first.
fn default_profile(collection: &SampleList) -> Profile {
    if let Some(config) = collection.config()
        && let Some(name) = config.profile_names().first()
        && let Ok(profile) = config.profile(name)
    {
        let mut profile = profile.clone();
        if !profile.columns.iter().any(|column| column.field == "name") {
            profile.columns.insert(
                0,
                ColumnSpec {
                    field: "name".to_string(),
                    label: None,
                    header: None,
                    precision: None,
                    template: None,
                },
            );
        }
        return profile;
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

/// [`word`] of a value of `sample`: one outdated for want of an input
/// emptied since waits for it, as `status` says — the workbench marked it
/// outdated, and `C` could do nothing for it.
pub fn word_of(sample: &Sample, state: &Freshness) -> String {
    let emptied = editing::emptied_inputs(sample, state);
    if emptied.is_empty() {
        word(state)
    } else {
        format!("waits for {}", emptied.join(", "))
    }
}

/// A state as the command line's `status` says it, whole: each input named,
/// *(itself not current)* where `word` writes `…`.
fn said_whole(sample: &Sample, state: &Freshness) -> String {
    let emptied = editing::emptied_inputs(sample, state);
    if emptied.is_empty() {
        crate::presentation::changes::state_said(state)
    } else {
        format!("waits for {}", emptied.join(", "))
    }
}

/// A state as the command line's `status` says it, whole: one word and one
/// sentence for each state on every surface.
pub fn word(state: &Freshness) -> String {
    match state {
        Freshness::Source => String::new(),
        Freshness::Current => "current".to_string(),
        other => crate::presentation::changes::state_said(other),
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
                    "  {:<16} {:<10} {}",
                    column.name.to_string(),
                    column.unit.clone().unwrap_or_default(),
                    column.kind.said()
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

/// The folders the workbench kept a state for, newest first, those still there:
/// the start page's recent projects, nine of them once the demo's steps are
/// left out. A state written before the folder was kept whole is not listed.
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
                    let folder = PathBuf::from(&state.directory);
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
    recent
}

fn remembered(root: &Path) -> Option<Remembered> {
    let file = state_file(root)?;
    let text = std::fs::read_to_string(&file).ok()?;
    match toml::from_str(&text) {
        Ok(state) => Some(state),
        // Said, and kept aside: read as none, it was written over by an
        // empty state as the session ended.
        Err(error) => {
            UNREAD_STATE.with(|said| {
                *said.borrow_mut() = Some(format!(
                    "what was remembered of this folder could not be read, and is kept aside \
                     as {}: {error}",
                    file.with_extension("unread.toml").display()
                ))
            });
            let _ = std::fs::rename(&file, file.with_extension("unread.toml"));
            None
        }
    }
}

thread_local! {
    /// What [`remembered`] could not read, for the workbench to say.
    static UNREAD_STATE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Saves what a session leaves for the next. A state that could not be
/// written is said once the workbench has closed: `Err` names it.
pub fn remember(workbench: &Workbench) -> Result<(), String> {
    // A folder gone — the one `+` made, left from the setup's first question
    // — is no project to come back to.
    if !workbench.root.is_dir() {
        return Ok(());
    }
    let Some(file) = state_file(&workbench.root) else {
        return Ok(());
    };
    let text = toml::to_string(&workbench.remembered()).map_err(|error| error.to_string())?;
    if let Some(directory) = file.parent() {
        std::fs::create_dir_all(directory).map_err(|error| {
            format!(
                "what the workbench remembers was not kept: {}: {error}",
                directory.display()
            )
        })?;
    }
    std::fs::write(&file, text).map_err(|error| {
        format!(
            "what the workbench remembers was not kept: {}: {error}",
            file.display()
        )
    })
}
