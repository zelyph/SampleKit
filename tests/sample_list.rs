//! The tests of `sample_list`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::sample_list::{self as list, FieldWarning, ListError};
use samplekit::core::identifier::Identifier;
use samplekit::core::property::Property;
use samplekit::core::sample::Sample;
use samplekit::core::value::Value;
use samplekit::query::field_addressing as fields;
use samplekit::query::filter_language as filter;
use samplekit::query::ordering::{SortKey, SortSpec};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-list-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    /// A sample file with a name, a status and an optional plato.
    fn sample(&self, file: &str, name: &str, status: &str, plato: Option<f64>) -> PathBuf {
        let mut body = format!("---\nschema_version: 1\nname: {name}\nstatus: {status}\n");
        body.push_str("properties:\n  malt: {v: 12.5, unit: g}\n");
        if let Some(plato) = plato {
            body.push_str(&format!("  plato: {{v: {plato}, unit: g/L}}\n"));
        }
        body.push_str("---\nBrewing notes for ");
        body.push_str(name);
        body.push_str(".\n");
        let path = self.0.join(file);
        fs::write(&path, body).unwrap();
        path
    }

    fn raw(&self, file: &str, body: &str) -> PathBuf {
        let path = self.0.join(file);
        fs::write(&path, body).unwrap();
        path
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Three casks: two approved, one reviewed, one of them unmeasured.
fn corpus(scratch: &Scratch) {
    scratch.sample("a.md", "A", "approved", Some(2.8));
    scratch.sample("b.md", "B", "reviewed", Some(1.2));
    scratch.sample("c.md", "C", "approved", None);
}

fn names(collection: &list::SampleList) -> Vec<String> {
    collection
        .iter()
        .map(|entry| entry.sample.borrow().name().unwrap().to_string())
        .collect()
}

fn spec(path: &str, descending: bool) -> SortSpec {
    SortSpec::new(vec![SortKey {
        field: fields::parse(path).unwrap(),
        descending,
    }])
    .unwrap()
}

// ------------------------------------------------------------- construction

#[test]
fn loads_a_directory_in_deterministic_order() {
    // Two loads agree.
    let scratch = Scratch::new("order");
    corpus(&scratch);
    let first = names(&list::from_directory(&scratch.0).unwrap());
    assert_eq!(first, ["A", "B", "C"]);
    for _ in 0..3 {
        assert_eq!(names(&list::from_directory(&scratch.0).unwrap()), first);
    }
}

#[test]
fn loading_does_not_execute_project_python() {
    // A model whose import is observable is not imported: the declared file
    // does not even exist, and the load succeeds.
    let scratch = Scratch::new("no-python");
    corpus(&scratch);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 3);
    let config = collection.config().unwrap();
    assert!(config.model().is_some());
    assert!(!config.resolve("model.py").exists());
}

#[test]
fn skipped_files_are_reportable_after_loading() {
    // The `--verbose` case: a scan that quietly omits files produces a query
    // over fewer samples than the user thinks.
    let scratch = Scratch::new("skipped");
    corpus(&scratch);
    scratch.raw("README.md", "# Notes\n");
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 3);
    assert_eq!(collection.skipped().len(), 1);
    assert!(collection.skipped()[0].path.ends_with("README.md"));
}

// ------------------------------------------------------- selection, order

#[test]
fn filter_selects_matching_samples() {
    let scratch = Scratch::new("filter");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let parsed = filter::parse("status == approved").unwrap();
    assert_eq!(names(&collection.filter(&parsed).unwrap()), ["A", "C"]);
}

#[test]
fn filter_shares_samples_with_the_parent() {
    // An edit through the subset is visible in the parent. This is why an
    // `Entry` holds an `Rc` and not a `Sample`.
    let scratch = Scratch::new("shared");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let parsed = filter::parse("status == reviewed").unwrap();
    let subset = collection.filter(&parsed).unwrap();
    assert_eq!(subset.len(), 1);

    subset
        .get(0)
        .unwrap()
        .sample
        .borrow_mut()
        .set_name(Some("edited".to_string()));
    assert_eq!(names(&collection), ["A", "edited", "C"]);
}

#[test]
fn filter_chains() {
    let scratch = Scratch::new("chain");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let approved = collection
        .filter(&filter::parse("status == approved").unwrap())
        .unwrap();
    let measured = approved
        .filter(&filter::parse("plato is present").unwrap())
        .unwrap();
    assert_eq!(names(&measured), ["A"]);
}

