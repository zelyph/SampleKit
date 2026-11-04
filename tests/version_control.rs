//! The tests of `version_control`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use samplekit::config::version_control as vcs;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-vcs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    /// A repository with one commit containing everything written so far.
    fn commit(&self) {
        git(&self.0, &["init", "-q"]);
        git(&self.0, &["config", "user.email", "t@example.invalid"]);
        git(&self.0, &["config", "user.name", "Test"]);
        git(&self.0, &["add", "-A"]);
        git(&self.0, &["commit", "-q", "-m", "initial"]);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(at: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------- history

const SAMPLE: &str = "---\nschema_version: 1\nname: S\nproperties:\n  diameter: 1.0\n---\n";

/// A project with a model, two samples, a sample's own file and an export.
fn project(scratch: &Scratch, model: &str) -> samplekit::config::project_config::ProjectConfig {
    scratch.file(
        ".samplekitrc",
        &format!(
            "schema_version = 1\n[collection]\nrecursive = true\nfiles = [\"images\"]\n\
             [model]\npath = \"{model}\"\nclass = \"M\"\n"
        ),
    );
    fs::create_dir_all(scratch.0.join("samples")).unwrap();
    fs::create_dir_all(scratch.0.join("images")).unwrap();
    fs::create_dir_all(scratch.0.join("out")).unwrap();
    scratch.file("samples/a.md", SAMPLE);
    scratch.file("samples/b.md", &SAMPLE.replace("name: S", "name: T"));
    scratch.file("images/a_photo.png", "png");
    scratch.file("out/table.csv", "name\nS\n");
    samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap()
}

/// The paths the history's last snapshot holds.
fn kept(scratch: &Scratch) -> Vec<String> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn commits(scratch: &Scratch) -> usize {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[test]
fn a_snapshot_keeps_the_samples_the_configuration_and_the_model() {
    let scratch = Scratch::new("history-kept");
    fs::create_dir_all(scratch.0.join("model/helpers")).unwrap();
    scratch.file("model/m.py", "import samplekit as sk\n");
    scratch.file("model/helpers/fit.py", "x = 1\n");
    let config = project(&scratch, "model/m.py");
    let taken = vcs::snapshot(&config, "first").unwrap();
    assert!(matches!(taken, vcs::Snapshot::Taken(_)), "{taken:?}");
    assert_eq!(
        kept(&scratch),
        [
            ".samplekitrc",
            "model/helpers/fit.py",
            "model/m.py",
            "samples/a.md",
            "samples/b.md"
        ]
    );
    // A sample removed is gone from the next snapshot.
    fs::remove_file(scratch.0.join("samples/b.md")).unwrap();
    vcs::snapshot(&config, "second").unwrap();
    assert!(!kept(&scratch).contains(&"samples/b.md".to_string()));
}

#[test]
fn a_snapshot_of_unchanged_files_is_not_taken() {
    let scratch = Scratch::new("history-unchanged");
    let config = project(&scratch, "model/m.py");
    vcs::snapshot(&config, "first").unwrap();
    assert_eq!(
        vcs::snapshot(&config, "again").unwrap(),
        vcs::Snapshot::Unchanged
    );
    assert_eq!(commits(&scratch), 1);
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "2.0"));
    assert!(matches!(
        vcs::snapshot(&config, "changed").unwrap(),
        vcs::Snapshot::Taken(_)
    ));
    assert_eq!(commits(&scratch), 2);
}

#[test]
fn a_model_outside_the_project_is_kept_under_at_model() {
    let scratch = Scratch::new("history-outside-model");
    let shared = Scratch::new("history-shared-model");
    fs::write(shared.0.join("piece.py"), "import samplekit as sk\n").unwrap();
    // With `/`, which a TOML string holds as it is on every system.
    let model = shared
        .0
        .join("piece.py")
        .display()
        .to_string()
        .replace('\\', "/");
    let config = project(&scratch, &model);
    vcs::snapshot(&config, "first").unwrap();
    assert!(
        kept(&scratch).contains(&"@model/piece.py".to_string()),
        "{:?}",
        kept(&scratch)
    );
}

#[test]
fn a_snapshot_written_out_names_the_environment_where_it_is() {
    // `../.venv`, the demo's, was looked for beside the snapshot written out
    // for `--at`, and found nowhere: no formula was judged, no figure drawn.
    let scratch = Scratch::new("history-environment");
    project(&scratch, "model/m.py");
    let rc = scratch.0.join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    scratch.file(".samplekitrc", &format!("{text}python = \"../.venv\"\n"));
    let config = samplekit::config::project_config::load(&rc).unwrap();
    let vcs::Snapshot::Taken(id) = vcs::snapshot(&config, "first").unwrap() else {
        panic!("a snapshot");
    };
    let out = Scratch::new("history-environment-out");
    let root = vcs::materialize(&config, &id, &out.0.join("project")).unwrap();
    let written = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    let real = dunce::canonicalize(&scratch.0).unwrap();
    assert_eq!(
        written.python().unwrap(),
        real.parent().unwrap().join(".venv")
    );
}

#[test]
fn the_researchers_repository_sees_nothing_of_the_history() {
    let scratch = Scratch::new("history-outer-repo");
    let config = project(&scratch, "model/m.py");
    scratch.file(".gitignore", "out/\n");
    scratch.commit();
    vcs::snapshot(&config, "first").unwrap();
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "3.0"));
    vcs::snapshot(&config, "second").unwrap();
    let status = Command::new("git")
        .arg("-C")
        .arg(&scratch.0)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .unwrap();
    // Their repository sees the sample it tracks change, and nothing else.
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        "M samples/a.md"
    );
    assert_eq!(
        fs::read_to_string(scratch.0.join(".gitignore")).unwrap(),
        "out/\n"
    );
}

