//! The tests of `configuration_edit`.

use std::fs;
use std::path::PathBuf;

use samplekit::config::configuration_edit::{self, ConfigurationEdit, EditError, Kind};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "samplekit-config-edit-{name}-{}",
                std::process::id()
            ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn rc(&self, text: &str) -> PathBuf {
        let path = self.0.join(".samplekitrc");
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const WRITTEN: &str = "\
schema_version = 1

# The cells that passed.
[query.approved]
filter = 'status == \"approved\"'   # set by the brewery

[profile.old]
columns = [{field = \"malt\"}]

[unit.\"g/L\"]
plain = \"g/L\"
";

#[test]
fn what_was_not_touched_is_kept_as_written() {
    let scratch = Scratch::new("kept");
    let path = scratch.rc(WRITTEN);
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    edit.set(Kind::Query, "approved", "filter", "status == \"kept\"");
    edit.set(Kind::Query, "strong", "filter", "brix > 3");
    assert!(edit.remove(Kind::Profile, "old"));
    edit.set_setting("render", "figure_style", "plain");
    let text = edit.text();
    assert!(
        text.contains("# The cells that passed.\n[query.approved]"),
        "{text}"
    );
    assert!(
        text.contains("filter = 'status == \"kept\"'   # set by the brewery"),
        "{text}"
    );
    assert!(
        text.contains("[query.strong]\nfilter = \"brix > 3\""),
        "{text}"
    );
    assert!(!text.contains("[profile"), "{text}");
    assert!(text.contains("[unit.\"g/L\"]\nplain = \"g/L\""), "{text}");
    assert_eq!(edit.names(Kind::Query), ["approved", "strong"]);
    let config = edit.write().unwrap();
    assert_eq!(config.query_names(), ["approved", "strong"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn an_edit_that_would_not_load_is_not_written() {
    let scratch = Scratch::new("refused");
    let path = scratch.rc(WRITTEN);
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    edit.set(Kind::Figure, "f", "kind", "pie");
    edit.set(Kind::Figure, "f", "x", "a");
    edit.set(Kind::Figure, "f", "y", "b");
    let refused = edit.write().unwrap_err();
    assert!(matches!(refused, EditError::Refused(_)), "{refused}");
    assert!(refused.to_string().contains("'pie'"), "{refused}");
    assert_eq!(fs::read_to_string(&path).unwrap(), WRITTEN);
}

#[test]
fn a_file_changed_since_it_was_read_is_not_overwritten() {
    let scratch = Scratch::new("changed");
    let path = scratch.rc(WRITTEN);
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    edit.set(Kind::Query, "strong", "filter", "brix > 3");
    fs::write(
        &path,
        format!("{WRITTEN}\n[query.theirs]\nfilter = \"a > 1\"\n"),
    )
    .unwrap();
    assert!(matches!(
        edit.write().unwrap_err(),
        EditError::Changed { .. }
    ));
    assert!(fs::read_to_string(&path).unwrap().contains("theirs"));
}

#[test]
fn a_file_made_since_it_was_looked_for_is_not_overwritten() {
    // A `.samplekitrc` that was not there when the edit began, and that another
    // writer made since, is theirs: read again as it is written, and refused,
    // as a sample written since is.
    let scratch = Scratch::new("made-since");
    let path = scratch.0.join(".samplekitrc");
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    assert!(edit.read_text().is_none());
    edit.set(Kind::Query, "strong", "filter", "brix > 3");
    fs::write(&path, "schema_version = 1\n# theirs\n").unwrap();
    assert!(matches!(
        edit.write().unwrap_err(),
        EditError::Changed { .. }
    ));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "schema_version = 1\n# theirs\n"
    );
}

#[test]
#[cfg(unix)]
fn an_edit_writes_through_a_linked_configuration() {
    // A `.samplekitrc` linked from a shared folder was replaced by a copy of
    // its own, which the next edit of the shared one no longer reached.
    let scratch = Scratch::new("linked");
    fs::create_dir_all(scratch.0.join("shared")).unwrap();
    fs::create_dir_all(scratch.0.join("project")).unwrap();
    let shared = scratch.0.join("shared/brewery.samplekitrc");
    fs::write(&shared, WRITTEN).unwrap();
    let link = scratch.0.join("project/.samplekitrc");
    std::os::unix::fs::symlink("../shared/brewery.samplekitrc", &link).unwrap();
    let mut edit = ConfigurationEdit::open(&link).unwrap();
    edit.set(Kind::Query, "strong", "filter", "brix > 3");
    edit.write().unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(&shared)
            .unwrap()
            .contains("[query.strong]")
    );
}

#[test]
fn the_changes_are_the_lines_that_move() {
    let scratch = Scratch::new("changes");
    let path = scratch.rc(WRITTEN);
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    edit.set(Kind::Query, "strong", "filter", "brix > 3");
    let changes = edit.changes();
    assert!(
        changes
            .iter()
            .any(|line| line.starts_with('+') && line.ends_with("[query.strong]")),
        "{changes:?}"
    );
    assert!(
        changes.iter().all(|line| line.starts_with('+')),
        "{changes:?}"
    );
    let fresh = ConfigurationEdit::open(&scratch.0.join("new.samplekitrc")).unwrap();
    assert_eq!(fresh.text(), "schema_version = 1\n");
}

#[test]
fn settings_are_read_back_and_nested_by_their_dots() {
    let dir = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-edit-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".samplekitrc");
    std::fs::write(&path, "schema_version = 1\n").unwrap();
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    edit.set_setting(
        "workbench.colors",
        "outdated",
        configuration_edit::value_of("#ff8800").unwrap(),
    );
    let text = edit.text();
    assert!(text.contains("[workbench.colors]"), "{text}");
    assert!(!text.contains("[workbench]\n"), "{text}");
    assert_eq!(
        edit.settings("workbench.colors"),
        [("outdated".to_string(), "\"#ff8800\"".to_string())]
    );
    edit.checked().unwrap();
    assert!(edit.unset_setting("workbench.colors", "outdated"));
    assert!(edit.settings("workbench.colors").is_empty());
    edit.set(
        Kind::Query,
        "approved",
        "filter",
        configuration_edit::value_of("status == \"approved\"").unwrap(),
    );
    assert_eq!(
        edit.entries(Kind::Query, "approved"),
        [("filter".to_string(), "'status == \"approved\"'".to_string())]
    );
    assert!(configuration_edit::value_of("12").unwrap().as_integer() == Some(12));
    assert!(configuration_edit::value_of("approved").unwrap().as_str() == Some("approved"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_value_is_read_without_its_comment_and_inline_tables_are_sections() {
    let dir = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-edit-inline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".samplekitrc");
    std::fs::write(
        &path,
        "schema_version = 1\n[render]\nstyle = \"plain\"   # the default\n\
         [workbench]\ncolors = { failed = \"red\" }   # mine\n[matplotlib]\nfont.size = 13\n\
         [query]\napproved = { filter = \"status == approved\" }\n",
    )
    .unwrap();
    let mut edit = ConfigurationEdit::open(&path).unwrap();
    // The comment beside a value is not the value, and stays where it was.
    assert_eq!(
        edit.settings("render"),
        [("style".to_string(), "\"plain\"".to_string())]
    );
    let typed = edit.settings("render")[0].1.clone();
    edit.set_setting(
        "render",
        "style",
        configuration_edit::value_of(&typed).unwrap(),
    );
    assert!(
        edit.text().contains("style = \"plain\"   # the default"),
        "{}",
        edit.text()
    );
    // Inline tables read, and written as sections once changed.
    assert_eq!(
        edit.settings("workbench.colors"),
        [("failed".to_string(), "\"red\"".to_string())]
    );
    assert_eq!(edit.entries(Kind::Query, "approved").len(), 1);
    assert!(edit.set_setting(
        "workbench.colors",
        "outdated",
        configuration_edit::value_of("yellow").unwrap()
    ));
    // Made a section: its header without the space before `=`, and the
    // comment beside it above it.
    let text = edit.text();
    assert!(text.contains("# mine\n[workbench.colors]\n"), "{text}");
    assert!(text.contains("outdated = \"yellow\""), "{text}");
    // A dotted key by its whole name, changed where the file dots it.
    assert_eq!(
        edit.settings("matplotlib"),
        [("font.size".to_string(), "13".to_string())]
    );
    assert!(edit.set_setting(
        "matplotlib",
        "font.size",
        configuration_edit::value_of("11").unwrap()
    ));
    assert!(edit.text().contains("font.size = 11"), "{}", edit.text());
    edit.checked().unwrap();
    assert!(edit.unset_setting("matplotlib", "font.size"));
    // Something other than a table under a section's name is refused.
    assert!(!edit.set_setting(
        "schema_version",
        "x",
        configuration_edit::value_of("1").unwrap()
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_value_typed_half_a_list_or_with_a_comment_is_refused() {
    for typed in ["[7, 5", "{ a = 1", "14 # bigger"] {
        assert!(configuration_edit::value_of(typed).is_err(), "{typed}");
    }
    assert!(
        configuration_edit::value_of("[7, 5]")
            .unwrap()
            .as_array()
            .is_some()
    );
    assert_eq!(
        configuration_edit::value_of("#ff8800").unwrap().as_str(),
        Some("#ff8800")
    );
}
