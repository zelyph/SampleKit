//! The tests of `plotting`.
//!
//! A figure is drawn by matplotlib in the project's Python, so these tests run
//! the **compiled binary** against the repository's `.venv`, where `maturin
//! develop` installed the package and matplotlib beside it. Every figure is
//! written to a file: a test opens no window.

// Runs a project's Python through Unix paths and signals.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use samplekit::config::project_config::{FigureDeclaration, FigureKind};
use samplekit::presentation::plotting::{
    FigureChoice, FigureOverrides, FigureRequest, request_json,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-plot-{name}-{}", std::process::id()));
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

/// The repository's environment, with the package and matplotlib in it.
fn venv() -> PathBuf {
    let venv = Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    let python = venv.join("bin").join("python");
    assert!(
        python.is_file(),
        "these tests need the repository's .venv, with the package built by `maturin develop`"
    );
    let found = Command::new(&python)
        .args(["-c", "import matplotlib, samplekit._figure"])
        .output()
        .expect("the environment's python runs");
    assert!(
        found.status.success(),
        "these tests need matplotlib and the package in .venv: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    venv
}

/// A model whose import leaves a mark, with a figure of one sample, one of the
/// collection, and one that raises.
const MODEL: &str = r#"import os

import samplekit as sk

open(os.path.join(os.path.dirname(__file__), "imported"), "w").close()
print("loading the adjustment")


class Billet(sk.Sample):
    @sk.figure
    def mass_bar(self, ax):
        ax.bar(["diameter"], [self.diameter.value])

    @sk.figure(subplots=(1, 2))
    @classmethod
    def overview(cls, samples, axes):
        names = [sample.name for sample in samples]
        axes[0].bar(names, [sample.diameter.value for sample in samples])
        axes[1].set_title(",".join(names))
        open(os.path.join(os.path.dirname(__file__), "drew"), "w").write(",".join(names))

    @sk.figure
    def broken(self, ax):
        return 1 / 0
"#;

const RC: &str = "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
[figure.diameter_volume]\nx = \"volume\"\ny = \"diameter\"\ngroup = \"alloy\"\n\
[figure.cobalt]\nx = \"volume\"\ny = \"diameter\"\nquery = \"cobalt\"\n\
[query.cobalt]\nfilter = \"alloy == cobalt\"\n\
[style.figure]\n[style.math]\n\
[unit.g]\nfigure = '$\\mathrm{g}$'\nmath = '\\si{\\gram}'\n\
[property.diameter]\nsymbol = \"m\"\nsymbol_figure = '$m$'\n";

fn project(scratch: &Scratch) -> PathBuf {
    scratch.write("project/.samplekitrc", RC);
    scratch.write("project/model.py", MODEL);
    for (name, diameter, alloy, hv) in [("c1", 12.0, "cobalt", 5.1), ("c2", 10.0, "chrome", 5.3)] {
        scratch.write(
            &format!("project/samples/{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {}\nalloy: {alloy}\nproperties:\n  \
                 diameter: {{v: {diameter:?}, u: 0.1, unit: g}}\n  volume: {{v: 4.0, unit: L}}\n\
                 tables:\n  m:\n    index: T\n    columns:\n      T: {{unit: C}}\n      \
                 hv: {{}}\n    rows:\n      - {{T: 20.0, hv: {{v: {hv}, u: 0.05}}}}\n---\n",
                name.to_uppercase()
            ),
        );
    }
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.at("project")
}

/// The binary, in the project, with nobody at a terminal and no display over
/// SSH unless a test says so.
fn run(scratch: &Scratch, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("NO_COLOR", "1")
        .env("MPLBACKEND", "Agg")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
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

fn starts_with(path: &Path, magic: &[u8]) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.starts_with(magic))
}

#[test]
fn a_declared_figure_is_written_to_a_file() {
    let scratch = Scratch::new("declared");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "out.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.png"), b"\x89PNG"));
    // A declared figure runs no code of the project's.
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn an_output_is_previewed_until_write() {
    let scratch = Scratch::new("preview");
    project(&scratch);
    // An interpreter that cannot start: a preview must not need one.
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let output = run(
        &scratch,
        &["samples", "plot", "diameter_volume", "-o", "out.png"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("out.png"), "{}", out(&output));
    assert!(out(&output).contains("nothing written"), "{}", out(&output));
    assert!(!scratch.at("project/out.png").exists());
}

#[test]
fn axes_given_on_the_command_line_draw_without_a_declaration() {
    let scratch = Scratch::new("axes");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples", "plot", "-x", "alloy", "-y", "diameter", "--kind", "bar", "--group", "name",
            "-o", "out.svg", "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.svg"), b"<?xml"));
}

#[test]
fn grouped_bars_on_a_numeric_x_are_refused_and_a_text_y_lies_them_down() {
    // Side by side, each bar stood off its x and was read at a value it did
    // not have.
    let scratch = Scratch::new("bars-numeric");
    project(&scratch);
    let refused = run(
        &scratch,
        &[
            "samples", "plot", "-x", "volume", "-y", "diameter", "--kind", "bar", "--group",
            "alloy", "-o", "no.svg", "--write",
        ],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("--kind scatter"),
        "{}",
        err(&refused)
    );
    assert!(!scratch.at("project/no.svg").exists());
    // A text y and a number x: horizontal bars, one per alloy.
    let lying = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "alloy", "--kind", "bar", "-o", "h.svg",
            "--write",
        ],
    );
    assert_eq!(code(&lying), 0, "{}", err(&lying));
    let drawn = fs::read_to_string(scratch.at("project/h.svg")).unwrap();
    assert!(
        drawn.contains("cobalt") && drawn.contains("chrome"),
        "the alloys label y"
    );
}

