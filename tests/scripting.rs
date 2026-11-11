//! The tests of `scripting`.
//!
//! A script is the model's code, run by the project's Python, so these tests
//! run the **compiled binary**, and the workbench, against the repository's
//! `.venv`, where `maturin develop` installed the package.

// Runs a project's Python through Unix paths.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use samplekit::presentation::scripting::{ScriptRequest, options_of, request_json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-script-{name}-{}", std::process::id()));
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The repository's environment, with the package in it.
fn venv() -> PathBuf {
    let venv = Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    let python = venv.join("bin").join("python");
    assert!(
        python.is_file(),
        "these tests need the repository's .venv, with the package built by `maturin develop`"
    );
    let found = Command::new(&python)
        .args(["-c", "import samplekit._script"])
        .output()
        .expect("the environment's python runs");
    assert!(
        found.status.success(),
        "these tests need the package in .venv: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    venv
}

/// A model with a script of one sample, one of the selection, one that saves
/// twice and makes a sample, one returning each kind of result, and one that
/// raises.
const MODEL: &str = r#"import samplekit as sk


class Billet(sk.Sample):
    @sk.script
    def rescale(self, factor: float = 0.5):
        """Scale the diameter by a factor.

        Said on the second paragraph, which the description leaves out.
        """
        print("scaling", self.name)
        self.diameter.value = self.diameter.value * factor

    @sk.script
    @classmethod
    def heaviest(cls, samples, count: int = 1, loud: bool = False):
        """The samples with the most diameter."""
        if loud:
            print("loud")
        return sk.SampleList(sorted(samples, key=lambda s: -s.diameter.value)[:count])

    @sk.script
    def twice(self, alloy: str):
        self.alloy = alloy
        self.save()
        self.diameter.value = 1.0
        self.save()
        fresh = sk.Sample.new(self.path.parent / "fresh.md", name="fresh")
        fresh.save()

    @sk.script
    @classmethod
    def shown(cls, samples):
        first = samples[0]
        table = sk.Table({"n": sk.Property(), "diameter": sk.Property(unit="g")}, index="n",
                         rows=[{"n": i, "diameter": s.diameter.value} for i, s in enumerate(samples)])
        return [first.diameter, table, first, samples.stats("diameter"), 42]

    @sk.script
    def asked(self):
        """Scale the diameter by the factor asked."""
        factor = float(input("Factor? "))
        self.diameter.value = self.diameter.value * factor

    @sk.script
    def broken(self):
        self.diameter.value = 0.0
        return 1 / 0
"#;

const RC: &str = "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
[style.math]\n[unit.g]\nmath = '\\si{\\gram}'\n";

fn project(scratch: &Scratch) -> PathBuf {
    scratch.write("project/.samplekitrc", RC);
    scratch.write("project/model.py", MODEL);
    for (name, diameter) in [("c1", 12.0), ("c2", 10.0)] {
        scratch.write(
            &format!("project/samples/{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {}\nalloy: cobalt\nproperties:\n  \
                 diameter: {{v: {diameter:?}, u: 0.1, unit: g}}\n---\n",
                name.to_uppercase()
            ),
        );
    }
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.at("project")
}

/// The binary, in the project, `input` its standard input.
fn run(scratch: &Scratch, arguments: &[&str], input: &str) -> Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("NO_COLOR", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
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
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
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

fn diameter_of(scratch: &Scratch, name: &str) -> String {
    let text = fs::read_to_string(scratch.at(&format!("project/samples/{name}.md"))).unwrap();
    text.lines()
        .find(|line| line.trim_start().starts_with("diameter:"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[test]
fn list_scripts_names_each_with_what_it_acts_on_and_its_options() {
    let scratch = Scratch::new("list");
    project(&scratch);
    let listed = run(&scratch, &["list", "scripts"], "");
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let text = out(&listed);
    assert!(
        text.contains(
            "rescale    on each sample\n    Scale the diameter by a factor.\n    \
             --set factor=NUMBER   0.5 by default\n"
        ),
        "{text}"
    );
    assert!(!text.contains("second paragraph"), "{text}");
    assert!(
        text.contains("heaviest   on the selection\n    The samples with the most diameter.\n"),
        "{text}"
    );
    assert!(
        text.contains("    --set loud=YES|NO     false by default"),
        "{text}"
    );
    // An option with no default is required.
    assert!(
        text.contains("    --set alloy=TEXT      required"),
        "{text}"
    );
}

#[test]
fn list_figures_and_list_scripts_share_one_layout() {
    // A figure said where it was declared, and nothing of what it draws; a
    // model's figure its docstring now, and what it acts on, as a script.
    let scratch = Scratch::new("list-figures");
    project(&scratch);
    let model = fs::read_to_string(scratch.at("project/model.py")).unwrap();
    fs::write(
        scratch.at("project/model.py"),
        model.replace(
            "class Billet(sk.Sample):\n",
            "class Billet(sk.Sample):\n    @sk.figure\n    def bars(self, ax):\n        \
             \"The diameter, as a bar.\"\n        ax.bar([\"diameter\"], [self.diameter.value])\n\n",
        ),
    )
    .unwrap();
    let listed = run(&scratch, &["list", "figures"], "");
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("bars   on each sample\n    The diameter, as a bar.\n"),
        "{}",
        out(&listed)
    );
}

#[test]
fn a_scripts_changes_are_shown_and_written_on_yes_alone() {
    let scratch = Scratch::new("asked");
    project(&scratch);
    // No, the default: shown, nothing written.
    let declined = run(
        &scratch,
        &["samples", "run", "rescale", "--set", "factor=0.25"],
        "\n",
    );
    assert_eq!(code(&declined), 0, "{}", err(&declined));
    let text = out(&declined);
    assert!(text.contains("scaling C1"), "{text}");
    assert!(text.contains("samples/c1.md"), "{text}");
    assert!(text.contains("12.00 ± 0.10 g  →  3.00 ± 0.10 g"), "{text}");
    assert!(text.contains("nothing written"), "{text}");
    assert!(
        err(&declined).contains("apply these changes? [y/N] n"),
        "{}",
        err(&declined)
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
    // Yes: written, in one snapshot of the history.
    let applied = run(&scratch, &["samples", "run", "rescale"], "y\n");
    assert_eq!(code(&applied), 0, "{}", err(&applied));
    assert!(
        out(&applied).contains("written: 2 files"),
        "{}",
        out(&applied)
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 6.0, u: 0.1, unit: g}"
    );
    assert_eq!(
        diameter_of(&scratch, "c2"),
        "diameter: {v: 5.0, u: 0.1, unit: g}"
    );
    let log = run(&scratch, &["log"], "");
    assert!(
        out(&log).contains("samplekit samples run rescale --write"),
        "{}",
        out(&log)
    );
}

#[test]
fn write_applies_a_scripts_changes_without_asking() {
    let scratch = Scratch::new("write");
    project(&scratch);
    let applied = run(
        &scratch,
        &["samples/c1.md", "run", "rescale", "--write"],
        "",
    );
    assert_eq!(code(&applied), 0, "{}", err(&applied));
    assert!(
        !err(&applied).contains("apply these changes?"),
        "{}",
        err(&applied)
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 6.0, u: 0.1, unit: g}"
    );
    assert_eq!(
        diameter_of(&scratch, "c2"),
        "diameter: {v: 10.0, u: 0.1, unit: g}"
    );
}

#[test]
fn a_script_on_the_selection_receives_it_whole() {
    let scratch = Scratch::new("selection");
    project(&scratch);
    let ran = run(
        &scratch,
        &[
            "samples", "run", "heaviest", "--set", "count=2", "--set", "loud=yes",
        ],
        "",
    );
    assert_eq!(code(&ran), 0, "{}", err(&ran));
    // A SampleList is its paths; nothing changed is nothing asked.
    assert_eq!(out(&ran), "loud\nsamples/c1.md\nsamples/c2.md\n");
    assert!(!err(&ran).contains("apply these changes?"), "{}", err(&ran));
    // The path first, as every command takes it.
    let first = run(&scratch, &["samples", "run", "heaviest"], "");
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert_eq!(out(&first), "samples/c1.md\n");
}

#[test]
fn a_scripts_options_are_read_as_its_signature_types_them() {
    let scratch = Scratch::new("options");
    project(&scratch);
    for (arguments, said) in [
        (
            &["samples", "run", "rescale", "--set", "factor=abc"][..],
            "--set factor=abc: a number",
        ),
        (
            &["samples", "run", "heaviest", "--set", "count=1.5"][..],
            "--set count=1.5: an integer",
        ),
        (
            &["samples", "run", "heaviest", "--set", "loud=maybe"][..],
            "--set loud=maybe: a yes or no, true or false",
        ),
        (
            &["samples", "run", "rescale", "--set", "factr=2"][..],
            "'rescale' takes no option 'factr' — its options: factor=0.5",
        ),
        (
            &["samples", "run", "twice"][..],
            "'twice' needs alloy: --set alloy=VALUE",
        ),
        (
            &["samples", "run", "rescale", "--set", "factor"][..],
            "--set factor: an option is NAME=VALUE",
        ),
    ] {
        let refused = run(&scratch, arguments, "");
        assert_eq!(code(&refused), 1, "{arguments:?}: {}", err(&refused));
        assert!(
            err(&refused).contains(said),
            "{arguments:?}: {}",
            err(&refused)
        );
    }
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
}

#[test]
fn an_unknown_script_names_the_nearest_and_those_there_are() {
    let scratch = Scratch::new("unknown");
    project(&scratch);
    let refused = run(&scratch, &["samples", "run", "rescal"], "");
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    let said = err(&refused);
    assert!(
        said.contains("no script named 'rescal': did you mean 'rescale'?"),
        "{said}"
    );
    assert!(
        said.contains("the model's scripts: rescale, heaviest, twice, shown, asked, broken"),
        "{said}"
    );
    // A path where the name goes is said as one.
    let path = run(&scratch, &["run", "samples"], "");
    assert_eq!(code(&path), 1, "{}", err(&path));
    assert!(err(&path).contains("'samples' is a path"), "{}", err(&path));
}

#[test]
fn a_script_that_raises_exits_two_with_its_traceback_and_writes_nothing() {
    let scratch = Scratch::new("raises");
    project(&scratch);
    let failed = run(&scratch, &["samples", "run", "broken", "--write"], "");
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    let said = err(&failed);
    assert!(said.contains("ZeroDivisionError"), "{said}");
    assert!(said.contains("model.py\", line"), "{said}");
    // The script's own frames, none of the runner's.
    assert!(!said.contains("_script.py"), "{said}");
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
}

#[test]
fn a_scripts_saves_are_held_and_shown_until_applied() {
    // Two saves of one sample and a sample made: each held, the second over
    // the first, and nothing on the disk until yes.
    let scratch = Scratch::new("held");
    project(&scratch);
    let declined = run(
        &scratch,
        &["samples/c1.md", "run", "twice", "--set", "alloy=chrome"],
        "n\n",
    );
    assert_eq!(code(&declined), 0, "{}", err(&declined));
    let text = out(&declined);
    assert!(text.contains("alloy"), "{text}");
    assert!(text.contains("→  chrome"), "{text}");
    assert!(text.contains("12.00 ± 0.10 g  →  1.00 ± 0.10 g"), "{text}");
    assert!(text.contains("samples/fresh.md   new sample"), "{text}");
    assert!(!scratch.at("project/samples/fresh.md").exists());
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
    let applied = run(
        &scratch,
        &["samples/c1.md", "run", "twice", "--set", "alloy=chrome"],
        "y\n",
    );
    assert_eq!(code(&applied), 0, "{}", err(&applied));
    assert!(scratch.at("project/samples/fresh.md").exists());
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 1.0, u: 0.1, unit: g}"
    );
}

#[test]
fn what_a_script_returns_is_shown_as_its_kind_is() {
    let scratch = Scratch::new("returns");
    project(&scratch);
    let ran = run(
        &scratch,
        &["samples", "run", "shown", "--style", "math"],
        "",
    );
    assert_eq!(code(&ran), 0, "{}", err(&ran));
    let text = out(&ran);
    // A property in the style, a table and a sample as `view` shows them, a
    // summary as `--summary` writes it, anything else as `str()`.
    assert!(text.contains("\\si{\\gram}"), "{text}");
    assert!(text.contains("diameter [\\si{\\gram}]"), "{text}");
    assert!(text.contains("C1\n  tags"), "{text}");
    assert!(text.contains("median"), "{text}");
    assert!(text.ends_with("42\n"), "{text}");
    // A style nobody declares is refused before the script runs.
    let refused = run(
        &scratch,
        &["samples", "run", "shown", "--style", "nope"],
        "",
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("no style 'nope'"),
        "{}",
        err(&refused)
    );
}

#[test]
fn the_request_is_one_json_object_and_options_are_name_value() {
    let request = ScriptRequest {
        script: "rescale".to_string(),
        samples: vec![PathBuf::from("samples/c1.md")],
        options: options_of(&["factor=0.25".to_string()]).unwrap(),
        style: Some("math".to_string()),
        timeout: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&request_json(&request, Path::new("/tmp/reply.json"))).unwrap();
    assert_eq!(json["script"], "rescale");
    assert_eq!(json["samples"][0], "samples/c1.md");
    assert_eq!(json["options"]["factor"], "0.25");
    assert_eq!(json["style"], "math");
    assert_eq!(json["reply"], "/tmp/reply.json");
    assert!(options_of(&["factor".to_string()]).is_err());
    assert!(options_of(&["=1".to_string()]).is_err());
    assert_eq!(
        options_of(&["a=1".to_string(), "a=2".to_string()]).unwrap_err(),
        "--set a is given twice"
    );
    // A value may hold `=`.
    assert_eq!(
        options_of(&["filter=a=b".to_string()]).unwrap()["filter"],
        "a=b"
    );
}

#[test]
fn p_runs_a_script_in_the_workbench_and_asks_before_writing() {
    use samplekit::tui::model::{Key, Mode, Workbench};
    let scratch = Scratch::new("workbench");
    project(&scratch);
    // SAFETY: this binary's one workbench test reads it, set before it opens.
    unsafe { std::env::set_var("SAMPLEKIT_STATE_DIR", scratch.at("state")) };
    let mut workbench = Workbench::open(&scratch.at("project/samples")).unwrap();
    // `p` offers the model's scripts first, under their heading, the cursor
    // on the first.
    workbench.key(Key::Char('p'));
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("the scripts offered: {}", workbench.message);
    };
    assert_eq!(items[0].0, "scripts", "{items:?}");
    assert!(items[1].0.starts_with("  rescale"), "{items:?}");
    assert!(
        items[1].0.contains("each sample") && items[1].0.contains("Scale the diameter"),
        "{items:?}"
    );
    assert!(items.iter().any(|(item, _)| item == "figures"), "{items:?}");
    assert_eq!(*cursor, 1);
    // Its options asked, with their defaults, then run beside the screen.
    workbench.key(Key::Enter);
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "{}",
        workbench.message
    );
    workbench.key(Key::Enter);
    let started = std::time::Instant::now();
    while workbench.scripting.is_some() {
        workbench.tick();
        assert!(started.elapsed().as_secs() < 60, "the script ends");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("its changes asked about: {}", workbench.message);
    };
    assert_eq!(title, "run rescale");
    assert!(
        lines.iter().any(|line| line.contains("scaling C1")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("→  6.00 ± 0.10 g")),
        "{lines:?}"
    );
    // Ended, said so: `running 'rescale'` stayed under its preview.
    assert_eq!(workbench.message, "'rescale' ran: 2 files changed");
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
    workbench.key(Key::Char('y'));
    // What it changed waits with the rest, until `w` writes it.
    assert!(
        workbench.message.contains("'rescale' changed 2 files")
            && workbench.message.contains("not written yet"),
        "{}",
        workbench.message
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
    workbench.key(Key::Char('w'));
    workbench.key(Key::Char('y'));
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 6.0, u: 0.1, unit: g}"
    );
    // A question is asked in the script's window, which shows what it prints
    // as it prints it, and answered on its foot's line.
    workbench.key(Key::Char('p'));
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("the scripts offered: {}", workbench.message);
    };
    let asked = items
        .iter()
        .position(|(item, _)| item.starts_with("  asked"))
        .expect("asked offered");
    for _ in *cursor..asked {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Enter);
    if matches!(workbench.mode, Mode::Prompt { .. }) {
        workbench.key(Key::Enter);
    }
    // Asked once a sample, each answered when it is asked.
    let started = std::time::Instant::now();
    let mut answered = 0;
    while workbench.scripting.is_some() {
        workbench.tick();
        let asking = workbench
            .scripting
            .as_ref()
            .map_or(0, |running| running.printed.matches("Factor? ").count());
        if asking > answered {
            assert!(
                matches!(workbench.mode, Mode::Script { .. }),
                "{}",
                workbench.message
            );
            for key in "0.5".chars() {
                workbench.key(Key::Char(key));
            }
            workbench.key(Key::Enter);
            answered += 1;
        }
        assert!(started.elapsed().as_secs() < 60, "the script ends");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(answered, 2);
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("its changes asked about: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line == "asked Factor?  0.5"),
        "{lines:?}"
    );
    workbench.key(Key::Char('y'));
    assert!(
        workbench
            .message
            .contains("'asked' changed 2 files — answered 'Factor?' 0.5, 'Factor?' 0.5"),
        "{}",
        workbench.message
    );
    workbench.key(Key::Char('w'));
    workbench.key(Key::Char('y'));
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 3.0, u: 0.1, unit: g}"
    );
}

