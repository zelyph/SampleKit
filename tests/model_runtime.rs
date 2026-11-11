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
const BILLET: &str = r#"import os

import samplekit as sk

HERE = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(HERE, "imported"), "a") as marker:
    marker.write("imported\n")


class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=self._ductility)
        self.roughness = sk.Property(compute_value=lambda: 1 - self.ductility.value / 4.0)
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.set_dependencies("roughness", depends_on=["ductility"])

    def _ductility(self):
        with open(os.path.join(HERE, "ran"), "a") as marker:
            marker.write("ductility\n")
        print("computing the ductility of", self.name)
        return self.diameter.value / self.volume.value
"#;

/// A second value that kills the command line running it, then takes its time.
const SLOW: &str = r#"import os
import signal
import time

import samplekit as sk


class Slow(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.first = sk.Property(compute_value=lambda: self.diameter.value * 2)
        self.second = sk.Property(compute_value=self._second)
        self.set_dependencies("first", depends_on=["diameter"])
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
        self.diameter = sk.Property(unit="g")
        self.vfi = sk.Property(compute_value=lambda: self.porosity.value + 1)
        self.porosity = sk.Property(compute_value=lambda: self.swarf.value / 4)
        self.swarf = sk.Property(compute_value=lambda: self.diameter.value * 2)
        self.set_dependencies("vfi", depends_on=["porosity"])
        self.set_dependencies("porosity", depends_on=["swarf"])
        self.set_dependencies("swarf", depends_on=["diameter"])
"#;

/// The five shapes a model can declare.
const SHAPES: &str = r#"import samplekit as sk


class Shapes(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=lambda: self.diameter.value / self.volume.value)
        # An instrument's resolution reads nothing of the sample, which is what
        # depends_on=[] says. Declaring is not optional: a formula that reads
        # what it never declared is never stale again.
        self.age = sk.Property(2.0, compute_uncertainty=lambda: 0.01, depends_on=[])
        self.pair = sk.Property(compute=lambda: (self.diameter.value * 2, 0.5))
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.set_dependencies("pair", depends_on=["diameter"])
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=lambda row: 1 / row.R.value, depends_on=["row.R"]),
                "N": sk.Property(compute_column=lambda columns: [value / 4.0 for value in columns["R"].values], depends_on=["preheating.R"]),
            },
            index="T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        )
"#;

/// A project: a `.samplekitrc` naming `model.py`, the repository's environment
/// as its `.venv`, and two billets.
fn project(scratch: &Scratch, model: &str) -> PathBuf {
    bare_project(
        scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
        model,
    );
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.at("project")
}

/// The same with no environment.
fn bare_project(scratch: &Scratch, rc: &str, model: &str) {
    scratch.write("project/.samplekitrc", rc);
    scratch.write("project/model.py", model);
    billet(scratch, "c1", 12.0, 4.0);
    billet(scratch, "c2", 10.0, 4.0);
}

fn billet(scratch: &Scratch, name: &str, diameter: f64, volume: f64) -> PathBuf {
    scratch.write(
        &format!("project/samples/{name}.md"),
        &format!(
            "---\nschema_version: 1\nname: {}\nproperties:\n  diameter: {{v: {diameter:?}, unit: g}}\n  \
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

/// The binary given `answers` on its standard input: `compute` asks whether
/// to compute, then whether to write.
fn answering(scratch: &Scratch, arguments: &[&str], answers: &str) -> Output {
    let mut child = command(scratch, &scratch.at("project"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// The binary in a pseudoterminal, answering its questions: each key typed
/// once the question is there, as a person types it — a key typed before a
/// question is no answer to it, and is dropped.
fn at_terminal(scratch: &Scratch, arguments: &[&str], answers: &str) -> Output {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let line = std::iter::once(env!("CARGO_BIN_EXE_samplekit"))
        .chain(arguments.iter().copied())
        .map(|word| format!("'{}'", word.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = command(scratch, &scratch.at("project"));
    let mut child = child.args([] as [&str; 0]).spawn_script(&line);
    let collect = |mut stream: Box<dyn Read + Send>| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let held = Arc::clone(&seen);
        let reader = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(read) = stream.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                held.lock().unwrap().extend_from_slice(&buffer[..read]);
            }
        });
        (seen, reader)
    };
    let (stdout, out_reader) = collect(Box::new(child.stdout.take().unwrap()));
    let (stderr, err_reader) = collect(Box::new(child.stderr.take().unwrap()));
    let mut stdin = child.stdin.take().unwrap();
    for key in answers.chars().filter(|key| *key != '\n') {
        // The question asked: output that has stopped growing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut last = usize::MAX;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let now = stdout.lock().unwrap().len();
            if (now == last && now > 0) || std::time::Instant::now() > deadline {
                break;
            }
            last = now;
        }
        if stdin.write_all(key.to_string().as_bytes()).is_err() {
            break;
        }
    }
    drop(stdin);
    let status = child.wait().unwrap();
    out_reader.join().unwrap();
    err_reader.join().unwrap();
    let stdout = stdout.lock().unwrap().clone();
    let stderr = stderr.lock().unwrap().clone();
    Output {
        status,
        stdout,
        stderr,
    }
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
        timeout: None,
        refused: Vec::new(),
        overran: Vec::new(),
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
         self.hb = sk.Property(unit=\"mL\", compute_uncertainty=self._durometer, depends_on=[])\n        \
         self.headspace = sk.Property(unit=\"hl\", compute_value=self._headspace, depends_on=[\"hb\"])\n\n    \
         def _durometer(self):\n        return 0.006\n\n    \
         def _headspace(self):\n        return math.pi * (self.hb.value / 2) ** 2 / 100\n",
    );
    // The shared fixture's billets declare quantities this model does not.
    for gone in ["c1", "c2"] {
        std::fs::remove_file(scratch.at(&format!("project/samples/{gone}.md"))).unwrap();
    }
    scratch.write(
        "project/samples/s.md",
        "---\nschema_version: 1\nname: S\nproperties:\n  hb: {v: 12.0, unit: mL}\n---\n",
    );
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    // The record names the channel its formula produced.
    let written = scratch.read("project/samples/s.md");
    assert!(written.contains("computed: {u: {}}"), "{written}");

    let set = run(&scratch, &["samples/s.md", "set", "hb=9.99", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    // Not an override: nothing computed that value.
    assert!(!out(&set).contains("an override"), "{}", out(&set));

    let state = run(&scratch, &["samples", "status"]);
    let said = out(&state);
    // `hb` appears only as what stales `headspace`, never as a row of its own.
    assert!(!said.contains("edited"), "{said}");
    assert_eq!(said.matches("outdated — hb").count(), 1, "{said}");
    // What reads the value is stale, which is the point of the consumer's
    // digest staying wide while the property's own narrows.
    assert!(said.contains("headspace"), "{said}");

    // `explain` names the number a formula made, rather than claiming the
    // quantity was computed.
    let why = run(&scratch, &["samples/s.md", "explain", "hb"]);
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
    project(&scratch, BILLET);
    for arguments in [
        vec!["samples"],
        vec!["samples", "-c", "diameter"],
        // `status` reads the model to plan, nothing asked, wherever the model
        // adds something: no reading command left is kept to the files by a
        // flag.
        vec!["samples", "list"],
        // `validate` reads each sample with the model, as `compute` does
        //: no longer a reading of the files alone.
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
    billet(&scratch, "c1", 12.0, 4.0);
    let from = scratch.at("project");
    for config in [None, Some(&loaded(&scratch))] {
        assert_eq!(
            runtime::availability(config, &from).unwrap(),
            Availability::NoTemplate
        );
    }
    let output = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("no model is declared"),
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
    bare_project(&scratch, "", BILLET);
    fake_python(&scratch, "touch \"$(dirname \"$0\")/started\"\nexit 1\n");
    for arguments in [
        vec!["samples"],
        vec!["samples", "-c", "diameter"],
        vec!["samples", "list"],
    ] {
        run(&scratch, &arguments);
    }
    assert!(!scratch.exists("project/started"));
    // The probe itself works: a computation does start it.
    run(&scratch, &["samples", "compute", "--write"]);
    assert!(scratch.exists("project/started"));
}

// --------------------------------------------------------------- the model

#[test]
fn loads_a_template_by_relative_path() {
    let scratch = Scratch::new("relative");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"models/billet.py\"\npython = \".venv\"\n",
        "",
    );
    fs::remove_file(scratch.at("project/model.py")).unwrap();
    scratch.write("project/models/billet.py", BILLET);
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    let template = runtime::template_of(&loaded(&scratch)).unwrap();
    assert_eq!(template.path(), scratch.at("project/models/billet.py"));

    // From another working directory than the configuration's.
    let output = command(&scratch, &scratch.0)
        .args(["project/samples", "compute", "--write"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("ductility:"));
}

#[test]
fn single_class_needs_no_class_name() {
    let scratch = Scratch::new("single-class");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("v: 3.0"));
}

#[test]
fn ambiguous_class_is_refused() {
    let scratch = Scratch::new("ambiguous");
    project(
        &scratch,
        &format!("{BILLET}\n\nclass Other(sk.Sample):\n    pass\n"),
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("Billet") && err(&output).contains("Other"),
        "{}",
        err(&output)
    );
    assert!(!scratch.read("project/samples/c1.md").contains("ductility"));
}

#[test]
fn named_class_not_found_lists_available() {
    let scratch = Scratch::new("class-not-found");
    project(&scratch, BILLET);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Billett\"\npython = \".venv\"\n",
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("Billett") && err(&output).contains("Billet"),
        "{}",
        err(&output)
    );
}

#[test]
fn missing_template_file_names_the_path() {
    let scratch = Scratch::new("missing-template");
    project(&scratch, BILLET);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"gone.py\"\npython = \".venv\"\n",
    );
    assert!(matches!(
        runtime::availability(Some(&loaded(&scratch)), &scratch.at("project")),
        Err(ModelError::TemplateNotFound { path }) if path == scratch.at("project/gone.py")
    ));
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    project(&scratch, BILLET);
    billet(&scratch, "c3", 13.0, 0.0);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let written = err(&output);
    assert!(written.contains("ZeroDivisionError"), "{written}");
    assert!(written.contains("model.py\", line"), "{written}");
    assert!(
        written.contains("return self.diameter.value / self.volume.v"),
        "{written}"
    );
    assert!(!written.contains("_worker.py"), "{written}");
}

#[test]
fn a_value_refused_as_returned_names_its_formula_not_the_worker() {
    // A NaN, a tuple: refused in SampleKit's own frames, and the traceback
    // named `_worker.py` alone, in the output and in the log `explain` reads.
    let scratch = Scratch::new("refused-traceback");
    project(
        &scratch,
        &BILLET.replace(
            "return self.diameter.value / self.volume.value",
            "return float(\"nan\")",
        ),
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    let written = err(&output);
    assert!(written.contains("not a finite number"), "{written}");
    assert!(written.contains("model.py\", line"), "{written}");
    assert!(written.contains("in Billet._ductility"), "{written}");
    assert!(!written.contains("_worker.py"), "{written}");
    let explained = run(&scratch, &["samples/c1.md", "explain", "ductility"]);
    assert!(
        out(&explained).contains("in Billet._ductility"),
        "{}",
        out(&explained)
    );
    assert!(
        !out(&explained).contains("_worker.py"),
        "{}",
        out(&explained)
    );
}

#[test]
fn a_worker_that_dies_says_how_and_what_it_was_computing() {
    // *the Python worker exited without answering*, and nothing more.
    let scratch = Scratch::new("worker-dies");
    project(
        &scratch,
        &BILLET.replace(
            "return self.diameter.value / self.volume.value",
            "os._exit(3)",
        ),
    );
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    let written = err(&output);
    assert!(written.contains("exited without answering"), "{written}");
    assert!(written.contains("it exited with code 3"), "{written}");
    assert!(written.contains("computing ductility of"), "{written}");
    assert!(written.contains("c1.md"), "{written}");
}

#[test]
fn set_names_what_reads_a_value_never_computed_yet() {
    // *nothing derived rests on this*, said of the input a value waited for.
    let scratch = Scratch::new("set-readers");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let described = run(&scratch, &["samples", "status"]);
    assert!(code(&described) < 3, "{}", err(&described));
    let output = run(&scratch, &["samples/c3.md", "set", "volume=4"]);
    let said = out(&output);
    assert!(
        said.contains("ductility reads it, never computed yet"),
        "{said}\n{}",
        err(&output)
    );
    assert!(!said.contains("nothing derived rests on this"), "{said}");
}

#[test]
fn show_output_shows_what_the_model_logs() {
    // Printed lines were shown; `logging`'s went to the run log alone.
    let scratch = Scratch::new("show-logging");
    project(
        &scratch,
        &BILLET
            .replace(
                "import samplekit as sk\n",
                "import logging\n\nimport samplekit as sk\n",
            )
            .replace(
                "        return self.diameter.value / self.volume.value",
                "        logging.getLogger(\"billet\").info(\"ductility logged\")\n        \
             return self.diameter.value / self.volume.value",
            ),
    );
    let output = run(
        &scratch,
        &["samples/c1.md", "compute", "--write", "--show-output"],
    );
    let written = err(&output);
    assert!(written.contains("INFO ductility logged"), "{written}");
    assert!(written.contains("computing the ductility of"), "{written}");
}

// ---------------------------------------------------------- availability

#[test]
fn build_without_interpreter_reports_unavailable() {
    let scratch = Scratch::new("no-interpreter");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
        BILLET,
    );
    assert!(matches!(
        runtime::availability(Some(&loaded(&scratch)), &scratch.at("project")).unwrap(),
        Availability::Unavailable { reason } if reason.contains("names nothing that exists")
    ));
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("names nothing that exists"),
        "{}",
        err(&output)
    );
    for arguments in [vec!["samples", "list"], vec!["samples", "-c", "diameter"]] {
        assert_eq!(code(&run(&scratch, &arguments)), 0, "{arguments:?}");
    }
}

#[test]
fn the_interpreter_is_declared_never_searched_for() {
    // A `.venv` beside the project, and one above: without the line, neither
    // is taken, and what to write is said.
    let scratch = Scratch::new("declared");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        BILLET,
    );
    scratch.write("project/.venv/bin/python", "");
    scratch.write(".venv/bin/python", "");
    match runtime::interpreter_for(&loaded(&scratch), &scratch.at("project/samples")) {
        Err(ModelError::NoInterpreter { reason }) => {
            assert!(reason.contains("no interpreter is declared"), "{reason}");
            assert!(reason.contains("python = \".venv\""), "{reason}");
        }
        other => panic!("expected no interpreter, got {other:?}"),
    }
}

#[test]
fn python_names_an_environment_folder_or_an_interpreter() {
    let scratch = Scratch::new("explicit");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
        BILLET,
    );
    let venv = scratch.write("project/.venv/bin/python", "");
    assert_eq!(
        runtime::interpreter_for(&loaded(&scratch), &scratch.at("project/samples")).unwrap(),
        venv
    );
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"tools/python\"\n",
        BILLET,
    );
    let tools = scratch.write("project/tools/python", "");
    assert_eq!(
        runtime::interpreter_for(&loaded(&scratch), &scratch.at("project/samples")).unwrap(),
        tools
    );
    // Declared without a model, for the figures.
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npython = \".venv\"\n",
        BILLET,
    );
    assert_eq!(
        runtime::interpreter_for(&loaded(&scratch), &scratch.at("project/samples")).unwrap(),
        venv
    );
}

#[test]
fn missing_environment_reports_what_was_looked_for() {
    let scratch = Scratch::new("looked-for");
    bare_project(
        &scratch,
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
        BILLET,
    );
    let from = scratch.at("project/samples");
    match runtime::interpreter_for(&loaded(&scratch), &from) {
        Err(ModelError::NoInterpreter { reason }) => {
            assert!(reason.contains("no interpreter is declared"), "{reason}");
            assert!(reason.contains("samplekit"), "{reason}");
        }
        other => panic!("expected no interpreter, got {other:?}"),
    }
}

#[test]
fn three_availability_states_are_distinguishable() {
    let none = Scratch::new("three-none");
    none.write("project/.samplekitrc", "schema_version = 1\n");
    billet(&none, "c1", 12.0, 4.0);
    let unavailable = Scratch::new("three-unavailable");
    bare_project(
        &unavailable,
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
        BILLET,
    );
    let ready = Scratch::new("three-ready");
    project(&ready, BILLET);

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
        out(&run(&none, &["samples", "compute"])),
        err(&answering(&unavailable, &["samples", "compute"], "y\nn\n")),
        out(&run(&ready, &["samples", "compute", "--write"])),
    ];
    let phrases = [
        "no model is declared",
        "names nothing that exists",
        "computed 4 values",
    ];
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
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("ductility:"));
    let named = err(&output)
        .lines()
        .position(|line| line.starts_with("model ") && line.contains("model.py"));
    // Named first, from here; its configuration under -v.
    assert_eq!(named, Some(0), "{}", err(&output));
    assert!(
        err(&output).starts_with("model  model.py\n"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("[y]"), "{}", err(&output));
    assert!(!scratch.exists("state/consent"));
    assert!(!scratch.exists("state/accepted.git"));
    // What computed each value is kept in the project's history, never on
    // the machine.
    assert!(!scratch.exists("state/computed.json"));
    assert!(
        traced(&loaded(&scratch), &scratch.at("project/samples/c1.md")).contains_key("ductility")
    );
}

#[test]
fn run_model_and_consent_are_unknown_options() {
    // The flags of a consent that no longer exists are refused as any option a
    // command does not take, with no message of their own, and nothing runs.
    let scratch = Scratch::new("retired-flags");
    project(&scratch, BILLET);
    let before = scratch.read("project/samples/c1.md");
    for arguments in [
        vec!["samples", "compute", "--write", "--run-model"],
        vec!["samples", "compute", "--consent"],
        vec!["samples", "status", "--run-model"],
        vec!["samples", "list", "figures", "--run-model"],
        vec!["samples", "plot", "look", "-o", "x.png", "--run-model"],
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
    project(&scratch, BILLET);
    let before = scratch.read("project/samples/c1.md");
    for arguments in [
        vec!["samples", "status", "--no-model"],
        vec!["samples", "compute", "--no-model"],
        vec!["samples", "compute", "--write", "--no-model"],
        vec!["samples/c1.md", "explain", "ductility", "--no-model"],
    ] {
        let output = run(&scratch, &arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains("unexpected argument '--no-model'"),
            "{arguments:?}: {}",
            err(&output)
        );
    }
    let near = run(&scratch, &["samples", "compute", "--run-model"]);
    assert!(!err(&near).contains("--no-model"), "{}", err(&near));
    assert!(!scratch.exists("project/imported"));
    assert_eq!(scratch.read("project/samples/c1.md"), before);
}

#[test]
fn a_python_file_added_to_the_model_is_seen_at_the_next_reading() {
    let scratch = Scratch::new("walk-again");
    project(&scratch, BILLET);
    let template = runtime::template_of(&loaded(&scratch)).unwrap();
    let before = runtime::template_files(&template).unwrap();
    let added = scratch.write("project/helpers/fit.py", "SCALE = 1\n");
    assert!(!before.contains(&added));
    samplekit::collection::sample_list::from_directory(&scratch.at("project")).unwrap();
    let files = runtime::template_files(&template).unwrap();
    assert!(files.contains(&added), "{files:?}");
    fs::remove_file(&added).unwrap();
    samplekit::collection::sample_list::from_directory(&scratch.at("project")).unwrap();
    assert!(!runtime::template_files(&template).unwrap().contains(&added));
}

/// The workbench's plan, once the thread making it beside the screen gave it.
fn planned_in(workbench: &mut samplekit::tui::model::Workbench) {
    let start = std::time::Instant::now();
    while workbench.planning.is_some() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(60),
            "no plan came"
        );
        workbench.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_reload_plans_again_the_sample_whose_file_changed() {
    let scratch = Scratch::new("plan-again");
    let root = project(&scratch, BILLET);
    let mut workbench = samplekit::tui::model::Workbench::open(&root).unwrap();
    planned_in(&mut workbench);
    let owed = |workbench: &samplekit::tui::model::Workbench, name: &str| {
        workbench.owed[&scratch.at(&format!("project/samples/{name}.md"))].len()
    };
    let (c1, c2) = (owed(&workbench, "c1"), owed(&workbench, "c2"));
    assert!(c1 > 0, "{:?}", workbench.owed);
    // c1's ductility entered by hand: owed no longer; c2 as it was.
    let file = scratch.at("project/samples/c1.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace("properties:\n", "properties:\n  ductility: {v: 3.0}\n"),
    )
    .unwrap();
    workbench.reload();
    planned_in(&mut workbench);
    assert!(owed(&workbench, "c1") < c1, "{:?}", workbench.owed);
    assert_eq!(owed(&workbench, "c2"), c2);
}

#[test]
fn the_models_digest_covers_its_tree() {
    let scratch = Scratch::new("digest-tree");
    project(&scratch, BILLET);
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
    bare_project(&scratch, "", BILLET);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\", \"protocol\": 3}}}}\\n'\nread line\necho Killed >&2\nexit 137\n",
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
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let first = scratch.read("project/samples/c1.md");
    assert!(
        first.contains("  ductility:\n    v: 3.0\n    computed: {v: {diameter: "),
        "{first}"
    );
    assert!(first.contains("  roughness:\n    v: 0.25\n"), "{first}");
    assert!(scratch.read("project/samples/c2.md").contains("v: 2.5"));
    // Nothing is pending once saved.
    let again = run(&scratch, &["samples", "compute", "--write"]);
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
    billet(&scratch, "c1", 3.0, 1.5);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}\n{}", out(&output), err(&output));
    let written = scratch.read("project/samples/c1.md");
    assert!(written.contains("  ductility:\n    v: 2.0\n"), "{written}");
    assert!(written.contains("u: 0.01"), "{written}");
    assert!(
        written.contains("  pair:\n    v: 6.0\n    u: 0.5\n"),
        "{written}"
    );
    for (column, value, input) in [
        ("G", "0.5", "{row.R: "),
        ("G", "0.25", "{row.R: "),
        ("N", "0.5", "{preheating.R: "),
        ("N", "1.0", "{preheating.R: "),
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
        &["samples", "compute", "-p", "vfi,porosity", "-p", "swarf"],
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
        at("swarf") < at("porosity") && at("porosity") < at("vfi"),
        "{listed}"
    );
}