#[test]
fn a_model_figure_draws_one_sample() {
    let scratch = Scratch::new("model-one");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples/c1.md",
            "plot",
            "mass_bar",
            "-o",
            "one.pdf",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/one.pdf"), b"%PDF"));
}

#[test]
fn a_collection_figure_draws_the_selection() {
    let scratch = Scratch::new("model-many");
    project(&scratch);
    let output = run(
        &scratch,
        &["samples", "plot", "overview", "-o", "all.png", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let drew = fs::read_to_string(scratch.at("project/drew")).unwrap();
    assert_eq!(drew, "C1,C2");
}

// Only Linux refuses: over SSH a Mac still has its own screen, and the figure
// would open there and wait.
#[test]
#[cfg(target_os = "linux")]
fn a_window_over_ssh_without_a_display_is_refused() {
    let scratch = Scratch::new("ssh");
    project(&scratch);
    let over_ssh = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .current_dir(scratch.at("project"))
            .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
            .env("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let window = over_ssh(&["samples", "plot", "diameter_volume"]);
    assert_eq!(code(&window), 1, "{}", err(&window));
    assert!(err(&window).contains("SSH"), "{}", err(&window));
    assert!(err(&window).contains("-o"), "{}", err(&window));
    let file = over_ssh(&[
        "samples",
        "plot",
        "diameter_volume",
        "-o",
        "ssh.png",
        "--write",
    ]);
    assert_eq!(code(&file), 0, "{}", err(&file));
    assert!(starts_with(&scratch.at("project/ssh.png"), b"\x89PNG"));
}

#[test]
fn an_unknown_figure_names_those_that_exist() {
    let scratch = Scratch::new("unknown");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volum",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    for name in ["diameter_volume", "mass_bar", "overview"] {
        assert!(err(&output).contains(name), "{}", err(&output));
    }
}

#[test]
fn a_figure_that_raises_exits_two_with_its_traceback() {
    let scratch = Scratch::new("raises");
    project(&scratch);
    let output = run(
        &scratch,
        &["samples/c1.md", "plot", "broken", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("ZeroDivisionError"),
        "{}",
        err(&output)
    );
    assert!(err(&output).contains("model.py"), "{}", err(&output));
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn a_one_sample_figure_over_several_with_an_output_is_refused() {
    let scratch = Scratch::new("one-over-several");
    project(&scratch);
    let output = run(
        &scratch,
        &["samples", "plot", "mass_bar", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("one sample"), "{}", err(&output));
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn list_figures_names_the_model_figures_where_it_may_run() {
    // Nothing to list is said, not printed as nothing.
    let empty = Scratch::new("list-empty");
    empty.write("project/.samplekitrc", "schema_version = 1\n");
    empty.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: 1.0\n---\n",
    );
    let said = run(&empty, &["samples", "list", "figures"]);
    assert_eq!(code(&said), 0, "{}", err(&said));
    assert!(
        out(&said).contains("no figure is declared"),
        "{}",
        out(&said)
    );

    let scratch = Scratch::new("list");
    project(&scratch);
    // The model is read to list its figures, nothing asked.
    let after = run(&scratch, &["samples", "list", "figures"]);
    assert_eq!(code(&after), 0, "{}", err(&after));
    for name in ["diameter_volume", "mass_bar", "overview", "broken"] {
        assert!(out(&after).contains(name), "{}", out(&after));
    }
    // What the model prints as it loads is not a figure.
    assert!(!out(&after).contains("adjustment"), "{}", out(&after));
}

#[test]
fn the_request_is_one_json_object() {
    let named = FigureRequest {
        choice: FigureChoice::Named {
            name: "vfi".to_string(),
            model: false,
        },
        samples: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
        output: Some(PathBuf::from("vfi.pdf")),
        overrides: FigureOverrides {
            title: Some("VFI".to_string()),
            style: Some("figure".to_string()),
            ..FigureOverrides::default()
        },
        project_style: true,
        said: None,
        snapshot: None,
        workbench: false,
        accept_outdated: false,
    };
    let body: serde_json::Value = serde_json::from_str(&request_json(&named)).unwrap();
    assert_eq!(body["figure"], "vfi");
    assert_eq!(body["model"], false);
    assert_eq!(body["samples"], serde_json::json!(["a.md", "b.md"]));
    assert_eq!(body["output"], "vfi.pdf");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["overrides"]["title"], "VFI");
    assert_eq!(body["overrides"]["style"], "figure");
    assert!(body["overrides"]["x_label"].is_null());
    assert!(body["overrides"]["group"].is_null());
    assert_eq!(body["project_style"], true);
    let axes = FigureRequest {
        choice: FigureChoice::AdHoc(Box::new(FigureDeclaration {
            name: String::new(),
            kind: FigureKind::Line,
            x: "period".to_string(),
            y: "vfi".to_string(),
            group: Some("alloy".to_string()),
            query: None,
            title: None,
            x_label: None,
            y_label: None,
            style: None,
            x_limits: None,
            y_limits: None,
            x_scale: None,
            y_scale: None,
            aspect: None,
            legend: None,
            figsize: None,
            matplotlib: Default::default(),
        })),
        samples: Vec::new(),
        output: None,
        overrides: FigureOverrides::default(),
        project_style: false,
        said: None,
        snapshot: None,
        workbench: false,
        accept_outdated: false,
    };
    let body: serde_json::Value = serde_json::from_str(&request_json(&axes)).unwrap();
    assert_eq!(body["axes"]["kind"], "line");
    assert_eq!(body["axes"]["group"], "alloy");
    assert_eq!(body["model"], false);
    assert!(body["output"].is_null());
}

#[test]
fn a_table_cell_is_an_axis_across_samples() {
    // A cell is a field like any other, a point per sample.
    let scratch = Scratch::new("cell");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "m.hv[20]", "-o", "cell.png", "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/cell.png"), b"\x89PNG"));
    let nowhere = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "m.hv[99]", "-o", "x.png", "--write",
        ],
    );
    assert_eq!(code(&nowhere), 1, "{}", err(&nowhere));
    assert!(!scratch.at("project/x.png").exists());
    let misspelt = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diametr", "-y", "volume", "-o", "x.png", "--write",
        ],
    );
    assert_eq!(code(&misspelt), 1, "{}", err(&misspelt));
    assert!(err(&misspelt).contains("diameter"), "{}", err(&misspelt));
}