#[test]
fn a_scripts_question_is_answered_in_the_terminal_and_kept() {
    let scratch = Scratch::new("answered");
    project(&scratch);
    let written = run(
        &scratch,
        &["samples/c1.md", "run", "asked", "--write"],
        "0.5\n",
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(out(&written).starts_with("Factor? "), "{}", out(&written));
    assert!(
        out(&written).contains("  asked Factor?  0.5\n"),
        "{}",
        out(&written)
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 6.0, u: 0.1, unit: g}"
    );
    let log = run(&scratch, &["log"], "");
    assert!(
        out(&log).contains("samplekit samples/c1.md run asked --write — answered 'Factor?' 0.5"),
        "{}",
        out(&log)
    );
}

#[test]
fn a_scripts_question_with_nothing_to_answer_it_fails_saying_so() {
    let scratch = Scratch::new("unanswered");
    project(&scratch);
    let failed = run(&scratch, &["samples/c1.md", "run", "asked", "--write"], "");
    assert_eq!(code(&failed), 2, "{}", err(&failed));
    assert!(
        err(&failed).contains(
            "the script asked 'Factor?', and no terminal can answer it: --set NAME=VALUE \
             gives a script its values"
        ),
        "{}",
        err(&failed)
    );
    assert_eq!(
        diameter_of(&scratch, "c1"),
        "diameter: {v: 12.0, u: 0.1, unit: g}"
    );
}
