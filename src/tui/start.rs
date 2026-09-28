//! The start page: what the workbench shows when it is opened where there is no
//! project — the recent projects, a new one, a folder to open, a project to
//! configure, the guide and the demo. It chooses a folder and what to do with
//! it; the workbench does it. The demo it writes itself, from the copy
//! embedded.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::model::{Key, Workbench, recent_projects, state_file};
use super::theme::{Theme, keyed};
use super::typing::Typed;

/// Where the guide is read: the site of this version, which the published
/// documentation keeps beside the others.
pub const GUIDE: &str = concat!(
    "https://zelyph.github.io/SampleKit/",
    env!("CARGO_PKG_VERSION"),
    "/index.html"
);
/// Where the tutorial is read: the demo's steps' own pages.
pub const TUTORIAL: &str = concat!(
    "https://zelyph.github.io/SampleKit/",
    env!("CARGO_PKG_VERSION"),
    "/tutorial/index.html"
);
/// The demo's steps: each folder, in order, and what it is about in a line.
/// Each step's name is its README's heading (`demo_steps`).
pub const STEPS: [(&str, &str); 8] = [
    (
        "01-first-brews",
        "three brews written by hand, read with your first commands",
    ),
    (
        "02-measuring",
        "uncertainties, units and tags; a change shown before it is written",
    ),
    (
        "03-selecting",
        "twelve brews filtered, shown and sorted; queries given a name",
    ),
    (
        "04-computing",
        "a model computes values, and says which are not current",
    ),
    (
        "05-fermentation",
        "a table in each brew: cells read, filtered, computed row by row",
    ),
    (
        "06-figures",
        "figures drawn by matplotlib, in a window or to a file",
    ),
    (
        "07-two-brewers",
        "two projects read as one, then the TUI with the keyboard",
    ),
    (
        "08-python",
        "the brews read, changed and computed from Python",
    ),
];
/// `examples/brewing/`, embedded by `build.rs`: a module of its own, so that
/// the constants of its contents stay out of this one's names.
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/demo.rs"));
}
pub use embedded::DEMO_FILES;

/// What the start page chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The workbench on a folder.
    Open(PathBuf),
    /// The workbench on a project, on its configuration (P).
    Configure(PathBuf),
    /// The workbench on a folder, on its setup's questions.
    Create(PathBuf),
    /// The same, on a folder `+` made: left from the setup's first question, it
    /// is removed while still empty.
    Made(PathBuf),
    /// The workbench on a step of the demo: quitting it comes back to the
    /// demo's page.
    Step(PathBuf),
    Quit,
}

/// What a key asks of the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// A web page, in the browser.
    Browse(&'static str),
    Done(Outcome),
}

/// Why a folder is being chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Open,
    Create,
    Configure,
}

/// One line of the folder being moved through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// The folder itself, chosen. No `..`: `←` `h` go up.
    This,
    Folder {
        path: PathBuf,
        project: bool,
    },
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Menu,
    /// Moving through the file system to a folder; `narrowing` the letters
    /// typed after `/`, the folders holding them shown, the closest first,
    /// `None` all of them.
    Browse {
        purpose: Purpose,
        folder: PathBuf,
        entries: Vec<Entry>,
        cursor: usize,
        narrowing: Option<Typed>,
    },
    /// A new folder's name, typed in its line, in `folder`.
    Naming {
        folder: PathBuf,
        text: Typed,
    },
    /// A recent project to configure, or another found by moving to it.
    Pick {
        cursor: usize,
    },
    /// The guide and the demo.
    Guide {
        cursor: usize,
    },
    /// The demo's page: its steps, the tutorial, its reset.
    Demo {
        cursor: usize,
    },
    /// The demo's reset, asked first: what it loses is said.
    Reset,
    /// `?`: the page's keys, and what each entry does; `back` the mode it was
    /// asked from, returned to as it was.
    Help {
        scroll: usize,
        back: Box<Mode>,
    },
}

/// The page's help, as `?` shows it: a heading, or a key and what it does.
pub const HELP: &[(&str, &str)] = &[
    ("", "The start page"),
    (
        "Enter",
        "open the project marked: the one SampleKit was started in comes first",
    ),
    ("1…9", "open that recent project"),
    (
        "x",
        "forget the recent project marked: its state removed, the folder kept",
    ),
    ("j k ↑ ↓", "move; Home End to the first and the last"),
    (
        "n",
        "New project: a folder moved to, or one + makes, then set up by a few questions",
    ),
    (
        "",
        "  Esc on the first question comes back here, the folder + made removed if still empty",
    ),
    (
        "o",
        "Open a folder: Enter on a project opens it, on any other goes in",
    ),
    (
        "c",
        "Configure a project: a recent one holding a .samplekitrc, or another moved to",
    ),
    (
        "g",
        "Guide and demo: the guide in the browser; the demo's page, its steps",
    ),
    (
        "",
        "  the first time, the demo is written and its first step opened",
    ),
    ("q Esc", "quit"),
    ("Ctrl+C", "quit, from the page or from a project"),
    ("?", "this help"),
    ("", ""),
    ("", "Moving through folders"),
    ("→ l", "into the folder marked"),
    ("← h", "up; Backspace too"),
    ("~", "the home folder"),
    (
        "/",
        "narrow the folders by typing; Esc clears it, as going in does",
    ),
    ("+", "a new folder here, for a new project"),
    (
        "Enter",
        "the folder itself, chosen; a project opened; another gone into",
    ),
    ("Esc", "back to the page"),
    ("", ""),
    ("", "The demo"),
    (
        "1…8",
        "open that step; quitting it comes back to the demo's page",
    ),
    (
        "Enter",
        "the step marked; the tutorial, the steps' own pages, in the browser; or Reset the demo, asked first",
    ),
    ("Esc q", "back to the start page"),
    ("", ""),
    (
        "",
        "A project opened from here comes back here when it is quit.",
    ),
];

/// The page's entries below the recent projects: a key, a name, what it
/// does.
pub const ITEMS: [(char, &str, &str); 5] = [
    (
        'n',
        "New project",
        "a folder chosen or made, then set up by a few questions",
    ),
    ('o', "Open a folder", "move to it; the projects are marked"),
    (
        'c',
        "Configure a project",
        "its .samplekitrc, section by section",
    ),
    (
        'g',
        "Guide and demo",
        "the guide to read, the demo's steps to try",
    ),
    ('q', "Quit", ""),
];

pub struct Start {
    /// The project SampleKit was started in, if any: offered first, Enter
    /// opening it.
    pub current: Option<PathBuf>,
    /// The folders the workbench kept a state for, newest first, the current
    /// project left out.
    pub recent: Vec<PathBuf>,
    /// Among the current project, the recent ones, then the items.
    pub cursor: usize,
    pub mode: Mode,
    pub message: String,
    /// Where SampleKit was started, the folder moving starts from.
    pub here: PathBuf,
    /// Where the demo is, or is written: `demo_folder`, `None` where the system
    /// has no data folder.
    pub demo: Option<PathBuf>,
    /// How the demo's environment is made: `demo_environment`, which the
    /// tests replace rather than install anything.
    pub environment: fn(&Path) -> Result<String, String>,
    /// The demo's environment being made, for the TUI opened on its first
    /// step to say when it is done (`hand_over`).
    pub installing: Option<mpsc::Receiver<Result<String, String>>>,
    /// Where each list was scrolled when last drawn, by name: kept from one
    /// frame to the next, the rows move only when the cursor leaves them, as
    /// the workbench's do. Scrolled afresh each frame, the browser held its
    /// cursor at the foot, the rows below it never shown.
    pub offsets: RefCell<HashMap<&'static str, usize>>,
}