#[test]
fn a_title_labels_and_a_style_are_given_on_the_command_line() {
    let scratch = Scratch::new("labels");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--title",
            "Diameters",
            "--x-label",
            "$V$",
            "--style",
            "figure",
            "-o",
            "l.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/l.png"), b"\x89PNG"));
    let unknown = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--style",
            "figur",
            "-o",
            "u.png",
            "--write",
        ],
    );
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(err(&unknown).contains("figure"), "{}", err(&unknown));
    let on_the_model = run(
        &scratch,
        &[
            "samples/c1.md",
            "plot",
            "mass_bar",
            "--x-label",
            "V",
            "-o",
            "m.png",
            "--write",
        ],
    );
    assert_eq!(code(&on_the_model), 1, "{}", err(&on_the_model));
    assert!(
        err(&on_the_model).contains("its own axes"),
        "{}",
        err(&on_the_model)
    );
}

#[test]
fn a_label_matplotlib_cannot_typeset_is_refused() {
    // The math style's unit is siunitx, which matplotlib writes out as it is.
    let scratch = Scratch::new("typeset");
    project(&scratch);
    let refused = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--style",
            "math",
            "-o",
            "t.png",
            "--write",
        ],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(err(&refused).contains("\\si{\\gram}"), "{}", err(&refused));
    assert!(!scratch.at("project/t.png").exists());
}

#[test]
fn axes_complete_as_columns_do() {
    let scratch = Scratch::new("complete");
    project(&scratch);
    let complete = |words: &[&str]| {
        let mut arguments = vec!["--", "samplekit", "plot"];
        arguments.extend_from_slice(words);
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(&arguments)
            .current_dir(scratch.at("project/samples"))
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", (arguments.len() - 2).to_string())
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        out(&output)
    };
    assert!(complete(&["-x", "vol"]).contains("volume"));
    assert!(complete(&["-y", "m.hv["]).contains("m.hv[20]"));
    assert!(complete(&["--group", "allo"]).contains("alloy"));
    assert!(complete(&["--style", "fig"]).contains("figure"));
    // A figure's name: the declared ones, and the model's read from its
    // source, which completion never imports.
    for (begun, name) in [("d", "diameter_volume"), ("m", "mass_bar")] {
        let figures = complete(&[begun]);
        assert!(figures.contains(name), "{figures}");
    }
    assert!(complete(&["o"]).contains("overview"));
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn a_group_and_a_kind_replace_a_declared_figures_own() {
    // `--group` and `--kind` change a declared figure as `--title` does, and
    // are refused for a model's, which draws its own axes.
    let scratch = Scratch::new("override");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--group",
            "name",
            "--kind",
            "line",
            "-o",
            "g.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/g.png"), b"\x89PNG"));
    let on_the_model = run(
        &scratch,
        &[
            "samples/c1.md",
            "plot",
            "mass_bar",
            "--group",
            "alloy",
            "-o",
            "m.png",
            "--write",
        ],
    );
    assert_eq!(code(&on_the_model), 1, "{}", err(&on_the_model));
    assert!(
        err(&on_the_model).contains("--group"),
        "{}",
        err(&on_the_model)
    );
}

#[test]
fn no_project_style_sets_the_projects_settings_aside() {
    // Read with the project, from any directory; set aside on request.
    let scratch = Scratch::new("project-style");
    project(&scratch);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        // Centimetres, as every size SampleKit takes: 7 inches wide.
        &format!("{rc}[matplotlib]\n\"figure.figsize\" = [17.78, 12.7]\n\"figure.dpi\" = 100\n"),
    );
    let width = |name: &str| {
        let bytes = fs::read(scratch.at(&format!("project/samples/{name}"))).unwrap();
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]])
    };
    let inside = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project/samples"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("MPLBACKEND", "Agg")
        .args([
            ".",
            "plot",
            "diameter_volume",
            "-o",
            "styled.png",
            "--write",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&inside), 0, "{}", err(&inside));
    assert_eq!(width("styled.png"), 700);
    let aside = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project/samples"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("MPLBACKEND", "Agg")
        .args([
            ".",
            "plot",
            "diameter_volume",
            "--no-project-style",
            "-o",
            "plain.png",
            "--write",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&aside), 0, "{}", err(&aside));
    assert_eq!(width("plain.png"), 640);
}

#[test]
fn limits_scales_and_aspect_are_given_on_the_command_line() {
    // Replacing a declared figure's own, and refused where they cannot apply.
    let scratch = Scratch::new("axes-settings");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--x-limits",
            "0,auto",
            "--y-scale",
            "log",
            "--aspect",
            "equal",
            "-o",
            "a.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/a.png"), b"\x89PNG"));
    let unreadable = run(
        &scratch,
        &["samples", "plot", "diameter_volume", "--x-limits", "0,cent"],
    );
    assert_eq!(code(&unreadable), 1, "{}", err(&unreadable));
    assert!(err(&unreadable).contains("'cent'"), "{}", err(&unreadable));
    let at_zero = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--y-limits",
            "0,20",
            "--y-scale",
            "log",
            "-o",
            "z.png",
            "--write",
        ],
    );
    assert_eq!(code(&at_zero), 1, "{}", err(&at_zero));
    assert!(
        err(&at_zero).contains("at or below zero"),
        "{}",
        err(&at_zero)
    );
}

