//! The tests of `model_runtime`.
//!
//! The tests that start a worker run the **compiled binary** against the
//! repository's `.venv`, where `maturin develop` installed the package it ships
//! with: a computation is only real observed from outside. The others call the
//! module directly.

// Runs a project's Python through Unix paths and signals.
#![cfg(unix)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use samplekit::config::model_runtime::{
    self as runtime, Availability, ModelError, Request, Worker,
};
use samplekit::config::project_config::{self as config, ProjectConfig};

/// A directory holding a project and a state directory beside it, removed when
/// the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-model-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(dunce::canonicalize(&path).unwrap())
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    fn write(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.at(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    fn exists(&self, relative: &str) -> bool {
        self.at(relative).exists()
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.at(relative)).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The repository's environment, which `maturin develop` fills.
fn venv() -> PathBuf {
    let venv = Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(
        venv.join("bin").join("python").is_file(),
        "these tests need the repository's .venv, with the package built by `maturin develop`"
    );
    refuse_a_stale_extension(&venv);
    venv
}

/// The worker imports the extension `maturin develop` installed, so these tests
/// test the last one built, not the source. Checked once per run.
fn refuse_a_stale_extension(venv: &Path) {
    static CHECKED: std::sync::Once = std::sync::Once::new();
    CHECKED.call_once(|| {
        if std::env::var_os("SAMPLEKIT_ALLOW_STALE_BUILD").is_some() {
            return;
        }
        let located = Command::new(venv.join("bin").join("python"))
            .args(["-c", "import samplekit._native as n; print(n.__file__)"])
            .output()
            .expect("the environment's python runs");
        let extension = PathBuf::from(String::from_utf8_lossy(&located.stdout).trim());
        let modified = |path: &Path| fs::metadata(path).and_then(|held| held.modified()).ok();
        let built = modified(&extension)
            .expect("samplekit is installed in .venv: run `maturin develop --release`");
        let mut pending = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut newest = None;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // The extension is the library: `src/interfaces` is the
                    // binary's alone, which cargo rebuilds for these tests.
                    if !path.ends_with("interfaces") {
                        pending.push(path);
                    }
                } else if path.extension().is_some_and(|e| e == "rs")
                    && let Some(at) = modified(&path)
                    && newest.as_ref().is_none_or(|(_, latest)| at > *latest)
                {
                    newest = Some((path, at));
                }
            }
        }
        let (source, changed) = newest.expect("src holds Rust files");
        assert!(
            built >= changed,
            "refusing to test a stale extension: {} is older than {} — run `maturin develop \
             --release`, or set SAMPLEKIT_ALLOW_STALE_BUILD=1 to run anyway",
            extension.display(),
            source.display()
        );
    });
}

/// A model that leaves a mark when it is imported and when a formula runs, and
/// prints from a formula.
const KEG: &str = r#"import os

import samplekit as sk

HERE = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(HERE, "imported"), "a") as marker:
    marker.write("imported\n")


class Keg(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.plato = sk.Property(compute=self._plato)
        self.haze = sk.Property(compute=lambda: 1 - self.plato.value / 4.0)
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.set_dependencies("haze", depends_on=["plato"])

    def _plato(self):
        with open(os.path.join(HERE, "ran"), "a") as marker:
            marker.write("plato\n")
        print("computing the plato of", self.name)
        return self.malt.value / self.volume.value
"#;

/// A second value that kills the command line running it, then takes its time.
const SLOW: &str = r#"import os
import signal
import time

import samplekit as sk


class Slow(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.first = sk.Property(compute=lambda: self.malt.value * 2)
        self.second = sk.Property(compute=self._second)
        self.set_dependencies("first", depends_on=["malt"])
        self.set_dependencies("second", depends_on=["first"])

    def _second(self):
        os.kill(os.getppid(), signal.SIGTERM)
        time.sleep(4)
        return self.first.value + 1
"#;

/// Declared out of dependency order, so that an ordered plan cannot come from
/// the declarations.
const CHAIN: &str = r#"import samplekit as sk


class Chain(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.vfi = sk.Property(compute=lambda: self.brix.value + 1)
        self.brix = sk.Property(compute=lambda: self.grist.value / 4)
        self.grist = sk.Property(compute=lambda: self.malt.value * 2)
        self.set_dependencies("vfi", depends_on=["brix"])
        self.set_dependencies("brix", depends_on=["grist"])
        self.set_dependencies("grist", depends_on=["malt"])
"#;

/// The five shapes a model can declare.
const SHAPES: &str = r#"import samplekit as sk


class Shapes(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.plato = sk.Property(compute=lambda: self.malt.value / self.volume.value)
        # An instrument's resolution reads nothing of the sample, which is what
        # depends_on=[] says. Declaring is not optional: a formula that reads
        # what it never declared is never stale again.
        self.age = sk.Property(2.0, compute_uncertainty=lambda: 0.01, depends_on=[])
        self.pair = sk.Property(compute_quantity=lambda: (self.malt.value * 2, 0.5))
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.set_dependencies("pair", depends_on=["malt"])
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column(), "N": sk.Column()},
            "T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
            compute_rows=[("G", ["row.R"], lambda row: 1 / row.R.value)],
            compute_columns=[
                ("N", ["mashing.R"], lambda columns: [value / 4.0 for value in columns["R"].values])
            ],
        )
"#;

/// A project: a `.samplekitrc` naming `model.py`, the repository's environment
/// as its `.venv`, and two kegs.
fn project(scratch: &Scratch, model: &str) -> PathBuf {
    bare_project(
        scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        model,
    );
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.at("project")
}

/// The same with no environment.
fn bare_project(scratch: &Scratch, rc: &str, model: &str) {
    scratch.write("project/.samplekitrc", rc);
    scratch.write("project/model.py", model);
    keg(scratch, "c1", 12.0, 4.0);
    keg(scratch, "c2", 10.0, 4.0);
}

fn keg(scratch: &Scratch, name: &str, malt: f64, volume: f64) -> PathBuf {
    scratch.write(
        &format!("project/samples/{name}.md"),
        &format!(
            "---\nschema_version: 1\nname: {}\nproperties:\n  malt: {{v: {malt:?}, unit: g}}\n  \
             volume: {{v: {volume:?}, unit: L}}\n---\n",
            name.to_uppercase()
        ),
    )
}

/// An interpreter that is a shell script, named by `[model] python`.
fn fake_python(scratch: &Scratch, body: &str) {
    let script = scratch.write("project/fake-python", &format!("#!/bin/sh\n{body}"));
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"fake-python\"\n",
    );
}

/// A worker on the fake interpreter, started again while another test's fork
/// still holds the script just written open (`Text file busy`).
fn start_fake(scratch: &Scratch) -> Result<Worker, ModelError> {
    for _ in 0..50 {
        match Worker::start(&scratch.at("project/fake-python")) {
            Err(ModelError::NoInterpreter { reason }) if reason.contains("Text file busy") => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => return other,
        }
    }
    Worker::start(&scratch.at("project/fake-python"))
}

fn loaded(scratch: &Scratch) -> ProjectConfig {
    config::load(&scratch.at("project/.samplekitrc")).unwrap()
}

fn command(scratch: &Scratch, directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_samplekit"));
    command
        .current_dir(directory)
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("COLUMNS", "200")
        .env("NO_COLOR", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    command
}

/// The binary, in the project, with nobody at a terminal.
fn run(scratch: &Scratch, arguments: &[&str]) -> Output {
    command(scratch, &scratch.at("project"))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

/// The binary in a pseudoterminal, answering its questions.
fn at_terminal(scratch: &Scratch, arguments: &[&str], answers: &str) -> Output {
    let line = std::iter::once(env!("CARGO_BIN_EXE_samplekit"))
        .chain(arguments.iter().copied())
        .map(|word| format!("'{}'", word.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = command(scratch, &scratch.at("project"));
    let mut child = child.args([] as [&str; 0]).spawn_script(&line);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

trait Script {
    fn spawn_script(&mut self, line: &str) -> std::process::Child;
}

impl Script for Command {
    fn spawn_script(&mut self, line: &str) -> std::process::Child {
        let mut script = Command::new("script");
        script.args(["--quiet", "--return", "--command", line, "/dev/null"]);
        if let Some(directory) = self.get_current_dir() {
            script.current_dir(directory);
        }
        for (key, value) in self.get_envs() {
            if let Some(value) = value {
                script.env(key, value);
            }
        }
        script
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("script creates a pseudoterminal")
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit code, not a signal")
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Every file under a directory, links not followed.
fn files_under(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            found.extend(files_under(&entry.path()));
        } else if kind.is_file() {
            found.push(entry.path());
        }
    }
    found
}

fn request(scratch: &Scratch, sample: &str) -> Request {
    Request {
        sample: scratch.at(sample),
        template: runtime::template_of(&loaded(scratch)).unwrap(),
        names: Vec::new(),
        rerun: false,
        force: false,
        refused: Vec::new(),
        recorded: Default::default(),
    }
}

// ------------------------------------------------------------------ shape

/// A surface that draws while it computes runs `compute` on a thread and reads
/// `Event` off a channel. Both properties hold today by accident of the fields;
/// this makes them hold on purpose, so that a field added later fails here
/// rather than in a screen nobody has written yet.
#[test]
fn a_worker_and_its_events_cross_a_thread() {
    fn sendable<T: Send>() {}
    sendable::<Worker>();
    sendable::<runtime::Event>();
}

/// A value entered beside a computed uncertainty is nobody's output, so a hand
/// changing it overrides nothing.
///
/// The tool used to answer *edited since it was computed* about a value
/// nothing had ever computed, and `--force` then erased the mark it should
/// never have written — laundering a hand-typed number into *current*.
#[test]
fn an_entered_value_beside_a_computed_uncertainty_is_not_an_override() {
    let scratch = Scratch::new("entered-beside-computed");
    project(
        &scratch,
        "import math\nimport samplekit as sk\n\n\n\
         class M(sk.Sample):\n    \
         def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.ibu = sk.Property(unit=\"mL\", compute_uncertainty=self._hydrometer, depends_on=[])\n        \
         self.headspace = sk.Property(unit=\"hl\", compute=self._headspace, depends_on=[\"ibu\"])\n\n    \
         def _hydrometer(self):\n        return 0.006\n\n    \
         def _headspace(self):\n        return math.pi * (self.ibu.value / 2) ** 2 / 100\n",
    );
    // The shared fixture's kegs declare quantities this model does not.
    for gone in ["c1", "c2"] {
        std::fs::remove_file(scratch.at(&format!("project/samples/{gone}.md"))).unwrap();
    }
    scratch.write(
        "project/samples/s.md",
        "---\nschema_version: 1\nname: S\nproperties:\n  ibu: {v: 12.0, unit: mL}\n---\n",
    );
    assert_eq!(code(&run(&scratch, &["compute", "samples", "--write"])), 0);
    // The record names the channel its formula produced.
    let written = scratch.read("project/samples/s.md");
    assert!(written.contains("computed: {u: {}}"), "{written}");

    let set = run(&scratch, &["set", "samples/s.md", "ibu=9.99", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    // Not an override: nothing computed that value.
    assert!(!out(&set).contains("an override"), "{}", out(&set));

    let state = run(&scratch, &["status", "samples"]);
    let said = out(&state);
    // `ibu` appears only as what stales `headspace`, never as a row of its own.
    assert!(!said.contains("edited"), "{said}");
    assert_eq!(said.matches("outdated — ibu").count(), 1, "{said}");
    // What reads the value is stale, which is the point of the consumer's
    // digest staying wide while the property's own narrows.
    assert!(said.contains("headspace"), "{said}");

    // `explain` names the number a formula made, rather than claiming the
    // quantity was computed.
    let why = run(&scratch, &["explain", "samples/s.md", "ibu"]);
    assert!(
        out(&why).contains("computed its uncertainty"),
        "{}",
        out(&why)
    );
    assert!(!out(&why).contains("edited since"), "{}", out(&why));
}

// ----------------------------------------------------------------- reading

#[test]
fn reading_never_loads_the_template() {
    let scratch = Scratch::new("reading");
    project(&scratch, KEG);
    for arguments in [
        vec!["samples"],
        vec!["samples", "-c", "malt"],
        // `status` reads the model to plan, nothing asked, wherever the model
        // adds something: no reading command left is kept to the files by a
        // flag.
        vec!["list", "samples"],
        vec!["validate", "samples"],
    ] {
        let output = run(&scratch, &arguments);
        assert!(code(&output) < 3, "{arguments:?}: {}", err(&output));
    }
    assert!(!scratch.exists("project/imported"));
}

#[test]
fn no_template_is_not_an_error() {
    let scratch = Scratch::new("no-template");
    scratch.write("project/.samplekitrc", "schema_version = 1\n");
    keg(&scratch, "c1", 12.0, 4.0);
    let from = scratch.at("project");
    for config in [None, Some(&loaded(&scratch))] {
        assert_eq!(
            runtime::availability(config, &from).unwrap(),
            Availability::NoTemplate
        );
    }
    let output = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("no model declared"),
        "{}",
        out(&output)
    );
}

#[test]
fn binary_links_no_python() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_samplekit")).unwrap();
    // `libpython3.` as a linked library names it, version and all: the bare
    // word came together by chance once the linker laid the string `lib` beside
    // `python3`, both of them init's.
    for needle in [b"libpython3.".as_slice(), b"Py_Initialize".as_slice()] {
        assert!(
            !bytes.windows(needle.len()).any(|window| window == needle),
            "{}",
            String::from_utf8_lossy(needle)
        );
    }
}

#[test]
fn no_worker_is_spawned_while_only_reading() {
    let scratch = Scratch::new("no-worker");
    bare_project(&scratch, "", KEG);
    fake_python(&scratch, "touch \"$(dirname \"$0\")/started\"\nexit 1\n");
    for arguments in [
        vec!["samples"],
        vec!["samples", "-c", "malt"],
        vec!["list", "samples"],
        vec!["validate", "samples"],
    ] {
        run(&scratch, &arguments);
    }
    assert!(!scratch.exists("project/started"));
    // The probe itself works: a computation does start it.
    run(&scratch, &["compute", "samples", "--write"]);
    assert!(scratch.exists("project/started"));
}

// --------------------------------------------------------------- the model

#[test]
fn loads_a_template_by_relative_path() {
    let scratch = Scratch::new("relative");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"models/keg.py\"\n",
        "",
    );
    fs::remove_file(scratch.at("project/model.py")).unwrap();
    scratch.write("project/models/keg.py", KEG);
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    let template = runtime::template_of(&loaded(&scratch)).unwrap();
    assert_eq!(template.path(), scratch.at("project/models/keg.py"));

    // From another working directory than the configuration's.
    let output = command(&scratch, &scratch.0)
        .args(["compute", "project/samples", "--write"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("plato:"));
}

#[test]
fn single_class_needs_no_class_name() {
    let scratch = Scratch::new("single-class");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("v: 3.0"));
}

#[test]
fn ambiguous_class_is_refused() {
    let scratch = Scratch::new("ambiguous");
    project(
        &scratch,
        &format!("{KEG}\n\nclass Other(sk.Sample):\n    pass\n"),
    );
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("Keg") && err(&output).contains("Other"),
        "{}",
        err(&output)
    );
    assert!(!scratch.read("project/samples/c1.md").contains("plato"));
}

#[test]
fn named_class_not_found_lists_available() {
    let scratch = Scratch::new("class-not-found");
    project(&scratch, KEG);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Kegg\"\n",
    );
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("Kegg") && err(&output).contains("Keg"),
        "{}",
        err(&output)
    );
}

#[test]
fn missing_template_file_names_the_path() {
    let scratch = Scratch::new("missing-template");
    project(&scratch, KEG);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"gone.py\"\n",
    );
    assert!(matches!(
        runtime::availability(Some(&loaded(&scratch)), &scratch.at("project")),
        Err(ModelError::TemplateNotFound { path }) if path == scratch.at("project/gone.py")
    ));
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains(&scratch.at("project/gone.py").display().to_string()),
        "{}",
        err(&output)
    );
}

