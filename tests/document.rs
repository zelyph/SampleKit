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

const CANONICAL: &str = "---\nschema_version: 1\nname: Keg 42\nproperties:\n  malt: {v: 12.5, u: 0.05, unit: g}\n---\n# Brewing notes\n\nTasted after conditioning.\n";

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
    assert_eq!(document.schema.name.as_deref(), Some("Keg 42"));
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
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();

    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("# Brewing notes\n\nTasted after conditioning.\n"),
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
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    let path = scratch.at("b.md");
    document::save_sample(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("plato: 3.05"), "{written}");
}

#[test]
fn save_of_unwritable_path_reports_the_path() {
    // The error names the file, not just the errno.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(1.0)))
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
        .set_property(id("malt"), Property::stored(number(1.0)))
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
        .property(&id("malt"))
        .unwrap()
        .set_value(number(42.0));
    document::save_sample(&mut sample, &Destination::Origin(next)).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("42.0"));
}

// ---------------------------------------------------------------- records

const WITH_RECORDS: &str = "---\nschema_version: 1\nproperties:\n  malt: {v: 12.5, fingerprint: 87c43f2ca298}\n  plato:\n    v: 3.05\n    computed: {malt: 87c43f2ca298}\n    fingerprint: aaaaaaaaaaaa\n---\nA note.\n";

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
        written.contains("computed: {malt: 87c43f2ca298}"),
        "{written}"
    );
    assert!(written.contains("fingerprint: aaaaaaaaaaaa"), "{written}");
    assert!(written.contains("A different note."), "{written}");
}

#[test]
fn records_survive_a_value_edit_of_another_property() {
    // Editing malt does not touch plato's record, only its freshness.
    let scratch = Scratch::new("other-edit");
    let path = scratch.file("h.md", WITH_RECORDS);
    let (mut sample, origin) = document::load_sample(&path).unwrap();
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    document::save_sample(&mut sample, &Destination::Origin(origin)).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("computed: {malt: 87c43f2ca298}"),
        "{written}"
    );
    // And malt lost its own record, because a typed-in value claims nothing.
    assert!(
        !written.contains("malt: {v: 99.0, fingerprint"),
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
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[Node::Named(id("malt"))])
        .unwrap();
    let path = scratch.at("i.md");
    sample.property(&id("plato")).unwrap().value().unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    // The record names the channel its formula produced.
    assert!(
        first.contains("computed: {v: {malt: 87c43f2ca298}}"),
        "{first}"
    );
    assert!(first.contains("fingerprint:"), "{first}");

    // A different malt gives a different recorded digest.
    let mut again = Sample::new();
    again
        .set_property(id("malt"), Property::stored(number(99.0)))
        .unwrap();
    again
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    again
        .declare_dependencies(&id("plato"), &[Node::Named(id("malt"))])
        .unwrap();
    again.property(&id("plato")).unwrap().value().unwrap();
    document::save_computed(&mut again, &Destination::Path(path.clone())).unwrap();
    let second = fs::read_to_string(&path).unwrap();
    assert!(!second.contains("87c43f2ca298"), "{second}");
    assert!(second.contains("computed: {v: {malt:"), "{second}");
}

// ----------------------------------------------------------------- filling

#[test]
fn load_into_fills_a_declaration() {
    use samplekit::core::formatting::Presentation;
    let scratch = Scratch::new("load-into");
    let path = scratch.file(
        "keg.md",
        "---\nschema_version: 1\nname: Keg 42\nproperties:\n  malt: {v: 12.5, u: 0.05}\n  plato: 3.0\n---\n# Notes\n",
    );
    let mut sample = Sample::new();
    let mut malt = Property::stored(Value::absent());
    malt.set_presentation(Presentation {
        unit: Some("g".to_string()),
        ..Presentation::default()
    });
    sample.set_property(id("malt"), malt).unwrap();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(9.0))))
        .unwrap();

    let origin = document::load_into(&path, &mut sample).unwrap();
    assert_eq!(origin.path, path);
    let plato = sample.property(&id("plato")).unwrap();
    assert!(plato.is_computed());
    assert_eq!(plato.value().unwrap(), number(3.0));
    let malt = sample.property(&id("malt")).unwrap();
    assert_eq!(malt.presentation().unit.as_deref(), Some("g"));
    assert_eq!(
        malt.uncertainty().unwrap().map(|u| u.magnitude()),
        Some(0.05)
    );
    assert_eq!(sample.name(), Some("Keg 42"));
    assert_eq!(sample.note(), "# Notes\n");
}

#[test]
fn a_stale_filled_value_stays_stale_after_saving() {
    use samplekit::core::dependency_graph::Node;
    use samplekit::format::fingerprint::{self, Freshness};
    let scratch = Scratch::new("stale-filled");
    let path = scratch.at("keg.md");
    let declared = || {
        let mut sample = Sample::new();
        sample
            .set_property(id("malt"), Property::stored(Value::absent()))
            .unwrap();
        sample
            .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
            .unwrap();
        sample
            .declare_dependencies(&id("plato"), &[Node::Named(id("malt"))])
            .unwrap();
        sample
    };
    // Computed and recorded against a malt of 12.5.
    let mut computed = declared();
    computed
        .property(&id("malt"))
        .unwrap()
        .set_value(number(12.5));
    computed.property(&id("plato")).unwrap().value().unwrap();
    document::save_computed(&mut computed, &Destination::Path(path.clone())).unwrap();
    // The malt is corrected by hand, and nothing recomputes the plato.
    let edited = fs::read_to_string(&path).unwrap().replace("12.5", "13.5");
    fs::write(&path, edited).unwrap();

    let mut sample = declared();
    document::load_into(&path, &mut sample).unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();

    let (reloaded, _) = document::load_sample(&path).unwrap();
    let freshness = fingerprint::check_property(&reloaded, &id("plato")).unwrap();
    assert!(
        matches!(freshness, Freshness::Stale { .. }),
        "{freshness:?}\n{}",
        fs::read_to_string(&path).unwrap()
    );
}

