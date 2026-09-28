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

const SAMPLE: &str = "---\nschema_version: 1\nname: S\nproperties:\n  malt: 1.0\n---\n";

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
    fs::write(shared.0.join("brew.py"), "import samplekit as sk\n").unwrap();
    // With `/`, which a TOML string holds as it is on every system.
    let model = shared
        .0
        .join("brew.py")
        .display()
        .to_string()
        .replace('\\', "/");
    let config = project(&scratch, &model);
    vcs::snapshot(&config, "first").unwrap();
    assert!(
        kept(&scratch).contains(&"@model/brew.py".to_string()),
        "{:?}",
        kept(&scratch)
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
    let written = scratch.file("out/table.csv", "name,malt\nS,1.0\n");
    let samples = [scratch.0.join("samples/a.md")];
    let hash = vcs::keep_output(
        &config,
        &written,
        "samplekit export table . --write",
        &samples,
    )
    .unwrap()
    .unwrap();
    let made = vcs::made(&config, &hash).unwrap().unwrap();
    assert_eq!(made.said, "samplekit export table . --write");
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
    scratch.file("samples/b.md", &SAMPLE.replace("1.0", "3.0"));
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
    let first = scratch.file("out/table.csv", "name,malt\nS,1.0\n");
    let hash = vcs::keep_output(&config, &first, "samplekit export table --write", &samples)
        .unwrap()
        .unwrap();
    scratch.file("samples/b.md", &SAMPLE.replace("1.0", "7.0"));
    let second = scratch.file("out/then.csv", "name,malt\nS,1.0\n");
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
        &[".", "-c", "name,malt", "-o", "t.csv", "--write"],
    );
    write_as(&scratch.0, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    write_as(&scratch.0, "tom", &["set", "b.md", "malt=2.5", "--write"]);
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
            ("tom", "samplekit set b.md malt=2.5 --write"),
            ("ana", "samplekit set a.md malt=1.5 --write"),
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
    write_as(&scratch.0, "ana", &["set", "a.md", "malt=1.5", "--write"]);
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
    // A record named without its machine, as one written before, still reads.
    fs::rename(tags.join(format!("{hash}@ana")), tags.join(&hash)).unwrap();
    let written: Vec<String> = vcs::made_all(&config, &hash)
        .unwrap()
        .into_iter()
        .map(|made| made.written)
        .collect();
    assert_eq!(written.len(), 3, "{written:?}");
    assert!(written.contains(&"t.csv".to_string()), "{written:?}");
}