impl Start {
    pub fn new(here: &Path) -> Start {
        Start::with_recent(here, recent_projects())
    }

    pub fn with_recent(here: &Path, recent: Vec<PathBuf>) -> Start {
        let here = dunce::canonicalize(here).unwrap_or_else(|_| here.to_path_buf());
        let current = project_at(&here);
        let demo = demo_folder();
        // The demo's steps are reached from its page, never listed here: they
        // pushed the user's own projects out of the nine. Its folder as written
        // and as resolved, a link in the data folder's path followed.
        let demo_paths: Vec<PathBuf> = demo
            .iter()
            .flat_map(|folder| [Some(folder.clone()), dunce::canonicalize(folder).ok()])
            .flatten()
            .collect();
        Start {
            recent: recent
                .into_iter()
                .filter(|folder| Some(folder) != current.as_ref())
                .filter(|folder| !demo_paths.iter().any(|demo| folder.starts_with(demo)))
                .collect(),
            current,
            cursor: 0,
            mode: Mode::Menu,
            message: String::new(),
            here,
            demo,
            environment: demo_environment,
            installing: None,
            offsets: RefCell::default(),
        }
    }

    /// The entries above the recent projects: the current one, if any.
    fn lead(&self) -> usize {
        usize::from(self.current.is_some())
    }

    /// The projects that are projects, the current one first: the ones to
    /// configure.
    pub fn configurable(&self) -> Vec<PathBuf> {
        self.current
            .iter()
            .chain(&self.recent)
            .filter(|folder| folder.join(".samplekitrc").is_file())
            .cloned()
            .collect()
    }

    /// Moving through `folder` for `purpose`, the cursor on its first folder.
    fn browse(&mut self, purpose: Purpose, folder: PathBuf) {
        // Resolved, so that a link followed names where it leads rather
        // than growing a path through it.
        let folder = dunce::canonicalize(&folder).unwrap_or(folder);
        let entries = entries(&folder);
        let cursor = entries
            .iter()
            .position(|entry| matches!(entry, Entry::Folder { .. }))
            .unwrap_or(0);
        self.message.clear();
        // Another folder's rows, from its top.
        self.offsets.borrow_mut().remove("browse");
        self.mode = Mode::Browse {
            purpose,
            folder,
            entries,
            cursor,
            narrowing: None,
        };
    }

    /// The first row of the list `name` shown in `rows` of `length`: where it
    /// was last drawn, moved only as far as brings `top` to `cursor` into
    /// sight, and no further down than fills the rows. `top` is the cursor's
    /// line, or the heading just above it.
    fn window(
        &self,
        name: &'static str,
        top: usize,
        cursor: usize,
        rows: usize,
        length: usize,
    ) -> usize {
        let kept = self.offsets.borrow().get(name).copied().unwrap_or(0);
        let mut first = kept.min(length.saturating_sub(rows));
        if top < first {
            first = top;
        }
        if cursor >= first + rows {
            first = cursor + 1 - rows;
        }
        self.offsets.borrow_mut().insert(name, first);
        first
    }

    /// The help's last first line: the one that shows its end at the foot,
    /// as last drawn; its last line before it is.
    fn help_end(&self) -> usize {
        self.offsets
            .borrow()
            .get("help-end")
            .copied()
            .unwrap_or(HELP.len().saturating_sub(1))
    }

    /// The page shown again once the workbench `outcome` opened is quit: the
    /// demo's page, its step marked, when it was a step of it.
    pub fn returning(&mut self, outcome: &Outcome) {
        if let Outcome::Step(step) = outcome {
            let name = step.file_name().map(|name| name.to_string_lossy());
            let cursor = STEPS
                .iter()
                .position(|(folder, _)| name.as_deref() == Some(folder))
                .unwrap_or(0);
            self.message.clear();
            self.mode = Mode::Demo { cursor };
        }
    }