#[test]
fn properties_rerun_and_force_choose_what_is_planned() {
    let scratch = Scratch::new("properties-rerun-force");
    project(&scratch, BILLET);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    let edit = |file: &str, before: &str, after: &str| {
        let text = scratch.read(file);
        assert!(text.contains(before), "{text}");
        scratch.write(file, &text.replacen(before, after, 1));
    };
    let planned = |arguments: &[&str]| {
        let mut all = vec!["samples", "compute"];
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
    // Each with what it moved on from, as `status` says it.
    assert_eq!(
        planned(&[]),
        [
            "C1 ductility outdated — diameter",
            "C1 roughness outdated — ductility (itself not current)"
        ]
    );
    assert_eq!(
        planned(&["-p", "roughness"]),
        [
            "C1 ductility outdated — diameter",
            "C1 roughness outdated — ductility (itself not current)"
        ]
    );
    assert_eq!(
        planned(&["-p", "roughness", "--rerun"]),
        [
            "C1 ductility outdated — diameter",
            "C1 roughness outdated — ductility (itself not current)",
            "C2 roughness current"
        ]
    );
    // An override holds, unless forced.
    edit("project/samples/c2.md", "v: 2.5", "v: 2.6");
    assert_eq!(
        planned(&["--rerun"]),
        [
            "C1 ductility outdated — diameter",
            "C1 roughness outdated — ductility (itself not current)",
            "C2 roughness outdated — ductility"
        ]
    );
    assert_eq!(
        planned(&["--force"]),
        [
            "C1 ductility outdated — diameter",
            "C1 roughness outdated — ductility (itself not current)",
            "C2 ductility edited",
            "C2 roughness outdated — ductility"
        ]
    );
}

#[test]
fn a_failed_value_does_not_stop_the_run() {
    let scratch = Scratch::new("failure-continues");
    project(&scratch, BILLET);
    billet(&scratch, "c0", 13.0, 0.0);
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    assert!(
        report.contains("waits for ductility, which failed"),
        "{report}"
    );
    assert!(scratch.read("project/samples/c2.md").contains("ductility:"));
}

#[test]
fn a_dry_run_computes_and_writes_nothing() {
    let scratch = Scratch::new("dry-run");
    project(&scratch, BILLET);
    let before = scratch.read("project/samples/c1.md");
    let output = run(&scratch, &["samples", "compute"]);
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
    project(&scratch, BILLET);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
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
    let output = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let listed = out(&output);
    assert!(
        listed.lines().any(|line| line.contains("C1")
            && line.contains("ductility")
            && line.contains("outdated")),
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
    let status = run(&scratch, &["samples", "status"]);
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
    bare_project(&scratch, "", BILLET);
    fake_python(
        &scratch,
        "printf '{\"ready\": {\"version\": \"0.0.0-other\", \"python\": \"3\"}}\\n'\ncat > /dev/null\n",
    );
    assert!(matches!(
        start_fake(&scratch),
        Err(ModelError::VersionMismatch { package, .. }) if package == "0.0.0-other"
    ));
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    bare_project(&scratch, "", BILLET);
    fake_python(
        &scratch,
        "echo \"ModuleNotFoundError: No module named 'samplekit'\" >&2\nexit 1\n",
    );
    assert!(matches!(
        start_fake(&scratch),
        Err(ModelError::NotInstalled { .. })
    ));
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    project(&scratch, BILLET);
    let output = run(
        &scratch,
        &["samples", "compute", "--write", "--show-output"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("computing the ductility of C1"),
        "{}",
        err(&output)
    );
    assert!(
        !out(&output).contains("computing the ductility"),
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
        &BILLET.replace(
            "print(\"computing the ductility of\", self.name)",
            "input(\"continue? \")",
        ),
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project"),
        &["samples", "compute", "--write"],
    );
    assert!(err(&output).contains("EOFError"), "{}", err(&output));
    assert!(scratch.read("project/samples/c2.md").contains("ductility"));
}

#[test]
fn a_model_printing_an_undecodable_name_computes() {
    // A lone surrogate is valid Python and no JSON reader accepts it: the line
    // was refused, and the run reported as a dead worker that went on saving.
    let scratch = Scratch::new("surrogate");
    project(
        &scratch,
        &BILLET.replace(
            "print(\"computing the ductility of\", self.name)",
            "print(os.fsdecode(b\"bad\\xff\"), self.name)",
        ),
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project"),
        &["samples", "compute", "--write", "--show-output"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(err(&output).contains("bad\u{fffd} C1"), "{}", err(&output));
    assert!(scratch.read("project/samples/c2.md").contains("ductility"));
}

#[test]
fn a_module_where_compute_runs_shadows_nothing() {
    // The folder the command runs in is not on the worker's `sys.path`: a
    // `json.py` there replaced the module the protocol is spoken with.
    let scratch = Scratch::new("shadow");
    project(&scratch, BILLET);
    scratch.write("project/samples/json.py", "raise SystemExit('shadowed')\n");
    scratch.write(
        "project/samples/logging.py",
        "raise SystemExit('shadowed')\n",
    );
    let output = run_within_a_minute(
        &scratch,
        &scratch.at("project/samples"),
        &[".", "compute", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("shadowed"), "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("ductility"));
}

#[test]
fn a_worker_writing_an_unreadable_line_is_stopped() {
    // After the line, the fake worker would sleep a minute: it is killed, not
    // waited for.
    let scratch = Scratch::new("unreadable-line");
    bare_project(&scratch, "", BILLET);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\", \"protocol\": 3}}}}\\n'\nread line\n\
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
    bare_project(&scratch, "", BILLET);
    fake_python(
        &scratch,
        &format!(
            "printf '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3\", \"protocol\": 3}}}}\\n'\n\
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
    project(&scratch, BILLET);
    // What a computation records of the formulas that computed each value.
    let line = format!(
        "'{}' samples compute --write",
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
        scratch.exists("xdg/samplekit/logs"),
        "{}{}",
        out(&output),
        err(&output)
    );
    assert!(!scratch.exists("xdg/samplekit/computed.json"));
    assert!(!scratch.exists("home/.local"));
}

#[test]
fn a_computation_names_its_configuration_and_model() {
    let scratch = Scratch::new("names");
    project(&scratch, BILLET);
    // Said under -v, as every command says them; not otherwise.
    let quiet = run(&scratch, &["samples", "compute", "--write"]);
    assert!(!err(&quiet).contains("configuration  "), "{}", err(&quiet));
    assert!(err(&quiet).contains("model  model.py"), "{}", err(&quiet));
    let output = run(
        &scratch,
        &[
            "samples",
            "compute",
            "--rerun",
            "--write",
            "--show-output",
            "-v",
        ],
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
    let computing = written.find("computing the ductility").unwrap();
    assert!(configuration < model && model < computing, "{written}");
}

#[test]
fn a_value_a_sample_does_not_have_is_reported_not_raised() {
    let scratch = Scratch::new("unknown-value");
    project(&scratch, BILLET);
    // Another project inside the selection, whose model has no ductility.
    scratch.write(
        "project/samples/other/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"other.py\"\npython = \"../../.venv\"\n",
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
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n[collection]\nrecursive = true\n",
    );

    let mixed = run(
        &scratch,
        &["samples", "compute", "-p", "ductility", "--write"],
    );
    assert_eq!(code(&mixed), 0, "{}", err(&mixed));
    assert!(
        err(&mixed).contains("O1: unknown value 'ductility'"),
        "{}",
        err(&mixed)
    );
    assert!(!err(&mixed).contains("Traceback"), "{}", err(&mixed));
    assert!(scratch.read("project/samples/c1.md").contains("ductility:"));

    let none = run(&scratch, &["samples", "compute", "-p", "dutility"]);
    assert_eq!(code(&none), 1, "{}", err(&none));
    assert!(
        err(&none).contains("no selected sample has dutility"),
        "{}",
        err(&none)
    );
    assert!(!err(&none).contains("Traceback"), "{}", err(&none));
}

#[test]
fn stale_lists_values_never_computed_when_the_model_may_run() {
    let scratch = Scratch::new("stale-never-computed");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(
        text.contains("ductility") && text.contains("never computed"),
        "{text}"
    );
    assert!(text.contains("C1") && text.contains("C2"), "{text}");
    assert!(!scratch.exists("project/ran"), "a formula ran");
}

#[test]
fn a_repeated_error_prints_its_traceback_once() {
    let scratch = Scratch::new("repeated-error");
    project(&scratch, BILLET);
    billet(&scratch, "c0", 13.0, 0.0);
    billet(&scratch, "c3", 11.0, 0.0);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let errors = err(&output);
    assert!(
        errors.matches("Traceback (most recent call last)").count() <= 2,
        "{errors}"
    );
    assert!(
        errors.contains("the same error as C0 ductility's above")
            || errors.contains("the same error as C3 ductility's above"),
        "{errors}"
    );
}

#[test]
fn an_unknown_value_suggests_the_nearest() {
    let scratch = Scratch::new("unknown-value-nearest");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "-p", "dutility"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("did you mean: ductility?"),
        "{}",
        err(&output)
    );
}

#[test]
fn an_interrupted_run_keeps_what_finished_and_not_what_was_running() {
    let scratch = Scratch::new("interrupted");
    project(&scratch, SLOW);
    let started = std::time::Instant::now();
    let _ = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let output = run(&scratch, &["samples/c1.md", "explain", "preheating.G[10]"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("computed from"), "{text}");
    assert!(text.contains("row.R"), "{text}");
}

#[test]
fn a_sample_that_cannot_be_saved_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("not-saved");
    project(&scratch, BILLET);
    let samples = scratch.at("project/samples");
    fs::set_permissions(&samples, fs::Permissions::from_mode(0o555)).unwrap();
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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
    project(&scratch, BILLET);
    let all = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&all), 0, "{}", err(&all));
    let path = "project/samples/c1.md";
    let changed = scratch
        .read(path)
        .replace("diameter: {v: 12.0", "diameter: {v: 13.0");
    scratch.write(path, &changed);
    let narrowed = run(
        &scratch,
        &["samples", "compute", "-p", "ductility", "--write"],
    );
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
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    let finished: Vec<&str> = report.lines().filter(|line| line.contains('✓')).collect();
    assert_eq!(finished.len(), 4, "{report}");
    // Each formula here takes far less than a second: no duration is shown.
    for line in &finished {
        let line = line.trim_end();
        let duration =
            line.ends_with('s') && line[..line.len() - 1].ends_with(|c: char| c.is_ascii_digit());
        assert!(!duration, "{line}");
    }
    assert!(
        finished[0].contains("C1") && finished[0].contains("ductility"),
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
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  diameter: {v: \"12.0\", unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        out(&output).contains("diameter is text: \"12.0\""),
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
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
        BILLET,
    );
    let unavailable = run(&scratch, &["samples", "status"]);
    assert_eq!(code(&unavailable), 0, "{}", err(&unavailable));
    assert!(
        err(&unavailable).contains("2 samples have a model that was not read")
            && err(&unavailable).contains("names nothing that exists"),
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
        "schema_version = 1\n[model]\npath = \"missing.py\"\npython = \".venv\"\n",
    );
    let missing = run(&scratch, &["samples", "status"]);
    assert!(
        err(&missing).contains("cannot be found"),
        "{}",
        err(&missing)
    );
}

#[test]
fn a_failure_is_written_and_cleared_by_the_next_success() {
    let scratch = Scratch::new("failure-written");
    project(&scratch, BILLET);
    billet(&scratch, "c0", 13.0, 0.0);
    let failed = run(&scratch, &["samples/c0.md", "compute", "--write"]);
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
    assert!(log.contains("## ductility"), "{log}");
    // Roughness waited behind it, and has no failure of its own.
    assert!(!log.contains("## roughness"), "{log}");
    assert!(
        log.contains("ZeroDivisionError") && log.contains("division by zero"),
        "{log}"
    );
    let stale = run(&scratch, &["samples/c0.md", "status"]);
    assert!(out(&stale).contains("failed"), "{}", out(&stale));
    scratch.write(
        "project/samples/c0.md",
        &text.replace("volume: {v: 0.0", "volume: {v: 4.0"),
    );
    let fixed = run(&scratch, &["samples/c0.md", "compute", "--write"]);
    assert_eq!(code(&fixed), 0, "{}", err(&fixed));
    let text = scratch.read("project/samples/c0.md");
    assert!(!text.contains("failed"), "{text}");
    assert!(text.contains("ductility:"), "{text}");
}

#[test]
fn try_computes_and_writes_nothing() {
    let scratch = Scratch::new("try");
    project(&scratch, BILLET);
    let before = scratch.read("project/samples/c1.md");
    let output = answering(&scratch, &["samples", "compute"], "y\nn\n");
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("nothing written"), "{}", out(&output));
    assert_eq!(scratch.read("project/samples/c1.md"), before);
    assert!(scratch.exists("project/ran"));
}

#[test]
fn write_shows_before_and_after() {
    let scratch = Scratch::new("before-after");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("before") && text.contains("after"), "{text}");
    assert!(text.contains("ductility") && text.contains(" 3"), "{text}");
}

#[test]
fn an_interruption_says_what_it_kept() {
    let scratch = Scratch::new("interruption-said");
    project(&scratch, SLOW);
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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
    project(&scratch, BILLET);
    // Asked to compute, then to write.
    let output = at_terminal(&scratch, &["samples", "compute"], "yy\n");
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert!(
        out(&output).contains("apply these changes?"),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("written: 2 files"),
        "{}",
        out(&output)
    );
    assert!(scratch.read("project/samples/c1.md").contains("ductility:"));
}

/// One value that fails beside one that computes, so that a rehearsal has
/// something to offer to write and a failure to write with it.
const MIXED: &str = r#"import samplekit as sk


class Mixed(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.doubled = sk.Property(compute_value=lambda: self.diameter.value * 2)
        self.ductility = sk.Property(compute_value=lambda: self.diameter.value / self.volume.value)
        self.set_dependencies("doubled", depends_on=["diameter"])
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
"#;

#[test]
#[cfg(target_os = "linux")]
fn try_writes_no_failure_log_until_the_failure_is_written() {
    // The log follows the file. *Computes and writes nothing* covers
    // the log; and a rehearsal written afterwards writes the traceback of the
    // failure it saves.
    let scratch = Scratch::new("try-failure-log");
    project(&scratch, MIXED);
    billet(&scratch, "c0", 13.0, 0.0);
    let before = scratch.read("project/samples/c0.md");
    let tried = answering(&scratch, &["samples/c0.md", "compute"], "y\nn\n");
    assert_eq!(code(&tried), 2, "{}", err(&tried));
    assert!(err(&tried).contains("ZeroDivisionError"), "{}", err(&tried));
    assert_eq!(scratch.read("project/samples/c0.md"), before);
    assert!(
        !scratch.exists("project/.samplekit/failures/samples/c0.log"),
        "a rehearsal wrote a failure log"
    );
    assert!(!scratch.exists("project/.samplekit/failures"));

    // *write these values?* answered yes.
    let written = at_terminal(&scratch, &["samples/c0.md", "compute"], "yy\n");
    assert!(
        out(&written).contains("apply these changes?"),
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
    assert!(log.contains("## ductility"), "{log}");
    assert!(
        log.contains("ZeroDivisionError") && log.contains("division by zero"),
        "{log}"
    );
}

#[test]
fn before_and_after_count_the_cells_a_column_changed() {
    let scratch = Scratch::new("before-after-cells");
    project(&scratch, SHAPES);
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(out(&first).contains("2 new"), "{}", out(&first));
    let text = scratch.read("project/samples/c1.md");
    assert!(text.contains("R: 4.0"), "{text}");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("R: 4.0", "R: 8.0", 1),
    );
    let second = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    assert!(out(&second).contains("1 changed"), "{}", out(&second));
}

#[test]
fn write_says_how_many_samples_it_wrote() {
    let scratch = Scratch::new("write-says");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("written: 2 files"),
        "{}",
        out(&output)
    );
}

#[test]
fn readings_with_no_statistic_chosen_are_said_so_with_the_ways_out() {
    // Forcing a value beside dosings the model chooses no statistic for
    // has nothing to give back: said as that state, never as a missing value.
    let scratch = Scratch::new("no-statistic-named");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  \
         diameter: {v: 13.0, readings: [12.0, 12.2], unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = answering(
        &scratch,
        &["samples/c1.md", "compute", "-p", "diameter", "--force"],
        "y\nn\n",
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
    project(&scratch, BILLET);
    let output = answering(
        &scratch,
        &["samples/c1.md", "compute", "-p", "diameter"],
        "y\nn\n",
    );
    let said = format!("{}{}", out(&output), err(&output));
    assert!(!said.contains("Traceback"), "{said}");
    assert!(said.contains("no formula"), "{said}");
}

#[test]
fn compute_says_the_overrides_it_kept() {
    let scratch = Scratch::new("overrides-kept");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    assert!(text.contains("v: 3.0"), "{text}");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("v: 3.0", "v: 9.0", 1),
    );
    let listed = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("1 value typed over its formula is kept"),
        "{}",
        out(&listed)
    );
    let forced = run(&scratch, &["samples", "compute", "--force"]);
    assert!(!out(&forced).contains("kept"), "{}", out(&forced));
}

#[test]
fn stale_says_an_uncertainty_never_computed_beside_an_entered_value() {
    let scratch = Scratch::new("uncertainty-never");
    project(&scratch, SHAPES);
    // Entered in the file, as a hb is; only its uncertainty is a formula's.
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("unit: L}\n---", "unit: L}\n  age: {v: 2.0}\n---", 1),
    );
    let output = run(&scratch, &["samples", "status"]);
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
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.resolution = sk.Property(unit="mL")
        # An entered age whose uncertainty an instrument's resolution gives.
        self.age = sk.Property(
            unit="mL",
            compute_uncertainty=lambda: self.resolution.value / 2,
            depends_on=["resolution"],
        )
"#;

const DOPED: &str = r#"import samplekit as sk


class Doped(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.volume = sk.Property(unit="L")
        self.porosity = sk.Property(unit="g/L", compute_value=lambda: self.diameter.value / self.volume.value, depends_on=["diameter", "volume"])
"#;

#[test]
fn a_corrected_reading_is_recomputed_without_force() {
    // `status` had learnt to say *stale — readings* and `compute` could not act
    // on it — the plan took every reloaded statistic for an override, ran what
    // read it from the old mean, and left the sample stale for ever.
    let scratch = Scratch::new("corrected-doping");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let settled = run(&scratch, &["samples/c1.md", "status"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));

    // One doping corrected by hand, in an editor.
    let computed = scratch.read("project/samples/c1.md");
    assert!(computed.contains("[11.0, 12.0, 13.0]"), "{computed}");
    scratch.write(
        "project/samples/c1.md",
        &computed.replacen("[11.0, 12.0, 13.0]", "[11.0, 12.0, 16.0]", 1),
    );
    let stale = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );

    // An ordinary compute, no --force: diameter first, then what reads it.
    let repaired = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&repaired), 0, "{}", err(&repaired));
    assert!(!out(&repaired).contains("kept"), "{}", out(&repaired));
    let after = scratch.read("project/samples/c1.md");
    assert!(after.contains("v: 13.0"), "{after}");
    assert!(after.contains("v: 3.25"), "{after}");
    let settled = run(&scratch, &["samples/c1.md", "status"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));
}

const UNDECLARED_READ: &str = r#"import samplekit as sk


class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.shell = sk.Property(unit="g")
        # Reads `shell`, and says only `diameter`: where shell is absent it raises.
        self.net = sk.Property(unit="g", compute_value=lambda: self.diameter.value - self.shell.value,
                               depends_on=["diameter"])
"#;

#[test]
fn a_failure_the_model_no_longer_gives_is_cleared() {
    // The model fixed, the value that had failed now waits for its input: the
    // file went on saying *failed*, and its traceback stayed in the log.
    let scratch = Scratch::new("failure-cleared");
    project(&scratch, UNDECLARED_READ);
    let failed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let log = scratch.at("project/.samplekit/failures/samples/c1.log");
    assert!(log.exists(), "the traceback is logged");
    scratch.write(
        "project/model.py",
        &UNDECLARED_READ.replace(
            r#"depends_on=["diameter"]"#,
            r#"depends_on=["diameter", "shell"]"#,
        ),
    );
    let fixed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&fixed), 0, "{}", err(&fixed));
    let status = run(&scratch, &["samples", "status"]);
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
        &DOPED.replace("value=sk.stats.mean", "value=sk.stats.median"),
    );
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 16.0], unit: g}",
            1,
        ),
    );
    let tagged = run(
        &scratch,
        &["samples/c1.md", "tag", "add", "doped", "--write"],
    );
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let written = scratch.read("project/samples/c1.md");
    assert!(
        written.contains("diameter: {readings: [11.0, 12.0, 16.0], unit: g}"),
        "no mean written beside them: {written}"
    );
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    assert!(
        scratch.read("project/samples/c1.md").contains("v: 12.0"),
        "the median"
    );
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(out(&status).contains("is current"), "{}", out(&status));
    let computed = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &computed.replacen("[11.0, 12.0, 16.0]", "[11.0, 13.0, 16.0]", 1),
    );
    let stale = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );
    assert!(!out(&stale).contains("record missing"), "{}", out(&stale));
}

const ROWS_READ_A_PROPERTY: &str = r#"import samplekit as sk


class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.shell = sk.Property(unit="g")
        self.runs = sk.Table(
            {
                "run": sk.Property(),
                "load": sk.Property(),
                "net": sk.Property(compute=self._net, depends_on=["row.load", "shell"]),
            },
            index="run",
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
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("waits for shell"), "{}", out(&output));
    assert!(!out(&output).contains("failed"), "{}", out(&output));
}

#[test]
fn a_project_that_cannot_run_stops_only_itself() {
    // One project's missing model stopped the whole run, the other projects
    // never computed.
    let scratch = Scratch::new("one-project-stops");
    project(&scratch, DOPED);
    scratch.write(
        "project/broken/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"missing.py\"\npython = \".venv\"\n",
    );
    scratch.write(
        "project/broken/b1.md",
        "---\nschema_version: 1\nname: B1\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let output = run(&scratch, &["broken", "compute", "samples", "--write"]);
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
    let failed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let plan = run(&scratch, &["samples", "compute"]);
    assert!(out(&plan).contains("failed"), "{}", out(&plan));
    scratch.write(
        "project/model.py",
        &UNDECLARED_READ.replace(
            r#"depends_on=["diameter"]"#,
            r#"depends_on=["diameter", "shell"]"#,
        ),
    );
    let plan = run(&scratch, &["samples", "compute"]);
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
    project(&scratch, DOPED);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let again = run(&scratch, &["samples", "compute", "--rerun", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(out(&again).contains("nothing written"), "{}", out(&again));
}

#[test]
fn a_new_sample_says_in_one_line_what_its_readings_wait_for() {
    // A line each, the second written apart from the first and its end grey
    // as advice.
    let scratch = Scratch::new("new-readings-statistic");
    project(&scratch, DOPED);
    let made = run(
        &scratch,
        &[
            "samples",
            "new",
            "c9",
            "diameter.readings=11,12,13",
            "--write",
        ],
    );
    assert_eq!(code(&made), 0, "{}", err(&made));
    assert!(
        out(&made).contains(
            "diameter waits for samplekit compute: the mean of its readings, as the model declares"
        ),
        "{}",
        out(&made)
    );
}

#[test]
fn readings_set_over_a_statistic_leave_it_stale_not_edited() {
    // New readings over a statistic once wrote the new mean over it, the digest
    // moved, and the value read *edited*: a hand's, which compute then left
    // alone. Given as the readings channel.
    let scratch = Scratch::new("set-readings-statistic");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let set = run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "diameter.readings=11.0,12.0,16.0",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    // The value and the uncertainty its file records wait alike: one line,
    // where the uncertainty's once ended on a grey command read as advice.
    assert!(
        out(&set).contains(
            "its value and its uncertainty, their mean and standard error, wait for \
             samplekit compute"
        ),
        "{}",
        out(&set)
    );
    assert!(!out(&set).contains("as its file records"), "{}", out(&set));
    let stale = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&stale).contains("outdated \u{2014} readings"),
        "{}",
        out(&stale)
    );
    assert!(!out(&stale).contains("edited"), "{}", out(&stale));
    let repaired = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&repaired), 0, "{}", err(&repaired));
    // What it is about to compute is not said stale in the middle of the run.
    assert!(
        !err(&repaired).contains("is outdated"),
        "{}",
        err(&repaired)
    );
    assert!(scratch.read("project/samples/c1.md").contains("v: 13.0"));
    let settled = run(&scratch, &["samples/c1.md", "status"]);
    assert!(out(&settled).contains("is current"), "{}", out(&settled));
}

