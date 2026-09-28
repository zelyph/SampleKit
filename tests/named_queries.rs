//! The tests of `named_queries`.

use std::fs;
use std::path::{Path, PathBuf};

use samplekit::collection::named_queries::{self as queries, QueryError};
use samplekit::collection::sample_list as list;
use samplekit::config::project_config;
use samplekit::core::identifier::Identifier;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-nq-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn sample(&self, dir: &str, file: &str, name: &str, status: &str) {
        let at = if dir.is_empty() {
            self.0.clone()
        } else {
            self.0.join(dir)
        };
        fs::create_dir_all(&at).unwrap();
        fs::write(
            at.join(file),
            format!(
                "---\nschema_version: 1\nname: {name}\nstatus: {status}\ntags: [reference]\n\
                 properties:\n  malt: {{v: 1.0, unit: g}}\n---\nnote\n"
            ),
        )
        .unwrap();
    }

    fn config(&self, body: &str) -> PathBuf {
        let path = self.0.join(".samplekitrc");
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn names(collection: &list::SampleList) -> Vec<String> {
    collection
        .iter()
        .map(|entry| entry.sample.borrow().name().unwrap().to_string())
        .collect()
}

const CONFIG: &str = r#"
schema_version = 1

[collection]
recursive = true

[query.approved]
filter = 'status == "approved"'
directory = "transport"

[query.anywhere]
filter = 'tags has reference'

[query.broken]
filter = 'plto == 1'
"#;

fn project(name: &str) -> (Scratch, project_config::ProjectConfig) {
    let scratch = Scratch::new(name);
    scratch.sample("", "a.md", "A", "approved");
    scratch.sample("", "b.md", "B", "reviewed");
    scratch.sample("transport", "c.md", "C", "approved");
    let path = scratch.config(CONFIG);
    let config = project_config::load(&path).unwrap();
    (scratch, config)
}

// ------------------------------------------------------------------ running

#[test]
fn query_filters_a_collection() {
    let (scratch, config) = project("filters");
    let collection = list::from_directory(&scratch.0).unwrap();
    let query = queries::named(&config, "approved").unwrap();
    assert_eq!(
        names(&queries::run(query, &collection).unwrap()),
        ["A", "C"]
    );
}

#[test]
fn a_selection_preserves_incoming_order() {
    // A selection never reorders: discovery order comes through untouched, and
    // any sort is applied afterwards by whatever presents it.
    let (scratch, config) = project("order");
    let collection = list::from_directory(&scratch.0).unwrap();
    let before = names(&collection);
    let query = queries::named(&config, "anywhere").unwrap();
    let selected = names(&queries::run(query, &collection).unwrap());
    assert_eq!(selected, before, "the order changed");
}

#[test]
fn running_does_not_modify_the_configuration() {
    // `run` takes `&NamedQuery`.
    let (scratch, config) = project("immutable");
    let collection = list::from_directory(&scratch.0).unwrap();
    let before = config.query("approved").unwrap().clone();
    let query = queries::named(&config, "approved").unwrap();
    let _ = queries::run(query, &collection).unwrap();
    assert_eq!(config.query("approved").unwrap(), &before);
}

#[test]
fn same_query_matches_across_surfaces() {
    // One implementation, used by all three: running the named query and
    // running its filter by hand must agree, which is what makes "one
    // definition, three surfaces" a fact rather than a hope.
    let (scratch, config) = project("surfaces");
    let collection = list::from_directory(&scratch.0).unwrap();
    let query = queries::named(&config, "approved").unwrap();
    let through_query = names(&queries::run(query, &collection).unwrap());

    let parsed = samplekit::query::filter_language::parse(&query.filter).unwrap();
    let by_hand = names(&collection.filter(&parsed).unwrap());
    assert_eq!(through_query, by_hand);
}

// ------------------------------------------------------------------- base

#[test]
fn declared_base_applies_when_no_target_is_given() {
    // Without it, the predicate runs over whatever directory the user happens
    // to be in — often the whole of `samples/`, whose subdirectories hold
    // casks described by different templates.
    let (_scratch, config) = project("base");
    let query = queries::named(&config, "approved").unwrap();
    assert_eq!(
        queries::directory_of(query, &config, None),
        config.root().join("transport")
    );
    // A query with no declared base falls back to the project root.
    let anywhere = queries::named(&config, "anywhere").unwrap();
    assert_eq!(
        queries::directory_of(anywhere, &config, None),
        config.root().to_path_buf()
    );
}

#[test]
fn explicit_target_replaces_the_declared_base() {
    // A declared base is a default, not a constraint.
    let (_scratch, config) = project("explicit");
    let query = queries::named(&config, "approved").unwrap();
    let target = Path::new("/elsewhere/runs");
    assert_eq!(
        queries::directory_of(query, &config, Some(target)),
        target.to_path_buf()
    );
}

// ----------------------------------------------------------------- errors

#[test]
fn error_names_the_query() {
    // A user running `-Q broken` gets an error about `broken`, not a
    // context-free complaint about a field they did not type.
    let (scratch, config) = project("named-error");
    let collection = list::from_directory(&scratch.0).unwrap();
    let query = queries::named(&config, "broken").unwrap();
    let error = queries::run(query, &collection).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("named query 'broken'"), "{message}");
    assert!(message.contains("plto"), "{message}");
}

#[test]
fn unknown_query_suggests_nearest() {
    let (_scratch, config) = project("unknown");
    let error = queries::named(&config, "aproved").unwrap_err();
    let QueryError::UnknownQuery {
        suggestion,
        available,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("approved"));
    assert!(available.contains(&"anywhere".to_string()));
}

// ------------------------------------------------- what is not stored here

#[test]
fn tui_state_is_not_written_to_project_config() {
    // A project's committed configuration is not rewritten by someone
    // adjusting their columns: nothing in this module writes a file at all.
    let (scratch, config) = project("no-write");
    let before = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    for name in ["approved", "anywhere"] {
        let query = queries::named(&config, name).unwrap();
        let _ = queries::run(query, &collection);
        let _ = queries::directory_of(query, &config, None);
    }
    assert_eq!(
        fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap(),
        before
    );
}

#[test]
fn membership_is_read_from_the_samples() {
    // There is no saved group: *the reference casks of this vintage* is
    // `tags has reference`, declared on each cask by a person rather than
    // recorded in a list outside the files.
    let (scratch, config) = project("membership");
    let collection = list::from_directory(&scratch.0).unwrap();
    let query = queries::named(&config, "anywhere").unwrap();
    assert_eq!(queries::run(query, &collection).unwrap().len(), 3);

    // Removing the tag from one sample changes the answer, because membership
    // lives in the file.
    let entry = collection.get(0).unwrap();
    entry.sample.borrow_mut().set_tags(Vec::new());
    assert_eq!(queries::run(query, &collection).unwrap().len(), 2);
    let _ = Identifier::new("reference").unwrap();
}