#[test]
fn sort_mutates_and_sorted_does_not() {
    // Both halves of the pair, following `list.sort()` and `sorted()`.
    let scratch = Scratch::new("sort-pair");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let by_plato = spec("plato", true);

    let copy = collection.sorted(&by_plato).unwrap();
    assert_eq!(names(&copy), ["A", "B", "C"]);
    assert_eq!(names(&collection), ["A", "B", "C"]);

    collection.sort(&spec("plato", false)).unwrap();
    assert_eq!(names(&collection), ["B", "A", "C"]);
}

#[test]
fn a_narrowed_list_keeps_the_vocabulary_it_came_from() {
    // A filter does not change what exists. Sorting the samples it kept by a
    // field only the others hold is a sort of absent values; a name no sample
    // of the collection holds is still unknown.
    let scratch = Scratch::new("narrowed-vocabulary");
    scratch.sample("a.md", "A", "approved", Some(2.8));
    scratch.sample("b.md", "B", "draft", None);
    let collection = list::from_directory(&scratch.0).unwrap();
    let kept = collection
        .filter(&filter::parse("name == \"B\"").unwrap())
        .unwrap();
    assert_eq!(kept.len(), 1);
    let plato = samplekit::query::ordering::parse_spec(&["plato".to_string()]).unwrap();
    assert_eq!(kept.sorted(&plato).unwrap().len(), 1);
    assert!(
        kept.vocabulary()
            .has_name(&Identifier::new("plato").unwrap())
    );
    // Narrowed twice, it is still the collection's.
    let again = kept.filter(&filter::parse("malt > 1").unwrap()).unwrap();
    assert!(again.sorted(&plato).is_ok());
    let ghost = samplekit::query::ordering::parse_spec(&["plto".to_string()]).unwrap();
    assert!(matches!(kept.sorted(&ghost), Err(ListError::Order(_))));
}

#[test]
fn failed_sort_leaves_order_unchanged() {
    // Propagated from `ordering`, where validation precedes extraction.
    let scratch = Scratch::new("sort-fail");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let before = names(&collection);
    let error = collection.sort(&spec("vfi", false)).unwrap_err();
    assert!(matches!(error, ListError::Order(_)), "{error:?}");
    assert_eq!(names(&collection), before);
}

#[test]
fn a_shared_sample_is_not_copied_by_sorting() {
    // Reordering moves handles, so an edit after a sort is still visible.
    let scratch = Scratch::new("sort-shares");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let held = collection.get(0).unwrap().sample.clone();
    collection.sort(&spec("plato", true)).unwrap();
    held.borrow_mut().set_name(Some("moved".to_string()));
    assert!(names(&collection).contains(&"moved".to_string()));
}

// ---------------------------------------------------------------- reading

#[test]
fn values_returns_one_entry_per_sample() {
    // Including `None` for absent, in collection order.
    let scratch = Scratch::new("values");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let values = collection.values(&fields::parse("plato").unwrap()).unwrap();
    assert_eq!(values, [Some(number(2.8)), Some(number(1.2)), None]);
}

