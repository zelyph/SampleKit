//! What the workbench's tests share: a scratch collection, keys typed, a
//! screen drawn off-screen. Not a test crate of its own.

#![allow(dead_code, unused_imports)]

pub use std::fs;
pub use std::path::PathBuf;

pub use ratatui::Terminal;
pub use ratatui::backend::TestBackend;
pub use samplekit::tui::model::{Effect, Key, Mode, Screen, Workbench};

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-workbench-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        // The workbench remembers each directory in a file of its own, so one
        // state directory serves every test: set once, since the tests run
        // together and a variable set per test raced.
        static STATE: std::sync::Once = std::sync::Once::new();
        STATE.call_once(|| {
            let state = dunce::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("samplekit-workbench-state-{}", std::process::id()));
            unsafe { std::env::set_var("SAMPLEKIT_STATE_DIR", &state) };
        });
        fs::write(
            path.join(".samplekitrc"),
            "schema_version = 1\n[collection]\nfiles = [\"images\"]\n\
             [property.malt]\nprecision = \".1f\"\n",
        )
        .unwrap();
        for (name, malt, beer) in [
            ("C1", 12.0, "schwarz"),
            ("C2", 10.0, "bock"),
            ("C3", 11.0, "schwarz"),
        ] {
            fs::write(
                path.join(format!("{name}.md")),
                format!(
                    "---\nschema_version: 1\nname: {name}\nbeer: {beer}\nproperties:\n  \
                     malt: {{v: {malt:?}, u: 0.1, unit: g}}\n---\nA note on {name}.\n"
                ),
            )
            .unwrap();
        }
        fs::create_dir_all(path.join("images")).unwrap();
        fs::write(path.join("images/C1_sem.png"), "").unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn keys(workbench: &mut Workbench, written: &str) -> Vec<Effect> {
    written
        .chars()
        .map(|character| {
            workbench.key(match character {
                '\n' => Key::Enter,
                '\u{1b}' => Key::Esc,
                '\t' => Key::Tab,
                other => Key::Char(other),
            })
        })
        .collect()
}

pub fn screen(workbench: &mut Workbench) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The scratch, with more in it: a saved query, a sample in a directory below,
/// and one holding readings and a table.
pub fn rich(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\nfiles = [\"images\"]\n\
         [property.malt]\nprecision = \".1f\"\n\
         [query.schwarz]\nfilter = 'beer == \"schwarz\"'\n",
    )
    .unwrap();
    fs::create_dir_all(scratch.0.join("more")).unwrap();
    fs::write(
        scratch.0.join("more/D1.md"),
        "---\nschema_version: 1\nname: D1\nbeer: bock\nproperties:\n  \
         malt: {v: 9.0, readings: [8.0, 10.0], unit: g}\n\
         tables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      \
         load: {unit: N}\n    rows:\n      - run: 1\n        load: 3.5\n      \
         - run: 2\n        load: 4.25\n---\nFirst line.\nSecond line.\n",
    )
    .unwrap();
    scratch
}

pub fn open_named(workbench: &mut Workbench, name: &str) {
    let at = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some(name))
        .unwrap();
    workbench.cursor = at;
    keys(workbench, "\n");
}

pub fn move_to(workbench: &mut Workbench, field: &str) {
    let at = workbench
        .entries()
        .iter()
        .position(|entry| entry.field == field)
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
}

/// The modal's frame on a drawn screen: its top row, bottom row, left and
/// right columns, found by its corners.
pub fn modal_frame(workbench: &mut Workbench, title: &str) -> (u16, u16, u16, u16) {
    let (width, height) = (100u16, 30u16);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |y: u16| -> String { (0..width).map(|x| buffer[(x, y)].symbol()).collect() };
    let top = (0..height)
        .find(|y| row(*y).contains(&format!(" {title} ")))
        .expect("the modal is drawn");
    // The corner nearest before the title: another frame may share the row.
    let at = row(top)
        .chars()
        .collect::<Vec<_>>()
        .windows(title.chars().count())
        .position(|window| window.iter().collect::<String>() == title)
        .unwrap() as u16;
    let left = (0..at)
        .rev()
        .find(|x| buffer[(*x, top)].symbol() == "┌")
        .unwrap();
    let right = (left + 1..width)
        .find(|x| buffer[(*x, top)].symbol() == "┐")
        .unwrap();
    let bottom = (top + 1..height)
        .find(|y| buffer[(left, *y)].symbol() == "└")
        .unwrap();
    (top, bottom, left, right)
}

/// A screen drawn at a width, its lines.
pub fn drawn_at(workbench: &mut Workbench, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..16)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A key held with others, as the runtime hands it to what is typed :
/// `ctrl('z')`, `held(KeyCode::Left, KeyModifiers::SHIFT)`.
pub fn held(code: crossterm::event::KeyCode, modifiers: crossterm::event::KeyModifiers) -> Key {
    Key::Chord(crossterm::event::KeyEvent::new(code, modifiers))
}

pub fn ctrl(character: char) -> Key {
    held(
        crossterm::event::KeyCode::Char(character),
        crossterm::event::KeyModifiers::CONTROL,
    )
}

pub fn alt(character: char) -> Key {
    held(
        crossterm::event::KeyCode::Char(character),
        crossterm::event::KeyModifiers::ALT,
    )
}

/// The left button of the mouse dragged to, or let go at, a cell.
pub fn pointer(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> Key {
    Key::Pointer(crossterm::event::MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    })
}