/// Twice a malt, read through its handle.
struct Twice(samplekit::core::sample::PropertyHandle);

impl Compute for Twice {
    fn compute(&self) -> Result<Value, ComputeError> {
        match self.0.value()? {
            Value::Number(malt) => Ok(number(malt * 2.0)),
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
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    let twice = Twice(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("double"), Property::computed(Rc::new(twice)))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[Node::Named(id("malt"))])
        .unwrap();
    document::save_computed(&mut sample, &Destination::Path(scratch.at("c.md"))).unwrap();

    let double = sample.property(&id("double")).unwrap();
    assert!(double.is_computed());
    sample
        .property(&id("malt"))
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
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("plato"), Property::computed(formula.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[Node::Named(id("malt"))])
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
    sample.property(&id("plato")).unwrap().value().unwrap();
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(20.0));
    let path = scratch.at("u.md");
    let refused = document::save_computed(&mut sample, &Destination::Path(path.clone()));
    assert!(
        matches!(&refused, Err(DocumentError::Unrecordable { values }) if values == &["plato"]),
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
    sample.property(&id("plato")).unwrap().value().unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let edited = text.replace("v: 3.05", "v: 3.5");
    assert_ne!(edited, text);
    fs::write(&path, edited).unwrap();

    let (mut again, formula) = counted_sample();
    document::load_into(&path, &mut again).unwrap();
    let plato = again.property(&id("plato")).unwrap();
    assert!(plato.is_edited());
    assert_eq!(plato.value().unwrap(), number(3.5));
    assert!(matches!(
        fingerprint::check_property(&again, &id("plato")).unwrap(),
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
        .property(&id("plato"))
        .unwrap()
        .set_value(number(3.5));
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("fingerprint: {edited: "), "{written}");
    assert_eq!(formula.0.get(), 0);

    let (mut again, formula) = counted_sample();
    document::load_into(&path, &mut again).unwrap();
    let plato = again.property(&id("plato")).unwrap();
    assert!(plato.is_edited());
    assert_eq!(plato.value().unwrap(), number(3.5));
    assert_eq!(formula.0.get(), 0);

    // Computed, then overridden.
    again.property(&id("plato")).unwrap().restore_formula();
    again.property(&id("plato")).unwrap().value().unwrap();
    fingerprint_stamp(&mut again);
    again.property(&id("plato")).unwrap().set_value(number(4.5));
    document::save_computed(&mut again, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("computed: {v: {malt: "), "{written}");
    assert!(written.contains("fingerprint: {edited: "), "{written}");
    let (mut third, _) = counted_sample();
    document::load_into(&path, &mut third).unwrap();
    assert!(third.property(&id("plato")).unwrap().is_edited());
    assert_eq!(
        third.property(&id("plato")).unwrap().value().unwrap(),
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
        "---\nschema_version: 1\nproperties:\n  malt: {v: 12.5, fingerprint: \"000000000000\"}\n  plato:\n    v: 3.05\n    computed: {malt: 87c43f2ca298}\n---\n",
    );
    let (mut sample, _) = document::load_sample(&path).unwrap();
    sample
        .declare_dependencies(&id("plato"), &[Node::Named(id("malt"))])
        .unwrap();
    document::save_computed(&mut sample, &Destination::Path(path.clone())).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("malt: {v: 12.5, fingerprint: 87c43f2ca298}"),
        "{written}"
    );
}

#[test]
fn a_parse_error_gives_one_line_number() {
    let source = "---\nschema_version: 1\nname: x\nproperties:\n  malt: {v: [1, }\n---\n";
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
    let text = "---\nschema_version: 1\ntables:\n  conditioning:\n    index: day\n    columns: {day: {}}\n    rows:\n      - {day: {v: 1}}\n      - {day: {v: 1}}\n---\nnote\n";
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
        "keg.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  malt: {v: 4.0, readings: [1.0, 2.0, 9.0]}\n---\n",
    );
    let mut sample = Sample::new();
    let mut malt = Property::stored(Value::absent());
    malt.declare_statistics(Some(Location::Median), None);
    sample.set_property(id("malt"), malt).unwrap();
    document::load_into(&path, &mut sample).unwrap();
    let malt = sample.property(&id("malt")).unwrap();
    assert_eq!(malt.value().unwrap(), number(4.0));
    assert!(malt.peek(Property::holds_written_override));
    // Read without the model, it is the value, as it always read.
    let (plain, _) = document::load_sample(&path).unwrap();
    assert_eq!(
        plain.property(&id("malt")).unwrap().value().unwrap(),
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
        "---\nschema_version: 1\nproperties:\n  malt: {v: 2.0}\n---\n",
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