#[test]
fn readings_no_statistic_was_taken_of_are_never_computed_everywhere() {
    // The demo's blonde steel held its hardnesses as readings alone. The table
    // showed `—`, and `status`, `compute` and Python's `not_current` said
    // nothing of them: the model gives the statistic as it reads the sample, so
    // the plan found nothing owed, and `explain` said it *waits for readings*
    // the file held.
    let scratch = Scratch::new("statistic-never-taken");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    // A sample whose statistic was taken, for `explain` to learn from.
    let other = run(&scratch, &["samples/c2.md", "compute", "--write"]);
    assert_eq!(code(&other), 0, "{}", err(&other));
    let status = run(&scratch, &["samples/c1.md", "status"]);
    let line = out(&status)
        .lines()
        .find(|line| line.contains(" diameter "))
        .map(str::to_string);
    assert!(
        line.as_deref()
            .is_some_and(|line| line.contains("never computed")),
        "{}",
        out(&status)
    );
    let plan = run(&scratch, &["samples/c1.md", "compute"]);
    assert!(
        out(&plan)
            .lines()
            .any(|line| line.contains(" diameter ") && line.contains("never computed")),
        "{}",
        out(&plan)
    );
    let explain = run(&scratch, &["samples/c1.md", "explain", "diameter"]);
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
             print(s.not_current().get('diameter'), '|', s.diameter.state)",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&python.stdout).trim(),
        "never computed | never_computed",
        "{}",
        String::from_utf8_lossy(&python.stderr)
    );
    // Taken, it is current everywhere.
    let written = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(scratch.read("project/samples/c1.md").contains("v: 12.0"));
    let settled = run(&scratch, &["samples/c1.md", "status"]);
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
    let written = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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

    let explained = run(&scratch, &["samples/c1.md", "explain", "age"]);
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
        &["samples/c1.md", "set", "resolution=0.02", "--write"],
    );
    assert_eq!(code(&changed), 0, "{}", err(&changed));
    assert!(out(&changed).contains("age"), "{}", out(&changed));
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("age") && out(&status).contains("outdated — resolution"),
        "{}",
        out(&status)
    );
    let again = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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
    let missing = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&missing).contains("age") && out(&missing).contains("record missing"),
        "{}",
        out(&missing)
    );
}

#[test]
fn a_derived_value_without_its_record_is_an_override_until_forced() {
    let scratch = Scratch::new("record-missing");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
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
    let stale = run(&scratch, &["samples", "status"]);
    assert!(out(&stale).contains("record missing"), "{}", out(&stale));
    let forced = run(&scratch, &["samples", "compute", "--force", "--write"]);
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    let again = run(&scratch, &["samples", "status"]);
    assert!(!out(&again).contains("record missing"), "{}", out(&again));
}

#[test]
fn try_says_the_comments_a_write_drops() {
    let scratch = Scratch::new("try-comments");
    project(&scratch, BILLET);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen("name: C1\n", "name: C1\n# doped twice\n", 1),
    );
    let output = answering(&scratch, &["samples/c1.md", "compute"], "y\nn\n");
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a write drops 1 YAML comment"),
        "{}",
        out(&output)
    );
}

/// `status` over the project's samples, the model allowed to run.
fn status(scratch: &Scratch) -> Output {
    run(scratch, &["samples", "status"])
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

// `script -e -c` is util-linux's; macOS's `script` takes neither.
#[cfg(target_os = "linux")]
#[test]
fn a_key_typed_before_a_question_does_not_answer_it() {
    // In a terminal `compute` asks with one key: the Enter typed by habit
    // after its `y` waited through the computation, answered the question to
    // write *no*, and the computation was thrown away.
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    let scratch = Scratch::new("typed-ahead");
    project(&scratch, BILLET);
    let line = format!("{} samples compute", env!("CARGO_BIN_EXE_samplekit"));
    let mut child = Command::new("script")
        .args(["-q", "-e", "-c", &line, "/dev/null"])
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("NO_COLOR", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("script runs");
    let seen = Arc::new(Mutex::new(String::new()));
    let mut stdout = child.stdout.take().unwrap();
    let reader = {
        let seen = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(read) = stdout.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                seen.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
        })
    };
    let questions = |count: usize| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while seen.lock().unwrap().matches("[y/N]").count() < count {
            assert!(
                std::time::Instant::now() < deadline,
                "{}",
                seen.lock().unwrap()
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    let mut stdin = child.stdin.take().unwrap();
    questions(1);
    stdin.write_all(b"y\r").unwrap();
    stdin.flush().unwrap();
    questions(2);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        !seen.lock().unwrap().contains("nothing written"),
        "{}",
        seen.lock().unwrap()
    );
    stdin.write_all(b"y").unwrap();
    stdin.flush().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    assert!(
        seen.lock().unwrap().contains("written:"),
        "{}",
        seen.lock().unwrap()
    );
    assert!(scratch.read("project/samples/c1.md").contains("ductility"));
}

#[test]
fn a_new_figure_makes_nothing_stale() {
    // A figure, a comment, a docstring and a helper nobody calls change no
    // formula: nothing is stale, and no model is said changed.
    let scratch = Scratch::new("new-figure");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let edited = BILLET
        .replace(
            "    def _ductility(self):\n",
            "    def _ductility(self):\n        \"\"\"Diameter over volume.\"\"\"\n        # g/L\n",
        )
        .replace(
            "class Billet(sk.Sample):",
            "def unused(x):\n    return x * 2\n\n\nclass Billet(sk.Sample):",
        )
        + "\n    @sk.figure\n    def curve(self, ax):\n        ax.plot([1], [self.ductility.value])\n";
    scratch.write("project/model.py", &edited);
    let status = status(&scratch);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("every computed value in 2 samples is current"),
        "{}",
        out(&status)
    );
    assert!(!err(&status).contains("model changed"), "{}", err(&status));
    let plan = run(&scratch, &["samples", "compute"]);
    assert!(
        out(&plan).contains("nothing to compute in 2 samples"),
        "{}",
        out(&plan)
    );
}

#[test]
fn renaming_a_method_or_the_class_recomputes_nothing() {
    // `_ductility` renamed `_by_volume`, `Billet` renamed `Tin`, bodies the same:
    // every value was *formula changed*, and hours recomputed.
    let scratch = Scratch::new("renamed");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let renamed = BILLET
        .replace("self._ductility", "self._by_volume")
        .replace("def _ductility(self)", "def _by_volume(self)")
        .replace("class Billet(", "class Tin(");
    scratch.write("project/model.py", &renamed);
    let described = status(&scratch);
    assert_eq!(code(&described), 0, "{}", err(&described));
    assert!(
        out(&described).contains("every computed value in 2 samples is current"),
        "{}",
        out(&described)
    );
    // Its body changed, it is changed.
    scratch.write(
        "project/model.py",
        &renamed.replace(
            "self.diameter.value / self.volume.value",
            "self.diameter.value / self.volume.value / 2",
        ),
    );
    assert_eq!(said(&status(&scratch), "ductility", "formula changed"), 2);
}

#[test]
fn a_computed_value_is_written_in_the_unit_the_model_declares() {
    // `ductility` moved from g/L to kg/L, its formula divided by 1000: the new
    // number was written under the old unit, wrong by a factor of 1000.
    let scratch = Scratch::new("declared-unit");
    let root = project(
        &scratch,
        &BILLET.replace(
            "sk.Property(compute_value=self._ductility)",
            "sk.Property(compute_value=self._ductility, unit=\"g/L\")",
        ),
    );
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(
        fs::read_to_string(root.join("samples/c1.md"))
            .unwrap()
            .contains("unit: g/L")
    );
    scratch.write(
        "project/model.py",
        &BILLET
            .replace(
                "sk.Property(compute_value=self._ductility)",
                "sk.Property(compute_value=self._ductility, unit=\"kg/L\")",
            )
            .replace(
                "return self.diameter.value / self.volume.value",
                "return self.diameter.value / self.volume.value / 1000",
            ),
    );
    // A unit is part of its formula: changed, it is *formula changed*.
    assert_eq!(said(&status(&scratch), "ductility", "formula changed"), 2);
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let written = fs::read_to_string(root.join("samples/c1.md")).unwrap();
    assert!(
        written.contains("v: 0.003") && written.contains("unit: kg/L"),
        "{written}"
    );
    assert!(!written.contains("unit: g/L"), "{written}");
}

#[test]
fn a_formula_writes_only_its_own_value() {
    // `self.diameter.value += 100` in a formula: the preview showed its value, the
    // write rewrote the measurement in silence, and each `--rerun --write`
    // added 100 again.
    let scratch = Scratch::new("formula-writes");
    let root = project(
        &scratch,
        &BILLET.replace(
            "        return self.diameter.value / self.volume.value",
            "        self.diameter.value += 100\n        return self.diameter.value / self.volume.value",
        ),
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    let said = format!("{}{}", out(&computed), err(&computed));
    assert!(
        said.contains("a formula writes only the value it returns: this one changed 'diameter'"),
        "{said}"
    );
    let written = fs::read_to_string(root.join("samples/c1.md")).unwrap();
    assert!(written.contains("diameter: {v: 12.0, unit: g"), "{written}");
    assert!(!written.contains("v: 28"), "{written}");
    // An attribute alike.
    scratch.write(
        "project/model.py",
        &BILLET.replace(
            "        return self.diameter.value / self.volume.value",
            "        self.style = \"changed\"\n        return self.diameter.value / self.volume.value",
        ),
    );
    let computed = run(&scratch, &["samples", "compute", "--write", "--rerun"]);
    let said = format!("{}{}", out(&computed), err(&computed));
    assert!(said.contains("this one changed 'style'"), "{said}");
    assert!(
        !fs::read_to_string(root.join("samples/c1.md"))
            .unwrap()
            .contains("changed")
    );
}

#[test]
fn an_edited_formula_stales_its_values_and_what_reads_them() {
    let scratch = Scratch::new("edited-formula");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &BILLET.replace(
            "return self.diameter.value / self.volume.value",
            "return 2 * self.diameter.value / self.volume.value",
        ),
    );
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "ductility", "formula changed"),
        2,
        "{}",
        out(&stale)
    );
    assert_eq!(
        said(
            &stale,
            "roughness",
            "reads ductility, whose formula changed"
        ),
        2,
        "{}",
        out(&stale)
    );
    assert!(!err(&stale).contains("model changed"), "{}", err(&stale));
    // Computed without --rerun: a changed formula is not current.
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    assert!(
        scratch.read("project/samples/c1.md").contains("v: 6.0"),
        "{}",
        scratch.read("project/samples/c1.md")
    );
    let current = status(&scratch);
    assert!(
        out(&current).contains("every computed value in 2 samples is current"),
        "{}",
        out(&current)
    );
}

#[test]
fn a_changed_column_statistic_stales_its_cells() {
    let scratch = Scratch::new("column-statistic");
    let viscous = BILLET.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.toughness = sk.Table(\n            {\"T\": sk.Property(), \"stiffness\": sk.Property(unit=\"cP\", \
         value=sk.stats.mean)},\n            index=\"T\",\n        )\n        self.set_dependencies(\"ductility\"",
    );
    project(&scratch, &viscous);
    let added = run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "--add-row",
            "toughness",
            "T=40",
            "stiffness.readings=[1,2,6]",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    let first = run(&scratch, &["samples", "compute", "--write"]);
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
        said(&stale, "toughness.stiffness", "formula changed"),
        1,
        "{}",
        out(&stale)
    );
    let second = run(&scratch, &["samples", "compute", "--write"]);
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
fn a_new_row_takes_readings_in_a_column_of_their_statistic() {
    // Once a cell of the column was computed, a new row's readings were
    // refused as a computed column's, the message before naming that very
    // syntax.
    let scratch = Scratch::new("readings-new-row");
    let viscous = BILLET.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.toughness = sk.Table(\n            {\"T\": sk.Property(), \"stiffness\": sk.Property(unit=\"cP\", \
         value=sk.stats.mean)},\n            index=\"T\",\n        )\n        self.set_dependencies(\"ductility\"",
    );
    project(&scratch, &viscous);
    let row = |at: &str| {
        run(
            &scratch,
            &[
                "samples/c1.md",
                "set",
                "--add-row",
                "toughness",
                at,
                "stiffness.readings=[1,2,6]",
                "--write",
            ],
        )
    };
    assert_eq!(code(&row("T=40")), 0);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let added = row("T=50");
    assert_eq!(code(&added), 0, "{}", err(&added));
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let file = scratch.read("project/samples/c1.md");
    assert_eq!(
        file.matches("stiffness: {v: 3.0, readings: [1.0, 2.0, 6.0]")
            .count(),
        2,
        "{file}"
    );
}

#[test]
fn a_column_of_a_computed_uncertainty_is_entered_in_a_new_row() {
    // Its value is the hand's: a new row's preview listed it among the
    // columns the model computes.
    let scratch = Scratch::new("uncertainty-new-row");
    let rated = BILLET.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.pressure = sk.Table(\n            {\"T\": sk.Property(), \"p\": sk.Property(unit=\"bar\", \
         compute_uncertainty=lambda row: 0.005 * row.p.value, depends_on=[\"row.p.v\"])},\n            \
         index=\"T\",\n        )\n        self.set_dependencies(\"ductility\"",
    );
    project(&scratch, &rated);
    let row = |at: &str, write: bool| {
        let mut arguments = vec!["samples/c1.md", "set", "--add-row", "pressure", at, "p=2.0"];
        if write {
            arguments.push("--write");
        }
        run(&scratch, &arguments)
    };
    assert_eq!(code(&row("T=40", true)), 0);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    let preview = row("T=50", false);
    assert!(
        !out(&preview).contains("computed by the model"),
        "{}",
        out(&preview)
    );
    assert_eq!(code(&row("T=50", true)), 0);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    let file = scratch.read("project/samples/c1.md");
    assert_eq!(file.matches("p: {v: 2.0, u: 0.01").count(), 2, "{file}");
}

#[test]
fn status_says_a_cell_resting_on_a_stale_cell_of_its_row() {
    // The command line reads the table from the file: a cell reading another
    // computed cell of its row was called current while that one was not.
    let scratch = Scratch::new("chained-cells");
    let chained = BILLET.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.preheat = sk.Table(\n            {\"T\": sk.Property(), \"g\": sk.Property(\
         compute=lambda row: 2 * self.diameter.value, depends_on=[\"diameter\"]), \"h\": sk.Property(\
         compute=lambda row: row.g.value + 1, depends_on=[\"row.g\"])},\n            \
         index=\"T\",\n        )\n        self.set_dependencies(\"ductility\"",
    );
    project(&scratch, &chained);
    let added = run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "--add-row",
            "preheat",
            "T=1",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "preheat.h", "outdated — row.g (itself not current)"),
        1,
        "{}",
        out(&stale)
    );
}

#[test]
fn a_column_never_computed_is_headed_by_the_unit_the_model_declares() {
    // Before its first computation no sample held the value, and its header
    // lost the unit it would carry after.
    let scratch = Scratch::new("declared-unit-header");
    let dense = BILLET.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.density = sk.Property(unit=\"g/L\", compute=lambda: self.diameter.value / \
         self.volume.value, depends_on=[\"diameter\", \"volume\"])\n        \
         self.set_dependencies(\"ductility\"",
    );
    project(&scratch, &dense);
    // The description taken, as any command reading the model takes it.
    assert_eq!(code(&status(&scratch)), 0);
    let listed = run(&scratch, &["samples", "-c", "name,density"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(out(&listed).contains("density [g/L]"), "{}", out(&listed));
}

#[test]
fn an_edited_helper_stales_the_formulas_that_reach_it() {
    let scratch = Scratch::new("edited-helper");
    let helped = BILLET
        .replace(
            "class Billet(sk.Sample):",
            "SCALE = 1.0\n\n\ndef scaled(x):\n    return x * SCALE\n\n\n\
             def unused(x):\n    return x\n\n\nclass Billet(sk.Sample):",
        )
        .replace(
            "return self.diameter.value / self.volume.value",
            "return scaled(self.diameter.value / self.volume.value)",
        );
    project(&scratch, &helped);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    // A helper nobody calls.
    scratch.write(
        "project/model.py",
        &helped.replace("    return x\n", "    return x + 1\n"),
    );
    let untouched = status(&scratch);
    assert!(
        out(&untouched).contains("every computed value in 2 samples is current"),
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
        said(&constant, "ductility", "formula changed"),
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
        said(&helper, "ductility", "formula changed"),
        2,
        "{}",
        out(&helper)
    );
}

#[test]
fn a_cache_a_formula_fills_stales_nothing() {
    // A cache at the module's level, filled as each sample computes: its
    // contents are not the formula's, the constant beside it is.
    let scratch = Scratch::new("filled-cache");
    let cached = BILLET
        .replace(
            "class Billet(sk.Sample):",
            "SCALE = 1.0\n_SEEN = {}\n\n\ndef scaled(x):\n    _SEEN[x] = x * SCALE\n    \
             return _SEEN[x]\n\n\nclass Billet(sk.Sample):",
        )
        .replace(
            "return self.diameter.value / self.volume.value",
            "return scaled(self.diameter.value / self.volume.value)",
        );
    project(&scratch, &cached);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let current = status(&scratch);
    assert!(
        out(&current).contains("every computed value in 2 samples is current"),
        "{}",
        out(&current)
    );
    scratch.write(
        "project/model.py",
        &cached.replace("SCALE = 1.0", "SCALE = 2.0"),
    );
    let constant = status(&scratch);
    assert_eq!(
        said(&constant, "ductility", "formula changed"),
        2,
        "{}",
        out(&constant)
    );
}

#[test]
fn a_formula_edited_by_its_run_is_said_changed() {
    // The digest recorded is of the formula as the run imported it: taken from
    // the file after the run, the second sample was recorded as computed by a
    // version that never ran.
    let scratch = Scratch::new("edited-by-its-run");
    project(
        &scratch,
        &BILLET.replace(
            "        return self.diameter.value / self.volume.value\n",
            "        text = open(__file__).read()\n        \
             open(__file__, \"w\").write(text.replace(\"+ 0.0  # offset\", \"+ 1.0  # offset\"))\n        \
             return self.diameter.value / self.volume.value + 0.0  # offset\n",
        ),
    );
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(scratch.read("project/model.py").contains("+ 1.0  # offset"));
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "ductility", "formula changed"),
        2,
        "{}",
        out(&stale)
    );
}

#[test]
fn a_failing_formula_keeps_the_last_good_value() {
    let scratch = Scratch::new("last-good");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replace("volume: {v: 4.0", "volume: {v: 0.0"),
    );
    let failed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let text = scratch.read("project/samples/c1.md");
    let ductility = &text[text.find("ductility:").expect("ductility is written")..];
    let ductility = &ductility[..ductility.find("roughness:").unwrap_or(ductility.len())];
    assert!(
        ductility.contains("3.0")
            && ductility.contains("computed:")
            && ductility.contains("failed:"),
        "{text}"
    );
    let table = run(
        &scratch,
        &["samples/c1.md", "-c", "ductility", "--width", "120"],
    );
    assert!(out(&table).contains("3 ✗"), "{}", out(&table));
    // Written as the table shows it, its state beside it.
    let csv = run(&scratch, &["samples/c1.md", "-c", "ductility", "--csv"]);
    assert!(out(&csv).contains("3.0,failed"), "{}", out(&csv));
}

#[test]
#[cfg(target_os = "linux")]
fn a_try_written_at_the_prompt_is_recorded_and_kept() {
    // The values written at the prompt are a write like `--write`'s: the
    // formula that computed each is kept in the trace, and the history keeps
    // one snapshot.
    let scratch = Scratch::new("try-prompt");
    let root = project(&scratch, BILLET);
    let output = at_terminal(&scratch, &["samples", "compute"], "yy\n");
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert!(scratch.read("project/samples/c1.md").contains("ductility"));
    // And the formula of each value it wrote, in the project's trace.
    assert!(
        traced(&loaded(&scratch), &scratch.at("project/samples/c1.md")).contains_key("ductility")
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
        Some("samplekit samples compute --write"),
        "{messages}"
    );
}

#[test]
fn a_dry_run_names_the_flags_that_apply_it() {
    // A preview says what applies it, on the line that reports it.
    let scratch = Scratch::new("dry-run-next-step");
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains(
            "4 values in 2 samples would be computed — --write computes and writes without asking"
        ),
        "{}",
        out(&output)
    );
}

#[test]
fn the_columns_of_a_table_computed_together_share_one_line() {
    let scratch = Scratch::new("table-one-line");
    project(&scratch, SHAPES);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    let tables: Vec<&str> = report
        .lines()
        .filter(|line| line.contains('✓') && line.contains("preheating"))
        .collect();
    assert_eq!(tables.len(), 2, "{report}");
    assert!(
        tables.iter().all(|line| line.contains("preheating: G, N")),
        "{report}"
    );
}

