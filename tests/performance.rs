//! A large project stays quick: what the terminal interface and the command
//! line do grows with the number of samples, never with its square.
//!
//! Timed against itself, not against a clock: the same operations on a
//! project four times larger take about four times as long where they grow
//! with the samples, sixteen where they grow with the square. A machine twice
//! as slow changes neither.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use samplekit::tui::model::{Key, Workbench};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "samplekit-performance-{name}-{}",
                std::process::id()
            ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A project of `count` samples beside its model, at its root and in two
/// folders, as a project grown over years lays them out: the model at the root
/// is what made every sample's reading walk every folder.
fn project(name: &str, count: usize) -> Scratch {
    let scratch = Scratch::new(name);
    let root = &scratch.0;
    fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [model]\npath = \"model.py\"\n",
    )
    .unwrap();
    fs::write(root.join("model.py"), "import samplekit as sk\n").unwrap();
    for folder in ["2024", "2025"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    let metals = ["copper", "steel", "aluminium", "titanium"];
    for at in 0..count {
        let folder = match at % 3 {
            0 => root.to_path_buf(),
            1 => root.join("2024"),
            _ => root.join("2025"),
        };
        let thickness = 1.9 + (at % 17) as f64 / 100.0;
        fs::write(
            folder.join(format!("specimen-{at:05}.md")),
            format!(
                "---\nschema_version: 1\nname: specimen-{at:05}\nmaterial: {}\n\
                 batch: {}\ncompared_with: \"[[specimen-{:05}]]\"\nproperties:\n  \
                 width: {{readings: [12.509, 12.498, 12.506], unit: mm}}\n  \
                 thickness: {{v: {thickness:.3}, u: 0.01, unit: mm}}\n  \
                 elongation: {{v: {}, unit: \"%\"}}\n\
                 tables:\n  curve:\n    index: step\n    columns:\n      step: {{}}\n      \
                 force: {{unit: kN}}\n    rows:\n      - step: 1\n        force: 1.5\n      \
                 - step: 2\n        force: 3.25\n---\nTested at 2 mm/min.\n",
                metals[at % metals.len()],
                at % 12,
                (at + 1) % count,
                10 + at % 25,
            ),
        )
        .unwrap();
    }
    scratch
}

/// The shortest of three runs: the others waited on the machine.
fn shortest(mut run: impl FnMut()) -> Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed()
        })
        .min()
        .unwrap()
}

/// What each operation took on the project at `root`.
fn timed(root: &Path) -> Vec<(&'static str, Duration)> {
    let mut workbench = Workbench::open(root).unwrap();
    let mut times = vec![
        (
            "open",
            shortest(|| {
                Workbench::open(root).unwrap();
            }),
        ),
        ("reload", shortest(|| workbench.reload())),
        (
            "sort",
            shortest(|| {
                workbench.key(Key::Char('s'));
                workbench.key(Key::Enter);
                workbench.key(Key::Esc);
            }),
        ),
        (
            "filter",
            shortest(|| {
                workbench.key(Key::Char('/'));
                for typed in "elongation > 20".chars() {
                    workbench.key(Key::Char(typed));
                }
                workbench.key(Key::Enter);
            }),
        ),
    ];
    let command = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(arguments)
            .current_dir(root)
            .env("SAMPLEKIT_STATE_DIR", root.join(".state"))
            .env("COLUMNS", "200")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    times.push((
        "list",
        shortest(|| command(&["-c", "name,material,thickness", "-s", "thickness"])),
    ));
    times.push(("validate", shortest(|| command(&["validate"]))));
    times
}

#[test]
fn four_times_the_samples_take_about_four_times_as_long() {
    let small = project("small", 500);
    let large = project("large", 2000);
    let (before, after) = (timed(&small.0), timed(&large.0));
    let mut slow = Vec::new();
    for ((name, small), (_, large)) in before.iter().zip(&after) {
        // A floor, so that what takes no time at all is no ratio of noise.
        let ratio = large.as_secs_f64() / small.as_secs_f64().max(0.005);
        println!("{name:<10}{small:>12.3?}{large:>12.3?}{ratio:>8.1}");
        if ratio > 9.0 {
            slow.push(format!(
                "{name}: {small:.3?} for 500 samples, {large:.3?} for 2000"
            ));
        }
    }
    assert!(
        slow.is_empty(),
        "grows faster than the samples:\n{}",
        slow.join("\n")
    );
}