#[test]
fn template_exception_propagates_unchanged() {
    let scratch = Scratch::new("exception");
    project(&scratch, KEG);
    keg(&scratch, "c3", 13.0, 0.0);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let written = err(&output);
    assert!(written.contains("ZeroDivisionError"), "{written}");
    assert!(written.contains("model.py\", line"), "{written}");
    assert!(
        written.contains("return self.malt.value / self.volume.v"),
        "{written}"
    );
    assert!(!written.contains("_worker.py"), "{written}");
}

// ---------------------------------------------------------- availability

#[test]
fn build_without_interpreter_reports_unavailable() {
    let scratch = Scratch::new("no-interpreter");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        KEG,
    );
    assert!(matches!(
        runtime::availability(Some(&loaded(&scratch)), &scratch.at("project")).unwrap(),
        Availability::Unavailable { reason } if reason.contains("no environment found")
    ));
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("no environment found"),
        "{}",
        err(&output)
    );
    for arguments in [vec!["list", "samples"], vec!["samples", "-c", "malt"]] {
        assert_eq!(code(&run(&scratch, &arguments)), 0, "{arguments:?}");
    }
}

#[test]
fn interpreter_is_found_by_walking_upward() {
    let scratch = Scratch::new("upward");
    let python = scratch.write("a/.venv/bin/python", "");
    fs::create_dir_all(scratch.at("a/b/c")).unwrap();
    assert_eq!(
        runtime::find_interpreter(&scratch.at("a/b/c")),
        Some(python)
    );
    assert_eq!(
        runtime::find_interpreter(&scratch.at("elsewhere-that-is-not-there")),
        None
    );
}

#[test]
fn explicit_interpreter_overrides_the_convention() {
    let scratch = Scratch::new("explicit");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"tools/python\"\n",
        KEG,
    );
    scratch.write("project/.venv/bin/python", "");
    let tools = scratch.write("project/tools/python", "");
    assert_eq!(
        runtime::interpreter_for(&loaded(&scratch), &scratch.at("project/samples")).unwrap(),
        tools
    );
}

#[test]
fn missing_environment_reports_what_was_looked_for() {
    let scratch = Scratch::new("looked-for");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        KEG,
    );
    let from = scratch.at("project/samples");
    match runtime::interpreter_for(&loaded(&scratch), &from) {
        Err(ModelError::NoInterpreter { reason }) => {
            assert!(reason.contains("looked for .venv/ from"), "{reason}");
            assert!(reason.contains(&from.display().to_string()), "{reason}");
            assert!(reason.contains("install samplekit"), "{reason}");
        }
        other => panic!("expected no interpreter, got {other:?}"),
    }
}

#[test]
fn three_availability_states_are_distinguishable() {
    let none = Scratch::new("three-none");
    none.write("project/.samplekitrc", "schema_version = 1\n");
    keg(&none, "c1", 12.0, 4.0);
    let unavailable = Scratch::new("three-unavailable");
    bare_project(
        &unavailable,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        KEG,
    );
    let ready = Scratch::new("three-ready");
    project(&ready, KEG);

    let from = |scratch: &Scratch| scratch.at("project");
    assert!(matches!(
        runtime::availability(Some(&loaded(&none)), &from(&none)),
        Ok(Availability::NoTemplate)
    ));
    assert!(matches!(
        runtime::availability(Some(&loaded(&unavailable)), &from(&unavailable)),
        Ok(Availability::Unavailable { .. })
    ));
    assert!(matches!(
        runtime::availability(Some(&loaded(&ready)), &from(&ready)),
        Ok(Availability::Ready { .. })
    ));

    let messages = [
        out(&run(&none, &["compute", "samples"])),
        err(&run(&unavailable, &["compute", "samples", "--try"])),
        out(&run(&ready, &["compute", "samples", "--write"])),
    ];
    let phrases = ["no model declared", "no environment found", "computed"];
    for (position, message) in messages.iter().enumerate() {
        for (other, phrase) in phrases.iter().enumerate() {
            assert_eq!(
                message.contains(phrase),
                position == other,
                "{position} / {phrase}: {message}"
            );
        }
    }
}

// --------------------------------------------------------- no consent

#[test]
fn a_model_runs_without_asking() {
    // No question, no record of an answer, and no flag to type; the model file
    // is named before it runs.
    let scratch = Scratch::new("without-asking");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("plato:"));
    let named = err(&output)
        .lines()
        .position(|line| line.starts_with("model ") && line.contains("model.py"));
    assert_eq!(named, Some(1), "{}", err(&output));
    assert!(!err(&output).contains("[y]"), "{}", err(&output));
    assert!(!scratch.exists("state/consent"));
    assert!(!scratch.exists("state/accepted.git"));
    assert!(scratch.exists("state/computed.json"));
}

#[test]
fn run_model_and_consent_are_unknown_options() {
    // The flags of a consent that no longer exists are refused as any option a
    // command does not take, with no message of their own, and nothing runs.
    let scratch = Scratch::new("retired-flags");
    project(&scratch, KEG);
    let before = scratch.read("project/samples/c1.md");
    for arguments in [
        vec!["compute", "samples", "--write", "--run-model"],
        vec!["compute", "samples", "--consent"],
        vec!["status", "samples", "--run-model"],
        vec!["list", "figures", "samples", "--run-model"],
        vec!["plot", "look", "samples", "-o", "x.png", "--run-model"],
    ] {
        let output = run(&scratch, &arguments);
        let flag = arguments.last().unwrap();
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains(&format!("unexpected argument '{flag}'")),
            "{arguments:?}: {}",
            err(&output)
        );
        assert!(
            !err(&output).contains("no longer needed"),
            "{arguments:?}: {}",
            err(&output)
        );
    }
    assert_eq!(scratch.read("project/samples/c1.md"), before);
    assert!(!scratch.exists("project/imported"));
}

#[test]
fn no_model_is_an_unknown_option() {
    // The model is read wherever it adds something, and the flag that kept
    // `status` and `compute` to the files is refused as any option a command
    // does not take — `--run-model` no longer offered as near it.
    let scratch = Scratch::new("no-model");
    project(&scratch, KEG);
    let before = scratch.read("project/samples/c1.md");
    for arguments in [
        vec!["status", "samples", "--no-model"],
        vec!["compute", "samples", "--no-model"],
        vec!["compute", "samples", "--write", "--no-model"],
        vec!["explain", "samples/c1.md", "plato", "--no-model"],
    ] {
        let output = run(&scratch, &arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains("unexpected argument '--no-model'"),
            "{arguments:?}: {}",
            err(&output)
        );
    }
    let near = run(&scratch, &["compute", "samples", "--run-model"]);
    assert!(!err(&near).contains("--no-model"), "{}", err(&near));
    assert!(!scratch.exists("project/imported"));
    assert_eq!(scratch.read("project/samples/c1.md"), before);
}

#[test]
fn the_models_digest_covers_its_tree() {
    let scratch = Scratch::new("digest-tree");
    project(&scratch, KEG);
    scratch.write("project/helpers/fit.py", "SCALE = 1\n");
    scratch.write("project/helpers/__pycache__/fit.cpython-314.pyc", "cache");
    let template = runtime::template_of(&loaded(&scratch)).unwrap();
    let files = runtime::template_files(&template).unwrap();
    assert!(
        files.contains(&scratch.at("project/helpers/fit.py")),
        "{files:?}"
    );
    assert!(
        files
            .iter()
            .all(|file| !file.starts_with(scratch.at("project/.venv"))
                && !file.to_string_lossy().contains("__pycache__")),
        "{files:?}"
    );
    let digest = runtime::digest_of(&template).unwrap();
    assert_eq!(digest.as_str().len(), 64);
    scratch.write("project/helpers/fit.py", "SCALE = 2\n");
    assert_ne!(runtime::digest_of(&template).unwrap(), digest);
}

// ------------------------------------------------------------------ worker

#[test]
fn a_dead_worker_is_not_reported_as_a_template_failure() {
    let scratch = Scratch::new("dead-worker");
    bare_project(&scratch, "", KEG);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\"}}}}\\n'\nread line\necho Killed >&2\nexit 137\n",
            runtime::VERSION
        ),
    );
    let mut worker = start_fake(&scratch).unwrap();
    match worker.plan(&request(&scratch, "project/samples/c1.md")) {
        Err(ModelError::WorkerDied { detail }) => {
            assert!(detail.unwrap_or_default().contains("Killed"));
        }
        other => panic!("expected a dead worker, got {other:?}"),
    }
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("exited without answering"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("raised"), "{}", err(&output));
}

#[test]
fn a_worker_computes_and_saves_each_sample() {
    let scratch = Scratch::new("computes-and-saves");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let first = scratch.read("project/samples/c1.md");
    assert!(
        first.contains("  plato:\n    v: 3.0\n    computed: {v: {malt: "),
        "{first}"
    );
    assert!(first.contains("  haze:\n    v: 0.25\n"), "{first}");
    assert!(scratch.read("project/samples/c2.md").contains("v: 2.5"));
    // Nothing is pending once saved.
    let again = run(&scratch, &["compute", "samples", "--write"]);
    assert!(
        out(&again).contains("nothing to compute in 2 samples"),
        "{}",
        out(&again)
    );
}

#[test]
fn a_worker_computes_every_shape() {
    let scratch = Scratch::new("every-shape");
    project(&scratch, SHAPES);
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    keg(&scratch, "c1", 3.0, 1.5);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}\n{}", out(&output), err(&output));
    let written = scratch.read("project/samples/c1.md");
    assert!(written.contains("  plato:\n    v: 2.0\n"), "{written}");
    assert!(written.contains("u: 0.01"), "{written}");
    assert!(
        written.contains("  pair:\n    v: 6.0\n    u: 0.5\n"),
        "{written}"
    );
    for (column, value, input) in [
        ("G", "0.5", "{row.R: "),
        ("G", "0.25", "{row.R: "),
        ("N", "0.5", "{mashing.R: "),
        ("N", "1.0", "{mashing.R: "),
    ] {
        // One line per derived cell.
        let cell = format!("        {column}: {{v: {value}, computed: {input}");
        assert!(
            written.contains(&cell),
            "{cell}
{written}"
        );
    }
}