#[test]
fn the_history_joins_once_the_files_hold_the_others_state() {
    let scratch = Scratch::new("machines-join");
    let (ana, tom) = (scratch.0.join("ana"), scratch.0.join("tom"));
    let config = two_samples(&ana);
    write_as(&ana, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    fs::create_dir_all(&tom).unwrap();
    fs::copy(ana.join(".samplekitrc"), tom.join(".samplekitrc")).unwrap();
    synchronise(&ana, &tom, &["a.md", "b.md"]);
    // A machine's first snapshot in a history another wrote follows it.
    write_as(&tom, "tom", &["set", "b.md", "malt=2.5", "--write"]);
    synchronise(&tom, &ana, &["b.md"]);
    write_as(&ana, "ana", &["set", "a.md", "malt=1.7", "--write"]);
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
    write_as(&tom, "tom", &["set", "b.md", "malt=2.7", "--write"]);
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
    write_as(&ana, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    fs::create_dir_all(&tom).unwrap();
    fs::copy(ana.join(".samplekitrc"), tom.join(".samplekitrc")).unwrap();
    synchronise(&ana, &tom, &["a.md", "b.md"]);
    write_as(&tom, "tom", &["set", "b.md", "malt=2.5", "--write"]);
    synchronise(&tom, &ana, &["b.md"]);
    write_as(&ana, "ana", &["set", "a.md", "malt=1.7", "--write"]);
    // The history arrives, and not a.md: tom's files do not hold ana's last.
    synchronise(&ana, &tom, &[]);
    write_as(&tom, "tom", &["set", "b.md", "malt=2.7", "--write"]);
    let config = samplekit::config::project_config::load(&tom.join(".samplekitrc")).unwrap();
    let entries = vcs::entries(&config).unwrap();
    // Taken beside ana's last, perhaps within its second: found by what it
    // says rather than by its place.
    let apart = entries
        .iter()
        .find(|entry| entry.message == "samplekit set b.md malt=2.7 --write")
        .expect("tom's snapshot");
    assert_eq!(apart.machine, "tom");
    assert_eq!(apart.parents.len(), 1, "{entries:#?}");
    assert!(apart.joined.is_empty());
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
    // Once a.md arrives, the next snapshot joins, and records nothing as
    // changed outside SampleKit.
    fs::copy(ana.join("a.md"), tom.join("a.md")).unwrap();
    write_as(&tom, "tom", &["set", "b.md", "malt=2.9", "--write"]);
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(entries[0].parents.len(), 2, "{entries:#?}");
    assert_eq!(entries[0].joined, ["ana"]);
    assert_eq!(entries[0].changed, ["b.md"]);
    assert!(outside_of(&entries).is_empty(), "{entries:#?}");
}

#[test]
fn a_history_kept_before_is_adopted_by_its_first_writer() {
    let scratch = Scratch::new("machines-adopted");
    let config = two_samples(&scratch.0);
    let history = scratch.0.join(".samplekit/history");
    // A history as SampleKit kept it before branches per machine: one branch, `main`, its
    // snapshots signed by no machine.
    let git_at = |arguments: &[&str], input: Option<&str>| -> String {
        use std::io::Write;
        let mut child = Command::new("git")
            .arg("--git-dir")
            .arg(&history)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "SampleKit")
            .env("GIT_AUTHOR_EMAIL", "samplekit@localhost")
            .env("GIT_COMMITTER_NAME", "SampleKit")
            .env("GIT_COMMITTER_EMAIL", "samplekit@localhost")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "git {arguments:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    fs::create_dir_all(&history).unwrap();
    git(&history, &["init", "-q", "--bare"]);
    git_at(&["symbolic-ref", "HEAD", "refs/heads/main"], None);
    let mut listing = String::new();
    for name in [".samplekitrc", "a.md", "b.md"] {
        let path = scratch.0.join(name);
        let blob = git_at(&["hash-object", "-w", path.to_str().unwrap()], None);
        listing.push_str(&format!("100644 blob {blob}\t{name}\n"));
    }
    let tree = git_at(&["mktree"], Some(&listing));
    let legacy = git_at(&["commit-tree", tree.as_str(), "-m", vcs::FIRST], None);
    git_at(&["update-ref", "refs/heads/main", legacy.as_str()], None);
    write_as(&scratch.0, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    assert!(!history.join("refs/heads/main").exists());
    assert_eq!(
        fs::read_to_string(history.join("HEAD")).unwrap().trim(),
        "ref: refs/heads/ana"
    );
    let entries = vcs::entries(&config).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].id, legacy, "nothing is rewritten");
    assert_eq!(entries[0].parents, std::slice::from_ref(&legacy));
    assert!(entries.iter().all(|entry| entry.machine == "ana"));
}

#[test]
fn an_undo_takes_back_this_machines_change_only() {
    let scratch = Scratch::new("machines-undo");
    let config = two_samples(&scratch.0);
    let me = vcs::machine();
    let other = format!("{me}-other");
    write_as(&scratch.0, &me, &["set", "a.md", "malt=1.5", "--write"]);
    write_as(&scratch.0, &other, &["set", "b.md", "malt=2.5", "--write"]);
    let undoable = vcs::last_undoable(&config).unwrap().expect("a change");
    assert_eq!(undoable.message, "samplekit set a.md malt=1.5 --write");
    assert_eq!(undoable.files.len(), 1);
    assert!(undoable.files[0].path.ends_with("a.md"));
}

#[test]
fn a_machine_name_is_safe_as_a_branch() {
    assert_eq!(vcs::machine_name("Toms-MacBook.local"), "toms-macbook");
    assert_eq!(vcs::machine_name("a b/c"), "a-b-c");
    assert_eq!(vcs::machine_name(""), "machine");
    assert_eq!(vcs::machine_name(".hidden"), "machine");
}