#[test]
fn a_rerun_marks_what_it_left_unchanged() {
    let scratch = Scratch::new("rerun-unchanged");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    // The scratch directory's name holds the word: a row of the table is read.
    assert!(
        !out(&first)
            .lines()
            .any(|line| line.trim_end().ends_with("unchanged")),
        "{}",
        out(&first)
    );
    let again = run(&scratch, &["samples", "compute", "--rerun", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let report = out(&again);
    assert!(
        report
            .lines()
            .any(|line| line.starts_with("ductility") && line.trim_end().ends_with("unchanged")),
        "{report}"
    );
    assert!(
        report.contains("computed 4 values in 2 samples, all unchanged"),
        "{report}"
    );
}

#[test]
fn a_narrow_terminal_hides_before_and_keeps_after() {
    let scratch = Scratch::new("narrow-changes");
    project(&scratch, BILLET);
    let output = run(
        &scratch,
        &["samples", "compute", "--write", "--width", "20"],
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
    project(&scratch, BILLET);
    let raw = in_terminal_of(
        &scratch,
        "80",
        &["samples", "compute", "--write", "--show-output"],
    );
    let printed: Vec<usize> = raw
        .match_indices("computing the ductility of")
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
    project(&scratch, BILLET);
    let raw = in_terminal_of(&scratch, "40", &["samples", "compute", "--write"]);
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
         self.diameter = sk.Property(unit=\"g\")\n        \
         self.ductility = sk.Property(compute_value=self._ductility)\n        \
         self.set_dependencies(\"ductility\", depends_on=[\"diameter\"])\n\n    \
         def _ductility(self):\n        time.sleep(2.2)\n        return self.diameter.value / 4\n",
    );
    let raw = in_terminal_of(&scratch, "120", &["samples/c1.md", "compute", "--write"]);
    let bars: Vec<&str> = raw
        .split("\r\x1b[2K")
        .filter(|segment| segment.starts_with('[') && segment.contains("ductility"))
        .collect();
    assert!(bars.len() >= 3, "{raw:?}");
    assert!(
        bars.iter()
            .any(|bar| bar.contains("ductility 1.") || bar.contains("ductility 2.")),
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
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        !err(&output).contains("computing the ductility"),
        "{}",
        err(&output)
    );
    let report = out(&output);
    assert!(report.contains("the model printed 2 lines — "), "{report}");
    let log = run_log_named(&report);
    assert!(
        log.contains("[C1] ductility: computing the ductility of C1"),
        "{log}"
    );
    assert!(scratch.at("state/logs").is_dir());
}

/// A model that logs an INFO record and a WARNING while computing.
const LOGGED: &str = r#"import logging

import samplekit as sk

LOG = logging.getLogger("billet")


class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=self._ductility)
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])

    def _ductility(self):
        LOG.info("measured %s", self.name)
        LOG.warning("thin billet %s", self.name)
        return self.diameter.value / self.volume.value
"#;

#[test]
fn logged_warnings_are_one_line_and_kept() {
    let scratch = Scratch::new("logged-warning");
    project(&scratch, LOGGED);
    let output = run(&scratch, &["samples", "compute", "--write", "-v"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = err(&output);
    assert!(shown.contains("C1 ductility: thin billet C1"), "{shown}");
    assert_eq!(
        shown.matches("the model logged 2 warnings").count(),
        1,
        "{shown}"
    );
    assert!(!shown.contains("measured C1"), "{shown}");
    let log = run_log_named(&out(&output));
    assert!(
        log.contains("[C1] ductility: WARNING thin billet C1"),
        "{log}"
    );
    assert!(log.contains("[C1] ductility: INFO measured C1"), "{log}");
}

#[test]
fn a_failed_value_keeps_its_own_traceback_when_another_reads_it() {
    // `second` read `first`, which failed again inside it: the exception the
    // two shared gathered `second`'s frames and note, and `explain first`
    // showed them as its own.
    let scratch = Scratch::new("own-traceback");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.t = sk.Table(
            {
                "x": sk.Property(),
                "y": sk.Property(compute=self._y, depends_on=["row.x", "diameter"]),
            },
            index="x",
        )
        self.first = sk.Property(compute=self._first, depends_on=["t.y"])
        self.second = sk.Property(compute=self._second, depends_on=["first", "t.y"])

    def _y(self, row):
        return row.x.value / self.diameter.value

    def _first(self):
        return sum(self.t.values("y"))

    def _second(self):
        return 2 * self.first.value
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: 1.0\ntables:\n  t:\n    \
         index: x\n    columns:\n      x: {}\n    rows:\n      - {x: 1}\n---\n",
    );
    // Computed once, then failing: each value keeps its last number, and
    // `second` runs, reading `first` again.
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let held = scratch
        .read("project/samples/c1.md")
        .replace("diameter: 1.0", "diameter: 0.0");
    scratch.write("project/samples/c1.md", &held);
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let first = out(&run(&scratch, &["samples/c1.md", "explain", "first"]));
    assert!(first.contains("in _first"), "{first}");
    assert!(first.contains("while computing 'first'"), "{first}");
    assert!(!first.contains("_second"), "{first}");
    assert!(!first.contains("while computing 'second'"), "{first}");
    // The one exception went through both: `second`'s says where it began.
    let second = out(&run(&scratch, &["samples/c1.md", "explain", "second"]));
    assert!(
        second.contains("in _second") && second.contains("in _first"),
        "{second}"
    );
}

#[test]
fn a_failure_shows_what_its_value_printed_last() {
    let scratch = Scratch::new("failure-tail");
    project(&scratch, BILLET);
    billet(&scratch, "c0", 13.0, 0.0);
    let output = run(&scratch, &["samples/c0.md", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let text = err(&output);
    assert!(text.contains("the last lines ductility printed:"), "{text}");
    assert!(text.contains("    computing the ductility of C0"), "{text}");
}

#[test]
fn a_run_log_is_the_last_run_of_its_project() {
    // One log a project, replaced by each run.
    let scratch = Scratch::new("run-log-kept");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let first_log = run_log_named(&out(&first));
    billet(&scratch, "c3", 14.0, 4.0);
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let names: Vec<String> = fs::read_dir(scratch.at("state/logs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("compute-project-"), "{names:?}");
    let last = run_log_named(&out(&second));
    assert!(last.contains("C3"), "{last}");
    assert_ne!(last, first_log);
}

#[test]
fn warnings_logged_are_counted_each() {
    // The same warning from two samples is two warnings, said once.
    let scratch = Scratch::new("warnings-counted");
    project(
        &scratch,
        &LOGGED.replace("\"thin billet %s\", self.name", "\"thin billet\""),
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = err(&output);
    assert_eq!(
        shown.matches("the model logged 2 warnings").count(),
        1,
        "{shown}"
    );
    let log = run_log_named(&out(&output));
    assert_eq!(log.matches("WARNING thin billet").count(), 2, "{log}");
}

#[test]
fn a_model_looping_as_it_loads_is_stopped_for_every_command() {
    // `status` and `compute`'s preview stop it as `--write` does, and
    // `validate` never waits on it.
    let scratch = Scratch::new("loading-loops");
    project(&scratch, &format!("{BILLET}\nwhile True:\n    pass\n"));
    for arguments in [
        &["samples", "status", "--timeout", "1"][..],
        &["samples", "validate", "--timeout", "1"][..],
        &["samples", "compute", "--timeout", "1"][..],
    ] {
        let started = std::time::Instant::now();
        let output = run(&scratch, arguments);
        let said = err(&output);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{arguments:?}: {said}"
        );
        // `validate` reads no more of the model than its kept description.
        let stopped = usize::from(arguments[1] != "validate");
        assert_eq!(
            said.matches("loading ran past").count(),
            stopped,
            "{arguments:?}: {said}"
        );
    }
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
        self.diameter = sk.Property(unit="g")
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=lambda row: 1 / row.R.value, depends_on=["row.R"]),
                "N": sk.Property(compute_column=lambda columns: [value / 4.0 for value in columns["R"].values], depends_on=["preheating.R"]),
            },
            index="T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        )
        self.late = sk.Property(compute_value=self._late)
        self.set_dependencies("late", depends_on=["preheating.G", "preheating.N"])

    def _late(self):
        print("late starts", flush=True)
        return 1.0
"#,
    );
    let raw = in_terminal_of(
        &scratch,
        "120",
        &["samples/c1.md", "compute", "--write", "--show-output"],
    );
    let table = raw
        .find("preheating: ")
        .unwrap_or_else(|| panic!("{raw:?}"));
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


class Preheating(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=self._g, depends_on=["row.R"]),
                "N": sk.Property(compute_column=self._n, depends_on=["preheating.R"]),
            },
            index="T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
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
            "samples/c1.md",
            "compute",
            "-p",
            "preheating.G",
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
    project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
        self.diameter = sk.Property(unit="g")
        self.first = sk.Property(compute_value=self._first)
        self.second = sk.Property(compute_value=self._second)
        self.set_dependencies("first", depends_on=["diameter"])
        self.set_dependencies("second", depends_on=["first"])

    def _first(self):
        print("first done", flush=True)
        return self.diameter.value * 2

    def _second(self):
        os.kill(os.getppid(), signal.SIGTERM)
        time.sleep(4)
        return self.first.value + 1
"#;

#[test]
fn an_interrupted_run_names_its_log() {
    let scratch = Scratch::new("interrupted-log");
    project(&scratch, PRINTS_THEN_KILLS);
    let output = run(&scratch, &["samples/c1.md", "compute", "--write"]);
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
        self.diameter = sk.Property(unit="g")
        self.t = sk.Table(
            {
                "T": sk.Property(),
                "x": sk.Property(),
                "y": sk.Property(compute=self._y, depends_on=["row.x"]),
            },
            index="T",
            rows=[{"T": 20, "x": 1.0}, {"T": 30, "x": 3.0}],
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
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    let line = cell_line(&text, "- T: 20", "y").to_string();
    let edited_line = line.replacen("v: 2.0", "v: 77.0", 1);
    assert_ne!(edited_line, line, "{text}");
    let edited = with_cell(&text, "- T: 20", "y", edited_line.trim_start_matches("y: "));
    scratch.write("project/samples/c1.md", &edited);
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&status).contains("edited — typed over its formula"),
        "{}",
        out(&status)
    );
    let kept = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let after = scratch.read("project/samples/c1.md");
    assert!(
        cell_line(&after, "- T: 20", "y").contains("77.0"),
        "{after}"
    );
    let forced = run(
        &scratch,
        &["samples/c1.md", "compute", "--force", "--write"],
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
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &with_cell(&text, "- T: 20", "y", "5.0"),
    );
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&status).contains("record missing — --force gives it back to its formula (1 row)"),
        "{}",
        out(&status)
    );
    let kept = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let after = scratch.read("project/samples/c1.md");
    assert!(cell_line(&after, "- T: 20", "y").contains("5.0"), "{after}");
    let listed = run(&scratch, &["samples/c1.md", "compute", "--force"]);
    assert!(
        out(&listed)
            .lines()
            .any(|line| line.contains("t.y") && line.contains("edited")),
        "{}",
        out(&listed)
    );
    let forced = run(
        &scratch,
        &["samples/c1.md", "compute", "--force", "--write"],
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
    billet(&scratch, "c1", 13.0, 5.0);
    let output = run(&scratch, &["samples", "compute"]);
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
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=self._ductility, depends_on=["diameter", "volume"])

    def _ductility(self):
        sys.stdout.buffer.write(b"through the buffer\n")
        os.write(2, "caf\xe9 in Latin-1\n".encode("latin-1"))
        os.write(2, b"and a line after it\n")
        return self.diameter.value / self.volume.value
"#,
    );
    billet(&scratch, "c1", 13.0, 5.0);
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.read("project/samples/c1.md").contains("ductility:"));
    assert!(!err(&output).contains("Broken pipe"), "{}", err(&output));
}

#[test]
fn two_samples_of_one_file_name_keep_two_failure_logs() {
    // Logs were named after the file alone: `explain a/S1.md` showed the
    // traceback `b/S1.md` had left.
    let scratch = Scratch::new("two-logs");
    project(&scratch, BILLET);
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    for (directory, volume) in [("a", 0.0), ("b", 0.0)] {
        scratch.write(
            &format!("project/samples/{directory}/s1.md"),
            &format!(
                "---\nschema_version: 1\nname: {directory}-s1\nproperties:\n  diameter: {{v: 13.0, unit: g}}\n  \
                 volume: {{v: {volume:?}, unit: L}}\n---\n"
            ),
        );
    }
    let failed = run(&scratch, &["samples", "compute", "--write"]);
    assert_ne!(code(&failed), 0, "{}", err(&failed));
    assert!(scratch.exists("project/.samplekit/failures/samples/a/s1.log"));
    assert!(scratch.exists("project/.samplekit/failures/samples/b/s1.log"));
    let explained = run(&scratch, &["samples/a/s1.md", "explain", "ductility"]);
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
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = scratch
        .read("project/samples/c1.md")
        .replace("v: 18.42", "v: 18.24");
    scratch.write("project/samples/c1.md", &text);
    let second = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let said = out(&second);
    assert!(!said.contains("unchanged"), "{said}");
    // With the digits that tell the two apart, never every one, and rounded
    // up as an uncertainty is: 0.01842 is 0.0185.
    assert!(said.contains("0.0185") && said.contains("0.0183"), "{said}");
}

#[test]
fn a_formula_over_absent_inputs_waits_and_writes_nothing() {
    // A sample whose volume nobody entered — ductility is not run, and is no
    // failure; roughness waits behind it; the run says what to fill in.
    let scratch = Scratch::new("waits");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c9.md",
        "---\nschema_version: 1\nname: C9\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let output = run(&scratch, &["samples/c9.md", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let report = out(&output);
    assert!(
        report.contains("ductility") && report.contains("waits for volume"),
        "{report}"
    );
    assert!(
        report.contains("roughness") && report.contains("waits for ductility"),
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
    let root = project(&scratch, BILLET);
    let output = run(&scratch, &["samples", "compute", "--write"]);
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
            "samplekit samples compute --write",
            "the project as SampleKit first kept it"
        ]
    );
}

#[test]
fn the_gate_can_accept_overrides_and_values_waiting() {
    // An override passes with --accept; a value waiting fails nothing, and
    // is no word --accept takes; a stale one never passes.
    let scratch = Scratch::new("gate-accept");
    project(&scratch, BILLET);
    let written = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = scratch.read("project/samples/c1.md");
    // An override of a value nothing reads: roughness.
    let overridden = text.replacen("v: 0.25", "v: 0.3", 1);
    assert_ne!(overridden, text, "{text}");
    scratch.write("project/samples/c1.md", &overridden);
    let strict = run(&scratch, &["samples", "status", "--exit-code"]);
    assert_eq!(code(&strict), 2, "{}", out(&strict));
    let accepting = run(
        &scratch,
        &["samples", "status", "--exit-code", "--accept", "edited"],
    );
    assert_eq!(
        code(&accepting),
        0,
        "{}{}",
        out(&accepting),
        err(&accepting)
    );
    // A new diameter makes the ductility stale, which no --accept passes.
    let set = run(
        &scratch,
        &["samples/c2.md", "set", "diameter=11", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let failing = run(
        &scratch,
        &["samples", "status", "--exit-code", "--accept", "edited"],
    );
    assert_eq!(code(&failing), 2, "{}", out(&failing));
    assert!(out(&failing).contains("outdated"), "{}", out(&failing));
}

#[test]
fn a_formula_reading_a_value_not_applicable_gives_it_too() {
    // Ductility reads diameter; diameter not applicable, ductility is, and roughness after
    // it, without running — and nothing is left to compute.
    let scratch = Scratch::new("not-applicable-compute");
    project(&scratch, BILLET);
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=n/a", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let table = out(&run(
        &scratch,
        &["samples", "-c", "name,ductility,roughness"],
    ));
    let c1 = table.lines().find(|line| line.starts_with("C1")).unwrap();
    assert_eq!(c1.matches("n/a").count(), 2, "{table}");
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(!status.contains("C1"), "{status}");
}

/// Readings a model takes the mean of, and a value computed from them.
const HARDNESSES: &str = r#"import samplekit as sk


class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.notch = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.modulus = sk.Property(compute_value=lambda: (self.radius.value - self.notch.value) * 131.25)
        self.set_dependencies("modulus", depends_on=["radius", "notch"])
"#;

#[test]
fn the_plan_counts_only_samples_with_something_to_compute() {
    // `5 values in 3 samples would be computed, 2 wait` computed 5 values in
    // 2 samples: the third only waited.
    let scratch = Scratch::new("plan-counts");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c9.md",
        "---\nschema_version: 1\nname: C9\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let plan = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&plan), 0, "{}", err(&plan));
    assert!(
        out(&plan).contains(
            "4 values in 2 samples would be computed, 2 wait for an input nobody entered"
        ),
        "{}",
        out(&plan)
    );
    // Nothing but what waits: said as nothing, not as *0 values in 1 sample*.
    let alone = run(&scratch, &["samples/c9.md", "compute"]);
    assert!(
        out(&alone).contains("nothing would be computed: 2 wait for an input nobody entered"),
        "{}",
        out(&alone)
    );
    assert!(!out(&alone).contains("0 values"), "{}", out(&alone));
}

#[test]
fn a_value_waiting_for_itself_is_said_one_way() {
    // The plan said `notch waits for notch`, the run *waits for its value, which
    // nobody entered*: the plan now says the run's words.
    let scratch = Scratch::new("waits-itself");
    project(&scratch, HARDNESSES);
    scratch.write(
        "project/samples/b1.md",
        "---\nschema_version: 1\nname: B1\nproperties:\n  radius: {readings: [1.05, 1.052]}\n---\n",
    );
    for arguments in [
        vec!["samples/b1.md", "compute", "--rerun"],
        vec!["samples/b1.md", "compute", "--rerun", "--write"],
    ] {
        let output = run(&scratch, &arguments);
        let said = out(&output);
        let notch = said
            .lines()
            .find(|line| {
                let words: Vec<&str> = line.split_whitespace().collect();
                words.contains(&"B1") && words.contains(&"notch")
            })
            .unwrap_or_else(|| panic!("{arguments:?}: {said}"));
        assert!(
            notch.contains("waits for its value, which nobody entered"),
            "{arguments:?}: {said}"
        );
    }
}

#[test]
fn a_value_whose_input_was_emptied_waits_for_it_everywhere() {
    // `status` said *stale — volume*, `compute` *waits for volume* and
    // `explain` *volume — current … stale — volume*, of one value.
    let scratch = Scratch::new("emptied-input");
    project(&scratch, BILLET);
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let cleared = run(&scratch, &["samples/c1.md", "set", "volume=", "--write"]);
    assert_eq!(code(&cleared), 0, "{}", err(&cleared));
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&status).contains("waits for volume"),
        "{}",
        out(&status)
    );
    assert!(!out(&status).contains("outdated"), "{}", out(&status));
    let plan = run(&scratch, &["samples/c1.md", "compute"]);
    assert!(out(&plan).contains("waits for volume"), "{}", out(&plan));
    let explained = run(&scratch, &["samples/c1.md", "explain", "ductility"]);
    let said = out(&explained);
    assert!(said.contains("nobody entered it"), "{said}");
    assert!(said.contains("this value waits for volume"), "{said}");
    assert!(!said.contains("outdated"), "{said}");
    // What reads the waiting value waits in turn.
    let roughness = run(&scratch, &["samples/c1.md", "explain", "roughness"]);
    assert!(
        out(&roughness).contains("this value waits for ductility"),
        "{}",
        out(&roughness)
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
    let output = run(&scratch, &["samples", "status"]);
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
        let output = run(&scratch, &["samples/t1.md", command]);
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
    let output = answering(&scratch, &["samples/c1.md", "compute"], "y\nn\n");
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
    // A value is shown at the precision the project declares, and as a table
    // shows it where it declares none: every digit,
    // `3.3333333333333335`, said the noise a computation leaves. A column's
    // cells nobody wrote are said to be empty, where `2 cells` read as two
    // values replaced.
    let scratch = Scratch::new("rehearsal-digits");
    project(&scratch, SHAPES);
    // Its rows measured and written, their computed cells not yet.
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: {v: 10.0, unit: g}\n  \
         volume: {v: 3.0, unit: L}\ntables:\n  preheating:\n    index: T\n    columns:\n      \
         T: {}\n      R: {}\n      G: {}\n      N: {}\n    rows:\n      - {T: 10, R: 2.0}\n      \
         - {T: 20, R: 4.0}\n---\n",
    );
    let output = answering(&scratch, &["samples/c1.md", "compute"], "y\nn\n");
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("3.33333"), "{text}");
    assert!(!text.contains("3.3333333333333335"), "{text}");
    assert!(text.contains("2 empty cells"), "{text}");
    assert!(!text.contains("2 cells"), "{text}");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
         [property.ductility]\nprecision = \".2f\"\n",
    );
    let output = answering(&scratch, &["samples/c1.md", "compute"], "y\nn\n");
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("3.33"), "{text}");
    assert!(!text.contains("3.3333"), "{text}");
}

// ------------------------------------------------------ the description

/// The shapes, with a statistic of readings, an attribute and a figure.
fn described_model() -> String {
    SHAPES.replace(
        "        self.set_dependencies(\"ductility\"",
        "        self.doped = sk.Property(value=sk.stats.mean, \
         uncertainty=sk.stats.standard_error, unit=\"g\")\n        \
         self.machinist = \"jo\"\n        self.set_dependencies(\"ductility\"",
    ) + "\n    @sk.figure\n    def curve(self, ax):\n        ax.plot([1], [2])\n"
}

fn description_of(scratch: &Scratch) -> runtime::ModelDescription {
    serde_json::from_str(&scratch.read("project/.samplekit/model.json")).unwrap()
}