#[test]
fn summarize_ignores_absent_values() {
    // And reports the count it used.
    let scratch = Scratch::new("summarize");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let summary = collection
        .summarize(&fields::parse("plato").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(summary.summary.count.get(), 2);
    assert!((summary.summary.mean - 2.0).abs() < 1e-9);
}

#[test]
fn summarize_reports_what_it_skipped() {
    // Fifty-eight platos out of seventy casks reads as `58 / 70`.
    let scratch = Scratch::new("denominator");
    for n in 0..70 {
        let plato = if n < 58 { Some(2.0) } else { None };
        scratch.sample(&format!("s{n:02}.md"), &format!("S{n}"), "approved", plato);
    }
    let collection = list::from_directory(&scratch.0).unwrap();
    let summary = collection
        .summarize(&fields::parse("plato").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(summary.summary.count.get(), 58);
    assert_eq!(summary.considered, 70);
}

#[test]
fn summarize_empty_column_yields_none() {
    // "Nothing to summarize here" is distinct from a zero-count summary, which
    // cannot occur — and distinct again from an unknown field, which is an
    // error. A textual column is known and has no numbers.
    let scratch = Scratch::new("empty-column");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    assert!(
        collection
            .summarize(&fields::parse("status").unwrap())
            .unwrap()
            .is_none()
    );
    // And a name no sample has stays an error rather than an empty summary.
    assert!(
        collection
            .summarize(&fields::parse("vfi").unwrap())
            .is_err()
    );
}

// ----------------------------------------------------------------- saving

#[test]
fn save_all_checks_every_destination_first() {
    // One conflict writes nothing: a batch of two hundred either runs or does
    // not, because half old and half new is the state hardest to recover from.
    let scratch = Scratch::new("save-check");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let out = scratch.dir("out");
    fs::write(out.join("b.md"), "in the way").unwrap();

    let error = collection.save_all(&out, false).unwrap_err();
    assert!(
        matches!(error, ListError::AlreadyExists { .. }),
        "{error:?}"
    );
    // Nothing was written, including the samples whose names were free.
    assert!(!out.join("a.md").exists());
    assert!(!out.join("c.md").exists());
    assert_eq!(fs::read_to_string(out.join("b.md")).unwrap(), "in the way");
}

#[test]
fn save_all_reports_every_conflict() {
    // Not just the first: clearing conflicts one run at a time is the same
    // defect as one-error-at-a-time diagnostics.
    let scratch = Scratch::new("save-conflicts");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let out = scratch.dir("out");
    fs::write(out.join("a.md"), "x").unwrap();
    fs::write(out.join("c.md"), "x").unwrap();

    let ListError::AlreadyExists { paths } = collection.save_all(&out, false).unwrap_err() else {
        panic!("expected a conflict report");
    };
    assert_eq!(paths.len(), 2);
    let message = ListError::AlreadyExists { paths }.to_string();
    assert!(
        message.contains("a.md") && message.contains("c.md"),
        "{message}"
    );
}

#[test]
fn save_all_refuses_two_samples_of_one_file_name() {
    // `a/S1.md` and `b/S1.md` into one directory were one file, the second
    // written over the first — with `overwrite` too, since neither existed.
    let scratch = Scratch::new("save-same-name");
    scratch.dir("a");
    scratch.dir("b");
    let first = scratch.sample("a/S1.md", "A1", "ok", None);
    let second = scratch.sample("b/S1.md", "B1", "ok", None);
    let mut collection = list::from_files(&[first.clone(), second.clone()]).unwrap();
    let out = scratch.dir("out");
    for overwrite in [false, true] {
        let error = collection.save_all(&out, overwrite).unwrap_err();
        let ListError::SameDestination { shared } = &error else {
            panic!("expected one destination refused, got {error:?}");
        };
        assert_eq!(
            shared,
            &[(out.join("S1.md"), vec![first.clone(), second.clone()])]
        );
        assert!(error.to_string().contains("nothing was written"), "{error}");
        assert!(!out.join("S1.md").exists());
    }
}

#[test]
fn save_all_with_overwrite_replaces() {
    // The opt-in path.
    let scratch = Scratch::new("save-overwrite");
    corpus(&scratch);
    let mut collection = list::from_directory(&scratch.0).unwrap();
    let out = scratch.dir("out");
    fs::write(out.join("a.md"), "in the way").unwrap();

    let written = collection.save_all(&out, true).unwrap();
    assert_eq!(written.len(), 3);
    assert!(
        fs::read_to_string(out.join("a.md"))
            .unwrap()
            .contains("schema_version")
    );
}

#[test]
fn save_preserves_every_note() {
    // Across a whole batch: the researcher's text is the thing that must
    // survive every operation.
    let scratch = Scratch::new("save-notes");
    corpus(&scratch);
    let before: Vec<String> = ["a.md", "b.md", "c.md"]
        .iter()
        .map(|name| {
            let text = fs::read_to_string(scratch.0.join(name)).unwrap();
            text[text.find("\n---\n").unwrap() + 5..].to_string()
        })
        .collect();

    let mut collection = list::from_directory(&scratch.0).unwrap();
    collection.save_each().unwrap();

    let after: Vec<String> = ["a.md", "b.md", "c.md"]
        .iter()
        .map(|name| {
            let text = fs::read_to_string(scratch.0.join(name)).unwrap();
            text[text.find("\n---\n").unwrap() + 5..].to_string()
        })
        .collect();
    assert_eq!(before, after);
}

#[test]
fn in_memory_sample_without_a_path_cannot_save_each() {
    // With its position named.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(1.0)))
        .unwrap();
    let mut collection = list::from_samples(vec![sample]);
    let error = collection.save_each().unwrap_err();
    let ListError::MissingPath { position } = error else {
        panic!("{error:?}");
    };
    assert_eq!(position, 0);
}

// ------------------------------------------------- checking a declaration

#[test]
fn a_property_missing_from_one_sample_warns_about_nothing() {
    // Absent is data; only unknown is a mistake. `plato` is missing from C
    // and present in A, so a profile naming it is correct.
    let scratch = Scratch::new("check-absent");
    corpus(&scratch);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"plato\"}]\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let config = collection.config().unwrap();
    assert!(list::check_profile(&collection, config.profile("p").unwrap()).is_empty());
}

#[test]
fn a_bare_table_is_not_reported_as_an_unknown_field() {
    let scratch = Scratch::new("check-bare-table");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"measurements\"}]\n",
    )
    .unwrap();
    scratch.raw(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\ntables:\n  measurements:\n    index: [T]\n    columns:\n      T: {}\n      ph: {}\n    rows:\n      - {T: 20, ph: 4.2}\n---\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let warnings = list::check_profile(
        &collection,
        collection.config().unwrap().profile("p").unwrap(),
    );
    assert_eq!(
        warnings,
        vec![FieldWarning::TableNeedsCell {
            table: "measurements".to_string(),
        }]
    );
}

#[test]
fn a_table_name_with_its_separator_still_asks_for_a_cell() {
    let scratch = Scratch::new("check-table-separator");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"measurements.\"}]\n",
    )
    .unwrap();
    scratch.raw(
        "a.sample.md",
        "---\nschema_version: 1\ntables:\n  measurements:\n    index: [T]\n    columns:\n      T: {}\n      ph: {}\n    rows:\n      - {T: 20, ph: 4.2}\n---\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let warnings = list::check_profile(
        &collection,
        collection.config().unwrap().profile("p").unwrap(),
    );
    assert_eq!(
        warnings,
        vec![FieldWarning::TableNeedsCell {
            table: "measurements".to_string(),
        }]
    );
}

#[test]
fn files_find_their_project() {
    // A file addressed on its own resolves its units and precision exactly as
    // it does inside its collection — `from_paths` has no project, and
    // `from_files` finds the one above it.
    let scratch = Scratch::new("from-files");
    scratch.raw(
        ".samplekitrc",
        "schema_version = 1\n[property.malt]\nunit = \"g\"\n",
    );
    let path = scratch.sample("a.md", "A", "approved", Some(2.8));
    assert!(
        list::from_paths(std::slice::from_ref(&path))
            .unwrap()
            .config()
            .is_none()
    );
    let found = list::from_files(&[path]).unwrap();
    let config = found.config().expect("the project above the file");
    assert_eq!(
        config.property("malt").and_then(|p| p.unit.as_deref()),
        Some("g")
    );
    assert_eq!(found.len(), 1);
}

#[test]
fn a_list_built_from_entries_shares_their_samples() {
    use samplekit::collection::sample_list::Entry;
    use std::cell::RefCell;
    use std::rc::Rc;
    let shared = Rc::new(RefCell::new(Sample::new()));
    shared
        .borrow_mut()
        .set_property(id("malt"), Property::stored(number(1.0)))
        .unwrap();
    let list = list::from_entries(vec![Entry {
        path: None,
        sample: shared.clone(),
        states: None,
    }]);
    shared
        .borrow()
        .property(&id("malt"))
        .unwrap()
        .set_value(number(2.0));
    let values = list.values(&fields::parse("malt").unwrap()).unwrap();
    assert_eq!(values, vec![Some(number(2.0))]);
    assert!(Rc::ptr_eq(&list.get(0).unwrap().sample, &shared));
}

#[test]
fn a_given_configuration_replaces_the_nearest_one() {
    let scratch = Scratch::new("given-configuration");
    corpus(&scratch);
    scratch.raw(".samplekitrc", "schema_version = 1\n");
    let other = scratch.dir("elsewhere").join("other.samplekitrc");
    fs::write(
        &other,
        "schema_version = 1\n[query.approved]\nfilter = 'status == \"approved\"'\n\
         [collection]\nexclude = [\"c.md\"]\n",
    )
    .unwrap();
    let given = samplekit::config::project_config::load(&other).unwrap();

    let nearest = list::from_directory(&scratch.0).unwrap();
    assert!(nearest.config().unwrap().query("approved").is_err());
    assert_eq!(names(&nearest).len(), 3);

    let forced = list::from_directory_with(&scratch.0, Some(given.clone())).unwrap();
    assert!(forced.config().unwrap().query("approved").is_ok());
    assert_eq!(names(&forced), ["A", "B"]);

    let file = list::from_files_with(&[scratch.0.join("a.md")], Some(given)).unwrap();
    assert!(file.config().unwrap().query("approved").is_ok());
}

#[test]
fn a_collection_spanning_configurations_says_so() {
    let scratch = Scratch::new("spanning");
    corpus(&scratch);
    scratch.raw(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    let nested = scratch.dir("nested");
    fs::write(nested.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    fs::write(
        nested.join("n.md"),
        "---\nschema_version: 1\nname: N\n---\n",
    )
    .unwrap();

    let spanning = list::from_directory(&scratch.0).unwrap();
    assert_eq!(spanning.warnings().len(), 1);
    let file = spanning
        .iter()
        .filter_map(|entry| entry.path.clone())
        .find(|path| path.ends_with("nested/n.md"))
        .unwrap();
    assert_eq!(
        spanning.configuration_of(&file),
        Some(
            dunce::canonicalize(nested.join(".samplekitrc"))
                .unwrap()
                .as_path()
        )
    );

    let forced = list::from_directory_with(&scratch.0, spanning.config().cloned()).unwrap();
    assert!(forced.warnings().is_empty());
    assert_eq!(forced.configuration_of(&file), None);
}

#[test]
fn a_file_that_will_not_load_is_skipped_and_named() {
    let scratch = Scratch::new("skipped-failure");
    corpus(&scratch);
    scratch.raw(
        "bad.md",
        "---\nschema_version: 1\nname: {a: 1}\n---\nNotes.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 3);
    let skipped = collection
        .skipped()
        .iter()
        .find(|skipped| skipped.path.ends_with("bad.md"))
        .unwrap_or_else(|| panic!("{:?}", collection.skipped()));
    assert!(!skipped.reason.to_string().is_empty());
}

fn profile_warnings(name: &str, columns: &[&str]) -> Vec<FieldWarning> {
    let scratch = Scratch::new(name);
    let written: Vec<String> = columns
        .iter()
        .map(|field| format!("{{field = \"{field}\"}}"))
        .collect();
    fs::write(
        scratch.0.join(".samplekitrc"),
        format!(
            "schema_version = 1\n[profile.p]\ncolumns = [{}]\n",
            written.join(", ")
        ),
    )
    .unwrap();
    scratch.raw("a.sample.md", "---\nschema_version: 1\nname: A\ntables:\n  measurements:\n    index: [T]\n    columns:\n      T: {}\n      co2: {}\n    rows:\n      - {T: 20, co2: 4.2}\n---\n");
    scratch.raw("b.sample.md", "---\nschema_version: 1\nname: B\n---\n");
    let collection = list::from_directory(&scratch.0).unwrap();
    list::check_profile(
        &collection,
        collection.config().unwrap().profile("p").unwrap(),
    )
}

#[test]
fn an_unknown_table_or_column_is_reported_with_the_field_corrected() {
    let warnings = profile_warnings(
        "check-cell-names",
        &["measurment.co2[20]", "measurements.coo2[20]"],
    );
    assert_eq!(
        warnings,
        vec![
            FieldWarning::Unknown {
                field: "measurment.co2[20]".to_string(),
                suggestion: Some("measurements.co2[20]".to_string()),
            },
            FieldWarning::Unknown {
                field: "measurements.coo2[20]".to_string(),
                suggestion: Some("measurements.co2[20]".to_string()),
            },
        ]
    );
}

#[test]
fn a_row_no_sample_holds_is_reported() {
    let warnings = profile_warnings(
        "check-cell-row",
        &["measurements.co2[20]", "measurements.co2[21]"],
    );
    assert_eq!(
        warnings,
        vec![FieldWarning::NoRow {
            field: "measurements.co2[21]".to_string(),
            table: "measurements".to_string(),
        }]
    );
}

#[test]
fn a_list_position_no_sample_holds_is_reported() {
    let scratch = Scratch::new("check-list-position");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"adjuncts[#1]\"}, {field = \"adjuncts[#2]\"}]\n",
    )
    .unwrap();
    scratch.raw(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\nadjuncts: [oats]\n---\n",
    );
    scratch.raw(
        "b.sample.md",
        "---\nschema_version: 1\nname: B\nadjuncts: [oats, FEC]\n---\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let warnings = list::check_profile(
        &collection,
        collection.config().unwrap().profile("p").unwrap(),
    );
    assert_eq!(
        warnings,
        vec![FieldWarning::NoItem {
            field: "adjuncts[#2]".to_string(),
            name: "adjuncts".to_string(),
        }]
    );
}

#[test]
fn each_sample_answers_to_its_nearest_configuration() {
    let scratch = Scratch::new("nearest-each");
    corpus(&scratch);
    scratch.raw(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[query.root_only]\nfilter = 'name == \"A\"'\n",
    );
    let nested = scratch.dir("nested");
    fs::write(
        nested.join(".samplekitrc"),
        "schema_version = 1\n[query.nested_only]\nfilter = 'name == \"N\"'\n",
    )
    .unwrap();
    fs::write(
        nested.join("n.md"),
        "---\nschema_version: 1\nname: N\n---\n",
    )
    .unwrap();

    let spanning = list::from_directory(&scratch.0).unwrap();
    let file = spanning
        .iter()
        .filter_map(|entry| entry.path.clone())
        .find(|path| path.ends_with("nested/n.md"))
        .unwrap();
    let own = spanning.config_for(&file).unwrap();
    assert!(own.query("nested_only").is_ok());
    assert!(own.query("root_only").is_err());

    let everything = spanning.filter_by(|_| true);
    let parts = spanning.by_configuration(&everything);
    assert_eq!(parts.len(), 2);
    let nested_part = parts
        .iter()
        .find(|part| {
            part.samples
                .iter()
                .any(|entry| entry.path.as_ref() == Some(&file))
        })
        .unwrap();
    assert_eq!(nested_part.samples.len(), 1);
    assert!(
        nested_part
            .samples
            .config()
            .unwrap()
            .query("nested_only")
            .is_ok()
    );
}

#[test]
fn lists_from_several_targets_merge_and_keep_each_configuration() {
    let scratch = Scratch::new("merged");
    corpus(&scratch);
    scratch.raw(
        ".samplekitrc",
        "schema_version = 1\n[query.root_only]\nfilter = 'name == \"A\"'\n",
    );
    let nested = scratch.dir("nested");
    fs::write(
        nested.join(".samplekitrc"),
        "schema_version = 1\n[query.nested_only]\nfilter = 'name == \"N\"'\n",
    )
    .unwrap();
    fs::write(
        nested.join("n.md"),
        "---\nschema_version: 1\nname: N\n---\n",
    )
    .unwrap();
    let merged = list::merged(vec![
        list::from_files(&[scratch.0.join("a.md")]).unwrap(),
        list::from_directory(&nested).unwrap(),
        list::from_files(&[scratch.0.join("a.md")]).unwrap(),
    ]);
    assert_eq!(merged.len(), 2);
    assert!(merged.spans_configurations());
    let path = |end: &str| {
        merged
            .iter()
            .filter_map(|entry| entry.path.clone())
            .find(|path| path.ends_with(end))
            .unwrap()
    };
    assert!(
        merged
            .config_for(&path("n.md"))
            .unwrap()
            .query("nested_only")
            .is_ok()
    );
    assert!(
        merged
            .config_for(&path("a.md"))
            .unwrap()
            .query("root_only")
            .is_ok()
    );
}

#[test]
fn an_unreadable_configuration_is_set_aside_and_said() {
    // A configuration that will not read is set aside like an unreadable file,
    // and the samples it describes are read without it — the target's own, as a
    // nested one already was.
    let scratch = Scratch::new("own-rc-unread");
    corpus(&scratch);
    scratch.raw(
        ".samplekitrc",
        "schema_version = 1\n[collection\nrecursive = true\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 3);
    assert!(collection.config().is_none());
    let skipped = collection
        .skipped()
        .iter()
        .find(|skipped| skipped.path.ends_with(".samplekitrc"))
        .unwrap_or_else(|| panic!("{:?}", collection.skipped()));
    assert!(
        matches!(
            skipped.reason,
            samplekit::config::discovery::SkipReason::Configuration { .. }
        ),
        "{:?}",
        skipped.reason
    );
    let said = skipped.reason.to_string();
    assert!(said.contains("unclosed table"), "{said}");
    assert!(said.starts_with("not read as a configuration"), "{said}");
    // A file addressed on its own is read without it too, and it is said.
    let one = list::from_files(&[scratch.0.join("a.md")]).unwrap();
    assert_eq!(one.len(), 1);
    assert!(
        one.skipped()
            .iter()
            .any(|skipped| skipped.path.ends_with(".samplekitrc")),
        "{:?}",
        one.skipped()
    );
}

#[test]
fn a_value_the_model_declares_is_a_name_before_it_is_computed() {
    // init's own project: its profile names brix before compute has run, and
    // a sort on it was refused as a misspelling.
    let scratch = Scratch::new("declared");
    scratch.sample("a.md", "A", "draft", None);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.brix = sk.Property(compute=lambda: 1.0)\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let brix = Identifier::new("brix").unwrap();
    assert!(collection.vocabulary().has_name(&brix));
    // A narrowing keeps it.
    assert!(collection.filter_by(|_| true).vocabulary().has_name(&brix));
    let spec = samplekit::query::ordering::parse_spec(&["-brix".to_string()]).unwrap();
    let mut narrowed = collection.filter_by(|_| true);
    narrowed.sort(&spec).unwrap();
}

#[test]
fn a_table_the_model_declares_is_not_a_value_name() {
    // `-c co2` named the table and printed dashes: the vocabulary took the
    // model's table for a property.
    let scratch = Scratch::new("declaredtable");
    scratch.sample("a.md", "A", "draft", None);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.brix = sk.Property(compute=lambda: 1.0)\n        \
         self.co2 = sk.Table({\"t\": sk.Column()})\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let vocabulary = collection.vocabulary();
    assert!(vocabulary.has_name(&Identifier::new("brix").unwrap()));
    assert!(!vocabulary.has_name(&Identifier::new("co2").unwrap()));
    assert!(!vocabulary.has_name(&Identifier::new("t").unwrap()));
}

#[test]
fn a_summary_weights_its_mean_when_every_uncertainty_is_known() {
    // Weighted by 1/u² where every value has an uncertainty above zero; the
    // plain mean where one has none.
    let scratch = Scratch::new("weighted-mean");
    for (file, value, uncertainty) in [("a.md", 1.0, "0.1"), ("b.md", 2.0, "0.2")] {
        scratch.raw(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nproperties:\n  x: {{v: {value}, u: {uncertainty}}}\n---\n"
            ),
        );
    }
    let collection = list::from_directory(&scratch.0).unwrap();
    let summary = collection
        .summarize(&fields::parse("x").unwrap())
        .unwrap()
        .unwrap();
    // (1/0.01 + 2/0.04) / (1/0.01 + 1/0.04) = 150 / 125
    let weighted = summary.weighted_mean.expect("every uncertainty is known");
    assert!((weighted - 1.2).abs() < 1e-9, "{weighted}");
    assert!((summary.mean() - 1.2).abs() < 1e-9);
    assert!((summary.summary.mean - 1.5).abs() < 1e-9);
    scratch.raw(
        "c.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  x: 3.0\n---\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let plain = collection
        .summarize(&fields::parse("x").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(plain.weighted_mean, None);
    assert!((plain.mean() - 2.0).abs() < 1e-9);
}

#[test]
fn a_list_is_grouped_by_the_combinations_of_its_values() {
    // One group per combination held, the values in their order and the samples
    // holding none after them; each group in the list's order.
    let scratch = Scratch::new("groups");
    corpus(&scratch);
    scratch.sample("d.md", "D", "approved", Some(10.0));
    let collection = list::from_directory(&scratch.0).unwrap();
    let status = fields::parse("status").unwrap();
    let groups = collection
        .group_by(std::slice::from_ref(&status), list::GroupOrder::Values)
        .unwrap();
    let keys: Vec<Vec<Value>> = groups.iter().map(|group| group.key.clone()).collect();
    assert_eq!(
        keys,
        [vec![Value::text("approved")], vec![Value::text("reviewed")]]
    );
    assert_eq!(names(&groups[0].samples), ["A", "C", "D"]);
    assert_eq!(names(&groups[1].samples), ["B"]);
    // Numbers as numbers: 2.8 before 10, which text would put first.
    let plato = fields::parse("plato").unwrap();
    let groups = collection
        .group_by(&[status.clone(), plato.clone()], list::GroupOrder::Values)
        .unwrap();
    let keys: Vec<Vec<Value>> = groups.iter().map(|group| group.key.clone()).collect();
    assert_eq!(
        keys,
        [
            vec![Value::text("approved"), number(2.8)],
            vec![Value::text("approved"), number(10.0)],
            vec![Value::text("approved"), Value::Absent],
            vec![Value::text("reviewed"), number(1.2)],
        ]
    );
    // One list, each group's samples together.
    let grouped = collection
        .grouped(&[plato], list::GroupOrder::Values)
        .unwrap();
    assert_eq!(names(&grouped), ["B", "A", "D", "C"]);
    match collection.group_by(&[fields::parse("plto").unwrap()], list::GroupOrder::Values) {
        Err(ListError::Field(error)) => {
            assert!(error.to_string().contains("plato"), "{error}")
        }
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("a misspelt field groups nothing"),
    }
}

#[test]
fn groups_listed_follow_the_order_of_their_first_samples() {
    // With a sort in force the groups come where their first samples come, so a
    // group field the sort names runs its way.
    let scratch = Scratch::new("groups-listed");
    corpus(&scratch);
    scratch.sample("d.md", "D", "approved", Some(10.0));
    let collection = list::from_directory(&scratch.0).unwrap();
    let plato = fields::parse("plato").unwrap();
    let status = fields::parse("status").unwrap();
    let keys = |groups: &[list::Group]| -> Vec<Vec<Value>> {
        groups.iter().map(|group| group.key.clone()).collect()
    };
    let heaviest_first = collection.sorted(&spec("plato", true)).unwrap();
    let groups = heaviest_first
        .group_by(std::slice::from_ref(&plato), list::GroupOrder::Listed)
        .unwrap();
    assert_eq!(keys(&groups)[..2], [vec![number(10.0)], vec![number(2.8)]]);
    // By another field: the status of the lightest sample first.
    let lightest_first = collection.sorted(&spec("plato", false)).unwrap();
    assert_eq!(names(&lightest_first)[0], "B");
    let groups = lightest_first
        .group_by(std::slice::from_ref(&status), list::GroupOrder::Listed)
        .unwrap();
    assert_eq!(
        keys(&groups),
        [vec![Value::text("reviewed")], vec![Value::text("approved")]]
    );
    // Each group keeps the list's order within it.
    assert_eq!(names(&groups[1].samples), ["A", "D", "C"]);
    // By their values, the same list groups as ever.
    let groups = lightest_first
        .group_by(std::slice::from_ref(&status), list::GroupOrder::Values)
        .unwrap();
    assert_eq!(
        keys(&groups),
        [vec![Value::text("approved")], vec![Value::text("reviewed")]]
    );
}

#[test]
fn states_a_list_carries_reach_what_is_narrowed_from_it() {
    // The states are read by whoever holds the list, and every list narrowed or
    // sorted from it keeps them.
    use fields::{State, States};
    let scratch = Scratch::new("states");
    corpus(&scratch);
    let collection = list::from_directory(&scratch.0).unwrap();
    let failed = filter::parse("state == failed").unwrap();
    assert!(matches!(
        collection.filter(&failed),
        Err(ListError::Filter(filter::FilterError::Field(error)))
            if *error == fields::FieldError::StatesNotRead
    ));
    let stated = collection.with_states(vec![
        States::new(vec![State::Failed], true),
        States::new(Vec::new(), true),
        States::new(vec![State::Stale], true),
    ]);
    let kept = stated.filter(&failed).unwrap();
    assert_eq!(names(&kept), ["A"]);
    let sorted = kept.sorted(&spec("plato", false)).unwrap();
    assert_eq!(names(&sorted.filter(&failed).unwrap()), ["A"]);
    let current = filter::parse("state == current").unwrap();
    assert_eq!(names(&stated.filter(&current).unwrap()), ["B"]);
}
