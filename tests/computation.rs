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

fn sample(name: &str, diameter: &str) -> String {
    format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {diameter}\n---\n")
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
    assert_eq!(b.refused, [("diameter".to_string(), "heavy".to_string())]);
    let a = planned
        .requests
        .iter()
        .find(|request| request.sample.ends_with("a.md"))
        .unwrap();
    assert!(a.refused.is_empty());
}
