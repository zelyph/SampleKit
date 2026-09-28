//! The tests of `tagging`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::sample_list as list;
use samplekit::collection::tagging::{self, Edit};
use samplekit::core::identifier::Identifier;

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn ids(names: &[&str]) -> Vec<Identifier> {
    names.iter().map(|name| id(name)).collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-tagging-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    /// A sample whose note would not survive a reformatting.
    fn sample(&self, file: &str, tags: &str) -> PathBuf {
        let path = self.0.join(file);
        fs::write(
            &path,
            format!(
                "---\nschema_version: 1\nname: {}\ntags: [{tags}]\nproperties:\n  \
                 malt: {{v: 12.5, unit: g}}\n---\n\n#  Notes \n\ntrailing spaces  \n",
                file.trim_end_matches(".md")
            ),
        )
        .unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn add_appends_and_is_idempotent() {
    assert_eq!(
        tagging::retagged(&ids(&["a"]), &Edit::Add(id("b"))),
        ids(&["a", "b"])
    );
    assert_eq!(
        tagging::retagged(&ids(&["a"]), &Edit::Add(id("a"))),
        ids(&["a"])
    );
}

#[test]
fn remove_of_an_absent_tag_changes_nothing() {
    let scratch = Scratch::new("remove-absent");
    let path = scratch.sample("a.md", "kept");
    let collection = list::from_directory(&scratch.0).unwrap();
    let plan = tagging::plan(&collection, Edit::Remove(id("ghost")));
    assert!(plan.changes.is_empty(), "{:?}", plan.changes);
    assert_eq!(plan.unchanged, [path]);
}

#[test]
fn rename_is_one_pass_and_keeps_the_position() {
    let edit = Edit::Rename {
        from: id("old"),
        to: id("new"),
    };
    assert_eq!(
        tagging::retagged(&ids(&["x", "old", "y"]), &edit),
        ids(&["x", "new", "y"])
    );
}

#[test]
fn rename_drops_a_resulting_duplicate() {
    let edit = Edit::Rename {
        from: id("old"),
        to: id("new"),
    };
    assert_eq!(
        tagging::retagged(&ids(&["new", "old"]), &edit),
        ids(&["new"])
    );
}

#[test]
fn a_plan_modifies_nothing() {
    let scratch = Scratch::new("plan");
    let path = scratch.sample("a.md", "kept");
    let before = fs::read(&path).unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let plan = tagging::plan(&collection, Edit::Add(id("reference")));
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].before, ids(&["kept"]));
    assert_eq!(plan.changes[0].after, ids(&["kept", "reference"]));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn apply_writes_the_changes_and_keeps_the_note() {
    let scratch = Scratch::new("apply");
    let path = scratch.sample("a.md", "kept");
    let collection = list::from_directory(&scratch.0).unwrap();
    let plan = tagging::plan(&collection, Edit::Add(id("reference")));
    assert_eq!(tagging::apply(&plan).unwrap(), std::slice::from_ref(&path));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("tags: [kept, reference]"), "{text}");
    // The note byte for byte, its doubled space and trailing spaces included.
    assert!(
        text.ends_with("---\n\n#  Notes \n\ntrailing spaces  \n"),
        "{text:?}"
    );
}

#[test]
fn apply_starts_from_what_the_file_holds_now() {
    // A tag another hand added between the plan and the write is kept: the
    // edit is what was asked for, not the plan's picture of the file.
    let scratch = Scratch::new("apply-now");
    let path = scratch.sample("a.md", "kept");
    let collection = list::from_directory(&scratch.0).unwrap();
    let plan = tagging::plan(&collection, Edit::Add(id("reference")));
    let by_hand = fs::read_to_string(&path)
        .unwrap()
        .replace("tags: [kept]", "tags: [kept, by_hand]");
    fs::write(&path, by_hand).unwrap();
    tagging::apply(&plan).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("tags: [kept, by_hand, reference]"), "{text}");
}

#[test]
fn a_failure_names_the_files_already_written() {
    let scratch = Scratch::new("failure");
    let first = scratch.sample("a.md", "kept");
    let second = scratch.sample("b.md", "kept");
    let third = scratch.sample("c.md", "kept");
    let collection = list::from_directory(&scratch.0).unwrap();
    let plan = tagging::plan(&collection, Edit::Add(id("reference")));
    assert_eq!(plan.changes.len(), 3);
    // The second file stops being a sample before it is written.
    fs::write(&second, "---\nschema_version: 1\nname: [\n---\n").unwrap();
    let untouched = fs::read(&third).unwrap();
    let error = tagging::apply(&plan).unwrap_err();
    assert_eq!(error.path, second);
    assert_eq!(error.written, std::slice::from_ref(&first));
    assert!(error.to_string().contains("a.md"), "{error}");
    assert!(
        fs::read_to_string(&first)
            .unwrap()
            .contains("tags: [kept, reference]")
    );
    assert_eq!(fs::read(&third).unwrap(), untouched);
}