    /// Whether a name or the folders' letters are being typed.
    fn typing(&mut self) -> Option<&mut Typed> {
        match &mut self.mode {
            Mode::Browse {
                narrowing: Some(text),
                ..
            }
            | Mode::Naming { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Text pasted into the terminal, typed into the line being typed, if one
    /// is.
    pub fn paste(&mut self, pasted: &str) {
        if let Some(text) = self.typing() {
            text.insert(pasted);
        }
        // The folders narrowed again by what the letters became.
        if let Mode::Browse {
            entries,
            cursor,
            narrowing: Some(text),
            ..
        } = &mut self.mode
            && let Some(first) = narrowed(entries, text.text()).first()
        {
            *cursor = *first;
        }
    }

    /// Ctrl+C over a selection in the line typed: copied, and no interrupt.
    pub fn copy(&mut self) -> bool {
        self.typing().is_some_and(|text| text.copy())
    }

    pub fn key(&mut self, key: Key) -> Effect {
        // Held with Ctrl or Alt, a key is the line's where one is typed, and
        // nothing elsewhere.
        let key = if self.typing().is_some() {
            key
        } else {
            match super::model::plain(key) {
                Some(key) => key,
                None => return Effect::None,
            }
        };
        // `?` shows the page's help wherever a name is not being typed.
        // Nor where the folders are narrowed: there it is a letter.
        if key == Key::Char('?')
            && !matches!(
                self.mode,
                Mode::Naming { .. }
                    | Mode::Help { .. }
                    | Mode::Browse {
                        narrowing: Some(_),
                        ..
                    }
            )
        {
            let back = Box::new(std::mem::replace(&mut self.mode, Mode::Menu));
            self.mode = Mode::Help { scroll: 0, back };
            return Effect::None;
        }
        match std::mem::replace(&mut self.mode, Mode::Menu) {
            Mode::Help { scroll, back } => {
                match key {
                    Key::Down | Key::Char('j') => {
                        self.mode = Mode::Help {
                            scroll: (scroll + 1).min(self.help_end()),
                            back,
                        }
                    }
                    Key::Up | Key::Char('k') => {
                        self.mode = Mode::Help {
                            scroll: scroll.saturating_sub(1),
                            back,
                        }
                    }
                    Key::Esc | Key::Char('q' | '?') | Key::Enter => self.mode = *back,
                    _ => self.mode = Mode::Help { scroll, back },
                }
                Effect::None
            }
            Mode::Menu => self.menu_key(key),
            Mode::Browse {
                purpose,
                folder,
                entries,
                cursor,
                narrowing,
            } => match narrowing {
                Some(text) => self.narrowing_key(key, purpose, folder, entries, cursor, text),
                None => self.browse_key(key, purpose, folder, entries, cursor),
            },
            Mode::Naming { folder, mut text } => match key {
                Key::Esc => {
                    self.browse(Purpose::Create, folder);
                    Effect::None
                }
                Key::Enter => {
                    let name = text.text().trim().to_string();
                    let name = name.as_str();
                    if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
                        self.message = "a folder's name, without a slash".to_string();
                        self.mode = Mode::Naming { folder, text };
                        return Effect::None;
                    }
                    let made = folder.join(name);
                    match std::fs::create_dir(&made) {
                        Ok(()) => Effect::Done(Outcome::Made(made)),
                        Err(error) => {
                            self.message = format!("{}: {error}", made.display());
                            self.mode = Mode::Naming { folder, text };
                            Effect::None
                        }
                    }
                }
                // Every other key is the line's: letters, words, a selection,
                // undo, the mouse.
                _ => {
                    text.key(key);
                    self.mode = Mode::Naming { folder, text };
                    Effect::None
                }
            },
            // Only `y` loses the demo's changes; any other key goes back.
            Mode::Reset => {
                self.mode = Mode::Demo {
                    cursor: STEPS.len() + 1,
                };
                match (key, self.demo.clone()) {
                    (Key::Char('y'), Some(folder)) => {
                        if let Err(error) = std::fs::remove_dir_all(&folder) {
                            self.message = format!("{}: {error}", shown(&folder));
                            return Effect::None;
                        }
                        self.write_and_open(folder)
                    }
                    _ => Effect::None,
                }
            }
            Mode::Pick { cursor } => {
                let projects = self.configurable();
                // The recent projects, then another found by moving to it.
                let last = projects.len();
                match key {
                    Key::Down | Key::Char('j') => {
                        self.mode = Mode::Pick {
                            cursor: (cursor + 1).min(last),
                        }
                    }
                    Key::Up | Key::Char('k') => {
                        self.mode = Mode::Pick {
                            cursor: cursor.saturating_sub(1),
                        }
                    }
                    Key::Enter => {
                        return match projects.get(cursor) {
                            Some(project) => Effect::Done(Outcome::Configure(project.clone())),
                            None => {
                                self.browse(Purpose::Configure, self.here.clone());
                                Effect::None
                            }
                        };
                    }
                    Key::Esc | Key::Char('q') => {}
                    _ => self.mode = Mode::Pick { cursor },
                }
                Effect::None
            }
            Mode::Demo { cursor } => self.demo_key(key, cursor),
            Mode::Guide { cursor } => match key {
                Key::Down | Key::Char('j') => {
                    self.mode = Mode::Guide {
                        cursor: (cursor + 1).min(1),
                    };
                    Effect::None
                }
                Key::Up | Key::Char('k') => {
                    self.mode = Mode::Guide {
                        cursor: cursor.saturating_sub(1),
                    };
                    Effect::None
                }
                Key::Enter => {
                    self.mode = Mode::Guide { cursor };
                    match cursor {
                        0 => Effect::Browse(GUIDE),
                        _ => self.open_demo(),
                    }
                }
                Key::Esc | Key::Char('q') => Effect::None,
                _ => {
                    self.mode = Mode::Guide { cursor };
                    Effect::None
                }
            },
        }
    }

    fn menu_key(&mut self, key: Key) -> Effect {
        let lead = self.lead();
        let count = lead + self.recent.len() + ITEMS.len();
        let item = |character: char| ITEMS.iter().position(|(key, ..)| *key == character);
        let chosen = match key {
            Key::Down | Key::Char('j') => {
                self.cursor = (self.cursor + 1).min(count - 1);
                return Effect::None;
            }
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                return Effect::None;
            }
            Key::Home => {
                self.cursor = 0;
                return Effect::None;
            }
            Key::End => {
                self.cursor = count - 1;
                return Effect::None;
            }
            Key::Esc => return Effect::Done(Outcome::Quit),
            // The recent project marked forgotten: its state removed, the
            // folder untouched.
            Key::Char('x') => {
                let Some(index) = self.cursor.checked_sub(lead) else {
                    return Effect::None;
                };
                if index < self.recent.len() {
                    let folder = self.recent.remove(index);
                    let _ = state_file(&folder).map(std::fs::remove_file);
                    self.message =
                        format!("{} is no longer among the recent projects", shown(&folder));
                    self.cursor = self.cursor.min(lead + self.recent.len() + ITEMS.len() - 1);
                }
                return Effect::None;
            }
            Key::Enter => self.cursor,
            Key::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if index >= self.recent.len() {
                    return Effect::None;
                }
                lead + index
            }
            Key::Char(character) => match item(character) {
                Some(index) => lead + self.recent.len() + index,
                None => return Effect::None,
            },
            _ => return Effect::None,
        };
        if let Some(folder) = self.current.iter().chain(&self.recent).nth(chosen) {
            return Effect::Done(Outcome::Open(folder.clone()));
        }
        match ITEMS[chosen - lead - self.recent.len()].0 {
            'n' => self.browse(Purpose::Create, self.here.clone()),
            'o' => self.browse(Purpose::Open, self.here.clone()),
            'c' => {
                if self.configurable().is_empty() {
                    self.browse(Purpose::Configure, self.here.clone());
                } else {
                    self.mode = Mode::Pick { cursor: 0 };
                }
            }
            'g' => self.mode = Mode::Guide { cursor: 0 },
            _ => return Effect::Done(Outcome::Quit),
        }
        Effect::None
    }

    fn browse_key(
        &mut self,
        key: Key,
        purpose: Purpose,
        folder: PathBuf,
        entries: Vec<Entry>,
        cursor: usize,
    ) -> Effect {
        let last = entries.len().saturating_sub(1);
        let moved = |cursor: usize| Mode::Browse {
            purpose,
            folder: folder.clone(),
            entries: entries.clone(),
            cursor,
            narrowing: None,
        };
        let up = folder.parent().map(Path::to_path_buf);
        match key {
            Key::Down | Key::Char('j') => self.mode = moved((cursor + 1).min(last)),
            Key::Up | Key::Char('k') => self.mode = moved(cursor.saturating_sub(1)),
            Key::Home => self.mode = moved(0),
            Key::End => self.mode = moved(last),
            Key::PageDown => self.mode = moved((cursor + 10).min(last)),
            Key::PageUp => self.mode = moved(cursor.saturating_sub(10)),
            Key::Esc | Key::Char('q') => {}
            Key::Left | Key::Char('h') | Key::Backspace => match up {
                Some(parent) => self.browse(purpose, parent),
                // Above a drive's root, the drives: on Windows, `C:\` was
                // where going up stopped, another drive out of reach.
                None if !folder.as_os_str().is_empty() && !drives().is_empty() => {
                    self.browse(purpose, PathBuf::new())
                }
                None => self.mode = moved(cursor),
            },
            Key::Char('~') => match etcetera::home_dir() {
                Ok(home) => self.browse(purpose, home),
                Err(_) => self.mode = moved(cursor),
            },
            // Narrowed by typing, as the pickers are: the cursor on a folder,
            // `‹ this folder ›` left out.
            Key::Char('/') => {
                let cursor = match entries.get(cursor) {
                    Some(Entry::Folder { .. }) => cursor,
                    _ => narrowed(&entries, "").first().copied().unwrap_or(cursor),
                };
                self.message.clear();
                self.mode = Mode::Browse {
                    purpose,
                    folder,
                    entries,
                    cursor,
                    narrowing: Some(Typed::new("")),
                };
            }
            Key::Char('+') if purpose == Purpose::Create => {
                self.message.clear();
                self.mode = Mode::Naming {
                    folder,
                    text: Typed::new(""),
                };
            }
            Key::Right | Key::Char('l') => match entries.get(cursor) {
                Some(Entry::Folder { path, .. }) => self.browse(purpose, path.clone()),
                _ => self.mode = moved(cursor),
            },
            Key::Enter => match entries.get(cursor) {
                Some(Entry::This) => return self.chosen(purpose, folder),
                Some(Entry::Folder { path, project }) => {
                    return self.entered(purpose, path.clone(), *project);
                }
                None => self.mode = moved(cursor),
            },
            _ => self.mode = moved(cursor),
        }
        Effect::None
    }

