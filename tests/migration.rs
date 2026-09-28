//! The tests of `migration`.

use std::fs;
use std::path::PathBuf;

use samplekit::format::migration::{self, Action, Alteration};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-mig-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Exactly what canonicalization writes.
const CANONICAL: &str =
    "---\nschema_version: 1\nproperties:\n  malt: {v: 12.5, unit: g}\n---\nA note.\n";

/// Valid, current, and not canonical: the fields are in another order.
const NONCANONICAL: &str =
    "---\nschema_version: 1\nproperties:\n  malt: {unit: g, v: 12.5}\n---\nA note.\n";

/// The previous implementation's shape. It says `schema_version: 1` and is not
/// this format; `unit_math` is what says so.
const LEGACY: &str = "---\nschema_version: 1\nproperties:\n  malt:\n    v: 12.5\n    unit: g\n    unit_math: \\mathrm{g}\n    precision: .3f\n    precision_unc: .1f\n---\nA brewing note.\n";

fn actions(plan: &migration::MigrationPlan) -> Vec<&Action> {
    plan.entries.iter().map(|entry| &entry.action).collect()
}

// -------------------------------------------------------------- planning

#[test]
fn plan_writes_nothing() {
    // Asserted against the filesystem, not against the code path.
    let scratch = Scratch::new("readonly");
    let paths: Vec<PathBuf> = [
        ("a.md", CANONICAL),
        ("b.md", LEGACY),
        ("c.md", NONCANONICAL),
    ]
    .iter()
    .map(|(name, body)| scratch.file(name, body))
    .collect();
    let before: Vec<(PathBuf, String, std::time::SystemTime)> = paths
        .iter()
        .map(|p| {
            (
                p.clone(),
                fs::read_to_string(p).unwrap(),
                fs::metadata(p).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let listing_before = fs::read_dir(&scratch.0).unwrap().count();

    let _ = migration::plan(&paths);

    for (path, contents, modified) in before {
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), listing_before);
}

#[test]
fn plan_reports_no_change_for_canonical_files() {
    let scratch = Scratch::new("nochange");
    let plan = migration::plan(&[scratch.file("a.md", CANONICAL)]);
    assert_eq!(actions(&plan), [&Action::NoChange]);
    assert_eq!(plan.entries[0].from_version, 1);
}

#[test]
fn plan_reports_canonicalize_for_valid_noncanonical_files() {
    // Reordered keys are detected without being rewritten.
    let scratch = Scratch::new("canon");
    let path = scratch.file("a.md", NONCANONICAL);
    let plan = migration::plan(std::slice::from_ref(&path));
    assert_eq!(actions(&plan), [&Action::Canonicalize]);
    assert_eq!(fs::read_to_string(&path).unwrap(), NONCANONICAL);
}

#[test]
fn a_plan_names_what_it_would_alter() {
    // Which field is dropped or renamed, and why.
    let scratch = Scratch::new("names");
    let plan = migration::plan(&[scratch.file("a.md", LEGACY)]);
    assert_eq!(actions(&plan), [&Action::Upgrade]);
    let alterations = &plan.entries[0].alterations;
    assert!(alterations.iter().any(|a| matches!(
        a,
        Alteration::Dropped { field, .. } if field == "unit_math"
    )));
    // A precision is the project's: both halves go, and say where.
    assert!(alterations.iter().any(|a| matches!(
        a,
        Alteration::Dropped { field, because }
            if field.contains("precision_unc") && because.contains("[property.*]")
    )));
    // Each says what and why, because a plan is read before it is applied.
    let drawn: Vec<String> = alterations.iter().map(ToString::to_string).collect();
    assert!(
        drawn.iter().any(|line| line.contains(".samplekitrc")),
        "{drawn:?}"
    );
    // And a pre-1 file is `from` zero, whatever its version line claims.
    assert_eq!(plan.entries[0].from_version, 0);
}

#[test]
fn plan_records_blocked_files_without_aborting() {
    // An unreadable file among readable ones is reported, and the rest planned.
    let scratch = Scratch::new("blocked");
    let paths = vec![
        scratch.file("a.md", CANONICAL),
        scratch.file("bad.md", "not a sample at all\n"),
        scratch.file("c.md", NONCANONICAL),
    ];
    let plan = migration::plan(&paths);
    assert_eq!(plan.entries.len(), 3);
    assert_eq!(plan.entries[0].action, Action::NoChange);
    assert!(matches!(plan.entries[1].action, Action::Blocked { .. }));
    assert_eq!(plan.entries[2].action, Action::Canonicalize);
}

#[test]
fn a_renamed_field_alone_marks_a_legacy_file() {
    // A pre-1 file that measured everything directly carries no `unit_math`,
    // no `symbol_math` and no `precision_unc`. It carries `data`.
    let scratch = Scratch::new("renamed-only");
    let path = scratch.file(
        "d.md",
        "---\nschema_version: 1\nproperties:\n  malt:\n    v: 12.5\n    \
         data: [12.4, 12.6]\n---\nA note.\n",
    );
    let plan = migration::plan(std::slice::from_ref(&path));
    assert_eq!(
        plan.entries[0].action,
        Action::Upgrade,
        "{:?}",
        plan.entries[0]
    );
    assert!(
        plan.entries[0].alterations.iter().any(
            |a| matches!(a, Alteration::Renamed { from, to } if from == "data" && to == "readings")
        ),
        "{:?}",
        plan.entries[0].alterations
    );
}

#[test]
fn a_legacy_field_in_the_note_is_not_a_legacy_file() {
    // The note is evidence about the researcher, not about the format.
    let scratch = Scratch::new("note-says-legacy");
    let path = scratch.file(
        "n.md",
        "---\nschema_version: 1\nproperties:\n  malt: {v: 12.5, bogus: 1}\n---\n\
         We dropped precision_unc: it duplicated precision.\n",
    );
    let plan = migration::plan(std::slice::from_ref(&path));
    assert!(
        matches!(plan.entries[0].action, Action::Blocked { .. }),
        "{:?}",
        plan.entries[0].action
    );
}

// -------------------------------------------------------------- applying

#[test]
fn plan_counts_the_comments_a_write_drops() {
    let scratch = Scratch::new("comments");
    let path = scratch.file(
        "a.md",
        "---\nschema_version: 1\nname: A # the first\n# a remark\nbatch: 1\n---\nN.\n",
    );
    let plan = migration::plan(std::slice::from_ref(&path));
    let said: Vec<String> = plan.entries[0]
        .alterations
        .iter()
        .map(|alteration| alteration.to_string())
        .collect();
    assert!(
        said.iter().any(|line| line.contains("2 YAML comments")),
        "{said:?}"
    );
}

#[test]
fn a_plan_repairs_an_unusable_tag() {
    let scratch = Scratch::new("tag-repair");
    let path = scratch.file(
        "a.md",
        "---\nschema_version: 1\ntags: [reference, \"my tag\"]\n---\nA note.\n",
    );
    let plan = migration::plan(std::slice::from_ref(&path));
    assert_eq!(actions(&plan), [&Action::Canonicalize]);
    assert!(
        plan.entries[0]
            .alterations
            .iter()
            .any(|alteration| matches!(alteration, Alteration::Renamed { to, .. } if to.contains("my_tag"))),
        "{:?}",
        plan.entries[0].alterations
    );
    // And the file is untouched: this module reads.
    assert!(
        fs::read_to_string(&path).unwrap().contains("\"my tag\""),
        "the plan wrote"
    );
}

#[test]
fn an_earlier_configuration_is_recognised_and_not_converted() {
    // Said where it is met, converted by nobody.
    for earlier in [
        "[model]\npython = \"model.py:Sample\"\n",
        "python = \"model.py:Sample\"\n",
        "[views.identity]\nfields = [\"name\"]\n",
        "[queries.q]\nfilter = \"malt > 1\"\n",
    ] {
        assert!(migration::is_earlier_configuration(earlier), "{earlier}");
    }
    let current =
        "schema_version = 1\n[model]\npath = \"model.py\"\n[query.q]\nfilter = \"malt > 1\"\n";
    assert!(!migration::is_earlier_configuration(current));
    assert!(!migration::is_earlier_configuration("not = toml ="));
}