#[test]
fn a_write_that_changes_nothing_leaves_no_history() {
    let scratch = Scratch::new("history-nothing");
    let config = project(&scratch, "model/m.py");
    let before = vcs::before(&config).unwrap().unwrap();
    assert_eq!(
        vcs::after(before, "nothing").unwrap(),
        vcs::Snapshot::Unchanged
    );
    assert!(!scratch.0.join(".samplekit/history").exists());
    // Changed, then written: what was there first, then the write.
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "4.0"));
    let before = vcs::before(&config).unwrap().unwrap();
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "5.0"));
    assert!(matches!(
        vcs::after(before, "the write").unwrap(),
        vcs::Snapshot::Taken(_)
    ));
    assert_eq!(commits(&scratch), 2);
    let shown = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["show", "HEAD~1:samples/a.md"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&shown.stdout).contains("4.0"));
}

#[test]
fn a_file_written_is_found_by_its_hash() {
    // A tag named after the file's hash, on the snapshot it was made from,
    // saying what wrote it and from which samples.
    let scratch = Scratch::new("history-output");
    let config = project(&scratch, "model/m.py");
    let written = scratch.file("out/table.csv", "name,diameter\nS,1.0\n");
    let samples = [scratch.0.join("samples/a.md")];
    let hash = vcs::keep_output(
        &config,
        &written,
        "samplekit . export table --write",
        &samples,
    )
    .unwrap()
    .unwrap();
    let made = vcs::made(&config, &hash).unwrap().unwrap();
    assert_eq!(made.said, "samplekit . export table --write");
    assert_eq!(made.written, "out/table.csv");
    assert_eq!(made.samples, ["samples/a.md"]);
    // The snapshot holds what the file was made from, the file itself not.
    assert!(kept(&scratch).contains(&"samples/a.md".to_string()));
    assert!(!kept(&scratch).contains(&"out/table.csv".to_string()));
    assert_eq!(
        vcs::made_from(&config, &made.snapshot).unwrap(),
        std::slice::from_ref(&made)
    );
    // Another file's hash is nothing the history made.
    let other = scratch.file("out/other.csv", "name\n");
    let hash = vcs::file_hash(&other).unwrap();
    assert_eq!(vcs::made(&config, &hash).unwrap(), None);
}

#[test]
fn line_endings_alone_are_no_change() {
    // Git for Windows writes what a clone or a pull brings with CRLF: the
    // same files, which neither a write nor the history takes for a change.
    let scratch = Scratch::new("history-crlf");
    let config = project(&scratch, "model/m.py");
    vcs::snapshot(&config, "start").unwrap();
    for name in [".samplekitrc", "samples/a.md", "samples/b.md"] {
        let text = fs::read_to_string(scratch.0.join(name)).unwrap();
        scratch.file(name, &text.replace('\n', "\r\n"));
    }
    assert_eq!(
        vcs::snapshot(&config, "again").unwrap(),
        vcs::Snapshot::Unchanged
    );
    let before = vcs::before(&config).unwrap().unwrap();
    scratch.file(
        "samples/a.md",
        &SAMPLE.replace("1.0", "2.0").replace('\n', "\r\n"),
    );
    assert!(matches!(
        vcs::after(before, "the write").unwrap(),
        vcs::Snapshot::Taken(_)
    ));
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.iter().all(|entry| entry.message != vcs::OUTSIDE));
    assert_eq!(entries[0].changed, ["samples/a.md"]);
    // Held with LF.
    let shown = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["show", "HEAD:samples/a.md"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&shown.stdout).contains("2.0"));
    assert!(!shown.stdout.contains(&b'\r'));
}

#[test]
fn a_file_written_is_found_whatever_its_line_endings() {
    let scratch = Scratch::new("history-output-crlf");
    let config = project(&scratch, "model/m.py");
    let written = scratch.file("out/table.csv", "name,diameter\nS,1.0\n");
    let samples = [scratch.0.join("samples/a.md")];
    let hash = vcs::keep_output(
        &config,
        &written,
        "samplekit . export table --write",
        &samples,
    )
    .unwrap()
    .unwrap();
    // Brought back by git with CRLF: still the file written.
    scratch.file("out/table.csv", "name,diameter\r\nS,1.0\r\n");
    assert_eq!(vcs::file_hash(&written).unwrap(), hash);
    assert!(vcs::made(&config, &hash).unwrap().is_some());
    // A file holding a NUL byte is read as it is.
    let with = scratch.0.join("out/with.png");
    let without = scratch.0.join("out/without.png");
    fs::write(&with, b"\x89PNG\0\r\n").unwrap();
    fs::write(&without, b"\x89PNG\0\n").unwrap();
    assert_ne!(
        vcs::file_hash(&with).unwrap(),
        vcs::file_hash(&without).unwrap()
    );
}