    /// `Enter` on a folder: a project is chosen; any other folder is gone
    /// into.
    fn entered(&mut self, purpose: Purpose, path: PathBuf, project: bool) -> Effect {
        if project && purpose != Purpose::Create {
            return self.chosen(purpose, path);
        }
        self.browse(purpose, path);
        Effect::None
    }

    /// A key while the folders are narrowed: letters typed, the cursor moved
    /// among the folders shown, `Enter` on the one marked; `Esc` clears the
    /// narrowing, the cursor kept on its folder.
    fn narrowing_key(
        &mut self,
        key: Key,
        purpose: Purpose,
        folder: PathBuf,
        entries: Vec<Entry>,
        cursor: usize,
        mut text: Typed,
    ) -> Effect {
        let shown = narrowed(&entries, text.text());
        let at = shown.iter().position(|index| *index == cursor);
        let step = |by: isize| -> usize {
            match at {
                Some(at) => {
                    let to = (at as isize + by).clamp(0, shown.len() as isize - 1);
                    shown[to as usize]
                }
                None => shown.first().copied().unwrap_or(cursor),
            }
        };
        let (cursor, narrowing) = match key {
            Key::Esc => (cursor, None),
            Key::Down => (step(1), Some(text)),
            Key::Up => (step(-1), Some(text)),
            Key::PageDown => (step(10), Some(text)),
            Key::PageUp => (step(-10), Some(text)),
            Key::Home => (shown.first().copied().unwrap_or(cursor), Some(text)),
            Key::End => (shown.last().copied().unwrap_or(cursor), Some(text)),
            Key::Enter | Key::Right => {
                if let (Some(_), Some(Entry::Folder { path, project })) = (at, entries.get(cursor))
                {
                    if key == Key::Right {
                        self.browse(purpose, path.clone());
                        return Effect::None;
                    }
                    return self.entered(purpose, path.clone(), *project);
                }
                (cursor, Some(text))
            }
            // Up a folder, the narrowing cleared with the folder.
            Key::Left => {
                self.mode = Mode::Browse {
                    purpose,
                    folder,
                    entries,
                    cursor,
                    narrowing: None,
                };
                return self.key(Key::Left);
            }
            Key::Backspace if text.is_empty() => (cursor, None),
            // The letters are the line's: typed, taken back by letter or by
            // word, selected, undone. `← → Home End` move among the folders, as
            // unnarrowed.
            _ => {
                let before = text.text().to_string();
                text.key(key);
                if text.text() == before {
                    (cursor, Some(text))
                } else {
                    let first = narrowed(&entries, text.text()).first().copied();
                    (first.unwrap_or(cursor), Some(text))
                }
            }
        };
        self.mode = Mode::Browse {
            purpose,
            folder,
            entries,
            cursor,
            narrowing,
        };
        Effect::None
    }

    /// A key on the demo's page: a step opened by `Enter` or its digit, the
    /// tutorial in the browser, the reset asked; `Esc` `q` back to the start
    /// page.
    fn demo_key(&mut self, key: Key, cursor: usize) -> Effect {
        let last = STEPS.len() + 1;
        let chosen = match key {
            Key::Down | Key::Char('j') => {
                self.mode = Mode::Demo {
                    cursor: (cursor + 1).min(last),
                };
                return Effect::None;
            }
            Key::Up | Key::Char('k') => {
                self.mode = Mode::Demo {
                    cursor: cursor.saturating_sub(1),
                };
                return Effect::None;
            }
            Key::Home => {
                self.mode = Mode::Demo { cursor: 0 };
                return Effect::None;
            }
            Key::End => {
                self.mode = Mode::Demo { cursor: last };
                return Effect::None;
            }
            Key::Esc | Key::Char('q') => return Effect::None,
            Key::Enter => cursor,
            Key::Char(digit @ '1'..='8') => digit as usize - '1' as usize,
            _ => {
                self.mode = Mode::Demo { cursor };
                return Effect::None;
            }
        };
        self.mode = Mode::Demo { cursor: chosen };
        if chosen == STEPS.len() {
            return Effect::Browse(TUTORIAL);
        }
        let Some(folder) = self.demo.clone() else {
            return self.open_demo();
        };
        // Nothing to lose where it was never written, or was removed since.
        if !written(&folder) {
            return self.write_and_open(folder);
        }
        if chosen == last {
            self.message.clear();
            self.mode = Mode::Reset;
            return Effect::None;
        }
        Effect::Done(Outcome::Step(folder.join(STEPS[chosen].0)))
    }

    /// The demo's page where the demo is — its steps, so that a second visit
    /// goes on at any step — or the demo written where it is not.
    fn open_demo(&mut self) -> Effect {
        let Some(folder) = self.demo.clone() else {
            self.message = "this system has no data folder to write the demo in: \
                            SAMPLEKIT_DATA_DIR names one"
                .to_string();
            return Effect::None;
        };
        if !written(&folder) {
            return self.write_and_open(folder);
        }
        self.message.clear();
        self.mode = Mode::Demo { cursor: 0 };
        Effect::None
    }

    /// The demo written into `folder`, then its first step opened, its
    /// environment made beside it: the TUI opened on the step says how the
    /// install went, as it does for a project set up.
    fn write_and_open(&mut self, folder: PathBuf) -> Effect {
        if let Err(error) = write_demo(&folder) {
            self.message = error;
            return Effect::None;
        }
        let (sender, receiver) = mpsc::channel();
        let make = self.environment;
        let root = folder.clone();
        std::thread::spawn(move || {
            let _ = sender.send(make(&root));
        });
        self.installing = Some(receiver);
        self.message = format!(
            "the demo is written in {} · installing samplekit {} in its .venv/, a minute or so",
            shown(&folder),
            crate::config::project_setup::python_spelling(env!("CARGO_PKG_VERSION"))
        );
        Effect::Done(Outcome::Step(folder.join(STEPS[0].0)))
    }

