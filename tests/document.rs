//! The tests of `document`.

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use samplekit::core::identifier::Identifier;
use samplekit::core::property::{Compute, ComputeError, Property};
use samplekit::core::sample::Sample;
use samplekit::core::value::Value;
use samplekit::format::document::{self, Destination, DocumentError};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

/// A scratch directory, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CANONICAL: &str = "---\nschema_version: 1\nname: Billet 42\nproperties:\n  diameter: {v: 12.5, u: 0.05, unit: g}\n---\n# Metals notes\n\nInspected after tempering.\n";

struct Fixed(f64);

impl Compute for Fixed {
    fn compute(&self) -> Result<Value, ComputeError> {
        Ok(number(self.0))
    }
}

// --------------------------------------------------------------- splitting

#[test]
fn round_trip_is_byte_identical() {
    let document = document::parse(CANONICAL).unwrap();
    assert_eq!(document::write(&document), CANONICAL);
    assert_eq!(document.schema.name.as_deref(), Some("Billet 42"));
}

#[test]
fn round_trip_preserves_note_verbatim() {
    // Trailing spaces, blank lines and an internal `---`.
    let awkward = "---\nschema_version: 1\n---\nFirst.  \n\n---\n\nstill the note   \n";
    let document = document::parse(awkward).unwrap();
    assert_eq!(
        document.note.as_str(),
        "First.  \n\n---\n\nstill the note   \n"
    );
    assert_eq!(document::write(&document), awkward);
}

#[test]
fn note_without_final_newline_is_preserved() {
    // The file does not gain one.
    let source = "---\nschema_version: 1\n---\nno trailing newline";
    let document = document::parse(source).unwrap();
    assert_eq!(document.note.as_str(), "no trailing newline");
    assert_eq!(document::write(&document), source);
}

#[test]
fn crlf_line_endings_are_preserved() {
    let source = "---\r\nschema_version: 1\r\n---\r\nA note.\r\n";
    let document = document::parse(source).unwrap();
    assert!(document.crlf);
    let out = document::write(&document);
    assert_eq!(out, source);
    assert!(!out.contains("\n\n"), "a lone LF crept in: {out:?}");
}

#[test]
fn bom_is_preserved() {
    let source = format!("\u{feff}{CANONICAL}");
    let document = document::parse(&source).unwrap();
    assert!(document.bom);
    assert_eq!(document::write(&document), source);
}

#[test]
fn empty_note_is_valid() {
    // Nothing after the closing delimiter parses fine.
    let source = "---\nschema_version: 1\n---\n";
    let document = document::parse(source).unwrap();
    assert_eq!(document.note.as_str(), "");
    assert_eq!(document::write(&document), source);
}

#[test]
fn missing_frontmatter_is_refused() {
    // Not read as a note-only file.
    let error = document::parse("Just a note.\n").unwrap_err();
    assert!(matches!(error, DocumentError::MissingFrontmatter));
    assert!(error.to_string().contains("first line"), "{error}");
}

#[test]
fn unterminated_frontmatter_is_refused() {
    // With the opening line in the message.
    let error = document::parse("---\nschema_version: 1\nname: A\n").unwrap_err();
    let DocumentError::UnterminatedFrontmatter { opened_at_line } = error else {
        panic!("expected an unterminated frontmatter, got {error:?}");
    };
    assert_eq!(opened_at_line, 1);
}

#[test]
fn malformed_yaml_reports_file_relative_line() {
    // The line number points into the actual file, not into a fragment.
    let source = "---\nschema_version: 1\nname: A\n  bad: indentation\n---\n";
    let error = document::parse(source).unwrap_err();
    let DocumentError::Yaml { line, .. } = &error else {
        panic!("expected a YAML error, got {error:?}");
    };
    let line = line.expect("a line number");
    assert!(
        line >= 3,
        "line {line} is inside the fragment, not the file"
    );
    assert!(error.to_string().contains(&line.to_string()), "{error}");
}

// ------------------------------------------------------------------- files

#[test]
fn note_survives_a_property_edit() {
    // Load, change a value, save: the note is untouched.
    let scratch = Scratch::new("note-edit");
    let path = scratch.file("a.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    sample
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(99.0));
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();

    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("# Metals notes\n\nInspected after tempering.\n"),
        "{written}"
    );
    assert!(written.contains("v: 99.0"), "{written}");
}

