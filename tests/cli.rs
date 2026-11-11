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

    /// One slab: a diameter, a ductility, a status and a tag.
    fn sample(
        &self,
        file: &str,
        name: &str,
        status: &str,
        diameter: f64,
        porosity: Option<f64>,
    ) -> PathBuf {
        let mut body = format!(
            "---\nschema_version: 1\nname: {name}\ntags: [reference]\nstatus: {status}\n\
             properties:\n  diameter: {{v: {diameter}, u: 0.05, unit: g}}\n"
        );
        if let Some(porosity) = porosity {
            body.push_str(&format!(
                "  porosity: {{v: {porosity}, u: 0.0012, unit: g/L}}\n"
            ));
        }
        body.push_str(&format!("---\nMetals notes for {name}.\n"));
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
             [property.diameter]\n\
             precision = \".2f\"\n\
             [property.porosity]\n\
             symbol = \"Bx\"\n\
             symbol_math = \"\\\\porosity\"\n\
             precision = \".4f\"\n\
             [style.math]\n\
             property = '{value} \\pm {uncertainty} {unit}'\n\
             [query.approved]\n\
             filter = 'status == \"approved\"'\n\
             [profile.ductilities]\n\
             columns = [{field = \"diameter\"}, {field = \"porosity\", label = \"Ductility\"}]\n\
             sort = [\"-porosity\", \"diameter\"]\n\
             [export.ductilities]\n\
             profile = \"ductilities\"\n\
             format = \"csv\"\n\
             output = \"out/ductilities.csv\"\n",
        )
    }

    /// Three approved slabs and one rejected, plus the project.
    fn corpus(&self) -> &Scratch {
        self.config();
        fs::create_dir_all(self.0.join("out")).unwrap();
        self.sample(
            "billet-01.sample.md",
            "billet-01",
            "approved",
            12.1,
            Some(2.801),
        );
        self.sample(
            "billet-02.sample.md",
            "billet-02",
            "approved",
            12.2,
            Some(2.802),
        );
        self.sample(
            "billet-03.sample.md",
            "billet-03",
            "approved",
            12.3,
            Some(2.803),
        );
        self.sample("billet-04.sample.md", "billet-04", "rejected", 9.7, None);
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
    // Each answer typed once its question is there, as a person types it:
    // what is typed before a question is no answer to it, and is dropped.
    let collect = |mut stream: Box<dyn std::io::Read + Send>| {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let held = std::sync::Arc::clone(&seen);
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
    for answer in answers.split_inclusive('\n') {
        // The question asked: output that has stopped growing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut last = usize::MAX;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let now = stdout.lock().unwrap().len();
            if now == last || std::time::Instant::now() > deadline {
                break;
            }
            last = now;
        }
        if stdin.write_all(answer.as_bytes()).is_err() {
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

/// What the binary printed, its relative paths with `/` whatever the system,
/// for a test that reads a path and not its separator.
fn out_slashed(output: &Output) -> String {
    slashed(out(output))
}

fn err_slashed(output: &Output) -> String {
    slashed(err(output))
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
        "---\nschema_version: 1\nname: RO\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let mode = |bits: u32| {
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };

    mode(0o000);
    let unread = at(scratch.path(), &["ro.md", "-c", "diameter"]);
    mode(0o644);
    assert_eq!(code(&unread), 3, "{}", err(&unread));
    let said = err(&unread);
    // Named once, and told nothing about its schema: nothing was read, so
    // nothing is known about the contents.
    assert_eq!(said.matches("ro.md").count(), 1, "{said}");
    assert!(!said.contains("predates schema 1"), "{said}");

    mode(0o444);
    let refused = at(scratch.path(), &["ro.md", "set", "diameter=9", "--write"]);
    mode(0o644);
    assert_eq!(code(&refused), 3, "{}", err(&refused));

    // Met while scanning a directory, the same refusal exits the same.
    mode(0o000);
    let scanned = at(scratch.path(), &[".", "status"]);
    mode(0o644);
    assert_eq!(code(&scanned), 3, "{}", err(&scanned));

    // A file that will not parse is still a data error, with its way out.
    scratch.write("bad.md", "not a sample\n");
    let malformed = at(scratch.path(), &["bad.md", "-c", "diameter"]);
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

    let named = at(scratch.path(), &["a.md", "-c", "diameter"]);
    assert_eq!(code(&named), 3, "{}", err(&named));
    assert!(
        err(&named).to_lowercase().contains("symbolic link"),
        "{}",
        err(&named)
    );
    // The scan already said it correctly; the two now agree.
    let scanned = at(scratch.path(), &[".", "list", "skipped"]);
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
    let plain = at(scratch.path(), &["-c", "name,diameter", "--csv", "."]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(!out(&plain).contains("state"), "{}", out(&plain));

    let asked = at(
        scratch.path(),
        &["-c", "name,diameter", "--csv", "--status", "."],
    );
    assert_eq!(code(&asked), 0, "{}", err(&asked));
    let shown = out(&asked);
    let header = shown.lines().next().unwrap();
    assert!(header.ends_with(",state"), "{shown}");
    assert!(!header.contains("diameter_state"), "{shown}");
    // A word, never a glyph: a ✗ in a CSV is something a parser reads as text.
    for glyph in ["⚠", "✎", "✗"] {
        assert!(!shown.contains(glyph), "{shown}");
    }
    // An entered diameter needs nothing: the row is current.
    assert!(
        shown.lines().nth(1).unwrap().ends_with(",current"),
        "{shown}"
    );

    // A row with something to act on says what, worst first, and which.
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: drifted\nproperties:\n  \
         diameter: {v: 2.0, unit: g, fingerprint: aaaaaaaaaaaa}\n  \
         porosity: {v: 1.0, computed: {diameter: bbbbbbbbbbbb}}\n  \
         volume: {v: 2.0, computed: {diameter: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    let drifted = at(
        scratch.path(),
        &[
            "-c",
            "name,porosity,volume",
            "--csv",
            "--status",
            "drifted.sample.md",
        ],
    );
    assert!(
        out(&drifted).contains(",failed: volume; outdated: porosity"),
        "{}",
        out(&drifted)
    );
    // The same column on a drawn table, piped, where the marks are lost —
    // the flag used to do nothing there.
    let piped = at(
        scratch.path(),
        &[
            "-c",
            "name,porosity,volume",
            "--status",
            "drifted.sample.md",
        ],
    );
    assert_eq!(code(&piped), 0, "{}", err(&piped));
    assert!(out(&piped).contains("state"), "{}", out(&piped));
    assert!(
        out(&piped).contains("failed: volume; outdated: porosity"),
        "{}",
        out(&piped)
    );
    // JSON already carries each quantity's, so the flag adds nothing there.
    let json = at(
        scratch.path(),
        &["-c", "name,diameter", "--json", "--status", "."],
    );
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(out(&json).contains("\"state\""), "{}", out(&json));
    assert_eq!(
        out(&json),
        out(&at(scratch.path(), &["-c", "name,diameter", "--json", "."]))
    );
}

/// A column whose samples do not agree on a unit is refused, not shown with a
/// header that names none.
///
/// Before, the table printed `diameter` over `1` and `2` where one was kilograms:
/// two incomparable numbers under a word that promises nothing.
#[test]
fn a_column_that_cannot_name_its_unit_is_refused() {
    let scratch = Scratch::new("mixed-units");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (file, name, unit) in [("a.md", "A", "g"), ("b.md", "B", "kg")] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: 1.0, unit: {unit}}}\n---\n"
            ),
        );
    }
    let refused = at(scratch.path(), &["-c", "diameter", "."]);
    assert_eq!(code(&refused), 2, "{}", out(&refused));
    let said = err(&refused);
    assert!(said.contains("cannot say which unit"), "{said}");
    assert!(said.contains("g and kg"), "{said}");
    // And one unit, or none at all, still draws.
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let shown = at(scratch.path(), &["-c", "diameter", "."]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert!(out(&shown).contains("diameter [g]"), "{}", out(&shown));
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
    let whole = at(scratch.path(), &[".", "list", "skipped"]);
    assert_eq!(code(&whole), 0, "{}", err(&whole));
    assert!(out(&whole).contains("bad1499.md"), "the report was cut");
}

fn table_corpus(scratch: &Scratch) {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "item.sample.md",
        "---\nschema_version: 1\nname: item\nproperties:\n  diameter: 2.4\n  wear: {v: 2.014, readings: [2.011, 2.017, 2.013, 2.015]}\ntables:\n  measurements:\n    index: [T]\n    columns:\n      T: {}\n      ph: {}\n      hrc: {}\n    rows:\n      - {T: 20, ph: 4.2, hrc: 0.1}\n      - {T: 30, ph: 4.8, hrc: 0.2}\n---\n",
    );
}

/// The order the rows came out in, by slab name.
fn order(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with("billet-"))
                .map(str::to_string)
        })
        .collect()
}

// ------------------------------------------------------- the target grammar

#[test]
fn query_options_without_a_path_use_the_current_directory() {
    let scratch = Scratch::new("no-path");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-c", "name,diameter", "-f", "diameter > 12.15"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["billet-02", "billet-03"]);
}

#[test]
fn file_and_directory_are_interchangeable() {
    // Every path-taking command accepts either.
    let scratch = Scratch::new("either");
    scratch.corpus();
    for arguments in [
        vec![".", "list"],
        vec!["billet-01.sample.md", "list"],
        vec![".", "validate"],
        vec!["billet-01.sample.md", "validate"],
        vec![".", "status"],
        vec!["billet-01.sample.md", "status"],
        vec!["."],
        vec!["billet-01.sample.md"],
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
    let output = at(
        scratch.path(),
        &["billet-01.sample.md", "-c", "name,diameter"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["billet-01"]);
}

#[test]
fn a_profile_is_explicit() {
    let scratch = Scratch::new("single");
    scratch.corpus();
    let shape = at(scratch.path(), &["--profile", "ductilities"]);
    assert_eq!(code(&shape), 0, "{}", err(&shape));
    assert!(out(&shape).contains("Ductility"), "{}", out(&shape));
}

#[test]
fn a_profile_name_that_is_a_path_is_unambiguous() {
    let scratch = Scratch::new("ambiguous");
    scratch.corpus();
    fs::create_dir_all(scratch.path().join("ductilities")).unwrap();
    let profile = at(scratch.path(), &["--profile", "ductilities"]);
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    let path = at(scratch.path(), &["ductilities"]);
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
    scratch.sample("sub/one.sample.md", "billet-07", "approved", 5.0, Some(2.5));
    let output = at(&scratch.path().join("sub"), &["-c", "porosity"]);
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
    let output = at(scratch.path(), &["-c", "diameter", "-f", "diameter > 9000"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(order(&out(&output)).is_empty(), "{}", out(&output));
}

#[test]
fn unknown_field_exits_one() {
    // And prints a suggestion: the question was malformed.
    let scratch = Scratch::new("unknown-field");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "diameterr"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("diameterr"), "{message}");
    assert!(message.contains("diameter"), "{message}");
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
    let output = at(scratch.path(), &["broken.sample.md", "-c", "diameter"]);
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
    let output = at(scratch.path(), &["-c", "diameterr"]);
    assert!(out(&output).is_empty(), "{}", out(&output));
    assert!(!err(&output).is_empty());
}

#[test]
fn warnings_do_not_change_the_exit_code() {
    // A skipped file still exits 0, and says so on stderr.
    let scratch = Scratch::new("warnings");
    scratch.corpus();
    scratch.write("notes.md", "Just prose, no frontmatter.\n");
    let output = at(scratch.path(), &["-c", "diameter", "-v"]);
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
            // The path first, then the command.
            page.lines()
                .any(|line| line.contains("samplekit ") && line.contains(&format!(" {verb} "))),
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
        vec!["list", "-s", "diameter"],
        vec!["validate", "--csv"],
        vec!["billet-01.sample.md", "explain", "diameter", "--overwrite"],
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
        .args(["--", "samplekit", "--profile", "d"])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("ductilities"), "{}", out(&output));
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
fn set_completes_a_link_with_the_samples_of_the_project() {
    let scratch = Scratch::new("complete-link");
    scratch.corpus();
    let complete = |word: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["--", "samplekit", "billet-01.sample.md", "set", word])
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "3")
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        assert_eq!(code(&output), 0, "{}", err(&output));
        out(&output)
    };
    let offered = complete("twin=[[billet-0");
    assert!(offered.contains("twin=[[billet-02]]"), "{offered}");
    // A file name, `.sample` taken off as a wikilink names it.
    assert!(!offered.contains(".sample]]"), "{offered}");
    // After a link closed, the next one of a list.
    let next = complete("parents=[[billet-02]],[[billet-0");
    assert!(
        next.contains("parents=[[billet-02]],[[billet-03]]"),
        "{next}"
    );
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
        "---\nschema_version: 1\nname: A\ntags: [reference, dry_treated]\n---\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: B\ntags: [reference, high_hardness]\n---\n",
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
        for tag in ["reference", "dry_treated", "high_hardness"] {
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
        out(&output).lines().any(|line| line == "dry_treated"),
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
    assert!(first.lines().any(|line| line == "diameter"), "{first}");
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

    let exact = out(&complete("wear"));
    assert!(exact.contains("wear.v"), "{exact}");
    assert!(exact.contains("wear.stats:statistics"), "{exact}");

    let stored = out(&complete("diameter"));
    assert!(!stored.contains("diameter.stats"), "{stored}");

    let stats = out(&complete("wear.stats."));
    assert!(stats.contains("wear.stats.mean"), "{stats}");
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
    for current in ["diameter,", "diameter,measurements.ph[20],"] {
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
    for current in ["diameter,", "diameter,-"] {
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
fn the_path_may_come_before_the_command() {
    // Typed first to be completed from, the path was refused: *'status' is a
    // subcommand and must come before the target*. It is the command's target.
    let scratch = Scratch::new("path-first");
    scratch.corpus();
    let first = at(scratch.path(), &[".", "status"]);
    let after = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&first), code(&after), "{}", err_slashed(&first));
    assert_eq!(out_slashed(&first), out_slashed(&after));
    // After the names a command takes first.
    let exported = at(scratch.path(), &[".", "export", "ductilities"]);
    assert_eq!(code(&exported), 0, "{}", err_slashed(&exported));
    assert!(
        out_slashed(&exported).contains("out/ductilities.csv"),
        "{}",
        out_slashed(&exported)
    );
    // A folder named as a command is written `./`.
    fs::create_dir_all(scratch.path().join("status")).unwrap();
    let folder = at(scratch.path(), &["./status", "-c", "name"]);
    assert_eq!(code(&folder), 0, "{}", err_slashed(&folder));
}

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
            "diameter > 12.15",
            "-c",
            "name,diameter",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["billet-02", "billet-03"]);
}

#[test]
fn repeated_filters_are_all_conjoined() {
    let scratch = Scratch::new("repeat-filter");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-f", "diameter > 12.05", "-f", "diameter < 12.25"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(out(&output).lines().count(), 2, "{}", out(&output));
}

#[test]
fn unknown_query_lists_the_available_ones() {
    let scratch = Scratch::new("unknown-query");
    scratch.corpus();
    let output = at(scratch.path(), &["--query", "aproved", "-c", "diameter"]);
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
        &[".", "--query", "approved", "-c", "name,diameter"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        order(&out(&output)),
        ["billet-01", "billet-02", "billet-03"]
    );
}

#[test]
fn a_malformed_filter_points_at_the_character() {
    // The caret is the message: a position on its own is a number.
    let scratch = Scratch::new("caret");
    scratch.corpus();
    let output = at(scratch.path(), &["-f", "diameter >", "-c", "diameter"]);
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
        &["-f", "diameterr > 1 and porosityy < 3", "-c", "diameter"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    let message = err(&output);
    assert!(message.contains("diameterr"), "{message}");
    assert!(message.contains("porosityy"), "{message}");
}

// --------------------------------------------------------- columns and order

#[test]
fn repeated_sort_flags_make_successive_keys() {
    let scratch = Scratch::new("bundled");
    scratch.config();
    scratch.sample("a.sample.md", "billet-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "billet-02", "approved", 12.2, Some(2.8));
    let output = at(
        scratch.path(),
        &["-s", "porosity", "-s", "-diameter", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(order(&out(&output)), ["billet-02", "billet-01"]);
}

#[test]
fn standalone_sort_takes_its_own_value() {
    // `-s c -p a,b` sorts by `c`: the two cases are told apart syntactically,
    // so nothing is guessed.
    let scratch = Scratch::new("standalone");
    scratch.corpus();
    let output = at(scratch.path(), &["-s", "-diameter", "-c", "name,porosity"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        order(&out(&output)),
        ["billet-03", "billet-02", "billet-01", "billet-04"]
    );
}

#[test]
fn sort_takes_a_comma_list_like_columns() {
    let scratch = Scratch::new("sort-comma-list");
    scratch.config();
    scratch.sample("a.sample.md", "billet-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "billet-02", "approved", 12.2, Some(2.8));
    let listed = at(scratch.path(), &["-s", "porosity,-diameter", "-c", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let repeated = at(
        scratch.path(),
        &["-s", "porosity", "-s", "-diameter", "-c", "name"],
    );
    assert_eq!(out(&listed), out(&repeated));
    assert_eq!(order(&out(&listed)), ["billet-02", "billet-01"]);
}

#[test]
fn an_empty_sort_key_is_refused() {
    let scratch = Scratch::new("sort-empty-key");
    scratch.corpus();
    let output = at(scratch.path(), &["-s", "porosity,", "-c", "name"]);
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
    let comma = at(scratch.path(), &["-c", "diameter,porosity"]);
    let repeated = at(scratch.path(), &["-c", "diameter", "-c", "porosity"]);
    assert_eq!(code(&comma), 0, "{}", err(&comma));
    assert_eq!(out(&comma), out(&repeated));
}

#[test]
fn properties_never_produces_a_literal_comma_column() {
    // The regression test: v1 documented `-p a -p b` and silently accepted
    // `-p a,b` as one field called `a,b`.
    let scratch = Scratch::new("comma-column");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "diameter,porosity"]);
    assert!(
        !out(&output).contains("diameter,porosity"),
        "{}",
        out(&output)
    );
    assert!(err(&output).is_empty(), "{}", err(&output));
}

#[test]
fn an_unknown_column_is_refused_before_anything_is_rendered() {
    // No column of dashes, ever: this is the v1 defect the project exists to
    // remove. Nothing is printed at all.
    let scratch = Scratch::new("no-dashes");
    scratch.corpus();
    let output = at(scratch.path(), &["-c", "porosity,vfy"]);
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
        &["-c", "tag,diametr", "--table-style", "plain"],
        "\n\n",
    );
    assert_eq!(code(&output), 0, "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("unknown field 'tag'; use 'tags'? [Y/n]"),
        "{text}"
    );
    assert!(
        text.contains("unknown field 'diametr'; use 'diameter'? [Y/n]"),
        "{text}"
    );
    assert!(text.contains("tags"), "{text}");
    assert!(text.contains("diameter"), "{text}");
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
        assert!(message.contains("columns  ph  hrc"), "{message}");
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
            "name,porosity.v,porosity.u,porosity.unit,porosity.symbol",
            "--table-style",
            "plain",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let first = text
        .lines()
        .find(|line| line.contains("billet-01"))
        .unwrap();
    assert!(!first.contains('±'), "{first}");
    for expected in ["2.8010", "0.0012", "g/L", "Bx"] {
        assert!(first.contains(expected), "missing {expected}: {first}");
    }
    let header = text.lines().next().unwrap();
    assert!(!header.contains("porosity.unit ["), "{header}");
    assert!(!header.contains("porosity.symbol ["), "{header}");
}

#[test]
fn an_empty_selection_still_checks_columns_against_the_collection() {
    // A vocabulary belongs to a collection, not to a filter's result: checking
    // the selection turns a well-formed question with an empty answer into a
    // false error, and exit 1 where the project promises 0.
    let scratch = Scratch::new("empty-vocabulary");
    scratch.corpus();
    let known = at(scratch.path(), &["-f", "diameter > 9000", "-c", "porosity"]);
    assert_eq!(code(&known), 0, "{}", err(&known));
    assert!(err(&known).is_empty(), "{}", err(&known));
    // And an unknown column is still unknown when nothing matched.
    let unknown = at(
        scratch.path(),
        &["-f", "diameter > 9000", "-c", "porosityy"],
    );
    assert_eq!(code(&unknown), 1, "{}", out(&unknown));
}

#[test]
fn reverse_flips_every_key() {
    // Including a profile's tie-break: a flag that reversed the primary key and
    // left the rest alone would produce an order nobody asked for.
    let scratch = Scratch::new("reverse");
    scratch.config();
    // Two slabs share a ductility and differ in diameter, so the profile's
    // second key decides.
    scratch.sample("a.sample.md", "billet-01", "approved", 12.1, Some(2.8));
    scratch.sample("b.sample.md", "billet-02", "approved", 12.2, Some(2.8));
    let plain = out(&at(scratch.path(), &["--profile", "ductilities"]));
    let reversed = out(&at(scratch.path(), &["--profile", "ductilities", "-r"]));
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
    let output = at(scratch.path(), &["-c", "diameter", "-r"]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("--sort"), "{}", err(&output));
    assert!(out(&output).is_empty(), "{}", out(&output));
    // With a key, it reverses it.
    let sorted = at(scratch.path(), &["-c", "diameter", "-s", "diameter", "-r"]);
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    // An export's profile declares its order, and -r reverses that one.
    let plain = at(scratch.path(), &["export", "ductilities", "-o", "-"]);
    let reversed = at(scratch.path(), &["export", "ductilities", "-r", "-o", "-"]);
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
    let first = at(scratch.path(), &["--profile", "ductilities", "--csv"]);
    let second = at(scratch.path(), &["--profile", "ductilities", "--csv"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert_eq!(out(&first), out(&second));
    assert!(out(&first).starts_with("diameter_value"), "{}", out(&first));
}

#[test]
fn name_is_never_an_implicit_table_column() {
    let scratch = Scratch::new("explicit-name");
    scratch.corpus();
    let without = out(&at(scratch.path(), &["-c", "diameter", "--csv"]));
    assert!(!without.contains("name"), "{without}");
    assert!(!without.contains("billet-01"), "{without}");
    let with = out(&at(scratch.path(), &["-c", "name,diameter", "--csv"]));
    assert!(with.starts_with("name,"), "{with}");
    assert!(with.contains("billet-01"), "{with}");
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
        &["-c", "name,diameter", "--csv", "-o", "data.csv"],
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
        &["-c", "name,diameter", "--csv", "-o", "data.csv", "--write"],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert_eq!(out(&first).trim(), "written: data.csv", "{}", out(&first));
    let before = fs::read_to_string(scratch.path().join("data.csv")).unwrap();

    // The second preview says the file is there, and leaves it alone.
    let again = at(scratch.path(), &["-c", "name", "--csv", "-o", "data.csv"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("the file is there, and is replaced"),
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
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
    let output = at(scratch.path(), &["-c", "porosity", "--csv"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(!text.contains('\u{2014}'), "{text}");
    assert!(!text.contains("billet-04"), "{text}");
    assert!(text.lines().any(|line| line.starts_with(',')), "{text}");
}

#[test]
fn the_table_style_comes_from_the_project_and_the_flag_wins() {
    // An argument beats a stored setting, which beats the default.
    let scratch = Scratch::new("style");
    scratch.corpus();
    let stored = at(scratch.path(), &["-c", "diameter"]);
    assert!(out(&stored).contains('\u{250c}'), "{}", out(&stored));
    let overridden = at(
        scratch.path(),
        &["-c", "diameter", "--table-style", "plain"],
    );
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

    let plain = at(scratch.path(), &["-c", "porosity,porosity.symbol"]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(out(&plain).contains("g/L"), "{}", out(&plain));
    assert!(out(&plain).contains("Bx"), "{}", out(&plain));

    let math = at(
        scratch.path(),
        &["-c", "porosity,porosity.symbol", "--style", "math"],
    );
    assert_eq!(code(&math), 0, "{}", err(&math));
    let text = out(&math);
    assert!(text.contains("\\mathrm{g\\,L^{-1}}"), "{text}");
    assert!(text.contains("\\pm"), "{text}");
    assert!(text.contains("\\porosity"), "{text}");

    let view = at(scratch.path(), &[".", "view", "--style", "math"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(out(&view).contains("\\pm"), "{}", out(&view));
    assert!(
        out(&view).contains("\\mathrm{g\\,L^{-1}}"),
        "{}",
        out(&view)
    );

    let explain = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "explain",
            "porosity",
            "--style",
            "math",
        ],
    );
    assert_eq!(code(&explain), 0, "{}", err(&explain));
    assert!(out(&explain).contains("\\pm"), "{}", out(&explain));

    // The transient override never changes the stored project preference.
    let again = at(scratch.path(), &["-c", "porosity,porosity.symbol"]);
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
    // The styles SampleKit gives, then the project's.
    assert_eq!(
        out(&output).lines().collect::<Vec<_>>(),
        ["plain", "html", "latex", "markdown", "math"]
    );
}

#[test]
fn global_precision_overrides_declared_presentation() {
    let scratch = Scratch::new("global-precision");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-c", "diameter,porosity", "--precision", ".3f"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    // `diameter` stores .2f and porosity is declared .4f; the explicit invocation wins.
    assert!(text.contains("12.100 ± 0.050"), "{text}");
    assert!(text.contains("2.801 ± 0.002"), "{text}");

    let profile = at(
        scratch.path(),
        &["--profile", "ductilities", "--precision", ".3f"],
    );
    assert_eq!(code(&profile), 0, "{}", err(&profile));
    assert!(
        out(&profile).contains("12.100 ± 0.050"),
        "{}",
        out(&profile)
    );

    let view = at(scratch.path(), &[".", "view", "--precision", ".3f"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(out(&view).contains("12.100 ± 0.050"), "{}", out(&view));
}

#[test]
fn column_precision_overrides_global_precision() {
    let scratch = Scratch::new("column-precision");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-c", "diameter:.1f,porosity", "--precision", ".3f"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("12.1 ± 0.1"), "{text}");
    assert!(text.contains("2.801 ± 0.002"), "{text}");
}

#[test]
fn an_ad_hoc_column_carries_its_label_and_python_style_precision() {
    let scratch = Scratch::new("column-declaration");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["-c", "diameter:.3f=Diameter: precise,porosity=Ductility"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("Diameter: precise [g]"), "{text}");
    assert!(text.contains("Ductility [g/L]"), "{text}");
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5}\ntables:\n  preheating:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      tension: {unit: lintner}\n    \
         rows:\n      - {temperature: 78.5, tension: 104.2}\n---\nN.\n",
    );
    let output = at(scratch.path(), &["list", "fields"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("properties (1)"), "{text}");
    assert!(text.contains("diameter"), "{text}");
    // The table once: its index, its columns, an address to copy.
    assert!(text.contains("preheating"), "{text}");
    assert!(text.contains("index    temperature: 78.5"), "{text}");
    assert!(text.contains("columns  tension"), "{text}");
    assert!(text.contains("'preheating.tension[78.5]'"), "{text}");
}

#[test]
fn list_makes_the_file_unnecessary() {
    // Every field a query can use appears — the friction this command exists
    // to remove is opening a `.md` in an editor to learn a column's name.
    let scratch = Scratch::new("list-all");
    scratch.corpus();
    scratch.write(
        "with-table.sample.md",
        "---\nschema_version: 1\nname: billet-05\nproperties:\n  \
         diameter: {v: 12.5}\ntables:\n  preheating:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      tension: {unit: lintner}\n    \
         rows:\n      - {temperature: 78.5, tension: 104.2}\n      \
         - {temperature: 303.15, tension: 108.9}\n---\nN.\n",
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
    assert!(!text.contains("\ndiameter\n"), "{text}");
    assert!(!text.contains("  diameter"), "{text}");
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
        "---\nschema_version: 1\nname: billet-05\ntables:\n  preheating:\n    \
         index: [temperature]\n    columns:\n      temperature: {}\n      tension: {}\n    \
         rows:\n      - {temperature: 78.5, tension: 104.2}\n      \
         - {temperature: 303.15, tension: 108.9}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &["list", "fields"]));
    assert!(text.contains("diameter"), "{text}");
    assert!(!text.contains("diameter.v"), "{text}");
    assert!(!text.contains("diameter.u"), "{text}");
    // Two rows, and still no line per cell.
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("preheating.tension[")),
        "{text}"
    );
    assert!(
        text.contains("index    temperature: 78.5  303.15"),
        "{text}"
    );
}

#[test]
fn list_fields_gives_the_index_values_in_order() {
    // In the order met, a row only one sample held came after every other
    // sample's: `24` after `44` on the demo.
    let scratch = Scratch::new("index-in-order");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let table = |rows: &str| {
        format!(
            "---\nschema_version: 1\ntables:\n  curve:\n    index: strain\n    columns:\n      \
             strain: {{}}\n      force: {{}}\n    rows:\n{rows}---\n"
        )
    };
    scratch.write(
        "a.md",
        &table("      - {strain: 1, force: 1.0}\n      - {strain: 10, force: 2.0}\n      - {strain: 2, force: 3.0}\n"),
    );
    scratch.write("b.md", &table("      - {strain: 5, force: 4.0}\n"));
    let text = out(&at(scratch.path(), &["list", "fields"]));
    assert!(text.contains("index    strain: 1  2  5  10\n"), "{text}");
    // Text as a sort orders it, whatever its case: `apple` before `Zed`.
    scratch.write(
        "c.md",
        "---\nschema_version: 1\ntables:\n  grips:\n    index: kind\n    columns:\n      \
         kind: {}\n      n: {}\n    rows:\n      - {kind: Zed, n: 1}\n      - {kind: apple, n: 2}\n---\n",
    );
    let text = out(&at(scratch.path(), &["list", "fields"]));
    assert!(text.contains("index    kind: apple  Zed\n"), "{text}");
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
    let one = at(scratch.path(), &["billet-01.sample.md", "list", "fields"]);
    assert_eq!(code(&one), 0, "{}", err(&one));
    // A lone path still counts.
    let counted = at(scratch.path(), &["billet-01.sample.md", "list"]);
    assert!(out(&counted).contains("1 sample\n"), "{}", out(&counted));
    // The target after the kind is read as well.
    let after = at(scratch.path(), &["list", "fields", "billet-01.sample.md"]);
    assert_eq!(code(&after), 0, "{}", err(&after));
    assert_eq!(out(&after), out(&one));
    // A kind that is also a folder is the kind, as a command is; the folder
    // is written `./tags`.
    fs::create_dir_all(scratch.path().join("tags")).unwrap();
    let both = at(scratch.path(), &["list", "tags"]);
    assert_eq!(code(&both), 0, "{}", err(&both));
    assert!(!out(&both).contains("samples"), "{}", out(&both));
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
    let found = at(scratch.path(), &[".", "-f", "diameter > 12.15"]);
    assert_eq!(code(&found), 0, "{}", err(&found));
    assert_eq!(out(&found).lines().count(), 2, "{}", out(&found));
    let tabled = at(
        scratch.path(),
        &["-f", "diameter > 12.15", "-c", "name,diameter"],
    );
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: '000000000000'}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("volume"), "{text}");
    assert!(text.contains("diameter"), "{text}");
    assert!(text.contains("outdated"), "{text}");
}

#[test]
fn stale_on_a_current_collection_says_so_and_exits_zero() {
    // Silence here would be indistinguishable from a failure.
    let scratch = Scratch::new("current");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    // No model, nothing computed: said so, never *every derived value is
    // current* of values there are none of.
    assert!(
        out(&output).contains("nothing is computed in") && out(&output).contains("no model"),
        "{}",
        out(&output)
    );
}

#[test]
fn explain_takes_several_targets_and_one_field() {
    // Targets as every command takes them, and the one word naming no file:
    // the field, wherever it is written.
    let scratch = Scratch::new("explain-args");
    scratch.corpus();
    let both = at(
        scratch.path(),
        &[
            "explain",
            "billet-01.sample.md",
            "billet-02.sample.md",
            "diameter",
        ],
    );
    assert_eq!(code(&both), 0, "{}", err(&both));
    assert!(
        out(&both).contains("billet-01 · diameter"),
        "{}",
        out(&both)
    );
    assert!(
        out(&both).contains("billet-02 · diameter"),
        "{}",
        out(&both)
    );
    let first = at(
        scratch.path(),
        &["explain", "diameter", "billet-01.sample.md"],
    );
    let last = at(
        scratch.path(),
        &["billet-01.sample.md", "explain", "diameter"],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert_eq!(out(&first), out(&last));
    // The unit a reader sees is the project's display form, as in a header.
    let accepted = at(
        scratch.path(),
        &["billet-01.sample.md", "explain", "porosity"],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    assert!(out(&accepted).contains("g/L"), "{}", out(&accepted));
    // A word naming neither a file nor a field is said to be neither.
    let typo = at(scratch.path(), &["explain", "billet-01.sample.md", "mlat"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
    assert!(
        err(&typo).contains("mlat: no such file, nor a field of the model"),
        "{}",
        err(&typo)
    );
    let two = at(
        scratch.path(),
        &["explain", "billet-01.sample.md", "mlat", "diameter"],
    );
    assert_eq!(code(&two), 1, "{}", out(&two));
    assert!(err(&two).contains("one field is taken"), "{}", err(&two));
    // A folder is its samples: one without the field holds nothing there,
    // the field being the project's.
    let folder = at(scratch.path(), &[".", "explain", "porosity"]);
    assert_eq!(code(&folder), 0, "{}", err(&folder));
    assert!(
        out(&folder).contains("billet-03 · porosity"),
        "{}",
        out(&folder)
    );
    assert!(
        out(&folder).contains("billet-04 · porosity = —\n\n  nothing is stored here"),
        "{}",
        out(&folder)
    );
}

#[test]
fn explain_says_when_nothing_computed_a_value() {
    // A measured quantity is not a silent blank.
    let scratch = Scratch::new("explain-measured");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["billet-01.sample.md", "explain", "diameter"],
    );
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
    // durometer's resolution beside an entered hb — and a heading over an
    // empty list would read as a blank. The sentence names the number the
    // formula made, where the record says which.
    let scratch = Scratch::new("explain-empty-record");
    scratch.config();
    scratch.write(
        "gauge.sample.md",
        "---\nschema_version: 1\nname: gauge\nproperties:\n  \
         hb: {v: 12.70, u: 0.0058, unit: mL, computed: {}, fingerprint: '000000000000'}\n---\n",
    );
    let output = at(scratch.path(), &["gauge.sample.md", "explain", "hb"]);
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
    let output = at(scratch.path(), &[".", "view", "--query", "approved"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert_eq!(text.matches("diameter").count(), 3, "{text}");
    assert_eq!(order(&text).len(), 3, "{text}");
    assert!(!text.contains('\u{250c}'), "{text}");
}

#[test]
fn export_regenerates_a_declared_dataset() {
    // The declared profile, format and destination.
    let scratch = Scratch::new("export");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("out/ductilities.csv")).unwrap();
    assert!(written.starts_with("diameter_value [g],"), "{written}");
    // Regenerating from unchanged data produces the same bytes.
    let again = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(
        fs::read_to_string(scratch.path().join("out/ductilities.csv")).unwrap(),
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
    let output = at(
        &scratch.path().join("empty"),
        &[".", "export", "ductilities"],
    );
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
    let preview = at(scratch.path(), &[".", "export", "ductilities"]);
    assert_eq!(code(&preview), 0, "{}", err_slashed(&preview));
    let shown = out_slashed(&preview);
    assert!(shown.contains("ductilities"), "{shown}");
    assert!(
        shown.contains("export ductilities · out/ductilities.csv · 4 rows"),
        "{shown}"
    );
    assert!(shown.contains("nothing written"), "{shown}");
    assert!(
        !scratch.path().join("out/ductilities.csv").exists(),
        "a preview wrote the file"
    );
    assert_eq!(
        code(&at(
            scratch.path(),
            &[".", "export", "ductilities", "--write"]
        )),
        0
    );
    // A second preview says the file is there and is replaced — where the
    // old shape refused, with a flag of its own.
    let again = out_slashed(&at(scratch.path(), &[".", "export", "ductilities"]));
    assert!(
        again.contains("the file is there, and is replaced"),
        "{again}"
    );
    assert_eq!(
        code(&at(
            scratch.path(),
            &[".", "export", "ductilities", "--write"]
        )),
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
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: C-01\nproperties:\n  \
         diameter: {v: 12.4, unit: g, fingerprint: deadbeefcafe, computed: {}}\n---\n",
    );
    for arguments in [
        vec![".", "export", "ductilities"],
        vec![".", "export", "ductilities", "-q"],
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
            out(&preview).contains("C-01 diameter"),
            "with {arguments:?}: {}",
            out(&preview)
        );
    }
    // And with --write, on stdout still: -q hid it when it was a warning.
    let written = at(
        scratch.path(),
        &[".", "export", "ductilities", "-q", "--write"],
    );
    assert!(out(&written).contains("not current"), "{}", out(&written));
}

#[test]
fn export_to_stdout_carries_only_the_data() {
    // `-` as the destination, for a redirection the author controls.
    let scratch = Scratch::new("export-stdout");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "export", "ductilities", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).starts_with("diameter_value [g],"),
        "{}",
        out(&output)
    );
    assert!(!scratch.path().join("out/ductilities.csv").exists());
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
    let output = without_git(&[".", "export", "ductilities", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).starts_with("diameter_value [g],"),
        "{}",
        out(&output)
    );
    let set = without_git(&["billet-01.sample.md", "set", "diameter=13", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(err(&set).is_empty(), "{}", err(&set));
    assert_eq!(
        history_messages(scratch.path()),
        [
            "samplekit billet-01.sample.md set diameter=13 --write",
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
    let output = at(scratch.path(), &[".", "validate"]);
    assert!(code(&output) == 0 || code(&output) == 2, "{}", err(&output));
    assert_eq!(before, snapshot(scratch.path()));
}

#[test]
fn validate_reports_what_validation_found() {
    // The dispatch is real, not a second implementation: two units for one
    // quantity is `validation`'s finding, and it appears here.
    let scratch = Scratch::new("validate-units");
    scratch.config();
    scratch.sample("a.sample.md", "billet-01", "approved", 12.1, Some(2.8));
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: billet-02\nproperties:\n  \
         porosity: {v: 2800.0, unit: kg/hL}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "validate"]);
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  porosity: {v: 2.8, unit: g/L}\n---\nN.\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: billet-02\nproperties:\n  porosity: {v: 2800.0, unit: kg/hL}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "validate"]);
    assert_eq!(code(&output), 2, "{}", out(&output));
    assert!(out(&output).contains("two units"), "{}", out(&output));
}

#[test]
fn validate_a_note_does_not_change_the_exit_code() {
    // A rewritable file is not an invalid one.
    let scratch = Scratch::new("validate-note");
    scratch.write(
        "unsorted.sample.md",
        "---\nname: billet-01\nschema_version: 1\nproperties:\n  \
         diameter:\n    v: 12.5\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "validate"]);
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
        "schema_version = 1\n[property.\"m.hv\"]\nprecision = \".1f\"\n",
    );
    for index in 1..=2 {
        rows.write(
            &format!("c{index}.md"),
            "---\nschema_version: 1\ntables:\n  m:\n    index: T\n    columns:\n      \
             T: {}\n      hv: {}\n    rows:\n      - {T: 1.0, hv: 0.01}\n      \
             - {T: 2.0, hv: 0.02}\n---\nN.\n",
        );
    }
    let text = out(&at(rows.path(), &[".", "validate"]));
    assert!(
        text.contains("m.hv: precision .1f writes a value as zero"),
        "{text}"
    );
    assert!(text.contains("\u{2014} 2 samples: c1, c2\n"), "{text}");
}

#[test]
fn a_file_target_is_checked_against_its_directory() {
    // A vocabulary belongs to the collection, not to the target: billet-04 has
    // no ductility, and its siblings do.
    let scratch = Scratch::new("file-vocabulary");
    scratch.corpus();
    let column = at(scratch.path(), &["billet-04.sample.md", "-c", "porosity"]);
    assert_eq!(code(&column), 0, "{}", err(&column));
    let filtered = at(
        scratch.path(),
        &[
            "billet-04.sample.md",
            "-c",
            "diameter",
            "-f",
            "porosity is missing",
        ],
    );
    assert_eq!(code(&filtered), 0, "{}", err(&filtered));
    let view = at(scratch.path(), &["billet-04.sample.md", "view"]);
    assert_eq!(code(&view), 0, "{}", err(&view));
    assert!(err(&view).is_empty(), "{}", err(&view));
    // A name no sample in the directory has is still a typo.
    let typo = at(scratch.path(), &["billet-04.sample.md", "-c", "porosityy"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
}

#[test]
fn validate_accepts_a_single_file() {
    // A collection of one, with the project found above it.
    let scratch = Scratch::new("validate-one");
    scratch.corpus();
    let output = at(scratch.path(), &["billet-01.sample.md", "validate"]);
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
            "diameter > 12.15",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(tags_of(scratch.path(), "billet-02.sample.md").contains("cold_crashed"));
    assert!(tags_of(scratch.path(), "billet-03.sample.md").contains("cold_crashed"));
    assert!(!tags_of(scratch.path(), "billet-01.sample.md").contains("cold_crashed"));
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
    let tags = tags_of(scratch.path(), "billet-01.sample.md");
    assert_eq!(tags.matches("reference").count(), 1, "{tags}");
}

#[test]
fn tag_preserves_the_note_and_the_records() {
    // Byte-identical note, untouched `computed`: a tag is not a value change
    // and invalidates no computation.
    let scratch = Scratch::new("tag-preserves");
    scratch.write(
        "computed.sample.md",
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n---\n\
         A note   with  deliberate    spacing.\n\nAnd a second paragraph.\n",
    );
    let output = at(scratch.path(), &["tag", "add", "reference", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let after = fs::read_to_string(scratch.path().join("computed.sample.md")).unwrap();
    assert!(
        after.contains("A note   with  deliberate    spacing.\n\nAnd a second paragraph.\n"),
        "{after}"
    );
    assert!(
        after.contains("computed: {diameter: aaaaaaaaaaaa}"),
        "{after}"
    );
    assert!(after.contains("fingerprint: bbbbbbbbbbbb"), "{after}");
}

#[test]
fn tag_rejects_a_non_identifier_before_writing() {
    // Nothing is half-applied, and the message names the rule.
    let scratch = Scratch::new("tag-identifier");
    scratch.corpus();
    let before = snapshot(scratch.path());
    let output = at(scratch.path(), &["tag", "add", "dext-casting", "--write"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("dext-casting"), "{}", err(&output));
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
            "diameter > 12.15",
            "--write",
        ],
    );
    let text = out(&output);
    assert!(text.contains("written: 2 files"), "{text}");
    assert!(text.contains("billet-02"), "{text}");
    assert!(text.contains("billet-03"), "{text}");
}

#[test]
fn tag_rename_drops_a_resulting_duplicate() {
    // One `reference`, in its existing position.
    let scratch = Scratch::new("tag-duplicate");
    scratch.write(
        "both.sample.md",
        "---\nschema_version: 1\nname: billet-01\ntags: [refernce, reference, cold_crashed]\n\
         properties:\n  diameter: {v: 12.5}\n---\nN.\n",
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
            "diameter > 12.15",
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
    // after the first pass a slab carries neither spelling, and an
    // interrupted run leaves a collection in that state.
    let scratch = Scratch::new("tag-one-pass");
    scratch.write(
        "middle.sample.md",
        "---\nschema_version: 1\nname: billet-01\ntags: [first, refernce, last]\n\
         properties:\n  diameter: {v: 12.5}\n---\nN.\n",
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
    let quiet = at(scratch.path(), &[".", "status"]);
    let loud = at(scratch.path(), &[".", "status", "-v"]);
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
    assert!(!out(&output).contains("billet-01"), "{}", out(&output));
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
        "soak.sample.md",
        "---\nschema_version: 1\nname: soak\ntables:\n  preheating:\n    index: [T]\n    columns:\n      \
         T: {}\n      R: {}\n      G: {}\n    rows:\n      - T: 10\n        R: 2.0\n        G:\n          \
         v: 0.5\n          computed: {row.R: '000000000000'}\n      - T: 20\n        R: 4.0\n        \
         G:\n          v: 0.25\n          computed: {row.R: '000000000000'}\n---\n",
    );
    let output = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("preheating.G"), "{text}");
    assert!(text.contains("(2 rows)"), "{text}");
    assert_eq!(text.matches("preheating.G").count(), 1, "{text}");
}

#[test]
fn list_counts_agree_with_their_number() {
    let scratch = Scratch::new("list-counts");
    table_corpus(&scratch);
    let output = at(scratch.path(), &[".", "list"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("1 sample\n"), "{}", out(&output));
    assert!(out(&output).contains(", 1 table —"), "{}", out(&output));
}

#[test]
fn a_summary_rounds_as_its_column() {
    let scratch = Scratch::new("summary-precision");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "porosity", "--summary"]);
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
        &[".", "-f", "status == approved & diameter > 1"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    let written = err(&output);
    let lines: Vec<&str> = written.lines().collect();
    assert!(lines[0].contains("'&&'"), "{written}");
    assert_eq!(lines[1], "  status == approved & diameter > 1", "{written}");
    assert_eq!(lines[2].find('^'), lines[1].find('&'), "{written}");
    assert!(!written.contains("(status"), "{written}");
    let symbols = at(
        scratch.path(),
        &[".", "-f", "status == approved && diameter > 12.15"],
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
        vec![".", "--profile", "ductilitis"],
        vec![".", "export", "ductilitis"],
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
         self.porosity = sk.Property(lambda s: s.diameter / 2)\n",
    );
    scratch.sample("a.sample.md", "a", "approved", 1.0, None);
    let output = at(scratch.path(), &["a.sample.md", "explain", "porosity"]);
    assert!(
        format!("{}{}", out(&output), err(&output)).contains(" compute --write"),
        "{}{}",
        out(&output),
        err(&output)
    );
    // A name the model does not declare is a misspelling, and computing
    // offers nothing for it.
    let unknown = at(scratch.path(), &["a.sample.md", "explain", "nosuch"]);
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
        "---\nschema_version: 1\nproperties:\n  diameter:\n    v: 12.5\n    unit: g\n    unit_math: \\mathrm{g}\n    precision: .3f\n    precision_unc: .1f\n---\nA metals note.\n",
    );
    for arguments in [vec!["."], vec![".", "status"], vec![".", "list"]] {
        let output = at(scratch.path(), &arguments);
        // Said, and the answer is partial: exit 2.
        assert_eq!(code(&output), 2, "{arguments:?}: {}", err(&output));
        assert!(
            err(&output).contains("legacy.sample.md was not read"),
            "{arguments:?}: {}",
            err(&output)
        );
        // Declaring schema 1, it is judged as schema 1: a field it does not
        // hold is an unknown field, with those it does.
        assert!(
            err(&output).contains("unknown field `unit_math`"),
            "{}",
            err(&output)
        );
    }
    let validate = at(scratch.path(), &[".", "validate"]);
    assert_eq!(code(&validate), 2, "{}", out(&validate));
}

#[test]
fn an_unknown_table_column_or_row_exits_one() {
    let scratch = Scratch::new("unknown-cell");
    scratch.corpus();
    scratch.write(
        "aged.sample.md",
        "---\nschema_version: 1\nname: aged\ntables:\n  tempering:\n    index: [day]\n    columns:\n      day: {}\n      nitridation: {}\n    rows:\n      - {day: 50, nitridation: 0.9}\n---\n",
    );
    let typo = at(scratch.path(), &["-c", "tempering.nitridatoin[50]"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
    assert!(
        err(&typo).contains("tempering.nitridation[50]"),
        "{}",
        err(&typo)
    );
    let row = at(scratch.path(), &["-c", "tempering.nitridation[51]"]);
    assert_eq!(code(&row), 1, "{}", out(&row));
    assert!(
        err(&row).contains("tempering.nitridation[51]"),
        "{}",
        err(&row)
    );
    let held = at(scratch.path(), &["-c", "tempering.nitridation[50]"]);
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
    let output = at(scratch.path(), &["billet-01.sample.md", "validate"]);
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
        &["..", "export", "ductilities", "-o", "here.csv", "--write"],
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
                "---\nschema_version: 1\nname: {file}\ntables:\n  tempering:\n    index: [day]\n    columns:\n      day: {{}}\n      nitridation: {{}}\n    rows:\n      - {{day: {cycle}, nitridation: 0.9}}\n---\n"
            ),
        );
    }
    let csv = at(
        scratch.path(),
        &["-c", "name,tempering.nitridation[50]", "--csv"],
    );
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(out(&csv).contains("aged-b.sample.md"), "{}", out(&csv));
    let summary = at(
        scratch.path(),
        &["-c", "tempering.nitridation[50]", "--summary"],
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
        &["-c", "diameter", "--csv", "-o", "typo/diameters.csv"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("typo does not exist, and is made"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.path().join("typo").exists());
    let written = at(
        scratch.path(),
        &[
            "-c",
            "diameter",
            "--csv",
            "-o",
            "typo/diameters.csv",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(scratch.path().join("typo/diameters.csv").is_file());
    let asked = at_terminal(
        scratch.path(),
        &[
            "-c",
            "diameter",
            "--csv",
            "-o",
            "again/diameters.csv",
            "--write",
        ],
        "",
    );
    assert_eq!(code(&asked), 0, "{}", out(&asked));
    assert!(!out(&asked).contains("create it?"), "{}", out(&asked));
    assert!(scratch.path().join("again/diameters.csv").is_file());
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
    let whole = at(scratch.path(), &["-c", "diameter"]);
    assert_eq!(code(&whole), 0, "{}", err(&whole));
    scratch.write(
        "broken.sample.md",
        "---\nschema_version: 1\nname: [oops\n---\n",
    );
    for arguments in [
        vec!["-c", "diameter"],
        vec!["list", "fields"],
        vec!["status"],
        vec!["export", "ductilities"],
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
    assert!(out(&at(scratch.path(), &["-c", "diameter"])).contains("12.10"));
}

#[test]
fn an_export_of_an_empty_selection_exits_zero() {
    // A question with no rows is a valid question. The preview says how many
    // rows it would write, and the export writes them — none.
    let scratch = Scratch::new("export-empty");
    scratch.corpus();
    let preview = at(
        scratch.path(),
        &["export", "ductilities", "-f", "diameter > 1000"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(out(&preview).contains("0 rows"), "{}", out(&preview));
    let written = at(
        scratch.path(),
        &["export", "ductilities", "-f", "diameter > 1000", "--write"],
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
         [query.heavy]\nfilter = \"diameter > 1.5\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"diameter\", precision = \".1f\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"-\"\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n\
         [query.light]\nfilter = \"diameter > 2.5\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"diameter\", precision = \".3f\"}]\n\
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
    let root = at(scratch.path(), &[".", "list", "queries"]);
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
            &format!("schema_version = 1\n[query.{query}]\nfilter = \"diameter > 1\"\n"),
        );
        above.sample(
            &format!("{project}/s.sample.md"),
            project,
            "approved",
            2.0,
            None,
        );
    }
    let both = at(above.path(), &["a", "b", "list", "queries"]);
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
             [property.diameter]\nunit = \"g\"\nprecision = \".1f\"\n\
             [query.heavy]\nfilter = \"diameter > 1.5\"\n\
             [profile.p]\ncolumns = [{{field = \"name\"}}, {{field = \"diameter\"}}]\n\
             [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"{output}\"\n\
             [figure.diameters]\nx = \"name\"\ny = \"diameter\"\n"
        ),
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "c1/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n\
         [property.diameter]\nprecision = \".3f\"\n\
         [profile.p]\nsort = [\"-diameter\"]\n\
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
    imported_collections(&scratch, "exports/{project}/e.csv");
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
        lines[1].starts_with("own") && lines[1].ends_with(" · local"),
        "{text}"
    );
    let queries = at(&c1, &["list", "queries"]);
    assert!(
        out(&queries).contains(" · ../.samplekitrc"),
        "{}",
        out(&queries)
    );
    let figures = at(&c1, &["list", "figures"]);
    assert!(
        out(&figures).contains("diameters   on the selection · ../.samplekitrc"),
        "{}",
        out(&figures)
    );
    // The root imports nothing: its names and what each is, no origin.
    let root = at(scratch.path(), &["list", "profiles"]);
    assert_eq!(out(&root), "p   name, diameter\n", "{}", out(&root));
}

#[test]
fn a_sample_follows_its_collection_wherever_the_command_runs_from() {
    let scratch = Scratch::new("follows-collection");
    imported_collections(&scratch, "exports/{project}/e.csv");
    let from_root = at(scratch.path(), &[".", "-c", "name,diameter"]);
    assert_eq!(code(&from_root), 0, "{}", err(&from_root));
    assert!(out(&from_root).contains("3.000"), "{}", out(&from_root));
    assert!(out(&from_root).contains("1.0 "), "{}", out(&from_root));
    let from_c1 = at(&scratch.path().join("c1"), &["-c", "name,diameter"]);
    assert!(out(&from_c1).contains("3.000"), "{}", out(&from_c1));
    let alone = at(scratch.path(), &["c1/x.sample.md", "-c", "name,diameter"]);
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
    let refused = at(scratch.path(), &[".", "export", "e", "--write"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("the second would replace the first")
            && err(&refused).contains("{project}"),
        "{}",
        err(&refused)
    );
    assert!(!scratch.path().join("exports/e.csv").exists());
    let apart = Scratch::new("one-export-file-apart");
    imported_collections(&apart, "exports/{project}/e.csv");
    let written = at(apart.path(), &[".", "export", "e", "--write"]);
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
    imported_collections(&scratch, "exports/{project}/e.csv");
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
    let output = at(scratch.path(), &[".", "export", "e"]);
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
/// `ductility` was edited by hand.
fn edited_and_stale(scratch: &Scratch) {
    scratch.config();
    scratch.write(
        "computed.sample.md",
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: aaaaaaaaaaaa}\n  ductility:\n    v: 3.0\n    \
         fingerprint: bbbbbbbbbbbb\n    computed: {diameter: aaaaaaaaaaaa}\n---\nN.\n",
    );
}

#[test]
fn explain_shows_each_input_with_its_value_and_state() {
    let scratch = Scratch::new("explain-input-values");
    edited_and_stale(&scratch);
    let output = at(scratch.path(), &["computed.sample.md", "explain", "volume"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("computed from"), "{text}");
    assert!(text.contains("diameter") && text.contains("12.5"), "{text}");
    assert!(!text.contains("aaaaaaaaaaaa"), "{text}");
}

#[test]
fn explain_shows_the_digests_with_verbose() {
    let scratch = Scratch::new("explain-digests");
    edited_and_stale(&scratch);
    let output = at(
        scratch.path(),
        &["computed.sample.md", "explain", "volume", "-v"],
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
        &[".", "-c", "diameter,volume,ductility", "--width", "120"],
    );
    assert_eq!(code(&terminal), 0, "{}", err(&terminal));
    assert!(out(&terminal).contains('✎'), "{}", out(&terminal));
    assert!(out(&terminal).contains('⚠'), "{}", out(&terminal));
    let piped = at(scratch.path(), &[".", "-c", "diameter,volume"]);
    assert!(
        !out(&piped).contains('✎') && !out(&piped).contains('⚠'),
        "{}",
        out(&piped)
    );
    let exported = at(scratch.path(), &[".", "-c", "diameter,volume", "--csv"]);
    assert!(!out(&exported).contains('✎'), "{}", out(&exported));
}

#[test]
fn a_piped_table_says_what_it_shows_not_current() {
    // Piped, a table has no marks: an outdated value read exactly as a
    // current one. Its count is said on stderr, the cells left clean.
    let scratch = Scratch::new("piped-not-current");
    edited_and_stale(&scratch);
    let piped = at(scratch.path(), &[".", "-c", "diameter,volume,ductility"]);
    assert_eq!(code(&piped), 0, "{}", err(&piped));
    assert!(
        err(&piped).contains("2 values shown are not current"),
        "{}",
        err(&piped)
    );
    let viewed = at(scratch.path(), &["computed.sample.md", "view"]);
    assert!(
        err(&viewed).contains("shown are not current"),
        "{}",
        err(&viewed)
    );
    // CSV says each state in a column of its own.
    let exported = at(scratch.path(), &[".", "-c", "volume", "--csv"]);
    assert!(
        !err(&exported).contains("shown are not current"),
        "{}",
        err(&exported)
    );
}

#[test]
fn a_folder_without_samples_says_where_they_are() {
    // `samplekit .` at a project's root, its samples in `samples/`, listed
    // nothing and said nothing.
    let scratch = Scratch::new("no-sample-here");
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = false\n",
    );
    scratch.write(
        "project/samples/a.md",
        "---\nschema_version: 1\nname: a\n---\n",
    );
    let listed = at(&scratch.path().join("project"), &["."]);
    assert_eq!(code(&listed), 0, "{}", err_slashed(&listed));
    assert!(
        err_slashed(&listed)
            .contains("no sample here — samples/ holds 1 sample: samplekit samples"),
        "{}",
        err_slashed(&listed)
    );
}

#[test]
fn the_identity_column_is_added_in_the_terminal_only() {
    let scratch = Scratch::new("identity");
    scratch.corpus();
    let terminal = at(scratch.path(), &[".", "-c", "diameter", "--width", "120"]);
    assert_eq!(code(&terminal), 0, "{}", err(&terminal));
    assert!(out(&terminal).contains("billet-01"), "{}", out(&terminal));
    let piped = at(scratch.path(), &[".", "-c", "diameter"]);
    assert!(!out(&piped).contains("billet-01"), "{}", out(&piped));
}

#[test]
fn an_identity_already_asked_for_is_not_repeated() {
    let scratch = Scratch::new("identity-asked");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--width", "120"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).matches("billet-01").count(),
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
    let by_file = at(scratch.path(), &[".", "-c", "diameter", "--width", "120"]);
    assert_eq!(code(&by_file), 0, "{}", err(&by_file));
    assert!(
        out(&by_file).contains("billet-01.sample.md"),
        "{}",
        out(&by_file)
    );
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[render]\nidentify = \"none\"\n",
    );
    let bare = at(scratch.path(), &[".", "-c", "diameter", "--width", "120"]);
    assert!(!out(&bare).contains("billet-01"), "{}", out(&bare));
}

#[test]
fn tags_are_joined_in_csv_and_an_array_in_json() {
    let scratch = Scratch::new("tags-export");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\ntags: [dry_treated, reference]\n---\n",
    );
    let csv = at(scratch.path(), &[".", "-c", "name,tags", "--csv"]);
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(
        out(&csv).contains("a,dry_treated;reference"),
        "{}",
        out(&csv)
    );
    let json = at(scratch.path(), &[".", "-c", "name,tags", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out(&json)).unwrap();
    assert!(
        parsed[0]["tags"] == serde_json::json!(["dry_treated", "reference"]),
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
        &[
            "billet-01.sample.md",
            "billet-02.sample.md",
            "-c",
            "name,diameter",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(
        text.contains("billet-01") && text.contains("billet-02"),
        "{text}"
    );
    assert!(!text.contains("billet-03"), "{text}");
    let filtered = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "billet-04.sample.md",
            "-f",
            "status == \"approved\"",
            "-c",
            "name",
        ],
    );
    assert_eq!(code(&filtered), 0, "{}", err(&filtered));
    assert!(out(&filtered).contains("billet-01"), "{}", out(&filtered));
    assert!(!out(&filtered).contains("billet-04"), "{}", out(&filtered));
}

#[test]
fn a_mistyped_command_before_a_target_suggests_the_command() {
    let scratch = Scratch::new("mistyped-command");
    scratch.corpus();
    let typo = at(scratch.path(), &["statu", "billet-01.sample.md"]);
    assert_eq!(code(&typo), 1, "{}", out(&typo));
    assert!(
        err(&typo).contains("did you mean 'samplekit status billet-01.sample.md'?"),
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
        &["billet-01.sample.md", "./billet-01.sample.md", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert_eq!(
        out(&output).matches("billet-01").count(),
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
        &[".", "-c", "name,diameter", "-o", "out/t.csv", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("out/t.csv")).unwrap();
    assert!(written.starts_with("name,diameter_value [g]"), "{written}");
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
            &[".", "-c", "name,diameter", "-o", refused, "--write"],
        );
        assert_eq!(code(&output), 1, "{refused}: {}", err(&output));
        assert!(
            err(&output).contains(".csv, .tsv, .json, .tex, .md or .html"),
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
        &[".", "-c", "name,diameter", "-o", "t.txt", "--write"],
    );
    assert_eq!(code(&text), 0, "{}", err(&text));
    let named = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name,diameter",
            "-o",
            "t.data",
            "--csv",
            "--write",
        ],
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
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, {field = \"porosity\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"e.csv\"\n",
    );
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  \
         porosity:\n    v: 9.0\n    computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: {edited: bbbbbbbbbbbb}\n---\n",
    );
    let preview = at(scratch.path(), &[".", "export", "e"]);
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
    let first = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let before = fs::read_to_string(scratch.path().join("out/ductilities.csv")).unwrap();
    scratch.write(
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nproperties: [\n---\n",
    );
    let preview = at(scratch.path(), &[".", "export", "ductilities"]);
    assert!(
        out(&preview).contains("--write writes nothing"),
        "{}",
        out(&preview)
    );
    let kept = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&kept), 2, "{}", err(&kept));
    assert!(err(&kept).contains("not replaced"), "{}", err(&kept));
    assert_eq!(
        fs::read_to_string(scratch.path().join("out/ductilities.csv")).unwrap(),
        before
    );
    // Nor is a new one written, as incomplete as a replaced one.
    let elsewhere = at(
        scratch.path(),
        &[".", "export", "ductilities", "-o", "fresh.csv", "--write"],
    );
    assert_eq!(code(&elsewhere), 2, "{}", err(&elsewhere));
    assert!(
        err(&elsewhere).contains("not written"),
        "{}",
        err(&elsewhere)
    );
    assert!(!scratch.path().join("fresh.csv").exists());
}

#[test]
fn an_export_from_values_not_current_names_their_folder_as_a_listing_does() {
    // It said `samplekit ./billets compute`, where a listing says `billets/`.
    let scratch = Scratch::new("export-outdated-folder");
    scratch.corpus();
    scratch.write(
        "billets/billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nstatus: approved\nproperties:\n  \
         diameter: {v: 12.1, u: 0.05, unit: g}\n  \
         porosity: {v: 2.801, unit: g/L, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    let written = at(
        scratch.path(),
        &["./billets", "export", "ductilities", "--write"],
    );
    assert!(
        out(&written).contains("samplekit billets compute --force --write"),
        "{}",
        out(&written)
    );
}

#[test]
fn an_export_from_values_not_current_exits_two_unless_accepted() {
    // Written and warned, it exited 0, and a script with `set -e` went on to
    // ship the file.
    let scratch = Scratch::new("export-outdated");
    scratch.corpus();
    // A porosity whose number is not the one its record says: edited, not current.
    scratch.write(
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nstatus: approved\nproperties:\n  \
         diameter: {v: 12.1, u: 0.05, unit: g}\n  \
         porosity: {v: 2.801, unit: g/L, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    let stale = at(scratch.path(), &[".", "-c", "name,porosity,state"]);
    assert!(out(&stale).contains("edited"), "{}", out(&stale));
    let written = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&written), 2, "{}", err(&written));
    assert!(scratch.path().join("out/ductilities.csv").exists());
    assert!(
        err(&written).contains("--accept edited"),
        "{}",
        err(&written)
    );
    let accepted = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "--accept",
            "edited",
            "--write",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
}

#[test]
fn quiet_still_says_why_an_answer_is_incomplete() {
    // -q hid the only line that explained exit 2: a file not read.
    let scratch = Scratch::new("quiet-unread");
    scratch.corpus();
    scratch.write(
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nproperties: [\n---\n",
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
    let output = at(scratch.path(), &[".", "export", "e"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("root-one"), "{}", out(&output));
    assert!(out(&output).contains("nested-one"), "{}", out(&output));
    assert!(!err(&output).contains("not exported"), "{}", err(&output));
}

#[test]
fn an_export_one_project_cannot_make_writes_no_project() {
    // The root's export wrote the root's file, then failed on a project
    // lacking a column, said of that project's file, the rest never written.
    let scratch = Scratch::new("export-checked-first");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"radius\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"out/{project}.csv\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  radius: 1.05\n---\n",
    );
    scratch.write(
        "cider/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n",
    );
    scratch.write(
        "cider/b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  sugar: 12\n---\n",
    );
    let output = at(scratch.path(), &[".", "export", "e", "--write"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("imported by") && err(&output).contains("cider"),
        "{}",
        err(&output)
    );
    assert!(
        err(&output).contains("nothing was written"),
        "{}",
        err(&output)
    );
    assert!(!scratch.path().join("out").exists());
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
fn soak(scratch: &Scratch) {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "soak.sample.md",
        "---\nschema_version: 1\nname: soak\ntables:\n  preheating:\n    index: [T]\n    columns:\n      \
         T: {}\n      R: {unit: lintner}\n      G: {}\n    rows:\n      - T: 10\n        R: 2.0\n        G:\n          \
         v: 0.5\n          computed: {row.R: '000000000000'}\n      - T: 20\n        R: 4.0\n        \
         G:\n          v: 0.25\n          computed: {row.R: '000000000000'}\n---\n",
    );
}

#[test]
fn explain_shows_a_row_input_value() {
    let scratch = Scratch::new("explain-row-input");
    soak(&scratch);
    let output = at(
        scratch.path(),
        &["soak.sample.md", "explain", "preheating.G[10]"],
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
    soak(&scratch);
    let output = at(
        scratch.path(),
        &["soak.sample.md", "explain", "preheating.G[99]"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("no such row"), "{}", err(&output));
}

#[test]
fn a_table_column_unit_reaches_the_header_and_the_json() {
    let scratch = Scratch::new("column-unit");
    soak(&scratch);
    let json = at(scratch.path(), &[".", "-c", "preheating.R[10]", "--json"]);
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
        "---\nschema_version: 1\nname: f\nproperties:\n  diameter: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  \
         loading:\n    computed: {diameter: aaaaaaaaaaaa}\n    \
         fingerprint: {failed: \"ZeroDivisionError: division by zero\"}\n---\n",
    );
    let output = at(scratch.path(), &["f.sample.md", "explain", "loading"]);
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
        &[".", "-c", "wear.stats.count", "--precision", ".2e"],
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
        "---\nschema_version: 1\nname: b\nproperties:\n  preheat_diameter: {v: \"12,5\", unit: mg}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["b.sample.md", "tag", "add", "checked", "--write"],
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
    let file = scratch.path().join("billet-01.sample.md");
    let before = fs::read_to_string(&file).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    let output = at(
        scratch.path(),
        &["billet-01.sample.md", "tag", "add", "fresh", "--write"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(err(&output).contains("read-only"), "{}", err(&output));
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o444);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
#[cfg(unix)]
fn a_write_the_filesystem_refuses_exits_three() {
    // `tag` on a read-only file exited 2 where `set` exited 3; a folder that
    // refuses a new file was said as the file refusing.
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("filesystem-refuses");
    scratch.corpus();
    let file = scratch.path().join("billet-01.sample.md");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    let tagged = at(
        scratch.path(),
        &["billet-01.sample.md", "tag", "add", "fresh", "--write"],
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code(&tagged), 3, "{}", err(&tagged));
    fs::create_dir_all(scratch.path().join("shut")).unwrap();
    fs::rename(&file, scratch.path().join("shut/billet-01.sample.md")).unwrap();
    let folder = scratch.path().join("shut");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
    let set = at(
        scratch.path(),
        &["shut/billet-01.sample.md", "set", "status=kept", "--write"],
    );
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code(&set), 3, "{}", err(&set));
    assert!(
        err(&set).contains("its folder shut cannot be written in"),
        "{}",
        err(&set)
    );
}

#[test]
fn a_file_that_is_not_utf8_text_is_a_data_error() {
    // A sample an editor saved as UTF-16 is what the file holds, not what the
    // filesystem refused: exit 2, said as such, as a list says it.
    let scratch = Scratch::new("utf16");
    scratch.corpus();
    let utf16: Vec<u8> = "\u{feff}---\nname: wide\n---\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    fs::write(scratch.path().join("wide.md"), utf16).unwrap();
    let set = at(
        scratch.path(),
        &["wide.md", "set", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 2, "{}", err(&set));
    assert!(err(&set).contains("is not UTF-8 text"), "{}", err(&set));
    let listed = at(scratch.path(), &["."]);
    assert_eq!(code(&listed), 2, "{}", err(&listed));
    assert!(err(&listed).contains("not UTF-8 text"), "{}", err(&listed));
}

#[test]
fn a_command_at_a_snapshot_says_paths_as_typed() {
    // Under `--at`, a path column named the temporary folder the past was
    // read from, and `-o -` wrote a file named `-` in the project.
    let scratch = Scratch::new("at-paths");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let listed = at(scratch.path(), &[".", "-c", "name,path", "--at", "1"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("billet-01.sample.md"),
        "{}",
        out(&listed)
    );
    assert!(!out(&listed).contains("samplekit-at-"), "{}", out(&listed));
    let printed = at(
        scratch.path(),
        &[
            ".", "-c", "name", "--csv", "-o", "-", "--write", "--at", "1",
        ],
    );
    assert_eq!(code(&printed), 0, "{}", err(&printed));
    assert!(out(&printed).contains("billet-01"), "{}", out(&printed));
    assert!(!scratch.path().join("-").exists());
}

#[test]
fn at_a_snapshot_a_configuration_importing_the_folder_above_is_read() {
    // `import = ".."` named a file no snapshot keeps, and every command at a
    // snapshot failed on the configuration.
    let scratch = Scratch::new("at-import");
    fs::write(
        scratch.path().join(".samplekitrc"),
        "schema_version = 1\n[query.heavy]\nfilter = \"mass > 1\"\n",
    )
    .unwrap();
    let project = scratch.path().join("project");
    fs::create_dir_all(project.join("samples")).unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\nimport = \"..\"\n",
    )
    .unwrap();
    fs::write(
        project.join("samples/a.md"),
        "---\nschema_version: 1\nproperties:\n  mass: 2\n---\n",
    )
    .unwrap();
    let set = at(&project, &["samples/a.md", "set", "mass=3", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let past = at(
        &project,
        &["samples", "--query", "heavy", "-c", "mass", "--at", "1"],
    );
    assert_eq!(code(&past), 0, "{}", err(&past));
    assert!(out(&past).contains('3'), "{}", out(&past));
}

#[test]
fn at_a_snapshot_each_file_a_list_imports_is_read() {
    // Each of a list read where it is, as for one folder.
    let scratch = Scratch::new("at-import-list");
    fs::write(scratch.path().join(".samplekitrc"), "schema_version = 1\n").unwrap();
    let project = scratch.path().join("project");
    fs::create_dir_all(project.join("samples")).unwrap();
    fs::write(
        project.join("queries.samplekitrc"),
        "schema_version = 1\n[query.heavy]\nfilter = \"mass > 1\"\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\nimport = [\"..\", \"queries.samplekitrc\"]\n",
    )
    .unwrap();
    fs::write(
        project.join("samples/a.md"),
        "---\nschema_version: 1\nproperties:\n  mass: 2\n---\n",
    )
    .unwrap();
    let set = at(&project, &["samples/a.md", "set", "mass=3", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let past = at(
        &project,
        &["samples", "--query", "heavy", "-c", "mass", "--at", "1"],
    );
    assert_eq!(code(&past), 0, "{}", err(&past));
    assert!(out(&past).contains('3'), "{}", out(&past));
}

#[test]
fn list_files_finds_a_folder_beside_the_samples() {
    // `files = ["photos"]` beside `pieces/`: every file was said to belong to
    // no sample, and from the project's folder the files were said not to be
    // declared at all.
    let scratch = Scratch::new("files-beside");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"photos\"]\n",
    );
    scratch.write("pieces/alpha-titanium.md", "---\nschema_version: 1\n---\n");
    for name in ["alpha-titanium.jpg", "other.jpg"] {
        scratch.write(&format!("photos/{name}"), "");
    }
    for arguments in [&["pieces", "list", "files"][..], &["list", "files"][..]] {
        let listed = at(scratch.path(), arguments);
        assert_eq!(code(&listed), 0, "{}", err_slashed(&listed));
        let said = out_slashed(&listed);
        let owned = said.split("belonging to no sample").next().unwrap();
        assert!(owned.contains("photos/alpha-titanium.jpg"), "{said}");
        assert!(!owned.contains("other.jpg"), "{said}");
        assert!(!said.contains("no directory is declared"), "{said}");
    }
}

#[test]
fn a_query_starts_in_its_declared_directory() {
    // `directory` was read and never used: from the project's folder, holding
    // no sample itself, `--query` failed with *unknown field* and *0 fields
    // available*.
    let scratch = Scratch::new("query-directory");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[query.nickels]\nfilter = 'style == \"nickel\"'\ndirectory = \"pieces\"\n\
         [query.nowhere]\nfilter = 'style == \"nickel\"'\ndirectory = \"gone\"\n",
    );
    scratch.write(
        "pieces/dry.md",
        "---\nschema_version: 1\nstyle: nickel\n---\n",
    );
    scratch.write(
        "pieces/bronze.md",
        "---\nschema_version: 1\nstyle: bronze\n---\n",
    );
    let listed = at(scratch.path(), &["--query", "nickels", "-c", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(out(&listed).contains("dry"), "{}", out(&listed));
    assert!(!out(&listed).contains("bronze"), "{}", out(&listed));
    // Over a folder of no sample, a filter selects nothing and says so.
    fs::create_dir_all(scratch.path().join("empty")).unwrap();
    let empty = at(&scratch.path().join("empty"), &["-f", "style == 1"]);
    assert_eq!(code(&empty), 0, "{}", err(&empty));
    assert!(err(&empty).contains("no sample in"), "{}", err(&empty));
    let checked = at(scratch.path(), &["pieces", "validate"]);
    assert!(
        out(&checked).contains("its directory 'gone' is no folder"),
        "{}",
        out(&checked)
    );
}

#[test]
fn validate_finds_what_a_command_would_refuse() {
    // Each passed `validate` with no defect and then failed the first command
    // that read it, in words naming neither .samplekitrc nor the profile.
    let scratch = Scratch::new("validate-declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}]\n\
         sort = [\"-stlye\"]\n[model]\npath = \"model\"\n",
    );
    fs::create_dir_all(scratch.path().join("model")).unwrap();
    scratch.write("a.md", "---\nschema_version: 1\nstyle: nickel\n---\n");
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 2, "{}", out(&checked));
    let said = out(&checked);
    assert!(
        said.contains("profile 'p': no sample has 'stlye' — did you mean 'style'?"),
        "{said}"
    );
    assert!(said.contains("[model] path names the folder"), "{said}");
    // A table's drawing naming nothing sets the configuration aside as it
    // loads, naming the file and the line, for every command alike.
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[render]\ntable = \"fancy\"\n",
    );
    let drawn = at(scratch.path(), &["-c", "name"]);
    assert!(
        err(&drawn).contains(".samplekitrc:3: [render] table is plain or boxed, not 'fancy'"),
        "{}",
        err(&drawn)
    );
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 2, "{}", out(&checked));
    assert!(out(&checked).contains("not read"), "{}", out(&checked));
}

#[test]
fn validate_notes_a_declaration_naming_nothing() {
    // `[property.radiuss]`, a column misspelt, a table declared as a quantity and a
    // variant for a style one slip from a declared one were all accepted in
    // silence.
    let scratch = Scratch::new("declarations-naming-nothing");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[style.math]\n[property.radiuss]\nsymbol = \"RADIUS\"\n\
         [property.\"ageing.temprature\"]\nprecision = \".3f\"\n\
         [property.ageing]\nprecision = \".3f\"\n[property.radius]\nsymbol_mth = \"x\"\n\
         [unit.degC]\nmth = \"x\"\nfigure = \"°C\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  radius: 1.05\ntables:\n  ageing:\n    \
         index: day\n    columns:\n      day: {}\n      temperature: {}\n    rows:\n      \
         - day: 1\n        temperature: 20\n---\n",
    );
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 0, "{}", out(&checked));
    let said = out(&checked);
    for expected in [
        "[property.radiuss]: no sample has 'radiuss' — did you mean 'radius'?",
        "did you mean 'ageing.temperature'?",
        "'ageing' is a table",
        "symbol_mth: no style 'mth' is declared — did you mean 'math'?",
        "[unit.\"degC\"]: mth: no style 'mth'",
    ] {
        assert!(said.contains(expected), "{expected}: {said}");
    }
    // A variant for a style not declared yet, and far from any, is said too:
    // no surface writes it until one is.
    assert!(
        said.contains("figure: no style 'figure' is declared: no surface writes this variant"),
        "{said}"
    );
}

#[test]
fn a_declared_export_is_held_to_its_format_and_its_own_file() {
    // `output = "x.json"` with `format = "csv"` wrote CSV into it, where `-o`
    // refused the same; two exports writing one file replaced each other.
    let scratch = Scratch::new("declared-exports");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.overview]\ncolumns = [{field = \"name\"}]\n\
         [export.overview]\nprofile = \"overview\"\nformat = \"csv\"\noutput = \"out/overview.csv\"\n\
         [export.x]\nprofile = \"overview\"\nformat = \"csv\"\noutput = \"out/x.json\"\n\
         [export.y]\nprofile = \"overview\"\nformat = \"csv\"\noutput = \"out/./overview.csv\"\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\n---\n");
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 2, "{}", out(&checked));
    assert!(
        out(&checked)
            .contains("[export.x]: output = \"out/x.json\" names JSON, and format writes CSV"),
        "{}",
        out(&checked)
    );
    assert!(
        out(&checked).contains("is [export.overview]'s too"),
        "{}",
        out(&checked)
    );
    let written = at(scratch.path(), &[".", "export", "x", "--write"]);
    assert_eq!(code(&written), 1, "{}", err(&written));
    assert!(!scratch.path().join("out/x.json").exists());
}

#[test]
fn view_shows_a_cell_as_a_column_shows_it() {
    // `view` dropped a cell's uncertainty, showed `n/a` as an empty cell, and
    // headed a column without the unit `[property."table.column"]` declares.
    let scratch = Scratch::new("view-cells");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.\"ageing.hardness\"]\nunit = \"SG\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\ntables:\n  ageing:\n    index: day\n    columns:\n      \
         day: {}\n      hardness: {}\n      temperature: {}\n    rows:\n      - day: 1\n        \
         hardness: 1.041\n        temperature: {v: 20.7, u: 0.1}\n      - day: 2\n        hardness: n/a\n---\n",
    );
    let viewed = at(scratch.path(), &[".", "view"]);
    assert_eq!(code(&viewed), 0, "{}", err(&viewed));
    let said = out(&viewed);
    assert!(said.contains("hardness [SG]"), "{said}");
    assert!(said.contains("20.70 ± 0.10"), "{said}");
    assert!(said.contains("n/a"), "{said}");
}

#[test]
fn a_template_places_the_unit_and_leaves_an_empty_cell_empty() {
    // `{unit}` was filled with nothing in a terminal, and a sample without the
    // value drew `\\num{ \\pm }`.
    let scratch = Scratch::new("template-cells");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"modulus\", template = \"{value:.1f}{unit}\"}, \
         {field = \"radius\", template = '\\num{{value} \\pm {uncertainty}}'}]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  modulus: {v: 6.82, unit: \"%\"}\n  radius: {v: 1.05, u: 0.001}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nproperties:\n  modulus: n/a\n---\n",
    );
    let drawn = at(scratch.path(), &[".", "--profile", "p"]);
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    let said = out(&drawn);
    assert!(said.contains("6.8%"), "{said}");
    assert!(said.contains("\\num{1.05 \\pm 0.001}"), "{said}");
    assert!(!said.contains("\\pm }"), "{said}");
    assert!(said.contains("n/a"), "{said}");
}

#[test]
fn an_import_is_said_by_the_file_that_holds_it() {
    // An error in the file imported named only it, though the importer was
    // set aside too; a copy was said to be declared by a file that only
    // imports it; a preview named `data/ana/../../exports`.
    let scratch = Scratch::new("import-words");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n[profile.p]\ncolumns = [{field = \"volume\"}]\n\
         [export.radius]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"exports/{project}/radius.csv\"\n",
    );
    scratch.write("data/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    scratch.write(
        "data/ana/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[property.volume]\nunit = \"L\"\n",
    );
    scratch.write(
        "data/ana/a.md",
        "---\nschema_version: 1\nproperties:\n  volume: 2\n---\n",
    );
    let checked = at(scratch.path(), &["data/ana", "validate"]);
    let root = dunce::canonicalize(scratch.path())
        .unwrap()
        .join(".samplekitrc");
    assert!(
        out_slashed(&checked).contains(&format!(
            "repeats what {} declares",
            slashed(root.display().to_string())
        )),
        "{}",
        out_slashed(&checked)
    );
    let previewed = at(scratch.path(), &["data/ana", "export", "radius"]);
    assert!(
        out_slashed(&previewed).contains("export radius · exports/data/ana/radius.csv"),
        "{}",
        out_slashed(&previewed)
    );
    assert!(
        out_slashed(&previewed).contains("  exports/data/ana does not exist"),
        "{}",
        out_slashed(&previewed)
    );
    scratch.write(".samplekitrc", "schema_version = 1\nbad = [\n");
    let listed = at(scratch.path(), &["data/ana"]);
    assert!(
        err_slashed(&listed)
            .contains("data/ana/.samplekitrc: it imports a configuration that is not read"),
        "{}",
        err_slashed(&listed)
    );
}

#[test]
fn validate_reads_the_collections_patterns() {
    // A glob that does not read matched nothing, and an `include` naming a
    // folder in a collection that is not recursive kept nothing, both without
    // a word.
    let scratch = Scratch::new("collection-patterns");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = false\ninclude = [\"2025/*.md\", \"*.md\"]\n\
         exclude = [\"[unclosed\"]\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\n---\n");
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 2, "{}", out(&checked));
    let said = out(&checked);
    assert!(
        said.contains("[collection] exclude '[unclosed': the pattern does not read"),
        "{said}"
    );
    assert!(
        said.contains("[collection] include '2025/*.md': it names a folder"),
        "{said}"
    );
}

#[test]
fn a_sample_without_the_value_has_no_unit_to_disagree_with() {
    // `[property.volume] unit = "L"` gave a unit to samples holding no volume,
    // and one file in mL made the table refused as *in L and mL*.
    let scratch = Scratch::new("declared-unit-alone");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  volume: {v: 20000, unit: mL}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nproperties:\n  radius: 1.05\n---\n",
    );
    let drawn = at(scratch.path(), &[".", "-c", "name,volume"]);
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(out(&drawn).contains("volume [mL]"), "{}", out(&drawn));
}

#[test]
fn a_text_no_sample_holds_is_said_with_the_nearest_held() {
    // `style == titaniumm` and `style == TITANIUM` gave an empty list in silence, where
    // a tag misspelt is said.
    let scratch = Scratch::new("text-near");
    scratch.write("a.md", "---\nschema_version: 1\nstyle: titanium\n---\n");
    scratch.write("b.md", "---\nschema_version: 1\nstyle: nickel\n---\n");
    for (written, near) in [("titaniumm", "titanium"), ("TITANIUM", "titanium")] {
        let listed = at(scratch.path(), &[".", "-f", &format!("style == {written}")]);
        assert_eq!(code(&listed), 0, "{}", err(&listed));
        assert!(
            err(&listed).contains(&format!(
                "no sample has style '{written}' — did you mean '{near}'?"
            )),
            "{}",
            err(&listed)
        );
    }
    let held = at(scratch.path(), &[".", "-f", "style == titanium"]);
    assert!(!err(&held).contains("did you mean"), "{}", err(&held));
}

#[test]
fn a_typed_number_is_previewed_as_the_table_shows_it_or_with_its_digits() {
    // `area=25.0` read `25 ± 0.028`: the table's decimals lost.
    let scratch = Scratch::new("set-typed-digits");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  area: {v: 24.918, u: 0.028, unit: mm2}\n---\n",
    );
    for (typed, shown) in [
        ("25.0", "25.000 ± 0.028"),
        ("25.1", "25.100 ± 0.028"),
        ("25.0123", "25.0123 ± 0.028"),
    ] {
        let set = at(scratch.path(), &["a.md", "set", &format!("area={typed}")]);
        assert_eq!(code(&set), 0, "{}", err(&set));
        assert!(out(&set).contains(shown), "{typed}: {}", out(&set));
    }
}

#[test]
fn set_says_a_new_field_one_slip_from_a_held_one() {
    // `radiuss=1.05` beside `radius` was written as a new attribute without a word.
    let scratch = Scratch::new("set-near-field");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  radius: 1.06\n---\n",
    );
    scratch.write("b.md", "---\nschema_version: 1\n---\n");
    let set = at(scratch.path(), &["b.md", "set", "radiuss=1.05"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        err(&set).contains(
            "no sample here holds 'radiuss', written as a new field — did you mean 'radius'?"
        ),
        "{}",
        err(&set)
    );
    let held = at(scratch.path(), &["b.md", "set", "radius=1.05"]);
    assert!(!err(&held).contains("did you mean"), "{}", err(&held));
}

#[test]
fn a_file_that_does_not_read_is_said_as_the_file_shows_it() {
    // Lines counted from the frontmatter sent the reader one line short; a
    // merge left unresolved read as broken YAML; `1,062` as a field `062`.
    let scratch = Scratch::new("file-words");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nproperties:\n  radius: {v: 1,062}\n---\n",
    );
    scratch.write("b.md", "---\nschema_version: 1\nname: [unclosed\n---\n");
    scratch.write(
        "c.md",
        "---\nschema_version: 1\n<<<<<<< HEAD\nstyle: titanium\n=======\nstyle: nickel\n>>>>>>> theirs\n---\n",
    );
    let checked = at(scratch.path(), &[".", "validate"]);
    let said = out(&checked);
    assert!(said.contains("a decimal comma? `,062`"), "{said}");
    assert!(said.contains("flow sequence at line 3 column 7"), "{said}");
    assert!(
        said.contains("line 3: a git merge left its conflict markers here"),
        "{said}"
    );
    let filtered = at(scratch.path(), &[".", "-f", "radius > 1,062"]);
    assert!(
        err(&filtered).contains("a decimal comma? a number is written with a point: 1.062"),
        "{}",
        err(&filtered)
    );
}

#[test]
fn a_command_offered_for_a_dashed_name_runs() {
    // `samplekit compute -odd.md --write`, offered by `set`, read `-o` as an
    // option; `./-odd.md` is read as the file.
    let scratch = Scratch::new("dashed-name");
    scratch.write(
        "-odd.md",
        "---\nschema_version: 1\nproperties:\n  radius: 1.05\n---\n",
    );
    let asked = at(scratch.path(), &["set", "--", "-odd.md"]);
    assert!(
        err(&asked).contains("samplekit ./-odd.md set radius=<value>"),
        "{}",
        err(&asked)
    );
    let offered = at(scratch.path(), &["./-odd.md", "set", "radius=1.06"]);
    assert_eq!(code(&offered), 0, "{}", err(&offered));
}

#[test]
fn status_json_reports_what_it_could_not_read() {
    // Under `--exit-code`, a file not read made the exit 2 and the report
    // said nothing of it: a script had an empty list and a failure.
    let scratch = Scratch::new("status-json-unread");
    scratch.corpus();
    scratch.write("broken.md", "---\nname: [x\n---\n");
    let status = at(scratch.path(), &[".", "status", "--json", "--exit-code"]);
    assert_eq!(code(&status), 2, "{}", err(&status));
    let report: serde_json::Value = serde_json::from_str(&out(&status)).unwrap();
    let unread: Vec<&serde_json::Value> = report
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["state"] == "not read")
        .collect();
    assert_eq!(unread.len(), 1, "{report}");
    assert!(
        unread[0]["sample"].as_str().unwrap().contains("broken.md"),
        "{report}"
    );
}

#[test]
fn init_gives_each_shell_its_own_completion_line() {
    // fish and elvish have no `<(…)`: the line `init` gave them failed.
    let scratch = Scratch::new("completion-shells");
    for (shell, line) in [
        ("/usr/bin/fish", "samplekit completions fish | source"),
        ("/bin/zsh", "source <(samplekit completions zsh)"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["init", "--no-venv"])
            .current_dir(scratch.path())
            .env("SHELL", shell)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(out(&output).contains(line), "{shell}: {}", out(&output));
    }
}

#[test]
fn init_says_the_model_runs_only_when_computing() {
    // The comment `init` wrote spoke of a consent withdrawn: *nothing runs it
    // until you allow it*, where computing runs it without asking.
    let scratch = Scratch::new("init-model-comment");
    let written = at(scratch.path(), &["init", "--write", "--no-venv"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(!rc.contains("until you allow it"), "{rc}");
    assert!(rc.contains("Only computing runs it"), "{rc}");
}

#[test]
fn view_draws_its_tables_in_the_table_style() {
    // `--table-style` was accepted by `view` and drew nothing differently.
    let scratch = Scratch::new("view-table-style");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\ntables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      \
         load: {}\n    rows:\n      - run: 1\n        load: 2\n---\n",
    );
    let plain = at(scratch.path(), &["a.md", "view"]);
    assert!(!out(&plain).contains('┌'), "{}", out(&plain));
    let boxed = at(scratch.path(), &["a.md", "view", "--table-style", "boxed"]);
    assert_eq!(code(&boxed), 0, "{}", err(&boxed));
    assert!(out(&boxed).contains('┌'), "{}", out(&boxed));
}

#[test]
fn help_takes_help_as_every_command_does() {
    // `samplekit help --help` read `--help` as a command to explain: exit 1.
    let scratch = Scratch::new("help-help");
    let helped = at(scratch.path(), &["help", "--help"]);
    assert_eq!(code(&helped), 0, "{}", err(&helped));
    assert!(
        out(&helped).contains("Usage: samplekit help"),
        "{}",
        out(&helped)
    );
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
    for (project, precision, diameter) in [("one", ".1f", 1.23456), ("two", ".3f", 2.34567)] {
        scratch.write(
            &format!("{project}/.samplekitrc"),
            &format!("schema_version = 1\n[property.diameter]\nprecision = \"{precision}\"\n"),
        );
        scratch.write(
            &format!("{project}/s.md"),
            &format!("---\nschema_version: 1\nname: {project}\nproperties:\n  diameter: {diameter}\n---\n"),
        );
    }
    for order in [["one", "two"], ["two", "one"]] {
        let written = at(
            scratch.path(),
            &[order[0], order[1], "-c", "name,diameter", "--csv"],
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
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, {field = \"porosity\"}]\n\
             [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"out/e.csv\"\n",
        );
    }
    scratch.write(
        "one/s.md",
        "---\nschema_version: 1\nname: fresh\nproperties:\n  porosity: 2.0\n---\n",
    );
    // A record naming an input digest that is not the input's: stale.
    scratch.write(
        "two/s.md",
        "---\nschema_version: 1\nname: behind\nproperties:\n  \
         diameter: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  porosity:\n    v: 3.0\n    \
         computed: {diameter: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
    );
    let output = at(scratch.path(), &["one", "two", "export", "e"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(
        said.contains("samplekit two compute --force --write"),
        "{said}"
    );
    assert!(!said.contains("samplekit one compute"), "{said}");
}

#[test]
fn a_target_that_does_not_exist_is_said() {
    let scratch = Scratch::new("target-missing");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "ductilities"]);
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
        &["billet-01.sample.md", "notes.py", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("billet-01"), "{}", out(&output));
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
    let output = at(scratch.path(), &[".", "-c", "porosity", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("1/3"), "{}", out(&output));
    assert!(!out(&output).contains(".samplekitrc"), "{}", out(&output));
}

#[test]
fn a_declared_export_creates_its_directory() {
    let scratch = Scratch::new("export-creates-directory");
    scratch.corpus();
    fs::remove_dir_all(scratch.path().join("out")).unwrap();
    let output = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(scratch.path().join("out/ductilities.csv").exists());
}

#[test]
fn a_narrowed_declared_export_says_so() {
    let scratch = Scratch::new("export-narrowed");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "-o",
            "-",
            "--query",
            "approved",
        ],
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
        "---\nschema_version: 1\nname: f\nproperties:\n  diameter: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  \
         loading:\n    computed: {diameter: aaaaaaaaaaaa}\n    \
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
        &["computed.sample.md", "view", "--width", "120"],
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
    let output = at(
        scratch.path(),
        &[".", "-c", "name,volume,ductility", "--json"],
    );
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
        &["item.sample.md", "explain", "wear.stats.mean"],
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
    soak(&scratch);
    let output = at(
        scratch.path(),
        &["soak.sample.md", "explain", "preheating.G[10]"],
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
    soak(&scratch);
    let output = at(scratch.path(), &["soak.sample.md", "explain", "preheating"]);
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
    assert!(shown.contains("preheating.<column>[<index>]"), "{shown}");
}

#[test]
fn list_takes_several_file_targets() {
    let scratch = Scratch::new("list-several");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["billet-01.sample.md", "billet-02.sample.md", "list"],
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
    let refused = at(scratch.path(), &["future.md", "validate"]);
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
    let output = at(scratch.path(), &["-c", "diameter", "--csv", "--json"]);
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
    let output = at(scratch.path(), &[".", "-c", "diameter", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let header = out(&output)
        .lines()
        .find(|line| line.contains("mean"))
        .unwrap_or_default()
        .to_string();
    for column in ["n", "mean", " s ", "sem", "median", "min", "max"] {
        assert!(header.contains(column), "{column}: {}", out(&output));
    }
    assert!(out(&output).contains("diameter [g]"), "{}", out(&output));
}

#[test]
fn a_summary_of_one_value_has_no_deviation() {
    let scratch = Scratch::new("summary-one");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &["billet-01.sample.md", "-c", "diameter", "--summary"],
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
        &[".", "tag", "rename", "my tag", "my_tag", "--write"],
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
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 1.0}\n---\nN.\n",
    );
    fs::create_dir_all(scratch.path().join("sub")).unwrap();
    scratch.write("sub/.samplekitrc", "schema_version = 1\n[view\n");
    scratch.write(
        "sub/b.sample.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  diameter: {v: 2.0}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "validate"]);
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: '000000000000'}\n---\nN.\n",
    );
    let hint = "samplekit compute lists what would run; --write computes and writes";
    let text = out(&at(scratch.path(), &[".", "status"]));
    assert!(text.contains(hint), "{text}");
    scratch.write(
        "drifted.sample.md",
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  diameter: 12.5\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &[".", "status"]));
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    \
         computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: {failed: ZeroDivisionError}\n---\nN.\n",
    );
    scratch.write(
        ".samplekit/failures/f.sample.log",
        "## loading\nTraceback (most recent call last):\nZeroDivisionError: division by zero\n",
    );
    let text = out(&at(scratch.path(), &[".", "status"]));
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    \
         computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: {failed: ZeroDivisionError}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &[".", "status"]));
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
    assert!(first.lines().any(|line| line == "diameter"), "{first}");
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
    let again = complete("diameter,");
    assert!(
        again
            .lines()
            .any(|line| line == "diameter,measurements:table"),
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
            "stty cols {columns} rows 40; {} . -c name,status,diameter,porosity",
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
    assert!(wide.contains("billet-01"), "{wide:?}");
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
        "model/specimen.py",
        "model/helpers/__init__.py",
        "model/helpers/measuring.py",
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
    fs::remove_file(scratch.path().join("model/helpers/measuring.py")).unwrap();
    let again = set_up(&scratch);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("already here, kept"),
        "{}",
        out(&again)
    );
    assert!(scratch.path().join("model/helpers/measuring.py").exists());
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
    assert!(shown.contains("samplekit samples/ compute"), "{shown}");
    assert!(shown.contains("samplekit samples/ status"), "{shown}");
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
        let all: Vec<&str> = suggested.split_whitespace().collect();
        // The path first, where one is given, as every command takes it.
        let path_like = |word: &str| kind(word) == Some("a path") || word.contains('/');
        let (lead, words) = match all.split_first() {
            Some((first, rest)) if path_like(first) => (Some("a path"), rest.to_vec()),
            _ => (None, all.clone()),
        };
        let verb: Vec<&str> = words
            .iter()
            .take_while(|word| {
                !word.starts_with('-') && word.chars().all(|c| c.is_lowercase() || c == '-')
            })
            .copied()
            .collect();
        if verb.is_empty() {
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
        // Each usage line of the command: `samplekit [TARGET] tag add <TAG>`.
        let usages: Vec<Vec<String>> = out(&help)
            .lines()
            .map(|line| line.trim_start().trim_start_matches("Usage:").trim())
            .filter(|line| line.starts_with("samplekit "))
            .map(|line| {
                line.split_whitespace()
                    .skip(1)
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        let used: Vec<&'static str> = words[verb.len()..].iter().filter_map(|w| kind(w)).collect();
        let read = usages.iter().any(|usage| {
            let (declared_lead, rest) = match usage.split_first() {
                Some((first, rest)) if first.starts_with(['[', '<']) => (kind(first), rest),
                _ => (None, &usage[..]),
            };
            let named: Vec<&str> = rest.iter().take(verb.len()).map(String::as_str).collect();
            if named != verb {
                return false;
            }
            let declared: Vec<&'static str> = rest[verb.len()..]
                .iter()
                .filter(|word| word.contains('<') || word.contains('['))
                .filter_map(|w| kind(w))
                .collect();
            (lead.is_none() || declared_lead == lead)
                && used
                    .iter()
                    .enumerate()
                    .all(|(position, wanted)| declared.get(position) == Some(wanted))
        });
        assert!(
            read,
            "init suggests '{suggested}', which no usage line of '{}' reads: {usages:?}",
            verb.join(" ")
        );
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
    let output = at(scratch.path(), &["new", "C-02", "diameter=12.4", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("samples/C-02.md")).unwrap();
    assert!(!written.contains("name:"), "{written}");
    assert!(written.contains("12.4"), "{written}");
    // The pattern's other quantities are a measurement nobody made.
    assert!(!written.contains("hb"), "{written}");
    let named = at(scratch.path(), &["-c", "name", "samples/C-02.md"]);
    assert!(out(&named).contains("C-02"), "{}", out(&named));
    scratch.write(
        "samples/C-01.md",
        "---\nschema_version: 1\nname: first\nbatch: B7\n---\n",
    );
    let kept = at(
        scratch.path(),
        &[
            "new",
            "C-03",
            "--keep",
            "samples/C-01.md",
            "batch",
            "--write",
        ],
    );
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let written = fs::read_to_string(scratch.path().join("samples/C-03.md")).unwrap();
    assert!(!written.contains("name:"), "{written}");
    assert!(written.contains("batch: B7"), "{written}");
}

#[test]
fn the_history_keeps_new_keep_as_it_runs() {
    // `--keep SAMPLE FIELDS` takes two: its fields were kept as the command's
    // own word, `new lead volume --keep a.md`, which does not run again.
    let scratch = Scratch::new("keep-logged");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  width: {v: 12.5, unit: mm}\n---\n",
    );
    let written = at(
        scratch.path(),
        &["new", "b", "--keep", "a.md", "width", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let log = out(&at(scratch.path(), &["log"]));
    assert!(
        log.contains("samplekit new b --keep a.md width --write"),
        "{log}"
    );
}

#[test]
fn new_keep_takes_what_it_names_and_nothing_else() {
    // Another sample's attributes, the shape of its quantities and its tables
    // came across unasked: only the fields named are taken.
    let scratch = Scratch::new("new-keep-only");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "pieces/titanium.md",
        "---\nschema_version: 1\nname: titanium\nstyle: titanium\ntester: tom\npoured: 2026-01-24\n\
         properties:\n  volume: {v: 20.5, unit: L}\n  radius: {v: 1.064, unit: SG}\n\
         tables:\n  trial:\n    index: day\n    columns:\n      day: {}\n      hardness: {}\n    \
         rows:\n      - day: 1\n        hardness: 1.05\n---\n",
    );
    let preview = at(
        scratch.path(),
        &[
            "new",
            "lead",
            "--keep",
            "pieces/titanium.md",
            "volume,style",
        ],
    );
    assert_eq!(code(&preview), 0, "{}", err_slashed(&preview));
    let shown = out_slashed(&preview);
    assert!(
        shown.contains("taken from pieces/titanium.md   volume, style = titanium"),
        "{shown}"
    );
    let written = at(
        scratch.path(),
        &[
            "new",
            "lead",
            "--keep",
            "pieces/titanium.md",
            "volume,style",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err_slashed(&written));
    // Beside the sample it took from.
    let file = fs::read_to_string(scratch.path().join("pieces/lead.md")).unwrap();
    assert!(
        file.contains("20.5") && file.contains("style: titanium"),
        "{file}"
    );
    for absent in ["tester", "poured", "radius", "trial", "name:"] {
        assert!(!file.contains(absent), "{absent}: {file}");
    }
    // --like is no option any more; --keep wants the sample and the fields.
    let like = at(
        scratch.path(),
        &["new", "x", "--like", "pieces/titanium.md"],
    );
    assert_eq!(code(&like), 1, "{}", err_slashed(&like));
    let half = at(
        scratch.path(),
        &["new", "x", "--keep", "pieces/titanium.md"],
    );
    assert_eq!(code(&half), 1, "{}", err_slashed(&half));
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
    let repaired = at(scratch.path(), &["old.md", "set", "volume=20.5", "--write"]);
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
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Piece\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\n\nclass Piece(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.ageing = sk.Table(\n            {\n                \
         \"day\": sk.Property(unit=\"d\"),\n                \"hardness\": sk.Property(),\n            },\n            \
         index=\"day\",\n        )\n",
    );
    scratch.write("b1.md", "---\nschema_version: 1\nname: b1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({}),
        serde_json::json!({"ageing": {
            "title": null, "index": ["day"], "rows": [],
            "columns": [
                {"name": "day", "unit": "d", "value": {"from": "entered"},
                 "uncertainty": null, "formula": null},
                {"name": "hardness", "unit": null, "value": {"from": "entered"},
                 "uncertainty": null, "formula": null},
            ],
        }}),
    );
    let added = at(
        scratch.path(),
        &[
            "b1.md",
            "set",
            "--add-row",
            "ageing",
            "hardness=1.040",
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
    assert!(said.contains(" set <field>=<value>"), "{said}");
}

/// One entered value and one the file says a formula produced, with the records
/// that make it derived. No model: `set` answers from the file.
fn derived(scratch: &Scratch) -> PathBuf {
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s.sample.md",
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  porosity:\n    v: 3.0\n    \
         computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n---\nNotes.\n",
    )
}

#[test]
fn set_previews_before_it_writes() {
    let scratch = Scratch::new("set-preview");
    let file = derived(&scratch);
    let before = fs::read_to_string(&file).unwrap();
    let preview = at(scratch.path(), &["s.sample.md", "set", "diameter=13.0"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("nothing written"),
        "{}",
        out(&preview)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let written = at(
        scratch.path(),
        &["s.sample.md", "set", "diameter=13.0", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(fs::read_to_string(&file).unwrap().contains("13"));
}

#[test]
fn set_says_what_a_change_makes_stale() {
    let scratch = Scratch::new("set-stale");
    derived(&scratch);
    let output = at(scratch.path(), &["s.sample.md", "set", "diameter=13.0"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    // What was already not current is counted, not listed again.
    assert!(
        shown.contains("nothing more becomes not current — 1 value was already"),
        "{shown}"
    );
    assert!(shown.contains("samplekit s.sample.md compute"), "{shown}");
}

#[test]
fn set_on_a_derived_value_writes_the_override_marked() {
    // What makes it derived is the record in the file, not the model: the answer
    // is the same with no interpreter in sight. And the file says so: the
    // formula's record stays, the value's own digest is marked edited, and
    // `status` reads it as edited — not as record missing.
    let scratch = Scratch::new("set-override");
    let file = derived(&scratch);
    let output = at(scratch.path(), &["s.sample.md", "set", "porosity=9.0"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("typed over its formula"),
        "{}",
        out(&output)
    );
    // Compute keeps an override: the way back is --force, not a plain compute.
    assert!(out(&output).contains("--force --write"), "{}", out(&output));
    assert!(
        !out(&output).contains("recomputes them"),
        "{}",
        out(&output)
    );
    let written = at(
        scratch.path(),
        &["s.sample.md", "set", "porosity=9.0", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("computed: {diameter: aaaaaaaaaaaa}"),
        "{text}"
    );
    assert!(text.contains("fingerprint: {edited: "), "{text}");
    let status = at(scratch.path(), &["s.sample.md", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("porosity")
            && out(&status).contains("edited — typed over its formula"),
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
         diameter:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: a91f42c8f0d1}\n    fingerprint: d312b70e4a55\n---\n",
    );
    let output = at(scratch.path(), &["w.sample.md", "explain", "diameter"]);
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
         diameter: {v: 12.5, unit: g, fingerprint: {edited: 7c01e2b3a9f4}}\n---\n",
    );
    let output = at(scratch.path(), &["w.sample.md", "explain", "diameter"]);
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
         diameter:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: '000000000000'}\n---\n",
    );
    let output = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("diameter"), "{shown}");
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
    let output = at(scratch.path(), &["s.md", "set", "a.readings=1,3,20"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("3 readings: 1, 3, 20, their median 3"),
        "{}",
        out(&output)
    );
}

#[test]
fn new_readings_are_said_to_the_place_of_their_uncertainty() {
    // `their mean 1.99133` for readings whose standard error is 0.0015.
    let scratch = Scratch::new("readings-place");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  thickness:\n    v: 2.0\n    \
         readings: [1.995, 2.004, 2.008]\n    u: 0.0038\n    unit: mm\n    \
         statistics: {v: mean, u: standard_error}\n    computed: {readings: aaaaaaaaaaaa}\n    \
         fingerprint: bbbbbbbbbbbb\n---\n",
    );
    let output = at(
        scratch.path(),
        &["s.md", "set", "thickness.readings=1.991,1.994,1.989"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("their mean 1.9913\n"),
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
         diameter: {v: 3.0571, readings: [3.0571, 3.0557, 3.0584], u: 0.00078, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["w.sample.md", "set", "diameter=4.8", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(shown.contains("keeps 3 readings"), "{shown}");
    assert!(shown.contains("3.0557"), "{shown}");
    // No statistic of them is recorded, so nothing computes the value back: the
    // way named is removing the value, never a `--force` that would refuse, and
    // no mean is offered in its place.
    assert!(
        shown.contains("diameter= removes it") && !shown.contains("--force"),
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
    // `diameter.v=` offered `diameter.v.u=`, which is no field.
    let channel = at(scratch.path(), &["w.sample.md", "set", "diameter.v=4.9"]);
    assert_eq!(code(&channel), 0, "{}", err(&channel));
    assert!(
        out(&channel).contains("diameter.u=0.01"),
        "{}",
        out(&channel)
    );
    assert!(!out(&channel).contains("diameter.v."), "{}", out(&channel));
}

#[test]
fn set_on_a_cell_keeps_the_channel_not_written() {
    // A cell rebuilt from the value alone lost its uncertainty silently.
    let scratch = Scratch::new("set-cell-channels");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "w.sample.md",
        "---\nschema_version: 1\nname: W\ntables:\n  measurements:\n    index: T\n    \
         columns:\n      T: {}\n      f: {unit: porosity}\n    rows:\n      - T: 20.0\n        \
         f: {v: 4.5437, u: 0.0003}\n---\n",
    );
    let set = |assignment: &str| {
        let output = at(
            scratch.path(),
            &["w.sample.md", "set", assignment, "--write"],
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
    // The value alone, its unit its column's: written bare, as the other rows.
    assert!(written.contains("f: 4.6\n"), "{written}");
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
         diameter:\n    v: 12.5\n    readings: [12.4, 12.5, 12.6]\n    \
         u: 0.0577350269\n    unit: g\n    \
         statistics: {u: standard_error}\n    \
         computed: {readings: a91f42c8f0d1}\n    fingerprint: d312b70e4a55\n---\n",
    );
    let output = at(scratch.path(), &["w.sample.md", "set", "diameter=13"]);
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
    let output = at(scratch.path(), &[".", "-c", "status,diameter", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains("status"), "{}", out(&output));
    assert!(out(&output).contains("diameter [g]"), "{}", out(&output));
    assert!(
        err(&output).contains("status left out of the summary"),
        "{}",
        err(&output)
    );
    let csv = at(
        scratch.path(),
        &[".", "-c", "status,diameter", "--summary", "--csv"],
    );
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert!(!out(&csv).contains("status,"), "{}", out(&csv));
    assert!(
        out(&csv).lines().any(|line| line.starts_with("diameter,")),
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
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\nN.\n",
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
        vec![".", "-c", "diameter"],
        vec!["a.sample.md", "-c", "diameter"],
        vec![".", "list", "skipped"],
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
    let table = at(scratch.path(), &[".", "-c", "diameter"]);
    assert!(out(&table).contains('1'), "{}", out(&table));
    let validated = at(scratch.path(), &[".", "validate"]);
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
    let declared = at(scratch.path(), &[".", "--profile", "ductilities"]);
    assert_eq!(code(&declared), 0, "{}", err(&declared));
    let columns = at(
        scratch.path(),
        &[".", "--profile", "ductilities", "-c", "name,diameter"],
    );
    assert_eq!(code(&columns), 0, "{}", err(&columns));
    // The corpus renders boxed, so the header is the line naming a column.
    let header = out(&columns)
        .lines()
        .find(|line| line.contains("diameter"))
        .unwrap_or_default()
        .to_string();
    assert!(header.contains("diameter"), "{}", out(&columns));
    assert!(!out(&columns).contains("porosity"), "{}", out(&columns));
    // The profile's order survives `-c`: `-porosity`, the unmeasured one last. The
    // declared table names no sample, so its order is read off the diameters.
    let shown = out(&declared);
    let diameters: Vec<&str> = shown
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with("12.") || word.starts_with("9."))
        })
        .collect();
    assert_eq!(
        diameters,
        ["12.30", "12.20", "12.10", "9.70"],
        "{}",
        out(&declared)
    );
    let by_profile = ["billet-03", "billet-02", "billet-01", "billet-04"];
    assert_eq!(order(&out(&columns)), by_profile, "{}", out(&columns));
    let sorted = at(
        scratch.path(),
        &[
            ".",
            "--profile",
            "ductilities",
            "-c",
            "name,diameter",
            "-s",
            "diameter",
        ],
    );
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    assert_eq!(
        order(&out(&sorted)),
        ["billet-04", "billet-01", "billet-02", "billet-03"],
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
            "{rc}\n[query.light]\nfilter = \"diameter < 12.25\"\n\n[export.light]\n\
             profile = \"ductilities\"\nformat = \"csv\"\noutput = \"out/light.csv\"\nquery = \"light\"\n"
        ),
    );
    let output = at(scratch.path(), &[".", "export", "light", "-o", "-"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let rows = out(&output).lines().count() - 1;
    assert_eq!(rows, 3, "{}", out(&output));
    assert!(!err(&output).contains("narrowed"), "{}", err(&output));
    // A filter on the command line narrows further, and is said.
    let narrowed = at(
        scratch.path(),
        &[".", "export", "light", "-o", "-", "-f", "diameter > 12.05"],
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
        "python = \"scripts.piece_model:PieceModel\"\n",
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
    assert!(said.contains("this project is set up"), "{said}");
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
            "{rc}\n[export.all]\nprofile = \"ductilities\"\nformat = \"csv\"\noutput = \"out/all.csv\"\n"
        ),
    );
    let output = at(
        scratch.path(),
        &[".", "export", "all", "-o", "-", "-f", "diameter > 1000000"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("unknown field"), "{}", err(&output));
    assert!(err(&output).contains("0 of"), "{}", err(&output));
    assert_eq!(out(&output).lines().count(), 1, "{}", out(&output));
    // A field nobody holds is still refused, against what was selected from.
    let misspelt = at(
        scratch.path(),
        &[".", "export", "all", "-o", "-", "-f", "diameterr > 1000000"],
    );
    assert_eq!(code(&misspelt), 1, "{}", err(&misspelt));
}

#[test]
fn set_of_an_unknown_name_writes_an_attribute_and_warns() {
    let scratch = Scratch::new("set-unknown");
    derived(&scratch);
    let output = at(scratch.path(), &["s.sample.md", "set", "humidity=45"]);
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
            "s.sample.md",
            "set",
            "diameter.readings=15.34,15.36,15.31",
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
            "s.sample.md",
            "set",
            "diameter.readings=15.34,15.36,15.31",
            "--write",
        ],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    // Readings replace readings, not a value their mean only stood for.
    let again = at(
        scratch.path(),
        &["s.sample.md", "set", "diameter.readings=[15.3, 15.4]"],
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
            "s.sample.md",
            "set",
            "diameter.readings=1,2",
            "diameter.readings=3,4",
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
        "---\nschema_version: 1\nname: T\ntables:\n  preheating:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n      derived: {}\n    rows:\n      \
         - step: 1\n        reading: 2.0\n        derived:\n          v: 4.0\n          \
         computed: {row.reading: aaaaaaaaaaaa}\n          fingerprint: bbbbbbbbbbbb\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &[
            "t.sample.md",
            "set",
            "--add-row",
            "preheating",
            "step=2",
            "reading=3.0",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let shown = out(&output);
    assert!(
        shown.contains("t.sample.md · a row in preheating"),
        "{shown}"
    );
    assert!(shown.contains("preheating.reading   —  →  3"), "{shown}");
    assert!(shown.contains("derived"), "{shown}");
}

#[test]
fn set_refuses_a_column_a_table_does_not_have() {
    let scratch = Scratch::new("set-no-column");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "t.sample.md",
        "---\nschema_version: 1\nname: T\ntables:\n  preheating:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n    rows:\n      - {step: 1, reading: 2.0}\n---\nN.\n",
    );
    let output = at(
        scratch.path(),
        &["t.sample.md", "set", "preheating.nocol[1]=3"],
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
    let output = at(scratch.path(), &["s.sample.md", "set"]);
    assert_ne!(code(&output), 0, "{}", out(&output));
    assert!(
        err(&output).contains("wants what to change"),
        "{}",
        err(&output)
    );
}

#[test]
fn set_reports_a_value_that_does_not_change_as_unchanged() {
    // `alumina → alumina` counted as a change is a lie told exactly where
    // someone is deciding whether to write.
    let scratch = Scratch::new("set-unchanged");
    let file = derived(&scratch);
    let before = fs::read_to_string(&file).unwrap();
    let output = at(
        scratch.path(),
        &["s.sample.md", "set", "diameter=12.5", "--write"],
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
        "---\nschema_version: 1\nname: T\ntables:\n  preheating:\n    index: step\n    \
         columns:\n      step: {}\n      reading: {}\n    rows:\n      - {step: 1, reading: 2.0}\n---\nN.\n",
    );
    let cell = at(
        scratch.path(),
        &["t.sample.md", "set", "nosuch.reading[1]=3"],
    );
    let row = at(
        scratch.path(),
        &["t.sample.md", "set", "--add-row", "nosuch", "step=2"],
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
         diameter: {readings: [4.73, 4.72, 4.74], unit: g}\n---\nN.\n",
    );
    let before = fs::read_to_string(&file).unwrap();
    let output = at(
        scratch.path(),
        &["s.sample.md", "set", "diameter.readings=5", "--write"],
    );
    assert_ne!(code(&output), 0, "{}", out(&output));
    // One number is a value, and is named so.
    assert!(err(&output).contains("diameter=5"), "{}", err(&output));
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: '000000000000'}\n---\nN.\n",
    );
    let plain = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    let gated = at(scratch.path(), &[".", "status", "--exit-code"]);
    assert_eq!(code(&gated), 2, "{}", err(&gated));
    assert!(err(&gated).contains("not current"), "{}", err(&gated));

    // Nothing to report exits 0, gate or no gate: the check passed.
    let clean = Scratch::new("status-gate-clean");
    clean.config();
    clean.sample("b.sample.md", "billet-02", "approved", 12.5, None);
    let output = at(clean.path(), &[".", "status", "--exit-code"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
}

#[test]
fn records_forming_a_cycle_are_never_read_as_current() {
    // One hand-edited record used to empty the whole sample's verdict, and
    // `status` answered *every value is current* over a diameter just changed.
    let one = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::Integer(1)),
        ..Default::default()
    });
    let scratch = Scratch::new("status-cycle");
    scratch.config();
    scratch.write(
        "looped.sample.md",
        &format!(
            "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
             diameter: {{v: 12.5}}\n  \
             a: {{v: 1, computed: {{b: {one}}}}}\n  \
             b: {{v: 1, computed: {{a: {one}}}}}\n  \
             ductility: {{v: 2.8, computed: {{diameter: '000000000000'}}}}\n---\nN.\n"
        ),
    );
    let status = at(scratch.path(), &[".", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    let said = out(&status);
    // The cycle is named with its path, and the unrelated stale value is still
    // judged: the rest of the sample keeps its answer.
    assert!(said.contains("not judged"), "{said}");
    assert!(said.contains("a \u{2192} b \u{2192} a"), "{said}");
    assert!(said.contains("outdated \u{2014} diameter"), "{said}");
    assert!(!said.contains("is current"), "{said}");

    let gated = at(scratch.path(), &[".", "status", "--exit-code"]);
    assert_eq!(code(&gated), 2, "{}", err(&gated));

    // `validate` holds the cycle as a defect, and the stale value as a note.
    let validated = at(scratch.path(), &[".", "validate"]);
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
    let json = at(scratch.path(), &[".", "-c", "a,ductility", "--json"]);
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
         diameter: {v: 12.5}\n  ductility: {v: 2.8, computed: {diameter: '000000000000'}}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "compute"]);
    assert_eq!(code(&output), 2, "{}", err(&output));
    // What can be planned is, and is read first.
    assert!(out(&output).contains("ductility"), "{}", out(&output));
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
    // `[property.diameter]` says this name is a measured quantity and how it is
    // written. Writing an attribute there ignores the answer the project
    // already gives — and no model is run to learn it.
    let scratch = Scratch::new("declared-quantity");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[property.diameter]\nunit = \"g\"\nsymbol = \"m\"\nprecision = \".2f\"\n",
    );
    let output = at(scratch.path(), &["new", "C-01", "diameter=12.4", "--write"]);
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
        &["C-01.md", "set", "diameter.u=0.05", "--write"],
    );
    assert_eq!(code(&channel), 0, "{}", err(&channel));
}

#[test]
fn a_name_nobody_declares_is_still_an_attribute() {
    // Nothing else can be guessed about a name that appears for the first time
    // and that no configuration describes.
    let scratch = Scratch::new("undeclared-attribute");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let output = at(scratch.path(), &["new", "C-02", "diameter=9.9", "--write"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("written as an attribute"),
        "{}",
        err(&output)
    );
    let written = fs::read_to_string(scratch.path().join("C-02.md")).unwrap();
    assert!(!written.contains("properties:"), "{written}");
    assert!(written.contains("diameter: 9.9"), "{written}");
}

#[test]
fn init_leaves_a_collection_in_an_older_format_alone() {
    // `init` sets a project up and converts nothing: a file in another
    // version's form stays exactly as it is.
    let scratch = Scratch::new("init-older-format");
    scratch.write(
        "samples/old.md",
        "---\nname: OLD\nproperties:\n  diameter: {v: 1.0, precision_unc: \".2f\"}\n---\nOld.\n",
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
    // Left out as no sample, as a README is; `validate` notes it.
    let validated = at(scratch.path(), &[".", "validate"]);
    assert!(
        out(&validated).contains("no schema_version"),
        "{}",
        out(&validated)
    );
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
    // Apart from what is written, which it read as one of.
    fs::create_dir_all(scratch.path().join("elsewhere")).unwrap();
    let preview = at(&scratch.path().join("elsewhere"), &["init", "--no-venv"]);
    assert!(
        out(&preview).contains("\n\n  completion, by hand"),
        "{}",
        out(&preview)
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
        "laboratory/beam.py",
        "import samplekit as sk\n\nclass Beam(sk.Sample):\n    pass\n",
    );
    let output = at_terminal(
        scratch.path(),
        &["init"],
        "1\n2\nlaboratory/none.py\nlaboratory/beam.py\n2\ny\n",
    );
    let said = out(&output);
    assert_eq!(code(&output), 0, "{said}");
    assert!(said.contains("the model file's path"), "{said}");
    assert!(said.contains("no file at laboratory/none.py"), "{said}");
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(rc.contains("path = \"laboratory/beam.py\""), "{rc}");
    assert!(!scratch.path().join("model").exists());
    // What comes next opens that file.
    assert!(said.contains("$EDITOR laboratory/beam.py"), "{said}");
}

#[test]
#[cfg(unix)]
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
        "---\nschema_version: 1\nname: billet-01\nproperties:\n  \
         diameter: {v: 12.5, fingerprint: aaaaaaaaaaaa}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: '000000000000'}\n---\nN.\n",
    );
    let shown = out(&in_colour(scratch.path(), &[".", "status"]));
    assert!(shown.contains("\u{1b}[33m"), "{shown:?}");
    // The command it invites is bold.
    assert!(shown.contains("\u{1b}[1m"), "{shown:?}");
    // NO_COLOR turns all of it off, in the same terminal.
    let quiet = at_terminal(scratch.path(), &[".", "status"], "");
    assert!(!out(&quiet).contains('\u{1b}'), "{:?}", out(&quiet));
    // And nothing reaches a pipe.
    let piped = at(scratch.path(), &[".", "status"]);
    assert!(!out(&piped).contains('\u{1b}'), "{:?}", out(&piped));
}

#[test]
#[cfg(target_os = "linux")]
fn the_command_line_wears_the_palette_and_a_file_does_not() {
    // White from end to end: a value, an attribute and a unit read alike.
    let scratch = Scratch::new("palette");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[colors]\nunit = \"magenta\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nmaterial: steel\nproperties:\n  \
         width: {v: 12.5, u: 0.01, unit: mm}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nmaterial: copper\nproperties:\n  \
         width: {v: 9.25, u: 0.02, unit: mm}\n---\n",
    );
    let shown = out(&in_colour(
        scratch.path(),
        &[".", "-c", "name,material,width"],
    ));
    // The value bold, the attribute blue, the header bold with its unit in the
    // project's colour.
    assert!(shown.contains("\u{1b}[1m12.50"), "{shown:?}");
    assert!(shown.contains("\u{1b}[34msteel"), "{shown:?}");
    assert!(shown.contains("\u{1b}[1mwidth"), "{shown:?}");
    assert!(shown.contains("\u{1b}[35m[mm]"), "{shown:?}");
    // The colours take no room: the table reads as the plain one.
    let plain = out(&at_terminal(
        scratch.path(),
        &[".", "-c", "name,material,width"],
        "",
    ));
    let drawn: String = {
        let mut drawn = String::new();
        let mut chars = shown.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\u{1b}' {
                for next in chars.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                drawn.push(character);
            }
        }
        drawn
    };
    assert_eq!(drawn, plain, "{shown:?}");
    // `view` colours the unit after its number.
    let viewed = out(&in_colour(scratch.path(), &["a.md", "view"]));
    assert!(viewed.contains("\u{1b}[35mmm"), "{viewed:?}");
    // A file written from a terminal carries none of it.
    let written = in_colour(
        scratch.path(),
        &[".", "-c", "name,material,width", "-o", "t.md", "--write"],
    );
    assert!(
        out(&written).contains("\u{1b}[32mwritten:"),
        "{:?}",
        out(&written)
    );
    let file = std::fs::read_to_string(scratch.path().join("t.md")).unwrap();
    assert!(!file.contains('\u{1b}'), "{file:?}");
    // An error in its colour, and the advice after it muted.
    let wrong = out(&in_colour(scratch.path(), &[".", "-c", "zzzz"]));
    assert!(wrong.contains("\u{1b}[31merror:"), "{wrong:?}");
    assert!(
        wrong.contains("\u{1b}[90msamplekit list fields"),
        "{wrong:?}"
    );
    // A state whose words no list holds, an edited value, coloured all the same.
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  width: {v: 1.0, unit: mm}\n  \
         twice:\n    v: 9.0\n    computed: {width: '000000000000'}\n    fingerprint: {edited: aaaaaaaaaaaa}\n---\n",
    );
    let said = out(&in_colour(scratch.path(), &[".", "status"]));
    assert!(said.contains("\u{1b}[35medited"), "{said:?}");
}

#[test]
fn a_query_error_names_the_query_and_points_into_what_it_says() {
    // Over several configurations, a declared query's filter was parsed inside
    // a conjunction nobody wrote: the caret pointed into `(diameter >)`, and
    // nothing named the query or its file.
    let scratch = Scratch::new("query-error");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[query.broken]\nfilter = \"diameter >\"\n",
    );
    scratch.sample("a.sample.md", "root-one", "approved", 2.0, None);
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n[query.broken]\nfilter = \"diameter >\"\n",
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
        assert!(!err(&output).contains("(diameter >)"), "{}", err(&output));
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
         diameter: {v: 1.0, fingerprint: aaaaaaaaaaaa}\n  loading:\n    v: 3.0\n    \
         computed: {diameter: aaaaaaaaaaaa}\n    fingerprint: {edited: bbbbbbbbbbbb}\n---\nN.\n",
    );
    let text = out(&at(scratch.path(), &[".", "status"]));
    assert!(text.contains("--force"), "{text}");
    assert!(!text.contains("lists what would run"), "{text}");
}

#[test]
fn a_reading_whose_uncertainty_a_formula_gives_is_measured_in_a_new_sample() {
    // A record whose channel is the uncertainty's says the value beside it was
    // entered: --keep takes it, as a measurement, and refuses nothing.
    let scratch = Scratch::new("new-keep-measured");
    scratch.config();
    scratch.write(
        "BR-01.md",
        "---\nschema_version: 1\nname: BR-01\nproperties:\n  \
         preheat_diameter:\n    v: 18.42\n    u: 0.01842\n    unit: mg\n    \
         computed: {u: {preheat_diameter.v: 0123456789ab}}\n    fingerprint: 0123456789ab\n  \
         hb: {v: 15.0, unit: mL}\n---\nN.\n",
    );
    let preview = at(
        scratch.path(),
        &["new", "BR-02", "--keep", "BR-01.md", "preheat_diameter,hb"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(said.contains("preheat_diameter, hb"), "{said}");
    assert!(!said.contains("model computes"), "{said}");
}

#[test]
fn init_checks_before_it_writes_anything() {
    // A file where a directory must go stopped the run halfway, having written
    // some files and said nothing of them.
    let scratch = Scratch::new("init-preflight");
    scratch.write("model", "in the way\n");
    let output = at(
        scratch.path(),
        &[".", "init", "--example", "--write", "--no-venv"],
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let unread = scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties: [\n---\n",
    );
    let tagged = at(scratch.path(), &[".", "tag", "add", "seen", "--write"]);
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
        "---\nschema_version: 1\nname: B\nproperties:\n  diameter: {v: 2.0, unit: g}\n---\n",
    );
    let mode = |bits: u32| {
        fs::set_permissions(&unread, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };
    mode(0o000);
    let tagged = at(scratch.path(), &[".", "tag", "add", "again", "--write"]);
    mode(0o644);
    assert_eq!(code(&tagged), 3, "{}", err(&tagged));
}

/// A file writes a declared precision's text, as the screen does.
#[test]
fn a_file_writes_a_number_as_its_precision_writes_it() {
    let scratch = Scratch::new("written-precision");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.diameter]\nprecision = \".2e\"\n\
         [property.period]\nprecision = \".0f\"\n[property.flatness]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 12.3, u: 0.00006}\n  \
         period: 6.0\n  flatness: {v: 0.05, u: 0.0002}\n  free: 6.0\n---\n",
    );
    let csv = out(&at(
        scratch.path(),
        &["-c", "diameter,period,flatness,free", "--csv"],
    ));
    // The uncertainty columns no sample fills are left out; an uncertainty is
    // rounded up, never written as zero.
    assert!(
        csv.ends_with("1.23e+01,6.00e-05,6,0.050,0.001,6.0\n"),
        "{csv}"
    );
    let json = out(&at(scratch.path(), &["-c", "diameter,period", "--json"]));
    assert!(json.contains("\"value\": 1.23e+01"), "{json}");
    assert!(json.contains("\"value\": 6,"), "{json}");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed[0]["diameter"]["value"], 12.3);
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
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
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
    let listed = out(&at(scratch.path(), &[".", "list", "files"]));
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
    let one = open(&["B24.md", "open", "*x500*"]);
    assert_eq!(code(&one), 0, "{}", err(&one));
    assert!(
        out(&one).contains("opened images/B24_photo_x500.png"),
        "{}",
        out(&one)
    );
    let folder = open(&["B24.md", "open", "--navigate"]);
    assert_eq!(code(&folder), 0, "{}", err(&folder));
    // Every file a pattern names opens.
    let several = open(&["B24.md", "open"]);
    assert_eq!(code(&several), 0, "{}", err(&several));
    assert!(
        out(&several).contains("opened images/B24_photo_x2000.png")
            && out(&several).contains("opened images/B24_photo_x500.png"),
        "{}",
        out(&several)
    );
    let none = open(&["B2.md", "open"]);
    assert_eq!(code(&none), 1, "{}", err(&none));
    // More than five open only after a yes, which no terminal here can give.
    for index in 0..4 {
        scratch.write(&format!("images/B24_extra{index}.png"), "");
    }
    let many = open(&["B24.md", "open"]);
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
        "---\nschema_version: 1\nname: W\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &["w.sample.md", "set", "diameter=2", "diameter=3", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("diameter is given twice"),
        "{}",
        err(&output)
    );
    assert!(fs::read_to_string(&file).unwrap().contains("v: 1.0"));
    // One field however it is spelled: diameter and diameter.v are the same value.
    let spelled = at(
        scratch.path(),
        &["w.sample.md", "set", "diameter=2", "diameter.v=3"],
    );
    assert_eq!(code(&spelled), 1, "{}", out(&spelled));
    assert!(err(&spelled).contains("given twice"), "{}", err(&spelled));
}

#[test]
fn an_uncertainty_never_computed_is_named_in_a_preview() {
    // A volume entered, its uncertainty a formula's never computed: the
    // preview said the volume never computed beside its number in the file.
    let scratch = Scratch::new("never-uncertainty");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write("model.py", "import samplekit as sk\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  volume: {v: 20.0, unit: L}\n---\n",
    );
    describe(
        scratch.path(),
        serde_json::json!({"volume": {
            "unit": "L", "value": {"from": "entered"},
            "uncertainty": {"from": "formula", "reads": []},
            "reads": [], "formula": "abcdef", "default": null,
        }}),
        serde_json::json!({}),
    );
    let output = at(scratch.path(), &[".", "-c", "name,volume", "-o", "x.csv"]);
    assert!(
        out(&output).contains("A volume's uncertainty"),
        "{}\n{}",
        out(&output),
        err(&output)
    );
}

#[test]
fn a_value_never_computed_is_said_in_the_state_column() {
    // A derived value one sample holds and the other has never been given:
    // `current` hid it.
    let scratch = Scratch::new("status-never");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 1.0, unit: g, fingerprint: '111111111111'}\n  \
         twice: {v: 2.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    scratch.write(
        "b.sample.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  diameter: {v: 3.0, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-c", "name,twice", "--status", "--csv"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("never_computed: twice"), "{text}");
    // The drawn table says it as the CSV does.
    let drawn = at(scratch.path(), &[".", "-c", "name,twice", "--status"]);
    assert!(
        out(&drawn).contains("never_computed: twice"),
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
         super().__init__(path, name=name)\n        self.porosity = sk.Property(compute_value=lambda: 1.0)\n",
    );
    for format in [None, Some("--csv")] {
        let mut args = vec![".", "-c", "name,porosity", "--status"];
        args.extend(format);
        let output = at(scratch.path(), &args);
        assert!(
            out(&output).contains("never_computed: porosity"),
            "{}",
            out(&output)
        );
    }
    let json = at(scratch.path(), &[".", "-c", "name,porosity", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out(&json)).unwrap();
    assert!(
        parsed[0]["porosity"]["value"].is_null()
            && parsed[0]["porosity"]["state"] == "never_computed",
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
    let one = at(scratch.path(), &["A.md", "list", "files"]);
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
    let all = out(&at(scratch.path(), &[".", "list", "files"]));
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 12.0, readings: [11.9, 12.1], unit: g}\n---\n",
    );
    scratch.write("other.md", "---\nschema_version: 1\nname: B\n---\n");
    let kept = at(
        scratch.path(),
        &[
            "new", "C", "--keep", "A.md", "diameter", "--into", ".", "--write",
        ],
    );
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let written = fs::read_to_string(scratch.path().join("C.md")).unwrap();
    assert!(written.contains("readings: [11.9, 12.1]"), "{written}");
    assert!(
        out(&kept).contains("taken from A.md   diameter"),
        "{}",
        out(&kept)
    );
    // A name the pattern lacks, or a value its model computes, is refused.
    scratch.write(
        "D.md",
        "---\nschema_version: 1\nname: D\nproperties:\n  diameter: {v: 1.0, unit: g}\n  \
         twice: {v: 2.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    for (keep, said) in [
        ("diametr", "holds no field of that name"),
        ("twice", "the model computes it"),
    ] {
        let refused = at(
            scratch.path(),
            &["new", "E", "--keep", "D.md", keep, "--into", "."],
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
    assert!(said.contains("written: 2 files"), "{said}");
    assert!(
        said.contains("model/main.py") && said.contains("samples/ new S-01"),
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
        "---\nschema_version: 1\nname: W\nalloy: chrome\nproperties:\n  diameter: {v: 9.0, u: 0.1, unit: g}\n\
         tables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      load: {unit: N}\n    rows:\n      \
         - run: 1\n        load: 3.5\n      - run: 2\n        load: 4.25\n---\nTreated twice.\n",
    );
    let output = at(scratch.path(), &["W.md", "view"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    for shown in [
        "W\n",
        "alloy",
        "chrome",
        "9.00 ± 0.10 g",
        "runs · 2 rows",
        "load [N]",
        "4.25",
    ] {
        assert!(text.contains(shown), "{shown}: {text}");
    }
    assert!(!text.contains("Treated twice."), "{text}");
    let noted = at(scratch.path(), &["W.md", "view", "--note"]);
    assert!(out(&noted).contains("Treated twice."), "{}", out(&noted));
}

#[test]
fn a_unit_renamed_says_the_number_is_not_converted() {
    // mg to g over 4.1: the number stays, and what read it keeps what it gave.
    let scratch = Scratch::new("unit-renamed");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "u.sample.md",
        "---\nschema_version: 1\nname: U\nproperties:\n  crucible: {v: 4.1, unit: mg, fingerprint: '111111111111'}\n  \
         alloying: {v: 2.0, unit: mg, computed: {crucible: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    let output = at(scratch.path(), &["u.sample.md", "set", "crucible.unit=g"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(
        said.contains("renamed, not converted: 4.1 mg is now read as 4.1 g"),
        "{said}"
    );
    assert!(said.contains("alloying read the number 4.1"), "{said}");
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
    let preview = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(!scratch.path().join(".samplekit/history").exists());
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let edited = scratch.path().join("billet-02.sample.md");
    let text = fs::read_to_string(&edited).unwrap();
    fs::write(&edited, text.replace("12.2", "12.25")).unwrap();
    let tag = at(
        scratch.path(),
        &[
            ".",
            "tag",
            "add",
            "checked",
            "-f",
            "name == 'billet-02'",
            "--write",
        ],
    );
    assert_eq!(code(&tag), 0, "{}", err(&tag));
    assert_eq!(
        history_messages(scratch.path()),
        [
            "samplekit . tag add checked -f 'name == '\\''billet-02'\\''' --write",
            "changed outside SampleKit",
            "samplekit billet-01.sample.md set diameter=13 --write",
            "the project as SampleKit first kept it",
        ]
    );
    // A --write that changes nothing takes no snapshot.
    let again = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(history_messages(scratch.path()).len(), 4);
    // SAMPLEKIT_HISTORY=off takes none.
    let off = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["billet-01.sample.md", "set", "diameter=14", "--write"])
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
        vec!["billet-01.sample.md", "set", "diameter=13", "--write"],
        vec!["billet-02.sample.md", "tag", "add", "checked", "--write"],
        vec!["new", "billet-09", "diameter=11", "--write"],
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
        rows[0].starts_with("1 ") && rows[0].contains("samplekit new billet-09"),
        "{log}"
    );
    assert!(
        rows[1].starts_with("2 ") && rows[1].contains("tag add checked"),
        "{log}"
    );
    assert!(
        rows[2].contains("samplekit billet-01.sample.md set diameter=13"),
        "{log}"
    );
    assert!(
        rows[3].starts_with("4 ") && rows[3].contains("first kept it"),
        "{log}"
    );
    assert!(rows[3].contains("4 samples, .samplekitrc"), "{log}");
    // Narrowed to one sample, its snapshots keep their numbers.
    let one = out(&at(scratch.path(), &["billet-01.sample.md", "log"]));
    assert!(one.contains("3 ") && one.contains("diameter=13"), "{one}");
    assert!(!one.contains("checked"), "{one}");
    // Changed in an editor: `now` says so.
    let file = scratch.path().join("billet-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("approved", "retested")).unwrap();
    let now = out(&at(scratch.path(), &["log"]));
    assert!(
        now.contains("changed outside SampleKit, not yet kept"),
        "{now}"
    );
    assert!(now.contains("billet-03"), "{now}");
}

#[test]
fn diff_compares_two_states_as_values() {
    let scratch = Scratch::new("diff");
    with_history(&scratch);
    let file = scratch.path().join("billet-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace("approved", "retested").replace(
            "notes for billet-03.",
            "notes for billet-03.\nA second line.",
        ),
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
    assert!(said.contains("billet-03.sample.md"), "{said}");
    assert!(said.contains("status     approved  →  retested"), "{said}");
    assert!(said.contains("the note   changed, +1 −0 lines"), "{said}");
    // From the first snapshot: a value at the project's precision, a tag, a
    // new sample.
    let whole = out(&at(scratch.path(), &["diff", "--from", "4", "--to", "1"]));
    assert!(
        whole.contains("diameter   12.10 ± 0.05 g  →  13.00 ± 0.05 g"),
        "{whole}"
    );
    assert!(
        whole.contains("tags   reference  →  reference, checked"),
        "{whole}"
    );
    assert!(whole.contains("billet-09.md   new sample"), "{whole}");
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
        &["billet-02.sample.md", "set", "diameter=12.3", "--write"],
    );
    assert_eq!(code(&write), 0, "{}", err(&write));
    let kept = out(&at(scratch.path(), &["diff"]));
    assert!(kept.contains("billet-02.sample.md"), "{kept}");
    assert!(!kept.contains("changed outside SampleKit since"), "{kept}");
    let nothing = out(&at(scratch.path(), &["diff", "--from", "1"]));
    assert!(
        nothing.contains("nothing changed since SampleKit last kept"),
        "{nothing}"
    );
    assert!(nothing.contains("samplekit diff alone"), "{nothing}");
    // One file named: the last change kept of that file.
    let one = out(&at(scratch.path(), &["billet-03.sample.md", "diff"]));
    assert!(one.contains("status     approved  →  retested"), "{one}");
    // And `log` of one file does not repeat its name on every line.
    let logged = out(&at(scratch.path(), &["billet-03.sample.md", "log"]));
    assert!(!logged.contains("billet-03"), "{logged}");
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
    let export = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&export), 0, "{}", err_slashed(&export));
    let written = scratch.path().join("out/ductilities.csv");
    let fresh = at(scratch.path(), &["out/ductilities.csv", "explain"]);
    assert_eq!(code(&fresh), 0, "{}", err_slashed(&fresh));
    let said = out_slashed(&fresh);
    assert!(
        said.contains("by samplekit . export ductilities --write"),
        "{said}"
    );
    assert!(said.contains("written   out/ductilities.csv"), "{said}");
    assert!(said.contains("it is up to date"), "{said}");
    // Copied outside the project, it is explained from the project's folder.
    let elsewhere = Scratch::new("explain-output-elsewhere");
    let copy = elsewhere.path().join("table-3.csv");
    fs::copy(&written, &copy).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err_slashed(&set));
    let stale = at(scratch.path(), &[&copy.display().to_string(), "explain"]);
    assert_eq!(code(&stale), 0, "{}", err_slashed(&stale));
    let said = out_slashed(&stale);
    assert!(said.contains("billet-01.sample.md"), "{said}");
    assert!(
        said.contains("diameter   12.10 ± 0.05 g  →  13.00 ± 0.05 g"),
        "{said}"
    );
    assert!(
        said.contains("it is not current — samplekit . export ductilities --write makes it again"),
        "{said}"
    );
    // Kept by no project around it, it is said so; a file SampleKit did not
    // write is said so too.
    let lost = at(elsewhere.path(), &["table-3.csv", "explain"]);
    assert_eq!(code(&lost), 1, "{}", out_slashed(&lost));
    assert!(
        err_slashed(&lost).contains("no project here or below keeps table-3.csv"),
        "{}",
        err_slashed(&lost)
    );
    fs::write(scratch.path().join("out/mine.csv"), "a,b\n").unwrap();
    let unknown = at(scratch.path(), &["out/mine.csv", "explain"]);
    assert_eq!(code(&unknown), 1, "{}", out_slashed(&unknown));
    assert!(
        err_slashed(&unknown).contains("is not a file SampleKit wrote"),
        "{}",
        err_slashed(&unknown)
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
        &["billet-01.sample.md", "explain", "diameter"],
    ));
    assert!(now.contains("diameter = 13.00 ± 0.05 g"), "{now}");
    let then = at(
        scratch.path(),
        &["billet-01.sample.md", "explain", "diameter", "--at", "4"],
    );
    assert_eq!(code(&then), 0, "{}", err(&then));
    let said = out(&then);
    assert!(said.starts_with("as kept at #4 · "), "{said}");
    assert!(said.contains("diameter = 12.10 ± 0.05 g"), "{said}");
    let absent = at(
        scratch.path(),
        &["billet-09.md", "explain", "diameter", "--at", "4"],
    );
    assert_eq!(code(&absent), 1, "{}", out(&absent));
    assert!(
        err(&absent).contains("was not in the project at #4"),
        "{}",
        err(&absent)
    );
}

#[test]
fn explain_never_offers_to_replace_the_file_as_it_is_now() {
    // A copy of an old export, explained, offered `-o out/ductilities.csv --at …`:
    // run as it was said, it replaced today's export with the old one.
    let scratch = Scratch::new("made-again-beside");
    scratch.corpus();
    let first = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&first), 0, "{}", err_slashed(&first));
    let then = fs::read(scratch.path().join("out/ductilities.csv")).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err_slashed(&set));
    let again = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&again), 0, "{}", err_slashed(&again));
    fs::write(scratch.path().join("old-ductilities.csv"), &then).unwrap();
    let said = out_slashed(&at(scratch.path(), &["old-ductilities.csv", "explain"]));
    assert!(said.contains("made again as it was"), "{said}");
    assert!(!said.contains("-o out/ductilities.csv "), "{said}");
    assert!(said.contains("-o out/ductilities-"), "{said}");
    // Today's export, explained, is made again where it is.
    let current = out_slashed(&at(scratch.path(), &["out/ductilities.csv", "explain"]));
    assert!(
        current.contains("-o out/ductilities.csv --at "),
        "{current}"
    );
}

#[test]
fn at_reads_the_project_as_a_snapshot_kept_it() {
    // A reading command over the project as it was; an export made again is the
    // file it was, byte for byte.
    let scratch = Scratch::new("at");
    scratch.corpus();
    let first = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let then = fs::read(scratch.path().join("out/ductilities.csv")).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_ne!(
        fs::read(scratch.path().join("out/ductilities.csv")).unwrap(),
        then
    );
    // Made again as it was, where -o says.
    let remade = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
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
    let table = out(&at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--at", "2"],
    ));
    assert!(table.contains("12.10"), "{table}");
    assert!(!table.contains("13.00"), "{table}");
    // explain says how the export remade is made again as it was.
    let said = out(&at(scratch.path(), &["out/then.csv", "explain"]));
    assert!(
        said.contains("made again as it was, from the project's folder: samplekit . export ductilities -o out/then.csv --write --at "),
        "{said}"
    );
    // Writing to the project, or an export to its declared file, is refused.
    let refused = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=1", "--at", "2"],
    );
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    let declared = at(
        scratch.path(),
        &[".", "export", "ductilities", "--at", "2", "--write"],
    );
    assert_eq!(code(&declared), 1, "{}", out(&declared));
    assert!(
        err(&declared).contains("where -o says"),
        "{}",
        err(&declared)
    );
    // Its preview too: it announced a file in a folder removed at the end.
    let previewed = at(scratch.path(), &[".", "export", "ductilities", "--at", "2"]);
    assert_eq!(code(&previewed), 1, "{}", out(&previewed));
    assert!(
        !out(&previewed).contains("samplekit-at-"),
        "{}",
        out(&previewed)
    );
    // What reads the configuration says it where it is, not in that folder.
    let shown = at(scratch.path(), &["--show-rc", "--at", "2"]);
    assert!(!out(&shown).contains("samplekit-at-"), "{}", out(&shown));
}

#[test]
fn project_is_a_field_over_several_projects() {
    // The folder of a sample's project, by name, a field like any.
    let scratch = Scratch::new("project-field");
    for (project, diameter) in [("ana", 1.0), ("tom", 2.0)] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        scratch.write(
            &format!("{project}/s-{project}.md"),
            &format!(
                "---\nschema_version: 1\nname: s-{project}\nproperties:\n  diameter: {diameter}\n---\n"
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
        &[".", "-c", "diameter", "--summary", "--group", "status"],
    );
    assert_eq!(code(&table), 0, "{}", err(&table));
    let said = out(&table);
    assert!(said.contains("approved"), "{said}");
    assert!(said.contains("rejected"), "{said}");
    let csv = out(&at(
        scratch.path(),
        &[
            ".",
            "-c",
            "diameter",
            "--summary",
            "--group",
            "status",
            "--csv",
        ],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "status,column,unit,n,of,not_applicable,mean,weights,s,sem,median,min,max"
    );
    assert!(lines[1].starts_with("approved,diameter,g,3,3,0,"), "{csv}");
    assert!(lines[2].starts_with("rejected,diameter,g,1,1,0,"), "{csv}");
}

#[test]
fn status_and_validate_speak_json() {
    // For scripts, one object per value, one per finding.
    let scratch = Scratch::new("json-status");
    scratch.corpus();
    let status = at(scratch.path(), &[".", "status", "--json"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    let listed: serde_json::Value = serde_json::from_str(&out(&status)).unwrap();
    assert!(listed.is_array(), "{listed}");
    fs::write(
        scratch.path().join("broken.sample.md"),
        "---\nschema_version: 1\nproperties: [1\n---\n",
    )
    .unwrap();
    let validate = at(scratch.path(), &[".", "validate", "--json"]);
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
        out(&help).contains("Usage: samplekit [DIRECTORY] tui"),
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
fn restore_takes_back_my_last_change_and_keeps_what_came_since() {
    let scratch = Scratch::new("restore-mine");
    let root = two_pieces(&scratch, "p");
    let run = |machine: &str, arguments: &[&str]| {
        let output = as_machine(&root, machine, arguments);
        (code(&output), out(&output), err(&output))
    };
    let (status, _, error) = run("ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(status, 0, "{error}");
    // A colleague's change since, in another file and in another field of
    // the same one: both stay.
    let (status, _, error) = run("tom", &["b.md", "set", "diameter=2.5", "--write"]);
    assert_eq!(status, 0, "{error}");
    let (status, _, error) = run("tom", &["a.md", "tag", "add", "inspected", "--write"]);
    assert_eq!(status, 0, "{error}");
    let (status, said, error) = run("ana", &["restore", "--write"]);
    assert_eq!(status, 0, "{error}");
    assert!(
        said.contains("restore · take back #") && said.contains("diameter=1.5"),
        "{said}"
    );
    let a = fs::read_to_string(root.join("a.md")).unwrap();
    assert!(
        a.contains("diameter: 1.0") && a.contains("inspected"),
        "{a}"
    );
    assert!(
        fs::read_to_string(root.join("b.md"))
            .unwrap()
            .contains("2.5")
    );
    // What the change changed, changed since by another: refused, named.
    let (status, _, error) = run("ana", &["a.md", "set", "diameter=1.7", "--write"]);
    assert_eq!(status, 0, "{error}");
    let (status, _, error) = run("tom", &["a.md", "set", "diameter=1.9", "--write"]);
    assert_eq!(status, 0, "{error}");
    let before = fs::read_to_string(root.join("a.md")).unwrap();
    let (status, _, error) = run("ana", &["restore", "--write"]);
    assert_eq!(status, 2, "{error}");
    assert!(error.contains("a.md: diameter, changed since"), "{error}");
    assert!(error.contains("samplekit restore --at"), "{error}");
    assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), before);
}

#[test]
fn restore_takes_back_the_last_change_or_goes_to_a_state() {
    // Previewed value by value, written with --write, a snapshot of its own; a
    // file named narrows it.
    let scratch = Scratch::new("restore");
    with_history(&scratch);
    let file = scratch.path().join("billet-01.sample.md");
    let kept = fs::read_to_string(&file).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=14", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let preview = at(scratch.path(), &["billet-01.sample.md", "restore"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("restore · take back #1 · "),
        "{}",
        out(&preview)
    );
    assert!(out(&preview).contains("→"), "{}", out(&preview));
    assert_ne!(fs::read_to_string(&file).unwrap(), kept);
    let written = at(
        scratch.path(),
        &["billet-01.sample.md", "restore", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(fs::read_to_string(&file).unwrap(), kept);
    let logged = out(&at(scratch.path(), &["log"]));
    assert!(
        logged.contains("samplekit billet-01.sample.md restore --write"),
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
        &[
            ".",
            "export",
            "ductilities",
            "-f",
            "diameter > 12.25",
            "--write",
        ],
    );
    assert_eq!(code(&export), 0, "{}", err(&export));
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let explained = at(scratch.path(), &["out/ductilities.csv", "explain"]);
    let said = out(&explained);
    assert!(
        said.contains("its selection would now also take billet-01.sample.md"),
        "{said}{}",
        err(&explained)
    );
    let rc = scratch.path().join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    let changed = text.replacen(
        "[export.ductilities]",
        "[export.ductilities]\nfilename = true",
        1,
    );
    assert_ne!(changed, text);
    fs::write(&rc, changed).unwrap();
    let said = out(&at(scratch.path(), &["out/ductilities.csv", "explain"]));
    assert!(
        said.contains("[export.ductilities] changed since"),
        "{said}"
    );
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
         properties:\n  radius:\n    readings: [1.060, 1.062]\n  big:\n    readings: [1.7e308, -1.7e308]\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nm: 7\nproperties:\n  radius: 1.0\n---\n",
    );
    let viewed = at(scratch.path(), &["a.md", "view"]);
    assert_eq!(code(&viewed), 0, "{}", err(&viewed));
    // n/a set to n/a changes nothing.
    let same = at(scratch.path(), &["a.md", "set", "m=n/a"]);
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
            &["b.md", "set", &format!("note_{at_}={text}"), "--write"],
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
    let set = at(scratch.path(), &["b.md", "set", "m=8", "--write"]);
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
    let rows = at(scratch.path(), &["c.md", "view"]);
    assert_eq!(code(&rows), 0, "{}{}", out(&rows), err(&rows));
}

#[test]
fn a_deleted_sample_is_logged_compared_and_restored_by_name() {
    // A path gone from the disk whose folder is in a project is one the
    // history still names; brought back, it is restored, not new.
    let scratch = Scratch::new("deleted");
    with_history(&scratch);
    let file = scratch.path().join("billet-09.md");
    let kept = fs::read_to_string(&file).unwrap();
    fs::remove_file(&file).unwrap();
    let log = at(scratch.path(), &["billet-09.md", "log"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    assert!(
        out(&log).contains("samplekit new billet-09"),
        "{}",
        out(&log)
    );
    let diff = at(scratch.path(), &["billet-09.md", "diff", "--from", "1"]);
    assert_eq!(code(&diff), 0, "{}", err(&diff));
    assert!(
        out(&diff).contains("billet-09.md   removed"),
        "{}",
        out(&diff)
    );
    let restored = at(scratch.path(), &["billet-09.md", "restore", "--at", "1"]);
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert!(
        out(&restored).contains("billet-09.md   restored"),
        "{}",
        out(&restored)
    );
    assert!(!out(&restored).contains("new sample"), "{}", out(&restored));
    let written = at(
        scratch.path(),
        &["billet-09.md", "restore", "--at", "1", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(fs::read_to_string(&file).unwrap(), kept);
    // A path whose folder does not exist, or that no snapshot held — a
    // misspelling — is still refused.
    for missing in ["gone/billet-09.md", "billet-99.md"] {
        let nowhere = at(scratch.path(), &[missing, "log"]);
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
        preview.contains(&format!("restore · back to #4 · {short} · ")),
        "{preview}"
    );
    assert!(preview.contains("nothing written"), "{preview}");
    // A file that cannot be written: those put back before it are said.
    let second = scratch.path().join("billet-02.sample.md");
    let mode = |bits: u32| {
        fs::set_permissions(&second, std::os::unix::fs::PermissionsExt::from_mode(bits)).unwrap()
    };
    mode(0o444);
    let refused = at(scratch.path(), &["restore", "--at", "4", "--write"]);
    mode(0o644);
    assert_eq!(code(&refused), 3, "{}", err(&refused));
    assert!(
        err(&refused).contains("restored before it: billet-01.sample.md"),
        "{}",
        err(&refused)
    );
    let written = out(&at(scratch.path(), &["restore", "--at", "5", "--write"]));
    assert!(
        written.contains(&format!("restore · back to #5 · {short} · ")),
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
        &["export", "ductilities", "-f", "diameter > 12.25", "--write"],
    );
    assert_eq!(code(&export), 0, "{}", err_slashed(&export));
    let fresh = out_slashed(&at(scratch.path(), &["out/ductilities.csv", "explain"]));
    assert!(fresh.contains("it is up to date"), "{fresh}");
    assert!(!fresh.contains("its selection"), "{fresh}");
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err_slashed(&set));
    let said = out_slashed(&at(scratch.path(), &["out/ductilities.csv", "explain"]));
    let taken = said
        .find("its selection would now also take billet-01.sample.md")
        .expect(&said);
    let verdict = said.find("it is not current").expect(&said);
    assert!(taken < verdict, "{said}");
    assert!(!said.contains("up to date"), "{said}");
    // Made again as it was: an export `--at` writes where -o says.
    assert!(said.contains(" -o out/ductilities.csv --at "), "{said}");
}

#[test]
fn the_same_contents_written_twice_are_told_apart() {
    // Two writes of the same bytes — one `--at` the snapshot it was first
    // made from — are two records; each file is explained by its own.
    let scratch = Scratch::new("explain-twice");
    scratch.corpus();
    let first = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert_eq!(code(&first), 0, "{}", err_slashed(&first));
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err_slashed(&set));
    let old = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "--at",
            "2",
            "-o",
            "out/old.csv",
            "--write",
        ],
    );
    assert_eq!(code(&old), 0, "{}", err_slashed(&old));
    assert_eq!(
        fs::read(scratch.path().join("out/old.csv")).unwrap(),
        fs::read(scratch.path().join("out/ductilities.csv")).unwrap()
    );
    let declared = out_slashed(&at(scratch.path(), &["out/ductilities.csv", "explain"]));
    assert!(
        declared.contains("written   out/ductilities.csv"),
        "{declared}"
    );
    assert!(declared.contains("it is not current"), "{declared}");
    let remade = out_slashed(&at(scratch.path(), &["out/old.csv", "explain"]));
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
    let figure = at(scratch.path(), &["figure.svg", "explain"]);
    assert_eq!(code(&figure), 1, "{}", out_slashed(&figure));
    assert!(
        err_slashed(&figure).contains("records no figure"),
        "{}",
        err_slashed(&figure)
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
        let output = at(scratch.path(), &[".", "tag", "add", tag, "--write"]);
        assert_eq!(code(&output), 0, "{}", err(&output));
    }
    let log = out(&at(scratch.path(), &["log", "--at", "1"]));
    let columns: Vec<usize> = log
        .lines()
        .filter_map(|line| line.find("4 samples"))
        .collect();
    assert_eq!(columns.len(), 3, "{log}");
    assert!(columns.iter().all(|at| *at == columns[0]), "{log}");
    let file = scratch.path().join("billet-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("approved", "retested")).unwrap();
    let one = out(&at(scratch.path(), &["billet-03.sample.md", "log"]));
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
    assert!(first.contains(".samplekitrc          new\n"), "{first}");
    assert!(
        first.contains("billet-01.sample.md   new sample\n"),
        "{first}"
    );
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
        &["billet-01.sample.md", "explain", "--at", "4"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("samplekit billet-01.sample.md explain diameter --at 4"),
        "{}",
        err(&output)
    );
}

// ------------------------------------------------ the review of 2026-09-27

/// A small metals project: `radius` measured without a unit and declared at
/// `.3f`, `modulus` computed at `.1f`, `volume` in litres, and a figure.
fn pieces(scratch: &Scratch) {
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\
         [collection]\n\
         exclude = [\"README.md\"]\n\
         [property.radius]\n\
         precision = [\".3f\", \".4f\"]\n\
         [property.modulus]\n\
         precision = \".1f\"\n\
         [property.volume]\n\
         unit = \"L\"\n\
         [figure.strength]\n\
         kind = \"scatter\"\n\
         x = \"radius\"\n\
         y = \"modulus\"\n\
         group = \"style\"\n",
    );
    scratch.write("README.md", "# Pieces\n\nNot a sample.\n");
    scratch.write(
        "alpha-titanium.md",
        "---\nschema_version: 1\nname: alpha-titanium\nstyle: titanium\nproperties:\n  radius:\n    v: 1.0636666666666668\n    \
         readings: [1.063, 1.065, 1.063]\n    u: 6.666666666666e-4\n    statistics: {v: mean, u: standard_error}\n    \
         computed: {readings: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n  notch: {v: 1.012}\n  volume: {v: 19.5, unit: L}\n  \
         modulus:\n    v: 6.825000000000035\n    unit: \"%\"\n    computed: {notch: cccccccccccc, radius: dddddddddddd}\n    \
         fingerprint: eeeeeeeeeeee\n---\n# Alpha TITANIUM\n",
    );
    scratch.write(
        "gamma-zinc.md",
        "---\nschema_version: 1\nname: gamma-zinc\nstyle: zinc\nproperties:\n  radius: {v: 1.066}\n  \
         volume: {v: 20.0, unit: L}\n  modulus: {unit: \"%\"}\n---\n# Gamma zinc\n",
    );
}

#[test]
fn a_closed_reader_still_lets_a_write_happen() {
    // The preview is printed before the write: `set … --write | head -1`
    // lost its reader on the second line, and the command ended there with
    // 0, having written nothing.
    let scratch = Scratch::new("closed-write");
    pieces(&scratch);
    let mut child = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["gamma-zinc.md", "set", "volume=21", "--write"])
        .current_dir(scratch.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The reader is gone before anything is written.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("gamma-zinc.md")).unwrap();
    assert!(written.contains("v: 21"), "{written}");
}

#[test]
fn explain_says_a_value_waits_for_an_input_nobody_entered() {
    // `modulus` of a piece with no `notch` was *never computed … samplekit compute*,
    // which computes nothing while notch is missing: `status` says it waits.
    let scratch = Scratch::new("explain-waits");
    pieces(&scratch);
    scratch.write(
        ".samplekitrc",
        &(fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap()
            + "[model]\npath = \"model.py\"\n"),
    );
    let output = at(scratch.path(), &["gamma-zinc.md", "explain", "modulus"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(said.contains("it waits for notch"), "{said}");
    assert!(
        said.contains("samplekit gamma-zinc.md set notch=<value>"),
        "{said}"
    );
    assert!(!said.contains("never computed"), "{said}");
}

#[test]
fn an_unknown_row_lists_the_rows_there_are() {
    // `no row of 'inspection' is indexed [Nobody]` listed no row, and said
    // `inspector: matched` of the one column that had not.
    let scratch = Scratch::new("unknown-row");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  inspection:\n    index: inspector\n    columns:\n      \
         inspector: {}\n      score: {}\n    rows:\n      - {inspector: Lea, score: 40}\n      \
         - {inspector: Sam, score: 38}\n---\n",
    );
    let output = at(scratch.path(), &[".", "-f", "inspection.score[Nobody] > 1"]);
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("the rows of 'inspection', by inspector: Lea, Sam"),
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
    pieces(&scratch);
    let recorded = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "radius.readings=1.06,1.062"],
    );
    let said = out(&recorded);
    assert!(
        said.contains("radius's uncertainty is their standard error, as its file records"),
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
        &["gamma-zinc.md", "set", "radius.readings=1.06,1.062"],
    );
    assert!(
        !out(&none).contains("radius's uncertainty is their"),
        "{}",
        out(&none)
    );
}

#[test]
fn plot_names_the_figures_when_a_name_is_no_figure() {
    // `plot -o x.png --write pieces`, the figure's name forgotten, asked the
    // model for a figure called `pieces`, and said only why it could not.
    let scratch = Scratch::new("plot-no-figure");
    pieces(&scratch);
    fs::create_dir_all(scratch.path().join("pieces")).unwrap();
    let forgotten = at(
        scratch.path(),
        &["pieces", "plot", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&forgotten), 1, "{}", err(&forgotten));
    assert!(
        err(&forgotten).contains("no figure named 'pieces'")
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
        .args([".", "plot", "nosuch", "-o", "x.png", "--write"])
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
    pieces(&scratch);
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
    // `export ductilities -o r.json --write` wrote CSV into r.json.
    let scratch = Scratch::new("export-extension");
    scratch.corpus();
    let output = at(
        scratch.path(),
        &[".", "export", "ductilities", "-o", "r.json", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(
        err(&output).contains("'ductilities' writes CSV, and the extension names JSON"),
        "{}",
        err(&output)
    );
    assert!(!scratch.path().join("r.json").exists());
    let same = at(
        scratch.path(),
        &[".", "export", "ductilities", "-o", "r.csv", "--write"],
    );
    assert_eq!(code(&same), 0, "{}", err(&same));
}

#[test]
fn list_skipped_says_not_read_in_columns() {
    // A file a newer SampleKit wrote was *malformed frontmatter* here and
    // *not read* in validate; and the reasons stood wherever each path ended.
    let scratch = Scratch::new("list-skipped");
    pieces(&scratch);
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
    pieces(&scratch);
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
}

#[test]
fn a_file_that_is_no_sample_is_said_to_be_none() {
    // `samplekit .samplekitrc` answered *because something already claimed
    // it was a sample*.
    let scratch = Scratch::new("no-sample");
    pieces(&scratch);
    let rc = at(scratch.path(), &[".samplekitrc"]);
    assert_eq!(code(&rc), 2, "{}", err(&rc));
    assert!(
        err(&rc).contains(".samplekitrc is the project's configuration, not a sample"),
        "{}",
        err(&rc)
    );
    let readme = at(scratch.path(), &["README.md", "view"]);
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
    let model = at(scratch.path(), &["model.py", "set", "x=1"]);
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
    pieces(&scratch);
    let output = at(scratch.path(), &[".", "-c", "radius", "--precision", "abc"]);
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
    pieces(&scratch);
    let output = at(scratch.path(), &["tag", "add", "a b"]);
    assert_eq!(code(&output), 1, "{}", out(&output));
    assert!(err(&output).contains("at character 2"), "{}", err(&output));
    assert!(!err(&output).contains("position"), "{}", err(&output));
}

#[test]
fn a_box_plot_names_whose_group_it_refuses() {
    // `--kind box` on a figure declaring `group` was refused as *--group has
    // nothing left to split*, of a --group nobody had typed; and `--help`
    // listed four kinds of five.
    let scratch = Scratch::new("box-group");
    pieces(&scratch);
    let output = at(
        scratch.path(),
        &[".", "plot", "strength", "--kind", "box", "-o", "s.png"],
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
            ".", "plot", "strength", "--kind", "box", "--group", "style", "-o", "s.png",
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
    pieces(&scratch);
    let added = at(
        scratch.path(),
        &["alpha-titanium.md", "tag", "add", "carbonaceous", "--write"],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
    assert!(
        out(&added).starts_with("tag +carbonaceous · 1 of 1 sample"),
        "{}",
        out(&added)
    );
    assert!(!out(&added).contains("will"), "{}", out(&added));
    let again = at(
        scratch.path(),
        &["alpha-titanium.md", "tag", "add", "carbonaceous"],
    );
    assert!(out(&again).contains("nothing to change"), "{}", out(&again));
    assert!(!out(&again).contains("pass --write"), "{}", out(&again));
}

#[test]
fn a_hint_names_the_field_and_file_given() {
    // Examples came from another collection: `BR-01.md feed_rate=13.2`,
    // `billet-04.sample.md porosity`, `-s diameter -r`.
    let scratch = Scratch::new("hints-given");
    pieces(&scratch);
    let set = at(scratch.path(), &["alpha-titanium.md", "set"]);
    assert!(
        err(&set).contains("samplekit alpha-titanium.md set radius=<value>"),
        "{}",
        err(&set)
    );
    let explain = at(scratch.path(), &["alpha-titanium.md", "explain"]);
    assert!(
        err(&explain).contains("samplekit alpha-titanium.md explain <field>"),
        "{}",
        err(&explain)
    );
    let reverse = at(scratch.path(), &[".", "-c", "name,radius", "-r"]);
    assert!(err(&reverse).contains("-s radius -r"), "{}", err(&reverse));
    for output in [&set, &explain, &reverse] {
        for foreign in ["BR-01", "feed_rate", "billet", "porosity", "diameter"] {
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
        "schema_version = 1\n[query.big]\nfilter = 'radius > 1'\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  radius: {v: 1.05}\ntables:\n  \
         ageing:\n    index: day\n    columns:\n      day: {}\n      hardness: {}\n    \
         rows:\n      - {day: 1, hardness: 1.04}\n---\n",
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
    let column = at(scratch.path(), &[".", "-c", "ageing.hardness"]);
    assert!(
        err(&column).contains("'ageing.hardness' is a column of the table 'ageing'")
            && err(&column).contains("e.g.     'ageing.hardness[1]'"),
        "{}",
        err(&column)
    );
}

#[test]
fn a_warning_is_one_warning_line() {
    // Each line of a warning was its own `warning:`, blank ones included,
    // and `set`'s advice was about `new --like`.
    let scratch = Scratch::new("one-warning");
    pieces(&scratch);
    let output = at(scratch.path(), &["alpha-titanium.md", "set", "unheard=3"]);
    let said = err(&output);
    assert_eq!(said.matches("warning:").count(), 1, "{said}");
    assert!(
        !said.lines().any(|line| line.trim() == "warning:"),
        "{said}"
    );
    assert!(said.contains("[property.unheard]"), "{said}");
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
    // `wrote`, `written to`, `12 rows written to` and `written pieces/…`.
    let scratch = Scratch::new("written-form");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert!(
        out_slashed(&set).contains("written: billet-01.sample.md"),
        "{}",
        out_slashed(&set)
    );
    let table = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "-o", "t.csv", "--write"],
    );
    assert!(
        out_slashed(&table).contains("written: t.csv"),
        "{}",
        out_slashed(&table)
    );
    let export = at(scratch.path(), &[".", "export", "ductilities", "--write"]);
    assert!(
        out_slashed(&export).contains("written: out/ductilities.csv, 4 rows"),
        "{}",
        out_slashed(&export)
    );
}

#[test]
fn list_sorts_tags_and_counts_every_kind() {
    let scratch = Scratch::new("list-tags-sorted");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntags: [rust, matte]\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\ntags: [prize]\n---\n",
    );
    let tags = out(&at(scratch.path(), &["list", "tags"]));
    assert_eq!(tags, "matte\nprize\nrust\n");
    let counted = out(&at(scratch.path(), &["list"]));
    assert!(
        counted.contains("3 tags in use — matte, prize, rust"),
        "{counted}"
    );
    fs::create_dir_all(scratch.path().join("empty")).unwrap();
    let empty = out(&at(scratch.path(), &["empty", "list"]));
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
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    for (form, file) in [
        ("-oattached.csv", "attached.csv"),
        ("-o=equals.csv", "equals.csv"),
        ("--output=long.csv", "long.csv"),
    ] {
        let output = at(
            scratch.path(),
            &[".", "-c", "name,diameter", "--at", "1", form, "--write"],
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
            "name,diameter",
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
#[cfg(target_os = "linux")]
fn open_takes_several_targets_and_one_pattern() {
    // Targets as every command takes them, and the one word naming no file:
    // the pattern, wherever it is written.
    let scratch = Scratch::new("open-several");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n",
    );
    for name in ["B24", "B25"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\n---\n"),
        );
    }
    for file in [
        "images/B24_photo.jpg",
        "images/B24_plot.png",
        "images/B25_photo.jpg",
    ] {
        scratch.write(file, "");
    }
    let bin = scratch.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let opener = bin.join("xdg-open");
    fs::write(&opener, "#!/bin/sh\nexit 0\n").unwrap();
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
    let both = open(&["open", "B24.md", "B25.md", "*.jpg"]);
    assert_eq!(code(&both), 0, "{}", err(&both));
    assert!(
        out(&both).contains("opened images/B24_photo.jpg")
            && out(&both).contains("opened images/B25_photo.jpg")
            && !out(&both).contains("B24_plot.png"),
        "{}",
        out(&both)
    );
    // The pattern first; a sample with nothing it names is said, and the
    // others opened.
    let first = open(&["open", "*.png", "B24.md", "B25.md"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(
        out(&first).contains("opened images/B24_plot.png"),
        "{}",
        out(&first)
    );
    assert!(out(&first).contains("B25.md · "), "{}", out(&first));
}

#[test]
fn a_row_added_to_a_bare_file_name_takes_its_siblings_table() {
    // `set nt.md --add-row …` read the empty parent of `nt.md` as no folder,
    // found no sibling, and took the table's shape from the model instead.
    let scratch = Scratch::new("add-row-bare");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  inspection:\n    index: inspector\n    columns:\n      \
         inspector: {}\n      score: {unit: pt}\n    rows:\n      - {inspector: Lea, score: 40}\n---\n",
    );
    scratch.write("nt.md", "---\nschema_version: 1\nname: nt\n---\n");
    let output = at(
        scratch.path(),
        &[
            "nt.md",
            "set",
            "--add-row",
            "inspection",
            "inspector=Sam",
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
fn new_keep_reads_the_declarations_of_the_project_it_writes_into() {
    // The declarations were read from --into or the current directory while
    // the file went beside the sample it took from, in another project.
    let scratch = Scratch::new("new-like-project");
    fs::create_dir_all(scratch.path().join("elsewhere")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\n[property.diameter]\nunit = \"g\"\n",
    );
    scratch.write(
        "project/a.md",
        "---\nschema_version: 1\nname: a\nbatch: B7\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let output = at(
        &scratch.path().join("elsewhere"),
        &[
            "new",
            "b",
            "--keep",
            "../project/a.md",
            "batch",
            "diameter=2",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("attribute"), "{}", err(&output));
    let file = fs::read_to_string(scratch.path().join("project/b.md")).unwrap();
    assert!(file.contains("diameter: {v: 2, unit: g}"), "{file}");
}

#[test]
fn a_declared_export_says_it_creates_its_directory() {
    let scratch = Scratch::new("export-directory-said");
    scratch.corpus();
    fs::remove_dir_all(scratch.path().join("out")).unwrap();
    let output = at(scratch.path(), &[".", "export", "ductilities"]);
    assert!(
        out(&output).contains("out does not exist, and is made"),
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
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n[property.radius]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  radius:\n    readings: [1.060, 1.062, 1.061]\n---\n",
    );
    fs::create_dir_all(scratch.path().join("images/b")).unwrap();
    fs::write(scratch.path().join("images/b/photo.jpg"), "").unwrap();
    let files = out(&at(scratch.path(), &[".", "list", "files"]));
    assert!(files.contains("photo.jpg"), "{files}");
    let count = out(&at(
        scratch.path(),
        &[".", "-c", "name,radius.stats.count", "--csv"],
    ));
    assert!(count.contains("b,3\n"), "{count}");
    let blank = at(scratch.path(), &["new", " ", "--write"]);
    assert_eq!(code(&blank), 1, "{}", out(&blank));
    scratch.write("new.md", "---\nschema_version: 99\nname: n\n---\n");
    let newer = err(&at(scratch.path(), &[".", "list"]));
    assert!(
        newer.contains("schema_version 99") && !newer.contains("malformed"),
        "{newer}"
    );
    let missing = at(scratch.path(), &["nothing.pdf", "explain"]);
    assert_eq!(code(&missing), 3, "{}", err(&missing));
    assert!(err(&missing).contains("no such file"), "{}", err(&missing));
}

#[test]
fn new_writes_a_value_as_the_collection_holds_it() {
    // `new inconel radius=1.048 volume=20` beside pieces that all measure both
    // wrote two attributes, and warned *'radius' is new here*.
    let scratch = Scratch::new("new-as-held");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, radius) in [("a", "1.050"), ("b", "1.060")] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  radius: {radius}\n  \
                 volume: {{v: 20.5, unit: L}}\n---\n"
            ),
        );
    }
    let output = at(
        scratch.path(),
        &["new", "inconel", "radius=1.048", "volume=20", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!err(&output).contains("attribute"), "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("inconel.md")).unwrap();
    assert!(
        written.contains("properties:\n  radius: 1.048"),
        "{written}"
    );
    assert!(written.contains("volume: {v: 20, unit: L}"), "{written}");
    // A decimal comma is refused as `set` refuses it.
    let comma = at(scratch.path(), &["new", "bronze", "radius=1,050"]);
    assert_eq!(code(&comma), 1, "{}", out(&comma));
    assert!(
        err(&comma).contains("'radius' holds a number, and '1,050' is text")
            && err(&comma).contains("radius=1.050"),
        "{}",
        err(&comma)
    );
    // A name nobody holds is an attribute, and the warning says why, truly.
    let unheld = at(scratch.path(), &["new", "nickel", "batch=3"]);
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
    // `hb` only the model declared was written as an attribute, and the
    // model then refused the sample: *'hb' is already a property*. The
    // model's description says so; importing the model would leave a mark.
    let scratch = Scratch::new("model-declares");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/billet.py\"\nclass = \"Billet\"\n",
    );
    scratch.write(
        "model/billet.py",
        "import pathlib\n\nimport samplekit as sk\n\n\
         pathlib.Path(__file__).with_name(\"imported\").write_text(\"\")\n\n\
         class Billet(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.hb = sk.Property(unit=\"mL\")\n        \
         self.height = sk.Property(\n            unit=\"mL\",\n        )\n",
    );
    describe(
        scratch.path(),
        serde_json::json!({"hb": entered(Some("mL")), "collar": entered(Some("mL"))}),
        serde_json::json!({}),
    );
    let new = at(scratch.path(), &["new", "S-01", "hb=25.1", "--write"]);
    assert_eq!(code(&new), 0, "{}", err(&new));
    assert!(
        out(&new).contains("a property model/billet.py declares"),
        "{}",
        out(&new)
    );
    let set = at(scratch.path(), &["S-01.md", "set", "collar=4.0", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = fs::read_to_string(scratch.path().join("S-01.md")).unwrap();
    assert!(written.contains("hb: {v: 25.1, unit: mL}"), "{written}");
    assert!(written.contains("collar: {v: 4.0, unit: mL}"), "{written}");
    assert!(!written.contains("\nhb:"), "an attribute: {written}");
    assert!(!scratch.path().join("model/imported").exists());
    let text = at(scratch.path(), &["new", "S-02", "hb=abc"]);
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
        "schema_version = 1\n\n[model]\npath = \"model/billet.py\"\nclass = \"Billet\"\n",
    );
    scratch.write(
        "model/billet.py",
        "import samplekit as sk\n\n\
         class Billet(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.rating = sk.Property()\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({"rating": entered(None)}),
        serde_json::json!({}),
    );
    let set = at(scratch.path(), &["S-01.md", "set", "rating=A", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        out(&set).contains("new, a property model/billet.py declares"),
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
        "schema_version = 1\n\n[model]\npath = \"model/billet.py\"\nclass = \"Billet\"\n\n\
         [property.diameter]\nunit = \"g\"\n",
    );
    scratch.write(
        "model/billet.py",
        "import samplekit as sk\n\n\
         class Billet(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.diameter = sk.Property(unit=\"kg\")\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    describe(
        scratch.path(),
        serde_json::json!({"diameter": entered(Some("kg"))}),
        serde_json::json!({}),
    );
    let set = at(
        scratch.path(),
        &["S-01.md", "set", "diameter=4.2", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let said = err(&set);
    assert!(
        said.contains("'diameter' is in g by .samplekitrc and in kg by"),
        "{said}"
    );
    assert!(said.contains("model/billet.py"), "{said}");
    let written = fs::read_to_string(scratch.path().join("S-01.md")).unwrap();
    assert!(written.contains("diameter: {v: 4.2, unit: g}"), "{written}");
}

#[test]
fn a_name_the_model_declares_where_it_cannot_be_read_is_said() {
    // No environment to run the model: its description cannot be written, and
    // the name is written as the samples hold it, said.
    let scratch = Scratch::new("model-unread");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\n[model]\npath = \"model/billet.py\"\nclass = \"Billet\"\n",
    );
    scratch.write(
        "model/billet.py",
        "import samplekit as sk\n\n\
         class Billet(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.hb = sk.Property(unit=\"mL\")\n",
    );
    scratch.write("S-01.md", "---\nschema_version: 1\n---\n");
    let set = at(scratch.path(), &["S-01.md", "set", "hb=25.1", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let said = err(&set);
    assert!(
        said.contains("the model could not be read, so what it declares is not known"),
        "{said}"
    );
    assert!(said.contains("no interpreter is declared"), "{said}");
    assert!(!scratch.path().join(".samplekit/model.json").exists());
}

#[test]
fn new_refuses_as_usage_what_it_cannot_write() {
    let scratch = Scratch::new("new-usage");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  radius: 1.05\ntables:\n  trial:\n    \
         index: day\n    columns:\n      day: {}\n      hardness: {}\n    rows:\n      \
         - day: 1\n        hardness: 1.05\n---\n",
    );
    // A destination that is no sample's file.
    let text = at(
        scratch.path(),
        &["new", "b", "--keep", "a.md", "radius", "--into", "b.txt"],
    );
    assert_eq!(code(&text), 1, "{}", err(&text));
    assert!(
        err(&text).contains("a sample's file ends in .md"),
        "{}",
        err(&text)
    );
    // A name already written, a value to keep that is none, a table to keep.
    let here = at(scratch.path(), &["new", "a", "--keep", "a.md", "radius"]);
    assert_eq!(code(&here), 1, "{}", err(&here));
    for (kept, said) in [
        ("nosuch", "holds no field of that name"),
        ("trial", "--keep takes values, and trial is a table"),
    ] {
        let output = at(scratch.path(), &["new", "b", "--keep", "a.md", kept]);
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
    pieces(&scratch);
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
    let inside = at(scratch.path(), &["nosuch", "tag", "add", "x"]);
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        // Run as root, nothing is refused: there is nothing to test.
        return;
    }
    let output = at(scratch.path(), &["locked", "-c", "diameter"]);
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
    pieces(&scratch);
    scratch.write(
        "inspected.md",
        "---\nschema_version: 1\nname: inspected\nproperties:\n  radius: 1.05\ntables:\n  inspection:\n    \
         index: inspector\n    columns:\n      inspector: {}\n      score: {}\n    rows:\n      \
         - inspector: Ivo\n        score: 40\n      - inspector: Lea\n        score: 44\n  trial:\n    \
         index: day\n    columns:\n      day: {}\n      hardness: {}\n      temp: {unit: degC}\n    \
         rows:\n      - day: 1\n        hardness: 1.05\n        temp: 18\n---\n",
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
        &["inspected.md", "set", "radius=true"],
        "a yes-or-no, not a number",
    );
    refused(
        &["inspected.md", "set", "tags=a,b"],
        "samplekit FILE tag add",
    );
    refused(
        &["inspected.md", "set", "radius.x=1"],
        "x is no channel of radius",
    );
    refused(
        &[
            "inspected.md",
            "set",
            "--add-row",
            "trial",
            "day=2",
            "hardness=1,011",
        ],
        "write the number alone: hardness=1.011",
    );
    refused(
        &[
            "inspected.md",
            "set",
            "--add-row",
            "trial",
            "day=2",
            "temp=18.1,18.3,18.2",
        ],
        "temp.readings=18.1,18.3,18.2",
    );
    let row = refused(
        &["inspected.md", "set", "inspection.score[lea]=45"],
        "did you mean 'Lea'?",
    );
    assert!(!row.contains("matched"), "{row}");
    refused(
        &[
            "inspected.md",
            "set",
            "--add-row",
            "trial",
            "day=1",
            "hardness=1.04",
        ],
        "row 1 of the table",
    );
    let table = refused(
        &["gamma-zinc.md", "set", "trial.hardness[1]=1"],
        "unknown table",
    );
    assert!(!table.contains("available: \n"), "{table}");
}

#[test]
fn not_applicable_carries_no_uncertainty() {
    let scratch = Scratch::new("na-uncertainty");
    pieces(&scratch);
    let output = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "radius=n/a", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains("their mean"), "{}", out(&output));
    let csv = at(
        scratch.path(),
        &[
            "alpha-titanium.md",
            "-c",
            "name,radius.u,radius.readings",
            "--csv",
        ],
    );
    let text = out(&csv);
    let row = text.lines().nth(1).unwrap_or_default();
    assert!(row.starts_with("alpha-titanium,,"), "{text}");
    assert!(row.contains("1.063"), "the readings stay: {text}");
}

#[test]
fn help_examples_run_on_the_demo_as_written() {
    let scratch = Scratch::new("help-demo");
    pieces(&scratch);
    let tag = at(scratch.path(), &["tag", "remove", "--help"]);
    assert!(
        !out(&tag).contains("prize pieces/alpha-titanium.md"),
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
    assert!(!out(&set).contains("tempering"), "{}", out(&set));
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
         drop:\n    v: 14.0\n    computed: {trial.hardness: aaaaaaaaaaaa}\n    fingerprint: bbbbbbbbbbbb\n\
         tables:\n  trial:\n    index: day\n    columns:\n      day: {}\n      hardness: {}\n      \
         temp: {unit: degC}\n    rows:\n      - day: 1\n        hardness: 1.05\n        temp: 18\n---\n",
    );
    let entered = out(&at(scratch.path(), &["b.md", "explain", "t"]));
    assert_eq!(entered.matches("entered").count(), 1, "{entered}");
    let table = out(&at(scratch.path(), &["b.md", "explain", "trial"]));
    assert!(table.contains("°C"), "{table}");
    assert!(!table.contains("degC"), "{table}");
    let inputs = out(&at(scratch.path(), &["b.md", "explain", "drop"]));
    // Each input's name stands apart from its value.
    assert!(
        !inputs.contains("trial.hardness1") && !inputs.contains("trial.hardness 1"),
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
        &["billet-01.sample.md", "tag", "add", "best", "--write"],
    );
    assert!(
        out(&tagged).contains("written: billet-01.sample.md"),
        "{}",
        out(&tagged)
    );
    fs::create_dir_all(scratch.path().join("sub")).unwrap();
    let exported = at(
        &scratch.path().join("sub"),
        &["..", "export", "ductilities", "--write"],
    );
    assert_eq!(code(&exported), 0, "{}", err(&exported));
    assert!(
        out(&exported).contains("written: ../out/ductilities.csv"),
        "{}",
        out(&exported)
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
    pieces(&scratch);
    let set = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "modulus=7", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let status = at(scratch.path(), &[".", "status"]);
    assert!(
        out(&status).contains(
            "2 values typed over their formulas are kept — samplekit compute --force gives them \
             back to their formulas"
        ),
        "{}",
        out(&status)
    );
    let narrow = at(scratch.path(), &[".", "status", "--width", "30"]);
    assert!(out(&narrow).contains("state"), "{}", out(&narrow));
    assert!(!out(&narrow).contains("hidden"), "{}", out(&narrow));
    assert!(!out(&narrow).contains("-c"), "{}", out(&narrow));
}

#[test]
fn a_figure_no_sample_can_draw_is_the_datas_answer() {
    let scratch = Scratch::new("figure-data");
    pieces(&scratch);
    // A named figure and one axis: the figure draws its own.
    let named = at(scratch.path(), &[".", "plot", "strength", "-x", "radius"]);
    assert_eq!(code(&named), 1, "{}", err(&named));
    assert!(
        err(&named).contains("a named figure draws its own axes"),
        "{}",
        err(&named)
    );
    // The one sample selected holds no modulus: exit 2, said of that sample.
    let empty = at(
        scratch.path(),
        &[
            "gamma-zinc.md",
            "plot",
            "-x",
            "radius",
            "-y",
            "modulus",
            "-o",
            "f.pdf",
        ],
    );
    assert_eq!(code(&empty), 2, "{}", err(&empty));
    assert!(
        err(&empty).contains("the one sample selected holds no modulus"),
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
         diameter: {v: 12.5}\n  volume:\n    v: 4.0\n    \
         computed: {diameter: '000000000000'}\n---\nN.\n",
    );
    let output = at(scratch.path(), &[".", "status"]);
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
        "zinc.md",
        &format!(
            "---\nschema_version: 1\nname: zinc\nproperties:\n  radius: {{v: 1.05}}\n  \
             notch: {{unit: SG}}\n  modulus: {{v: 5.0, computed: {{radius: {one}, notch: '000000000000'}}}}\n---\n"
        ),
    );
    let listed = at(scratch.path(), &[".", "status"]);
    assert!(out(&listed).contains("waits for notch"), "{}", out(&listed));
    let gated = at(scratch.path(), &[".", "status", "--exit-code"]);
    assert_eq!(code(&gated), 0, "{}{}", out(&gated), err(&gated));
    // Something stale beside it still fails.
    scratch.write(
        "nickel.md",
        "---\nschema_version: 1\nname: nickel\nproperties:\n  diameter: {v: 12.5}\n  \
         volume: {v: 4.0, computed: {diameter: '000000000000'}}\n---\n",
    );
    let stale = at(scratch.path(), &[".", "status", "--exit-code"]);
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
    let before = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
    for arguments in [
        vec!["tag", "remove", "refrence", "--write"],
        vec!["tag", "rename", "refrence", "reference", "--write"],
        vec!["tag", "rename", "reference", "reference", "--write"],
    ] {
        let output = at(scratch.path(), &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        assert!(
            out(&output).contains("nothing to change"),
            "{arguments:?}: {}",
            out(&output)
        );
    }
    let removed = at(scratch.path(), &["tag", "remove", "refrence"]);
    assert!(
        out(&removed).contains("nothing to change — no sample carries 'refrence'"),
        "{}",
        out(&removed)
    );
    assert!(out(&removed).contains("'reference'"), "{}", out(&removed));
    let after = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
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
        &["billet-04.sample.md", "tag", "add", "Reference"],
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
    samplekit::config::version_control::snapshot(&config, "python -c · saved billet-01.sample.md")
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
    let export = at(scratch.path(), &["export", "ductilities", "--write"]);
    assert_eq!(code(&export), 0, "{}", err(&export));
    for command in ["log", "diff"] {
        let output = at(scratch.path(), &["out/ductilities.csv", command]);
        assert_eq!(code(&output), 0, "{command}: {}", err(&output));
        assert!(
            out(&output).contains("samplekit out/ductilities.csv explain"),
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
        &["billet-01.sample.md", "set", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    fs::remove_file(scratch.path().join("billet-01.sample.md")).unwrap();
    let preview = at(scratch.path(), &["billet-01.sample.md", "restore"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("restore · back to #1"),
        "{}",
        out(&preview)
    );
    assert!(
        out(&preview).contains("the last content kept of billet-01.sample.md, deleted since"),
        "{}",
        out(&preview)
    );
    let written = at(
        scratch.path(),
        &["billet-01.sample.md", "restore", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let back = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
    assert!(back.contains("status: kept"), "{back}");
}

#[test]
fn the_history_is_read_past_a_configuration_that_does_not_parse() {
    // A configuration broken by an edit is what a restore is there to mend:
    // `log`, `diff` and `restore` read the history with the configuration it
    // last kept, and say so, rather than refusing on the parse error.
    let scratch = Scratch::new("history-broken-rc");
    scratch.corpus();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let rc = scratch.path().join(".samplekitrc");
    let kept = fs::read_to_string(&rc).unwrap();
    fs::write(&rc, format!("{kept}bad = [\n")).unwrap();
    let log = at(scratch.path(), &["log"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    assert!(
        err(&log).contains("the configuration it last kept"),
        "{}",
        err(&log)
    );
    assert!(out(&log).contains("status=kept"), "{}", out(&log));
    let diff = at(scratch.path(), &["diff", "--from", "1"]);
    assert_eq!(code(&diff), 0, "{}", err(&diff));
    assert!(out(&diff).contains(".samplekitrc"), "{}", out(&diff));
    let restored = at(scratch.path(), &[".samplekitrc", "restore", "--write"]);
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert_eq!(fs::read_to_string(&rc).unwrap(), kept);
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
        &["billet-01.sample.md", "set", "status=kept", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let path = scratch.path().join("billet-01.sample.md");
    let kept = fs::read_to_string(&path).unwrap();
    fs::write(&path, kept.replace("status: kept", "status: by-hand")).unwrap();
    let preview = at(scratch.path(), &["billet-01.sample.md", "restore"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("restore · back to #1"),
        "{}",
        out(&preview)
    );
    assert!(
        out(&preview).contains(
            "the last content kept of billet-01.sample.md, changed outside SampleKit since"
        ),
        "{}",
        out(&preview)
    );
    let written = at(
        scratch.path(),
        &["billet-01.sample.md", "restore", "--write"],
    );
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
    assert!(out(&none).contains("nothing to change —"), "{}", out(&none));
    let tagged = at(scratch.path(), &["tag", "add", "x", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let first = at(scratch.path(), &["restore", "--at", "1"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(
        out(&first).contains("nothing to change — the files are already as it kept them"),
        "{}",
        out(&first)
    );
}

#[test]
fn a_preview_shows_a_value_at_its_declared_precision() {
    // As a table shows it, where the digits a computation leaves read as noise,
    // and every digit where nothing is declared; two values the precision
    // writes alike are said whole, since they are still a change.
    let scratch = Scratch::new("preview-digits");
    pieces(&scratch);
    let output = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "modulus=7", "volume=19.123456"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    assert!(said.contains("6.8 %  →  7.0 %"), "{said}");
    assert!(said.contains("19.5 L  →  19.123456 L"), "{said}");
    let hidden = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "modulus=6.83"],
    );
    assert_eq!(code(&hidden), 0, "{}", err(&hidden));
    assert!(
        out(&hidden).contains("6.825 %  →  6.830 %"),
        "{}",
        out(&hidden)
    );
    // A value typed is shown as it is written, every digit: a new sample's
    // `radius=1.0456789` was shown `1.046`, and the typing could not be checked.
    let created = at(scratch.path(), &["new", "tin", "radius=1.0456789"]);
    assert_eq!(code(&created), 0, "{}", err(&created));
    assert!(out(&created).contains("1.0456789"), "{}", out(&created));
}

#[test]
fn a_blank_value_removes_the_value_and_says_so() {
    // `style=` and `style= ` remove the value, an edit said *removed*, where
    // `nickel → —` read as a value nobody had given yet; on a sample without the
    // name there is nothing to remove.
    let scratch = Scratch::new("blank-removes");
    pieces(&scratch);
    for blank in ["style=", "style=  "] {
        let output = at(scratch.path(), &["alpha-titanium.md", "set", blank]);
        assert_eq!(code(&output), 0, "{}", err(&output));
        assert!(
            out(&output).contains("titanium  →  removed"),
            "{blank:?}: {}",
            out(&output)
        );
    }
    let written = at(
        scratch.path(),
        &["alpha-titanium.md", "set", "style=", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(scratch.path().join("alpha-titanium.md")).unwrap();
    assert!(!text.contains("style:"), "{text}");
    let absent = at(scratch.path(), &["alpha-titanium.md", "set", "process="]);
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
    pieces(&scratch);
    for name in ["", " "] {
        let output = at(scratch.path(), &["new", name, "--write"]);
        assert_eq!(code(&output), 1, "{}", out(&output));
        assert!(err(&output).contains("cannot be empty"), "{}", err(&output));
    }
    let spaced = at(scratch.path(), &["new", "bronze alloy", "--write"]);
    assert_eq!(code(&spaced), 1, "{}", out(&spaced));
    assert!(
        err(&spaced).contains("write bronze-alloy"),
        "{}",
        err(&spaced)
    );
    assert!(!err(&spaced).contains("--into"), "{}", err(&spaced));
    let dashed = at(scratch.path(), &["new", "--write", "--", "-bronze"]);
    assert_eq!(code(&dashed), 1, "{}", out(&dashed));
    assert!(
        err(&dashed).contains("begins with a dash"),
        "{}",
        err(&dashed)
    );
    assert!(!scratch.path().join("bronze alloy.md").exists());
    assert!(!scratch.path().join("-bronze.md").exists());
    // A folder a shell would split is still quoted in what is suggested.
    fs::create_dir_all(scratch.path().join("my pieces")).unwrap();
    let into = at(
        scratch.path(),
        &["new", "tin", "--into", "my pieces", "--write"],
    );
    assert_eq!(code(&into), 0, "{}", err(&into));
    assert!(
        out(&into).contains("samplekit 'my pieces/tin.md' set <field>=<value>"),
        "{}",
        out(&into)
    );
    // --keep names the sample and the fields: one alone is refused.
    let kept = at(scratch.path(), &["new", "x", "--keep", "radius"]);
    assert_eq!(code(&kept), 1, "{}", out(&kept));
    assert!(err(&kept).contains("<SAMPLE> <FIELDS>"), "{}", err(&kept));
}

#[test]
fn restore_previews_at_the_declared_precision() {
    // `restore` previews as `set` does: at the project's precision, a change
    // the precision hides said whole, and as a table shows it where none is
    // declared; `diff` says the same.
    let scratch = Scratch::new("restore-digits");
    pieces(&scratch);
    let set = at(
        scratch.path(),
        &[
            "gamma-zinc.md",
            "set",
            "radius=1.0661234",
            "volume=20.123456",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let preview = at(scratch.path(), &["gamma-zinc.md", "restore"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(said.contains("1.0661  →  1.0660"), "{said}");
    assert!(said.contains("20.1235 L  →  20 L"), "{said}");
    let compared = at(scratch.path(), &["gamma-zinc.md", "diff"]);
    assert!(
        out(&compared).contains("1.0660  →  1.0661"),
        "{}",
        out(&compared)
    );
    assert!(!out(&compared).contains("records"), "{}", out(&compared));
    let moved = at(
        scratch.path(),
        &["gamma-zinc.md", "set", "radius=1.07", "--write"],
    );
    assert_eq!(code(&moved), 0, "{}", err(&moved));
    let preview = at(scratch.path(), &["gamma-zinc.md", "restore"]);
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
        "---\nschema_version: 1\nproperties:\n  diameter: {v: 2.0}\n---\n",
    );
    scratch.write(
        "a-2.md",
        "---\nschema_version: 1\nname: Alpha\nproperties:\n  diameter: {v: 1.0}\n---\n",
    );
    let listed = at(scratch.path(), &["-c", "name,diameter", "-s", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let said = out(&listed);
    let (first, second) = (said.find("Alpha"), said.find("b-1"));
    assert!(first.is_some() && second.is_some(), "{said}");
    assert!(first < second, "{said}");
    let filtered = at(
        scratch.path(),
        &["-c", "name,diameter", "-f", "name == b-1"],
    );
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
        "---\nschema_version: 1\nproperties:\n  diameter: {v: 2.0}\n---\n",
    );
    let preview = at(scratch.path(), &["s-1.md", "set", "name=Slab A, batch 3"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("s-1  →  Slab A, batch 3"),
        "{}",
        out(&preview)
    );
    let written = at(
        scratch.path(),
        &["s-1.md", "set", "name=Slab A, batch 3", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(scratch.path().join("s-1.md")).unwrap();
    assert!(text.contains("name: \"Slab A, batch 3\""), "{text}");
    let removed = at(scratch.path(), &["s-1.md", "set", "name=", "--write"]);
    assert_eq!(code(&removed), 0, "{}", err(&removed));
    assert!(
        out(&removed).contains("Slab A, batch 3  →  s-1"),
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
    let again = at(scratch.path(), &["s-1.md", "set", "name="]);
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
        "---\nschema_version: 1\nname: a\ntables:\n  inspection:\n    index: inspector\n    columns:\n      \
         inspector: {}\n      score: {}\n    rows:\n      - {inspector: Lea, score: 40}\n      \
         - {inspector: Sam, score: 38}\n  ageing:\n    index: day\n    columns:\n      \
         day: {}\n      hardness: {}\n    rows:\n      - {day: 1, hardness: 1.03}\n      \
         - {day: 4, hardness: 1.02}\n      - {day: 8, hardness: 1.01}\n---\n",
    );
    let column = at(scratch.path(), &[".", "-c", "name,inspection.score[Lee]"]);
    assert_eq!(code(&column), 1, "{}", err(&column));
    assert!(
        err(&column).contains("did you mean: 'inspection.score[Lea]'?"),
        "{}",
        err(&column)
    );
    assert!(
        err(&column).contains("the rows of 'inspection', by inspector: Lea, Sam"),
        "{}",
        err(&column)
    );
    let filtered = at(scratch.path(), &[".", "-f", "inspection.score[Lee] > 1"]);
    assert_eq!(code(&filtered), 1, "{}", err(&filtered));
    assert!(
        err(&filtered).contains("did you mean Lea?"),
        "{}",
        err(&filtered)
    );
    let explained = at(
        scratch.path(),
        &["a.md", "explain", "inspection.score[Lee]"],
    );
    assert_eq!(code(&explained), 1, "{}", err(&explained));
    assert!(
        err(&explained).contains("did you mean: 'inspection.score[Lea]'?"),
        "{}",
        err(&explained)
    );
    // A number by proximity, a channel kept.
    let numbered = at(scratch.path(), &["a.md", "explain", "ageing.hardness[5].u"]);
    assert!(
        err(&numbered).contains("did you mean: 'ageing.hardness[4].u'?"),
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
            "{rc}\n[export.all]\nprofile = \"ductilities\"\nformat = \"csv\"\noutput = \"out/all.csv\"\n"
        ),
    );
    let only = "name == \"bare\"";
    let sorted = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "-s", "diameter", "-f", only],
    );
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    assert!(out(&sorted).contains("bare"), "{}", out(&sorted));

    let narrowed = at(
        scratch.path(),
        &[".", "export", "all", "-o", "-", "-f", only],
    );
    assert_eq!(code(&narrowed), 0, "{}", err(&narrowed));
    let header = out(&narrowed)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(header.starts_with("diameter_value [g],"), "{header}");
    assert!(header.contains("porosity_value"), "{header}");
    assert!(!header.contains("uncertainty"), "{header}");
    assert_eq!(out(&narrowed).lines().count(), 2, "{}", out(&narrowed));

    let misspelt = at(
        scratch.path(),
        &[".", "-c", "name", "-s", "diameterr", "-f", only],
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
        &["billet-02.sample.md", "set", "diameter=n/a", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let written = fs::read_to_string(scratch.path().join("billet-02.sample.md")).unwrap();
    assert!(written.contains("n/a"), "{written}");
    let table = out(&at(scratch.path(), &[".", "-c", "name,diameter"]));
    assert!(
        table
            .lines()
            .any(|line| line.contains("billet-02") && line.contains("n/a")),
        "{table}"
    );
    let heavy = out(&at(
        scratch.path(),
        &[".", "-f", "diameter > 0", "-c", "name"],
    ));
    assert!(!heavy.contains("billet-02"), "{heavy}");
    let summary = out(&at(scratch.path(), &[".", "-c", "diameter", "--summary"]));
    assert!(summary.contains("3/3 (1 n/a)"), "{summary}");
    let csv = out(&at(scratch.path(), &[".", "-c", "name,diameter", "--csv"]));
    assert!(csv.contains("billet-02,n/a,\n"), "{csv}");
    let tsv = out(&at(scratch.path(), &[".", "-c", "name,diameter", "--tsv"]));
    assert!(tsv.contains("billet-02\tn/a\t\n"), "{tsv}");
    // JSON keeps null: a string where numbers go breaks every reader of it.
    let json = out(&at(scratch.path(), &[".", "-c", "name,diameter", "--json"]));
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
        &[
            ".",
            "-c",
            "name,diameter",
            "--csv",
            "-o",
            "g.json",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let written = fs::read_to_string(scratch.path().join("g.json")).unwrap();
    assert!(written.starts_with("name,diameter_value [g],"), "{written}");
}

#[test]
fn an_export_is_current_by_the_fields_it_writes() {
    // `set ore_mass=6.2` made an overview holding no ore_mass *not
    // current*: an export is judged by the fields it writes and by what they
    // are computed from.
    let scratch = Scratch::new("export-fields-current");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.light]\ncolumns = [{field = \"name\"}, {field = \"diameter\"}, \
         {field = \"porosity\"}]\n[export.light]\nprofile = \"light\"\nformat = \"csv\"\n\
         output = \"light.csv\"\n",
    );
    let volume = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        value: Some(samplekit::core::value::Value::number(4.0).unwrap()),
        ..Default::default()
    });
    scratch.write(
        "a.md",
        &format!(
            "---\nschema_version: 1\nname: a\nstyle: titanium\nproperties:\n  diameter: {{v: 12.0}}\n  \
             volume: {{v: 4.0}}\n  porosity: {{v: 3.0, computed: {{volume: {volume}}}}}\n---\n"
        ),
    );
    let export = at(scratch.path(), &["export", "light", "--write"]);
    assert_eq!(code(&export), 0, "{}", err(&export));
    let unrelated = at(scratch.path(), &["a.md", "set", "style=nickel", "--write"]);
    assert_eq!(code(&unrelated), 0, "{}", err(&unrelated));
    let current = at(scratch.path(), &["light.csv", "explain"]);
    assert_eq!(code(&current), 0, "{}", err(&current));
    assert!(out(&current).contains("up to date"), "{}", out(&current));
    assert!(!out(&current).contains("style"), "{}", out(&current));
    // `porosity` is written, and computed from `volume`.
    let input = at(scratch.path(), &["a.md", "set", "volume=5", "--write"]);
    assert_eq!(code(&input), 0, "{}", err(&input));
    let behind = at(scratch.path(), &["light.csv", "explain"]);
    assert!(out(&behind).contains("volume"), "{}", out(&behind));
    assert!(out(&behind).contains("not current"), "{}", out(&behind));
}

#[test]
fn a_filter_leaves_aside_a_sample_whose_field_holds_text() {
    // One sample's `radius` written `high` refused `radius > 1.06` over the whole
    // collection: it is left aside and named on stderr, exit 0.
    let scratch = Scratch::new("filter-text-aside");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, radius) in [("a", "1.05"), ("b", "high"), ("c", "1.07")] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  radius: {radius}\n---\n"
            ),
        );
    }
    let output = at(scratch.path(), &[".", "-f", "radius > 1.06", "-c", "name"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains('c'), "{}", out(&output));
    assert!(!out(&output).contains('b'), "{}", out(&output));
    assert!(
        err(&output).contains("b left aside: 'radius' holds text there, which > cannot compare"),
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
        vec!["billet-01.sample.md", "validate"],
        vec!["billet-01.sample.md", "billet-02.sample.md", "validate"],
    ] {
        let output = at(scratch.path(), &targets);
        assert_eq!(code(&output), 0, "{targets:?}: {}", out(&output));
        assert!(!out(&output).contains("query 'bad'"), "{}", out(&output));
    }
    let whole = at(scratch.path(), &[".", "validate"]);
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
            "name,diameter",
            "-s",
            "-diameter",
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
    let csv = at(
        scratch.path(),
        &[".", "-c", "diameter", "--summary", "--csv"],
    );
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert_eq!(
        csv_header(&out(&csv)),
        "column,unit,n,of,not_applicable,mean,weights,s,sem,median,min,max"
    );
    assert!(out(&csv).contains(",1/u²,"), "{}", out(&csv));
    // The text the screen shows, as a table's CSV writes it: `9.70` and
    // `12.30`, where the file held `9.7` and `12.3`.
    let shown = out(&at(
        scratch.path(),
        &[".", "-c", "diameter:.2f", "--summary"],
    ));
    assert!(shown.contains("12.15 │ 9.70 │ 12.30"), "{shown}");
    let written = out(&at(
        scratch.path(),
        &[".", "-c", "diameter:.2f", "--summary", "--csv"],
    ));
    assert!(written.contains(",12.15,9.70,12.30"), "{written}");
    let json = at(
        scratch.path(),
        &[".", "-c", "diameter", "--summary", "--json"],
    );
    assert_eq!(code(&json), 0, "{}", err(&json));
    assert!(out(&json).contains("\"median\""), "{}", out(&json));
    assert!(out(&json).contains("\"weights\""), "{}", out(&json));
}

#[test]
fn a_number_is_refused_as_text_where_only_its_statistic_is_written() {
    // A computation left `notch` holding only the statistic it declares, and
    // `set notch=1,016` wrote it as text, exit 0.
    let scratch = Scratch::new("statistic-only");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  notch: {statistics: {v: mean, u: standard_error}}\n---\n",
    );
    let refused = at(scratch.path(), &["a.md", "set", "notch=1,016"]);
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    assert!(err(&refused).contains("notch=1.016"), "{}", err(&refused));
}

#[test]
fn an_output_is_never_the_models_environment() {
    // `-o .venv/pyvenv.cfg` wrote over the environment the model runs in,
    // exit 0; through `.venv/bin/python`, a link, it reached the system's.
    let scratch = Scratch::new("output-environment");
    scratch.corpus();
    let rc = scratch.path().join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    fs::write(&rc, format!("{text}\n[model]\npython = \".venv\"\n")).unwrap();
    scratch.write(".venv/pyvenv.cfg", "home = /usr\n");
    for output in [".venv/pyvenv.cfg", ".venv/bin/python"] {
        let written = at(
            scratch.path(),
            &[".", "-c", "name", "--csv", "-o", output, "--write"],
        );
        assert_eq!(code(&written), 1, "{output}: {}", err(&written));
        assert!(
            err(&written).contains("the environment the model runs in"),
            "{}",
            err(&written)
        );
    }
    assert_eq!(
        fs::read_to_string(scratch.path().join(".venv/pyvenv.cfg")).unwrap(),
        "home = /usr\n"
    );
}

#[test]
fn a_filter_nested_past_reading_is_refused() {
    // Ten thousand parentheses overflowed the stack: SIGABRT, exit 134.
    let scratch = Scratch::new("deep-filter");
    scratch.corpus();
    let deep = format!("{}name == a{}", "(".repeat(10_000), ")".repeat(10_000));
    let refused = at(scratch.path(), &[".", "-f", &deep]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("at most 200 parentheses"),
        "{}",
        err(&refused)
    );
}

#[test]
fn a_frontmatter_nested_deep_is_refused_at_once() {
    // Brackets sixty thousand deep held every command reading the folder for
    // thirteen seconds before the parser refused them.
    let scratch = Scratch::new("deep-frontmatter");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let deep = format!("{}a{}", "[".repeat(60_000), "]".repeat(60_000));
    scratch.write(
        "deep.md",
        &format!("---\nschema_version: 1\nstyle: {deep}\n---\n"),
    );
    let began = std::time::Instant::now();
    let checked = at(scratch.path(), &[".", "validate"]);
    assert!(began.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        out(&checked).contains("nest deeper than 64"),
        "{}",
        out(&checked)
    );
}

#[test]
fn a_name_holding_what_a_screen_does_not_show_is_refused() {
    // A zero-width space or a direction override made names that look one,
    // or read otherwise than they are; `x.MD` made `x.MD.md`.
    let scratch = Scratch::new("invisible-names");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for name in ["a\u{200b}b", "x\u{202e}ab", "\u{feff}c"] {
        let refused = at(scratch.path(), &["new", name]);
        assert_eq!(code(&refused), 1, "{name:?}");
        assert!(
            err(&refused).contains("a screen does not show"),
            "{}",
            err(&refused)
        );
    }
    let file = at(scratch.path(), &["new", "x.MD"]);
    assert!(err(&file).contains("its file is x.md"), "{}", err(&file));
}

#[test]
fn a_file_declaring_schema_1_is_judged_as_schema_1() {
    // A key schema 1 does not hold is an unknown field of schema 1, as any
    // other.
    let scratch = Scratch::new("declared-schema-1");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  radius: {value: 1.05}\n---\n",
    );
    let read = at(scratch.path(), &["a.md"]);
    assert!(
        err(&read).contains("unknown field `value`"),
        "{}",
        err(&read)
    );
    assert!(!err(&read).contains("predates"), "{}", err(&read));
}

#[test]
fn a_table_written_by_hand_needs_no_columns() {
    // `columns:` repeated what the model declares, and a file without it was
    // refused.
    let scratch = Scratch::new("table-no-columns");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  t:\n    index: d\n    rows:\n      - {d: 1, g: 1.5}\n      - {d: 2, g: 1.2}\n---\n",
    );
    let read = at(scratch.path(), &["a.md", "-c", "name,t.g[2]"]);
    assert_eq!(code(&read), 0, "{}", err(&read));
    assert!(out(&read).contains("1.2"), "{}", out(&read));
}

#[test]
#[cfg(unix)]
fn a_folder_holding_a_link_out_of_its_project_is_tagged_or_refused_whole() {
    // Found as its turn came, the link stopped a folder half tagged, and the
    // files after it could never be.
    let scratch = Scratch::new("tag-outside-link");
    scratch.corpus();
    let outside = Scratch::new("tag-outside-link-elsewhere");
    fs::copy(
        scratch.path().join("billet-01.sample.md"),
        outside.path().join("far.md"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("far.md"),
        scratch.path().join("a-far.md"),
    )
    .unwrap();
    let refused = at(scratch.path(), &[".", "tag", "add", "zz", "--write"]);
    assert_ne!(code(&refused), 0, "{}", out(&refused));
    assert!(
        err(&refused).contains("nothing was written"),
        "{}",
        err(&refused)
    );
    assert!(
        fs::read_dir(scratch.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "md"))
            .all(|entry| !fs::read_to_string(entry.path()).unwrap().contains("zz"))
    );
}

#[test]
#[cfg(unix)]
fn a_path_is_written_to_a_pipe_as_it_is() {
    // An escape in a file's name was stripped with the colours, and the path
    // printed named no file.
    let scratch = Scratch::new("escaped-path");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("a\u{1b}[31mb.md", "---\nschema_version: 1\nname: x\n---\n");
    let listed = at(scratch.path(), &["."]);
    assert!(
        out(&listed).contains("a\u{1b}[31mb.md"),
        "{:?}",
        out(&listed)
    );
}

#[test]
fn every_spread_is_rounded_up_where_it_is_written() {
    // A summary of an uncertainty column, a template's `{uncertainty}`, a
    // standard error read as a field and one of exactly ten were each rounded
    // to the nearest, or the float's noise up.
    let scratch = Scratch::new("spreads-up");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.q]\nprecision = [\".3f\", \".2f\"]\n\
         [profile.t]\ncolumns = [{field = \"x\", template = \"{value} +- {uncertainty}\"}]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  q: {v: 5.0, u: 0.001}\n  \
         x: {v: 2.5, u: 0.1234561}\n  \
         r: {v: 2.00537, readings: [2.0, 2.00537, 2.01074], u: 0.0031003947, \
         statistics: {v: mean, u: standard_error}}\n  p: 0\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  p: 0\n---\n",
    );
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  p: 30\n---\n",
    );
    let summary = out(&at(
        scratch.path(),
        &["a.md", "-c", "q.u", "--summary", "--csv"],
    ));
    assert!(summary.contains(",0.01,"), "{summary}");
    assert!(!summary.contains("0.00,"), "{summary}");
    let templated = out(&at(scratch.path(), &["a.md", "--profile", "t"]));
    assert!(templated.contains("2.5 +- 0.123457"), "{templated}");
    let read = out(&at(
        scratch.path(),
        &[
            "a.md",
            "-c",
            "r.stats.standard_error",
            "--precision",
            ".3f",
            "--csv",
        ],
    ));
    assert!(read.ends_with("0.004\n"), "{read}");
    // Three values 0, 0 and 30: a standard error of 10, said 10, not 11.
    let ten = out(&at(scratch.path(), &[".", "-c", "p", "--summary", "--csv"]));
    assert!(ten.contains(",10,0,0,30"), "{ten}");
}

#[test]
fn a_summary_weights_its_mean_and_says_so() {
    // Every diameter carries its uncertainty: the mean is weighted by 1/u², and the
    // header says so; a column without them keeps the plain mean.
    let scratch = Scratch::new("summary-weighted");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, diameter, u) in [("a", 1.0, 0.1), ("b", 2.0, 0.2)] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: {diameter}, u: {u}}}\n  \
                 volume: {diameter}\n---\n"
            ),
        );
    }
    let both = out(&at(scratch.path(), &[".", "-c", "diameter", "--summary"]));
    assert!(both.contains("mean (1/u²)"), "{both}");
    assert!(both.contains("1.2"), "{both}");
    // Its standard error is the weighted mean's, 1/√Σw = 1/√125 = 0.0894,
    // rounded up.
    assert!(both.contains("0.090"), "{both}");
    // Some rows weighted: each weighted mean says it, under a plain heading.
    let mixed = out(&at(
        scratch.path(),
        &[".", "-c", "diameter,volume", "--summary"],
    ));
    let diameter = mixed
        .lines()
        .find(|line| line.starts_with("diameter"))
        .unwrap();
    let volume = mixed
        .lines()
        .find(|line| line.starts_with("volume"))
        .unwrap();
    assert!(
        diameter.contains("(1/u²)") && !volume.contains("1/u²"),
        "{mixed}"
    );
    assert!(!mixed.contains("mean (1/u²"), "{mixed}");
    let plain = out(&at(scratch.path(), &[".", "-c", "volume", "--summary"]));
    assert!(!plain.contains("1/u²"), "{plain}");
    // A group whose uncertainty is 0 takes the plain mean, and says nothing
    // of 1/u²; the other group's weighted mean says it.
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nkind: z\nproperties:\n  diameter: {v: 3.0, u: 0}\n---\n",
    );
    let grouped = out(&at(
        scratch.path(),
        &[".", "-c", "diameter", "--summary", "--group", "kind"],
    ));
    let zero = grouped.lines().find(|line| line.starts_with('z')).unwrap();
    assert!(!zero.contains("1/u²"), "{grouped}");
    assert!(grouped.contains("(1/u²)"), "{grouped}");
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
            "diameter",
            "--summary",
            "--group",
            "status,name",
            "--csv",
        ],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,column,"), "{csv}");
    assert!(
        lines[1].starts_with("approved,billet-01,diameter,"),
        "{csv}"
    );
    assert!(
        lines[4].starts_with("rejected,billet-04,diameter,"),
        "{csv}"
    );
    let table = out(&at(
        scratch.path(),
        &[".", "-c", "diameter", "--summary", "--group", "status,name"],
    ));
    assert!(
        table.contains("status") && table.contains("name"),
        "{table}"
    );
    let unknown = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "diameter",
            "--summary",
            "--group",
            "status,nosuch",
        ],
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
        "billet-05.sample.md",
        "---\nschema_version: 1\nname: billet-05\nproperties:\n  \
         diameter: {v: 8.0, u: 0.05, unit: g}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-c", "diameter", "--group", "status", "--width", "120"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    let approved = text.find("status = approved\n").expect(&text);
    let rejected = text.find("status = rejected\n").expect(&text);
    let none = text.find("status = (none)\n").expect(&text);
    assert!(approved < rejected && rejected < none, "{text}");
    assert!(text[approved..rejected].contains("billet-03"), "{text}");
    assert!(!text[approved..rejected].contains("billet-04"), "{text}");
    assert!(text[rejected..none].contains("billet-04"), "{text}");
    assert!(text[none..].contains("billet-05"), "{text}");
    assert_eq!(text.matches(" of 5 samples").count(), 1, "{text}");
    // Several fields: a table per combination.
    let both = out(&at(
        scratch.path(),
        &[".", "-c", "diameter", "--group", "status,name"],
    ));
    assert!(
        both.contains("status = approved · name = billet-02\n"),
        "{both}"
    );
    assert_eq!(both.matches("status = approved").count(), 3, "{both}");
    let unknown = at(
        scratch.path(),
        &[".", "-c", "diameter", "--group", "statuss"],
    );
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
    scratch.sample("billet-00.sample.md", "billet-00", "rejected", 9.0, None);
    let csv = out(&at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--group", "status", "--csv"],
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
        &[".", "-c", "status,diameter", "--group", "status", "--csv"],
    ));
    assert_eq!(csv_header(&named).matches("status").count(), 1, "{named}");
}

#[test]
fn a_grouped_document_heads_or_parts_its_groups() {
    // A Markdown or an HTML file of groups ran them together; a style's
    // `{group}` heads each, the first included.
    let scratch = Scratch::new("document-groups");
    scratch.corpus();
    scratch.sample("billet-00.sample.md", "billet-00", "rejected", 9.0, None);
    let written = |style: &str| {
        out(&at(
            scratch.path(),
            &[
                ".",
                "-c",
                "name,diameter",
                "--group",
                "status",
                "--style",
                style,
                "-o",
                "-",
            ],
        ))
    };
    let markdown = written("markdown");
    assert_eq!(markdown.matches("| **").count(), 2, "{markdown}");
    let approved = markdown.find("| **approved** |").expect(&markdown);
    let rejected = markdown.find("| **rejected** |").expect(&markdown);
    assert!(approved < rejected, "{markdown}");
    let html = written("html");
    assert!(
        html.contains("<thead>") && html.contains("</thead>"),
        "{html}"
    );
    assert_eq!(html.matches("<tbody>").count(), 2, "{html}");
    assert_eq!(html.matches("</tbody>").count(), 2, "{html}");
    // A heading of one's own, its columns counted.
    scratch.write(
        ".samplekitrc",
        &format!(
            "{}\n[style.latex]\nbetween_groups = '\\midrule \\multicolumn{{{{columns}}}}{{l}}{{{{group}}}} \\\\'\n",
            fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap()
        ),
    );
    let latex = written("latex");
    assert!(latex.contains("\\multicolumn{3}{l}{approved}"), "{latex}");
}

#[test]
fn a_filter_asks_a_tables_rows_one_at_a_time() {
    // Two conditions met in two rows are not one row meeting both.
    let scratch = Scratch::new("filter-rows");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  heat:\n    index: step\n    columns:\n      \
         step: {}\n      temperature: {unit: degC}\n      atmosphere: {}\n    rows:\n      \
         - {step: 1, temperature: 1050, atmosphere: argon}\n      \
         - {step: 2, temperature: 720, atmosphere: air}\n---\n",
    );
    scratch.write("b.md", "---\nschema_version: 1\nname: b\n---\n");
    let names = |filter: &str| {
        let output = at(scratch.path(), &[".", "-f", filter, "-c", "name", "--csv"]);
        assert_eq!(code(&output), 0, "{filter}: {}", err(&output));
        out(&output)
            .lines()
            .skip(1)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("heat.atmosphere has air"), ["a"]);
    assert_eq!(names("heat has argon"), ["a"]);
    assert_eq!(
        names("heat where (temperature > 1000 && atmosphere == argon)"),
        ["a"]
    );
    assert!(names("heat where (temperature > 1000 && atmosphere == air)").is_empty());
    assert_eq!(
        names("heat.temperature has 1050 && heat.atmosphere has air"),
        ["a"]
    );
    // A column in degC here and K there compares nothing: refused, naming
    // the column, as a field is.
    scratch.write(
        "k.md",
        "---\nschema_version: 1\nname: k\ntables:\n  heat:\n    index: step\n    columns:\n      \
         step: {}\n      temperature: {unit: K}\n    rows:\n      - {step: 1, temperature: 1300}\n---\n",
    );
    for filter in [
        "heat where (temperature > 700)",
        "heat.temperature has 1050",
    ] {
        let refused = at(scratch.path(), &[".", "-f", filter, "-c", "name"]);
        assert_ne!(code(&refused), 0, "{filter}");
        assert!(
            err(&refused).contains("'heat.temperature' is in degC and K"),
            "{filter}: {}",
            err(&refused)
        );
    }
}

#[test]
fn groups_are_made_within_each_project() {
    // The projects first, as a table heads them, and the groups within.
    let scratch = Scratch::new("groups-per-project");
    two_projects(&scratch);
    scratch.sample("b.sample.md", "root-two", "rejected", 1.0, None);
    let output = at(
        scratch.path(),
        &["nested", ".", "-c", "name,diameter", "--group", "status"],
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
    pieces(&scratch);
    scratch.write(
        "aged.md",
        "---\nschema_version: 1\nname: aged\nproperties:\n  radius: 1.05\ntables:\n  trial:\n    \
         index: day\n    columns:\n      day: {}\n      temp: {unit: degC}\n    rows:\n      \
         - day: 1\n        temp: 18\n---\n",
    );
    // A property's readings, in brackets, beside a value in one command.
    let set = at(
        scratch.path(),
        &[
            "gamma-zinc.md",
            "set",
            "radius.readings=[1.066, 1.067]",
            "modulus=5",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(out(&set).contains("· 2 changes"), "{}", out(&set));
    let written = fs::read_to_string(scratch.path().join("gamma-zinc.md")).unwrap();
    assert!(written.contains("readings: [1.066, 1.067]"), "{written}");
    // The value typed before stays beside them.
    assert!(written.contains("v: 1.066, readings"), "{written}");
    // A cell of a row that exists: its value stays beside them.
    let cell = at(
        scratch.path(),
        &["aged.md", "set", "trial.temp[1].readings=18,19", "--write"],
    );
    assert_eq!(code(&cell), 0, "{}", err(&cell));
    let written = fs::read_to_string(scratch.path().join("aged.md")).unwrap();
    assert!(
        written.contains("temp: {v: 18, readings: [18.0, 19.0]"),
        "{written}"
    );
    // A cell of a new row: readings alone, and no mean beside them.
    let row = at(
        scratch.path(),
        &[
            "aged.md",
            "set",
            "--add-row",
            "trial",
            "day=2",
            "temp.readings=17.5,18.5",
            "--write",
        ],
    );
    assert_eq!(code(&row), 0, "{}", err(&row));
    assert!(
        out(&row).contains("trial.temp.readings   —  →  17.5, 18.5"),
        "{}",
        out(&row)
    );
    let written = fs::read_to_string(scratch.path().join("aged.md")).unwrap();
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
            "inconel",
            "radius.readings=1.048,1.049",
            "notch.readings=[1.010, 1.011]",
            "--write",
        ],
    );
    assert_eq!(code(&new), 0, "{}", err(&new));
    assert!(
        out(&new).contains("radius, notch have no statistic recorded"),
        "{}",
        out(&new)
    );
    let written = fs::read_to_string(scratch.path().join("inconel.md")).unwrap();
    assert!(
        written.contains("radius: {readings: [1.048, 1.049]}"),
        "{written}"
    );
    assert!(
        written.contains("notch: {readings: [1.01, 1.011]}"),
        "{written}"
    );
    assert!(!written.contains("v:"), "no mean is written: {written}");
}

#[test]
fn the_readings_flag_is_an_unknown_option() {
    // A retired option is an unknown option, refused as any option a command
    // does not take, with no message of its own.
    let scratch = Scratch::new("readings-flag");
    pieces(&scratch);
    let before = fs::read_to_string(scratch.path().join("alpha-titanium.md")).unwrap();
    let output = at(
        scratch.path(),
        &[
            "alpha-titanium.md",
            "set",
            "--readings",
            "notch=1.011,1.010",
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
        fs::read_to_string(scratch.path().join("alpha-titanium.md")).unwrap(),
        before
    );
}

#[test]
fn readings_like_decimal_commas_are_warned_about() {
    let scratch = Scratch::new("readings-commas");
    pieces(&scratch);
    let comma = at(
        scratch.path(),
        &["gamma-zinc.md", "set", "radius.readings=20,5,21,5"],
    );
    assert_eq!(code(&comma), 0, "{}", err(&comma));
    assert!(
        err(&comma).contains("radius.readings=[20.5, 21.5]"),
        "{}",
        err(&comma)
    );
    assert!(out(&comma).contains("4 readings"), "{}", out(&comma));
    let points = at(
        scratch.path(),
        &["gamma-zinc.md", "set", "radius.readings=1.05,1.06"],
    );
    assert_eq!(code(&points), 0, "{}", err(&points));
    assert!(!err(&points).contains("decimal comma"), "{}", err(&points));
}

#[test]
fn state_selects_as_status_and_validate_say() {
    // `state` is a field every filter reads, and a column.
    let scratch = Scratch::new("state-field");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a-failed.md",
        "---\nschema_version: 1\nname: a-failed\nproperties:\n  diameter: {v: 2.0}\n  \
         volume: {computed: {diameter: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    scratch.write(
        "b-defective.md",
        "---\nschema_version: 1\nname: b-defective\nproperties:\n  diameter: {v: \"2.0\"}\n---\nN.\n",
    );
    scratch.write(
        "c-clean.md",
        "---\nschema_version: 1\nname: c-clean\nproperties:\n  diameter: {v: 2.0}\n---\nN.\n",
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
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 2.0}\n---\nN.\n",
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
        "- billet-01.sample.md",
        "- billet-04.sample.md",
        "- .samplekitrc",
        "- billet-09.md",
    ] {
        assert!(text.contains(file), "{file}: {text}");
    }
    // A target and `--at` narrow it as they narrow `log`.
    let one = out(&at(
        scratch.path(),
        &["billet-01.sample.md", "log", "--export"],
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
fn two_pieces(scratch: &Scratch, folder: &str) -> PathBuf {
    scratch.write(&format!("{folder}/.samplekitrc"), "schema_version = 1\n");
    for (name, diameter) in [("a", "1.0"), ("b", "2.0")] {
        scratch.write(
            &format!("{folder}/{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {diameter}\n---\n"
            ),
        );
    }
    scratch.path().join(folder)
}

#[test]
fn the_history_commands_above_several_projects_answer_for_each() {
    // From the folder above two projects, `status` and `set` read both, and
    // `log`, `diff` and `restore` said no history was kept here.
    let scratch = Scratch::new("history-above");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for project in ["rolled", "cast"] {
        let root = two_pieces(&scratch, project);
        let set = at(&root, &["a.md", "set", "diameter=9", "--write"]);
        assert_eq!(code(&set), 0, "{}", err(&set));
    }
    let log = at(scratch.path(), &["log"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    let listed = out(&log);
    assert!(
        listed.contains("rolled\n") && listed.contains("cast\n"),
        "{listed}"
    );
    assert!(!listed.contains("no history is kept"), "{listed}");
    let diff = out(&at(scratch.path(), &["diff"]));
    assert!(diff.matches("diameter").count() >= 2, "{diff}");
    let restore = out(&at(scratch.path(), &["restore"]));
    assert!(restore.matches("take back").count() == 2, "{restore}");
    // A snapshot is one project's: refused, naming them.
    // So is an instant of `log` and `diff`, before anything is said.
    for command in [
        vec!["-c", "name", "--at", "1"],
        vec!["restore", "--at", "1"],
        vec!["log", "--at", "1"],
        vec!["diff", "--from", "1"],
    ] {
        let refused = at(scratch.path(), &command);
        assert_eq!(code(&refused), 1, "{command:?}: {}", out(&refused));
        assert!(out(&refused).is_empty(), "{command:?}: {}", out(&refused));
        assert!(
            err(&refused).contains("cast, rolled") && err(&refused).contains(" 1"),
            "{}",
            err(&refused)
        );
    }
}

#[test]
fn a_clones_first_write_joins_the_history_it_came_with() {
    // A file edited by hand on a machine that had written nothing yet: its
    // first snapshot was a root of its own, and a restore to it took the
    // other machine's work away.
    let scratch = Scratch::new("clone-first-write");
    let root = two_pieces(&scratch, "p");
    let first = as_machine(&root, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let edited = fs::read_to_string(root.join("b.md"))
        .unwrap()
        .replace("2.0", "2.2");
    fs::write(root.join("b.md"), edited).unwrap();
    let written = as_machine(&root, "tom", &["a.md", "set", "diameter=1.6", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let log = out(&as_machine(&root, "tom", &["log"]));
    let outside = log
        .lines()
        .find(|line| line.contains("changed outside SampleKit"))
        .unwrap_or_default();
    // The machine reading is itself, you.
    assert!(outside.contains("you, joins ana"), "{log}");
    assert!(outside.trim_end().ends_with('b'), "{log}");
    if let Ok(roots) = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["rev-list", "--max-parents=0", "--all"])
        .output()
    {
        assert_eq!(
            String::from_utf8_lossy(&roots.stdout).lines().count(),
            1,
            "{log}"
        );
    }
}

#[test]
fn restore_takes_back_my_change_and_keeps_an_edit_by_hand() {
    // Without a file named, restore took back an edit by hand first; named,
    // a file edited by hand went back to this machine's last content, and a
    // colleague's work kept since went with it.
    let scratch = Scratch::new("restore-by-hand");
    let root = two_pieces(&scratch, "p");
    let mine = as_machine(&root, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&mine), 0, "{}", err(&mine));
    let theirs = as_machine(&root, "tom", &["a.md", "set", "style=titanium", "--write"]);
    assert_eq!(code(&theirs), 0, "{}", err(&theirs));
    let edited = fs::read_to_string(root.join("a.md"))
        .unwrap()
        .replace("name: a\n", "name: a\nprocess: us-05\n");
    fs::write(root.join("a.md"), edited).unwrap();
    let log = out(&as_machine(&root, "ana", &["log"]));
    let now = log
        .lines()
        .find(|line| line.contains("now"))
        .unwrap_or_default();
    assert!(now.contains("you"), "{log}");
    // Nothing named: my change is taken back, the edit by hand said and kept.
    let unnamed = out(&as_machine(&root, "ana", &["restore"]));
    assert!(unnamed.contains("restore · take back"), "{unnamed}");
    assert!(unnamed.contains("diameter=1.5"), "{unnamed}");
    assert!(
        unnamed.contains("a.md, changed outside SampleKit since, stays"),
        "{unnamed}"
    );
    assert!(!unnamed.contains("us-05  →"), "{unnamed}");
    // Named: the edit by hand is taken back, and tom's style stays.
    let named = as_machine(&root, "ana", &["a.md", "restore", "--write"]);
    assert_eq!(code(&named), 0, "{}", err(&named));
    let held = fs::read_to_string(root.join("a.md")).unwrap();
    assert!(!held.contains("us-05"), "{held}");
    assert!(held.contains("style: titanium"), "{held}");
    assert!(held.contains("1.5"), "{held}");
}

#[test]
fn a_configuration_broken_by_hand_is_restored_by_its_name() {
    // A write under a configuration that did not read kept the broken file;
    // the next took the model for deleted, and `restore .samplekitrc` had
    // nothing to take back.
    let scratch = Scratch::new("restore-broken-rc");
    let root = two_pieces(&scratch, "p");
    scratch.write(
        "p/.samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    );
    scratch.write("p/model.py", "import samplekit as sk\n");
    let good = fs::read_to_string(root.join(".samplekitrc")).unwrap();
    let first = at(&root, &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    fs::write(root.join(".samplekitrc"), format!("{good}garbage = [\n")).unwrap();
    for value in ["diameter=1.6", "diameter=1.7"] {
        let _ = at(&root, &["set", "a.md", value, "--write"]);
    }
    // The model is in the first snapshot, and in no change after it.
    let log = out(&at(&root, &["log"]));
    assert!(
        !log.lines()
            .filter(|line| !line.contains("first kept it"))
            .any(|line| line.contains("model.py")),
        "{log}"
    );
    let restored = at(&root, &[".samplekitrc", "restore", "--write"]);
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert_eq!(fs::read_to_string(root.join(".samplekitrc")).unwrap(), good);
    assert!(root.join("model.py").is_file());
}

#[test]
fn the_diff_of_a_join_compares_with_this_machines_last() {
    let scratch = Scratch::new("diff-join-back");
    let root = two_pieces(&scratch, "p");
    for (machine, change) in [
        ("ana", "a.md diameter=1.5"),
        ("tom", "a.md diameter=3.5"),
        ("ana", "a.md diameter=1.5"),
    ] {
        let (file, assignment) = change.split_once(' ').unwrap();
        let arguments = vec![file, "set", assignment, "--write"];
        let written = as_machine(&root, machine, &arguments);
        assert_eq!(code(&written), 0, "{}", err(&written));
    }
    // Ana set back the value she held: no change of hers, and what tom
    // changed and she undid came with the join, and is gone.
    let diff = out(&as_machine(&root, "ana", &["diff"]));
    assert!(diff.contains("this machine's last"), "{diff}");
    assert!(
        diff.contains("the files hold what this machine last kept"),
        "{diff}"
    );
    assert!(!diff.contains("3.5  →  1.5"), "{diff}");
}

#[test]
fn log_names_each_machine_and_where_they_joined() {
    let scratch = Scratch::new("log-machines");
    let root = two_pieces(&scratch, "p");
    // One machine: no column.
    let alone = as_machine(&root, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&alone), 0, "{}", err(&alone));
    let log = out(&as_machine(&root, "ana", &["log"]));
    assert!(!log.contains("machine"), "{log}");
    // Two, taking turns on one folder: the files always hold the other's.
    for (machine, change) in [("tom", "b.md diameter=2.5"), ("ana", "a.md diameter=1.7")] {
        let (file, assignment) = change.split_once(' ').unwrap();
        let arguments = vec![file, "set", assignment, "--write"];
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
    // Read on tom's machine: tom is you.
    assert!(row("1").contains("ana, joins you"), "{log}");
    assert!(
        row("1").contains("samplekit a.md set diameter=1.7 --write"),
        "{log}"
    );
    assert!(row("2").contains("you, joins ana"), "{log}");
    assert!(
        row("3").contains("ana ") && !row("3").contains("joins"),
        "{log}"
    );
    assert!(!log.contains("changed outside SampleKit"), "{log}");
    // `diff` alone shows the join's own change, not what it joined.
    // Compared with this machine's last, what came from the other apart.
    let diff = out(&as_machine(&root, "ana", &["diff"]));
    assert!(diff.contains("this machine's last"), "{diff}");
    let (own, came) = diff
        .split_once("came with the join, from #2 of tom:")
        .expect(&diff);
    assert!(
        own.contains("1.5  →  1.7") && !own.contains("b.md"),
        "{diff}"
    );
    assert!(came.contains("b.md") && came.contains("2.5"), "{diff}");
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

// Git for Windows writes what the clone and the pull bring with CRLF, as it
// does by default: the same files, which the history still joins.
#[test]
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
    let ana = two_pieces(&scratch, "ana");
    git(&ana, &["init", "-q"]);
    let written = as_machine(&ana, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    // The one line: the history no longer hides itself from git.
    fs::write(
        ana.join(".samplekit/history/.gitignore"),
        ".tmp*\n*.lock\n/HEAD\n/config\n/description\n/hooks/\n/logs/\n",
    )
    .unwrap();
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana"]);
    git(&ana, &["push", "-q", remote.to_str().unwrap(), "HEAD:main"]);
    git(
        scratch.path(),
        &["clone", "-q", "-b", "main", remote.to_str().unwrap(), "tom"],
    );
    let tom = scratch.path().join("tom");
    let written = as_machine(&tom, "tom", &["b.md", "set", "diameter=2.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    git(&tom, &["add", "-A"]);
    git(&tom, &["commit", "-qm", "tom"]);
    git(&tom, &["push", "-q", "origin", "HEAD:main"]);
    // Ana wrote meanwhile; the pull merges without a conflict.
    let written = as_machine(&ana, "ana", &["a.md", "set", "diameter=1.6", "--write"]);
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
    let written = as_machine(&ana, "ana", &["a.md", "set", "diameter=1.7", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let log = out(&as_machine(&ana, "ana", &["log"]));
    assert!(
        log.lines()
            .nth(2)
            .is_some_and(|line| line.contains("you, joins tom")),
        "{log}"
    );
    assert!(log.contains("tom, joins you"), "{log}");
    assert!(!log.contains("changed outside SampleKit"), "{log}");
}

#[test]
fn two_machines_that_each_joined_the_other_join_again() {
    // Two days each writing, then syncing: each line had joined the other's,
    // the two shared two last snapshots, and the next write took the other
    // machine's work for a change made outside SampleKit — which `restore`
    // then took back.
    let scratch = Scratch::new("criss-cross");
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
    let ana = two_pieces(&scratch, "ana");
    git(&ana, &["init", "-q"]);
    let written = as_machine(&ana, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    fs::write(
        ana.join(".samplekit/history/.gitignore"),
        ".tmp*\n*.lock\n/HEAD\n/config\n/description\n/hooks/\n/logs/\n",
    )
    .unwrap();
    let origin = remote.to_str().unwrap();
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana"]);
    git(&ana, &["push", "-q", origin, "HEAD:main"]);
    git(
        scratch.path(),
        &["clone", "-q", "-b", "main", origin, "tom"],
    );
    let tom = scratch.path().join("tom");
    for day in ["7", "8"] {
        let written = as_machine(
            &ana,
            "ana",
            &["a.md", "set", &format!("diameter={day}"), "--write"],
        );
        assert_eq!(code(&written), 0, "{}", err(&written));
        let written = as_machine(
            &tom,
            "tom",
            &["b.md", "set", &format!("diameter={day}"), "--write"],
        );
        assert_eq!(code(&written), 0, "{}", err(&written));
        git(&tom, &["add", "-A"]);
        git(&tom, &["commit", "-qm", "tom"]);
        git(&tom, &["push", "-q", "origin", "HEAD:main"]);
        git(&ana, &["add", "-A"]);
        git(&ana, &["commit", "-qm", "ana"]);
        git(
            &ana,
            &["pull", "-q", "--no-rebase", "--no-edit", origin, "main"],
        );
        git(&ana, &["push", "-q", origin, "HEAD:main"]);
        git(
            &tom,
            &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
        );
    }
    let written = as_machine(&tom, "tom", &["b.md", "set", "diameter=9", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let log = out(&as_machine(&tom, "tom", &["log"]));
    assert!(!log.contains("changed outside SampleKit"), "{log}");
    assert!(
        log.lines()
            .nth(2)
            .is_some_and(|line| line.contains("you, joins ana")),
        "{log}"
    );
}

#[test]
fn a_folder_made_a_project_on_two_computers_shares_its_history() {
    // Brought on each computer, the folder's history was not shared — its
    // `.gitignore` held `*` — and, shared by hand, each computer had written
    // the other's branch: git kept one, and a snapshot was lost.
    let scratch = Scratch::new("bring-two");
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
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let remote = scratch.path().join("remote.git");
    fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare"]);
    let origin = remote.to_str().unwrap();
    let ana = scratch.path().join("ana");
    scratch.write(
        "ana/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    for name in ["h", "d"] {
        scratch.write(
            &format!("ana/sub/{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nx: 0\n---\n"),
        );
    }
    git(&ana, &["init", "-q"]);
    let written = as_machine(&ana, "ana", &["sub/h.md", "set", "x=1", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    fs::write(
        ana.join(".samplekit/history/.gitignore"),
        ".tmp*\n*.lock\n/HEAD\n/config\n/description\n/hooks/\n/logs/\n",
    )
    .unwrap();
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana"]);
    git(&ana, &["push", "-q", origin, "HEAD:main"]);
    git(
        scratch.path(),
        &["clone", "-q", "-b", "main", origin, "tom"],
    );
    let tom = scratch.path().join("tom");
    // Made a project on ana's computer, pushed with no command run there.
    scratch.write("ana/sub/.samplekitrc", "schema_version = 1\n");
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "sub"]);
    git(&ana, &["push", "-q", origin, "HEAD:main"]);
    git(
        &tom,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    // Each writes there before the other's write arrives: each brings it.
    for (who, at, file) in [("ana", &ana, "sub/h.md"), ("tom", &tom, "sub/d.md")] {
        let written = as_machine(at, who, &[file, "set", "x=2", "--write"]);
        assert!(err(&written).contains("brought"), "{}", err(&written));
    }
    git(&tom, &["add", "-A"]);
    git(&tom, &["commit", "-qm", "tom"]);
    git(&tom, &["push", "-q", "origin", "HEAD:main"]);
    git(&ana, &["add", "-A"]);
    git(&ana, &["commit", "-qm", "ana"]);
    git(
        &ana,
        &["pull", "-q", "--no-rebase", "--no-edit", origin, "main"],
    );
    git(&ana, &["push", "-q", origin, "HEAD:main"]);
    git(
        &tom,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    for (who, at, file) in [("ana", &ana, "sub/h.md"), ("tom", &tom, "sub/d.md")] {
        let written = as_machine(at, who, &[file, "set", "x=3", "--write"]);
        assert_eq!(code(&written), 0, "{}", err(&written));
        let log = out(&as_machine(at, who, &["sub", "log"]));
        assert!(!log.contains("changed outside SampleKit"), "{log}");
        assert!(
            log.contains("sub/h.md set x=2") && log.contains("sub/d.md set x=2"),
            "{log}"
        );
        assert!(
            log.lines()
                .nth(2)
                .is_some_and(|line| line.contains("you, joins")),
            "{log}"
        );
    }
}

// ------------------------------------------------ a profile's groups

/// The corpus, under a configuration whose profiles group.
fn grouping_profiles(scratch: &Scratch) {
    scratch.corpus();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\
         [profile.by_status]\ncolumns = [{field = \"name\"}, {field = \"diameter\"}]\n\
         group = [\"status\"]\n\
         [profile.lightest]\ncolumns = [{field = \"name\"}, {field = \"diameter\"}]\n\
         sort = [\"diameter\"]\ngroup = [\"status\"]\n\
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
    assert!(text[rejected..].contains("billet-04"), "{text}");
    let replaced = out(&at(
        scratch.path(),
        &[".", "--profile", "by_status", "--group", "name"],
    ));
    assert!(replaced.contains("name = billet-01\n"), "{replaced}");
    assert!(!replaced.contains("status = "), "{replaced}");
    let csv = out(&at(
        scratch.path(),
        &[".", "--profile", "by_status", "--csv"],
    ));
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,"), "{csv}");
    assert!(lines[4].starts_with("rejected,billet-04"), "{csv}");
}

#[test]
fn an_export_whose_profile_groups_writes_one_table_carrying_its_groups() {
    // One table, as a grouped `--csv` is.
    let scratch = Scratch::new("export-groups");
    grouping_profiles(&scratch);
    let output = at(scratch.path(), &[".", "export", "by_status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let csv = out(&output);
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("status,name,"), "{csv}");
    assert!(
        lines[1..4].iter().all(|line| line.starts_with("approved,")),
        "{csv}"
    );
    assert!(lines[4].starts_with("rejected,billet-04"), "{csv}");
}

#[test]
fn the_groups_follow_the_sort_in_force() {
    // With a sort in force the groups come where their first samples come;
    // without one, by their values.
    let scratch = Scratch::new("groups-sorted");
    grouping_profiles(&scratch);
    let heaviest = out(&at(
        scratch.path(),
        &[".", "-c", "diameter", "--group", "name", "-s", "-diameter"],
    ));
    let third = heaviest.find("name = billet-03\n").expect(&heaviest);
    let first = heaviest.find("name = billet-01\n").expect(&heaviest);
    assert!(third < first, "{heaviest}");
    // By another field: billet-04, the lightest, is rejected.
    let lightest = out(&at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name,diameter",
            "--group",
            "status",
            "-s",
            "diameter",
        ],
    ));
    let rejected = lightest.find("status = rejected\n").expect(&lightest);
    let approved = lightest.find("status = approved\n").expect(&lightest);
    assert!(rejected < approved, "{lightest}");
    // No sort: the values' order.
    let unsorted = out(&at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--group", "status"],
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
            "diameter",
            "--summary",
            "--group",
            "status",
            "-s",
            "diameter",
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

#[test]
fn a_save_that_computed_nothing_writes_readings_as_they_were() {
    // A tag written to a sample whose readings stand for their value through
    // a statistic: the file keeps the readings alone, never the statistic as
    // a value with no record — which a corrected reading would never move.
    let scratch = Scratch::new("readings-as-they-were");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "forms.md",
        "---\nschema_version: 1\nname: F\nproperties:\n  hb: {readings: [40, 42, 44], \
         statistics: {v: median, u: sample_stdev}}\n---\n",
    );
    let tagged = at(
        scratch.path(),
        &["forms.md", "tag", "add", "tmp", "--write"],
    );
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("v: 42") && !text.contains("u: 2"), "{text}");
    fs::write(&file, text.replace("[40.0, 42.0, 44.0]", "[50, 52, 54]")).unwrap();
    let shown = at(scratch.path(), &["forms.md", "-c", "hb"]);
    assert!(out(&shown).contains("52.0 ± 2.0"), "{}", out(&shown));
}

#[test]
fn a_summary_writes_its_extremes_as_the_values_are() {
    // A median, a minimum and a maximum are values of the column: rounded to
    // the spread of the others, a minimum of 0.99 read 0.
    let scratch = Scratch::new("summary-extremes");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, value, u) in [
        ("b", "155", "250"),
        ("c", "123456", "250"),
        ("d", "0.99", "0.996"),
        ("e", "1234.5", "12"),
        ("f", "98765", "4321"),
    ] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  x: {{v: {value}, u: {u}}}\n---\n"),
        );
    }
    let summary = at(scratch.path(), &[".", "-c", "x", "--summary"]);
    assert_eq!(code(&summary), 0, "{}", err(&summary));
    let row = out(&summary);
    for written in ["1234.5", "0.99", "123456"] {
        assert!(row.contains(written), "{row}");
    }
}

#[test]
fn two_units_in_one_column_are_refused_on_every_surface() {
    // A table refused them; a summary, a data format and a declared export
    // took a mean of litres and millilitres, or wrote both under no unit.
    let scratch = Scratch::new("two-units");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n[profile.all]\ncolumns = \
         [{field = \"name\"}, {field = \"volume\"}]\n[export.all]\nprofile = \"all\"\nformat \
         = \"csv\"\noutput = \"all.csv\"\n",
    );
    for (name, volume, unit) in [("a", "20", "L"), ("b", "21", "L"), ("c", "20000", "mL")] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  volume: {{v: {volume}, unit: {unit}}}\n---\n"
            ),
        );
    }
    for arguments in [
        &[".", "-c", "name,volume"][..],
        &[".", "-c", "volume", "--summary"],
        &[".", "-c", "name,volume", "--csv"],
        &[".", "export", "all", "--write"],
    ] {
        let refused = at(scratch.path(), arguments);
        assert_eq!(code(&refused), 2, "{arguments:?}: {}", out(&refused));
        assert!(
            err(&refused).contains("volume is in L and mL"),
            "{arguments:?}: {}",
            err(&refused)
        );
    }
    assert!(!scratch.path().join("all.csv").exists());
}

#[test]
fn two_projects_declaring_two_units_are_refused_in_one_column() {
    let scratch = Scratch::new("two-projects-units");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n",
    );
    scratch.write("ana/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    scratch.write(
        "tom/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[property.volume]\nunit = \"gal\"\n",
    );
    scratch.write(
        "ana/a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  volume: {v: 20}\n---\n",
    );
    scratch.write(
        "tom/t.md",
        "---\nschema_version: 1\nname: T\nproperties:\n  volume: {v: 5.5}\n---\n",
    );
    for arguments in [
        &["ana", "tom", "-c", "name,volume", "--csv"][..],
        &["ana", "tom", "-c", "volume", "--summary"],
    ] {
        let refused = at(scratch.path(), arguments);
        assert_eq!(code(&refused), 2, "{arguments:?}: {}", out(&refused));
        assert!(
            err(&refused).contains("volume is in L and gal"),
            "{}",
            err(&refused)
        );
    }
    // A table per project says each.
    let tables = at(scratch.path(), &["ana", "tom", "-c", "name,volume"]);
    assert_eq!(code(&tables), 0, "{}", err(&tables));
}

#[test]
fn status_where_no_sample_is_says_so() {
    // Outside every project, `status` said every derived value in 0 samples
    // was current: a reader in the wrong folder was told all was well.
    let scratch = Scratch::new("status-nowhere");
    let answered = at(scratch.path(), &["status"]);
    assert_eq!(code(&answered), 0, "{}", err(&answered));
    assert!(
        out(&answered).contains("no sample here, and no .samplekitrc above this folder"),
        "{}",
        out(&answered)
    );
    assert!(
        !out(&answered).contains("every derived value"),
        "{}",
        out(&answered)
    );
}

#[test]
fn set_warns_of_a_value_outside_its_range_and_writes_it() {
    let scratch = Scratch::new("set-range");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\nrange = [0, 50]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  volume: {v: 20, unit: L}\n---\n",
    );
    let set = at(scratch.path(), &["a.md", "set", "volume=-5", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        err(&set).contains("volume = -5 L is outside its range, 0 to 50 L"),
        "{}",
        err(&set)
    );
    assert!(
        fs::read_to_string(scratch.path().join("a.md"))
            .unwrap()
            .contains("-5")
    );
    let inside = at(scratch.path(), &["a.md", "set", "volume=12", "--write"]);
    assert!(
        !err(&inside).contains("outside its range"),
        "{}",
        err(&inside)
    );
}

#[test]
fn status_accepts_the_overrides_of_the_values_named_alone() {
    // `--accept edited` let any value typed by hand pass, a typo included:
    // `edited:NAME,…` lets those values alone.
    let scratch = Scratch::new("accept-named");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 12.1, unit: g}\n  \
         porosity: {v: 2.8, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n  \
         ductility: {v: 7.0, computed: {diameter: '111111111111'}, fingerprint: '333333333333'}\n---\n",
    );
    let gate = |accept: &str| {
        let mut arguments = vec![".", "status", "--exit-code"];
        if !accept.is_empty() {
            arguments.extend(["--accept", accept]);
        }
        code(&at(scratch.path(), &arguments))
    };
    assert_eq!(gate(""), 2);
    assert_eq!(gate("edited"), 0);
    assert_eq!(gate("edited:porosity"), 2);
    assert_eq!(gate("edited:porosity,ductility"), 0);
    // A value waiting fails nothing: there is nothing to accept.
    let waiting = at(
        scratch.path(),
        &[
            ".",
            "status",
            "--exit-code",
            "--accept",
            "waiting,edited:ductility,porosity",
        ],
    );
    assert_eq!(code(&waiting), 1, "{}", err(&waiting));
    assert!(
        err(&waiting).contains("a value waiting for an input fails nothing"),
        "{}",
        err(&waiting)
    );
    let refused = at(
        scratch.path(),
        &[".", "status", "--exit-code", "--accept", "stale"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(err(&refused).contains("edited:NAME"), "{}", err(&refused));
}

#[cfg(target_os = "linux")]
#[test]
fn open_edit_opens_with_the_systems_application() {
    // `$VISUAL` is no longer read: the file opens as every other opening
    // does, and is said opened, SampleKit not seeing the editing.
    let scratch = Scratch::new("open-edit");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("a.md", "---\nschema_version: 1\nname: a\n---\n");
    let bin = scratch.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let opened = scratch.path().join("opened");
    let visual = scratch.path().join("visual");
    for (name, marker) in [("xdg-open", &opened), ("fake-visual", &visual)] {
        let program = bin.join(name);
        fs::write(
            &program,
            format!("#!/bin/sh\necho \"$1\" >> '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["a.md", "open", "--edit"])
        .current_dir(scratch.path())
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("VISUAL", bin.join("fake-visual"))
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("opened a.md"), "{}", out(&output));
    let started = std::time::Instant::now();
    while !opened.exists() && started.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(opened.exists(), "the system's opener started");
    assert!(!visual.exists(), "$VISUAL is not read");
}

#[test]
fn show_rc_verbose_says_where_each_value_comes_from() {
    let scratch = Scratch::new("show-rc-merged");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"root.py\"\n[property.radius]\nprecision = \".3f\"\nunit = \"SG\"\n",
    );
    scratch.write(
        "proj/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[property.radius]\nprecision = \".4f\"\n",
    );
    let output = at(&scratch.path().join("proj"), &["--show-rc", "-v", "."]);
    assert_eq!(code(&output), 0, "{}", err_slashed(&output));
    let said = out_slashed(&output);
    let line = |key: &str| {
        said.lines()
            .find(|line| line.starts_with(key))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        line("property.radius.precision").contains("\".4f\""),
        "{said}"
    );
    assert!(
        line("property.radius.precision").ends_with("proj/.samplekitrc"),
        "{said}"
    );
    assert!(line("property.radius.unit").contains("\"SG\""), "{said}");
    assert!(!line("property.radius.unit").contains("proj"), "{said}");
    // The model's path is never imported.
    assert!(line("model.path").is_empty(), "{said}");
    // Without -v, no merged listing.
    let plain = out_slashed(&at(&scratch.path().join("proj"), &["--show-rc", "."]));
    assert!(!plain.contains("property.radius"), "{plain}");
}

#[test]
fn what_a_write_would_change_besides_is_noted_and_said() {
    // A `null`, a YAML tag, an impossible date, an uncertainty beside `n/a`:
    // each changed a file at its next write, and nothing said so.
    let scratch = Scratch::new("changed-besides");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let file = scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\npoured: 2026-02-30\nproperties:\n  notch: {v: n/a, u: 0.1}\n\
         x: null\n---\n",
    );
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nthing: !custom 3\n---\n",
    );
    let validated = at(scratch.path(), &[".", "validate"]);
    let said = out(&validated);
    for note in [
        "poured: 2026-02-30 is no date",
        "notch is n/a beside an uncertainty",
        "x is null: the next write removes it",
        "thing: the YAML tag !custom is not something SampleKit reads",
    ] {
        assert!(said.contains(note), "{note}\n{said}");
    }
    let previewed = at(scratch.path(), &["a.md", "tag", "add", "inspected"]);
    let said = out(&previewed);
    assert!(said.contains("what else this write changes"), "{said}");
    assert!(
        said.contains("a.md: x is null: this write removes it"),
        "{said}"
    );
    let written = at(
        scratch.path(),
        &["a.md", "tag", "add", "inspected", "--write"],
    );
    assert!(
        out(&written).contains("a.md: x is null: removed"),
        "{}",
        out(&written)
    );
    assert!(!fs::read_to_string(&file).unwrap().contains("null"));
}

#[test]
fn a_bare_number_is_set_in_the_declared_unit() {
    // The project declares litres, the file holds 22000 mL ± 288.7: `set
    // volume=23` wrote 23 mL beside the old uncertainty, said without a unit.
    let scratch = Scratch::new("declared-unit-set");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\n",
    );
    let file = scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  volume: {v: 22000, u: 288.7, unit: mL}\n---\n",
    );
    let previewed = at(scratch.path(), &["a.md", "set", "volume=23"]);
    let said = out(&previewed);
    assert!(
        said.contains("in L, the unit the project declares — the file wrote mL")
            && said.contains("its uncertainty, 288.7 mL, is removed"),
        "{said}"
    );
    let written = at(scratch.path(), &["a.md", "set", "volume=23", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("volume: {v: 23, unit: L}"),
        "{}",
        fs::read_to_string(&file).unwrap()
    );
}

#[test]
fn a_filter_or_a_sort_over_two_units_is_refused() {
    // `-f "volume > 20"` over 20 L and 21000 mL kept the second, compared with
    // 20 of no unit; a sort ordered the numbers as they stood.
    let scratch = Scratch::new("two-units");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  volume: {v: 20, unit: L}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  volume: {v: 21000, unit: mL}\n---\n",
    );
    for asked in [
        &["-f", "volume > 20"][..],
        &["-s", "volume", "-c", "name"][..],
    ] {
        let output = at(scratch.path(), asked);
        assert_ne!(code(&output), 0, "{asked:?}: {}", out(&output));
        assert!(
            err(&output).contains("'volume' is in L and mL"),
            "{}",
            err(&output)
        );
    }
    let present = at(scratch.path(), &["-f", "volume is present", "-c", "name"]);
    assert_eq!(code(&present), 0, "{}", err(&present));
}

#[test]
fn a_text_format_is_written_by_name_or_by_extension() {
    // `--style NAME -o -` writes a table as a style writing documents says
    // it; `-o t.tex` chooses the given `latex` by its extension.
    let scratch = Scratch::new("text-format");
    scratch.corpus();
    let latex = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--style", "latex", "-o", "-"],
    );
    assert_eq!(code(&latex), 0, "{}", err(&latex));
    let text = out(&latex);
    assert!(
        text.starts_with("\\begin{tabular}{lr}\n\\toprule\n"),
        "{text}"
    );
    assert!(
        text.contains("name & diameter (g) \\\\\n\\midrule\nbillet-01 & $12.10 \\pm 0.05$ \\\\\n"),
        "{text}"
    );
    assert!(text.ends_with("\\bottomrule\n\\end{tabular}\n"), "{text}");
    let markdown = out(&at(
        scratch.path(),
        &[".", "-c", "name", "--style", "markdown", "-o", "-"],
    ));
    assert!(
        markdown.starts_with("| name |\n| --- |\n| billet-01 |\n"),
        "{markdown}"
    );
    let written = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "-o", "out/t.tex", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert_eq!(
        fs::read_to_string(scratch.path().join("out/t.tex")).unwrap(),
        text
    );
    let unknown = at(
        scratch.path(),
        &[".", "-c", "name", "--style", "latx", "-o", "-"],
    );
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(err(&unknown).contains("latex"), "{}", err(&unknown));
    // `--format` is no option.
    let gone = at(scratch.path(), &[".", "-c", "name", "--format", "latex"]);
    assert_eq!(code(&gone), 1, "{}", err(&gone));
}

#[test]
fn a_figure_of_two_units_on_one_axis_is_refused() {
    // Points in grams and kilograms drawn on one scale compare nothing: an
    // axis is refused as a table's column is, in one project or across two.
    let scratch = Scratch::new("figure-two-units");
    scratch.corpus();
    let kilograms = fs::read_to_string(scratch.path().join("billet-01.sample.md"))
        .unwrap()
        .replace("unit: g}", "unit: kg}");
    scratch.write("billet-01.sample.md", &kilograms);
    let plot = at(
        scratch.path(),
        &[".", "plot", "-x", "porosity", "-y", "diameter"],
    );
    assert_eq!(code(&plot), 2, "{}", err(&plot));
    assert!(
        err(&plot).contains("an axis cannot say which unit it is in: diameter is in"),
        "{}",
        err(&plot)
    );
    // Two projects, one declaring the unit its files leave out.
    let other = Scratch::new("figure-two-units-other");
    other.write(
        ".samplekitrc",
        "schema_version = 1\n[property.diameter]\nunit = \"kg\"\n",
    );
    other.write(
        "a.sample.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 1.5}\n  porosity: {v: 3.0, unit: g/L}\n---\n",
    );
    let first = scratch.path().join("billet-02.sample.md");
    let second = other.path().join("a.sample.md");
    let across = at(
        scratch.path(),
        &[
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "plot",
            "-x",
            "porosity",
            "-y",
            "diameter",
        ],
    );
    assert_eq!(code(&across), 2, "{}", err(&across));
    assert!(
        err(&across).contains("diameter is in g and kg"),
        "{}",
        err(&across)
    );
}

/// Three projects importing one above them, as a platform shares its
/// declarations: `SG` for `radius`, a profile sorted by `-radius`, a boxed table.
fn platform(scratch: &Scratch) {
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[render]\ntable = \"boxed\"\n[style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n\
         [property.radius]\nunit = \"SG\"\nprecision = \".3f\"\n\
         [profile.overview]\ncolumns = [{field = \"name\"}, {field = \"radius\"}]\nsort = [\"-radius\"]\n",
    );
    for project in ["alpha", "beta"] {
        scratch.write(
            &format!("{project}/.samplekitrc"),
            "schema_version = 1\nimport = \"..\"\n",
        );
    }
    scratch.write(
        "gamma/.samplekitrc",
        "schema_version = 1\n[profile.overview]\ncolumns = [{field = \"radius\"}, {field = \"name\"}]\n",
    );
    for project in ["alpha", "beta", "gamma"] {
        for (at, radius) in [(1, 1.057), (2, 1.064), (3, 1.071)] {
            scratch.write(
                &format!("{project}/s{at}.md"),
                &format!(
                    "---\nschema_version: 1\nname: s{at}\nproperties:\n  radius: {{v: {radius}, u: 0.001}}\n---\n"
                ),
            );
        }
    }
}

#[test]
fn several_projects_keep_each_ones_declarations() {
    // Over several projects, what each declares is its own, whatever order
    // they were named in: the unit all declare heads a data file, a profile
    // sorts each project's table by its own order, a summary takes what they
    // agree on, and a style one declares is looked up in each.
    let scratch = Scratch::new("platform-projects");
    platform(&scratch);
    let csv = out(&at(
        scratch.path(),
        &["alpha", "beta", "-c", "radius", "--csv"],
    ));
    assert!(
        csv.starts_with("radius_value [SG],radius_uncertainty [SG]\n"),
        "{csv}"
    );
    let json = out(&at(
        scratch.path(),
        &["alpha", "beta", "-c", "radius", "--json"],
    ));
    assert!(json.contains("\"unit\": \"SG\""), "{json}");
    // gamma's profile sorts nothing: its samples in their own order.
    let tables = out(&at(
        scratch.path(),
        &["alpha", "gamma", "--profile", "overview"],
    ));
    let gamma = tables.split("gamma").nth(1).unwrap_or_default().to_string();
    let first = |name: &str| gamma.find(name).unwrap_or(usize::MAX);
    assert!(
        first("s1") < first("s2") && first("s2") < first("s3"),
        "{tables}"
    );
    // gamma declares no unit, no precision and no boxed table: a summary of
    // both says none, in either order.
    let one = out(&at(
        scratch.path(),
        &["alpha", "gamma", "-c", "radius", "--summary"],
    ));
    let other = out(&at(
        scratch.path(),
        &["gamma", "alpha", "-c", "radius", "--summary"],
    ));
    assert_eq!(one, other);
    assert!(!one.contains("[SG]") && !one.contains('┌'), "{one}");
    // gamma declares no style `math`: said, and its samples in its own.
    let styled = at(
        scratch.path(),
        &["alpha", "gamma", "-c", "radius", "--style", "math"],
    );
    assert_eq!(code(&styled), 0, "{}", err(&styled));
    assert!(
        err(&styled).contains("declares no style 'math'"),
        "{}",
        err(&styled)
    );
    assert!(
        out(&styled).contains("\\pm") && out(&styled).contains('±'),
        "{}",
        out(&styled)
    );
}

#[test]
fn a_declared_export_writes_paths_from_its_project() {
    // `path = true` wrote `gamma/s3.md`, `./s3.md` or an absolute path,
    // depending on the folder the export ran from.
    let scratch = Scratch::new("export-paths");
    platform(&scratch);
    scratch.write(
        "alpha/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[export.j]\nprofile = \"overview\"\n\
         format = \"json\"\noutput = \"out/j.json\"\npath = true\n",
    );
    let above = at(scratch.path(), &["alpha", "export", "j", "--write"]);
    assert_eq!(code(&above), 0, "{}", err(&above));
    let written = fs::read_to_string(scratch.path().join("alpha/out/j.json")).unwrap();
    assert!(written.contains("\"path\": \"s1.md\""), "{written}");
    let within = at(&scratch.path().join("alpha"), &["export", "j", "--write"]);
    assert_eq!(code(&within), 0, "{}", err(&within));
    assert_eq!(
        fs::read_to_string(scratch.path().join("alpha/out/j.json")).unwrap(),
        written
    );
}

#[test]
fn validate_judges_a_configuration_from_its_own_folder() {
    // An imported `[model] python` was looked for from the folder the command
    // ran in; an attribute declared a unit, and `{project}` naming a hidden
    // file, passed; two projects' `s1` were one sample.
    let scratch = Scratch::new("validate-own-folder");
    platform(&scratch);
    fs::create_dir_all(scratch.path().join("env/bin")).unwrap();
    scratch.write("env/bin/python", "");
    let root = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!("{root}[model]\npython = \"env/bin/python\"\n"),
    );
    scratch.write(
        "alpha/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[property.style]\nunit = \"kg\"\n\
         [export.t]\nprofile = \"overview\"\nformat = \"tsv\"\noutput = \"{project}.tsv\"\n",
    );
    scratch.write(
        "alpha/s1.md",
        "---\nschema_version: 1\nname: s1\nstyle: titanium\nproperties:\n  radius: {v: 1.057, u: 0.001}\n---\n",
    );
    let checked = at(scratch.path(), &["alpha", "validate"]);
    let said = out_slashed(&checked);
    assert!(!said.contains("does not exist"), "{said}");
    assert!(
        said.contains("[property.style]: style is an attribute in 1 sample"),
        "{said}"
    );
    assert!(
        said.contains("it writes ..tsv, a file a listing hides"),
        "{said}"
    );
    let table = out_slashed(&at(scratch.path(), &["alpha", "-c", "name,style", "--csv"]));
    assert!(table.starts_with("name,style\n"), "{table}");
    // Two files a canonical pass would rewrite, one `s2` in each project.
    for project in ["alpha", "beta"] {
        scratch.write(
            &format!("{project}/s2.md"),
            "---\nschema_version: 1\nproperties:\n  radius: {v: 1.064, u: 0.001}\nname: s2\n---\n",
        );
    }
    let both = out_slashed(&at(scratch.path(), &["alpha", "validate", "beta"]));
    assert!(both.contains("6 samples"), "{both}");
    assert!(
        both.contains("— 2 samples: alpha/s2.md, beta/s2.md"),
        "{both}"
    );
}

#[test]
fn a_figure_without_a_python_says_what_draws_it() {
    // A project of figures and no model was told *the model cannot run*.
    let scratch = Scratch::new("figure-python");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n",
    );
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  a: {v: 1}\n  b: {v: 2}\n---\n",
    );
    let drawn = at(scratch.path(), &["plot", "f", "-o", "f.png", "--write"]);
    assert_eq!(code(&drawn), 3, "{}", err(&drawn));
    assert!(
        err(&drawn).contains("no Python is declared to draw figures"),
        "{}",
        err(&drawn)
    );
    assert!(
        !err(&drawn).contains("the model cannot run"),
        "{}",
        err(&drawn)
    );
    let shown = out(&at(scratch.path(), &["--show-rc"]));
    assert!(shown.contains("interpreter    none declared"), "{shown}");
}

#[test]
fn a_pattern_excluding_every_sample_is_said() {
    // `exclude = ["*"]` listed each `.samplekitrc` as a sample left out, and
    // `validate` read none without a word.
    let scratch = Scratch::new("exclude-everything");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nexclude = [\"*\"]\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nname: a\n---\n");
    let skipped = out(&at(scratch.path(), &[".", "list", "skipped"]));
    assert!(skipped.contains("a.md"), "{skipped}");
    assert!(!skipped.contains(".samplekitrc"), "{skipped}");
    let checked = out(&at(scratch.path(), &["validate"]));
    assert!(
        checked.contains("[collection] exclude: '*' excludes every sample — 1 file left out"),
        "{checked}"
    );
}

#[test]
fn a_write_under_a_configuration_that_does_not_read_is_said_and_kept() {
    // `set --write` wrote in silence, exited 0 and kept no snapshot: the
    // change was then taken for an edit made outside SampleKit.
    let scratch = Scratch::new("write-unread-configuration");
    scratch.corpus();
    let tagged = at(scratch.path(), &[".", "tag", "add", "t", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let rc = scratch.path().join(".samplekitrc");
    let good = fs::read_to_string(&rc).unwrap();
    fs::write(&rc, format!("{good}bogus = 1\n")).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 2, "{}", err(&set));
    assert!(
        err(&set).contains("configuration not read"),
        "{}",
        err(&set)
    );
    fs::write(&rc, good).unwrap();
    let log = out(&at(scratch.path(), &["log"]));
    assert!(
        log.contains("samplekit billet-01.sample.md set diameter=13 --write"),
        "{log}"
    );
}

#[test]
fn a_sample_that_no_longer_reads_is_kept_as_it_is() {
    // Its YAML broken, or a merge's conflict markers left in it, a sample was
    // left out of every snapshot: `diff` said it removed, and a restore would
    // have replaced the edit, kept nowhere.
    let scratch = Scratch::new("history-broken-sample");
    scratch.corpus();
    let tagged = at(
        scratch.path(),
        &["billet-01.sample.md", "tag", "add", "t", "--write"],
    );
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let path = scratch.path().join("billet-02.sample.md");
    let good = fs::read_to_string(&path).unwrap();
    let broken = good.replace("status: approved", "status: [approved");
    fs::write(&path, &broken).unwrap();
    let again = at(
        scratch.path(),
        &["billet-01.sample.md", "tag", "add", "u", "--write"],
    );
    assert_eq!(code(&again), 0, "{}", err(&again));
    let diff = out(&at(scratch.path(), &["diff", "--from", "3", "--to", "2"]));
    assert!(diff.contains("billet-02.sample.md   changed"), "{diff}");
    assert!(!diff.contains("removed"), "{diff}");
    // Kept, it can be put back as it was before the edit broke it.
    let restored = at(
        scratch.path(),
        &["billet-02.sample.md", "restore", "--at", "3", "--write"],
    );
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert_eq!(fs::read_to_string(&path).unwrap(), good);
    // Conflict markers above the frontmatter are a sample too.
    fs::write(
        &path,
        format!("<<<<<<< HEAD\n{good}=======\n{good}>>>>>>> theirs\n"),
    )
    .unwrap();
    let kept = at(
        scratch.path(),
        &["billet-01.sample.md", "tag", "add", "v", "--write"],
    );
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let diff = out(&at(scratch.path(), &["diff", "--from", "3", "--to", "2"]));
    assert!(!diff.contains("removed"), "{diff}");
}

#[test]
fn a_file_written_at_a_snapshot_may_replace_one_in_another_project() {
    // `--at 1 -o ../other/t.csv --write` a second time, the file there, took
    // the other project — without a history — for the one read, and refused.
    let scratch = Scratch::new("at-into-another-project");
    scratch.corpus();
    scratch.write("other/.samplekitrc", "schema_version = 1\n");
    let project = scratch.path().join("mine");
    fs::create_dir_all(&project).unwrap();
    for entry in fs::read_dir(scratch.path()).unwrap().flatten() {
        let name = entry.file_name();
        if entry.path().is_file() {
            fs::rename(entry.path(), project.join(&name)).unwrap();
        }
    }
    let tagged = at(&project, &[".", "tag", "add", "t", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    for _ in 0..2 {
        let written = at(
            &project,
            &[
                "export",
                "ductilities",
                "--at",
                "1",
                "-o",
                "../other/t.csv",
                "--write",
            ],
        );
        assert_eq!(code(&written), 0, "{}", err(&written));
    }
    assert!(scratch.path().join("other/t.csv").is_file());
}

#[test]
fn a_copy_of_a_stale_export_is_said_stale_though_made_again_at_a_snapshot() {
    // The same table made again with `--at`, a copy of it — the paper's — was
    // said *made from the project as it was then, on purpose*, and its
    // staleness went unsaid.
    let scratch = Scratch::new("explain-copy-remade");
    scratch.corpus();
    let exported = at(scratch.path(), &["export", "ductilities", "--write"]);
    assert_eq!(code(&exported), 0, "{}", err(&exported));
    let table = fs::read_to_string(scratch.path().join("out/ductilities.csv")).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let again = at(
        scratch.path(),
        &[
            "export",
            "ductilities",
            "--at",
            "2",
            "-o",
            "again.csv",
            "--write",
        ],
    );
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert_eq!(
        fs::read_to_string(scratch.path().join("again.csv")).unwrap(),
        table
    );
    scratch.write("paper/table.csv", &table);
    let explained = out(&at(scratch.path(), &["paper/table.csv", "explain"]));
    assert!(explained.contains("it is not current"), "{explained}");
    assert!(!explained.contains("on purpose"), "{explained}");
}

#[test]
fn a_join_whose_files_are_the_other_machines_state_is_that_state() {
    // After a pull that brought the other machine's model and samples, the
    // join set the model pulled over this machine's own samples — a state no
    // disk held — and called the rest a change made outside SampleKit.
    let scratch = Scratch::new("join-as-kept");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write("model.py", "# the model, first\n");
    scratch.write("s.md", "---\nschema_version: 1\nname: s\n---\n");
    let root = scratch.path();
    let first = as_machine(root, "ana", &["s.md", "tag", "add", "t0", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let s0 = fs::read_to_string(root.join("s.md")).unwrap();
    // Tom changes the model and the sample.
    scratch.write("model.py", "# the model, as tom changed it\n");
    let toms = as_machine(root, "tom", &["s.md", "tag", "add", "t1", "--write"]);
    assert_eq!(code(&toms), 0, "{}", err(&toms));
    // Ana, who never saw Tom's work, adds a sample over her own state.
    scratch.write("model.py", "# the model, first\n");
    scratch.write("s.md", &s0);
    scratch.write("u.md", "---\nschema_version: 1\nname: u\n---\n");
    let anas = as_machine(root, "ana", &["u.md", "tag", "add", "t2", "--write"]);
    assert_eq!(code(&anas), 0, "{}", err(&anas));
    // Tom pulls Ana's state whole, and writes.
    let pulled = as_machine(root, "tom", &["u.md", "tag", "add", "t3", "--write"]);
    assert_eq!(code(&pulled), 0, "{}", err(&pulled));
    let log = out(&as_machine(root, "tom", &["log"]));
    assert!(log.contains("the project as ana kept it"), "{log}");
    assert!(!log.contains("the model changed"), "{log}");
    // Ana's last snapshot is the join's state.
    let anas_number = log
        .lines()
        .find(|line| line.contains("tag add t2"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_default()
        .to_string();
    let diff = out(&as_machine(
        root,
        "tom",
        &["diff", "--from", &anas_number, "--to", "2"],
    ));
    assert!(
        diff.contains("nothing changed between them"),
        "{log}\n{diff}"
    );
}

#[test]
fn the_example_project_does_what_its_configuration_says() {
    // Its comments promised `--style math`, which no `[style.math]` declared;
    // its profile sorted by a field its own new samples did not hold.
    let scratch = Scratch::new("example-promises");
    let made = at(
        scratch.path(),
        &["init", "--example", "--no-venv", "--write"],
    );
    assert_eq!(code(&made), 0, "{}", err_slashed(&made));
    let styled = at(
        scratch.path(),
        &["samples", "-c", "tensile_strength", "--style", "math"],
    );
    assert_eq!(code(&styled), 0, "{}", err_slashed(&styled));
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(rc.contains("[style.math]"), "{rc}");
    assert!(rc.contains("sort = [\"-tensile_strength\"]"), "{rc}");
    // Set up already, it names the folder the samples are in.
    scratch.write("specimens/a.md", "---\nschema_version: 1\nname: a\n---\n");
    fs::remove_dir_all(scratch.path().join("samples")).unwrap();
    let again = out_slashed(&at(scratch.path(), &["init", "--no-venv"]));
    assert!(again.contains("samplekit specimens/ status"), "{again}");
    assert!(!again.contains("samplekit samples/ status"), "{again}");
}

#[test]
fn a_project_without_a_model_is_not_asked_an_environment_first() {
    let scratch = Scratch::new("no-model-no-venv");
    let made = out(&at(
        scratch.path(),
        &["init", "--no-model", "--no-venv", "--write"],
    ));
    assert!(!made.contains("an environment with samplekit"), "{made}");
    assert!(!made.contains("then, in this order"), "{made}");
}

#[test]
#[cfg(unix)]
fn what_a_write_cannot_do_is_said_in_samplekits_words() {
    // An export's lock exited 3 without its name; `-o x.md.lock` made a lock
    // nothing takes back; a refusal to replace a sample said the system
    // refused it; a machine's name written otherwise said nothing; a pipe
    // named `.md` stopped every command; `-c diameter,diameter.u` sent the reader to a
    // profile; a number too small, too large or too wide was said badly.
    let scratch = Scratch::new("adversary-small");
    scratch.corpus();
    fs::write(scratch.path().join("out/ductilities.csv.lock"), "").unwrap();
    let locked = at(scratch.path(), &["export", "ductilities", "--write"]);
    assert_eq!(code(&locked), 2, "{}", err(&locked));
    assert!(
        err(&locked).contains("ductilities.csv.lock"),
        "{}",
        err(&locked)
    );
    fs::remove_file(scratch.path().join("out/ductilities.csv.lock")).unwrap();
    let lock = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name",
            "--csv",
            "-o",
            "billet-01.sample.md.lock",
            "--write",
        ],
    );
    assert_eq!(code(&lock), 1, "{}", err(&lock));
    assert!(!scratch.path().join("billet-01.sample.md.lock").exists());
    let sample = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name",
            "--csv",
            "-o",
            "billet-02.sample.md",
            "--write",
        ],
    );
    assert_eq!(code(&sample), 1, "{}", err(&sample));
    let named = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["billet-01.sample.md", "tag", "add", "t", "--write"])
        .current_dir(scratch.path())
        .env("SAMPLEKIT_MACHINE", "a/b")
        .output()
        .unwrap();
    assert!(
        err(&named).contains("SAMPLEKIT_MACHINE='a/b' is written 'a-b'"),
        "{}",
        err(&named)
    );
    let fifo = scratch.path().join("pipe.sample.md");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let listed = at(scratch.path(), &[".", "-c", "name"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    fs::remove_file(&fifo).unwrap();
    let doubled = at(
        scratch.path(),
        &[".", "-c", "name,diameter,diameter.u", "--csv"],
    );
    assert!(
        err(&doubled).contains("'diameter' writes its uncertainty already"),
        "{}",
        err(&doubled)
    );
    scratch.write(
        "tiny.sample.md",
        "---\nschema_version: 1\nname: tiny\nproperties:\n  diameter: {v: 1e-400}\n---\n",
    );
    scratch.write(
        "huge.sample.md",
        "---\nschema_version: 1\nname: huge\nproperties:\n  diameter: 1e999\n---\n",
    );
    scratch.write(
        "wide.sample.md",
        "---\nschema_version: 1\nname: wide\nproperties:\n  diameter: 12345678901234567890123\n---\n",
    );
    let checked = out(&at(scratch.path(), &["validate"]));
    assert!(
        checked.contains("1e-400 is smaller than any number a file holds"),
        "{checked}"
    );
    assert!(checked.contains("a measurement is finite"), "{checked}");
    assert!(checked.contains("too large a whole number"), "{checked}");
}

#[test]
fn a_preview_says_what_it_makes_not_current_and_keeps_an_uncertainty() {
    // `--add-row` and `restore` said the numbers and not what they made
    // outdated; `diameter=12.5±0.1` was advised `diameter=12.5`, the uncertainty lost.
    let scratch = Scratch::new("previews-not-current");
    pieces(&scratch);
    let typed = at(
        scratch.path(),
        &["gamma-zinc.md", "set", "radius=1.066±0.001"],
    );
    assert!(
        err(&typed).contains("radius=1.066 radius.u=0.001"),
        "{}",
        err(&typed)
    );
}

#[test]
fn set_refuses_a_name_another_sample_holds() {
    // Written, two samples held one name, and `validate` then found it.
    let scratch = Scratch::new("rename-taken");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for name in ["A", "B"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\n---\n"),
        );
    }
    let refused = at(scratch.path(), &["A.md", "set", "name=B", "--write"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).contains("B is already a sample's name, in"),
        "{}",
        err(&refused)
    );
    assert!(
        fs::read_to_string(scratch.path().join("A.md"))
            .unwrap()
            .contains("name: A")
    );
    // Its own name again, or another, is written.
    let renamed = at(scratch.path(), &["A.md", "set", "name=C", "--write"]);
    assert_eq!(code(&renamed), 0, "{}", err(&renamed));
}

#[test]
fn a_groups_heading_reads_in_the_declared_precision() {
    // `radius = 1.0539999999999998` headed a group whose cells read `1.054`.
    let scratch = Scratch::new("group-precision");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.radius]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "A.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  radius: {v: 1.0539999999999998}\n---\n",
    );
    let table = at(
        scratch.path(),
        &[".", "-c", "name,radius", "--group", "radius"],
    );
    assert_eq!(code(&table), 0, "{}", err(&table));
    assert!(out(&table).contains("radius = 1.054"), "{}", out(&table));
    assert!(!out(&table).contains("1.05399"), "{}", out(&table));
}

#[test]
fn explain_says_readings_stored_without_a_statistic() {
    // *nothing is stored here* was said of a file holding two readings.
    let scratch = Scratch::new("explain-readings-alone");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "w.md",
        "---\nschema_version: 1\nname: w\nproperties:\n  ore: {readings: [4.1, 4.2], unit: kg}\n---\n",
    );
    let explained = at(scratch.path(), &["w.md", "explain", "ore"]);
    assert!(
        out(&explained)
            .contains("2 readings are stored here, and no statistic of them is recorded"),
        "{}",
        out(&explained)
    );
    assert!(!out(&explained).contains("nothing is stored here"));
}

#[test]
fn a_name_two_projects_hold_is_told_apart_by_its_path() {
    // `alpha-titanium (alpha-titanium.md)` twice over two projects told them apart by
    // nothing.
    let scratch = Scratch::new("status-two-projects");
    for project in ["one", "two"] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        scratch.write(
            &format!("{project}/s.md"),
            "---\nschema_version: 1\nname: s\nproperties:\n  a: {v: 1.0, fingerprint: '111111111111'}\n  \
             b: {v: 2.0, computed: {a: '222222222222'}, fingerprint: '333333333333'}\n---\n",
        );
    }
    let status = at(scratch.path(), &["one", "status", "two"]);
    let said = out_slashed(&status);
    assert!(said.contains("s (one/s.md)"), "{said}");
    assert!(said.contains("s (two/s.md)"), "{said}");
}

#[test]
fn a_group_whose_samples_all_say_not_applicable_is_counted_alike_everywhere() {
    // Left out as *holding text*, and `0/1` on a screen where a file said `0`
    // of `0`: the samples saying n/a are counted apart on every surface.
    let scratch = Scratch::new("summary-all-na");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for (name, group, y) in [("s1", "a", "2.0"), ("s2", "a", "2.0"), ("s3", "b", "n/a")] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\ngrp: {group}\nproperties:\n  y: {{v: {y}}}\n---\n"
            ),
        );
    }
    let table = at(
        scratch.path(),
        &[".", "-c", "y", "--summary", "--group", "grp"],
    );
    assert!(!err(&table).contains("holds text"), "{}", err(&table));
    assert!(out(&table).contains("0/0 (1 n/a)"), "{}", out(&table));
    let csv = out(&at(
        scratch.path(),
        &[".", "-c", "y", "--summary", "--group", "grp", "--csv"],
    ));
    assert!(
        csv.lines().any(|line| line.starts_with("b,y,,0,0,1,")),
        "{csv}"
    );
}

#[test]
fn json_keys_a_channel_as_it_is_named() {
    // `radius.u` became `radius_uncertainty`, the CSV header, beside `radius.readings` and
    // a quantity keyed `radius`.
    let scratch = Scratch::new("json-channel-key");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "s.md",
        "---\nschema_version: 1\nname: s\nproperties:\n  radius: {v: 1.05, u: 0.001}\n---\n",
    );
    let json = out(&at(
        scratch.path(),
        &["s.md", "-c", "name,radius.readings,radius.u", "--json"],
    ));
    assert!(json.contains("\"radius.u\": 0.001"), "{json}");
    assert!(!json.contains("radius_uncertainty"), "{json}");
    let csv = out(&at(
        scratch.path(),
        &["s.md", "-c", "name,radius.u", "--csv"],
    ));
    assert!(csv.starts_with("name,radius_uncertainty"), "{csv}");
}

#[test]
fn validate_says_a_model_with_no_python_to_run_it() {
    // `compute` stopped at it, exit 3, where `validate` said nothing: nothing
    // is searched for, a `.venv/` beside the project included.
    let scratch = Scratch::new("validate-no-python");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\nclass Piece(sk.Sample):\n    pass\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nstyle: nickel\n---\n");
    // A note: a script of the user's runs the model in its own Python.
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 0, "{}", out(&checked));
    assert!(
        out(&checked).contains("no python for the command line to run it in"),
        "{}",
        out(&checked)
    );
}

#[test]
fn a_profiles_unknown_column_names_the_profile_and_its_file() {
    // `unknown field 'attenuaton'` named neither the profile nor .samplekitrc,
    // where nothing on the command line wrote it.
    let scratch = Scratch::new("profile-unknown-column");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.carnet]\ncolumns = [{field = \"name\"}, {field = \"stlye\"}]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nstyle: nickel\n---\n",
    );
    let shown = at(scratch.path(), &["--profile", "carnet"]);
    assert_ne!(code(&shown), 0);
    assert!(
        err(&shown).contains("profile 'carnet' in")
            && err(&shown).contains(".samplekitrc shows 'stlye'"),
        "{}",
        err(&shown)
    );
}

#[test]
fn explain_says_an_export_written_from_values_not_current() {
    // Written over an edited porosity, then explained: *it is up to date*, since
    // nothing changed after it was written.
    let scratch = Scratch::new("explain-export-behind");
    scratch.corpus();
    scratch.write(
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nstatus: approved\nproperties:\n  \
         diameter: {v: 12.1, u: 0.05, unit: g}\n  \
         porosity: {v: 2.801, unit: g/L, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    let accepted = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "--accept",
            "edited",
            "--write",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    let explained = at(scratch.path(), &["out/ductilities.csv", "explain"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    let said = out(&explained);
    assert!(
        said.contains("written from values not current: porosity"),
        "{said}"
    );
    assert!(!said.contains("it is up to date"), "{said}");
}

#[test]
fn explain_leaves_an_export_current_over_a_configuration_change_it_does_not_read() {
    // Another profile declared made every export *not current*, the file
    // made again byte for byte the same.
    let scratch = Scratch::new("explain-export-unread");
    scratch.corpus();
    let written = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "--accept",
            "outdated,never-computed",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let rc = scratch.path().join(".samplekitrc");
    let declared = fs::read_to_string(&rc).unwrap();
    fs::write(
        &rc,
        format!("{declared}\n[profile.unrelated]\ncolumns = [{{field = \"name\"}}]\n"),
    )
    .unwrap();
    let explained = at(scratch.path(), &["out/ductilities.csv", "explain"]);
    let said = out(&explained);
    assert!(
        said.contains(".samplekitrc  changed, in nothing it reads"),
        "{said}"
    );
    assert!(!said.contains("it is not current"), "{said}");
    // A section every file may read counts.
    fs::write(
        &rc,
        format!("{declared}\n[property.unrelated]\nprecision = \".1f\"\n"),
    )
    .unwrap();
    let explained = at(scratch.path(), &["out/ductilities.csv", "explain"]);
    assert!(
        out(&explained).contains("it is not current"),
        "{}",
        out(&explained)
    );
}

#[test]
fn a_querys_missing_directory_is_said_as_the_querys() {
    // Said as if typed, `gone: no such file or directory`, naming neither the
    // query nor .samplekitrc.
    let scratch = Scratch::new("query-missing-directory");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[query.nowhere]\nfilter = 'style == \"nickel\"'\ndirectory = \"gone\"\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nstyle: nickel\n---\n");
    for command in [
        &["--query", "nowhere"][..],
        &["view", "--query", "nowhere"][..],
    ] {
        let listed = at(scratch.path(), command);
        assert_ne!(code(&listed), 0);
        assert!(
            err(&listed).contains("[query.nowhere] directory names 'gone'"),
            "{command:?}: {}",
            err(&listed)
        );
    }
}

#[test]
fn new_under_a_configuration_not_read_says_so() {
    // `set` said it and exited 2; `new` wrote under it in silence and exited 0.
    let scratch = Scratch::new("new-configuration-unread");
    scratch.write(".samplekitrc", "schema_version = 1\nbogus = 1\n");
    fs::create_dir_all(scratch.path().join("samples")).unwrap();
    let made = at(
        scratch.path(),
        &["new", "z", "a=2", "--into", "samples", "--write"],
    );
    assert_eq!(code(&made), 2, "{}", err(&made));
    assert!(
        err(&made).contains("configuration not read"),
        "{}",
        err(&made)
    );
}

#[test]
fn explain_prints_the_traceback_of_a_cell_that_held_a_value() {
    // A table's formula failing over cells already filled: `explain` said no
    // traceback was kept, where `status` named the log that held it.
    let scratch = Scratch::new("explain-cell-traceback");
    scratch.config();
    scratch.write(
        "f.sample.md",
        "---\nschema_version: 1\nname: billet-01\ntables:\n  ageing:\n    index: day\n    \
         columns:\n      day: {}\n      hardness: {}\n      rate: {}\n    rows:\n      \
         - day: 1\n        hardness: 1.05\n        rate: {v: 2.0, computed: {row.hardness: aaaaaaaaaaaa}, \
         fingerprint: {failed: ValueError}}\n---\nN.\n",
    );
    scratch.write(
        ".samplekit/failures/f.sample.log",
        "## ageing.rate\nTraceback (most recent call last):\nValueError: no rate on day 1\n",
    );
    let explained = at(
        scratch.path(),
        &["f.sample.md", "explain", "ageing.rate[1]"],
    );
    assert!(
        out(&explained).contains("ValueError: no rate on day 1"),
        "{}{}",
        out(&explained),
        err(&explained)
    );
}

#[test]
fn a_sort_key_and_a_filters_field_misspelt_get_advice() {
    // `-c nmae` offered 'name'; `-s nmae` and `-f 'nmae == x'` offered nothing.
    let scratch = Scratch::new("sort-filter-advice");
    scratch.corpus();
    for command in [
        &[".", "-c", "name", "-s", "nmae"][..],
        &[".", "-f", "nmae == \"x\""][..],
    ] {
        let said = at(scratch.path(), command);
        assert_ne!(code(&said), 0);
        assert!(
            err(&said).contains("did you mean: 'name'?"),
            "{command:?}: {}",
            err(&said)
        );
    }
}

#[test]
fn set_says_nothing_of_what_rests_on_a_change_under_a_model_unread() {
    // *nothing derived rests on this* was said where the model was never
    // read, and could not tell.
    let scratch = Scratch::new("set-model-unread");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"nowhere\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\nclass Piece(sk.Sample):\n    pass\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  radius: {v: 1.05}\n---\n",
    );
    let shown = out(&at(scratch.path(), &["a.md", "set", "radius=1.06"]));
    assert!(!shown.contains("nothing derived rests on this"), "{shown}");
    assert!(
        shown.contains("what rests on this is not known here"),
        "{shown}"
    );
}

#[test]
fn a_property_is_written_in_its_styles_form_on_the_terminal() {
    // `--style` gave a table's units alone: a property is written as the
    // style's `property` says, `\SI{12.10 \pm 0.05}{\gram}`.
    let scratch = Scratch::new("style-property");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!(
            "{rc}[style.si]\nproperty = '\\SI{{{{value}} \\pm {{uncertainty}}}}{{{{unit}}}}'\n\
             [unit.g]\nsi = '\\gram'\n"
        ),
    );
    let shown = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "--style", "si"],
    );
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert!(
        out(&shown).contains("\\SI{12.10 \\pm 0.05}{\\gram}"),
        "{}",
        out(&shown)
    );
}

#[test]
fn a_style_escapes_for_its_language_or_by_its_replacements() {
    // HTML and a language SampleKit does not know: escaped by the crate, or
    // by the replacements the style declares.
    let scratch = Scratch::new("style-escape");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    scratch.write(
        ".samplekitrc",
        &format!("{rc}[style.typst]\nrow = \"{{cells}}\"\ncells = \" | \"\nescape = {{ \"-\" = \"\\\\-\" }}\n"),
    );
    let html = at(
        scratch.path(),
        &[".", "-c", "name", "--style", "html", "-o", "-"],
    );
    assert_eq!(code(&html), 0, "{}", err(&html));
    assert!(
        out(&html).contains("<tr><td>billet-01</td></tr>"),
        "{}",
        out(&html)
    );
    let typst = at(
        scratch.path(),
        &[".", "-c", "name", "--style", "typst", "-o", "-"],
    );
    assert_eq!(code(&typst), 0, "{}", err(&typst));
    assert!(out(&typst).contains("billet\\-01"), "{}", out(&typst));
}

/// The binary, run with `input` on its standard input.
fn answering(directory: &Path, arguments: &[&str], input: &str) -> Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(arguments)
        .current_dir(directory)
        .env("COLUMNS", "200")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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

#[test]
fn a_preview_asks_to_apply_and_no_is_the_default() {
    // A preview ended on *pass --write*: it asks, one answer read from the
    // standard input, and nothing but yes writes.
    let scratch = Scratch::new("preview-asks");
    scratch.corpus();
    let file = scratch.path().join("billet-01.sample.md");
    let before = fs::read_to_string(&file).unwrap();
    let silent = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13"],
    );
    assert!(
        err(&silent).contains("apply these changes? [y/N] n"),
        "{}",
        err(&silent)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let no = answering(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13"],
        "\n",
    );
    assert!(err(&no).contains("[y/N] n\n"), "{}", err(&no));
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    let yes = answering(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13"],
        "y\n",
    );
    assert_eq!(code(&yes), 0, "{}", err(&yes));
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("diameter: {v: 13"),
        "{}",
        out(&yes)
    );
    // `--try` is no option: asking replaces it.
    let tried = at(scratch.path(), &[".", "compute", "--try"]);
    assert_eq!(code(&tried), 1, "{}", err(&tried));
}

#[test]
fn completion_after_a_path_completes_the_command() {
    // The path typed first, to be completed from: the command after it is
    // offered, and its arguments completed from the path.
    let scratch = Scratch::new("complete-path-first");
    scratch.corpus();
    let complete = |index: &str, words: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", index)
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        out(&output)
    };
    let commands = complete("2", &["samplekit", ".", "sta"]);
    assert!(commands.contains("status"), "{commands}");
    let exports = complete("3", &["samplekit", ".", "export", ""]);
    assert!(
        exports.lines().any(|line| line == "ductilities"),
        "{exports}"
    );
}

#[test]
#[cfg(unix)]
fn completion_after_a_path_offers_paths_and_names() {
    // Paths are read after the command too: offered there beside the names,
    // a path written first or not.
    let scratch = Scratch::new("complete-second-path");
    scratch.corpus();
    fs::create_dir_all(scratch.path().join("scripts")).unwrap();
    let complete = |index: &str, words: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .arg("--")
            .args(words)
            .current_dir(scratch.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", index)
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        out(&output)
    };
    let kinds = complete("3", &["samplekit", "out/", "list", "scr"]);
    assert!(kinds.lines().any(|line| line == "scripts"), "{kinds}");
    assert!(kinds.contains("scripts/"), "{kinds}");
    let targets = complete("3", &["samplekit", ".", "status", "s"]);
    assert!(targets.contains("scripts/"), "{targets}");
    let paths = complete("2", &["samplekit", "list", "scr"]);
    assert!(paths.contains("scripts/"), "{paths}");
}

#[test]
fn an_unknown_key_is_said_once_with_its_line() {
    // validate said the whole path and every key twice, as a warning and as
    // its defect.
    let scratch = Scratch::new("unknown-style-key");
    fs::write(
        scratch.path().join(".samplekitrc"),
        "schema_version = 1\n[style.math]\nproprety = 'x'\n",
    )
    .unwrap();
    let checked = at(scratch.path(), &["validate"]);
    assert_eq!(code(&checked), 2, "{}", err(&checked));
    let said = format!("{}{}", out(&checked), err(&checked));
    assert!(
        said.contains("not read: line 3: unknown key `proprety` — did you mean `property`?"),
        "{said}"
    );
    assert!(!said.contains("warning:"), "{said}");
    // No list of every key.
    assert!(!said.contains("header_cells"), "{said}");
}

#[test]
fn a_style_writing_documents_shows_its_document_on_the_terminal() {
    // `--style NAME` showed the terminal's table in the style's forms, where
    // `-o -` wrote the document: the terminal shows what the file would hold.
    let scratch = Scratch::new("style-document-terminal");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    fs::write(
        scratch.path().join(".samplekitrc"),
        format!(
            "{rc}\n[style.paper]\nbefore = '\\begin{{tabular}}{{{{align}}}}'\nafter = '\\end{{tabular}}'\n\
             header = '{{cells}} \\\\'\nrow = '{{cells}} \\\\'\ncells = ' & '\n"
        ),
    )
    .unwrap();
    let shown = at(scratch.path(), &["-c", "name,diameter", "--style", "paper"]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    let written = at(
        scratch.path(),
        &["-c", "name,diameter", "--style", "paper", "-o", "-"],
    );
    assert_eq!(out(&shown), out(&written));
    assert!(
        out(&shown).starts_with("\\begin{tabular}"),
        "{}",
        out(&shown)
    );
    // A style of units alone is the terminal's table.
    let plain = at(scratch.path(), &["-c", "name,diameter", "--style", "plain"]);
    assert!(out(&plain).contains('─'), "{}", out(&plain));
}

#[test]
fn the_help_and_the_advice_write_the_path_first() {
    // Every command took its path first, and its help and advice still
    // wrote it after: `samplekit pieces/ status`.
    let scratch = Scratch::new("path-first-help");
    scratch.corpus();
    for (verb, usage) in [
        ("status", "Usage: samplekit [TARGET] status [OPTIONS]"),
        (
            "set",
            "Usage: samplekit [TARGET] set [FIELD=VALUE]... [OPTIONS]",
        ),
        (
            "export",
            "Usage: samplekit [TARGET] export <EXPORT> [OPTIONS]",
        ),
    ] {
        let help = out(&at(scratch.path(), &[verb, "-h"]));
        assert!(help.contains(usage), "{help}");
    }
    let overview = out(&at(scratch.path(), &["-h"]));
    assert!(
        overview.contains("samplekit [TARGET] <COMMAND> [OPTIONS]"),
        "{overview}"
    );
    // The long page says that a command wins over a path of its name.
    let long = out(&at(scratch.path(), &["--help"]));
    assert!(
        long.contains("The first word naming a command is the command"),
        "{long}"
    );
    // A path written first that is not there is said missing, not the order
    // refused.
    let missing = at(scratch.path(), &["nowhere/", "status"]);
    assert!(
        err(&missing).contains("no such file or directory"),
        "{}",
        err(&missing)
    );
    // Advice names the file first too.
    let advice = at(scratch.path(), &["billet-01.sample.md", "explain"]);
    assert!(
        err(&advice).contains("samplekit billet-01.sample.md explain"),
        "{}",
        err(&advice)
    );
}

#[test]
fn a_change_is_said_whole_as_every_surface_says_it() {
    // `set` said `12.1  →  13` in a column of 28, where `diff` and `restore`
    // said the value whole: one change, one line, everywhere.
    let scratch = Scratch::new("change-whole");
    scratch.corpus();
    let preview = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13.25"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(
        said.starts_with("billet-01.sample.md · 1 change\n"),
        "{said}"
    );
    // The number as it is typed and written, then its uncertainty and unit.
    assert!(
        said.contains("\n  diameter   12.10 ± 0.05 g  →  13.25 ± 0.05 g\n"),
        "{said}"
    );
    // Readings changed say those they replace.
    let readings = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter.readings=12,13"],
    );
    assert!(
        out(&readings).contains("diameter.readings   —  →  12, 13"),
        "{}",
        out(&readings)
    );
}

#[test]
fn a_yes_writes_without_the_preview_again() {
    // Answered yes, the command ran again with --write and printed the whole
    // preview a second time before its outcome.
    let scratch = Scratch::new("yes-once");
    scratch.corpus();
    let written = answering(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=13"],
        "y\n",
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let said = out(&written);
    assert_eq!(said.matches("· 1 change").count(), 1, "{said}");
    assert!(
        said.trim_end().ends_with("written: billet-01.sample.md"),
        "{said}"
    );
    let tagged = answering(scratch.path(), &[".", "tag", "add", "gold"], "y\n");
    let said = out(&tagged);
    assert_eq!(said.matches("tag +gold ·").count(), 1, "{said}");
    assert!(
        said.contains("written: 4 files\n  billet-01.sample.md"),
        "{said}"
    );
}

#[test]
fn new_keeps_a_sample_by_its_name_and_takes_a_list() {
    // `--keep <sample>`, as new's own advice writes it, refused the sample's
    // name as no file; and a list set writes was refused as a number.
    let scratch = Scratch::new("new-keep-name");
    scratch.corpus();
    let kept = at(
        scratch.path(),
        &[
            "new",
            "billet-09",
            "--keep",
            "billet-01",
            "status",
            "carbon=[a, b]",
        ],
    );
    assert_eq!(code(&kept), 0, "{}", err(&kept));
    let said = out(&kept);
    assert!(said.starts_with("billet-09.md · new sample"), "{said}");
    assert!(said.contains("taken from billet-01.sample.md"), "{said}");
    assert!(said.contains("carbon   —  →  [a, b]"), "{said}");
    let missing = at(scratch.path(), &["new", "x", "--keep", "nobody", "status"]);
    assert!(
        err(&missing).contains("no sample here is named so"),
        "{}",
        err(&missing)
    );
}

#[test]
fn every_preview_opens_with_what_and_where_and_asks_one_question() {
    // Each command opened its preview its own way — `would add 'x' to`,
    // `ductilities → out/…`, `would restore to` — and asked its own question.
    let scratch = Scratch::new("one-preview");
    scratch.corpus();
    for (arguments, first) in [
        (
            &[".", "tag", "add", "gold"][..],
            "tag +gold · 4 of 4 samples",
        ),
        (
            &[".", "export", "ductilities"][..],
            "export ductilities · out/ductilities.csv · 4 rows",
        ),
        (
            &["billet-01.sample.md", "set", "diameter=13"][..],
            "billet-01.sample.md · 1 change",
        ),
    ] {
        let asked = answering(scratch.path(), arguments, "\n");
        let said = out_slashed(&asked);
        assert!(said.starts_with(first), "{arguments:?}: {said}");
        assert!(
            err_slashed(&asked).contains("apply these changes? [y/N] n"),
            "{}",
            err_slashed(&asked)
        );
        assert!(
            said.trim_end().ends_with("nothing written"),
            "{arguments:?}: {said}"
        );
    }
}

#[test]
fn init_asks_as_every_command_asks() {
    // Off a terminal init said `pass --write` and asked nothing, where every
    // other preview asks: `yes |` answers it.
    let scratch = Scratch::new("init-asks");
    let declined = answering(scratch.path(), &["init"], "\n");
    assert!(
        out(&declined).starts_with("init · here"),
        "{}",
        out(&declined)
    );
    assert!(
        err(&declined).contains("apply these changes? [y/N] n"),
        "{}",
        err(&declined)
    );
    assert!(!scratch.path().join(".samplekitrc").exists());
    let accepted = answering(scratch.path(), &["init", "--no-venv"], "y\n");
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    assert!(scratch.path().join(".samplekitrc").exists());
}

#[test]
fn list_queries_profiles_and_exports_say_what_each_is() {
    // Bare names, where the workbench's pickers said each query's filter and
    // each export's file: listed as figures and scripts are.
    let scratch = Scratch::new("list-what-each-is");
    scratch.corpus();
    let queries = out_slashed(&at(scratch.path(), &["list", "queries"]));
    let line = queries
        .lines()
        .find(|line| line.starts_with("approved"))
        .unwrap_or_else(|| panic!("{queries}"));
    assert!(line.split_whitespace().nth(1).is_some(), "{queries}");
    let exports = out_slashed(&at(scratch.path(), &["list", "exports"]));
    assert!(exports.contains("out/"), "{exports}");
    assert!(exports.contains("· profile "), "{exports}");
}

// ------------------------------------------------- the grammar, path first

#[test]
fn several_paths_before_the_command_are_read() {
    // Two paths before the command, and an option between them and it.
    let scratch = Scratch::new("paths-first");
    let root = two_pieces(&scratch, "p");
    let status = at(&root, &["a.md", "b.md", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(out(&status).contains("in 2 samples"), "{}", out(&status));
    let table = at(&root, &["a.md", "b.md", "-c", "diameter"]);
    assert_eq!(code(&table), 0, "{}", err(&table));
    assert_eq!(out(&table).lines().count(), 4, "{}", out(&table));
    let between = at(&root, &["a.md", "--json", "status"]);
    assert_eq!(code(&between), 0, "{}", err(&between));
    assert_eq!(out(&between).trim(), "[]");
}

#[test]
fn a_word_before_the_command_is_a_path() {
    // A word naming nothing is missing, not told to move; a folder named as
    // a kind of `list` is that folder.
    let scratch = Scratch::new("word-is-a-path");
    let root = two_pieces(&scratch, "p");
    let missing = at(&root, &["piece", "status"]);
    assert_eq!(code(&missing), 3, "{}", err(&missing));
    assert!(
        err(&missing).contains("piece: no such file or directory"),
        "{}",
        err(&missing)
    );
    scratch.write(
        "p/tags/c.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  diameter: 3.0\n---\n",
    );
    let listed = at(&root, &["tags", "list"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(out(&listed).starts_with("1 sample\n"), "{}", out(&listed));
}

#[test]
fn what_samplekit_writes_names_the_path_first() {
    // The history, and the commands SampleKit suggests, in one order.
    let scratch = Scratch::new("written-path-first");
    let root = two_pieces(&scratch, "p");
    let set = at(&root, &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert_eq!(
        history_messages(&root)[0],
        "samplekit a.md set diameter=1.5 --write"
    );
    let preview = at(&root, &["a.md", "set", "diameter=1.6"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = format!("{}{}", out(&preview), err(&preview));
    assert!(!said.contains("samplekit set "), "{said}");
}

#[test]
fn a_snapshot_says_its_command_in_one_form() {
    // However it was typed, one message, so that one search finds it.
    let scratch = Scratch::new("one-form");
    let root = two_pieces(&scratch, "p");
    for (arguments, diameter) in [
        (["a.md", "set", "--write", "diameter=1.6"], "1.6"),
        (["a.md", "set", "diameter=1.7", "--write"], "1.7"),
    ] {
        let output = at(&root, &arguments);
        assert_eq!(code(&output), 0, "{arguments:?}: {}", err(&output));
        assert_eq!(
            history_messages(&root)[0],
            format!("samplekit a.md set diameter={diameter} --write")
        );
    }
}

#[test]
fn a_fresh_clone_shows_whose_snapshots_it_holds() {
    // Another computer's history shows its column on the first `log`.
    let scratch = Scratch::new("whose-snapshots");
    let root = two_pieces(&scratch, "p");
    let written = as_machine(&root, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let theirs = out(&as_machine(&root, "tom", &["log"]));
    assert!(
        theirs
            .lines()
            .next()
            .unwrap_or_default()
            .contains("machine"),
        "{theirs}"
    );
    assert!(theirs.contains("ana"), "{theirs}");
    let mine = out(&as_machine(&root, "ana", &["log"]));
    assert!(!mine.contains("machine"), "{mine}");
}

/// The binary at a pseudoterminal, `keys` typed once the question is shown.
#[cfg(target_os = "linux")]
fn answered_at_terminal(directory: &Path, command: &str, keys: &[u8]) -> (i32, String) {
    use std::io::Read as _;
    let mut child = Command::new("script")
        .args(["--quiet", "--return", "--command", command, "/dev/null"])
        .current_dir(directory)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("script creates a pseudoterminal");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut seen = Vec::new();
    let mut buffer = [0; 256];
    while !String::from_utf8_lossy(&seen).contains("[y/N]") {
        let read = stdout.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        seen.extend_from_slice(&buffer[..read]);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    stdin.write_all(keys).unwrap();
    drop(stdin);
    stdout.read_to_end(&mut seen).unwrap();
    let status = child.wait().unwrap();
    (
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&seen).into_owned(),
    )
}

#[test]
#[cfg(target_os = "linux")]
fn a_question_answered_no_exits_zero_and_ctrl_c_130() {
    let scratch = Scratch::new("question-codes");
    let root = two_pieces(&scratch, "p");
    let before = fs::read_to_string(root.join("a.md")).unwrap();
    let command = format!("{} a.md set diameter=2", env!("CARGO_BIN_EXE_samplekit"));
    let (no, said) = answered_at_terminal(&root, &command, b"n");
    assert_eq!(no, 0, "{said}");
    assert!(said.contains("nothing written"), "{said}");
    let (interrupted, said) = answered_at_terminal(&root, &command, b"\x03");
    assert_eq!(interrupted, 130, "{said}");
    assert!(said.contains("interrupted: nothing written"), "{said}");
    assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), before);
}

#[test]
#[cfg(target_os = "linux")]
fn a_question_closed_on_stderr_never_panics() {
    // The question cannot be shown: it is answered no, and nothing written.
    let scratch = Scratch::new("question-closed");
    let root = two_pieces(&scratch, "p");
    let before = fs::read_to_string(root.join("a.md")).unwrap();
    let command = format!(
        "{} a.md set diameter=2 2>&-",
        env!("CARGO_BIN_EXE_samplekit")
    );
    let mut child = Command::new("script")
        .args(["--quiet", "--return", "--command", &command, "/dev/null"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"y\n").unwrap();
    let closed = child.wait_with_output().unwrap();
    assert_eq!(code(&closed), 0, "{}", out(&closed));
    assert!(!out(&closed).contains("panicked"), "{}", out(&closed));
    assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), before);
}

#[test]
fn a_path_mistyped_is_offered_the_nearest() {
    let scratch = Scratch::new("nearest-path");
    let root = two_pieces(&scratch, "p");
    let missing = at(&root, &["b.mdx", "status"]);
    assert_eq!(code(&missing), 3, "{}", err(&missing));
    assert!(
        err(&missing).contains("did you mean 'b.md'?"),
        "{}",
        err(&missing)
    );
}

/// A sample whose `porosity` its file says outdated, with no model.
fn drifted(scratch: &Scratch, file: &str) {
    scratch.write(
        file,
        "---\nschema_version: 1\nname: drifted\nproperties:\n  \
         diameter: {v: 2.0, unit: g, fingerprint: aaaaaaaaaaaa}\n  \
         porosity: {v: 1.0, computed: {diameter: bbbbbbbbbbbb}}\n---\nN.\n",
    );
}

#[test]
fn status_over_projects_is_one_table_each() {
    let scratch = Scratch::new("status-projects");
    for project in ["p", "q"] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        drifted(&scratch, &format!("{project}/{project}1.md"));
    }
    let output = at(scratch.path(), &["p", "q", "status"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let said = out(&output);
    let headings: Vec<&str> = said
        .lines()
        .filter(|line| *line == "p" || *line == "q")
        .collect();
    assert_eq!(headings, ["p", "q"], "{said}");
    assert!(said.contains("samplekit p q compute"), "{said}");
}

#[test]
fn a_file_written_from_a_failed_value_exits_two_unless_accepted() {
    // A failed value passed `export` and `-o`, exit 0, and failed `plot`; a
    // state had no word of its own to accept it by.
    let scratch = Scratch::new("failed-accepted");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, {field = \"hardness\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"e.csv\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  \
         hardness: {v: 1.02, computed: {radius: aaaaaaaaaaaa}, fingerprint: {failed: ZeroDivisionError}}\n---\n",
    );
    for command in [
        vec![".", "-c", "name,hardness", "-o", "t.csv", "--write"],
        vec![".", "export", "e", "--write"],
    ] {
        let written = at(scratch.path(), &command);
        assert_eq!(code(&written), 2, "{command:?}: {}", err(&written));
        assert!(
            err(&written).contains("--accept failed"),
            "{}",
            err(&written)
        );
        let mut accepted = command.clone();
        accepted.extend(["--accept", "failed"]);
        let accepted = at(scratch.path(), &accepted);
        assert_eq!(code(&accepted), 0, "{command:?}: {}", err(&accepted));
    }
    // One spelling of a state, the others still read.
    let spelled = at(
        scratch.path(),
        &[
            ".",
            "export",
            "e",
            "--write",
            "--accept",
            "failed,never-computed",
        ],
    );
    assert_eq!(code(&spelled), 0, "{}", err(&spelled));
}

#[test]
fn a_table_written_from_values_not_current_says_so() {
    // A file does not show the states the terminal did: said when previewed
    // and once written; `export -o -` says it and fails as a file written.
    let scratch = Scratch::new("table-behind");
    scratch.corpus();
    drifted(&scratch, "drifted.sample.md");
    let previewed = at(scratch.path(), &[".", "-c", "name,porosity", "-o", "t.csv"]);
    assert_eq!(code(&previewed), 0, "{}", err(&previewed));
    assert!(
        out(&previewed)
            .contains("the table is written from values that are not current: 1 outdated"),
        "{}",
        out(&previewed)
    );
    // Written, and a failure all the same, as an export is.
    let written = at(
        scratch.path(),
        &[".", "-c", "name,porosity", "-o", "t.csv", "--write"],
    );
    assert_eq!(code(&written), 2, "{}", err(&written));
    assert!(scratch.path().join("t.csv").is_file());
    // On stdout, as an export says it.
    assert!(
        out(&written).contains("the table is written from values that are not current"),
        "{}",
        out(&written)
    );
    let accepted = at(
        scratch.path(),
        &[
            ".",
            "-c",
            "name,porosity",
            "-o",
            "t.csv",
            "--write",
            "--accept",
            "outdated",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
    let streamed = at(scratch.path(), &[".", "export", "ductilities", "-o", "-"]);
    assert_eq!(code(&streamed), 2, "{}", err(&streamed));
    assert!(err(&streamed).contains("not current"), "{}", err(&streamed));
    let accepted = at(
        scratch.path(),
        &[
            ".",
            "export",
            "ductilities",
            "-o",
            "-",
            "--accept",
            "outdated",
        ],
    );
    assert_eq!(code(&accepted), 0, "{}", err(&accepted));
}

#[test]
fn a_unit_holding_a_control_character_is_refused() {
    let scratch = Scratch::new("unit-control");
    let root = two_pieces(&scratch, "p");
    let output = at(
        &root,
        &["a.md", "set", "diameter.unit=g\u{1b}[31m", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("control character"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_scripts_options_complete_after_set() {
    let scratch = Scratch::new("set-completion");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Billet\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\n\nclass Billet(sk.Sample):\n    pass\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nname: a\n---\n");
    describe(scratch.path(), serde_json::json!({}), serde_json::json!({}));
    // The description as the worker writes it, with one script.
    let path = scratch.path().join(".samplekit/model.json");
    let mut held: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    held["scripts"] = serde_json::json!([{
        "name": "scale", "acts": "sample", "description": "Scale.",
        "options": [{"name": "factor", "type": "float", "default": "0.5"}],
    }]);
    fs::write(&path, held.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", ".", "run", "scale", "--set", "f"])
        .current_dir(scratch.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "5")
        .env("_CLAP_IFS", "\n")
        .output()
        .unwrap();
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("factor="), "{}", out(&output));
}

#[test]
fn a_short_name_mistyped_before_its_readings_is_said() {
    // `notcj.readings=…` for `notch` wrote a field of its own without a word, and
    // `notch` kept the readings it was meant to correct.
    let scratch = Scratch::new("short-typo");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  notch: {readings: [1.010, 1.011]}\n---\n",
    );
    for field in ["notcj.readings=1.011,1.012", "notcj=1.012"] {
        let output = at(scratch.path(), &["a.md", "set", field]);
        assert!(
            err(&output).contains(
                "no sample here holds 'notcj', written as a new field — did you mean 'notch'?"
            ),
            "{field}: {}",
            err(&output)
        );
    }
}

#[test]
fn a_refused_restore_says_the_values_to_set_and_what_restore_at_reaches() {
    // After a conflict resolved by taking the colleague's file, the one way
    // offered was `restore --at`, said to reach *the whole project*, which
    // took the colleague's work back too.
    let scratch = Scratch::new("restore-advice");
    let root = two_pieces(&scratch, "p");
    let mine = as_machine(&root, "ana", &["a.md", "set", "diameter=1.5", "--write"]);
    assert_eq!(code(&mine), 0, "{}", err(&mine));
    let theirs = as_machine(&root, "tom", &["a.md", "set", "diameter=1.8", "--write"]);
    assert_eq!(code(&theirs), 0, "{}", err(&theirs));
    let refused = as_machine(&root, "ana", &["a.md", "restore"]);
    assert_eq!(code(&refused), 2, "{}", out(&refused));
    let said = err(&refused);
    assert!(
        said.contains("set those values again by hand: samplekit diff --from"),
        "{said}"
    );
    assert!(
        said.contains("puts a.md back as it was before it, and takes back with it what was changed there since"),
        "{said}"
    );
    assert!(!said.contains("whole project"), "{said}");
}

#[test]
fn a_mean_is_shown_with_its_readings() {
    // A reading typed wrong was found only with `-c name,notch.readings`.
    let scratch = Scratch::new("mean-readings");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  notch:\n    v: 1.045\n    \
         readings: [1.012, 1.13, 1.012]\n    statistics: {v: mean}\n    \
         computed: {readings: aaaaaaaaaaaa}\n---\n",
    );
    let viewed = out(&at(scratch.path(), &["a.md", "view"]));
    assert!(viewed.contains("readings 1.012, 1.13, 1.012"), "{viewed}");
}

#[test]
fn a_kept_value_leaves_its_computed_uncertainty_to_the_model() {
    // `new --keep … volume` wrote the uncertainty a formula gave with no
    // record: *record missing* for good.
    let scratch = Scratch::new("keep-uncertainty");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  volume:\n    v: 20.5\n    \
         u: 0.2886751345948129\n    unit: L\n    computed: {u: {}}\n---\n",
    );
    let made = at(
        scratch.path(),
        &[".", "new", "b", "--keep", "a.md", "volume", "--write"],
    );
    assert_eq!(code(&made), 0, "{}", err(&made));
    let written = fs::read_to_string(scratch.path().join("b.md")).unwrap();
    assert!(written.contains("volume: {v: 20.5, unit: L}"), "{written}");
}

#[test]
fn explain_lists_the_readings_a_mean_is_taken_of() {
    let scratch = Scratch::new("explain-readings");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\nclass = \"Billet\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\n\nclass Billet(sk.Sample):\n    pass\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  notch:\n    v: 1.045\n    \
         readings: [1.012, 1.13, 1.012]\n    statistics: {v: mean}\n    \
         computed: {readings: aaaaaaaaaaaa}\n---\n",
    );
    describe(
        scratch.path(),
        serde_json::json!({"notch": {
            "unit": null, "value": {"from": "statistic", "statistic": "mean"},
            "uncertainty": null, "reads": [], "formula": null, "default": null,
        }}),
        serde_json::json!({}),
    );
    let explained = out(&at(scratch.path(), &["a.md", "explain", "notch"]));
    assert!(
        explained.contains("computed from its own readings — 1.012, 1.13, 1.012 — as their mean"),
        "{explained}"
    );
}

/// A root above two projects, `p` declaring a figure and an export, `q`
/// declaring none.
fn a_root_of_projects(scratch: &Scratch) {
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write(
        "p/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n[figure.fp]\nx = \"diameter\"\ny = \"diameter\"\n\
         [profile.pp]\ncolumns = [{field = \"diameter\"}]\n\
         [export.ep]\nprofile = \"pp\"\nformat = \"csv\"\noutput = \"out/ep.csv\"\n",
    );
    scratch.write("q/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    for (file, name) in [("p/a.md", "a"), ("q/b.md", "b")] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
        );
    }
}

#[test]
fn a_figure_from_a_root_is_the_selections_project() {
    // `samplekit . plot fp -f …` drew nothing where the root declares no
    // figure, though every sample selected was `p`'s.
    let scratch = Scratch::new("root-figure");
    a_root_of_projects(&scratch);
    let one = at(
        scratch.path(),
        &[".", "plot", "fp", "-f", "name == 'a'", "-o", "f.png"],
    );
    assert_eq!(code(&one), 0, "{}", err(&one));
    assert!(out(&one).contains("figure fp"), "{}", out(&one));
    let mixed = at(scratch.path(), &[".", "plot", "fp", "-o", "f.png"]);
    assert_eq!(code(&mixed), 1, "{}", out(&mixed));
    assert!(
        err(&mixed).contains("the selection spans several projects of different models"),
        "{}",
        err(&mixed)
    );
    assert!(
        err(&mixed).contains("samplekit p plot fp"),
        "{}",
        err(&mixed)
    );
    let listed = at(scratch.path(), &[".", "list", "figures"]);
    assert!(
        err(&listed).contains("span several projects of different models"),
        "{}",
        err(&listed)
    );
}

#[test]
fn list_skipped_counts_what_include_leaves_out() {
    // A Markdown file no include pattern matches is no candidate, and was
    // listed nowhere: a pattern written too narrowly went unseen.
    let scratch = Scratch::new("include-leaves-out");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\ninclude = [\"piece-*.md\"]\nexclude = [\"piece-old.md\"]\n",
    );
    for name in ["piece-a", "piece-old", "other", "notes"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\n---\n"),
        );
    }
    let listed = at(scratch.path(), &[".", "list", "skipped"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        out(&listed).contains("piece-old.md  excluded by the pattern 'piece-old.md'"),
        "{}",
        out(&listed)
    );
    assert!(
        out(&listed).contains(
            "2 Markdown files the include patterns leave out — include = [\"piece-*.md\"]"
        ),
        "{}",
        out(&listed)
    );
    // Under the default include, nothing is counted.
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let default = at(scratch.path(), &[".", "list", "skipped"]);
    assert!(!out(&default).contains("leave out"), "{}", out(&default));
}

#[test]
fn an_export_run_from_above_its_project_is_made_again_where_it_was() {
    // Run from the folder above, the command `explain` gave climbed one
    // folder for each part of that folder's whole path, out of the disk's
    // root.
    let scratch = Scratch::new("export-from-above");
    scratch.write(
        "proj/.samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"out/e.csv\"\n",
    );
    scratch.write("proj/a.md", "---\nschema_version: 1\nname: a\n---\n");
    let written = at(scratch.path(), &["proj", "export", "e", "--write"]);
    assert_eq!(code(&written), 0, "{}", err_slashed(&written));
    let explained = at(scratch.path(), &["proj/out/e.csv", "explain"]);
    let said = out_slashed(&explained);
    let again = said
        .lines()
        .find_map(|line| {
            line.split_once("made again as it was, ")
                .map(|(_, rest)| rest)
        })
        .and_then(|rest| {
            rest.split_once(": ")
                .map(|(_, command)| command.to_string())
        })
        .expect(&said);
    assert!(again.contains("-o proj/out/e.csv"), "{again}");
    // Run as it is said, from that folder, it writes the file again there.
    fs::remove_file(scratch.path().join("proj/out/e.csv")).unwrap();
    let words = shell_words::split(&again).unwrap();
    let made = at(
        scratch.path(),
        &words[1..].iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(code(&made), 0, "{}", err_slashed(&made));
    assert!(scratch.path().join("proj/out/e.csv").is_file());
}

#[test]
fn a_file_written_over_two_projects_is_not_offered_again_at_one_snapshot() {
    // Its command was given `--at` a snapshot of one project's history,
    // which the other's refused; and from a root with a history of its own,
    // the file was said changed since, the records being the projects'.
    let scratch = Scratch::new("two-projects-again");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    for project in ["ana", "tom"] {
        scratch.write(&format!("{project}/.samplekitrc"), "schema_version = 1\n");
        scratch.write(
            &format!("{project}/{project}-1.md"),
            &format!("---\nschema_version: 1\nname: {project}-1\n---\n"),
        );
    }
    let written = at(
        scratch.path(),
        &[
            "ana",
            "tom",
            "-c",
            "project,name",
            "-o",
            "paper/t.csv",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    for arguments in [
        vec!["paper/t.csv", "explain"],
        vec!["explain", "paper/t.csv"],
    ] {
        let explained = at(scratch.path(), &arguments);
        assert_eq!(code(&explained), 0, "{arguments:?}: {}", err(&explained));
        assert!(
            out(&explained).contains("written over 2 projects, ana, tom"),
            "{arguments:?}: {}",
            out(&explained)
        );
        assert!(
            !out(&explained).contains("made again as it was,"),
            "{}",
            out(&explained)
        );
    }
}

#[test]
fn a_folder_made_a_project_since_is_not_said_removed() {
    // Its files, kept by the root's history before the folder had a
    // `.samplekitrc` of its own, were said *removed*, and `restore` named a
    // command that found no history.
    let scratch = Scratch::new("folder-made-a-project");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write(
        "sub/a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    );
    let written = at(
        scratch.path(),
        &["sub/a.md", "set", "diameter=2.0", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    scratch.write("sub/.samplekitrc", "schema_version = 1\n");
    // Each project's last change: the root's says nothing removed. A
    // snapshot named over the two is refused, naming them.
    let compared = at(scratch.path(), &[".", "diff"]);
    assert_eq!(code(&compared), 0, "{}", err(&compared));
    assert!(!out(&compared).contains("removed"), "{}", out(&compared));
    let named = at(scratch.path(), &[".", "diff", "--from", "1"]);
    assert_eq!(code(&named), 1, "{}", out(&named));
    assert!(err(&named).contains("., sub"), "{}", err(&named));
    let restored = at(scratch.path(), &[".", "restore"]);
    assert!(
        !out(&restored).contains("deleted since"),
        "{}",
        out(&restored)
    );
}

#[test]
fn a_path_is_read_before_or_after_the_command() {
    // The path written after the command reads as written before it, and
    // the two sides are one list of targets.
    let scratch = Scratch::new("path-after-command");
    scratch.corpus();
    for (after, before) in [
        (vec!["status", "."], vec![".", "status"]),
        (vec!["list", "fields", "."], vec![".", "list", "fields"]),
        (
            vec!["view", "billet-01.sample.md"],
            vec!["billet-01.sample.md", "view"],
        ),
    ] {
        let read = at(scratch.path(), &after);
        assert_eq!(code(&read), 0, "{after:?}: {}", err(&read));
        assert_eq!(out(&read), out(&at(scratch.path(), &before)), "{after:?}");
    }
    let both = at(
        scratch.path(),
        &["billet-01.sample.md", "status", "billet-02.sample.md"],
    );
    assert!(out(&both).contains("in 2 samples"), "{}", out(&both));
    let set = at(
        scratch.path(),
        &["set", "billet-01.sample.md", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        fs::read_to_string(scratch.path().join("billet-01.sample.md"))
            .unwrap()
            .contains("v: 13")
    );
    let tagged = at(scratch.path(), &["tag", "add", "x", ".", "--write"]);
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    assert!(
        fs::read_to_string(scratch.path().join("billet-04.sample.md"))
            .unwrap()
            .contains("x]")
    );
}

#[test]
fn an_option_before_the_command_is_the_commands() {
    // An option written first, with no path, is the command's.
    let scratch = Scratch::new("option-first");
    scratch.corpus();
    let status = at(scratch.path(), &["-f", "diameter > 12", "status"]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(out(&status).contains("in 3 samples"), "{}", out(&status));
    let export = at(
        scratch.path(),
        &["-f", "diameter > 12", "export", "ductilities"],
    );
    assert_eq!(code(&export), 0, "{}", err(&export));
}

#[test]
fn the_command_wins_over_a_folder_of_its_name() {
    // A folder named as a command is written another way, or after the
    // command.
    let scratch = Scratch::new("command-folder");
    scratch.corpus();
    scratch.write(
        "status/s1.md",
        "---\nschema_version: 1\nname: s1\nproperties:\n  diameter: 3.0\n---\n",
    );
    let command = at(scratch.path(), &["status"]);
    assert_eq!(code(&command), 0, "{}", err(&command));
    assert!(!out(&command).contains("in 1 sample"), "{}", out(&command));
    for arguments in [vec!["status", "status"], vec!["status/", "status"]] {
        let folder = at(scratch.path(), &arguments);
        assert_eq!(code(&folder), 0, "{arguments:?}: {}", err(&folder));
        assert!(
            out(&folder).contains("in 1 sample"),
            "{arguments:?}: {}",
            out(&folder)
        );
    }
}

#[test]
fn an_unquoted_filter_says_to_quote_it() {
    // `-f` takes one word: the filter's other words are said, not read as
    // paths.
    let scratch = Scratch::new("unquoted-filter");
    scratch.corpus();
    let unquoted = at(
        scratch.path(),
        &["status", "-f", "tags", "has", "reference"],
    );
    assert_eq!(code(&unquoted), 1, "{}", out(&unquoted));
    assert!(
        err(&unquoted).contains(
            "has: no such file — a filter of several words is quoted: -f 'tags has reference'"
        ),
        "{}",
        err(&unquoted)
    );
    let missing = at(scratch.path(), &["status", "-f", "diameter > 1", "nowhere"]);
    assert_eq!(code(&missing), 3, "{}", out(&missing));
    assert!(
        err(&missing).contains("no such file or directory"),
        "{}",
        err(&missing)
    );
}

#[test]
fn every_command_on_samples_takes_a_selection() {
    // `-f` and `--query` narrow every command that reads samples.
    let scratch = Scratch::new("selection-everywhere");
    scratch.corpus();
    let validated = at(scratch.path(), &["validate", "-f", "status == rejected"]);
    assert_eq!(code(&validated), 0, "{}", err(&validated));
    assert!(out(&validated).contains("1 sample"), "{}", out(&validated));
    let fields = at(
        scratch.path(),
        &["list", "fields", "-f", "status == rejected"],
    );
    assert_eq!(code(&fields), 0, "{}", err(&fields));
    assert!(!out(&fields).contains("porosity"), "{}", out(&fields));
    let explained = at(
        scratch.path(),
        &["explain", "diameter", "-f", "status == rejected"],
    );
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("billet-04 · diameter"),
        "{}",
        out(&explained)
    );
    assert!(
        !out(&explained).contains("billet-01"),
        "{}",
        out(&explained)
    );
    // Each sample previewed, one question for them all.
    let previewed = at(
        scratch.path(),
        &["set", "-f", "status == approved", "diameter=13"],
    );
    assert_eq!(code(&previewed), 0, "{}", err(&previewed));
    assert_eq!(
        out(&previewed).matches("diameter  ").count(),
        3,
        "{}",
        out(&previewed)
    );
    assert_eq!(
        err(&previewed).matches("apply these changes?").count(),
        1,
        "{}",
        err(&previewed)
    );
    let set = at(
        scratch.path(),
        &["set", "-f", "status == approved", "diameter=13", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    for (file, changed) in [
        ("billet-01.sample.md", true),
        ("billet-04.sample.md", false),
    ] {
        let text = fs::read_to_string(scratch.path().join(file)).unwrap();
        assert_eq!(text.contains("v: 13"), changed, "{file}: {text}");
    }
    let refused = at(scratch.path(), &["new", "x", "-f", "diameter > 1"]);
    assert_eq!(code(&refused), 1, "{}", out(&refused));
}

#[test]
fn log_diff_and_restore_take_a_selection() {
    // The history of the samples a selection keeps: now for `log` and
    // `diff`, at the snapshot put back for `restore`.
    let scratch = Scratch::new("history-selection");
    with_history(&scratch);
    let log = at(scratch.path(), &["log", "-f", "tags has checked"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    assert!(out(&log).contains("tag add checked"), "{}", out(&log));
    assert!(!out(&log).contains("diameter=13"), "{}", out(&log));
    let diff = at(
        scratch.path(),
        &["diff", "--from", "4", "-f", "diameter > 12.5"],
    );
    assert_eq!(code(&diff), 0, "{}", err(&diff));
    assert!(out(&diff).contains("billet-01"), "{}", out(&diff));
    assert!(!out(&diff).contains("billet-09"), "{}", out(&diff));
    // Deleted since, it is still kept by the selection as it was then.
    fs::remove_file(scratch.path().join("billet-02.sample.md")).unwrap();
    let restored = at(
        scratch.path(),
        &["restore", "-f", "tags has checked", "--at", "2", "--write"],
    );
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert!(scratch.path().join("billet-02.sample.md").is_file());
    assert!(
        fs::read_to_string(scratch.path().join("billet-01.sample.md"))
            .unwrap()
            .contains("v: 13")
    );
}

#[test]
fn explain_looks_for_a_written_file_in_the_projects_around() {
    // A copy outside its project, explained from a folder above it: found in
    // the project below; from a folder with none, said so.
    let scratch = Scratch::new("explain-around");
    scratch.write("proj/.samplekitrc", "schema_version = 1\n");
    scratch.write(
        "proj/a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: 3.0\n---\n",
    );
    let state = scratch.path().join("state");
    let run = |folder: &Path, arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(arguments)
            .current_dir(folder)
            .env("COLUMNS", "200")
            .env("SAMPLEKIT_STATE_DIR", &state)
            .output()
            .unwrap()
    };
    let written = run(
        &scratch.path().join("proj"),
        &["-c", "name,diameter", "-o", "t.csv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    scratch.write(
        "report/t.csv",
        &fs::read_to_string(scratch.path().join("proj/t.csv")).unwrap(),
    );
    let found = run(scratch.path(), &["explain", "report/t.csv"]);
    assert_eq!(code(&found), 0, "{}", err(&found));
    assert!(out(&found).contains("written   t.csv"), "{}", out(&found));
    let lost = run(&scratch.path().join("report"), &["explain", "t.csv"]);
    assert_eq!(code(&lost), 1, "{}", out(&lost));
    assert!(
        err(&lost).contains("no project here or below keeps t.csv"),
        "{}",
        err(&lost)
    );
}

#[test]
fn a_written_column_names_a_data_files_header() {
    // `-c field=NAME` names the column in what is written: the header of a
    // data file, the label of a table on the terminal; quoted, a name holds a
    // comma.
    let scratch = Scratch::new("written-header");
    scratch.corpus();
    let written = at(
        scratch.path(),
        &[".", "-c", "name,diameter=M", "-o", "x.csv", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let csv = fs::read_to_string(scratch.path().join("x.csv")).unwrap();
    assert!(
        csv.lines()
            .next()
            .unwrap()
            .starts_with("name,M_value [g],M_uncertainty [g]"),
        "{csv}"
    );
    let comma = at(
        scratch.path(),
        &[".", "-c", "name,diameter='Diameter, dry'"],
    );
    assert_eq!(code(&comma), 0, "{}", err(&comma));
    assert!(out(&comma).contains("Diameter, dry"), "{}", out(&comma));
}

#[test]
fn advice_names_the_target_it_was_given() {
    // Run from above the project, advice naming no target sent the reader to
    // the folder they were in.
    let scratch = Scratch::new("advice-target");
    two_pieces(&scratch, "p");
    let unknown = at(
        scratch.path(),
        &["p", "-f", "tags has nosuch", "-c", "name"],
    );
    assert_eq!(code(&unknown), 1, "{}", out(&unknown));
    assert!(
        err(&unknown).contains("samplekit p list tags"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn a_descending_sort_reads_under_at() {
    // `-s -diameter --at 1` read the sort's value as a path to move into the
    // project as it was.
    let scratch = Scratch::new("sort-at");
    with_history(&scratch);
    let sorted = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "-s", "-diameter", "--at", "1"],
    );
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    let shown = out(&sorted);
    let first = shown.find("billet-01").unwrap();
    let last = shown.find("billet-04").unwrap();
    assert!(first < last, "{shown}");
}

#[test]
fn verbose_names_what_the_include_patterns_leave_out() {
    // A pattern written too narrowly left samples out of every table, and
    // `-v`, which says what was skipped, said nothing of them.
    let scratch = Scratch::new("include-verbose");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    fs::write(
        scratch.path().join(".samplekitrc"),
        format!("{rc}\n[collection]\ninclude = [\"billet-01*\"]\n"),
    )
    .unwrap();
    let listed = at(scratch.path(), &[".", "-c", "name", "-v"]);
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    assert!(
        err(&listed).contains("skipped billet-02.sample.md: matches no include pattern"),
        "{}",
        err(&listed)
    );
    let quiet = at(scratch.path(), &["list"]);
    assert!(!err(&quiet).contains("include"), "{}", err(&quiet));
}

#[test]
fn a_quantity_takes_no_list_in_new_and_a_blank_removes_an_attribute() {
    // `new … 'diameter=[…]'` wrote an attribute where the project declares a
    // quantity, and `diameter=` then removed nothing.
    let scratch = Scratch::new("new-list-quantity");
    scratch.corpus();
    let refused = at(
        scratch.path(),
        &["new", "billet-09", "diameter=[12.1, 12.2]"],
    );
    assert_eq!(code(&refused), 1, "{}", out(&refused));
    assert!(
        err(&refused).contains("diameter.readings=12.1,12.2"),
        "{}",
        err(&refused)
    );
    scratch.write(
        "odd.sample.md",
        "---\nschema_version: 1\nname: odd\ncarbon: [Alpha, Mesophase]\n---\n",
    );
    let removed = at(
        scratch.path(),
        &["odd.sample.md", "set", "carbon=", "--write"],
    );
    assert_eq!(code(&removed), 0, "{}", err(&removed));
    assert!(
        !fs::read_to_string(scratch.path().join("odd.sample.md"))
            .unwrap()
            .contains("carbon"),
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_folder_made_a_project_brings_its_history() {
    // The root kept the snapshots of a folder before it was made a project:
    // brought into the folder's own history, before its first.
    let scratch = Scratch::new("bring-history");
    let root = scratch.path().join("root");
    scratch.write(
        "root/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    for name in ["a", "b"] {
        scratch.write(
            &format!("root/sub/{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
        );
    }
    for (file, value) in [("sub/a.md", "diameter=2"), ("sub/b.md", "diameter=3")] {
        let set = at(&root, &[file, "set", value, "--write"]);
        assert_eq!(code(&set), 0, "{}", err(&set));
    }
    scratch.write("root/sub/.samplekitrc", "schema_version = 1\n");
    // Reached outside a terminal: brought without asking, and said.
    let read = at(&root, &["sub", "status"]);
    assert_eq!(code(&read), 0, "{}", err(&read));
    assert!(
        err(&read).contains("3 snapshots of sub brought from the history of ."),
        "{}",
        err(&read)
    );
    let log = out(&at(&root, &["sub", "log"]));
    assert!(
        log.contains("samplekit sub/a.md set diameter=2 --write"),
        "{log}"
    );
    assert!(log.contains("first kept it"), "{log}");
    // The root keeps them, numbered as before, and shows none of them: the
    // files are the folder's now. Nothing is said again.
    let kept = out(&at(&root, &["log"]));
    assert!(kept.lines().any(|line| line.starts_with("3 ")), "{kept}");
    assert!(!err(&at(&root, &["sub", "status"])).contains("brought"));
}

#[test]
fn a_selection_says_a_file_it_could_not_read() {
    // `validate -f` over a folder holding a file that does not read said
    // *0 defects*, exit 0: no selection can say the file would be left out.
    let scratch = Scratch::new("selection-unread");
    scratch.corpus();
    scratch.write("broken.sample.md", "---\nschema_version: 1\nname: [\n---\n");
    let validated = at(
        scratch.path(),
        &[".", "validate", "-f", "status == approved"],
    );
    assert_eq!(code(&validated), 2, "{}", out(&validated));
    assert!(
        out(&validated).contains("broken.sample.md"),
        "{}",
        out(&validated)
    );
    assert!(out(&validated).contains("3 samples"), "{}", out(&validated));
    for arguments in [
        vec![".", "log", "-f", "status == approved"],
        vec![".", "diff", "-f", "status == approved"],
    ] {
        let read = at(scratch.path(), &arguments);
        assert_eq!(code(&read), 2, "{arguments:?}: {}", err(&read));
    }
}

#[test]
fn list_with_a_selection_lists_the_selection() {
    // `list tags -f` listed every tag of the collection, and `list -f` said
    // *no .samplekitrc above this directory*.
    let scratch = Scratch::new("list-selection");
    scratch.corpus();
    let tagged = at(
        scratch.path(),
        &["billet-04.sample.md", "tag", "add", "spoilt", "--write"],
    );
    assert_eq!(code(&tagged), 0, "{}", err(&tagged));
    let tags = at(
        scratch.path(),
        &["list", "tags", "-f", "status == approved"],
    );
    assert_eq!(out(&tags), "reference\n");
    let listed = at(scratch.path(), &["list", "-f", "status == approved"]);
    assert!(out(&listed).starts_with("3 samples"), "{}", out(&listed));
    assert!(
        !out(&listed).contains("no .samplekitrc"),
        "{}",
        out(&listed)
    );
    assert!(out(&listed).contains("declared in"), "{}", out(&listed));
}

#[test]
fn a_history_selection_keeps_a_sample_deleted_since() {
    // `diff -f` and `log -f` read the selection in the files there now
    // alone: a sample deleted since escaped it.
    let scratch = Scratch::new("history-selection-deleted");
    with_history(&scratch);
    fs::remove_file(scratch.path().join("billet-04.sample.md")).unwrap();
    let diff = at(scratch.path(), &["diff", "-f", "status == rejected"]);
    assert_eq!(code(&diff), 0, "{}", err(&diff));
    assert!(out(&diff).contains("billet-04"), "{}", out(&diff));
    // Its snapshots, the deletion not yet kept among them.
    let log = at(scratch.path(), &["log", "-f", "status == rejected"]);
    assert!(out(&log).contains("not yet kept"), "{}", out(&log));
    assert!(out(&log).contains("first kept it"), "{}", out(&log));
}

#[test]
fn set_over_several_samples_writes_under_a_configuration_not_read() {
    // The first sample's warning stopped the preview, and with --write
    // nothing was written, nothing said, exit 2.
    let scratch = Scratch::new("set-several-unread-rc");
    scratch.corpus();
    let rc = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    fs::write(
        scratch.path().join(".samplekitrc"),
        format!("{rc}garbage = [\n"),
    )
    .unwrap();
    let written = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "billet-02.sample.md",
            "set",
            "status=checked",
            "--write",
        ],
    );
    assert_eq!(code(&written), 2, "{}", err(&written));
    assert!(
        err(&written).contains("configuration not read"),
        "{}",
        err(&written)
    );
    for file in ["billet-01.sample.md", "billet-02.sample.md"] {
        let text = fs::read_to_string(scratch.path().join(file)).unwrap();
        assert!(text.contains("status: checked"), "{file}: {text}");
    }
    let preview = at(
        scratch.path(),
        &[
            "billet-03.sample.md",
            "billet-04.sample.md",
            "set",
            "status=x",
        ],
    );
    assert_eq!(
        err(&preview).matches("apply these changes?").count(),
        1,
        "{}",
        err(&preview)
    );
}

#[test]
fn a_selection_unchanged_is_said_once() {
    // Each sample unchanged said *nothing to change — every value…* before
    // the question asked for the others.
    let scratch = Scratch::new("set-several-unchanged");
    scratch.corpus();
    let preview = at(
        scratch.path(),
        &["set", "-f", "status == approved", "status=approved"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert_eq!(
        out(&preview)
            .matches("nothing to change — every value")
            .count(),
        1,
        "{}",
        out(&preview)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn open_counts_the_files_of_every_sample_and_refuses_a_pattern_with_edit() {
    // Over several samples the question counted samples, not files: fifteen
    // opened outside a terminal, unasked; `--edit` ignored a pattern.
    let scratch = Scratch::new("open-count");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images\"]\n",
    );
    for name in ["B1", "B2"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\n---\n"),
        );
        for index in 0..4 {
            scratch.write(&format!("images/{name}_{index}.png"), "");
        }
    }
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
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let many = open(&["open", "B1.md", "B2.md"]);
    assert_eq!(code(&many), 1, "{}", out(&many));
    assert!(
        err(&many).contains("8 files of 2 samples"),
        "{}",
        err(&many)
    );
    assert!(!opened.exists());
    let edit = open(&["open", "B1.md", "*.png", "--edit"]);
    assert_eq!(code(&edit), 1, "{}", out(&edit));
    assert!(err(&edit).contains("one or the other"), "{}", err(&edit));
}

#[test]
fn explain_takes_a_markdown_table_samplekit_wrote() {
    // A table written as Markdown was taken for a sample, and refused.
    let scratch = Scratch::new("explain-markdown-table");
    scratch.corpus();
    let written = at(
        scratch.path(),
        &[".", "-c", "name,diameter", "-o", "t.md", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let explained = at(scratch.path(), &["explain", "t.md"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("written   t.md"),
        "{}",
        out(&explained)
    );
}

#[test]
fn a_key_written_twice_is_not_read() {
    // The last value was read by every command without a word, and a write
    // dropped the first.
    let scratch = Scratch::new("key-twice");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "d.md",
        "---\nschema_version: 1\nname: d\nproperties:\n  radius: 1.05\n  radius: 1.07\n---\n",
    );
    let shown = at(scratch.path(), &[".", "-c", "name,radius"]);
    // Named at its own lines, where the line of its mapping was said.
    assert!(
        err(&shown).contains("line 5: 'radius' is written twice, at lines 5 and 6"),
        "{}",
        err(&shown)
    );
    let set = at(scratch.path(), &["d.md", "set", "radius=1.06", "--write"]);
    assert_ne!(code(&set), 0, "{}", out(&set));
    assert!(
        fs::read_to_string(scratch.path().join("d.md"))
            .unwrap()
            .contains("radius: 1.05")
    );
}

#[test]
fn a_prepared_table_survives_a_write() {
    // A table its file held without a row was dropped by any write, and
    // `--add-row` then found no table.
    let scratch = Scratch::new("prepared-table");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "e.md",
        "---\nschema_version: 1\nname: e\nproperties:\n  diameter: 1.0\ntables:\n  inspection:\n    \
         title: Inspection\n    index: inspector\n    columns:\n      inspector: {}\n      score: {unit: pt}\n    \
         rows: []\n---\n",
    );
    let set = at(scratch.path(), &["e.md", "set", "diameter=2", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let text = fs::read_to_string(scratch.path().join("e.md")).unwrap();
    assert!(
        text.contains("inspection:") && text.contains("rows: []"),
        "{text}"
    );
    let added = at(
        scratch.path(),
        &[
            "e.md",
            "set",
            "--add-row",
            "inspection",
            "inspector=Sam",
            "score=4",
            "--write",
        ],
    );
    assert_eq!(code(&added), 0, "{}", err(&added));
}

#[test]
fn in_and_has_refuse_a_number_against_text() {
    // `radius in (high, low)` selected nothing, and its negation everything.
    let scratch = Scratch::new("in-has-types");
    scratch.corpus();
    for filter in ["diameter in (high, low)", "status in (1, 2)"] {
        let output = at(scratch.path(), &[".", "-f", filter, "-c", "name"]);
        assert_eq!(code(&output), 1, "{filter}: {}", out(&output));
        assert!(
            err(&output).contains("cannot be compared"),
            "{filter}: {}",
            err(&output)
        );
    }
}

#[test]
fn has_on_a_column_a_sample_lacks_is_unknown() {
    // `!(inspection.brittleness has 2)` selected a sample nothing was known of.
    let scratch = Scratch::new("has-unknown-column");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  inspection:\n    index: inspector\n    rows:\n      \
         - {inspector: Lea, brittleness: 2}\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\ntables:\n  inspection:\n    index: inspector\n    rows:\n      \
         - {inspector: Sam}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-f", "!(inspection.brittleness has 2)", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains('b'), "{}", out(&output));
}

#[test]
fn the_path_field_is_one_form_however_the_target_is_typed() {
    // `./pieces` and `pieces` gave the field two values, and a saved query
    // answered one way and the other.
    let scratch = Scratch::new("path-form");
    let root = two_pieces(&scratch, "p");
    for target in ["p", "./p", "p/"] {
        let output = at(
            scratch.path(),
            &[target, "-f", "path starts_with \"p/\"", "-c", "path"],
        );
        assert_eq!(code(&output), 0, "{target}: {}", err(&output));
        assert!(
            out(&output).contains("p/a.md"),
            "{target}: {}",
            out(&output)
        );
    }
    let whole = root.display().to_string();
    let output = at(scratch.path(), &[&whole, "-c", "path"]);
    assert!(out(&output).contains("p/a.md"), "{}", out(&output));
}

#[test]
fn equality_with_a_number_shown_rounded_warns() {
    let scratch = Scratch::new("rounded-equality");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "d.md",
        "---\nschema_version: 1\nname: d\nproperties:\n  radius: 1.0487\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-f", "radius == 1.049", "-c", "name"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("one holds 1.0487, shown 1.049 rounded"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_summary_leaves_tags_out() {
    // Tags were summarised as a numeric column no sample fills: `tags 0/4`.
    let scratch = Scratch::new("summary-tags");
    scratch.corpus();
    let output = at(scratch.path(), &[".", "-c", "tags,diameter", "--summary"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(!out(&output).contains("0/4"), "{}", out(&output));
}

/// A root that kept three snapshots of `sub/` before `sub/` was made a
/// project; the root's folder.
fn folder_made_a_project(scratch: &Scratch) -> PathBuf {
    let root = scratch.path().join("root");
    scratch.write(
        "root/.samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write(
        "root/top.md",
        "---\nschema_version: 1\nname: top\nproperties:\n  diameter: 5.0\n---\n",
    );
    for name in ["a", "b"] {
        scratch.write(
            &format!("root/sub/{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
        );
    }
    for (file, value) in [("sub/a.md", "diameter=2"), ("sub/b.md", "diameter=3")] {
        let set = at(&root, &[file, "set", value, "--write"]);
        assert_eq!(code(&set), 0, "{}", err(&set));
    }
    scratch.write("root/sub/.samplekitrc", "schema_version = 1\n");
    root
}

#[test]
fn a_first_write_brings_the_history_before_it() {
    // Answered no, or never asked outside a terminal, a first write there
    // started the folder's own history: the snapshots above were out of
    // reach for good.
    let scratch = Scratch::new("bring-first-write");
    let root = folder_made_a_project(&scratch);
    let written = at(&root, &["sub/a.md", "set", "diameter=4", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        err(&written).contains("3 snapshots of sub brought"),
        "{}",
        err(&written)
    );
    let log = out(&at(&root, &["sub", "log"]));
    assert!(log.contains("set diameter=2 --write"), "{log}");
    assert!(log.contains("set diameter=4 --write"), "{log}");
}

#[test]
fn a_root_write_away_from_the_folder_is_not_refused() {
    // Outside a terminal, every write of the root was refused while a
    // folder below had not answered the offer; now nothing is refused.
    let scratch = Scratch::new("offer-root-write");
    let root = folder_made_a_project(&scratch);
    let written = at(&root, &["top.md", "set", "diameter=6", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    // Its message is no output: a table written to a file holds none of it.
    let shown = at(&root, &["sub", "-c", "name,diameter", "--csv"]);
    assert!(err(&shown).contains("brought"), "{}", err(&shown));
    assert!(!out(&shown).contains("snapshot"), "{}", out(&shown));
    let written = at(&root, &["sub/a.md", "set", "diameter=6", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
}

#[test]
fn a_failed_cell_is_written_with_its_last_number() {
    // A cell whose formula failed is written as the table shows it, its last
    // number, its state saying not to trust it.
    let scratch = Scratch::new("failed-cell");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  ageing:\n    index: day\n    rows:\n      \
         - {day: 1, hardness: {v: 1.02, computed: {day: aaaaaaaaaaaa}, fingerprint: {failed: ZeroDivisionError}}}\n---\n",
    );
    let output = at(
        scratch.path(),
        &[".", "-c", "name,ageing.hardness[1]", "--csv"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("a,1.02,failed"), "{}", out(&output));
}

#[test]
fn a_sample_that_no_longer_reads_is_not_kept_deleted() {
    // Its `schema_version` taken out by hand, a sample was kept *removed* in a
    // snapshot, and a restore then replaced every edit it held.
    let scratch = Scratch::new("unread-kept");
    with_history(&scratch);
    let file = scratch.path().join("billet-03.sample.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("schema_version: 1\n", "")).unwrap();
    let set = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=14", "--write"],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    assert!(
        err(&set).contains("does not read as a sample now"),
        "{}",
        err(&set)
    );
    let diff = at(scratch.path(), &["diff", "--from", "2", "--to", "1"]);
    assert!(!out(&diff).contains("removed"), "{}", out(&diff));
}

#[test]
fn an_unusable_tag_keeps_its_place_through_a_write() {
    // `[2024, batch_1]` was written `[batch_1, "2024"]`, the tag that is no
    // name moved last.
    let scratch = Scratch::new("tag-order");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "g.md",
        "---\nschema_version: 1\nname: g\ntags: [2024, batch_1]\nproperties:\n  radius: 1.05\n---\n",
    );
    let set = at(scratch.path(), &["g.md", "set", "radius=1.06", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let text = fs::read_to_string(scratch.path().join("g.md")).unwrap();
    assert!(
        text.contains("tags: ['2024', batch_1]") || text.contains("tags: [\"2024\", batch_1]"),
        "{text}"
    );
}

#[test]
fn a_set_preview_says_the_comments_a_write_drops() {
    // `compute` said them; `set` dropped them unsaid, and a file closed by a
    // `---` without a line ending counted none.
    let scratch = Scratch::new("comments-dropped");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\n# a remark\nproperties:\n  m: 4\n---",
    );
    let preview = at(scratch.path(), &["b.md", "set", "m=5"]);
    assert!(
        out(&preview).contains("this write drops 1 YAML comment"),
        "{}",
        out(&preview)
    );
}

#[test]
fn several_samples_written_are_said_once() {
    // Each sample written said `written: X`, where a write of several says
    // how many, once.
    let scratch = Scratch::new("written-once");
    scratch.corpus();
    let written = at(
        scratch.path(),
        &["set", "-f", "status == approved", "diameter=13", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        out(&written).contains("written: 3 files"),
        "{}",
        out(&written)
    );
    assert_eq!(
        out(&written).matches("written:").count(),
        1,
        "{}",
        out(&written)
    );
}

#[test]
fn a_summary_of_values_far_apart_is_a_number() {
    // Two finite values far apart overflowed the mean, said `-inf`, and an
    // exponential precision panicked on a number that is no number.
    let scratch = Scratch::new("summary-overflow");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  x: 1.5e+308\n---\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: b\nproperties:\n  x: -1.5e+308\n---\n",
    );
    let summary = at(
        scratch.path(),
        &[".", "-c", "x", "--summary", "--precision", ".2e"],
    );
    assert_eq!(code(&summary), 0, "{}", err(&summary));
    // The mean and the median are 0; the spread, beyond the largest number,
    // is said infinite.
    assert!(
        out(&summary).contains("2/2   0.00e+00"),
        "{}",
        out(&summary)
    );
}

#[test]
fn a_write_from_above_is_kept_in_the_project_below() {
    // `samplekit . -f … set --write` at the root changed two samples of a
    // project below: kept in no history, they were then said *changed outside
    // SampleKit*.
    let scratch = Scratch::new("write-from-above");
    let root = folder_made_a_project(&scratch);
    let first = at(&root, &["sub/a.md", "set", "diameter=4", "--write"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let written = at(
        &root,
        &[".", "-f", "diameter < 5", "set", "diameter=7", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(!err(&written).contains("not kept"), "{}", err(&written));
    let log = out(&at(&root, &["sub", "log"]));
    let newest = log
        .lines()
        .find(|line| line.starts_with("1 "))
        .unwrap_or_default();
    assert!(
        newest.contains("set diameter=7 -f 'diameter < 5' --write"),
        "{log}"
    );
    assert!(newest.ends_with("a, b"), "{log}");
    // Its own `.samplekitrc` is kept as the folder made a project, not as a
    // change outside SampleKit.
    assert!(!log.contains("outside SampleKit"), "{log}");
    assert!(
        log.contains("the folder made a project of its own"),
        "{log}"
    );
}

#[test]
fn a_field_other_samples_hold_is_absent_from_one_without_it() {
    // A sample made empty, named alone: `-s style` and an export naming
    // `style` said *unknown field* of what every other sample holds.
    let scratch = Scratch::new("absent-from-one");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.overview]\ncolumns = [{field = \"name\"}, {field = \"style\"}]\n\
         sort = [\"style\"]\n[export.overview]\nprofile = \"overview\"\nformat = \"csv\"\n\
         output = \"out/overview.csv\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nstyle: nickel\n---\n",
    );
    scratch.write("b.md", "---\nschema_version: 1\n---\n");
    let sorted = at(scratch.path(), &["b.md", "-c", "name,style", "-s", "style"]);
    assert_eq!(code(&sorted), 0, "{}", err(&sorted));
    assert!(out(&sorted).contains('—'), "{}", out(&sorted));
    let exported = at(scratch.path(), &["b.md", "export", "overview", "--write"]);
    assert_eq!(code(&exported), 0, "{}", err(&exported));
    let written = fs::read_to_string(scratch.path().join("out/overview.csv")).unwrap();
    assert_eq!(written.lines().nth(1), Some("b,"), "{written}");
    // A name no sample holds is still refused.
    let refused = at(scratch.path(), &["b.md", "-c", "name,stlye"]);
    assert_ne!(code(&refused), 0);
    assert!(
        err(&refused).contains("did you mean: 'style'"),
        "{}",
        err(&refused)
    );
}

#[test]
fn a_csv_says_an_edited_or_failed_value_in_a_state_column() {
    // A value edited over its formula had no `_state` column where nothing
    // else in its column was late, and the preview of `-o` said nothing of a
    // failed one.
    let scratch = Scratch::new("csv-states");
    scratch.corpus();
    scratch.write(
        "billet-01.sample.md",
        "---\nschema_version: 1\nname: billet-01\nstatus: approved\nproperties:\n  \
         diameter: {v: 12.1, u: 0.05, unit: g}\n  \
         porosity: {v: 2.801, unit: g/L, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    );
    let written = at(
        scratch.path(),
        &[".", "-c", "name,porosity", "-o", "e.csv", "--write"],
    );
    assert!(scratch.path().join("e.csv").is_file(), "{}", err(&written));
    let csv = fs::read_to_string(scratch.path().join("e.csv")).unwrap();
    let header = csv.lines().next().unwrap_or_default();
    assert!(header.contains("porosity_state"), "{csv}");
    assert!(
        csv.lines()
            .any(|line| line.starts_with("billet-01,") && line.ends_with(",edited")),
        "{csv}"
    );

    let scratch = Scratch::new("csv-failed");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  \
         hardness: {v: 1.02, computed: {radius: aaaaaaaaaaaa}, fingerprint: {failed: ZeroDivisionError}}\n---\n",
    );
    let previewed = at(scratch.path(), &[".", "-c", "name,hardness", "-o", "f.csv"]);
    assert!(
        out(&previewed).contains("1 failed in 1 sample"),
        "{}",
        out(&previewed)
    );
    let written = at(
        scratch.path(),
        &[".", "-c", "name,hardness", "-o", "f.csv", "--write"],
    );
    let csv = fs::read_to_string(scratch.path().join("f.csv")).unwrap();
    assert!(
        csv.starts_with("name,hardness_value,hardness_state\n"),
        "{}\n{csv}",
        err(&written)
    );
    assert!(csv.contains("a,1.02,failed"), "{csv}");
}

#[test]
fn log_of_a_selection_keeps_a_sample_that_left_it() {
    // `set … -f 'style == titanium' --write`, then one TITANIUM made a bronze alloy: `log -f
    // 'style == titanium'` said the first changed the others alone, and showed
    // nothing of the second.
    let scratch = Scratch::new("log-left-selection");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    for name in ["a", "b"] {
        scratch.write(
            &format!("{name}.md"),
            &format!("---\nschema_version: 1\nname: {name}\nstyle: titanium\n---\n"),
        );
    }
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nstyle: nickel\n---\n",
    );
    let set = at(
        scratch.path(),
        &[
            ".",
            "-f",
            "style == titanium",
            "set",
            "tester=lea",
            "--write",
        ],
    );
    assert_eq!(code(&set), 0, "{}", err(&set));
    let moved = at(
        scratch.path(),
        &["b.md", "set", "style=bronze_alloy", "--write"],
    );
    assert_eq!(code(&moved), 0, "{}", err(&moved));
    let log = at(scratch.path(), &[".", "log", "-f", "style == titanium"]);
    assert_eq!(code(&log), 0, "{}", err(&log));
    let listed = out(&log);
    assert!(
        listed
            .lines()
            .any(|line| line.contains("set style=bronze_alloy") && line.ends_with('b')),
        "{listed}"
    );
    assert!(
        listed
            .lines()
            .any(|line| line.contains("set tester=lea") && line.ends_with("a, b")),
        "{listed}"
    );
    assert!(!listed.contains("c\n"), "{listed}");
}

#[test]
fn diff_after_a_join_keeps_the_other_machines_changes_apart() {
    // A field of this machine's longer than the other's aligned every line
    // apart from what the other brought: all of it was said this machine's.
    let scratch = Scratch::new("diff-join-apart");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("a.md", "---\nschema_version: 1\nname: a\nvolume: 21\n---\n");
    scratch.write("h.md", "---\nschema_version: 1\nname: h\nmass: 5\n---\n");
    for (machine, arguments) in [
        ("ana", vec!["h.md", "set", "mass=4.9", "--write"]),
        ("bob", vec!["a.md", "set", "volume=22", "--write"]),
        ("bob", vec!["a.md", "tag", "add", "summer", "--write"]),
        ("ana", vec!["a.md", "set", "recovery=81", "--write"]),
    ] {
        let done = as_machine(scratch.path(), machine, &arguments);
        assert_eq!(code(&done), 0, "{arguments:?}: {}", err(&done));
    }
    let diff = as_machine(scratch.path(), "ana", &["diff"]);
    let said = out(&diff);
    let (own, came) = said.split_once("came with the join").expect(&said);
    assert!(own.contains("recovery"), "{said}");
    assert!(!own.contains("volume") && !own.contains("summer"), "{said}");
    assert!(came.contains("volume") && came.contains("summer"), "{said}");
}

#[test]
#[cfg(unix)]
fn init_installs_with_uv_into_an_environment_uv_made() {
    // An environment uv made holds no pip: uv installs into it where it is on
    // the PATH, and the command to type is said where it is not.
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("init-uv");
    let executable = |path: &Path, body: &str| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let project = scratch.path().join("project");
    executable(
        &project.join(".venv/bin/python"),
        "#!/bin/sh\necho 'python: No module named pip' >&2\nexit 1\n",
    );
    let called = scratch.path().join("uv-called");
    executable(
        &scratch.path().join("bin/uv"),
        &format!("#!/bin/sh\necho \"$@\" > '{}'\n", called.display()),
    );
    let init = |path: &str| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["init", "--write"])
            .current_dir(&project)
            .env("PATH", path)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let with_uv = init(&format!(
        "{}:/usr/bin:/bin",
        scratch.path().join("bin").display()
    ));
    let said = fs::read_to_string(&called).unwrap_or_default();
    assert!(
        said.starts_with("pip install --quiet --python") && said.contains("samplekit=="),
        "{said}\n{}",
        err(&with_uv)
    );
    for made in [".samplekitrc", "model", "samples"] {
        let path = project.join(made);
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            let _ = fs::remove_file(path);
        }
    }
    let without = init("/usr/bin:/bin");
    assert!(
        err(&without).contains("install it by hand: uv pip install --python .venv/bin/python"),
        "{}",
        err(&without)
    );
}

#[test]
fn init_names_the_completion_line_of_powershell() {
    // PowerShell sets no SHELL: init said to find the bridge oneself.
    let scratch = Scratch::new("init-powershell");
    let preview = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["init", "--no-venv"])
        .current_dir(scratch.path())
        .env_remove("SHELL")
        .env("PSModulePath", "C:\\Program Files\\PowerShell\\Modules")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out(&preview).contains(
            "add to your PowerShell profile, $PROFILE:  samplekit completions powershell | \
             Out-String | Invoke-Expression"
        ),
        "{}",
        out(&preview)
    );
}

// ------------------------------------------------------- saving what is shown

#[test]
fn save_previews_the_lines_it_adds_and_writes_nothing() {
    let scratch = Scratch::new("save-preview");
    scratch.corpus();
    let before = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    let shown = at(
        scratch.path(),
        &["-f", "porosity > 2.8015", "--save", "query=dense"],
    );
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert!(
        out(&shown).starts_with("save in .samplekitrc\n"),
        "{}",
        out(&shown)
    );
    assert!(out(&shown).contains("[query.dense]"), "{}", out(&shown));
    assert!(
        out(&shown).contains("filter = \"porosity > 2.8015\""),
        "{}",
        out(&shown)
    );
    assert_eq!(
        fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap(),
        before
    );
}

#[test]
fn a_saved_query_selects_as_the_filters_shown() {
    let scratch = Scratch::new("save-query");
    scratch.corpus();
    let saved = at(
        scratch.path(),
        &[
            "--query",
            "approved",
            "-f",
            "porosity > 2.8015",
            "--save",
            "query=dense",
            "--write",
        ],
    );
    assert_eq!(code(&saved), 0, "{}", err(&saved));
    assert_eq!(
        out(&saved),
        "written: .samplekitrc\nquery dense declared — --query dense selects with it\n"
    );
    let config = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    assert!(
        config.contains("filter = '(porosity > 2.8015) && (status == \"approved\")'")
            || config.contains("filter = \"(porosity > 2.8015) && (status == \\\"approved\\\")\""),
        "{config}"
    );
    let selected = at(scratch.path(), &["--query", "dense"]);
    assert_eq!(code(&selected), 0, "{}", err(&selected));
    assert_eq!(out(&selected).lines().count(), 2, "{}", out(&selected));
}

#[test]
fn a_saved_profile_shows_the_columns_sort_and_groups_shown() {
    let scratch = Scratch::new("save-profile");
    scratch.corpus();
    let saved = at(
        scratch.path(),
        &[
            "-c",
            "name,porosity:.2f=B",
            "-s",
            "-diameter",
            "-r",
            "--group",
            "status",
            "--save",
            "profile=mine",
            "--write",
        ],
    );
    assert_eq!(code(&saved), 0, "{}", err(&saved));
    let config = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    let declared = config.split("[profile.mine]").nth(1).unwrap_or_default();
    assert!(declared.contains("{ field = \"name\" }"), "{config}");
    assert!(
        declared.contains("field = \"porosity\", label = \"B\""),
        "{config}"
    );
    assert!(declared.contains("precision = \".2f\""), "{config}");
    // `-r` turned the sort, as the table sorted.
    assert!(declared.contains("sort = [\"diameter\"]"), "{config}");
    assert!(declared.contains("group = [\"status\"]"), "{config}");
    // The profile shows the table the command showed.
    let asked = at(
        scratch.path(),
        &[
            "-c",
            "name,porosity:.2f=B",
            "-s",
            "-diameter",
            "-r",
            "--group",
            "status",
        ],
    );
    let shown = at(scratch.path(), &["--profile", "mine"]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert_eq!(out(&shown), out(&asked));
    assert_eq!(order(&out(&shown)).len(), 4, "{}", out(&shown));
}

#[test]
fn a_saved_export_declares_its_query_profile_and_file() {
    let scratch = Scratch::new("save-export");
    scratch.corpus();
    let saved = at(
        scratch.path(),
        &[
            "-c",
            "name,diameter",
            "--query",
            "approved",
            "--tsv",
            "--save",
            "export=approved_diameters",
            "--write",
        ],
    );
    assert_eq!(code(&saved), 0, "{}", err(&saved));
    let config = fs::read_to_string(scratch.path().join(".samplekitrc")).unwrap();
    for line in [
        "[query.approved_diameters]",
        "[profile.approved_diameters]",
        "[export.approved_diameters]",
        "query = \"approved_diameters\"",
        "profile = \"approved_diameters\"",
        "format = \"tsv\"",
        "output = \"out/approved_diameters.tsv\"",
    ] {
        assert!(config.contains(line), "{line}\n{config}");
    }
    let written = at(scratch.path(), &["export", "approved_diameters", "--write"]);
    assert_eq!(code(&written), 0, "{}", err(&written));
    let file = fs::read_to_string(scratch.path().join("out/approved_diameters.tsv")).unwrap();
    assert_eq!(file.lines().count(), 4, "{file}");
}

#[test]
fn save_refuses_a_name_declared_already() {
    let scratch = Scratch::new("save-taken");
    scratch.corpus();
    let refused = at(
        scratch.path(),
        &["-c", "diameter", "--save", "profile=ductilities"],
    );
    assert_eq!(code(&refused), 1);
    assert_eq!(
        err(&refused),
        "error: a profile named ductilities is declared already: another name\n"
    );
    // An export declares its profile under its own name too.
    let refused = at(
        scratch.path(),
        &["-c", "diameter", "--save", "export=ductilities"],
    );
    assert_eq!(code(&refused), 1);
    assert!(
        err(&refused).contains("a profile named ductilities"),
        "{}",
        err(&refused)
    );
}

#[test]
fn save_refuses_what_it_could_not_declare() {
    let scratch = Scratch::new("save-refused");
    scratch.corpus();
    for (arguments, said) in [
        (
            &["--save", "query=all"][..],
            "error: query all would select every sample: a filter first\n",
        ),
        (
            &["-f", "diameter > 10", "--save", "profile=p"][..],
            "error: a profile saves the columns shown: name them with --columns or --profile\n",
        ),
        (&["-c", "diameter", "--save", "profile=two words"][..], ""),
    ] {
        let refused = at(scratch.path(), arguments);
        assert_eq!(code(&refused), 1, "{arguments:?}");
        if said.is_empty() {
            assert!(
                err(&refused).starts_with("error: 'two words' is no name"),
                "{}",
                err(&refused)
            );
        } else {
            assert_eq!(err(&refused), said, "{arguments:?}");
        }
    }
    let refused = at(scratch.path(), &["-c", "diameter", "--save", "figure=f"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused)
            .contains("'figure' is not saved: KIND=NAME, KIND one of query, profile or export"),
        "{}",
        err(&refused)
    );
    let refused = at(
        scratch.path(),
        &["-c", "diameter", "--style", "math", "--save", "export=e"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(
        err(&refused).starts_with("error: --save export= saves a csv, tsv or json export"),
        "{}",
        err(&refused)
    );
    let refused = at(
        scratch.path(),
        &["-c", "diameter", "--save", "profile=p", "-o", "t.csv"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
}

// ------------------------------------------------------------ removing samples

#[test]
fn remove_previews_the_files_it_would_delete() {
    let scratch = Scratch::new("remove-preview");
    scratch.corpus();
    let shown = at(scratch.path(), &["-f", "status == rejected", "remove"]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert_eq!(
        out(&shown),
        "remove · 1 sample file\n\n  billet-04.sample.md\n\n\
         samplekit restore gives it back, as it was\n\nnothing written\n"
    );
    assert!(scratch.path().join("billet-04.sample.md").is_file());
}

#[test]
fn removed_samples_are_kept_in_the_history_and_restored() {
    let scratch = Scratch::new("remove-write");
    scratch.corpus();
    let removed = at(
        scratch.path(),
        &[
            "billet-03.sample.md",
            "billet-04.sample.md",
            "remove",
            "--write",
        ],
    );
    assert_eq!(code(&removed), 0, "{}", err(&removed));
    assert_eq!(
        out(&removed),
        "removed: billet-03.sample.md\nremoved: billet-04.sample.md\n\
         samplekit restore gives them back\n"
    );
    assert!(!scratch.path().join("billet-03.sample.md").exists());
    let log = at(scratch.path(), &["log"]);
    assert!(out(&log).contains("remove --write"), "{}", out(&log));
    let restored = at(scratch.path(), &["restore", "--write"]);
    assert_eq!(code(&restored), 0, "{}", err(&restored));
    assert!(scratch.path().join("billet-03.sample.md").is_file());
    assert!(scratch.path().join("billet-04.sample.md").is_file());
}

#[test]
fn remove_refuses_a_whole_folder_or_nothing_named() {
    let scratch = Scratch::new("remove-refused");
    scratch.corpus();
    let refused = at(scratch.path(), &["remove", "--write"]);
    assert_eq!(code(&refused), 1);
    assert_eq!(
        err(&refused),
        "error: remove wants the samples to delete: name their files, or narrow a folder \
         with -f or --query\n"
    );
    let refused = at(scratch.path(), &[".", "remove", "--write"]);
    assert_eq!(code(&refused), 1);
    assert!(
        err(&refused).contains("remove deletes no folder whole"),
        "{}",
        err(&refused)
    );
    let none = at(
        scratch.path(),
        &["-f", "diameter > 100", "remove", "--write"],
    );
    assert_eq!(code(&none), 0, "{}", err(&none));
    assert_eq!(out(&none), "nothing to change — no sample is selected\n");
    assert_eq!(
        fs::read_dir(scratch.path())
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "md"))
            .count(),
        4
    );
}

#[test]
fn set_note_replaces_the_note_and_an_empty_one_empties_it() {
    let scratch = Scratch::new("set-note");
    scratch.corpus();
    let file = scratch.path().join("billet-01.sample.md");
    let written = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "note=Cracked at the weld.",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        out(&written).contains("note   Metals notes for billet-01.  →  Cracked at the weld."),
        "{}",
        out(&written)
    );
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with("---\nCracked at the weld.\n"), "{text}");
    // A note, never an attribute named note.
    assert!(!text.contains("note:"), "{text}");
    let emptied = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "note=", "--write"],
    );
    assert_eq!(code(&emptied), 0, "{}", err(&emptied));
    assert!(fs::read_to_string(&file).unwrap().ends_with("---\n"));
}

#[test]
fn a_write_selecting_no_sample_says_nothing_to_change() {
    let scratch = Scratch::new("selects-none");
    scratch.corpus();
    for arguments in [
        &["-f", "diameter > 100", "set", "status=kept", "--write"][..],
        &["-f", "diameter > 100", "tag", "add", "checked", "--write"][..],
        &["-f", "diameter > 100", "remove", "--write"][..],
    ] {
        let none = at(scratch.path(), arguments);
        assert_eq!(code(&none), 0, "{arguments:?}: {}", err(&none));
        assert_eq!(
            out(&none),
            "nothing to change — no sample is selected\n",
            "{arguments:?}"
        );
    }
}

#[test]
fn set_writes_a_markdown_link_as_text_quoted() {
    let scratch = Scratch::new("set-markdown-link");
    scratch.corpus();
    let written = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "twin=[the second](billet-02.sample.md)",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let text = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
    assert!(
        text.contains("twin: \"[the second](billet-02.sample.md)\"\n"),
        "{text}"
    );
    // A list typed by hand is still refused as one.
    let refused = at(
        scratch.path(),
        &["billet-01.sample.md", "set", "diameter=[12, 13]"],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
}

#[test]
fn a_filter_selects_the_samples_linking_to_one() {
    let scratch = Scratch::new("filter-links-to");
    scratch.corpus();
    let linked = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "twin=[the second](billet-02.sample.md)",
            "parents=[[billet-03]],[[billet-02]]",
            "--write",
        ],
    );
    assert_eq!(code(&linked), 0, "{}", err(&linked));
    let names = |filter: &str| {
        let output = at(scratch.path(), &["-f", filter, "-c", "name"]);
        assert_eq!(code(&output), 0, "{filter}: {}", err(&output));
        out(&output)
            .lines()
            .map(|line| line.trim_matches(|c: char| c == '│' || c.is_whitespace()))
            .filter(|line| line.starts_with("billet-"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("links to billet-02"), ["billet-01"]);
    assert_eq!(names("parents links to billet-03"), ["billet-01"]);
    assert_eq!(names("twin links to 'billet-03'"), Vec::<String>::new());
    assert_eq!(names("!(links to billet-02)").len(), 3);
    // As Obsidian writes a link: by the end of a path, with its extension,
    // in another case.
    let obsidian = at(
        scratch.path(),
        &[
            "billet-03.sample.md",
            "set",
            "after=[[more/Billet-01.md]]",
            "--write",
        ],
    );
    assert_eq!(code(&obsidian), 0, "{}", err(&obsidian));
    assert_eq!(names("links to billet-01"), ["billet-03"]);
}

#[cfg(target_os = "linux")]
#[test]
fn a_terminal_shows_links_by_their_text_and_a_pipe_as_written() {
    let scratch = Scratch::new("links-shown");
    scratch.corpus();
    let linked = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "twin=[[billet-02|the second]]",
            "standard=[the norm](https://example.org/norm)",
            "lost=[[billet-99]]",
            "--write",
        ],
    );
    assert_eq!(code(&linked), 0, "{}", err(&linked));
    for arguments in [
        &["billet-01.sample.md", "-c", "name,twin,standard,lost"][..],
        &["billet-01.sample.md", "view"][..],
    ] {
        let drawn = out(&at_terminal(scratch.path(), arguments, ""));
        assert!(drawn.contains("the second"), "{arguments:?}: {drawn}");
        assert!(!drawn.contains("[[billet-02"), "{arguments:?}: {drawn}");
        assert!(drawn.contains("the norm ↗"), "{arguments:?}: {drawn}");
        assert!(drawn.contains("billet-99 ✗"), "{arguments:?}: {drawn}");
        let piped = at(scratch.path(), arguments);
        assert_eq!(code(&piped), 0, "{}", err(&piped));
        assert!(
            out(&piped).contains("[[billet-02|the second]]"),
            "{}",
            out(&piped)
        );
        assert!(!out(&piped).contains('↗'), "{}", out(&piped));
    }
    let coloured = out(&in_colour(scratch.path(), &["billet-01.sample.md", "view"]));
    assert!(
        coloured.contains("\u{1b}[4m"),
        "a link is underlined: {coloured:?}"
    );
}

#[test]
fn paths_written_with_backslashes_read_on_every_system() {
    // A .samplekitrc and a link written on Windows: `\` read as a separator,
    // as `/` is, where Linux and macOS took it for part of a file's name.
    let scratch = Scratch::new("backslashes");
    scratch.write(
        "shared/.samplekitrc",
        "schema_version = 1\n[query.big]\nfilter = \"size > 1\"\n",
    );
    scratch.write(
        "project/.samplekitrc",
        "schema_version = 1\nimport = '..\\shared'\n[collection]\nfiles = ['machine\\{name}\\*']\n\
         [profile.all]\ncolumns = [{ field = \"name\" }]\n\
         [export.table]\nprofile = \"all\"\nformat = \"csv\"\noutput = 'out\\table.csv'\n",
    );
    scratch.write(
        "project/samples/s1.md",
        "---\nschema_version: 1\nname: s1\nsize: 2\nraw: '[curve](..\\machine\\s1\\curve.csv)'\n---\n",
    );
    scratch.write("project/machine/s1/curve.csv", "1,2\n");
    let project = scratch.path().join("project");
    let checked = at(&project, &["samples", "validate"]);
    assert_eq!(code(&checked), 0, "{}{}", out(&checked), err(&checked));
    let files = at(&project, &["list", "files"]);
    assert!(
        out_slashed(&files).contains("machine/s1/curve.csv"),
        "{}",
        out(&files)
    );
    let queried = at(&project, &["samples", "--query", "big"]);
    assert!(
        out_slashed(&queried).contains("samples/s1.md"),
        "{}{}",
        out(&queried),
        err(&queried)
    );
    let exported = at(&project, &["export", "table", "--write"]);
    assert_eq!(code(&exported), 0, "{}", err(&exported));
    assert!(project.join("out").join("table.csv").exists());
}

#[test]
fn view_shows_one_table_and_writes_its_rows() {
    let scratch = Scratch::new("view-table");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let table = |name: &str, first: f64| {
        format!(
            "---\nschema_version: 1\nname: {name}\ntables:\n  heating:\n    index: [T]\n    \
             columns:\n      T: {{unit: degC}}\n      hrc: {{}}\n    rows:\n      \
             - {{T: 20, hrc: {first}}}\n      - {{T: 30, hrc: {{v: 0.2, u: 0.01}}}}\n---\n"
        )
    };
    scratch.write("p1.md", &table("p1", 0.1));
    scratch.write("p2.md", &table("p2", 0.3));
    let shown = at(scratch.path(), &["p1.md", "view", "heating"]);
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    assert!(
        out(&shown).starts_with("p1\n  heating · 2 rows\n"),
        "{}",
        out(&shown)
    );
    let csv = at(scratch.path(), &["p1.md", "view", "heating", "--csv"]);
    assert_eq!(code(&csv), 0, "{}", err(&csv));
    assert_eq!(
        out(&csv),
        "T_value [degC],hrc_value,hrc_uncertainty\n20,0.1,\n30,0.2,0.01\n"
    );
    assert!(
        err(&csv).contains("T_uncertainty is left out: it is empty in every row"),
        "{}",
        err(&csv)
    );
    // Over several samples, each row says whose it is.
    let both = at(scratch.path(), &["view", "heating", "--csv"]);
    assert_eq!(code(&both), 0, "{}", err(&both));
    let printed = out(&both);
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines[0], "name,T_value [degC],hrc_value,hrc_uncertainty");
    assert_eq!(lines[1], "p1,20,0.1,");
    assert_eq!(lines[3], "p2,20,0.3,");
    // A document, previewed, then written and kept in the history.
    let preview = at(
        scratch.path(),
        &["p1.md", "view", "heating", "-o", "heating.md"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("table · heating.md"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.path().join("heating.md").exists());
    let written = at(
        scratch.path(),
        &["p1.md", "view", "heating", "-o", "heating.md", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let document = fs::read_to_string(scratch.path().join("heating.md")).unwrap();
    assert!(document.starts_with("| T (degC) | hrc |"), "{document}");
    assert!(document.contains("| 30 | 0.2 ± 0.01 |"), "{document}");
    let explained = at(scratch.path(), &["heating.md", "explain"]);
    assert!(
        out(&explained).contains("samplekit p1.md view heating -o heating.md --write"),
        "{}",
        out(&explained)
    );
    // A word that is neither a file nor a table, and a format with no table.
    let unknown = at(scratch.path(), &["p1.md", "view", "heat"]);
    assert_eq!(code(&unknown), 1);
    assert!(
        err(&unknown).contains(
            "'heat' is no file here, and no table of the samples selected\n  tables: heating"
        ),
        "{}",
        err(&unknown)
    );
    let whole = at(scratch.path(), &["p1.md", "view", "--csv"]);
    assert_eq!(code(&whole), 1);
    assert!(
        err(&whole).contains("view writes one table: name it after view"),
        "{}",
        err(&whole)
    );
}

#[test]
fn remove_says_the_links_it_leaves_leading_nowhere() {
    let scratch = Scratch::new("remove-links");
    scratch.corpus();
    let linked = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "twin=[[billet-02]]",
            "parents=[[billet-03]],[the second](billet-02.sample.md)",
            "--write",
        ],
    );
    assert_eq!(code(&linked), 0, "{}", err(&linked));
    let preview = at(scratch.path(), &["billet-02.sample.md", "remove"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out(&preview);
    assert!(
        said.contains("2 links lead to it, and will lead nowhere:\n  billet-01.sample.md\n"),
        "{said}"
    );
    assert!(said.contains("twin     [[billet-02]]"), "{said}");
    assert!(
        said.contains("parents  [the second](billet-02.sample.md)"),
        "{said}"
    );
    assert!(!said.contains("billet-03]]"), "{said}");
    // Removed with the one it links from, no link is left behind.
    let together = at(
        scratch.path(),
        &["billet-02.sample.md", "billet-01.sample.md", "remove"],
    );
    assert!(
        !out(&together).contains("lead nowhere"),
        "{}",
        out(&together)
    );
    let written = at(
        scratch.path(),
        &["billet-02.sample.md", "remove", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(
        out(&written).contains("2 links now lead nowhere — samplekit validate lists them"),
        "{}",
        out(&written)
    );
    let held = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
    assert!(
        held.contains("twin: \"[[billet-02]]\""),
        "left as written: {held}"
    );
}

#[test]
fn rename_writes_again_the_links_to_the_files_it_moves() {
    let scratch = Scratch::new("rename-own-links");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"photos\", \"machine/{name}/*\"]\n",
    );
    scratch.write(
        "samples/p3.md",
        "---\nschema_version: 1\nname: p3\nraw: \"[the curve](../machine/p3/p3.csv)\"\n---\n",
    );
    scratch.write(
        "samples/s1.md",
        "---\nschema_version: 1\nname: s1\nseen: \"[top](../photos/p3-top.jpg)\"\n\
         near: \"[other](../photos/xp3-top.jpg)\"\n---\n",
    );
    scratch.write("photos/p3-top.jpg", "jpg");
    scratch.write("photos/xp3-top.jpg", "jpg");
    scratch.write("machine/p3/p3.csv", "1,2\n");
    let written = at(
        scratch.path(),
        &["samples/p3.md", "rename", "plate-03", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let samples = scratch.path().join("samples");
    let renamed = fs::read_to_string(samples.join("plate-03.md")).unwrap();
    assert!(
        renamed.contains("[the curve](../machine/plate-03/p3.csv)"),
        "the folder renamed, the file in it kept: {renamed}"
    );
    let other = fs::read_to_string(samples.join("s1.md")).unwrap();
    assert!(
        other.contains("[top](../photos/plate-03-top.jpg)"),
        "{other}"
    );
    assert!(other.contains("[other](../photos/xp3-top.jpg)"), "{other}");
    let checked = at(scratch.path(), &["samples", "validate"]);
    assert_eq!(code(&checked), 0, "{}{}", out(&checked), err(&checked));
}

#[test]
fn rename_keeps_each_links_own_form() {
    let scratch = Scratch::new("rename-forms");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("samples/p3.md", "---\nschema_version: 1\nname: p3\n---\n");
    scratch.write(
        "samples/s1.md",
        "---\nschema_version: 1\nname: s1\nwith: \"[[P3.md]]\"\n\
         placed: \"[[samples/p3#Notes|the notes]]\"\nheading: \"[p](p3.md#top)\"\n---\n",
    );
    let written = at(
        scratch.path(),
        &["samples/p3.md", "rename", "plate-03", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    let linking = fs::read_to_string(scratch.path().join("samples/s1.md")).unwrap();
    assert!(linking.contains("\"[[plate-03.md]]\""), "{linking}");
    assert!(
        linking.contains("\"[[samples/plate-03#Notes|the notes]]\""),
        "{linking}"
    );
    assert!(linking.contains("\"[p](plate-03.md#top)\""), "{linking}");
}

#[test]
fn rename_moves_a_sample_its_files_and_the_links_leading_to_it() {
    let scratch = Scratch::new("rename-sample");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"photos\", \"machine/{name}/*\"]\n",
    );
    scratch.write(
        "samples/p3.md",
        "---\nschema_version: 1\nname: p3\nalloy: steel\n---\nThe plate.\n",
    );
    scratch.write(
        "samples/s1.md",
        "---\nschema_version: 1\nname: s1\nfrom: \"[[p3|the plate]]\"\n\
         also: [\"[p](p3.md)\", \"[[s1]]\"]\n---\n",
    );
    scratch.write("photos/p3-top.jpg", "jpg");
    scratch.write("machine/p3/curve.csv", "1,2\n");
    let samples = scratch.path().join("samples");
    let preview = at(scratch.path(), &["samples/p3.md", "rename", "plate-03"]);
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    let said = out_slashed(&preview);
    assert!(
        said.contains("samples/p3.md · rename to plate-03.md")
            && said.contains("samples/p3.md  →  samples/plate-03.md"),
        "{said}"
    );
    assert!(
        said.contains("[[p3|the plate]]  →  [[plate-03|the plate]]"),
        "{said}"
    );
    assert!(said.contains("[p](p3.md)  →  [p](plate-03.md)"), "{said}");
    assert!(samples.join("p3.md").exists(), "a preview writes nothing");
    let written = at(
        scratch.path(),
        &["samples/p3.md", "rename", "plate-03", "--write"],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(!samples.join("p3.md").exists());
    let renamed = fs::read_to_string(samples.join("plate-03.md")).unwrap();
    assert!(renamed.contains("name: plate-03\n"), "{renamed}");
    assert!(renamed.contains("The plate.\n"), "{renamed}");
    let linking = fs::read_to_string(samples.join("s1.md")).unwrap();
    assert!(linking.contains("\"[[plate-03|the plate]]\""), "{linking}");
    assert!(linking.contains("\"[p](plate-03.md)\""), "{linking}");
    assert!(linking.contains("\"[[s1]]\""), "{linking}");
    assert!(scratch.path().join("photos/plate-03-top.jpg").exists());
    assert!(scratch.path().join("machine/plate-03/curve.csv").exists());
    assert!(
        out(&written).contains("rename p3 --write renames it back"),
        "{}",
        out(&written)
    );
    // A name taken, and a folder, are refused.
    let taken = at(scratch.path(), &["samples/plate-03.md", "rename", "s1"]);
    assert_eq!(code(&taken), 1, "{}", err(&taken));
    assert!(
        err(&taken).contains("another sample has that name"),
        "{}",
        err(&taken)
    );
    let folder = at(scratch.path(), &["samples", "rename", "x"]);
    assert_eq!(code(&folder), 1, "{}", err(&folder));
}

#[test]
fn set_writes_links_typed_one_after_another_as_a_list() {
    let scratch = Scratch::new("set-link-list");
    scratch.corpus();
    for typed in [
        "parents=[[billet-02]],[the third](billet-03.sample.md)",
        "parents=[ [[billet-02]], [the third](billet-03.sample.md) ]",
    ] {
        let written = at(
            scratch.path(),
            &["billet-01.sample.md", "set", typed, "--write"],
        );
        assert_eq!(code(&written), 0, "{}", err(&written));
        let text = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
        assert!(
            text.contains(
                "parents:\n  - \"[[billet-02]]\"\n  - \"[the third](billet-03.sample.md)\"\n"
            ) || text
                .contains("parents: [\"[[billet-02]]\", \"[the third](billet-03.sample.md)\"]"),
            "{typed}: {text}"
        );
    }
    // One link alone is one text.
    let one = at(
        scratch.path(),
        &[
            "billet-01.sample.md",
            "set",
            "twin=[[billet-02]]",
            "--write",
        ],
    );
    assert_eq!(code(&one), 0, "{}", err(&one));
    let text = fs::read_to_string(scratch.path().join("billet-01.sample.md")).unwrap();
    assert!(text.contains("twin: \"[[billet-02]]\"\n"), "{text}");
}

#[test]
fn powershell_completion_completes_the_empty_word_after_a_space() {
    let scratch = Scratch::new("powershell-completion");
    scratch.corpus();
    let bridge = at(scratch.path(), &["completions", "powershell"]);
    assert!(
        out(&bridge).contains("$commandAst.CommandElements")
            && out(&bridge).contains("_SAMPLEKIT_COMPLETE_EMPTY")
            // Nothing to complete is said, not left to PowerShell's file names.
            && out(&bridge).contains("nothing to complete here"),
        "{}",
        out(&bridge)
    );
    let complete = |words: &[&str], empty: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_samplekit"));
        command
            .arg("--")
            .args(words)
            .current_dir(scratch.path())
            .env("COMPLETE", "powershell")
            .env_remove("_CLAP_COMPLETE_INDEX");
        if empty {
            command.env("_SAMPLEKIT_COMPLETE_EMPTY", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(code(&output), 0, "{}", err(&output));
        out(&output)
            .lines()
            .map(|line| line.split('\t').next().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
    };
    // After a space, the profile's name; PowerShell dropped the empty word,
    // and `--profile` came back as its own completion, Tab after Tab.
    let names = complete(&["samplekit", "--profile"], true);
    assert_eq!(names, ["ductilities"], "{names:?}");
    assert!(!complete(&["samplekit", "list"], true).contains(&"list".to_string()));
    // The word typed is completed, the command after a path too.
    assert_eq!(
        complete(&["samplekit", "--profile", "d"], false),
        ["ductilities"]
    );
    assert!(complete(&["samplekit", ".", "sta"], false).contains(&"status".to_string()));
    // Under an alias, `Set-Alias sk samplekit`: registered for it too, and
    // the line it begins completed as the command's.
    assert!(
        out(&bridge).contains("Get-Alias -Definition 'samplekit'"),
        "{}",
        out(&bridge)
    );
    assert!(complete(&["sk", ".", "sta"], false).contains(&"status".to_string()));
    assert_eq!(complete(&["sk", "--profile", "d"], false), ["ductilities"]);
    // Where no profile is, nothing: which the bridge says.
    let elsewhere = Scratch::new("powershell-nothing");
    let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["--", "samplekit", "--profile"])
        .current_dir(elsewhere.path())
        .env("COMPLETE", "powershell")
        .env("_SAMPLEKIT_COMPLETE_EMPTY", "1")
        .env_remove("_CLAP_COMPLETE_INDEX")
        .output()
        .unwrap();
    assert_eq!(out(&output).trim(), "", "{}", out(&output));
}