#[test]
fn a_plan_is_in_dependency_order() {
    let scratch = Scratch::new("dependency-order");
    project(&scratch, CHAIN);
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    let output = run(
        &scratch,
        &["compute", "samples", "-p", "vfi,brix", "-p", "grist"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let listed = out(&output);
    let at = |value: &str| {
        listed
            .lines()
            .position(|line| line.split_whitespace().nth(1) == Some(value))
            .unwrap_or_else(|| panic!("{value} is not planned:\n{listed}"))
    };
    assert!(
        at("grist") < at("brix") && at("brix") < at("vfi"),
        "{listed}"
    );
}

#[test]
fn properties_rerun_and_force_choose_what_is_planned() {
    let scratch = Scratch::new("properties-rerun-force");
    project(&scratch, KEG);
    assert_eq!(code(&run(&scratch, &["compute", "samples", "--write"])), 0);
    let edit = |file: &str, before: &str, after: &str| {
        let text = scratch.read(file);
        assert!(text.contains(before), "{text}");
        scratch.write(file, &text.replacen(before, after, 1));
    };
    let planned = |arguments: &[&str]| {
        let mut all = vec!["compute", "samples"];
        all.extend_from_slice(arguments);
        let output = run(&scratch, &all);
        assert_eq!(code(&output), 0, "{}", err(&output));
        out(&output)
            .lines()
            .filter_map(|line| {
                let words: Vec<&str> = line
                    .split(|c: char| c.is_whitespace() || c == '│')
                    .filter(|word| !word.is_empty())
                    .collect();
                (words.len() >= 3 && words[0].starts_with('C') && words[0].len() == 2)
                    .then(|| format!("{} {} {}", words[0], words[1], words[2..].join(" ")))
            })
            .collect::<Vec<_>>()
    };
    edit("project/samples/c1.md", "v: 12.0", "v: 16.0");
    assert_eq!(planned(&[]), ["C1 plato outdated", "C1 haze outdated"]);
    assert_eq!(
        planned(&["-p", "haze"]),
        ["C1 plato outdated", "C1 haze outdated"]
    );
    assert_eq!(
        planned(&["-p", "haze", "--rerun"]),
        ["C1 plato outdated", "C1 haze outdated", "C2 haze current"]
    );
    // An override holds, unless forced.
    edit("project/samples/c2.md", "v: 2.5", "v: 2.6");
    assert_eq!(
        planned(&["--rerun"]),
        ["C1 plato outdated", "C1 haze outdated", "C2 haze outdated"]
    );
    assert_eq!(
        planned(&["--force"]),
        [
            "C1 plato outdated",
            "C1 haze outdated",
            "C2 plato edited",
            "C2 haze outdated"
        ]
    );
}

#[test]
fn a_failed_value_does_not_stop_the_run() {
    let scratch = Scratch::new("failure-continues");
    project(&scratch, KEG);
    keg(&scratch, "c0", 13.0, 0.0);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let report = out(&output);
    assert!(report.contains("✗ C0"), "{report}");
    assert!(
        report.contains("✓ C1") && report.contains("✓ C2"),
        "{report}"
    );
    // What reads the failed value waits behind it, and is no second failure
    // with the same traceback.
    assert!(
        report.contains("computed 4 values in 3 samples, 1 failed, 1 waits behind what failed"),
        "{report}"
    );
    assert!(report.contains("waits for plato, which failed"), "{report}");
    assert!(scratch.read("project/samples/c2.md").contains("plato:"));
}

#[test]
fn a_dry_run_computes_and_writes_nothing() {
    let scratch = Scratch::new("dry-run");
    project(&scratch, KEG);
    let before = scratch.read("project/samples/c1.md");
    let output = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("never computed"), "{}", out(&output));
    assert!(
        out(&output).contains("4 values in 2 samples would be computed"),
        "{}",
        out(&output)
    );
    assert_eq!(scratch.read("project/samples/c1.md"), before);
    assert!(!scratch.exists("project/ran"));
}

#[test]
fn a_dry_run_without_the_model_reads_the_files() {
    let scratch = Scratch::new("dry-run-files");
    project(&scratch, KEG);
    assert_eq!(code(&run(&scratch, &["compute", "samples", "--write"])), 0);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("v: 12.0", "v: 16.0", 1),
    );
    fs::remove_file(scratch.at("project/imported")).unwrap();
    // No flag keeps the listing to the files; a model that cannot be read does,
    // and the listing says why and fails as a run would.
    fs::rename(
        scratch.at("project/model.py"),
        scratch.at("project/moved.py"),
    )
    .unwrap();
    let output = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let listed = out(&output);
    assert!(
        listed
            .lines()
            .any(|line| line.contains("C1") && line.contains("plato") && line.contains("outdated")),
        "{listed}"
    );
    assert!(
        err(&output).contains("known only to the model"),
        "{}",
        err(&output)
    );
    assert!(err(&output).contains("model.py"), "{}", err(&output));
    // What cannot run is not offered: no --write without the model.
    assert!(listed.contains("once the model is found"), "{listed}");
    assert!(!listed.contains("--write writes"), "{listed}");
    // The same over a status, which lists the files and says the model
    // was not read.
    let status = run(&scratch, &["status", "samples"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(out(&status).contains("outdated"), "{}", out(&status));
    assert!(
        out(&status).contains("the model could not be read"),
        "{}",
        out(&status)
    );
    assert!(!scratch.exists("project/imported"));
}

#[test]
fn the_worker_and_the_binary_must_share_a_version() {
    let scratch = Scratch::new("version");
    bare_project(&scratch, "", KEG);
    fake_python(
        &scratch,
        "printf '{\"ready\": {\"version\": \"0.0.0-other\", \"python\": \"3\"}}\\n'\ncat > /dev/null\n",
    );
    assert!(matches!(
        start_fake(&scratch),
        Err(ModelError::VersionMismatch { package, .. }) if package == "0.0.0-other"
    ));
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 3, "{}", err(&output));
    let written = err(&output);
    assert!(
        written.contains("0.0.0-other") && written.contains(runtime::VERSION),
        "{written}"
    );
    assert!(written.contains("install samplekit"), "{written}");
}

#[test]
fn an_environment_without_samplekit_is_unavailable() {
    let scratch = Scratch::new("not-installed");
    bare_project(&scratch, "", KEG);
    fake_python(
        &scratch,
        "echo \"ModuleNotFoundError: No module named 'samplekit'\" >&2\nexit 1\n",
    );
    assert!(matches!(
        start_fake(&scratch),
        Err(ModelError::NotInstalled { .. })
    ));
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("cannot import samplekit"),
        "{}",
        err(&output)
    );
    assert!(
        err(&output).contains("install samplekit"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_model_print_never_corrupts_the_protocol() {
    let scratch = Scratch::new("print");
    project(&scratch, KEG);
    let output = run(
        &scratch,
        &["compute", "samples", "--write", "--show-output"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("computing the plato of C1"),
        "{}",
        err(&output)
    );
    assert!(
        !out(&output).contains("computing the plato"),
        "{}",
        out(&output)
    );
}

/// The binary in `directory`, killed if it has not ended within a minute: a
/// run that waits for ever fails the test rather than holding the suite.
fn run_within_a_minute(scratch: &Scratch, directory: &Path, arguments: &[&str]) -> Output {
    let mut child = command(scratch, directory)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("{arguments:?} was still running after a minute");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn a_model_reading_stdin_reads_nothing() {
    // An `input()` left in a formula read the next request as its answer, or
    // waited for ever on a line that never came.
    let scratch = Scratch::new("stdin");
    project(
        &scratch,
        &KEG.replace(
            "print(\"computing the plato of\", self.name)",
            "input(\"continue? \")",
        ),
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project"),
        &["compute", "samples", "--write"],
    );
    assert!(err(&output).contains("EOFError"), "{}", err(&output));
    assert!(scratch.read("project/samples/c2.md").contains("plato"));
}

#[test]
fn a_model_printing_an_undecodable_name_computes() {
    // A lone surrogate is valid Python and no JSON reader accepts it: the line
    // was refused, and the run reported as a dead worker that went on saving.
    let scratch = Scratch::new("surrogate");
    project(
        &scratch,
        &KEG.replace(
            "print(\"computing the plato of\", self.name)",
            "print(os.fsdecode(b\"bad\\xff\"), self.name)",
        ),
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project"),
        &["compute", "samples", "--write", "--show-output"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(err(&output).contains("bad\u{fffd} C1"), "{}", err(&output));
    assert!(scratch.read("project/samples/c2.md").contains("plato"));
}

#[test]
fn a_module_where_compute_runs_shadows_nothing() {
    // The folder the command runs in is not on the worker's `sys.path`: a
    // `json.py` there replaced the module the protocol is spoken with.
    let scratch = Scratch::new("shadow");
    project(&scratch, KEG);
    scratch.write("project/samples/json.py", "raise SystemExit('shadowed')\n");
    scratch.write(
        "project/samples/logging.py",
        "raise SystemExit('shadowed')\n",
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project/samples"),
        &["compute", ".", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("shadowed"), "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("plato"));
}

#[test]
fn a_worker_writing_an_unreadable_line_is_stopped() {
    // After the line, the fake worker would sleep a minute: it is killed, not
    // waited for.
    let scratch = Scratch::new("unreadable-line");
    bare_project(&scratch, "", KEG);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\"}}}}\\n'\nread line\n\
             printf '{{\"output\": {{\"value\": null, \"text\": \"bad\\\\udcff\"}}}}\\n'\n\
             exec sleep 60\n",
            runtime::VERSION
        ),
    );
    let mut worker = start_fake(&scratch).unwrap();
    let started = std::time::Instant::now();
    match worker.plan(&request(&scratch, "project/samples/c1.md")) {
        Err(ModelError::WorkerDied { detail }) => {
            assert!(detail.unwrap_or_default().contains("udcff"));
        }
        other => panic!("expected the worker stopped, got {other:?}"),
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}

#[test]
fn a_worker_is_never_waited_for_behind_a_full_pipe() {
    // A megabyte on stdout that nobody asked for, then the end of its input
    // awaited: the worker is let go by reading what it writes.
    let scratch = Scratch::new("full-pipe");
    bare_project(&scratch, "", KEG);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\"}}}}\\n'\n\
             head -c 1000000 /dev/zero | tr '\\000' x\necho\nexec cat > /dev/null\n",
            runtime::VERSION
        ),
    );
    let worker = start_fake(&scratch).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(worker);
        sender.send(()).unwrap();
    });
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .is_ok(),
        "the worker was waited for behind a full pipe"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn the_state_folder_is_the_systems_own() {
    // Without `SAMPLEKIT_STATE_DIR`, the XDG state folder here; macOS's data
    // folder and Windows's local one are the same code on another system.
    let scratch = Scratch::new("state-folder");
    project(&scratch, KEG);
    // What a computation records of the formulas that computed each value.
    let line = format!(
        "'{}' compute samples --write",
        env!("CARGO_BIN_EXE_samplekit")
    );
    let mut child = Command::new("script")
        .args(["--quiet", "--return", "--command", &line, "/dev/null"])
        .current_dir(scratch.at("project"))
        .env_remove("SAMPLEKIT_STATE_DIR")
        .env("XDG_STATE_HOME", scratch.at("xdg"))
        .env("HOME", scratch.at("home"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script creates a pseudoterminal");
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        scratch.exists("xdg/samplekit/computed.json"),
        "{}{}",
        out(&output),
        err(&output)
    );
    assert!(!scratch.exists("home/.local"));
}

#[test]
fn a_computation_names_its_configuration_and_model() {
    let scratch = Scratch::new("names");
    project(&scratch, KEG);
    let output = run(
        &scratch,
        &["compute", "samples", "--write", "--show-output"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = err(&output);
    let configuration = written
        .find(&format!(
            "configuration  {}",
            scratch.at("project/.samplekitrc").display()
        ))
        .unwrap_or_else(|| panic!("{written}"));
    let model = written
        .find(&format!(
            "model          {}",
            scratch.at("project/model.py").display()
        ))
        .unwrap_or_else(|| panic!("{written}"));
    let computing = written.find("computing the plato").unwrap();
    assert!(configuration < model && model < computing, "{written}");
}

#[test]
fn a_value_a_sample_does_not_have_is_reported_not_raised() {
    let scratch = Scratch::new("unknown-value");
    project(&scratch, KEG);
    // Another project inside the selection, whose model has no plato.
    scratch.write(
        "project/samples/other/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"other.py\"\n",
    );
    scratch.write(
        "project/samples/other/other.py",
        "import samplekit as sk\n\n\nclass Other(sk.Sample):\n    pass\n",
    );
    scratch.write(
        "project/samples/other/o1.md",
        "---\nschema_version: 1\nname: O1\n---\n",
    );
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n[collection]\nrecursive = true\n",
    );

    let mixed = run(&scratch, &["compute", "samples", "-p", "plato", "--write"]);
    assert_eq!(code(&mixed), 0, "{}", err(&mixed));
    assert!(
        err(&mixed).contains("O1: unknown value 'plato'"),
        "{}",
        err(&mixed)
    );
    assert!(!err(&mixed).contains("Traceback"), "{}", err(&mixed));
    assert!(scratch.read("project/samples/c1.md").contains("plato:"));

    let none = run(&scratch, &["compute", "samples", "-p", "plto"]);
    assert_eq!(code(&none), 1, "{}", err(&none));
    assert!(
        err(&none).contains("no selected sample has plto"),
        "{}",
        err(&none)
    );
    assert!(!err(&none).contains("Traceback"), "{}", err(&none));
}

#[test]
fn stale_lists_values_never_computed_when_the_model_may_run() {
    let scratch = Scratch::new("stale-never-computed");
    project(&scratch, KEG);
    let output = run(&scratch, &["status", "samples"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(
        text.contains("plato") && text.contains("never computed"),
        "{text}"
    );
    assert!(text.contains("C1") && text.contains("C2"), "{text}");
    assert!(!scratch.exists("project/ran"), "a formula ran");
}

#[test]
fn a_repeated_error_prints_its_traceback_once() {
    let scratch = Scratch::new("repeated-error");
    project(&scratch, KEG);
    keg(&scratch, "c0", 13.0, 0.0);
    keg(&scratch, "c3", 11.0, 0.0);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let errors = err(&output);
    assert!(
        errors.matches("Traceback (most recent call last)").count() <= 2,
        "{errors}"
    );
    assert!(errors.contains("the same error as above"), "{errors}");
}

#[test]
fn an_unknown_value_suggests_the_nearest() {
    let scratch = Scratch::new("unknown-value-nearest");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "-p", "plto"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("did you mean: plato?"),
        "{}",
        err(&output)
    );
}

#[test]
fn an_interrupted_run_keeps_what_finished_and_not_what_was_running() {
    let scratch = Scratch::new("interrupted");
    project(&scratch, SLOW);
    let started = std::time::Instant::now();
    let _ = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    // Past the moment the second value would have finished, had the worker
    // outlived the command line.
    let waited = std::time::Duration::from_secs(5).saturating_sub(started.elapsed());
    std::thread::sleep(waited);
    let text = scratch.read("project/samples/c1.md");
    assert!(text.contains("first:"), "{text}");
    // Declared and never finished: not in the file at all.
    let second = text
        .lines()
        .find(|line| line.trim_start().starts_with("second:"));
    // Nothing without a value is written: the second is absent.
    assert_eq!(second, None, "{text}");
    assert!(
        files_under(&scratch.at("project/samples"))
            .iter()
            .all(|file| file.extension().is_some_and(|extension| extension == "md")),
        "a temporary file was left behind"
    );
}

#[test]
fn a_model_that_will_not_load_is_an_error_not_nothing_to_compute() {
    let scratch = Scratch::new("model-will-not-load");
    project(
        &scratch,
        "import samplekit as sk\n\n\nclass Broken(sk.Sample):\n    def __init__(self, path=None, name=None):\n        super().__init__(path, name=name)\n        raise RuntimeError(\"the model is broken\")\n",
    );
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        !out(&output).contains("nothing to compute"),
        "{}",
        out(&output)
    );
    assert_eq!(
        err(&output)
            .matches("RuntimeError: the model is broken")
            .count(),
        1,
        "{}",
        err(&output)
    );
}

#[test]
fn explain_names_the_inputs_of_a_computed_cell() {
    let scratch = Scratch::new("explain-cell");
    project(&scratch, SHAPES);
    let computed = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let output = run(&scratch, &["explain", "samples/c1.md", "mashing.G[10]"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("computed from"), "{text}");
    assert!(text.contains("row.R"), "{text}");
}

#[test]
fn a_sample_that_cannot_be_saved_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("not-saved");
    project(&scratch, KEG);
    let samples = scratch.at("project/samples");
    fs::set_permissions(&samples, fs::Permissions::from_mode(0o555)).unwrap();
    let output = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    fs::set_permissions(&samples, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(out(&output).contains("not saved"), "{}", out(&output));
    assert!(
        !out(&output).contains("raised while loading"),
        "{}",
        out(&output)
    );
}

#[test]
fn a_narrowed_run_counts_what_it_left_stale() {
    let scratch = Scratch::new("narrowed-left-stale");
    project(&scratch, KEG);
    let all = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&all), 0, "{}", err(&all));
    let path = "project/samples/c1.md";
    let changed = scratch
        .read(path)
        .replace("malt: {v: 12.0", "malt: {v: 13.0");
    scratch.write(path, &changed);
    let narrowed = run(&scratch, &["compute", "samples", "-p", "plato", "--write"]);
    assert_eq!(code(&narrowed), 0, "{}", err(&narrowed));
    assert!(
        out(&narrowed).contains("1 value left outdated"),
        "{}",
        out(&narrowed)
    );
}

#[test]
fn progress_reports_each_value_and_a_duration_only_past_a_second() {
    let scratch = Scratch::new("progress");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    let finished: Vec<&str> = report.lines().filter(|line| line.contains('✓')).collect();
    assert_eq!(finished.len(), 4, "{report}");
    // Each formula here takes far less than a second: no duration is shown.
    for line in &finished {
        assert!(!line.trim_end().ends_with('s'), "{line}");
    }
    assert!(
        finished[0].contains("C1") && finished[0].contains("plato"),
        "{report}"
    );
    assert!(
        report.contains("computed 4 values in 2 samples"),
        "{report}"
    );
}

#[test]
fn a_text_input_is_refused_before_its_formula_runs() {
    let scratch = Scratch::new("text-input");
    project(&scratch, KEG);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  malt: {v: \"12.0\", unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        out(&output).contains("malt is text: \"12.0\""),
        "{}",
        out(&output)
    );
    assert!(!err(&output).contains("TypeError"), "{}", err(&output));
    assert!(out(&output).contains("✓ C1"), "{}", out(&output));
}

#[test]
fn stale_says_why_it_did_not_read_the_model() {
    let scratch = Scratch::new("stale-why");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        KEG,
    );
    let unavailable = run(&scratch, &["status", "samples"]);
    assert_eq!(code(&unavailable), 0, "{}", err(&unavailable));
    assert!(
        err(&unavailable).contains("2 samples have a model that was not read")
            && err(&unavailable).contains("no environment found"),
        "{}",
        err(&unavailable)
    );
    assert!(
        !out(&unavailable).contains("every derived value"),
        "{}",
        out(&unavailable)
    );
    assert!(!scratch.exists("project/imported"));
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"missing.py\"\n",
    );
    let missing = run(&scratch, &["status", "samples"]);
    assert!(
        err(&missing).contains("cannot be found"),
        "{}",
        err(&missing)
    );
}

#[test]
fn a_failure_is_written_and_cleared_by_the_next_success() {
    let scratch = Scratch::new("failure-written");
    project(&scratch, KEG);
    keg(&scratch, "c0", 13.0, 0.0);
    let failed = run(&scratch, &["compute", "samples/c0.md", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let text = scratch.read("project/samples/c0.md");
    // The file keeps the exception's type alone; the traceback lives beside the
    // project, so that a sample stays data and stays portable.
    assert!(
        text.contains("fingerprint: {failed: ZeroDivisionError}"),
        "{text}"
    );
    assert!(!text.contains("division by zero"), "{text}");
    let log = scratch.read("project/.samplekit/failures/samples/c0.log");
    assert!(log.contains("## plato"), "{log}");
    // Haze waited behind it, and has no failure of its own.
    assert!(!log.contains("## haze"), "{log}");
    assert!(
        log.contains("ZeroDivisionError") && log.contains("division by zero"),
        "{log}"
    );
    let stale = run(&scratch, &["status", "samples/c0.md"]);
    assert!(out(&stale).contains("failed"), "{}", out(&stale));
    scratch.write(
        "project/samples/c0.md",
        &text.replace("volume: {v: 0.0", "volume: {v: 4.0"),
    );
    let fixed = run(&scratch, &["compute", "samples/c0.md", "--write"]);
    assert_eq!(code(&fixed), 0, "{}", err(&fixed));
    let text = scratch.read("project/samples/c0.md");
    assert!(!text.contains("failed"), "{text}");
    assert!(text.contains("plato:"), "{text}");
}

#[test]
fn try_computes_and_writes_nothing() {
    let scratch = Scratch::new("try");
    project(&scratch, KEG);
    let before = scratch.read("project/samples/c1.md");
    let output = run(&scratch, &["compute", "samples", "--try"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("nothing written"), "{}", out(&output));
    assert_eq!(scratch.read("project/samples/c1.md"), before);
    assert!(scratch.exists("project/ran"));
}

#[test]
fn write_shows_before_and_after() {
    let scratch = Scratch::new("before-after");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("before") && text.contains("after"), "{text}");
    assert!(text.contains("plato") && text.contains(" 3"), "{text}");
}

#[test]
fn an_interruption_says_what_it_kept() {
    let scratch = Scratch::new("interruption-said");
    project(&scratch, SLOW);
    let output = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(output.status.code(), Some(130), "{}", err(&output));
    assert!(
        err(&output).contains("interrupted: 1 value kept in 1 sample"),
        "{}",
        err(&output)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn try_in_a_terminal_writes_what_it_computed_when_told() {
    let scratch = Scratch::new("try-then-write");
    project(&scratch, KEG);
    // Nothing is asked before the offer to write.
    let output = at_terminal(&scratch, &["compute", "samples", "--try"], "y\n");
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert!(
        out(&output).contains("write these values?"),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("written: 2 samples"),
        "{}",
        out(&output)
    );
    assert!(scratch.read("project/samples/c1.md").contains("plato:"));
}

/// One value that fails beside one that computes, so that a rehearsal has
/// something to offer to write and a failure to write with it.
const MIXED: &str = r#"import samplekit as sk


class Mixed(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.doubled = sk.Property(compute=lambda: self.malt.value * 2)
        self.plato = sk.Property(compute=lambda: self.malt.value / self.volume.value)
        self.set_dependencies("doubled", depends_on=["malt"])
        self.set_dependencies("plato", depends_on=["malt", "volume"])
"#;

#[test]
#[cfg(target_os = "linux")]
fn try_writes_no_failure_log_until_the_failure_is_written() {
    // The log follows the file. *Computes and writes nothing* covers
    // the log; and a rehearsal written afterwards writes the traceback of the
    // failure it saves.
    let scratch = Scratch::new("try-failure-log");
    project(&scratch, MIXED);
    keg(&scratch, "c0", 13.0, 0.0);
    let before = scratch.read("project/samples/c0.md");
    let tried = run(&scratch, &["compute", "samples/c0.md", "--try"]);
    assert_eq!(code(&tried), 2, "{}", err(&tried));
    assert!(err(&tried).contains("ZeroDivisionError"), "{}", err(&tried));
    assert_eq!(scratch.read("project/samples/c0.md"), before);
    assert!(
        !scratch.exists("project/.samplekit/failures/samples/c0.log"),
        "a rehearsal wrote a failure log"
    );
    assert!(!scratch.exists("project/.samplekit/failures"));

    // *write these values?* answered yes.
    let written = at_terminal(&scratch, &["compute", "samples/c0.md", "--try"], "y\n");
    assert!(
        out(&written).contains("write these values?"),
        "{}",
        out(&written)
    );
    let text = scratch.read("project/samples/c0.md");
    assert!(text.contains("doubled:"), "{text}");
    assert!(
        text.contains("fingerprint: {failed: ZeroDivisionError}"),
        "{text}"
    );
    let log = scratch.read("project/.samplekit/failures/samples/c0.log");
    assert!(log.contains("## plato"), "{log}");
    assert!(
        log.contains("ZeroDivisionError") && log.contains("division by zero"),
        "{log}"
    );
}

#[test]
fn before_and_after_count_the_cells_a_column_changed() {
    let scratch = Scratch::new("before-after-cells");
    project(&scratch, SHAPES);
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(out(&first).contains("2 new"), "{}", out(&first));
    let text = scratch.read("project/samples/c1.md");
    assert!(text.contains("R: 4.0"), "{text}");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("R: 4.0", "R: 8.0", 1),
    );
    let second = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    assert!(out(&second).contains("1 changed"), "{}", out(&second));
}

#[test]
fn write_says_how_many_samples_it_wrote() {
    let scratch = Scratch::new("write-says");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("written: 2 samples"),
        "{}",
        out(&output)
    );
}

#[test]
fn readings_with_no_statistic_chosen_are_said_so_with_the_ways_out() {
    // Forcing a value beside dosings the model chooses no statistic for
    // has nothing to give back: said as that state, never as a missing value.
    let scratch = Scratch::new("no-statistic-named");
    project(&scratch, KEG);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  \
         malt: {v: 13.0, readings: [12.0, 12.2], unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(
        &scratch,
        &["compute", "samples/c1.md", "-p", "malt", "--force", "--try"],
    );
    let said = err(&output);
    assert_eq!(code(&output), 1, "{said}");
    assert!(said.contains("no statistic chosen"), "{said}");
    // No mean is offered in its place.
    assert!(said.contains("a value written beside them"), "{said}");
    assert!(!said.contains("their mean"), "{said}");
    assert!(!said.contains("no selected sample has"), "{said}");
}

#[test]
fn a_value_without_a_formula_is_said_without_a_traceback() {
    let scratch = Scratch::new("no-formula-named");
    project(&scratch, KEG);
    let output = run(
        &scratch,
        &["compute", "samples/c1.md", "-p", "malt", "--try"],
    );
    let said = format!("{}{}", out(&output), err(&output));
    assert!(!said.contains("Traceback"), "{said}");
    assert!(said.contains("no formula"), "{said}");
}

#[test]
fn compute_says_the_overrides_it_kept() {
    let scratch = Scratch::new("overrides-kept");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    assert!(text.contains("v: 3.0"), "{text}");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("v: 3.0", "v: 9.0", 1),
    );
    let listed = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("1 value edited by hand is kept"),
        "{}",
        out(&listed)
    );
    let forced = run(&scratch, &["compute", "samples", "--force"]);
    assert!(!out(&forced).contains("kept"), "{}", out(&forced));
}

#[test]
fn stale_says_an_uncertainty_never_computed_beside_an_entered_value() {
    let scratch = Scratch::new("uncertainty-never");
    project(&scratch, SHAPES);
    // Entered in the file, as a ibu is; only its uncertainty is a formula's.
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("unit: L}\n---", "unit: L}\n  age: {v: 2.0}\n---", 1),
    );
    let output = run(&scratch, &["status", "samples"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("uncertainty never computed"),
        "{}",
        out(&output)
    );
}

const RESOLUTION: &str = r#"import samplekit as sk


class Gauge(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.resolution = sk.Property(unit="mL")
        # An entered age whose uncertainty an instrument's resolution gives.
        self.age = sk.Property(
            unit="mL",
            compute_uncertainty=lambda: self.resolution.value / 2,
            depends_on=["resolution"],
        )
"#;

const DOSED: &str = r#"import samplekit as sk


class Dosed(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.volume = sk.Property(unit="L")
        self.brix = sk.Property(unit="g/L", compute=lambda: self.malt.value / self.volume.value, depends_on=["malt", "volume"])
"#;

#[test]
fn a_corrected_reading_is_recomputed_without_force() {
    // `status` had learnt to say *stale — readings* and `compute` could not act
    // on it — the plan took every reloaded statistic for an override, ran what
    // read it from the old mean, and left the sample stale for ever.
    let scratch = Scratch::new("corrected-dosing");
    project(&scratch, DOSED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "malt: {v: 12.0, unit: g}",
            "malt: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let settled = run(&scratch, &["status", "samples/c1.md"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));

    // One dosing corrected by hand, in an editor.
    let computed = scratch.read("project/samples/c1.md");
    assert!(computed.contains("[11.0, 12.0, 13.0]"), "{computed}");
    scratch.write(
        "project/samples/c1.md",
        &computed.replacen("[11.0, 12.0, 13.0]", "[11.0, 12.0, 16.0]", 1),
    );
    let stale = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );

    // An ordinary compute, no --force: malt first, then what reads it.
    let repaired = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&repaired), 0, "{}", err(&repaired));
    assert!(!out(&repaired).contains("kept"), "{}", out(&repaired));
    let after = scratch.read("project/samples/c1.md");
    assert!(after.contains("v: 13.0"), "{after}");
    assert!(after.contains("v: 3.25"), "{after}");
    let settled = run(&scratch, &["status", "samples/c1.md"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));
}

const UNDECLARED_READ: &str = r#"import samplekit as sk


class Keg(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.shell = sk.Property(unit="g")
        # Reads `shell`, and says only `malt`: where shell is absent it raises.
        self.net = sk.Property(unit="g", compute=lambda: self.malt.value - self.shell.value,
                               depends_on=["malt"])
"#;

#[test]
fn a_failure_the_model_no_longer_gives_is_cleared() {
    // The model fixed, the value that had failed now waits for its input: the
    // file went on saying *failed*, and its traceback stayed in the log.
    let scratch = Scratch::new("failure-cleared");
    project(&scratch, UNDECLARED_READ);
    let failed = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let log = scratch.at("project/.samplekit/failures/samples/c1.log");
    assert!(log.exists(), "the traceback is logged");
    scratch.write(
        "project/model.py",
        &UNDECLARED_READ.replace(r#"depends_on=["malt"]"#, r#"depends_on=["malt", "shell"]"#),
    );
    let fixed = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&fixed), 0, "{}", err(&fixed));
    let status = run(&scratch, &["status", "samples"]);
    assert!(!out(&status).contains("failed"), "{}", out(&status));
    assert!(out(&status).contains("waits for shell"), "{}", out(&status));
    assert!(!log.exists(), "the log goes with the failure");
}

#[test]
fn a_write_without_the_model_leaves_readings_alone() {
    // A command without the model — a tag — writes readings with no value
    // beside them. Computed under a model declaring the median, they take it
    // and read current; corrected, *stale — readings*, never *record missing*.
    let scratch = Scratch::new("readings-alone");
    project(
        &scratch,
        &DOSED.replace("value=sk.stats.mean", "value=sk.stats.median"),
    );
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "malt: {v: 12.0, unit: g}",
            "malt: {readings: [11.0, 12.0, 16.0], unit: g}",
            1,
        ),
    );
    let tagged = run(
        &scratch,
        &["tag", "add", "dosed", "samples/c1.md", "--write"],
    );
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let written = scratch.read("project/samples/c1.md");
    assert!(
        written.contains("malt: {readings: [11.0, 12.0, 16.0], unit: g}"),
        "no mean written beside them: {written}"
    );
    let computed = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    assert!(
        scratch.read("project/samples/c1.md").contains("v: 12.0"),
        "the median"
    );
    let status = run(&scratch, &["status", "samples/c1.md"]);
    assert!(out(&status).contains("is current"), "{}", out(&status));
    let computed = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &computed.replacen("[11.0, 12.0, 16.0]", "[11.0, 13.0, 16.0]", 1),
    );
    let stale = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );
    assert!(!out(&stale).contains("record missing"), "{}", out(&stale));
}

const ROWS_READ_A_PROPERTY: &str = r#"import samplekit as sk


class Keg(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.shell = sk.Property(unit="g")
        self.runs = sk.Table(
            {"run": sk.Column(), "load": sk.Column(), "net": sk.Column()},
            "run",
            compute_rows=[(["net"], ["row.load", "shell"], self._net)],
        )

    def _net(self, row):
        return {"net": row.load.value - self.shell.value}
"#;

#[test]
fn a_tables_rows_wait_for_an_input_nobody_entered() {
    // Properties wait; a column computed row by row ran over the
    // absent `shell` and failed, once per sample.
    let scratch = Scratch::new("rows-wait");
    project(&scratch, ROWS_READ_A_PROPERTY);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "\n---\n",
            "\ntables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      load: {}\n      \
             net: {}\n    rows:\n      - {run: 1, load: 3.5}\n---\n",
            1,
        ),
    );
    let output = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("waits for shell"), "{}", out(&output));
    assert!(!out(&output).contains("failed"), "{}", out(&output));
}