#[test]
fn a_preview_refuses_what_write_would() {
    // A preview that approves what --write then refuses reads as approval.
    let scratch = Scratch::new("preview-refuses");
    project(&scratch);
    for arguments in [
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "x.png",
            "--style",
            "nope",
        ][..],
        &[
            "samples",
            "plot",
            "mass_bar",
            "-o",
            "x.png",
            "--x-label",
            "m",
        ][..],
        &["samples", "plot", "diameter_volume", "-o", "noext"][..],
        &["samples", "plot", "diameter_volume", "-o", "-"][..],
        &["samples", "plot", "diameter_volume", "-o", "x.xyz"][..],
    ] {
        let output = run(&scratch, arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", out(&output));
        assert!(
            !out(&output).contains("--write to apply"),
            "{}",
            out(&output)
        );
    }
    // matplotlib would have written noext.png, and said nothing.
    let written = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "noext",
            "--write",
        ],
    );
    assert_eq!(code(&written), 1, "{}", err(&written));
    assert!(!scratch.at("project/noext.png").exists());

    let missing = run(
        &scratch,
        &["samples", "plot", "diameter_volume", "-o", "figs/x.png"],
    );
    assert_eq!(code(&missing), 0, "{}", err(&missing));
    assert!(
        out(&missing).contains("figs does not exist"),
        "{}",
        out(&missing)
    );
}