    /// What the page leaves the TUI it opened: the demo's environment being
    /// made, and what the page said of it — the page is gone once the TUI
    /// is open, and the install goes on.
    pub fn hand_over(&mut self, workbench: &mut Workbench) {
        if let Some(installing) = self.installing.take() {
            workbench.installing = Some(installing);
            workbench.message = std::mem::take(&mut self.message);
        }
    }

    /// `folder`, chosen for `purpose`: resolved, as a folder gone into is,
    /// so that a link opens where it leads and the recent projects never
    /// hold two paths for one project.
    fn chosen(&mut self, purpose: Purpose, folder: PathBuf) -> Effect {
        let folder = dunce::canonicalize(&folder).unwrap_or(folder);
        match purpose {
            Purpose::Open => Effect::Done(Outcome::Open(folder)),
            Purpose::Create => Effect::Done(Outcome::Create(folder)),
            Purpose::Configure if folder.join(".samplekitrc").is_file() => {
                Effect::Done(Outcome::Configure(folder))
            }
            Purpose::Configure => {
                let said = format!(
                    "{} is not a project: it holds no .samplekitrc",
                    shown(&folder)
                );
                self.browse(purpose, folder);
                self.message = said;
                Effect::None
            }
        }
    }
}

/// The entries `text` keeps, by their positions: the folders whose names hold
/// its letters in order, the closest first, as the pickers narrow; every folder
/// where nothing is typed.
fn narrowed(entries: &[Entry], text: &str) -> Vec<usize> {
    let folders: Vec<(usize, (String, bool))> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            Entry::Folder { path, .. } => Some((index, (folder_name(path), false))),
            Entry::This => None,
        })
        .collect();
    let items: Vec<(String, bool)> = folders.iter().map(|(_, item)| item.clone()).collect();
    super::model::visible(&items, text)
        .into_iter()
        .map(|at| folders[at].0)
        .collect()
}

/// A folder's name as the browser lists it: a drive's root, which has
/// none, by its letter.
fn folder_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// The demo's steps: each folder, its name — its README's heading, `Step 1 ·`
/// left out — and what it is about.
pub fn demo_steps() -> Vec<(&'static str, String, &'static str)> {
    STEPS
        .iter()
        .map(|(folder, what)| {
            let readme = format!("{folder}/README.md");
            let heading = DEMO_FILES
                .iter()
                .find(|(name, _)| *name == readme)
                .and_then(|(_, bytes)| {
                    let text = String::from_utf8_lossy(bytes);
                    let line = text.lines().find(|line| line.starts_with("# "))?;
                    let heading = line.trim_start_matches("# ");
                    Some(match heading.split_once(" · ") {
                        Some((_, name)) => name.to_string(),
                        None => heading.to_string(),
                    })
                })
                // A README without its heading: the folder says the step.
                .unwrap_or_else(|| folder.to_string());
            (*folder, heading, *what)
        })
        .collect()
}

/// The project `here` is in — the folder of the nearest `.samplekitrc` at or
/// above it — or `here` itself when it holds samples without one: a file
/// discovery would admit, its frontmatter first. Any `.md` offered a git
/// repository's README, or the home folder, as a project.
pub fn project_at(here: &Path) -> Option<PathBuf> {
    if let Some(config) = crate::config::project_config::find(here) {
        return config.parent().map(Path::to_path_buf);
    }
    std::fs::read_dir(here)
        .ok()?
        .flatten()
        .any(|entry| {
            let path = entry.path();
            path.extension().is_some_and(|extension| extension == "md")
                && crate::config::discovery::classify(&path).is_ok()
        })
        .then(|| here.to_path_buf())
}

/// The drives there are, on Windows — `A:\` to `Z:\` — where going up from
/// a drive's root leads; none elsewhere, where `/` is the top.
pub fn drives() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    ('A'..='Z')
        .map(|letter| PathBuf::from(format!("{letter}:\\")))
        .filter(|root| root.is_dir())
        .collect()
}

/// The workbench the page's choice opens: on its configuration, on its setup
/// for a new project, or as it is; `None` when the page was quit. A new
/// project's folder that is one already opens as it is; one inside another
/// project is set up as a project of its own, said — a nested project is
/// allowed.
pub fn opened(outcome: &Outcome) -> Option<Result<super::model::Workbench, String>> {
    let folder = match outcome {
        Outcome::Quit => return None,
        Outcome::Open(folder)
        | Outcome::Step(folder)
        | Outcome::Configure(folder)
        | Outcome::Create(folder)
        | Outcome::Made(folder) => folder,
    };
    let mut workbench = match super::model::Workbench::open(folder) {
        Ok(workbench) => workbench,
        Err(error) => return Some(Err(error)),
    };
    // Quitting it comes back here, and says so.
    workbench.from_start = true;
    match outcome {
        Outcome::Configure(_) => {
            workbench.open_workspace();
            workbench.back_to_start = true;
        }
        // Its own `.samplekitrc`, not one above: `Workbench::open` reads
        // the nearest, and a folder inside a project was said to be one.
        Outcome::Create(_) if folder.join(".samplekitrc").is_file() => {
            workbench.message = format!("{} is a project already", shown(folder));
        }
        Outcome::Create(_) | Outcome::Made(_) => {
            if matches!(outcome, Outcome::Made(_)) {
                workbench.made = Some(folder.clone());
            }
            if !matches!(workbench.screen, super::model::Screen::Setup { .. }) {
                workbench.open_setup();
            }
            if let Some(above) = workbench
                .collection
                .config()
                .map(|config| config.root().to_path_buf())
            {
                workbench.message = format!(
                    "inside the project at {}: this folder is set up as a project of its own",
                    shown(&above)
                );
            }
        }
        _ => {}
    }
    Some(Ok(workbench))
}

/// A folder's lines: itself and its folders, hidden ones left
/// out, projects marked. A link leading back to the folder or above it is
/// left out: `/home/home -> /home` went into `/home/home/home…` without end.
pub fn entries(folder: &Path) -> Vec<Entry> {
    // No folder: the drives, above every drive's root (Windows).
    if folder.as_os_str().is_empty() {
        return drives()
            .into_iter()
            .map(|path| Entry::Folder {
                project: path.join(".samplekitrc").is_file(),
                path,
            })
            .collect();
    }
    let here = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut entries = vec![Entry::This];
    let mut folders: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|read| {
            read.flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .filter(|path| {
                    !path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                })
                .filter(|path| {
                    dunce::canonicalize(path).map_or(true, |target| !here.starts_with(target))
                })
                .collect()
        })
        .unwrap_or_default();
    folders.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    entries.extend(folders.into_iter().map(|path| Entry::Folder {
        project: path.join(".samplekitrc").is_file(),
        path,
    }));
    entries
}