#[test]
fn a_project_that_cannot_run_stops_only_itself() {
    // One project's missing model stopped the whole run, the other projects
    // never computed.
    let scratch = Scratch::new("one-project-stops");
    project(&scratch, DOSED);
    scratch.write(
        "project/broken/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"missing.py\"\n",
    );
    scratch.write(
        "project/broken/b1.md",
        "---\nschema_version: 1\nname: B1\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let output = run(&scratch, &["compute", "broken", "samples", "--write"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(err(&output).contains("broken"), "{}", err(&output));
    assert!(
        out(&output).contains("computed"),
        "the other project ran: {}",
        out(&output)
    );
    assert!(scratch.read("project/samples/c1.md").contains("computed:"));
}

#[test]
fn the_plan_says_failed_and_waiting_as_status_does() {
    // The plan called a failed value and a waiting one *never computed*, and
    // counted what would wait among what would be computed.
    let scratch = Scratch::new("plan-says");
    project(&scratch, UNDECLARED_READ);
    let failed = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let plan = run(&scratch, &["compute", "samples"]);
    assert!(out(&plan).contains("failed"), "{}", out(&plan));
    scratch.write(
        "project/model.py",
        &UNDECLARED_READ.replace(r#"depends_on=["malt"]"#, r#"depends_on=["malt", "shell"]"#),
    );
    let plan = run(&scratch, &["compute", "samples"]);
    assert!(out(&plan).contains("waits for shell"), "{}", out(&plan));
    assert!(
        out(&plan).contains("nothing would be computed"),
        "{}",
        out(&plan)
    );
}

#[test]
fn a_rerun_that_changes_nothing_writes_no_sample() {
    // "11 samples written" was said where 4 files had changed: a value
    // computed again to the same number rewrites nothing.
    let scratch = Scratch::new("rerun-written-count");
    project(&scratch, DOSED);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let again = run(&scratch, &["compute", "samples", "--rerun", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("written: 0 samples"),
        "{}",
        out(&again)
    );
}

#[test]
fn readings_set_over_a_statistic_leave_it_stale_not_edited() {
    // New readings over a statistic once wrote the new mean over it, the digest
    // moved, and the value read *edited*: a hand's, which compute then left
    // alone. Given as the readings channel.
    let scratch = Scratch::new("set-readings-statistic");
    project(&scratch, DOSED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "malt: {v: 12.0, unit: g}",
            "malt: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let set = run(
        &scratch,
        &[
            "set",
            "samples/c1.md",
            "malt.readings=11.0,12.0,16.0",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let stale = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );
    assert!(!out(&stale).contains("edited"), "{}", out(&stale));
    let repaired = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&repaired), 0, "{}", err(&repaired));
    // What it is about to compute is not said stale in the middle of the run.
    assert!(
        !err(&repaired).contains("is outdated"),
        "{}",
        err(&repaired)
    );
    assert!(scratch.read("project/samples/c1.md").contains("v: 13.0"));
    let settled = run(&scratch, &["status", "samples/c1.md"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));
}

#[test]
fn readings_no_statistic_was_taken_of_are_never_computed_everywhere() {
    // The demo's blonde saison held its gravities as readings alone. The table
    // showed `—`, and `status`, `compute` and Python's `not_current` said
    // nothing of them: the model gives the statistic as it reads the sample, so
    // the plan found nothing owed, and `explain` said it *waits for readings*
    // the file held.
    let scratch = Scratch::new("statistic-never-taken");
    project(&scratch, DOSED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "malt: {v: 12.0, unit: g}",
            "malt: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    // A sample whose statistic was taken, for `explain` to learn from.
    let other = run(&scratch, &["compute", "samples/c2.md", "--write"]);
    assert_eq!(code(&other), 0, "{}", err(&other));
    let status = run(&scratch, &["status", "samples/c1.md"]);
    let line = out(&status)
        .lines()
        .find(|line| line.contains(" malt "))
        .map(str::to_string);
    assert!(
        line.as_deref()
            .is_some_and(|line| line.contains("never computed")),
        "{}",
        out(&status)
    );
    let plan = run(&scratch, &["compute", "samples/c1.md"]);
    assert!(
        out(&plan)
            .lines()
            .any(|line| line.contains(" malt ") && line.contains("never computed")),
        "{}",
        out(&plan)
    );
    let explain = run(&scratch, &["explain", "samples/c1.md", "malt"]);
    assert!(
        !out(&explain).contains("waits for readings"),
        "{}",
        out(&explain)
    );
    let python = Command::new(venv().join("bin/python"))
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .args([
            "-c",
            "import samplekit as sk\ns = sk.load('samples/c1.md')\n\
             print(s.not_current().get('malt'), '|', s.malt.state)",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&python.stdout).trim(),
        "never computed | never computed",
        "{}",
        String::from_utf8_lossy(&python.stderr)
    );
    // Taken, it is current everywhere.
    let written = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(scratch.read("project/samples/c1.md").contains("v: 12.0"));
    let settled = run(&scratch, &["status", "samples/c1.md"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));
}

#[test]
fn a_computed_uncertainty_carries_a_record_and_goes_stale() {
    // One record per property, covering both channels.
    let scratch = Scratch::new("uncertainty-record");
    project(&scratch, RESOLUTION);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "unit: L}\n---",
            "unit: L}\n  resolution: {v: 0.01, unit: mL}\n  age: {v: 2.0, unit: mL}\n---",
            1,
        ),
    );
    let written = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let after = scratch.read("project/samples/c1.md");
    let age = after
        .lines()
        .skip_while(|line| !line.starts_with("  age:"))
        .take_while(|line| line.starts_with("  age:") || line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(age.contains("u: 0.005"), "{after}");
    // The uncertainty alone has a formula, so the record names its channel.
    assert!(age.contains("computed: {u: {resolution: "), "{after}");
    assert!(age.contains("fingerprint: "), "{after}");

    let explained = run(&scratch, &["explain", "samples/c1.md", "age"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("computed from"),
        "{}",
        out(&explained)
    );
    assert!(
        out(&explained).contains("resolution"),
        "{}",
        out(&explained)
    );
    assert!(
        out(&explained).contains("this value is current"),
        "{}",
        out(&explained)
    );

    // The input moves: the uncertainty is outdated.
    let changed = run(
        &scratch,
        &["set", "samples/c1.md", "resolution=0.02", "--write"],
    );
    assert_eq!(code(&changed), 0, "{}", err(&changed));
    assert!(out(&changed).contains("age"), "{}", out(&changed));
    let status = run(&scratch, &["status", "samples/c1.md"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("age") && out(&status).contains("outdated — resolution"),
        "{}",
        out(&status)
    );
    let again = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        scratch.read("project/samples/c1.md").contains("u: 0.01,")
            || scratch.read("project/samples/c1.md").contains("u: 0.01\n"),
        "{}",
        scratch.read("project/samples/c1.md")
    );

    // Supplied without its record, a computed uncertainty is record missing.
    let text = scratch.read("project/samples/c1.md");
    // The record names its channel, so it nests: `computed: {u: {resolution:
    // …}}`, and the cut goes to the matching brace.
    let start = text.find("computed: {u: {resolution").expect("a record");
    let opened = start + text[start..].find('{').expect("a record opens");
    let end = opened
        + text[opened..]
            .char_indices()
            .scan(0i32, |depth, (at, c)| {
                match c {
                    '{' => *depth += 1,
                    '}' => *depth -= 1,
                    _ => {}
                }
                Some((at, *depth))
            })
            .find(|(_, depth)| *depth == 0)
            .map(|(at, _)| at)
            .expect("a closing brace")
        + 1;
    let line_start = text[..start].rfind('\n').unwrap();
    scratch.write(
        "project/samples/c1.md",
        &format!("{}{}", &text[..line_start], &text[end..]),
    );
    let missing = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&missing).contains("age") && out(&missing).contains("record missing"),
        "{}",
        out(&missing)
    );
}

#[test]
fn a_derived_value_without_its_record_is_an_override_until_forced() {
    let scratch = Scratch::new("record-missing");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    let start = text.find("computed: {").expect("a record");
    // To the **matching** brace: a record that names its channel nests one map
    // inside another, and cutting at the first `}` left a stray one.
    let opened = start + text[start..].find('{').expect("a record opens");
    let end = opened
        + text[opened..]
            .char_indices()
            .scan(0i32, |depth, (at, c)| {
                match c {
                    '{' => *depth += 1,
                    '}' => *depth -= 1,
                    _ => {}
                }
                Some((at, *depth))
            })
            .find(|(_, depth)| *depth == 0)
            .map(|(at, _)| at)
            .expect("a closing brace")
        + 1;
    let (from, to) = match text[..start].rfind('\n') {
        Some(line) if text[line + 1..start].trim().is_empty() => (line, end),
        _ => (text[..start].rfind(", ").unwrap(), end),
    };
    scratch.write(
        "project/samples/c1.md",
        &format!("{}{}", &text[..from], &text[to..]),
    );
    let stale = run(&scratch, &["status", "samples"]);
    assert!(out(&stale).contains("record missing"), "{}", out(&stale));
    let forced = run(&scratch, &["compute", "samples", "--force", "--write"]);
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    let again = run(&scratch, &["status", "samples"]);
    assert!(!out(&again).contains("record missing"), "{}", out(&again));
}

#[test]
fn try_says_the_comments_a_write_drops() {
    let scratch = Scratch::new("try-comments");
    project(&scratch, KEG);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("name: C1\n", "name: C1\n# dosed twice\n", 1),
    );
    let output = run(&scratch, &["compute", "samples/c1.md", "--try"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a write drops 1 YAML comment"),
        "{}",
        out(&output)
    );
}

/// `status` over the project's samples, the model allowed to run.
fn status(scratch: &Scratch) -> Output {
    run(scratch, &["status", "samples"])
}

/// The lines of `status` naming `value` with `state`.
fn said(output: &Output, value: &str, state: &str) -> usize {
    out(output)
        .lines()
        .filter(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.get(1) == Some(&value) && line.contains(state)
        })
        .count()
}