#[test]
fn the_model_is_described_as_it_is_imported() {
    let scratch = Scratch::new("described");
    project(&scratch, &described_model());
    let listed = run(&scratch, &["samples", "compute"]);
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
    let doped = &described.properties["doped"];
    assert_eq!(doped.unit.as_deref(), Some("g"));
    assert_eq!(
        doped.value,
        runtime::Origin::Statistic {
            statistic: "mean".to_string()
        }
    );
    assert_eq!(
        doped.uncertainty,
        Some(runtime::Origin::Statistic {
            statistic: "standard_error".to_string()
        })
    );
    // A formula's, with what it reads and its digest.
    let ductility = &described.properties["ductility"];
    assert_eq!(
        ductility.value,
        runtime::Origin::Formula {
            reads: vec!["diameter".to_string(), "volume".to_string()]
        }
    );
    assert!(
        ductility
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
    let preheating = &described.tables["preheating"];
    assert_eq!(preheating.index, ["T"]);
    // The rows it gives itself, each cell as a file would hold it; a cell no
    // formula has filled holds nothing.
    assert_eq!(
        serde_json::to_value(&preheating.rows).unwrap(),
        serde_json::json!([
            {"T": {"value": 10}, "R": {"value": 2.0}},
            {"T": {"value": 20}, "R": {"value": 4.0}},
        ])
    );
    let columns: Vec<&str> = preheating
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(columns, ["T", "R", "G", "N"]);
    assert_eq!(
        preheating.columns[2].value,
        runtime::Origin::Rows {
            reads: vec!["row.R".to_string()],
            fills: vec!["G".to_string()]
        }
    );
    assert!(matches!(
        preheating.columns[3].value,
        runtime::Origin::Columns { .. }
    ));
    // An attribute, and a figure.
    assert_eq!(described.attributes["machinist"], serde_json::json!("jo"));
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
    let again = run(&scratch, &["samples", "compute"]);
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
            "diameter": {"unit": "g", "value": {"from": "entered"}, "uncertainty": null,
                     "reads": [], "formula": null, "default": null}
        },
        "tables": {
            "preheating": {"title": null, "index": ["T"], "rows": [], "columns": [
                {"name": "T", "unit": "degC", "value": {"from": "entered"},
                 "uncertainty": null, "formula": null}
            ]}
        },
        "attributes": {},
        "figures": []
    });
    let mut with = without.clone();
    with["properties"]["diameter"]["symbol"] = serde_json::json!("m");
    with["tables"]["preheating"]["columns"][0]["symbol"] = serde_json::json!("T");
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
    project(&scratch, BILLET);
    let listed = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let first = description_of(&scratch);
    assert!(!first.properties.contains_key("wear"));
    scratch.write(
        "project/model.py",
        &BILLET.replace(
            "        self.set_dependencies(\"ductility\"",
            "        self.wear = sk.Property(unit=\"mL\")\n        \
             self.set_dependencies(\"ductility\"",
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
    assert_eq!(second.properties["wear"].unit.as_deref(), Some("mL"));
    assert_ne!(second.model.digest, first.model.digest);
    assert!(runtime::current_description(&loaded(&scratch)).is_some());
    let current = status(&scratch);
    assert_eq!(code(&current), 0, "{}", err(&current));
    assert_eq!(imports(), before + 1);
}

#[test]
fn status_with_a_current_description_starts_no_python() {
    let scratch = Scratch::new("described-status");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  diameter: {v: 9.0, unit: g}\n---\n",
    );
    let listed = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(scratch.exists("project/.samplekit/model.json"));
    // The interpreter now records being started, and runs nothing.
    fake_python(&scratch, "touch \"$(dirname \"$0\")/started\"\nexit 1\n");
    let said = status(&scratch);
    assert_eq!(code(&said), 0, "{}", err(&said));
    assert_eq!(said_of(&said, "C1", "ductility"), "never computed");
    assert_eq!(said_of(&said, "C3", "ductility"), "waits for volume");
    for arguments in [
        vec!["samples", "status", "--exit-code"],
        vec!["samples", "list", "figures"],
        vec!["samples", "-f", "state == never_computed"],
    ] {
        run(&scratch, &arguments);
    }
    assert!(!scratch.exists("project/started"));
    // The probe works: stale, the description is written by the interpreter.
    scratch.write("project/model.py", &format!("{BILLET}\n# edited\n"));
    run(&scratch, &["samples", "status"]);
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
    project(&scratch, BILLET);
    let listed = run(&scratch, &["samples", "compute"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(scratch.exists("project/.samplekit/model.json"));
    scratch.write(
        "project/model.py",
        &format!("raise RuntimeError(\"broken\")\n{BILLET}"),
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
        said(&stale, "ductility", "never computed"),
        0,
        "{}",
        out(&stale)
    );
    assert!(!scratch.exists("project/.samplekit/model.json"));
}

#[test]
fn the_description_is_not_kept_in_the_history() {
    let scratch = Scratch::new("described-history");
    project(&scratch, BILLET);
    let written = run(&scratch, &["samples", "compute", "--write"]);
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
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.doped = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error, unit="g")
        self.age = sk.Property(unit="mL", compute_uncertainty=lambda: 0.01, depends_on=[])
        self.ductility = sk.Property(compute_value=self._ductility)
        self.roughness = sk.Property(compute_value=lambda: 1 - self.ductility.value / 4.0)
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=lambda row: row.R.value * 2, depends_on=["row.R", "ductility"]),
            },
            index="T",
        )
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.set_dependencies("roughness", depends_on=["ductility"])

    def _ductility(self):
        return self.diameter.value / self.volume.value
"#;