/// Where the demo is written: `demo` in `$SAMPLEKIT_DATA_DIR`, or in the
/// system's own data folder for a program, by `etcetera` as the state folder is
/// chosen — `$XDG_DATA_HOME/samplekit`, `~/.local/share/samplekit` without it,
/// `~/Library/Application Support/samplekit` on macOS, and on Windows
/// `%LOCALAPPDATA%\samplekit`, the machine's own rather than the roaming one.
/// Data, not state: these are files the user edits.
pub fn demo_folder() -> Option<PathBuf> {
    use etcetera::BaseStrategy;
    if let Some(directory) =
        std::env::var_os("SAMPLEKIT_DATA_DIR").filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(directory).join("demo"));
    }
    let strategy = etcetera::base_strategy::choose_native_strategy().ok()?;
    let base = if cfg!(windows) {
        strategy.cache_dir()
    } else {
        strategy.data_dir()
    };
    Some(base.join("samplekit").join("demo"))
}

/// Whether the demo is there: its folder holds anything. An empty one is
/// written into as one missing is.
fn written(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some())
}

/// The demo's environment made, as a project's setup makes one: `.venv/`
/// created in its folder, and samplekit installed in it by pip. What was
/// installed, or what went wrong and the command to run by hand; the steps read
/// without it.
pub fn demo_environment(folder: &Path) -> Result<String, String> {
    crate::config::project_setup::make_environment(
        folder,
        &crate::config::project_setup::Step::Environment { create: true },
    )
}

