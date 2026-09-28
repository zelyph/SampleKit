//! The tests of `cli`.
//!
//! These run the **compiled binary**. Exit codes and stream separation are only
//! real when observed from outside, and a test calling a library function would
//! prove neither.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use samplekit::collection::sample_list as list;
use samplekit::query::field_addressing::{self as fields, Field};

/// A directory of samples, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "samplekit-cli-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, file: &str, body: &str) -> PathBuf {
        let path = self.0.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    /// One cask: a malt, a plato, a status and a tag.
    fn sample(
        &self,
        file: &str,
        name: &str,
        status: &str,
        malt: f64,
        brix: Option<f64>,
    ) -> PathBuf {
        let mut body = format!(
            "---\nschema_version: 1\nname: {name}\ntags: [reference]\nstatus: {status}\n\
             properties:\n  malt: {{v: {malt}, u: 0.05, unit: g}}\n"
        );
        if let Some(brix) = brix {
            body.push_str(&format!("  brix: {{v: {brix}, u: 0.0012, unit: g/L}}\n"));
        }
        body.push_str(&format!("---\nBrewing notes for {name}.\n"));
        self.write(file, &body)
    }

    /// The project: a query, a view, a profile and an export.
    fn config(&self) -> PathBuf {
        self.write(
            ".samplekitrc",
            "schema_version = 1\n\
             [render]\n\
             table = \"boxed\"\n\
             [unit.\"g/L\"]\n\
             plain = \"g/L\"\n\
             math = \"\\\\mathrm{g\\\\,L^{-1}}\"\n\
             [property.malt]\n\
             precision = \".2f\"\n\
             [property.brix]\n\
             symbol = \"Bx\"\n\
             symbol_math = \"\\\\brix\"\n\
             precision = \".4f\"\n\
             [style.math]\n\
             separator = \"\\\\pm\"\n\
             [query.approved]\n\
             filter = 'status == \"approved\"'\n\
             [profile.platos]\n\
             columns = [{field = \"malt\"}, {field = \"brix\", label = \"Plato\"}]\n\
             sort = [\"-brix\", \"malt\"]\n\
             [export.platos]\n\
             profile = \"platos\"\n\
             format = \"csv\"\n\
             output = \"out/platos.csv\"\n",
        )
    }

    /// Three approved casks and one rejected, plus the project.
    fn corpus(&self) -> &Scratch {
        self.config();
        fs::create_dir_all(self.0.join("out")).unwrap();
        self.sample("keg-01.sample.md", "keg-01", "approved", 12.1, Some(2.801));
        self.sample("keg-02.sample.md", "keg-02", "approved", 12.2, Some(2.802));
        self.sample("keg-03.sample.md", "keg-03", "approved", 12.3, Some(2.803));
        self.sample("keg-04.sample.md", "keg-04", "rejected", 9.7, None);
        self
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The binary, run in a directory, with `COLUMNS` fixed so that layout never
/// depends on whoever's terminal the tests run in.
fn at(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(arguments)
        .current_dir(directory)
        .env("COLUMNS", "200")
        .output()
        .expect("the binary runs")
}

/// The model's description, written as the worker writes it, for a model no
/// Python here imports: `properties` and `tables` as the description holds
/// them, the rest taken from the project's configuration and files.
fn describe(root: &Path, properties: serde_json::Value, tables: serde_json::Value) {
    use samplekit::config::model_runtime as runtime;
    let config = samplekit::config::project_config::load_for(root)
        .unwrap()
        .expect("a configuration");
    let template = runtime::template_of(&config).expect("a model");
    let digest = runtime::digest_of(&template).unwrap();
    let description: runtime::ModelDescription = serde_json::from_value(serde_json::json!({
        "format": runtime::FORMAT,
        "samplekit": runtime::VERSION,
        "model": {
            "path": runtime::described_path(config.root(), template.path()),
            "class": template.class().unwrap_or("Model"),
            "digest": digest.as_str(),
        },
        "properties": properties,
        "tables": tables,
        "attributes": {},
        "figures": [],
    }))
    .unwrap();
    runtime::write_description(config.root(), &description).unwrap();
}

/// A property the model declares, entered, as a description holds it.
fn entered(unit: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "unit": unit, "value": {"from": "entered"}, "uncertainty": null,
        "reads": [], "formula": null, "default": null,
    })
}

fn shell_word(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
fn at_terminal(directory: &Path, arguments: &[&str], answers: &str) -> Output {
    let command = std::iter::once(env!("CARGO_BIN_EXE_samplekit"))
        .chain(arguments.iter().copied())
        .map(shell_word)
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = Command::new("script")
        .args(["--quiet", "--return", "--command", &command, "/dev/null"])
        .current_dir(directory)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script creates a pseudoterminal");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// The binary in a pseudoterminal that **allows** colour, which `at_terminal`
/// deliberately does not: its comparisons are of words, and styling would put
/// escapes through every one of them.
#[cfg(target_os = "linux")]
fn in_colour(directory: &Path, arguments: &[&str]) -> Output {
    let command = std::iter::once(env!("CARGO_BIN_EXE_samplekit"))
        .chain(arguments.iter().copied())
        .map(shell_word)
        .collect::<Vec<_>>()
        .join(" ");
    Command::new("script")
        .args(["--quiet", "--return", "--command", &command, "/dev/null"])
        .current_dir(directory)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .output()
        .expect("script creates a pseudoterminal")
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

/// What the binary said, its paths written with `/` as on Unix: Windows
/// writes them with `\`, and a test reads the path, not its separator.
fn slashed(text: String) -> String {
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// A failure says which kind it is, and names its file once.
#[test]
#[cfg(unix)]
fn what_the_filesystem_refuses_exits_three() {
    let scratch = Scratch::new("refused");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let path = scratch.write(
        "ro.md",
        "---\nschema_version: 1\nname: RO\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let mode = |bits: u32| {
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };

    mode(0o000);
    let unread = at(scratch.path(), &["ro.md", "-c", "malt"]);
    mode(0o644);
    assert_eq!(code(&unread), 3, "{}", err(&unread));
    let said = err(&unread);
    // Named once, and told nothing about its schema: nothing was read, so
    // nothing is known about the contents.
    assert_eq!(said.matches("ro.md").count(), 1, "{said}");
    assert!(!said.contains("predates schema 1"), "{said}");

    mode(0o444);
    let refused = at(scratch.path(), &["set", "ro.md", "malt=9", "--write"]);
    mode(0o644);
    assert_eq!(code(&refused), 3, "{}", err(&refused));

    // Met while scanning a directory, the same refusal exits the same.
    mode(0o000);
    let scanned = at(scratch.path(), &["status", "."]);
    mode(0o644);
    assert_eq!(code(&scanned), 3, "{}", err(&scanned));

    // A file that will not parse is still a data error, with its way out.
    scratch.write("bad.md", "not a sample\n");
    let malformed = at(scratch.path(), &["bad.md", "-c", "malt"]);
    assert_eq!(code(&malformed), 2, "{}", err(&malformed));
}

/// A symbolic loop says so, whether it is named outright or met in a scan.
///
/// `exists` follows links, so a loop answered *false* and was reported as *no
/// such file or directory* — a missing file that is right there.
#[test]
#[cfg(unix)]
fn a_symbolic_loop_is_not_reported_as_a_missing_file() {
    let scratch = Scratch::new("symlink-loop");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    std::os::unix::fs::symlink("b.md", scratch.path().join("a.md")).unwrap();
    std::os::unix::fs::symlink("a.md", scratch.path().join("b.md")).unwrap();

    let named = at(scratch.path(), &["a.md", "-c", "malt"]);
    assert_eq!(code(&named), 3, "{}", err(&named));
    assert!(
        err(&named).to_lowercase().contains("symbolic link"),
        "{}",
        err(&named)
    );
    // The scan already said it correctly; the two now agree.
    let scanned = at(scratch.path(), &["list", "skipped", "."]);
    assert!(
        out(&scanned).to_lowercase().contains("symbolic link"),
        "{}",
        out(&scanned)
    );
    // A genuinely missing path keeps its own words.
    let absent = at(scratch.path(), &["nowhere.md"]);
    assert!(
        err(&absent).contains("no such file or directory"),
        "{}",
        err(&absent)
    );
}

/// A state travels as a column, asked for, and never as a glyph — one column
/// for the row, the same in every row-shaped format.
///
/// The marks `⚠ ✎ ✗` are the terminal's and vanish in a pipe — specified, and
/// documented only as *colour*, so a reader believed they survived.
#[test]
fn status_adds_one_state_column_per_row() {
    let scratch = Scratch::new("status-column");
    scratch.corpus();
    let plain = at(scratch.path(), &["-c", "name,malt", "--csv", "."]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(!out(&plain).contains("state"), "{}", out(&plain));

    let asked = at(
        scratch.path(),
        &["-c", "name,malt", "--csv", "--status", "."],
    );
    assert_eq!(code(&asked), 0, "{}", err(&asked));
    let shown = out(&asked);
    let header = shown.lines().next().unwrap();
    assert!(header.ends_with(",state"), "{shown}");
    assert!(!header.contains("malt_state"), "{shown}");
    // A word, never a glyph: a ✗ in a CSV is something a parser reads as text.
    for glyph in ["⚠", "✎", "✗"] {
        assert!(!shown.contains(glyph), "{shown}");
    }
    // An entered malt needs nothing: the row is current.
    assert!(
        shown.lines().nth(1).unwrap().ends_with(",current"),
        "{shown}"
    );

    // A row with something to act on says what, worst first, and which.
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: drifted\nproperties:\n  \
         malt: {v: 2.0, unit: g, fingerprint: aaaaaaaaaaaa}\n  \
         brix: {v: 1.0, computed: {malt: bbbbbbbbbbbb}}\n  \
         volume: {v: 2.0, computed: {malt: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    let drifted = at(
        scratch.path(),
        &[
            "-c",
            "name,brix,volume",
            "--csv",
            "--status",
            "drifted.sample.md",
        ],
    );
    assert!(
        out(&drifted).contains(",failed: volume; outdated: brix"),
        "{}",
        out(&drifted)
    );
    // The same column on a drawn table, piped, where the marks are lost —
    // the flag used to do nothing there.
    let piped = at(
        scratch.path(),
        &["-c", "name,brix,volume", "--status", "drifted.sample.md"],
    );
    assert_eq!(code(&piped), 0, "{}", err(&piped));
    assert!(out(&piped).contains("state"), "{}", out(&piped));
    assert!(
        out(&piped).contains("failed: volume; outdated: brix"),
        "{}",
        out(&piped)
    );
    // JSON already carries each quantity's, so the flag adds nothing there.
    let json = at(
        scratch.path(),
        &["-c", "name,malt", "--json", "--status", "."],
    );
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(out(&json).contains("\"state\""), "{}", out(&json));
    assert_eq!(
        out(&json),
        out(&at(scratch.path(), &["-c", "name,malt", "--json", "."]))
    );
}

/// A column whose samples do not agree on a unit is refused, not shown with a
/// header that names none.
///
/// Before, the table printed `malt` over `1` and `2` where one was kilograms:
/// two incomparable numbers under a word that promises nothing.
#[test]
fn a_column_that_cannot_name_its_unit_is_refused() {
    let scratch = Scratch::new("mixed-units");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (file, name, unit) in [("a.md", "A", "g"), ("b.md", "B", "kg")] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {{v: 1.0, unit: {unit}}}\n---\n"
            ),
        );
    }
    let refused = at(scratch.path(), &["-c", "malt", "."]);
    assert_eq!(code(&refused), 2, "{}", out(&refused));
    let said = err(&refused);
    assert!(said.contains("cannot say which unit"), "{said}");
    assert!(said.contains("g and kg"), "{said}");
    // And one unit, or none at all, still draws.
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let shown = at(scratch.path(), &["-c", "malt", "."]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert!(out(&shown).contains("malt [g]"), "{}", out(&shown));
}

/// A reader that stops reading ends the command quietly, on **either** stream.
///
/// `samplekit. -v 2>&1 | head -1` exited 101 — Rust's panic code — thirty times
/// out of thirty, and said nothing while doing it: `eprintln!` panics when the
/// write fails, and the panic message goes to the same closed stream. stdout
/// was already handled; stderr was not.
#[test]
#[cfg(unix)]
fn a_closed_reader_ends_the_command_quietly() {
    let scratch = Scratch::new("closed-pipe");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    // Enough refused files that the report outruns a pipe's buffer, so a write
    // genuinely lands after the reader has gone.
    for index in 0..1500 {
        scratch.write(&format!("bad{index}.md"), "not a sample\n");
    }
    let binary = env!("CARGO_BIN_EXE_samplekit");
    for redirection in ["2>&1", "2>/dev/null"] {
        let script =
            format!("\"$1\" . -v {redirection} | head -1 >/dev/null; exit ${{PIPESTATUS[0]}}");
        let output = Command::new("bash")
            .args(["-c", &script, "bash", binary])
            .current_dir(scratch.path())
            .output()
            .expect("bash runs");
        let status = output.status.code().expect("an exit code, not a signal");
        assert_ne!(status, 101, "panicked with {redirection}: {}", err(&output));
        assert_eq!(status, 0, "with {redirection}: {}", err(&output));
    }
    // And with nobody closing anything, the report is still written whole.
    let whole = at(scratch.path(), &["list", "skipped", "."]);
    assert_eq!(code(&whole), 0, "{}", err(&whole));
    assert!(out(&whole).contains("bad1499.md"), "the report was cut");
}

fn table_corpus(scratch: &Scratch) {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "item.sample.md",
        "---\nschema_version: 1\nname: item\nproperties:\n  malt: 2.4\n  foam: {v: 2.014, readings: [2.011, 2.017, 2.013, 2.015]}\ntables:\n  measurements:\n    index: [T]\n    columns:\n      T: {}\n      ph: {}\n      srm: {}\n    rows:\n      - {T: 20, ph: 4.2, srm: 0.1}\n      - {T: 30, ph: 4.8, srm: 0.2}\n---\n",
    );
}

/// The order the rows came out in, by cask name.
fn order(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with("keg-"))
                .map(str::to_string)
        })
        .collect()
}

// ------------------------------------------------------- the target grammar

#[test]
fn query_options_without_a_path_use_the_current_directory() {
    let scratch = Scratch::new("no-path");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "name,malt", "-f", "malt > 12.15"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["keg-02", "keg-03"]);
}

#[test]
fn file_and_directory_are_interchangeable() {
    // Every path-taking command accepts either.
    let scratch = Scratch::new("either");
    scratch.corpus();
    for arguments in [
        vec!["list", "."],
        vec!["list", "keg-01.sample.md"],
        vec!["validate", "."],
        vec!["validate", "keg-01.sample.md"],
        vec!["status", "."],
        vec!["status", "keg-01.sample.md"],
        vec!["."],
        vec!["keg-01.sample.md"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert!(
            code(&output) == 0 || code(&output) == 2,
            "{arguments:?}: {}",
            err(&output)
        );
        assert!(
            !err(&output).contains("is a directory"),
            "{arguments:?}: {}",
            err(&output)
        );
    }
}

#[test]
fn a_file_target_selects_one_sample() {
    // And not the directory containing it.
    let scratch = Scratch::new("one-file");
    scratch.corpus();
    let output = at(scratch.path(), &["keg-01.sample.md", "-c", "name,malt"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["keg-01"]);
}

#[test]
fn a_profile_is_explicit() {
    let scratch = Scratch::new("single");
    scratch.corpus();
    let shape = at(scratch.path(), &["--profile", "platos"]);
    assert_eq!(code(&shape), 0, "{}", err(&shape));
    assert!(out(&shape).contains("Plato"), "{}", out(&shape));
}

#[test]
fn a_profile_name_that_is_a_path_is_unambiguous() {
    let scratch = Scratch::new("ambiguous");
    scratch.corpus();
    fs::create_dir_all(scratch.path().join("platos")).unwrap();
    let profile = at(scratch.path(), &["--profile", "platos"]);
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    let path = at(scratch.path(), &["platos"]);
    assert_eq!(code(&path), 0, "{}", err(&path));
}

#[test]
fn a_missing_implicit_target_is_an_io_error() {
    let scratch = Scratch::new("neither");
    scratch.corpus();
    let output = at(scratch.path(), &["platso"]);
    assert_eq!(code(&output), 3, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("no such file"), "{message}");
}

#[test]
fn a_sub_collection_finds_its_parent_configuration() {
    // The upward search, from the command line: a directory with no
    // `.samplekitrc` of its own uses the nearest one above it.
    let scratch = Scratch::new("upward");
    scratch.config();
    scratch.sample("sub/one.sample.md", "keg-07", "approved", 5.0, Some(2.5));
    let output = at(&scratch.path().join("sub"), &["-c", "brix"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    // The parent declares the display form and the precision.
    assert!(out(&output).contains("g/L"), "{}", out(&output));
    assert!(out(&output).contains("2.5000"), "{}", out(&output));
}

// --------------------------------------------------- exit codes and streams

#[test]
fn empty_result_exits_zero() {
    // A valid question with no matches is a valid answer.
    let scratch = Scratch::new("empty");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt", "-f", "malt > 9000"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(order(&out(&output)).is_empty(), "{}", out(&output));
}

#[test]
fn unknown_field_exits_one() {
    // And prints a suggestion: the question was malformed.
    let scratch = Scratch::new("unknown-field");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "maltt"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("maltt"), "{message}");
    assert!(message.contains("malt"), "{message}");
}

#[test]
fn parse_failure_exits_two() {
    // Distinguished from a usage error, and naming the file. A file named on
    // the command line is a failure; one found in a directory is set aside.
    let scratch = Scratch::new("parse");
    scratch.corpus();
    scratch.write(
        "broken.sample.md",
        "---\nschema_version: 1\nname: {a: 1}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["broken.sample.md", "-c", "malt"]);
    assert_eq!(code(&output), 2, "{}", out(&output));
    assert!(
        err(&output).contains("broken.sample.md"),
        "{}",
        err(&output)
    );
}

#[test]
fn missing_path_exits_three() {
    let scratch = Scratch::new("missing");
    scratch.corpus();
    let output = at(scratch.path(), &["./no-such-directory"]);
    assert_eq!(code(&output), 3, "{}", out(&output));
    assert!(
        err(&output).contains("no-such-directory"),
        "{}",
        err(&output)
    );
}

#[test]
fn diagnostics_go_to_stderr() {
    // stdout stays clean under redirection, which is what makes
    // `samplekit … > out.csv` usable.
    let scratch = Scratch::new("streams");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "maltt"]);
    assert!(out(&output).is_empty(), "{}", out(&output));
    assert!(!err(&output).is_empty());
}

#[test]
fn warnings_do_not_change_the_exit_code() {
    // A skipped file still exits 0, and says so on stderr.
    let scratch = Scratch::new("warnings");
    scratch.corpus();
    scratch.write("notes.md", "Just prose, no frontmatter.\n");
    let output = at(scratch.path(), &["-c", "malt", "-v"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(err(&output).contains("notes.md"), "{}", err(&output));
    assert!(err(&output).contains("frontmatter"), "{}", err(&output));
}

#[test]
fn help_is_available_for_every_command() {
    let scratch = Scratch::new("help");
    scratch.corpus();
    for arguments in [
        vec!["--help"],
        vec!["-h"],
        vec!["tag", "--help"],
        vec!["explain", "--help"],
        vec!["list", "--help"],
        vec!["status", "--help"],
        vec!["view", "--help"],
        vec!["export", "--help"],
        vec!["validate", "--help"],
        vec!["init", "--help"],
        vec!["new", "--help"],
        vec!["set", "--help"],
        vec!["completions", "--help"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        assert!(!out(&output).is_empty(), "{arguments:?}");
    }
}

#[test]
fn a_verb_help_describes_only_that_verb() {
    // A command's page describes that command rather than the overview.
    let scratch = Scratch::new("verb-help");
    let overview = out(&at(scratch.path(), &["--help"]));
    for verb in [
        "list",
        "status",
        "explain",
        "view",
        "export",
        "validate",
        "tag",
        "init",
        "new",
        "set",
        "completions",
    ] {
        let page = out(&at(scratch.path(), &[verb, "--help"]));
        assert_ne!(page, overview, "{verb} fell back to the overview");
        assert!(
            page.contains(&format!("Usage: samplekit {verb}")),
            "{verb}: {page}"
        );
        // Not the overview's command list.
        assert!(page.contains("--help"), "{verb}: {page}");
    }
}

/// Every help page, by the arguments that reach it.
const HELP_PAGES: &[&[&str]] = &[
    &[],
    &["list"],
    &["status"],
    &["explain"],
    &["view"],
    &["export"],
    &["validate"],
    &["tag"],
    &["tag", "add"],
    &["tag", "remove"],
    &["tag", "rename"],
    &["init"],
    &["new"],
    &["set"],
    &["completions"],
];

/// The sections of a `--help` page, in their order.
const HELP_SECTIONS: [&str; 5] = [
    "Selection:",
    "Shape:",
    "Presentation:",
    "Output:",
    "General:",
];

fn help_page(scratch: &Scratch, command: &[&str], flag: &str) -> String {
    let arguments: Vec<&str> = command.iter().copied().chain([flag]).collect();
    let output = at(scratch.path(), &arguments);
    assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
    out(&output)
}

/// The long name of every option a help page lists, in the order it lists
/// them. An option line is indented by at most six columns; a description on
/// the line below is indented further.
fn options_listed(page: &str) -> Vec<String> {
    page.lines()
        .filter(|line| line.len() - line.trim_start().len() <= 6)
        .filter(|line| line.trim_start().starts_with('-'))
        .filter_map(|line| line.split_whitespace().find(|word| word.starts_with("--")))
        .map(|word| word.trim_end_matches(',').to_string())
        .collect()
}

#[test]
fn short_help_is_one_unsectioned_list() {
    // `-h` is a summary, so it has no sections — and, as amended, every option
    // keeps its description.
    let scratch = Scratch::new("short-help");
    for command in HELP_PAGES {
        let page = help_page(&scratch, command, "-h");
        for section in HELP_SECTIONS {
            assert!(
                !page.lines().any(|line| line == section),
                "{command:?} has {section}: {page}"
            );
        }
        for line in page
            .lines()
            .filter(|line| line.trim_start().starts_with('-'))
        {
            // The flag, at least two spaces, then a description.
            let parts = line
                .trim()
                .split("  ")
                .filter(|part| !part.trim().is_empty())
                .count();
            assert!(parts >= 2, "{command:?}, no description: {line}");
        }
    }
    let page = help_page(&scratch, &[], "-h");
    assert!(
        page.lines()
            .any(|line| line.contains("--filter <EXPR>") && line.contains("repeatable")),
        "{page}"
    );
}

#[test]
fn long_help_groups_options_into_sections() {
    let scratch = Scratch::new("long-help");
    let page = help_page(&scratch, &[], "--help");
    let lines: Vec<&str> = page.lines().collect();
    let sections: Vec<usize> = HELP_SECTIONS
        .iter()
        .map(|section| {
            lines
                .iter()
                .position(|line| line == section)
                .unwrap_or_else(|| panic!("{section} is missing: {page}"))
        })
        .collect();
    assert!(sections.windows(2).all(|pair| pair[0] < pair[1]), "{page}");
    // An option with no section would fall under clap's own `Options:`.
    assert!(!lines.contains(&"Options:"), "{page}");

    for (option, section) in [
        ("--filter", 0),
        ("--query", 0),
        ("--reverse", 0),
        ("--columns", 1),
        ("--summary", 1),
        ("--precision", 2),
        ("--width", 2),
        ("--csv", 3),
        ("--write", 3),
        ("--verbose", 4),
        ("--version", 4),
    ] {
        let line = lines
            .iter()
            .position(|line| {
                line.len() - line.trim_start().len() <= 6
                    && line.trim_start().starts_with('-')
                    && line
                        .split_whitespace()
                        .any(|word| word.trim_end_matches(',') == option)
            })
            .unwrap_or_else(|| panic!("{option} is missing: {page}"));
        assert!(sections[section] < line, "{option}: {page}");
        if let Some(next) = sections.get(section + 1) {
            assert!(line < *next, "{option}: {page}");
        }
    }
    // Both selection options name the one conjunction they join.
    assert_eq!(
        page.matches("every --filter and every --query must hold")
            .count(),
        2,
        "{page}"
    );
}

#[test]
fn short_and_long_help_list_options_in_one_order() {
    let scratch = Scratch::new("help-order");
    for command in HELP_PAGES {
        let short = options_listed(&help_page(&scratch, command, "-h"));
        let long = options_listed(&help_page(&scratch, command, "--help"));
        assert!(
            short.contains(&"--help".to_string()),
            "{command:?}: {short:?}"
        );
        assert_eq!(short, long, "{command:?}");
    }
}

#[test]
fn help_command_prints_the_long_help() {
    let scratch = Scratch::new("help-command");
    for command in [&[][..], &["list"][..], &["tag"][..], &["tag", "add"][..]] {
        let mut arguments = vec!["help"];
        arguments.extend_from_slice(command);
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        assert_eq!(
            out(&output),
            help_page(&scratch, command, "--help"),
            "{arguments:?}"
        );
    }
    // A verb's own `help` reaches the same page.
    let output = at(scratch.path(), &["tag", "help", "add"]);
    assert_eq!(out(&output), help_page(&scratch, &["tag", "add"], "--help"));
}

#[test]
fn version_uses_the_package_version() {
    let scratch = Scratch::new("version");
    let output = at(scratch.path(), &["--version"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).trim(),
        format!("samplekit {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn irrelevant_options_are_rejected_by_each_command() {
    let scratch = Scratch::new("strict-command-options");
    scratch.corpus();
    for arguments in [
        vec!["list", "-f", "malt > 1"],
        vec!["validate", "--csv"],
        vec!["explain", "keg-01.sample.md", "malt", "--overwrite"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", out(&output));
        assert!(
            err(&output).contains("unexpected argument"),
            "{arguments:?}: {}",
            err(&output)
        );
    }
}

#[test]
fn completions_command_emits_a_dynamic_bridge() {
    let scratch = Scratch::new("completion-bridge");
    let output = at(scratch.path(), &["completions", "zsh"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("#compdef samplekit"), "{text}");
    assert!(text.contains("COMPLETE=\"zsh\""), "{text}");
    assert!(
        text.contains("_samplekit_compadd samplekit-values ' '"),
        "{text}"
    );
}

#[test]
fn dynamic_completion_reads_project_names() {
    let scratch = Scratch::new("dynamic-completion");
    scratch.corpus();
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "--profile", "p"])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("platos"), "{}", out(&output));
}

#[test]
fn dynamic_completion_uses_the_target_already_on_the_line() {
    let scratch = Scratch::new("completion-target");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[profile.remote]\ncolumns = [{field = \"special\"}]\n",
    );
    scratch.write(
        "project/remote.sample.md",
        "---\nschema_version: 1\nname: remote\nproperties:\n  special: 4.2\n---\nN.\n",
    );
    let complete = |words: &[&str], index: &str| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", index)
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap()
    };
    let profile = complete(&["samplekit", "project", "--profile", "r"], "3");
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    assert!(out(&profile).contains("remote"), "{}", out(&profile));
    let column = complete(&["samplekit", "project", "-c", "s"], "3");
    assert_eq!(code(&column), 0, "{}", err(&column));
    assert!(out(&column).contains("special"), "{}", out(&column));
}

#[test]
fn completions_is_hidden_and_still_runs() {
    // Shell completion is set up by `init`; the command stays usable.
    let scratch = Scratch::new("completions-hidden");
    scratch.corpus();
    for flag in ["-h", "--help"] {
        let page = out(&at(scratch.path(), &[flag]));
        let listed = |verb: &str| {
            page.lines()
                .any(|line| line.trim_start().starts_with(&format!("{verb} ")))
        };
        assert!(listed("list"), "{flag}: {page}");
        assert!(!listed("completions"), "{flag}: {page}");
    }
    let completion = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "1")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    let candidates = out(&completion);
    assert!(
        candidates.lines().any(|line| line.starts_with("list:")),
        "{candidates}"
    );
    assert!(
        !candidates
            .lines()
            .any(|line| line.starts_with("completions")),
        "{candidates}"
    );

    let output = at(scratch.path(), &["completions", "zsh"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).is_empty());
}

#[test]
fn tag_completes_the_tags_the_collection_holds() {
    // A tag argument names a tag, and the collection's own vocabulary is what
    // `list tags` prints. Offering the spellings in use is what keeps `refernce`
    // from becoming a second tag beside `reference`.
    let scratch = Scratch::new("tag-completion");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\ntags: [reference, dry_hopped]\n---\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: B\ntags: [reference, high_gravity]\n---\n",
    );
    for words in [
        vec!["samplekit", "tag", "add", ""],
        vec!["samplekit", "tag", "remove", ""],
        vec!["samplekit", "tag", "rename", ""],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(&words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "3")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{}", err(&output));
        let lines: Vec<String> = out(&output).lines().map(str::to_string).collect();
        for tag in ["reference", "dry_hopped", "high_gravity"] {
            assert!(
                lines.iter().any(|line| line == tag),
                "{words:?} offered {lines:?}"
            );
        }
    }
    // The new name of a rename takes one that need not exist yet, and is still
    // offered what does.
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "tag", "rename", "reference", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).lines().any(|line| line == "dry_hopped"),
        "{}",
        out(&output)
    );
}

#[test]
fn completion_lists_commands_before_filesystem_candidates() {
    let scratch = Scratch::new("completion-order");
    scratch.corpus();
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "1")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    let lines: Vec<_> = out(&output).lines().map(str::to_string).collect();
    let command = lines
        .iter()
        .position(|line| line.starts_with("list:"))
        .unwrap();
    let path = lines.iter().position(|line| line == ".").unwrap();
    assert!(command < path, "{lines:?}");
}

#[test]
fn completion_never_extends_an_unknown_short_option() {
    let scratch = Scratch::new("completion-invalid-short");
    for (words, index) in [
        (vec!["samplekit", "-p"], "1"),
        (vec!["samplekit", "list", "-c"], "2"),
        (vec!["samplekit", "status", "-o"], "2"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(&words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", index)
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{words:?}: {}", err(&output));
        assert!(out(&output).is_empty(), "{words:?}: {}", out(&output));
    }
}

#[test]
fn dynamic_completion_walks_a_table_field_segment_by_segment() {
    let scratch = Scratch::new("completion-table-segments");
    table_corpus(&scratch);
    let complete = |current: &str| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["--", "samplekit", ".", "-c", current])
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "3")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap()
    };

    let first = out(&complete(""));
    assert!(first.lines().any(|line| line == "malt"), "{first}");
    assert!(
        first.lines().any(|line| line == "measurements:table"),
        "{first}"
    );
    assert!(!first.contains("measurements.ph[20]"), "{first}");

    let columns = out(&complete("measurements."));
    assert!(
        columns.lines().any(|line| line == "measurements.ph["),
        "{columns}"
    );
    assert!(!columns.contains("[20]"), "{columns}");

    let indexes = out(&complete("measurements.ph["));
    assert!(
        indexes.lines().any(|line| line == "measurements.ph[20]"),
        "{indexes}"
    );

    let direct = out(&complete("ph"));
    assert!(direct.contains("measurements.ph[20]:cell"), "{direct}");
    assert!(direct.contains("measurements.ph[30]:cell"), "{direct}");

    let exact = out(&complete("foam"));
    assert!(exact.contains("foam.v"), "{exact}");
    assert!(exact.contains("foam.stats:statistics"), "{exact}");

    let stored = out(&complete("malt"));
    assert!(!stored.contains("malt.stats"), "{stored}");

    let stats = out(&complete("foam.stats."));
    assert!(stats.contains("foam.stats.mean"), "{stats}");
    assert!(!stats.contains("<statistic>"), "{stats}");
}

#[test]
fn column_completion_keeps_a_comma_attached() {
    let scratch = Scratch::new("completion-column-nospace");
    table_corpus(&scratch);
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(bridge.contains("${words[$CURRENT-1]} == -c"), "{bridge}");
    assert!(
        bridge.contains("_samplekit_compadd samplekit-values ''"),
        "{bridge}"
    );
}

#[test]
fn column_completion_restarts_after_a_comma() {
    let scratch = Scratch::new("completion-column-comma");
    table_corpus(&scratch);
    for current in ["malt,", "malt,measurements.ph[20],"] {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["--", "samplekit", ".", "-c", current])
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "3")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{current}: {}", err(&output));
        let text = out(&output);
        assert!(
            text.lines()
                .any(|line| line == format!("{current}measurements:table")),
            "{current}: {text}"
        );
    }
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(bridge.contains("compset -P '*,'"), "{bridge}");
}

#[test]
fn sort_completion_restarts_after_a_comma() {
    let scratch = Scratch::new("completion-sort-comma");
    table_corpus(&scratch);
    for current in ["malt,", "malt,-"] {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["--", "samplekit", ".", "-s", current])
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "3")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{current}: {}", err(&output));
        let text = out(&output);
        assert!(
            text.lines()
                .any(|line| line == format!("{current}measurements:table")),
            "{current}: {text}"
        );
    }
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(bridge.contains("${words[$CURRENT-1]} == -s"), "{bridge}");
}

#[test]
fn zsh_bridge_unquotes_a_column_before_completing_its_index() {
    let scratch = Scratch::new("completion-unquote-column");
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(
        bridge.contains("completion_words[$CURRENT]=${(Q)completion_words[$CURRENT]}"),
        "{bridge}"
    );
    assert!(bridge.contains("-- \"${completion_words[@]}\""), "{bridge}");
}

#[test]
fn zsh_bridge_groups_commands_before_paths_and_options() {
    let scratch = Scratch::new("completion-zsh-order");
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(
        bridge.contains("_samplekit_compadd samplekit-commands ' '"),
        "{bridge}"
    );
    let root = bridge
        .split_once("        else\n            (( ${#commands} ))")
        .unwrap()
        .1;
    let commands = root.find("samplekit-commands ' '").unwrap();
    let paths = root.find("samplekit-paths ''").unwrap();
    let options = root.find("samplekit-options ' '").unwrap();
    assert!(commands < paths && paths < options, "{bridge}");
    assert!(bridge.contains("compadd -V"), "{bridge}");
    assert!(!bridge.contains("compadd -V \"$group\" -X"), "{bridge}");
    assert!(bridge.contains("-i \"$hidden\""), "{bridge}");
}

#[test]
fn zsh_bridge_displays_stats_without_its_delimiter() {
    let scratch = Scratch::new("completion-zsh-statistics");
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(
        bridge.contains("*':statistics'"),
        "statistics candidates need their own group: {bridge}"
    );
    assert!(
        bridge.contains("_samplekit_compadd samplekit-statistics '.'"),
        "the delimiter must be an insertion suffix: {bridge}"
    );
}

#[test]
fn zsh_bridge_puts_folded_cells_last() {
    let scratch = Scratch::new("completion-zsh-cells-last");
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    let fields = bridge.find("samplekit-values ''").unwrap();
    let tables = bridge.find("samplekit-tables '.'").unwrap();
    let options = bridge.find("samplekit-options ' '").unwrap();
    let cells = bridge.find("samplekit-cells ''").unwrap();
    assert!(
        fields < tables && tables < options && options < cells,
        "{bridge}"
    );
}

#[test]
fn a_misplaced_subcommand_suggests_the_valid_order() {
    let scratch = Scratch::new("misplaced-subcommand");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("'status' is a subcommand and must come before the target '.'"),
        "{}",
        err(&output)
    );
    assert!(
        err(&output).contains("use: samplekit status ."),
        "{}",
        err(&output)
    );
}

// ---------------------------------------------------------------- selection

#[test]
fn named_and_inline_filters_compose() {
    // `-Q` and `-f` conjoin rather than one silently winning, which is what the
    // prototype did.
    let scratch = Scratch::new("compose");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            "--query",
            "approved",
            "-f",
            "malt > 12.15",
            "-c",
            "name,malt",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["keg-02", "keg-03"]);
}

#[test]
fn repeated_filters_are_all_conjoined() {
    let scratch = Scratch::new("repeat-filter");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-f", "malt > 12.05", "-f", "malt < 12.25"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(out(&output).lines().count(), 2, "{}", out(&output));
}

#[test]
fn unknown_query_lists_the_available_ones() {
    let scratch = Scratch::new("unknown-query");
    scratch.corpus();
    let output = at(scratch.path(), &["--query", "aproved", "-c", "malt"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("aproved"), "{message}");
    assert!(message.contains("approved"), "{message}");
}

#[test]
fn query_option_narrows_the_base() {
    // A path and a filter compose: the path says where to start.
    let scratch = Scratch::new("narrow");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "--query", "approved", "-c", "name,malt"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["keg-01", "keg-02", "keg-03"]);
}

#[test]
fn a_malformed_filter_points_at_the_character() {
    // The caret is the message: a position on its own is a number.
    let scratch = Scratch::new("caret");
    scratch.corpus();
    let output = at(scratch.path(), &["-f", "malt >", "-c", "malt"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains('^'), "{}", err(&output));
}

#[test]
fn every_filter_problem_is_reported_at_once() {
    // Two unknown fields produce two lines: clearing them one run at a time is
    // the defect one-error-at-a-time diagnostics always are.
    let scratch = Scratch::new("all-at-once");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-f", "maltt > 1 and brixx < 3", "-c", "malt"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("maltt"), "{message}");
    assert!(message.contains("brixx"), "{message}");
}

// --------------------------------------------------------- columns and order

#[test]
fn repeated_sort_flags_make_successive_keys() {
    let scratch = Scratch::new("bundled");
    scratch.config();
    scratch.sample("a.sample.md", "keg-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "keg-02", "approved", 12.2, Some(2.8));
    let output = at(scratch.path(), &["-s", "brix", "-s", "-malt", "-c", "name"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["keg-02", "keg-01"]);
}

#[test]
fn standalone_sort_takes_its_own_value() {
    // `-s c -p a,b` sorts by `c`: the two cases are told apart syntactically,
    // so nothing is guessed.
    let scratch = Scratch::new("standalone");
    scratch.corpus();
    let output = at(scratch.path(), &["-s", "-malt", "-c", "name,brix"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        order(&out(&output)),
        ["keg-03", "keg-02", "keg-01", "keg-04"]
    );
}

#[test]
fn sort_takes_a_comma_list_like_columns() {
    let scratch = Scratch::new("sort-comma-list");
    scratch.config();
    scratch.sample("a.sample.md", "keg-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "keg-02", "approved", 12.2, Some(2.8));
    let listed = at(scratch.path(), &["-s", "brix,-malt", "-c", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let repeated = at(scratch.path(), &["-s", "brix", "-s", "-malt", "-c", "name"]);
    assert_eq!(out(&listed), out(&repeated));
    assert_eq!(order(&out(&listed)), ["keg-02", "keg-01"]);
}

#[test]
fn an_empty_sort_key_is_refused() {
    let scratch = Scratch::new("sort-empty-key");
    scratch.corpus();
    let output = at(scratch.path(), &["-s", "brix,", "-c", "name"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("a sort key cannot be empty"),
        "{}",
        err(&output)
    );
}

#[test]
fn properties_accepts_repeated_and_comma_separated() {
    let scratch = Scratch::new("repeated");
    scratch.corpus();
    let comma = at(scratch.path(), &["-c", "malt,brix"]);
    let repeated = at(scratch.path(), &["-c", "malt", "-c", "brix"]);
    assert_eq!(code(&comma), 0, "{}", err(&comma));
    assert_eq!(out(&comma), out(&repeated));
}

#[test]
fn properties_never_produces_a_literal_comma_column() {
    // The regression test: v1 documented `-p a -p b` and silently accepted
    // `-p a,b` as one field called `a,b`.
    let scratch = Scratch::new("comma-column");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt,brix"]);
    assert!(!out(&output).contains("malt,brix"), "{}", out(&output));
    assert!(err(&output).is_empty(), "{}", err(&output));
}

#[test]
fn an_unknown_column_is_refused_before_anything_is_rendered() {
    // No column of dashes, ever: this is the v1 defect the project exists to
    // remove. Nothing is printed at all.
    let scratch = Scratch::new("no-dashes");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "brix,vfy"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(out(&output).is_empty(), "{}", out(&output));
    assert!(err(&output).contains("vfy"), "{}", err(&output));
}

#[test]
#[cfg(target_os = "linux")]
fn interactive_column_typos_can_be_corrected_in_one_pass() {
    let scratch = Scratch::new("correct-two-columns");
    scratch.corpus();
    let output = at_terminal(
        scratch.path(),
        &["-c", "tag,mal", "--table-style", "plain"],
        "\n\n",
    );
    assert_eq!(code(&output), 0, "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("unknown field 'tag'; use 'tags'? [Y/n]"),
        "{text}"
    );
    assert!(
        text.contains("unknown field 'mal'; use 'malt'? [Y/n]"),
        "{text}"
    );
    assert!(text.contains("tags"), "{text}");
    assert!(text.contains("malt"), "{text}");
}

#[test]
fn non_interactive_column_typos_never_prompt() {
    let scratch = Scratch::new("no-column-prompt");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "tag"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(!err(&output).contains("[Y/n]"), "{}", err(&output));
}

#[test]
fn suggested_field_error_shows_count_instead_of_prefix() {
    let scratch = Scratch::new("suggested-field-count");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "tag"]);
    let message = err(&output);
    assert!(message.contains("did you mean: 'tags'?"), "{message}");
    assert!(
        message.contains("fields available — samplekit list fields"),
        "{message}"
    );
    assert!(!message.contains("available: name,"), "{message}");
}

#[test]
fn unsuggested_field_error_shows_prefix() {
    let scratch = Scratch::new("unsuggested-field-prefix");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "zzzzzzzz"]);
    let message = err(&output);
    assert!(!message.contains("did you mean"), "{message}");
    assert!(
        message.contains("available: name, path, filename"),
        "{message}"
    );
    assert!(
        message.contains("fields available — samplekit list fields"),
        "{message}"
    );
}

#[test]
fn a_bare_table_column_explains_how_to_address_a_cell() {
    let scratch = Scratch::new("bare-table-column");
    table_corpus(&scratch);
    for field in ["measurements", "measurements."] {
        let output = at(scratch.path(), &["-c", field]);
        assert_eq!(code(&output), 1, "{field}: {}", out(&output));
        let message = err(&output);
        assert!(
            message.contains("'measurements' names a table"),
            "{field}: {message}"
        );
        assert!(message.contains("index    T: 20  30"), "{message}");
        assert!(message.contains("columns  ph  srm"), "{message}");
        assert!(message.contains("'measurements.ph[20]'"), "{message}");
        assert!(!message.contains("unknown field"), "{field}: {message}");
    }
}

#[test]
fn an_explicit_channel_is_not_rendered_as_a_quantity() {
    let scratch = Scratch::new("explicit-channel");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            "-c",
            "name,brix.v,brix.u,brix.unit,brix.symbol",
            "--table-style",
            "plain",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let first = text.lines().find(|line| line.contains("keg-01")).unwrap();
    assert!(!first.contains('±'), "{first}");
    for expected in ["2.8010", "0.0012", "g/L", "Bx"] {
        assert!(first.contains(expected), "missing {expected}: {first}");
    }
    let header = text.lines().next().unwrap();
    assert!(!header.contains("brix.unit ["), "{header}");
    assert!(!header.contains("brix.symbol ["), "{header}");
}

#[test]
fn an_empty_selection_still_checks_columns_against_the_collection() {
    // A vocabulary belongs to a collection, not to a filter's result: checking
    // the selection turns a well-formed question with an empty answer into a
    // false error, and exit 1 where the project promises 0.
    let scratch = Scratch::new("empty-vocabulary");
    scratch.corpus();
    let known = at(scratch.path(), &["-f", "malt > 9000", "-c", "brix"]);
    assert_eq!(code(&known), 0, "{}", err(&known));
    assert!(err(&known).is_empty(), "{}", err(&known));
    // And an unknown column is still unknown when nothing matched.
    let unknown = at(scratch.path(), &["-f", "malt > 9000", "-c", "brixx"]);
    assert_eq!(code(&unknown), 1, "{}", out(&unknown));
}

#[test]
fn reverse_flips_every_key() {
    // Including a profile's tie-break: a flag that reversed the primary key and
    // left the rest alone would produce an order nobody asked for.
    let scratch = Scratch::new("reverse");
    scratch.config();
    // Two casks share a plato and differ in malt, so the profile's
    // second key decides.
    scratch.sample("a.sample.md", "keg-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "keg-02", "approved", 12.2, Some(2.8));
    let plain = out(&at(scratch.path(), &["--profile", "platos"]));
    let reversed = out(&at(scratch.path(), &["--profile", "platos", "-r"]));
    assert!(
        plain.find("12.1").unwrap() < plain.find("12.2").unwrap(),
        "{plain}"
    );
    assert!(
        reversed.find("12.2").unwrap() < reversed.find("12.1").unwrap(),
        "{reversed}"
    );
}

#[test]
fn reverse_with_nothing_to_reverse_is_refused() {
    // With no key given and none from a profile, -r did nothing, and said
    // nothing.
    let scratch = Scratch::new("reverse-nothing");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt", "-r"]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("--sort"), "{}", err(&output));
    assert!(out(&output).is_empty(), "{}", out(&output));
    // With a key, it reverses it.
    let sorted = at(scratch.path(), &["-c", "malt", "-s", "malt", "-r"]);
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    // An export's profile declares its order, and -r reverses that one.
    let plain = at(scratch.path(), &["export", "platos", "-o", "-"]);
    let reversed = at(scratch.path(), &["export", "platos", "-r", "-o", "-"]);
    assert_eq!(code(&reversed), 0, "{}", err(&reversed));
    // A sample without the key stays last, in either direction.
    let rows = |output: &std::process::Output| -> Vec<String> {
        out(output)
            .lines()
            .skip(1)
            .filter(|row| !row.ends_with(",,"))
            .map(str::to_string)
            .collect()
    };
    let mut expected = rows(&plain);
    expected.reverse();
    assert_eq!(rows(&reversed), expected);
}

// ---------------------------------------------------------------- rendering

#[test]
fn csv_output_is_deterministic() {
    // Byte-identical across runs — required for a Makefile.
    let scratch = Scratch::new("deterministic");
    scratch.corpus();
    let first = at(scratch.path(), &["--profile", "platos", "--csv"]);
    let second = at(scratch.path(), &["--profile", "platos", "--csv"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert_eq!(out(&first), out(&second));
    assert!(out(&first).starts_with("malt_value"), "{}", out(&first));
}

#[test]
fn name_is_never_an_implicit_table_column() {
    let scratch = Scratch::new("explicit-name");
    scratch.corpus();
    let without = out(&at(scratch.path(), &["-c", "malt", "--csv"]));
    assert!(!without.contains("name"), "{without}");
    assert!(!without.contains("keg-01"), "{without}");
    let with = out(&at(scratch.path(), &["-c", "name,malt", "--csv"]));
    assert!(with.starts_with("name,"), "{with}");
    assert!(with.contains("keg-01"), "{with}");
}

/// `-o` is a file, and a file is a change: previewed, then written with
/// `--write`, like everything else the tool changes.
///
/// It used to write at once and refuse only an existing destination, with an
/// `--overwrite` of its own — two shapes to learn for one act.
#[test]
fn output_option_previews_then_atomically_replaces_a_file() {
    let scratch = Scratch::new("output-file");
    scratch.corpus();
    let preview = at(
        scratch.path(),
        &["-c", "name,malt", "--csv", "-o", "data.csv"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("nothing written"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.path().join("data.csv").exists(), "a preview wrote");

    let first = at(
        scratch.path(),
        &["-c", "name,malt", "--csv", "-o", "data.csv", "--write"],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(out(&first).is_empty(), "{}", out(&first));
    let before = fs::read_to_string(scratch.path().join("data.csv")).unwrap();

    // The second preview says the file is there, and leaves it alone.
    let again = at(scratch.path(), &["-c", "name", "--csv", "-o", "data.csv"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("--write replaces it"),
        "{}",
        out(&again)
    );
    assert_eq!(
        fs::read_to_string(scratch.path().join("data.csv")).unwrap(),
        before
    );

    let replaced = at(
        scratch.path(),
        &["-c", "name", "--csv", "-o", "data.csv", "--write"],
    );
    assert_eq!(code(&replaced), 0, "{}", err(&replaced));
    assert_ne!(
        fs::read_to_string(scratch.path().join("data.csv")).unwrap(),
        before
    );
}

#[test]
fn piped_output_is_not_truncated() {
    // Regardless of terminal width: output truncated for display and then
    // parsed is data loss.
    let scratch = Scratch::new("not-truncated");
    scratch.config();
    scratch.write(
        "wide.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         description: a deliberately very long text value that no terminal of any \
         reasonable width could possibly show in full without cutting it short\n---\nN.\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["-c", "description"])
        .current_dir(scratch.path())
        .env("COLUMNS", "40")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("cutting it short"),
        "{}",
        out(&output)
    );
    assert!(!out(&output).contains('\u{2026}'), "{}", out(&output));
}

#[test]
fn a_data_format_carries_no_placeholder_for_an_absent_value() {
    // A dash in a CSV is a *value*, and one that poisons a numeric column.
    let scratch = Scratch::new("no-placeholder");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "brix", "--csv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(!text.contains('\u{2014}'), "{text}");
    assert!(!text.contains("keg-04"), "{text}");
    assert!(text.lines().any(|line| line.starts_with(',')), "{text}");
}

#[test]
fn the_table_style_comes_from_the_project_and_the_flag_wins() {
    // An argument beats a stored setting, which beats the default.
    let scratch = Scratch::new("style");
    scratch.corpus();
    let stored = at(scratch.path(), &["-c", "malt"]);
    assert!(out(&stored).contains('\u{250c}'), "{}", out(&stored));
    let overridden = at(scratch.path(), &["-c", "malt", "--table-style", "plain"]);
    assert!(
        !out(&overridden).contains('\u{250c}'),
        "{}",
        out(&overridden)
    );
    // A style adds a border and never a digit.
    assert!(out(&overridden).contains("12.10"), "{}", out(&overridden));
}

#[test]
fn table_style_completion_lists_every_style() {
    let scratch = Scratch::new("complete-table-style");
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "--table-style", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(out(&output).lines().collect::<Vec<_>>(), ["plain", "boxed"]);
}

#[test]
fn render_style_overrides_the_project_for_one_invocation() {
    let scratch = Scratch::new("semantic-style");
    scratch.corpus();

    let plain = at(scratch.path(), &["-c", "brix,brix.symbol"]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(out(&plain).contains("g/L"), "{}", out(&plain));
    assert!(out(&plain).contains("Bx"), "{}", out(&plain));

    let math = at(
        scratch.path(),
        &["-c", "brix,brix.symbol", "--style", "math"],
    );
    assert_eq!(code(&math), 0, "{}", err(&math));
    let text = out(&math);
    assert!(text.contains("\\mathrm{g\\,L^{-1}}"), "{text}");
    assert!(text.contains("\\pm"), "{text}");
    assert!(text.contains("\\brix"), "{text}");

    let view = at(scratch.path(), &["view", ".", "--style", "math"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(out(&view).contains("\\pm"), "{}", out(&view));
    assert!(
        out(&view).contains("\\mathrm{g\\,L^{-1}}"),
        "{}",
        out(&view)
    );

    let explain = at(
        scratch.path(),
        &["explain", "keg-01.sample.md", "brix", "--style", "math"],
    );
    assert_eq!(code(&explain), 0, "{}", err(&explain));
    assert!(out(&explain).contains("\\pm"), "{}", out(&explain));

    // The transient override never changes the stored project preference.
    let again = at(scratch.path(), &["-c", "brix,brix.symbol"]);
    assert!(out(&again).contains("g/L"), "{}", out(&again));
    assert!(!out(&again).contains("\\pm"), "{}", out(&again));
}

#[test]
fn render_style_completion_lists_project_styles() {
    let scratch = Scratch::new("complete-render-style");
    scratch.corpus();
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "--style", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(out(&output).lines().collect::<Vec<_>>(), ["plain", "math"]);
}

#[test]
fn global_precision_overrides_declared_presentation() {
    let scratch = Scratch::new("global-precision");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt,brix", "--precision", ".3f"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    // `malt` stores .2f and brix is declared .4f; the explicit invocation wins.
    assert!(text.contains("12.100 ± 0.050"), "{text}");
    assert!(text.contains("2.801 ± 0.001"), "{text}");

    let profile = at(
        scratch.path(),
        &["--profile", "platos", "--precision", ".3f"],
    );
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    assert!(
        out(&profile).contains("12.100 ± 0.050"),
        "{}",
        out(&profile)
    );

    let view = at(scratch.path(), &["view", ".", "--precision", ".3f"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(out(&view).contains("12.100 ± 0.050"), "{}", out(&view));
}

#[test]
fn column_precision_overrides_global_precision() {
    let scratch = Scratch::new("column-precision");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-c", "malt:.1f,brix", "--precision", ".3f"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("12.1 ± 0.1"), "{text}");
    assert!(text.contains("2.801 ± 0.001"), "{text}");
}

#[test]
fn an_ad_hoc_column_carries_its_label_and_python_style_precision() {
    let scratch = Scratch::new("column-declaration");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt:.3f=Malt: precise,brix=Plato"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("Malt: precise [g]"), "{text}");
    assert!(text.contains("Plato [g/L]"), "{text}");
    assert!(text.contains("12.100 ± 0.050"), "{text}");
}

#[test]
fn column_separators_ignore_table_index_content() {
    let scratch = Scratch::new("column-punctuation");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "item.sample.md",
        "---\nschema_version: 1\nname: item\ntables:\n  multi:\n    index: [a, b]\n    columns:\n      a: {}\n      b: {}\n      z: {}\n    rows:\n      - {a: 65, b: 2, z: 4.2}\n  named:\n    index: [key]\n    columns:\n      key: {}\n      z: {}\n    rows:\n      - {key: 'a=b', z: 8.4}\n---\n",
    );
    let output = at(scratch.path(), &["-c", "multi.z[65, 2],named.z[\"a=b\"]"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("multi.z[65, 2]"), "{text}");
    assert!(text.contains("named.z[\"a=b\"]"), "{text}");
    assert!(text.contains("4.2"), "{text}");
    assert!(text.contains("8.4"), "{text}");
}

#[test]
fn an_empty_column_name_is_refused_during_parsing() {
    let scratch = Scratch::new("empty-column");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", ","]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("cannot be empty"), "{}", err(&output));
}

// --------------------------------------------------------------------- list

#[test]
fn list_enumerates_every_addressable_field() {
    // Including `table.column[index]` paths.
    let scratch = Scratch::new("list-fields");
    scratch.config();
    scratch.write(
        "with-table.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5}\ntables:\n  mashing:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      wort: {unit: lintner}\n    \
         rows:\n      - {temperature: 78.5, wort: 104.2}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["list", "fields"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("properties (1)"), "{text}");
    assert!(text.contains("malt"), "{text}");
    // The table once: its index, its columns, an address to copy.
    assert!(text.contains("mashing"), "{text}");
    assert!(text.contains("index    temperature: 78.5"), "{text}");
    assert!(text.contains("columns  wort"), "{text}");
    assert!(text.contains("'mashing.wort[78.5]'"), "{text}");
}

#[test]
fn list_makes_the_file_unnecessary() {
    // Every field a query can use appears — the friction this command exists
    // to remove is opening a `.md` in an editor to learn a column's name.
    let scratch = Scratch::new("list-all");
    scratch.corpus();
    scratch.write(
        "with-table.sample.md",
        "---\nschema_version: 1\nname: keg-05\nproperties:\n  \
         malt: {v: 12.5}\ntables:\n  mashing:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      wort: {unit: lintner}\n    \
         rows:\n      - {temperature: 78.5, wort: 104.2}\n      \
         - {temperature: 303.15, wort: 108.9}\n---\nN.\n",
    );
    let listed = out(&at(scratch.path(), &["list", "fields"]));
    let words: Vec<String> = listed
        .split_whitespace()
        .map(|word| word.trim_matches(|c| "'():,".contains(c)).to_string())
        .collect();
    let has = |word: &str| words.iter().any(|known| known == word);
    let collection = list::from_directory(scratch.path()).unwrap();
    for field in collection.available_fields() {
        // Readable from the output: a name as itself, a cell as its table,
        // its column and its index value.
        match &field {
            Field::Cell {
                table, column, row, ..
            } => {
                let written = fields::describe(&field);
                let index = written
                    .rsplit_once('[')
                    .map(|(_, rest)| rest.trim_end_matches(']').to_string())
                    .unwrap();
                assert!(has(table.as_str()), "{table}: {listed}");
                assert!(has(column.as_str()), "{column}: {listed}");
                assert!(has(&index), "{row:?}: {listed}");
            }
            _ => {
                let written = fields::describe(&field);
                assert!(has(&written), "{written}: {listed}");
            }
        }
        // And every one of them is accepted as a column.
        let written = fields::describe(&field);
        let query = at(scratch.path(), &["-c", &written]);
        assert_eq!(code(&query), 0, "{written}: {}", err(&query));
    }
}

#[test]
fn list_names_every_declared_object() {
    let scratch = Scratch::new("list-declared");
    scratch.corpus();
    let text = out(&at(scratch.path(), &["list"]));
    assert!(
        text.contains("1 query,") || text.contains("1 query "),
        "{text}"
    );
    assert!(
        text.contains("1 profile,") || text.contains("1 profile "),
        "{text}"
    );
    assert!(
        text.contains("1 export,") || text.contains("1 export "),
        "{text}"
    );
    for kind in ["queries", "profiles", "exports"] {
        let listed = at(scratch.path(), &["list", kind]);
        assert_eq!(code(&listed), 0, "{kind}: {}", err(&listed));
        assert!(!out(&listed).trim().is_empty(), "{kind}");
    }
}

#[test]
fn list_gathers_tags_from_the_samples() {
    // Not from the configuration, which declares none: listing them is the only
    // way to learn what a collection uses.
    let scratch = Scratch::new("list-tags");
    scratch.corpus();
    let output = at(scratch.path(), &["list", "tags"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(out(&output).trim(), "reference");
}

#[test]
fn list_alone_counts_fields_rather_than_printing_them() {
    // The summary stays readable on a large collection.
    let scratch = Scratch::new("list-counts");
    scratch.corpus();
    let text = out(&at(scratch.path(), &["list"]));
    assert!(text.contains("2 properties"), "{text}");
    assert!(text.contains("0 tables"), "{text}");
    assert!(!text.contains("\nmalt\n"), "{text}");
    assert!(!text.contains("  malt"), "{text}");
}

#[test]
fn list_fields_enumerates_one_entry_per_quantity() {
    // Channels are not expanded: `.value`, `.uncertainty` and `.unit` are a
    // suffix grammar that applies to everything, and listing them would
    // quadruple the output.
    let scratch = Scratch::new("one-per-quantity");
    scratch.corpus();
    scratch.write(
        "with-table.sample.md",
        "---\nschema_version: 1\nname: keg-05\ntables:\n  mashing:\n    \
         index: [temperature]\n    columns:\n      temperature: {}\n      wort: {}\n    \
         rows:\n      - {temperature: 78.5, wort: 104.2}\n      \
         - {temperature: 303.15, wort: 108.9}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &["list", "fields"]));
    assert!(text.contains("malt"), "{text}");
    assert!(!text.contains("malt.v"), "{text}");
    assert!(!text.contains("malt.u"), "{text}");
    // Two rows, and still no line per cell.
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("mashing.wort[")),
        "{text}"
    );
    assert!(
        text.contains("index    temperature: 78.5  303.15"),
        "{text}"
    );
}

#[test]
fn list_takes_the_kind_before_the_target() {
    // `list fields` and `list fields <file>`, which is what the owner typed
    // first, and which the first build answered with "no such file".
    let scratch = Scratch::new("kind-first");
    scratch.corpus();
    let here = at(scratch.path(), &["list", "fields"]);
    assert_eq!(code(&here), 0, "{}", err(&here));
    assert!(out(&here).contains("properties"), "{}", out(&here));
    let one = at(scratch.path(), &["list", "fields", "keg-01.sample.md"]);
    assert_eq!(code(&one), 0, "{}", err(&one));
    // A lone path still counts.
    let counted = at(scratch.path(), &["list", "keg-01.sample.md"]);
    assert!(out(&counted).contains("1 sample\n"), "{}", out(&counted));
    // The earlier order is refused with the right spelling.
    let earlier = at(scratch.path(), &["list", ".", "fields"]);
    assert_eq!(code(&earlier), 1, "{}", out(&earlier));
    assert!(
        err(&earlier).contains("samplekit list fields ."),
        "{}",
        err(&earlier)
    );
    // A kind that is also a directory is two readings, and refused.
    fs::create_dir_all(scratch.path().join("tags")).unwrap();
    let both = at(scratch.path(), &["list", "tags"]);
    assert_eq!(code(&both), 1, "{}", out(&both));
    assert!(err(&both).contains("both"), "{}", err(&both));
}

#[test]
fn list_first_positional_completes_kinds_and_paths() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    let scratch = Scratch::new("list-first-completion");
    scratch.write(
        "samples/item.sample.md",
        "---\nschema_version: 1\nname: item\n---\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "list", ""])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).lines().any(|line| line.starts_with("fields")));
    assert!(
        out(&output)
            .lines()
            .any(|line| line.starts_with("samples/"))
    );
}

#[test]
fn an_unknown_kind_to_list_suggests_the_nearest() {
    let scratch = Scratch::new("bad-kind");
    scratch.corpus();
    let output = at(scratch.path(), &["list", "feilds"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("fields"), "{message}");
    assert!(message.contains("available:"), "{message}");
}

// ------------------------------------------------------ find, stale, explain

#[test]
fn implicit_selection_emits_one_path_per_line() {
    // Usable with `xargs`, and nothing else on stdout.
    let scratch = Scratch::new("find");
    scratch.corpus();
    let output = at(scratch.path(), &["."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{:?}", lines);
    for line in lines {
        assert!(line.ends_with(".sample.md"), "{line}");
        assert!(Path::new(scratch.path()).join(line).exists(), "{line}");
    }
}

#[test]
fn implicit_paths_take_the_same_selection_as_a_table() {
    // One grammar, two destinations.
    let scratch = Scratch::new("find-selection");
    scratch.corpus();
    let found = at(scratch.path(), &[".", "-f", "malt > 12.15"]);
    assert_eq!(code(&found), 0, "{}", err(&found));
    assert_eq!(out(&found).lines().count(), 2, "{}", out(&found));
    let tabled = at(scratch.path(), &["-f", "malt > 12.15", "-c", "name,malt"]);
    assert_eq!(order(&out(&tabled)).len(), 2);
}

#[test]
fn stale_names_the_value_and_what_changed() {
    // Not a count: a read of the fingerprints, which is what makes it usable on
    // a collection whose recompute takes hours.
    let scratch = Scratch::new("stale");
    scratch.config();
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: 000000000000}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("volume"), "{text}");
    assert!(text.contains("malt"), "{text}");
    assert!(text.contains("outdated"), "{text}");
}

#[test]
fn stale_on_a_current_collection_says_so_and_exits_zero() {
    // Silence here would be indistinguishable from a failure.
    let scratch = Scratch::new("current");
    scratch.corpus();
    let output = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("is current"), "{}", out(&output));
}

#[test]
fn explain_takes_a_sample_and_a_field() {
    // A directory is refused, naming what it wants: the question is about one
    // number.
    let scratch = Scratch::new("explain-args");
    scratch.corpus();
    let refused = at(scratch.path(), &["explain", ".", "brix"]);
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    assert!(err(&refused).contains("one number"), "{}", err(&refused));
    let accepted = at(scratch.path(), &["explain", "keg-01.sample.md", "brix"]);
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    // The unit a reader sees is the project's display form, as in a header.
    assert!(out(&accepted).contains("g/L"), "{}", out(&accepted));
}

#[test]
fn explain_says_when_nothing_computed_a_value() {
    // A measured quantity is not a silent blank.
    let scratch = Scratch::new("explain-measured");
    scratch.corpus();
    let output = at(scratch.path(), &["explain", "keg-01.sample.md", "malt"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("nothing computed it"),
        "{}",
        out(&output)
    );
}

#[test]
fn explain_says_a_formula_reads_nothing_for_an_empty_record() {
    // `computed: {}` is a derivation that reads nothing of the sample — a
    // hydrometer's resolution beside an entered ibu — and a heading over an
    // empty list would read as a blank. The sentence names the number the
    // formula made, where the record says which.
    let scratch = Scratch::new("explain-empty-record");
    scratch.config();
    scratch.write(
        "gauge.sample.md",
        "---\nschema_version: 1\nname: gauge\nproperties:\n  \
         ibu: {v: 12.70, u: 0.0058, unit: mL, computed: {}, fingerprint: 000000000000}\n---\n",
    );
    let output = at(scratch.path(), &["explain", "gauge.sample.md", "ibu"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a formula that reads nothing of this sample computed it"),
        "{}",
        out(&output)
    );
    assert!(!out(&output).contains("computed from"), "{}", out(&output));
}

// ------------------------------------------------------------ view and export

#[test]
fn view_renders_once_per_selected_sample() {
    // It never becomes a table.
    let scratch = Scratch::new("view");
    scratch.corpus();
    let output = at(scratch.path(), &["view", ".", "--query", "approved"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert_eq!(text.matches("malt").count(), 3, "{text}");
    assert_eq!(order(&text).len(), 3, "{text}");
    assert!(!text.contains('\u{250c}'), "{text}");
}

#[test]
fn export_regenerates_a_declared_dataset() {
    // The declared profile, format and destination.
    let scratch = Scratch::new("export");
    scratch.corpus();
    let output = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("out/platos.csv")).unwrap();
    assert!(written.starts_with("malt_value [g],"), "{written}");
    // Regenerating from unchanged data produces the same bytes.
    let again = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(
        fs::read_to_string(scratch.path().join("out/platos.csv")).unwrap(),
        written
    );
}

#[test]
fn a_folder_without_samples_says_so_rather_than_every_field_unknown() {
    // Run where no sample was read, an export named each of its columns an
    // unknown field.
    let scratch = Scratch::new("export-no-samples");
    scratch.corpus();
    fs::create_dir_all(scratch.path().join("empty")).unwrap();
    let output = at(&scratch.path().join("empty"), &["export", "platos", "."]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("no sample was read here"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("unknown field"), "{}", err(&output));
}

/// `export` shows what it would write and writes on `--write`, like every other
/// command that changes something.
///
/// It used to write immediately and refuse only an existing file, with an
/// `--overwrite` that exists nowhere else — which is what let a stale number
/// reach a published file behind a suppressible warning.
#[test]
fn export_previews_before_it_writes() {
    let scratch = Scratch::new("export-preview");
    scratch.corpus();
    let preview = at(scratch.path(), &["export", "platos", "."]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let shown = out(&preview);
    assert!(shown.contains("platos"), "{shown}");
    assert!(shown.contains("to write"), "{shown}");
    assert!(shown.contains("nothing written"), "{shown}");
    assert!(
        !scratch.path().join("out/platos.csv").exists(),
        "a preview wrote the file"
    );
    assert_eq!(
        code(&at(scratch.path(), &["export", "platos", ".", "--write"])),
        0
    );
    // A second preview says the file is there and that --write replaces it —
    // where the old shape refused, with a flag of its own.
    let again = out(&at(scratch.path(), &["export", "platos", "."]));
    assert!(again.contains("--write replaces it"), "{again}");
    assert_eq!(
        code(&at(scratch.path(), &["export", "platos", ".", "--write"])),
        0,
        "--write did not replace it"
    );
}

/// A stale value reaches a file only past a preview that named it — and says so
/// on stdout, where `-q` cannot reach it.
#[test]
fn an_export_says_in_its_preview_what_is_not_current() {
    let scratch = Scratch::new("export-stale-preview");
    scratch.corpus();
    scratch.write(
        "keg-01.sample.md",
        "---\nschema_version: 1\nname: C-01\nproperties:\n  \
         malt: {v: 12.4, unit: g, fingerprint: deadbeefcafe, computed: {}}\n---\n",
    );
    for arguments in [
        vec!["export", "platos", "."],
        vec!["export", "platos", ".", "-q"],
    ] {
        let preview = at(scratch.path(), &arguments);
        assert_eq!(code(&preview), 0, "{}", err(&preview));
        assert!(
            out(&preview).contains("not current"),
            "with {arguments:?}: {}",
            out(&preview)
        );
        // Naming them: a count alone sent the reader to `status`
        // for what the preview already knew.
        assert!(
            out(&preview).contains("C-01 malt"),
            "with {arguments:?}: {}",
            out(&preview)
        );
    }
    // And with --write, on stdout still: -q hid it when it was a warning.
    let written = at(scratch.path(), &["export", "platos", ".", "-q", "--write"]);
    assert!(out(&written).contains("not current"), "{}", out(&written));
}

#[test]
fn export_to_stdout_carries_only_the_data() {
    // `-` as the destination, for a redirection the author controls.
    let scratch = Scratch::new("export-stdout");
    scratch.corpus();
    let output = at(scratch.path(), &["export", "platos", ".", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).starts_with("malt_value [g],"),
        "{}",
        out(&output)
    );
    assert!(!scratch.path().join("out/platos.csv").exists());
}

#[test]
fn the_history_is_kept_without_git_installed() {
    // The history is written in the program; with `git` unreachable, every
    // command works and a write is still a snapshot.
    let scratch = Scratch::new("no-git");
    scratch.corpus();
    let without_git = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(arguments)
            .current_dir(scratch.path())
            .env("COLUMNS", "200")
            .env("PATH", "/nonexistent")
            .output()
            .unwrap()
    };
    let output = without_git(&["export", "platos", ".", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).starts_with("malt_value [g],"),
        "{}",
        out(&output)
    );
    let set = without_git(&["set", "keg-01.sample.md", "malt=13", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(err(&set).is_empty(), "{}", err(&set));
    assert_eq!(
        history_messages(scratch.path()),
        [
            "samplekit set keg-01.sample.md malt=13 --write",
            "the project as SampleKit first kept it"
        ]
    );
}

// ----------------------------------------------------------------- validate

#[test]
fn validate_never_modifies() {
    // Asserted against the filesystem.
    let scratch = Scratch::new("validate-read-only");
    scratch.corpus();
    let before = snapshot(scratch.path());
    let output = at(scratch.path(), &["validate", "."]);
    assert!(code(&output) == 0 || code(&output) == 2, "{}", err(&output));
    assert_eq!(before, snapshot(scratch.path()));
}

#[test]
fn validate_reports_what_validation_found() {
    // The dispatch is real, not a second implementation: two units for one
    // quantity is `validation`'s finding, and it appears here.
    let scratch = Scratch::new("validate-units");
    scratch.config();
    scratch.sample("a.sample.md", "keg-01", "approved", 12.1, Some(2.8));
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: keg-02\nproperties:\n  \
         brix: {v: 2800.0, unit: kg/hL}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&output), 2, "{}", out(&output));
    let text = out(&output);
    assert!(text.contains("two units"), "{text}");
    assert!(text.contains("kg/hL"), "{text}");
}

#[test]
fn validate_exits_two_on_a_defect() {
    // A data error, not a usage error: one quantity in two units.
    let scratch = Scratch::new("validate-defect");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  brix: {v: 2.8, unit: g/L}\n---\nN.\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: keg-02\nproperties:\n  brix: {v: 2800.0, unit: kg/hL}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&output), 2, "{}", out(&output));
    assert!(out(&output).contains("two units"), "{}", out(&output));
}

#[test]
fn validate_a_note_does_not_change_the_exit_code() {
    // A rewritable file is not an invalid one.
    let scratch = Scratch::new("validate-note");
    scratch.write(
        "unsorted.sample.md",
        "---\nname: keg-01\nschema_version: 1\nproperties:\n  \
         malt:\n    v: 12.5\n---\nN.\n",
    );
    let output = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert!(out(&output).contains("rewritten"), "{}", out(&output));
}

#[test]
fn validate_groups_notes_by_what_they_say() {
    // Seventy identical lines bury the defect worth reading: identical notes
    // are one line with a count and the first names.
    // A note about a table's rows is one per row, and counts its sample once;
    // a precision's zeros are grouped by quantity, not value.
    let rows = Scratch::new("validate-grouped-rows");
    rows.write(
        ".samplekitrc",
        "schema_version = 1\n[property.\"m.ebc\"]\nprecision = \".1f\"\n",
    );
    for index in 1..=2 {
        rows.write(
            &format!("c{index}.md"),
            "---\nschema_version: 1\ntables:\n  m:\n    index: T\n    columns:\n      \
             T: {}\n      ebc: {}\n    rows:\n      - {T: 1.0, ebc: {v: 5.0, u: 0.01}}\n      \
             - {T: 2.0, ebc: {v: 5.0, u: 0.02}}\n---\nN.\n",
        );
    }
    let text = out(&at(rows.path(), &["validate", "."]));
    assert!(
        text.contains("m.ebc.u: precision .1f writes a value as zero"),
        "{text}"
    );
    assert!(text.contains("\u{2014} 2 samples: c1, c2\n"), "{text}");
}

#[test]
fn a_file_target_is_checked_against_its_directory() {
    // A vocabulary belongs to the collection, not to the target: keg-04 has
    // no plato, and its siblings do.
    let scratch = Scratch::new("file-vocabulary");
    scratch.corpus();
    let column = at(scratch.path(), &["keg-04.sample.md", "-c", "brix"]);
    assert_eq!(code(&column), 0, "{}", err(&column));
    let filtered = at(
        scratch.path(),
        &["keg-04.sample.md", "-c", "malt", "-f", "brix is missing"],
    );
    assert_eq!(code(&filtered), 0, "{}", err(&filtered));
    let view = at(scratch.path(), &["view", "keg-04.sample.md"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(err(&view).is_empty(), "{}", err(&view));
    // A name no sample in the directory has is still a typo.
    let typo = at(scratch.path(), &["keg-04.sample.md", "-c", "brixx"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
}

#[test]
fn validate_accepts_a_single_file() {
    // A collection of one, with the project found above it.
    let scratch = Scratch::new("validate-one");
    scratch.corpus();
    let output = at(scratch.path(), &["validate", "keg-01.sample.md"]);
    assert!(code(&output) == 0 || code(&output) == 2, "{}", err(&output));
    assert!(out(&output).contains("1 sample ·"), "{}", out(&output));
}

#[test]
fn tag_without_write_modifies_nothing() {
    let scratch = Scratch::new("tag-preview");
    scratch.corpus();
    let before = snapshot(scratch.path());
    let output = at(scratch.path(), &["tag", "add", "cold_crashed"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("nothing written"), "{}", out(&output));
    assert_eq!(before, snapshot(scratch.path()));
}

#[test]
fn tag_applies_to_the_filtered_selection() {
    // The target grammar, unchanged: tagging turns a transient answer into a
    // durable one recorded in each file.
    let scratch = Scratch::new("tag-selection");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            "tag",
            "add",
            "cold_crashed",
            "-f",
            "malt > 12.15",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(tags_of(scratch.path(), "keg-02.sample.md").contains("cold_crashed"));
    assert!(tags_of(scratch.path(), "keg-03.sample.md").contains("cold_crashed"));
    assert!(!tags_of(scratch.path(), "keg-01.sample.md").contains("cold_crashed"));
}

#[test]
fn tag_add_is_idempotent() {
    // An already-tagged sample is reported unchanged, not duplicated: the end
    // state is what was asked for.
    let scratch = Scratch::new("tag-idempotent");
    scratch.corpus();
    let output = at(scratch.path(), &["tag", "add", "reference", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("unchanged"), "{}", out(&output));
    let tags = tags_of(scratch.path(), "keg-01.sample.md");
    assert_eq!(tags.matches("reference").count(), 1, "{tags}");
}

#[test]
fn tag_preserves_the_note_and_the_records() {
    // Byte-identical note, untouched `computed`: a tag is not a value change
    // and invalidates no computation.
    let scratch = Scratch::new("tag-preserves");
    scratch.write(
        "computed.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n---\n\
         A note   with  deliberate    spacing.\n\nAnd a second paragraph.\n",
    );
    let output = at(scratch.path(), &["tag", "add", "reference", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let after = fs::read_to_string(scratch.path().join("computed.sample.md")).unwrap();
    assert!(
        after.contains("A note   with  deliberate    spacing.\n\nAnd a second paragraph.\n"),
        "{after}"
    );
    assert!(after.contains("computed: {malt: aaaaaaaaaaaa}"), "{after}");
    assert!(after.contains("fingerprint: bbbbbbbbbbbb"), "{after}");
}

#[test]
fn tag_rejects_a_non_identifier_before_writing() {
    // Nothing is half-applied, and the message names the rule.
    let scratch = Scratch::new("tag-identifier");
    scratch.corpus();
    let before = snapshot(scratch.path());
    let output = at(scratch.path(), &["tag", "add", "dext-vintage", "--write"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("dext-vintage"), "{}", err(&output));
    assert_eq!(before, snapshot(scratch.path()));
}

#[test]
fn tag_reports_exactly_which_files_changed() {
    // By name, not as a count: "some of 50" is not an outcome anyone can act
    // on.
    let scratch = Scratch::new("tag-reports");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            "tag",
            "add",
            "cold_crashed",
            "-f",
            "malt > 12.15",
            "--write",
        ],
    );
    let text = out(&output);
    assert!(text.contains("written: 2 files"), "{text}");
    assert!(text.contains("keg-02"), "{text}");
    assert!(text.contains("keg-03"), "{text}");
}

#[test]
fn tag_rename_drops_a_resulting_duplicate() {
    // One `reference`, in its existing position.
    let scratch = Scratch::new("tag-duplicate");
    scratch.write(
        "both.sample.md",
        "---\nschema_version: 1\nname: keg-01\ntags: [refernce, reference, cold_crashed]\n\
         properties:\n  malt: {v: 12.5}\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &["tag", "rename", "refernce", "reference", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let tags = tags_of(scratch.path(), "both.sample.md");
    assert_eq!(tags.matches("reference").count(), 1, "{tags}");
    assert!(tags.contains("[reference, cold_crashed]"), "{tags}");
}

#[test]
fn tag_rename_over_a_subset_warns_about_the_remainder() {
    // The operation did what was asked, and the thing it was almost certainly
    // *for* — making the old spelling stop existing — did not happen.
    let scratch = Scratch::new("tag-subset");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            "tag",
            "rename",
            "reference",
            "control",
            "-f",
            "malt > 12.15",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let message = err(&output);
    assert!(message.contains("2 samples outside"), "{message}");
    assert!(message.contains("reference"), "{message}");
}

#[test]
fn tag_rename_is_one_pass() {
    // The tag keeps its position, which a remove followed by an add cannot do:
    // after the first pass a cask carries neither spelling, and an
    // interrupted run leaves a collection in that state.
    let scratch = Scratch::new("tag-one-pass");
    scratch.write(
        "middle.sample.md",
        "---\nschema_version: 1\nname: keg-01\ntags: [first, refernce, last]\n\
         properties:\n  malt: {v: 12.5}\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &["tag", "rename", "refernce", "reference", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        tags_of(scratch.path(), "middle.sample.md").contains("[first, reference, last]"),
        "{}",
        tags_of(scratch.path(), "middle.sample.md")
    );
}

// ------------------------------------------------------------------ helpers

/// Every file's bytes, for asserting that a command modified nothing.
fn snapshot(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found: Vec<(PathBuf, Vec<u8>)> = walk(directory)
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap_or_default();
            (path, bytes)
        })
        .collect();
    found.sort();
    found
}

fn walk(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// The `tags:` line of a file, as written.
fn tags_of(directory: &Path, file: &str) -> String {
    fs::read_to_string(directory.join(file))
        .unwrap()
        .lines()
        .find(|line| line.starts_with("tags:"))
        .unwrap_or_default()
        .to_string()
}

// ------------------------------------------------------- the configuration

#[test]
fn verbose_names_the_configuration_and_model() {
    let scratch = Scratch::new("verbose-configuration");
    scratch.corpus();
    let quiet = at(scratch.path(), &["status", "."]);
    let loud = at(scratch.path(), &["status", ".", "-v"]);
    assert_eq!(out(&quiet), out(&loud));
    let file = dunce::canonicalize(scratch.path())
        .unwrap()
        .join(".samplekitrc");
    assert!(
        err(&loud).contains(&format!("configuration: {}", file.display())),
        "{}",
        err(&loud)
    );
    assert!(
        err(&loud).contains("model: none declared"),
        "{}",
        err(&loud)
    );
    assert!(!err(&quiet).contains("configuration:"), "{}", err(&quiet));
}

#[test]
fn show_rc_prints_the_configuration_and_runs_nothing() {
    let scratch = Scratch::new("show-rc");
    scratch.corpus();
    let output = at(scratch.path(), &["--show-rc"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let file = dunce::canonicalize(scratch.path())
        .unwrap()
        .join(".samplekitrc");
    assert!(
        out(&output).starts_with(&format!("configuration  {}\n", file.display())),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("model          none declared"),
        "{}",
        out(&output)
    );
    assert!(!out(&output).contains("keg-01"), "{}", out(&output));
}

#[test]
fn rc_replaces_the_nearest_configuration() {
    let scratch = Scratch::new("rc-forces");
    scratch.corpus();
    scratch.write("nested/.samplekitrc", "schema_version = 1\n");
    scratch.sample("nested/n1.sample.md", "n1", "approved", 1.0, None);
    scratch.sample("nested/n2.sample.md", "n2", "rejected", 1.0, None);
    // The nested configuration declares no query.
    let nearest = at(scratch.path(), &["nested", "--query", "approved"]);
    assert_eq!(code(&nearest), 1, "{}", err(&nearest));
    let forced = at(
        scratch.path(),
        &["nested", "--query", "approved", "--rc", ".samplekitrc"],
    );
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    assert!(
        out(&forced).contains("n1") && !out(&forced).contains("n2"),
        "{}",
        out(&forced)
    );
    assert!(
        err(&forced).contains("2 samples have their own configuration, ignored"),
        "{}",
        err(&forced)
    );
}

#[test]
fn help_names_the_configuration_found_here() {
    let scratch = Scratch::new("help-configuration");
    scratch.corpus();
    let output = at(scratch.path(), &["--help"]);
    assert_eq!(code(&output), 0);
    let file = dunce::canonicalize(scratch.path())
        .unwrap()
        .join(".samplekitrc");
    assert!(
        out(&output).contains(&format!("Configuration here: {}", file.display())),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("found from the current directory"),
        "{}",
        out(&output)
    );
}

#[test]
fn compute_is_a_listed_command() {
    let scratch = Scratch::new("compute-listed");
    let output = at(scratch.path(), &["--help"]);
    let listed = out(&output);
    let status = listed
        .find("  status")
        .unwrap_or_else(|| panic!("{listed}"));
    let compute = listed
        .find("  compute")
        .unwrap_or_else(|| panic!("{listed}"));
    assert!(compute > status, "{listed}");
    assert!(
        listed.contains("Compute the values the project's model gives"),
        "{listed}"
    );
}

#[test]
fn stale_names_a_table_column_holding_stale_cells() {
    let scratch = Scratch::new("stale-cells");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "boil.sample.md",
        "---\nschema_version: 1\nname: boil\ntables:\n  mashing:\n    index: [T]\n    columns:\n      \
         T: {}\n      R: {}\n      G: {}\n    rows:\n      - T: 10\n        R: 2.0\n        G:\n          \
         v: 0.5\n          computed: {row.R: 000000000000}\n      - T: 20\n        R: 4.0\n        \
         G:\n          v: 0.25\n          computed: {row.R: 000000000000}\n---\n",
    );
    let output = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("mashing.G"), "{text}");
    assert!(text.contains("(2 rows)"), "{text}");
    assert_eq!(text.matches("mashing.G").count(), 1, "{text}");
}

#[test]
fn list_counts_agree_with_their_number() {
    let scratch = Scratch::new("list-counts");
    table_corpus(&scratch);
    let output = at(scratch.path(), &["list", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("1 sample\n"), "{}", out(&output));
    assert!(out(&output).contains(", 1 table —"), "{}", out(&output));
}

#[test]
fn a_summary_rounds_as_its_column() {
    let scratch = Scratch::new("summary-precision");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "brix", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("2.8020"), "{}", out(&output));
    assert!(!out(&output).contains("2.802000"), "{}", out(&output));
}

#[test]
fn a_filter_error_points_into_the_filter_as_written() {
    let scratch = Scratch::new("filter-caret");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "-f", "status == approved & malt > 1"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    let written = err(&output);
    let lines: Vec<&str> = written.lines().collect();
    assert!(lines[0].contains("'&&'"), "{written}");
    assert_eq!(lines[1], "  status == approved & malt > 1", "{written}");
    assert_eq!(lines[2].find('^'), lines[1].find('&'), "{written}");
    assert!(!written.contains("(status"), "{written}");
    let symbols = at(
        scratch.path(),
        &[".", "-f", "status == approved && malt > 12.15"],
    );
    assert_eq!(code(&symbols), 0, "{}", err(&symbols));
    assert_eq!(out(&symbols).lines().count(), 2, "{}", out(&symbols));
}

#[test]
fn an_unknown_declaration_lists_what_is_available_once() {
    let scratch = Scratch::new("available-once");
    scratch.corpus();
    for arguments in [
        vec![".", "--query", "aproved"],
        vec![".", "--profile", "plat"],
        vec!["export", "plat", "."],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        assert_eq!(
            err(&output).matches("available:").count(),
            1,
            "{arguments:?}: {}",
            err(&output)
        );
    }
}

#[test]
fn explain_points_a_missing_field_to_compute() {
    let scratch = Scratch::new("explain-missing");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\nclass S(sk.Sample):\n    def __init__(self):\n        \
         self.brix = sk.Property(lambda s: s.malt / 2)\n",
    );
    scratch.sample("a.sample.md", "a", "approved", 1.0, None);
    let output = at(scratch.path(), &["explain", "a.sample.md", "brix"]);
    assert!(
        format!("{}{}", out(&output), err(&output)).contains("samplekit compute"),
        "{}{}",
        out(&output),
        err(&output)
    );
    // A name the model does not declare is a misspelling, and computing
    // offers nothing for it.
    let unknown = at(scratch.path(), &["explain", "a.sample.md", "nosuch"]);
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(
        err(&unknown).contains("unknown field 'nosuch'"),
        "{}",
        err(&unknown)
    );
    assert!(!err(&unknown).contains("compute"), "{}", err(&unknown));
}

#[test]
fn an_unreadable_file_is_said_by_every_command() {
    let scratch = Scratch::new("unreadable-said");
    scratch.corpus();
    scratch.write(
        "legacy.sample.md",
        "---\nschema_version: 1\nproperties:\n  malt:\n    v: 12.5\n    unit: g\n    unit_math: \\mathrm{g}\n    precision: .3f\n    precision_unc: .1f\n---\nA brewing note.\n",
    );
    for arguments in [vec!["."], vec!["status", "."], vec!["list", "."]] {
        let output = at(scratch.path(), &arguments);
        // Said, and the answer is partial: exit 2.
        assert_eq!(code(&output), 2, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains("legacy.sample.md was not read"),
            "{arguments:?}: {}",
            err(&output)
        );
        assert!(
            err(&output).contains("no command converts it"),
            "{}",
            err(&output)
        );
    }
    let validate = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&validate), 2, "{}", out(&validate));
}

#[test]
fn an_unknown_table_column_or_row_exits_one() {
    let scratch = Scratch::new("unknown-cell");
    scratch.corpus();
    scratch.write(
        "aged.sample.md",
        "---\nschema_version: 1\nname: aged\ntables:\n  conditioning:\n    index: [day]\n    columns:\n      day: {}\n      carbonation: {}\n    rows:\n      - {day: 50, carbonation: 0.9}\n---\n",
    );
    let typo = at(scratch.path(), &["-c", "conditioning.carbonatoin[50]"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
    assert!(
        err(&typo).contains("conditioning.carbonation[50]"),
        "{}",
        err(&typo)
    );
    let row = at(scratch.path(), &["-c", "conditioning.carbonation[51]"]);
    assert_eq!(code(&row), 1, "{}", out(&row));
    assert!(
        err(&row).contains("conditioning.carbonation[51]"),
        "{}",
        err(&row)
    );
    let held = at(scratch.path(), &["-c", "conditioning.carbonation[50]"]);
    assert_eq!(code(&held), 0, "{}", err(&held));
}

#[test]
fn an_unknown_command_suggests_the_nearest() {
    let scratch = Scratch::new("unknown-command");
    scratch.corpus();
    let output = at(scratch.path(), &["validat"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("did you mean 'validate'?"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_count_agrees_with_its_number() {
    let scratch = Scratch::new("counts");
    scratch.corpus();
    let output = at(scratch.path(), &["validate", "keg-01.sample.md"]);
    let text = out(&output);
    assert!(text.contains("1 sample ·"), "{text}");
    assert!(!text.contains("(s)"), "{text}");
}

#[test]
fn help_describes_every_argument_and_names_no_decision() {
    let scratch = Scratch::new("help-words");
    scratch.corpus();
    let cites = |text: &str| {
        text.match_indices("D-")
            .any(|(at, _)| text[at + 2..].starts_with(|c: char| c.is_ascii_digit()))
    };
    for arguments in [
        vec!["--help"],
        vec!["list", "--help"],
        vec!["status", "--help"],
        vec!["compute", "--help"],
        vec!["explain", "--help"],
        vec!["view", "--help"],
        vec!["export", "--help"],
        vec!["validate", "--help"],
        vec!["tag", "add", "--help"],
        vec!["tag", "rename", "--help"],
        vec!["completions", "--help"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert!(!cites(&out(&output)), "{arguments:?}: {}", out(&output));
    }
    let tag = out(&at(scratch.path(), &["tag", "rename", "--help"]));
    assert!(
        tag.contains("The tag to rename") && tag.contains("Its new name"),
        "{tag}"
    );
    let list = out(&at(scratch.path(), &["list", "--help"]));
    assert!(list.contains("fields, tags, queries"), "{list}");
}

#[test]
fn an_output_path_on_the_command_line_is_relative_to_the_current_directory() {
    let scratch = Scratch::new("output-relative");
    scratch.corpus();
    let sub = scratch.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    let output = at(
        &sub,
        &["export", "platos", "..", "-o", "here.csv", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(sub.join("here.csv").is_file(), "{}", err(&output));
    assert!(!scratch.path().join("here.csv").exists());
}

#[test]
fn a_row_one_sample_lacks_leaves_its_cell_empty() {
    let scratch = Scratch::new("row-one-lacks");
    scratch.corpus();
    for (file, cycle) in [("aged-a.sample.md", 50), ("aged-b.sample.md", 60)] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\ntables:\n  conditioning:\n    index: [day]\n    columns:\n      day: {{}}\n      carbonation: {{}}\n    rows:\n      - {{day: {cycle}, carbonation: 0.9}}\n---\n"
            ),
        );
    }
    let csv = at(
        scratch.path(),
        &["-c", "name,conditioning.carbonation[50]", "--csv"],
    );
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(out(&csv).contains("aged-b.sample.md"), "{}", out(&csv));
    let summary = at(
        scratch.path(),
        &["-c", "conditioning.carbonation[50]", "--summary"],
    );
    assert_eq!(code(&summary), 0, "{}", err(&summary));
}

#[test]
#[cfg(target_os = "linux")]
fn an_output_into_a_missing_folder_makes_it() {
    // The folder -o names is made by --write, and said by the preview; nothing
    // is asked, at a terminal or without one.
    let scratch = Scratch::new("missing-output-directory");
    scratch.corpus();
    let preview = at(
        scratch.path(),
        &["-c", "malt", "--csv", "-o", "typo/malts.csv"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("typo does not exist, and --write creates it"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.path().join("typo").exists());
    let written = at(
        scratch.path(),
        &["-c", "malt", "--csv", "-o", "typo/malts.csv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(scratch.path().join("typo/malts.csv").is_file());
    let asked = at_terminal(
        scratch.path(),
        &["-c", "malt", "--csv", "-o", "again/malts.csv", "--write"],
        "",
    );
    assert_eq!(code(&asked), 0, "{}", out(&asked));
    assert!(!out(&asked).contains("create it?"), "{}", out(&asked));
    assert!(scratch.path().join("again/malts.csv").is_file());
}

#[test]
fn no_arguments_prints_the_short_help() {
    let scratch = Scratch::new("bare");
    scratch.corpus();
    let output = at(scratch.path(), &[]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("Usage"), "{text}");
    assert!(!text.contains("12.10"), "{text}");
}

#[test]
fn a_read_that_could_not_read_a_file_exits_two() {
    // The table, `list`, `status` and `export` show what they read and name the
    // file, and their code says the answer is partial. A file that is not a
    // sample is skipped, not unread, and changes nothing.
    let scratch = Scratch::new("partial-read");
    scratch.corpus();
    scratch.write("notes.md", "Just prose, no frontmatter.\n");
    let whole = at(scratch.path(), &["-c", "malt"]);
    assert_eq!(code(&whole), 0, "{}", err(&whole));
    scratch.write(
        "broken.sample.md",
        "---\nschema_version: 1\nname: [oops\n---\n",
    );
    for arguments in [
        vec!["-c", "malt"],
        vec!["list", "fields"],
        vec!["status"],
        vec!["export", "platos"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 2, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains("broken.sample.md was not read"),
            "{arguments:?}: {}",
            err(&output)
        );
        assert!(!err(&output).contains("error:"), "{}", err(&output));
    }
    // What it could read is still shown.
    assert!(out(&at(scratch.path(), &["-c", "malt"])).contains("12.10"));
}

#[test]
fn an_export_of_an_empty_selection_exits_zero() {
    // A question with no rows is a valid question. The preview says how many
    // rows it would write, and the export writes them — none.
    let scratch = Scratch::new("export-empty");
    scratch.corpus();
    let preview = at(scratch.path(), &["export", "platos", "-f", "malt > 1000"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(out(&preview).contains("0 rows"), "{}", out(&preview));
    let written = at(
        scratch.path(),
        &["export", "platos", "-f", "malt > 1000", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
}

#[test]
fn several_files_not_read_are_said_in_one_line() {
    let scratch = Scratch::new("several-unread");
    scratch.corpus();
    scratch.write(
        "broken-1.sample.md",
        "---\nschema_version: 1\nname: [oops\n---\n",
    );
    scratch.write(
        "broken-2.sample.md",
        "---\nschema_version: 1\nname: [oops\n---\n",
    );
    let output = at(scratch.path(), &["."]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("2 files were not read"),
        "{}",
        err(&output)
    );
    assert!(
        !err(&output).contains("broken-1.sample.md"),
        "{}",
        err(&output)
    );
    let verbose = at(scratch.path(), &[".", "-v"]);
    assert!(
        err(&verbose).contains("broken-1.sample.md"),
        "{}",
        err(&verbose)
    );
}

/// A root project and a sub-project, each declaring its own.
fn two_projects(scratch: &Scratch) {
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [query.heavy]\nfilter = \"malt > 1.5\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"malt\", precision = \".1f\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"-\"\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n\
         [query.light]\nfilter = \"malt > 2.5\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"malt\", precision = \".3f\"}]\n\
         [profile.nested_only]\ncolumns = [{field = \"name\"}]\n",
    );
    scratch.sample("nested/n.sample.md", "nested-one", "approved", 3.0, None);
}

#[test]
fn a_query_selects_only_among_the_samples_of_its_configuration() {
    let scratch = Scratch::new("query-own-configuration");
    two_projects(&scratch);
    let heavy = at(scratch.path(), &[".", "--query", "heavy", "-c", "name"]);
    assert_eq!(code(&heavy), 0, "{}", err(&heavy));
    assert!(out(&heavy).contains("root-one"), "{}", out(&heavy));
    assert!(!out(&heavy).contains("nested-one"), "{}", out(&heavy));
    let light = at(scratch.path(), &[".", "--query", "light", "-c", "name"]);
    assert_eq!(code(&light), 0, "{}", err(&light));
    assert!(out(&light).contains("nested-one"), "{}", out(&light));
    assert!(!out(&light).contains("root-one"), "{}", out(&light));
}

#[test]
fn a_profile_one_configuration_declares_shows_only_its_samples() {
    let scratch = Scratch::new("profile-own-configuration");
    two_projects(&scratch);
    let output = at(scratch.path(), &[".", "--profile", "nested_only"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("nested-one"), "{}", out(&output));
    assert!(!out(&output).contains("root-one"), "{}", out(&output));
    assert!(
        err(&output).contains("1 sample not shown"),
        "{}",
        err(&output)
    );
}

#[test]
fn list_offers_the_configuration_of_the_folder_it_runs_from() {
    let scratch = Scratch::new("list-offered");
    two_projects(&scratch);
    let root = at(scratch.path(), &["list", "queries", "."]);
    assert_eq!(code(&root), 0, "{}", err(&root));
    assert!(out(&root).contains("heavy"), "{}", out(&root));
    assert!(!out(&root).contains("light"), "{}", out(&root));
    let nested = at(&scratch.path().join("nested"), &["list", "queries"]);
    assert_eq!(code(&nested), 0, "{}", err(&nested));
    assert!(out(&nested).contains("light"), "{}", out(&nested));
    assert!(!out(&nested).contains("heavy"), "{}", out(&nested));
    // Two targets, each a project: each one's, under its file.
    let above = Scratch::new("list-offered-above");
    for (project, query) in [("a", "heavy"), ("b", "light")] {
        above.write(
            &format!("{project}/.samplekitrc"),
            &format!("schema_version = 1\n[query.{query}]\nfilter = \"malt > 1\"\n"),
        );
        above.sample(
            &format!("{project}/s.sample.md"),
            project,
            "approved",
            2.0,
            None,
        );
    }
    let both = at(above.path(), &["list", "queries", "a", "b"]);
    assert_eq!(code(&both), 0, "{}", err(&both));
    let text = out(&both);
    assert!(text.contains("heavy") && text.contains("light"), "{text}");
    assert_eq!(text.matches(".samplekitrc").count(), 2, "{text}");
}

/// A root declaring what is common, and two collections importing it: `c1` with
/// a finer precision, a profile of its own and the root's sorted its way.
fn imported_collections(scratch: &Scratch, output: &str) {
    scratch.write(
        ".samplekitrc",
        &format!(
            "schema_version = 1\n[collection]\nrecursive = true\n\
             [property.malt]\nunit = \"g\"\nprecision = \".1f\"\n\
             [query.heavy]\nfilter = \"malt > 1.5\"\n\
             [profile.p]\ncolumns = [{{field = \"name\"}}, {{field = \"malt\"}}]\n\
             [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"{output}\"\n\
             [figure.malts]\nx = \"name\"\ny = \"malt\"\n"
        ),
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "c1/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n\
         [property.malt]\nprecision = \".3f\"\n\
         [profile.p]\nsort = [\"-malt\"]\n\
         [profile.own]\ncolumns = [{field = \"name\"}]\n",
    );
    scratch.sample("c1/x.sample.md", "c1-one", "approved", 3.0, None);
    scratch.write("c2/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    scratch.sample("c2/y.sample.md", "c2-one", "approved", 1.0, None);
}

#[test]
fn a_listing_marks_where_each_declaration_comes_from() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    let scratch = Scratch::new("list-origin");
    imported_collections(&scratch, "exports/{collection}/e.csv");
    let c1 = scratch.path().join("c1");
    let profiles = at(&c1, &["list", "profiles"]);
    assert_eq!(code(&profiles), 0, "{}", err(&profiles));
    let text = out(&profiles);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("p ") && lines[0].ends_with("local over ../.samplekitrc"),
        "{text}"
    );
    assert!(
        lines[1].starts_with("own") && lines[1].ends_with("  local"),
        "{text}"
    );
    let queries = at(&c1, &["list", "queries"]);
    assert!(
        out(&queries).contains("heavy  ../.samplekitrc"),
        "{}",
        out(&queries)
    );
    let figures = at(&c1, &["list", "figures"]);
    assert!(
        out(&figures).contains("malts  ../.samplekitrc"),
        "{}",
        out(&figures)
    );
    // The root imports nothing: its names alone.
    let root = at(scratch.path(), &["list", "profiles"]);
    assert_eq!(out(&root), "p\n", "{}", out(&root));
}

#[test]
fn a_sample_follows_its_collection_wherever_the_command_runs_from() {
    let scratch = Scratch::new("follows-collection");
    imported_collections(&scratch, "exports/{collection}/e.csv");
    let from_root = at(scratch.path(), &[".", "-c", "name,malt"]);
    assert_eq!(code(&from_root), 0, "{}", err(&from_root));
    assert!(out(&from_root).contains("3.000"), "{}", out(&from_root));
    assert!(out(&from_root).contains("1.0 "), "{}", out(&from_root));
    let from_c1 = at(&scratch.path().join("c1"), &["-c", "name,malt"]);
    assert!(out(&from_c1).contains("3.000"), "{}", out(&from_c1));
    let alone = at(scratch.path(), &["c1/x.sample.md", "-c", "name,malt"]);
    assert!(out(&alone).contains("3.000"), "{}", out(&alone));
    // A profile only the root declares reaches every collection importing it.
    let profile = at(scratch.path(), &[".", "--profile", "p"]);
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    assert!(
        out(&profile).contains("c1-one") && out(&profile).contains("c2-one"),
        "{}",
        out(&profile)
    );
    assert!(!err(&profile).contains("not shown"), "{}", err(&profile));
}

#[test]
fn two_collections_writing_one_export_file_are_refused() {
    let scratch = Scratch::new("one-export-file");
    imported_collections(&scratch, "exports/e.csv");
    let refused = at(scratch.path(), &["export", "e", ".", "--write"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("the second would replace the first")
            && err(&refused).contains("{collection}"),
        "{}",
        err(&refused)
    );
    assert!(!scratch.path().join("exports/e.csv").exists());
    let apart = Scratch::new("one-export-file-apart");
    imported_collections(&apart, "exports/{collection}/e.csv");
    let written = at(apart.path(), &["export", "e", ".", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    for file in ["exports/e.csv", "exports/c1/e.csv", "exports/c2/e.csv"] {
        assert!(
            apart.path().join(file).exists(),
            "{file}: {}",
            err(&written)
        );
    }
}

#[test]
fn show_rc_names_the_imported_files() {
    let scratch = Scratch::new("show-rc-imports");
    imported_collections(&scratch, "exports/{collection}/e.csv");
    let output = at(&scratch.path().join("c1"), &["--show-rc"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let root = dunce::canonicalize(scratch.path().join(".samplekitrc")).unwrap();
    assert!(
        out(&output).contains(&format!("imports        {}", root.display())),
        "{}",
        out(&output)
    );
}

#[test]
fn a_warning_of_one_kind_is_one_line_and_v_shows_them() {
    // Paths as Unix writes them, whatever the system said them with.
    let err = |output: &Output| slashed(err(output));
    let scratch = Scratch::new("one-warning-line");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.only]\ncolumns = [{field = \"name\"}]\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    for project in ["b", "c"] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        scratch.sample(
            &format!("{project}/s.sample.md"),
            project,
            "approved",
            2.0,
            None,
        );
    }
    let output = at(scratch.path(), &[".", "--profile", "only"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = err(&output);
    assert_eq!(said.matches("warning:").count(), 1, "{said}");
    assert!(
        said.contains(
            "2 samples not shown: 2 configurations declare no profile 'only' — -v shows them"
        ),
        "{said}"
    );
    let verbose = at(scratch.path(), &[".", "--profile", "only", "-v"]);
    let said = err(&verbose);
    assert!(
        said.contains("b/.samplekitrc declares no profile 'only'")
            && said.contains("c/.samplekitrc declares no profile 'only'"),
        "{said}"
    );
    // Two targets that are not Markdown, likewise.
    scratch.write("notes.py", "print('hello')\n");
    scratch.write("notes.txt", "hello\n");
    let targets = at(
        scratch.path(),
        &["a.sample.md", "notes.py", "notes.txt", "-c", "name"],
    );
    let said = err(&targets);
    assert_eq!(said.matches("warning:").count(), 1, "{said}");
    assert!(
        said.contains("2 targets are not Markdown sample files, and were ignored — -v shows them"),
        "{said}"
    );
}

#[test]
fn an_export_writes_only_the_samples_of_its_configuration() {
    let scratch = Scratch::new("export-own-configuration");
    two_projects(&scratch);
    let output = at(scratch.path(), &["export", "e", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("root-one"), "{}", out(&output));
    assert!(!out(&output).contains("nested-one"), "{}", out(&output));
    assert!(
        err(&output).contains("1 sample not exported"),
        "{}",
        err(&output)
    );
}

/// Values whose input moved since they were computed: `volume` is stale, and
/// `plato` was edited by hand.
fn edited_and_stale(scratch: &Scratch) {
    scratch.config();
    scratch.write(
        "computed.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: aaaaaaaaaaaa}\n  plato:\n    v: 3.0\n    \
         fingerprint: bbbbbbbbbbbb\n    computed: {malt: aaaaaaaaaaaa}\n---\nN.\n",
    );
}

#[test]
fn explain_shows_each_input_with_its_value_and_state() {
    let scratch = Scratch::new("explain-input-values");
    edited_and_stale(&scratch);
    let output = at(scratch.path(), &["explain", "computed.sample.md", "volume"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("computed from"), "{text}");
    assert!(text.contains("malt") && text.contains("12.5"), "{text}");
    assert!(!text.contains("aaaaaaaaaaaa"), "{text}");
}

#[test]
fn explain_shows_the_digests_with_verbose() {
    let scratch = Scratch::new("explain-digests");
    edited_and_stale(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "computed.sample.md", "volume", "-v"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("aaaaaaaaaaaa"), "{}", out(&output));
}

#[test]
fn a_table_marks_edited_and_stale_values_in_the_terminal_only() {
    let scratch = Scratch::new("marks");
    edited_and_stale(&scratch);
    let terminal = at(
        scratch.path(),
        &[".", "-c", "malt,volume,plato", "--width", "120"],
    );
    assert_eq!(code(&terminal), 0, "{}", err(&terminal));
    assert!(out(&terminal).contains('✎'), "{}", out(&terminal));
    assert!(out(&terminal).contains('⚠'), "{}", out(&terminal));
    let piped = at(scratch.path(), &[".", "-c", "malt,volume"]);
    assert!(
        !out(&piped).contains('✎') && !out(&piped).contains('⚠'),
        "{}",
        out(&piped)
    );
    let exported = at(scratch.path(), &[".", "-c", "malt,volume", "--csv"]);
    assert!(!out(&exported).contains('✎'), "{}", out(&exported));
}

#[test]
fn the_identity_column_is_added_in_the_terminal_only() {
    let scratch = Scratch::new("identity");
    scratch.corpus();
    let terminal = at(scratch.path(), &[".", "-c", "malt", "--width", "120"]);
    assert_eq!(code(&terminal), 0, "{}", err(&terminal));
    assert!(out(&terminal).contains("keg-01"), "{}", out(&terminal));
    let piped = at(scratch.path(), &[".", "-c", "malt"]);
    assert!(!out(&piped).contains("keg-01"), "{}", out(&piped));
}

#[test]
fn an_identity_already_asked_for_is_not_repeated() {
    let scratch = Scratch::new("identity-asked");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "name,malt", "--width", "120"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).matches("keg-01").count(),
        1,
        "{}",
        out(&output)
    );
}

#[test]
fn identify_chooses_the_file_or_nothing() {
    let scratch = Scratch::new("identify-setting");
    scratch.corpus();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[render]\nidentify = \"filename\"\n",
    );
    let by_file = at(scratch.path(), &[".", "-c", "malt", "--width", "120"]);
    assert_eq!(code(&by_file), 0, "{}", err(&by_file));
    assert!(
        out(&by_file).contains("keg-01.sample.md"),
        "{}",
        out(&by_file)
    );
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[render]\nidentify = \"none\"\n",
    );
    let bare = at(scratch.path(), &[".", "-c", "malt", "--width", "120"]);
    assert!(!out(&bare).contains("keg-01"), "{}", out(&bare));
}

#[test]
fn tags_are_joined_in_csv_and_an_array_in_json() {
    let scratch = Scratch::new("tags-export");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\ntags: [dry_hopped, reference]\n---\n",
    );
    let csv = at(scratch.path(), &[".", "-c", "name,tags", "--csv"]);
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(
        out(&csv).contains("a,dry_hopped;reference"),
        "{}",
        out(&csv)
    );
    let json = at(scratch.path(), &[".", "-c", "name,tags", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out(&json)).unwrap();
    assert!(
        parsed[0]["tags"] == serde_json::json!(["dry_hopped", "reference"]),
        "{}",
        out(&json)
    );
}

#[test]
fn several_file_targets_of_one_project_are_read_together() {
    let scratch = Scratch::new("several-files");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["keg-01.sample.md", "keg-02.sample.md", "-c", "name,malt"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("keg-01") && text.contains("keg-02"), "{text}");
    assert!(!text.contains("keg-03"), "{text}");
    let filtered = at(
        scratch.path(),
        &[
            "keg-01.sample.md",
            "keg-04.sample.md",
            "-f",
            "status == \"approved\"",
            "-c",
            "name",
        ],
    );
    assert_eq!(code(&filtered), 0, "{}", err(&filtered));
    assert!(out(&filtered).contains("keg-01"), "{}", out(&filtered));
    assert!(!out(&filtered).contains("keg-04"), "{}", out(&filtered));
}

#[test]
fn a_mistyped_command_before_a_target_suggests_the_command() {
    let scratch = Scratch::new("mistyped-command");
    scratch.corpus();
    let typo = at(scratch.path(), &["statu", "keg-01.sample.md"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
    assert!(
        err(&typo).contains("did you mean 'samplekit status keg-01.sample.md'?"),
        "{}",
        err(&typo)
    );
}

#[test]
fn a_target_given_twice_is_read_once() {
    let scratch = Scratch::new("target-twice");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["keg-01.sample.md", "./keg-01.sample.md", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).matches("keg-01").count(),
        1,
        "{}",
        out(&output)
    );
    assert!(err(&output).contains("more than once"), "{}", err(&output));
}

#[test]
fn an_output_file_extension_chooses_its_format() {
    let scratch = Scratch::new("output-extension");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "-c", "name,malt", "-o", "out/t.csv", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("out/t.csv")).unwrap();
    assert!(written.starts_with("name,malt_value [g]"), "{written}");
}

#[test]
fn an_output_file_names_a_format_or_is_refused() {
    // `-o x.xlsx` wrote the terminal's box-drawn table into a spreadsheet's
    // name. An extension names a format; `.txt` is the table as shown.
    let scratch = Scratch::new("output-unknown-extension");
    scratch.corpus();
    for refused in ["out/t.xlsx", "out/t"] {
        let output = at(
            scratch.path(),
            &[".", "-c", "name,malt", "-o", refused, "--write"],
        );
        assert_eq!(code(&output), 1, "{refused}: {}", err(&output));
        assert!(
            err(&output).contains(".csv, .tsv or .json"),
            "{}",
            err(&output)
        );
        assert!(!scratch.path().join(refused).exists());
    }
    // Without columns, the list of paths is refused under a spreadsheet's name too.
    let paths = at(scratch.path(), &[".", "-o", "out/p.xlsx", "--write"]);
    assert_eq!(code(&paths), 1, "{}", err(&paths));
    assert!(!scratch.path().join("out/p.xlsx").exists());
    let text = at(
        scratch.path(),
        &[".", "-c", "name,malt", "-o", "t.txt", "--write"],
    );
    assert_eq!(code(&text), 0, "{}", err(&text));
    let named = at(
        scratch.path(),
        &[".", "-c", "name,malt", "-o", "t.data", "--csv", "--write"],
    );
    assert_eq!(code(&named), 0, "{}", err(&named));
    assert!(
        fs::read_to_string(scratch.path().join("t.data"))
            .unwrap()
            .starts_with("name,")
    );
}

#[test]
fn an_export_over_an_override_names_force() {
    // An override is what compute keeps: `compute --write` was offered for it.
    let scratch = Scratch::new("export-override");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, {field = \"brix\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"e.csv\"\n",
    );
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  \
         brix:\n    v: 9.0\n    computed: {malt: aaaaaaaaaaaa}\n    fingerprint: {edited: bbbbbbbbbbbb}\n---\n",
    );
    let preview = at(scratch.path(), &["export", "e", "."]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("--force --write"),
        "{}",
        out(&preview)
    );
}

#[test]
fn an_export_is_not_replaced_while_a_sample_cannot_be_read() {
    // A sample that could not be read has no row: replacing the file lost
    // what the previous export held of it.
    let scratch = Scratch::new("export-keeps");
    scratch.corpus();
    let first = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let before = fs::read_to_string(scratch.path().join("out/platos.csv")).unwrap();
    scratch.write(
        "keg-01.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties: [\n---\n",
    );
    let preview = at(scratch.path(), &["export", "platos", "."]);
    assert!(
        out(&preview).contains("--write keeps it"),
        "{}",
        out(&preview)
    );
    let kept = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&kept), 2, "{}", err(&kept));
    assert!(err(&kept).contains("not replaced"), "{}", err(&kept));
    assert_eq!(
        fs::read_to_string(scratch.path().join("out/platos.csv")).unwrap(),
        before
    );
}

#[test]
fn quiet_still_says_why_an_answer_is_incomplete() {
    // -q hid the only line that explained exit 2: a file not read.
    let scratch = Scratch::new("quiet-unread");
    scratch.corpus();
    scratch.write(
        "keg-01.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties: [\n---\n",
    );
    let quiet = at(scratch.path(), &[".", "-c", "name", "-q"]);
    assert_eq!(code(&quiet), 2, "{}", err(&quiet));
    assert!(err(&quiet).contains("could not be read"), "{}", err(&quiet));
    assert!(!err(&quiet).contains("warning"), "{}", err(&quiet));
}

#[test]
fn an_export_declared_by_a_sub_project_writes_its_samples() {
    let scratch = Scratch::new("sub-project-export");
    two_projects(&scratch);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n\
         [profile.p]\ncolumns = [{field = \"name\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"-\"\n",
    );
    let output = at(scratch.path(), &["export", "e", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("root-one"), "{}", out(&output));
    assert!(out(&output).contains("nested-one"), "{}", out(&output));
    assert!(!err(&output).contains("not exported"), "{}", err(&output));
}

#[test]
fn a_query_one_configuration_lacks_says_what_it_left_out() {
    let scratch = Scratch::new("query-left-out");
    two_projects(&scratch);
    let output = at(scratch.path(), &[".", "--query", "heavy", "-c", "name"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("1 sample not shown") && err(&output).contains("no query 'heavy'"),
        "{}",
        err(&output)
    );
}

/// A table whose derived cells read their row, and a column with a unit.
fn boil(scratch: &Scratch) {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "boil.sample.md",
        "---\nschema_version: 1\nname: boil\ntables:\n  mashing:\n    index: [T]\n    columns:\n      \
         T: {}\n      R: {unit: lintner}\n      G: {}\n    rows:\n      - T: 10\n        R: 2.0\n        G:\n          \
         v: 0.5\n          computed: {row.R: 000000000000}\n      - T: 20\n        R: 4.0\n        \
         G:\n          v: 0.25\n          computed: {row.R: 000000000000}\n---\n",
    );
}

#[test]
fn explain_shows_a_row_input_value() {
    let scratch = Scratch::new("explain-row-input");
    boil(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "boil.sample.md", "mashing.G[10]"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let line = out(&output)
        .lines()
        .find(|line| line.contains("row.R"))
        .map(str::to_string)
        .unwrap_or_default();
    assert!(
        line.contains('2') && !line.contains('—'),
        "{}",
        out(&output)
    );
}

#[test]
fn explain_of_a_row_that_does_not_exist_is_refused() {
    let scratch = Scratch::new("explain-no-row");
    boil(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "boil.sample.md", "mashing.G[99]"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("no such row"), "{}", err(&output));
}

#[test]
fn a_table_column_unit_reaches_the_header_and_the_json() {
    let scratch = Scratch::new("column-unit");
    boil(&scratch);
    let json = at(scratch.path(), &[".", "-c", "mashing.R[10]", "--json"]);
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(
        out(&json).contains("\"unit\": \"lintner\""),
        "{}",
        out(&json)
    );
}

#[test]
fn explain_says_a_formula_failed() {
    let scratch = Scratch::new("explain-failed");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "f.sample.md",
        "---\nschema_version: 1\nname: f\nproperties:\n  malt: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  \
         loading:\n    computed: {malt: aaaaaaaaaaaa}\n    \
         fingerprint: {failed: \"ZeroDivisionError: division by zero\"}\n---\n",
    );
    let output = at(scratch.path(), &["explain", "f.sample.md", "loading"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("ZeroDivisionError"),
        "{}",
        out(&output)
    );
}

#[test]
fn a_global_precision_leaves_counts_whole() {
    let scratch = Scratch::new("count-precision");
    table_corpus(&scratch);
    let output = at(
        scratch.path(),
        &[".", "-c", "foam.stats.count", "--precision", ".2e"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains('4'), "{}", out(&output));
    assert!(!out(&output).contains("e+00"), "{}", out(&output));
}

#[test]
fn json_keeps_an_empty_uncertainty_without_a_warning() {
    let scratch = Scratch::new("json-empty-uncertainty");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  body: {v: 3.0}\n---\n",
    );
    let output = at(scratch.path(), &[".", "-c", "name,body", "--json"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("\"uncertainty\": null"),
        "{}",
        out(&output)
    );
    assert!(!err(&output).contains("left out"), "{}", err(&output));
}

#[test]
fn a_text_holding_a_comma_is_written_quoted() {
    let scratch = Scratch::new("comma-text");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  mash_malt: {v: \"12,5\", unit: mg}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["tag", "add", "checked", "b.sample.md", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("b.sample.md")).unwrap();
    assert!(written.contains("\"12,5\""), "{written}");
}

#[test]
#[cfg(unix)]
fn a_read_only_file_is_not_rewritten() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("read-only");
    scratch.corpus();
    let file = scratch.path().join("keg-01.sample.md");
    let before = fs::read_to_string(&file).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    let output = at(
        scratch.path(),
        &["tag", "add", "fresh", "keg-01.sample.md", "--write"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(err(&output).contains("read-only"), "{}", err(&output));
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o444);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn a_selection_spanning_configurations_is_said_under_verbose() {
    let scratch = Scratch::new("spanning-verbose");
    two_projects(&scratch);
    let plain = at(scratch.path(), &["."]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(!err(&plain).contains("spans"), "{}", err(&plain));
    let verbose = at(scratch.path(), &[".", "-v"]);
    assert!(
        err(&verbose).contains("spans 2 configurations"),
        "{}",
        err(&verbose)
    );
}

#[test]
fn targets_of_two_projects_are_read_together() {
    let scratch = Scratch::new("targets-two-projects");
    two_projects(&scratch);
    let files = at(
        scratch.path(),
        &["a.sample.md", "nested/n.sample.md", "-c", "name"],
    );
    assert_eq!(code(&files), 0, "{}", err(&files));
    assert!(
        out(&files).contains("root-one") && out(&files).contains("nested-one"),
        "{}",
        out(&files)
    );
    let mixed = at(scratch.path(), &["nested", "a.sample.md", "-c", "name"]);
    assert_eq!(code(&mixed), 0, "{}", err(&mixed));
    assert!(
        out(&mixed).contains("root-one") && out(&mixed).contains("nested-one"),
        "{}",
        out(&mixed)
    );
}

#[test]
fn each_project_writes_its_rows_at_its_own_precision() {
    // A CSV over two projects wrote every row at the first target's
    // precision: the numbers a file held turned on the order of the targets.
    let scratch = Scratch::new("precision-per-project");
    for (project, precision, malt) in [("one", ".1f", 1.23456), ("two", ".3f", 2.34567)] {
        scratch.write(
            &format!("{project}/.samplekitrc"),
            &format!("schema_version = 1\n[property.malt]\nprecision = \"{precision}\"\n"),
        );
        scratch.write(
            &format!("{project}/s.md"),
            &format!("---\nschema_version: 1\nname: {project}\nproperties:\n  malt: {malt}\n---\n"),
        );
    }
    for order in [["one", "two"], ["two", "one"]] {
        let written = at(
            scratch.path(),
            &[order[0], order[1], "-c", "name,malt", "--csv"],
        );
        assert_eq!(code(&written), 0, "{}", err(&written));
        let text = out(&written);
        // No uncertainty column: no sample fills one.
        assert!(text.contains("one,1.2\n"), "{order:?}: {text}");
        assert!(text.contains("two,2.346\n"), "{order:?}: {text}");
    }
}

#[test]
fn an_export_over_two_projects_names_each_ones_samples_to_compute() {
    // The way to bring a project's export up to date named the first target
    // given, for the other project's stale samples.
    let scratch = Scratch::new("export-two-projects-hint");
    for project in ["one", "two"] {
        scratch.write(
            &format!("{project}/.samplekitrc"),
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, {field = \"brix\"}]\n\
             [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"out/e.csv\"\n",
        );
    }
    scratch.write(
        "one/s.md",
        "---\nschema_version: 1\nname: fresh\nproperties:\n  brix: 2.0\n---\n",
    );
    // A record naming an input digest that is not the input's: stale.
    scratch.write(
        "two/s.md",
        "---\nschema_version: 1\nname: behind\nproperties:\n  \
         malt: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  brix:\n    v: 3.0\n    \
         computed: {malt: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
    );
    let output = at(scratch.path(), &["export", "e", "one", "two"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(
        said.contains("samplekit compute two --force --write"),
        "{said}"
    );
    assert!(!said.contains("samplekit compute one"), "{said}");
}

#[test]
fn a_target_that_does_not_exist_is_said() {
    let scratch = Scratch::new("target-missing");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "platos"]);
    assert_eq!(code(&output), 3, "{}", out(&output));
    assert!(err(&output).contains("no such file"), "{}", err(&output));
}

#[test]
fn a_target_that_is_not_markdown_is_ignored_and_said() {
    let scratch = Scratch::new("target-not-markdown");
    scratch.corpus();
    scratch.write("notes.py", "print('hello')\n");
    let output = at(
        scratch.path(),
        &["keg-01.sample.md", "notes.py", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("keg-01"), "{}", out(&output));
    assert!(
        err(&output).contains("not a Markdown sample file"),
        "{}",
        err(&output)
    );
}

#[test]
fn quiet_prints_no_warning() {
    let scratch = Scratch::new("quiet");
    scratch.corpus();
    scratch.write("bad.sample.md", "---\nschema_version: 1\nname: [\n---\n");
    let plain = at(scratch.path(), &[".", "-c", "name"]);
    assert!(err(&plain).contains("was not read"), "{}", err(&plain));
    let quiet = at(scratch.path(), &[".", "-c", "name", "--quiet"]);
    // Quiet, and the code still says the answer is partial.
    assert_eq!(code(&quiet), 2, "{}", err(&quiet));
    assert!(!err(&quiet).contains("warning"), "{}", err(&quiet));
}

#[test]
fn a_data_format_over_several_configurations_is_one_table() {
    let scratch = Scratch::new("format-several");
    two_projects(&scratch);
    let output = at(scratch.path(), &[".", "-c", "name", "--csv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("root-one") && out(&output).contains("nested-one"),
        "{}",
        out(&output)
    );
}

#[test]
fn a_summary_over_several_configurations_is_one_summary() {
    let scratch = Scratch::new("summary-several");
    two_projects(&scratch);
    scratch.sample("b.sample.md", "root-two", "approved", 1.0, Some(2.5));
    let output = at(scratch.path(), &[".", "-c", "brix", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("1/3"), "{}", out(&output));
    assert!(!out(&output).contains(".samplekitrc"), "{}", out(&output));
}

#[test]
fn a_declared_export_creates_its_directory() {
    let scratch = Scratch::new("export-creates-directory");
    scratch.corpus();
    fs::remove_dir_all(scratch.path().join("out")).unwrap();
    let output = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.path().join("out/platos.csv").exists());
}

#[test]
fn a_narrowed_declared_export_says_so() {
    let scratch = Scratch::new("export-narrowed");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["export", "platos", ".", "-o", "-", "--query", "approved"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(err(&output).contains("narrowed"), "{}", err(&output));
}

#[test]
fn a_failed_value_is_marked_in_the_table() {
    let scratch = Scratch::new("failed-mark");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "f.sample.md",
        "---\nschema_version: 1\nname: f\nproperties:\n  malt: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  \
         loading:\n    computed: {malt: aaaaaaaaaaaa}\n    \
         fingerprint: {failed: \"ZeroDivisionError: division by zero\"}\n---\n",
    );
    let output = at(scratch.path(), &[".", "-c", "loading", "--width", "120"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains('✗'), "{}", out(&output));
}

#[test]
fn a_view_carries_the_marks() {
    let scratch = Scratch::new("view-marks");
    edited_and_stale(&scratch);
    let output = at(
        scratch.path(),
        &["view", "computed.sample.md", "--width", "120"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains('⚠') && out(&output).contains('✎'),
        "{}",
        out(&output)
    );
}

#[test]
fn json_gives_each_quantity_its_state() {
    let scratch = Scratch::new("json-state");
    edited_and_stale(&scratch);
    let output = at(scratch.path(), &[".", "-c", "name,volume,plato", "--json"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("\"state\": \"outdated\""),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("\"state\": \"edited\""),
        "{}",
        out(&output)
    );
}

#[test]
fn explain_names_a_statistic_as_one() {
    let scratch = Scratch::new("explain-statistic");
    table_corpus(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "item.sample.md", "foam.stats.mean"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a statistic of 4 readings"),
        "{}",
        out(&output)
    );
    assert!(!out(&output).contains("entered"), "{}", out(&output));
}

#[test]
fn explain_gives_a_row_input_its_state() {
    let scratch = Scratch::new("explain-row-state");
    boil(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "boil.sample.md", "mashing.G[10]"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let line = out(&output)
        .lines()
        .find(|line| line.contains("row.R"))
        .map(str::to_string)
        .unwrap_or_default();
    assert!(line.contains("entered"), "{}", out(&output));
}

#[test]
fn explain_of_a_table_describes_its_columns() {
    // Asking about a table is a fair question — what its columns are, which are
    // measured, which the model fills and from what — and refusing it sent the
    // reader away with nothing. Replaces `explain_of_a_table_asks_for_a_cell`,
    // which asserted the refusal.
    let scratch = Scratch::new("explain-table");
    boil(&scratch);
    let output = at(scratch.path(), &["explain", "boil.sample.md", "mashing"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("2 rows, 3 columns"), "{shown}");
    // The index, a measured column, and one the model fills, each said as such.
    assert!(shown.contains("index"), "{shown}");
    assert!(shown.contains("measured"), "{shown}");
    // `computed` is the word the format itself uses, rather than a second
    // vocabulary for the same thing.
    assert!(shown.contains("computed from"), "{shown}");
    // And what that column was computed from, which is what explain is for.
    assert!(shown.contains("row.R"), "{shown}");
    // It still points at the cell, for the number itself.
    assert!(shown.contains("mashing.<column>[<index>]"), "{shown}");
}

#[test]
fn list_takes_several_file_targets() {
    let scratch = Scratch::new("list-several");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["list", "keg-01.sample.md", "keg-02.sample.md"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("2 samples"), "{}", out(&output));
}

#[test]
fn show_rc_names_every_configuration_a_directory_spans() {
    let scratch = Scratch::new("show-rc-several");
    two_projects(&scratch);
    let output = at(scratch.path(), &[".", "--show-rc"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).matches("configuration  ").count(),
        2,
        "{}",
        out(&output)
    );
}

#[test]
fn a_newer_schema_asks_for_a_newer_samplekit() {
    let scratch = Scratch::new("newer-schema");
    scratch.write("future.md", "---\nschema_version: 7\nname: future\n---\n");
    let refused = at(scratch.path(), &["validate", "future.md"]);
    assert_eq!(code(&refused), 2, "{}", err(&refused));
    assert!(
        err(&refused).contains("upgrade SampleKit"),
        "{}",
        err(&refused)
    );
    // And nothing is offered in place of it: a file this build cannot read is
    // not a file it knows how to convert.
    assert!(
        !err(&refused).contains("no command converts it"),
        "{}",
        err(&refused)
    );
}

#[test]
fn output_formats_are_mutually_exclusive_among_themselves() {
    let scratch = Scratch::new("formats-exclusive");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "malt", "--csv", "--json"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("cannot be used with"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_summary_shows_the_sample_deviation_its_error_and_the_median() {
    let scratch = Scratch::new("summary-columns");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "malt", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let header = out(&output)
        .lines()
        .find(|line| line.contains("mean"))
        .unwrap_or_default()
        .to_string();
    for column in ["n", "mean", " s ", "sem", "median", "min", "max"] {
        assert!(header.contains(column), "{column}: {}", out(&output));
    }
    assert!(out(&output).contains("malt [g]"), "{}", out(&output));
}

#[test]
fn a_summary_of_one_value_has_no_deviation() {
    let scratch = Scratch::new("summary-one");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["keg-01.sample.md", "-c", "malt", "--summary"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains('—'), "{}", out(&output));
}

#[test]
fn a_summary_without_a_precision_rounds_to_its_deviation() {
    let scratch = Scratch::new("summary-convention");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (file, value) in [
        ("a.sample.md", 1.23456),
        ("b.sample.md", 2.34567),
        ("c.sample.md", 3.45678),
    ] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  x: {value}\n---\n"),
        );
    }
    let output = at(scratch.path(), &[".", "-c", "x", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("2.3"), "{}", out(&output));
    assert!(!out(&output).contains("2.345670"), "{}", out(&output));
}

#[test]
fn tag_rename_repairs_an_unusable_tag() {
    let scratch = Scratch::new("tag-unusable");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\ntags: [reference, \"my tag\"]\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &["tag", "rename", "my tag", "my_tag", ".", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = fs::read_to_string(scratch.path().join("a.sample.md")).unwrap();
    assert!(
        text.contains("my_tag") && !text.contains("my tag"),
        "{text}"
    );
}

#[test]
fn validate_reports_an_unread_configuration_and_the_rest() {
    let scratch = Scratch::new("validate-unread-rc");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  malt: {v: 1.0}\n---\nN.\n",
    );
    fs::create_dir_all(scratch.path().join("sub")).unwrap();
    scratch.write("sub/.samplekitrc", "schema_version = 1\n[view\n");
    scratch.write(
        "sub/b.sample.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains(".samplekitrc"), "{text}");
    assert!(text.contains("2 samples"), "{text}");
}

/// A command line split as a shell splits it: words, with single and double
/// quotes holding spaces.
fn shell_words(line: &str) -> Vec<String> {
    let (mut words, mut word, mut quote, mut started) = (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, ' ') => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => word.push(c),
        }
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    words
}

#[test]
fn every_help_page_ends_with_examples() {
    // Every page, summary and reference alike, ends with examples, and each is
    // a command line the parser accepts.
    let scratch = Scratch::new("help-examples");
    scratch.corpus();
    for page in [
        vec![],
        vec!["list"],
        vec!["status"],
        vec!["compute"],
        vec!["explain"],
        vec!["view"],
        vec!["export"],
        vec!["validate"],
        vec!["tag"],
        vec!["tag", "add"],
        vec!["tag", "remove"],
        vec!["tag", "rename"],
        vec!["init"],
        vec!["new"],
        vec!["set"],
        vec!["completions"],
    ] {
        for flag in ["-h", "--help"] {
            let mut arguments = page.clone();
            arguments.push(flag);
            let text = out(&at(scratch.path(), &arguments));
            let examples: Vec<&str> = text
                .split_once("Examples:\n")
                .unwrap_or_else(|| panic!("{arguments:?} has no examples:\n{text}"))
                .1
                .lines()
                .filter(|line| line.starts_with("  samplekit "))
                .collect();
            assert!(!examples.is_empty(), "{arguments:?}: {text}");
            if flag == "-h" || page.first() == Some(&"completions") {
                continue;
            }
            for example in examples {
                let mut words = shell_words(example.trim());
                words.remove(0);
                // `help` takes a command, not options: its page is checked above.
                if words.first().map(String::as_str) == Some("help") {
                    continue;
                }
                words.push("--show-rc".to_string());
                let words: Vec<&str> = words.iter().map(String::as_str).collect();
                let output = at(scratch.path(), &words);
                assert_eq!(code(&output), 0, "{example}: {}", err(&output));
            }
        }
    }
}

#[test]
fn stale_names_the_command_that_computes() {
    // A listing of what is not current ends with what computes it.
    let scratch = Scratch::new("stale-next-step");
    scratch.config();
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: 000000000000}\n---\nN.\n",
    );
    let hint = "samplekit compute lists what would run; --write computes and writes";
    let text = out(&at(scratch.path(), &["status", "."]));
    assert!(text.contains(hint), "{text}");
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  malt: 12.5\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &["status", "."]));
    assert!(!text.contains(hint), "{text}");
}

#[test]
fn status_names_the_log_of_what_failed() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    // The file keeps the exception's type alone, so the traceback is findable
    // only if the listing says where it went.
    let scratch = Scratch::new("status-failure-log");
    scratch.config();
    scratch.write(
        "f.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    \
         computed: {malt: aaaaaaaaaaaa}\n    fingerprint: {failed: ZeroDivisionError}\n---\nN.\n",
    );
    scratch.write(
        ".samplekit/failures/f.sample.log",
        "## loading\nTraceback (most recent call last):\nZeroDivisionError: division by zero\n",
    );
    let text = out(&at(scratch.path(), &["status", "."]));
    assert!(text.contains("failed \u{2014} ZeroDivisionError"), "{text}");
    assert!(text.contains(".samplekit/failures/f.sample.log"), "{text}");
}

#[test]
fn status_says_when_a_failure_kept_no_traceback() {
    // A failure a Python save wrote has no log beside it, and `status` pointed
    // at one anyway: the reader went looking for a file that does not exist.
    let scratch = Scratch::new("status-no-failure-log");
    scratch.config();
    scratch.write(
        "f.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    \
         computed: {malt: aaaaaaaaaaaa}\n    fingerprint: {failed: ZeroDivisionError}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &["status", "."]));
    assert!(!text.contains(".samplekit/failures/"), "{text}");
    assert!(text.contains("kept no traceback"), "{text}");
}

#[test]
fn properties_completion_offers_values_and_table_columns() {
    let scratch = Scratch::new("completion-properties");
    table_corpus(&scratch);
    let complete = |current: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["--", "samplekit", "compute", ".", "-p", current])
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "4")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{current}: {}", err(&output));
        out(&output)
    };
    let first = complete("");
    assert!(first.lines().any(|line| line == "malt"), "{first}");
    assert!(
        first.lines().any(|line| line == "measurements:table"),
        "{first}"
    );
    assert!(!first.contains('['), "{first}");
    let columns = complete("measurements.");
    assert!(
        columns.lines().any(|line| line == "measurements.ph"),
        "{columns}"
    );
    assert!(!columns.contains('['), "{columns}");
    let again = complete("malt,");
    assert!(
        again.lines().any(|line| line == "malt,measurements:table"),
        "{again}"
    );
    let bridge = out(&at(scratch.path(), &["completions", "zsh"]));
    assert!(bridge.contains("--properties"), "{bridge}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_table_measures_the_terminal_it_is_drawn_on() {
    // Without $COLUMNS, a table reads the width the terminal reports.
    let scratch = Scratch::new("terminal-width");
    scratch.corpus();
    let drawn = |columns: u32| {
        let line = format!(
            "stty cols {columns} rows 40; {} . -c name,status,malt,brix",
            shell_word(env!("CARGO_BIN_EXE_samplekit"))
        );
        let output = Command::new("script")
            .args(["--quiet", "--return", "--command", &line, "/dev/null"])
            .current_dir(scratch.path())
            .env_remove("COLUMNS")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("script creates a pseudoterminal");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let narrow = drawn(30);
    assert!(
        narrow.contains("hidden — --width shows them, or -c names fewer"),
        "{narrow:?}"
    );
    let wide = drawn(200);
    assert!(wide.contains("keg-01"), "{wide:?}");
    assert!(!wide.contains("hidden"), "{wide:?}");
}

// --------------------------------------------------- `init`, `new` and `set`

/// A project written by `init`, without git or an environment: the tests here
/// never start an interpreter, and a model is read as a file like any other.
fn set_up(scratch: &Scratch) -> Output {
    at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    )
}

/// `--model PATH` names a model one has: from the project's folder when it is
/// inside, and a path that names no `.py` file writes nothing.
#[test]
fn init_names_a_model_one_already_has() {
    let scratch = Scratch::new("init-model");
    std::fs::create_dir_all(scratch.path().join("models")).unwrap();
    std::fs::write(
        scratch.path().join("models/mine.py"),
        "import samplekit as sk\n",
    )
    .unwrap();
    let refused = at(
        scratch.path(),
        &["init", "--model", "models/none.py", "--write"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(err(&refused).contains("no file there"), "{}", err(&refused));
    assert!(!scratch.path().join(".samplekitrc").exists());
    let written = at(
        scratch.path(),
        &["init", "--model", "models/mine.py", "--no-venv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let rc = std::fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(rc.contains("path = \"models/mine.py\""), "{rc}");
    assert!(!scratch.path().join("model/main.py").exists());
}

#[test]
fn init_previews_before_it_writes() {
    let scratch = Scratch::new("init-preview");
    let preview = at(scratch.path(), &["init", "--example", "--no-venv"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("nothing written"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.path().join(".samplekitrc").exists());
    let written = set_up(&scratch);
    assert_eq!(code(&written), 0, "{}", err(&written));
    for file in [
        ".samplekitrc",
        "model/brew.py",
        "model/helpers/__init__.py",
        "model/helpers/hydrometer.py",
        "samples/EXAMPLE.md",
    ] {
        assert!(scratch.path().join(file).exists(), "{file} was not written");
    }
}

#[test]
fn init_keeps_a_file_that_is_already_there() {
    // Running it twice is how a guided setup absorbs a step it did not run the
    // first time, so the second run must be safe by construction.
    let scratch = Scratch::new("init-again");
    assert_eq!(code(&set_up(&scratch)), 0);
    let mine = "---\nschema_version: 1\nname: MINE\n---\nMy own sample.\n";
    scratch.write("samples/EXAMPLE.md", mine);
    fs::remove_file(scratch.path().join("model/helpers/hydrometer.py")).unwrap();
    let again = set_up(&scratch);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("already here, kept"),
        "{}",
        out(&again)
    );
    assert!(scratch.path().join("model/helpers/hydrometer.py").exists());
    let kept = fs::read_to_string(scratch.path().join("samples/EXAMPLE.md")).unwrap();
    assert_eq!(kept, mine, "an existing file was overwritten");
}

#[test]
fn init_names_what_to_try_next() {
    // A project that runs is useless to someone who does not know which verb
    // comes first.
    let scratch = Scratch::new("init-next");
    let written = set_up(&scratch);
    let shown = out(&written);
    assert!(shown.contains("samplekit compute samples/"), "{shown}");
    assert!(shown.contains("samplekit status samples/"), "{shown}");
}

/// Every command `init` teaches is a command that exists, spelt the way its
/// own `--help` spells it.
///
/// It taught `tag add PATH TAG` for a week; the real order is `tag add TAG
/// PATH`. Nothing caught it, because nothing read the lines back — the block
/// was checked for the verbs it mentions and never for the grammar it claims.
#[test]
fn every_command_init_suggests_is_spelt_the_way_it_is_used() {
    /// What a placeholder stands for, so that two spellings of one thing
    /// (`PATH` in a suggestion, `FILE` in a usage line) compare equal while a
    /// tag standing where a path belongs does not.
    fn kind(word: &str) -> Option<&'static str> {
        let bare = word
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '=')
            .split('=')
            .next()
            .unwrap_or(word)
            .to_uppercase();
        match bare.as_str() {
            "PATH" | "FILE" | "TARGET" | "TARGETS" => Some("a path"),
            "NAME" | "TAG" | "EXPORT" | "VIEW" | "SHELL" => Some("a name"),
            "FIELD" => Some("a field"),
            _ => None,
        }
    }

    let scratch = Scratch::new("init-spelling");
    let shown = out(&set_up(&scratch));
    let mut checked = 0;
    for line in shown.lines() {
        // Each suggestion is `samplekit …` and then, past a run of spaces,
        // what it is for. Only the command half is a command.
        let Some(suggested) = line.trim().strip_prefix("samplekit ") else {
            continue;
        };
        let suggested = suggested.split("  ").next().unwrap_or(suggested).trim();
        let words: Vec<&str> = suggested.split_whitespace().collect();
        let verb: Vec<&str> = words
            .iter()
            .take_while(|word| word.chars().all(|c| c.is_lowercase() || c == '-'))
            .copied()
            .collect();
        if verb.is_empty() || verb[0].starts_with('-') {
            continue;
        }
        let mut asked: Vec<&str> = verb.clone();
        asked.push("--help");
        let help = at(scratch.path(), &asked);
        assert_eq!(
            code(&help),
            0,
            "init suggests '{suggested}', which is not a command"
        );
        let usage = out(&help)
            .lines()
            .find(|line| line.trim_start().starts_with("Usage:"))
            .unwrap_or_default()
            .to_string();
        let used: Vec<&'static str> = words[verb.len()..].iter().filter_map(|w| kind(w)).collect();
        let declared: Vec<&'static str> = usage
            .split_whitespace()
            .skip(verb.len() + 1)
            .filter(|word| word.contains('<') || word.contains('['))
            .filter_map(|w| kind(w))
            .collect();
        for (position, wanted) in used.iter().enumerate() {
            assert_eq!(
                declared.get(position),
                Some(wanted),
                "init suggests '{suggested}': argument {} is {wanted}, but \
                 '{}' takes {declared:?}",
                position + 1,
                verb.join(" ")
            );
        }
        checked += 1;
    }
    // The suggestions that name a verb: `compute`, `status`, `new`, `set`,
    // `tag add`, `validate`. The three that do not — the bare target, the
    // profile, and `--help` — have no grammar to disagree with.
    assert!(checked >= 6, "only {checked} suggestions were read back");
}

#[test]
fn new_writes_what_was_given_and_no_name() {
    // The file's name is the sample's, so `name:` is not written — nor copied
    // from a pattern writing one.
    let scratch = Scratch::new("new-bare");
    assert_eq!(code(&set_up(&scratch)), 0);
    let output = at(scratch.path(), &["new", "C-02", "malt=12.4", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("samples/C-02.md")).unwrap();
    assert!(!written.contains("name:"), "{written}");
    assert!(written.contains("12.4"), "{written}");
    // The pattern's other quantities are a measurement nobody made.
    assert!(!written.contains("ibu"), "{written}");
    let named = at(scratch.path(), &["-c", "name", "samples/C-02.md"]);
    assert!(out(&named).contains("C-02"), "{}", out(&named));
    scratch.write(
        "samples/C-01.md",
        "---\nschema_version: 1\nname: first\nbatch: B7\n---\n",
    );
    let like = at(
        scratch.path(),
        &["new", "C-03", "--like", "samples/C-01.md", "--write"],
    );
    assert_eq!(code(&like), 0, "{}", err(&like));
    let written = fs::read_to_string(scratch.path().join("samples/C-03.md")).unwrap();
    assert!(!written.contains("name:"), "{written}");
    assert!(written.contains("batch: B7"), "{written}");
}

#[test]
fn new_like_takes_the_shape_and_not_the_values() {
    let scratch = Scratch::new("new-like");
    assert_eq!(code(&set_up(&scratch)), 0);
    let output = at(
        scratch.path(),
        &["new", "C-03", "--like", "samples/EXAMPLE.md"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("measured, for you to fill in"), "{shown}");
    assert!(shown.contains("copied from the pattern"), "{shown}");
    assert!(shown.contains("tables, not carried over"), "{shown}");
    assert!(shown.contains("nothing written"), "{shown}");
}

#[test]
fn new_like_carries_neither_dates_nor_status() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    // As the workbench's `N`, the pattern's dates and `status` are its own,
    // said `not carried`; one given on the command line is written.
    let scratch = Scratch::new("new-like-own");
    assert_eq!(code(&set_up(&scratch)), 0);
    let pattern = scratch.path().join("samples/EXAMPLE.md");
    let text = fs::read_to_string(&pattern).unwrap();
    fs::write(
        &pattern,
        text.replace("style: pale ale\n", "style: pale ale\nstatus: approved\n"),
    )
    .unwrap();
    let output = at(
        scratch.path(),
        &["new", "C-04", "--like", "samples/EXAMPLE.md", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(
        shown
            .lines()
            .any(|line| line.contains("not carried") && line.contains("status, brewed")),
        "{shown}"
    );
    assert!(shown.contains("style = pale ale"), "{shown}");
    let written = fs::read_to_string(scratch.path().join("samples/C-04.md")).unwrap();
    assert!(written.contains("style: pale ale"), "{written}");
    assert!(!written.contains("brewed: 2026"), "{written}");
    assert!(!written.contains("status"), "{written}");

    let given = at(
        scratch.path(),
        &[
            "new",
            "C-05",
            "--like",
            "samples/EXAMPLE.md",
            "status=draft",
        ],
    );
    assert_eq!(code(&given), 0, "{}", err(&given));
    let line = out(&given)
        .lines()
        .find(|line| line.contains("not carried"))
        .map(str::to_string)
        .unwrap_or_default();
    assert!(
        line.contains("brewed") && !line.contains("status"),
        "{}",
        out(&given)
    );
}

#[test]
fn new_like_takes_readings_for_a_measurement_to_fill_in() {
    // A statistic of readings records `readings` as its input: it was taken
    // for a value the model computes, and left out of the new sample.
    let scratch = Scratch::new("new-like-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "brew.md",
        "---\nschema_version: 1\nname: brew\nproperties:\n  og:\n    v: 1.05\n    \
         readings: [1.049, 1.05, 1.051]\n    u: 0.0006\n    \
         statistics: {v: mean, u: standard_error}\n    computed: {readings: aaaaaaaaaaaa}\n    \
         fingerprint: bbbbbbbbbbbb\n  abv:\n    v: 5.0\n    computed: {og: bbbbbbbbbbbb}\n    \
         fingerprint: cccccccccccc\n---\n",
    );
    let output = at(scratch.path(), &["new", "next", "--like", "brew.md"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    let measured = shown
        .lines()
        .find(|line| line.contains("measured, for you to fill in"))
        .unwrap_or_default();
    assert!(measured.contains("og"), "{shown}");
    let left = shown
        .lines()
        .find(|line| line.contains("left out"))
        .unwrap_or_default();
    assert!(left.contains("abv") && !left.contains("og"), "{shown}");
}

#[test]
fn text_for_a_declared_quantity_is_refused_and_an_attribute_repaired() {
    // `new … volume=20,5` wrote the text as an attribute of the quantity's
    // name, which the model then refused: a project no command could compute.
    let scratch = Scratch::new("text-for-quantity");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n",
    );
    let refused = at(scratch.path(), &["new", "b1", "volume=20,5", "--write"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(err(&refused).contains("write 20.5"), "{}", err(&refused));
    assert!(!scratch.path().join("b1.md").exists());
    // Written as text before: a number puts it right, as the quantity.
    scratch.write(
        "old.md",
        "---\nschema_version: 1\nname: old\nvolume: \"20,5\"\n---\n",
    );
    let repaired = at(scratch.path(), &["set", "old.md", "volume=20.5", "--write"]);
    assert_eq!(code(&repaired), 0, "{}", err(&repaired));
    let written = fs::read_to_string(scratch.path().join("old.md")).unwrap();
    assert!(
        written.contains("properties:\n  volume: {v: 20.5, unit: L"),
        "{written}"
    );
}

#[test]
fn the_first_row_of_a_table_takes_its_columns_from_the_models_description() {
    // No sample held the table yet, and its columns were the model's: the hint
    // said to compute once, which creates no table. The model's description
    // says them.
    let scratch = Scratch::new("first-row");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Brew\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\n\nclass Brew(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.fermentation = sk.Table(\n            {\n                \
         \"day\": sk.Column(unit=\"d\"),\n                \"gravity\": sk.Column(),\n            },\n            \
         \"day\",\n        )\n",
    );
    scratch.write("b1.md", "---\nschema_version: 1\nname: b1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({}),
        serde_json::json!({"fermentation": {
            "title": null, "index": ["day"], "rows": [],
            "columns": [
                {"name": "day", "unit": "d", "value": {"from": "entered"},
                 "uncertainty": null, "formula": null},
                {"name": "gravity", "unit": null, "value": {"from": "entered"},
                 "uncertainty": null, "formula": null},
            ],
        }}),
    );
    let added = at(
        scratch.path(),
        &[
            "set",
            "b1.md",
            "--add-row",
            "fermentation",
            "gravity=1.040",
            "day=1",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    let written = fs::read_to_string(scratch.path().join("b1.md")).unwrap();
    assert!(written.contains("index: day"), "{written}");
    assert!(written.contains("day: {unit: d}"), "{written}");
}

#[test]
fn new_refuses_a_name_already_taken() {
    let scratch = Scratch::new("new-taken");
    assert_eq!(code(&set_up(&scratch)), 0);
    let output = at(scratch.path(), &["new", "EXAMPLE", "--write"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    let said = err(&output);
    assert!(said.contains("already here"), "{said}");
    assert!(said.contains("samplekit set"), "{said}");
}

/// One entered value and one the file says a formula produced, with the records
/// that make it derived. No model: `set` answers from the file.
fn derived(scratch: &Scratch) -> PathBuf {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s.sample.md",
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  brix:\n    v: 3.0\n    \
         computed: {malt: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n---\nNotes.\n",
    )
}

#[test]
fn set_previews_before_it_writes() {
    let scratch = Scratch::new("set-preview");
    let file = derived(&scratch);
    let before = fs::read_to_string(&file).unwrap();
    let preview = at(scratch.path(), &["set", "s.sample.md", "malt=13.0"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("nothing written"),
        "{}",
        out(&preview)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let written = at(
        scratch.path(),
        &["set", "s.sample.md", "malt=13.0", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(fs::read_to_string(&file).unwrap().contains("13"));
}

#[test]
fn set_says_what_a_change_makes_stale() {
    let scratch = Scratch::new("set-stale");
    derived(&scratch);
    let output = at(scratch.path(), &["set", "s.sample.md", "malt=13.0"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("brix"), "{shown}");
    assert!(shown.contains("outdated"), "{shown}");
    assert!(shown.contains("samplekit compute"), "{shown}");
}

#[test]
fn set_on_a_derived_value_writes_the_override_marked() {
    // What makes it derived is the record in the file, not the model: the answer
    // is the same with no interpreter in sight. And the file says so: the
    // formula's record stays, the value's own digest is marked edited, and
    // `status` reads it as edited — not as record missing.
    let scratch = Scratch::new("set-override");
    let file = derived(&scratch);
    let output = at(scratch.path(), &["set", "s.sample.md", "brix=9.0"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("override"), "{}", out(&output));
    // Compute keeps an override: the way back is --force, not a plain compute.
    assert!(out(&output).contains("--force --write"), "{}", out(&output));
    assert!(
        !out(&output).contains("recomputes them"),
        "{}",
        out(&output)
    );
    let written = at(
        scratch.path(),
        &["set", "s.sample.md", "brix=9.0", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("computed: {malt: aaaaaaaaaaaa}"), "{text}");
    assert!(text.contains("fingerprint: {edited: "), "{text}");
    let status = at(scratch.path(), &["status", "s.sample.md"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("brix") && out(&status).contains("edited since it was computed"),
        "{}",
        out(&status)
    );
    assert!(!out(&status).contains("record missing"), "{}", out(&status));
}

#[test]
fn explain_says_a_statistic_of_its_own_readings() {
    // A record naming the reserved `readings` is this property's own
    // observations, never an input the sample holds.
    let scratch = Scratch::new("explain-own-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  \
         malt:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: a91f42c8f0d1}\n    fingerprint: d312b70e4a55\n---\n",
    );
    let output = at(scratch.path(), &["explain", "w.sample.md", "malt"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(
        shown.contains("its own readings"),
        "said as a statistic of its observations: {shown}"
    );
    assert!(
        !shown.contains("computed from\n"),
        "never as a heading over an input the sample holds: {shown}"
    );
}

#[test]
fn explain_never_calls_an_edited_value_entered() {
    // The provenance line and the state line must agree: a value the file marks
    // edited was not entered from nothing.
    let scratch = Scratch::new("explain-edited");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  \
         malt: {v: 12.5, unit: g, fingerprint: {edited: 7c01e2b3a9f4}}\n---\n",
    );
    let output = at(scratch.path(), &["explain", "w.sample.md", "malt"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("edited"), "the state line stands: {shown}");
    assert!(
        !shown.contains("entered or measured"),
        "and the provenance line no longer contradicts it: {shown}"
    );
}

#[test]
fn stale_names_a_statistic_its_readings_left_behind() {
    // A corrected reading leaves the declared statistic behind, and `status`
    // names the observations rather than staying silent.
    let scratch = Scratch::new("stale-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  \
         malt:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: 000000000000}\n---\n",
    );
    let output = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("malt"), "{shown}");
    assert!(
        shown.contains("readings"),
        "the observations are named as what moved: {shown}"
    );
}

#[test]
fn the_units_that_disagree_can_be_listed() {
    // `-c name,m.unit` over kg and g was refused as a column with two units,
    // where it is how a reader finds which files disagree.
    let scratch = Scratch::new("list-units");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  m: {v: 1.0, unit: kg}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  m: {v: 1.0, unit: g}\n---\n",
    );
    let listed = at(scratch.path(), &[".", "-c", "name,m.unit"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("kg") && out(&listed).contains('g'),
        "{}",
        out(&listed)
    );
    // The values themselves are still refused a header that would lie.
    let values = at(scratch.path(), &[".", "-c", "name,m"]);
    assert_ne!(code(&values), 0, "{}", out(&values));
}

#[test]
fn new_readings_are_said_by_the_statistic_their_file_records() {
    // The preview said `mean 8` over readings whose declared statistic is the
    // median, 3.
    let scratch = Scratch::new("readings-median");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  a:\n    v: 2.0\n    readings: [1.0, 2.0, 10.0]\n    \
         statistics: {v: median}\n    computed: {readings: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n---\n",
    );
    let output = at(scratch.path(), &["set", "s.md", "a.readings=1,3,20"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("3 readings: 1, 3, 20, their median 3"),
        "{}",
        out(&output)
    );
}

#[test]
fn set_over_readings_keeps_them_and_says_the_way_back() {
    // A scalar over a quantity holding three readings
    // **keeps** them. The value written outranks the statistic they declare,
    // and the preview names the way back rather than an abandonment.
    let scratch = Scratch::new("set-over-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  \
         malt: {v: 3.0571, readings: [3.0571, 3.0557, 3.0584], u: 0.00078, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["set", "w.sample.md", "malt=4.8", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("keeps 3 readings"), "{shown}");
    assert!(shown.contains("3.0557"), "{shown}");
    // No statistic of them is recorded, so nothing computes the value back: the
    // way named is removing the value, never a `--force` that would refuse, and
    // no mean is offered in its place.
    assert!(
        shown.contains("malt= removes it") && !shown.contains("--force"),
        "the way back is named: {shown}"
    );
    assert!(!shown.contains("their mean"), "{shown}");
    let written = fs::read_to_string(&file).unwrap();
    assert!(
        written.contains("3.0557"),
        "the readings are the evidence and stay: {written}"
    );
    assert!(
        written.contains("4.8"),
        "the value written is the value: {written}"
    );
    // Written through its channel, the ways out still name the quantity:
    // `malt.v=` offered `malt.v.u=`, which is no field.
    let channel = at(scratch.path(), &["set", "w.sample.md", "malt.v=4.9"]);
    assert_eq!(code(&channel), 0, "{}", err(&channel));
    assert!(out(&channel).contains("malt.u=0.01"), "{}", out(&channel));
    assert!(!out(&channel).contains("malt.v."), "{}", out(&channel));
}

#[test]
fn set_on_a_cell_keeps_the_channel_not_written() {
    // A cell rebuilt from the value alone lost its uncertainty silently.
    let scratch = Scratch::new("set-cell-channels");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\ntables:\n  measurements:\n    index: T\n    \
         columns:\n      T: {}\n      f: {unit: brix}\n    rows:\n      - T: 20.0\n        \
         f: {v: 4.5437, u: 0.0003}\n---\n",
    );
    let set = |assignment: &str| {
        let output = at(
            scratch.path(),
            &["set", "w.sample.md", assignment, "--write"],
        );
        assert_eq!(code(&output), 0, "{}", err(&output));
        fs::read_to_string(&file).unwrap()
    };
    let written = set("measurements.f[20]=4.6");
    assert!(written.contains("v: 4.6, u: 0.0003"), "{written}");
    let written = set("measurements.f[20].u=0.001");
    assert!(written.contains("v: 4.6, u: 0.001"), "{written}");
    let written = set("measurements.f[20].u=");
    assert!(!written.contains("u: 0.001"), "{written}");
    assert!(written.contains("v: 4.6"), "{written}");
}

#[test]
fn set_over_readings_with_a_recorded_statistic_names_force() {
    // Where the file records the statistic the readings declare, `compute
    // --force` gives the value back to it, and the preview says so.
    let scratch = Scratch::new("set-over-recorded-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  \
         malt:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: a91f42c8f0d1}\n    fingerprint: d312b70e4a55\n---\n",
    );
    let output = at(scratch.path(), &["set", "w.sample.md", "malt=13"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("compute --force returns it"), "{shown}");
    assert!(!shown.contains("writes their mean"), "{shown}");
}

#[test]
fn a_summary_leaves_out_a_text_column_and_says_so() {
    // An empty cell and an inapplicable one are not the same answer.
    let scratch = Scratch::new("summary-text");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "status,malt", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains("status"), "{}", out(&output));
    assert!(out(&output).contains("malt [g]"), "{}", out(&output));
    assert!(
        err(&output).contains("status left out of the summary"),
        "{}",
        err(&output)
    );
    let csv = at(
        scratch.path(),
        &[".", "-c", "status,malt", "--summary", "--csv"],
    );
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(!out(&csv).contains("status,"), "{}", out(&csv));
    assert!(
        out(&csv).lines().any(|line| line.starts_with("malt,")),
        "{}",
        out(&csv)
    );
}

#[test]
fn an_unreadable_configuration_of_the_target_is_set_aside() {
    // A configuration that will not read is set aside like an unreadable file,
    // and the samples it describes are read without it — the target's own, as a
    // nested one already was.
    let scratch = Scratch::new("own-rc-unread");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection\nrecursive = true\n",
    );
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  malt: {v: 1.0, unit: g}\n---\nN.\n",
    );
    // What needs the configuration says it was found and not read, rather than
    // sending the reader to look for a file that is there.
    let needing = at(scratch.path(), &[".", "--profile", "overview"]);
    assert_eq!(code(&needing), 1, "{}", err(&needing));
    assert!(err(&needing).contains("was not read"), "{}", err(&needing));
    assert!(
        !err(&needing).contains("none was found"),
        "{}",
        err(&needing)
    );
    for arguments in [
        vec![".", "-c", "malt"],
        vec!["a.sample.md", "-c", "malt"],
        vec!["list", "skipped", "."],
    ] {
        let output = at(scratch.path(), &arguments);
        // Read without their configuration: a partial answer.
        assert_eq!(code(&output), 2, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains(".samplekitrc") && err(&output).contains("unclosed table"),
            "{arguments:?}: {}",
            err(&output)
        );
        assert!(
            !err(&output).contains("malformed frontmatter"),
            "{}",
            err(&output)
        );
    }
    let table = at(scratch.path(), &[".", "-c", "malt"]);
    assert!(out(&table).contains('1'), "{}", out(&table));
    let validated = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&validated), 2, "{}", err(&validated));
    assert!(
        out(&validated).contains(".samplekitrc"),
        "{}",
        out(&validated)
    );
}

#[test]
fn a_profile_composes_with_columns_and_sort() {
    // A profile declares, and each option written beside it overrides the part
    // it names — `-c` the columns, keeping the profile's order; `-s` the order.
    let scratch = Scratch::new("profile-composes");
    scratch.corpus();
    let declared = at(scratch.path(), &[".", "--profile", "platos"]);
    assert_eq!(code(&declared), 0, "{}", err(&declared));
    let columns = at(
        scratch.path(),
        &[".", "--profile", "platos", "-c", "name,malt"],
    );
    assert_eq!(code(&columns), 0, "{}", err(&columns));
    // The corpus renders boxed, so the header is the line naming a column.
    let header = out(&columns)
        .lines()
        .find(|line| line.contains("malt"))
        .unwrap_or_default()
        .to_string();
    assert!(header.contains("malt"), "{}", out(&columns));
    assert!(!out(&columns).contains("brix"), "{}", out(&columns));
    // The profile's order survives `-c`: `-brix`, the unmeasured one last. The
    // declared table names no sample, so its order is read off the malts.
    let shown = out(&declared);
    let malts: Vec<&str> = shown
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with("12.") || word.starts_with("9."))
        })
        .collect();
    assert_eq!(
        malts,
        ["12.30", "12.20", "12.10", "9.70"],
        "{}",
        out(&declared)
    );
    let by_profile = ["keg-03", "keg-02", "keg-01", "keg-04"];
    assert_eq!(order(&out(&columns)), by_profile, "{}", out(&columns));
    let sorted = at(
        scratch.path(),
        &[".", "--profile", "platos", "-c", "name,malt", "-s", "malt"],
    );
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    assert_eq!(
        order(&out(&sorted)),
        ["keg-04", "keg-01", "keg-02", "keg-03"],
        "{}",
        out(&sorted)
    );
}

#[test]
fn an_export_applies_the_query_it_declares() {
    // An export may carry its own selection.
    let scratch = Scratch::new("export-query");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!(
            "{rc}\n[query.light]\nfilter = \"malt < 12.25\"\n\n[export.light]\n\
             profile = \"platos\"\nformat = \"csv\"\noutput = \"out/light.csv\"\nquery = \"light\"\n"
        ),
    );
    let output = at(scratch.path(), &["export", "light", ".", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let rows = out(&output).lines().count() - 1;
    assert_eq!(rows, 3, "{}", out(&output));
    assert!(!err(&output).contains("narrowed"), "{}", err(&output));
    // A filter on the command line narrows further, and is said.
    let narrowed = at(
        scratch.path(),
        &["export", "light", ".", "-o", "-", "-f", "malt > 12.05"],
    );
    assert_eq!(code(&narrowed), 0, "{}", err(&narrowed));
    assert_eq!(out(&narrowed).lines().count() - 1, 2, "{}", out(&narrowed));
    assert!(err(&narrowed).contains("narrowed"), "{}", err(&narrowed));
}

#[test]
fn init_says_a_configuration_it_keeps_is_not_read() {
    // *Already here, kept* over a `.samplekitrc` no command can load is a
    // project reported as set up while every command sets its configuration
    // aside.
    let scratch = Scratch::new("init-unread-rc");
    scratch.write(
        ".samplekitrc",
        "python = \"scripts.brew_model:BrewModel\"\n",
    );
    let output = at(scratch.path(), &["init", "--example", "--no-venv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("already here, kept"),
        "{}",
        out(&output)
    );
    assert!(
        out(&output).contains("not read as it stands"),
        "{}",
        out(&output)
    );
    assert!(out(&output).contains("python"), "{}", out(&output));
    // Everything else written, "already set up" says it all the same.
    let written = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let again = at(scratch.path(), &["init", "--example", "--no-venv"]);
    let said = out(&again);
    assert!(said.contains("already set up"), "{said}");
    assert!(said.contains("does not load as it stands"), "{said}");
    assert!(!said.contains("D-232"), "{said}");
}

#[test]
fn an_export_of_a_selection_that_kept_nothing_is_not_an_unknown_field() {
    // A filter matching nothing is an answer, exit 0, as it is for a table. The
    // columns were checked against the emptied selection, which holds no field,
    // so every one of them was *unknown field … 0 fields available*.
    let scratch = Scratch::new("export-nothing");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!(
            "{rc}\n[export.all]\nprofile = \"platos\"\nformat = \"csv\"\noutput = \"out/all.csv\"\n"
        ),
    );
    let output = at(
        scratch.path(),
        &["export", "all", ".", "-o", "-", "-f", "malt > 1000000"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("unknown field"), "{}", err(&output));
    assert!(err(&output).contains("0 of"), "{}", err(&output));
    assert_eq!(out(&output).lines().count(), 1, "{}", out(&output));
    // A field nobody holds is still refused, against what was selected from.
    let misspelt = at(
        scratch.path(),
        &["export", "all", ".", "-o", "-", "-f", "maltt > 1000000"],
    );
    assert_eq!(code(&misspelt), 1, "{}", err(&misspelt));
}

#[test]
fn a_query_base_is_refused_naming_the_key_that_replaced_it() {
    // The key names a directory, not a parent query. A configuration still
    // writing `base` is set aside naming the rename — and nothing rewrites it,
    // because no command converts a configuration.
    let scratch = Scratch::new("query-base");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    let written = format!(
        "{rc}\n# where it applies\n[query.old]\nfilter = \"malt > 1\"\nbase   = \"transport/\"\n"
    );
    scratch.write(".samplekitrc", &written);
    let refused = at(scratch.path(), &[".", "-c", "malt"]);
    // The samples are read without it, and the answer is partial.
    assert_eq!(code(&refused), 2, "{}", err(&refused));
    assert!(
        err(&refused).contains("[query.old] 'base' is now 'directory'"),
        "{}",
        err(&refused)
    );
    assert!(
        err(&refused).contains("no command rewrites a configuration"),
        "{}",
        err(&refused)
    );
    assert!(
        !err(&refused).contains("samplekit init"),
        "{}",
        err(&refused)
    );

    // `init --write` sets the project up and leaves the file as it is.
    let set_up = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&set_up), 0, "{}", err(&set_up));
    assert_eq!(
        fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap(),
        written
    );

    // Renamed by hand, the query is read.
    scratch.write(
        ".samplekitrc",
        &written.replace("base   =", "directory   ="),
    );
    let accepted = at(scratch.path(), &[".", "-c", "malt", "--query", "old"]);
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    assert!(!err(&accepted).contains("base"), "{}", err(&accepted));
}
#[test]
fn set_of_an_unknown_name_writes_an_attribute_and_warns() {
    let scratch = Scratch::new("set-unknown");
    derived(&scratch);
    let output = at(scratch.path(), &["set", "s.sample.md", "humidity=45"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(err(&output).contains("attribute"), "{}", err(&output));
}

#[test]
fn set_keeps_readings_as_readings() {
    let scratch = Scratch::new("set-readings");
    let file = derived(&scratch);
    let output = at(
        scratch.path(),
        &[
            "set",
            "s.sample.md",
            "malt.readings=15.34,15.36,15.31",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(&file).unwrap();
    assert!(written.contains("readings"), "{written}");
    assert!(
        written.contains("15.34") && written.contains("15.31"),
        "{written}"
    );
}

#[test]
fn readings_say_what_they_replace_and_a_repeat_is_refused() {
    let scratch = Scratch::new("set-readings-preview");
    derived(&scratch);
    let first = at(
        scratch.path(),
        &[
            "set",
            "s.sample.md",
            "malt.readings=15.34,15.36,15.31",
            "--write",
        ],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    // Readings replace readings, not a value their mean only stood for.
    let again = at(
        scratch.path(),
        &["set", "s.sample.md", "malt.readings=[15.3, 15.4]"],
    );
    let said = out(&again);
    assert!(said.contains("· 1 change\n"), "{said}");
    assert!(
        said.contains("they replace the 3 readings it held"),
        "{said}"
    );
    // The value written before the first readings still stands.
    assert!(said.contains("the value 12.5 written stays"), "{said}");
    let twice = at(
        scratch.path(),
        &[
            "set",
            "s.sample.md",
            "malt.readings=1,2",
            "malt.readings=3,4",
        ],
    );
    assert_ne!(code(&twice), 0, "{}", err(&twice));
    assert!(err(&twice).contains("given twice"), "{}", err(&twice));
}

#[test]
fn set_adds_a_row_and_names_the_computed_columns() {
    let scratch = Scratch::new("set-add-row");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "t.sample.md",
        "---\nschema_version: 1\nname: T\ntables:\n  mashing:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n      derived: {}\n    rows:\n      \
         - step: 1\n        reading: 2.0\n        derived:\n          v: 4.0\n          \
         computed: {row.reading: aaaaaaaaaaaa}\n          fingerprint: bbbbbbbbbbbb\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &[
            "set",
            "t.sample.md",
            "--add-row",
            "mashing",
            "step=2",
            "reading=3.0",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("would add a row"), "{shown}");
    assert!(shown.contains("reading = 3"), "{shown}");
    assert!(shown.contains("derived"), "{shown}");
}

#[test]
fn set_refuses_a_column_a_table_does_not_have() {
    let scratch = Scratch::new("set-no-column");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "t.sample.md",
        "---\nschema_version: 1\nname: T\ntables:\n  mashing:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n    rows:\n      - {step: 1, reading: 2.0}\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &["set", "t.sample.md", "mashing.nocol[1]=3"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    let said = err(&output);
    assert!(said.contains("nocol"), "{said}");
    assert!(said.contains("reading"), "{said}");
}

#[test]
fn set_wants_something_to_change() {
    let scratch = Scratch::new("set-nothing");
    derived(&scratch);
    let output = at(scratch.path(), &["set", "s.sample.md"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("wants what to change"),
        "{}",
        err(&output)
    );
}

#[test]
fn set_reports_a_value_that_does_not_change_as_unchanged() {
    // `altbier → altbier` counted as a change is a lie told exactly where
    // someone is deciding whether to write.
    let scratch = Scratch::new("set-unchanged");
    let file = derived(&scratch);
    let before = fs::read_to_string(&file).unwrap();
    let output = at(
        scratch.path(),
        &["set", "s.sample.md", "malt=12.5", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("unchanged"), "{shown}");
    assert!(shown.contains("nothing to change"), "{shown}");
    assert!(!shown.contains('→'), "{shown}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        before,
        "the file was written although nothing changed"
    );
}

#[test]
fn new_into_a_missing_directory_names_it_once() {
    let scratch = Scratch::new("new-into-missing");
    assert_eq!(code(&set_up(&scratch)), 0);
    let output = at(
        scratch.path(),
        &["new", "C-04", "--into", "nowhere", "--write"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    let said = err(&output);
    assert!(said.contains("no such directory"), "{said}");
    assert_eq!(said.matches("nowhere").count(), 1, "named twice: {said}");
}

#[test]
fn an_unknown_table_is_worded_the_same_everywhere() {
    // One situation — this sample has no such table — reached by two routes.
    let scratch = Scratch::new("unknown-table");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "t.sample.md",
        "---\nschema_version: 1\nname: T\ntables:\n  mashing:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n    rows:\n      - {step: 1, reading: 2.0}\n---\nN.\n",
    );
    let cell = at(
        scratch.path(),
        &["set", "t.sample.md", "nosuch.reading[1]=3"],
    );
    let row = at(
        scratch.path(),
        &["set", "t.sample.md", "--add-row", "nosuch", "step=2"],
    );
    assert_ne!(code(&cell), 0, "{}", out(&cell));
    assert_ne!(code(&row), 0, "{}", out(&row));
    assert!(
        err(&cell).contains("unknown table 'nosuch'"),
        "{}",
        err(&cell)
    );
    assert!(
        err(&row).contains("unknown table 'nosuch'"),
        "{}",
        err(&row)
    );
}

#[test]
fn readings_are_not_set_as_a_scalar() {
    // Assigning one number to a list would drop the other readings, or invent
    // a list of one — which is how `± 0.000` reached three real samples.
    let scratch = Scratch::new("set-readings-scalar");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "s.sample.md",
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         malt: {readings: [4.73, 4.72, 4.74], unit: g}\n---\nN.\n",
    );
    let before = fs::read_to_string(&file).unwrap();
    let output = at(
        scratch.path(),
        &["set", "s.sample.md", "malt.readings=5", "--write"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    // One number is a value, and is named so.
    assert!(err(&output).contains("malt=5"), "{}", err(&output));
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
}

#[test]
fn status_exit_code_fails_when_anything_is_not_current() {
    // The gate continuous integration asks for, the way `git diff --exit-code`
    // offers one. Without it the listing stays an answer.
    let scratch = Scratch::new("status-gate");
    scratch.config();
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: 000000000000}\n---\nN.\n",
    );
    let plain = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    let gated = at(scratch.path(), &["status", ".", "--exit-code"]);
    assert_eq!(code(&gated), 2, "{}", err(&gated));
    assert!(err(&gated).contains("not current"), "{}", err(&gated));

    // Nothing to report exits 0, gate or no gate: the check passed.
    let clean = Scratch::new("status-gate-clean");
    clean.config();
    clean.sample("b.sample.md", "keg-02", "approved", 12.5, None);
    let output = at(clean.path(), &["status", ".", "--exit-code"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
}

#[test]
fn records_forming_a_cycle_are_never_read_as_current() {
    // One hand-edited record used to empty the whole sample's verdict, and
    // `status` answered *every value is current* over a malt just changed.
    let one = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::Integer(1)),
        ..Default::default()
    });
    let scratch = Scratch::new("status-cycle");
    scratch.config();
    scratch.write(
        "looped.sample.md",
        &format!(
            "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
             malt: {{v: 12.5}}\n  \
             a: {{v: 1, computed: {{b: {one}}}}}\n  \
             b: {{v: 1, computed: {{a: {one}}}}}\n  \
             plato: {{v: 2.8, computed: {{malt: 000000000000}}}}\n---\nN.\n"
        ),
    );
    let status = at(scratch.path(), &["status", "."]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    let said = out(&status);
    // The cycle is named with its path, and the unrelated stale value is still
    // judged: the rest of the sample keeps its answer.
    assert!(said.contains("not judged"), "{said}");
    assert!(said.contains("a \u{2192} b \u{2192} a"), "{said}");
    assert!(said.contains("outdated \u{2014} malt"), "{said}");
    assert!(!said.contains("is current"), "{said}");

    let gated = at(scratch.path(), &["status", ".", "--exit-code"]);
    assert_eq!(code(&gated), 2, "{}", err(&gated));

    // `validate` holds the cycle as a defect, and the stale value as a note.
    let validated = at(scratch.path(), &["validate", "."]);
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    assert!(
        out(&validated).contains("form a cycle"),
        "{}",
        out(&validated)
    );
    assert!(
        out(&validated).contains("1 value is not current"),
        "{}",
        out(&validated)
    );

    // JSON writes the state beside the value rather than leaving it out.
    let json = at(scratch.path(), &[".", "-c", "a,plato", "--json"]);
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(
        out(&json).contains("\"state\": \"unjudged\""),
        "{}",
        out(&json)
    );
    assert!(
        out(&json).contains("\"state\": \"outdated\""),
        "{}",
        out(&json)
    );
}

#[test]
fn compute_leaves_out_a_sample_it_cannot_order_and_exits_two() {
    // *nothing to compute*, exit 0, over a sample the command could not look at
    // is the answer a script takes for success.
    let one = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::Integer(1)),
        ..Default::default()
    });
    let scratch = Scratch::new("compute-cycle");
    // A model the listing cannot read: what the files record is listed, the
    // cycle among it, with no Python needed.
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"missing.py\"\n",
    );
    scratch.write(
        "looped.sample.md",
        &format!(
            "---\nschema_version: 1\nname: looped\nproperties:\n  \
             a: {{v: 1, computed: {{b: {one}}}}}\n  \
             b: {{v: 1, computed: {{a: {one}}}}}\n---\nN.\n"
        ),
    );
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: drifted\nproperties:\n  \
         malt: {v: 12.5}\n  plato: {v: 2.8, computed: {malt: 000000000000}}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["compute", "."]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    // What can be planned is, and is read first.
    assert!(out(&output).contains("plato"), "{}", out(&output));
    assert!(!out(&output).contains("not judged"), "{}", out(&output));
    assert!(
        err(&output).contains("1 sample was left out"),
        "{}",
        err(&output)
    );
    assert!(
        err(&output).contains("a \u{2192} b \u{2192} a"),
        "{}",
        err(&output)
    );
    assert!(
        err(&output).contains("samplekit validate"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_name_the_project_declares_is_written_as_a_quantity() {
    // `[property.malt]` says this name is a measured quantity and how it is
    // written. Writing an attribute there ignores the answer the project
    // already gives — and no model is run to learn it.
    let scratch = Scratch::new("declared-quantity");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[property.malt]\nunit = \"g\"\nsymbol = \"m\"\nprecision = \".2f\"\n",
    );
    let output = at(scratch.path(), &["new", "C-01", "malt=12.4", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("declared in .samplekitrc"),
        "{}",
        out(&output)
    );
    assert!(err(&output).is_empty(), "warned anyway: {}", err(&output));
    let written = fs::read_to_string(scratch.path().join("C-01.md")).unwrap();
    assert!(written.contains("properties:"), "{written}");
    assert!(written.contains("unit: g"), "{written}");
    // The unit is data, the symbol the project's, and not written.
    assert!(!written.contains("symbol"), "{written}");
    // And it is a quantity afterwards: a channel only a quantity has.
    let channel = at(
        scratch.path(),
        &["set", "C-01.md", "malt.u=0.05", "--write"],
    );
    assert_eq!(code(&channel), 0, "{}", err(&channel));
}

#[test]
fn a_name_nobody_declares_is_still_an_attribute() {
    // Nothing else can be guessed about a name that appears for the first time
    // and that no configuration describes.
    let scratch = Scratch::new("undeclared-attribute");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let output = at(scratch.path(), &["new", "C-02", "malt=9.9", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("written as an attribute"),
        "{}",
        err(&output)
    );
    let written = fs::read_to_string(scratch.path().join("C-02.md")).unwrap();
    assert!(!written.contains("properties:"), "{written}");
    assert!(written.contains("malt: 9.9"), "{written}");
}

#[test]
fn init_leaves_a_collection_in_an_older_format_alone() {
    // `init` sets a project up and converts nothing. A collection an older
    // SampleKit wrote is named by every command, with its reason, and stays
    // exactly as it is.
    let scratch = Scratch::new("init-older-format");
    // `precision_unc` inside a property is what version 1 refuses, and so what
    // says the file predates it.
    scratch.write(
        "samples/old.md",
        "---\nname: OLD\nproperties:\n  malt: {v: 1.0, precision_unc: \".2f\"}\n---\nOld.\n",
    );
    let before = fs::read_to_string(scratch.path().join("samples/old.md")).unwrap();
    let preview = at(scratch.path(), &["init", "--example", "--no-venv"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(!out(&preview).contains("older format"), "{}", out(&preview));

    let written = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(
        fs::read_to_string(scratch.path().join("samples/old.md")).unwrap(),
        before,
        "init wrote to a sample it was not asked to touch"
    );
    // And the file is still named, with what to do instead.
    let listed = at(scratch.path(), &["list", "."]);
    assert!(
        err(&listed).contains("no command converts it"),
        "{}",
        err(&listed)
    );
}

#[test]
fn an_earlier_configuration_is_named_and_not_converted() {
    // A configuration is no different from a sample. Every command says what it
    // is — `unknown field views` sends nobody anywhere — and nothing rewrites
    // it.
    let scratch = Scratch::new("earlier-rc");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[views.physical]\nkind = \"properties\"\n\
         include = [\"malt\"]\nlabels = [\"Weight\"]\nprecision = [\".2f\"]\nprecision_unc = [\".1e\"]\n",
    );
    scratch.sample("a.sample.md", "keg-01", "approved", 12.5, None);
    let before = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    let said = at(scratch.path(), &[".", "-c", "malt"]);
    assert!(err(&said).contains("earlier samplekit"), "{}", err(&said));
    assert!(
        err(&said).contains("no command converts it"),
        "{}",
        err(&said)
    );
    assert!(!err(&said).contains("samplekit init"), "{}", err(&said));

    let written = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(
        fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap(),
        before
    );
    assert!(!scratch.path().join(".samplekitrc.pre-1").exists());
}

#[test]
fn init_leaves_a_merely_uncanonical_file_alone() {
    // A file that differs from the canonical spelling is valid. Rewriting it
    // would have `init` reformat a collection nobody asked it to touch — it
    // caught its own EXAMPLE.md doing that before the step was narrowed.
    let scratch = Scratch::new("init-canonical");
    assert_eq!(code(&set_up(&scratch)), 0);
    let example = scratch.path().join("samples/EXAMPLE.md");
    let before = fs::read_to_string(&example).unwrap();
    let again = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(!out(&again).contains("older format"), "{}", out(&again));
    assert_eq!(fs::read_to_string(&example).unwrap(), before);
}

#[test]
fn init_says_where_completion_goes() {
    // Installing the bridge is a step of `init`, which names it for the shell
    // in use — and never writes it, the file being outside the project.
    let scratch = Scratch::new("init-completion");
    let output = at(scratch.path(), &["init", "--example", "--no-venv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("samplekit completions"),
        "{}",
        out(&output)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn init_at_a_terminal_asks_and_writes_on_a_yes() {
    // An empty project, no model, no environment, then yes.
    let scratch = Scratch::new("init-asked");
    let output = at_terminal(scratch.path(), &["init"], "1\n3\n2\ny\n");
    let said = out(&output);
    assert_eq!(code(&output), 0, "{said}");
    // Each question as project-setup words it.
    assert!(
        said.contains("What should the new project start from?"),
        "{said}"
    );
    assert!(
        said.contains("Which model should the project use? A model is a Python file"),
        "{said}"
    );
    assert!(said.contains("no model for now"), "{said}");
    assert!(
        said.contains("Should SampleKit prepare the Python environment the model runs in?"),
        "{said}"
    );
    assert!(said.contains("an empty project (default)"), "{said}");
    assert!(scratch.path().join(".samplekitrc").is_file(), "{said}");
    assert!(scratch.path().join("samples").is_dir());
    assert!(!scratch.path().join("model").exists());
    assert!(!scratch.path().join(".venv").exists());
    // Without a model the configuration names none, and says how to add one.
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(!rc.contains("\n[model]"), "{rc}");
    assert!(rc.contains("#   [model]"), "{rc}");
    assert!(!said.contains("$EDITOR"), "{said}");
    // A no writes nothing.
    let declined = Scratch::new("init-declined");
    let output = at_terminal(declined.path(), &["init"], "\n\n2\nn\n");
    assert!(out(&output).contains("nothing written"), "{}", out(&output));
    assert!(!declined.path().join(".samplekitrc").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn init_at_a_terminal_names_a_model_already_there() {
    // The model's second answer asks its path, until one is there.
    let scratch = Scratch::new("init-own-model");
    scratch.write(
        "brewery/beam.py",
        "import samplekit as sk\n\nclass Beam(sk.Sample):\n    pass\n",
    );
    let output = at_terminal(
        scratch.path(),
        &["init"],
        "1\n2\nbrewery/none.py\nbrewery/beam.py\n2\ny\n",
    );
    let said = out(&output);
    assert_eq!(code(&output), 0, "{said}");
    assert!(said.contains("the model file's path"), "{said}");
    assert!(said.contains("no file at brewery/none.py"), "{said}");
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(rc.contains("path = \"brewery/beam.py\""), "{rc}");
    assert!(!scratch.path().join("model").exists());
    // What comes next opens that file.
    assert!(said.contains("$EDITOR brewery/beam.py"), "{said}");
}

#[test]
fn init_installs_samplekit_in_the_environment_it_makes() {
    // The environment is made with samplekit in it, by pip alone. pip is kept
    // off the network here, so it finds nothing: its reason and the command to
    // run by hand are said, and every file is written all the same.
    let scratch = Scratch::new("init-venv-install");
    let project = scratch.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let written = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["init", "--write"])
        .current_dir(&project)
        .env("PIP_NO_INDEX", "1")
        .env("SAMPLEKIT_STATE_DIR", scratch.path().join("state"))
        .output()
        .unwrap();
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(project.join(".samplekitrc").is_file());
    assert!(project.join(".venv").is_dir(), "{}", err(&written));
    let said = format!("{}{}", out(&written), err(&written));
    assert!(said.contains("installing samplekit"), "{said}");
    assert!(
        said.contains("install it by hand")
            && said.contains(&format!(
                "pip install samplekit=={}",
                samplekit::config::project_setup::python_spelling(env!("CARGO_PKG_VERSION"))
            )),
        "{said}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_state_carries_its_colour_in_a_terminal() {
    // Yellow reads *out of date*. The mark in a table and the words `status`
    // writes take the same colour for the same state — and a pipe receives none
    // of it.
    let scratch = Scratch::new("state-colour");
    scratch.config();
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: keg-01\nproperties:\n  \
         malt: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {malt: 000000000000}\n---\nN.\n",
    );
    let shown = out(&in_colour(scratch.path(), &["status", "."]));
    assert!(shown.contains("\u{1b}[33m"), "{shown:?}");
    // The command it invites is bold.
    assert!(shown.contains("\u{1b}[1m"), "{shown:?}");
    // NO_COLOR turns all of it off, in the same terminal.
    let quiet = at_terminal(scratch.path(), &["status", "."], "");
    assert!(!out(&quiet).contains('\u{1b}'), "{:?}", out(&quiet));
    // And nothing reaches a pipe.
    let piped = at(scratch.path(), &["status", "."]);
    assert!(!out(&piped).contains('\u{1b}'), "{:?}", out(&piped));
}

#[test]
fn a_withdrawn_command_says_what_replaced_it() {
    // A guide or a script still calling `migrate` met *no such file or
    // directory*, which sent the reader looking for a file.
    let scratch = Scratch::new("withdrawn-verb");
    scratch.corpus();
    for (verb, said) in [("migrate", "was withdrawn"), ("stale", "samplekit status")] {
        let output = at(scratch.path(), &[verb]);
        assert_eq!(code(&output), 1, "{}", err(&output));
        assert!(err(&output).contains(said), "{verb}: {}", err(&output));
    }
}

#[test]
fn a_query_error_names_the_query_and_points_into_what_it_says() {
    // Over several configurations, a declared query's filter was parsed inside
    // a conjunction nobody wrote: the caret pointed into `(malt >)`, and
    // nothing named the query or its file.
    let scratch = Scratch::new("query-error");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[query.broken]\nfilter = \"malt >\"\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n[query.broken]\nfilter = \"malt >\"\n",
    );
    scratch.sample("nested/n.sample.md", "nested-one", "approved", 3.0, None);
    for place in [scratch.path().join("nested"), scratch.path().to_path_buf()] {
        let output = at(&place, &[".", "--query", "broken"]);
        assert_eq!(code(&output), 1, "{}", err(&output));
        assert!(
            err(&output).contains("query 'broken' in"),
            "{}",
            err(&output)
        );
        assert!(!err(&output).contains("(malt >)"), "{}", err(&output));
    }
}

#[test]
fn an_unknown_tag_points_at_the_tags_in_use() {
    // The only thing wrong was a tag, and the advice named the fields.
    let scratch = Scratch::new("unknown-tag");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-f", "tags has ghost"]);
    assert!(
        err(&output).contains("samplekit list tags"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("list fields"), "{}", err(&output));
}

#[test]
fn status_names_force_when_what_is_left_is_an_override() {
    // With only an override left, `status` sent the reader to a
    // `compute --write` that answered *nothing to compute*.
    let scratch = Scratch::new("status-override");
    scratch.config();
    scratch.write(
        "e.sample.md",
        "---\nschema_version: 1\nname: e\nproperties:\n  \
         malt: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    v: 3.0\n    \
         computed: {malt: aaaaaaaaaaaa}\n    fingerprint: {edited: bbbbbbbbbbbb}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &["status", "."]));
    assert!(text.contains("--force"), "{text}");
    assert!(!text.contains("lists what would run"), "{text}");
}

#[test]
fn a_reading_whose_uncertainty_a_formula_gives_is_measured_in_a_new_sample() {
    // A record whose channel is the uncertainty's says the value beside it was
    // entered. `new --like` left it out as if the model computed it, then said
    // it was both filled in and left out.
    let scratch = Scratch::new("new-like-measured");
    scratch.config();
    scratch.write(
        "BR-01.md",
        "---\nschema_version: 1\nname: BR-01\nproperties:\n  \
         mash_malt:\n    v: 18.42\n    u: 0.01842\n    unit: mg\n    \
         computed: {u: {mash_malt.v: 0123456789ab}}\n    fingerprint: 0123456789ab\n  \
         ibu: {v: 15.0, unit: mL}\n---\nN.\n",
    );
    let preview = at(scratch.path(), &["new", "BR-02", "--like", "BR-01.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    let waiting = said
        .lines()
        .find(|line| line.contains("for you to fill in"))
        .unwrap_or_default();
    assert!(waiting.contains("mash_malt"), "{said}");
    assert!(!said.contains("model computes them: mash_malt"), "{said}");

    // Filled in now and kept: neither waits, and nothing is said twice.
    let given = at(
        scratch.path(),
        &[
            "new",
            "BR-02",
            "--like",
            "BR-01.md",
            "mash_malt=21.07",
            "--keep",
            "ibu",
        ],
    );
    let said = out(&given);
    assert!(!said.contains("for you to fill in"), "{said}");
    assert!(!said.contains("left out"), "{said}");
}

#[test]
fn init_checks_before_it_writes_anything() {
    // A file where a directory must go stopped the run halfway, having written
    // some files and said nothing of them.
    let scratch = Scratch::new("init-preflight");
    scratch.write("model", "in the way\n");
    let output = at(
        scratch.path(),
        &["init", ".", "--example", "--write", "--no-venv"],
    );
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("nothing was written"),
        "{}",
        err(&output)
    );
    assert!(!scratch.path().join(".samplekitrc").exists());
}

/// A write over a selection it could not read in full says so in its code, as a
/// read does.
#[test]
#[cfg(unix)]
fn a_write_over_an_unread_file_exits_two() {
    let scratch = Scratch::new("write-unread");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let unread = scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties: [\n---\n",
    );
    let tagged = at(scratch.path(), &["tag", "add", "seen", ".", "--write"]);
    assert_eq!(code(&tagged), 2, "{}", err(&tagged));
    assert!(err(&tagged).contains("b.md"), "{}", err(&tagged));
    assert!(
        fs::read_to_string(scratch.path().join("a.md"))
            .unwrap()
            .contains("seen"),
        "the file it could read is written"
    );
    // Refused by the filesystem rather than malformed, it exits 3, as a read
    // does.
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  malt: {v: 2.0, unit: g}\n---\n",
    );
    let mode = |bits: u32| {
        fs::set_permissions(&unread, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };
    mode(0o000);
    let tagged = at(scratch.path(), &["tag", "add", "again", ".", "--write"]);
    mode(0o644);
    assert_eq!(code(&tagged), 3, "{}", err(&tagged));
}

/// A file writes a declared precision's text, as the screen does.
#[test]
fn a_file_writes_a_number_as_its_precision_writes_it() {
    let scratch = Scratch::new("written-precision");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.malt]\nprecision = \".2e\"\n\
         [property.period]\nprecision = \".0f\"\n[property.flatness]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 12.3, u: 0.00006}\n  \
         period: 6.0\n  flatness: {v: 0.05, u: 0.0002}\n  free: 6.0\n---\n",
    );
    let csv = out(&at(
        scratch.path(),
        &["-c", "malt,period,flatness,free", "--csv"],
    ));
    // The uncertainty columns no sample fills are left out.
    assert!(
        csv.ends_with("1.23e+01,6.00e-05,6,0.050,0.000,6.0\n"),
        "{csv}"
    );
    let json = out(&at(scratch.path(), &["-c", "malt,period", "--json"]));
    assert!(json.contains("\"value\": 1.23e+01"), "{json}");
    assert!(json.contains("\"value\": 6,"), "{json}");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed[0]["malt"]["value"], 12.3);
}

/// A sample's own files are listed, opened and navigated to from the command
/// line, through the system's opener.
#[test]
#[cfg(target_os = "linux")]
fn a_samples_files_are_listed_and_opened() {
    let scratch = Scratch::new("open-files");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n",
    );
    for name in ["B2", "B24", "B24-2"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"),
        );
    }
    for file in [
        "images/B24_photo_x500.png",
        "images/B24_photo_x2000.png",
        "images/B24-2_photo.png",
        "images/C9_orphan.png",
    ] {
        scratch.write(file, "");
    }
    let listed = out(&at(scratch.path(), &["list", "files", "."]));
    assert!(
        listed.contains("B24\n  images/B24_photo_x2000.png\n  images/B24_photo_x500.png\n"),
        "{listed}"
    );
    assert!(
        listed.contains("B24-2\n  images/B24-2_photo.png\n"),
        "{listed}"
    );
    assert!(!listed.contains("B2\n"), "{listed}");
    assert!(listed.contains("belonging to no sample"), "{listed}");
    assert!(listed.contains("images/C9_orphan.png"), "{listed}");

    // The system's opener, standing in: it says what it was given.
    let bin = scratch.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let opened = scratch.path().join("opened");
    let opener = bin.join("xdg-open");
    fs::write(
        &opener,
        format!("#!/bin/sh\necho \"$1\" >> '{}'\n", opened.display()),
    )
    .unwrap();
    fs::set_permissions(&opener, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let open = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(arguments)
            .current_dir(scratch.path())
            .env("PATH", &path)
            .env_remove("SSH_CONNECTION")
            .env_remove("SSH_TTY")
            .output()
            .unwrap()
    };
    let one = open(&["open", "B24.md", "*x500*"]);
    assert_eq!(code(&one), 0, "{}", err(&one));
    assert!(
        out(&one).contains("opened images/B24_photo_x500.png"),
        "{}",
        out(&one)
    );
    let folder = open(&["open", "B24.md", "--navigate"]);
    assert_eq!(code(&folder), 0, "{}", err(&folder));
    // Every file a pattern names opens.
    let several = open(&["open", "B24.md"]);
    assert_eq!(code(&several), 0, "{}", err(&several));
    assert!(
        out(&several).contains("opened images/B24_photo_x2000.png")
            && out(&several).contains("opened images/B24_photo_x500.png"),
        "{}",
        out(&several)
    );
    let none = open(&["open", "B2.md"]);
    assert_eq!(code(&none), 1, "{}", err(&none));
    // More than five open only after a yes, which no terminal here can give.
    for index in 0..4 {
        scratch.write(&format!("images/B24_extra{index}.png"), "");
    }
    let many = open(&["open", "B24.md"]);
    assert_eq!(code(&many), 1, "{}", err(&many));
    assert!(
        err(&many).contains("6 files of B24") && err(&many).contains("after a yes"),
        "{}",
        err(&many)
    );
    for _ in 0..50 {
        if fs::read_to_string(&opened).is_ok_and(|text| text.lines().count() == 4) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let said = fs::read_to_string(&opened).unwrap();
    assert!(said.contains("B24_photo_x500.png"), "{said}");
    assert!(said.lines().any(|line| line.ends_with("images")), "{said}");
}

#[test]
fn set_refuses_a_field_given_twice() {
    let scratch = Scratch::new("set-twice");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["set", "w.sample.md", "malt=2", "malt=3", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("malt is given twice"),
        "{}",
        err(&output)
    );
    assert!(fs::read_to_string(&file).unwrap().contains("v: 1.0"));
    // One field however it is spelled: malt and malt.v are the same value.
    let spelled = at(
        scratch.path(),
        &["set", "w.sample.md", "malt=2", "malt.v=3"],
    );
    assert_eq!(code(&spelled), 1, "{}", out(&spelled));
    assert!(err(&spelled).contains("given twice"), "{}", err(&spelled));
}

#[test]
fn a_value_never_computed_is_said_in_the_state_column() {
    // A derived value one sample holds and the other has never been given:
    // `current` hid it.
    let scratch = Scratch::new("status-never");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 1.0, unit: g, fingerprint: 111111111111}\n  \
         twice: {v: 2.0, computed: {malt: 111111111111}, fingerprint: 222222222222}\n---\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  malt: {v: 3.0, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-c", "name,twice", "--status", "--csv"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("never computed: twice"), "{text}");
    // The drawn table says it as the CSV does.
    let drawn = at(scratch.path(), &[".", "-c", "name,twice", "--status"]);
    assert!(
        out(&drawn).contains("never computed: twice"),
        "{}",
        out(&drawn)
    );
    // A value only the model's source declares, before anything computed it:
    // in every format, and as a quantity in JSON from the first.
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.brix = sk.Property(compute=lambda: 1.0)\n",
    );
    for format in [None, Some("--csv")] {
        let mut args = vec![".", "-c", "name,brix", "--status"];
        args.extend(format);
        let output = at(scratch.path(), &args);
        assert!(
            out(&output).contains("never computed: brix"),
            "{}",
            out(&output)
        );
    }
    let json = at(scratch.path(), &[".", "-c", "name,brix", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out(&json)).unwrap();
    assert!(
        parsed[0]["brix"]["value"].is_null() && parsed[0]["brix"]["state"] == "never computed",
        "{}",
        out(&json)
    );
}

#[test]
fn listing_one_samples_files_calls_no_others_ownerless() {
    let scratch = Scratch::new("files-one");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n",
    );
    scratch.write("A.md", "---\nschema_version: 1\nname: A\n---\n");
    scratch.write("B.md", "---\nschema_version: 1\nname: B\n---\n");
    scratch.write("images/A_sem.png", "");
    scratch.write("images/B_sem.png", "");
    scratch.write("images/Z_grid.png", "");
    let one = at(scratch.path(), &["list", "files", "A.md"]);
    assert_eq!(code(&one), 0, "{}", err(&one));
    let said = out(&one);
    assert!(
        said.contains("A_sem.png") && !said.contains("B_sem.png"),
        "{said}"
    );
    assert!(!said.contains("belonging to no sample"), "{said}");
    // Left out of a listing of part of the project, and said to be.
    assert!(
        said.contains("1 file belongs to no sample — listed over the whole project"),
        "{said}"
    );
    let all = out(&at(scratch.path(), &["list", "files", "."]));
    assert!(
        all.contains("belonging to no sample") && all.contains("Z_grid.png"),
        "{all}"
    );
    assert!(!all.contains("no sample —\n  images/B_sem"), "{all}");
}

#[test]
fn new_keeps_a_values_readings_and_refuses_a_name_held_elsewhere() {
    let scratch = Scratch::new("new-keep");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "A.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 12.0, readings: [11.9, 12.1], unit: g}\n---\n",
    );
    scratch.write("other.md", "---\nschema_version: 1\nname: B\n---\n");
    let kept = at(
        scratch.path(),
        &[
            "new", "C", "--like", "A.md", "--keep", "malt", "--into", ".", "--write",
        ],
    );
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let written = fs::read_to_string(scratch.path().join("C.md")).unwrap();
    assert!(written.contains("readings: [11.9, 12.1]"), "{written}");
    assert!(
        out(&kept).contains("kept from the pattern          malt"),
        "{}",
        out(&kept)
    );
    // A name the pattern lacks, or a value its model computes, is refused.
    scratch.write(
        "D.md",
        "---\nschema_version: 1\nname: D\nproperties:\n  malt: {v: 1.0, unit: g}\n  \
         twice: {v: 2.0, computed: {malt: 111111111111}, fingerprint: 222222222222}\n---\n",
    );
    for (keep, said) in [
        ("mal", "holds no value"),
        ("twice", "the model computes it"),
    ] {
        let refused = at(
            scratch.path(),
            &["new", "E", "--like", "D.md", "--keep", keep, "--into", "."],
        );
        assert_ne!(code(&refused), 0);
        assert!(err(&refused).contains(said), "{}", err(&refused));
    }
    let taken = at(scratch.path(), &["new", "B", "--into", ".", "--write"]);
    assert_ne!(code(&taken), 0);
    assert!(
        err(&taken).contains("already a sample's name"),
        "{}",
        err(&taken)
    );
    assert!(!scratch.path().join("B.md").exists());
}

#[test]
fn init_writes_the_empty_structure_and_its_first_steps_by_default() {
    let scratch = Scratch::new("init-empty");
    let output = at(scratch.path(), &["init", "--no-venv", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.path().join("model/main.py").is_file());
    assert!(scratch.path().join("samples").is_dir());
    assert!(!scratch.path().join("samples/EXAMPLE.md").exists());
    let said = out(&output);
    assert!(said.contains("samples/ "), "{said}");
    assert!(said.contains("2 files written"), "{said}");
    assert!(
        said.contains("model/main.py") && said.contains("new S-01"),
        "{said}"
    );
    // The project reads: its one sample, made next, is listed.
    let new = at(
        scratch.path(),
        &["new", "S-01", "--into", "samples", "--write"],
    );
    assert_eq!(code(&new), 0, "{}", err(&new));
    let listed = at(scratch.path(), &["samples/"]);
    assert!(out(&listed).contains("S-01.md"), "{}", out(&listed));
}

#[test]
fn view_shows_a_sample_whole_its_note_on_request() {
    // Values, then every table unfolded; the note with --note alone.
    let scratch = Scratch::new("view-whole");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "W.md",
        "---\nschema_version: 1\nname: W\nbeer: bock\nproperties:\n  malt: {v: 9.0, u: 0.1, unit: g}\n\
         tables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      load: {unit: N}\n    rows:\n      \
         - run: 1\n        load: 3.5\n      - run: 2\n        load: 4.25\n---\nHopped twice.\n",
    );
    let output = at(scratch.path(), &["view", "W.md"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    for shown in [
        "W\n",
        "beer",
        "bock",
        "9.00 ± 0.10 g",
        "runs · 2 rows",
        "load [N]",
        "4.25",
    ] {
        assert!(text.contains(shown), "{shown}: {text}");
    }
    assert!(!text.contains("Hopped twice."), "{text}");
    let noted = at(scratch.path(), &["view", "W.md", "--note"]);
    assert!(out(&noted).contains("Hopped twice."), "{}", out(&noted));
}

#[test]
fn a_unit_renamed_says_the_number_is_not_converted() {
    // mg to g over 4.1: the number stays, and what read it keeps what it gave.
    let scratch = Scratch::new("unit-renamed");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "u.sample.md",
        "---\nschema_version: 1\nname: U\nproperties:\n  kettle: {v: 4.1, unit: mg, fingerprint: 111111111111}\n  \
         fermentable: {v: 2.0, unit: mg, computed: {kettle: 111111111111}, fingerprint: 222222222222}\n---\n",
    );
    let output = at(scratch.path(), &["set", "u.sample.md", "kettle.unit=g"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(
        said.contains("renamed, not converted: 4.1 mg is now read as 4.1 g"),
        "{said}"
    );
    assert!(said.contains("fermentable read the number 4.1"), "{said}");
    assert!(!said.contains("nothing derived rests on this"), "{said}");
}

/// The messages of a project's history, newest first.
fn history_messages(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_write_is_a_snapshot_named_by_its_command() {
    // Each --write is a snapshot saying the command as typed, and what changed
    // before it, in an editor, is a snapshot of its own.
    let scratch = Scratch::new("history-cli");
    scratch.corpus();
    let preview = at(scratch.path(), &["set", "keg-01.sample.md", "malt=13"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(!scratch.path().join(".samplekit/history").exists());
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let edited = scratch.path().join("keg-02.sample.md");
    let text = fs::read_to_string(&edited).unwrap();
    fs::write(&edited, text.replace("12.2", "12.25")).unwrap();
    let tag = at(
        scratch.path(),
        &[
            "tag",
            "add",
            "checked",
            ".",
            "-f",
            "name == 'keg-02'",
            "--write",
        ],
    );
    assert_eq!(code(&tag), 0, "{}", err(&tag));
    assert_eq!(
        history_messages(scratch.path()),
        [
            "samplekit tag add checked . -f 'name == '\\''keg-02'\\''' --write",
            "changed outside SampleKit",
            "samplekit set keg-01.sample.md malt=13 --write",
            "the project as SampleKit first kept it",
        ]
    );
    // A --write that changes nothing takes no snapshot.
    let again = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(history_messages(scratch.path()).len(), 4);
    // SAMPLEKIT_HISTORY=off takes none.
    let off = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["set", "keg-01.sample.md", "malt=14", "--write"])
        .current_dir(scratch.path())
        .env("SAMPLEKIT_HISTORY", "off")
        .output()
        .unwrap();
    assert_eq!(code(&off), 0, "{}", err(&off));
    assert_eq!(history_messages(scratch.path()).len(), 4);
}

/// A corpus with a history: the first snapshot, a `set`, a tag, a new sample.
fn with_history(scratch: &Scratch) {
    scratch.corpus();
    for arguments in [
        vec!["set", "keg-01.sample.md", "malt=13", "--write"],
        vec!["tag", "add", "checked", "keg-02.sample.md", "--write"],
        vec!["new", "keg-09", "malt=11", "--write"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
    }
}

#[test]
fn log_lists_the_snapshots_numbered_and_narrowed() {
    // Newest first from 1; a target keeps the numbers; `now` where the files
    // differ from the last snapshot.
    let scratch = Scratch::new("log");
    scratch.corpus();
    let none = at(scratch.path(), &["log"]);
    assert_eq!(code(&none), 0, "{}", err(&none));
    assert!(
        out(&none).contains("no history is kept here yet"),
        "{}",
        out(&none)
    );
    with_history(&scratch);
    let log = out(&at(scratch.path(), &["log"]));
    // Each row as its cells, whatever the project's table style draws.
    let rows: Vec<String> = log
        .lines()
        .map(|line| line.replace(['│', '┃'], " ").trim().to_string())
        .filter(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(rows.len(), 4, "{log}");
    assert!(
        rows[0].starts_with("1 ") && rows[0].contains("samplekit new keg-09"),
        "{log}"
    );
    assert!(
        rows[1].starts_with("2 ") && rows[1].contains("samplekit tag add checked"),
        "{log}"
    );
    assert!(
        rows[2].contains("samplekit set keg-01.sample.md malt=13"),
        "{log}"
    );
    assert!(
        rows[3].starts_with("4 ") && rows[3].contains("first kept it"),
        "{log}"
    );
    assert!(rows[3].contains("4 samples, .samplekitrc"), "{log}");
    // Narrowed to one sample, its snapshots keep their numbers.
    let one = out(&at(scratch.path(), &["log", "keg-01.sample.md"]));
    assert!(one.contains("3 ") && one.contains("malt=13"), "{one}");
    assert!(!one.contains("checked"), "{one}");
    // Changed in an editor: `now` says so.
    let file = scratch.path().join("keg-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("approved", "retested")).unwrap();
    let now = out(&at(scratch.path(), &["log"]));
    assert!(
        now.contains("changed outside SampleKit, not yet kept"),
        "{now}"
    );
    assert!(now.contains("keg-03"), "{now}");
}

#[test]
fn diff_compares_two_states_as_values() {
    let scratch = Scratch::new("diff");
    with_history(&scratch);
    let file = scratch.path().join("keg-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace("approved", "retested")
            .replace("notes for keg-03.", "notes for keg-03.\nA second line."),
    )
    .unwrap();
    // By default, the last change kept, and a change made outside SampleKit
    // since is said; `--from 1` shows it.
    let last = out(&at(scratch.path(), &["diff"]));
    assert!(last.contains("to    #1"), "{last}");
    assert!(last.contains("changed outside SampleKit since"), "{last}");
    let now = at(scratch.path(), &["diff", "--from", "1"]);
    assert_eq!(code(&now), 0, "{}", err(&now));
    let said = out(&now);
    assert!(said.contains("keg-03.sample.md"), "{said}");
    assert!(said.contains("status     approved  →  retested"), "{said}");
    assert!(said.contains("the note   changed, +1 −0 lines"), "{said}");
    // From the first snapshot: a value at the project's precision, a tag, a
    // new sample.
    let whole = out(&at(scratch.path(), &["diff", "--from", "4", "--to", "1"]));
    assert!(
        whole.contains("malt   12.10 ± 0.05 g  →  13.00 ± 0.05 g"),
        "{whole}"
    );
    assert!(
        whole.contains("tags   reference  →  reference, checked"),
        "{whole}"
    );
    assert!(whole.contains("keg-09.md   new sample"), "{whole}");
    // The configuration, said by its lines.
    let rc = scratch.path().join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    fs::write(
        &rc,
        text.replace("precision = \".2f\"", "precision = \".3f\""),
    )
    .unwrap();
    let configured = out(&at(scratch.path(), &["diff", "--from", "1"]));
    assert!(
        configured.contains(".samplekitrc   changed, +1 −1 lines"),
        "{configured}"
    );
    // A change SampleKit writes is what `diff` alone shows next; nothing is
    // left outside, and `--from 1` says nothing changed since.
    let write = at(
        scratch.path(),
        &["set", "keg-02.sample.md", "malt=12.3", "--write"],
    );
    assert_eq!(code(&write), 0, "{}", err(&write));
    let kept = out(&at(scratch.path(), &["diff"]));
    assert!(kept.contains("keg-02.sample.md"), "{kept}");
    assert!(!kept.contains("changed outside SampleKit since"), "{kept}");
    let nothing = out(&at(scratch.path(), &["diff", "--from", "1"]));
    assert!(
        nothing.contains("nothing changed since SampleKit last kept"),
        "{nothing}"
    );
    assert!(nothing.contains("samplekit diff alone"), "{nothing}");
    // One file named: the last change kept of that file.
    let one = out(&at(scratch.path(), &["diff", "keg-03.sample.md"]));
    assert!(one.contains("status     approved  →  retested"), "{one}");
    // And `log` of one file does not repeat its name on every line.
    let logged = out(&at(scratch.path(), &["log", "keg-03.sample.md"]));
    assert!(!logged.contains("keg-03"), "{logged}");
    // A bare date is the whole day: the snapshots taken today are found by
    // today's date.
    let today: String = logged
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .find(|word| word.len() == 10 && word.matches('-').count() == 2)
        .expect(&logged)
        .to_string();
    let dated = at(scratch.path(), &["diff", "--to", &today]);
    assert_eq!(code(&dated), 0, "{}", err(&dated));
    // `log --at` starts the list at the snapshot it names, and `--at` is
    // offered only where it is taken.
    let since = out(&at(scratch.path(), &["log", "--at", "2"]));
    assert!(!since.contains("│ 1 │"), "{since}");
    assert!(since.contains("│ 2 │"), "{since}");
    assert!(!out(&at(scratch.path(), &["compute", "--help"])).contains("--at"));
}

#[test]
fn diff_names_a_state_by_number_id_or_date() {
    let scratch = Scratch::new("diff-when");
    with_history(&scratch);
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(scratch.path().join(".samplekit/history"))
        .args(["rev-parse", "HEAD~2"])
        .output()
        .unwrap();
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let by_number = out(&at(scratch.path(), &["diff", "--from", "3", "--to", "1"]));
    let by_id = out(&at(
        scratch.path(),
        &["diff", "--from", &id[..7], "--to", "1"],
    ));
    assert_eq!(by_number, by_id);
    assert!(by_number.starts_with("from  #3 · "), "{by_number}");
    // Tomorrow names the last snapshot; a day long past names none.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let tomorrow = chrono::DateTime::from_timestamp(seconds + 2 * 86_400, 0)
        .unwrap()
        .date_naive()
        .to_string();
    let dated = out(&at(
        scratch.path(),
        &["diff", "--from", &tomorrow, "--to", "1"],
    ));
    assert!(dated.contains("nothing changed between them"), "{dated}");
    for refused in ["2001-01-01", "99", "zzzz", "abcdef0"] {
        let output = at(scratch.path(), &["diff", "--from", refused]);
        assert_eq!(code(&output), 1, "{refused}: {}", out(&output));
    }
}

#[test]
fn explain_finds_a_file_samplekit_wrote_and_what_changed_since() {
    // An export found again by its hash, wherever it was copied, with what
    // changed since it was made.
    let scratch = Scratch::new("explain-output");
    scratch.corpus();
    let export = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&export), 0, "{}", err(&export));
    let written = scratch.path().join("out/platos.csv");
    let fresh = at(scratch.path(), &["explain", "out/platos.csv"]);
    assert_eq!(code(&fresh), 0, "{}", err(&fresh));
    let said = out(&fresh);
    assert!(
        said.contains("by samplekit export platos . --write"),
        "{said}"
    );
    assert!(said.contains("written   out/platos.csv"), "{said}");
    assert!(said.contains("it is up to date"), "{said}");
    // Copied outside the project, it is looked up in the project named.
    let elsewhere = Scratch::new("explain-output-elsewhere");
    let copy = elsewhere.path().join("table-3.csv");
    fs::copy(&written, &copy).unwrap();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let stale = at(
        elsewhere.path(),
        &[
            "explain",
            "table-3.csv",
            &scratch.path().display().to_string(),
        ],
    );
    assert_eq!(code(&stale), 0, "{}", err(&stale));
    let said = out(&stale);
    assert!(said.contains("keg-01.sample.md"), "{said}");
    assert!(
        said.contains("malt   12.10 ± 0.05 g  →  13.00 ± 0.05 g"),
        "{said}"
    );
    assert!(
        said.contains("it is not current — samplekit export platos . --write makes it again"),
        "{said}"
    );
    // Without a project to look in, it asks for one; a file SampleKit did not
    // write is said so.
    let lost = at(elsewhere.path(), &["explain", "table-3.csv"]);
    assert_eq!(code(&lost), 1, "{}", out(&lost));
    assert!(err(&lost).contains("FOLDER"), "{}", err(&lost));
    fs::write(scratch.path().join("out/mine.csv"), "a,b\n").unwrap();
    let unknown = at(scratch.path(), &["explain", "out/mine.csv"]);
    assert_eq!(code(&unknown), 1, "{}", out(&unknown));
    assert!(
        err(&unknown).contains("is not a file SampleKit wrote"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn explain_at_shows_a_value_as_the_history_kept_it() {
    // The sample's file of that snapshot, explained under the project's
    // configuration; a sample the snapshot did not hold is refused.
    let scratch = Scratch::new("explain-at");
    with_history(&scratch);
    let now = out(&at(
        scratch.path(),
        &["explain", "keg-01.sample.md", "malt"],
    ));
    assert!(now.contains("malt = 13.00 ± 0.05 g"), "{now}");
    let then = at(
        scratch.path(),
        &["explain", "keg-01.sample.md", "malt", "--at", "4"],
    );
    assert_eq!(code(&then), 0, "{}", err(&then));
    let said = out(&then);
    assert!(said.starts_with("as kept at #4 · "), "{said}");
    assert!(said.contains("malt = 12.10 ± 0.05 g"), "{said}");
    let absent = at(
        scratch.path(),
        &["explain", "keg-09.md", "malt", "--at", "4"],
    );
    assert_eq!(code(&absent), 1, "{}", out(&absent));
    assert!(
        err(&absent).contains("was not in the project at #4"),
        "{}",
        err(&absent)
    );
}

#[test]
fn at_reads_the_project_as_a_snapshot_kept_it() {
    // A reading command over the project as it was; an export made again is the
    // file it was, byte for byte.
    let scratch = Scratch::new("at");
    scratch.corpus();
    let first = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let then = fs::read(scratch.path().join("out/platos.csv")).unwrap();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_ne!(
        fs::read(scratch.path().join("out/platos.csv")).unwrap(),
        then
    );
    // Made again as it was, where -o says.
    let remade = at(
        scratch.path(),
        &[
            "export",
            "platos",
            ".",
            "--at",
            "2",
            "-o",
            "out/then.csv",
            "--write",
        ],
    );
    assert_eq!(code(&remade), 0, "{}", err(&remade));
    assert!(
        err(&remade).starts_with("as kept at #2 · "),
        "{}",
        err(&remade)
    );
    assert_eq!(fs::read(scratch.path().join("out/then.csv")).unwrap(), then);
    // A table as it was, and today's untouched.
    let table = out(&at(scratch.path(), &[".", "-c", "name,malt", "--at", "2"]));
    assert!(table.contains("12.10"), "{table}");
    assert!(!table.contains("13.00"), "{table}");
    // explain says how the export remade is made again as it was.
    let said = out(&at(scratch.path(), &["explain", "out/then.csv"]));
    assert!(
        said.contains("made again as it was, from the project's folder: samplekit export platos . -o out/then.csv --write --at "),
        "{said}"
    );
    // Writing to the project, or an export to its declared file, is refused.
    let refused = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=1", "--at", "2"],
    );
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    let declared = at(
        scratch.path(),
        &["export", "platos", ".", "--at", "2", "--write"],
    );
    assert_eq!(code(&declared), 1, "{}", out(&declared));
    assert!(
        err(&declared).contains("where -o says"),
        "{}",
        err(&declared)
    );
}

#[test]
fn project_is_a_field_over_several_projects() {
    // The folder of a sample's project, by name, a field like any.
    let scratch = Scratch::new("project-field");
    for (project, malt) in [("ana", 1.0), ("tom", 2.0)] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        scratch.write(
            &format!("{project}/s-{project}.md"),
            &format!(
                "---\nschema_version: 1\nname: s-{project}\nproperties:\n  malt: {malt}\n---\n"
            ),
        );
    }
    let filtered = out(&at(
        scratch.path(),
        &["ana", "tom", "-f", "project == tom", "-c", "name"],
    ));
    assert!(
        filtered.contains("s-tom") && !filtered.contains("s-ana"),
        "{filtered}"
    );
    let csv = out(&at(
        scratch.path(),
        &[
            "ana",
            "tom",
            "-c",
            "name,project",
            "-s",
            "-project",
            "--csv",
        ],
    ));
    assert_eq!(csv, "name,project\ns-tom,tom\ns-ana,ana\n");
    // One table per project, each with the column asked for once.
    let table = out(&at(scratch.path(), &["ana", "tom", "-c", "name,project"]));
    for part in table.split("\n\n") {
        assert_eq!(part.matches("project").count(), 1, "{table}");
    }
}

#[test]
fn a_summary_is_grouped_by_a_field() {
    // One summary per value of a field.
    let scratch = Scratch::new("summary-group");
    scratch.corpus();
    let table = at(
        scratch.path(),
        &[".", "-c", "malt", "--summary", "--group", "status"],
    );
    assert_eq!(code(&table), 0, "{}", err(&table));
    let said = out(&table);
    assert!(said.contains("approved"), "{said}");
    assert!(said.contains("rejected"), "{said}");
    let csv = out(&at(
        scratch.path(),
        &[".", "-c", "malt", "--summary", "--group", "status", "--csv"],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "status,column,unit,n,of,mean,weights,s,sem,median,min,max"
    );
    assert!(lines[1].starts_with("approved,malt,g,3,3,"), "{csv}");
    assert!(lines[2].starts_with("rejected,malt,g,1,1,"), "{csv}");
}

#[test]
fn status_and_validate_speak_json() {
    // For scripts, one object per value, one per finding.
    let scratch = Scratch::new("json-status");
    scratch.corpus();
    let status = at(scratch.path(), &["status", ".", "--json"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    let listed: serde_json::Value = serde_json::from_str(&out(&status)).unwrap();
    assert!(listed.is_array(), "{listed}");
    fs::write(
        scratch.path().join("broken.sample.md"),
        "---\nschema_version: 1\nproperties: [1\n---\n",
    )
    .unwrap();
    let validate = at(scratch.path(), &["validate", ".", "--json"]);
    let findings: serde_json::Value = serde_json::from_str(&out(&validate)).unwrap();
    let findings = findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding["severity"] == "defect" || finding["severity"] == "note"),
        "{findings:?}"
    );
}

#[test]
fn a_cell_no_row_holds_is_absent_in_json() {
    let scratch = Scratch::new("json-absent-cell");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, rows) in [
        ("a", "[{T: 1, R: 2.0}, {T: 2, R: 3.0}]"),
        ("b", "[{T: 1, R: 5.0}]"),
    ] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\ntables:\n  t:\n    index: T\n    \
                 columns:\n      T: {{}}\n      R: {{}}\n    rows: {rows}\n---\n"
            ),
        );
    }
    let output = at(
        scratch.path(),
        &[".", "-c", "name,t.R[2]", "--json", "--status"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let json = out(&output);
    assert!(json.contains("\"state\": \"absent\""), "{json}");
}

#[test]
fn the_workbench_is_the_tui_command_with_its_help() {
    // The command is `tui`; the workbench is what it opens.
    let scratch = Scratch::new("workbench-help");
    scratch.corpus();
    let help = at(scratch.path(), &["tui", "--help"]);
    assert_eq!(code(&help), 0, "{}", err(&help));
    assert!(
        out(&help).contains("Usage: samplekit tui"),
        "{}",
        out(&help)
    );
    let listed = out(&at(scratch.path(), &["--help"]));
    assert!(listed.contains("  tui "), "{listed}");
    // The old name is not kept: nothing was published under it.
    assert!(!listed.contains("  workbench "), "{listed}");
    // No terminal here: refused, rather than drawn into a pipe.
    let piped = at(scratch.path(), &["tui"]);
    assert_eq!(code(&piped), 1, "{}", out(&piped));
    assert!(err(&piped).contains("needs a terminal"), "{}", err(&piped));
}

#[test]
fn restore_takes_back_the_last_change_or_goes_to_a_state() {
    // Previewed value by value, written with --write, a snapshot of its own; a
    // file named narrows it.
    let scratch = Scratch::new("restore");
    with_history(&scratch);
    let file = scratch.path().join("keg-01.sample.md");
    let kept = fs::read_to_string(&file).unwrap();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=14", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let preview = at(scratch.path(), &["restore", "keg-01.sample.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("would restore to #2 · "),
        "{}",
        out(&preview)
    );
    assert!(out(&preview).contains("→"), "{}", out(&preview));
    assert_ne!(fs::read_to_string(&file).unwrap(), kept);
    let written = at(scratch.path(), &["restore", "keg-01.sample.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(fs::read_to_string(&file).unwrap(), kept);
    let logged = out(&at(scratch.path(), &["log"]));
    assert!(
        logged.contains("samplekit restore keg-01.sample.md --write"),
        "{logged}"
    );
    // To a state named: the project as SampleKit first kept it.
    let first = out(&at(scratch.path(), &["log"]))
        .lines()
        .filter(|line| line.contains("first kept it"))
        .count();
    assert_eq!(first, 1);
    let whole = at(scratch.path(), &["restore", "--at", "2026-01-01"]);
    assert_eq!(code(&whole), 1, "{}", out(&whole));
}

#[test]
fn explain_runs_a_files_selection_again_and_compares_its_declaration() {
    // A sample changed into its filter is one it would now take, and its
    // declaration changed makes it differently.
    let scratch = Scratch::new("explain-reselect");
    scratch.corpus();
    let export = at(
        scratch.path(),
        &["export", "platos", ".", "-f", "malt > 12.25", "--write"],
    );
    assert_eq!(code(&export), 0, "{}", err(&export));
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let explained = at(scratch.path(), &["explain", "out/platos.csv"]);
    let said = out(&explained);
    assert!(
        said.contains("its selection would now also take keg-01.sample.md"),
        "{said}{}",
        err(&explained)
    );
    let rc = scratch.path().join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    let changed = text.replacen("[export.platos]", "[export.platos]\nfilename = true", 1);
    assert_ne!(changed, text);
    fs::write(&rc, changed).unwrap();
    let said = out(&at(scratch.path(), &["explain", "out/platos.csv"]));
    assert!(said.contains("[export.platos] changed since"), "{said}");
}

#[test]
#[cfg(unix)]
fn a_file_reads_and_writes_what_its_text_says() {
    // The core review's defects, end to end: a text beginning with a date is
    // text; texts YAML would read otherwise are quoted; overflowing readings
    // give no value instead of a panic; `n/a` is no change from itself; it
    // sorts last both ways; `true` and `"true"` are two rows; a write
    // through a link writes the file it points at.
    let scratch = Scratch::new("core-review");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntitle: \"2024-01-01 meeting notes\"\nm: n/a\n\
         properties:\n  og:\n    readings: [1.060, 1.062]\n  big:\n    readings: [1.7e308, -1.7e308]\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nm: 7\nproperties:\n  og: 1.0\n---\n",
    );
    let viewed = at(scratch.path(), &["view", "a.md"]);
    assert_eq!(code(&viewed), 0, "{}", err(&viewed));
    // n/a set to n/a changes nothing.
    let same = at(scratch.path(), &["set", "a.md", "m=n/a"]);
    assert!(!out(&same).contains("n/a → n/a"), "{}", out(&same));
    // n/a sorts after 7 both ways.
    for sort in ["m", "-m"] {
        let sorted = out(&at(scratch.path(), &[".", "-s", sort, "-c", "name"]));
        let lines: Vec<&str> = sorted
            .lines()
            .filter(|line| line.trim() == "a" || line.trim() == "b")
            .collect();
        assert_eq!(
            lines.last().map(|line| line.trim()),
            Some("a"),
            "{sort}: {sorted}"
        );
    }
    // Texts written back as they read.
    for (at_, text) in ["ratio:", ".inf", "0x1F", "TRUE", "1_000", "+x"]
        .iter()
        .enumerate()
    {
        let text = *text;
        let set = at(
            scratch.path(),
            &["set", "b.md", &format!("note_{at_}={text}"), "--write"],
        );
        assert_eq!(code(&set), 0, "{text}: {}", err(&set));
    }
    let written = fs::read_to_string(scratch.path().join("b.md")).unwrap();
    for text in [
        "\"ratio:\"",
        "\".inf\"",
        "\"0x1F\"",
        "\"TRUE\"",
        "\"1_000\"",
        "\"+x\"",
    ] {
        assert!(written.contains(text), "{text} in {written}");
    }
    // Through a link, the file linked to.
    fs::create_dir_all(scratch.path().join("real")).unwrap();
    fs::rename(
        scratch.path().join("b.md"),
        scratch.path().join("real/b.md"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        scratch.path().join("real/b.md"),
        scratch.path().join("b.md"),
    )
    .unwrap();
    let set = at(scratch.path(), &["set", "b.md", "m=8", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        fs::symlink_metadata(scratch.path().join("b.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(scratch.path().join("real/b.md"))
            .unwrap()
            .contains("m: 8")
    );
    // Two rows, `true` and `"true"`.
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\ntables:\n  t:\n    index: x\n    columns:\n      x: {}\n      y: {}\n    rows:\n      - x: true\n        y: 1\n      - x: \"true\"\n        y: 2\n---\n",
    );
    let rows = at(scratch.path(), &["view", "c.md"]);
    assert_eq!(code(&rows), 0, "{}{}", out(&rows), err(&rows));
}

#[test]
fn a_deleted_sample_is_logged_compared_and_restored_by_name() {
    // A path gone from the disk whose folder is in a project is one the
    // history still names; brought back, it is restored, not new.
    let scratch = Scratch::new("deleted");
    with_history(&scratch);
    let file = scratch.path().join("keg-09.md");
    let kept = fs::read_to_string(&file).unwrap();
    fs::remove_file(&file).unwrap();
    let log = at(scratch.path(), &["log", "keg-09.md"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    assert!(out(&log).contains("samplekit new keg-09"), "{}", out(&log));
    let diff = at(scratch.path(), &["diff", "keg-09.md", "--from", "1"]);
    assert_eq!(code(&diff), 0, "{}", err(&diff));
    assert!(out(&diff).contains("keg-09.md   removed"), "{}", out(&diff));
    let restored = at(scratch.path(), &["restore", "keg-09.md", "--at", "1"]);
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert!(
        out(&restored).contains("keg-09.md   restored"),
        "{}",
        out(&restored)
    );
    assert!(!out(&restored).contains("new sample"), "{}", out(&restored));
    let written = at(
        scratch.path(),
        &["restore", "keg-09.md", "--at", "1", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(fs::read_to_string(&file).unwrap(), kept);
    // A path whose folder does not exist, or that no snapshot held — a
    // misspelling — is still refused.
    for missing in ["gone/keg-09.md", "keg-99.md"] {
        let nowhere = at(scratch.path(), &["log", missing]);
        assert_eq!(code(&nowhere), 1, "{}", out(&nowhere));
        assert!(
            err(&nowhere).contains("does not exist"),
            "{}",
            err(&nowhere)
        );
    }
}

#[test]
#[cfg(unix)]
fn restore_names_its_state_by_number_and_id_and_says_what_it_wrote() {
    let scratch = Scratch::new("restore-said");
    with_history(&scratch);
    // Nothing to restore: no advice to pass --write.
    let same = out(&at(scratch.path(), &["restore", "--at", "1"]));
    assert!(same.contains("already as it kept them"), "{same}");
    assert!(!same.contains("pass --write"), "{same}");
    // The state by its number and the beginning of its id, which stays when
    // the restore takes a number of its own.
    let entries = samplekit::config::version_control::entries(
        &samplekit::config::project_config::load(&scratch.path().join(".samplekitrc")).unwrap(),
    )
    .unwrap();
    let short = &entries[3].id[..7];
    let preview = out(&at(scratch.path(), &["restore", "--at", "4"]));
    assert!(
        preview.contains(&format!("would restore to #4 · {short} · ")),
        "{preview}"
    );
    assert!(preview.contains("pass --write to restore"), "{preview}");
    // A file that cannot be written: those put back before it are said.
    let second = scratch.path().join("keg-02.sample.md");
    let mode = |bits: u32| {
        fs::set_permissions(&second, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };
    mode(0o444);
    let refused = at(scratch.path(), &["restore", "--at", "4", "--write"]);
    mode(0o644);
    assert_eq!(code(&refused), 3, "{}", err(&refused));
    assert!(
        err(&refused).contains("restored before it: keg-01.sample.md"),
        "{}",
        err(&refused)
    );
    let written = out(&at(scratch.path(), &["restore", "--at", "5", "--write"]));
    assert!(
        written.contains(&format!("restored to {short} (#5 until now)")),
        "{written}"
    );
}

#[test]
fn explain_says_its_selection_before_its_verdict() {
    // An export made without a target is selected again from the folder it ran
    // in; what its selection would now take is said first, and the verdict
    // follows it.
    let scratch = Scratch::new("explain-verdict");
    scratch.corpus();
    let export = at(
        scratch.path(),
        &["export", "platos", "-f", "malt > 12.25", "--write"],
    );
    assert_eq!(code(&export), 0, "{}", err(&export));
    let fresh = out(&at(scratch.path(), &["explain", "out/platos.csv"]));
    assert!(fresh.contains("it is up to date"), "{fresh}");
    assert!(!fresh.contains("its selection"), "{fresh}");
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let said = out(&at(scratch.path(), &["explain", "out/platos.csv"]));
    let taken = said
        .find("its selection would now also take keg-01.sample.md")
        .expect(&said);
    let verdict = said.find("it is not current").expect(&said);
    assert!(taken < verdict, "{said}");
    assert!(!said.contains("up to date"), "{said}");
    // Made again as it was: an export `--at` writes where -o says.
    assert!(said.contains(" -o out/platos.csv --at "), "{said}");
}

#[test]
fn the_same_contents_written_twice_are_told_apart() {
    // Two writes of the same bytes — one `--at` the snapshot it was first
    // made from — are two records; each file is explained by its own.
    let scratch = Scratch::new("explain-twice");
    scratch.corpus();
    let first = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let old = at(
        scratch.path(),
        &[
            "export",
            "platos",
            ".",
            "--at",
            "2",
            "-o",
            "out/old.csv",
            "--write",
        ],
    );
    assert_eq!(code(&old), 0, "{}", err(&old));
    assert_eq!(
        fs::read(scratch.path().join("out/old.csv")).unwrap(),
        fs::read(scratch.path().join("out/platos.csv")).unwrap()
    );
    let declared = out(&at(scratch.path(), &["explain", "out/platos.csv"]));
    assert!(declared.contains("written   out/platos.csv"), "{declared}");
    assert!(declared.contains("it is not current"), "{declared}");
    let remade = out(&at(scratch.path(), &["explain", "out/old.csv"]));
    assert!(remade.contains("written   out/old.csv"), "{remade}");
    assert!(remade.contains("on purpose"), "{remade}");
    // A figure's carried snapshot is matched against the figures made from
    // it alone, never an export.
    let entries = samplekit::config::version_control::entries(
        &samplekit::config::project_config::load(&scratch.path().join(".samplekitrc")).unwrap(),
    )
    .unwrap();
    scratch.write(
        "figure.svg",
        &format!(
            "<svg><desc>{}{}</desc></svg>",
            samplekit::config::version_control::CARRIED,
            entries[1].id
        ),
    );
    let figure = at(scratch.path(), &["explain", "figure.svg"]);
    assert_eq!(code(&figure), 1, "{}", out(&figure));
    assert!(
        err(&figure).contains("records no figure"),
        "{}",
        err(&figure)
    );
}

#[test]
fn a_minute_reaches_the_snapshots_taken_within_it() {
    // A bare date is the whole day; a minute is the whole minute.
    let scratch = Scratch::new("when-minute");
    with_history(&scratch);
    let entries = samplekit::config::version_control::entries(
        &samplekit::config::project_config::load(&scratch.path().join(".samplekitrc")).unwrap(),
    )
    .unwrap();
    let minute = samplekit::presentation::changes::when(&entries[0]);
    let dated = at(scratch.path(), &["diff", "--from", &minute, "--to", "1"]);
    assert_eq!(code(&dated), 0, "{}", err(&dated));
    assert!(out(&dated).starts_with("from  #1 · "), "{}", out(&dated));
}

#[test]
fn log_sets_its_text_columns_from_the_left() {
    // A column of *4 samples* reads to the table as numbers: its cells are
    // set from the left, as written, and `now` is said under *when*.
    let scratch = Scratch::new("log-columns");
    scratch.corpus();
    for tag in ["first", "second"] {
        let output = at(scratch.path(), &["tag", "add", tag, ".", "--write"]);
        assert_eq!(code(&output), 0, "{}", err(&output));
    }
    let log = out(&at(scratch.path(), &["log", "--at", "1"]));
    let columns: Vec<usize> = log
        .lines()
        .filter_map(|line| line.find("4 samples"))
        .collect();
    assert_eq!(columns.len(), 3, "{log}");
    assert!(columns.iter().all(|at| *at == columns[0]), "{log}");
    let file = scratch.path().join("keg-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("approved", "retested")).unwrap();
    let one = out(&at(scratch.path(), &["log", "keg-03.sample.md"]));
    let now = one.lines().find(|line| line.contains("now")).expect(&one);
    // The `#` column holds its numbers: one digit wide, and blank for now.
    assert!(
        now.replace(['│', '┃'], " ").trim_start().starts_with("now"),
        "{one}"
    );
    let first = one
        .lines()
        .find(|line| line.contains("tag add second"))
        .expect(&one);
    assert!(first.contains("│ 1 │"), "{one}");
}

#[test]
fn diff_from_before_the_first_snapshot_aligns_its_files() {
    let scratch = Scratch::new("diff-first");
    with_history(&scratch);
    let first = out(&at(scratch.path(), &["diff", "--to", "4"]));
    assert!(
        first.starts_with("from  (before the first snapshot)\n"),
        "{first}"
    );
    assert!(first.contains(".samplekitrc       new\n"), "{first}");
    assert!(first.contains("keg-01.sample.md   new sample\n"), "{first}");
}

#[test]
fn log_diff_and_restore_help_say_their_own_defaults() {
    let scratch = Scratch::new("history-help");
    scratch.corpus();
    let log = out(&at(scratch.path(), &["log", "--help"]));
    assert!(log.contains("Start the list at this snapshot"), "{log}");
    assert!(!log.contains("Read the project as"), "{log}");
    assert!(!log.contains("second snapshot"), "{log}");
    let diff = out(&at(scratch.path(), &["diff", "--help"]));
    assert!(diff.contains("now with --from alone"), "{diff}");
    let restore = out(&at(scratch.path(), &["restore", "--help"]));
    assert!(!restore.contains("third snapshot"), "{restore}");
}

#[test]
fn script_and_restore_complete_snapshots_not_now() {
    // `now` is refused by `log --script` and `restore --at`: not offered.
    let scratch = Scratch::new("complete-snapshots");
    with_history(&scratch);
    let complete = |words: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", (words.len() - 1).to_string())
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        out(&output)
    };
    for words in [
        ["samplekit", "log", "--script", ""],
        ["samplekit", "restore", "--at", ""],
    ] {
        let offered = complete(&words);
        assert!(
            offered.lines().any(|line| line.starts_with('1')),
            "{offered}"
        );
        assert!(
            !offered.lines().any(|line| line.starts_with("now")),
            "{offered}"
        );
    }
    let diff = complete(&["samplekit", "diff", "--from", ""]);
    assert!(diff.lines().any(|line| line.starts_with("now")), "{diff}");
}

#[test]
fn a_state_naming_nothing_is_refused_where_no_history_is_kept() {
    let scratch = Scratch::new("no-history-states");
    scratch.corpus();
    for words in [
        vec!["log", "--script", "1"],
        vec!["diff", "--from", "1"],
        vec!["restore", "--at", "1"],
    ] {
        let output = at(scratch.path(), &words);
        assert_eq!(code(&output), 1, "{words:?}: {}", out(&output));
        assert!(
            err(&output).contains("no history is kept here yet"),
            "{words:?}: {}",
            err(&output)
        );
        assert!(!err(&output).contains("from 1 to 0"), "{}", err(&output));
    }
    // Without a state named, none kept is said, exit 0, and nothing to write
    // is not offered to be written.
    let restore = at(scratch.path(), &["restore"]);
    assert_eq!(code(&restore), 0, "{}", err(&restore));
    assert!(!out(&restore).contains("--write"), "{}", out(&restore));
    // `restore --at now` names where the files already are.
    let now = at(scratch.path(), &["restore", "--at", "now"]);
    assert_eq!(code(&now), 1, "{}", out(&now));
    assert!(err(&now).contains("already are"), "{}", err(&now));
}

#[test]
fn explain_at_without_a_field_names_one_of_the_sample() {
    // The sample named and one of its own fields as the example, never
    // another project's.
    let scratch = Scratch::new("explain-at-field");
    with_history(&scratch);
    let output = at(
        scratch.path(),
        &["explain", "keg-01.sample.md", "--at", "4"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("samplekit explain keg-01.sample.md malt --at 4"),
        "{}",
        err(&output)
    );
}

// ------------------------------------------------ the review of 2026-09-27

/// A small brewing project: `og` measured without a unit and declared at
/// `.3f`, `abv` computed at `.1f`, `volume` in litres, and a figure.
fn brews(scratch: &Scratch) {
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\
         [collection]\n\
         exclude = [\"README.md\"]\n\
         [property.og]\n\
         precision = [\".3f\", \".4f\"]\n\
         [property.abv]\n\
         precision = \".1f\"\n\
         [property.volume]\n\
         unit = \"L\"\n\
         [figure.strength]\n\
         kind = \"scatter\"\n\
         x = \"og\"\n\
         y = \"abv\"\n\
         group = \"style\"\n",
    );
    scratch.write("README.md", "# Brews\n\nNot a sample.\n");
    scratch.write(
        "citra-ipa.md",
        "---\nschema_version: 1\nname: citra-ipa\nstyle: ipa\nproperties:\n  og:\n    v: 1.0636666666666668\n    \
         readings: [1.063, 1.065, 1.063]\n    u: 6.666666666666e-4\n    statistics: {v: mean, u: standard_error}\n    \
         computed: {readings: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n  fg: {v: 1.012}\n  volume: {v: 19.5, unit: L}\n  \
         abv:\n    v: 6.825000000000035\n    unit: \"%\"\n    computed: {fg: cccccccccccc, og: dddddddddddd}\n    \
         fingerprint: eeeeeeeeeeee\n---\n# Citra IPA\n",
    );
    scratch.write(
        "smoked-porter.md",
        "---\nschema_version: 1\nname: smoked-porter\nstyle: porter\nproperties:\n  og: {v: 1.066}\n  \
         volume: {v: 20.0, unit: L}\n  abv: {unit: \"%\"}\n---\n# Smoked porter\n",
    );
}

#[test]
fn a_closed_reader_still_lets_a_write_happen() {
    // The preview is printed before the write: `set … --write | head -1`
    // lost its reader on the second line, and the command ended there with
    // 0, having written nothing.
    let scratch = Scratch::new("closed-write");
    brews(&scratch);
    let mut child = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["set", "smoked-porter.md", "volume=21", "--write"])
        .current_dir(scratch.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The reader is gone before anything is written.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("smoked-porter.md")).unwrap();
    assert!(written.contains("v: 21"), "{written}");
}

#[test]
fn explain_says_a_value_waits_for_an_input_nobody_entered() {
    // `abv` of a brew with no `fg` was *never computed … samplekit compute*,
    // which computes nothing while fg is missing: `status` says it waits.
    let scratch = Scratch::new("explain-waits");
    brews(&scratch);
    scratch.write(
        ".samplekitrc",
        &(fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap()
            + "[model]\npath = \"model.py\"\n"),
    );
    let output = at(scratch.path(), &["explain", "smoked-porter.md", "abv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(said.contains("it waits for fg"), "{said}");
    assert!(
        said.contains("samplekit set smoked-porter.md fg=<value>"),
        "{said}"
    );
    assert!(!said.contains("never computed"), "{said}");
}

#[test]
fn an_unknown_row_lists_the_rows_there_are() {
    // `no row of 'tasting' is indexed [Nobody]` listed no row, and said
    // `taster: matched` of the one column that had not.
    let scratch = Scratch::new("unknown-row");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  tasting:\n    index: taster\n    columns:\n      \
         taster: {}\n      score: {}\n    rows:\n      - {taster: Lea, score: 40}\n      \
         - {taster: Sam, score: 38}\n---\n",
    );
    let output = at(scratch.path(), &[".", "-f", "tasting.score[Nobody] > 1"]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("the rows of 'tasting', by taster: Lea, Sam"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("matched"), "{}", err(&output));
}

#[test]
fn readings_say_the_convention_their_file_records() {
    // The note said no convention was named, beneath readings whose file
    // records standard_error; and the lines read as fragments.
    let scratch = Scratch::new("readings-convention");
    brews(&scratch);
    let recorded = at(
        scratch.path(),
        &["set", "citra-ipa.md", "og.readings=1.06,1.062"],
    );
    let said = out(&recorded);
    assert!(
        said.contains("og's uncertainty is their standard error, as its file records"),
        "{said}"
    );
    assert!(
        said.contains("they replace the 3 readings it held"),
        "{said}"
    );
    assert!(
        !said.contains("only where the model names a convention"),
        "{said}"
    );
    let none = at(
        scratch.path(),
        &["set", "smoked-porter.md", "og.readings=1.06,1.062"],
    );
    assert!(
        !out(&none).contains("og's uncertainty is their"),
        "{}",
        out(&none)
    );
}

#[test]
fn plot_names_the_figures_when_a_name_is_no_figure() {
    // `plot -o x.png --write brews`, the figure's name forgotten, asked the
    // model for a figure called `brews`, and said only why it could not.
    let scratch = Scratch::new("plot-no-figure");
    brews(&scratch);
    fs::create_dir_all(scratch.path().join("brews")).unwrap();
    let forgotten = at(scratch.path(), &["plot", "-o", "x.png", "--write", "brews"]);
    assert_eq!(code(&forgotten), 1, "{}", err(&forgotten));
    assert!(
        err(&forgotten).contains("no figure named 'brews'")
            && err(&forgotten).contains("the figures: strength"),
        "{}",
        err(&forgotten)
    );
    assert!(!scratch.path().join("x.png").exists());
    // A name no declaration holds, the model beyond reach: the name first.
    scratch.write(
        ".samplekitrc",
        &(fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap()
            + "[model]\npath = \"model.py\"\n"),
    );
    let unknown = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["plot", "nosuch", ".", "-o", "x.png", "--write"])
        .current_dir(scratch.path())
        .env("SAMPLEKIT_STATE_DIR", scratch.path().join("state"))
        .output()
        .unwrap();
    assert_ne!(code(&unknown), 0, "{}", out(&unknown));
    assert!(
        err(&unknown).contains("no figure named 'nosuch' is declared — the figures: strength"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn an_output_file_without_columns_asks_for_them() {
    // `-o t.csv` without columns was refused as an extension it plainly has.
    let scratch = Scratch::new("output-columns");
    brews(&scratch);
    let output = at(scratch.path(), &[".", "-o", "t.csv", "--write"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("has no columns — name them with --columns or --profile"),
        "{}",
        err(&output)
    );
    assert!(
        !err(&output).contains("the extension names"),
        "{}",
        err(&output)
    );
}

#[test]
fn an_export_refuses_an_output_named_for_another_format() {
    // `export platos -o r.json --write` wrote CSV into r.json.
    let scratch = Scratch::new("export-extension");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["export", "platos", ".", "-o", "r.json", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("'platos' writes CSV, and the extension names JSON"),
        "{}",
        err(&output)
    );
    assert!(!scratch.path().join("r.json").exists());
    let same = at(
        scratch.path(),
        &["export", "platos", ".", "-o", "r.csv", "--write"],
    );
    assert_eq!(code(&same), 0, "{}", err(&same));
}

#[test]
fn list_skipped_says_not_read_in_columns() {
    // A file a newer SampleKit wrote was *malformed frontmatter* here and
    // *not read* in validate; and the reasons stood wherever each path ended.
    let scratch = Scratch::new("list-skipped");
    brews(&scratch);
    scratch.write("zz-newer.md", "---\nschema_version: 9\nname: z\n---\n");
    let output = at(scratch.path(), &["list", "skipped"]);
    let said = out(&output);
    assert!(said.contains("not read: schema_version 9"), "{said}");
    assert!(!said.contains("malformed frontmatter"), "{said}");
    // Two files of two lengths, README.md and zz-newer.md: their reasons
    // begin in one column.
    assert_eq!(
        said.lines().filter(|line| !line.is_empty()).count(),
        2,
        "{said}"
    );
    let columns: std::collections::HashSet<usize> = said
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let at = line.find("  ").unwrap();
            at + line[at..].len() - line[at..].trim_start().len()
        })
        .collect();
    assert_eq!(columns.len(), 1, "{said}");
}

#[test]
fn an_unknown_word_names_every_command() {
    // The list written by hand lacked restore, init, new, set and tui.
    let scratch = Scratch::new("unknown-word");
    brews(&scratch);
    let output = at(scratch.path(), &["nosuchdir"]);
    for command in ["restore", "init", "new", "set", "tui", "log", "diff"] {
        assert!(
            err(&output).contains(command),
            "{command}: {}",
            err(&output)
        );
    }
    let near = at(scratch.path(), &["resore"]);
    assert!(
        err(&near).contains("did you mean 'restore'"),
        "{}",
        err(&near)
    );
    fs::create_dir_all(scratch.path().join("brews")).unwrap();
    let after = at(scratch.path(), &["brews", "log"]);
    assert!(
        err(&after).contains("use: samplekit log brews"),
        "{}",
        err(&after)
    );
}

#[test]
fn a_file_that_is_no_sample_is_said_to_be_none() {
    // `samplekit .samplekitrc` answered *because something already claimed
    // it was a sample*.
    let scratch = Scratch::new("no-sample");
    brews(&scratch);
    let rc = at(scratch.path(), &[".samplekitrc"]);
    assert_eq!(code(&rc), 2, "{}", err(&rc));
    assert!(
        err(&rc).contains(".samplekitrc is the project's configuration, not a sample"),
        "{}",
        err(&rc)
    );
    let readme = at(scratch.path(), &["view", "README.md"]);
    assert!(
        err(&readme).contains("README.md is not a sample"),
        "{}",
        err(&readme)
    );
    assert!(
        err(&readme).contains("the project leaves it out"),
        "{}",
        err(&readme)
    );
    scratch.write("model.py", "x = 1\n");
    let model = at(scratch.path(), &["set", "model.py", "x=1"]);
    assert!(
        err(&model).contains("model.py is not a sample"),
        "{}",
        err(&model)
    );
    for output in [&rc, &readme, &model] {
        assert!(!err(output).contains("already claimed"), "{}", err(output));
    }
}

#[test]
fn a_precision_that_is_no_precision_is_said_so() {
    let scratch = Scratch::new("precision-word");
    brews(&scratch);
    let output = at(scratch.path(), &[".", "-c", "og", "--precision", "abc"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("'abc' is not a precision"),
        "{}",
        err(&output)
    );
    assert!(err(&output).contains(".3f"), "{}", err(&output));
    assert!(!err(&output).contains("fill"), "{}", err(&output));
}

#[test]
fn a_tag_error_counts_characters_from_one() {
    // A tag's error said `position 1` of the second character, a filter's
    // `character 2`.
    let scratch = Scratch::new("tag-position");
    brews(&scratch);
    let output = at(scratch.path(), &["tag", "add", "a b"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("at character 2"), "{}", err(&output));
    assert!(!err(&output).contains("position"), "{}", err(&output));
}

#[test]
fn new_like_says_which_measured_values_the_file_holds() {
    // The preview listed `og` to fill in, and the file written had no `og`:
    // an empty quantity is written only for its unit.
    let scratch = Scratch::new("new-like-measured");
    brews(&scratch);
    let preview = at(
        scratch.path(),
        &["new", "pale", "--like", "smoked-porter.md"],
    );
    let said = out(&preview);
    assert!(
        said.contains("measured, for you to fill in   og, volume"),
        "{said}"
    );
    assert!(
        said.contains(
            "og has no unit, so the file holds no line for it until samplekit set writes a value"
        ),
        "{said}"
    );
    let written = at(
        scratch.path(),
        &["new", "pale", "--like", "smoked-porter.md", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let file = fs::read_to_string(scratch.path().join("pale.md")).unwrap();
    assert!(
        file.contains("volume: {unit: L}") && !file.contains("og"),
        "{file}"
    );
    // Its help promises no tables.
    let help = out(&at(scratch.path(), &["new", "--help"]));
    assert!(!help.contains("units and tables"), "{help}");
}

#[test]
fn a_box_plot_names_whose_group_it_refuses() {
    // `--kind box` on a figure declaring `group` was refused as *--group has
    // nothing left to split*, of a --group nobody had typed; and `--help`
    // listed four kinds of five.
    let scratch = Scratch::new("box-group");
    brews(&scratch);
    let output = at(
        scratch.path(),
        &["plot", "strength", ".", "--kind", "box", "-o", "s.png"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("the figure's group, 'style'"),
        "{}",
        err(&output)
    );
    let typed = at(
        scratch.path(),
        &[
            "plot", "strength", ".", "--kind", "box", "--group", "style", "-o", "s.png",
        ],
    );
    assert!(
        err(&typed).contains("--group style has nothing"),
        "{}",
        err(&typed)
    );
    let help = out(&at(scratch.path(), &["plot", "--help"]));
    assert!(help.contains("scatter, line, step, bar or box"), "{help}");
}

#[test]
fn a_write_says_what_it_did_and_nothing_to_write_offers_no_flag() {
    let scratch = Scratch::new("tag-tense");
    brews(&scratch);
    let added = at(
        scratch.path(),
        &["tag", "add", "hoppy", "citra-ipa.md", "--write"],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    assert!(
        out(&added).starts_with("added 'hoppy' to 1 of 1 samples"),
        "{}",
        out(&added)
    );
    assert!(!out(&added).contains("will"), "{}", out(&added));
    let again = at(scratch.path(), &["tag", "add", "hoppy", "citra-ipa.md"]);
    assert!(out(&again).contains("nothing to write"), "{}", out(&again));
    assert!(!out(&again).contains("pass --write"), "{}", out(&again));
}

#[test]
fn a_hint_names_the_field_and_file_given() {
    // Examples came from another collection: `BR-01.md hop_rate=13.2`,
    // `keg-04.sample.md brix`, `-s malt -r`.
    let scratch = Scratch::new("hints-given");
    brews(&scratch);
    let set = at(scratch.path(), &["set", "citra-ipa.md"]);
    assert!(
        err(&set).contains("samplekit set citra-ipa.md og=<value>"),
        "{}",
        err(&set)
    );
    let explain = at(scratch.path(), &["explain", "citra-ipa.md"]);
    assert!(
        err(&explain).contains("samplekit explain citra-ipa.md <field>"),
        "{}",
        err(&explain)
    );
    let reverse = at(scratch.path(), &[".", "-c", "name,og", "-r"]);
    assert!(err(&reverse).contains("-s og -r"), "{}", err(&reverse));
    for output in [&set, &explain, &reverse] {
        for foreign in ["BR-01", "hop_rate", "keg", "brix", "malt"] {
            assert!(!err(output).contains(foreign), "{foreign}: {}", err(output));
        }
    }
}

#[test]
fn the_long_help_opens_with_the_tagline() {
    let scratch = Scratch::new("tagline");
    let long = out(&at(scratch.path(), &["--help"]));
    let short = out(&at(scratch.path(), &["-h"]));
    assert_eq!(long.lines().next(), short.lines().next(), "{long}\n{short}");
    assert!(
        long.contains("prints the path of each sample selected"),
        "{long}"
    );
    let view = out(&at(scratch.path(), &["view", "--help"]));
    assert!(
        view.contains("Read the project as it was at a snapshot of its history"),
        "{view}"
    );
}

#[test]
fn an_unknown_field_is_said_alike_by_every_option() {
    // -c listed name, path, filename and project and ended on the count; -f
    // and -s left them out and ended elsewhere. A table's column without its
    // index was explained under -c alone.
    let scratch = Scratch::new("unknown-alike");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[query.big]\nfilter = 'og > 1'\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  og: {v: 1.05}\ntables:\n  \
         fermentation:\n    index: day\n    columns:\n      day: {}\n      gravity: {}\n    \
         rows:\n      - {day: 1, gravity: 1.04}\n---\n",
    );
    let said: Vec<String> = [
        vec![".", "-c", "zzz"],
        vec![".", "-f", "zzz > 1"],
        vec![".", "-s", "zzz"],
    ]
    .iter()
    .map(|arguments| err(&at(scratch.path(), arguments)))
    .collect();
    for message in &said {
        assert!(message.contains("unknown field 'zzz'"), "{message}");
        assert!(
            message.contains("available: name, path, filename, project"),
            "{message}"
        );
        assert!(
            message.contains("fields available — samplekit list fields"),
            "{message}"
        );
    }
    let query = at(scratch.path(), &[".", "--query", "nosuch"]);
    assert!(
        err(&query).contains("no query named 'nosuch'"),
        "{}",
        err(&query)
    );
    let column = at(scratch.path(), &[".", "-c", "fermentation.gravity"]);
    assert!(
        err(&column).contains("'fermentation.gravity' is a column of the table 'fermentation'")
            && err(&column).contains("e.g.     'fermentation.gravity[1]'"),
        "{}",
        err(&column)
    );
}

#[test]
fn a_warning_is_one_warning_line() {
    // Each line of a warning was its own `warning:`, blank ones included,
    // and `set`'s advice was about `new --like`.
    let scratch = Scratch::new("one-warning");
    brews(&scratch);
    let output = at(scratch.path(), &["set", "citra-ipa.md", "nosuch=3"]);
    let said = err(&output);
    assert_eq!(said.matches("warning:").count(), 1, "{said}");
    assert!(
        !said.lines().any(|line| line.trim() == "warning:"),
        "{said}"
    );
    assert!(said.contains("[property.nosuch]"), "{said}");
    assert!(!said.contains("--like"), "{said}");
}

#[test]
fn help_sections_close_with_the_global_options() {
    // compute put Presentation after General; plot put -h among its own
    // General options; plot -h wrote --legend's places on one long line.
    let scratch = Scratch::new("help-order");
    let compute = out(&at(scratch.path(), &["compute", "--help"]));
    let at_heading = |page: &str, heading: &str| page.find(&format!("\n{heading}:")).unwrap();
    assert!(
        at_heading(&compute, "Presentation") < at_heading(&compute, "Output"),
        "{compute}"
    );
    let plot = out(&at(scratch.path(), &["plot", "--help"]));
    assert!(
        plot.find("--quiet").unwrap() < plot.find("--help").unwrap(),
        "{plot}"
    );
    assert!(
        plot.find("--quiet").unwrap() < plot.find("--no-color").unwrap(),
        "{plot}"
    );
    let short = out(&at(scratch.path(), &["plot", "-h"]));
    assert!(
        short.lines().all(|line| line.chars().count() < 160),
        "{short}"
    );
}

#[test]
fn a_written_file_is_said_in_one_form() {
    // `wrote`, `written to`, `12 rows written to` and `written brews/…`.
    let scratch = Scratch::new("written-form");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert!(
        out(&set).contains("written: keg-01.sample.md"),
        "{}",
        out(&set)
    );
    let table = at(
        scratch.path(),
        &[".", "-c", "name,malt", "-o", "t.csv", "--write"],
    );
    assert!(err(&table).contains("written: t.csv"), "{}", err(&table));
    let export = at(scratch.path(), &["export", "platos", ".", "--write"]);
    assert!(
        err(&export).contains("platos.csv, 4 rows") && err(&export).starts_with("written: "),
        "{}",
        err(&export)
    );
}

#[test]
fn list_sorts_tags_and_counts_every_kind() {
    let scratch = Scratch::new("list-tags-sorted");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntags: [zest, amber]\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\ntags: [medal]\n---\n",
    );
    let tags = out(&at(scratch.path(), &["list", "tags"]));
    assert_eq!(tags, "amber\nmedal\nzest\n");
    let counted = out(&at(scratch.path(), &["list"]));
    assert!(
        counted.contains("3 tags in use — amber, medal, zest"),
        "{counted}"
    );
    fs::create_dir_all(scratch.path().join("empty")).unwrap();
    let empty = out(&at(scratch.path(), &["list", "empty"]));
    assert!(
        empty.contains("0 properties, 0 attributes, 0 tables"),
        "{empty}"
    );
}

#[test]
fn at_writes_an_output_given_in_any_form() {
    // `-oFILE` and `-o=FILE` were not moved back to where the command ran:
    // the file went into the temporary project, removed at the end, and the
    // command said it had written it.
    let scratch = Scratch::new("at-output-forms");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "malt=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    for (form, file) in [
        ("-oattached.csv", "attached.csv"),
        ("-o=equals.csv", "equals.csv"),
        ("--output=long.csv", "long.csv"),
    ] {
        let output = at(
            scratch.path(),
            &[".", "-c", "name,malt", "--at", "1", form, "--write"],
        );
        assert_eq!(code(&output), 0, "{form}: {}", err(&output));
        assert!(
            scratch.path().join(file).is_file(),
            "{form}: {}",
            err(&output)
        );
    }
    let clustered = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name,malt",
            "--at",
            "1",
            "-qo",
            "clustered.csv",
            "--write",
        ],
    );
    assert_eq!(code(&clustered), 0, "{}", err(&clustered));
    assert!(scratch.path().join("clustered.csv").is_file());
}

#[test]
fn open_refuses_a_folder() {
    // `open brews --edit` edited whichever sample the scan met first.
    let scratch = Scratch::new("open-folder");
    brews(&scratch);
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["open", ".", "--edit"])
        .current_dir(scratch.path())
        .env("EDITOR", "false")
        .output()
        .unwrap();
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("is a folder"), "{}", err(&output));
}

#[test]
fn a_row_added_to_a_bare_file_name_takes_its_siblings_table() {
    // `set nt.md --add-row …` read the empty parent of `nt.md` as no folder,
    // found no sibling, and took the table's shape from the model instead.
    let scratch = Scratch::new("add-row-bare");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  tasting:\n    index: taster\n    columns:\n      \
         taster: {}\n      score: {unit: pt}\n    rows:\n      - {taster: Lea, score: 40}\n---\n",
    );
    scratch.write("nt.md", "---\nschema_version: 1\nname: nt\n---\n");
    let output = at(
        scratch.path(),
        &[
            "set",
            "nt.md",
            "--add-row",
            "tasting",
            "taster=Sam",
            "score=38",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("the table is created here, with the columns of a.md"),
        "{}",
        out(&output)
    );
}

#[test]
fn new_like_reads_the_declarations_of_the_project_it_writes_into() {
    // The declarations were read from --into or the current directory while
    // the file went beside its pattern, in another project.
    let scratch = Scratch::new("new-like-project");
    fs::create_dir_all(scratch.path().join("elsewhere")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[property.malt]\nunit = \"g\"\n",
    );
    scratch.write(
        "project/a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let output = at(
        &scratch.path().join("elsewhere"),
        &["new", "b", "--like", "../project/a.md", "malt=2", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("attribute"), "{}", err(&output));
    let file = fs::read_to_string(scratch.path().join("project/b.md")).unwrap();
    assert!(file.contains("malt: {v: 2, unit: g}"), "{file}");
}

#[test]
fn a_declared_export_says_it_creates_its_directory() {
    let scratch = Scratch::new("export-directory-said");
    scratch.corpus();
    fs::remove_dir_all(scratch.path().join("out")).unwrap();
    let output = at(scratch.path(), &["export", "platos", "."]);
    assert!(
        out(&output).contains("out does not exist, and --write creates it"),
        "{}",
        out(&output)
    );
    assert!(!scratch.path().join("out").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn init_answered_at_a_terminal_is_a_snapshot() {
    // Only a typed --write took a snapshot: the yes at a terminal wrote a
    // project with no history.
    let scratch = Scratch::new("init-asked-history");
    let output = at_terminal(scratch.path(), &["init"], "1\n3\n2\ny\n");
    assert_eq!(code(&output), 0, "{}", out(&output));
    let log = at(scratch.path(), &["log"]);
    assert!(
        out(&log).contains("answered at the terminal"),
        "{}",
        out(&log)
    );
}

#[test]
fn what_the_review_left_between_the_agents_is_corrected() {
    // Files in a folder named after their sample; a count exported whole; a blank name;
    // a newer format said as such; explain on a file that is not there.
    let scratch = Scratch::new("review-leftovers");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n[property.og]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  og:\n    readings: [1.060, 1.062, 1.061]\n---\n",
    );
    fs::create_dir_all(scratch.path().join("images/b")).unwrap();
    fs::write(scratch.path().join("images/b/photo.jpg"), "").unwrap();
    let files = out(&at(scratch.path(), &["list", "files", "."]));
    assert!(files.contains("photo.jpg"), "{files}");
    let count = out(&at(
        scratch.path(),
        &[".", "-c", "name,og.stats.count", "--csv"],
    ));
    assert!(count.contains("b,3\n"), "{count}");
    let blank = at(scratch.path(), &["new", " ", "--write"]);
    assert_eq!(code(&blank), 1, "{}", out(&blank));
    scratch.write("new.md", "---\nschema_version: 99\nname: n\n---\n");
    let newer = err(&at(scratch.path(), &["list", "."]));
    assert!(
        newer.contains("schema_version 99") && !newer.contains("malformed"),
        "{newer}"
    );
    let missing = at(scratch.path(), &["explain", "nothing.pdf"]);
    assert_eq!(code(&missing), 3, "{}", err(&missing));
    assert!(err(&missing).contains("no such file"), "{}", err(&missing));
}

#[test]
fn new_writes_a_value_as_the_collection_holds_it() {
    // `new pilsner og=1.048 volume=20` beside brews that all measure both
    // wrote two attributes, and warned *'og' is new here*.
    let scratch = Scratch::new("new-as-held");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, og) in [("a", "1.050"), ("b", "1.060")] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  og: {og}\n  \
                 volume: {{v: 20.5, unit: L}}\n---\n"
            ),
        );
    }
    let output = at(
        scratch.path(),
        &["new", "pilsner", "og=1.048", "volume=20", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("attribute"), "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("pilsner.md")).unwrap();
    assert!(written.contains("properties:\n  og: 1.048"), "{written}");
    assert!(written.contains("volume: {v: 20, unit: L}"), "{written}");
    // A decimal comma is refused as `set` refuses it.
    let comma = at(scratch.path(), &["new", "pale", "og=1,050"]);
    assert_eq!(code(&comma), 1, "{}", out(&comma));
    assert!(
        err(&comma).contains("'og' holds a number, and '1,050' is text")
            && err(&comma).contains("og=1.050"),
        "{}",
        err(&comma)
    );
    // A name nobody holds is an attribute, and the warning says why, truly.
    let unheld = at(scratch.path(), &["new", "stout", "batch=3"]);
    assert!(
        err(&unheld).contains(
            "no sample holds it as one, and neither .samplekitrc nor the model declares it"
        ),
        "{}",
        err(&unheld)
    );
    assert!(!err(&unheld).contains("is new here"), "{}", err(&unheld));
}

#[test]
fn a_name_only_the_model_declares_is_written_as_a_quantity() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    // `ibu` only the model declared was written as an attribute, and the
    // model then refused the sample: *'ibu' is already a property*. The
    // model's description says so; importing the model would leave a mark.
    let scratch = Scratch::new("model-declares");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n",
    );
    scratch.write(
        "model/keg.py",
        "import pathlib\n\nimport samplekit as sk\n\n\
         pathlib.Path(__file__).with_name(\"imported\").write_text(\"\")\n\n\
         class Keg(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.ibu = sk.Property(unit=\"mL\")\n        \
         self.height = sk.Property(\n            unit=\"mL\",\n        )\n",
    );
    describe(
        scratch.path(),
        serde_json::json!({"ibu": entered(Some("mL")), "collar": entered(Some("mL"))}),
        serde_json::json!({}),
    );
    let new = at(scratch.path(), &["new", "S-01", "ibu=25.1", "--write"]);
    assert_eq!(code(&new), 0, "{}", err(&new));
    assert!(
        out(&new).contains("ibu (new, a quantity model/keg.py declares)"),
        "{}",
        out(&new)
    );
    let set = at(scratch.path(), &["set", "S-01.md", "collar=4.0", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = fs::read_to_string(scratch.path().join("S-01.md")).unwrap();
    assert!(written.contains("ibu: {v: 25.1, unit: mL}"), "{written}");
    assert!(written.contains("collar: {v: 4.0, unit: mL}"), "{written}");
    assert!(!written.contains("\nibu:"), "an attribute: {written}");
    assert!(!scratch.path().join("model/imported").exists());
    let text = at(scratch.path(), &["new", "S-02", "ibu=abc"]);
    assert_eq!(code(&text), 1, "{}", out(&text));
    assert!(err(&text).contains("mL"), "{}", err(&text));
}

#[test]
fn text_for_a_property_the_model_declares_is_a_property() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    // `rating`, which only the model declares and gives no unit, is a property
    // whatever is written: as an attribute, the model refused it.
    let scratch = Scratch::new("model-text");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n",
    );
    scratch.write(
        "model/keg.py",
        "import samplekit as sk\n\n\
         class Keg(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.rating = sk.Property()\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({"rating": entered(None)}),
        serde_json::json!({}),
    );
    let set = at(scratch.path(), &["set", "S-01.md", "rating=A", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        out(&set).contains("new, a quantity model/keg.py declares"),
        "{}",
        out(&set)
    );
    let written = fs::read_to_string(scratch.path().join("S-01.md")).unwrap();
    assert!(written.contains("properties:\n  rating:"), "{written}");
    let new = at(scratch.path(), &["new", "S-02", "rating=B", "--write"]);
    assert_eq!(code(&new), 0, "{}", err(&new));
    let written = fs::read_to_string(scratch.path().join("S-02.md")).unwrap();
    assert!(written.contains("properties:\n  rating:"), "{written}");
}

#[test]
fn a_unit_the_configuration_and_the_model_disagree_on_is_warned() {
    // Paths as Unix writes them, whatever the system said them with.
    let err = |output: &Output| slashed(err(output));
    let scratch = Scratch::new("model-unit");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n\n\
         [property.malt]\nunit = \"g\"\n",
    );
    scratch.write(
        "model/keg.py",
        "import samplekit as sk\n\n\
         class Keg(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.malt = sk.Property(unit=\"kg\")\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({"malt": entered(Some("kg"))}),
        serde_json::json!({}),
    );
    let set = at(scratch.path(), &["set", "S-01.md", "malt=4.2", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let said = err(&set);
    assert!(
        said.contains("'malt' is in g by .samplekitrc and in kg by"),
        "{said}"
    );
    assert!(said.contains("model/keg.py"), "{said}");
    let written = fs::read_to_string(scratch.path().join("S-01.md")).unwrap();
    assert!(written.contains("malt: {v: 4.2, unit: g}"), "{written}");
}

#[test]
fn a_name_the_model_declares_where_it_cannot_be_read_is_said() {
    // No environment to run the model: its description cannot be written, and
    // the name is written as the samples hold it, said.
    let scratch = Scratch::new("model-unread");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n",
    );
    scratch.write(
        "model/keg.py",
        "import samplekit as sk\n\n\
         class Keg(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.ibu = sk.Property(unit=\"mL\")\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    let set = at(scratch.path(), &["set", "S-01.md", "ibu=25.1", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let said = err(&set);
    assert!(
        said.contains("the model could not be read, so what it declares is not known"),
        "{said}"
    );
    assert!(said.contains("no environment found"), "{said}");
    assert!(!scratch.path().join(".samplekit/model.json").exists());
}

#[test]
fn new_refuses_as_usage_what_it_cannot_write() {
    let scratch = Scratch::new("new-usage");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  og: 1.05\ntables:\n  ferm:\n    \
         index: day\n    columns:\n      day: {}\n      gravity: {}\n    rows:\n      \
         - day: 1\n        gravity: 1.05\n---\n",
    );
    // A destination that is no sample's file.
    let text = at(
        scratch.path(),
        &["new", "b", "--like", "a.md", "--into", "b.txt"],
    );
    assert_eq!(code(&text), 1, "{}", err(&text));
    assert!(
        err(&text).contains("a sample's file ends in .md"),
        "{}",
        err(&text)
    );
    // A name already written, a value to keep that is none, a table to keep.
    let here = at(scratch.path(), &["new", "a", "--like", "a.md"]);
    assert_eq!(code(&here), 1, "{}", err(&here));
    for (kept, said) in [
        ("nosuch", "holds no value of that name"),
        ("ferm", "--keep takes values, and ferm is a table"),
    ] {
        let output = at(
            scratch.path(),
            &["new", "b", "--like", "a.md", "--keep", kept],
        );
        assert_eq!(code(&output), 1, "{kept}: {}", err(&output));
        assert!(err(&output).contains(said), "{kept}: {}", err(&output));
    }
    // Alone, the preview holds no empty section; and without a model it
    // offers no computation.
    let alone = at(scratch.path(), &["new", "c"]);
    assert!(!out(&alone).contains("\n\n\n"), "{:?}", out(&alone));
    let written = at(scratch.path(), &["new", "c", "--write"]);
    assert!(
        !out(&written).contains("samplekit compute"),
        "{}",
        out(&written)
    );
}

#[test]
fn a_mistyped_command_alone_exits_one_naming_the_nearest() {
    let scratch = Scratch::new("mistyped-alone");
    brews(&scratch);
    for (typed, meant) in [("lgo", "log"), ("resore", "restore")] {
        let output = at(scratch.path(), &[typed]);
        assert_eq!(code(&output), 1, "{typed}: {}", err(&output));
        assert!(
            err(&output).contains(&format!("did you mean '{meant}'?")),
            "{typed}: {}",
            err(&output)
        );
    }
    // Inside a command, a word is a file: the commands are not listed.
    let inside = at(scratch.path(), &["tag", "add", "x", "nosuch"]);
    assert_eq!(code(&inside), 3, "{}", err(&inside));
    assert!(!err(&inside).contains("nor a command"), "{}", err(&inside));
}

#[test]
#[cfg(unix)]
fn a_field_asked_of_files_that_could_not_be_read_exits_three() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("locked-field");
    let path = scratch.write(
        "locked/a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        // Run as root, nothing is refused: there is nothing to test.
        return;
    }
    let output = at(scratch.path(), &["locked", "-c", "malt"]);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("could not be read"),
        "{}",
        err(&output)
    );
    assert!(
        !err(&output).contains("name the folder"),
        "{}",
        err(&output)
    );
}

#[test]
fn set_says_what_a_quantity_or_a_row_cannot_take() {
    let scratch = Scratch::new("set-refusals");
    brews(&scratch);
    scratch.write(
        "tasted.md",
        "---\nschema_version: 1\nname: tasted\nproperties:\n  og: 1.05\ntables:\n  tasting:\n    \
         index: taster\n    columns:\n      taster: {}\n      score: {}\n    rows:\n      \
         - taster: Ivo\n        score: 40\n      - taster: Lea\n        score: 44\n  ferm:\n    \
         index: day\n    columns:\n      day: {}\n      gravity: {}\n      temp: {unit: degC}\n    \
         rows:\n      - day: 1\n        gravity: 1.05\n        temp: 18\n---\n",
    );
    let refused = |arguments: &[&str], said: &str| {
        let output = at(scratch.path(), arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains(said),
            "{arguments:?}: {}",
            err(&output)
        );
        err(&output)
    };
    refused(
        &["set", "tasted.md", "og=true"],
        "a yes-or-no, not a number",
    );
    refused(&["set", "tasted.md", "tags=a,b"], "samplekit tag add");
    refused(&["set", "tasted.md", "og.x=1"], "x is no channel of og");
    refused(
        &[
            "set",
            "tasted.md",
            "--add-row",
            "ferm",
            "day=2",
            "gravity=1,011",
        ],
        "write the number alone: gravity=1.011",
    );
    refused(
        &[
            "set",
            "tasted.md",
            "--add-row",
            "ferm",
            "day=2",
            "temp=18.1,18.3,18.2",
        ],
        "temp.readings=18.1,18.3,18.2",
    );
    let row = refused(
        &["set", "tasted.md", "tasting.score[lea]=45"],
        "did you mean 'Lea'?",
    );
    assert!(!row.contains("matched"), "{row}");
    refused(
        &[
            "set",
            "tasted.md",
            "--add-row",
            "ferm",
            "day=1",
            "gravity=1.04",
        ],
        "row 1 of the table",
    );
    let table = refused(
        &["set", "smoked-porter.md", "ferm.gravity[1]=1"],
        "unknown table",
    );
    assert!(!table.contains("available: \n"), "{table}");
}

#[test]
fn not_applicable_carries_no_uncertainty() {
    let scratch = Scratch::new("na-uncertainty");
    brews(&scratch);
    let output = at(
        scratch.path(),
        &["set", "citra-ipa.md", "og=n/a", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains("their mean"), "{}", out(&output));
    let csv = at(
        scratch.path(),
        &["citra-ipa.md", "-c", "name,og.u,og.readings", "--csv"],
    );
    let text = out(&csv);
    let row = text.lines().nth(1).unwrap_or_default();
    assert!(row.starts_with("citra-ipa,,"), "{text}");
    assert!(row.contains("1.063"), "the readings stay: {text}");
}

#[test]
fn help_examples_run_on_the_demo_as_written() {
    let scratch = Scratch::new("help-demo");
    brews(&scratch);
    let tag = at(scratch.path(), &["tag", "remove", "--help"]);
    assert!(
        !out(&tag).contains("medal brews/citra-ipa.md"),
        "{}",
        out(&tag)
    );
    let export = at(scratch.path(), &["export", "-h"]);
    assert!(
        out(&export).starts_with("Preview an export declared in .samplekitrc"),
        "{}",
        out(&export)
    );
    let open = at(scratch.path(), &["open", "--help"]);
    assert!(
        out(&open).contains("Where [collection] files is declared"),
        "{}",
        out(&open)
    );
    let tui = at(scratch.path(), &["tui", "--help"]);
    assert!(out(&tui).contains("Examples:"), "{}", out(&tui));
    assert!(out(&tui).contains("start page"), "{}", out(&tui));
    let set = at(scratch.path(), &["set", "--help"]);
    assert!(!out(&set).contains("conditioning"), "{}", out(&set));
}

#[test]
fn explain_says_each_thing_once_in_the_projects_spellings() {
    let scratch = Scratch::new("explain-once");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[unit.\"degC\"]\nplain = \"°C\"\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  t: {v: 65, unit: degC}\n  \
         drop:\n    v: 14.0\n    computed: {ferm.gravity: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n\
         tables:\n  ferm:\n    index: day\n    columns:\n      day: {}\n      gravity: {}\n      \
         temp: {unit: degC}\n    rows:\n      - day: 1\n        gravity: 1.05\n        temp: 18\n---\n",
    );
    let entered = out(&at(scratch.path(), &["explain", "b.md", "t"]));
    assert_eq!(entered.matches("entered").count(), 1, "{entered}");
    let table = out(&at(scratch.path(), &["explain", "b.md", "ferm"]));
    assert!(table.contains("°C"), "{table}");
    assert!(!table.contains("degC"), "{table}");
    let inputs = out(&at(scratch.path(), &["explain", "b.md", "drop"]));
    // Each input's name stands apart from its value.
    assert!(
        !inputs.contains("ferm.gravity1") && !inputs.contains("ferm.gravity 1"),
        "{inputs}"
    );
}

#[test]
fn a_file_written_is_said_written_and_from_here() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    let scratch = Scratch::new("written-here");
    scratch.corpus();
    let tagged = at(
        scratch.path(),
        &["tag", "add", "best", "keg-01.sample.md", "--write"],
    );
    assert!(out(&tagged).contains("written: 1 file"), "{}", out(&tagged));
    fs::create_dir_all(scratch.path().join("sub")).unwrap();
    let exported = at(
        &scratch.path().join("sub"),
        &["export", "platos", "..", "--write"],
    );
    assert_eq!(code(&exported), 0, "{}", err(&exported));
    assert!(
        err(&exported).contains("written: ../out/platos.csv"),
        "{}",
        err(&exported)
    );
}

#[test]
fn an_empty_list_says_it_is_empty() {
    let scratch = Scratch::new("empty-lists");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("a.md", "---\nschema_version: 1\nname: a\n---\n");
    assert_eq!(
        out(&at(scratch.path(), &["list", "tags"])),
        "no tags in use\n"
    );
    assert_eq!(
        out(&at(scratch.path(), &["list", "queries"])),
        "no query declared\n"
    );
    let validated = out(&at(scratch.path(), &["validate"]));
    assert!(!validated.starts_with('\n'), "{validated:?}");
    scratch.write("b.md", "---\nschema_version: 1\nname: [\n---\n");
    let skipped = at(scratch.path(), &["list", "skipped"]);
    assert!(!out(&skipped).contains("./"), "{}", out(&skipped));
    assert!(!err(&skipped).contains("./"), "{}", err(&skipped));
}

#[test]
fn overrides_are_said_one_way_and_a_narrow_status_keeps_its_state() {
    let scratch = Scratch::new("overrides-said");
    brews(&scratch);
    let set = at(scratch.path(), &["set", "citra-ipa.md", "abv=7", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let status = at(scratch.path(), &["status", "."]);
    assert!(
        out(&status).contains(
            "2 values edited by hand are kept — samplekit compute --force gives them back to \
             their formulas"
        ),
        "{}",
        out(&status)
    );
    let narrow = at(scratch.path(), &["status", ".", "--width", "30"]);
    assert!(out(&narrow).contains("state"), "{}", out(&narrow));
    assert!(!out(&narrow).contains("hidden"), "{}", out(&narrow));
    assert!(!out(&narrow).contains("-c"), "{}", out(&narrow));
}

#[test]
fn a_figure_no_sample_can_draw_is_the_datas_answer() {
    let scratch = Scratch::new("figure-data");
    brews(&scratch);
    // A named figure and one axis: the figure draws its own.
    let named = at(scratch.path(), &["plot", "strength", "-x", "og", "."]);
    assert_eq!(code(&named), 1, "{}", err(&named));
    assert!(
        err(&named).contains("a named figure draws its own axes"),
        "{}",
        err(&named)
    );
    // The one sample selected holds no abv: exit 2, said of that sample.
    let empty = at(
        scratch.path(),
        &[
            "plot",
            "-x",
            "og",
            "-y",
            "abv",
            "smoked-porter.md",
            "-o",
            "f.pdf",
        ],
    );
    assert_eq!(code(&empty), 2, "{}", err(&empty));
    assert!(
        err(&empty).contains("the one sample selected holds no abv"),
        "{}",
        err(&empty)
    );
}

#[test]
fn status_heads_its_answer_with_a_model_it_could_not_read() {
    // Under a table of stale values, the model that could not be read was a
    // warning after the advice; it heads the answer whatever is listed.
    let scratch = Scratch::new("status-unread-model");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"nosuch.py\"\n",
    );
    scratch.write(
        "drifted.md",
        "---\nschema_version: 1\nname: drifted\nproperties:\n  \
         malt: {v: 12.5}\n  volume:\n    v: 4.0\n    \
         computed: {malt: 000000000000}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["status", "."]);
    let said = out(&output);
    assert!(
        said.starts_with("the model could not be read for drifted"),
        "{said}"
    );
    assert!(said.contains("1 value is not current"), "{said}");
    assert!(said.contains("outdated"), "{said}");
}

#[test]
fn a_value_waiting_for_its_input_does_not_fail_the_gate() {
    // A value waiting for an input nobody entered is not yet a failure of
    // anything: `--exit-code` passes it without `--accept`, and still fails on
    // what is stale.
    let scratch = Scratch::new("status-waiting-gate");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let one = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::number(1.05).unwrap()),
        ..Default::default()
    });
    scratch.write(
        "porter.md",
        &format!(
            "---\nschema_version: 1\nname: porter\nproperties:\n  og: {{v: 1.05}}\n  \
             fg: {{unit: SG}}\n  abv: {{v: 5.0, computed: {{og: {one}, fg: 000000000000}}}}\n---\n"
        ),
    );
    let listed = at(scratch.path(), &["status", "."]);
    assert!(out(&listed).contains("waits for fg"), "{}", out(&listed));
    let gated = at(scratch.path(), &["status", ".", "--exit-code"]);
    assert_eq!(code(&gated), 0, "{}{}", out(&gated), err(&gated));
    // Something stale beside it still fails.
    scratch.write(
        "stout.md",
        "---\nschema_version: 1\nname: stout\nproperties:\n  malt: {v: 12.5}\n  \
         volume: {v: 4.0, computed: {malt: 000000000000}}\n---\n",
    );
    let stale = at(scratch.path(), &["status", ".", "--exit-code"]);
    assert_eq!(code(&stale), 2, "{}", out(&stale));
    assert!(
        err(&stale).contains("1 value is not current"),
        "{}",
        err(&stale)
    );
}

#[test]
fn a_tag_nobody_carries_leaves_everything_unchanged() {
    // Nothing to do is an answer: *unchanged*, exit 0, with the nearest tag.
    let scratch = Scratch::new("tag-nobody-unchanged");
    scratch.corpus();
    let before = fs::read_to_string(scratch.path().join("keg-01.sample.md")).unwrap();
    for arguments in [
        vec!["tag", "remove", "refrence", "--write"],
        vec!["tag", "rename", "refrence", "reference", "--write"],
        vec!["tag", "rename", "reference", "reference", "--write"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        assert!(
            out(&output).contains("unchanged"),
            "{arguments:?}: {}",
            out(&output)
        );
    }
    let removed = at(scratch.path(), &["tag", "remove", "refrence"]);
    assert!(
        out(&removed).contains("no sample carries 'refrence' — unchanged"),
        "{}",
        out(&removed)
    );
    assert!(out(&removed).contains("'reference'"), "{}", out(&removed));
    let after = fs::read_to_string(scratch.path().join("keg-01.sample.md")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn a_tag_differing_only_by_case_is_warned_about() {
    // `Reference` beside `reference` is a second spelling in the making; the
    // project's tags are compared, not only the file named.
    let scratch = Scratch::new("tag-case");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["tag", "add", "Reference", "keg-04.sample.md"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("'Reference' differs from the tag 'reference' only by case"),
        "{}",
        err(&output)
    );
    // Renaming to the other case is how the second spelling goes: no warning.
    let renamed = at(scratch.path(), &["tag", "rename", "reference", "Reference"]);
    assert!(!err(&renamed).contains("only by case"), "{}", err(&renamed));
}

#[test]
fn log_script_without_a_script_exits_one() {
    // What was asked for is not there: `log --script N > fix.py` must not leave
    // an empty file taken for a script. `python -c` is still said to have had
    // no script file to keep.
    let scratch = Scratch::new("script-none");
    scratch.corpus();
    let config =
        samplekit::config::project_config::load(&scratch.path().join(".samplekitrc")).unwrap();
    samplekit::config::version_control::snapshot(&config, "python -c · saved keg-01.sample.md")
        .unwrap();
    let inline = at(scratch.path(), &["log", "--script", "1"]);
    assert_eq!(code(&inline), 1, "{}", err(&inline));
    assert!(out(&inline).is_empty(), "{}", out(&inline));
    assert!(err(&inline).contains("no script file"), "{}", err(&inline));
    assert!(
        !err(&inline).contains("not made by a Python script"),
        "{}",
        err(&inline)
    );
    let tagged = at(scratch.path(), &["tag", "add", "x", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let command = at(scratch.path(), &["log", "--script", "1"]);
    assert_eq!(code(&command), 1, "{}", err(&command));
    assert!(
        err(&command).contains("not made by a Python script"),
        "{}",
        err(&command)
    );
}

#[test]
fn log_and_diff_of_a_written_file_point_to_explain() {
    // An export is kept as the record of how it was made, not as a file that
    // changes: *no snapshot changed what is named here* sent the reader
    // nowhere.
    let scratch = Scratch::new("log-export");
    scratch.corpus();
    let export = at(scratch.path(), &["export", "platos", "--write"]);
    assert_eq!(code(&export), 0, "{}", err(&export));
    for command in ["log", "diff"] {
        let output = at(scratch.path(), &[command, "out/platos.csv"]);
        assert_eq!(code(&output), 0, "{command}: {}", err(&output));
        assert!(
            out(&output).contains("samplekit explain out/platos.csv"),
            "{command}: {}",
            out(&output)
        );
        assert!(
            !out(&output).contains("no snapshot changed"),
            "{command}: {}",
            out(&output)
        );
    }
}

#[test]
fn restore_brings_a_deleted_file_back_as_last_kept() {
    // Deleted since the last snapshot, a file comes back with its last content
    // kept — not as the snapshot before its last edit — and the preview says
    // which.
    let scratch = Scratch::new("restore-deleted");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    fs::remove_file(scratch.path().join("keg-01.sample.md")).unwrap();
    let preview = at(scratch.path(), &["restore", "keg-01.sample.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("would restore to #1"),
        "{}",
        out(&preview)
    );
    assert!(
        out(&preview).contains("the last content kept of keg-01.sample.md, deleted since"),
        "{}",
        out(&preview)
    );
    let written = at(scratch.path(), &["restore", "keg-01.sample.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let back = fs::read_to_string(scratch.path().join("keg-01.sample.md")).unwrap();
    assert!(back.contains("status: kept"), "{back}");
}

#[test]
fn restore_takes_back_only_a_change_made_outside() {
    // Changed outside SampleKit since the last snapshot, a file goes back to
    // its last content kept — the edit SampleKit kept stays — and the preview
    // says which snapshot, and why.
    let scratch = Scratch::new("restore-outside");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["set", "keg-01.sample.md", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let path = scratch.path().join("keg-01.sample.md");
    let kept = fs::read_to_string(&path).unwrap();
    fs::write(&path, kept.replace("status: kept", "status: by-hand")).unwrap();
    let preview = at(scratch.path(), &["restore", "keg-01.sample.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("would restore to #1"),
        "{}",
        out(&preview)
    );
    assert!(
        out(&preview)
            .contains("the last content kept of keg-01.sample.md, changed outside SampleKit since"),
        "{}",
        out(&preview)
    );
    let written = at(scratch.path(), &["restore", "keg-01.sample.md", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(fs::read_to_string(&path).unwrap(), kept);
}

#[test]
fn restore_with_nothing_to_do_says_unchanged() {
    // No history, or nothing to take back: exit 0, *unchanged*.
    let scratch = Scratch::new("restore-unchanged");
    scratch.corpus();
    let none = at(scratch.path(), &["restore"]);
    assert_eq!(code(&none), 0, "{}", err(&none));
    assert!(out(&none).contains("— unchanged"), "{}", out(&none));
    let tagged = at(scratch.path(), &["tag", "add", "x", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let first = at(scratch.path(), &["restore", "--at", "1"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(out(&first).contains("— unchanged"), "{}", out(&first));
}

#[test]
fn a_preview_shows_a_value_at_its_declared_precision() {
    // As a table shows it, where the digits a computation leaves read as noise,
    // and every digit where nothing is declared; two values the precision
    // writes alike are said whole, since they are still a change.
    let scratch = Scratch::new("preview-digits");
    brews(&scratch);
    let output = at(
        scratch.path(),
        &["set", "citra-ipa.md", "abv=7", "volume=19.123456"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(said.contains("6.8  →  7.0"), "{said}");
    assert!(said.contains("19.5  →  19.123456"), "{said}");
    let hidden = at(scratch.path(), &["set", "citra-ipa.md", "abv=6.83"]);
    assert_eq!(code(&hidden), 0, "{}", err(&hidden));
    assert!(
        out(&hidden).contains("6.825000000000035  →  6.83"),
        "{}",
        out(&hidden)
    );
    let created = at(scratch.path(), &["new", "lager", "og=1.0456789"]);
    assert_eq!(code(&created), 0, "{}", err(&created));
    assert!(out(&created).contains("1.046"), "{}", out(&created));
    assert!(!out(&created).contains("1.0456789"), "{}", out(&created));
}

#[test]
fn a_blank_value_removes_the_value_and_says_so() {
    // `style=` and `style= ` remove the value, an edit said *removed*, where
    // `stout → —` read as a value nobody had given yet; on a sample without the
    // name there is nothing to remove.
    let scratch = Scratch::new("blank-removes");
    brews(&scratch);
    for blank in ["style=", "style=  "] {
        let output = at(scratch.path(), &["set", "citra-ipa.md", blank]);
        assert_eq!(code(&output), 0, "{}", err(&output));
        assert!(
            out(&output).contains("ipa  →  removed"),
            "{blank:?}: {}",
            out(&output)
        );
    }
    let written = at(
        scratch.path(),
        &["set", "citra-ipa.md", "style=", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(scratch.path().join("citra-ipa.md")).unwrap();
    assert!(!text.contains("style:"), "{text}");
    let absent = at(scratch.path(), &["set", "citra-ipa.md", "yeast="]);
    assert_eq!(code(&absent), 0, "{}", err(&absent));
    assert!(out(&absent).contains("unchanged"), "{}", out(&absent));
    assert!(!out(&absent).contains("removed"), "{}", out(&absent));
}

#[test]
fn new_refuses_a_name_a_command_line_would_split() {
    // Paths as Unix writes them, whatever the system said them with.
    let out = |output: &Output| slashed(out(output));
    let err = |output: &Output| slashed(err(output));
    // A name is typed after `samplekit set` and in filters: a space splits it,
    // a leading dash reads as an option. An empty one wrote `.md`.
    let scratch = Scratch::new("new-names-split");
    brews(&scratch);
    for name in ["", " "] {
        let output = at(scratch.path(), &["new", name, "--write"]);
        assert_eq!(code(&output), 1, "{}", out(&output));
        assert!(err(&output).contains("cannot be empty"), "{}", err(&output));
    }
    let spaced = at(scratch.path(), &["new", "pale ale", "--write"]);
    assert_eq!(code(&spaced), 1, "{}", out(&spaced));
    assert!(err(&spaced).contains("write pale-ale"), "{}", err(&spaced));
    assert!(!err(&spaced).contains("--into"), "{}", err(&spaced));
    let dashed = at(scratch.path(), &["new", "--write", "--", "-pale"]);
    assert_eq!(code(&dashed), 1, "{}", out(&dashed));
    assert!(
        err(&dashed).contains("begins with a dash"),
        "{}",
        err(&dashed)
    );
    assert!(!scratch.path().join("pale ale.md").exists());
    assert!(!scratch.path().join("-pale.md").exists());
    // A folder a shell would split is still quoted in what is suggested.
    fs::create_dir_all(scratch.path().join("my brews")).unwrap();
    let into = at(
        scratch.path(),
        &["new", "lager", "--into", "my brews", "--write"],
    );
    assert_eq!(code(&into), 0, "{}", err(&into));
    assert!(
        out(&into).contains("samplekit set 'my brews/lager.md' <field>=<value>"),
        "{}",
        out(&into)
    );
    // --keep copies from the sample --like names, and there is none.
    let kept = at(scratch.path(), &["new", "x", "--keep", "og"]);
    assert_eq!(code(&kept), 1, "{}", out(&kept));
    assert!(err(&kept).contains("--like"), "{}", err(&kept));
}

#[test]
fn restore_previews_at_the_declared_precision() {
    // `restore` previews as `set` does: at the project's precision, a change
    // the precision hides said whole, and every digit where none is declared;
    // `diff` says the same.
    let scratch = Scratch::new("restore-digits");
    brews(&scratch);
    let set = at(
        scratch.path(),
        &[
            "set",
            "smoked-porter.md",
            "og=1.0661234",
            "volume=20.123456",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let preview = at(scratch.path(), &["restore", "smoked-porter.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(said.contains("1.0661234  →  1.066"), "{said}");
    assert!(said.contains("20.123456 L  →  20 L"), "{said}");
    let compared = at(scratch.path(), &["diff", "smoked-porter.md"]);
    assert!(
        out(&compared).contains("1.066  →  1.0661234"),
        "{}",
        out(&compared)
    );
    assert!(!out(&compared).contains("records"), "{}", out(&compared));
    let moved = at(
        scratch.path(),
        &["set", "smoked-porter.md", "og=1.07", "--write"],
    );
    assert_eq!(code(&moved), 0, "{}", err(&moved));
    let preview = at(scratch.path(), &["restore", "smoked-porter.md"]);
    assert!(
        out(&preview).contains("1.070  →  1.066"),
        "{}",
        out(&preview)
    );
}

#[test]
fn a_sample_without_a_name_is_named_by_its_file() {
    // Its file's name without the extension, wherever a name is read: a column,
    // a filter, a sort; a name written wins.
    let scratch = Scratch::new("named-by-file");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "b-1.md",
        "---\nschema_version: 1\nproperties:\n  malt: {v: 2.0}\n---\n",
    );
    scratch.write(
        "a-2.md",
        "---\nschema_version: 1\nname: Alpha\nproperties:\n  malt: {v: 1.0}\n---\n",
    );
    let listed = at(scratch.path(), &["-c", "name,malt", "-s", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let said = out(&listed);
    let (first, second) = (said.find("Alpha"), said.find("b-1"));
    assert!(first.is_some() && second.is_some(), "{said}");
    assert!(first < second, "{said}");
    let filtered = at(scratch.path(), &["-c", "name,malt", "-f", "name == b-1"]);
    assert_eq!(code(&filtered), 0, "{}", err(&filtered));
    assert!(out(&filtered).contains("b-1"), "{}", out(&filtered));
    assert!(!out(&filtered).contains("Alpha"), "{}", out(&filtered));
}

#[test]
fn set_edits_the_name_in_the_file() {
    // `name` is an attribute a user edits: it changes what the file says, never
    // the file's name, and a blank one removes `name:`, the file's name
    // standing for it again.
    let scratch = Scratch::new("set-name");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s-1.md",
        "---\nschema_version: 1\nproperties:\n  malt: {v: 2.0}\n---\n",
    );
    let preview = at(scratch.path(), &["set", "s-1.md", "name=Cask A, batch 3"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("s-1  →  Cask A, batch 3"),
        "{}",
        out(&preview)
    );
    let written = at(
        scratch.path(),
        &["set", "s-1.md", "name=Cask A, batch 3", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(scratch.path().join("s-1.md")).unwrap();
    assert!(text.contains("name: \"Cask A, batch 3\""), "{text}");
    let removed = at(scratch.path(), &["set", "s-1.md", "name=", "--write"]);
    assert_eq!(code(&removed), 0, "{}", err(&removed));
    assert!(
        out(&removed).contains("Cask A, batch 3  →  s-1"),
        "{}",
        out(&removed)
    );
    assert!(
        out(&removed).contains("the file's name"),
        "{}",
        out(&removed)
    );
    let text = fs::read_to_string(scratch.path().join("s-1.md")).unwrap();
    assert!(!text.contains("name:"), "{text}");
    // Nothing to remove twice; a path and a line break are refused.
    let again = at(scratch.path(), &["set", "s-1.md", "name="]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(out(&again).contains("unchanged"), "{}", out(&again));
    for refused in ["name=a/b", "name=a\nb"] {
        let output = at(scratch.path(), &["set", "s-1.md", refused]);
        assert_eq!(code(&output), 1, "{refused}: {}", err(&output));
    }
    assert!(scratch.path().join("s-1.md").is_file());
}

#[test]
fn every_row_lookup_offers_the_nearest_row() {
    // `set` offered *did you mean 'Lea'?*; a filter, a column and `explain`
    // named the rows, or nothing.
    let scratch = Scratch::new("nearest-row");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  tasting:\n    index: taster\n    columns:\n      \
         taster: {}\n      score: {}\n    rows:\n      - {taster: Lea, score: 40}\n      \
         - {taster: Sam, score: 38}\n  fermentation:\n    index: day\n    columns:\n      \
         day: {}\n      gravity: {}\n    rows:\n      - {day: 1, gravity: 1.03}\n      \
         - {day: 4, gravity: 1.02}\n      - {day: 8, gravity: 1.01}\n---\n",
    );
    let column = at(scratch.path(), &[".", "-c", "name,tasting.score[Lee]"]);
    assert_eq!(code(&column), 1, "{}", err(&column));
    assert!(
        err(&column).contains("did you mean: 'tasting.score[Lea]'?"),
        "{}",
        err(&column)
    );
    assert!(
        err(&column).contains("the rows of 'tasting', by taster: Lea, Sam"),
        "{}",
        err(&column)
    );
    let filtered = at(scratch.path(), &[".", "-f", "tasting.score[Lee] > 1"]);
    assert_eq!(code(&filtered), 1, "{}", err(&filtered));
    assert!(
        err(&filtered).contains("did you mean Lea?"),
        "{}",
        err(&filtered)
    );
    let explained = at(scratch.path(), &["explain", "a.md", "tasting.score[Lee]"]);
    assert_eq!(code(&explained), 1, "{}", err(&explained));
    assert!(
        err(&explained).contains("did you mean: 'tasting.score[Lea]'?"),
        "{}",
        err(&explained)
    );
    // A number by proximity, a channel kept.
    let numbered = at(
        scratch.path(),
        &["explain", "a.md", "fermentation.gravity[5].u"],
    );
    assert!(
        err(&numbered).contains("did you mean: 'fermentation.gravity[4].u'?"),
        "{}",
        err(&numbered)
    );
}

#[test]
fn a_selection_of_incomplete_samples_keeps_its_value_columns() {
    // A filter does not change what exists, and a column is a quantity by the
    // collection, not the selection; an uncertainty column no selected sample
    // fills is left out.
    let scratch = Scratch::new("incomplete-selection-kept");
    scratch.corpus();
    scratch.write(
        "bare.sample.md",
        "---\nschema_version: 1\nname: bare\nstatus: approved\n---\nN.\n",
    );
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!(
            "{rc}\n[export.all]\nprofile = \"platos\"\nformat = \"csv\"\noutput = \"out/all.csv\"\n"
        ),
    );
    let only = "name == \"bare\"";
    let sorted = at(
        scratch.path(),
        &[".", "-c", "name,malt", "-s", "malt", "-f", only],
    );
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    assert!(out(&sorted).contains("bare"), "{}", out(&sorted));

    let narrowed = at(
        scratch.path(),
        &["export", "all", ".", "-o", "-", "-f", only],
    );
    assert_eq!(code(&narrowed), 0, "{}", err(&narrowed));
    let header = out(&narrowed)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(header.starts_with("malt_value [g],"), "{header}");
    assert!(header.contains("brix_value"), "{header}");
    assert!(!header.contains("uncertainty"), "{header}");
    assert_eq!(out(&narrowed).lines().count(), 2, "{}", out(&narrowed));

    let misspelt = at(
        scratch.path(),
        &[".", "-c", "name", "-s", "maltt", "-f", only],
    );
    assert_eq!(code(&misspelt), 1, "{}", err(&misspelt));
    assert!(
        err(&misspelt).contains("did you mean"),
        "{}",
        err(&misspelt)
    );
}

#[test]
fn an_empty_uncertainty_column_is_left_out_and_said() {
    // A column no sample can fill read as an uncertainty of zero, or as a
    // defect of the export: left out, and said on stderr.
    let scratch = Scratch::new("empty-uncertainty-left-out");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (file, name) in [("a.sample.md", "a"), ("b.sample.md", "b")] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  body: {{v: 3.0}}\n---\n"
            ),
        );
    }
    for format in ["--csv", "--tsv"] {
        let plain = at(scratch.path(), &[".", "-c", "name,body", format]);
        assert_eq!(code(&plain), 0, "{}", err(&plain));
        assert!(
            !out(&plain).lines().next().unwrap().contains("uncertainty"),
            "{}",
            out(&plain)
        );
        assert!(
            err(&plain).contains("body_uncertainty is left out"),
            "{}",
            err(&plain)
        );
    }
    // One sample filling it keeps the column.
    scratch.write(
        "c.sample.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  body: {v: 3.0, u: 0.1}\n---\n",
    );
    let kept = at(scratch.path(), &[".", "-c", "name,body", "--csv"]);
    assert_eq!(
        out(&kept).lines().next().unwrap(),
        "name,body_value,body_uncertainty"
    );
}

#[test]
fn a_value_not_applicable_is_exported_as_written() {
    // `n/a` written, shown, read as none by a filter, apart in a summary; a
    // data file writes it `n/a`, where an empty field read as a value nobody
    // measured.
    let scratch = Scratch::new("not-applicable-exported");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["set", "keg-02.sample.md", "malt=n/a", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = fs::read_to_string(scratch.path().join("keg-02.sample.md")).unwrap();
    assert!(written.contains("n/a"), "{written}");
    let table = out(&at(scratch.path(), &[".", "-c", "name,malt"]));
    assert!(
        table
            .lines()
            .any(|line| line.contains("keg-02") && line.contains("n/a")),
        "{table}"
    );
    let heavy = out(&at(scratch.path(), &[".", "-f", "malt > 0", "-c", "name"]));
    assert!(!heavy.contains("keg-02"), "{heavy}");
    let summary = out(&at(scratch.path(), &[".", "-c", "malt", "--summary"]));
    assert!(summary.contains("3/3 (1 n/a)"), "{summary}");
    let csv = out(&at(scratch.path(), &[".", "-c", "name,malt", "--csv"]));
    assert!(csv.contains("keg-02,n/a,\n"), "{csv}");
    let tsv = out(&at(scratch.path(), &[".", "-c", "name,malt", "--tsv"]));
    assert!(tsv.contains("keg-02\tn/a\t\n"), "{tsv}");
    // JSON keeps null: a string where numbers go breaks every reader of it.
    let json = out(&at(scratch.path(), &[".", "-c", "name,malt", "--json"]));
    assert!(!json.contains("\"n/a\""), "{json}");
}

#[test]
fn an_implicit_export_takes_any_format_it_is_told() {
    // `--csv -o g.json` is a redirection, not a declared destination: the
    // format named is written under the name given.
    let scratch = Scratch::new("implicit-redirection");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "-c", "name,malt", "--csv", "-o", "g.json", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("g.json")).unwrap();
    assert!(written.starts_with("name,malt_value [g],"), "{written}");
}

#[test]
fn an_export_is_current_by_the_fields_it_writes() {
    // `set grain_mass=6.2` made an overview holding no grain_mass *not
    // current*: an export is judged by the fields it writes and by what they
    // are computed from.
    let scratch = Scratch::new("export-fields-current");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.light]\ncolumns = [{field = \"name\"}, {field = \"malt\"}, \
         {field = \"brix\"}]\n[export.light]\nprofile = \"light\"\nformat = \"csv\"\n\
         output = \"light.csv\"\n",
    );
    let volume = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::number(4.0).unwrap()),
        ..Default::default()
    });
    scratch.write(
        "a.md",
        &format!(
            "---\nschema_version: 1\nname: a\nstyle: ipa\nproperties:\n  malt: {{v: 12.0}}\n  \
             volume: {{v: 4.0}}\n  brix: {{v: 3.0, computed: {{volume: {volume}}}}}\n---\n"
        ),
    );
    let export = at(scratch.path(), &["export", "light", "--write"]);
    assert_eq!(code(&export), 0, "{}", err(&export));
    let unrelated = at(scratch.path(), &["set", "a.md", "style=stout", "--write"]);
    assert_eq!(code(&unrelated), 0, "{}", err(&unrelated));
    let current = at(scratch.path(), &["explain", "light.csv"]);
    assert_eq!(code(&current), 0, "{}", err(&current));
    assert!(out(&current).contains("up to date"), "{}", out(&current));
    assert!(!out(&current).contains("style"), "{}", out(&current));
    // `brix` is written, and computed from `volume`.
    let input = at(scratch.path(), &["set", "a.md", "volume=5", "--write"]);
    assert_eq!(code(&input), 0, "{}", err(&input));
    let behind = at(scratch.path(), &["explain", "light.csv"]);
    assert!(out(&behind).contains("volume"), "{}", out(&behind));
    assert!(out(&behind).contains("not current"), "{}", out(&behind));
}

#[test]
fn a_filter_leaves_aside_a_sample_whose_field_holds_text() {
    // One sample's `og` written `high` refused `og > 1.06` over the whole
    // collection: it is left aside and named on stderr, exit 0.
    let scratch = Scratch::new("filter-text-aside");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, og) in [("a", "1.05"), ("b", "high"), ("c", "1.07")] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  og: {og}\n---\n"),
        );
    }
    let output = at(scratch.path(), &[".", "-f", "og > 1.06", "-c", "name"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains('c'), "{}", out(&output));
    assert!(!out(&output).contains('b'), "{}", out(&output));
    assert!(
        err(&output).contains("b left aside: 'og' holds text there, which > cannot compare"),
        "{}",
        err(&output)
    );
}

#[test]
fn validate_of_files_judges_them_alone() {
    // `validate FILE` judged the configuration's queries against the one file
    // given, and a field its siblings hold was unknown there.
    let scratch = Scratch::new("validate-alone");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!("{rc}[query.bad]\nfilter = \"nosuch > 1\"\n"),
    );
    for targets in [
        vec!["validate", "keg-01.sample.md"],
        vec!["validate", "keg-01.sample.md", "keg-02.sample.md"],
    ] {
        let output = at(scratch.path(), &targets);
        assert_eq!(code(&output), 0, "{targets:?}: {}", out(&output));
        assert!(!out(&output).contains("query 'bad'"), "{}", out(&output));
    }
    let whole = at(scratch.path(), &["validate", "."]);
    assert!(out(&whole).contains("query 'bad'"), "{}", out(&whole));
}

#[test]
fn columns_over_several_projects_are_one_table_per_project() {
    // One table per project, headed by it and in the order given, each sorted
    // by `-s` within, as `--profile` draws them.
    let scratch = Scratch::new("tables-per-project");
    two_projects(&scratch);
    let output = at(
        scratch.path(),
        &[
            "nested",
            ".",
            "-c",
            "name,malt",
            "-s",
            "-malt",
            "--width",
            "120",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.starts_with("nested\n"), "{text}");
    assert!(text.contains("\n.\n"), "{text}");
    assert_eq!(text.matches("name").count(), 2, "{text}");
    assert!(!text.contains("project"), "{text}");
    assert!(!text.contains(".samplekitrc"), "{text}");
    assert!(
        text.find("nested-one").unwrap() < text.find("root-one").unwrap(),
        "{text}"
    );
}

#[test]
fn a_profile_over_several_projects_heads_each_table_with_it() {
    // Each table under its project's name, at its profile's precision.
    let scratch = Scratch::new("profile-projects-headed");
    two_projects(&scratch);
    let output = at(scratch.path(), &[".", "--profile", "p", "--width", "120"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("3.000") && !text.contains("2.000"), "{text}");
    assert!(text.contains("nested\n"), "{text}");
    assert!(!text.contains("project"), "{text}");
    assert!(!text.contains(".samplekitrc"), "{text}");
}

#[test]
fn a_summary_writes_csv_and_json_with_its_weights() {
    // A data format says beside the mean how it was taken: `1/u²` where it is
    // weighted.
    let scratch = Scratch::new("summary-formats-weights");
    scratch.corpus();
    let csv = at(scratch.path(), &[".", "-c", "malt", "--summary", "--csv"]);
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert_eq!(
        csv_header(&out(&csv)),
        "column,unit,n,of,mean,weights,s,sem,median,min,max"
    );
    assert!(out(&csv).contains(",1/u²,"), "{}", out(&csv));
    let json = at(scratch.path(), &[".", "-c", "malt", "--summary", "--json"]);
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(out(&json).contains("\"median\""), "{}", out(&json));
    assert!(out(&json).contains("\"weights\""), "{}", out(&json));
}

#[test]
fn a_summary_weights_its_mean_and_says_so() {
    // Every malt carries its uncertainty: the mean is weighted by 1/u², and the
    // header says so; a column without them keeps the plain mean.
    let scratch = Scratch::new("summary-weighted");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, malt, u) in [("a", 1.0, 0.1), ("b", 2.0, 0.2)] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {{v: {malt}, u: {u}}}\n  \
                 volume: {malt}\n---\n"
            ),
        );
    }
    let both = out(&at(scratch.path(), &[".", "-c", "malt", "--summary"]));
    assert!(both.contains("mean (1/u²)"), "{both}");
    assert!(both.contains("1.2"), "{both}");
    let mixed = out(&at(
        scratch.path(),
        &[".", "-c", "malt,volume", "--summary"],
    ));
    assert!(mixed.contains("mean (1/u²: malt)"), "{mixed}");
    let plain = out(&at(scratch.path(), &[".", "-c", "volume", "--summary"]));
    assert!(!plain.contains("1/u²"), "{plain}");
}

#[test]
fn a_summary_is_grouped_by_several_fields() {
    // `--group status,name` summarises each combination, a column per field; a
    // field nobody holds is refused as a column's is, where it grouped every
    // sample under `—`.
    let scratch = Scratch::new("summary-groups");
    scratch.corpus();
    let csv = out(&at(
        scratch.path(),
        &[
            ".",
            "-c",
            "malt",
            "--summary",
            "--group",
            "status,name",
            "--csv",
        ],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,column,"), "{csv}");
    assert!(lines[1].starts_with("approved,keg-01,malt,"), "{csv}");
    assert!(lines[4].starts_with("rejected,keg-04,malt,"), "{csv}");
    let table = out(&at(
        scratch.path(),
        &[".", "-c", "malt", "--summary", "--group", "status,name"],
    ));
    assert!(
        table.contains("status") && table.contains("name"),
        "{table}"
    );
    let unknown = at(
        scratch.path(),
        &[".", "-c", "malt", "--summary", "--group", "status,nosuch"],
    );
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(
        err(&unknown).contains("unknown field 'nosuch'"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn a_table_is_one_table_per_group_headed_by_its_values() {
    // `--group` splits a table as several projects split one, each headed by
    // its values, the count said once under the last.
    let scratch = Scratch::new("table-groups");
    scratch.corpus();
    scratch.write(
        "keg-05.sample.md",
        "---\nschema_version: 1\nname: keg-05\nproperties:\n  \
         malt: {v: 8.0, u: 0.05, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-c", "malt", "--group", "status", "--width", "120"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let approved = text.find("status = approved\n").expect(&text);
    let rejected = text.find("status = rejected\n").expect(&text);
    let none = text.find("status = (none)\n").expect(&text);
    assert!(approved < rejected && rejected < none, "{text}");
    assert!(text[approved..rejected].contains("keg-03"), "{text}");
    assert!(!text[approved..rejected].contains("keg-04"), "{text}");
    assert!(text[rejected..none].contains("keg-04"), "{text}");
    assert!(text[none..].contains("keg-05"), "{text}");
    assert_eq!(text.matches(" of 5 samples").count(), 1, "{text}");
    // Several fields: a table per combination.
    let both = out(&at(
        scratch.path(),
        &[".", "-c", "malt", "--group", "status,name"],
    ));
    assert!(
        both.contains("status = approved · name = keg-02\n"),
        "{both}"
    );
    assert_eq!(both.matches("status = approved").count(), 3, "{both}");
    let unknown = at(scratch.path(), &[".", "-c", "malt", "--group", "statuss"]);
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(
        err(&unknown).contains("unknown field 'statuss'") && err(&unknown).contains("status"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn a_grouped_data_format_is_one_table_carrying_its_groups() {
    // A file has no headings; its groups are a column, their rows together.
    let scratch = Scratch::new("csv-groups");
    scratch.corpus();
    scratch.sample("keg-00.sample.md", "keg-00", "rejected", 9.0, None);
    let csv = out(&at(
        scratch.path(),
        &[".", "-c", "name,malt", "--group", "status", "--csv"],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,"), "{csv}");
    assert_eq!(lines.len(), 6, "{csv}");
    assert!(
        lines[1..4].iter().all(|line| line.starts_with("approved,")),
        "{csv}"
    );
    assert!(
        lines[4..].iter().all(|line| line.starts_with("rejected,")),
        "{csv}"
    );
    // Named among the columns, it is not repeated.
    let named = out(&at(
        scratch.path(),
        &[".", "-c", "status,malt", "--group", "status", "--csv"],
    ));
    assert_eq!(csv_header(&named).matches("status").count(), 1, "{named}");
}

#[test]
fn groups_are_made_within_each_project() {
    // The projects first, as a table heads them, and the groups within.
    let scratch = Scratch::new("groups-per-project");
    two_projects(&scratch);
    scratch.sample("b.sample.md", "root-two", "rejected", 1.0, None);
    let output = at(
        scratch.path(),
        &["nested", ".", "-c", "name,malt", "--group", "status"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let nested = text.find("nested · status = approved\n").expect(&text);
    let approved = text.find(". · status = approved\n").expect(&text);
    let rejected = text.find(". · status = rejected\n").expect(&text);
    assert!(nested < approved && approved < rejected, "{text}");
    assert!(text[rejected..].contains("root-two"), "{text}");
}

fn csv_header(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

// ------------------------------------------------ readings as a channel

#[test]
fn readings_are_a_channel_of_set_new_and_add_row() {
    let scratch = Scratch::new("readings-channel");
    brews(&scratch);
    scratch.write(
        "fermented.md",
        "---\nschema_version: 1\nname: fermented\nproperties:\n  og: 1.05\ntables:\n  ferm:\n    \
         index: day\n    columns:\n      day: {}\n      temp: {unit: degC}\n    rows:\n      \
         - day: 1\n        temp: 18\n---\n",
    );
    // A property's readings, in brackets, beside a value in one command.
    let set = at(
        scratch.path(),
        &[
            "set",
            "smoked-porter.md",
            "og.readings=[1.066, 1.067]",
            "abv=5",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(out(&set).contains("· 2 changes"), "{}", out(&set));
    let written = fs::read_to_string(scratch.path().join("smoked-porter.md")).unwrap();
    assert!(written.contains("readings: [1.066, 1.067]"), "{written}");
    // The value typed before stays beside them.
    assert!(written.contains("v: 1.066, readings"), "{written}");
    // A cell of a row that exists: its value stays beside them.
    let cell = at(
        scratch.path(),
        &[
            "set",
            "fermented.md",
            "ferm.temp[1].readings=18,19",
            "--write",
        ],
    );
    assert_eq!(code(&cell), 0, "{}", err(&cell));
    let written = fs::read_to_string(scratch.path().join("fermented.md")).unwrap();
    assert!(
        written.contains("temp: {v: 18, readings: [18.0, 19.0]"),
        "{written}"
    );
    // A cell of a new row: readings alone, and no mean beside them.
    let row = at(
        scratch.path(),
        &[
            "set",
            "fermented.md",
            "--add-row",
            "ferm",
            "day=2",
            "temp.readings=17.5,18.5",
            "--write",
        ],
    );
    assert_eq!(code(&row), 0, "{}", err(&row));
    assert!(
        out(&row).contains("temp.readings = [17.5, 18.5]"),
        "{}",
        out(&row)
    );
    let written = fs::read_to_string(scratch.path().join("fermented.md")).unwrap();
    assert!(
        written.contains("temp: {readings: [17.5, 18.5]}"),
        "{written}"
    );
    // A new sample: readings with no statistic recorded give no value, said
    // once for both.
    let new = at(
        scratch.path(),
        &[
            "new",
            "pilsner",
            "og.readings=1.048,1.049",
            "fg.readings=[1.010, 1.011]",
            "--write",
        ],
    );
    assert_eq!(code(&new), 0, "{}", err(&new));
    assert!(
        out(&new).contains("og, fg have no statistic recorded"),
        "{}",
        out(&new)
    );
    let written = fs::read_to_string(scratch.path().join("pilsner.md")).unwrap();
    assert!(
        written.contains("og: {readings: [1.048, 1.049]}"),
        "{written}"
    );
    assert!(
        written.contains("fg: {readings: [1.01, 1.011]}"),
        "{written}"
    );
    assert!(!written.contains("v:"), "no mean is written: {written}");
}

#[test]
fn the_readings_flag_is_an_unknown_option() {
    // A retired option is an unknown option, refused as any option a command
    // does not take, with no message of its own.
    let scratch = Scratch::new("readings-flag");
    brews(&scratch);
    let before = fs::read_to_string(scratch.path().join("citra-ipa.md")).unwrap();
    let output = at(
        scratch.path(),
        &[
            "set",
            "citra-ipa.md",
            "--readings",
            "fg=1.011,1.010",
            "--write",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("unexpected argument '--readings'"),
        "{}",
        err(&output)
    );
    assert!(!err(&output).contains("retired"), "{}", err(&output));
    assert_eq!(
        fs::read_to_string(scratch.path().join("citra-ipa.md")).unwrap(),
        before
    );
}

#[test]
fn readings_like_decimal_commas_are_warned_about() {
    let scratch = Scratch::new("readings-commas");
    brews(&scratch);
    let comma = at(
        scratch.path(),
        &["set", "smoked-porter.md", "og.readings=20,5,21,5"],
    );
    assert_eq!(code(&comma), 0, "{}", err(&comma));
    assert!(
        err(&comma).contains("og.readings=[20.5, 21.5]"),
        "{}",
        err(&comma)
    );
    assert!(out(&comma).contains("4 readings"), "{}", out(&comma));
    let points = at(
        scratch.path(),
        &["set", "smoked-porter.md", "og.readings=1.05,1.06"],
    );
    assert_eq!(code(&points), 0, "{}", err(&points));
    assert!(err(&points).is_empty(), "{}", err(&points));
}

#[test]
fn state_selects_as_status_and_validate_say() {
    // `state` is a field every filter reads, and a column.
    let scratch = Scratch::new("state-field");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a-failed.md",
        "---\nschema_version: 1\nname: a-failed\nproperties:\n  malt: {v: 2.0}\n  \
         volume: {computed: {malt: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    scratch.write(
        "b-defective.md",
        "---\nschema_version: 1\nname: b-defective\nyeast_strain: US05\nyeast_strain: S04\n\
         properties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    scratch.write(
        "c-clean.md",
        "---\nschema_version: 1\nname: c-clean\nproperties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    let names = |arguments: &[&str]| -> Vec<String> {
        let output = at(scratch.path(), arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        out(&output)
            .lines()
            .map(|line| {
                line.trim_start_matches("./")
                    .trim_start_matches(".\\")
                    .to_string()
            })
            .collect()
    };
    assert_eq!(names(&["-f", "state == failed"]), ["a-failed.md"]);
    assert_eq!(names(&["-f", "state == defective"]), ["b-defective.md"]);
    assert_eq!(names(&["-f", "state == not_current"]), ["a-failed.md"]);
    // A project declaring no model owes nothing: a clean sample is current.
    assert_eq!(
        names(&["-f", "state == current"]),
        ["b-defective.md", "c-clean.md"]
    );
    assert!(names(&["-f", "state == failed && state == defective"]).is_empty());
    let shown = out(&at(scratch.path(), &["-c", "name,state", "--csv"]));
    assert!(shown.contains("a-failed,failed"), "{shown}");
    assert!(shown.contains("c-clean,current"), "{shown}");

    // A model that cannot be read: a word only it could deny selects
    // nothing, and the reason is said.
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"missing.py\"\n",
    );
    let unread = at(scratch.path(), &["-f", "state == never_computed"]);
    assert_eq!(code(&unread), 0, "{}", err(&unread));
    assert!(out(&unread).trim().is_empty(), "{}", out(&unread));
    assert!(
        err(&unread).contains("the model was not read for 3 samples"),
        "{}",
        err(&unread)
    );
    // What the files say is still answered.
    assert_eq!(names(&["-f", "state == failed"]), ["a-failed.md"]);
}

#[test]
fn sorting_by_state_says_a_sample_holds_several() {
    // The list's advice, `[#n]`, named an item `state` does not have; the
    // refusal says why and what answers instead, as Python's does.
    let scratch = Scratch::new("sort-state");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    for arguments in [
        vec!["--sort", "state"],
        vec!["-c", "name,state", "-s", "name,-state"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", err(&output));
        let said = err(&output);
        assert!(said.contains("several states"), "{said}");
        assert!(said.contains("-f 'state == failed'"), "{said}");
        assert!(said.contains("-c name,state"), "{said}");
        assert!(!said.contains("[#n]"), "{said}");
    }
}

#[test]
fn log_export_writes_an_account_to_share() {
    // The history stays on each machine; an account of it is shared.
    let scratch = Scratch::new("log-export");
    with_history(&scratch);
    let config =
        samplekit::config::project_config::load(&scratch.path().join(".samplekitrc")).unwrap();
    samplekit::config::version_control::script_step(
        &config,
        "python fix_it.py",
        Some(&("fix_it.py".to_string(), b"print(1)\n".to_vec())),
    )
    .unwrap();
    let account = at(scratch.path(), &["log", "--export"]);
    assert_eq!(code(&account), 0, "{}", err(&account));
    let text = out(&account);
    assert!(text.starts_with("# History of "), "{text}");
    assert!(text.contains("5 snapshots, newest first"), "{text}");
    assert!(text.contains("## #1 · "), "{text}");
    assert!(text.contains("Script: fix\\_it.py"), "{text}");
    assert!(text.contains("## #2 · "), "{text}");
    // Every file, where `log` counts past three.
    for file in [
        "- keg-01.sample.md",
        "- keg-04.sample.md",
        "- .samplekitrc",
        "- keg-09.md",
    ] {
        assert!(text.contains(file), "{file}: {text}");
    }
    // A target and `--at` narrow it as they narrow `log`.
    let one = out(&at(
        scratch.path(),
        &["log", "--export", "keg-01.sample.md"],
    ));
    assert!(
        one.contains("## #4 · ") && one.contains("## #5 · "),
        "{one}"
    );
    assert!(!one.contains("## #3 · "), "{one}");
    let since = out(&at(scratch.path(), &["log", "--export", "--at", "4"]));
    assert!(
        since.contains("## #4 · ") && !since.contains("## #3 · "),
        "{since}"
    );
    // A file previewed, then written.
    let preview = at(scratch.path(), &["log", "--export", "-o", "HISTORY.md"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(!scratch.path().join("HISTORY.md").exists());
    let written = at(
        scratch.path(),
        &["log", "--export", "-o", "HISTORY.md", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(
        fs::read_to_string(scratch.path().join("HISTORY.md")).unwrap(),
        text
    );
}

// ------------------------------------------ several machines

/// `samplekit ARGS` in `directory`, as the machine `machine`.
fn as_machine(directory: &Path, machine: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(arguments)
        .current_dir(directory)
        .env("COLUMNS", "200")
        .env("SAMPLEKIT_MACHINE", machine)
        .output()
        .expect("the binary runs")
}

/// Two samples and no model.
fn two_brews(scratch: &Scratch, folder: &str) -> PathBuf {
    scratch.write(&format!("{folder}/.samplekitrc"), "schema_version = 1\n");
    for (name, malt) in [("a", "1.0"), ("b", "2.0")] {
        scratch.write(
            &format!("{folder}/{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {malt}\n---\n"),
        );
    }
    scratch.path().join(folder)
}

#[test]
fn log_names_each_machine_and_where_they_joined() {
    let scratch = Scratch::new("log-machines");
    let root = two_brews(&scratch, "p");
    // One machine: no column.
    let alone = as_machine(&root, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    assert_eq!(code(&alone), 0, "{}", err(&alone));
    let log = out(&as_machine(&root, "ana", &["log"]));
    assert!(!log.contains("machine"), "{log}");
    // Two, taking turns on one folder: the files always hold the other's.
    for (machine, change) in [("tom", "b.md malt=2.5"), ("ana", "a.md malt=1.7")] {
        let mut arguments = vec!["set"];
        arguments.extend(change.split(' '));
        arguments.push("--write");
        let written = as_machine(&root, machine, &arguments);
        assert_eq!(code(&written), 0, "{}", err(&written));
    }
    let log = as_machine(&root, "tom", &["log"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    let log = out(&log);
    let lines: Vec<&str> = log.lines().collect();
    assert!(
        lines[0].contains("when") && lines[0].contains("machine") && lines[0].contains("what"),
        "{log}"
    );
    let row = |number: &str| {
        lines
            .iter()
            .find(|line| line.starts_with(&format!("{number} ")))
            .copied()
            .unwrap_or_default()
    };
    assert!(row("1").contains("ana, joins tom"), "{log}");
    assert!(
        row("1").contains("samplekit set a.md malt=1.7 --write"),
        "{log}"
    );
    assert!(row("2").contains("tom, joins ana"), "{log}");
    assert!(
        row("3").contains("ana ") && !row("3").contains("joins"),
        "{log}"
    );
    assert!(!log.contains("changed outside SampleKit"), "{log}");
    // `diff` alone shows the join's own change, not what it joined.
    let diff = out(&as_machine(&root, "ana", &["diff"]));
    assert!(diff.contains("with #2 of tom"), "{diff}");
    assert!(diff.contains("1.5  →  1.7"), "{diff}");
    assert!(!diff.contains("b.md"), "{diff}");
    // The account names the machines.
    let account = out(&as_machine(&root, "ana", &["log", "--export"]));
    assert!(
        account.contains("Kept on 2 machines, ana and tom"),
        "{account}"
    );
    assert!(
        account.contains("## #1 · ") && account.contains(" · ana\n"),
        "{account}"
    );
    assert!(account.contains("Joins the snapshots of tom."), "{account}");
}

// On Windows the second machine does not yet join the first's history: an open
// question, to be found on a Windows machine.
#[test]
#[cfg(unix)]
fn a_history_shared_through_git_joins_after_a_pull() {
    let scratch = Scratch::new("history-git");
    let git = |directory: &Path, arguments: &[&str]| {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let remote = scratch.path().join("remote.git");
    fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare"]);
    let ana = two_brews(&scratch, "ana");
    git(&ana, &["init", "-q"]);
    let written = as_machine(&ana, "ana", &["set", "a.md", "malt=1.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    // The one line: the history no longer hides itself from git.
    fs::write(ana.join(".samplekit/history/.gitignore"), ".tmp*\n*.lock\n").unwrap();
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana"]);
    git(&ana, &["push", "-q", remote.to_str().unwrap(), "HEAD:main"]);
    git(
        scratch.path(),
        &["clone", "-q", "-b", "main", remote.to_str().unwrap(), "tom"],
    );
    let tom = scratch.path().join("tom");
    let written = as_machine(&tom, "tom", &["set", "b.md", "malt=2.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    git(&tom, &["add", "-A"]);
    git(&tom, &["commit", "-qm", "tom"]);
    git(&tom, &["push", "-q", "origin", "HEAD:main"]);
    // Ana wrote meanwhile; the pull merges without a conflict.
    let written = as_machine(&ana, "ana", &["set", "a.md", "malt=1.6", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana again"]);
    git(
        &ana,
        &[
            "pull",
            "-q",
            "--no-rebase",
            remote.to_str().unwrap(),
            "main",
        ],
    );
    let written = as_machine(&ana, "ana", &["set", "a.md", "malt=1.7", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let log = out(&as_machine(&ana, "ana", &["log"]));
    assert!(
        log.lines()
            .nth(2)
            .is_some_and(|line| line.contains("ana, joins tom")),
        "{log}"
    );
    assert!(log.contains("tom, joins ana"), "{log}");
    assert!(!log.contains("changed outside SampleKit"), "{log}");
}

// ------------------------------------------------ a profile's groups

/// The corpus, under a configuration whose profiles group.
fn grouping_profiles(scratch: &Scratch) {
    scratch.corpus();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\
         [profile.by_status]\ncolumns = [{field = \"name\"}, {field = \"malt\"}]\n\
         group = [\"status\"]\n\
         [profile.lightest]\ncolumns = [{field = \"name\"}, {field = \"malt\"}]\n\
         sort = [\"malt\"]\ngroup = [\"status\"]\n\
         [export.by_status]\nprofile = \"by_status\"\nformat = \"csv\"\noutput = \"-\"\n",
    );
}

#[test]
fn a_profile_groups_its_table_and_group_replaces_its_groups() {
    // A profile declaring `group` prints a table per group, as `--group` does;
    // `--group` beside it replaces them, as `-s` its sort.
    let scratch = Scratch::new("profile-groups");
    grouping_profiles(&scratch);
    let output = at(
        scratch.path(),
        &[".", "--profile", "by_status", "--width", "120"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let approved = text.find("status = approved\n").expect(&text);
    let rejected = text.find("status = rejected\n").expect(&text);
    assert!(approved < rejected, "{text}");
    assert!(text[rejected..].contains("keg-04"), "{text}");
    let replaced = out(&at(
        scratch.path(),
        &[".", "--profile", "by_status", "--group", "name"],
    ));
    assert!(replaced.contains("name = keg-01\n"), "{replaced}");
    assert!(!replaced.contains("status = "), "{replaced}");
    let csv = out(&at(
        scratch.path(),
        &[".", "--profile", "by_status", "--csv"],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,"), "{csv}");
    assert!(lines[4].starts_with("rejected,keg-04"), "{csv}");
}

#[test]
fn an_export_whose_profile_groups_writes_one_table_carrying_its_groups() {
    // One table, as a grouped `--csv` is.
    let scratch = Scratch::new("export-groups");
    grouping_profiles(&scratch);
    let output = at(scratch.path(), &["export", "by_status", "."]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let csv = out(&output);
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,"), "{csv}");
    assert!(
        lines[1..4].iter().all(|line| line.starts_with("approved,")),
        "{csv}"
    );
    assert!(lines[4].starts_with("rejected,keg-04"), "{csv}");
}

#[test]
fn the_groups_follow_the_sort_in_force() {
    // With a sort in force the groups come where their first samples come;
    // without one, by their values.
    let scratch = Scratch::new("groups-sorted");
    grouping_profiles(&scratch);
    let heaviest = out(&at(
        scratch.path(),
        &[".", "-c", "malt", "--group", "name", "-s", "-malt"],
    ));
    let third = heaviest.find("name = keg-03\n").expect(&heaviest);
    let first = heaviest.find("name = keg-01\n").expect(&heaviest);
    assert!(third < first, "{heaviest}");
    // By another field: keg-04, the lightest, is rejected.
    let lightest = out(&at(
        scratch.path(),
        &[".", "-c", "name,malt", "--group", "status", "-s", "malt"],
    ));
    let rejected = lightest.find("status = rejected\n").expect(&lightest);
    let approved = lightest.find("status = approved\n").expect(&lightest);
    assert!(rejected < approved, "{lightest}");
    // No sort: the values' order.
    let unsorted = out(&at(
        scratch.path(),
        &[".", "-c", "name,malt", "--group", "status"],
    ));
    assert!(
        unsorted.find("status = approved\n") < unsorted.find("status = rejected\n"),
        "{unsorted}"
    );
    // A profile's sort orders its groups the same.
    let profile = out(&at(scratch.path(), &[".", "--profile", "lightest"]));
    assert!(
        profile.find("status = rejected\n") < profile.find("status = approved\n"),
        "{profile}"
    );
    // And a summary per group.
    let summary = out(&at(
        scratch.path(),
        &[
            ".",
            "-c",
            "malt",
            "--summary",
            "--group",
            "status",
            "-s",
            "malt",
            "--csv",
        ],
    ));
    let rows: Vec<&str> = summary.lines().collect();
    assert!(rows[1].starts_with("rejected,"), "{summary}");
    assert!(rows[2].starts_with("approved,"), "{summary}");
}

#[test]
fn profiles_grouping_differently_over_two_projects_are_refused() {
    // The groups are made over the whole selection, and one selection has one
    // grouping: which profile would win is not guessed.
    let scratch = Scratch::new("groups-two-projects");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.p]\ncolumns = [{field = \"name\"}]\ngroup = [\"status\"]\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}]\n",
    );
    scratch.sample("nested/n.sample.md", "nested-one", "approved", 3.0, None);
    let refused = at(scratch.path(), &[".", "--profile", "p"]);
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    let said = err(&refused);
    assert!(
        said.contains("group differently")
            && said.contains("by status")
            && said.contains("by nothing"),
        "{said}"
    );
    assert!(said.contains("--group"), "{said}");
    let chosen = at(
        scratch.path(),
        &[".", "--profile", "p", "--group", "status"],
    );
    assert_eq!(code(&chosen), 0, "{}", err(&chosen));
    assert!(
        out(&chosen).contains("status = approved"),
        "{}",
        out(&chosen)
    );
}