#[test]
fn a_snapshot_taken_meanwhile_is_not_taken_back() {
    // A Python script reads the project at its first save and keeps its
    // snapshot when it ends; a command run meanwhile took one of its own.
    // What the script read is then not committed: it would take that
    // command's change back and keep a state the project never held.
    let scratch = Scratch::new("history-meanwhile");
    let config = project(&scratch, "model/m.py");
    vcs::snapshot(&config, "start").unwrap();
    let before = vcs::before(&config).unwrap().unwrap();
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "2.0"));
    vcs::snapshot(&config, "another command").unwrap();
    // Written by the script, as every write of SampleKit is.
    samplekit::format::document::write_atomically(
        &scratch.0.join("samples/b.md"),
        SAMPLE.replace("1.0", "3.0").as_bytes(),
    )
    .unwrap();
    assert!(matches!(
        vcs::after(before, "the script").unwrap(),
        vcs::Snapshot::Taken(_)
    ));
    let messages: Vec<String> = vcs::entries(&config)
        .unwrap()
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    assert_eq!(messages, ["the script", "another command", "start"]);
    // The other command's change stands in every snapshot after it.
    let last = &vcs::entries(&config).unwrap()[0];
    let files = vcs::files_at(&config, &last.id).unwrap();
    assert!(String::from_utf8_lossy(&files["samples/a.md"]).contains("2.0"));
    assert!(String::from_utf8_lossy(&files["samples/b.md"]).contains("3.0"));
}

#[test]
fn each_write_of_the_same_contents_is_recorded() {
    // The same bytes written twice, from two snapshots to two places: two
    // records, newest first, each found by the snapshot it was made from.
    let scratch = Scratch::new("history-twice");
    let config = project(&scratch, "model/m.py");
    let samples = [scratch.0.join("samples/a.md")];
    let first = scratch.file("out/table.csv", "name,diameter\nS,1.0\n");
    let hash = vcs::keep_output(&config, &first, "samplekit export table --write", &samples)
        .unwrap()
        .unwrap();
    scratch.file("samples/b.md", &SAMPLE.replace("1.0", "7.0"));
    let second = scratch.file("out/then.csv", "name,diameter\nS,1.0\n");
    let again = vcs::keep_output(
        &config,
        &second,
        "samplekit export table -o out/then.csv --write",
        &samples,
    )
    .unwrap()
    .unwrap();
    assert_eq!(again, hash);
    let records = vcs::made_all(&config, &hash).unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    let written: Vec<&str> = records.iter().map(|made| made.written.as_str()).collect();
    assert!(written.contains(&"out/table.csv") && written.contains(&"out/then.csv"));
    assert_ne!(records[0].snapshot, records[1].snapshot);
    assert_eq!(vcs::made(&config, &hash).unwrap().as_ref(), records.first());
    for made in &records {
        assert_eq!(
            vcs::made_from(&config, &made.snapshot).unwrap(),
            std::slice::from_ref(made)
        );
    }
    // Written again exactly as a record says: nothing new.
    vcs::keep_output(
        &config,
        &second,
        "samplekit export table -o out/then.csv --write",
        &samples,
    )
    .unwrap();
    assert_eq!(vcs::made_all(&config, &hash).unwrap().len(), 2);
}

#[test]
fn a_figure_carries_its_snapshot() {
    let scratch = Scratch::new("history-carried");
    let id = "0123456789abcdef0123456789abcdef01234567";
    let figure = scratch.file(
        "figure.svg",
        &format!("<svg><desc>{}{id}</desc></svg>", vcs::CARRIED),
    );
    assert_eq!(vcs::carried_snapshot(&figure).as_deref(), Some(id));
    let plain = scratch.file("plain.svg", "<svg></svg>");
    assert_eq!(vcs::carried_snapshot(&plain), None);
}

// ------------------------------------------------ several machines