/// The demo written into `folder`, made if it is not there. One that holds
/// anything is refused before a file is written: the demo never mixes with, or
/// writes over, what is there.
pub fn write_demo(folder: &Path) -> Result<(), String> {
    match std::fs::read_dir(folder) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(format!(
                    "{} is not empty: the demo is written into a new or empty folder",
                    shown(folder)
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", shown(folder))),
    }
    for (name, bytes) in DEMO_FILES {
        let file = name
            .split('/')
            .fold(folder.to_path_buf(), |path, part| path.join(part));
        let written = match file.parent() {
            Some(parent) => std::fs::create_dir_all(parent),
            None => Ok(()),
        }
        .and_then(|()| std::fs::write(&file, bytes));
        if let Err(error) = written {
            return Err(format!("{}: {error}", shown(&file)));
        }
    }
    Ok(())
}

/// A folder as it is said: from the home folder, with `~`.
pub fn shown(path: &Path) -> String {
    if let Ok(home) = etcetera::home_dir()
        && let Ok(inside) = path.strip_prefix(&home)
    {
        if inside.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", inside.display());
    }
    path.display().to_string()
}

/// The name, in figlet's ANSI Shadow.
const LOGO: [&str; 6] = [
    "███████╗ █████╗ ███╗   ███╗██████╗ ██╗     ███████╗██╗  ██╗██╗████████╗",
    "██╔════╝██╔══██╗████╗ ████║██╔══██╗██║     ██╔════╝██║ ██╔╝██║╚══██╔══╝",
    "███████╗███████║██╔████╔██║██████╔╝██║     █████╗  █████╔╝ ██║   ██║",
    "╚════██║██╔══██║██║╚██╔╝██║██╔═══╝ ██║     ██╔══╝  ██╔═██╗ ██║   ██║",
    "███████║██║  ██║██║ ╚═╝ ██║██║     ███████╗███████╗██║  ██╗██║   ██║",
    "╚══════╝╚═╝  ╚═╝╚═╝     ╚═╝╚═╝     ╚══════╝╚══════╝╚═╝  ╚═╝╚═╝   ╚═╝",
];

/// The width the start page is centred in: its left edge where a page this wide
/// begins, the logo and every mode's lines from there.
const PAGE: usize = 100;

/// The page, centred. What it offers is always there to read: on a short
/// terminal the logo gives way to a line, the list scrolls with its cursor,
/// and the keys stay at its foot; a line too long for the width is cut with
/// an ellipsis, a path from its start, so that its name stays.
pub fn draw(frame: &mut Frame, start: &mut Start) {
    let theme = Theme::of(None);
    super::theme::set(theme);
    let area = frame.area();
    // The widest a line may be: the column's two cells of margin aside. The
    // page's left edge is fixed, a page `PAGE` wide centred, whatever the mode
    // shows: centred on its widest line, the logo moved sideways from one mode
    // to the next. A line runs on to the right edge.
    let left = (area.width as usize).saturating_sub(PAGE) / 2;
    let room = (area.width as usize).saturating_sub(left + 2).max(1);
    // Every key in the one style the TUI names keys in.
    let key = super::theme::key_style(&theme);
    let muted = Style::new().fg(theme.muted);
    let marker = |on: bool| if on { "▸ " } else { "  " };
    let on_style = |on: bool| {
        if on {
            Style::new().fg(theme.selected).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        }
    };
    // A folder as the page says it, cut from its start to what is left of
    // the line after `lead` columns.
    let path = |folder: &Path, lead: usize| {
        super::view::ellipsized_start(&shown(folder), room.saturating_sub(lead).max(4))
    };
    // What the page shows above the list, the list — `cursor` its line —
    // what follows it, and the keys.
    let mut above: Vec<Line> = Vec::new();
    let mut list: Vec<Line> = Vec::new();
    let mut cursor = 0;
    // The line kept in sight with the cursor's: the heading just above it.
    let mut top: Option<usize> = None;
    // Which list it is, for where it was scrolled.
    let mut name: &'static str = "other";
    let mut below: Vec<Line> = Vec::new();
    // The help's first line shown, where it is scrolled rather than moved
    // through.
    let mut scrolled: Option<usize> = None;
    // The line typed, if one is: its line above the list, and how far in its
    // editor draws it.
    let mut typed: Option<(usize, u16)> = None;
    let pairs: Vec<(&str, &str)> = match &start.mode {
        Mode::Help { scroll, .. } => {
            above.push(Line::styled(
                "  Help",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            above.push(Line::raw(""));
            scrolled = Some(*scroll);
            for (pressed, what) in HELP {
                // What each does may name another key: `Esc clears it`.
                list.push(if pressed.is_empty() {
                    if what.starts_with(' ') {
                        let mut spans = vec![Span::raw("  ")];
                        spans.extend(keyed(what, muted));
                        Line::from(spans)
                    } else {
                        Line::styled(
                            format!("  {what}"),
                            Style::new().add_modifier(Modifier::BOLD),
                        )
                    }
                } else {
                    let mut spans = vec![Span::styled(format!("  {pressed:<9}"), key)];
                    spans.extend(keyed(what, muted));
                    Line::from(spans)
                });
            }
            vec![("j k ↑ ↓", "scroll"), ("Esc", "back")]
        }
        Mode::Menu => {
            name = "menu";
            let lead = start.lead();
            if let Some(folder) = &start.current {
                let on = start.cursor == 0;
                list.push(Line::styled("  This project", muted));
                if on {
                    cursor = list.len();
                    top = Some(cursor - 1);
                }
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled("↵  ", key),
                    Span::styled(path(folder, 5), on_style(on)),
                ]));
                list.push(Line::raw(""));
            }
            if !start.recent.is_empty() {
                list.push(Line::styled("  Recent projects", muted));
                for (index, folder) in start.recent.iter().enumerate() {
                    let on = start.cursor == lead + index;
                    if on {
                        cursor = list.len();
                        if index == 0 {
                            top = Some(cursor - 1);
                        }
                    }
                    list.push(Line::from(vec![
                        Span::styled(marker(on), on_style(on)),
                        Span::styled(format!("{}  ", index + 1), key),
                        Span::styled(path(folder, 5), on_style(on)),
                    ]));
                }
                list.push(Line::raw(""));
            }
            for (index, (character, name, what)) in ITEMS.iter().enumerate() {
                let on = start.cursor == lead + start.recent.len() + index;
                if on {
                    cursor = list.len();
                }
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled(format!("{character}  "), key),
                    Span::styled(format!("{name:<22}"), on_style(on)),
                    Span::styled(what.to_string(), muted),
                ]));
            }
            vec![
                ("j k ↑ ↓", "move"),
                ("x", "forget a recent project"),
                ("?", "help"),
                ("Esc", "quit"),
            ]
        }
        Mode::Browse {
            purpose,
            folder,
            entries,
            cursor: on_entry,
            narrowing,
        } => {
            name = "browse";
            let asked = match purpose {
                Purpose::Open => "Open a folder",
                Purpose::Create => "New project: where?",
                Purpose::Configure => "Configure a project",
            };
            above.push(Line::styled(
                format!("  {asked}"),
                Style::new().add_modifier(Modifier::BOLD),
            ));
            above.push(Line::styled(
                if folder.as_os_str().is_empty() {
                    "  the drives".to_string()
                } else {
                    format!("  {}", path(folder, 2))
                },
                muted,
            ));
            // The folders the letters typed keep, the closest first.
            let kept: Vec<usize> = match narrowing {
                Some(text) => {
                    typed = Some((above.len(), 4));
                    above.push(Line::from(vec![Span::styled("  / ", key)]));
                    narrowed(entries, text.text())
                }
                None => (0..entries.len()).collect(),
            };
            above.push(Line::raw(""));
            cursor = kept.iter().position(|index| index == on_entry).unwrap_or(0);
            if kept.is_empty() {
                list.push(Line::styled("  no folder holds these letters", muted));
            }
            for (index, entry) in kept.iter().map(|index| (*index, &entries[*index])) {
                let on = index == *on_entry;
                let (name, note) = match entry {
                    Entry::This => (
                        match purpose {
                            Purpose::Open => "‹ open this folder ›".to_string(),
                            Purpose::Create => "‹ set a project up here ›".to_string(),
                            Purpose::Configure => "‹ configure this folder's project ›".to_string(),
                        },
                        "",
                    ),
                    Entry::Folder { path, project } => (
                        match path.file_name() {
                            Some(_) => format!("{}/", folder_name(path)),
                            // A drive's root has no name: its letter.
                            None => folder_name(path),
                        },
                        if *project { "  project" } else { "" },
                    ),
                };
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled(name, on_style(on)),
                    Span::styled(note, Style::new().fg(theme.current)),
                ]));
            }
            match (purpose, narrowing) {
                (_, Some(_)) => vec![
                    ("↑ ↓", "move"),
                    ("Enter", "open a project, or go in"),
                    ("→", "in"),
                    ("Esc", "all the folders"),
                ],
                (Purpose::Create, None) => vec![
                    ("j k ↑ ↓", "move"),
                    ("Enter → l", "in"),
                    ("/", "narrow"),
                    ("+", "a new folder here"),
                    ("← h", "up"),
                    ("~", "home"),
                    ("Esc", "back"),
                ],
                _ => vec![
                    ("j k ↑ ↓", "move"),
                    ("Enter", "open a project, or go in"),
                    ("→ l", "in"),
                    ("/", "narrow"),
                    ("← h", "up"),
                    ("~", "home"),
                    ("Esc", "back"),
                ],
            }
        }
        Mode::Naming { folder, .. } => {
            above.push(Line::styled(
                format!("  A new folder in {}", path(folder, 18)),
                Style::new().add_modifier(Modifier::BOLD),
            ));
            above.push(Line::raw(""));
            typed = Some((above.len(), 8));
            above.push(Line::raw("  name: "));
            vec![
                ("Enter", "make it and set a project up in it"),
                ("Esc", "back"),
            ]
        }
        Mode::Pick { cursor: on_pick } => {
            above.push(Line::styled(
                "  Configure a project",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            above.push(Line::raw(""));
            let projects = start.configurable();
            name = "pick";
            cursor = *on_pick;
            for (index, folder) in projects.iter().enumerate() {
                let on = index == *on_pick;
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled(path(folder, 2), on_style(on)),
                ]));
            }
            let on = *on_pick == projects.len();
            list.push(Line::from(vec![
                Span::styled(marker(on), on_style(on)),
                Span::styled("‹ another, by moving to it ›", on_style(on)),
            ]));
            vec![("j k ↑ ↓", "move"), ("Enter", "choose"), ("Esc", "back")]
        }
        Mode::Guide { cursor: on_guide } => {
            above.push(Line::styled(
                "  Guide and demo",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            above.push(Line::raw(""));
            name = "guide";
            cursor = *on_guide;
            let there = start.demo.as_deref().is_some_and(written);
            for (index, (entry, what)) in [
                ("The guide, from installing to figures", "in the browser"),
                (
                    "The demo, a brewer's notebook in eight steps",
                    if there {
                        "its steps"
                    } else {
                        "written, then its first step"
                    },
                ),
            ]
            .iter()
            .enumerate()
            {
                let on = index == *on_guide;
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled(format!("{entry:<46}"), on_style(on)),
                    Span::styled(what.to_string(), muted),
                ]));
            }
            below.push(Line::raw(""));
            below.push(Line::styled(format!("  the guide: {GUIDE}"), muted));
            if let Some(folder) = &start.demo {
                below.push(Line::styled(
                    format!("  the demo: {}", path(folder, 13)),
                    muted,
                ));
            }
            vec![("j k ↑ ↓", "move"), ("Enter", "open"), ("Esc", "back")]
        }
        Mode::Demo { cursor: on_demo } => {
            name = "demo";
            above.push(Line::styled(
                "  The demo, a brewer's notebook in eight steps",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            if let Some(folder) = &start.demo {
                above.push(Line::styled(format!("  {}", path(folder, 2)), muted));
            }
            above.push(Line::raw(""));
            let steps = demo_steps();
            let width = steps
                .iter()
                .map(|(_, title, _)| title.chars().count())
                .chain(["Reset the demo".len()])
                .max()
                .unwrap_or(0)
                + 2;
            for (index, (_, title, what)) in steps.iter().enumerate() {
                let on = index == *on_demo;
                if on {
                    cursor = list.len();
                }
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::styled(format!("{}  ", index + 1), key),
                    Span::styled(format!("{title:<width$}"), on_style(on)),
                    Span::styled(what.to_string(), muted),
                ]));
            }
            list.push(Line::raw(""));
            for (index, (entry, what)) in [
                ("The tutorial", "these steps' pages, in the browser"),
                ("Reset the demo", "as it came, asked first"),
            ]
            .iter()
            .enumerate()
            {
                let on = STEPS.len() + index == *on_demo;
                if on {
                    cursor = list.len();
                    if index == 0 {
                        top = Some(cursor - 1);
                    }
                }
                list.push(Line::from(vec![
                    Span::styled(marker(on), on_style(on)),
                    Span::raw("   "),
                    Span::styled(format!("{entry:<width$}"), on_style(on)),
                    Span::styled(what.to_string(), muted),
                ]));
            }
            vec![
                ("j k ↑ ↓", "move"),
                ("1…8", "a step"),
                ("Enter", "open"),
                ("Esc", "back"),
            ]
        }
        Mode::Reset => {
            above.push(Line::styled(
                "  Reset the demo?",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            if let Some(folder) = &start.demo {
                above.push(Line::styled(format!("  {}", path(folder, 2)), muted));
            }
            above.push(Line::raw(""));
            for lost in [
                "  every file of it written again as it comes: your changes to it are lost",
                "  anything added to it removed, its history (.samplekit/) with it",
                "  its Python environment (.venv/) made again",
            ] {
                above.push(Line::raw(lost));
            }
            vec![("y", "reset it"), ("Esc", "back")]
        }
    };
    // The keys, those that choose and leave kept to the last.
    let mut footer = vec![Line::raw("")];
    footer.extend(keys_lines(&pairs, room, key, muted));
    if !start.message.is_empty() {
        footer.push(Line::raw(""));
        let mut said = vec![Span::raw("  ")];
        said.extend(keyed(&start.message, Style::new().fg(theme.message)));
        footer.push(Line::from(said));
    }
    // The logo where it leaves the page its room, a line of title where not:
    // at 40×12 it took the whole height.
    let version = format!(
        "  {} · sample data in plain Markdown: computed, current, tracked",
        env!("CARGO_PKG_VERSION")
    );
    let height = area.height as usize;
    let rest = above.len() + list.len() + below.len() + footer.len();
    let logo_fits =
        LOGO.len() + 2 + rest <= height && LOGO.iter().all(|line| line.chars().count() + 2 <= room);
    let mut header: Vec<Line> = if logo_fits {
        let mut header: Vec<Line> = LOGO
            .iter()
            .map(|line| Line::styled(format!("  {line}"), Style::new().fg(theme.accent)))
            .collect();
        header.push(Line::styled(version, muted));
        header
    } else {
        vec![Line::from(vec![
            Span::styled(
                "  SampleKit",
                Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", env!("CARGO_PKG_VERSION")), muted),
        ])]
    };
    if header.len() + 1 + rest <= height {
        header.push(Line::raw(""));
    }
    // The list scrolled to its cursor in the rows left; what follows it given
    // up first, then what comes above it, the keys never.
    let mut spare = height.saturating_sub(header.len() + footer.len());
    if above.len() + below.len() + list.len() > spare {
        below.clear();
    }
    let mut removed = 0;
    while above.len() + list.len().min(1) > spare && !above.is_empty() {
        above.remove(0);
        removed += 1;
    }
    let typed_row =
        typed.and_then(|(row, from)| Some((header.len() + row.checked_sub(removed)?, from)));
    spare = spare.saturating_sub(above.len() + below.len());
    let shown_rows = spare.max(1).min(list.len().max(1));
    let first = match scrolled {
        // Scrolled: from its line, no further than fills the rows — and `j`
        // no further than that, rather than on unseen.
        Some(scroll) => {
            let end = list.len().saturating_sub(shown_rows);
            start.offsets.borrow_mut().insert("help-end", end);
            scroll.min(end)
        }
        // Moved through: the rows move only when the cursor leaves them.
        None => start.window(name, top.unwrap_or(cursor), cursor, shown_rows, list.len()),
    };
    let list: Vec<Line> = list.into_iter().skip(first).take(shown_rows).collect();
    let lines: Vec<Line> = header
        .into_iter()
        .chain(above)
        .chain(list)
        .chain(below)
        .chain(footer)
        .map(|line| cut_line(line, room))
        .collect();
    let height = (lines.len() as u16).min(area.height);
    let column = Rect {
        x: area.x + left as u16,
        width: area.width.saturating_sub(left as u16),
        ..area
    };
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(2),
    ])
    .areas(column);
    frame.render_widget(Paragraph::new(lines), middle);
    // The line typed, drawn by its editor where its line is.
    let typing = match &mut start.mode {
        Mode::Browse {
            narrowing: Some(text),
            ..
        }
        | Mode::Naming { text, .. } => Some(text),
        _ => None,
    };
    if let (Some(text), Some((row, from))) = (typing, typed_row)
        && row < middle.height as usize
        && from < middle.width
    {
        let area = Rect {
            x: middle.x + from,
            y: middle.y + row as u16,
            width: middle.width - from,
            height: 1,
        };
        super::view::draw_typed(frame, area, text, Style::new().fg(theme.selected));
    }
}