#[test]
fn the_description_plans_as_the_worker_does() {
    let scratch = Scratch::new("described-plan");
    project(&scratch, EVERY_STATE);
    let full = "---\nschema_version: 1\nname: {N}\nproperties:\n  diameter: {v: 12.0, unit: g}\n  \
                volume: {v: 4.0, unit: L}\n  age: {v: 2.0, unit: mL}\n  \
                doped: {readings: [1.0, 2.0, 3.0], unit: g}\ntables:\n  preheating:\n    \
                index: T\n    columns:\n      T: {}\n      R: {}\n    rows:\n      \
                - {T: 10, R: 2.0}\n---\n";
    // Never computed, and computed below.
    scratch.write("project/samples/c1.md", &full.replace("{N}", "C1"));
    scratch.write("project/samples/c2.md", &full.replace("{N}", "C2"));
    // Waiting: no volume, no age.
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  diameter: {v: 9.0, unit: g}\n---\n",
    );
    // A computed value written by hand, with no record.
    scratch.write(
        "project/samples/c4.md",
        &full
            .replace("{N}", "C4")
            .replace("  age:", "  ductility: {v: 3.0}\n  age:"),
    );
    let written = run(&scratch, &["samples/c2.md", "compute", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    // Outdated: an input edited since.
    let c2 = scratch.read("project/samples/c2.md");
    scratch.write(
        "project/samples/c2.md",
        &c2.replace("diameter: {v: 12.0", "diameter: {v: 16.0"),
    );
    let compare = |scratch: &Scratch| {
        let config = loaded(scratch);
        let description = runtime::describe(&config, &scratch.at("project")).unwrap();
        let template = runtime::template_of(&config).unwrap();
        let python = runtime::interpreter_for(&config, &scratch.at("project")).unwrap();
        let mut worker = Worker::start(&python).unwrap();
        for name in ["c1", "c2", "c3", "c4"] {
            let path = scratch.at(&format!("project/samples/{name}.md"));
            let recorded = traced(&loaded(scratch), &path);
            let request = Request {
                sample: path.clone(),
                template: template.clone(),
                names: Vec::new(),
                rerun: false,
                force: true,
                timeout: None,
                refused: Vec::new(),
                overran: Vec::new(),
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
            "self.diameter.value / self.volume.value",
            "self.diameter.value / self.volume.value * 1.0",
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
    let head = "---\nschema_version: 1\nname: {N}\nproperties:\n  diameter: {v: 10.0, unit: g}\n  \
                volume: {v: 2.0, unit: L}\n";
    // No table in the file: the model's two rows, their cells never computed.
    scratch.write(
        "project/samples/c1.md",
        &format!("{head}---\n").replace("{N}", "C1"),
    );
    // One of the model's rows, its reading changed, and a row of its own.
    let partial = format!(
        "{head}tables:\n  preheating:\n    index: T\n    columns:\n      T: {{}}\n      \
         R: {{}}\n      G: {{}}\n      N: {{}}\n    rows:\n      - {{T: 10, R: 5.0}}\n      \
         - {{T: 30, R: 6.0}}\n---\n"
    );
    scratch.write("project/samples/c2.md", &partial.replace("{N}", "C2"));
    // Computed whole, then a reading the rows' formula reads edited.
    scratch.write("project/samples/c3.md", &partial.replace("{N}", "C3"));
    let written = run(&scratch, &["samples/c3.md", "compute", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let c3 = scratch.read("project/samples/c3.md");
    assert!(c3.contains("T: 20"), "{c3}");
    scratch.write("project/samples/c3.md", &c3.replacen("R: 4.0", "R: 8.0", 1));
    let config = loaded(&scratch);
    let description = runtime::describe(&config, &scratch.at("project")).unwrap();
    assert_eq!(description.tables["preheating"].rows.len(), 2);
    let template = runtime::template_of(&config).unwrap();
    let python = runtime::interpreter_for(&config, &scratch.at("project")).unwrap();
    let mut worker = Worker::start(&python).unwrap();
    for name in ["c1", "c2", "c3"] {
        let path = scratch.at(&format!("project/samples/{name}.md"));
        let recorded = traced(&config, &path);
        let request = Request {
            sample: path.clone(),
            template: template.clone(),
            names: Vec::new(),
            rerun: false,
            force: true,
            timeout: None,
            refused: Vec::new(),
            overran: Vec::new(),
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
                    .any(|planned| planned.value.starts_with("preheating.")),
                "{by_description:?}"
            );
        }
    }
}

/// What the project's trace knows of the formulas that computed the sample at
/// `path`, as `status` and `compute` read it.
fn traced(
    config: &samplekit::config::project_config::ProjectConfig,
    path: &std::path::Path,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let trace = samplekit::config::version_control::trace(config).unwrap();
    let (sample, _) = samplekit::format::document::load_sample(path).unwrap();
    samplekit::collection::computation::recorded_for(Some(&trace), &sample)
}

// ------------------------------------------------ the trace of the formulas

/// The binary in `directory`, with nobody at a terminal.
fn run_in(scratch: &Scratch, directory: &Path, arguments: &[&str]) -> Output {
    command(scratch, directory)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

/// `from` copied whole to `to`, the history's files included, as a clone or
/// a copy of the project's folder carries them.
fn copied(from: &Path, to: &Path) {
    let done = Command::new("cp")
        .arg("-a")
        .arg(from)
        .arg(to)
        .status()
        .unwrap();
    assert!(done.success());
}

/// The BILLET model with its `ductility` formula edited.
fn billet_edited() -> String {
    BILLET.replace(
        "return self.diameter.value / self.volume.value",
        "return 2 * self.diameter.value / self.volume.value",
    )
}

/// The snapshots of the project's history, newest first, by message.
fn snapshots(root: &Path) -> Vec<String> {
    let log = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--all", "--format=%H %s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&log.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_file_of_fields_is_not_made_stale_by_a_model_it_does_not_read() {
    // A table, an export or a declared figure runs no code of the model: a
    // formula edited, and the values it computes written again, leave a file
    // of other fields what its command makes, where `explain` said *not
    // current* for the model's file.
    let scratch = Scratch::new("stale-by-model");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let table = run(
        &scratch,
        &[
            "samples",
            "-c",
            "name,diameter",
            "--csv",
            "-o",
            "diameters.csv",
            "--write",
        ],
    );
    assert_eq!(code(&table), 0, "{}", err(&table));
    scratch.write("project/model.py", &billet_edited());
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let explained = run(&scratch, &["diameters.csv", "explain"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("it is up to date"),
        "{}",
        out(&explained)
    );
    assert!(!out(&explained).contains("model.py"), "{}", out(&explained));
}

#[test]
fn a_restored_value_carries_back_the_formula_that_computed_it() {
    // The trace is of what a value records, not of the file: a value restored
    // from a snapshot is judged by the formula that computed it then.
    let scratch = Scratch::new("restored-formula");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let computed = snapshots(&root)[0].split(' ').next().unwrap().to_string();
    scratch.write("project/model.py", &billet_edited());
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let current = status(&scratch);
    assert!(
        out(&current).contains("every computed value in 2 samples is current"),
        "{}",
        out(&current)
    );
    let restored = run(
        &scratch,
        &["samples/c1.md", "restore", "--at", &computed, "--write"],
    );
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    let after = status(&scratch);
    assert_eq!(
        said(&after, "ductility", "formula changed"),
        1,
        "{}",
        out(&after)
    );
    assert!(out(&after).contains("C1"), "{}", out(&after));
    assert!(!out(&after).contains("C2"), "{}", out(&after));
}

#[test]
fn a_value_computed_on_another_computer_is_judged_by_its_own_record() {
    // A colleague recomputes after a formula changed: their values are never
    // said *formula changed* here, whatever this computer computed before —
    // not known while their history is not here, current once it is.
    let scratch = Scratch::new("colleague-formula");
    let mine = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let theirs = scratch.at("theirs");
    copied(&mine, &theirs);
    scratch.write("theirs/model.py", &billet_edited());
    let recomputed = run_in(&scratch, &theirs, &["samples", "compute", "--write"]);
    assert_eq!(code(&recomputed), 0, "{}", err(&recomputed));
    // Their samples and model arrive, their history does not.
    for file in ["model.py", "samples/c1.md", "samples/c2.md"] {
        fs::copy(theirs.join(file), mine.join(file)).unwrap();
    }
    let pulled = status(&scratch);
    assert_eq!(
        said(&pulled, "ductility", "formula changed"),
        0,
        "{}",
        out(&pulled)
    );
    assert!(
        err(&pulled).contains("is not known here"),
        "{}",
        err(&pulled)
    );
    // Their history arrives too.
    fs::remove_dir_all(mine.join(".samplekit/history")).unwrap();
    copied(
        &theirs.join(".samplekit/history"),
        &mine.join(".samplekit/history"),
    );
    let joined = status(&scratch);
    assert!(
        out(&joined).contains("every computed value in 2 samples is current"),
        "{}",
        out(&joined)
    );
    assert!(!err(&joined).contains("not known"), "{}", err(&joined));
}

#[test]
fn a_moved_project_keeps_the_trace_of_its_formulas() {
    let scratch = Scratch::new("moved-trace");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let moved = scratch.at("elsewhere");
    fs::rename(scratch.at("project"), &moved).unwrap();
    scratch.write("elsewhere/model.py", &billet_edited());
    let after = run_in(&scratch, &moved, &["samples", "status"]);
    assert_eq!(
        said(&after, "ductility", "formula changed"),
        2,
        "{}",
        out(&after)
    );
}

#[test]
fn computations_at_once_keep_every_formula() {
    // Four projects computed at the same time, on one machine: each project's
    // trace holds what its own run computed, and a formula edited afterwards
    // is said of every value it computed.
    let scratches: Vec<Scratch> = (0..4)
        .map(|n| Scratch::new(&format!("at-once-{n}")))
        .collect();
    for scratch in &scratches {
        project(scratch, BILLET);
    }
    let children: Vec<_> = scratches
        .iter()
        .map(|scratch| {
            command(scratch, &scratch.at("project"))
                .env("SAMPLEKIT_STATE_DIR", scratches[0].at("state"))
                .args(["samples", "compute", "--write"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", err(&output));
    }
    for scratch in &scratches {
        scratch.write("project/model.py", &billet_edited());
        let stale = status(scratch);
        assert_eq!(
            said(&stale, "ductility", "formula changed"),
            2,
            "{}",
            out(&stale)
        );
    }
}

#[test]
fn a_fresh_clone_with_its_history_sees_a_changed_formula() {
    // A machine that never computed the project — a colleague's, a CI's —
    // reads the trace from the history the clone carries.
    let scratch = Scratch::new("fresh-clone");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let clone = scratch.at("clone");
    copied(&root, &clone);
    scratch.write("clone/model.py", &billet_edited());
    let gate = command(&scratch, &clone)
        .env("SAMPLEKIT_STATE_DIR", scratch.at("another-machine"))
        .args(["samples", "status", "--exit-code"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&gate), 2, "{}{}", out(&gate), err(&gate));
    assert_eq!(
        said(&gate, "ductility", "formula changed"),
        2,
        "{}",
        out(&gate)
    );
}

#[test]
fn a_change_of_the_model_is_a_snapshot_of_its_own() {
    // Taken before the computation that follows it, naming the formula whose
    // digest changed: the model that computed what can be read, and computed
    // again, from its snapshot.
    let scratch = Scratch::new("model-snapshot");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let messages: Vec<String> = snapshots(&root)
        .iter()
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    assert_eq!(
        messages[..2],
        [
            "samplekit samples compute --write".to_string(),
            "the model changed: ductility".to_string()
        ],
        "{messages:?}"
    );
}

#[test]
fn a_model_reading_its_own_folder_moves_with_its_project() {
    // BILLET keeps `os.path.dirname(__file__)` in a constant its formula reads: a
    // path under the model's folder is taken from that folder, so that the
    // project moved, or cloned elsewhere, changes no formula.
    let scratch = Scratch::new("moved-folder");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let moved = scratch.at("elsewhere");
    fs::rename(scratch.at("project"), &moved).unwrap();
    let after = run_in(&scratch, &moved, &["samples", "status"]);
    assert!(
        out(&after).contains("every computed value in 2 samples is current"),
        "{}",
        out(&after)
    );
    assert!(!err(&after).contains("not known"), "{}", err(&after));
}

#[test]
fn a_model_in_a_folder_is_found_past_another_model_module() {
    // `model/billet.py` was imported as `model.billet`: a `model.py` Python
    // finds first, installed or another project's, made it "'model' is not
    // a package", and nothing was computed.
    let scratch = Scratch::new("model-folder-shadowed");
    project(&scratch, BILLET);
    fs::rename(scratch.at("project/model.py"), {
        fs::create_dir_all(scratch.at("project/model")).unwrap();
        scratch.at("project/model/billet.py")
    })
    .unwrap();
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model/billet.py\"\npython = \".venv\"\n",
    );
    scratch.write("elsewhere/model.py", "shadow = True\n");
    let computed = command(&scratch, &scratch.at("project"))
        .env("PYTHONPATH", scratch.at("elsewhere"))
        .args(["samples", "compute", "--write"])
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(code(&computed), 0, "{}{}", out(&computed), err(&computed));
    assert!(
        !err(&computed).contains("is not a package"),
        "{}",
        err(&computed)
    );
    let after = run(&scratch, &["samples", "status"]);
    assert!(
        out(&after).contains("every computed value in 2 samples is current"),
        "{}",
        out(&after)
    );
}

#[test]
fn a_first_trace_takes_the_values_held_as_its_baseline() {
    // A project computed where no trace was kept — the demo as it is shipped,
    // a project moved in: its first computation takes what the files hold as
    // what the formulas as they are computed. Nothing is said not known, and
    // a formula edited afterwards is.
    let scratch = Scratch::new("first-trace");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    fs::remove_dir_all(root.join(".samplekit")).unwrap();
    let _ = fs::remove_dir_all(scratch.at("state"));
    let second = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&second), 0, "{}", err(&second));
    let quiet = status(&scratch);
    assert!(!err(&quiet).contains("not known"), "{}", err(&quiet));
    assert!(
        out(&quiet).contains("every computed value in 2 samples is current"),
        "{}",
        out(&quiet)
    );
    scratch.write("project/model.py", &billet_edited());
    let stale = status(&scratch);
    assert_eq!(
        said(&stale, "ductility", "formula changed"),
        2,
        "{}",
        out(&stale)
    );
}

// -------------------------------------------- readings and their statistic

/// A hardness read three times, its statistic declared, and a value read from it.
const HARDNESS: &str = r#"import samplekit as sk


class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.double = sk.Property(compute_value=lambda: 2 * self.radius.value)
        self.set_dependencies("double", depends_on=["radius"])
"#;

/// The HARDNESS project, computed: one sample, `radius` read three times.
fn hardness(scratch: &Scratch) {
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    scratch.write("project/model.py", HARDNESS);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  radius: {readings: [1.063, 1.065, 1.063]}\n---\n",
    );
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    let first = run(scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
}

#[test]
fn an_emptied_value_is_given_back_to_its_statistic() {
    // `set radius=` over a value typed beside the readings gives the quantity
    // back to its statistic, without computing: no empty override is left,
    // and the next computation takes the statistic again.
    let scratch = Scratch::new("emptied-statistic");
    hardness(&scratch);
    let typed = run(
        &scratch,
        &["samples/c1.md", "set", "radius=1.070", "--write"],
    );
    assert_eq!(code(&typed), 0, "{}", err(&typed));
    assert_eq!(said(&status(&scratch), "radius", "edited"), 1);
    let emptied = run(&scratch, &["samples/c1.md", "set", "radius=", "--write"]);
    assert_eq!(code(&emptied), 0, "{}", err(&emptied));
    let file = scratch.read("project/samples/c1.md");
    assert!(
        !file.contains("edited") && !file.contains("v: 1.07"),
        "{file}"
    );
    assert_eq!(said(&status(&scratch), "radius", "edited"), 0);
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&status(&scratch)).contains("every computed value in 1 sample is current"),
        "{}",
        out(&status(&scratch))
    );
}

#[test]
fn an_uncertainty_typed_over_its_statistic_is_an_override() {
    // Every surface says the same override: `status` and `explain` name it,
    // what reads it is computed again with it, and `--force` gives it back.
    let scratch = Scratch::new("typed-uncertainty");
    hardness(&scratch);
    let typed = run(
        &scratch,
        &["samples/c1.md", "set", "radius.u=0.001", "--write"],
    );
    assert_eq!(code(&typed), 0, "{}", err(&typed));
    let stale = status(&scratch);
    assert_eq!(said(&stale, "radius", "edited"), 1, "{}", out(&stale));
    assert_eq!(said(&stale, "double", "outdated"), 1, "{}", out(&stale));
    let explained = run(&scratch, &["samples/c1.md", "explain", "radius"]);
    assert!(
        out(&explained).contains("typed over its formula"),
        "{}",
        out(&explained)
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let after = status(&scratch);
    assert_eq!(said(&after, "double", "outdated"), 0, "{}", out(&after));
    assert_eq!(said(&after, "radius", "edited"), 1, "{}", out(&after));
    let forced = run(
        &scratch,
        &["samples", "compute", "-p", "radius", "--force", "--write"],
    );
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    assert_eq!(said(&status(&scratch), "radius", "edited"), 0);
}

// ------------------------------------------------- one state, every surface

#[test]
fn a_changed_formula_is_outdated_on_every_surface() {
    // `status` said *formula changed*; its JSON said never computed, the
    // state filter selected nothing, the export's state column and `explain`
    // said current.
    let scratch = Scratch::new("changed-everywhere");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    // The description written again, as the first command after an edit does.
    assert_eq!(said(&status(&scratch), "ductility", "formula changed"), 2);
    let json = run(&scratch, &["samples", "status", "--json"]);
    let states: Vec<serde_json::Value> = serde_json::from_str(&out(&json)).unwrap();
    assert!(
        states
            .iter()
            .any(|state| state["value"] == "ductility" && state["state"] == "outdated"),
        "{}",
        out(&json)
    );
    let filtered = run(&scratch, &["samples", "-f", "state == outdated"]);
    assert!(out(&filtered).contains("c1.md"), "{}", out(&filtered));
    let csv = run(
        &scratch,
        &["samples", "-c", "name,ductility", "--status", "--csv"],
    );
    assert!(out(&csv).contains("outdated: ductility"), "{}", out(&csv));
    let explained = run(&scratch, &["samples/c1.md", "explain", "ductility"]);
    assert!(
        out(&explained).contains("outdated — formula changed"),
        "{}",
        out(&explained)
    );
}

#[test]
fn a_value_waiting_is_said_waiting_by_the_state_column() {
    let scratch = Scratch::new("waiting-column");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let csv = run(
        &scratch,
        &["samples/c1.md", "-c", "name,ductility", "--status", "--csv"],
    );
    assert!(out(&csv).contains("waiting: ductility"), "{}", out(&csv));
    assert!(!out(&csv).contains(",current"), "{}", out(&csv));
}

#[test]
fn explain_names_what_an_owed_value_is_computed_from() {
    // Never computed: what the model computes it from, read from its
    // description, never from another sample's file; waiting: the input
    // nobody entered, followed through what the model computes.
    let scratch = Scratch::new("owed-explained");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let described = status(&scratch);
    assert_eq!(code(&described), 0, "{}", err(&described));
    let owed = run(&scratch, &["samples/c2.md", "explain", "ductility"]);
    assert!(
        out(&owed).contains("the model computes it from diameter, volume")
            && out(&owed).contains("it was never computed"),
        "{}",
        out(&owed)
    );
    let waiting = run(&scratch, &["samples/c1.md", "explain", "roughness"]);
    assert!(
        out(&waiting).contains("it waits for volume, which nobody entered here")
            && !out(&waiting).contains("ductility=<value>"),
        "{}",
        out(&waiting)
    );
}

#[test]
fn status_counts_the_values_waiting_apart() {
    // A value waiting for an input nobody entered is not among the values
    // not current, as their definition says: counted apart in the headline.
    let scratch = Scratch::new("waiting-apart");
    project(&scratch, BILLET);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: {v: 12.0, unit: g}\n---\n",
    );
    let owed = status(&scratch);
    assert!(
        out(&owed).contains("2 values are not current, and 2 wait for an input nobody entered"),
        "{}",
        out(&owed)
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let waiting = status(&scratch);
    assert!(
        out(&waiting)
            .contains("2 values wait for an input nobody entered, and nothing else is not current"),
        "{}",
        out(&waiting)
    );
}

#[test]
fn a_new_sample_is_filled_in_the_workbench() {
    // A sample made by `N` showed none of the values the model declares for a
    // hand to write, nor its tables: `e` and `+` had nothing to act on, and
    // only an editor could give a new piece its hardness.
    use samplekit::tui::model::{Key, Workbench};
    let scratch = Scratch::new("fill-new");
    workbench_state();
    copied(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/metals/05-ageing"),
        &scratch.at("project"),
    );
    // The step names the .venv beside the steps.
    std::os::unix::fs::symlink(venv(), scratch.at(".venv")).unwrap();
    let first = run(&scratch, &["pieces", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/pieces/fresh.md", "---\nschema_version: 1\n---\n");
    let mut workbench = Workbench::open(&scratch.at("project/pieces")).unwrap();
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("fresh"))
        .unwrap();
    workbench.key(Key::Enter);
    let entries = workbench.entries();
    let entry = |field: &str| entries.iter().find(|entry| entry.field == field).cloned();
    assert_eq!(
        entry("radius").map(|radius| radius.state).as_deref(),
        Some("to fill in")
    );
    assert_eq!(
        entry("ageing").map(|table| table.state).as_deref(),
        Some("to fill in")
    );
    // Its first row, from the table's line: the table is made with it.
    let at = entries
        .iter()
        .position(|entry| entry.field == "ageing")
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    // Unfolded as any table, on the empty row: its cells typed, the index
    // last, and the row previewed whole.
    assert!(workbench.on_the_empty_row(), "{:?}", workbench.message);
    // In the typing mode: the second cell, then the index, which keeps the
    // row; out of it, `w` writes what waits.
    workbench.key(Key::Char('e'));
    workbench.key(Key::Right);
    for character in "1.04\n".chars() {
        workbench.key(if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        });
    }
    workbench.key(Key::Left);
    for character in "1\n".chars() {
        workbench.key(if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        });
    }
    for key in [Key::Esc, Key::Char('w'), Key::Char('y')] {
        workbench.key(key);
    }
    let written = fs::read_to_string(scratch.at("project/pieces/fresh.md")).unwrap();
    assert!(
        written.contains("hardness: 1.04"),
        "{written}\n{}",
        workbench.message
    );
    // Back to the sample; and a value the model declares, typed on its line.
    workbench.key(Key::Esc);
    let at = workbench
        .entries()
        .iter()
        .position(|entry| entry.field == "radius")
        .unwrap();
    for _ in 0..40 {
        workbench.key(Key::Up);
    }
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    // Typed in the typing mode, kept, then written out of it.
    for character in "e1.05\n".chars() {
        workbench.key(if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        });
    }
    for key in [Key::Esc, Key::Char('w'), Key::Char('y')] {
        workbench.key(key);
    }
    let written = fs::read_to_string(scratch.at("project/pieces/fresh.md")).unwrap();
    assert!(
        written.contains("radius: 1.05"),
        "{written}\n{}",
        workbench.message
    );
}

#[test]
fn readings_are_refused_for_a_value_the_model_computes() {
    // A sample that never held `modulus` had no record to say a formula gives it:
    // its readings were written, and the model then refused to load the file,
    // stopping the whole project's computation while `validate` said nothing.
    let scratch = Scratch::new("readings-under-formula");
    copied(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/metals/05-ageing"),
        &scratch.at("project"),
    );
    // The step names the .venv beside the steps.
    std::os::unix::fs::symlink(venv(), scratch.at(".venv")).unwrap();
    let first = run(&scratch, &["pieces", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/pieces/fresh.md", "---\nschema_version: 1\n---\n");
    let set = run(
        &scratch,
        &[
            "pieces/fresh.md",
            "set",
            "modulus.readings=6.1,6.2",
            "--write",
        ],
    );
    assert_ne!(code(&set), 0, "{}", out(&set));
    assert!(
        err(&set).contains("modulus is computed by the model"),
        "{}",
        err(&set)
    );
    scratch.write(
        "project/pieces/fresh.md",
        "---\nschema_version: 1\nproperties:\n  modulus: {readings: [6.1, 6.2]}\n---\n",
    );
    let checked = run(&scratch, &["pieces", "validate"]);
    assert_eq!(code(&checked), 2, "{}", out(&checked));
    assert!(
        out(&checked).contains("modulus: the file holds readings for a value the model computes"),
        "{}",
        out(&checked)
    );
}

#[test]
fn a_formula_that_does_not_stop_past_its_timeout_is_stopped() {
    // Asleep, waiting on a file or inside a compiled library, a formula ran on
    // past --timeout for ever: the worker's own stop comes between two lines
    // of Python. Its worker is stopped, and the samples after it computed.
    let scratch = Scratch::new("timeout-stopped");
    let model = BILLET.replace(
        "        return self.diameter.value / self.volume.value",
        "        import time\n        if self.name == \"C1\":\n            time.sleep(60)\n        \
         return self.diameter.value / self.volume.value",
    );
    project(&scratch, &model);
    let began = std::time::Instant::now();
    let computed = run(
        &scratch,
        &["samples", "compute", "--write", "--timeout", "1"],
    );
    assert!(began.elapsed() < std::time::Duration::from_secs(30));
    let said = format!("{}{}", out(&computed), err(&computed));
    assert!(
        said.contains("ran past --timeout 1 s and did not stop"),
        "{said}"
    );
    // Failed and written, and what reads it waits behind it.
    assert!(
        fs::read_to_string(scratch.at("project/samples/c1.md"))
            .unwrap()
            .contains("failed: TimedOut"),
        "{said}"
    );
    assert!(
        fs::read_to_string(scratch.at("project/samples/c2.md"))
            .unwrap()
            .contains("ductility"),
        "{said}"
    );
}

#[test]
fn a_formula_that_gives_no_value_is_said_so() {
    // A `return` forgotten: the value came out as nothing, `compute` said
    // *unchanged* beside a value never there, and `status` *never computed*.
    // It fails, saying what a formula returns where a value does not apply.
    let scratch = Scratch::new("gave-nothing");
    let model = BILLET.replace(
        "return self.diameter.value / self.volume.value",
        "self.diameter.value / self.volume.value",
    );
    project(&scratch, &model);
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    let said = format!("{}{}", out(&computed), err(&computed));
    assert!(said.contains("returned None"), "{said}");
    assert!(said.contains("sk.NA"), "{said}");
    assert!(!said.contains("unchanged"), "{said}");
}

#[test]
fn status_lists_in_the_order_sort_gives() {
    // `-s` and `-r` were documented, and status listed by name whatever they
    // said.
    let scratch = Scratch::new("status-sorted");
    project(&scratch, BILLET);
    let first = |arguments: &[&str]| -> String {
        let output = run(&scratch, arguments);
        let report: serde_json::Value = serde_json::from_str(&out(&output)).unwrap();
        report[0]["sample"].as_str().unwrap().to_string()
    };
    assert_eq!(first(&["samples", "status", "--json"]), "C1");
    assert_eq!(
        first(&["samples", "status", "--json", "-s", "diameter"]),
        "C2"
    );
    assert_eq!(
        first(&["samples", "status", "--json", "-s", "diameter", "-r"]),
        "C1"
    );
}

#[test]
#[cfg(unix)]
fn another_version_of_the_package_is_named() {
    // An environment holding SampleKit 0.2 — importable, without the worker —
    // was said to lack samplekit altogether, its version never named.
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("older-package");
    project(&scratch, BILLET);
    let python = scratch.at("older-python");
    fs::write(
        &python,
        "#!/bin/sh\ncase \"$*\" in\n  *importlib.metadata*) echo 0.2.0 ;;\n  \
         *) echo \"No module named samplekit._worker\" >&2; exit 1 ;;\nesac\n",
    )
    .unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!(
            "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"{}\"\n",
            python.display()
        ),
    );
    let computed = answering(&scratch, &["samples", "compute"], "y\nn\n");
    assert_eq!(code(&computed), 3, "{}", err(&computed));
    assert!(
        err(&computed).contains("has samplekit 0.2.0"),
        "{}",
        err(&computed)
    );
}

#[test]
#[cfg(unix)]
fn a_package_of_another_build_is_refused() {
    // A local build and the published package share a version number, and
    // the published worker read this command line's requests otherwise:
    // every formula came out *formula changed*. The protocol is checked too.
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("another-build");
    project(&scratch, BILLET);
    let version = String::from_utf8(
        Command::new(venv().join("bin/python"))
            .args(["-c", "import samplekit; print(samplekit.__version__)"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let python = scratch.at("older-build");
    fs::write(
        &python,
        format!(
            "#!/bin/sh\necho '{{\"ready\": {{\"version\": \"{}\", \"python\": \"3.14.0\"}}}}'\ncat >/dev/null\n",
            version.trim()
        ),
    )
    .unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!(
            "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"{}\"\n",
            python.display()
        ),
    );
    let computed = answering(&scratch, &["samples", "compute"], "y\nn\n");
    assert_eq!(code(&computed), 3, "{}", err(&computed));
    assert!(
        err(&computed).contains("of another build"),
        "{}",
        err(&computed)
    );
}

#[test]
fn a_trace_is_named_as_it_reads_on_every_machine() {
    // Kept as an annotated tag, a trace carried its machine and its time:
    // two machines keeping the same facts wrote one name with two contents,
    // which git met as a conflict in files nobody edits.
    let scratch = Scratch::new("trace-name");
    let root = project(&scratch, BILLET);
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let traces = root.join(".samplekit/history/refs/tags/trace");
    let mut named = 0;
    for entry in fs::read_dir(&traces).unwrap() {
        let entry = entry.unwrap();
        let held = fs::read_to_string(entry.path()).unwrap();
        assert_eq!(
            held.trim(),
            entry.file_name().to_string_lossy(),
            "{}",
            entry.path().display()
        );
        named += 1;
    }
    assert!(named > 0);
}

#[test]
fn explain_and_a_script_see_a_formula_changed_before_status_runs() {
    // After an edit to the model, or a pull that brought one, `explain` and a
    // script read the description the edit left behind — until `status`
    // described the model again — and called a value of the old formula
    // current.
    let scratch = Scratch::new("stale-description");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    let explained = run(&scratch, &["samples/c1.md", "explain", "ductility"]);
    assert!(
        out(&explained).contains("formula changed"),
        "{}",
        out(&explained)
    );
    let script = Command::new(venv().join("bin/python"))
        .current_dir(&root)
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .args([
            "-c",
            "import samplekit as sk; print(sorted(sk.load('samples/c1.md').not_current()))",
        ])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&script.stdout);
    assert!(
        said.contains("ductility"),
        "{said}{}",
        String::from_utf8_lossy(&script.stderr)
    );
}

#[test]
fn a_tables_state_column_says_what_its_csv_says() {
    // The table's `--status` column said `current` of a value whose formula
    // changed, where its CSV said `outdated`.
    let scratch = Scratch::new("state-column");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    let status = run(&scratch, &["samples", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    let table = run(&scratch, &["samples", "-c", "name,ductility", "--status"]);
    let csv = run(
        &scratch,
        &["samples", "-c", "name,ductility", "--status", "--csv"],
    );
    assert!(out(&csv).contains("outdated: ductility"), "{}", out(&csv));
    assert!(
        out(&table).contains("outdated: ductility"),
        "{}",
        out(&table)
    );
}

#[test]
fn a_table_no_sample_holds_takes_its_first_row_in_the_workbench() {
    // `+` borrowed a table's columns from another sample only, and refused
    // the first row of a table no sample of the project held yet.
    use samplekit::tui::model::{Key, Workbench};
    let scratch = Scratch::new("first-table-row");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    scratch.write(
        "project/model.py",
        "import samplekit as sk\n\n\nclass Log(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.radius = sk.Property()\n        \
         self.runs = sk.Table({\"run\": sk.Property(), \"load\": sk.Property()}, index=\"run\")\n",
    );
    scratch.write(
        "project/samples/a.md",
        "---\nschema_version: 1\nproperties:\n  radius: 1.05\n---\n",
    );
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    let described = run(&scratch, &["samples", "status"]);
    assert_eq!(code(&described), 0, "{}", err(&described));
    workbench_state();
    let mut workbench = Workbench::open(&scratch.at("project/samples")).unwrap();
    workbench.key(Key::Enter);
    let at = workbench
        .entries()
        .iter()
        .position(|entry| entry.field == "runs")
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    assert!(workbench.on_the_empty_row(), "{}", workbench.message);
    // In the typing mode: the second cell, then the index, which keeps the
    // row; out of it, `w` writes what waits.
    workbench.key(Key::Char('e'));
    workbench.key(Key::Right);
    for character in "2\n".chars() {
        workbench.key(if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        });
    }
    workbench.key(Key::Left);
    for character in "1\n".chars() {
        workbench.key(if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        });
    }
    for key in [Key::Esc, Key::Char('w'), Key::Char('y')] {
        workbench.key(key);
    }
    let written = fs::read_to_string(scratch.at("project/samples/a.md")).unwrap();
    assert!(
        written.contains("load: 2"),
        "{written}\n{}",
        workbench.message
    );
}

/// The state folder the workbench tests of this binary share: set once, since
/// the tests run together and a variable set per test raced.
fn workbench_state() {
    static STATE: std::sync::Once = std::sync::Once::new();
    STATE.call_once(|| {
        let state = std::env::temp_dir().join(format!(
            "samplekit-model-workbench-state-{}",
            std::process::id()
        ));
        // SAFETY: set once, before any workbench of this binary reads it.
        unsafe { std::env::set_var("SAMPLEKIT_STATE_DIR", state) };
    });
}

#[test]
fn compute_warns_of_a_value_outside_its_range() {
    let scratch = Scratch::new("range");
    project(&scratch, BILLET);
    // roughness = 1 - ductility / 4: 0.25 for the first billet, 0.375 for the second.
    let rc = scratch.read("project/.samplekitrc");
    scratch.write(
        "project/.samplekitrc",
        &format!("{rc}[property.roughness]\nrange = [0.3, 1]\n"),
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("roughness = 0.25 is outside its range, 0.3 to 1"),
        "{}",
        err(&output)
    );
    assert!(
        !err(&output).contains("roughness = 0.375"),
        "{}",
        err(&output)
    );
}

/// A formula that never returns, beside one that does.
const LOOPING: &str = r#"import samplekit as sk


class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=lambda: self.diameter.value / self.volume.value)
        self.stuck = sk.Property(compute_value=self._stuck)
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.set_dependencies("stuck", depends_on=["diameter"])

    def _stuck(self):
        while True:
            pass
"#;

#[test]
fn a_formula_past_its_timeout_fails_and_the_others_run() {
    let scratch = Scratch::new("timeout");
    project(&scratch, LOOPING);
    let started = std::time::Instant::now();
    let output = run(
        &scratch,
        &["samples/c1.md", "compute", "--timeout", "0.5", "--write"],
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_ne!(code(&output), 0, "{}", out(&output));
    let said = format!("{}{}", out(&output), err(&output));
    assert!(said.contains("TimedOut"), "{said}");
    assert!(said.contains("ran past --timeout 0.5 s"), "{said}");
    let file = scratch.read("project/samples/c1.md");
    assert!(file.contains("ductility"), "{file}");
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&status).contains("failed — TimedOut"),
        "{}",
        out(&status)
    );
    // A number that is none is refused before anything runs.
    let refused = run(
        &scratch,
        &["samples", "compute", "--timeout", "0", "--write"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("--timeout 30, or 0.5"),
        "{}",
        err(&refused)
    );
}

#[test]
fn a_formula_that_does_not_stop_fails_and_its_sample_goes_on() {
    // Asleep in a C call, a formula does not see the timeout's exception: its
    // worker was stopped, its value left never computed with no failure, and
    // the rest of its sample never ran.
    let scratch = Scratch::new("timeout-asleep");
    project(
        &scratch,
        &LOOPING.replace(
            "        while True:\n            pass\n",
            "        import time\n        time.sleep(60)\n",
        ),
    );
    let started = std::time::Instant::now();
    let output = run(
        &scratch,
        &["samples/c1.md", "compute", "--timeout", "0.5", "--write"],
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!(code(&output), 2, "{}", err(&output));
    let said = format!("{}{}", out(&output), err(&output));
    assert!(
        said.contains("did not stop: its worker was stopped"),
        "{said}"
    );
    // Its failure is written, and the other value computed.
    let file = scratch.read("project/samples/c1.md");
    assert!(file.contains("failed: TimedOut"), "{file}");
    let status = out(&run(&scratch, &["samples/c1.md", "status"]));
    assert!(
        status.contains("stuck") && status.contains("failed — TimedOut"),
        "{status}"
    );
    assert!(!status.contains("ductility"), "{status}");
}

#[test]
fn what_reads_a_value_past_its_timeout_waits_for_it() {
    // `slow` asleep, read by `after`: `after` ran it again past the timeout,
    // a third worker started, the sample was not saved, and what the stopped
    // worker finished was said and counted twice.
    let scratch = Scratch::new("timeout-read");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property()
        self.first = sk.Property(compute_value=lambda: self.a.value + 1, depends_on=["a"])
        self.slow = sk.Property(compute_value=self._slow, depends_on=["a"])
        self.after = sk.Property(compute_value=lambda: self.slow.value * 2, depends_on=["slow"])
        self.other = sk.Property(compute_value=lambda: self.a.value * 3, depends_on=["a"])

    def _slow(self):
        if self.a.value > 1:
            import time
            time.sleep(60)
        return self.a.value
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  a: 1.0\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let set = run(&scratch, &["samples/c1.md", "set", "a=2", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let started = std::time::Instant::now();
    let output = run(
        &scratch,
        &[
            "samples",
            "compute",
            "--rerun",
            "--timeout",
            "0.5",
            "--write",
        ],
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    let said = out(&output);
    assert_eq!(said.matches("✗").count(), 1, "{said}\n{}", err(&output));
    assert!(said.contains("waits for slow, which failed"), "{said}");
    assert_eq!(said.matches("✓ C1  first").count(), 1, "{said}");
    assert!(said.contains("computed 2 values in 1 sample"), "{said}");
    assert!(
        !err(&output).contains("changed while it was computed"),
        "{}",
        err(&output)
    );
    let file = scratch.read("project/samples/c1.md");
    assert!(file.contains("failed: TimedOut"), "{file}");
}

#[test]
fn a_column_past_its_timeout_is_written_failed() {
    // Its cells kept their old values as current, `status` said all was
    // current, and what read the column computed from them.
    let scratch = Scratch::new("timeout-column");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.k = sk.Property()
        self.t = sk.Table(
            {
                "x": sk.Property(),
                "y": sk.Property(compute_value=self._y, depends_on=["row.x", "k"]),
            },
            index="x",
        )
        self.top = sk.Property(compute_value=lambda: max(self.t.values("y")), depends_on=["t.y"])

    def _y(self, row):
        if self.k.value > 1:
            import time
            time.sleep(60)
        return row.x.value * self.k.value
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  k: 1.0\ntables:\n  t:\n    index: x\n    \
         columns:\n      x: {}\n    rows:\n      - {x: 1}\n      - {x: 2}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let set = run(&scratch, &["samples/c1.md", "set", "k=2", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let output = run(
        &scratch,
        &["samples", "compute", "--timeout", "0.5", "--write"],
    );
    let said = out(&output);
    assert_eq!(said.matches("✗").count(), 1, "{said}\n{}", err(&output));
    assert!(said.contains("waits for t.y, which failed"), "{said}");
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("failed — TimedOut (2 rows)"), "{status}");
}

#[test]
fn what_reads_a_waiting_value_waits_and_fails_no_gate() {
    // It was said outdated through it, and failed `status --exit-code`,
    // where the value it reads waits for an entry.
    let scratch = Scratch::new("wait-behind");
    project(
        &scratch,
        "import samplekit as sk\n\n\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n        \
         self.b = sk.Property(compute_value=lambda: self.a.value * 2, depends_on=[\"a\"])\n        \
         self.c = sk.Property(compute_value=lambda: self.b.value + 1, depends_on=[\"b\"])\n",
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  a: 1.0\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let emptied = run(&scratch, &["samples/c1.md", "set", "a=", "--write"]);
    assert_eq!(code(&emptied), 0, "{}", err(&emptied));
    let gate = run(&scratch, &["samples", "status", "--exit-code"]);
    assert_eq!(code(&gate), 0, "{}", out(&gate));
    let said = out(&gate);
    assert!(said.contains("waits for a"), "{said}");
    assert!(said.contains("waits for b, which waits"), "{said}");
    assert!(!said.contains("outdated"), "{said}");
}

#[test]
fn compute_counts_by_cells_and_two_formulas_count_two() {
    // `compute` counted a column once where `set` and `status` counted its
    // cells; and a value and its uncertainty of two formulas reading the same
    // input counted once, their record being one.
    let scratch = Scratch::new("compute-cells");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property()
        self.q = sk.Property(compute_value=lambda: self.a.value * 2,
                             compute_uncertainty=lambda: self.a.value * 0.1, depends_on=["a"])
        self.t = sk.Table(
            {"x": sk.Property(),
             "y": sk.Property(compute_value=lambda row: row.x.value * self.a.value,
                              depends_on=["row.x", "a"])},
            index="x",
        )
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  a: 1.0\ntables:\n  t:\n    index: x\n    \
         columns:\n      x: {}\n    rows:\n      - {x: 1}\n      - {x: 2}\n      - {x: 3}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    assert!(
        out(&computed).contains("computed 5 values in 1 sample"),
        "{}",
        out(&computed)
    );
    let set = run(&scratch, &["samples/c1.md", "set", "a=2", "--write"]);
    assert!(
        out(&set).contains("this makes 5 values not current"),
        "{}",
        out(&set)
    );
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("5 values are not current"), "{status}");
    assert!(status.contains("q.u"), "{status}");
    let previewed = run(&scratch, &["samples", "compute"]);
    assert!(
        out(&previewed).contains("5 values in 1 sample would be computed"),
        "{}",
        out(&previewed)
    );
}

#[test]
fn validate_reads_each_sample_with_the_model() {
    // The files alone were judged: `0 defects` of samples no computation
    // could read.
    let scratch = Scratch::new("validate-model");
    project(
        &scratch,
        "import samplekit as sk\n\n\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.kind = sk.Property()\n",
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nkind: steel\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let validated = run(&scratch, &["samples", "validate"]);
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    assert!(
        out(&validated).contains("[model]: the model cannot read this sample: line 4: 'kind'"),
        "{}",
        out(&validated)
    );
    // A model that cannot be read at all: its configuration's defect.
    scratch.write(
        "project/model.py",
        "import samplekit as sk\n\n\nclass M(sk.Sample):\n    nothing = here\n",
    );
    let validated = run(&scratch, &["samples", "validate"]);
    assert!(
        out(&validated)
            .contains("the model cannot be read, so no sample can be computed: NameError"),
        "{}",
        out(&validated)
    );
}

#[test]
fn a_value_and_its_uncertainty_of_two_formulas_count_apart() {
    // Each is computed and outdated on its own: an input the uncertainty
    // alone reads makes `q.u` outdated, one both read makes both.
    let scratch = Scratch::new("count-apart");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property()
        self.b = sk.Property()
        self.q = sk.Property(compute_value=self._v, compute_uncertainty=self._u,
                             depends_on={"v": ["a"], "u": ["a", "b"]})

    def _v(self):
        return 2 * self.a.value

    def _u(self):
        return 0.1 * self.b.value
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  a: 1.0\n  b: 1.0\n---\n",
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let one = run(&scratch, &["samples/c1.md", "set", "b=5", "--write"]);
    assert!(
        out(&one).contains("this makes 1 value not current"),
        "{}",
        out(&one)
    );
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(
        status.contains("1 value is not current") && status.contains("q.u"),
        "{status}"
    );
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let both = run(&scratch, &["samples/c1.md", "set", "a=7", "--write"]);
    assert!(
        out(&both).contains("this makes 2 values not current"),
        "{}",
        out(&both)
    );
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("2 values are not current"), "{status}");
}

#[test]
fn a_waiting_value_keeps_its_number_and_says_it_waits() {
    let scratch = Scratch::new("waiting-marked");
    project(&scratch, BILLET);
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let cleared = run(&scratch, &["samples/c1.md", "set", "volume=", "--write"]);
    assert_eq!(code(&cleared), 0, "{}", err(&cleared));
    // A table brackets it, and says why.
    let table = run(&scratch, &["samples", "-c", "name,ductility"]);
    assert!(out(&table).contains("(3"), "{}", out(&table));
    assert!(
        err(&table).contains("a value in brackets waits"),
        "{}",
        err(&table)
    );
    // A CSV writes its state beside it, and every other state but current:
    // C2 was never computed.
    let csv = run(&scratch, &["samples", "-c", "name,ductility", "--csv"]);
    let written = out(&csv);
    let lines: Vec<&str> = written.lines().collect();
    assert!(lines[0].ends_with("ductility_state"), "{written}");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("C1,") && line.ends_with(",waiting"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("C2,") && line.ends_with(",never_computed"))
    );
    // JSON says it waits, not outdated.
    let json = run(&scratch, &["samples", "-c", "name,ductility", "--json"]);
    assert!(
        out(&json).contains("\"state\": \"waiting\""),
        "{}",
        out(&json)
    );
}

#[test]
fn a_formula_reading_a_whole_column_waits_for_a_missing_cell() {
    // One hardness of twenty left out: `drop`, reading the column, and `rate`,
    // giving a column from it, were given `None` among the numbers and raised
    // a TypeError. They wait, naming the cell; `nominal`, row by row, runs.
    let scratch = Scratch::new("column-hole");
    copied(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/metals/05-ageing"),
        &scratch.at("project"),
    );
    std::os::unix::fs::symlink(venv(), scratch.at(".venv")).unwrap();
    let piece = scratch.at("project/pieces/alpha-titanium.md");
    let held = fs::read_to_string(&piece).unwrap();
    let at = held.find("      - day: 3\n").unwrap();
    let line = held[at..].find("        hardness:").unwrap() + at;
    let end = held[line..].find('\n').unwrap() + line + 1;
    fs::write(&piece, format!("{}{}", &held[..line], &held[end..])).unwrap();
    let plan = run(
        &scratch,
        &["pieces/alpha-titanium.md", "compute", "--rerun"],
    );
    let said = out(&plan);
    for value in ["drop", "ageing.rate"] {
        assert!(
            said.lines().any(|line| line.contains(&format!(" {value} "))
                && line.contains("waits for ageing.hardness[3]")),
            "{value}\n{said}"
        );
    }
    assert!(
        said.lines()
            .any(|line| line.contains("ageing.nominal") && line.contains("current")),
        "{said}"
    );
    let written = run(
        &scratch,
        &["pieces/alpha-titanium.md", "compute", "--rerun", "--write"],
    );
    assert!(!err(&written).contains("TypeError"), "{}", err(&written));
}

#[test]
fn a_value_the_model_no_longer_computes_blocks_nothing() {
    // `roughness` renamed `rigidity`: the old `roughness`, its record in the file, was
    // *outdated* for good once an input moved — `compute --write` could do
    // nothing, and `status --exit-code` failed for ever.
    let scratch = Scratch::new("orphan");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &BILLET
            .replace("self.roughness", "self.rigidity")
            .replace("\"roughness\"", "\"rigidity\""),
    );
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=14", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let gate = run(&scratch, &["samples", "status", "--exit-code"]);
    assert_eq!(code(&gate), 0, "{}", out(&gate));
    // Apart, under its own sentence, outside the count.
    let said = out(&gate);
    assert!(
        said.contains("every computed value in 2 samples is current"),
        "{said}"
    );
    assert!(
        said.contains("2 values the model no longer computes"),
        "{said}"
    );
    assert!(
        said.contains("samplekit samples/c1.md set roughness= removes it"),
        "{said}"
    );
    assert!(!said.contains("not current"), "{said}");
    let validated = run(&scratch, &["samples", "validate"]);
    assert!(
        out(&validated).contains("roughness: the model no longer computes it"),
        "{}",
        out(&validated)
    );
    assert!(
        !out(&validated).contains("not current"),
        "{}",
        out(&validated)
    );
    // The first way out: removed.
    let removed = run(&scratch, &["samples/c1.md", "set", "roughness=", "--write"]);
    assert_eq!(code(&removed), 0, "{}", err(&removed));
    assert!(
        !fs::read_to_string(root.join("samples/c1.md"))
            .unwrap()
            .contains("roughness")
    );
}

#[test]
fn validate_asks_matplotlib_of_a_figures_own_settings() {
    // `matplotlib = { "lines.bogus" = 3 }` in a figure passed `validate`, which
    // asked only of `[matplotlib]`; `plot` refused it, after.
    let scratch = Scratch::new("validate-figure-matplotlib");
    project(&scratch, BILLET);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!(
            "{rc}[figure.f]\nx = \"diameter\"\ny = \"diameter\"\nmatplotlib = {{ \"lines.bogus\" = 3 }}\n"
        ),
    );
    let validated = run(&scratch, &["samples", "validate"]);
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    assert!(
        out(&validated).contains("[figure.f] matplotlib: lines.bogus is not a matplotlib setting"),
        "{}",
        out(&validated)
    );
}

#[test]
fn validate_asks_matplotlib_of_the_project_settings() {
    // `"axes.gird" = true` and a figure size of three sides passed `validate`;
    // only `plot` refused them, after.
    let scratch = Scratch::new("validate-matplotlib");
    project(&scratch, BILLET);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!("{rc}[matplotlib]\n\"axes.gird\" = true\n\"figure.figsize\" = [1, 2, 3]\n\"font.size\" = 11\n"),
    );
    let validated = run(&scratch, &["samples", "validate"]);
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    let said = out(&validated);
    assert!(
        said.contains("axes.gird is not a matplotlib setting"),
        "{said}"
    );
    assert!(said.contains("did you mean: axes.grid?"), "{said}");
    assert!(
        said.contains("figure.figsize = [1, 2, 3]: Expected 2 values"),
        "{said}"
    );
    assert!(!said.contains("font.size"), "{said}");
    // No Python to ask: a note, not a defect.
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[matplotlib]\n\"axes.gird\" = true\n",
    );
    let unchecked = run(&scratch, &["samples", "validate"]);
    assert!(
        out(&unchecked).contains("[matplotlib] was not checked"),
        "{}",
        out(&unchecked)
    );
    assert_eq!(code(&unchecked), 0, "{}", out(&unchecked));
}

#[test]
fn a_snapshot_is_judged_by_the_model_it_held() {
    // `status --at` never saw a changed formula, and `explain --at` judged a
    // value of its day's formula against today's model, offering a command in
    // a folder already removed.
    let scratch = Scratch::new("judged-then");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let log = out(&run(&scratch, &["log"]));
    // #1 the values of the edited formula, #2 the edit, #3 the first values.
    assert!(log.contains("the model changed"), "{log}");
    let then = run(&scratch, &["samples", "status", "--at", "2"]);
    assert!(out(&then).contains("formula changed"), "{}", out(&then));
    let now = run(&scratch, &["samples", "status", "--at", "1"]);
    assert!(!out(&now).contains("formula changed"), "{}", out(&now));
    let explained = out(&run(
        &scratch,
        &["samples/c1.md", "explain", "ductility", "--at", "3"],
    ));
    assert!(
        explained.contains("this value was current then"),
        "{explained}"
    );
    let explained = out(&run(
        &scratch,
        &["samples/c1.md", "explain", "ductility", "--at", "2"],
    ));
    assert!(
        explained.contains("this value was outdated then — formula changed"),
        "{explained}"
    );
    assert!(!explained.contains("samplekit-at-"), "{explained}");
}

#[test]
fn a_formula_rewritten_to_the_same_number_is_kept_by_its_recompute() {
    // `compute --write` over a formula rewritten without changing its number
    // wrote no file and took no snapshot: the model the values were now
    // judged by was *changed outside SampleKit, not yet kept*.
    let scratch = Scratch::new("same-number-kept");
    project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &BILLET.replace(
            "return self.diameter.value / self.volume.value",
            "return (self.diameter.value * 2) / (self.volume.value * 2)",
        ),
    );
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(out(&again).contains("nothing written"), "{}", out(&again));
    let log = out(&run(&scratch, &["log"]));
    assert!(!log.contains("not yet kept"), "{log}");
    assert!(log.contains("the model changed: ductility"), "{log}");
    assert!(
        !out(&run(&scratch, &["samples", "status"])).contains("formula changed"),
        "{log}"
    );
}

/// A model whose table formulas break their contract where a sample's `mode`
/// says: a key too few for a row, a cell too many for a column.
const CONTRACT: &str = r#"
import samplekit as sk

class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.mode = 0
        self.t = sk.Table(
            {
                "x": sk.Property(),
                "y": sk.Property(),
                "z": sk.Property(),
                "w": sk.Property(compute_column=self._column, depends_on=["t.x", "mode"]),
            },
            index="x",
            computed=[
                sk.ComputedColumns(["y", "z"], compute=self._row, depends_on=["row.x", "mode"]),
            ],
        )

    def _row(self, row):
        if self.mode == 1:
            return {"y": row.x.value}
        return {"y": row.x.value * 2, "z": row.x.value * 3}

    def _column(self, columns):
        xs = columns["x"].values
        if self.mode == 1:
            return xs + [99]
        return [x * 10 for x in xs]
"#;

#[test]
fn a_formula_that_breaks_its_contract_failed() {
    // A derivation returning a key too few, or a cell too many, wrote empty
    // cells said *never computed*; `explain` refused `t.y`, the name `status`
    // gives, and said nothing of the failure.
    let scratch = Scratch::new("contract-failed");
    project(&scratch, CONTRACT);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nmode: 1\ntables:\n  t:\n    index: x\n    \
         columns:\n      x: {}\n    rows:\n      - x: 1\n      - x: 2\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 2, "{}", err(&computed));
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("t.y     failed — ValueError"), "{status}");
    assert!(status.contains("t.w     failed — ValueError"), "{status}");
    assert!(!status.contains("never computed"), "{status}");
    let listed = out(&run(&scratch, &["samples", "compute"]));
    assert!(
        listed.contains("failed") && !listed.contains("never computed"),
        "{listed}"
    );
    let explained = out(&run(&scratch, &["samples/c1.md", "explain", "t.y"]));
    assert!(
        explained.contains("t.y is a column: t.y[#0]"),
        "{explained}"
    );
    assert!(
        explained.contains("its formula failed: ValueError"),
        "{explained}"
    );
    assert!(
        explained.contains("a derivation declared [y, z]"),
        "{explained}"
    );
}

/// `b` fails on the sample `c1`; `c` reads it.
const DOWNSTREAM: &str = r#"
import samplekit as sk

class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.b = sk.Property(compute_value=self._b, depends_on=["diameter"])
        self.c = sk.Property(compute_value=lambda: self.b.value + 1, depends_on=["b"])
        self.d = sk.Property(compute_value=lambda: self.c.value + 1, depends_on=["c"])

    def _b(self):
        if self.diameter.value > 11:
            raise ValueError("too much diameter")
        return self.diameter.value
"#;

#[test]
fn a_value_behind_a_failure_waits_for_it_on_every_surface() {
    // `compute` said *c waits for b, which failed*; `status` and its JSON said
    // never computed, and `explain` promised a run that would fail again.
    let scratch = Scratch::new("behind-failure");
    project(&scratch, DOWNSTREAM);
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 2, "{}", err(&computed));
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("waits for b, which failed"), "{status}");
    assert!(status.contains("behind what failed"), "{status}");
    let json = out(&run(&scratch, &["samples", "status", "--json"]));
    assert!(
        json.contains("\"said\": \"waits for b, which failed\""),
        "{json}"
    );
    let explained = out(&run(&scratch, &["samples/c1.md", "explain", "c"]));
    assert!(
        explained.contains("it waits for b, which failed: samplekit samples/c1.md explain b"),
        "{explained}"
    );
    // `-p` lists what it runs, not what reads it.
    scratch.write(
        "project/model.py",
        &DOWNSTREAM.replace(
            "return self.diameter.value\n",
            "return self.diameter.value * 1.0\n",
        ),
    );
    let _ = run(&scratch, &["samples", "status"]);
    let listed = out(&run(&scratch, &["samples", "compute", "-p", "b"]));
    assert!(!listed.contains("reads b"), "{listed}");
}

/// Each row's `nominal`, derived from its hardness; a table of two rows.
const ROWS: &str = r#"
import samplekit as sk

class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.trial = sk.Table(
            {
                "day": sk.Property(),
                "hardness": sk.Property(),
                "nominal": sk.Property(compute=lambda row: {"nominal": row.hardness.value * 2}, depends_on=["row.hardness"]),
            },
            index="day",
        )
"#;

#[test]
fn values_not_current_are_counted_by_cells_and_as_shown() {
    // `status` counted a column once, `view` each cell, and a table's banner
    // the cells of a column it did not show.
    let scratch = Scratch::new("count-cells");
    project(&scratch, ROWS);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\ntables:\n  trial:\n    index: day\n    columns:\n      \
         day: {}\n      hardness: {}\n    rows:\n      - {day: 1, hardness: 1.05}\n      \
         - {day: 2, hardness: 1.04}\n      - {day: 3, hardness: 1.03}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let set = run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "trial.hardness[1]=1.06",
            "trial.hardness[2]=1.045",
            "trial.hardness[3]=1.035",
            "--write",
        ],
    );
    assert!(
        out(&set).contains("this makes 3 values not current"),
        "{}",
        out(&set)
    );
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("3 values are not current"), "{status}");
    let shown = run(&scratch, &["samples/c1.md", "-c", "trial.nominal[1]"]);
    assert!(
        err(&shown).contains("1 value shown is not current"),
        "{}",
        err(&shown)
    );
    // A column whose formula changed: its rows, in the list and in the
    // gate's own count, which counted lines.
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let model = scratch.read("project/model.py");
    scratch.write("project/model.py", &model.replace("* 2}", "* 3}"));
    let gate = run(&scratch, &["samples", "status", "--exit-code"]);
    assert!(
        out(&gate).contains("formula changed (3 rows)"),
        "{}",
        out(&gate)
    );
    assert!(
        out(&gate).contains("3 values are not current"),
        "{}",
        out(&gate)
    );
    assert!(
        err(&gate).contains("error: 3 values are not current"),
        "{}",
        err(&gate)
    );
}

