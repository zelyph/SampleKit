//! The tests of `computation`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::computation::{self, Asked};
use samplekit::collection::sample_list as list;
use samplekit::config::model_runtime as runtime;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "samplekit-computation-{name}-{}",
                std::process::id()
            ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(dunce::canonicalize(&path).unwrap())
    }

    fn write(&self, relative: &str, body: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(name: &str, malt: &str) -> String {
    format!("---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {malt}\n---\n")
}

#[test]
fn samples_are_grouped_by_their_configuration() {
    let scratch = Scratch::new("groups");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write("a.md", &sample("A", "1.0"));
    scratch.write("other/.samplekitrc", "schema_version = 1\n");
    scratch.write("other/b.md", &sample("B", "2.0"));
    let selected = list::from_directory(&scratch.0).unwrap();
    let groups = computation::groups_of(&selected, None).unwrap();
    assert_eq!(groups.len(), 2, "{groups:?}");
    assert_eq!(groups[0].entries, [0]);
    assert!(
        groups[1]
            .file
            .as_ref()
            .is_some_and(|file| file.starts_with(scratch.0.join("other")))
    );
}

#[test]
fn text_among_numbers_is_refused_by_name() {
    let scratch = Scratch::new("text");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write("model.py", "import samplekit as sk\n");
    scratch.write("a.md", &sample("A", "1.0"));
    scratch.write("b.md", &sample("B", "heavy"));
    let selected = list::from_directory(&scratch.0).unwrap();
    let groups = computation::groups_of(&selected, None).unwrap();
    let template = runtime::template_of(groups[0].config.as_ref().unwrap()).unwrap();
    let planned = computation::requests(&selected, &groups[0], &template, &Asked::default());
    let b = planned
        .requests
        .iter()
        .find(|request| request.sample.ends_with("b.md"))
        .unwrap();
    assert_eq!(b.refused, [("malt".to_string(), "heavy".to_string())]);
    let a = planned
        .requests
        .iter()
        .find(|request| request.sample.ends_with("a.md"))
        .unwrap();
    assert!(a.refused.is_empty());
}

#[test]
fn the_model_recorded_is_the_one_the_run_began_with() {
    // The digest is taken before the worker starts and passed in: the model
    // edited during the run is not recorded as the one that computed.
    let scratch = Scratch::new("recorded");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write("model.py", "import samplekit as sk\n");
    scratch.write("a.md", &sample("A", "1.0"));
    let selected = list::from_directory(&scratch.0).unwrap();
    let groups = computation::groups_of(&selected, None).unwrap();
    let template = runtime::template_of(groups[0].config.as_ref().unwrap()).unwrap();
    let began = runtime::digest_of(&template).unwrap();
    let planned = computation::requests(&selected, &groups[0], &template, &Asked::default());
    scratch.write("model.py", "import samplekit as sk\n# edited meanwhile\n");
    let store = runtime::ComputedStore::at(&scratch.0.join("state"));
    let request = &planned.requests[0];
    computation::record_computed(&store, request, &runtime::Report::default(), &began);
    assert_eq!(store.digest_for(&request.sample), None, "nothing computed");
    let report = runtime::Report {
        computed: 1,
        ..runtime::Report::default()
    };
    computation::record_computed(&store, request, &report, &began);
    assert_eq!(
        store.digest_for(&request.sample).as_deref(),
        Some(began.as_str())
    );
    assert_ne!(runtime::digest_of(&template).unwrap(), began);
}

#[test]
fn a_formula_is_recorded_by_what_computed_it() {
    // Each formula's digest, by the rules of model-runtime.
    let scratch = Scratch::new("formulas");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write("model.py", "import samplekit as sk\n");
    for name in ["a", "b", "c"] {
        scratch.write(&format!("{name}.md"), &sample(name, "1.0"));
    }
    let selected = list::from_directory(&scratch.0).unwrap();
    let groups = computation::groups_of(&selected, None).unwrap();
    let template = runtime::template_of(groups[0].config.as_ref().unwrap()).unwrap();
    let first = runtime::digest_of(&template).unwrap();
    scratch.write("model.py", "import samplekit as sk\n# another version\n");
    let second = runtime::digest_of(&template).unwrap();
    let store = runtime::ComputedStore::at(&scratch.0.join("state"));
    let requests = computation::requests(&selected, &groups[0], &template, &Asked::default());
    let request = |name: &str| {
        requests
            .requests
            .iter()
            .find(|request| request.sample.ends_with(format!("{name}.md")))
            .unwrap()
    };
    let report = |digests: &[(&str, &str)], computed: &[&str]| runtime::Report {
        formulas: runtime::Formulas {
            digests: digests
                .iter()
                .map(|(name, digest)| (name.to_string(), digest.to_string()))
                .collect(),
            computed: computed.iter().map(|name| name.to_string()).collect(),
        },
        ..runtime::Report::default()
    };
    let recorded = |name: &str| {
        store
            .recorded_formulas(&request(name).sample)
            .into_iter()
            .collect::<Vec<_>>()
    };
    let pairs = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(name, digest)| (name.to_string(), digest.to_string()))
            .collect::<Vec<_>>()
    };

    // The first record is the baseline, even of a run that computed nothing.
    computation::record_computed(
        &store,
        request("a"),
        &report(&[("x", "1"), ("y", "2")], &[]),
        &first,
    );
    assert_eq!(recorded("a"), pairs(&[("x", "1"), ("y", "2")]));
    // A value computed takes its formula's digest; one not computed keeps its
    // own; a formula the model no longer has is dropped; one new since is
    // taken as it is.
    computation::record_computed(
        &store,
        request("a"),
        &report(&[("x", "1b"), ("y", "2b"), ("z", "3")], &["x"]),
        &second,
    );
    assert_eq!(recorded("a"), pairs(&[("x", "1b"), ("y", "2"), ("z", "3")]));
    computation::record_computed(&store, request("a"), &report(&[("x", "1b")], &[]), &second);
    assert_eq!(recorded("a"), pairs(&[("x", "1b")]));
    assert!(!store.formulas_unknown(&request("a").sample, &first));

    // A record of the whole model only: mapped under the model it names, and
    // *not known* under another, until computed.
    let key = |name: &str| dunce::canonicalize(&request(name).sample).unwrap();
    scratch.write(
        "state/computed",
        &format!(
            "{first} {}\n{first} {}\n",
            key("b").display(),
            key("c").display()
        ),
    );
    assert!(!store.formulas_unknown(&request("b").sample, &first));
    assert!(store.formulas_unknown(&request("b").sample, &second));
    computation::record_computed(
        &store,
        request("b"),
        &report(&[("x", "1"), ("y", "2")], &[]),
        &first,
    );
    assert_eq!(recorded("b"), pairs(&[("x", "1"), ("y", "2")]));
    computation::record_computed(
        &store,
        request("c"),
        &report(&[("x", "1"), ("y", "2")], &["x"]),
        &second,
    );
    assert_eq!(recorded("c"), pairs(&[("x", "1")]));
    assert!(store.formulas_unknown(&request("c").sample, &second));
    computation::record_computed(
        &store,
        request("c"),
        &report(&[("x", "1"), ("y", "2")], &["y"]),
        &second,
    );
    assert!(!store.formulas_unknown(&request("c").sample, &second));
}