/// The keys of a screen, each in the colour the entries' keys have, what it
/// does muted: on a second line where one cannot hold them, broken between
/// whole keys; two still too few, `Enter` and `Esc` kept to the last, the
/// others given up from the end — the key line lost its `Esc` first.
fn keys_lines(pairs: &[(&str, &str)], room: usize, key: Style, muted: Style) -> Vec<Line<'static>> {
    let written: Vec<String> = pairs
        .iter()
        .map(|(pressed, what)| format!("{pressed} {what}"))
        .collect();
    let laid = super::view::wrap_hints(&written.join(" · "), room.saturating_sub(2), 2);
    laid.iter()
        .map(|fitted| {
            let mut spans = vec![Span::raw("  ")];
            for (index, hint) in fitted.split(" · ").enumerate() {
                if index > 0 {
                    spans.push(Span::styled(" · ", muted));
                }
                match pairs
                    .iter()
                    .find(|(pressed, what)| hint == format!("{pressed} {what}"))
                {
                    Some((pressed, what)) => {
                        spans.push(Span::styled(pressed.to_string(), key));
                        spans.push(Span::styled(format!(" {what}"), muted));
                    }
                    // One cut to the width, an ellipsis at its end.
                    None => spans.push(Span::styled(hint.to_string(), muted)),
                }
            }
            Line::from(spans)
        })
        .collect()
}

/// A line cut to `room` columns, an ellipsis where it was cut: the recent
/// projects and the entries' descriptions ran off the right edge unsaid.
fn cut_line(line: Line<'static>, room: usize) -> Line<'static> {
    if line.width() <= room {
        return line;
    }
    let mut left = room.saturating_sub(1);
    let mut spans = Vec::new();
    for span in line.spans {
        if left == 0 {
            break;
        }
        let width = span.width();
        if width <= left {
            left -= width;
            spans.push(span);
        } else {
            let cut = super::view::ellipsized(&span.content, left + 1);
            let cut = cut.trim_end_matches('…').to_string();
            spans.push(Span::styled(cut, span.style));
            left = 0;
        }
    }
    let style = spans.last().map(|span| span.style).unwrap_or_default();
    spans.push(Span::styled("…", style));
    Line::from(spans)
}