#[test]
fn a_new_row_makes_what_reads_a_computed_column_outdated() {
    // `first` read t.y, computed for every row, and was said to wait for the
    // new row's cell; `second`, reading `first`, was outdated: two words for
    // one cause.
    let scratch = Scratch::new("new-row-outdated");
    project(
        &scratch,
        r#"import samplekit as sk


class M(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.t = sk.Table(
            {
                "x": sk.Property(),
                "y": sk.Property(compute=self._y, depends_on=["row.x"]),
            },
            index="x",
        )
        self.first = sk.Property(compute=self._first, depends_on=["t.y"])
        self.second = sk.Property(compute=self._second, depends_on=["first", "t.y"])

    def _y(self, row):
        return 2 * row.x.value

    def _first(self):
        return sum(self.t.values("y"))

    def _second(self):
        return 2 * self.first.value
"#,
    );
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\ntables:\n  t:\n    index: x\n    columns:\n      \
         x: {}\n    rows:\n      - {x: 1}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).ok();
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let preview = run(&scratch, &["samples/c1.md", "set", "--add-row", "t", "x=3"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(!said.contains("waits for"), "{said}");
    for value in ["first", "second"] {
        assert!(
            said.lines()
                .any(|line| line.trim_start().starts_with(value) && line.contains("outdated")),
            "{value}: {said}"
        );
    }
}

#[test]
fn a_new_rows_derived_cell_is_never_computed_on_every_surface() {
    // Added by `--add-row`, a row's derived cell was *current* in a table's
    // state column, *absent* in JSON, and `explain` doubted the model
    // computed it.
    let scratch = Scratch::new("new-row-owed");
    project(&scratch, ROWS);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\ntables:\n  trial:\n    index: day\n    columns:\n      \
         day: {}\n      hardness: {}\n    rows:\n      - {day: 1, hardness: 1.05}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let added = run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "--add-row",
            "trial",
            "day=2",
            "hardness=1.02",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    let _ = run(&scratch, &["samples", "status"]);
    let table = out(&run(
        &scratch,
        &["samples/c1.md", "-c", "trial.nominal[2]", "--status"],
    ));
    assert!(
        table.contains("never_computed: trial.nominal[2]"),
        "{table}"
    );
    let json = out(&run(
        &scratch,
        &["samples/c1.md", "-c", "trial.nominal[2]", "--json"],
    ));
    assert!(json.contains("\"state\": \"never_computed\""), "{json}");
    let explained = out(&run(
        &scratch,
        &["samples/c1.md", "explain", "trial.nominal[2]"],
    ));
    assert!(
        explained.contains("the model computes it from row.hardness"),
        "{explained}"
    );
    assert!(
        !explained.contains("if the model computes it"),
        "{explained}"
    );
}

/// A drop over a whole column of hardnesses.
const DROP: &str = r#"
import samplekit as sk

class Billet(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.trial = sk.Table({"day": sk.Property(), "hardness": sk.Property()}, index="day")
        self.drop = sk.Property(compute_value=lambda: self.trial.values("hardness")[0] - self.trial.values("hardness")[-1])
        self.set_dependencies("drop", depends_on=["trial.hardness"])
"#;

#[test]
fn a_preview_says_what_a_new_row_or_a_restore_makes_not_current() {
    // `--add-row` named the columns the model fills, and `restore` the numbers
    // it puts back: neither said the values they made outdated.
    let scratch = Scratch::new("preview-outdates");
    project(&scratch, DROP);
    scratch.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\ntables:\n  trial:\n    index: day\n    columns:\n      \
         day: {}\n      hardness: {}\n    rows:\n      - {day: 1, hardness: 1.05}\n      \
         - {day: 2, hardness: 1.02}\n---\n",
    );
    fs::remove_file(scratch.at("project/samples/c2.md")).unwrap();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let added = out(&run(
        &scratch,
        &[
            "samples/c1.md",
            "set",
            "--add-row",
            "trial",
            "day=3",
            "hardness=1.01",
        ],
    ));
    assert!(added.contains("this makes 1 value not current"), "{added}");
    assert!(added.contains("drop"), "{added}");
    // A hardness changed and the drop computed again: taking back the
    // computation leaves the drop outdated, which the preview says.
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "trial.hardness[2]=1.01", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let back = out(&run(&scratch, &["samples/c1.md", "restore"]));
    assert!(back.contains("not current"), "{back}");
    assert!(back.contains("drop"), "{back}");
}

#[test]
fn a_statistic_a_file_states_gives_way_to_the_models() {
    // A property and its statistic are one across a project: a file stating
    // the median is read as it stands, and the next computation takes the
    // model's mean, saying so before it does.
    let scratch = Scratch::new("stated-statistic");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 16.0], statistics: {v: median}, unit: g}",
            1,
        ),
    );
    let read = run(&scratch, &["samples/c1.md", "-c", "diameter"]);
    assert!(
        out(&read).contains("12"),
        "the median, as it stands: {}",
        out(&read)
    );
    let status = run(&scratch, &["samples/c1.md", "status"]);
    assert!(
        out(&status).contains("statistic median \u{2192} mean"),
        "{}",
        out(&status)
    );
    let preview = run(&scratch, &["samples/c1.md", "compute"]);
    assert!(
        out(&preview).contains("statistic median \u{2192} mean"),
        "{}{}",
        out(&preview),
        err(&preview)
    );
    let computed = run(&scratch, &["samples/c1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let written = scratch.read("project/samples/c1.md");
    assert!(written.contains("v: 13.0"), "the mean: {written}");
    assert!(
        written.contains("statistics: {v: mean, u: standard_error}"),
        "{written}"
    );
}

#[test]
fn validate_notes_a_statistic_the_model_does_not_declare() {
    let scratch = Scratch::new("stated-statistic-noted");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 16.0], statistics: {v: median}, unit: g}",
            1,
        ),
    );
    // The model described, as `status` has it described.
    let _ = run(&scratch, &["samples", "status"]);
    let validated = run(&scratch, &["samples", "validate"]);
    assert_eq!(code(&validated), 0, "{}", err(&validated));
    assert!(
        out(&validated)
            .contains("diameter: the file states the median of its readings, the model the mean"),
        "{}",
        out(&validated)
    );
}

/// A table whose `G` is computed row by row and `N` from the whole column `R`.
const PREHEAT: &str = r#"import samplekit as sk


class Preheat(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=self._g, depends_on=["row.R"]),
                "N": sk.Property(compute_column=lambda columns: [value / 4.0 for value in columns["R"].values], depends_on=["preheating.R"]),
            },
            index="T",
        )

    def _g(self, row):
        return 1 / row.R.value
"#;