#[test]
#[cfg(unix)]
fn an_atomic_write_replaces_the_file_and_leaves_nothing_behind() {
    // The new bytes, the old permissions, no temporary file; and into a missing
    // directory an error, with nothing created.
    let scratch = Scratch::new("atomic-write");
    let path = scratch.file("kept.txt", "old\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    document::write_atomically(&path, b"new\n").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }
    let hostile = scratch.at("nowhere").join("kept.txt");
    assert!(document::write_atomically(&hostile, b"x").is_err());
    assert!(!scratch.at("nowhere").exists());
    let leftovers: Vec<_> = fs::read_dir(&scratch.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn save_materializes_lazy_values() {
    // A computed property is written as a plain value.
    let scratch = Scratch::new("materialize");
    let mut sample = Sample::new();
    sample
        .set_property(id("ductility"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    let path = scratch.at("b.md");
    document::save_sample(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("ductility: 3.05"), "{written}");
}

#[test]
fn save_of_unwritable_path_reports_the_path() {
    // The error names the file, not just the errno.
    let mut sample = Sample::new();
    sample
        .set_property(id("diameter"), Property::stored(number(1.0)))
        .unwrap();
    let path = PathBuf::from("/proc/samplekit-cannot-write/a.md");
    let error = document::save_sample(&mut sample, &Destination::Path(path.clone())).unwrap_err();
    assert!(
        error.to_string().contains("samplekit-cannot-write"),
        "{error}"
    );
}

#[test]
fn save_is_atomic_on_failure() {
    // A write failing mid-way leaves the original file complete.
    let scratch = Scratch::new("atomic");
    let path = scratch.file("c.md", CANONICAL);
    // A directory where the temporary file cannot be created.
    let hostile = scratch.at("nowhere").join("c.md");
    let mut sample = Sample::new();
    sample
        .set_property(id("diameter"), Property::stored(number(1.0)))
        .unwrap();
    assert!(document::save_sample(&mut sample, &Destination::Path(hostile)).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), CANONICAL);
    // And no temporary file was left behind beside it.
    let leftovers: Vec<_> = fs::read_dir(&scratch.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

// -------------------------------------------------------- concurrent edits

#[test]
fn a_held_lock_refuses_a_second_save() {
    // Detection that can be stepped over is not detection: while one save holds
    // the file, the next one stops rather than racing it.
    let scratch = Scratch::new("locked");
    let path = scratch.file("d.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    let mut lock_name = path.clone().into_os_string();
    lock_name.push(".lock");
    fs::write(&lock_name, "").unwrap();

    let error = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap_err();
    assert!(matches!(error, DocumentError::Locked { .. }), "{error:?}");
    assert!(error.to_string().contains("d.md"), "{error}");
    // And the file it would have written is untouched.
    assert_eq!(fs::read_to_string(&path).unwrap(), CANONICAL);
}

#[test]
#[cfg(unix)]
fn a_lock_its_writer_left_is_taken_back() {
    // A save killed inside its section left its lock and its temporary, and
    // every later save of the file was refused until someone removed the lock
    // by hand. A lock naming a process that no longer runs is taken back.
    let scratch = Scratch::new("lock-abandoned");
    let path = scratch.file("d.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let mut lock_name = path.clone().into_os_string();
    lock_name.push(".lock");
    fs::write(
        &lock_name,
        format!(
            "{pid}\nhost {}\nuser {}\n",
            samplekit::format::lock::own_host(),
            samplekit::format::lock::own_user()
        ),
    )
    .unwrap();
    let temporary = scratch.0.join(format!(".d.md.{pid}.tmp"));
    fs::write(&temporary, "half").unwrap();

    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    assert!(!PathBuf::from(&lock_name).exists(), "the lock was kept");
    assert!(!temporary.exists(), "the temporary was kept");
}

#[test]
#[cfg(unix)]
fn a_lock_of_another_host_or_user_is_never_taken_back() {
    // A lock is judged by its process only where that process could run here:
    // on a shared folder, another computer's live save was taken for a dead
    // one because its id ran nothing on this one.
    let scratch = Scratch::new("lock-elsewhere");
    let path = scratch.file("d.md", CANONICAL);
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let mut lock_name = path.clone().into_os_string();
    lock_name.push(".lock");
    for (host, user) in [
        (
            "lab-server".to_string(),
            samplekit::format::lock::own_user(),
        ),
        (
            samplekit::format::lock::own_host(),
            "someone-else".to_string(),
        ),
    ] {
        fs::write(&lock_name, format!("{pid}\nhost {host}\nuser {user}\n")).unwrap();
        let (mut sample, origin) = document::load_sample(&path).unwrap();
        let error = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap_err();
        let said = error.to_string();
        assert!(
            said.contains(&format!("process {pid} of {user} on {host}")),
            "{said}"
        );
        assert!(said.contains("remove"), "{said}");
        assert_eq!(fs::read_to_string(&path).unwrap(), CANONICAL);
    }
    // Its own host and user, the process gone: taken back.
    fs::write(
        &lock_name,
        format!(
            "{pid}\nhost {}\nuser {}\n",
            samplekit::format::lock::own_host(),
            samplekit::format::lock::own_user()
        ),
    )
    .unwrap();
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    assert!(!PathBuf::from(&lock_name).exists());
}

#[test]
#[cfg(unix)]
fn a_lock_of_a_process_another_account_runs_is_never_taken_back() {
    // `kill -0` refused on a root process read as no process: a lock naming
    // one by its id alone was taken back while it ran.
    let scratch = Scratch::new("lock-root");
    let path = scratch.file("d.md", CANONICAL);
    let mut lock_name = path.clone().into_os_string();
    lock_name.push(".lock");
    fs::write(&lock_name, "1").unwrap();
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    let error = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap_err();
    assert!(error.to_string().contains("process 1"), "{error}");
    assert_eq!(fs::read_to_string(&path).unwrap(), CANONICAL);
    assert!(PathBuf::from(&lock_name).exists());
}

#[test]
fn a_lock_is_released_when_a_save_fails() {
    // A refused save that left its lock behind would block every save after it.
    let scratch = Scratch::new("lock-released");
    let path = scratch.file("d.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    fs::write(&path, CANONICAL.replace("12.5", "77.7")).unwrap();
    let error = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap_err();
    assert!(
        matches!(error, DocumentError::ConcurrentEdit { .. }),
        "{error:?}"
    );

    let mut lock_name = path.clone().into_os_string();
    lock_name.push(".lock");
    assert!(
        !PathBuf::from(&lock_name).exists(),
        "a lock survived a refusal"
    );
    // And the next save, given what is now on disk, goes through.
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
}

#[test]
fn a_changed_file_is_not_overwritten() {
    // Load, change the file behind the program's back, save: refused.
    let scratch = Scratch::new("concurrent");
    let path = scratch.file("d.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    fs::write(&path, CANONICAL.replace("12.5", "77.7")).unwrap();

    let error = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap_err();
    assert!(
        matches!(error, DocumentError::ConcurrentEdit { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("d.md"), "{error}");
    // And the other session's write is still there.
    assert!(fs::read_to_string(&path).unwrap().contains("77.7"));
}

#[test]
fn writing_to_a_path_compares_nothing() {
    // The deliberate overwrite works, and is a different call.
    let scratch = Scratch::new("deliberate");
    let path = scratch.file("e.md", CANONICAL);
    let (mut sample, _) = document::load_sample(&path).unwrap();
    fs::write(&path, CANONICAL.replace("12.5", "77.7")).unwrap();
    document::save_sample(&mut sample, &Destination::Path(path.clone())).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("12.5"));
}

#[test]
fn a_save_returns_a_usable_origin() {
    // Saving twice in one session needs no reload.
    let scratch = Scratch::new("twice");
    let path = scratch.file("f.md", CANONICAL);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    let next = document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    sample
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(42.0));
    document::save_sample(&mut sample, &Destination::Origin(next)).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("42.0"));
}

// ---------------------------------------------------------------- records

const WITH_RECORDS: &str = "---\nschema_version: 1\nproperties:\n  diameter: {v: 12.5, fingerprint: 87c43f2ca298}\n  ductility:\n    v: 3.05\n    computed: {diameter: 87c43f2ca298}\n    fingerprint: aaaaaaaaaaaa\n---\nA note.\n";

#[test]
fn save_without_a_model_preserves_records() {
    // Load, edit the note, save: `computed` is byte-identical.
    let scratch = Scratch::new("records");
    let path = scratch.file("g.md", WITH_RECORDS);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    sample.set_note("A different note.\n".to_string());
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("computed: {diameter: 87c43f2ca298}"),
        "{written}"
    );
    assert!(written.contains("fingerprint: aaaaaaaaaaaa"), "{written}");
    assert!(written.contains("A different note."), "{written}");
}

#[test]
fn records_survive_a_value_edit_of_another_property() {
    // Editing diameter does not touch ductility's record, only its freshness.
    let scratch = Scratch::new("other-edit");
    let path = scratch.file("h.md", WITH_RECORDS);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    sample
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(99.0));
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("computed: {diameter: 87c43f2ca298}"),
        "{written}"
    );
    // And diameter lost its own record, because a typed-in value claims nothing.
    assert!(
        !written.contains("diameter: {v: 99.0, fingerprint"),
        "{written}"
    );
}

#[test]
fn save_computed_restamps_records() {
    // Recomputing after an input changed writes the new fingerprints.
    use samplekit::core::dependency_graph::Node;
    let scratch = Scratch::new("restamp");
    let mut sample = Sample::new();
    sample
        .set_property(id("diameter"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("ductility"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    sample
        .declare_dependencies(&id("ductility"), &[Node::Named(id("diameter"))])
        .unwrap();
    let path = scratch.at("i.md");
    sample.property(&id("ductility")).unwrap().value().unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    // The record names the channel its formula produced.
    assert!(
        first.contains("computed: {v: {diameter: 87c43f2ca298}}"),
        "{first}"
    );
    assert!(first.contains("fingerprint:"), "{first}");

    // A different diameter gives a different recorded digest.
    let mut again = Sample::new();
    again
        .set_property(id("diameter"), Property::stored(number(99.0)))
        .unwrap();
    again
        .set_property(id("ductility"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    again
        .declare_dependencies(&id("ductility"), &[Node::Named(id("diameter"))])
        .unwrap();
    again.property(&id("ductility")).unwrap().value().unwrap();
    document::save_computed(&mut again, &Destination::Path(path.clone())).unwrap();
    let second = fs::read_to_string(&path).unwrap();
    assert!(!second.contains("87c43f2ca298"), "{second}");
    assert!(second.contains("computed: {v: {diameter:"), "{second}");
}

// ----------------------------------------------------------------- filling

#[test]
fn load_into_fills_a_declaration() {
    use samplekit::core::formatting::Presentation;
    let scratch = Scratch::new("load-into");
    let path = scratch.file(
        "billet.md",
        "---\nschema_version: 1\nname: Billet 42\nproperties:\n  diameter: {v: 12.5, u: 0.05}\n  ductility: 3.0\n---\n# Notes\n",
    );
    let mut sample = Sample::new();
    let mut diameter = Property::stored(Value::absent());
    diameter.set_presentation(Presentation {
        unit: Some("g".to_string()),
        ..Presentation::default()
    });
    sample.set_property(id("diameter"), diameter).unwrap();
    sample
        .set_property(id("ductility"), Property::computed(Rc::new(Fixed(9.0))))
        .unwrap();

    let origin = document::load_into(&path, &mut sample).unwrap();
    assert_eq!(origin.path, path);
    let ductility = sample.property(&id("ductility")).unwrap();
    assert!(ductility.is_computed());
    assert_eq!(ductility.value().unwrap(), number(3.0));
    let diameter = sample.property(&id("diameter")).unwrap();
    assert_eq!(diameter.presentation().unit.as_deref(), Some("g"));
    assert_eq!(
        diameter.uncertainty().unwrap().map(|u| u.magnitude()),
        Some(0.05)
    );
    assert_eq!(sample.name(), Some("Billet 42"));
    assert_eq!(sample.note(), "# Notes\n");
}

#[test]
fn a_stale_filled_value_stays_stale_after_saving() {
    use samplekit::core::dependency_graph::Node;
    use samplekit::format::fingerprint::{self, Freshness};
    let scratch = Scratch::new("stale-filled");
    let path = scratch.at("billet.md");
    let declared = || {
        let mut sample = Sample::new();
        sample
            .set_property(id("diameter"), Property::stored(Value::absent()))
            .unwrap();
        sample
            .set_property(id("ductility"), Property::computed(Rc::new(Fixed(3.05))))
            .unwrap();
        sample
            .declare_dependencies(&id("ductility"), &[Node::Named(id("diameter"))])
            .unwrap();
        sample
    };
    // Computed and recorded against a diameter of 12.5.
    let mut computed = declared();
    computed
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(12.5));
    computed
        .property(&id("ductility"))
        .unwrap()
        .value()
        .unwrap();
    document::save_computed(&mut computed, &Destination::Path(path.clone())).unwrap();
    // The diameter is corrected by hand, and nothing recomputes the ductility.
    let edited = fs::read_to_string(&path).unwrap().replace("12.5", "13.5");
    fs::write(&path, edited).unwrap();

    let mut sample = declared();
    document::load_into(&path, &mut sample).unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();

    let (reloaded, _) = document::load_sample(&path).unwrap();
    let freshness = fingerprint::check_property(&reloaded, &id("ductility")).unwrap();
    assert!(
        matches!(freshness, Freshness::Stale { .. }),
        "{freshness:?}\n{}",
        fs::read_to_string(&path).unwrap()
    );
}

/// Twice a diameter, read through its handle.
struct Twice(samplekit::core::sample::PropertyHandle);

impl Compute for Twice {
    fn compute(&self) -> Result<Value, ComputeError> {
        match self.0.value()? {
            Value::Number(diameter) => Ok(number(diameter * 2.0)),
            other => Ok(other),
        }
    }
}

#[test]
fn save_computed_keeps_the_formulas() {
    use samplekit::core::dependency_graph::Node;
    let scratch = Scratch::new("keeps-formulas");
    let mut sample = Sample::new();
    sample
        .set_property(id("diameter"), Property::stored(number(12.5)))
        .unwrap();
    let twice = Twice(sample.property(&id("diameter")).unwrap());
    sample
        .set_property(id("double"), Property::computed(Rc::new(twice)))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[Node::Named(id("diameter"))])
        .unwrap();
    document::save_computed(&mut sample, &Destination::Path(scratch.at("c.md"))).unwrap();

    let double = sample.property(&id("double")).unwrap();
    assert!(double.is_computed());
    sample
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(20.0));
    assert_eq!(double.value().unwrap(), number(40.0));
}

// ------------------------------------------------------- saving as it stands

/// Counts its runs, so that a save can be shown to run none.
struct Counted(std::cell::Cell<usize>);

impl Compute for Counted {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.0.set(self.0.get() + 1);
        Ok(number(3.05))
    }
}

fn counted_sample() -> (Sample, Rc<Counted>) {
    use samplekit::core::dependency_graph::Node;
    let formula = Rc::new(Counted(std::cell::Cell::new(0)));
    let mut sample = Sample::new();
    sample
        .set_property(id("diameter"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("ductility"), Property::computed(formula.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("ductility"), &[Node::Named(id("diameter"))])
        .unwrap();
    (sample, formula)
}

#[test]
fn save_computed_runs_no_formula() {
    let scratch = Scratch::new("runs-nothing");
    let (mut sample, formula) = counted_sample();
    let path = scratch.at("n.md");
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    assert_eq!(formula.0.get(), 0);
    let written = fs::read_to_string(&path).unwrap();
    assert!(!written.contains("3.05"), "{written}");
    assert!(!written.contains("computed"), "{written}");
}

#[test]
fn save_computed_refuses_a_value_it_cannot_record() {
    let scratch = Scratch::new("unrecordable");
    let (mut sample, _) = counted_sample();
    sample.property(&id("ductility")).unwrap().value().unwrap();
    sample
        .property(&id("diameter"))
        .unwrap()
        .set_value(number(20.0));
    let path = scratch.at("u.md");
    let refused = document::save_computed(&mut sample, &Destination::Path(path.clone()));
    assert!(
        matches!(&refused, Err(DocumentError::Unrecordable { values }) if values == &["ductility"]),
        "{refused:?}"
    );
    assert!(!path.exists());
}

// --------------------------------------------------------------- overrides

#[test]
fn a_value_edited_in_the_file_loads_as_an_override() {
    use samplekit::format::fingerprint::{self, Freshness};
    let scratch = Scratch::new("edited-in-file");
    let path = scratch.at("e.md");
    let (mut sample, _) = counted_sample();
    sample.property(&id("ductility")).unwrap().value().unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let edited = text.replace("v: 3.05", "v: 3.5");
    assert_ne!(edited, text);
    fs::write(&path, edited).unwrap();

    let (mut again, formula) = counted_sample();
    document::load_into(&path, &mut again).unwrap();
    let ductility = again.property(&id("ductility")).unwrap();
    assert!(ductility.is_edited());
    assert_eq!(ductility.value().unwrap(), number(3.5));
    assert!(matches!(
        fingerprint::check_property(&again, &id("ductility")).unwrap(),
        Freshness::Edited
    ));
    assert_eq!(formula.0.get(), 0);
}

#[test]
fn an_override_round_trips_through_save_computed() {
    let scratch = Scratch::new("override-round-trip");
    let path = scratch.at("o.md");
    // Never computed, then overridden.
    let (mut sample, formula) = counted_sample();
    sample
        .property(&id("ductility"))
        .unwrap()
        .set_value(number(3.5));
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("fingerprint: {edited: "), "{written}");
    assert_eq!(formula.0.get(), 0);

    let (mut again, formula) = counted_sample();
    document::load_into(&path, &mut again).unwrap();
    let ductility = again.property(&id("ductility")).unwrap();
    assert!(ductility.is_edited());
    assert_eq!(ductility.value().unwrap(), number(3.5));
    assert_eq!(formula.0.get(), 0);

    // Computed, then overridden.
    again.property(&id("ductility")).unwrap().restore_formula();
    again.property(&id("ductility")).unwrap().value().unwrap();
    fingerprint_stamp(&mut again);
    again
        .property(&id("ductility"))
        .unwrap()
        .set_value(number(4.5));
    document::save_computed(&mut again, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("computed: {v: {diameter: "), "{written}");
    assert!(written.contains("fingerprint: {edited: "), "{written}");
    let (mut third, _) = counted_sample();
    document::load_into(&path, &mut third).unwrap();
    assert!(third.property(&id("ductility")).unwrap().is_edited());
    assert_eq!(
        third.property(&id("ductility")).unwrap().value().unwrap(),
        number(4.5)
    );
}

fn fingerprint_stamp(sample: &mut Sample) {
    samplekit::format::fingerprint::stamp(sample).unwrap();
}

#[test]
fn an_input_fingerprint_follows_its_value() {
    use samplekit::core::dependency_graph::Node;
    let scratch = Scratch::new("input-fingerprint");
    let path = scratch.file(
        "i.md",
        "---\nschema_version: 1\nproperties:\n  diameter: {v: 12.5, fingerprint: \"000000000000\"}\n  ductility:\n    v: 3.05\n    computed: {diameter: 87c43f2ca298}\n---\n",
    );
    let (mut sample, _) = document::load_sample(&path).unwrap();
    sample
        .declare_dependencies(&id("ductility"), &[Node::Named(id("diameter"))])
        .unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("diameter: {v: 12.5, fingerprint: 87c43f2ca298}"),
        "{written}"
    );
}

#[test]
fn a_parse_error_gives_one_line_number() {
    let source = "---\nschema_version: 1\nname: x\nproperties:\n  diameter: {v: [1, }\n---\n";
    let message = samplekit::format::document::parse(source)
        .unwrap_err()
        .to_string();
    assert!(message.starts_with("line "), "{message}");
    assert!(!message.contains(" at line "), "{message}");
}

#[test]
fn a_mapping_attribute_names_its_key_and_line() {
    let source = "---\nschema_version: 1\nname: m\nbatch: 1\nfoo: {bar: 1}\n---\n";
    let message = samplekit::format::document::parse(source)
        .unwrap_err()
        .to_string();
    assert!(message.contains("line 5"), "{message}");
    assert!(message.contains("'foo'"), "{message}");
}

#[test]
#[cfg(unix)]
fn a_save_that_changes_nothing_writes_nothing() {
    use samplekit::format::document::{self, Destination};
    use std::os::unix::fs::PermissionsExt;
    let directory = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-doc-noop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("a.md");
    let parsed = document::parse("---\nschema_version: 1\nname: A\n---\nN.\n").unwrap();
    let canonical = document::write(&parsed);
    std::fs::write(&path, &canonical).unwrap();
    // Read-only: a write would be refused, and nothing is written.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let saved = document::save(&parsed, &Destination::Path(path.clone()));
    assert!(saved.is_ok(), "{saved:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), canonical);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_sample_with_a_part_set_aside_is_not_written() {
    let directory = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-doc-aside-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("a.md");
    let text = "---\nschema_version: 1\ntables:\n  tempering:\n    index: day\n    columns: {day: {}}\n    rows:\n      - {day: {v: 1}}\n      - {day: {v: 1}}\n---\nnote\n";
    fs::write(&path, text).unwrap();
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    let refused = document::save_sample(&mut sample, &Destination::Origin(origin));
    assert!(matches!(refused, Err(DocumentError::PartSetAside { .. })));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_write_reads_the_file_again_and_refuses_another_writers_change() {
    // What a command read and showed is what it replaces: a file another writer
    // changed, or made, since is refused and left as it is.
    let scratch = Scratch::new("replace-if-unchanged");
    let path = scratch.file("x.csv", "theirs\n");
    let changed = document::replace_if_unchanged(&path, Some(b"mine\n"), Some(b"new\n"));
    assert!(
        matches!(changed, Err(DocumentError::ConcurrentEdit { .. })),
        "{changed:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
    let made = document::replace_if_unchanged(&path, None, Some(b"new\n"));
    assert!(
        matches!(made, Err(DocumentError::WrittenSince { .. })),
        "{made:?}"
    );
    document::replace_if_unchanged(&path, Some(b"theirs\n"), Some(b"new\n")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    document::replace_if_unchanged(&path, Some(b"new\n"), None).unwrap();
    assert!(!path.exists());
    // A new sample: one made there since it was looked for is not replaced.
    let taken = scratch.file(
        "s.md",
        "---\nschema_version: 1\nname: s\n---\nAnother writer's.\n",
    );
    let mut sample = Sample::new();
    sample.set_name(Some("s".to_string()));
    let refused = document::save_sample(&mut sample, &Destination::New(taken.clone()));
    assert!(
        matches!(refused, Err(DocumentError::WrittenSince { .. })),
        "{refused:?}"
    );
    let fresh = scratch.at("t.md");
    document::save_sample(&mut sample, &Destination::New(fresh.clone())).unwrap();
    assert!(fresh.exists());
}

#[test]
fn a_mean_an_earlier_version_wrote_is_a_value_written() {
    // The mean an earlier SampleKit wrote beside readings carries no record,
    // and nothing tells it from a value typed. Under a model declaring the
    // median it is read as written, held over the statistic, and never let go
    // for matching the mean.
    use samplekit::core::statistics::Location;
    let scratch = Scratch::new("legacy-mean");
    let path = scratch.file(
        "billet.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  diameter: {v: 4.0, readings: [1.0, 2.0, 9.0]}\n---\n",
    );
    let mut sample = Sample::new();
    let mut diameter = Property::stored(Value::absent());
    diameter.declare_statistics(Some(Location::Median), None);
    sample.set_property(id("diameter"), diameter).unwrap();
    document::load_into(&path, &mut sample).unwrap();
    let diameter = sample.property(&id("diameter")).unwrap();
    assert_eq!(diameter.value().unwrap(), number(4.0));
    assert!(diameter.peek(Property::holds_written_override));
    // Read without the model, it is the value, as it always read.
    let (plain, _) = document::load_sample(&path).unwrap();
    assert_eq!(
        plain.property(&id("diameter")).unwrap().value().unwrap(),
        number(4.0)
    );
}

#[test]
fn a_file_names_a_sample_that_writes_no_name() {
    // Read, a sample is named by its file where it writes no `name:`, and
    // saving writes none for it; saved elsewhere, it is named by its new file.
    let scratch = Scratch::new("named-by-file");
    let path = scratch.0.join("s-1.md");
    fs::write(
        &path,
        "---\nschema_version: 1\nproperties:\n  diameter: {v: 2.0}\n---\n",
    )
    .unwrap();
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    assert_eq!(sample.name(), Some("s-1"));
    assert_eq!(sample.written_name(), None);
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("name:"));
    let copy = scratch.0.join("s-2.md");
    document::save_sample(&mut sample, &Destination::New(copy.clone())).unwrap();
    assert_eq!(sample.name(), Some("s-2"));
    assert!(!fs::read_to_string(&copy).unwrap().contains("name:"));
}

#[cfg(unix)]
#[test]
fn a_link_out_of_its_project_is_not_written_through() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-document-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("p/inner")).unwrap();
    std::fs::create_dir_all(root.join("outside")).unwrap();
    std::fs::write(root.join("p/.samplekitrc"), "schema_version = 1\n").unwrap();
    let text = "---\nschema_version: 1\nname: ext\n---\n";
    std::fs::write(root.join("outside/ext.md"), text).unwrap();
    std::fs::write(root.join("p/inner/own.md"), text).unwrap();
    std::os::unix::fs::symlink("../outside/ext.md", root.join("p/out.md")).unwrap();
    std::os::unix::fs::symlink("inner/own.md", root.join("p/in.md")).unwrap();
    let refused = document::refuse_link_out_of_project(&root.join("p/out.md")).unwrap_err();
    assert!(
        refused.to_string().contains("outside its project"),
        "{refused}"
    );
    // A link inside its project, and a file that is none, are written.
    assert!(document::refuse_link_out_of_project(&root.join("p/in.md")).is_ok());
    assert!(document::refuse_link_out_of_project(&root.join("p/inner/own.md")).is_ok());
    // Nothing is written through the outward one.
    let (read, origin) = document::load(&root.join("p/out.md")).unwrap();
    assert!(document::save(&read, &document::Destination::Origin(origin)).is_err());
    assert!(
        document::replace_if_unchanged(
            &root.join("p/out.md"),
            Some(text.as_bytes()),
            Some(b"---\nschema_version: 1\nname: changed\n---\n"),
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("outside/ext.md")).unwrap(),
        text
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_cells_own_statistic_survives_a_save() {
    // Read — its median gave the cell its value — then dropped at the next
    // write, and the value with it: readings with no statistic have none.
    let scratch = Scratch::new("cell-statistic");
    let path = scratch.file(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  t:\n    index: day\n    columns:\n      \
         day: {}\n      g: {statistics: {v: mean}}\n      h: {}\n    rows:\n      - day: 1\n        \
         g: {readings: [1.0, 2.0, 10.0], statistics: {v: median}}\n        \
         h: {readings: [1.0, 2.0, 10.0], statistics: {v: median}}\n      - day: 2\n        \
         g: {readings: [1.0, 3.0], statistics: {v: mean}}\n        h: 4\n---\n",
    );
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("g: {readings: [1.0, 2.0, 10.0], statistics: {v: median}}"),
        "{written}"
    );
    assert!(
        written.contains("h: {readings: [1.0, 2.0, 10.0], statistics: {v: median}}"),
        "{written}"
    );
    // The column's own, not repeated on its cell.
    assert!(written.contains("g: {readings: [1.0, 3.0]}"), "{written}");
}

#[test]
fn a_list_written_null_is_none() {
    // `tags: ~` made the whole file unreadable, in serde's words.
    let scratch = Scratch::new("null-list");
    let path = scratch.file(
        "a.md",
        "---\nschema_version: 1\nname: a\ntags: ~\nproperties: ~\ntables: ~\n---\n",
    );
    let (sample, _) = document::load_sample(&path).unwrap();
    assert!(sample.tags().is_empty());
}

#[test]
fn yamls_own_tag_is_said_dropped_by_a_write() {
    // The parser reads `!!binary` away: found in the text.
    let said = document::changed_by_a_write(
        "---\nschema_version: 1\nname: b\nblob: !!binary aGVsbG8=\nnote: wow !!\n---\n",
    );
    assert_eq!(
        said,
        ["blob carries the YAML tag !!binary: the next write drops it"]
    );
}

#[test]
fn the_comments_a_write_drops_are_counted() {
    let source = "---\nschema_version: 1\n# a remark\nname: A  # its name\nnote: \"not # this\"\n---\n# a heading, in the note\n";
    assert_eq!(document::comments_in(source), 2);
    let windows = format!("\u{feff}{}", source.replace('\n', "\r\n"));
    assert_eq!(document::comments_in(&windows), 2);
    assert_eq!(document::comments_in("# no frontmatter\n"), 0);
}

#[test]
fn a_link_written_unquoted_is_refused_at_its_line() {
    let source = "---\nschema_version: 1\nname: A\nreference: [[bar-02]]  # the twin\n---\n";
    let error = document::parse(source).unwrap_err();
    assert_eq!(
        error.to_string(),
        "line 4: [[bar-02]] unquoted reads as a list: a link is quoted, \
         reference: \"[[bar-02]]\""
    );
    // Quoted, it is text; a list of lists is no link, and refused as it was.
    assert!(document::parse("---\nschema_version: 1\nreference: \"[[bar-02]]\"\n---\n").is_ok());
    let nested = document::parse("---\nschema_version: 1\npairs: [[1, 2]]\n---\n").unwrap_err();
    assert!(!nested.to_string().contains("a link is quoted"), "{nested}");
}

#[test]
fn a_markdown_link_written_unquoted_is_refused_at_its_line() {
    let source = "---\nschema_version: 1\nname: A\nreference: [the twin](bar-02.md)\n---\n";
    let error = document::parse(source).unwrap_err();
    assert_eq!(
        error.to_string(),
        "line 4: [the twin](bar-02.md) unquoted reads as a list: a link is quoted, \
         reference: \"[the twin](bar-02.md)\""
    );
    assert!(
        document::parse("---\nschema_version: 1\nreference: \"[the twin](bar-02.md)\"\n---\n")
            .is_ok()
    );
}

// ------------------------------------------------------------ changes waiting

const WAITING: &str = "---\nschema_version: 1\nname: C1\nalloy: cobalt\n---\n";

/// A project of one sample whose changes wait in a folder beside it, as the
/// terminal interface keeps them; the folder let go when the test ends.
struct Waiting {
    scratch: Scratch,
    folder: PathBuf,
}

impl Waiting {
    fn new(name: &str) -> Waiting {
        let scratch = Scratch::new(name);
        fs::create_dir_all(scratch.0.join("project")).unwrap();
        scratch.file("project/C1.md", WAITING);
        let folder = scratch.0.join("waiting");
        samplekit::format::stage::begin(&folder, Some(&scratch.0.join("project"))).unwrap();
        Waiting { scratch, folder }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.scratch.0.join("project").join(name)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        samplekit::format::stage::end(&self.folder);
    }
}

fn saved_as(path: &std::path::Path, alloy: &str) {
    let (mut sample, origin) = document::load_sample(path).unwrap();
    sample
        .set_attribute(
            id("alloy"),
            samplekit::core::sample::AttributeValue::Scalar(Value::text(alloy)),
        )
        .unwrap();
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
}

#[test]
fn a_save_waits_and_the_file_reads_as_it_would() {
    let waiting = Waiting::new("waiting-save");
    let path = waiting.at("C1.md");
    saved_as(&path, "nickel");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        WAITING,
        "the file is untouched"
    );
    let read = document::read_text(&path).unwrap();
    assert!(read.contains("alloy: nickel"), "{read}");
    // A second save follows the first, read as it left it.
    saved_as(&path, "chrome");
    assert!(
        document::read_text(&path)
            .unwrap()
            .contains("alloy: chrome")
    );
    assert_eq!(samplekit::format::stage::count(&waiting.folder), 1);
    // Back to what the disk holds: nothing waits.
    saved_as(&path, "cobalt");
    assert_eq!(samplekit::format::stage::count(&waiting.folder), 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), WAITING);
}

#[test]
fn what_waits_is_written_at_once_and_refused_over_another_hands_change() {
    let waiting = Waiting::new("waiting-commit");
    let first = waiting.at("C1.md");
    let second = waiting.at("C2.md");
    fs::write(&second, WAITING.replace("C1", "C2")).unwrap();
    saved_as(&first, "nickel");
    saved_as(&second, "nickel");
    let written = samplekit::format::stage::commit(&waiting.folder).unwrap();
    assert_eq!(written.len(), 2);
    assert!(
        fs::read_to_string(&first)
            .unwrap()
            .contains("alloy: nickel")
    );
    assert!(
        fs::read_to_string(&second)
            .unwrap()
            .contains("alloy: nickel")
    );
    assert_eq!(samplekit::format::stage::count(&waiting.folder), 0);
    // Another hand writes C1 while a change waits on it: nothing is written.
    saved_as(&first, "tin");
    saved_as(&second, "tin");
    fs::write(
        &first,
        "---\nschema_version: 1\nname: C1\nalloy: zinc\n---\n",
    )
    .unwrap();
    match samplekit::format::stage::commit(&waiting.folder) {
        Err(DocumentError::ConcurrentEdit { path }) => assert!(path.ends_with("C1.md"), "{path:?}"),
        other => panic!("{other:?}"),
    }
    assert!(fs::read_to_string(&first).unwrap().contains("alloy: zinc"));
    assert!(
        fs::read_to_string(&second)
            .unwrap()
            .contains("alloy: nickel")
    );
    assert_eq!(samplekit::format::stage::count(&waiting.folder), 2);
}

#[test]
fn a_removal_and_a_move_wait_and_a_file_made_then_removed_was_never_there() {
    let waiting = Waiting::new("waiting-remove");
    let path = waiting.at("C1.md");
    let photo = waiting.at("C1.png");
    fs::write(&photo, [137u8, 80, 78, 71]).unwrap();
    let moved = waiting.at("photos/C9.png");
    document::remove_file(&path).unwrap();
    document::move_file(&photo, &moved).unwrap();
    assert!(
        path.is_file() && photo.is_file() && !moved.exists(),
        "nothing moves yet"
    );
    assert!(
        document::read_text(&path).is_err(),
        "removed, it reads as no file"
    );
    assert!(samplekit::format::stage::exists(&moved));
    samplekit::format::stage::commit(&waiting.folder).unwrap();
    assert!(!path.exists() && !photo.exists());
    assert_eq!(fs::read(&moved).unwrap(), [137u8, 80, 78, 71]);
    // Made while waiting, then removed: nothing waits.
    let made = waiting.at("C2.md");
    fs::write(waiting.at("C3.md"), WAITING.replace("C1", "C3")).unwrap();
    let (mut sample, _) = document::load_sample(&waiting.at("C3.md")).unwrap();
    document::save_sample(&mut sample, &Destination::New(made.clone())).unwrap();
    assert!(!made.exists() && samplekit::format::stage::exists(&made));
    document::remove_file(&made).unwrap();
    assert_eq!(samplekit::format::stage::count(&waiting.folder), 0);
}

#[test]
fn what_waits_outlasts_the_process_and_is_read_again() {
    let waiting = Waiting::new("waiting-again");
    let path = waiting.at("C1.md");
    saved_as(&path, "nickel");
    fs::write(waiting.at("C2.md"), WAITING.replace("C1", "C2")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    saved_as(&waiting.at("C2.md"), "tin");
    // An entry cut short, as a process killed while writing leaves it.
    fs::write(waiting.folder.join("cut.json"), "{\"path\": ").unwrap();
    samplekit::format::stage::end(&waiting.folder);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        WAITING,
        "ended, the disk's is read"
    );
    let count =
        samplekit::format::stage::begin(&waiting.folder, Some(&waiting.scratch.0.join("project")))
            .unwrap();
    assert_eq!(count, 2);
    let entries = samplekit::format::stage::entries(&waiting.folder);
    assert!(entries[0].path.ends_with("C1.md") && entries[1].path.ends_with("C2.md"));
    assert!(
        document::read_text(&path)
            .unwrap()
            .contains("alloy: nickel")
    );
}