#[test]
fn a_new_figure_makes_nothing_stale() {
    // A figure, a comment, a docstring and a helper nobody calls change no
    // formula: nothing is stale, and no model is said changed.
    let scratch = Scratch::new("new-figure");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let edited = KEG
        .replace(
            "    def _plato(self):\n",
            "    def _plato(self):\n        \"\"\"Malt over volume.\"\"\"\n        # g/L\n",
        )
        .replace(
            "class Keg(sk.Sample):",
            "def unused(x):\n    return x * 2\n\n\nclass Keg(sk.Sample):",
        )
        + "\n    @sk.figure\n    def curve(self, ax):\n        ax.plot([1], [self.plato.value])\n";
    scratch.write("project/model.py", &edited);
    let status = status(&scratch);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("every derived value in 2 samples is current"),
        "{}",
        out(&status)
    );
    assert!(!err(&status).contains("model changed"), "{}", err(&status));
    let plan = run(&scratch, &["compute", "samples"]);
    assert!(
        out(&plan).contains("nothing to compute in 2 samples"),
        "{}",
        out(&plan)
    );
}

#[test]
fn an_edited_formula_stales_its_values_and_what_reads_them() {
    let scratch = Scratch::new("edited-formula");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &KEG.replace(
            "return self.malt.value / self.volume.value",
            "return 2 * self.malt.value / self.volume.value",
        ),
    );
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "plato", "formula changed"),
        2,
        "{}",
        out(&stale)
    );
    assert_eq!(
        said(&stale, "haze", "reads plato, whose formula changed"),
        2,
        "{}",
        out(&stale)
    );
    assert!(!err(&stale).contains("model changed"), "{}", err(&stale));
    // Computed without --rerun: a changed formula is not current.
    let second = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    assert!(
        scratch.read("project/samples/c1.md").contains("v: 6.0"),
        "{}",
        scratch.read("project/samples/c1.md")
    );
    let current = status(&scratch);
    assert!(
        out(&current).contains("every derived value in 2 samples is current"),
        "{}",
        out(&current)
    );
}

#[test]
fn a_changed_column_statistic_stales_its_cells() {
    let scratch = Scratch::new("column-statistic");
    let viscous = KEG.replace(
        "        self.set_dependencies(\"plato\"",
        "        self.mouthfeel = sk.Table(\n            {\"T\": sk.Column(), \"sweetness\": sk.Column(unit=\"cP\", \
         value=sk.stats.mean)},\n            \"T\",\n        )\n        self.set_dependencies(\"plato\"",
    );
    project(&scratch, &viscous);
    let added = run(
        &scratch,
        &[
            "set",
            "samples/c1.md",
            "--add-row",
            "mouthfeel",
            "T=40",
            "sweetness.readings=[1,2,6]",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(
        out(&status(&scratch)).contains("is current"),
        "{}",
        out(&status(&scratch))
    );
    // The column's statistic is its cells' formula: changed, they are not
    // current, and computed again they take the new one.
    scratch.write(
        "project/model.py",
        &viscous.replace("value=sk.stats.mean)", "value=sk.stats.median)"),
    );
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "mouthfeel.sweetness", "formula changed"),
        1,
        "{}",
        out(&stale)
    );
    let second = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let file = scratch.read("project/samples/c1.md");
    assert!(file.contains("v: 2"), "{file}");
    assert!(
        out(&status(&scratch)).contains("is current"),
        "{}",
        out(&status(&scratch))
    );
}

#[test]
fn an_edited_helper_stales_the_formulas_that_reach_it() {
    let scratch = Scratch::new("edited-helper");
    let helped = KEG
        .replace(
            "class Keg(sk.Sample):",
            "SCALE = 1.0\n\n\ndef scaled(x):\n    return x * SCALE\n\n\n\
             def unused(x):\n    return x\n\n\nclass Keg(sk.Sample):",
        )
        .replace(
            "return self.malt.value / self.volume.value",
            "return scaled(self.malt.value / self.volume.value)",
        );
    project(&scratch, &helped);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    // A helper nobody calls.
    scratch.write(
        "project/model.py",
        &helped.replace("    return x\n", "    return x + 1\n"),
    );
    let untouched = status(&scratch);
    assert!(
        out(&untouched).contains("every derived value in 2 samples is current"),
        "{}",
        out(&untouched)
    );
    // The constant the formula's helper reads.
    scratch.write(
        "project/model.py",
        &helped.replace("SCALE = 1.0", "SCALE = 2.0"),
    );
    let constant = status(&scratch);
    assert_eq!(
        said(&constant, "plato", "formula changed"),
        2,
        "{}",
        out(&constant)
    );
    // The helper itself.
    scratch.write(
        "project/model.py",
        &helped.replace("return x * SCALE", "return x * SCALE * 1.5"),
    );
    let helper = status(&scratch);
    assert_eq!(
        said(&helper, "plato", "formula changed"),
        2,
        "{}",
        out(&helper)
    );
}

#[test]
fn a_record_of_the_whole_model_is_mapped_or_said() {
    // What an earlier version recorded: the whole model's digest, a line per
    // sample, in `computed`.
    let legacy = |scratch: &Scratch, digest: &str| {
        let _ = fs::remove_file(scratch.at("state/computed.json"));
        let lines: String = ["c1", "c2"]
            .iter()
            .map(|name| {
                let path =
                    dunce::canonicalize(scratch.at(&format!("project/samples/{name}.md"))).unwrap();
                format!("{digest} {}\n", path.display())
            })
            .collect();
        scratch.write("state/computed", &lines);
    };
    let scratch = Scratch::new("legacy-record");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));

    // Under the model it names: the next run, computing nothing, records each
    // formula, and an edit afterwards stales its formula alone.
    legacy(&scratch, &permit_digest(&scratch));
    let mapped = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&mapped), 0, "{}", err(&mapped));
    let formulas = runtime::ComputedStore::at(&scratch.at("state"))
        .recorded_formulas(&scratch.at("project/samples/c1.md"));
    assert!(
        formulas.contains_key("plato") && formulas.contains_key("haze"),
        "{formulas:?}"
    );
    scratch.write(
        "project/model.py",
        &KEG.replace(
            "return self.malt.value / self.volume.value",
            "return 2 * self.malt.value / self.volume.value",
        ),
    );
    let edited = status(&scratch);
    assert_eq!(
        said(&edited, "plato", "formula changed"),
        2,
        "{}",
        out(&edited)
    );
    assert!(!err(&edited).contains("model changed"), "{}", err(&edited));

    // Under another model: which formula changed is not known, and is said as
    // it was — until a run computes them.
    scratch.write("project/model.py", KEG);
    legacy(&scratch, &"0".repeat(64));
    let unknown = status(&scratch);
    assert!(
        err(&unknown).contains("the model changed since the values of 2 samples were computed"),
        "{}",
        err(&unknown)
    );
    let nothing = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&nothing), 0, "{}", err(&nothing));
    let still = status(&scratch);
    assert!(
        err(&still).contains("the model changed since the values of 2 samples were computed"),
        "{}",
        err(&still)
    );
    let rerun = run(&scratch, &["compute", "samples", "--rerun", "--write"]);
    assert_eq!(code(&rerun), 0, "{}", err(&rerun));
    let known = status(&scratch);
    assert!(!err(&known).contains("model changed"), "{}", err(&known));
}

#[test]
fn a_formula_edited_by_its_run_is_said_changed() {
    // The digest recorded is of the formula as the run imported it: taken from
    // the file after the run, the second sample was recorded as computed by a
    // version that never ran.
    let scratch = Scratch::new("edited-by-its-run");
    project(
        &scratch,
        &KEG.replace(
            "        return self.malt.value / self.volume.value\n",
            "        text = open(__file__).read()\n        \
             open(__file__, \"w\").write(text.replace(\"+ 0.0  # offset\", \"+ 1.0  # offset\"))\n        \
             return self.malt.value / self.volume.value + 0.0  # offset\n",
        ),
    );
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(scratch.read("project/model.py").contains("+ 1.0  # offset"));
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "plato", "formula changed"),
        2,
        "{}",
        out(&stale)
    );
}

#[test]
fn a_failing_formula_keeps_the_last_good_value() {
    let scratch = Scratch::new("last-good");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replace("volume: {v: 4.0", "volume: {v: 0.0"),
    );
    let failed = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let text = scratch.read("project/samples/c1.md");
    let plato = &text[text.find("plato:").expect("plato is written")..];
    let plato = &plato[..plato.find("haze:").unwrap_or(plato.len())];
    assert!(
        plato.contains("3.0") && plato.contains("computed:") && plato.contains("failed:"),
        "{text}"
    );
    let table = run(
        &scratch,
        &["samples/c1.md", "-c", "plato", "--width", "120"],
    );
    assert!(out(&table).contains("3 ✗"), "{}", out(&table));
    let csv = run(&scratch, &["samples/c1.md", "-c", "plato", "--csv"]);
    assert!(!out(&csv).contains('3'), "{}", out(&csv));
}