fn preheat(scratch: &Scratch, model: &str) {
    project(scratch, model);
    scratch.write(
        "project/samples/m1.md",
        "---\nschema_version: 1\nname: M1\ntables:\n  preheating:\n    index: T\n    rows:\n      \
         - {T: 10, R: 2.0}\n      - {T: 20, R: 4.0}\n---\n",
    );
}

#[test]
fn a_row_without_its_input_waits_on_every_surface() {
    // A row added without `R`: `N`, read from the whole column, waits for
    // `preheating.R[30]` in `status` and `explain` as in `compute`.
    let scratch = Scratch::new("row-waits");
    preheat(&scratch, PREHEAT);
    let computed = run(&scratch, &["samples/m1.md", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let added = run(
        &scratch,
        &[
            "samples/m1.md",
            "set",
            "--add-row",
            "preheating",
            "T=30",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    for arguments in [
        &["samples/m1.md", "status"][..],
        &["samples/m1.md", "compute"][..],
    ] {
        let said = out(&run(&scratch, arguments));
        let line = said
            .lines()
            .find(|line| line.contains("preheating.N"))
            .unwrap_or_default();
        assert!(
            line.contains("waits for preheating.R[30]"),
            "{arguments:?}: {said}"
        );
    }
    let explained = out(&run(
        &scratch,
        &["samples/m1.md", "explain", "preheating.N"],
    ));
    assert!(
        explained.contains("this value waits for preheating.R[30], which nobody entered"),
        "{explained}"
    );
}

#[test]
fn a_row_formula_failure_names_its_row() {
    let scratch = Scratch::new("row-failure");
    preheat(
        &scratch,
        &PREHEAT.replace(
            "        return 1 / row.R.value",
            "        if row.T.value == 20:\n            raise ValueError(\"bad row\")\n        return 1 / row.R.value",
        ),
    );
    let output = run(&scratch, &["samples/m1.md", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(err(&output).contains("in the row [20]"), "{}", err(&output));
}

#[test]
fn a_value_formula_returning_a_pair_is_told_compute() {
    let scratch = Scratch::new("pair-returned");
    project(
        &scratch,
        &BILLET.replace(
            "return self.diameter.value / self.volume.value",
            "return (self.diameter.value / self.volume.value, 0.1)",
        ),
    );
    let output = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("a value and its uncertainty are returned by a formula of compute="),
        "{}",
        err(&output)
    );
}

#[test]
fn a_check_without_history_says_what_it_cannot_vouch_for() {
    // A clone without the history, on a computer that computed nothing:
    // `status --exit-code` counts the values whose formula it cannot know.
    let scratch = Scratch::new("check-without-history");
    project(&scratch, BILLET);
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    fs::remove_dir_all(scratch.at("project/.samplekit")).unwrap();
    let _ = fs::remove_dir_all(scratch.at("state"));
    let checked = run(&scratch, &["samples", "status", "--exit-code"]);
    assert!(
        err(&checked).contains("the formula that computed 4 values in 2 samples is not known here"),
        "{}",
        err(&checked)
    );
    let plain = run(&scratch, &["samples", "status"]);
    assert!(
        !err(&plain).contains("is not known here"),
        "{}",
        err(&plain)
    );
}

#[test]
fn a_write_during_a_computation_leaves_the_models_change_named() {
    // A `set` while a long computation ran kept the model's change first,
    // unnamed, the formulas being known to the computation alone: the
    // computation keeps it before it runs.
    let scratch = Scratch::new("model-change-concurrent");
    let root = project(&scratch, BILLET);
    scratch.write(
        "project/samples/notes.md",
        "---\nschema_version: 1\nname: notes\n---\n",
    );
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &billet_edited().replace(
            "        return 2 * self.diameter.value",
            "        import time\n        time.sleep(1.5)\n        return 2 * self.diameter.value",
        ),
    );
    let computing = command(&scratch, &root)
        .args(["samples", "compute", "--write"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let set = run(
        &scratch,
        &["samples/notes.md", "set", "tester=ana", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let computed = computing.wait_with_output().unwrap();
    assert_eq!(computed.status.code(), Some(0));
    let messages: Vec<String> = snapshots(&root)
        .iter()
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    assert_eq!(
        messages[..3],
        [
            "samplekit samples compute --write".to_string(),
            "samplekit samples/notes.md set tester=ana --write".to_string(),
            "the model changed: ductility".to_string()
        ],
        "{messages:?}"
    );
}

#[test]
fn a_first_write_takes_the_model_as_the_one_that_computed_the_values() {
    // Values computed where no history was kept — a collection moved in, a
    // demo copied — then a first `set`: a formula changed after it was said
    // current, and computed nothing.
    let scratch = Scratch::new("first-write-baseline");
    let root = project(&scratch, BILLET);
    let computed = command(&scratch, &root)
        .args(["samples", "compute", "--write"])
        .env("SAMPLEKIT_HISTORY", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    assert!(!root.join(".samplekit/history").exists());
    let _ = fs::remove_dir_all(scratch.at("state"));
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=12.5", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    scratch.write("project/model.py", &billet_edited());
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(status.contains("formula changed"), "{status}");
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(out(&again).contains("computed 4 values"), "{}", out(&again));
}

/// A formula reading an attribute its files give, which the model does not
/// declare.
const MATERIAL: &str = r#"import samplekit as sk

REFERENCE = {"aluminium": 2.70, "steel": 7.85}


class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.mass = sk.Property(unit="g")
        self.volume = sk.Property(unit="cm3")
        self.density = sk.Property(unit="g/cm3", compute_value=self._density,
                                   depends_on=["mass", "volume"])
        self.deviation = sk.Property(unit="%", compute_value=self._deviation,
                                     depends_on=["density", "material"])

    def _density(self):
        return self.mass.value / self.volume.value

    def _deviation(self):
        reference = REFERENCE[self.material]
        return 100.0 * (self.density.value - reference) / reference
"#;

#[test]
fn a_formula_reading_an_attribute_is_a_model_that_reads() {
    // The model said unreadable, `new` wrote its quantities as attributes,
    // and a file without the attribute was not read at all.
    let scratch = Scratch::new("attribute-input");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    scratch.write("project/model.py", MATERIAL);
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    fs::create_dir_all(scratch.at("project/samples")).unwrap();
    let root = scratch.at("project");
    for arguments in [
        &[
            "samples",
            "new",
            "P-01",
            "material=aluminium",
            "mass=27.1",
            "mass.u=0.1",
            "volume=10.02",
            "--write",
        ][..],
        &["samples", "new", "P-02", "mass=54", "volume=20", "--write"][..],
    ] {
        let made = command(&scratch, &root)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(code(&made), 0, "{arguments:?}: {}", err(&made));
        assert!(!err(&made).contains("could not be read"), "{}", err(&made));
    }
    let written = fs::read_to_string(root.join("samples/P-01.md")).unwrap();
    assert!(
        written.contains("mass: {v: 27.1, u: 0.1, unit: g}"),
        "{written}"
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(
        status
            .lines()
            .any(|line| line.contains("P-02") && line.contains("waits for material")),
        "{status}"
    );
    let set = run(
        &scratch,
        &["samples/P-02.md", "set", "material=steel", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let status = out(&run(&scratch, &["samples", "status"]));
    assert!(
        status.contains("every computed value in 2 samples is current"),
        "{status}"
    );
}

#[test]
fn a_quantity_written_as_an_attribute_is_named_with_its_way_out() {
    // Data moved in before the model said what it is: `compute` said
    // *'mass' is already a property*, naming neither the file nor what to
    // do, and `validate` found nothing.
    let scratch = Scratch::new("quantity-as-attribute");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    scratch.write("project/model.py", MATERIAL);
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.write(
        "project/samples/p3.md",
        "---\nschema_version: 1\nmaterial: steel\nmass: 54\nvolume: 20\n---\n",
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert!(
        err(&computed).contains(
            "'mass' is written as an attribute, at the top level, and the model declares it a \
             property: samplekit samples/p3.md set mass=54 writes it as one"
        ),
        "{}",
        err(&computed)
    );
    let validated = run(&scratch, &["samples", "validate"]);
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    assert!(
        out(&validated).contains("'volume' is written as an attribute"),
        "{}",
        out(&validated)
    );
    let moved = run(
        &scratch,
        &["samples/p3.md", "set", "mass=54", "volume=20", "--write"],
    );
    assert_eq!(code(&moved), 0, "{}", err(&moved));
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
}

#[test]
fn a_write_during_a_computation_keeps_only_its_own_change() {
    // A `set` on a sample the computation had already written kept, under its
    // own name, the values the computation wrote there.
    let scratch = Scratch::new("own-change-concurrent");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write(
        "project/model.py",
        &billet_edited().replace(
            "        return 2 * self.diameter.value",
            "        import time\n        if self.name == \"C2\":\n            time.sleep(3.0)\n        return 2 * self.diameter.value",
        ),
    );
    let computing = command(&scratch, &root)
        .args(["samples", "compute", "--write"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let set = run(&scratch, &["samples/c1.md", "set", "volume=5", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let computed = computing.wait_with_output().unwrap();
    assert_eq!(computed.status.code(), Some(0));
    let messages: Vec<String> = snapshots(&root)
        .iter()
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    let at = messages
        .iter()
        .position(|message| message == "samplekit samples/c1.md set volume=5 --write")
        .expect("the set's snapshot");
    assert!(messages[at + 1].ends_with("— under way"), "{messages:?}");
    let number = (at + 1).to_string();
    let before = (at + 2).to_string();
    let diff = out(&run(
        &scratch,
        &["diff", "--from", &before, "--to", &number],
    ));
    assert!(diff.contains("volume"), "{diff}");
    assert!(!diff.contains("ductility"), "{diff}");
}

#[test]
fn a_formula_given_back_is_no_change_of_the_model() {
    // A formula broken, computed, then put back, and another changed: the
    // snapshot named both, the one put back said changed.
    let scratch = Scratch::new("formula-given-back");
    let root = project(&scratch, BILLET);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &billet_edited());
    let broken = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&broken), 0, "{}", err(&broken));
    scratch.write(
        "project/model.py",
        &BILLET.replace(
            "1 - self.ductility.value / 4.0",
            "1 - self.ductility.value / 5.0",
        ),
    );
    let again = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    let messages: Vec<String> = snapshots(&root)
        .iter()
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    assert!(
        messages.contains(&"the model changed: roughness".to_string()),
        "{messages:?}"
    );
}

const HAND_TABLE: &str = r#"import samplekit as sk


class Hand(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ageing = sk.Table(
            {"day": sk.Property(unit="d"), "hardness": sk.Property(), "temperature": sk.Property(unit="degC")},
            index="day",
        )
"#;

#[test]
fn a_table_written_by_hand_is_shown_in_the_models_units() {
    // Its rows alone, no `columns:`: before a write puts the model's
    // columns in the file, it read without their units.
    let scratch = Scratch::new("hand-table-units");
    project(&scratch, HAND_TABLE);
    let source = "---\nschema_version: 1\nname: hand\ntables:\n  ageing:\n    index: day\n    \
                  rows:\n      - {day: 0, hardness: 1.050, temperature: 19}\n---\n";
    scratch.write("project/samples/hand.md", source);
    let described = status(&scratch);
    assert_eq!(code(&described), 0, "{}", err(&described));
    for target in ["samples/hand.md", "samples"] {
        let shown = run(&scratch, &[target, "view"]);
        assert_eq!(code(&shown), 0, "{}", err(&shown));
        assert!(out(&shown).contains("day [d]"), "{target}: {}", out(&shown));
        assert!(
            out(&shown).contains("temperature [°C]") || out(&shown).contains("temperature [degC]"),
            "{target}: {}",
            out(&shown)
        );
    }
    // Shown, not written.
    assert_eq!(scratch.read("project/samples/hand.md"), source);
}

#[test]
fn a_write_gives_a_hand_written_table_the_models_units() {
    // `set` wrote the model's columns without their units, which only a
    // computation then gave.
    let scratch = Scratch::new("hand-table-write-units");
    project(&scratch, HAND_TABLE);
    scratch.write(
        "project/samples/hand.md",
        "---\nschema_version: 1\nname: hand\ntables:\n  ageing:\n    index: day\n    \
         rows:\n      - {day: 0, hardness: 1.050, temperature: 19}\n---\n",
    );
    let described = status(&scratch);
    assert_eq!(code(&described), 0, "{}", err(&described));
    let written = run(
        &scratch,
        &["samples/hand.md", "set", "status=open", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let file = scratch.read("project/samples/hand.md");
    assert!(file.contains("day: {unit: d}"), "{file}");
    assert!(file.contains("temperature: {unit: degC}"), "{file}");
}

#[test]
fn a_value_without_its_record_is_found_edited_by_a_filter() {
    // `-c state` said `edited`, as the model's plan says *record missing*;
    // `state == edited`, answered from the files alone, found none, and
    // `status` advised a `compute` that had nothing to compute.
    let scratch = Scratch::new("record-missing-filter");
    project(&scratch, BILLET);
    for name in ["c1", "c2"] {
        scratch.write(
            &format!("project/samples/{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {}\nproperties:\n  diameter: {{v: 12.0, unit: g}}\n  \
                 volume: {{v: 4.0, unit: L}}\n  ductility: {{v: 3.0}}\n  roughness: {{v: 0.25}}\n---\n",
                name.to_uppercase()
            ),
        );
    }
    let found = run(&scratch, &["samples", "-f", "state == edited"]);
    assert_eq!(code(&found), 0, "{}", err(&found));
    assert!(out(&found).contains("c1.md"), "{}", out(&found));
    let said = status(&scratch);
    assert!(out(&said).contains("record missing"), "{}", out(&said));
    assert!(
        !out(&said).contains("compute lists what would run"),
        "{}",
        out(&said)
    );
}

#[test]
fn a_cached_formula_changed_is_outdated() {
    // `functools.lru_cache` wraps the model's function: taken for a
    // library's, the function changed staled nothing.
    let scratch = Scratch::new("lru-cached");
    let cached = BILLET
        .replace(
            "class Billet(sk.Sample):",
            "import functools\n\n\n@functools.lru_cache(maxsize=None)\ndef scaled(x):\n    \
             return x * 1.0\n\n\nclass Billet(sk.Sample):",
        )
        .replace(
            "return self.diameter.value / self.volume.value",
            "return scaled(self.diameter.value / self.volume.value)",
        );
    project(&scratch, &cached);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    scratch.write("project/model.py", &cached.replace("x * 1.0", "x * 2.0"));
    let changed = status(&scratch);
    assert_eq!(
        said(&changed, "ductility", "formula changed"),
        2,
        "{}",
        out(&changed)
    );
}

#[test]
fn a_global_a_formula_fills_on_first_use_stales_nothing() {
    // `global _SCALE; _SCALE = load()`: read by the object it held, the new
    // object a formula bound was read full, and every value said changed.
    let scratch = Scratch::new("lazy-global");
    let lazy = BILLET
        .replace(
            "class Billet(sk.Sample):",
            "_SCALE = None\n\n\ndef scale():\n    global _SCALE\n    if _SCALE is None:\n        \
             _SCALE = {\"factor\": 1.0}\n    return _SCALE[\"factor\"]\n\n\nclass Billet(sk.Sample):",
        )
        .replace(
            "return self.diameter.value / self.volume.value",
            "return self.diameter.value / self.volume.value * scale()",
        );
    project(&scratch, &lazy);
    let first = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let current = status(&scratch);
    assert!(
        out(&current).contains("every computed value in 2 samples is current"),
        "{}",
        out(&current)
    );
}

#[test]
fn a_computation_from_above_is_kept_in_the_project_below() {
    // `compute --write` over a folder holding a project below computed its
    // samples, and their change was kept in no history.
    let scratch = Scratch::new("compute-from-above");
    bare_project(
        &scratch,
        "schema_version = 1\n[collection]\nrecursive = true\n[model]\npath = \"model.py\"\n\
         python = \".venv\"\n",
        BILLET,
    );
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.write(
        "project/samples/sub/c3.md",
        "---\nschema_version: 1\nname: C3\nproperties:\n  diameter: {v: 9.0, unit: g}\n  \
         volume: {v: 3.0, unit: L}\n---\n",
    );
    let set = run(
        &scratch,
        &["samples/sub/c3.md", "set", "diameter=8", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    scratch.write(
        "project/samples/sub/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"../../model.py\"\npython = \"../../.venv\"\n",
    );
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    let log = out(&run(&scratch, &["samples/sub", "log"]));
    let newest = log
        .lines()
        .find(|line| line.trim_start().starts_with("1 "))
        .unwrap_or_default();
    assert!(newest.contains("compute --write"), "{log}");
    assert!(!log.contains("outside SampleKit"), "{log}");
}

#[test]
fn readings_never_computed_owe_their_value_in_a_written_table() {
    // Readings with no statistic computed were said owing *diameter's
    // uncertainty* alone, where the file wrote the value empty too.
    let scratch = Scratch::new("readings-owed");
    project(&scratch, DOPED);
    let text = scratch.read("project/samples/c1.md");
    scratch.write(
        "project/samples/c1.md",
        &text.replacen(
            "diameter: {v: 12.0, unit: g}",
            "diameter: {readings: [11.0, 12.0, 13.0], unit: g}",
            1,
        ),
    );
    let previewed = run(&scratch, &["samples", "-c", "name,diameter", "-o", "t.csv"]);
    let said = out(&previewed);
    assert!(said.contains("C1 diameter\n"), "{said}");
    assert!(!said.contains("diameter's uncertainty"), "{said}");
}

#[test]
fn a_first_trace_says_the_values_it_takes_as_computed() {
    // A model changed before a project's first command was never seen
    // changed: its old values were called current, and nothing said so.
    let scratch = Scratch::new("first-trace-said");
    project(&scratch, BILLET);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    // The project as it arrives elsewhere: its values, and no history.
    fs::remove_dir_all(scratch.at("project/.samplekit")).unwrap();
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        err(&set).contains("taken as computed by the model as it is now")
            && err(&set).contains("compute --rerun --write"),
        "{}",
        err(&set)
    );
    let again = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=14", "--write"],
    );
    assert!(
        !err(&again).contains("taken as computed"),
        "{}",
        err(&again)
    );
}

#[test]
fn a_first_trace_taken_by_compute_says_so() {
    // The commonest first write after an upgrade, a compute with nothing to
    // compute, took the trace without a word.
    let scratch = Scratch::new("first-trace-compute");
    project(&scratch, BILLET);
    assert_eq!(code(&run(&scratch, &["samples", "compute", "--write"])), 0);
    fs::remove_dir_all(scratch.at("project/.samplekit")).unwrap();
    let computed = run(&scratch, &["samples", "compute", "--write"]);
    assert_eq!(code(&computed), 0, "{}", err(&computed));
    assert!(
        err(&computed).contains("taken as computed by the model as it is now"),
        "{}",
        err(&computed)
    );
}

/// Started over a folder of changes waiting, the worker computes from what
/// waits and saves into it: the sample's file is untouched until the changes
/// are written.
#[test]
fn a_worker_over_what_waits_computes_from_it_and_adds_to_it() {
    use samplekit::format::{document, stage};
    let scratch = Scratch::new("waiting-worker");
    let project = project(&scratch, BILLET);
    let path = scratch.at("project/samples/c1.md");
    let on_disk = fs::read_to_string(&path).unwrap();
    let folder = scratch.at("waiting");
    stage::begin(&folder, Some(&project)).unwrap();
    // The diameter changed, waiting: 12 becomes 20.
    let waiting = on_disk.replace("v: 12.0", "v: 20.0");
    stage::save(&path, &waiting).unwrap();
    let config = loaded(&scratch);
    let python = runtime::interpreter_for(&config, &project).unwrap();
    let mut worker = Worker::start_waiting(&python, Some(&folder)).unwrap();
    let report = worker
        .compute(
            &request(&scratch, "project/samples/c1.md"),
            true,
            &mut |_| {},
        )
        .unwrap();
    drop(worker);
    assert!(report.computed >= 2, "{report:?}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        on_disk,
        "the file is untouched"
    );
    stage::refresh(&folder).unwrap();
    let computed = document::read_text(&path).unwrap();
    stage::end(&folder);
    // 20 / 4: computed from what waited, not from the file's 12.
    assert!(computed.contains("v: 5.0"), "{computed}");
}

#[test]
fn the_documentations_templates_make_a_project_that_computes() {
    // The four templates the guide offers, each placed where the guide says:
    // the configuration as `.samplekitrc`, the model where `[model] path`
    // names it, the sample in `samples/`, the script in `scripts/`.
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/templates");
    let template = |name: &str| fs::read_to_string(templates.join(name)).unwrap();
    let scratch = Scratch::new("templates");
    scratch.write("project/.samplekitrc", &template("samplekitrc.toml"));
    let loaded = loaded(&scratch);
    let model = loaded.model().expect("the template declares a model");
    let model_path = scratch.at("project").join(&model.path);
    assert!(
        model_path.starts_with(scratch.at("project")),
        "{}",
        model_path.display()
    );
    fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    fs::write(&model_path, template("model.py")).unwrap();
    scratch.write("project/samples/specimen-01.md", &template("sample.md"));
    scratch.write("project/scripts/script.py", &template("script.py"));
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    let said = |output: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };

    // Valid as written, the sample found by the collection.
    let validated = run(&scratch, &["validate"]);
    assert!(validated.status.success(), "{}", said(&validated));
    assert!(
        said(&validated).contains("1 sample · 0 defects"),
        "{}",
        said(&validated)
    );

    // The computed value and the table's computed column, written.
    let computed = run(&scratch, &["compute", "--write"]);
    assert!(computed.status.success(), "{}", said(&computed));
    let shown = run(&scratch, &["samples/specimen-01.md", "view"]);
    let view = said(&shown);
    assert!(view.contains("area       200.30 ± 0.36 mm²"), "{view}");
    assert!(view.contains("stress [MPa]"), "{view}");
    assert!(view.contains("20.9"), "{view}");
    assert!(view.contains("49.1"), "{view}");
    let written = scratch.read("project/samples/specimen-01.md");
    assert_eq!(written.matches("stress: {v: ").count(), 5, "{written}");
    let status = run(&scratch, &["status"]);
    assert!(status.status.success(), "{}", said(&status));
    assert!(
        said(&status).contains("every computed value in 1 sample is current"),
        "{}",
        said(&status)
    );

    // The model's script, with its option, run by the command line.
    let ran = run(
        &scratch,
        &["run", "classify", "--set", "limit=150", "--write"],
    );
    assert!(ran.status.success(), "{}", said(&ran));
    assert!(
        scratch
            .read("project/samples/specimen-01.md")
            .contains("size: large"),
        "{}",
        said(&ran)
    );

    // The script of one's own, with its option, run by the project's Python.
    let script = Command::new(scratch.at("project/.venv/bin/python"))
        .args(["scripts/script.py", "--limit", "500"])
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(script.status.success(), "{}", said(&script));
    assert!(
        said(&script).contains("specimen-01    200.30 ± 0.36 mm²  small"),
        "{}",
        said(&script)
    );
}
