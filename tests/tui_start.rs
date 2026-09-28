//! The start page's keys, on a model that draws nothing.

mod workbench_fixtures;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use samplekit::tui::start::{DEMO_FILES, Effect, GUIDE, Mode, Outcome, Start};
use workbench_fixtures::{Key, Scratch, Terminal, TestBackend, Workbench};

/// A folder of folders: `project/` holding a `.samplekitrc`, `plain/` not.
fn folders(name: &str) -> PathBuf {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-start-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("plain")).unwrap();
    fs::create_dir_all(root.join("project")).unwrap();
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join("project/.samplekitrc"), "schema_version = 1\n").unwrap();
    dunce::canonicalize(root).unwrap()
}

fn press(start: &mut Start, typed: &str) -> Effect {
    let mut effect = Effect::None;
    for character in typed.chars() {
        effect = start.key(match character {
            '\n' => Key::Enter,
            '\u{1b}' => Key::Esc,
            other => Key::Char(other),
        });
    }
    effect
}

#[test]
fn a_recent_project_opens_by_its_digit_or_enter() {
    let root = folders("recent");
    let recent = vec![root.join("project"), root.join("plain")];
    let mut start = Start::with_recent(&root, recent.clone());
    assert_eq!(
        press(&mut start, "2"),
        Effect::Done(Outcome::Open(recent[1].clone()))
    );
    let mut start = Start::with_recent(&root, recent.clone());
    assert_eq!(
        press(&mut start, "\n"),
        Effect::Done(Outcome::Open(recent[0].clone()))
    );
    // A digit past the list does nothing.
    assert_eq!(press(&mut start, "7"), Effect::None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_workbench_keeps_its_folders_whole_for_the_start_page() {
    let scratch = Scratch::new("recent-kept");
    let workbench = Workbench::open(&scratch.0).unwrap();
    samplekit::tui::model::remember(&workbench);
    let recent = samplekit::tui::model::recent_projects();
    assert!(
        recent.contains(&dunce::canonicalize(&scratch.0).unwrap()),
        "{recent:?}"
    );
}

#[test]
fn a_folder_is_opened_by_moving_to_it() {
    let root = folders("open");
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "o");
    let Mode::Browse {
        entries, cursor, ..
    } = &start.mode
    else {
        panic!("browsing");
    };
    // Itself, then its folders, hidden ones left out; no `..`.
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(*cursor, 1);
    // plain/ is gone into; back up, then project/ is opened by Enter.
    press(&mut start, "\n");
    let Mode::Browse { folder, .. } = &start.mode else {
        panic!("browsing");
    };
    assert_eq!(folder, &root.join("plain"));
    press(&mut start, "h");
    assert_eq!(
        press(&mut start, "j\n"),
        Effect::Done(Outcome::Open(root.join("project")))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_new_project_is_a_folder_made_then_set_up() {
    let root = folders("new");
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "n+");
    assert!(matches!(start.mode, Mode::Naming { .. }));
    // A name with a slash is refused, and the name kept to correct.
    press(&mut start, "a/b\n");
    assert!(
        matches!(start.mode, Mode::Naming { .. }),
        "{}",
        start.message
    );
    for _ in 0..3 {
        start.key(Key::Backspace);
    }
    assert_eq!(
        press(&mut start, "brews\n"),
        Effect::Done(Outcome::Made(root.join("brews")))
    );
    assert!(root.join("brews").is_dir());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn only_a_project_is_configured() {
    let root = folders("configure");
    let mut start = Start::with_recent(&root, vec![root.join("plain"), root.join("project")]);
    // The recent projects that are projects, then another by moving.
    press(&mut start, "c");
    assert_eq!(start.configurable(), [root.join("project")]);
    assert_eq!(
        press(&mut start, "\n"),
        Effect::Done(Outcome::Configure(root.join("project")))
    );
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "c");
    // The folder itself is no project: said, and the page stays.
    assert_eq!(press(&mut start, "k\n"), Effect::None);
    assert!(start.message.contains("not a project"), "{}", start.message);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_guide_opens_in_the_browser() {
    let root = folders("guide");
    let mut start = Start::with_recent(&root, Vec::new());
    assert_eq!(press(&mut start, "g\n"), Effect::Browse(GUIDE));
    press(&mut start, "\u{1b}");
    assert!(matches!(start.mode, Mode::Menu));
    assert_eq!(press(&mut start, "q"), Effect::Done(Outcome::Quit));
    let _ = fs::remove_dir_all(root);
}

/// The files under `folder`, by their `/`-separated path: what running the
/// demo's steps leaves beside them aside, as `build.rs` leaves it.
fn files_under(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, folder: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if [".samplekit", "__pycache__", ".venv", "out"].contains(&name.as_str())
                || name.ends_with(".pyc")
            {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let parts: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.insert(parts.join("/"), fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(folder, folder, &mut found);
    found
}

/// A stand-in for the demo's environment: no Python, no pip, a line in
/// `.venv/made` for each time it was made.
fn fake_environment(folder: &Path) -> Result<String, String> {
    use std::io::Write;
    fs::create_dir_all(folder.join(".venv")).unwrap();
    let mut made = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(".venv/made"))
        .unwrap();
    writeln!(made, "made").unwrap();
    Ok("the fake environment is made".to_string())
}

/// A start page whose demo is in `data`, made by `fake_environment`.
fn demo_page(root: &Path, data: &Path) -> Start {
    let mut start = Start::with_recent(root, Vec::new());
    start.demo = Some(data.join("demo"));
    start.environment = fake_environment;
    start
}

/// The TUI the page opened on the demo, its environment's install waited for.
fn opened_on_demo(start: &mut Start, outcome: &Outcome) -> Workbench {
    let mut workbench = samplekit::tui::start::opened(outcome).unwrap().unwrap();
    start.hand_over(&mut workbench);
    assert!(
        workbench.message.contains("installing samplekit"),
        "{}",
        workbench.message
    );
    for _ in 0..500 {
        if workbench.installing.is_none() {
            break;
        }
        workbench.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(workbench.installing.is_none());
    workbench
}

#[test]
fn the_demo_is_written_the_first_time_then_opened() {
    // The machine's data folder by default, `SAMPLEKIT_DATA_DIR` first.
    if cfg!(target_os = "linux")
        && std::env::var_os("SAMPLEKIT_DATA_DIR").is_none()
        && std::env::var_os("XDG_DATA_HOME").is_none()
    {
        let default = samplekit::tui::start::demo_folder().unwrap();
        assert!(
            default.ends_with(".local/share/samplekit/demo"),
            "{default:?}"
        );
    }
    let _state = Scratch::new("demo-first-state");
    let root = folders("demo-first");
    let data = root.join("data");
    let demo = data.join("demo");
    let mut start = demo_page(&root, &data);
    let outcome = press(&mut start, "gj\n");
    let first = demo.join("01-first-brews");
    assert_eq!(outcome, Effect::Done(Outcome::Step(first.clone())));
    // Every file of the demo in the repository, byte for byte: the one the
    // binary embeds, and the one written.
    let repository = files_under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/brewing"));
    let embedded: BTreeMap<String, Vec<u8>> = DEMO_FILES
        .iter()
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .collect();
    assert_eq!(
        embedded.keys().collect::<Vec<_>>(),
        repository.keys().collect::<Vec<_>>()
    );
    assert!(embedded == repository);
    assert!(files_under(&demo) == repository);
    assert!(repository.contains_key("01-first-brews/.samplekitrc"));
    // The first step opened, the environment made beside it and said.
    let Effect::Done(outcome) = outcome else {
        unreachable!()
    };
    let workbench = opened_on_demo(&mut start, &outcome);
    assert_eq!(workbench.message, "the fake environment is made");
    assert_eq!(
        fs::read_to_string(demo.join(".venv/made")).unwrap(),
        "made\n"
    );
    // A step written reads, its brews computed.
    let step = Workbench::open(&demo.join("05-fermentation")).unwrap();
    assert_eq!(step.collection.len(), 12);
    let stout = fs::read_to_string(demo.join("05-fermentation/brews/dry-stout.md")).unwrap();
    assert!(stout.contains("abv:"), "{stout}");
    // A second visit shows the demo's page as it is, the first step marked.
    let brew = demo.join("01-first-brews/brews/citra-ipa.md");
    fs::write(&brew, "edited\n").unwrap();
    let mut start = demo_page(&root, &data);
    assert_eq!(press(&mut start, "gj\n"), Effect::None);
    assert_eq!(start.mode, Mode::Demo { cursor: 0 });
    assert!(start.installing.is_none());
    assert_eq!(fs::read_to_string(&brew).unwrap(), "edited\n");
    assert_eq!(
        fs::read_to_string(demo.join(".venv/made")).unwrap(),
        "made\n"
    );
    assert_eq!(press(&mut start, "\n"), Effect::Done(Outcome::Step(first)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_demo_is_a_page_of_its_steps() {
    // Opening the demo went into its first step, from which
    // nothing led to the others, and quitting it went to the page's root.
    let _state = Scratch::new("demo-page-state");
    let root = folders("demo-steps");
    let data = root.join("data");
    let demo = data.join("demo");
    let mut start = demo_page(&root, &data);
    let Effect::Done(outcome) = press(&mut start, "gj\n") else {
        panic!("written: {}", start.message);
    };
    opened_on_demo(&mut start, &outcome);
    // Quitting the first step, written and opened at once, comes back to
    // the demo's page.
    let mut start = demo_page(&root, &data);
    start.returning(&outcome);
    assert_eq!(start.mode, Mode::Demo { cursor: 0 });
    // Every step of the demo is on the page, named by its README's heading
    // and said in a line, and the tutorial beside them.
    let steps = samplekit::tui::start::demo_steps();
    let folders_embedded: std::collections::BTreeSet<&str> = DEMO_FILES
        .iter()
        .filter_map(|(name, _)| name.split_once('/').map(|(folder, _)| folder))
        .collect();
    assert_eq!(
        folders_embedded.into_iter().collect::<Vec<_>>(),
        steps.iter().map(|(folder, ..)| *folder).collect::<Vec<_>>()
    );
    assert_eq!(steps[0].1, "First brews");
    assert_eq!(steps[4].1, "Fermentation");
    let drawn = page_at(&mut start, 100, 30).join("\n");
    for (at, (_, title, what)) in steps.iter().enumerate() {
        assert!(
            drawn.contains(&format!("{}  {title}", at + 1)),
            "{title}\n{drawn}"
        );
        assert!(drawn.contains(what), "{what}\n{drawn}");
    }
    for said in ["The tutorial", "Reset the demo", "1…8 a step", "Esc back"] {
        assert!(drawn.contains(said), "{said}\n{drawn}");
    }
    // Enter on step 5 opens it, and so does its digit.
    let fifth = demo.join("05-fermentation");
    assert_eq!(
        press(&mut start, "jjjj\n"),
        Effect::Done(Outcome::Step(fifth.clone()))
    );
    let mut start = demo_page(&root, &data);
    press(&mut start, "gj\n");
    assert_eq!(
        press(&mut start, "5"),
        Effect::Done(Outcome::Step(fifth.clone()))
    );
    let workbench = samplekit::tui::start::opened(&Outcome::Step(fifth.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(workbench.collection.len(), 12);
    // Quitting it comes back to the page, step 5 marked; Esc back to the
    // start page, whose `q` quits.
    let mut start = demo_page(&root, &data);
    start.returning(&Outcome::Step(fifth));
    assert_eq!(start.mode, Mode::Demo { cursor: 4 });
    assert!(
        page_at(&mut start, 100, 30)
            .join("\n")
            .contains("▸ 5  Fermentation")
    );
    press(&mut start, "\u{1b}");
    assert_eq!(start.mode, Mode::Menu);
    // A project opened as any other comes back to the page's root.
    let mut start = demo_page(&root, &data);
    start.returning(&Outcome::Open(root.join("project")));
    assert_eq!(start.mode, Mode::Menu);
    // The tutorial, in the browser.
    let mut start = demo_page(&root, &data);
    assert_eq!(
        press(&mut start, "gj\njjjjjjjj\n"),
        Effect::Browse(samplekit::tui::start::TUTORIAL)
    );
    assert_eq!(press(&mut start, "q"), Effect::None);
    assert_eq!(start.mode, Mode::Menu);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resetting_the_demo_asks_then_writes_it_again() {
    let _state = Scratch::new("demo-reset-state");
    let root = folders("demo-reset");
    let data = root.join("data");
    let demo = data.join("demo");
    // Never written, there is nothing to lose: written without asking.
    let mut start = demo_page(&root, &data);
    let Effect::Done(outcome) = press(&mut start, "gj\n") else {
        panic!("written: {:?} {}", start.mode, start.message);
    };
    opened_on_demo(&mut start, &outcome);
    // A file added, a brew edited.
    let added = demo.join("04-computing/notes.txt");
    let brew = demo.join("04-computing/brews/dry-stout.md");
    let original = fs::read(&brew).unwrap();
    fs::write(&added, "mine\n").unwrap();
    fs::write(&brew, "edited\n").unwrap();
    // Asked first, from the demo's page, saying what is lost; nothing
    // changes on another key.
    let mut start = demo_page(&root, &data);
    assert_eq!(press(&mut start, "gj\njjjjjjjjj\n"), Effect::None);
    assert!(matches!(start.mode, Mode::Reset), "{:?}", start.mode);
    let drawn = page_at(&mut start, 100, 30).join("\n");
    for said in [
        "Reset the demo?",
        "changes to it are lost",
        ".venv/",
        "y reset it",
    ] {
        assert!(drawn.contains(said), "{said}\n{drawn}");
    }
    assert_eq!(press(&mut start, "n"), Effect::None);
    assert_eq!(start.mode, Mode::Demo { cursor: 9 });
    assert!(added.exists());
    // `y`: written again as it comes, and its environment made anew.
    press(&mut start, "\n");
    let Effect::Done(outcome) = press(&mut start, "y") else {
        panic!("reset: {}", start.message);
    };
    assert_eq!(outcome, Outcome::Step(demo.join("01-first-brews")));
    assert!(!added.exists());
    assert_eq!(fs::read(&brew).unwrap(), original);
    let workbench = opened_on_demo(&mut start, &outcome);
    assert_eq!(workbench.message, "the fake environment is made");
    assert_eq!(
        fs::read_to_string(demo.join(".venv/made")).unwrap(),
        "made\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_demo_is_never_written_over_a_folder_that_holds_anything() {
    let root = folders("demo-full");
    let full = root.join("full");
    fs::create_dir_all(&full).unwrap();
    fs::write(full.join("mine.md"), "mine\n").unwrap();
    let said = samplekit::tui::start::write_demo(&full).unwrap_err();
    assert!(said.contains("not empty"), "{said}");
    assert_eq!(fs::read_dir(&full).unwrap().count(), 1);
    let empty = root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    samplekit::tui::start::write_demo(&empty).unwrap();
    assert_eq!(files_under(&empty).len(), DEMO_FILES.len());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_page_draws_its_entries_in_any_mode() {
    let root = folders("draw");
    let mut start = Start::with_recent(&root, vec![root.join("project")]);
    for typed in ["", "o", "\u{1b}n+", "\u{1b}\u{1b}c", "\u{1b}g"] {
        press(&mut start, typed);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| samplekit::tui::start::draw(frame, &mut start))
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        let expected = match typed {
            "" => "New project",
            "o" => "open this folder",
            "\u{1b}n+" => "A new folder in",
            "\u{1b}\u{1b}c" => "another, by moving to it",
            _ => "The demo",
        };
        assert!(screen.contains(expected), "{typed:?}: {screen}");
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_project_here_is_offered_first() {
    // Started in a project, Enter opens it; the recent projects keep their
    // digits, the one here not listed twice.
    let root = folders("here");
    let inside = root.join("project/samples");
    fs::create_dir_all(&inside).unwrap();
    let recent = vec![root.join("plain"), root.join("project")];
    let mut start = Start::with_recent(&inside, recent.clone());
    assert_eq!(start.current, Some(root.join("project")));
    assert_eq!(start.recent, [root.join("plain")]);
    assert_eq!(start.configurable()[0], root.join("project"));
    assert_eq!(
        press(&mut start, "\n"),
        Effect::Done(Outcome::Open(root.join("project")))
    );
    assert_eq!(
        press(&mut start, "1"),
        Effect::Done(Outcome::Open(root.join("plain")))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_configuration_opened_from_the_page_goes_back_to_it() {
    let scratch = Scratch::new("configure-back");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.open_workspace();
    workbench.back_to_start = true;
    // Changes not written keep it there, said.
    workbench.workspace.as_mut().unwrap().dirty = true;
    assert_eq!(workbench.key(Key::Esc), samplekit::tui::model::Effect::None);
    assert!(
        workbench.message.contains("r lets them go"),
        "{}",
        workbench.message
    );
    workbench.workspace.as_mut().unwrap().dirty = false;
    assert_eq!(workbench.key(Key::Esc), samplekit::tui::model::Effect::Quit);
}

#[test]
fn x_forgets_a_recent_project() {
    // Its state removed, the folder itself untouched.
    let scratch = Scratch::new("recent-forgotten");
    let workbench = Workbench::open(&scratch.0).unwrap();
    samplekit::tui::model::remember(&workbench);
    let folder = dunce::canonicalize(&scratch.0).unwrap();
    let mut start = Start::with_recent(
        &dunce::canonicalize(std::env::temp_dir()).unwrap(),
        vec![folder.clone()],
    );
    // On an item, x does nothing.
    press(&mut start, "jx");
    assert_eq!(start.recent.len(), 1);
    press(&mut start, "kx");
    assert!(start.recent.is_empty());
    assert!(start.message.contains("no longer"), "{}", start.message);
    assert!(!samplekit::tui::model::recent_projects().contains(&folder));
    assert!(folder.join(".samplekitrc").is_file());
}

#[test]
#[cfg(unix)]
fn a_link_back_up_is_not_gone_into() {
    // `/home/home -> /home` grew `/home/home/home…` as it was gone into.
    let root = folders("loop");
    std::os::unix::fs::symlink(&root, root.join("again")).unwrap();
    std::os::unix::fs::symlink(root.join("plain"), root.join("elsewhere")).unwrap();
    let names: Vec<String> = samplekit::tui::start::entries(&root)
        .iter()
        .filter_map(|entry| match entry {
            samplekit::tui::start::Entry::Folder { path, .. } => {
                Some(path.file_name().unwrap().to_string_lossy().into_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(names, ["elsewhere", "plain", "project"]);
    // A link elsewhere leads where it points.
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "o\n");
    let Mode::Browse { folder, .. } = &start.mode else {
        panic!("browsing");
    };
    assert_eq!(folder, &root.join("plain"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_new_project_inside_another_is_set_up_as_its_own() {
    // `Workbench::open` reads the nearest .samplekitrc: a folder inside a
    // project was said to be one already, and never set up.
    use samplekit::tui::model::Screen;
    use samplekit::tui::start::opened;
    let root = folders("nested");
    let inner = root.join("project/inner");
    fs::create_dir_all(&inner).unwrap();
    let workbench = opened(&Outcome::Create(inner)).unwrap().unwrap();
    assert!(matches!(workbench.screen, Screen::Setup { .. }));
    assert!(
        workbench.message.contains("inside the project at"),
        "{}",
        workbench.message
    );
    assert!(workbench.from_start);
    // Its own .samplekitrc: a project already, opened as it is.
    let workbench = opened(&Outcome::Create(root.join("project")))
        .unwrap()
        .unwrap();
    assert!(!matches!(workbench.screen, Screen::Setup { .. }));
    assert!(
        workbench.message.contains("is a project already"),
        "{}",
        workbench.message
    );
    assert!(opened(&Outcome::Quit).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_folder_of_notes_is_no_project() {
    // A git repository's README.md, the home folder's notes: offered as a
    // project where no sample was.
    use samplekit::tui::start::project_at;
    let root = folders("notes");
    let plain = root.join("plain");
    fs::write(plain.join("README.md"), "# A repository\n").unwrap();
    assert_eq!(project_at(&plain), None);
    fs::write(
        plain.join("S1.md"),
        "---\nschema_version: 1\nname: S1\n---\n",
    )
    .unwrap();
    assert_eq!(project_at(&plain), Some(plain.clone()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn going_up_from_the_top_offers_the_drives_or_stays() {
    // Windows' `\\?\C:\` stopped going up; its drives are offered above a
    // drive's root. Elsewhere `/` is the top, and `~` goes home from it.
    let top = if cfg!(windows) {
        std::path::PathBuf::from("C:\\")
    } else {
        std::path::PathBuf::from("/")
    };
    let mut start = Start::with_recent(&top, Vec::new());
    press(&mut start, "o");
    let Mode::Browse { folder, .. } = &start.mode else {
        panic!("browsing");
    };
    assert_eq!(folder, &top);
    assert!(!folder.display().to_string().starts_with(r"\\?\"));
    press(&mut start, "h");
    let Mode::Browse {
        folder, entries, ..
    } = &start.mode
    else {
        panic!("browsing");
    };
    if cfg!(windows) {
        assert!(folder.as_os_str().is_empty());
        assert!(!entries.is_empty(), "the drives");
    } else {
        assert_eq!(folder, &top);
    }
    press(&mut start, "~");
    let Mode::Browse { folder, .. } = &start.mode else {
        panic!("browsing");
    };
    assert_ne!(folder, &top);
}

/// The page drawn at a size, its lines.
fn page_at(start: &mut Start, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::start::draw(frame, start))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn ctrl_c_on_the_page_is_an_interrupt() {
    // Ctrl+C exits 130; on the start page it ended as a quit, 0.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use samplekit::tui::run::{Ended, StartStep, start_event};
    let root = folders("interrupt");
    let mut start = Start::with_recent(&root, Vec::new());
    let pressed = |code, modifiers| Event::Key(KeyEvent::new(code, modifiers));
    assert_eq!(
        start_event(
            &mut start,
            &pressed(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        StartStep::Ended(Ended::Interrupted)
    );
    assert_eq!(
        start_event(&mut start, &pressed(KeyCode::Char('q'), KeyModifiers::NONE)),
        StartStep::Ended(Ended::Quit)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_short_or_narrow_terminal_keeps_the_entries_and_the_keys() {
    // At 80×24 with a project here and nine recent ones, c, g, q and the
    // keys fell off; at 40×12 the logo took the whole height.
    let root = folders("short");
    let inside = root.join("project");
    let recent: Vec<PathBuf> = (1..=9)
        .map(|at| {
            let folder = root.join(format!(
                "a-recent-project-with-a-long-enough-name-to-be-cut-{at}"
            ));
            fs::create_dir_all(&folder).unwrap();
            folder
        })
        .collect();
    let mut start = Start::with_recent(&inside, recent);
    let lines = page_at(&mut start, 80, 24);
    let drawn = lines.join("\n");
    for wanted in ["Configure a project", "Guide and demo", "Quit", "Esc quit"] {
        assert!(drawn.contains(wanted), "{wanted}\n{drawn}");
    }
    // A path too long is cut from its start, keeping its end, and says so.
    assert!(drawn.contains("…"), "{drawn}");
    assert!(drawn.contains("cut-9"), "{drawn}");
    // At 40×12: no logo, and the cursor's line in sight wherever it goes.
    for _ in 0..14 {
        press(&mut start, "j");
        let lines = page_at(&mut start, 40, 12);
        let drawn = lines.join("\n");
        assert!(!drawn.contains("███"), "{drawn}");
        assert!(drawn.contains('▸'), "{drawn}");
        assert!(drawn.contains("Esc quit"), "{drawn}");
    }
    // Every mode keeps its entries and its keys at 40×12.
    for (typed, wanted) in [
        ("\u{1b}o", "open this folder"),
        ("\u{1b}\u{1b}n+", "name:"),
        ("\u{1b}\u{1b}\u{1b}c", "another, by moving"),
        ("\u{1b}\u{1b}g", "The demo"),
    ] {
        let mut start = Start::with_recent(&inside, Vec::new());
        press(&mut start, typed.trim_start_matches('\u{1b}'));
        let drawn = page_at(&mut start, 40, 12).join("\n");
        assert!(drawn.contains(wanted), "{typed:?}: {wanted}\n{drawn}");
        assert!(drawn.contains("Esc back"), "{typed:?}\n{drawn}");
    }
    let _ = fs::remove_dir_all(root);
}

/// The rows of a page drawn between its path and its keys: the folders
/// shown, `▸` marking the cursor's.
fn rows_shown(start: &mut Start, width: u16, height: u16) -> Vec<String> {
    page_at(start, width, height)
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| line.starts_with("f") || line.starts_with("▸"))
        .collect()
}

#[test]
fn the_rows_move_only_when_the_cursor_leaves_them() {
    // The browser held its cursor at the foot, the rows below
    // it never shown.
    let root = folders("scroll");
    let many = root.join("many");
    for at in 0..20 {
        fs::create_dir_all(many.join(format!("f{at:02}"))).unwrap();
    }
    let mut start = Start::with_recent(&many, Vec::new());
    press(&mut start, "o");
    // At 60×14: a line of title, the purpose, the path, the list, the keys.
    let rows = rows_shown(&mut start, 60, 14);
    assert!(rows.len() >= 4, "{rows:?}");
    assert_eq!(rows[0], "▸ f00/", "{rows:?}");
    // Rows below the cursor from the first.
    assert!(rows.len() > 1 && rows[1] == "f01/", "{rows:?}");
    let window = rows.len();
    // Down past the foot: the rows move with it.
    for _ in 0..window + 3 {
        press(&mut start, "j");
        rows_shown(&mut start, 60, 14);
    }
    let rows = rows_shown(&mut start, 60, 14);
    let cursor = |rows: &[String]| rows.iter().position(|row| row.starts_with('▸')).unwrap();
    assert_eq!(cursor(&rows), rows.len() - 1, "{rows:?}");
    let bottom = rows.clone();
    // Back up: the cursor moves, not the rows, and those below it are seen.
    press(&mut start, "kk");
    let rows = rows_shown(&mut start, 60, 14);
    assert_eq!(
        rows.iter()
            .map(|row| row.trim_start_matches("▸ "))
            .collect::<Vec<_>>(),
        bottom
            .iter()
            .map(|row| row.trim_start_matches("▸ "))
            .collect::<Vec<_>>()
    );
    assert_eq!(cursor(&rows), rows.len() - 3, "{rows:?}");
    // On up past the top: the rows move back, the cursor at their top.
    for _ in 0..window + 1 {
        press(&mut start, "k");
        rows_shown(&mut start, 60, 14);
    }
    let rows = rows_shown(&mut start, 60, 14);
    assert_eq!(rows[0], "▸ f00/", "{rows:?}");
    // Every list of the page does so: the menu, its recent projects many.
    let recent: Vec<PathBuf> = (0..9).map(|at| many.join(format!("f{at:02}"))).collect();
    let mut start = Start::with_recent(&root, recent);
    for _ in 0..12 {
        press(&mut start, "j");
        page_at(&mut start, 60, 12);
    }
    press(&mut start, "k");
    let drawn = page_at(&mut start, 60, 12);
    let marked = drawn.iter().position(|line| line.contains('▸')).unwrap();
    assert!(
        drawn[marked + 1].contains("Guide and demo"),
        "{}",
        drawn.join("\n")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_folders_are_narrowed_by_typing() {
    // The owner: a folder among many is found by typing, as in the pickers.
    let root = folders("narrow");
    for name in ["measurements", "manuscript", "beers", "notes"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "o");
    assert!(
        page_at(&mut start, 100, 30).join("\n").contains("/ narrow"),
        "the keys say it"
    );
    // Letters in order, not side by side: `br` keeps beers alone.
    press(&mut start, "/br");
    let drawn = page_at(&mut start, 100, 30).join("\n");
    assert!(drawn.contains("/ br"), "{drawn}");
    assert!(drawn.contains("▸ beers/"), "{drawn}");
    for gone in ["measurements/", "notes/", "plain/", "open this folder"] {
        assert!(!drawn.contains(gone), "{gone}\n{drawn}");
    }
    assert!(drawn.contains("Esc all the folders"), "{drawn}");
    // Esc clears the narrowing, the cursor kept on its folder.
    press(&mut start, "\u{1b}");
    let Mode::Browse {
        entries,
        cursor,
        narrowing,
        ..
    } = &start.mode
    else {
        panic!("browsing: {:?}", start.mode);
    };
    assert_eq!(narrowing, &None);
    assert!(
        matches!(&entries[*cursor], samplekit::tui::start::Entry::Folder { path, .. } if *path == root.join("beers")),
        "{entries:?}"
    );
    let drawn = page_at(&mut start, 100, 30).join("\n");
    assert!(
        drawn.contains("notes/") && drawn.contains("plain/"),
        "{drawn}"
    );
    // `m`, then down among what it keeps; Enter goes into the one marked,
    // the narrowing cleared with the folder.
    press(&mut start, "/ma");
    start.key(Key::Down);
    let Mode::Browse {
        entries, cursor, ..
    } = &start.mode
    else {
        panic!("browsing");
    };
    let marked = entries[*cursor].clone();
    press(&mut start, "\n");
    let Mode::Browse {
        folder, narrowing, ..
    } = &start.mode
    else {
        panic!("browsing: {:?}", start.mode);
    };
    assert!(
        matches!(&marked, samplekit::tui::start::Entry::Folder { path, .. } if path == folder),
        "{marked:?} {folder:?}"
    );
    assert_eq!(narrowing, &None);
    // A project narrowed to is opened by Enter.
    press(&mut start, "h/proj");
    assert_eq!(
        press(&mut start, "\n"),
        Effect::Done(Outcome::Open(root.join("project")))
    );
    // Letters no folder holds: nothing to open, said.
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "o/zzz");
    assert!(
        page_at(&mut start, 100, 30)
            .join("\n")
            .contains("no folder holds these letters")
    );
    assert_eq!(press(&mut start, "\n"), Effect::None);
    let _ = fs::remove_dir_all(root);
}

#[test]
#[cfg(unix)]
fn a_linked_project_opens_where_it_leads() {
    // Gone into, a link was resolved; opened, it was not, and the recent
    // projects held two paths for one project.
    let root = folders("linked");
    std::os::unix::fs::symlink(root.join("project"), root.join("link")).unwrap();
    let mut start = Start::with_recent(&root, Vec::new());
    press(&mut start, "o");
    let Mode::Browse {
        entries, cursor, ..
    } = &start.mode
    else {
        panic!("browsing");
    };
    let at = entries
        .iter()
        .position(|entry| {
            matches!(entry, samplekit::tui::start::Entry::Folder { path, .. }
                if path.ends_with("link"))
        })
        .unwrap();
    for _ in *cursor..at {
        start.key(Key::Down);
    }
    assert_eq!(
        start.key(Key::Enter),
        Effect::Done(Outcome::Open(root.join("project")))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_configuration_opened_from_the_page_says_esc_goes_back_to_it() {
    // Its help said `Esc q back to the collection`, where Esc went back to
    // the start page.
    let scratch = Scratch::new("configure-help");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.open_workspace();
    workbench.back_to_start = true;
    let said: Vec<&str> = workbench
        .help_sections()
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .filter(|(keys, _)| keys == "Esc q")
        .map(|(_, said)| said)
        .collect();
    assert!(
        said.iter().all(|said| said.contains("start page")),
        "{said:?}"
    );
    assert!(!said.is_empty());
}

#[test]
fn a_question_mark_shows_the_pages_help() {
    // `?` on the page did nothing, and its keys were said nowhere.
    let root = folders("help");
    let mut start = Start::with_recent(&root, vec![root.join("project")]);
    press(&mut start, "?");
    assert!(matches!(start.mode, Mode::Help { .. }));
    let screen = page_at(&mut start, 100, 40).join("\n");
    for said in [
        "The start page",
        "forget the recent project marked",
        "New project",
        "Configure a project",
        "Moving through folders",
        "a new folder here",
    ] {
        assert!(screen.contains(said), "{said}: {screen}");
    }
    // Esc goes back to where `?` was pressed, as it was left.
    press(&mut start, "\u{1b}");
    assert_eq!(start.mode, Mode::Menu);
    press(&mut start, "o");
    let browsing = start.mode.clone();
    press(&mut start, "?j\u{1b}");
    assert_eq!(start.mode, browsing);
    // The menu's keys name it.
    press(&mut start, "\u{1b}");
    assert!(page_at(&mut start, 100, 30).join("\n").contains("? help"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn esc_on_the_first_setup_question_goes_back_removing_the_folder_made() {
    // `Esc` from the first question fell on the empty collection of the folder
    // `+` had just made, which stayed behind.
    use samplekit::tui::model::Effect as Did;
    use samplekit::tui::start::opened;
    let root = folders("setup-back");
    let made = root.join("brews");
    fs::create_dir_all(&made).unwrap();
    let mut workbench = opened(&Outcome::Made(made.clone())).unwrap().unwrap();
    assert_eq!(workbench.key(Key::Esc), Did::Quit);
    assert!(!made.exists(), "the empty folder made is removed");
    // Something put in it since is someone's: the folder stays.
    fs::create_dir_all(&made).unwrap();
    let mut workbench = opened(&Outcome::Made(made.clone())).unwrap().unwrap();
    fs::write(made.join("notes.txt"), "mine").unwrap();
    assert_eq!(workbench.key(Key::Esc), Did::Quit);
    assert!(made.join("notes.txt").is_file());
    // A folder chosen, not made: back to the page, the folder untouched.
    let mut workbench = opened(&Outcome::Create(root.join("plain")))
        .unwrap()
        .unwrap();
    assert_eq!(workbench.key(Key::Esc), Did::Quit);
    assert!(root.join("plain").is_dir());
    // A later question still goes back a question.
    let mut workbench = opened(&Outcome::Create(root.join("plain")))
        .unwrap()
        .unwrap();
    workbench.key(Key::Enter);
    assert_eq!(workbench.key(Key::Esc), Did::None);
    assert!(matches!(
        workbench.screen,
        samplekit::tui::model::Screen::Setup { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_demos_steps_are_not_recent_projects() {
    // The demo's page leads to its steps; listed here too, they pushed the
    // user's own projects out of the nine.
    let root = folders("demo-not-recent");
    let demo = samplekit::tui::start::demo_folder().expect("a data folder");
    let recent = vec![
        demo.join("03-selecting"),
        root.join("project"),
        demo.join("01-first-brews"),
        root.join("plain"),
    ];
    let mut start = Start::with_recent(&root, recent);
    assert_eq!(start.recent, [root.join("project"), root.join("plain")]);
    // The others keep their digits.
    assert_eq!(
        press(&mut start, "2"),
        Effect::Done(Outcome::Open(root.join("plain")))
    );
    let _ = fs::remove_dir_all(root);
}

/// The page drawn at a size, its cells kept with their styles.
fn page_buffer(start: &mut Start, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::start::draw(frame, start))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Whether the cell `offset` characters into the first `needle` drawn is drawn
/// as the TUI draws a key: the key colour, bold.
fn page_key(buffer: &ratatui::buffer::Buffer, needle: &str, offset: usize) -> bool {
    let wanted: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
    for y in 0..buffer.area.height {
        let row: Vec<String> = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        if let Some(x) = row
            .windows(wanted.len())
            .position(|window| window == wanted.as_slice())
        {
            let cell = &buffer[((x + offset) as u16, y)];
            let key = samplekit::tui::theme::Theme::default().key;
            return cell.fg == key && cell.modifier.contains(ratatui::style::Modifier::BOLD);
        }
    }
    panic!("{needle} is not drawn");
}

#[test]
fn the_pages_keys_are_coloured_as_the_tuis() {
    let root = folders("keys-coloured");
    let mut start = Start::with_recent(&root, vec![root.join("project")]);
    start.message = "the step marked; Enter on a project opens it".to_string();
    let buffer = page_buffer(&mut start, 100, 40);
    assert!(page_key(&buffer, "n  New project", 0));
    assert!(!page_key(&buffer, "n  New project", 3));
    assert!(page_key(&buffer, "1  ", 0));
    assert!(page_key(&buffer, "? help", 0));
    assert!(!page_key(&buffer, "? help", 2));
    // A key a message names.
    assert!(page_key(&buffer, "Enter on a project", 0));
    assert!(!page_key(&buffer, "Enter on a project", 6));
    // And one the help's descriptions name.
    start.message.clear();
    press(&mut start, "?");
    let buffer = page_buffer(&mut start, 100, 60);
    assert!(page_key(&buffer, "Esc clears it", 0));
    assert!(!page_key(&buffer, "Esc clears it", 4));
    assert!(page_key(&buffer, "Ctrl+C", 0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_logo_keeps_its_place_in_every_mode() {
    // The page centred on its widest line moved the logo sideways from the menu
    // to the browser to the naming of a folder.
    let root = folders("logo-still");
    let mut start = Start::with_recent(&root, vec![root.join("project")]);
    let logo_at = |start: &mut Start| -> usize {
        let lines = page_at(start, 160, 50);
        let line = lines
            .iter()
            .find(|line| line.contains("███████╗ █████╗"))
            .expect("the logo drawn");
        line.chars().take_while(|c| *c == ' ').count()
    };
    let menu = logo_at(&mut start);
    press(&mut start, "o");
    assert!(matches!(start.mode, Mode::Browse { .. }));
    assert_eq!(logo_at(&mut start), menu);
    press(&mut start, "\u{1b}n+");
    assert!(matches!(start.mode, Mode::Naming { .. }));
    assert_eq!(logo_at(&mut start), menu);
    press(&mut start, "\u{1b}\u{1b}?");
    assert_eq!(logo_at(&mut start), menu);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_name_and_the_letters_are_typed_in_rat_texts_line() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let chord = |code: KeyCode, modifiers: KeyModifiers| Key::Chord(KeyEvent::new(code, modifiers));
    let root = folders("typed-line");
    for name in ["measurements", "beers", "notes"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let mut start = Start::with_recent(&root, Vec::new());
    // Ctrl+Y on the menu does nothing.
    let before = start.mode.clone();
    assert_eq!(
        start.key(chord(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        Effect::None
    );
    assert_eq!(start.mode, before);
    // A new folder's name: its last word taken out, then given back.
    press(&mut start, "n+brews of may");
    start.key(chord(KeyCode::Backspace, KeyModifiers::CONTROL));
    let Mode::Naming { text, .. } = &start.mode else {
        panic!("naming: {:?}", start.mode);
    };
    assert_eq!(text, "brews of ");
    start.key(chord(KeyCode::Char('z'), KeyModifiers::CONTROL));
    let Mode::Naming { text, .. } = &start.mode else {
        panic!("naming: {:?}", start.mode);
    };
    assert_eq!(text, "brews of may");
    // A paste into the narrowing keeps the folders holding it.
    press(&mut start, "\u{1b}\u{1b}o/");
    start.paste("mtrl");
    let Mode::Browse {
        entries,
        cursor,
        narrowing: Some(text),
        ..
    } = &start.mode
    else {
        panic!("narrowing: {:?}", start.mode);
    };
    assert_eq!(text, "mtrl");
    assert!(
        matches!(&entries[*cursor], samplekit::tui::start::Entry::Folder { path, .. } if *path == root.join("beers")),
        "{entries:?}"
    );
    let _ = fs::remove_dir_all(root);
}