#[test]
fn an_output_makes_its_folder() {
    // As `export` makes its own: the paper's how-to began with mkdir.
    let scratch = Scratch::new("output-folder");
    project(&scratch);
    let preview = run(
        &scratch,
        &["samples", "plot", "diameter_volume", "-o", "figs/x.png"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("figs does not exist, and is made"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.at("project/figs").exists());
    let written = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "figs/x.png",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(starts_with(&scratch.at("project/figs/x.png"), b"\x89PNG"));
}

#[test]
fn a_declared_figures_query_is_counted_by_the_preview() {
    let scratch = Scratch::new("preview-query");
    project(&scratch);
    let output = run(&scratch, &["samples", "plot", "cobalt", "-o", "x.png"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("figure cobalt · x.png · 1 sample"),
        "{}",
        out(&output)
    );
}

#[test]
fn an_unknown_group_is_refused_naming_the_nearest() {
    let scratch = Scratch::new("group");
    project(&scratch);
    for arguments in [
        &["plot", "-x", "volume", "-y", "diameter", "--group", "alloi"][..],
        &["plot", "diameter_volume", "--group", "alloi"][..],
    ] {
        let mut arguments = [&["samples"][..], arguments].concat();
        arguments.extend(["-o", "x.png", "--write"]);
        let output = run(&scratch, &arguments);
        assert_eq!(code(&output), 1, "{}", err(&output));
        assert!(
            err(&output).contains("did you mean: alloy"),
            "{}",
            err(&output)
        );
    }
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn a_figures_name_beside_axes_is_a_usage_error() {
    let scratch = Scratch::new("name-and-axes");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-x",
            "volume",
            "-y",
            "diameter",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("not both"), "{}", err(&output));
}

#[test]
fn a_python_killed_by_a_signal_says_so() {
    // A crash in matplotlib's toolkit is not Ctrl-C, and is not silent.
    let scratch = Scratch::new("killed");
    project(&scratch);
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let python = scratch.write("project/.venv/bin/python", "#!/bin/sh\nkill -SEGV $$\n");
    fs::set_permissions(&python, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(err(&output).contains("signal 11"), "{}", err(&output));
}

#[test]
fn an_environment_without_samplekit_is_an_environment_failure() {
    let scratch = Scratch::new("no-samplekit");
    project(&scratch);
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let python = scratch.write(
        "project/.venv/bin/python",
        "#!/bin/sh\nexec /usr/bin/env -i PATH=/usr/bin:/bin python3 -I \"$@\"\n",
    );
    fs::set_permissions(&python, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("has no samplekit"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_name_close_to_a_figures_is_offered_before_consent() {
    // A typo is not the model's figure, and the model is not loaded.
    let scratch = Scratch::new("near-name");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volum",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("did you mean 'diameter_volume'"),
        "{}",
        err(&output)
    );
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn a_name_declared_and_the_models_is_refused() {
    // Two figures of one name are refused, the model read as text.
    let scratch = Scratch::new("double-name");
    project(&scratch);
    let rc = std::fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!("{rc}[figure.mass_bar]\nx = \"volume\"\ny = \"diameter\"\n"),
    );
    let output = run(
        &scratch,
        &["samples", "plot", "mass_bar", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("rename one"), "{}", err(&output));
    assert!(!scratch.at("project/imported").exists());
    let validated = run(&scratch, &["samples", "validate"]);
    assert!(
        out(&validated).contains("the model defines a figure of the same name"),
        "{}",
        out(&validated)
    );
}

#[test]
fn a_figure_of_one_sample_writes_a_file_per_sample() {
    // {name} in -o, one file each.
    let scratch = Scratch::new("per-sample");
    project(&scratch);
    let preview = run(
        &scratch,
        &["samples", "plot", "mass_bar", "-o", "figs/{name}.png"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("a file per sample, 2 files"),
        "{}",
        out(&preview)
    );
    fs::create_dir_all(scratch.at("project/figs")).unwrap();
    let written = run(
        &scratch,
        &[
            "samples",
            "plot",
            "mass_bar",
            "-o",
            "figs/{name}.png",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    for name in ["C1", "C2"] {
        assert!(starts_with(
            &scratch.at(&format!("project/figs/{name}.png")),
            b"\x89PNG"
        ));
    }
    let declared = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "figs/{name}.png",
        ],
    );
    assert_eq!(code(&declared), 1, "{}", err(&declared));
}

#[test]
fn a_legend_a_size_and_a_box_are_given_on_the_command_line() {
    let scratch = Scratch::new("legend-size-box");
    project(&scratch);
    let sized = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--legend",
            "outside",
            "--figsize",
            // Centimetres: 4 by 3 inches, 400 by 300 pixels.
            "10.16,7.62",
            "-o",
            "s.png",
            "--write",
        ],
    );
    assert_eq!(code(&sized), 0, "{}", err(&sized));
    let png = fs::read(scratch.at("project/s.png")).unwrap();
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((width, height), (400, 300));
    let boxed = run(
        &scratch,
        &[
            "samples", "plot", "-x", "alloy", "-y", "diameter", "--kind", "box", "-o", "b.png",
            "--write",
        ],
    );
    assert_eq!(code(&boxed), 0, "{}", err(&boxed));
    let bad = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--figsize",
            "4",
            "-o",
            "x.png",
        ],
    );
    assert_eq!(code(&bad), 1, "{}", err(&bad));
}

#[test]
fn a_figure_says_what_it_leaves_out() {
    let scratch = Scratch::new("left-out");
    project(&scratch);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let output = run(
        &scratch,
        &[
            "samples", "plot", "-x", "volume", "-y", "diameter", "-o", "x.png", "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("warning: 1 of 3 samples not drawn: no volume"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_figure_over_two_projects_draws_them_both() {
    // Axes given draw every sample, whatever project; a
    // declared figure needs every project to declare it alike.
    let scratch = Scratch::new("two-projects");
    project(&scratch);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &rc.replace(
            "schema_version = 1\n",
            "schema_version = 1\n[collection]\nrecursive = true\n",
        ),
    );
    scratch.write(
        "project/samples/nested/.samplekitrc",
        "schema_version = 1\n",
    );
    scratch.write(
        "project/samples/nested/n1.md",
        "---\nschema_version: 1\nname: N1\nalloy: tin\nproperties:\n  diameter: {v: 3.0, unit: g}\n  \
         volume: {v: 1.0, unit: L}\n---\n",
    );
    let axes = run(
        &scratch,
        &[
            "samples", "plot", "-x", "volume", "-y", "diameter", "-o", "a.png", "--write",
        ],
    );
    assert_eq!(code(&axes), 0, "{}", err(&axes));
    assert!(!err(&axes).contains("not drawn"), "{}", err(&axes));
    assert!(starts_with(&scratch.at("project/a.png"), b"\x89PNG"));
    // The nested project does not declare `diameter_volume`: refused, naming it.
    let declared = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "d.png",
            "--write",
        ],
    );
    assert_ne!(code(&declared), 0, "{}", err(&declared));
    assert!(
        err(&declared).contains("does not declare it"),
        "{}",
        err(&declared)
    );
    assert!(!scratch.at("project/d.png").exists());
}

#[test]
fn a_figure_written_is_found_again_and_carries_its_snapshot() {
    // The file tied to the snapshot it was made from, by its hash; a copy
    // changed since still names that snapshot in its metadata.
    let scratch = Scratch::new("figure-history");
    project(&scratch);
    for (file, marker) in [("out.svg", true), ("out.pdf", true), ("out.png", true)] {
        let output = run(
            &scratch,
            &["samples", "plot", "diameter_volume", "-o", file, "--write"],
        );
        assert_eq!(code(&output), 0, "{}", err(&output));
        let bytes = fs::read(scratch.at(&format!("project/{file}"))).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.contains("samplekit snapshot "), marker, "{file}");
    }
    let explained = run(&scratch, &["out.svg", "explain"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("by samplekit samples plot diameter_volume -o out.svg --write"),
        "{}",
        out(&explained)
    );
    let svg = scratch.at("project/out.svg");
    let edited = fs::read_to_string(&svg)
        .unwrap()
        .replace("</svg>", "<!-- edited --></svg>");
    fs::write(scratch.at("project/edited.svg"), edited).unwrap();
    let carried = run(&scratch, &["edited.svg", "explain"]);
    assert_eq!(code(&carried), 0, "{}", err(&carried));
    assert!(
        out(&carried).contains("carries the snapshot it was made from"),
        "{}",
        out(&carried)
    );
}

#[test]
fn a_figure_drawn_twice_is_the_same_file() {
    // A PDF and an SVG recorded the time they were written, so that a figure
    // made again with `--at` never compared equal to the one published.
    let scratch = Scratch::new("figure-undated");
    project(&scratch);
    for file in ["out.svg", "out.pdf"] {
        let mut drawn = Vec::new();
        for _ in 0..2 {
            let output = run(
                &scratch,
                &["samples", "plot", "diameter_volume", "-o", file, "--write"],
            );
            assert_eq!(code(&output), 0, "{}", err(&output));
            drawn.push(fs::read(scratch.at(&format!("project/{file}"))).unwrap());
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        assert!(drawn[0] == drawn[1], "{file} differs from itself");
    }
}

#[test]
fn a_figure_is_drawn_again_as_it_was() {
    // The figure of the project as a snapshot kept it, carrying that snapshot,
    // recorded in the history against it.
    let scratch = Scratch::new("figure-at");
    project(&scratch);
    let first = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "first.svg",
            "--write",
        ],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    let set = run(
        &scratch,
        &["samples/c1.md", "set", "diameter=15", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let log = out(&run(&scratch, &["log"]));
    assert!(
        log.contains("samplekit samples/c1.md set diameter=15"),
        "{log}"
    );
    let then = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "--at",
            "2",
            "-o",
            "then.svg",
            "--write",
        ],
    );
    assert_eq!(code(&then), 0, "{}", err(&then));
    let drawn = fs::read_to_string(scratch.at("project/then.svg")).unwrap();
    let carried = drawn
        .split("samplekit snapshot ")
        .nth(1)
        .map(|rest| rest[..40].to_string())
        .unwrap();
    let first_drawn = fs::read_to_string(scratch.at("project/first.svg")).unwrap();
    assert!(
        first_drawn.contains(&carried),
        "drawn from the snapshot the first was"
    );
    let explained = out(&run(&scratch, &["then.svg", "explain"]));
    assert!(
        explained.contains("made from the project as it was then, on purpose"),
        "{explained}"
    );
}

#[test]
fn a_preview_says_which_samples_will_not_be_drawn() {
    // The warning came with --write only; the preview said all were drawn.
    let scratch = Scratch::new("preview-left-out");
    project(&scratch);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(
        &scratch,
        &[
            "samples", "plot", "-x", "volume", "-y", "diameter", "-o", "out.png",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("· 2 of 3 samples\n\n  1 has no diameter"),
        "{}",
        out(&output)
    );
}

#[cfg(unix)]
#[test]
fn a_termination_reaches_the_drawing_and_ends_the_command() {
    // SIGTERM or SIGHUP to `plot`, a window open, did nothing: the handler
    // taking them was empty, and the drawing went on. An interpreter that
    // sleeps stands in for a window, so this needs no environment.
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};
    let scratch = Scratch::new("terminated");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npython = \".venv\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  m: {v: 1.0}\n---\n",
    );
    let python = scratch.write(".venv/bin/python", "#!/bin/sh\nexec sleep 30\n");
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    let mut plot = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args([".", "plot", "-x", "m", "-y", "m"])
        .current_dir(&scratch.0)
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    // Still drawing: what ends it below is the signal, not a refusal.
    assert!(plot.try_wait().unwrap().is_none(), "plot ended by itself");
    Command::new("kill")
        .args(["-TERM", &plot.id().to_string()])
        .status()
        .unwrap();
    let sent = Instant::now();
    loop {
        if let Some(status) = plot.try_wait().unwrap() {
            assert!(!status.success(), "{status:?}");
            break;
        }
        if sent.elapsed() > Duration::from_secs(10) {
            let _ = plot.kill();
            panic!("plot was still drawing ten seconds after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn an_empty_selection_is_refused() {
    // A figure of nothing is refused, as Python refuses it: it said *nothing
    // drawn* and exited 0.
    let scratch = Scratch::new("empty-selection");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "-x",
            "volume",
            "-y",
            "diameter",
            "-f",
            "alloy == gold",
            "-o",
            "out.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("no sample is selected: nothing to draw"),
        "{}",
        err(&output)
    );
    assert!(!scratch.at("project/out.png").exists());
    // A declared figure's query that keeps nothing is named.
    let output = run(
        &scratch,
        &["samples", "plot", "cobalt", "-f", "alloy == chrome"],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("(the figure's query is 'cobalt')"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_group_over_several_fields_draws_their_combinations() {
    // `--group alloy,name`: a series per combination of the two.
    let scratch = Scratch::new("several-groups");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "-x",
            "volume",
            "-y",
            "diameter",
            "--group",
            "alloy,name",
            "-o",
            "out.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.png"), b"\x89PNG"));
    // Each field is checked: a misspelt one is refused, naming the nearest.
    let output = run(
        &scratch,
        &[
            "samples",
            "plot",
            "-x",
            "volume",
            "-y",
            "diameter",
            "--group",
            "alloy,nmae",
            "-o",
            "bad.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("name"), "{}", err(&output));
    assert!(!scratch.at("project/bad.png").exists());
}

#[test]
fn a_figure_that_draws_nothing_for_a_sample_is_said() {
    // A model figure of one sample per file wrote a blank file for a sample
    // it had nothing to draw of, where a declared figure says so.
    let scratch = Scratch::new("per-sample-blank");
    project(&scratch);
    scratch.write(
        "project/model.py",
        &MODEL.replace(
            "    @sk.figure\n    def mass_bar",
            "    @sk.figure\n    def sparse(self, ax):\n        if self.name == \"C1\":\n            \
             ax.plot([1, 2], [1, 2])\n        else:\n            ax.plot([], [])\n\n    \
             @sk.figure\n    def mass_bar",
        ),
    );
    fs::create_dir_all(scratch.at("project/figs")).unwrap();
    let written = run(
        &scratch,
        &[
            "samples",
            "plot",
            "sparse",
            "-o",
            "figs/{name}.png",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        err(&written).contains("1 of 2 figures drew nothing, and is written blank: C2"),
        "{}",
        err(&written)
    );
}

#[test]
fn a_figure_from_values_not_current_exits_two_unless_accepted() {
    // An export from values not current is written and exits 2; a figure
    // warned and exited 0. Alike now, and said in the preview.
    let scratch = Scratch::new("stale");
    project(&scratch);
    let file = scratch.at("project/samples/c1.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace(
            "  volume: {v: 4.0, unit: L}\n",
            "  volume: {v: 4.0, unit: L}\n  twice: {v: 24.0, computed: {diameter: '111111111111'}, \
             fingerprint: '222222222222'}\n",
        ),
    )
    .unwrap();
    let preview = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "twice", "-o", "f.png",
        ],
    );
    assert!(
        out(&preview).contains("drawn from values that are not current"),
        "{}",
        out(&preview)
    );
    let drawn = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "twice", "-o", "f.png", "--write",
        ],
    );
    assert_eq!(code(&drawn), 2, "{}", err(&drawn));
    // Typed over its formula, the value is edited, and accepted by its word
    //.
    assert!(err(&drawn).contains("--accept edited"), "{}", err(&drawn));
    assert!(scratch.at("project/f.png").exists());
    let accepted = run(
        &scratch,
        &[
            "samples", "plot", "-x", "diameter", "-y", "twice", "-o", "g.png", "--write",
            "--accept", "edited",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
}

#[test]
fn a_figure_from_a_column_not_current_exits_two_unless_accepted() {
    // A table's column drawn while not current wrote the figure and exited 0,
    // where a property's exits 2: alike now.
    let scratch = Scratch::new("stale-column");
    project(&scratch);
    let file = scratch.at("project/samples/c1.md");
    let text = fs::read_to_string(&file).unwrap();
    // A cell recorded from a T whose digest is not the one it holds.
    fs::write(
        &file,
        text.replace(
            "hv: {v: 5.1, u: 0.05}",
            "hv: {v: 5.1, computed: {row.T: '111111111111'}, fingerprint: '222222222222'}",
        ),
    )
    .unwrap();
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!("{rc}\n[figure.colour]\nkind = \"line\"\nx = \"m.T\"\ny = \"m.hv\"\n"),
    );
    let drawn = run(
        &scratch,
        &["samples", "plot", "colour", "-o", "f.png", "--write"],
    );
    assert_eq!(code(&drawn), 2, "{}", err(&drawn));
    assert!(err(&drawn).contains("C1 m.hv"), "{}", err(&drawn));
    let accepted = run(
        &scratch,
        &[
            "samples", "plot", "colour", "-o", "g.png", "--write", "--accept", "edited",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
}

#[test]
fn a_figure_over_projects_takes_the_first_named_and_refuses_disagreeing_units() {
    // `diameter` in g here, in no unit in `gamma`: refused, naming both; given
    // one unit, either order draws, the first named's spellings said.
    let scratch = Scratch::new("over-projects");
    project(&scratch);
    scratch.write(
        "gamma/.samplekitrc",
        "schema_version = 1\n[model]\npython = \"../project/.venv\"\n",
    );
    scratch.write(
        "gamma/samples/g1.md",
        "---\nschema_version: 1\nname: G1\nproperties:\n  diameter: 11.0\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let axes = ["-x", "volume", "-y", "diameter", "-o", "out.svg"];
    let refused = run(
        &scratch,
        &[&["samples", "../gamma/samples", "plot"][..], &axes].concat(),
    );
    assert_eq!(code(&refused), 2, "{}", err(&refused));
    let said = err(&refused);
    assert!(said.contains("diameter is in g in "), "{said}");
    assert!(
        said.contains("in no unit in ") && said.contains("gamma"),
        "{said}"
    );

    scratch.write(
        "gamma/samples/g1.md",
        "---\nschema_version: 1\nname: G1\nproperties:\n  diameter: {v: 11.0, unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    for targets in [
        ["samples", "../gamma/samples"],
        ["../gamma/samples", "samples"],
    ] {
        let output = run(&scratch, &[&targets[..], &["plot"], &axes].concat());
        assert_eq!(code(&output), 0, "{targets:?}: {}", err(&output));
        let note = err(&output);
        assert_eq!(
            note.matches("the first project named").count(),
            1,
            "{targets:?}: {note}"
        );
        let first = if targets[0] == "samples" {
            "project"
        } else {
            "gamma"
        };
        assert!(
            note.contains(&format!(
                "unit spellings of {}",
                scratch.at(first).join(".samplekitrc").display()
            )),
            "{targets:?}: {note}"
        );
    }
}

/// The fixture's project, with exports of its figures beside its own
/// declarations.
fn project_exporting(scratch: &Scratch, exports: &str) -> PathBuf {
    let root = project(scratch);
    scratch.write("project/.samplekitrc", &format!("{RC}{exports}"));
    root
}

#[test]
fn a_figure_is_exported_to_its_declared_file() {
    // As a table for a paper is written again by one command, its figure is.
    let scratch = Scratch::new("export-figure");
    project_exporting(
        &scratch,
        "[export.curves]\nfigure = \"diameter_volume\"\noutput = \"figs/curves.png\"\n",
    );
    let preview = run(&scratch, &["samples", "export", "curves"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview)
            .contains("export curves · figure diameter_volume · figs/curves.png · 2 samples"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.at("project/figs").exists());
    let written = run(&scratch, &["samples", "export", "curves", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(starts_with(
        &scratch.at("project/figs/curves.png"),
        b"\x89PNG"
    ));
    // Listed with the exports, by the figure it draws.
    let listed = run(&scratch, &["list", "exports"]);
    assert!(
        out(&listed).contains("figs/curves.png · figure diameter_volume"),
        "{}",
        out(&listed)
    );
    // Made again by the command that made it.
    let explained = run(&scratch, &["figs/curves.png", "explain"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("by samplekit samples export curves --write"),
        "{}",
        out(&explained)
    );
    // -o writes it elsewhere.
    let elsewhere = run(
        &scratch,
        &["samples", "export", "curves", "-o", "other.png", "--write"],
    );
    assert_eq!(code(&elsewhere), 0, "{}", err(&elsewhere));
    assert!(starts_with(&scratch.at("project/other.png"), b"\x89PNG"));
}

#[test]
fn a_figures_export_takes_its_query_and_its_style() {
    let scratch = Scratch::new("export-figure-query");
    project_exporting(
        &scratch,
        "[export.dark]\nfigure = \"diameter_volume\"\noutput = \"dark.png\"\n\
         query = \"cobalt\"\nstyle = \"figure\"\n",
    );
    let preview = run(&scratch, &["samples", "export", "dark"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("export dark · figure diameter_volume · dark.png · 1 sample"),
        "{}",
        out(&preview)
    );
    let written = run(&scratch, &["samples", "export", "dark", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(starts_with(&scratch.at("project/dark.png"), b"\x89PNG"));
}

#[test]
fn a_model_figure_is_exported_a_file_per_sample() {
    let scratch = Scratch::new("export-model-figure");
    project_exporting(
        &scratch,
        "[export.bars]\nfigure = \"mass_bar\"\noutput = \"figs/{name}-bar.png\"\n",
    );
    let written = run(&scratch, &["samples", "export", "bars", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    for name in ["C1", "C2"] {
        assert!(
            starts_with(
                &scratch.at(&format!("project/figs/{name}-bar.png")),
                b"\x89PNG"
            ),
            "{name}"
        );
    }
}

#[test]
fn an_export_writes_a_profile_or_a_figure() {
    // Refused as the file loads, each saying what to write.
    let scratch = Scratch::new("export-figure-refused");
    for (declared, said) in [
        (
            "[profile.p]\ncolumns = [{field = \"diameter\"}]\n\
             [export.e]\nprofile = \"p\"\nfigure = \"diameter_volume\"\noutput = \"e.png\"\n",
            "a profile or a figure, not both",
        ),
        (
            "[export.e]\noutput = \"e.png\"\n",
            "profile = … for a table, figure = … for a figure",
        ),
        (
            "[export.e]\nfigure = \"diameter_volume\"\nformat = \"csv\"\noutput = \"e.png\"\n",
            "format is a table's",
        ),
        (
            "[export.e]\nfigure = \"diameter_volume\"\noutput = \"e.csv\"\n",
            "one matplotlib writes",
        ),
    ] {
        if !scratch.at("project").exists() {
            project(&scratch);
        }
        scratch.write("project/.samplekitrc", &format!("{RC}{declared}"));
        let output = run(&scratch, &["samples", "export", "e"]);
        assert_ne!(code(&output), 0, "{declared}");
        assert!(err(&output).contains(said), "{declared}\n{}", err(&output));
    }
}

#[test]
fn a_figures_export_refuses_a_tables_options() {
    let scratch = Scratch::new("export-figure-options");
    project_exporting(
        &scratch,
        "[export.curves]\nfigure = \"diameter_volume\"\noutput = \"curves.png\"\n",
    );
    for option in [&["--precision", ".3f"][..], &["--status"][..]] {
        let mut arguments = vec!["samples", "export", "curves"];
        arguments.extend(option);
        let output = run(&scratch, &arguments);
        assert_eq!(code(&output), 1, "{}", err(&output));
        assert!(
            err(&output).contains(&format!("{} is a table's", option[0])),
            "{}",
            err(&output)
        );
    }
}

#[test]
fn an_export_naming_no_figure_is_a_defect() {
    let scratch = Scratch::new("export-figure-unknown");
    project_exporting(
        &scratch,
        "[export.lost]\nfigure = \"nothing\"\noutput = \"lost.png\"\n",
    );
    let validated = run(&scratch, &["samples", "validate"]);
    assert!(
        out(&validated).contains("figure = \"nothing\" names no figure"),
        "{}",
        out(&validated)
    );
    let exported = run(&scratch, &["samples", "export", "lost", "--write"]);
    assert_eq!(code(&exported), 1, "{}", err(&exported));
    assert!(!scratch.at("project/lost.png").exists());
}

#[test]
fn a_figures_export_over_several_projects_is_refused() {
    // A figure is drawn from one project: the refusal names one.
    let scratch = Scratch::new("export-figure-projects");
    let exports = "[export.curves]\nfigure = \"diameter_volume\"\noutput = \"curves.png\"\n";
    project_exporting(&scratch, exports);
    scratch.write("project/other/.samplekitrc", &format!("{RC}{exports}"));
    scratch.write(
        "project/other/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nalloy: chrome\nproperties:\n  \
         diameter: {v: 9.0, u: 0.1, unit: g}\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(&scratch, &[".", "export", "curves", "--write"]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("a figure is drawn from one project"),
        "{}",
        err(&output)
    );
    assert!(!scratch.at("project/curves.png").exists());
}

#[test]
fn a_style_sheet_or_a_font_not_there_is_said_by_validate_and_plot() {
    let scratch = Scratch::new("matplotlib-sheets");
    project_exporting(
        &scratch,
        "[matplotlib]\nstyle = [\"nonesuch\"]\nfonts = [\"fonts/*.otf\"]\n",
    );
    let validated = run(&scratch, &["samples", "validate"]);
    for said in [
        "matplotlib has no style sheet 'nonesuch'",
        "fonts/*.otf finds no file",
    ] {
        assert!(out(&validated).contains(said), "{}", out(&validated));
    }
    let drawn = run(
        &scratch,
        &[
            "samples",
            "plot",
            "diameter_volume",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_ne!(code(&drawn), 0, "{}", err(&drawn));
    assert!(
        err(&drawn).contains("no style sheet 'nonesuch'"),
        "{}",
        err(&drawn)
    );
    assert!(!scratch.at("project/x.png").exists());
}