/// A project of two samples and no model, in `folder`.
fn two_samples(folder: &Path) -> samplekit::config::project_config::ProjectConfig {
    fs::create_dir_all(folder).unwrap();
    fs::write(folder.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    fs::write(folder.join("a.md"), SAMPLE.replace("name: S", "name: a")).unwrap();
    fs::write(folder.join("b.md"), SAMPLE.replace("name: S", "name: b")).unwrap();
    samplekit::config::project_config::load(&folder.join(".samplekitrc")).unwrap()
}

/// `samplekit ARGS`, run in `folder` as the machine `machine`.
fn write_as(folder: &Path, machine: &str, arguments: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(arguments)
        .current_dir(folder)
        .env("SAMPLEKIT_MACHINE", machine)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// What a synchroniser does, from `from` to `to`: every file of the history
/// copied — each machine writes only its own branch — and the samples named.
fn synchronise(from: &Path, to: &Path, samples: &[&str]) {
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                if target.exists() {
                    // An object is read-only, as Git writes it.
                    let mut permissions = fs::metadata(&target).unwrap().permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    permissions.set_readonly(false);
                    fs::set_permissions(&target, permissions).unwrap();
                }
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
    copy(
        &from.join(".samplekit/history"),
        &to.join(".samplekit/history"),
    );
    for sample in samples {
        fs::copy(from.join(sample), to.join(sample)).unwrap();
    }
}

fn outside_of(entries: &[vcs::Entry]) -> Vec<&vcs::Entry> {
    entries
        .iter()
        .filter(|entry| entry.message == vcs::OUTSIDE)
        .collect()
}

#[test]
fn each_machine_writes_a_branch_of_its_own() {
    let scratch = Scratch::new("machines-branches");
    let config = two_samples(&scratch.0);
    write_as(
        &scratch.0,
        "ana",
        &[".", "-c", "name,diameter", "-o", "t.csv", "--write"],
    );
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    write_as(
        &scratch.0,
        "tom",
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let branches = scratch.0.join(".samplekit/history/refs/heads");
    assert!(branches.join("ana").is_file() && branches.join("tom").is_file());
    let entries = vcs::entries(&config).unwrap();
    let said: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.machine.as_str(), entry.message.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("tom", "samplekit b.md set diameter=2.5 --write"),
            ("ana", "samplekit a.md set diameter=1.5 --write"),
            ("ana", vcs::FIRST),
        ]
    );
    // Each snapshot names its machine, wherever it is read from.
    let authors = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["log", "--format=%ae", "refs/heads/tom"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&authors.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["tom@samplekit", "ana@samplekit", "ana@samplekit"]
    );
}

#[test]
fn a_record_of_a_file_written_names_its_machine() {
    // A machine alone names its record too, so that two machines that each kept
    // a history alone and are then synchronised never wrote one tag.
    let scratch = Scratch::new("machines-records");
    let config = two_samples(&scratch.0);
    let tags = scratch.0.join(".samplekit/history/refs/tags/output");
    let names = || {
        let mut names: Vec<String> = fs::read_dir(&tags)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let write = |machine: &str, to: &str| {
        write_as(
            &scratch.0,
            machine,
            &[".", "-c", "name", "-o", to, "--write"],
        );
    };
    write("ana", "t.csv");
    let hash = vcs::file_hash(&scratch.0.join("t.csv")).unwrap();
    assert_eq!(names(), [format!("{hash}@ana")]);
    // The same bytes written again from another snapshot, then on another
    // machine.
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    write("ana", "t2.csv");
    write("tom", "t3.csv");
    for copy in ["t2.csv", "t3.csv"] {
        assert_eq!(
            fs::read(scratch.0.join(copy)).unwrap(),
            fs::read(scratch.0.join("t.csv")).unwrap()
        );
    }
    assert_eq!(
        names(),
        [
            format!("{hash}@ana"),
            format!("{hash}@ana.2"),
            format!("{hash}@tom")
        ]
    );
    assert_eq!(vcs::made_all(&config, &hash).unwrap().len(), 3);
}

#[test]
fn the_history_joins_once_the_files_hold_the_others_state() {
    let scratch = Scratch::new("machines-join");
    let (ana, tom) = (scratch.0.join("ana"), scratch.0.join("tom"));
    let config = two_samples(&ana);
    write_as(&ana, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    fs::create_dir_all(&tom).unwrap();
    fs::copy(ana.join(".samplekitrc"), tom.join(".samplekitrc")).unwrap();
    synchronise(&ana, &tom, &["a.md", "b.md"]);
    // A machine's first snapshot in a history another wrote follows it.
    write_as(&tom, "tom", &["b.md", "set", "diameter=2.5", "--write"]);
    synchronise(&tom, &ana, &["b.md"]);
    write_as(&ana, "ana", &["a.md", "set", "diameter=1.7", "--write"]);
    let entries = vcs::entries(&config).unwrap();
    let join = &entries[0];
    assert_eq!(join.machine, "ana");
    assert_eq!(join.parents.len(), 2, "{entries:#?}");
    assert_eq!(join.joined, ["tom"]);
    assert_eq!(join.changed, ["a.md"], "its own change alone");
    assert_eq!(entries[1].machine, "tom");
    assert_eq!(entries[1].joined, ["ana"]);
    assert_eq!(entries[1].changed, ["b.md"]);
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
    // What the join was taken over holds tom's change.
    let before = vcs::files_before(&config, &join.id).unwrap();
    assert!(String::from_utf8_lossy(&before["b.md"]).contains("2.5"));
    assert!(String::from_utf8_lossy(&before["a.md"]).contains("1.5"));
    // And back the same way.
    synchronise(&ana, &tom, &["a.md"]);
    write_as(&tom, "tom", &["b.md", "set", "diameter=2.7", "--write"]);
    let config = samplekit::config::project_config::load(&tom.join(".samplekitrc")).unwrap();
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(entries[0].machine, "tom");
    assert_eq!(entries[0].parents.len(), 2);
    assert_eq!(entries[0].joined, ["ana"]);
    assert_eq!(entries[0].changed, ["b.md"]);
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
    assert_eq!(entries.len(), 5);
}

#[test]
fn files_not_synchronised_are_not_joined() {
    let scratch = Scratch::new("machines-apart");
    let (ana, tom) = (scratch.0.join("ana"), scratch.0.join("tom"));
    two_samples(&ana);
    write_as(&ana, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    fs::create_dir_all(&tom).unwrap();
    fs::copy(ana.join(".samplekitrc"), tom.join(".samplekitrc")).unwrap();
    synchronise(&ana, &tom, &["a.md", "b.md"]);
    write_as(&tom, "tom", &["b.md", "set", "diameter=2.5", "--write"]);
    synchronise(&tom, &ana, &["b.md"]);
    write_as(&ana, "ana", &["a.md", "set", "diameter=1.7", "--write"]);
    // The history arrives, and not a.md: tom's files do not hold ana's last.
    synchronise(&ana, &tom, &[]);
    write_as(&tom, "tom", &["b.md", "set", "diameter=2.7", "--write"]);
    let config = samplekit::config::project_config::load(&tom.join(".samplekitrc")).unwrap();
    let entries = vcs::entries(&config).unwrap();
    // Taken beside ana's last, perhaps within its second: found by what it
    // says rather than by its place.
    let apart = entries
        .iter()
        .find(|entry| entry.message == "samplekit b.md set diameter=2.7 --write")
        .expect("tom's snapshot");
    assert_eq!(apart.machine, "tom");
    assert_eq!(apart.parents.len(), 1, "{entries:#?}");
    assert!(apart.joined.is_empty());
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
    // Once a.md arrives, the next snapshot joins, and records nothing as
    // changed outside SampleKit.
    fs::copy(ana.join("a.md"), tom.join("a.md")).unwrap();
    write_as(&tom, "tom", &["b.md", "set", "diameter=2.9", "--write"]);
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(entries[0].parents.len(), 2, "{entries:#?}");
    assert_eq!(entries[0].joined, ["ana"]);
    assert_eq!(entries[0].changed, ["b.md"]);
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
}

#[test]
fn an_undo_takes_back_this_machines_change_only() {
    let scratch = Scratch::new("machines-undo");
    let config = two_samples(&scratch.0);
    let me = vcs::machine();
    let other = format!("{me}-other");
    write_as(&scratch.0, &me, &["a.md", "set", "diameter=1.5", "--write"]);
    write_as(
        &scratch.0,
        &other,
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let undoable = vcs::last_undoable(&config).unwrap().expect("a change");
    assert_eq!(undoable.message, "samplekit a.md set diameter=1.5 --write");
    assert_eq!(undoable.files.len(), 1);
    assert!(undoable.files[0].path.ends_with("a.md"));
}

#[test]
fn a_machine_name_is_safe_as_a_branch() {
    assert_eq!(vcs::machine_name("a b/c"), "a-b-c");
    assert_eq!(vcs::machine_name("node1.cluster-a"), "node1.cluster-a");
    assert_eq!(vcs::machine_name(""), "machine");
    assert_eq!(vcs::machine_name("."), "machine");
    assert_eq!(vcs::host_name("Toms-MacBook.local"), "toms-macbook");
}

/// `samplekit ARGS`, run in `folder` with `state` as the machine's state
/// directory and no machine named by the environment.
fn write_with_state(folder: &Path, state: &Path, arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(arguments)
        .current_dir(folder)
        .env_remove("SAMPLEKIT_MACHINE")
        .env("SAMPLEKIT_STATE_DIR", state)
        .env("COLUMNS", "200")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn history_git(folder: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(folder.join(".samplekit/history"))
        .args(arguments)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn a_machine_is_an_identifier_drawn_once() {
    let scratch = Scratch::new("machines-identifier");
    two_samples(&scratch.0.join("p"));
    let state = scratch.0.join("state");
    write_with_state(
        &scratch.0.join("p"),
        &state,
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    write_with_state(
        &scratch.0.join("p"),
        &state,
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let identifier = fs::read_to_string(state.join("machine")).unwrap();
    let identifier = identifier.trim();
    assert_eq!(identifier.len(), 16);
    assert!(identifier.chars().all(|c| c.is_ascii_hexdigit()));
    let branches: Vec<String> = fs::read_dir(scratch.0.join("p/.samplekit/history/refs/heads"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(branches, [identifier]);
    let authors = history_git(
        &scratch.0.join("p"),
        &["log", "--format=%an <%ae>", identifier],
    );
    let expected = format!("{} <{identifier}@samplekit>", vcs::own_host());
    assert!(
        authors.lines().count() == 3 && authors.lines().all(|line| line == expected),
        "{authors}"
    );
}

#[test]
fn this_machine_is_you_and_another_is_shown_by_its_name() {
    let scratch = Scratch::new("machines-shown");
    let folder = scratch.0.join("p");
    two_samples(&folder);
    let state = scratch.0.join("state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("machine"), "0123456789abcdef\n").unwrap();
    write_as(
        &folder,
        "ana-pc",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    write_with_state(&folder, &state, &["b.md", "set", "diameter=2.5", "--write"]);
    let log = write_with_state(&folder, &state, &["log"]);
    assert!(
        log.contains("you, joins ana-pc") || log.contains("you "),
        "{log}"
    );
    assert!(log.contains("ana-pc"), "{log}");
    fs::write(state.join("machines"), "ana-pc Ana's laptop\n").unwrap();
    let log = write_with_state(&folder, &state, &["log"]);
    assert!(
        log.contains("Ana's laptop") && !log.contains("ana-pc"),
        "{log}"
    );
    // An account read away from this machine names each by its host.
    let account = write_with_state(&folder, &state, &["log", "--export"]);
    assert!(
        account.contains(&vcs::own_host()) && !account.contains(" you"),
        "{account}"
    );
}

#[test]
fn a_history_arriving_without_its_head_is_opened_again() {
    let scratch = Scratch::new("machines-head");
    let config = two_samples(&scratch.0);
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    let history = scratch.0.join(".samplekit/history");
    for gone in ["HEAD", "config", "description"] {
        let _ = fs::remove_file(history.join(gone));
    }
    let _ = fs::remove_dir_all(history.join("hooks"));
    let _ = fs::remove_dir_all(history.join("logs"));
    // Read before anything is written: a fresh clone had no history until it
    // wrote — no log, no --at, and a formula changed not seen.
    let log = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .arg("log")
        .current_dir(&scratch.0)
        .env("SAMPLEKIT_MACHINE", "ana")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(log.contains("diameter=1.5"), "{log}");
    write_as(
        &scratch.0,
        "ana",
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    assert_eq!(
        fs::read_to_string(history.join("HEAD")).unwrap(),
        "ref: refs/heads/ana\n"
    );
    assert_eq!(vcs::entries(&config).unwrap().len(), 3);
}

/// A journal of a write whose process is gone, saying `lines` after its
/// command: what a command killed, or whose snapshot failed, leaves.
fn journal_left(folder: &Path, lines: &str) {
    // A process that ends at once, on each system.
    let mut child = if cfg!(windows) {
        Command::new("cmd").args(["/c", "exit 0"]).spawn().unwrap()
    } else {
        Command::new("true").spawn().unwrap()
    };
    let pid = child.id();
    child.wait().unwrap();
    fs::create_dir_all(folder.join(".samplekit")).unwrap();
    fs::write(
        folder.join(format!(".samplekit/writing-{pid}.lock")),
        format!(
            "{pid}\nhost {}\nuser {}\nfor samplekit compute --write\n{lines}",
            samplekit::format::lock::own_host(),
            samplekit::format::lock::own_user()
        ),
    )
    .unwrap();
}

#[test]
fn a_formula_said_by_a_command_killed_is_known() {
    // A compute killed before it kept its trace left values with no formula
    // known: once their formula changed, they were said current.
    let scratch = Scratch::new("journal-facts");
    let config = two_samples(&scratch.0);
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    assert!(vcs::trace(&config).unwrap().is_empty());
    fs::write(
        scratch.0.join("a.md"),
        SAMPLE.replace("name: S", "name: a").replace("1.0", "3.0"),
    )
    .unwrap();
    journal_left(&scratch.0, "wrote a.md\nfact diameter\tabcdef\tf1\n");
    assert!(!vcs::trace(&config).unwrap().is_empty());
    // The next write keeps it, and says the command after its snapshot.
    write_as(
        &scratch.0,
        "ana",
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let said: Vec<String> = vcs::entries(&config)
        .unwrap()
        .iter()
        .map(|entry| entry.message.clone())
        .collect();
    assert!(
        said.iter()
            .any(|message| message == "samplekit compute --write — interrupted"),
        "{said:?}"
    );
    assert!(
        fs::read_dir(scratch.0.join(".samplekit"))
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().starts_with("writing-"))
    );
}

#[test]
fn a_write_whose_snapshot_failed_is_kept_late() {
    // Kept by the next command under its name, it was said interrupted,
    // which a whole write is not.
    let scratch = Scratch::new("journal-kept-late");
    let config = two_samples(&scratch.0);
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    fs::write(
        scratch.0.join("a.md"),
        SAMPLE.replace("name: S", "name: a").replace("1.0", "3.0"),
    )
    .unwrap();
    journal_left(&scratch.0, "wrote a.md\nnot kept\n");
    write_as(
        &scratch.0,
        "ana",
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let said: Vec<String> = vcs::entries(&config)
        .unwrap()
        .iter()
        .map(|entry| entry.message.clone())
        .collect();
    assert!(
        said.iter()
            .any(|message| message == "samplekit compute --write — kept late"),
        "{said:?}"
    );
}

#[test]
fn a_first_snapshot_leaves_out_what_another_write_under_way_wrote() {
    // A script's first save in a project with no history yet was in the
    // snapshot a command beside it took first, and the script's snapshot
    // changed nothing of it.
    let scratch = Scratch::new("first-snapshot-others");
    let config = two_samples(&scratch.0);
    // This test is the other write under way: its journal says a.md.
    let under_way = vcs::before(&config).unwrap().unwrap();
    samplekit::format::document::write_atomically(
        &scratch.0.join("a.md"),
        SAMPLE
            .replace("name: S", "name: a")
            .replace("1.0", "3.0")
            .as_bytes(),
    )
    .unwrap();
    write_as(
        &scratch.0,
        "ana",
        &["b.md", "set", "diameter=2.5", "--write"],
    );
    let entries = vcs::entries(&config).unwrap();
    let first = entries.last().unwrap();
    let files = vcs::files_at(&config, &first.id).unwrap();
    assert!(!files.contains_key("a.md"), "{:?}", files.keys());
    drop(under_way);
}

#[test]
fn the_samplekit_folder_keeps_its_own_files_out_of_git() {
    let scratch = Scratch::new("samplekit-gitignore");
    two_samples(&scratch.0);
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=1.5", "--write"],
    );
    let ignore = fs::read_to_string(scratch.0.join(".samplekit/.gitignore")).unwrap();
    let lines: Vec<&str> = ignore
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    // Its own alone: `.gitignore` left out the history's too, which a shared
    // history commits.
    assert_eq!(lines, ["/.gitignore", "model.json", "failures/", "*.lock"]);
    // One written before the journals is given their line, and its own
    // `.gitignore` alone.
    fs::write(
        scratch.0.join(".samplekit/.gitignore"),
        "# Written by SampleKit: what each machine makes for itself.\n.gitignore\n",
    )
    .unwrap();
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=2.0", "--write"],
    );
    assert!(
        fs::read_to_string(scratch.0.join(".samplekit/.gitignore"))
            .unwrap()
            .ends_with("\n/.gitignore\n*.lock\n")
    );
    fs::write(scratch.0.join(".samplekit/.gitignore"), "mine\n").unwrap();
    write_as(
        &scratch.0,
        "ana",
        &["a.md", "set", "diameter=2.5", "--write"],
    );
    assert_eq!(
        fs::read_to_string(scratch.0.join(".samplekit/.gitignore")).unwrap(),
        "mine\n"
    );
}

#[test]
fn a_change_is_taken_back_field_by_field() {
    let sample = |diameter: &str, tags: &str| {
        format!(
            "---\nschema_version: 1\nname: a\ntags: [{tags}]\nproperties:\n  diameter: {diameter}\n---\n"
        )
        .into_bytes()
    };
    let (before, after) = (sample("1.0", ""), sample("1.5", ""));
    // A tag added since stays; the value comes back.
    let now = sample("1.5", "inspected");
    let given = vcs::taken_back("a.md", Some(&before), Some(&after), Some(&now))
        .unwrap()
        .unwrap();
    let given = String::from_utf8(given).unwrap();
    assert!(
        given.contains("diameter: 1.0") && given.contains("inspected"),
        "{given}"
    );
    // The value changed since: refused, named.
    let now = sample("1.9", "");
    assert_eq!(
        vcs::taken_back("a.md", Some(&before), Some(&after), Some(&now)).unwrap_err(),
        ["a.md: diameter, changed since"]
    );
    // A file the change made, unchanged since: gone.
    assert_eq!(
        vcs::taken_back("b.md", None, Some(&after), Some(&after)).unwrap(),
        None
    );
    // Any other file, whole.
    assert!(vcs::taken_back(".samplekitrc", Some(b"a"), Some(b"b"), Some(b"c")).is_err());
}

// --------------------------------------------------- writes side by side

#[test]
fn a_command_keeps_only_the_files_it_wrote() {
    // Twenty `set` at once: each snapshot held what the others had written
    // by then, under one command's name, and theirs were lost.
    let scratch = Scratch::new("own-files");
    scratch.file(".samplekitrc", "schema_version = 1\n");
    scratch.file("a.md", SAMPLE);
    scratch.file("b.md", &SAMPLE.replace("name: S", "name: T"));
    let config = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    vcs::snapshot(&config, "first").unwrap();
    // This process starts a write of a.md …
    let (writing, _) = vcs::before_writing(std::slice::from_ref(&scratch.0));
    samplekit::format::document::write_atomically(
        &scratch.0.join("a.md"),
        SAMPLE.replace("1.0", "2.0").as_bytes(),
    )
    .unwrap();
    // … while another command writes b.md and keeps its snapshot first.
    let other = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(&scratch.0)
        .args(["b.md", "set", "diameter=3", "--write"])
        .output()
        .unwrap();
    assert!(
        other.status.success(),
        "{}",
        String::from_utf8_lossy(&other.stderr)
    );
    assert!(vcs::after_writing(writing, "mine").is_empty());
    let changed = |message: &str| -> Vec<String> {
        let entries = vcs::entries(&config).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.message.starts_with(message))
            .unwrap();
        entry.changed.clone()
    };
    assert_eq!(changed("samplekit b.md set"), ["b.md"]);
    assert_eq!(changed("mine"), ["a.md"]);
}

#[test]
#[cfg(unix)]
fn a_write_interrupted_is_kept_under_its_own_name() {
    // A write killed before its snapshot was kept by the next as a change
    // made outside SampleKit: whose it was was lost.
    let scratch = Scratch::new("interrupted");
    scratch.file(".samplekitrc", "schema_version = 1\n");
    scratch.file("a.md", SAMPLE);
    scratch.file("b.md", &SAMPLE.replace("name: S", "name: T"));
    let config = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    vcs::snapshot(&config, "first").unwrap();
    let mut gone = Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    fs::create_dir_all(scratch.0.join(".samplekit")).unwrap();
    fs::write(
        scratch.0.join(format!(".samplekit/writing-{pid}.lock")),
        format!(
            "{pid}\nhost {}\nuser {}\nfor samplekit a.md set diameter=2 --write\nwrote {}\n",
            samplekit::format::lock::own_host(),
            samplekit::format::lock::own_user(),
            scratch.0.join("a.md").display()
        ),
    )
    .unwrap();
    scratch.file("a.md", &SAMPLE.replace("1.0", "2.0"));
    let set = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(&scratch.0)
        .args(["b.md", "set", "diameter=3", "--write"])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(
        entries[1].message,
        "samplekit a.md set diameter=2 --write — interrupted"
    );
    assert_eq!(entries[1].changed, ["a.md"]);
    assert_eq!(entries[0].changed, ["b.md"]);
    assert!(
        !scratch
            .0
            .join(format!(".samplekit/writing-{pid}.lock"))
            .exists()
    );
}

#[test]
#[cfg(unix)]
fn a_reference_lock_left_in_the_history_is_taken_back() {
    // Left by a snapshot cut short, it stopped every snapshot after it, for
    // good, with a message naming neither the file nor what to do.
    let scratch = Scratch::new("ref-lock");
    scratch.file(".samplekitrc", "schema_version = 1\n");
    scratch.file("a.md", SAMPLE);
    let config = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    let set = |diameter: &str| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .current_dir(&scratch.0)
            .env("SAMPLEKIT_MACHINE", "ana")
            .args(["a.md", "set", &format!("diameter={diameter}"), "--write"])
            .output()
            .unwrap()
    };
    assert!(set("2").status.success());
    let lock = scratch.0.join(".samplekit/history/refs/heads/ana.lock");
    fs::write(&lock, "").unwrap();
    // Two minutes old, set by the file itself: `touch -d` is GNU's alone.
    fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120))
        .unwrap();
    let output = set("3");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("ana.lock") && said.contains("taken back"),
        "{said}"
    );
    assert!(!lock.exists());
    assert_eq!(
        vcs::entries(&config).unwrap()[0].message,
        "samplekit a.md set diameter=3 --write"
    );
}

#[test]
fn a_machines_snapshots_follow_their_chain_whatever_their_clock() {
    // Two snapshots taken with the clock a year ahead, then one at the right
    // time: the log put the true last change below them, and `restore`
    // took back two changes.
    let scratch = Scratch::new("clock");
    scratch.file(".samplekitrc", "schema_version = 1\n");
    scratch.file("a.md", SAMPLE);
    let config = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    vcs::snapshot(&config, "first").unwrap();
    let history = scratch.0.join(".samplekit/history");
    let git_at = |arguments: &[&str], env: &[(&str, &str)]| -> String {
        let output = Command::new("git")
            .arg("--git-dir")
            .arg(&history)
            .args(arguments)
            .envs(env.iter().copied())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let head = vcs::head(&config).unwrap().unwrap();
    let author = git_at(&["log", "-1", "--format=%an\n%ae", &head], &[]);
    let (name, email) = author.split_once('\n').unwrap();
    let branch = git_at(&["symbolic-ref", "HEAD"], &[]);
    let tree = git_at(&["rev-parse", &format!("{head}^{{tree}}")], &[]);
    let ahead = "2099-01-01T12:00:00+00:00";
    let future = git_at(
        &["commit-tree", &tree, "-p", &head, "-m", "ahead"],
        &[
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
            ("GIT_AUTHOR_DATE", ahead),
            ("GIT_COMMITTER_DATE", ahead),
        ],
    );
    git_at(&["update-ref", &branch, &future], &[]);
    scratch.file("a.md", &SAMPLE.replace("1.0", "2.0"));
    vcs::snapshot(&config, "now").unwrap();
    let entries = vcs::entries(&config).unwrap();
    let messages: Vec<&str> = entries.iter().map(|entry| entry.message.as_str()).collect();
    assert_eq!(messages, ["now", "ahead", "first"]);
    assert!(entries[0].clock_went_back);
    assert!(!entries[1].clock_went_back);
    assert!(samplekit::presentation::changes::when(&entries[0]).ends_with("(clock went back)"));
}

#[test]
#[cfg(unix)]
fn ctrl_c_stops_a_tag_after_the_file_under_way() {
    // SIGINT during `tag` went to the end, or killed it with no snapshot.
    let scratch = Scratch::new("tag-interrupted");
    scratch.file(".samplekitrc", "schema_version = 1\n");
    for at in 0..400 {
        scratch.file(
            &format!("s{at:03}.md"),
            &SAMPLE.replace("name: S", &format!("name: s{at:03}")),
        );
    }
    let config = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(&scratch.0)
        .args([".", "tag", "add", "inspected", "--write"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let tagged = || {
        (0..400)
            .filter(|at| {
                fs::read_to_string(scratch.0.join(format!("s{at:03}.md")))
                    .unwrap_or_default()
                    .contains("inspected")
            })
            .count()
    };
    let started = std::time::Instant::now();
    while tagged() == 0 && started.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    let count = tagged();
    if output.status.code() == Some(0) {
        // Too quick to be stopped on this machine: all written.
        assert_eq!(count, 400);
        return;
    }
    assert_eq!(output.status.code(), Some(130), "{said}");
    assert!(
        said.contains(&format!("interrupted: {count} files written")),
        "{said}"
    );
    assert!(count < 400);
    let last = &vcs::entries(&config).unwrap()[0];
    assert!(last.message.ends_with("— interrupted"), "{}", last.message);
    assert_eq!(last.changed.len(), count);
}

// ------------------------------------------------------- a damaged history

#[test]
fn a_write_over_a_damaged_history_says_so() {
    let scratch = Scratch::new("damaged");
    scratch.file("model.py", "import samplekit as sk\n");
    let config = project(&scratch, "model.py");
    vcs::snapshot(&config, "first").unwrap();
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "2.0"));
    vcs::snapshot(&config, "second").unwrap();
    // The first snapshot's tree removed, as a pull that moved it would.
    let history = scratch.0.join(".samplekit/history");
    let listed = Command::new("git")
        .arg("--git-dir")
        .arg(&history)
        .args(["log", "--all", "--format=%T"])
        .output()
        .unwrap();
    let first = String::from_utf8(listed.stdout)
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_string();
    fs::remove_file(history.join("objects").join(&first[..2]).join(&first[2..])).unwrap();
    vcs::notices();
    scratch.file("samples/a.md", &SAMPLE.replace("1.0", "3.0"));
    assert!(matches!(
        vcs::snapshot(&config, "third").unwrap(),
        vcs::Snapshot::Taken(_)
    ));
    let said = vcs::notices().join("\n");
    assert!(
        said.contains("is damaged") && said.contains(&first),
        "{said}"
    );
}

#[test]
fn history_shared_writes_the_historys_gitignore() {
    // Sharing the history through git meant writing seven patterns by hand.
    let scratch = Scratch::new("history-shared");
    scratch.file("model.py", "import samplekit as sk\n");
    let config = project(&scratch, "model.py");
    vcs::snapshot(&config, "first").unwrap();
    let ignore = scratch.0.join(".samplekit/history/.gitignore");
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
    let rc = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    let declared = |shared: &str| {
        scratch.file(
            ".samplekitrc",
            &format!("{rc}[history]\nshared = {shared}\n"),
        );
        samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap()
    };
    // Read, and the file is written: no write needed first.
    vcs::entries(&declared("true")).unwrap();
    let shared = fs::read_to_string(&ignore).unwrap();
    assert_eq!(
        shared,
        ".tmp*\n*.lock\n/HEAD\n/config\n/description\n/hooks/\n/logs/\n"
    );
    vcs::entries(&declared("false")).unwrap();
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
    // Declared nowhere, a file written by hand is left as it is.
    fs::write(&ignore, ".tmp*\n*.lock\n").unwrap();
    scratch.file(".samplekitrc", &rc);
    let bare = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    vcs::snapshot(&bare, "second").unwrap();
    assert_eq!(fs::read_to_string(&ignore).unwrap(), ".tmp*\n*.lock\n");
}