#[test]
#[cfg(target_os = "linux")]
fn a_try_written_at_the_prompt_is_recorded_and_kept() {
    // The values written after `--try` are a write like `--write`'s: the model
    // that computed them is recorded, and the history keeps one snapshot.
    let scratch = Scratch::new("try-prompt");
    let root = project(&scratch, KEG);
    let output = at_terminal(&scratch, &["compute", "samples", "--try"], "y\n");
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert!(scratch.read("project/samples/c1.md").contains("plato"));
    let computed = runtime::ComputedStore::at(&scratch.at("state"));
    assert_eq!(
        computed
            .digest_for(&scratch.at("project/samples/c1.md"))
            .as_deref(),
        Some(permit_digest(&scratch).as_str())
    );
    // And the formula of each value it wrote.
    assert!(
        computed
            .recorded_formulas(&scratch.at("project/samples/c1.md"))
            .contains_key("plato")
    );
    let log = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    let messages = String::from_utf8_lossy(&log.stdout).into_owned();
    assert_eq!(
        messages.lines().next(),
        Some("samplekit compute samples --try (written at the prompt)"),
        "{messages}"
    );
}

/// The model's digest as it is now.
fn permit_digest(scratch: &Scratch) -> String {
    runtime::digest_of(&runtime::template_of(&loaded(scratch)).unwrap())
        .unwrap()
        .to_string()
}

#[test]
fn a_dry_run_names_the_flags_that_apply_it() {
    // A preview says what applies it, on the line that reports it.
    let scratch = Scratch::new("dry-run-next-step");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains(
            "4 values in 2 samples would be computed — --try computes without writing, --write writes"
        ),
        "{}",
        out(&output)
    );
}

#[test]
fn the_columns_of_a_table_computed_together_share_one_line() {
    let scratch = Scratch::new("table-one-line");
    project(&scratch, SHAPES);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    let tables: Vec<&str> = report
        .lines()
        .filter(|line| line.contains('✓') && line.contains("mashing"))
        .collect();
    assert_eq!(tables.len(), 2, "{report}");
    assert!(
        tables.iter().all(|line| line.contains("mashing: G, N")),
        "{report}"
    );
}

#[test]
fn a_rerun_marks_what_it_left_unchanged() {
    let scratch = Scratch::new("rerun-unchanged");
    project(&scratch, KEG);
    let first = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    // The scratch directory's name holds the word: a row of the table is read.
    assert!(
        !out(&first)
            .lines()
            .any(|line| line.trim_end().ends_with("unchanged")),
        "{}",
        out(&first)
    );
    let again = run(&scratch, &["compute", "samples", "--rerun", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let report = out(&again);
    assert!(
        report
            .lines()
            .any(|line| line.starts_with("plato") && line.trim_end().ends_with("unchanged")),
        "{report}"
    );
    assert!(
        report.contains("computed 4 values in 2 samples, 4 unchanged"),
        "{report}"
    );
}

#[test]
fn a_narrow_terminal_hides_before_and_keeps_after() {
    let scratch = Scratch::new("narrow-changes");
    project(&scratch, KEG);
    let output = run(
        &scratch,
        &["compute", "samples", "--write", "--width", "20"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    assert!(
        report.contains("before hidden — --width shows it"),
        "{report}"
    );
    assert!(!report.contains("column hidden"), "{report}");
    assert!(
        report
            .lines()
            .any(|line| line.starts_with("value") && line.contains("after")),
        "{report}"
    );
}

/// The binary in a pseudoterminal `columns` wide, its raw output.
fn in_terminal_of(scratch: &Scratch, columns: &str, arguments: &[&str]) -> String {
    let line = std::iter::once(env!("CARGO_BIN_EXE_samplekit"))
        .chain(arguments.iter().copied())
        .map(|word| format!("'{}'", word.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = command(scratch, &scratch.at("project"));
    let child = command.env("COLUMNS", columns).spawn_script(&line);
    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[cfg(target_os = "linux")]
fn a_model_print_is_written_above_the_progress_bar() {
    let scratch = Scratch::new("print-above-bar");
    project(&scratch, KEG);
    let raw = in_terminal_of(
        &scratch,
        "80",
        &["compute", "samples", "--write", "--show-output"],
    );
    let printed: Vec<usize> = raw
        .match_indices("computing the plato of")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(printed.len(), 2, "{raw:?}");
    for at in printed {
        let before = &raw[..at];
        assert!(
            before.ends_with("\x1b[2K") || before.ends_with('\n'),
            "{:?}",
            &raw[at.saturating_sub(60)..at + 30]
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn the_progress_line_fits_the_terminal() {
    let scratch = Scratch::new("bar-fits");
    project(&scratch, KEG);
    let raw = in_terminal_of(&scratch, "40", &["compute", "samples", "--write"]);
    let bars: Vec<&str> = raw
        .split("\r\x1b[2K")
        .filter(|segment| segment.starts_with('['))
        .map(|segment| {
            segment
                .split(['\r', '\n', '\x1b'])
                .next()
                .unwrap_or_default()
        })
        .collect();
    assert!(!bars.is_empty(), "{raw:?}");
    for bar in bars {
        assert!(bar.chars().count() < 40, "{bar:?}");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn the_progress_clock_runs_while_a_formula_does() {
    // A formula computing in silence still sees the bar's clock move.
    let scratch = Scratch::new("bar-clock");
    project(
        &scratch,
        "import time\n\nimport samplekit as sk\n\n\nclass Quiet(sk.Sample):\n    \
         def __init__(self, path=None, name=None):\n        super().__init__(path, name=name)\n        \
         self.malt = sk.Property(unit=\"g\")\n        \
         self.plato = sk.Property(compute=self._plato)\n        \
         self.set_dependencies(\"plato\", depends_on=[\"malt\"])\n\n    \
         def _plato(self):\n        time.sleep(2.2)\n        return self.malt.value / 4\n",
    );
    let raw = in_terminal_of(&scratch, "120", &["compute", "samples/c1.md", "--write"]);
    let bars: Vec<&str> = raw
        .split("\r\x1b[2K")
        .filter(|segment| segment.starts_with('[') && segment.contains("plato"))
        .collect();
    assert!(bars.len() >= 3, "{raw:?}");
    assert!(
        bars.iter()
            .any(|bar| bar.contains("plato 1.") || bar.contains("plato 2.")),
        "{raw:?}"
    );
}

/// The run log a computation named in its closing line, read.
fn run_log_named(report: &str) -> String {
    let line = report
        .lines()
        .find(|line| line.starts_with("the model printed "))
        .unwrap_or_else(|| panic!("no run log named:\n{report}"));
    let path = line.split(" — ").nth(1).expect("a path after the dash");
    fs::read_to_string(path).unwrap()
}

#[test]
fn a_model_print_goes_to_the_run_log() {
    let scratch = Scratch::new("run-log");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        !err(&output).contains("computing the plato"),
        "{}",
        err(&output)
    );
    let report = out(&output);
    assert!(report.contains("the model printed 2 lines — "), "{report}");
    let log = run_log_named(&report);
    assert!(
        log.contains("[C1] plato: computing the plato of C1"),
        "{log}"
    );
    assert!(scratch.at("state/logs").is_dir());
}

/// A model that logs an INFO record and a WARNING while computing.
const LOGGED: &str = r#"import logging

import samplekit as sk

LOG = logging.getLogger("keg")


class Keg(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.plato = sk.Property(compute=self._plato)
        self.set_dependencies("plato", depends_on=["malt", "volume"])

    def _plato(self):
        LOG.info("measured %s", self.name)
        LOG.warning("thin keg %s", self.name)
        return self.malt.value / self.volume.value
"#;

#[test]
fn logged_warnings_are_one_line_and_kept() {
    let scratch = Scratch::new("logged-warning");
    project(&scratch, LOGGED);
    let output = run(&scratch, &["compute", "samples", "--write", "-v"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = err(&output);
    assert!(shown.contains("C1 plato: thin keg C1"), "{shown}");
    assert_eq!(
        shown.matches("the model logged 2 warnings").count(),
        1,
        "{shown}"
    );
    assert!(!shown.contains("measured C1"), "{shown}");
    let log = run_log_named(&out(&output));
    assert!(log.contains("[C1] plato: WARNING thin keg C1"), "{log}");
    assert!(log.contains("[C1] plato: INFO measured C1"), "{log}");
}

#[test]
fn a_failure_shows_what_its_value_printed_last() {
    let scratch = Scratch::new("failure-tail");
    project(&scratch, KEG);
    keg(&scratch, "c0", 13.0, 0.0);
    let output = run(&scratch, &["compute", "samples/c0.md", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let text = err(&output);
    assert!(text.contains("the last lines plato printed:"), "{text}");
    assert!(text.contains("    computing the plato of C0"), "{text}");
}

#[test]
fn a_run_log_keeps_the_last_twenty() {
    let scratch = Scratch::new("run-log-kept");
    project(&scratch, KEG);
    for second in 0..25 {
        scratch.write(
            &format!("state/logs/compute-2000-01-01T00-00-{second:02}Z-1.log"),
            "old\n",
        );
    }
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let mut names: Vec<String> = fs::read_dir(scratch.at("state/logs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 20, "{names:?}");
    assert!(
        !names.contains(&"compute-2000-01-01T00-00-05Z-1.log".to_string()),
        "{names:?}"
    );
    let newest = names.last().unwrap();
    assert!(!newest.starts_with("compute-2000"), "{names:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_table_line_is_said_before_the_next_value_runs() {
    let scratch = Scratch::new("table-line-early");
    project(
        &scratch,
        r#"import samplekit as sk


class Late(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column(), "N": sk.Column()},
            "T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
            compute_rows=[("G", ["row.R"], lambda row: 1 / row.R.value)],
            compute_columns=[
                ("N", ["mashing.R"], lambda columns: [value / 4.0 for value in columns["R"].values])
            ],
        )
        self.late = sk.Property(compute=self._late)
        self.set_dependencies("late", depends_on=["mashing.G", "mashing.N"])

    def _late(self):
        print("late starts", flush=True)
        return 1.0
"#,
    );
    let raw = in_terminal_of(
        &scratch,
        "120",
        &["compute", "samples/c1.md", "--write", "--show-output"],
    );
    let table = raw.find("mashing: ").unwrap_or_else(|| panic!("{raw:?}"));
    let late = raw.find("late starts").unwrap_or_else(|| panic!("{raw:?}"));
    assert!(table < late, "{raw:?}");
}

#[test]
fn computing_a_table_column_runs_only_its_derivation() {
    // Computing G runs its row formula, never the column formula of N.
    let scratch = Scratch::new("column-alone");
    project(
        &scratch,
        r#"import samplekit as sk


class Mashing(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column(), "N": sk.Column()},
            "T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
            compute_rows=[("G", ["row.R"], self._g)],
            compute_columns=[("N", ["mashing.R"], self._n)],
        )

    def _g(self, row):
        print("G runs")
        return 1 / row.R.value

    def _n(self, columns):
        print("N runs")
        return [value / 4.0 for value in columns["R"].values]
"#,
    );
    let output = run(
        &scratch,
        &[
            "compute",
            "samples/c1.md",
            "-p",
            "mashing.G",
            "--write",
            "--show-output",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = err(&output);
    assert!(shown.contains("G runs"), "{shown}");
    assert!(!shown.contains("N runs"), "{shown}");
}

/// The log is named **once, at the end** — never in the middle of the values.
///
/// It used to be said as soon as the model wrote its first line, because exit
/// paths existed that named nothing. There are none left, and on a real run
/// reporting forty-one values the line landed in the middle of them.
#[test]
fn the_run_log_is_named_once_at_the_end() {
    let scratch = Scratch::new("run-log-named-once");
    project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let whole = format!("{}{}", out(&output), err(&output));
    assert_eq!(whole.matches("the model printed ").count(), 1, "{whole}");
    assert!(
        whole.contains("--show-output shows them as they come"),
        "{whole}"
    );
    // Nothing about the log stands between the values.
    let lines: Vec<&str> = whole.lines().collect();
    let last_value = lines.iter().rposition(|line| line.contains("  ✓ "));
    let named = lines
        .iter()
        .position(|line| line.contains("the model printed "));
    assert!(
        matches!((last_value, named), (Some(value), Some(at)) if at > value),
        "{whole}"
    );
}

/// A first value that prints, and a second that kills the command line.
const PRINTS_THEN_KILLS: &str = r#"import os
import signal
import time

import samplekit as sk


class Slow(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.first = sk.Property(compute=self._first)
        self.second = sk.Property(compute=self._second)
        self.set_dependencies("first", depends_on=["malt"])
        self.set_dependencies("second", depends_on=["first"])

    def _first(self):
        print("first done", flush=True)
        return self.malt.value * 2

    def _second(self):
        os.kill(os.getppid(), signal.SIGTERM)
        time.sleep(4)
        return self.first.value + 1
"#;

#[test]
fn an_interrupted_run_names_its_log() {
    let scratch = Scratch::new("interrupted-log");
    project(&scratch, PRINTS_THEN_KILLS);
    let output = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(output.status.code(), Some(130), "{}", err(&output));
    assert!(err(&output).contains("interrupted:"), "{}", err(&output));
    assert!(
        out(&output).contains("the model printed 1 line — "),
        "{}",
        out(&output)
    );
}

/// A table whose derived cells a person may edit: `y = 2 x`.
const DOUBLED: &str = r#"import samplekit as sk


class Doubled(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.t = sk.Table(
            {"T": sk.Column(), "x": sk.Column(), "y": sk.Column()},
            "T",
            rows=[{"T": 20, "x": 1.0}, {"T": 30, "x": 3.0}],
            compute_rows=[("y", ["row.x"], self._y)],
        )

    def _y(self, row):
        return 2 * row.x.value
"#;

/// The file with the cell `column` of the row `index_line` rewritten as `cell`.
fn with_cell(text: &str, index_line: &str, column: &str, cell: &str) -> String {
    let start = text.find(index_line).expect("the row");
    let key = format!("{column}: ");
    let at = start + text[start..].find(&key).expect("the cell");
    let end = at + text[at..].find('\n').expect("the end of the cell");
    format!("{}{column}: {cell}{}", &text[..at], &text[end..])
}

/// The value a cell's line holds, as written.
fn cell_line<'a>(text: &'a str, index_line: &str, column: &str) -> &'a str {
    let start = text.find(index_line).expect("the row");
    let key = format!("{column}: ");
    let at = start + text[start..].find(&key).expect("the cell");
    let end = at + text[at..].find('\n').expect("the end of the cell");
    &text[at..end]
}

#[test]
fn an_edited_cell_is_kept_without_force() {
    let scratch = Scratch::new("edited-cell-kept");
    project(&scratch, DOUBLED);
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    let line = cell_line(&text, "- T: 20", "y").to_string();
    let edited_line = line.replacen("v: 2.0", "v: 77.0", 1);
    assert_ne!(edited_line, line, "{text}");
    let edited = with_cell(&text, "- T: 20", "y", edited_line.trim_start_matches("y: "));
    scratch.write("project/samples/c1.md", &edited);
    let status = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&status).contains("edited since it was computed"),
        "{}",
        out(&status)
    );
    let kept = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let after = scratch.read("project/samples/c1.md");
    assert!(
        cell_line(&after, "- T: 20", "y").contains("77.0"),
        "{after}"
    );
    let forced = run(
        &scratch,
        &["compute", "samples/c1.md", "--force", "--write"],
    );
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    let given_back = scratch.read("project/samples/c1.md");
    assert!(
        cell_line(&given_back, "- T: 20", "y").contains("v: 2.0"),
        "{given_back}"
    );
}

#[test]
fn a_cell_without_its_record_is_given_back_by_force() {
    let scratch = Scratch::new("record-missing-cell");
    project(&scratch, DOUBLED);
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &with_cell(&text, "- T: 20", "y", "5.0"),
    );
    let status = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&status).contains("record missing — --force gives it back to its formula (1 row)"),
        "{}",
        out(&status)
    );
    let kept = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let after = scratch.read("project/samples/c1.md");
    assert!(cell_line(&after, "- T: 20", "y").contains("5.0"), "{after}");
    let listed = run(&scratch, &["compute", "samples/c1.md", "--force"]);
    assert!(
        out(&listed)
            .lines()
            .any(|line| line.contains("t.y") && line.contains("edited")),
        "{}",
        out(&listed)
    );
    let forced = run(
        &scratch,
        &["compute", "samples/c1.md", "--force", "--write"],
    );
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    let given_back = scratch.read("project/samples/c1.md");
    assert!(
        cell_line(&given_back, "- T: 20", "y").contains("v: 2.0"),
        "{given_back}"
    );
}

#[test]
fn a_model_that_raises_as_it_is_imported_keeps_its_traceback() {
    // Reported by its message alone — no file, no line, no exception type.
    let scratch = Scratch::new("import-traceback");
    project(
        &scratch,
        "import samplekit as sk\n\nundefined_name_at_import\n\nclass M(sk.Sample):\n    pass\n",
    );
    keg(&scratch, "c1", 13.0, 5.0);
    let output = run(&scratch, &["compute", "samples"]);
    assert_ne!(code(&output), 0, "{}", err(&output));
    let said = err(&output);
    assert!(said.contains("model.py"), "{said}");
    assert!(said.contains("NameError"), "{said}");
    assert!(said.contains("line 3"), "{said}");
}

#[test]
fn a_model_writes_bytes_and_undecodable_lines_as_a_script_would() {
    // `sys.stdout.buffer` did not exist under the command line, and one line
    // of Latin-1 on the model's stderr stopped the relay and broke the pipe.
    let scratch = Scratch::new("model-bytes");
    project(
        &scratch,
        r#"import os
import sys

import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.plato = sk.Property(compute=self._plato, depends_on=["malt", "volume"])

    def _plato(self):
        sys.stdout.buffer.write(b"through the buffer\n")
        os.write(2, "caf\xe9 in Latin-1\n".encode("latin-1"))
        os.write(2, b"and a line after it\n")
        return self.malt.value / self.volume.value
"#,
    );
    keg(&scratch, "c1", 13.0, 5.0);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("plato:"));
    assert!(!err(&output).contains("Broken pipe"), "{}", err(&output));
}

#[test]
fn two_samples_of_one_file_name_keep_two_failure_logs() {
    // Logs were named after the file alone: `explain a/S1.md` showed the
    // traceback `b/S1.md` had left.
    let scratch = Scratch::new("two-logs");
    project(&scratch, KEG);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[model]\npath = \"model.py\"\n",
    );
    for (directory, volume) in [("a", 0.0), ("b", 0.0)] {
        scratch.write(
            &format!("project/samples/{directory}/s1.md"),
            &format!(
                "---\nschema_version: 1\nname: {directory}-s1\nproperties:\n  malt: {{v: 13.0, unit: g}}\n  \
                 volume: {{v: {volume:?}, unit: L}}\n---\n"
            ),
        );
    }
    let failed = run(&scratch, &["compute", "samples", "--write"]);
    assert_ne!(code(&failed), 0, "{}", err(&failed));
    assert!(scratch.exists("project/.samplekit/failures/samples/a/s1.log"));
    assert!(scratch.exists("project/.samplekit/failures/samples/b/s1.log"));
    let explained = run(&scratch, &["explain", "samples/a/s1.md", "plato"]);
    assert!(
        out(&explained).contains("ZeroDivisionError"),
        "{}",
        out(&explained)
    );
}

#[test]
fn a_change_below_the_shown_precision_is_not_called_unchanged() {
    // The before-and-after is the audit trail: an uncertainty computed again
    // from a corrected reading moved from 0.01842 to 0.01824, both shown
    // 0.018, and the line said *unchanged*.
    let scratch = Scratch::new("below-precision");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.reading = sk.Property(unit="mg", compute_uncertainty=self._u,
                                   depends_on=["reading.v"])

    def _u(self):
        return abs(self.reading.value) * 0.001
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  reading: {v: 18.42, unit: mg}\n---\n",
    );
    let first = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch
        .read("project/samples/c1.md")
        .replace("v: 18.42", "v: 18.24");
    scratch.write("project/samples/c1.md", &text);
    let second = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let said = out(&second);
    assert!(!said.contains("unchanged"), "{said}");
    assert!(said.contains("0.01824"), "{said}");
}

#[test]
fn a_formula_over_absent_inputs_waits_and_writes_nothing() {
    // A sample whose volume nobody entered — plato is not run, and is no
    // failure; haze waits behind it; the run says what to fill in.
    let scratch = Scratch::new("waits");
    project(&scratch, KEG);
    scratch.write(
        "project/samples/c9.md",
        "---\nschema_version: 1\nname: C9\nproperties:\n  malt: {v: 12.0, unit: g}\n---\n",
    );
    let output = run(&scratch, &["compute", "samples/c9.md", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    assert!(
        report.contains("plato") && report.contains("waits for volume"),
        "{report}"
    );
    assert!(
        report.contains("haze") && report.contains("waits for plato"),
        "{report}"
    );
    assert!(report.contains("fill in volume"), "{report}");
    assert!(!err(&output).contains("Traceback"), "{}", err(&output));
    let written = scratch.read("project/samples/c9.md");
    assert!(!written.contains("failed"), "{written}");
    // The formula never ran: its marker was never written.
    assert!(!scratch.exists("project/ran"));
}

#[test]
fn a_computation_is_one_snapshot_whatever_the_worker_saves() {
    // The worker saves each sample it computes, with the history off; the
    // command is the one snapshot.
    let scratch = Scratch::new("history-compute");
    let root = project(&scratch, KEG);
    let output = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let log = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [
            "samplekit compute samples --write",
            "the project as SampleKit first kept it"
        ]
    );
}

#[test]
fn the_gate_can_accept_overrides_and_values_waiting() {
    // An override and a value waiting pass with --accept; a stale one never
    // does.
    let scratch = Scratch::new("gate-accept");
    project(&scratch, KEG);
    let written = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = scratch.read("project/samples/c1.md");
    // An override of a value nothing reads: haze.
    let overridden = text.replacen("v: 0.25", "v: 0.3", 1);
    assert_ne!(overridden, text, "{text}");
    scratch.write("project/samples/c1.md", &overridden);
    let strict = run(&scratch, &["status", "samples", "--exit-code"]);
    assert_eq!(code(&strict), 2, "{}", out(&strict));
    let accepting = run(
        &scratch,
        &[
            "status",
            "samples",
            "--exit-code",
            "--accept",
            "edited,waiting",
        ],
    );
    assert_eq!(
        code(&accepting),
        0,
        "{}{}",
        out(&accepting),
        err(&accepting)
    );
    // A new malt makes the plato stale, which no --accept passes.
    let set = run(&scratch, &["set", "samples/c2.md", "malt=11", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let failing = run(
        &scratch,
        &[
            "status",
            "samples",
            "--exit-code",
            "--accept",
            "edited,waiting",
        ],
    );
    assert_eq!(code(&failing), 2, "{}", out(&failing));
    assert!(out(&failing).contains("outdated"), "{}", out(&failing));
}

#[test]
fn a_formula_reading_a_value_not_applicable_gives_it_too() {
    // Plato reads malt; malt not applicable, plato is, and haze after
    // it, without running — and nothing is left to compute.
    let scratch = Scratch::new("not-applicable-compute");
    project(&scratch, KEG);
    let set = run(&scratch, &["set", "samples/c1.md", "malt=n/a", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let table = out(&run(&scratch, &["samples", "-c", "name,plato,haze"]));
    let c1 = table.lines().find(|line| line.starts_with("C1")).unwrap();
    assert_eq!(c1.matches("n/a").count(), 2, "{table}");
    let status = out(&run(&scratch, &["status", "samples"]));
    assert!(!status.contains("C1"), "{status}");
}

/// Readings a model takes the mean of, and a value computed from them.
const GRAVITIES: &str = r#"import samplekit as sk


class Brew(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.abv = sk.Property(compute=lambda: (self.og.value - self.fg.value) * 131.25)
        self.set_dependencies("abv", depends_on=["og", "fg"])
"#;

#[test]
fn the_plan_counts_only_samples_with_something_to_compute() {
    // `5 values in 3 samples would be computed, 2 wait` computed 5 values in
    // 2 samples: the third only waited.
    let scratch = Scratch::new("plan-counts");
    project(&scratch, KEG);
    scratch.write(
        "project/samples/c9.md",
        "---\nschema_version: 1\nname: C9\nproperties:\n  malt: {v: 12.0, unit: g}\n---\n",
    );
    let plan = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&plan), 0, "{}", err(&plan));
    assert!(
        out(&plan).contains(
            "4 values in 2 samples would be computed, 2 wait for an input nobody entered"
        ),
        "{}",
        out(&plan)
    );
    // Nothing but what waits: said as nothing, not as *0 values in 1 sample*.
    let alone = run(&scratch, &["compute", "samples/c9.md"]);
    assert!(
        out(&alone).contains("nothing would be computed: 2 wait for an input nobody entered"),
        "{}",
        out(&alone)
    );
    assert!(!out(&alone).contains("0 values"), "{}", out(&alone));
}

#[test]
fn a_value_waiting_for_itself_is_said_one_way() {
    // The plan said `fg waits for fg`, the run *waits for its value, which
    // nobody entered*: the plan now says the run's words.
    let scratch = Scratch::new("waits-itself");
    project(&scratch, GRAVITIES);
    scratch.write(
        "project/samples/b1.md",
        "---\nschema_version: 1\nname: B1\nproperties:\n  og: {readings: [1.05, 1.052]}\n---\n",
    );
    for arguments in [
        vec!["compute", "samples/b1.md", "--rerun"],
        vec!["compute", "samples/b1.md", "--rerun", "--write"],
    ] {
        let output = run(&scratch, &arguments);
        let said = out(&output);
        let fg = said
            .lines()
            .find(|line| {
                let words: Vec<&str> = line.split_whitespace().collect();
                words.contains(&"B1") && words.contains(&"fg")
            })
            .unwrap_or_else(|| panic!("{arguments:?}: {said}"));
        assert!(
            fg.contains("waits for its value, which nobody entered"),
            "{arguments:?}: {said}"
        );
    }
}

#[test]
fn a_value_whose_input_was_emptied_waits_for_it_everywhere() {
    // `status` said *stale — volume*, `compute` *waits for volume* and
    // `explain` *volume — current … stale — volume*, of one value.
    let scratch = Scratch::new("emptied-input");
    project(&scratch, KEG);
    let computed = run(&scratch, &["compute", "samples/c1.md", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let cleared = run(&scratch, &["set", "samples/c1.md", "volume=", "--write"]);
    assert_eq!(code(&cleared), 0, "{}", err(&cleared));
    let status = run(&scratch, &["status", "samples/c1.md"]);
    assert!(
        out(&status).contains("waits for volume"),
        "{}",
        out(&status)
    );
    assert!(!out(&status).contains("outdated"), "{}", out(&status));
    let plan = run(&scratch, &["compute", "samples/c1.md"]);
    assert!(out(&plan).contains("waits for volume"), "{}", out(&plan));
    let explained = run(&scratch, &["explain", "samples/c1.md", "plato"]);
    let said = out(&explained);
    assert!(said.contains("nobody entered it"), "{said}");
    assert!(said.contains("this value waits for volume"), "{said}");
    assert!(!said.contains("outdated"), "{said}");
    // What reads the waiting value waits in turn.
    let haze = run(&scratch, &["explain", "samples/c1.md", "haze"]);
    assert!(
        out(&haze).contains("this value waits for plato"),
        "{}",
        out(&haze)
    );
}

#[test]
fn a_model_that_could_not_be_read_heads_the_status() {
    // *every value the files record in 2 samples is current* stood alone
    // above a model that had raised.
    let scratch = Scratch::new("status-model-unread");
    project(
        &scratch,
        "import samplekit as sk\n\n\nclass Broken(sk.Sample):\n    def __init__(self, path=None, name=None):\n        super().__init__(path, name=name)\n        raise RuntimeError(\"the model is broken\")\n",
    );
    let output = run(&scratch, &["status", "samples"]);
    let said = out(&output);
    let first = said.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("the model could not be read for C1, C2"),
        "{said}"
    );
}

#[test]
fn an_uncertainty_never_computed_is_said_so_by_status_and_compute() {
    // `status` said *uncertainty never computed* of an entered volume whose
    // uncertainty a formula owes, and `compute` *never computed*.
    let scratch = Scratch::new("uncertainty-owed-said-alike");
    project(
        &scratch,
        "import samplekit as sk\n\n\nclass Tank(sk.Sample):\n    def __init__(self, path=None, name=None):\n        super().__init__(path, name=name)\n        self.volume = sk.Property(unit=\"L\", compute_uncertainty=lambda: 0.3, depends_on=[])\n",
    );
    scratch.write(
        "project/samples/t1.md",
        "---\nschema_version: 1\nname: T1\nproperties:\n  volume: {v: 21, unit: L}\n---\n",
    );
    for command in ["status", "compute"] {
        let output = run(&scratch, &[command, "samples/t1.md"]);
        let said = out(&output);
        let line = said
            .lines()
            .find(|line| line.contains("T1") && line.contains("volume"))
            .unwrap_or_else(|| panic!("{command}: {said}"));
        assert!(
            line.contains("uncertainty never computed"),
            "{command}: {said}"
        );
    }
}

#[test]
fn an_interrupted_try_says_once_that_nothing_was_written() {
    // *nothing was written; the value in progress was not written* said the
    // second half twice.
    let scratch = Scratch::new("interrupted-try");
    project(&scratch, PRINTS_THEN_KILLS);
    let output = run(&scratch, &["compute", "samples/c1.md", "--try"]);
    assert_eq!(output.status.code(), Some(130), "{}", err(&output));
    assert!(
        err(&output).contains("interrupted: nothing was written\n"),
        "{}",
        err(&output)
    );
    assert!(
        !err(&output).contains("value in progress"),
        "{}",
        err(&output)
    );
}
#[test]
fn a_rehearsal_shows_a_value_at_its_precision_and_says_empty_cells() {
    // A value is shown at the precision the project declares, as a table shows
    // it, and every digit where it declares none: `3.33333` hid the rest of a
    // plato nothing rounds. A column's cells nobody wrote are said to be
    // empty, where `2 cells` read as two values replaced.
    let scratch = Scratch::new("rehearsal-digits");
    project(&scratch, SHAPES);
    // Its rows measured and written, their computed cells not yet.
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  malt: {v: 10.0, unit: g}\n  \
         volume: {v: 3.0, unit: L}\ntables:\n  mashing:\n    index: T\n    columns:\n      \
         T: {}\n      R: {}\n      G: {}\n      N: {}\n    rows:\n      - {T: 10, R: 2.0}\n      \
         - {T: 20, R: 4.0}\n---\n",
    );
    let output = run(&scratch, &["compute", "samples/c1.md", "--try"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("3.3333333333333335"), "{text}");
    assert!(text.contains("2 empty cells"), "{text}");
    assert!(!text.contains("2 cells"), "{text}");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n\
         [property.plato]\nprecision = \".2f\"\n",
    );
    let output = run(&scratch, &["compute", "samples/c1.md", "--try"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("3.33"), "{text}");
    assert!(!text.contains("3.3333"), "{text}");
}

// ------------------------------------------------------ the description

/// The shapes, with a statistic of readings, an attribute and a figure.
fn described_model() -> String {
    SHAPES.replace(
        "        self.set_dependencies(\"plato\"",
        "        self.dosed = sk.Property(value=sk.stats.mean, \
         uncertainty=sk.stats.standard_error, unit=\"g\")\n        \
         self.maltster = \"jo\"\n        self.set_dependencies(\"plato\"",
    ) + "\n    @sk.figure\n    def curve(self, ax):\n        ax.plot([1], [2])\n"
}

fn description_of(scratch: &Scratch) -> runtime::ModelDescription {
    serde_json::from_str(&scratch.read("project/.samplekit/model.json")).unwrap()
}

#[test]
fn the_model_is_described_as_it_is_imported() {
    let scratch = Scratch::new("described");
    project(&scratch, &described_model());
    let listed = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let described = description_of(&scratch);
    assert_eq!(described.format, runtime::FORMAT);
    assert_eq!(described.samplekit, runtime::VERSION);
    assert_eq!(described.model.path, "model.py");
    assert_eq!(described.model.class, "Shapes");
    let template = runtime::template_of(&loaded(&scratch)).unwrap();
    assert_eq!(
        described.model.digest,
        runtime::digest_of(&template).unwrap().as_str()
    );
    // A statistic's property.
    let dosed = &described.properties["dosed"];
    assert_eq!(dosed.unit.as_deref(), Some("g"));
    assert_eq!(
        dosed.value,
        runtime::Origin::Statistic {
            statistic: "mean".to_string()
        }
    );
    assert_eq!(
        dosed.uncertainty,
        Some(runtime::Origin::Statistic {
            statistic: "standard_error".to_string()
        })
    );
    // A formula's, with what it reads and its digest.
    let plato = &described.properties["plato"];
    assert_eq!(
        plato.value,
        runtime::Origin::Formula {
            reads: vec!["malt".to_string(), "volume".to_string()]
        }
    );
    assert!(
        plato
            .formula
            .as_ref()
            .is_some_and(|digest| digest.len() == 64)
    );
    assert!(matches!(
        described.properties["pair"].value,
        runtime::Origin::Quantity { .. }
    ));
    // An entered value with a default, and a formula for its uncertainty.
    let age = &described.properties["age"];
    assert_eq!(age.value, runtime::Origin::Entered);
    assert_eq!(age.default, Some(serde_json::json!(2.0)));
    assert!(matches!(
        age.uncertainty,
        Some(runtime::Origin::Formula { .. })
    ));
    // A table: its index, its columns in order, a derivation of each span.
    let mashing = &described.tables["mashing"];
    assert_eq!(mashing.index, ["T"]);
    // The rows it gives itself, each cell as a file would hold it; a cell no
    // formula has filled holds nothing.
    assert_eq!(
        serde_json::to_value(&mashing.rows).unwrap(),
        serde_json::json!([
            {"T": {"value": 10}, "R": {"value": 2.0}},
            {"T": {"value": 20}, "R": {"value": 4.0}},
        ])
    );
    let columns: Vec<&str> = mashing
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(columns, ["T", "R", "G", "N"]);
    assert_eq!(
        mashing.columns[2].value,
        runtime::Origin::Rows {
            reads: vec!["row.R".to_string()],
            fills: vec!["G".to_string()]
        }
    );
    assert!(matches!(
        mashing.columns[3].value,
        runtime::Origin::Columns { .. }
    ));
    // An attribute, and a figure.
    assert_eq!(described.attributes["maltster"], serde_json::json!("jo"));
    assert_eq!(described.figures.len(), 1);
    assert_eq!(described.figures[0].name, "curve");
    assert_eq!(described.figures[0].draws, runtime::Draws::Sample);
    assert_eq!(
        runtime::current_description(&loaded(&scratch)).as_ref(),
        Some(&described)
    );
    // Imported again, the model is described alike, and the file is left as
    // it was.
    let path = scratch.at("project/.samplekit/model.json");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let again = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn a_description_written_with_a_symbol_still_reads() {
    // A model declares no symbol, and a description written before carries one
    // beside each unit; it reads, the field ignored.
    let without = serde_json::json!({
        "format": runtime::FORMAT,
        "samplekit": runtime::VERSION,
        "model": {"path": "model.py", "class": "Shapes", "digest": "9f2c"},
        "properties": {
            "malt": {"unit": "g", "value": {"from": "entered"}, "uncertainty": null,
                     "reads": [], "formula": null, "default": null}
        },
        "tables": {
            "mashing": {"title": null, "index": ["T"], "rows": [], "columns": [
                {"name": "T", "unit": "degC", "value": {"from": "entered"},
                 "uncertainty": null, "formula": null}
            ]}
        },
        "attributes": {},
        "figures": []
    });
    let mut with = without.clone();
    with["properties"]["malt"]["symbol"] = serde_json::json!("m");
    with["tables"]["mashing"]["columns"][0]["symbol"] = serde_json::json!("T");
    let read = |text: &serde_json::Value| {
        serde_json::from_value::<runtime::ModelDescription>(text.clone()).unwrap()
    };
    assert_eq!(read(&with), read(&without));
    // Written again, it carries none.
    assert!(!read(&with).to_text().contains("symbol"));
}

#[test]
fn an_edited_model_is_described_again() {
    let scratch = Scratch::new("described-again");
    project(&scratch, KEG);
    let listed = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let first = description_of(&scratch);
    assert!(!first.properties.contains_key("foam"));
    scratch.write(
        "project/model.py",
        &KEG.replace(
            "        self.set_dependencies(\"plato\"",
            "        self.foam = sk.Property(unit=\"mL\")\n        \
             self.set_dependencies(\"plato\"",
        ),
    );
    // Stale: never read.
    assert!(runtime::current_description(&loaded(&scratch)).is_none());
    let imports = || scratch.read("project/imported").lines().count();
    let before = imports();
    let stale = status(&scratch);
    assert_eq!(code(&stale), 0, "{}", err(&stale));
    // Once: `status` had it written, by one import of the model.
    assert_eq!(imports(), before + 1);
    let second = description_of(&scratch);
    assert_eq!(second.properties["foam"].unit.as_deref(), Some("mL"));
    assert_ne!(second.model.digest, first.model.digest);
    assert!(runtime::current_description(&loaded(&scratch)).is_some());
    let current = status(&scratch);
    assert_eq!(code(&current), 0, "{}", err(&current));
    assert_eq!(imports(), before + 1);
}

#[test]
fn status_with_a_current_description_starts_no_python() {
    let scratch = Scratch::new("described-status");
    project(&scratch, KEG);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  malt: {v: 9.0, unit: g}\n---\n",
    );
    let listed = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(scratch.exists("project/.samplekit/model.json"));
    // The interpreter now records being started, and runs nothing.
    fake_python(&scratch, "touch \"$(dirname \"$0\")/started\"\nexit 1\n");
    let said = status(&scratch);
    assert_eq!(code(&said), 0, "{}", err(&said));
    assert_eq!(said_of(&said, "C1", "plato"), "never computed");
    assert_eq!(said_of(&said, "C3", "plato"), "waits for volume");
    for arguments in [
        vec!["status", "samples", "--exit-code"],
        vec!["list", "figures", "samples"],
        vec!["samples", "-f", "state == never_computed"],
    ] {
        run(&scratch, &arguments);
    }
    assert!(!scratch.exists("project/started"));
    // The probe works: stale, the description is written by the interpreter.
    scratch.write("project/model.py", &format!("{KEG}\n# edited\n"));
    run(&scratch, &["status", "samples"]);
    assert!(scratch.exists("project/started"));
}

/// The state `status` says of `value` in the sample labelled `label`.
fn said_of(output: &Output, label: &str, value: &str) -> String {
    out(output)
        .lines()
        .find_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            (words.first() == Some(&label) && words.get(1) == Some(&value))
                .then(|| words[2..].join(" "))
        })
        .unwrap_or_else(|| panic!("no line for {label} {value}:\n{}", out(output)))
}

#[test]
fn a_model_that_cannot_be_read_leaves_no_description() {
    let scratch = Scratch::new("undescribed");
    project(&scratch, KEG);
    let listed = run(&scratch, &["compute", "samples"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(scratch.exists("project/.samplekit/model.json"));
    scratch.write(
        "project/model.py",
        &format!("raise RuntimeError(\"broken\")\n{KEG}"),
    );
    let stale = status(&scratch);
    assert_eq!(code(&stale), 0, "{}", err(&stale));
    assert!(
        out(&stale).contains("could not be read") || err(&stale).contains("could not be read"),
        "{}{}",
        out(&stale),
        err(&stale)
    );
    // Nothing is read from the stale description.
    assert_eq!(
        said(&stale, "plato", "never computed"),
        0,
        "{}",
        out(&stale)
    );
    assert!(!scratch.exists("project/.samplekit/model.json"));
}

#[test]
fn the_description_is_not_kept_in_the_history() {
    let scratch = Scratch::new("described-history");
    project(&scratch, KEG);
    let written = run(&scratch, &["compute", "samples", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(scratch.exists("project/.samplekit/model.json"));
    let kept = samplekit::config::version_control::kept_files(&loaded(&scratch)).unwrap();
    let names: Vec<&str> = kept.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"model.py"), "{names:?}");
    assert!(names.contains(&"samples/c1.md"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.starts_with(".samplekit/")),
        "{names:?}"
    );
}

/// A model of each state a plan tells apart.
const EVERY_STATE: &str = r#"import samplekit as sk


class Mixed(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.dosed = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error, unit="g")
        self.age = sk.Property(unit="mL", compute_uncertainty=lambda: 0.01, depends_on=[])
        self.plato = sk.Property(compute=self._plato)
        self.haze = sk.Property(compute=lambda: 1 - self.plato.value / 4.0)
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
            "T",
            compute_rows=[("G", ["row.R", "plato"], lambda row: row.R.value * 2)],
        )
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.set_dependencies("haze", depends_on=["plato"])

    def _plato(self):
        return self.malt.value / self.volume.value
"#;

#[test]
fn the_description_plans_as_the_worker_does() {
    let scratch = Scratch::new("described-plan");
    project(&scratch, EVERY_STATE);
    let full = "---\nschema_version: 1\nname: {N}\nproperties:\n  malt: {v: 12.0, unit: g}\n  \
                volume: {v: 4.0, unit: L}\n  age: {v: 2.0, unit: mL}\n  \
                dosed: {readings: [1.0, 2.0, 3.0], unit: g}\ntables:\n  mashing:\n    \
                index: T\n    columns:\n      T: {}\n      R: {}\n    rows:\n      \
                - {T: 10, R: 2.0}\n---\n";
    // Never computed, and computed below.
    scratch.write("project/samples/c1.md", &full.replace("{N}", "C1"));
    scratch.write("project/samples/c2.md", &full.replace("{N}", "C2"));
    // Waiting: no volume, no age.
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  malt: {v: 9.0, unit: g}\n---\n",
    );
    // A computed value written by hand, with no record.
    scratch.write(
        "project/samples/c4.md",
        &full
            .replace("{N}", "C4")
            .replace("  age:", "  plato: {v: 3.0}\n  age:"),
    );
    let written = run(&scratch, &["compute", "samples/c2.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    // Outdated: an input edited since.
    let c2 = scratch.read("project/samples/c2.md");
    scratch.write(
        "project/samples/c2.md",
        &c2.replace("malt: {v: 12.0", "malt: {v: 16.0"),
    );
    let store = runtime::ComputedStore::at(&scratch.at("state"));
    let compare = |scratch: &Scratch| {
        let config = loaded(scratch);
        let description = runtime::describe(&config, &scratch.at("project")).unwrap();
        let template = runtime::template_of(&config).unwrap();
        let python = runtime::interpreter_for(&config, &scratch.at("project")).unwrap();
        let mut worker = Worker::start(&python).unwrap();
        for name in ["c1", "c2", "c3", "c4"] {
            let path = scratch.at(&format!("project/samples/{name}.md"));
            let recorded = store.recorded_formulas(&path);
            let request = Request {
                sample: path.clone(),
                template: template.clone(),
                names: Vec::new(),
                rerun: false,
                force: true,
                refused: Vec::new(),
                recorded: recorded.clone(),
            };
            let by_worker = worker.plan(&request).unwrap();
            let by_description = description.plan(&path, &recorded).unwrap();
            assert_eq!(by_description, by_worker, "{name}");
        }
    };
    compare(&scratch);
    // A formula changed: what it gives, and what reads it.
    scratch.write(
        "project/model.py",
        &EVERY_STATE.replace(
            "self.malt.value / self.volume.value",
            "self.malt.value / self.volume.value * 1.0",
        ),
    );
    compare(&scratch);
}

#[test]
fn the_description_plans_the_rows_a_model_gives() {
    // The rows a model declares itself (`rows=`) are in its description, so
    // that `status` owes their cells as `compute` does — where the file holds
    // none of them, some, or all computed — without starting Python.
    let scratch = Scratch::new("described-rows");
    project(&scratch, SHAPES);
    let head = "---\nschema_version: 1\nname: {N}\nproperties:\n  malt: {v: 10.0, unit: g}\n  \
                volume: {v: 2.0, unit: L}\n";
    // No table in the file: the model's two rows, their cells never computed.
    scratch.write(
        "project/samples/c1.md",
        &format!("{head}---\n").replace("{N}", "C1"),
    );
    // One of the model's rows, its reading changed, and a row of its own.
    let partial = format!(
        "{head}tables:\n  mashing:\n    index: T\n    columns:\n      T: {{}}\n      \
         R: {{}}\n      G: {{}}\n      N: {{}}\n    rows:\n      - {{T: 10, R: 5.0}}\n      \
         - {{T: 30, R: 6.0}}\n---\n"
    );
    scratch.write("project/samples/c2.md", &partial.replace("{N}", "C2"));
    // Computed whole, then a reading the rows' formula reads edited.
    scratch.write("project/samples/c3.md", &partial.replace("{N}", "C3"));
    let written = run(&scratch, &["compute", "samples/c3.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let c3 = scratch.read("project/samples/c3.md");
    assert!(c3.contains("T: 20"), "{c3}");
    scratch.write("project/samples/c3.md", &c3.replacen("R: 4.0", "R: 8.0", 1));
    let store = runtime::ComputedStore::at(&scratch.at("state"));
    let config = loaded(&scratch);
    let description = runtime::describe(&config, &scratch.at("project")).unwrap();
    assert_eq!(description.tables["mashing"].rows.len(), 2);
    let template = runtime::template_of(&config).unwrap();
    let python = runtime::interpreter_for(&config, &scratch.at("project")).unwrap();
    let mut worker = Worker::start(&python).unwrap();
    for name in ["c1", "c2", "c3"] {
        let path = scratch.at(&format!("project/samples/{name}.md"));
        let recorded = store.recorded_formulas(&path);
        let request = Request {
            sample: path.clone(),
            template: template.clone(),
            names: Vec::new(),
            rerun: false,
            force: true,
            refused: Vec::new(),
            recorded: recorded.clone(),
        };
        let by_worker = worker.plan(&request).unwrap();
        let by_description = description.plan(&path, &recorded).unwrap();
        assert_eq!(by_description, by_worker, "{name}");
        // The model's rows are owed where the file holds none of them.
        if name == "c1" {
            assert!(
                by_description
                    .iter()
                    .any(|planned| planned.value.starts_with("mashing.")),
                "{by_description:?}"
            );
        }
    }
}
