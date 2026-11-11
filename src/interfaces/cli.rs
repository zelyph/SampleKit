//! The `samplekit` binary.
//!
//! Argument parsing and dispatch, and nothing else: a behavior that exists only
//! here is a behavior the TUI and Python cannot reach. Every command below is a
//! call into Layer 5 or 6 plus a layout.
//!
//! ```text
//! samplekit                                    the TUI, in a terminal
//! samplekit [target] [options]                 paths, or an explicitly shaped table
//! samplekit list [kind] [target]               what is declared and addressable
//! samplekit status [target]                    derived values that are not current, and why
//! samplekit explain <file> <field>             where one number came from
//! samplekit view [target] [--note]             each sample whole, its tables unfolded
//! samplekit export <export> [target]           a configured dataset
//! samplekit validate [target]                  what is wrong, modifying nothing
//! samplekit tag add|remove|rename …            the one command that edits samples
//! samplekit completions <shell>                dynamic shell completion bridge
//! ```

use samplekit::config::configuration_edit::{self, ConfigurationEdit, Kind as Named};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Instant;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::engine::{
    ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Shells, Zsh};

use samplekit::collection::sample_list::{self as list, FieldWarning, SampleList};
use samplekit::collection::validation::{self as validation, Detail, Severity};
use samplekit::collection::{exports, named_queries, tagging};
use samplekit::config::model_runtime::{
    self as runtime, Availability, Event, ModelError, Reason, Request, Template, Worker,
};
use samplekit::config::profiles::{self, first_top_level, last_top_level, split_top_level};
use samplekit::config::project_config::{
    ColumnSpec, DeclarationKind, DeclaredIn, Export, FigureExport, Format, Profile, ProjectConfig,
};
use samplekit::core::formatting::Precision;
use samplekit::core::identifier::{self, Identifier};
use samplekit::core::table::RowAddress;
use samplekit::core::value::Value;
use samplekit::format::document::{self, Destination};
use samplekit::format::fingerprint::{self, Freshness};
use samplekit::format::schema::PrecisionSchema;
use samplekit::presentation::export_formats;
use samplekit::presentation::terminal_rendering::{self as render, Style, Target};
use samplekit::query::field_addressing::{self as fields, Channel, Field, Subject};
use samplekit::query::filter_language as filter;
use samplekit::query::ordering::{self, SortKey, SortSpec};

static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);
/// `-v`, for what is said only when asked.
static VERBOSE: AtomicBool = AtomicBool::new(false);
/// `--quiet`: no warning at all.
static QUIET: AtomicBool = AtomicBool::new(false);
/// `validate` reports each file not read itself: not said again as a
/// warning before its report.
static REPORTING_UNREAD: AtomicBool = AtomicBool::new(false);
/// stdout's reader has gone: nothing more is printed there, and the work goes
/// on.
static STDOUT_GONE: AtomicBool = AtomicBool::new(false);

/// stdout, where a closed reader stops the printing and not the work.
///
/// `samplekit list fields | head` closes the pipe after ten lines, and `println!`
/// panics on that. A pipeline stopping early is how pipelines work, not a
/// failure worth a backtrace.
///
/// The command is **not** ended there. It once was — `exit(0)` at the first
/// failed write — and `samplekit set … --write | head -1` then printed the
/// preview's first line, lost its reader on the second, and exited 0 having
/// written nothing: a preview comes before the write it describes. So the
/// output is dropped from then on, and the command runs to its own end, its
/// writes and its exit code included.
macro_rules! outln {
    () => {
        outln!("")
    };
    ($($argument:tt)*) => {{
        if !crate::STDOUT_GONE.load(std::sync::atomic::Ordering::Relaxed)
            && crate::to_stdout(&format!($($argument)*), true).is_err()
        {
            crate::STDOUT_GONE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }};
}

macro_rules! out {
    ($($argument:tt)*) => {{
        if !crate::STDOUT_GONE.load(std::sync::atomic::Ordering::Relaxed)
            && crate::to_stdout(&format!($($argument)*), false).is_err()
        {
            crate::STDOUT_GONE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }};
}

/// The standard output. **Into a pipe or a file, as it is**: a path holding
/// an escape or a NUL was printed without them — every escape sequence
/// stripped with the colours — and named a file that does not exist. **On a
/// terminal, a control character is written visibly**, `\x1b`, `\0`, but the
/// colours SampleKit paints: one sent raw could reset or rewrite the
/// terminal.
/// A preview answered yes, run again to write: what it says before its
/// outcome was said by the preview, and is not said twice.
static APPLYING: AtomicBool = AtomicBool::new(false);
/// Whether the rerun reached its outcome, `written…`, said from there on.
static OUTCOME_SAID: AtomicBool = AtomicBool::new(false);

fn to_stdout(text: &str, newline: bool) -> std::io::Result<()> {
    use std::io::{IsTerminal as _, Write as _};
    let text =
        if APPLYING.load(AtomicOrdering::Relaxed) && !OUTCOME_SAID.load(AtomicOrdering::Relaxed) {
            match text.find("written") {
                Some(at) if text[..at].trim().is_empty() || text[..at].ends_with('\n') => {
                    OUTCOME_SAID.store(true, AtomicOrdering::Relaxed);
                    &text[at..]
                }
                _ => return Ok(()),
            }
        } else {
            text
        };
    let ending = if newline { "\n" } else { "" };
    if !std::io::stdout().is_terminal() {
        let mut out = std::io::stdout().lock();
        out.write_all(text.as_bytes())?;
        return out.write_all(ending.as_bytes());
    }
    let mut out = anstream::stdout();
    let text = lines_painted(text, render::palette().enabled);
    write!(out, "{}{ending}", visible_controls(&text))
}

/// A line the model printed, written to stderr as the command line's are.
fn relay_to_stderr(line: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{}", to_stderr_painted(line));
}

/// What goes to stderr, its words of outcome and advice painted where stderr
/// is a terminal and nothing turned colour off.
fn to_stderr_painted(text: &str) -> std::borrow::Cow<'_, str> {
    use std::io::IsTerminal as _;
    let on = std::io::stderr().is_terminal() && COLOR_ENABLED.load(AtomicOrdering::Relaxed);
    lines_painted(text, on)
}

/// SampleKit's own words, in their colours on a terminal: `✓` and
/// `written:` as a value current, `nothing written` and a `note:` muted, as
/// what changes nothing; `error:` and
/// `warning:` in theirs, and advice muted: a line proposing a command,
/// `samplekit … compute --write   run the model on it`, and the end of a line
/// after ` — ` where it proposes one, `… would be computed — --write computes
/// and writes without asking`. Fixed forms of this command line's messages,
/// recognised by their place in the line; a line already painted is left.
fn lines_painted(text: &str, on: bool) -> std::borrow::Cow<'_, str> {
    if !on || text.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }
    let palette = render::palette();
    let styled = |text: &str, style: anstyle::Style| render::painted(text, style, true);
    let advises = |text: &str| {
        text.contains("samplekit ")
            || text.split_whitespace().any(|word| {
                word.starts_with("--")
                    && word.len() > 2
                    && word[2..].starts_with(char::is_alphabetic)
            })
    };
    // The advice at the end of what a line says, muted: after ` — ` where it
    // proposes a command or an option, after `: ` where it is a command.
    let tipped = |said: &str| -> String {
        if let Some((fact, tip)) = said.rsplit_once(" — ")
            && advises(tip)
        {
            format!("{fact} — {}", styled(tip, palette.muted))
        } else if let Some((fact, tip)) = said.rsplit_once(": ")
            && tip.starts_with("samplekit ")
        {
            format!("{fact}: {}", styled(tip, palette.muted))
        } else {
            said.to_string()
        }
    };
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            if line.contains('\u{1b}') {
                return line.to_string();
            }
            let indent = &line[..line.len() - line.trim_start().len()];
            let rest = line.trim_start();
            if let Some(after) = rest.strip_prefix("✓ ") {
                format!("{indent}{} {after}", styled("✓", palette.current))
            } else if let Some(after) = rest.strip_prefix("written:") {
                format!("{indent}{}{after}", styled("written:", palette.current))
            } else if rest.starts_with("nothing written")
                || rest.starts_with("note:")
                || (rest.starts_with("samplekit ") && !rest.starts_with("samplekit ["))
            {
                format!("{indent}{}", styled(rest, palette.muted))
            } else if let Some(after) = rest.strip_prefix("error:") {
                format!(
                    "{indent}{}{}",
                    styled("error:", palette.error),
                    tipped(after)
                )
            } else if let Some(after) = rest.strip_prefix("warning:") {
                format!(
                    "{indent}{}{}",
                    styled("warning:", palette.message),
                    tipped(after)
                )
            } else {
                format!("{indent}{}", tipped(rest))
            }
        })
        .collect();
    std::borrow::Cow::Owned(lines.join("\n"))
}

/// `text` with each control character but a tab and a line's end written
/// visibly, and the colour sequences — `ESC [ … m` — kept as they are.
fn visible_controls(text: &str) -> std::borrow::Cow<'_, str> {
    if !text
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
    {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut shown = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '\u{1b}' {
            // A colour: `ESC [`, digits and `;`, then `m`.
            let rest = &text[at + 1..];
            if let Some(body) = rest.strip_prefix('[') {
                let length = body
                    .find(|c: char| !(c.is_ascii_digit() || c == ';'))
                    .unwrap_or(body.len());
                if body[length..].starts_with('m') {
                    shown.push_str(&text[at..at + 2 + length + 1]);
                    for _ in 0..1 + length + 1 {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        if c.is_control() && c != '\t' && c != '\n' {
            match c {
                '\0' => shown.push_str("\\0"),
                c if (c as u32) < 0x100 => shown.push_str(&format!("\\x{:02x}", c as u32)),
                c => shown.push_str(&format!("\\u{{{:x}}}", c as u32)),
            }
        } else {
            shown.push(c);
        }
    }
    std::borrow::Cow::Owned(shown)
}

/// stderr, where a closed reader changes nothing.
///
/// `errln!` panics when the write fails, and `samplekit . -v 2>&1 | head -1`
/// makes it fail every time: the reader leaves after one line. The process then
/// died with 101 and **said nothing**, because the panic message goes to the
/// same closed stream — only `echo $?` showed it.
///
/// Like [`outln`], it never ends the command: failing to deliver the product
/// or the account of the work is no reason to stop the work or to change the
/// verdict it ends with.
macro_rules! errln {
    ($($argument:tt)*) => {{
        use std::io::Write as _;
        let said = format!($($argument)*);
        let _ = writeln!(std::io::stderr(), "{}", crate::to_stderr_painted(&said));
    }};
}

/// `eprint!` that never panics: a closed stderr drops the text.
macro_rules! errp {
    ($($argument:tt)*) => {{
        use std::io::Write as _;
        let mut err = std::io::stderr();
        let _ = write!(err, $($argument)*);
        let _ = err.flush();
    }};
}

// The commands, by what they do. Declared after the macros above, whose scope
// is textual, and each opens with `use super::*`: the types, the statics and
// what every command shares stay here. The paths are written out because this
// file is the binary's root, where `mod x;` looks beside it.
#[path = "cli/completion.rs"]
mod completion;
#[path = "cli/compute.rs"]
mod compute;
#[path = "cli/edit.rs"]
mod edit;
#[path = "cli/history.rs"]
mod history;
#[path = "cli/init.rs"]
mod init;
#[path = "cli/inspect.rs"]
mod inspect;
#[path = "cli/plot.rs"]
mod plot;
#[path = "cli/run.rs"]
mod run;
#[path = "cli/tables.rs"]
mod tables;

use completion::*;
use compute::*;
use edit::*;
use init::*;
use inspect::*;
use plot::*;
use run::RunFlags;
use tables::*;

// ------------------------------------------------------------------ failure

/// What went wrong, and which exit code says so.
///
/// 0 · it worked, including a query that legitimately matched nothing.
/// 1 · the question was malformed: bad arguments, a bad filter, an unknown name.
/// 2 · the data was: a file that will not parse, a formula that raised, a defect.
/// 3 · the filesystem was: a missing path, a refused destination.
/// 130 · interrupted, by the convention for SIGINT — the account of what was
///   kept is already said, so no `error:` line follows it.
struct Fail {
    code: u8,
    message: String,
}

impl Fail {
    fn usage(message: impl Into<String>) -> Fail {
        Fail {
            code: 1,
            message: message.into(),
        }
    }

    fn data(message: impl Into<String>) -> Fail {
        Fail {
            code: 2,
            message: message.into(),
        }
    }

    fn io(message: impl Into<String>) -> Fail {
        Fail {
            code: 3,
            message: message.into(),
        }
    }
}

/// A write that failed, told apart by **why**.
///
/// A file that could not be written because the filesystem refused — read-only,
/// permission denied, a directory in the way — is an I/O error, exit 3, the
/// same class as a missing path. Anything else about the document is a data
/// error, exit 2. They were one code, so a script could not tell *fix your
/// permissions* from *fix your data*.
fn write_failed(path: &Path, error: &document::DocumentError) -> Fail {
    let said = of_file(path, &error.to_string());
    match error {
        document::DocumentError::Io { .. } => Fail::io(said),
        _ => Fail::data(said),
    }
}

/// A reason, said of its file — once. Most reasons already begin with the
/// path they are about, and prefixing it again printed it twice:
/// `samples/A.md: samples/A.md: the file is read-only`.
fn of_file(path: &Path, reason: &str) -> String {
    let shown = path.display().to_string();
    if reason.starts_with(&shown) {
        reason.to_string()
    } else {
        format!("{shown}: {reason}")
    }
}

/// A file that could not be read, told apart by why, as a write is: a
/// permission refused is the filesystem's, exit 3; a file that will not parse
/// is the data's, exit 2.
fn read_failed(path: &Path, error: &document::DocumentError) -> Fail {
    if matches!(error, document::DocumentError::MissingFrontmatter) {
        return Fail::data(not_a_sample(path));
    }
    let said = of_file(path, &error.to_string());
    match error {
        document::DocumentError::Io { .. } => Fail::io(said),
        _ => Fail::data(said),
    }
}

/// A file named as a sample that is none: said as that, and as what the
/// project makes of it. The format's own words — *because something already
/// claimed it was a sample* — answered `samplekit .samplekitrc` as though the
/// configuration were a broken sample.
fn not_a_sample(path: &Path) -> String {
    let shown = path.display();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name == ".samplekitrc" {
        return format!("{shown} is the project's configuration, not a sample");
    }
    let mut said = if is_markdown(path) {
        format!("{shown} is not a sample: a sample's first line is ---, opening its frontmatter")
    } else {
        format!("{shown} is not a sample: a sample is a Markdown file whose first line is ---")
    };
    let beside = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if let Ok(Some(config)) = samplekit::config::project_config::load_for(beside) {
        let root = config.root();
        let whole = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        if !samplekit::config::discovery::matches(&whole, &root, config.collection()) {
            said.push_str(
                "\n  and the project leaves it out: [collection] in its .samplekitrc does not \
                 read it as one",
            );
        }
    }
    said
}

type Outcome = Result<(), Fail>;

/// The command on a stack of its own, as deep on every system: Windows gives
/// the main thread 1 MiB, where Linux gives 8, and a filter or a frontmatter
/// nested as deep as SampleKit reads overflowed it before it was refused.
fn main() -> ExitCode {
    const STACK: usize = 16 << 20;
    match std::thread::Builder::new()
        .name("samplekit".to_string())
        .stack_size(STACK)
        .spawn(command)
    {
        Ok(running) => match running.join() {
            Ok(code) => code,
            Err(panicked) => std::panic::resume_unwind(panicked),
        },
        // No thread to be had: the command on the stack there is.
        Err(_) => command(),
    }
}

fn command() -> ExitCode {
    // A generated shell bridge calls back into this binary with COMPLETE set.
    // Completion is read-only and must run before anything writes to stdout.
    if completion_starts_with_an_unknown_short_option() {
        return ExitCode::SUCCESS;
    }
    complete_environment();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // `samplekit pieces status`: the path first, as it is typed to be
    // completed from, is the command's target, as it is written after the
    // command; options written before the command are the command's.
    if let Some(said) = unquoted_filter(&arguments) {
        errln!("error: {said}");
        return ExitCode::from(1);
    }
    let arguments = match path_first(&arguments) {
        Some((moved, _, _)) => moved,
        None => arguments,
    };
    let mut outcome = run(&arguments);
    // A preview answered yes: the same command, written.
    if outcome.is_ok() && APPLY.load(AtomicOrdering::Relaxed) {
        APPLY.store(false, AtomicOrdering::Relaxed);
        let mut written = arguments.clone();
        written.push("--write".to_string());
        APPLYING.store(true, AtomicOrdering::Relaxed);
        outcome = run(&written);
        APPLYING.store(false, AtomicOrdering::Relaxed);
    }
    // What the history said and no write said yet — a trace that could not be
    // read, by a command that only read.
    for said in samplekit::config::version_control::notices() {
        warn(&said);
    }
    // A warning of one kind given several times is said once, as the command
    // ends.
    said_gathered();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(fail) => {
            // Every diagnostic on stderr, so that `samplekit … > out.csv`
            // produces clean data with the problem still visible.
            // A failure whose account was already given says nothing more:
            // an interruption printed *interrupted: …* and then `error:
            // interrupted`, the same thing twice.
            if !fail.message.is_empty() {
                errln!("error: {}", fail.message);
            }
            ExitCode::from(fail.code)
        }
    }
}

// ------------------------------------------------------------------ options

// The sections of a `--help` page, in the order it prints them. The `-h` page
// prints the same options without them, so an option's place among the fields
// below is its place on both pages: a section's options are declared together,
// and sections in this order.
const SELECTION: &str = "Selection";
const SHAPE: &str = "Shape";
const PRESENTATION: &str = "Presentation";
const OUTPUT: &str = "Output";
const GENERAL: &str = "General";

#[derive(Debug, Parser)]
#[command(
    name = "samplekit",
    version,
    about = "Sample data in plain Markdown: computed, current, tracked",
    long_about = "Sample data in plain Markdown: computed, current, tracked\n\nFind, show, check, compute, export and plot the samples of a project. Given files or a folder and no table options, samplekit prints the path of each sample selected; -c or --profile shows a table instead. Alone at a terminal, samplekit opens the TUI; elsewhere it prints this help.\n\nPaths and options may come before the command or after it: samplekit samples status and samplekit status samples read the same. The first word naming a command is the command, even where a file or a folder has its name: write that one status/ or ./status, or after the command.",
    args_conflicts_with_subcommands = true,
    // The path first, the command after it: as every example writes it.
    override_usage = "samplekit [TARGET] [OPTIONS]\n       samplekit [TARGET] <COMMAND> [OPTIONS]",
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Cli {
    #[command(flatten)]
    query: QueryArgs,
    /// No colour in messages (also NO_COLOR or TERM=dumb)
    // The global options close General on every page, after a command's own:
    // their place is fixed here, where a command with many options of its own
    // otherwise met them halfway down its list, `-h` among `--show-output`s.
    #[arg(long, global = true, help_heading = GENERAL, display_order = 1000)]
    no_color: bool,
    /// Use this .samplekitrc instead of the nearest one
    #[arg(long, global = true, value_name = "PATH", help_heading = GENERAL, value_hint = ValueHint::FilePath, display_order = 1001)]
    rc: Option<PathBuf>,
    /// Print the configuration and model a command would use
    #[arg(long, global = true, help_heading = GENERAL, display_order = 1002)]
    show_rc: bool,
    #[command(flatten)]
    at: AtArgs,
    /// Print help; --help for sections and details
    #[arg(
        short = 'h',
        long,
        global = true,
        action = clap::ArgAction::Help,
        help_heading = GENERAL,
        display_order = 1003,
        long_help = "Print help; -h for a compact summary"
    )]
    help: Option<bool>,
    /// Print version
    #[arg(short = 'V', long, action = clap::ArgAction::Version, help_heading = GENERAL, display_order = 1004)]
    version: Option<bool>,
    #[command(subcommand)]
    command: Option<Commands>,
}

/// What narrows a selection: every command on samples takes it.
#[derive(Debug, Args, Default, Clone)]
struct FilterArgs {
    /// Keep the samples an expression selects; repeatable
    #[arg(
        short = 'f',
        long = "filter",
        value_name = "EXPR",
        help_heading = SELECTION,
        long_help = "Keep the samples an expression selects. Repeatable: every --filter and every --query must hold.\n\nelongation > 20, material == steel, material == \"cast iron\", tensile_strength > yield_strength, material in (steel, copper), tags has reference, curve where (strain > 1 && stress < 300), elongation is missing. Combine with &&, || and ! and parentheses, and quote the whole expression for the shell. Every form: https://zelyph.github.io/SampleKit/latest/reference/filters.html",
        action = clap::ArgAction::Append,
        add = ArgValueCompleter::new(complete_filters)
    )]
    filters: Vec<String>,
    /// Keep the samples a saved query selects; repeatable
    #[arg(
        long = "query",
        value_name = "NAME",
        help_heading = SELECTION,
        long_help = "Keep the samples a query saved in .samplekitrc selects. Repeatable: every --filter and every --query must hold",
        action = clap::ArgAction::Append,
        add = ArgValueCompleter::new(complete_queries)
    )]
    queries: Vec<String>,
}

#[derive(Debug, Args, Default)]
struct SelectionArgs {
    #[command(flatten)]
    narrowing: FilterArgs,
    /// Sort by these fields; repeat or comma-separate
    #[arg(
        short = 's',
        long = "sort",
        value_name = "FIELDS",
        help_heading = SELECTION,
        long_help = "Sort by these fields, the first deciding first; repeat or comma-separate. A leading - sorts that field in descending order: -s material,-tensile_strength",
        value_parser = parse_sort_list,
        action = clap::ArgAction::Append,
        allow_hyphen_values = true,
        add = ArgValueCompleter::new(complete_sort_keys)
    )]
    sorts: Vec<SortList>,
    /// Reverse the sort
    #[arg(short = 'r', long, help_heading = SELECTION)]
    reverse: bool,
}

#[derive(Debug, Args, Default)]
struct RenderArgs {
    /// Draw a plain or boxed table
    #[arg(long, value_name = "STYLE", help_heading = PRESENTATION, group = "output-format", value_parser = parse_table_style, add = ArgValueCompleter::new(complete_table_styles))]
    table_style: Option<String>,
    /// Assume this terminal width
    #[arg(long, help_heading = PRESENTATION, value_parser = parse_width)]
    width: Option<usize>,
}

#[derive(Debug, Args, Default)]
struct PresentationArgs {
    /// Render in this declared style
    #[arg(
        long,
        value_name = "STYLE",
        help_heading = PRESENTATION,
        long_help = "Render in this style, declared in .samplekitrc, instead of [render] style: its unit spellings, its symbols and how it writes a property",
        add = ArgValueCompleter::new(complete_render_styles)
    )]
    style: Option<String>,
    /// Precision of every numeric column, such as .3f
    #[arg(
        long,
        value_name = "SPEC",
        help_heading = PRESENTATION,
        long_help = "Precision of every numeric column, such as .3f; a column's own FIELD:PRECISION wins",
        value_parser = parse_precision
    )]
    precision: Option<PrecisionSchema>,
}

/// `-v`, declared last wherever it appears so that General closes every page.
#[derive(Debug, Args, Default)]
struct VerboseArgs {
    /// Also say which files were skipped, and which configuration is used
    #[arg(short = 'v', long, help_heading = GENERAL, conflicts_with = "quiet")]
    verbose: bool,
    /// Print no warning
    #[arg(short = 'q', long, help_heading = GENERAL)]
    quiet: bool,
}

/// The columns one `-c` names, parsed by `profiles` so that Python's `columns=`
/// names the same ones.
#[derive(Debug, Clone)]
struct ColumnList(Vec<ColumnSpec>);

/// The keys one `-s` names, in order: `-s diameter,-porosity` is `-s diameter -s -porosity`.
#[derive(Debug, Clone)]
struct SortList(Vec<String>);

#[derive(Debug, Args, Default)]
#[command(group(clap::ArgGroup::new("writes").args(["output", "save"]).multiple(true)))]
struct QueryArgs {
    /// Sample files or folders; the current folder when an option is given
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Columns as FIELD[:PRECISION][=NAME]; repeat or comma-separate
    #[arg(short = 'c', long = "columns", value_name = "COLUMNS", help_heading = SHAPE, long_help = "Columns as FIELD[:PRECISION][=NAME]; repeat or comma-separate. NAME titles the column on the terminal and in a document, and names its columns in a CSV, TSV or JSON file: young_modulus=E writes E_value and E_uncertainty. A NAME holding a comma is quoted: young_modulus='Modulus, E'", value_parser = parse_column_list, action = clap::ArgAction::Append, add = ArgValueCompleter::new(complete_columns))]
    columns: Vec<ColumnList>,
    /// Show the table a profile declares; -c, -s, --group and --precision override their part
    #[arg(
        long,
        value_name = "NAME",
        help_heading = SHAPE,
        long_help = "Show the table a profile declared in .samplekitrc gives. It sets the columns, the order, the groups and the precision, and each option written beside it overrides its part: -c the columns, -s the order, --group the groups, --precision the digits",
        add = ArgValueCompleter::new(complete_profiles)
    )]
    profile: Option<String>,
    /// Summarise each column instead of listing the samples
    #[arg(long, help_heading = SHAPE)]
    summary: bool,
    /// One table per value of these fields, comma-separated, or per combination of values; with --summary, a summary per group
    #[arg(long, value_name = "FIELDS", help_heading = SHAPE, add = ArgValueCompleter::new(complete_fields))]
    group: Option<String>,
    #[command(flatten)]
    presentation: PresentationArgs,
    #[command(flatten)]
    render: RenderArgs,
    /// Print CSV instead of a table
    #[arg(long, help_heading = OUTPUT, group = "output-format")]
    csv: bool,
    /// Print TSV instead of a table
    #[arg(long, help_heading = OUTPUT, group = "output-format")]
    tsv: bool,
    /// Print JSON instead of a table
    #[arg(long, help_heading = OUTPUT, group = "output-format")]
    json: bool,
    /// Add a state column: what in each row is not current
    #[arg(long, help_heading = OUTPUT)]
    status: bool,
    /// Write to this file instead of printing; '-' prints
    #[arg(short = 'o', long, value_name = "PATH", help_heading = OUTPUT, value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::any().stdio()))]
    output: Option<PathBuf>,
    /// Save what the table shows in .samplekitrc: query=NAME, profile=NAME or export=NAME
    #[arg(
        long,
        value_name = "KIND=NAME",
        help_heading = OUTPUT,
        long_help = "Save what the table shows in the project's .samplekitrc, under NAME: query=NAME the filter, profile=NAME the columns, the sort and the groups, export=NAME both and an export written to out/NAME.csv (--tsv or --json for another format). Nothing is printed but the lines added; a name already declared is refused",
        value_parser = parse_save,
        conflicts_with_all = ["output", "summary", "status"]
    )]
    save: Option<SaveAs>,
    /// Write the --output file, or what --save adds; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT, requires = "writes")]
    write: bool,
    /// States a write from values not current may hold and exit 0: outdated, edited[:NAME,…], failed, never_computed
    #[arg(long, value_name = "STATES", help_heading = OUTPUT, value_parser = accepted_states, requires = "output")]
    accept: Option<Accepted>,
    #[command(flatten)]
    general: VerboseArgs,
}

/// `--save KIND=NAME`: what the table shows, declared under NAME.
#[derive(Debug, Clone)]
struct SaveAs {
    kind: Named,
    name: String,
}

fn parse_save(text: &str) -> Result<SaveAs, String> {
    let said = "KIND=NAME, KIND one of query, profile or export";
    let (kind, name) = text.split_once('=').ok_or_else(|| said.to_string())?;
    let kind = match kind.trim() {
        "query" => Named::Query,
        "profile" => Named::Profile,
        "export" => Named::Export,
        other => return Err(format!("'{other}' is not saved: {said}")),
    };
    Ok(SaveAs {
        kind,
        name: name.trim().to_string(),
    })
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List fields, tags and what .samplekitrc declares
    #[command(override_usage = "samplekit [TARGET] list [KIND] [OPTIONS]")]
    List(ListArgs),
    /// Show which computed values are not current, and why
    #[command(override_usage = "samplekit [TARGET] status [OPTIONS]")]
    Status(SelectedTargetArgs),
    /// Compute the values the project's model gives
    #[command(override_usage = "samplekit [TARGET] compute [OPTIONS]")]
    Compute(ComputeArgs),
    /// Explain where one value came from
    #[command(override_usage = "samplekit [TARGET] explain <FIELD> [OPTIONS]")]
    Explain(ExplainArgs),
    /// Open a sample's own files (images, reports) or the folder holding them
    #[command(override_usage = "samplekit [TARGET] open [PATTERN] [OPTIONS]")]
    Open(OpenArgs),
    /// Show each selected sample whole: its values, then its tables; --note adds the note
    #[command(override_usage = "samplekit [TARGET] view [OPTIONS]")]
    View(ViewArgs),
    /// Preview an export declared in .samplekitrc, a table or a figure; --write writes it
    #[command(override_usage = "samplekit [TARGET] export <EXPORT> [OPTIONS]")]
    Export(ExportArgs),
    /// Draw a figure with matplotlib, in a window or to a file
    #[command(
        override_usage = "samplekit [TARGET] plot <FIGURE> [OPTIONS]\n       samplekit [TARGET] plot -x <FIELD> -y <FIELD> [OPTIONS]"
    )]
    Plot(Box<PlotArgs>),
    /// Run a script the model declares on each sample, or the selection; what it changes is shown and asked
    #[command(override_usage = "samplekit [TARGET] run <SCRIPT> [OPTIONS]")]
    Run(RunArgs),
    /// Report the defects of sample files; nothing is changed
    #[command(override_usage = "samplekit [TARGET] validate [OPTIONS]")]
    Validate(TargetArgs),
    /// Add, remove or rename tags
    #[command(
        override_usage = "samplekit [TARGET] tag add <TAG> [OPTIONS]\n       samplekit [TARGET] tag remove <TAG> [OPTIONS]\n       samplekit [TARGET] tag rename <OLD> <NEW> [OPTIONS]"
    )]
    Tag(TagArgs),
    /// List the snapshots of the project's history
    #[command(override_usage = "samplekit [TARGET] log [OPTIONS]")]
    Log(LogArgs),
    /// Compare two states of the project, value by value
    #[command(override_usage = "samplekit [TARGET] diff [OPTIONS]")]
    Diff(DiffArgs),
    /// Put files back as the history kept them: the last change taken back, or a state named
    #[command(override_usage = "samplekit [TARGET] restore [OPTIONS]")]
    Restore(RestoreArgs),
    /// Delete sample files; the history keeps them, and restore gives them back
    #[command(override_usage = "samplekit [TARGET] remove [OPTIONS]")]
    Remove(TagTargetArgs),
    /// Rename a sample, its own files with it, and the links leading to it
    #[command(override_usage = "samplekit FILE rename <NEW> [OPTIONS]")]
    Rename(RenameArgs),
    /// Set up a project, or complete one that exists
    #[command(override_usage = "samplekit [DIRECTORY] init [OPTIONS]")]
    Init(InitArgs),
    /// Create a sample from what the project already knows
    #[command(override_usage = "samplekit [FOLDER] new <NAME> [FIELD=VALUE]... [OPTIONS]")]
    New(NewArgs),
    /// Change values of a sample
    #[command(override_usage = "samplekit [TARGET] set [FIELD=VALUE]... [OPTIONS]")]
    Set(SetArgs),
    /// Open the TUI, the full-screen interface, on a folder; `samplekit` alone opens its start page
    #[command(override_usage = "samplekit [DIRECTORY] tui [OPTIONS]")]
    Tui(TuiArgs),
    /// Generate a dynamic shell-completion bridge
    // Hidden until `init` installs completion; typed, it still runs.
    #[command(hide = true)]
    Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Category to enumerate (fields, tags, queries, profiles, exports,
    /// figures, scripts, files or skipped), or a target when used alone
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_list_kind_or_target))]
    kind_or_target: Option<String>,
    /// Sample files or folders, after a category; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

/// `--at`, on the commands that read the project as its history kept it, and on
/// `log`, whose list it starts at that snapshot.
#[derive(Debug, Args, Default)]
struct AtArgs {
    /// Read the project as it was at a snapshot of its history
    ///
    /// WHEN is a number from `samplekit log`, the beginning of a snapshot's id, or a date such as 2026-09-12 or '2026-09-12 14:30'.
    #[arg(long, value_name = "WHEN", help_heading = GENERAL, add = ArgValueCompleter::new(complete_when))]
    at: Option<String>,
}

#[derive(Debug, Args)]
struct SelectedTargetArgs {
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Stop the model's loading past this, in seconds; 60 by default
    #[arg(long, value_name = "SECONDS", value_parser = seconds, help_heading = SELECTION)]
    timeout: Option<std::time::Duration>,
    #[command(flatten)]
    render: RenderArgs,
    /// Exit 2 when anything is not current, for a check that must fail
    #[arg(
        long,
        help_heading = OUTPUT,
        long_help = "Exit 2 when any value is not current, as git diff --exit-code does: a check for continuous integration. A value waiting for an input nobody entered does not fail it. Without --exit-code, status exits 0 whatever it lists"
    )]
    exit_code: bool,
    /// With --exit-code, states that do not fail the check: edited, or edited:NAME,… for those values alone; waiting never does
    #[arg(long, value_name = "STATES", help_heading = OUTPUT, value_parser = accepted_states, requires = "exit_code")]
    accept: Option<Accepted>,
    /// Print JSON: one object per value not current
    #[arg(long, help_heading = OUTPUT)]
    json: bool,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct ComputeArgs {
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Compute only these values; repeat or comma-separate
    #[arg(
        short = 'p',
        long = "properties",
        value_name = "VALUES",
        help_heading = SELECTION,
        long_help = "Compute only these values (properties, or table columns as table.column) and the inputs they need. The values computed from them are left outdated, for a compute without -p. Repeat or comma-separate",
        value_delimiter = ',',
        action = clap::ArgAction::Append,
        add = ArgValueCompleter::new(complete_values)
    )]
    properties: Vec<String>,
    /// Also run values that are current
    #[arg(
        long,
        help_heading = SELECTION,
        long_help = "Also run values that are current: after a formula changed, or when one reads data outside the samples"
    )]
    rerun: bool,
    /// Also run edited values, giving them back to their formula
    #[arg(long, help_heading = SELECTION)]
    force: bool,
    /// Stop a formula that runs longer than this, in seconds: its value is failed, and the others run
    #[arg(long, value_name = "SECONDS", value_parser = seconds, help_heading = SELECTION)]
    timeout: Option<std::time::Duration>,
    // Presentation, before Output and General, as every page orders them.
    #[command(flatten)]
    render: RenderArgs,
    /// Compute and write the files, showing before and after, asking nothing
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    /// Show what the model prints as it prints it; it is always kept in the run's log
    #[arg(long, help_heading = OUTPUT)]
    show_output: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct TargetArgs {
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// Stop the model's loading past this, in seconds; 60 by default
    #[arg(long, value_name = "SECONDS", value_parser = seconds, help_heading = SELECTION)]
    timeout: Option<std::time::Duration>,
    /// Print JSON: one object per finding
    #[arg(long, help_heading = OUTPUT)]
    json: bool,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

/// What `status` answers with besides the listing.
struct StatusGate {
    /// Exit 2 when something is not current.
    exit_code: bool,
    /// The states that do not fail it: `edited`, of every value or of those
    /// named, and `waiting`, which never does.
    accept: Accepted,
    /// The listing as JSON.
    json: bool,
}

#[derive(Debug, Args)]
struct ExplainArgs {
    /// Sample files or folders, or files SampleKit wrote, and the field to explain: the one word naming no file
    #[arg(value_name = "TARGET|FIELD", add = ArgValueCompleter::new(complete_targets_or_fields))]
    words: Vec<String>,
    /// The paths written before the command, read by the command line before it parses.
    #[arg(long = "target", value_name = "PATH", hide = true)]
    before: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    #[command(flatten)]
    presentation: PresentationArgs,
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct TuiArgs {
    /// The folder to open; the current one by default
    #[arg(value_hint = ValueHint::DirPath, add = ArgValueCompleter::new(PathCompleter::dir()))]
    directory: Option<PathBuf>,
}

#[derive(Debug, Args)]
// `--at` is every reading command's, and reads the project as it was; `log`
// reads the history itself, and starts its list there.
#[command(mut_arg("at", |at| at
    .help("Start the list at this snapshot")
    .long_help("Start the list at this snapshot, the newer ones left out; the numbers stay those of the whole history\n\nWHEN is a number from `samplekit log`, the beginning of a snapshot's id, or a date such as 2026-09-12 or '2026-09-12 14:30'.")))]
struct LogArgs {
    /// Files or folders whose changes to list; the whole project by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// Print the Python script that made a snapshot, as it ran
    ///
    /// WHEN names the snapshot as --at does: a number from `samplekit log`, the beginning of its id, or a date.
    #[arg(long, value_name = "WHEN", add = ArgValueCompleter::new(complete_snapshot), conflicts_with = "export")]
    script: Option<String>,
    /// Write an account of the history in Markdown, to share beside the files
    ///
    /// Each snapshot with when, what made it, every file it changed and the script kept beside it. Printed, or written to -o FILE with --write.
    #[arg(long, help_heading = OUTPUT)]
    export: bool,
    /// Write the account to this file; '-' prints
    #[arg(short = 'o', long, value_name = "PATH", help_heading = OUTPUT, requires = "export", value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::any().stdio()))]
    output: Option<PathBuf>,
    /// Write the --output file, replacing it; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT, requires = "output")]
    write: bool,
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct RestoreArgs {
    /// Files or folders to put back; the whole project by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// The state to put them back to; by default, the last change SampleKit wrote on this computer is taken back, keeping what was changed since, or a file deleted or changed outside SampleKit comes back as last kept
    ///
    /// WHEN is a number from `samplekit log`, the beginning of a snapshot's id, or a date such as 2026-09-12 or '2026-09-12 14:30'.
    #[arg(long, value_name = "WHEN", add = ArgValueCompleter::new(complete_snapshot))]
    at: Option<String>,
    /// Write the files; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct DiffArgs {
    /// Files or folders to compare; the whole project by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// The state compared from; by default the snapshot before the one --to names, or before the last change kept
    ///
    /// WHEN is now, a number from `samplekit log`, the beginning of a snapshot's id, or a date such as 2026-09-12 or '2026-09-12 14:30'. Without --from or --to, diff shows the last change kept: that snapshot against the one before it.
    #[arg(long, value_name = "WHEN", add = ArgValueCompleter::new(complete_when))]
    from: Option<String>,
    /// The state compared to; now with --from alone, the last change kept without either
    ///
    /// WHEN is named as --from is.
    #[arg(long, value_name = "WHEN", add = ArgValueCompleter::new(complete_when))]
    to: Option<String>,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct OpenArgs {
    /// Sample files or folders, and a glob naming their files to open, such as '*.jpg': the one word naming no file; more than five ask first
    #[arg(value_name = "TARGET|PATTERN", add = ArgValueCompleter::new(complete_targets_or_sample_files))]
    words: Vec<String>,
    /// The paths written before the command, read by the command line before it parses.
    #[arg(long = "target", value_name = "PATH", hide = true)]
    before: Vec<PathBuf>,
    /// Open the folder holding them in the file manager instead
    #[arg(long)]
    navigate: bool,
    /// Open the sample's own file, with the application the system gives its kind
    #[arg(long, conflicts_with = "navigate")]
    edit: bool,
    #[command(flatten)]
    narrowing: FilterArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("view-writes").args(["output"])))]
struct ViewArgs {
    /// Sample files or folders, the current folder by default; then, alone, a table's name
    #[arg(
        value_hint = ValueHint::AnyPath,
        long_help = "Sample files or folders, the current folder by default. A word that is no file and names a table of the samples selected shows that table alone: samplekit samples/steel-01.md view curve",
        add = ArgValueCompleter::new(complete_targets)
    )]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    #[command(flatten)]
    presentation: PresentationArgs,
    #[command(flatten)]
    render: RenderArgs,
    /// Print each sample's note after its values and tables
    #[arg(long, help_heading = OUTPUT)]
    note: bool,
    /// Print a table named as CSV
    #[arg(long, help_heading = OUTPUT, group = "view-format")]
    csv: bool,
    /// Print a table named as TSV
    #[arg(long, help_heading = OUTPUT, group = "view-format")]
    tsv: bool,
    /// Print a table named as JSON
    #[arg(long, help_heading = OUTPUT, group = "view-format")]
    json: bool,
    /// Write a table named to this file, its extension naming the format; '-' prints
    #[arg(short = 'o', long, value_name = "PATH", help_heading = OUTPUT, value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::any().stdio()))]
    output: Option<PathBuf>,
    /// Write the --output file; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT, requires = "view-writes")]
    write: bool,
    /// States a write from values not current may hold and exit 0: outdated, edited[:NAME,…], failed, never_computed
    #[arg(long, value_name = "STATES", help_heading = OUTPUT, value_parser = accepted_states, requires = "output")]
    accept: Option<Accepted>,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct ExportArgs {
    /// An export declared in .samplekitrc
    #[arg(add = ArgValueCompleter::new(complete_exports))]
    export: String,
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Precision of every numeric column, such as .3f, instead of the declared one
    #[arg(
        long,
        value_name = "SPEC",
        help_heading = PRESENTATION,
        long_help = "Precision of every numeric column, such as .3f, instead of what the profile and the project declare: an export is written at the declared precision, and this asks for another one, or for more digits: --precision .6e",
        value_parser = parse_precision
    )]
    precision: Option<PrecisionSchema>,
    /// Spell the units in this style, plain or one declared, instead of as the files write them
    #[arg(
        long,
        value_name = "STYLE",
        help_heading = PRESENTATION,
        long_help = "Spell the units in this style instead of as the sample files write them: plain for the forms [unit.*] declares for the screen, a spreadsheet's headers in Unicode, or a style declared in .samplekitrc",
        add = ArgValueCompleter::new(complete_render_styles)
    )]
    style: Option<String>,
    /// Add a state column: what in each row is not current
    #[arg(long, help_heading = OUTPUT)]
    status: bool,
    /// States a write from values not current may hold and exit 0: outdated, edited[:NAME,…], failed, never_computed
    #[arg(long, value_name = "STATES", help_heading = OUTPUT, value_parser = accepted_states)]
    accept: Option<Accepted>,
    /// Write to this file instead of the declared one; '-' prints
    #[arg(short = 'o', long, value_name = "PATH", help_heading = OUTPUT, value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::any().stdio()))]
    output: Option<PathBuf>,
    /// Write the file, replacing it; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct PlotArgs {
    /// A figure .samplekitrc or the model declares; with -x and -y, a sample file or folder
    #[arg(add = ArgValueCompleter::new(complete_figures))]
    figure: Option<String>,
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// The field on the x axis, for a figure not declared: a property, an attribute, or a table's cell or column
    #[arg(short = 'x', long = "x", value_name = "FIELD", help_heading = SHAPE, requires = "y", add = ArgValueCompleter::new(complete_fields))]
    x: Option<String>,
    /// The field on the y axis
    #[arg(short = 'y', long = "y", value_name = "FIELD", help_heading = SHAPE, requires = "x", add = ArgValueCompleter::new(complete_fields))]
    y: Option<String>,
    /// One series per value of these fields, comma-separated, or per combination of values; replaces the figure's own
    #[arg(long, value_name = "FIELDS", help_heading = SHAPE, add = ArgValueCompleter::new(complete_fields))]
    group: Option<String>,
    /// scatter, line, step, bar or box; replaces the figure's own
    #[arg(long, value_name = "KIND", help_heading = SHAPE, value_parser = ["scatter", "line", "step", "bar", "box"])]
    kind: Option<String>,
    /// The figure's title, instead of its name
    #[arg(long, value_name = "TEXT", help_heading = PRESENTATION)]
    title: Option<String>,
    /// The x axis label, instead of the field's symbol and unit
    #[arg(long, value_name = "TEXT", help_heading = PRESENTATION)]
    x_label: Option<String>,
    /// The y axis label, instead of the field's symbol and unit
    #[arg(long, value_name = "TEXT", help_heading = PRESENTATION)]
    y_label: Option<String>,
    /// The declared style whose symbols and units label the axes; plain otherwise
    #[arg(long, value_name = "STYLE", help_heading = PRESENTATION, add = ArgValueCompleter::new(complete_render_styles))]
    style: Option<String>,
    /// Write the figure to this file, in the format its extension names; without it, a window opens
    #[arg(short = 'o', long, value_name = "PATH", help_heading = OUTPUT, value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::any()))]
    output: Option<PathBuf>,
    /// Write the --output file, replacing it; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT, requires = "output")]
    write: bool,
    /// States a write from values not current may hold and exit 0: outdated, edited[:NAME,…], failed, never_computed
    #[arg(long, value_name = "STATES", help_heading = OUTPUT, value_parser = accepted_states)]
    accept: Option<Accepted>,
    /// The x axis bounds, LOW,HIGH; auto leaves one free: 0,100 or 0,auto
    #[arg(long, value_name = "LOW,HIGH", help_heading = PRESENTATION, value_parser = parse_figure_limits, allow_hyphen_values = true)]
    x_limits: Option<samplekit::config::project_config::Limits>,
    /// The y axis bounds, LOW,HIGH; auto leaves one free
    #[arg(long, value_name = "LOW,HIGH", help_heading = PRESENTATION, value_parser = parse_figure_limits, allow_hyphen_values = true)]
    y_limits: Option<samplekit::config::project_config::Limits>,
    /// The x axis scale: linear, log or symlog
    #[arg(long, value_name = "SCALE", help_heading = PRESENTATION, value_parser = ["linear", "log", "symlog"])]
    x_scale: Option<String>,
    /// The y axis scale: linear, log or symlog
    #[arg(long, value_name = "SCALE", help_heading = PRESENTATION, value_parser = ["linear", "log", "symlog"])]
    y_scale: Option<String>,
    /// equal draws one unit as long on both axes; auto lets them differ
    #[arg(long, value_name = "ASPECT", help_heading = PRESENTATION, value_parser = ["equal", "auto"])]
    aspect: Option<String>,
    /// Where the legend goes: best, outside, none, or a matplotlib place such as "upper left"
    #[arg(long, value_name = "PLACE", help_heading = PRESENTATION, value_parser = samplekit::config::project_config::LEGENDS)]
    legend: Option<String>,
    /// The figure's width and height in centimetres, as 18,12
    #[arg(long, value_name = "W,H", help_heading = PRESENTATION, value_parser = parse_figsize)]
    figsize: Option<(f64, f64)>,
    /// Draw with matplotlib's own settings, leaving out [matplotlib] of .samplekitrc
    #[arg(long, help_heading = PRESENTATION)]
    no_project_style: bool,
    /// Read by the command line before it parses.
    #[allow(dead_code)]
    #[command(flatten)]
    at: AtArgs,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct RunArgs {
    /// A script the model declares: samplekit list scripts names them
    #[arg(add = ArgValueCompleter::new(complete_scripts))]
    script: Option<String>,
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// One of the script's options, NAME=VALUE; repeatable
    #[arg(
        long = "set",
        value_name = "NAME=VALUE",
        help_heading = SHAPE,
        long_help = "One of the script's options, NAME=VALUE, read as the script's signature types it: --set factor=0.8. Repeatable; samplekit list scripts names each script's options",
        action = clap::ArgAction::Append,
        add = ArgValueCompleter::new(complete_script_options)
    )]
    set: Vec<String>,
    /// Stop the script if it runs longer than this, in seconds: nothing is written
    #[arg(long, value_name = "SECONDS", value_parser = seconds, help_heading = SELECTION)]
    timeout: Option<std::time::Duration>,
    /// The style what the script returns is written in; [render] style's otherwise
    #[arg(long, value_name = "STYLE", help_heading = PRESENTATION, add = ArgValueCompleter::new(complete_render_styles))]
    style: Option<String>,
    /// Write what the script changes, asking nothing
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct TagArgs {
    #[command(subcommand)]
    edit: TagEdit,
}

#[derive(Debug, Subcommand)]
enum TagEdit {
    /// Add a tag to the samples selected
    #[command(override_usage = "samplekit [TARGET] tag add <TAG> [OPTIONS]")]
    Add(TagOneArgs),
    /// Remove a tag from the samples selected
    #[command(override_usage = "samplekit [TARGET] tag remove <TAG> [OPTIONS]")]
    Remove(TagOneArgs),
    /// Rename a tag in the samples selected
    #[command(override_usage = "samplekit [TARGET] tag rename <OLD> <NEW> [OPTIONS]")]
    Rename(TagRenameArgs),
}

#[derive(Debug, Args)]
struct TagOneArgs {
    /// The tag
    #[arg(add = ArgValueCompleter::new(complete_tags))]
    name: String,
    #[command(flatten)]
    target: TagTargetArgs,
}

#[derive(Debug, Args)]
struct TagRenameArgs {
    /// The tag to rename
    #[arg(add = ArgValueCompleter::new(complete_tags))]
    old: String,
    /// Its new name
    #[arg(add = ArgValueCompleter::new(complete_tags))]
    new: String,
    #[command(flatten)]
    target: TagTargetArgs,
}

#[derive(Debug, Args)]
struct TagTargetArgs {
    /// Sample files or folders; the current folder by default
    #[arg(value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Write the change; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct RenameArgs {
    /// The sample's new name: its file's, without the folder or `.md`
    #[arg(value_name = "NEW")]
    new: String,
    /// The sample file to rename
    #[arg(value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(complete_targets))]
    targets: Vec<PathBuf>,
    /// Write the change; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// The folder to set up; the current one by default
    #[arg(value_hint = ValueHint::DirPath, add = ArgValueCompleter::new(complete_targets))]
    directory: Option<PathBuf>,
    /// Start from the example project, a model with one formula of each kind,
    /// rather than an empty one
    #[arg(long, conflicts_with = "no_model")]
    example: bool,
    /// An empty project without a model: every value typed in the samples
    #[arg(long)]
    no_model: bool,
    /// Name a model file one already has in .samplekitrc, rather than write
    /// one to fill in; nothing is copied
    #[arg(long, value_name = "PATH", conflicts_with_all = ["example", "no_model"],
          value_hint = ValueHint::FilePath, add = ArgValueCompleter::new(PathCompleter::file()))]
    model: Option<PathBuf>,
    /// Make no Python environment, nor install samplekit in one
    #[arg(long)]
    no_venv: bool,
    /// Write the files; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct NewArgs {
    /// The new sample's name; its file takes the same stem
    #[arg(value_name = "NAME")]
    name: String,
    /// Values to fill in now, as field=value; repeatable
    #[arg(value_name = "FIELD=VALUE", add = ArgValueCompleter::new(complete_values))]
    assignments: Vec<String>,
    /// Take these fields from another sample, as they are, and nothing else: `--keep samples/steel-01.md width,grade`
    #[arg(long, num_args = 2, value_names = ["SAMPLE", "FIELDS"], add = ArgValueCompleter::new(PathCompleter::any()))]
    keep: Vec<String>,
    /// The folder to write it in, or the path of the file itself; by default beside
    /// the sample --keep names, in samples/ where there is one, or in the current folder
    #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::AnyPath, add = ArgValueCompleter::new(PathCompleter::any()))]
    into: Option<PathBuf>,
    /// Write the file; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Debug, Args)]
struct SetArgs {
    /// Sample files or folders, and what to change, as field=value: a word holding = is a change. An
    /// uncertainty is width.u, readings width.readings=12.51,12.52, a table cell
    /// curve.force[0.2], the name the file shows, name=…, and the note below the
    /// frontmatter, note=…
    #[arg(value_name = "TARGET|FIELD=VALUE", add = ArgValueCompleter::new(complete_targets_or_values))]
    words: Vec<String>,
    /// The paths written before the command, read by the command line before it parses.
    #[arg(long = "target", value_name = "PATH", hide = true)]
    before: Vec<PathBuf>,
    /// Add a row to this table instead, the assignments naming its columns
    #[arg(long, value_name = "TABLE", add = ArgValueCompleter::new(complete_tables))]
    add_row: Option<String>,
    #[command(flatten)]
    narrowing: FilterArgs,
    /// Write the change; without it, a preview, then the question apply these changes? [y/N]
    #[arg(long, help_heading = OUTPUT)]
    write: bool,
    #[command(flatten)]
    general: VerboseArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Debug, Args)]
struct CompletionsArgs {
    #[arg(value_enum)]
    shell: CompletionShell,
}

#[derive(Default, Clone)]
struct Options {
    filter: Vec<String>,
    query: Vec<String>,
    columns: Vec<ColumnSpec>,
    profile: Option<String>,
    sort: Vec<String>,
    reverse: bool,
    summary: bool,
    /// `--group FIELDS`: a table per group, or with `--summary` a summary per
    /// group.
    group: Option<String>,
    format: Option<Format>,
    table_style: Option<String>,
    style: Option<String>,
    precision: Option<PrecisionSchema>,
    width: Option<usize>,
    output: Option<PathBuf>,
    write: bool,
    status: bool,
    /// `--accept`: the states a file written from values not current may
    /// hold and exit 0.
    accept: Accepted,
    /// `--style NAME` on the terminal: its document where the style says how
    /// a table is one, the terminal's table otherwise.
    document_if_declared: bool,
    verbose: bool,
    positionals: Vec<String>,
}

impl Options {
    fn narrowing(mut self, narrowing: FilterArgs) -> Self {
        self.filter = narrowing.filters;
        self.query = narrowing.queries;
        self
    }

    fn selection(mut self, selection: SelectionArgs) -> Self {
        self = self.narrowing(selection.narrowing);
        self.sort = selection
            .sorts
            .into_iter()
            .flat_map(|list| list.0)
            .collect();
        self.reverse = selection.reverse;
        self
    }

    fn general(mut self, general: VerboseArgs) -> Self {
        self.verbose = general.verbose;
        VERBOSE.store(general.verbose, AtomicOrdering::Relaxed);
        QUIET.store(general.quiet, AtomicOrdering::Relaxed);
        self
    }

    /// The first target a command reads, and any further file of one project,
    /// as a shell glob gives them.
    fn target(mut self, targets: Vec<PathBuf>) -> Self {
        let mut targets = targets.into_iter();
        if let Some(target) = targets.next() {
            self.positionals.push(target.to_string_lossy().into_owned());
        }
        EXTRA_TARGETS.with(|extra| *extra.borrow_mut() = targets.collect());
        self
    }

    fn render(mut self, render: RenderArgs) -> Self {
        self.width = render.width;
        self.table_style = render.table_style;
        self
    }

    fn presentation(mut self, presentation: PresentationArgs) -> Self {
        self.style = presentation.style;
        self.precision = presentation.precision;
        self
    }
}

/// clap-complete's PowerShell bridge hands the program the command line as one
/// text, `$commandAst`, which loses the empty word being completed after a
/// space: `samplekit --profile <Tab>` completed `--profile` again, and each Tab
/// wrote the word before the cursor once more. This one hands it the words up
/// to the cursor, each without its quotes, and says an empty last word by an
/// environment variable, since Windows PowerShell drops an empty argument
/// given to a program. A candidate holding a space or a character PowerShell
/// reads is quoted, and one completing a word opened with a quote keeps it
/// open, for the rest of a filter. With nothing to offer, the word is given
/// back as it is, where PowerShell offered the folder's files.
struct SamplekitPowershell;

/// Set by the PowerShell bridge where the word being completed is empty.
const POWERSHELL_EMPTY_WORD: &str = "_SAMPLEKIT_COMPLETE_EMPTY";

impl EnvCompleter for SamplekitPowershell {
    fn name(&self) -> &'static str {
        "powershell"
    }

    fn is(&self, name: &str) -> bool {
        name == "powershell" || name == "powershell_ise"
    }

    fn write_registration(
        &self,
        var: &str,
        _name: &str,
        bin: &str,
        completer: &str,
        output: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        let quoted = |text: &str| format!("'{}'", text.replace('\'', "''"));
        // The command's aliases too, `Set-Alias sk BIN`, those defined
        // before this runs: PowerShell gives an alias no completion of its own.
        let script = r#"
$names = @('BIN') + @(Get-Alias -Definition 'BIN' -ErrorAction SilentlyContinue | ForEach-Object Name)
Register-ArgumentCompleter -Native -CommandName $names -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $words = @()
    foreach ($element in $commandAst.CommandElements) {
        if ($element.Extent.StartOffset -ge $cursorPosition) { break }
        $text = $element.Extent.Text
        if ($element.Extent.EndOffset -gt $cursorPosition) {
            $text = $text.Substring(0, $cursorPosition - $element.Extent.StartOffset)
        }
        $words += $text.Trim([char]39, [char]34)
    }
    $prev = $env:VAR
    $env:VAR = "powershell"
    if ($wordToComplete -eq '') { $env:EMPTY = '1' }
    $results = & COMPLETER -- @words 2>$null
    Remove-Item Env:\EMPTY -ErrorAction SilentlyContinue
    if ($null -eq $prev) { Remove-Item Env:\VAR } else { $env:VAR = $prev }
    # Nothing to offer, the word is given back as it is: PowerShell offered
    # the folder's files in its place, `--profile <Tab>` outside a project.
    # An empty word is given back as a space: PowerShell refuses an empty text.
    if (-not $results) {
        $same = if ($wordToComplete -eq '') { ' ' } else { $wordToComplete }
        return [System.Management.Automation.CompletionResult]::new($same, '(nothing to complete here)', 'ParameterValue', 'nothing to complete here')
    }
    $results | ForEach-Object {
        $split = $_.Split("`t")
        $value = $split[0]
        $help = if ($split.Length -eq 2) { $split[1] } else { $split[0] }
        $text = $value
        # A word opened with a quote, a filter being typed, keeps it open.
        if ($wordToComplete -match "^['`"]") {
            $text = $wordToComplete.Substring(0, 1) + $value
        } elseif ($value -match "[\s'`"``$&|;(){}<>@#]") {
            $text = "'" + $value.Replace("'", "''") + "'"
        }
        [System.Management.Automation.CompletionResult]::new($text, $value, 'ParameterValue', $help)
    }
}
"#
        .replace("BIN", bin)
        .replace("COMPLETER", &quoted(completer))
        .replace("EMPTY", POWERSHELL_EMPTY_WORD)
        .replace("VAR", var);
        write!(output, "{script}")
    }

    fn write_complete(
        &self,
        command: &mut clap::Command,
        arguments: Vec<OsString>,
        current_dir: Option<&Path>,
        output: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        // The word being completed: where the command line was reordered,
        // the index it was given; otherwise the last.
        let index = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|index| index.parse::<usize>().ok())
            .filter(|index| *index < arguments.len())
            .unwrap_or_else(|| arguments.len().saturating_sub(1));
        let completions = clap_complete::engine::complete(command, arguments, index, current_dir)?;
        for candidate in completions {
            write!(output, "{}", candidate.get_value().to_string_lossy())?;
            if let Some(help) = candidate.get_help() {
                write!(
                    output,
                    "\t{}",
                    help.to_string().lines().next().unwrap_or_default()
                )?;
            }
            writeln!(output)?;
        }
        Ok(())
    }
}

/// clap-complete's zsh bridge normally appends a space after every accepted
/// candidate. A comma-separated `-c` or `-s` value and an incomplete table path both
/// need to remain attached to the cursor, so this adapter separates those
/// candidates and calls `_describe` without a suffix for them.
struct SamplekitZsh;

impl EnvCompleter for SamplekitZsh {
    fn name(&self) -> &'static str {
        "zsh"
    }

    fn is(&self, name: &str) -> bool {
        name == "zsh"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        output: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        let name = name.replace('-', "_");
        let bin = zsh_quote(bin);
        let completer = zsh_quote(completer);
        let script = r#"#compdef BIN
function _clap_dynamic_completer_NAME() {
    local _CLAP_COMPLETE_INDEX=$(expr $CURRENT - 1)
    local _CLAP_IFS=$'\n'

    local -a completion_words=("${words[@]}")
    completion_words[$CURRENT]=${(Q)completion_words[$CURRENT]}

    local completions=("${(@f)$( \
        _CLAP_IFS="$_CLAP_IFS" \
        _CLAP_COMPLETE_INDEX="$_CLAP_COMPLETE_INDEX" \
        VAR="zsh" \
        COMPLETER -- "${completion_words[@]}" 2>/dev/null \
    )}")

    if [[ -n $completions ]]; then
        local field_list=0
        if [[ ${words[$CURRENT]} == -c* || ${words[$CURRENT]} == --columns=* ||
              ${words[$CURRENT]} == -s* || ${words[$CURRENT]} == --sort=* ||
              ${words[$CURRENT]} == --properties=* ||
              ${words[$CURRENT-1]} == -c || ${words[$CURRENT-1]} == --columns ||
              ${words[$CURRENT-1]} == -s || ${words[$CURRENT-1]} == --sort ||
              ${words[$CURRENT-1]} == -p || ${words[$CURRENT-1]} == --properties ]]; then
            field_list=1
        fi
        local column_prefix=''
        if (( field_list )) && [[ ${completion_words[$CURRENT]} == *,* ]]; then
            column_prefix="${completion_words[$CURRENT]%,*},"
            compset -P '*,'
        fi

        local filter_expr=0
        if [[ ${words[$CURRENT]} == -f* || ${words[$CURRENT]} == --filter=* ||
              ${words[$CURRENT-1]} == -f || ${words[$CURRENT-1]} == --filter ]]; then
            filter_expr=1
        fi
        if (( filter_expr )); then
            local -a expressions
            local expression raw_word=${words[$CURRENT]}
            for expression in "${completions[@]}"; do
                expression=${expression%%:*}
                if [[ $raw_word == '"'* || $raw_word == *='"'* ]]; then
                    expression=${expression//'"'/"'"}
                fi
                expressions+=("$expression")
            done
            compadd -V samplekit-filter -Q -S '' -a expressions
            return
        fi

        local -a commands paths values tables statistics cells continuations options
        local completion value
        for completion in "${completions[@]}"; do
            [[ -n $column_prefix ]] && completion=${completion#"$column_prefix"}
            value=${completion%%:*}
            if [[ $completion == *':table' ]]; then
                tables+=("$completion")
            elif [[ $completion == *':statistics' ]]; then
                statistics+=("$completion")
            elif [[ $completion == *':cell' ]]; then
                cells+=("$completion")
            elif [[ $value == (COMMAND_WORDS) ]]; then
                commands+=("$completion")
            elif [[ $value == -* ]]; then
                options+=("$completion")
            elif [[ $value == *'[' || $value == *'\[' ]]; then
                continuations+=("$completion")
            elif [[ $completion != *:* && -e ${(Q)value} ]]; then
                paths+=("$completion:path")
            else
                values+=("$completion")
            fi
        done

        if (( field_list )); then
            (( ${#values} )) && _samplekit_compadd samplekit-values '' "${values[@]}"
            (( ${#tables} )) && _samplekit_compadd samplekit-tables '.' "${tables[@]}"
            (( ${#statistics} )) && _samplekit_compadd samplekit-statistics '.' "${statistics[@]}"
            (( ${#continuations} )) && _samplekit_compadd samplekit-continuations '' "${continuations[@]}"
            (( ${#paths} )) && _samplekit_compadd samplekit-paths '' "${paths[@]}"
            (( ${#options} )) && _samplekit_compadd samplekit-options ' ' "${options[@]}"
            (( ${#cells} )) && _samplekit_compadd samplekit-cells '' "${cells[@]}"
        else
            (( ${#commands} )) && _samplekit_compadd samplekit-commands ' ' "${commands[@]}"
            (( ${#values} )) && _samplekit_compadd samplekit-values ' ' "${values[@]}"
            (( ${#paths} )) && _samplekit_compadd samplekit-paths '' "${paths[@]}"
            (( ${#tables} )) && _samplekit_compadd samplekit-tables '.' "${tables[@]}"
            (( ${#statistics} )) && _samplekit_compadd samplekit-statistics '.' "${statistics[@]}"
            (( ${#continuations} )) && _samplekit_compadd samplekit-continuations '' "${continuations[@]}"
            (( ${#options} )) && _samplekit_compadd samplekit-options ' ' "${options[@]}"
            (( ${#cells} )) && _samplekit_compadd samplekit-cells '' "${cells[@]}"
        fi
    fi
}

function _samplekit_compadd() {
    local group=$1 suffix=$2
    shift 2
    local -a matches displays
    local candidate value description hidden body
    if [[ $group == samplekit-cells ]]; then
        for candidate in "$@"; do
            value=${candidate%%:*}
            description=${candidate#*:}
            hidden="${value%%.*}."
            body=${value#*.}
            local -a display=("${(Q)value} -- $description")
            compadd -V "$group" -i "$hidden" -d display -- "${(Q)body}"
        done
        return
    fi
    for candidate in "$@"; do
        value=${candidate%%:*}
        if [[ $candidate == *:* ]]; then
            description=${candidate#*:}
            displays+=("${(Q)value} -- $description")
        else
            displays+=("${(Q)value}")
        fi
        matches+=("${(Q)value}")
    done
    compadd -V "$group" -S "$suffix" -d displays -a matches
}

compdef _clap_dynamic_completer_NAME BIN"#
            // The commands the grammar declares, rather than a list written
            // again here that missed `open`, `log` and every later one.
            .replace("COMMAND_WORDS", &command_words().join("|"))
            .replace("NAME", &name)
            .replace("COMPLETER", &completer)
            .replace("BIN", &bin)
            .replace("VAR", var);
        writeln!(output, "{script}")
    }

    fn write_complete(
        &self,
        command: &mut clap::Command,
        arguments: Vec<OsString>,
        current_dir: Option<&Path>,
        output: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        Zsh.write_complete(command, arguments, current_dir, output)
    }
}

fn parse_width(written: &str) -> Result<usize, String> {
    let width = written
        .parse::<usize>()
        .map_err(|_| format!("'{written}' is not a terminal width"))?;
    if width == 0 {
        return Err("terminal width must be at least 1".to_string());
    }
    Ok(width)
}

fn parse_table_style(written: &str) -> Result<String, String> {
    Style::parse(written)
        .map(|_| written.to_string())
        .ok_or_else(|| {
            format!(
                "'{written}' is not a table style; available: {}",
                Style::names().join(", ")
            )
        })
}

/// `--x-limits 0,100`: two bounds, a free one written auto.
fn parse_figsize(written: &str) -> Result<(f64, f64), String> {
    samplekit::config::project_config::parse_figsize(&toml::Value::String(written.to_string()))
}

fn parse_figure_limits(written: &str) -> Result<samplekit::config::project_config::Limits, String> {
    samplekit::config::project_config::parse_limits(&toml::Value::String(written.to_string()))
}

fn parse_precision(written: &str) -> Result<PrecisionSchema, String> {
    Precision::both(written)
        .map(|_| PrecisionSchema::Both(written.to_string()))
        .map_err(|error| {
            // A word with no digit in it was never trying to be a precision:
            // the grammar's own diagnosis read `abc` as a fill and an
            // alignment, and advised on a width nobody had written.
            if !written.chars().any(|c| c.is_ascii_digit()) {
                format!(
                    "'{written}' is not a precision: one is written as .3f, three decimals, \
                     or .2e, two in scientific notation"
                )
            } else {
                error.to_string()
            }
        })
}

/// `-s` splits exactly as `-c` does, so a comma inside a table index stays in
/// its key. An empty key is refused rather than skipped.
fn parse_sort_list(written: &str) -> Result<SortList, String> {
    split_top_level(written, ',')
        .into_iter()
        .map(|part| {
            let key = part.trim();
            if key.is_empty() || key == "-" {
                Err("a sort key cannot be empty".to_string())
            } else {
                Ok(key.to_string())
            }
        })
        .collect::<Result<_, _>>()
        .map(SortList)
}

fn parse_column_list(written: &str) -> Result<ColumnList, String> {
    profiles::parse_columns(written)
        .map(ColumnList)
        .map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Outcome {
    // `help --help`, as every other command takes it: the help of `help`,
    // where clap read `--help` as the command to explain.
    if matches!(
        arguments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
        ["help", "--help" | "-h"]
    ) {
        return run(&["help".to_string(), "help".to_string()]);
    }
    // `samplekit` alone opens the workbench where a person is; in a pipe
    // or a script it is the short help it always was.
    if arguments.is_empty() {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            // Always the start page, the project here offered first.
            return workbench(Path::new("."), true);
        }
        return run(&["-h".to_string()]);
    }
    // A reading command given --at reads the project as its history kept it;
    // explain and log have their own.
    if arguments
        .first()
        .is_none_or(|word| !matches!(word.as_str(), "explain" | "log" | "restore"))
        && let Some(at) = arguments
            .iter()
            .position(|word| word == "--at" || word.starts_with("--at="))
    {
        return run_at(arguments, at);
    }
    let command = with_configuration_footer(Cli::command());
    let command = if wants_long_help(&command, arguments) {
        command
    } else {
        compact(command)
    };
    let cli = match command
        .try_get_matches_from(
            std::iter::once("samplekit").chain(arguments.iter().map(String::as_str)),
        )
        .and_then(|matches| Cli::from_arg_matches(&matches))
    {
        Ok(cli) => cli,
        Err(error) => return clap_failure(error, arguments),
    };
    let no_color = cli.no_color
        || std::env::var_os("NO_COLOR").is_some()
        || std::env::var_os("TERM").is_some_and(|term| term == "dumb");
    COLOR_ENABLED.store(!no_color, AtomicOrdering::Relaxed);
    // The command line's colours, the defaults until a project is read; on
    // only where stdout is a terminal.
    render::set_palette(render::Palette::of(None, coloured()));
    render::set_stderr_painted(
        std::io::stderr().is_terminal() && COLOR_ENABLED.load(AtomicOrdering::Relaxed),
    );
    // What the model prints reaches stderr as this command line's own lines
    // do, its warnings in their colour; `compute` relays above its bar.
    samplekit::config::model_runtime::relay_output_through(relay_to_stderr);
    if let Some(path) = &cli.rc {
        force_configuration(path)?;
    }
    let _ = ADVICE_TARGET.set(target_of(&cli));
    let _ = ADVICE_SELECTION.set(selection_words(arguments));
    if cli.show_rc {
        return show_rc(&target_of(&cli), cli.query.general.verbose);
    }

    // A read that could not read every file answers for part of the collection,
    // and its code says so; so does a write over a selection it could not read
    // in full. What it could read is still shown or written, and each file
    // still named.
    let reading = matches!(
        cli.command,
        None | Some(
            Commands::List(_)
                | Commands::Status(_)
                | Commands::Explain(_)
                | Commands::View(_)
                | Commands::Export(_)
                | Commands::Plot(_)
                | Commands::Tag(_)
                | Commands::Compute(_)
                | Commands::Run(_)
                | Commands::Set(_)
                | Commands::Open(_)
                | Commands::Log(_)
                | Commands::Diff(_)
                | Commands::Restore(_)
                | Commands::Remove(_)
                | Commands::Rename(_)
        )
    );
    // A write is a snapshot in each project's history, named by the command as
    // it was typed; what changed before it is one of its own. Taken whatever
    // the command answers: a computation that failed on one value wrote the
    // others.
    let _ = COMMAND_LINE.set(command_line(arguments));
    let writing = arguments.iter().any(|word| word == "--write");
    // Ctrl-C while files are written one by one ends the file under way, then
    // stops: what was written is said, and kept in a snapshot of its own.
    if writing
        && matches!(
            cli.command,
            Some(
                Commands::Tag(_) | Commands::Restore(_) | Commands::Remove(_) | Commands::Rename(_)
            )
        )
    {
        let _ = ctrlc::set_handler(|| STOP.store(true, AtomicOrdering::SeqCst));
    }
    // What refuses to run before the offer is answered: a write to samples,
    // which starts their project's history. A table written with -o, an
    // export or a figure writes no sample.
    WRITING.store(
        writing
            && matches!(
                cli.command,
                Some(
                    Commands::Set(_)
                        | Commands::New(_)
                        | Commands::Tag(_)
                        | Commands::Compute(_)
                        | Commands::Run(_)
                        | Commands::Restore(_)
                        | Commands::Remove(_)
                        | Commands::Rename(_)
                )
            ),
        AtomicOrdering::Relaxed,
    );
    // A folder made a project brings its history before anything is kept in
    // it: the first snapshot taken there ends the offer.
    // Asked before anything else is said or asked: after a preview's own
    // question, the Enter of its `y` answered this one. The projects the
    // paths name; a sample a selection reaches below them is asked of as it is
    // read.
    let may_write = writing
        || matches!(
            cli.command,
            Some(
                Commands::Set(_)
                    | Commands::New(_)
                    | Commands::Tag(_)
                    | Commands::Compute(_)
                    | Commands::Run(_)
                    | Commands::Restore(_)
                    | Commands::Remove(_)
                    | Commands::Rename(_)
                    | Commands::Export(_)
                    | Commands::Plot(_)
            )
        );
    if may_write {
        for config in samplekit::config::version_control::projects_of(&places_reached(arguments)) {
            bring_history_from_above(&config)?;
        }
    }
    let before = writing.then(|| history_before(arguments));
    let outcome = dispatch(cli);
    if let Some(before) = before {
        let message = if STOP.load(AtomicOrdering::SeqCst) {
            format!(
                "{} — {}",
                command_line(arguments),
                samplekit::config::version_control::INTERRUPTED
            )
        } else {
            command_line(arguments)
        };
        let answered = ANSWERED.lock().map(|said| said.clone()).unwrap_or_default();
        history_after(before, &format!("{message}{answered}"));
    }
    if reading && outcome.is_ok() && UNREAD.load(AtomicOrdering::Relaxed) {
        // Already said, file by file — unless -q silenced the warnings, and the
        // exit code was then all a reader had: said once, as an error, which -q
        // never silences. What the filesystem refused exits as it does named
        // directly.
        let said = if QUIET.load(AtomicOrdering::Relaxed) {
            "a file could not be read, so this answer is incomplete — samplekit list skipped \
             names it"
                .to_string()
        } else {
            String::new()
        };
        if !MALFORMED.load(AtomicOrdering::Relaxed) {
            return Err(Fail::io(said));
        }
        return Err(Fail::data(said));
    }
    outcome
}

/// Whether the configuration above `file` does not read: said as a warning
/// now, for a write that goes on without it. `set` wrote under it in silence,
/// and exited 0.
fn configuration_unread(file: &Path) -> bool {
    let folder = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match samplekit::config::project_config::load_for(folder) {
        Err(error) => {
            warn(&format!(
                "configuration not read, and the sample written without it — {error}"
            ));
            true
        }
        Ok(_) => false,
    }
}

/// A write that succeeded under a configuration not read exits 2, as a
/// reading command does: its answer is not the project's.
fn written_without_configuration(unread: bool, outcome: Outcome) -> Outcome {
    match outcome {
        Ok(()) if unread => Err(Fail::data(String::new())),
        other => other,
    }
}

/// The places a command reaches: the folder it runs in, and the paths it
/// names that exist.
fn places_reached(arguments: &[String]) -> Vec<PathBuf> {
    std::iter::once(PathBuf::from("."))
        .chain(
            arguments
                .iter()
                .map(PathBuf::from)
                .filter(|path| path.exists()),
        )
        .collect()
}

/// What each project the command reaches holds before it writes.
fn history_before(arguments: &[String]) -> samplekit::config::version_control::Writing {
    let (writing, failures) =
        samplekit::config::version_control::before_writing(&places_reached(arguments));
    history_failures(failures);
    writing
}

/// After a write, a snapshot saying `message` in each project it changed.
fn history_after(writing: samplekit::config::version_control::Writing, message: &str) {
    history_failures(samplekit::config::version_control::after_writing(
        writing, message,
    ));
}

/// A history not kept is a warning, never the command's failure.
fn history_failures(failures: Vec<(PathBuf, samplekit::config::version_control::VcsError)>) {
    for said in samplekit::config::version_control::notices() {
        warn(&said);
    }
    for (root, error) in failures {
        // Several projects' are one line, each under -v.
        warn_gathered(
            "history not kept",
            |count| format!("the history of {count} projects was not kept"),
            format!("the history of {} was not kept: {error}", root.display()),
        );
    }
}

/// The command in its one form, for a snapshot's message: its paths first,
/// as the help writes it, however it was typed — `samplekit pieces compute
/// --write` for `samplekit compute pieces --write` too — so that one search
/// finds it. A path is a word the command takes as a target: one that is
/// there, or written as a path, and `new`'s `--into`.
fn command_line(arguments: &[String]) -> String {
    let Some((command, after)) = arguments.split_first() else {
        return "samplekit".to_string();
    };
    if !is_command(command) || command == "help" || command == "completions" {
        return std::iter::once("samplekit".to_string())
            .chain(arguments.iter().map(|word| shell_word(word)))
            .collect::<Vec<_>>()
            .join(" ");
    }
    // As the help writes it: the paths, the command, its words, its options
    // in the order typed, and `--write` last — one message whatever the
    // order it was typed in. Its words are told from its paths as the command
    // tells them.
    let mut paths: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut options: Vec<String> = Vec::new();
    let mut write = false;
    let mut words = after.iter().peekable();
    while let Some(word) = words.next() {
        if word == "--into" || word == "--target" {
            if let Some(path) = words.next() {
                paths.push(path.clone());
            }
            continue;
        }
        if word == "--write" {
            write = true;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            options.push(shell_word(word));
            if takes_a_value(word)
                && let Some(value) = words.next()
            {
                options.push(shell_word(value));
            }
            // `--keep SAMPLE FIELDS` takes two: its fields were read as the
            // command's own word, `new steel-03 width,grade --keep …`, which
            // does not run.
            if word == "--keep"
                && let Some(fields) = words.next_if(|next| !next.starts_with('-'))
            {
                options.push(shell_word(fields));
            }
            continue;
        }
        positionals.push(word.clone());
    }
    let mut words_of: Vec<String> = Vec::new();
    let own = names_first(command, &positionals);
    for (at, word) in positionals.into_iter().enumerate() {
        let is_own = match command.as_str() {
            "new" => true,
            "set" => word.contains('='),
            "explain" | "open" => names_nothing(&word),
            _ => at < own,
        };
        if is_own {
            words_of.push(word);
        } else {
            paths.push(word);
        }
    }
    std::iter::once("samplekit".to_string())
        .chain(paths.iter().map(|word| shell_word(word)))
        .chain(std::iter::once(command.clone()))
        .chain(words_of.iter().map(|word| shell_word(word)))
        .chain(options)
        .chain(write.then(|| "--write".to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A command answered yes at its question, kept as `--write` would have
/// written it: one message for one command, however it was answered.
fn answered_yes(said: &str) -> String {
    if said.split(' ').any(|word| word == "--write") {
        said.to_string()
    } else {
        format!("{said} --write")
    }
}

/// One word as a shell reads it back: quoted where it holds a space or a
/// symbol the shell would take. A command a message suggests is copied into a
/// terminal, and `samplekit set a b.md …` there set nothing in `a b.md`.
fn shell_word(word: &str) -> String {
    // `\` is Windows's separator, which its shells read as it is.
    let plain = if cfg!(windows) {
        "-_./=,:@+%\\"
    } else {
        "-_./=,:@+%"
    };
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || plain.contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// A name refused, its offending character counted from 1 as a filter error
/// counts it — `character 2` — where the identifier's own words counted from
/// 0, `position 1`, and the two messages disagreed about the same text.
fn identifier_refused(error: &samplekit::core::identifier::IdentifierError) -> String {
    // Characters are counted from 1 where the error is written.
    error.to_string()
}

/// The styles there are where no configuration is read: `plain`, and those
/// SampleKit gives. `--style latex` was refused there while `-o x.tex` wrote
/// the very style.
fn styles_without_a_project<'a>() -> Vec<&'a str> {
    std::iter::once("plain")
        .chain(samplekit::config::project_config::TextFormat::GIVEN)
        .collect()
}

/// The target a suggested command names, path first, as the help writes it:
/// `pieces/ ` before the command, nothing for the current folder.
fn path_first_word(target: &Path) -> String {
    if target.as_os_str().is_empty() || target == Path::new(".") {
        String::new()
    } else {
        format!("{} ", shell_path(target))
    }
}

/// The target and those named beside it, `ana/pieces tom/pieces `, for a
/// command a message suggests over every project the answer was given for.
fn targets_first_words(target: &Path) -> String {
    let extra = EXTRA_TARGETS.with(|extra| extra.borrow().clone());
    let mut words = path_first_word(target);
    if !extra.is_empty() && words.is_empty() {
        words = ". ".to_string();
    }
    for other in &extra {
        words.push_str(&path_first_word(Path::new(other)));
    }
    words
}

/// A path as a shell word, for a command a message suggests.
/// A path recorded or built with `/`, as the system writes it: `\` on
/// Windows, where a message never mixes the two.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// A folder as a message names it: its path, the system's separator after it.
fn folder_said(path: &str) -> String {
    format!("{}{}", native(path), std::path::MAIN_SEPARATOR)
}

fn shell_path(path: &Path) -> String {
    let shown = path.display().to_string();
    // A file whose name opens with `-` is read as an option: `./-odd.md`,
    // which every command reads as the file.
    if shown.starts_with('-') {
        return shell_word(&format!("./{shown}"));
    }
    shell_word(&shown)
}

/// Runs the command the arguments name.
/// `--csv`, `--tsv`, `--json`, `--style` and `-o` made one format: a style
/// asked for a file writes its document, and `-o`'s extension names the
/// format where nothing else does. `has_shape`: what is written is a table.
fn output_format(
    options: &mut Options,
    [csv, tsv, json]: [bool; 3],
    output: Option<PathBuf>,
    has_shape: bool,
) -> Result<(), Fail> {
    options.format = if csv {
        Some(Format::Csv)
    } else if tsv {
        Some(Format::Tsv)
    } else if json {
        Some(Format::Json)
    } else {
        // A style asked for a file, or for `-o -`, writes its document:
        // named here, and read from the project's declarations once it
        // is read — a stand-in until then. On the terminal, the style
        // is the table's cells' form alone.
        let data = |path: &Path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .is_some_and(|extension| {
                    matches!(extension.as_str(), "csv" | "tsv" | "json" | "txt")
                })
        };
        // On the terminal too, a style that says how a table is a
        // document shows the document the file would hold; one that
        // says units alone, the terminal's table in its forms.
        match (&options.style, &output) {
            (Some(name), None) => {
                options.document_if_declared = true;
                Some(Format::Text(Box::new(
                    samplekit::config::project_config::TextFormat::given(name).unwrap_or_else(
                        || samplekit::config::project_config::TextFormat::named(name),
                    ),
                )))
            }
            (Some(name), Some(path)) if !data(path) => Some(Format::Text(Box::new(
                samplekit::config::project_config::TextFormat::given(name)
                    .unwrap_or_else(|| samplekit::config::project_config::TextFormat::named(name)),
            ))),
            _ => None,
        }
    };
    options.output = output;
    // A style writing another document than the file's extension
    // names is written as the style says, and said: `--style markdown
    // -o t.tex` wrote Markdown into a .tex without a word.
    if let (Some(Format::Text(document)), Some(extension)) = (
        options.format.as_ref(),
        options
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str()),
    ) && !extension.eq_ignore_ascii_case(&document.extension)
        && samplekit::config::project_config::TextFormat::GIVEN
            .iter()
            .filter_map(|name| samplekit::config::project_config::TextFormat::given(name))
            .any(|given| extension.eq_ignore_ascii_case(&given.extension))
    {
        errln!(
            "note: {} is written as {}, as --style says, whatever its .{extension}",
            options.output.as_deref().unwrap_or(Path::new("")).display(),
            document.name
        );
    }
    // `-o table.csv` writes CSV, never a terminal table into a .csv.
    if has_shape
        && options.format.is_none()
        && let Some(extension) = options
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
    {
        options.format = match extension.to_ascii_lowercase().as_str() {
            "csv" => Some(Format::Csv),
            "tsv" => Some(Format::Tsv),
            "json" => Some(Format::Json),
            // The extension of a text format SampleKit gives: `.tex`.
            other => samplekit::config::project_config::TextFormat::GIVEN
                .iter()
                .filter_map(|name| samplekit::config::project_config::TextFormat::given(name))
                .find(|given| given.extension == other)
                .map(|given| Format::Text(Box::new(given))),
        };
    }
    // A data format holds a table: without columns, `-o t.csv` was
    // refused as an extension it plainly has.
    if !has_shape
        && let Some(extension) = options
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|extension| matches!(extension.as_str(), "csv" | "tsv" | "json"))
    {
        return Err(Fail::usage(format!(
            "-o {}: a .{extension} file holds a table, and this one has no columns — \
             name them with --columns or --profile",
            options.output.as_deref().unwrap_or(Path::new("")).display()
        )));
    }
    // Any other name wrote the terminal's table under it: `x.xlsx` held
    // box drawing, or the list of paths without columns. `.txt` asks for
    // what the terminal shows.
    if options.format.is_none()
        && let Some(path) = options
            .output
            .as_ref()
            .filter(|path| *path != Path::new("-"))
    {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        if extension.as_deref() != Some("txt") {
            return Err(Fail::usage(format!(
                "-o {}: the extension names the format — .csv, .tsv, .json, .tex, \
                 .md or .html, or .txt for what the terminal shows; --csv, --tsv, \
                 --json or --style NAME names it otherwise",
                path.display()
            )));
        }
    }
    Ok(())
}

fn dispatch(cli: Cli) -> Outcome {
    match cli.command {
        None => {
            NO_COMMAND.store(true, AtomicOrdering::Relaxed);
            let query = cli.query;
            let has_shape = !query.columns.is_empty() || query.profile.is_some();
            let mut options = Options::default()
                .selection(query.selection)
                .target(query.targets)
                .render(query.render)
                .presentation(query.presentation)
                .general(query.general);
            options.columns = query
                .columns
                .into_iter()
                .flat_map(|columns| columns.0)
                .collect();
            options.profile = query.profile;
            options.summary = query.summary;
            options.group = query.group;
            output_format(
                &mut options,
                [query.csv, query.tsv, query.json],
                query.output,
                has_shape,
            )?;
            options.write = query.write;
            options.status = query.status;
            options.accept = query.accept.unwrap_or_default();
            if !has_shape && (options.summary || options.status || options.format.is_some()) {
                return Err(Fail::usage(if options.summary {
                    "--summary summarises columns: name them with --columns or --profile"
                        .to_string()
                } else if options.status {
                    // Accepted and ignored, it was a silence.
                    "--status adds a state column to a table: name its columns with --columns \
                     or --profile — samplekit status says what is not current"
                        .to_string()
                } else {
                    "an output format writes a table: name its columns with --columns or --profile"
                        .to_string()
                }));
            }
            if let Some(save) = &query.save {
                return tables::save_view(&options, save, has_shape);
            }
            if has_shape {
                table(&options)
            } else {
                find(&options)
            }
        }
        Some(Commands::List(args)) => {
            let mut targets = args.targets;
            let kind_after = targets
                .iter()
                .find(|target| KINDS.contains(&target.to_string_lossy().as_ref()))
                .cloned();
            let kind = match args.kind_or_target {
                None => None,
                // The kind wins over a folder of its name, as a command does:
                // the folder is written `./scripts`.
                Some(first) if KINDS.contains(&first.as_str()) => Some(first),
                Some(first) if kind_after.is_some() => {
                    return Err(Fail::usage(format!(
                        "the kind comes after the command, the target before it\n  use: samplekit {first} list {}",
                        kind_after.unwrap_or_default().display()
                    )));
                }
                // Several files, as a glob gives them.
                Some(first) if Path::new(&first).exists() || looks_like_a_path(&first) => {
                    targets.insert(0, PathBuf::from(first));
                    None
                }
                Some(first) => return Err(unknown_kind(&first)),
            };
            let narrowed = !args.narrowing.filters.is_empty() || !args.narrowing.queries.is_empty();
            // A selection keeps samples: the project's declarations are listed
            // whole, whatever it keeps.
            if narrowed
                && let Some(kind) = kind
                    .as_deref()
                    .filter(|kind| !matches!(*kind, "fields" | "tags" | "files"))
            {
                return Err(Fail::usage(format!(
                    "a selection keeps samples, and list {kind} lists what the project declares: \
                     -f and --query go with list, list fields, list tags or list files"
                )));
            }
            let options = Options::default()
                .narrowing(args.narrowing)
                .target(targets)
                .general(args.general);
            let root = options
                .positionals
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            // After a kind, a word is a target: no command is offered for it.
            if kind.is_some() && !root.exists() && root.symlink_metadata().is_err() {
                return Err(Fail::io(format!(
                    "{}: no such file or directory",
                    root.display()
                )));
            }
            let collection = loaded(&root)?;
            warn_skipped(&collection, &options);
            let selected = if narrowed {
                Some(select(&collection, &options, &root)?)
            } else {
                None
            };
            inventory(&collection, selected.as_ref(), kind.as_deref(), &options)
        }
        Some(Commands::Status(args)) => {
            if let Some(limit) = args.timeout {
                runtime::set_load_limit(limit);
            }
            let gate = StatusGate {
                exit_code: args.exit_code,
                accept: args.accept.unwrap_or_default(),
                json: args.json,
            };
            stale(
                &Options::default()
                    .selection(args.selection)
                    .target(args.targets)
                    .render(args.render)
                    .general(args.general),
                &gate,
            )
        }
        Some(Commands::Compute(args)) => {
            // `--timeout` is the model's loading's limit too.
            if let Some(limit) = args.timeout {
                runtime::set_load_limit(limit);
            }
            let options = Options::default()
                .selection(args.selection)
                .target(args.targets)
                .render(args.render)
                .general(args.general);
            let flags = ComputeFlags {
                names: args.properties,
                rerun: args.rerun,
                force: args.force,
                timeout: args.timeout,
                // Without --write, compute lists, then asks.
                dry_run: !args.write,
                write: args.write,
                show_output: args.show_output,
            };
            compute(&options, &flags)?;
            // What the plan would compute, asked first: computed on yes, and
            // then asked whether to write it.
            let planned = TO_COMPUTE.swap(0, AtomicOrdering::Relaxed);
            if flags.write || planned == 0 {
                return Ok(());
            }
            if ask(&if planned == 1 {
                "compute this value?".to_string()
            } else {
                format!("compute these {planned} values?")
            }) {
                outln!();
                return compute(
                    &options,
                    &ComputeFlags {
                        dry_run: false,
                        ..flags
                    },
                );
            }
            // A no said as every other: nothing ran, nothing was written.
            outln!("nothing written");
            Ok(())
        }
        Some(Commands::Log(args)) => {
            let mut options = Options::default().general(args.general);
            options.output = args.output;
            options.write = args.write;
            let narrowed = !args.narrowing.filters.is_empty() || !args.narrowing.queries.is_empty();
            let mut targets =
                history_targets(args.targets.clone(), args.narrowing.clone(), &["1"])?;
            if narrowed {
                for file in ever_selected(args.targets, args.narrowing)? {
                    if !targets.iter().any(|held| same_file(held, &file)) {
                        targets.push(file);
                    }
                }
            }
            if narrowed && targets.is_empty() {
                outln!("no sample is selected: nothing to list");
                return Ok(());
            }
            match &args.script {
                Some(when) => history::script(&targets, when),
                None if args.export => {
                    history::log_export(&targets, args.at.at.as_deref(), &options)
                }
                None => history::log(&targets, args.at.at.as_deref(), &options),
            }
        }
        Some(Commands::Remove(args)) => {
            let mut options = Options::default()
                .selection(args.selection)
                .target(args.targets)
                .general(args.general);
            options.write = args.write;
            edit::remove(&options)
        }
        Some(Commands::Rename(args)) => {
            let mut options = Options::default()
                .target(args.targets)
                .general(args.general);
            options.write = args.write;
            edit::rename(&options, &args.new)
        }
        Some(Commands::Restore(args)) => {
            let _ = Options::default().general(args.general);
            let narrowed = !args.narrowing.filters.is_empty() || !args.narrowing.queries.is_empty();
            // The samples a selection keeps, as they were at the snapshot put
            // back: one deleted since among them.
            let targets = match (&args.at, narrowed) {
                (Some(when), true) => {
                    match history::selected_at(&args.targets, &args.narrowing, when)? {
                        Some(files) => files,
                        None => history_targets(args.targets, args.narrowing, &[])?,
                    }
                }
                _ => history_targets(args.targets, args.narrowing, &["1"])?,
            };
            if narrowed && targets.is_empty() {
                outln!("nothing to change — no sample is selected");
                return Ok(());
            }
            history::restore(&targets, args.at.as_deref(), args.write)
        }
        Some(Commands::Diff(args)) => {
            // For -v and -q, which every command takes.
            let _ = Options::default().general(args.general);
            let narrowed = !args.narrowing.filters.is_empty() || !args.narrowing.queries.is_empty();
            // The selection as it is now, as it was where the comparison
            // starts, and as the last snapshot holds it.
            let kept: Vec<&str> = std::iter::once("1")
                .chain(args.from.as_deref())
                .chain(args.to.as_deref())
                .collect();
            let targets = history_targets(args.targets, args.narrowing, &kept)?;
            if narrowed && targets.is_empty() {
                outln!("no sample is selected: nothing to compare");
                return Ok(());
            }
            history::diff(&targets, args.from.as_deref(), args.to.as_deref())
        }
        Some(Commands::Explain(args)) => {
            let (targets, field) = targets_and_word(args.before, args.words, "field")?;
            // A path missing is said before what explain would want of it;
            // `--at` reads a sample deleted since.
            if args.at.at.is_none()
                && let Some(missing) = targets
                    .iter()
                    .find(|target| names_nothing(&target.to_string_lossy()))
            {
                return Err(Fail::io(format!(
                    "{}: no such file or directory",
                    missing.display()
                )));
            }
            // A file that is not a sample is one SampleKit may have written,
            // found by its hash in the history, and explained whole.
            // A Markdown file without a frontmatter is a table SampleKit may
            // have written, `-o table.md`, and no sample.
            let written = |target: &PathBuf| {
                target.is_file()
                    && (!target
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                        || std::fs::read(target).is_ok_and(|bytes| {
                            !bytes
                                .strip_prefix(b"\xef\xbb\xbf")
                                .unwrap_or(&bytes)
                                .starts_with(b"---")
                        }))
            };
            let (outputs, samples): (Vec<PathBuf>, Vec<PathBuf>) =
                targets.into_iter().partition(written);
            if !outputs.is_empty() {
                if args.at.at.is_some() {
                    return Err(Fail::usage(
                        "--at explains a sample's value as it was: a file SampleKit wrote is \
                         explained as it is, with what changed since"
                            .to_string(),
                    ));
                }
                if let Some(field) = &field {
                    return Err(Fail::usage(format!(
                        "{}: a file SampleKit wrote is explained whole, with no field: \
                         samplekit {} explain",
                        shell_word(field),
                        outputs
                            .iter()
                            .map(|output| shell_path(output))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )));
                }
            }
            for (at, output) in outputs.iter().enumerate() {
                if at > 0 {
                    outln!();
                }
                history::explain_output(output)?;
            }
            if !outputs.is_empty() && samples.is_empty() {
                return Ok(());
            }
            let files = sample_files(samples, args.narrowing)?;
            if files.is_empty() {
                outln!("no sample is selected: nothing to explain");
                return Ok(());
            }
            let mut options = Options::default()
                .presentation(args.presentation)
                .general(args.general);
            // Over several samples, one that has no such field is said in
            // one line, and the others explained: the field is refused where
            // no sample has it.
            let several = files.len() > 1;
            let mut lacking: Vec<Fail> = Vec::new();
            for (at, file) in files.iter().enumerate() {
                if at > 0 || !outputs.is_empty() {
                    outln!();
                }
                options.positionals = std::iter::once(file.to_string_lossy().into_owned())
                    .chain(field.clone())
                    .collect();
                let explained = match &args.at.at {
                    Some(when) => history::explain_at(&options, when),
                    None => explain(&options),
                };
                let Err(mut fail) = explained else {
                    continue;
                };
                let Some(field) = &field else {
                    return Err(fail);
                };
                if fail.code != 1 || !fail.message.contains(&format!("'{field}'")) {
                    return Err(fail);
                }
                if several {
                    outln!("{} · no {field} here", relative_to_here(file));
                    lacking.push(fail);
                    continue;
                }
                // The field is the one word naming no file: said so, where it
                // is no field either — a path mistyped is either.
                fail.message = format!(
                    "{}: no such file, nor a field of the model — {}",
                    shell_word(field),
                    fail.message
                );
                return Err(fail);
            }
            if lacking.len() == files.len()
                && let (Some(mut fail), Some(field)) = (lacking.into_iter().next(), &field)
            {
                fail.message = format!(
                    "{}: no such file, nor a field of the selected samples — {}",
                    shell_word(field),
                    fail.message
                );
                return Err(fail);
            }
            Ok(())
        }
        Some(Commands::Open(args)) => {
            let (targets, pattern) = targets_and_word(args.before, args.words, "pattern")?;
            if args.edit
                && let Some(pattern) = &pattern
            {
                return Err(Fail::usage(format!(
                    "--edit opens the sample's own file, and '{pattern}' names its other files: \
                     one or the other"
                )));
            }
            let options = Options::default().general(args.general);
            let files = sample_files(targets, args.narrowing)?;
            if files.is_empty() {
                outln!("no sample is selected: nothing to open");
                return Ok(());
            }
            // What they open, counted over them all: more than five files open
            // after one yes at a terminal, as one sample's do.
            if files.len() > 1 {
                OPEN_COUNTING.store(true, AtomicOrdering::Relaxed);
                OPEN_COUNTED.store(0, AtomicOrdering::Relaxed);
                for file in &files {
                    let _ = open_file(&options, file, pattern.as_deref(), args.navigate, args.edit);
                }
                OPEN_COUNTING.store(false, AtomicOrdering::Relaxed);
                let counted = OPEN_COUNTED.load(AtomicOrdering::Relaxed);
                if counted > samplekit::presentation::opening::OPENED_WITHOUT_ASKING {
                    let what = format!(
                        "{} of {}",
                        if args.navigate {
                            counted_as(counted, "folder", "folders")
                        } else {
                            counted_as(counted, "file", "files")
                        },
                        counted_as(files.len(), "sample", "samples")
                    );
                    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
                        return Err(Fail::usage(format!(
                            "{what}: more than {} open only after a yes at a terminal — a \
                             narrower pattern or selection names fewer",
                            samplekit::presentation::opening::OPENED_WITHOUT_ASKING
                        )));
                    }
                    if !ask(&format!("{what}: open them all?")) {
                        outln!("nothing opened");
                        return Ok(());
                    }
                    OPEN_ASKED.store(true, AtomicOrdering::Relaxed);
                }
            }
            // Over several samples, one with nothing to open is said in one
            // line, and the others opened.
            let mut failed: Vec<Fail> = Vec::new();
            for file in &files {
                match open_file(&options, file, pattern.as_deref(), args.navigate, args.edit) {
                    Ok(()) => {}
                    Err(fail) if files.len() > 1 && fail.code == 1 => {
                        outln!(
                            "{} · {}",
                            relative_to_here(file),
                            fail.message.lines().next().unwrap_or_default()
                        );
                        failed.push(fail);
                    }
                    Err(fail) => return Err(fail),
                }
            }
            if failed.len() == files.len()
                && let Some(fail) = failed.into_iter().next()
            {
                return Err(fail);
            }
            Ok(())
        }
        Some(Commands::View(args)) => {
            // The one word that is no file, the others all files, is a
            // table's name: `view curve`, written after the command.
            let mut targets = args.targets;
            let missing: Vec<usize> = (0..targets.len())
                .filter(|at| !targets[*at].exists())
                .collect();
            let table = match missing.as_slice() {
                [at] => Some(targets.remove(*at).to_string_lossy().into_owned()),
                _ => None,
            };
            let mut options = Options::default()
                .selection(args.selection)
                .target(targets)
                .render(args.render)
                .presentation(args.presentation)
                .general(args.general);
            if table.is_none() && (args.csv || args.tsv || args.json || args.output.is_some()) {
                return Err(Fail::usage(
                    "view writes one table: name it after view, samplekit samples/steel-01.md \
                     view curve --csv — a table of samples is written with -c and -o"
                        .to_string(),
                ));
            }
            output_format(
                &mut options,
                [args.csv, args.tsv, args.json],
                args.output,
                true,
            )?;
            options.write = args.write;
            options.accept = args.accept.unwrap_or_default();
            view(&options, args.note, table.as_deref())
        }
        Some(Commands::Export(args)) => {
            let mut options = Options::default()
                .selection(args.selection)
                .target(args.targets)
                .general(args.general);
            options.positionals.insert(0, args.export);
            options.precision = args.precision;
            options.status = args.status;
            options.output = args.output;
            options.write = args.write;
            options.accept = args.accept.unwrap_or_default();
            options.style = args.style;
            export(&options)
        }
        Some(Commands::Run(args)) => {
            let mut options = Options::default()
                .selection(args.selection)
                .target(args.targets)
                .general(args.general);
            options.write = args.write;
            run::run(
                options,
                &RunFlags {
                    script: args.script,
                    set: args.set,
                    style: args.style,
                    timeout: args.timeout,
                },
            )
        }
        Some(Commands::Plot(args)) => {
            let args = *args;
            let mut options = Options::default()
                .selection(args.selection)
                .general(args.general);
            // With axes given there is no figure name: every positional is a
            // target.
            let (figure, mut targets) = match (args.x.is_some(), args.figure) {
                (true, Some(first)) => {
                    let mut targets = vec![PathBuf::from(first)];
                    targets.extend(args.targets);
                    (None, targets)
                }
                (_, figure) => (figure, args.targets),
            };
            if !targets.is_empty() {
                options
                    .positionals
                    .push(targets.remove(0).to_string_lossy().into_owned());
            }
            EXTRA_TARGETS.with(|extra| *extra.borrow_mut() = targets);
            options.output = args.output;
            options.write = args.write;
            plot(
                options,
                &PlotFlags {
                    accept: args.accept.unwrap_or_default(),
                    figure,
                    export: None,
                    axes: args.x.zip(args.y),
                    no_project_style: args.no_project_style,
                    overrides: samplekit::presentation::plotting::FigureOverrides {
                        title: args.title,
                        x_label: args.x_label,
                        y_label: args.y_label,
                        style: args.style,
                        group: args.group,
                        kind: args.kind,
                        x_limits: args.x_limits,
                        y_limits: args.y_limits,
                        x_scale: args.x_scale,
                        y_scale: args.y_scale,
                        aspect: args.aspect,
                        legend: args.legend,
                        figsize: args.figsize,
                    },
                },
            )
        }
        Some(Commands::Validate(args)) => {
            if let Some(limit) = args.timeout {
                runtime::set_load_limit(limit);
            }
            let options = Options::default()
                .narrowing(args.narrowing)
                .target(args.targets)
                .general(args.general);
            validate(&options, args.json)
        }
        Some(Commands::Tag(args)) => {
            let (words, target) = match args.edit {
                TagEdit::Add(args) => (vec!["add".to_string(), args.name], args.target),
                TagEdit::Remove(args) => (vec!["remove".to_string(), args.name], args.target),
                TagEdit::Rename(args) => {
                    (vec!["rename".to_string(), args.old, args.new], args.target)
                }
            };
            let mut options = Options::default()
                .selection(target.selection)
                .general(target.general);
            options.positionals = words;
            let mut targets = target.targets.into_iter();
            if let Some(path) = targets.next() {
                options
                    .positionals
                    .push(path.to_string_lossy().into_owned());
            }
            EXTRA_TARGETS.with(|extra| *extra.borrow_mut() = targets.collect());
            options.write = target.write;
            tag(&options)
        }
        Some(Commands::Init(args)) => {
            // A folder made a project brings the history the project above
            // kept of it: asked, or with --write where no one can answer.
            let folder = args.directory.clone().unwrap_or_else(|| PathBuf::from("."));
            if folder
                .join(samplekit::config::project_config::FILENAME)
                .is_file()
                && let Some(config) =
                    samplekit::config::version_control::projects_of(std::slice::from_ref(&folder))
                        .into_iter()
                        .next()
            {
                bring_history_from_above(&config)?;
            }
            let options = Options {
                write: args.write,
                ..Options::default()
            }
            .general(args.general);
            init_project(
                args.directory.as_deref(),
                &init::Told {
                    example: args.example,
                    no_model: args.no_model,
                    model: args.model,
                    no_venv: args.no_venv,
                },
                &options,
            )
        }
        Some(Commands::New(args)) => {
            let mut options = Options {
                write: args.write,
                ..Options::default()
            }
            .general(args.general);
            options.positionals = args.assignments;
            let keep = match args.keep.as_slice() {
                [sample, fields] => Some((
                    PathBuf::from(sample),
                    fields
                        .split(',')
                        .map(str::trim)
                        .filter(|field| !field.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            };
            // Where it goes: the folder `--into` names, else beside the sample
            // kept from, else here. A configuration there that does not read
            // is said, as `set` says it: `new` wrote under it in silence.
            let folder = args
                .into
                .clone()
                .or_else(|| {
                    keep.as_ref()
                        .and_then(|(sample, _)| sample.parent().map(Path::to_path_buf))
                })
                .unwrap_or_else(|| PathBuf::from("."));
            let unread = configuration_unread(&folder.join("new.md"));
            let outcome = new_sample(&args.name, keep, args.into.as_deref(), &options);
            written_without_configuration(unread, outcome)
        }
        Some(Commands::Set(args)) => {
            let mut options = Options {
                write: args.write,
                ..Options::default()
            }
            .general(args.general);
            // A word holding `=` is a change, any other a target.
            let (changes, targets): (Vec<String>, Vec<String>) =
                args.words.into_iter().partition(|word| word.contains('='));
            options.positionals = changes;
            let targets: Vec<PathBuf> = args
                .before
                .into_iter()
                .chain(targets.into_iter().map(PathBuf::from))
                .collect();
            if targets.is_empty()
                && args.narrowing.filters.is_empty()
                && args.narrowing.queries.is_empty()
            {
                return Err(Fail::usage(
                    "set wants the sample to change: samplekit <file> set <field>=<value>, or a \
                     folder and a selection: samplekit <folder> set -f <expression> <field>=<value>"
                        .to_string(),
                ));
            }
            let files = sample_files(targets, args.narrowing)?;
            match files.as_slice() {
                [] => {
                    outln!("nothing to change — no sample is selected");
                    Ok(())
                }
                [file] => {
                    let unread = configuration_unread(file);
                    let outcome = set_values(file, args.add_row.as_deref(), &options);
                    written_without_configuration(unread, outcome)
                }
                several => set_several(several, args.add_row.as_deref(), &options),
            }
        }
        Some(Commands::Completions(args)) => completions(args.shell),
        Some(Commands::Tui(args)) => {
            let root = args.directory.clone().unwrap_or_else(|| PathBuf::from("."));
            if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
                return Err(Fail::usage(
                    "the TUI needs a terminal: run it where a person reads it".to_string(),
                ));
            }
            // A folder named is chosen; the start page is for none.
            let start = args.directory.is_none() && outside_a_project(&root);
            workbench(&root, start)
        }
    }
}

/// What each file holds that this write drops or changes — a `null`, a YAML
/// tag, an impossible date, an uncertainty beside `n/a` — said in a preview,
/// so that nothing changes in a file without being said.
fn changed_by_this_write<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<String> {
    let mut said = Vec::new();
    for path in paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for sentence in samplekit::format::document::changed_by_a_write(&source) {
            said.push(format!(
                "{}: {}",
                relative_to_here(path),
                sentence.replace("the next write", "this write")
            ));
        }
        // Comments, which `compute` said and `set` and `tag` did not.
        let comments = samplekit::format::document::comments_in(&source);
        if comments > 0 {
            said.push(format!(
                "{}: this write drops {} — remarks belong in the note",
                relative_to_here(path),
                counted_as(comments, "YAML comment", "YAML comments")
            ));
        }
    }
    said
}

/// [`changed_by_this_write`] said under a preview.
fn say_changed_by_this_write<'a>(paths: impl IntoIterator<Item = &'a Path>) {
    let said = changed_by_this_write(paths);
    if said.is_empty() {
        return;
    }
    outln!("\n  what else this write changes:");
    for line in said {
        outln!("    {line}");
    }
}

/// Ctrl-C asked a write of several files to stop after the file under way.
pub(crate) static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `explain --at`: the value explained is one the project held then, written
/// out apart. Its verdict is its day's, and no command is offered for it: one
/// named the temporary folder, and a write `--at` refuses.
pub(crate) static PAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The file `explain --at` was given, as it was typed: what its advice names.
pub(crate) static PAST_TYPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// A write of several files stopped by Ctrl-C: the files written said, and
/// exit 130.
fn stopped(written: &[String]) -> Fail {
    for name in written {
        outln!("  changed {name}");
    }
    Fail {
        code: 130,
        message: format!(
            "interrupted: {} written, the others left as they were",
            counted_as(written.len(), "file", "files")
        ),
    }
}

/// Whether `folder` is outside any project — no `.samplekitrc` in it or above,
/// and no sample file in it — where `tui` opens on its start page.
fn outside_a_project(folder: &Path) -> bool {
    samplekit::tui::start::project_at(folder).is_none()
}

/// The workbench, until it is quit — or interrupted by Ctrl+C, which exits
/// 130 as an interrupted command does.
fn workbench(root: &Path, start: bool) -> Outcome {
    match samplekit::tui::run::run_until(root, start).map_err(Fail::io)? {
        samplekit::tui::run::Ended::Interrupted => Err(Fail {
            code: 130,
            message: String::new(),
        }),
        samplekit::tui::run::Ended::Quit => Ok(()),
    }
}

/// The `-h` page: the `--help` page's options in the same order, without their
/// sections.
fn compact(command: clap::Command) -> clap::Command {
    command
        .mut_args(|argument| {
            // A long list of values is the reference's: on the summary,
            // `--legend`'s eleven places made one line of 280 characters.
            let long_list = argument.get_possible_values().len() > 6;
            argument
                .help_heading(None::<&'static str>)
                .hide_possible_values(long_list)
        })
        .mut_subcommands(compact)
}

/// Whether the page asked for is the sectioned one: `--help` met before `-h`,
/// or a `help` command. Both forms of the command read the same arguments into
/// the same values, so this chooses how a help page is drawn and nothing else.
fn wants_long_help(command: &clap::Command, arguments: &[String]) -> bool {
    let mut current = command;
    for argument in arguments {
        if argument == "help" && current.has_subcommands() {
            return true;
        }
        match current.find_subcommand(argument) {
            Some(subcommand) => current = subcommand,
            None => break,
        }
    }
    arguments
        .iter()
        .take_while(|argument| *argument != "--")
        .find(|argument| *argument == "--help" || *argument == "-h")
        .is_some_and(|argument| argument == "--help")
}

fn clap_failure(error: clap::Error, arguments: &[String]) -> Outcome {
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            out!("{error}");
            Ok(())
        }
        _ => {
            // clap's own message stays: the hint is added beneath it.
            let written = error.to_string();
            // `plot score -x radius`: a figure's name and one axis. clap named the
            // missing `--y`, which is no answer — the figure draws its own.
            if error.kind() == ErrorKind::MissingRequiredArgument
                && let Some(at) = arguments.iter().position(|word| word == "plot")
                && let Some(figure) = arguments
                    .get(at + 1)
                    .filter(|word| !word.starts_with('-') && !Path::new(word).exists())
                && (written.contains("--y") || written.contains("--x"))
            {
                return Err(Fail::usage(format!(
                    "'{figure}' names a figure, and a named figure draws its own axes: drop -x \
                     and -y, or give both, without the figure's name, to draw two fields"
                )));
            }
            let mut message = written.trim_start_matches("error: ").trim_end().to_string();
            if let Some(hint) = misread_target(&written, arguments) {
                message = format!("{hint}\n\n{message}");
            }
            Err(Fail::usage(message))
        }
    }
}

/// `samplekit stal cells` reads `stal` as the target and `cells` as a command,
/// and `samplekit a.md b.md` reads `b.md` as one: say what was meant instead.
fn misread_target(message: &str, arguments: &[String]) -> Option<String> {
    let rest = message.split("the subcommand '").nth(1)?;
    let command = rest.split('\'').next()?;
    let at = arguments.iter().position(|argument| argument == command)?;
    let before = arguments[..at]
        .iter()
        .rev()
        .find(|argument| !argument.starts_with('-'))?;
    if !looks_like_a_path(before)
        && !Path::new(before).exists()
        && let Some(nearest) = samplekit::core::identifier::nearest(
            before,
            listed_commands().iter().map(String::as_str),
        )
    {
        return Some(format!(
            "'{before}' is not a command: did you mean 'samplekit {nearest} {command}'?"
        ));
    }
    Some(format!(
        "one target per command, and '{before}' and '{command}' are two: give their directory, \
         and narrow it with -f or --query"
    ))
}

// ------------------------------------------------------------- the target

/// Where a command naming no target starts: the folder a `--query` declares
/// with `directory`, where the queries given agree on one, and the current
/// folder otherwise. The default only: a target given replaces it. A folder
/// the query declares and that is not there is said as the query's: it was
/// said as if typed, `nowhere: no such file or directory`.
fn default_target(options: &Options) -> Result<PathBuf, Fail> {
    let here = PathBuf::from(".");
    if options.query.is_empty() {
        return Ok(here);
    }
    let Ok(Some(config)) = samplekit::config::project_config::load_for(&here) else {
        return Ok(here);
    };
    let mut folders = options.query.iter().filter_map(|name| {
        let query = named_queries::named(&config, name).ok()?;
        let written = query.directory.as_ref()?;
        Some((
            name,
            written,
            named_queries::directory_of(query, &config, None),
        ))
    });
    match (folders.next(), folders.next()) {
        (Some((name, written, one)), None) if !one.is_dir() => Err(Fail::usage(format!(
            "[query.{name}] directory names '{}', which is no folder here — in {}",
            written.display(),
            config.root().join(".samplekitrc").display()
        ))),
        (Some((_, _, one)), None) => Ok(relative_to_here_path(&one)),
        _ => Ok(here),
    }
}

/// A folder as a command would have been given it from here, so that what it
/// says of its samples reads as if typed.
fn relative_to_here_path(folder: &Path) -> PathBuf {
    let here = std::env::current_dir()
        .ok()
        .and_then(|here| dunce::canonicalize(here).ok());
    let full = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    here.and_then(|here| full.strip_prefix(&here).ok().map(Path::to_path_buf))
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(full)
}

/// Whether a word names no file and no folder — a link that leads nowhere
/// is one that exists.
fn names_nothing(word: &str) -> bool {
    let path = Path::new(word);
    !path.exists() && path.symlink_metadata().is_err()
}

/// The words after `explain` or `open`, told apart by what they are: the
/// paths written before the command, every word naming a file or a folder,
/// and every word holding `/`, which no field nor pattern of a sample's own
/// files holds, are targets; the one word naming nothing is the command's
/// own, `what` saying what it is. Two such words are refused.
fn targets_and_word(
    before: Vec<PathBuf>,
    words: Vec<String>,
    what: &str,
) -> Result<(Vec<PathBuf>, Option<String>), Fail> {
    let mut targets = before;
    let mut own: Vec<String> = Vec::new();
    for word in words {
        // `\` is a separator on Windows, where a path written with it is
        // one as `/` makes one.
        let slashed = word.contains('/') || (cfg!(windows) && word.contains('\\'));
        if names_nothing(&word) && !slashed {
            own.push(word);
        } else {
            targets.push(PathBuf::from(word));
        }
    }
    if own.len() > 1 {
        return Err(Fail::usage(format!(
            "one {what} is taken, and {} name no file: is a path mistyped?",
            own.iter()
                .map(|word| shell_word(word))
                .collect::<Vec<_>>()
                .join(" and ")
        )));
    }
    Ok((targets, own.pop()))
}

/// The sample files a command acts on one by one: its targets as given, or,
/// where a folder is among them or a selection narrows them, every sample
/// they hold that the selection keeps. No target is the current folder.
fn sample_files(targets: Vec<PathBuf>, narrowing: FilterArgs) -> Result<Vec<PathBuf>, Fail> {
    let narrowed = !narrowing.filters.is_empty() || !narrowing.queries.is_empty();
    if !narrowed && !targets.is_empty() && targets.iter().all(|target| !target.is_dir()) {
        return Ok(targets);
    }
    let options = Options::default().narrowing(narrowing).target(targets);
    let (root, _) = target_and_extra(&options)?;
    let collection = loaded(&root)?;
    let kept = if narrowed {
        select(&collection, &options, &root)?
    } else {
        collection.filter_by(|_| true)
    };
    EXTRA_TARGETS.with(|extra| extra.borrow_mut().clear());
    Ok(kept.iter().filter_map(|entry| entry.path.clone()).collect())
}

/// The targets a history command reads: as given, or the files of the samples
/// a selection keeps among them — as they are now, and as the snapshots
/// `kept` named held them, so that a sample deleted since is kept by the
/// selection it met. A snapshot that cannot be read adds nothing.
fn history_targets(
    targets: Vec<PathBuf>,
    narrowing: FilterArgs,
    kept: &[&str],
) -> Result<Vec<PathBuf>, Fail> {
    if narrowing.filters.is_empty() && narrowing.queries.is_empty() {
        return Ok(targets);
    }
    let mut files = sample_files(targets.clone(), narrowing.clone())?;
    for when in kept {
        if let Ok(Some(then)) = history::selected_at(&targets, &narrowing, when) {
            for file in then {
                let same = |held: &PathBuf| {
                    dunce::canonicalize(held).ok() == dunce::canonicalize(&file).ok()
                        && held.exists()
                        || *held == file
                };
                if !files.iter().any(same) {
                    files.push(file);
                }
            }
        }
    }
    Ok(files)
}

/// One file however written: the same where both are there, else the same
/// words.
fn same_file(held: &Path, file: &Path) -> bool {
    dunce::canonicalize(held).ok() == dunce::canonicalize(file).ok() && held.exists()
        || held == file
}

/// The files of the samples a selection met in any version the history
/// keeps of them, among `targets`: what `log` of a selection lists, a sample
/// that left it since among them. `log --query titaniums` lost the TITANIUM made a bronze
/// alloy since, and the snapshots that changed it.
fn ever_selected(targets: Vec<PathBuf>, narrowing: FilterArgs) -> Result<Vec<PathBuf>, Fail> {
    let options = Options::default()
        .narrowing(narrowing)
        .target(targets.clone());
    let (root, _) = target_and_extra(&options)?;
    let base = loaded(&root)?;
    EXTRA_TARGETS.with(|extra| extra.borrow_mut().clear());
    // A state is how a sample stands today: no version kept has one.
    if states_wanted(&base, &options).is_some() {
        return Ok(Vec::new());
    }
    let places: Vec<PathBuf> = if targets.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        targets
    };
    let within = |file: &Path| {
        let file = dunce::canonicalize(file.parent().unwrap_or(file))
            .map(|folder| folder.join(file.file_name().unwrap_or_default()))
            .unwrap_or_else(|_| file.to_path_buf());
        places.iter().any(|place| {
            dunce::canonicalize(place).is_ok_and(|place| {
                if place.is_dir() {
                    file.starts_with(&place)
                } else {
                    file == place
                }
            })
        })
    };
    let mut entries = Vec::new();
    for config in samplekit::config::version_control::projects_of(&places) {
        let versions = samplekit::config::version_control::sample_versions(&config)
            .map_err(|error| Fail::io(error.to_string()))?;
        let folder = relative_to_here_path(config.root());
        for (name, bytes) in versions {
            let path = folder.join(&name);
            if !within(&path) {
                continue;
            }
            // A version that no longer reads is no sample of the selection.
            let Ok(sample) = samplekit::format::document::sample_from_text(
                &String::from_utf8_lossy(&bytes),
                &path,
            ) else {
                continue;
            };
            entries.push(samplekit::collection::sample_list::Entry {
                path: Some(path),
                sample: std::rc::Rc::new(std::cell::RefCell::new(sample)),
                states: None,
            });
        }
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let met = select(&base.holding(entries), &options, &root)?;
    let mut files: Vec<PathBuf> = Vec::new();
    for path in met.iter().filter_map(|entry| entry.path.clone()) {
        if !files.contains(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

/// What a command operates on. A path says *where to start*; `--query` and `-f`
/// narrow it.
fn target_and_extra(options: &Options) -> Result<(PathBuf, Option<String>), Fail> {
    match options.positionals.len() {
        0 => Ok((default_target(options)?, None)),
        1 => Ok((PathBuf::from(&options.positionals[0]), None)),
        _ => Ok((
            PathBuf::from(&options.positionals[0]),
            Some(options.positionals[1].clone()),
        )),
    }
}

/// The listed commands, for a word that is neither a path nor one of them:
/// read from the grammar, so that a command added there is suggested too —
/// the list written here once lacked `restore`, `init`, `new`, `set` and `tui`.
fn listed_commands() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .filter(|command| !command.is_hide_set())
        .map(|command| command.get_name().to_string())
        .collect()
}

/// The command a mistyped word was meant for: by edit distance counting a
/// swap of two letters as one, so that `lgo` names `log` as `resore` names
/// `restore`.
fn nearest_command(written: &str) -> Option<String> {
    let budget = written.chars().count().max(1);
    listed_commands()
        .into_iter()
        .map(|command| (strsim::osa_distance(&command, written), command))
        .filter(|(distance, _)| *distance > 0 && distance * 3 <= budget)
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, command)| command)
}

/// Whether the command line names no command: a word it holds then stands
/// where a command would, and may be one mistyped.
static NO_COMMAND: AtomicBool = AtomicBool::new(false);

/// A preview's question answered yes: the command runs again with `--write`.
static APPLY: AtomicBool = AtomicBool::new(false);

/// While several samples are previewed one by one, their question waits for
/// the last: [`TO_APPLY`] counts those that would change.
pub(crate) static ASK_AFTER: AtomicBool = AtomicBool::new(false);
/// While several samples are written one by one, `written:` waits for the
/// last, and is said once for them all.
pub(crate) static WRITTEN_AFTER: AtomicBool = AtomicBool::new(false);
static TO_APPLY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `set` on several samples: each previewed, one question for them all. With
/// `--write`, every change is checked first, saying nothing, and one that a
/// sample refuses writes none of them.
fn set_several(files: &[PathBuf], add_row: Option<&str>, options: &Options) -> Outcome {
    let preview = Options {
        write: false,
        ..options.clone()
    };
    // A configuration that does not read is said, and the samples changed
    // under it all the same: the command then exits 2, as one sample's does.
    let unread = std::cell::Cell::new(false);
    let each = |file: &PathBuf, options: &Options| {
        if !QUIET.load(AtomicOrdering::Relaxed) && configuration_unread(file) {
            unread.set(true);
        }
        set_values(file, add_row, options)
    };
    ASK_AFTER.store(true, AtomicOrdering::Relaxed);
    TO_APPLY.store(0, AtomicOrdering::Relaxed);
    let checked = if options.write {
        let gone = STDOUT_GONE.swap(true, AtomicOrdering::Relaxed);
        let quiet = QUIET.swap(true, AtomicOrdering::Relaxed);
        let refused = files
            .iter()
            .find_map(|file| each(file, &preview).err().map(|fail| (file, fail)));
        STDOUT_GONE.store(gone, AtomicOrdering::Relaxed);
        QUIET.store(quiet, AtomicOrdering::Relaxed);
        match refused {
            // Said as a preview of that file says it: what was checked in
            // silence was said nowhere, its warnings with it.
            Some((file, _)) => {
                let shown = each(file, &preview);
                ASK_AFTER.store(false, AtomicOrdering::Relaxed);
                return match shown {
                    Err(fail) => Err(fail),
                    Ok(()) => Err(Fail::data(format!(
                        "{}: this change could not be checked; nothing was written",
                        relative_to_here(file)
                    ))),
                };
            }
            None => Ok(()),
        }
    } else {
        files.iter().try_for_each(|file| each(file, &preview))
    };
    ASK_AFTER.store(false, AtomicOrdering::Relaxed);
    checked?;
    if options.write {
        WRITTEN_AFTER.store(true, AtomicOrdering::Relaxed);
        let mut written: Vec<String> = Vec::new();
        let mut outcome = Ok(());
        for file in files {
            if let Err(fail) = each(file, options) {
                outcome = Err(fail);
                break;
            }
            written.push(relative_to_here(file));
        }
        WRITTEN_AFTER.store(false, AtomicOrdering::Relaxed);
        if !written.is_empty() {
            outln!();
            for line in written_lines(&written) {
                outln!("{line}");
            }
        }
        outcome?;
        return written_without_configuration(unread.get(), Ok(()));
    }
    if TO_APPLY.swap(0, AtomicOrdering::Relaxed) > 0 {
        outln!();
        offer_to_apply("apply these changes?", "");
        // A yes writes them, and that write exits 2.
        return Ok(());
    }
    outln!("\nnothing to change — every value is already what it was asked to be");
    written_without_configuration(unread.get(), Ok(()))
}

/// How many values `compute`'s plan would compute: asked before they are.
static TO_COMPUTE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A question answered yes or no, no unless yes is said: one key at a
/// terminal, the answer repeated and the line ended; a line read otherwise,
/// so that `yes | samplekit compute` answers, and an empty input is no.
fn ask(question: &str) -> bool {
    use std::io::Write as _;
    let _ = io::stdout().flush();
    // A question the one at the terminal cannot read — stderr closed, sent
    // to a file or through `| head` — is answered no, unseen answers being
    // no answers; and a reader that has gone never panics the command.
    let mut err = io::stderr();
    if io::stdin().is_terminal() && !err.is_terminal() {
        return false;
    }
    if write!(err, "{question} [y/N] ")
        .and_then(|()| err.flush())
        .is_err()
    {
        return false;
    }
    let line = || {
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => false,
            Ok(_) => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        }
    };
    let yes = if io::stdin().is_terminal() {
        match one_key() {
            Some(Key::Yes) => true,
            Some(Key::No) => false,
            // Ctrl-C at the question is an interruption, as anywhere else:
            // nothing was written, and the command exits 130.
            Some(Key::Interrupt) => {
                let _ = writeln!(err, "^C");
                let _ = writeln!(err, "interrupted: nothing written");
                std::process::exit(130);
            }
            None => line(),
        }
    } else {
        line()
    };
    let _ = writeln!(err, "{}", if yes { "y" } else { "n" });
    yes
}

/// A key answering a question.
enum Key {
    Yes,
    No,
    Interrupt,
}

/// One key, read in raw mode: `y` is yes, Ctrl-C an interruption, anything
/// else — Enter, Esc — no. `None` where the terminal takes no raw mode.
///
/// Keys typed before the question are no answer to it: the Enter typed after
/// the `y` of `compute these values?` waited through the computation, then
/// answered `apply these changes?` no, and the computation was thrown away.
/// They are read and dropped first. Answers piped in are lines, read as they
/// come: `yes | samplekit compute` still answers every question.
fn one_key() -> Option<Key> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    crossterm::terminal::enable_raw_mode().ok()?;
    // An input that ended — Ctrl-D, or the NUL a terminal whose input is
    // closed sends — is no key typed ahead: it answers no, as it did.
    let mut ended = false;
    while crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
        match crossterm::event::read() {
            // Ctrl-D, or the NUL a terminal whose input closed sends.
            Ok(Event::Key(key))
                if matches!(key.code, KeyCode::Char('d' | ' ' | '@'))
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                ended = true;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    if ended {
        let _ = crossterm::terminal::disable_raw_mode();
        return Some(Key::No);
    }
    let answer = loop {
        match crossterm::event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                break match key.code {
                    KeyCode::Char('c' | 'C') if control => Key::Interrupt,
                    KeyCode::Char('y' | 'Y') if !control => Key::Yes,
                    _ => Key::No,
                };
            }
            Ok(_) => {}
            Err(_) => break Key::No,
        }
    };
    let _ = crossterm::terminal::disable_raw_mode();
    Some(answer)
}

/// After a preview of a write: asked, and on yes the command runs again with
/// `--write`; on no, how to write it later.
fn offer_to_apply(question: &str, _advice_text: &str) {
    // Several samples changed by one command are asked once, after them all.
    if ASK_AFTER.load(AtomicOrdering::Relaxed) {
        TO_APPLY.fetch_add(1, AtomicOrdering::Relaxed);
        return;
    }
    // One question and one answer on every command: `nothing written`.
    let _ = question;
    if ask("apply these changes?") {
        APPLY.store(true, AtomicOrdering::Relaxed);
    } else {
        outln!("nothing written");
    }
}

/// Every word that names a command, a hidden one and `help` included: what a
/// target can never be read as.
/// A command line written with its paths or its options first, `ana/ tom/
/// status -f …`, as the command takes it, `status ana/ tom/ -f …`, with where
/// in the words after the command the paths went and how many there were;
/// `None` where nothing comes before the command, or no command follows.
/// **Every word before the command is a path**, there or not: one missing is
/// then said missing by the command. Options written before the command are
/// the command's. The command is the first word naming one, and wins over a
/// folder of its name: `status/` is the folder. Where a command tells its
/// own words from its paths by what they are — `set`'s changes, `explain`'s
/// field, `open`'s pattern — a path written first is given by `--target`, so
/// that nothing about it is guessed.
fn path_first(words: &[String]) -> Option<(Vec<String>, usize, usize)> {
    let mut paths: Vec<String> = Vec::new();
    let mut options: Vec<String> = Vec::new();
    let mut at_word = 0;
    let command = loop {
        let word = words.get(at_word)?;
        if word == "--" {
            return None;
        }
        if word.starts_with('-') && word.len() > 1 {
            options.push(word.clone());
            if takes_a_value(word) {
                options.push(words.get(at_word + 1)?.clone());
                at_word += 1;
            }
            at_word += 1;
            continue;
        }
        if is_command(word) {
            if paths.is_empty() && options.is_empty() {
                return None;
            }
            break word.clone();
        }
        paths.push(word.clone());
        at_word += 1;
    };
    let after = &words[at_word + 1..];
    let count = paths.len();
    match command.as_str() {
        // `samplekit pieces help`: the help, of what follows if anything.
        "help" => {
            let mut moved = vec![command];
            moved.extend(after.iter().cloned());
            return Some((moved, 0, count));
        }
        "completions" => return None,
        _ => {}
    }
    // Where the command takes its targets: after the names it takes first.
    let at = match command.as_str() {
        // `new` makes a sample in a folder: `--into`.
        "new" => {
            let mut moved = vec![command.clone()];
            moved.extend(after.iter().cloned());
            for path in &paths {
                moved.push("--into".to_string());
                moved.push(path.clone());
            }
            moved.extend(options);
            return Some((moved, after.len(), count));
        }
        // Told from their own words by what they are, not by their place.
        "set" | "explain" | "open" => {
            let mut moved = vec![command.clone()];
            moved.extend(after.iter().cloned());
            for path in &paths {
                moved.push("--target".to_string());
                moved.push(path.clone());
            }
            moved.extend(options);
            return Some((moved, after.len(), count));
        }
        _ => names_first(&command, after),
    };
    // A folder named as a kind is a folder before `list`: `samplekit tags
    // list` lists the folder, as `./tags` says it after the command.
    let paths = paths.into_iter().map(|path| {
        if command == "list" && KINDS.contains(&path.as_str()) {
            format!("./{path}")
        } else {
            path
        }
    });
    let mut moved = vec![command.clone()];
    moved.extend(after[..at].iter().cloned());
    moved.extend(paths);
    moved.extend(after[at..].iter().cloned());
    moved.extend(options);
    Some((moved, at, count))
}

/// How many of the words after `command` are the names it takes before its
/// targets: `export NAME`, `plot NAME`, `run NAME`, `tag add NAME`, `tag
/// rename OLD NEW`, and `list`'s kind — or a word being typed that is no path.
fn names_first(command: &str, after: &[String]) -> usize {
    let leading = |most: usize| {
        after
            .iter()
            .take(most)
            .take_while(|word| !word.starts_with('-'))
            .count()
    };
    match command {
        "export" | "plot" | "run" | "rename" => leading(1),
        "tag" => match after.first().map(String::as_str) {
            Some("add" | "remove") => leading(2),
            Some("rename") => leading(3),
            _ => 0,
        },
        "list" => usize::from(after.first().is_some_and(|word| {
            KINDS.contains(&word.as_str()) || (!word.starts_with('-') && !Path::new(word).exists())
        })),
        // `view curve`: a table's name, no file.
        "view" => usize::from(
            after
                .first()
                .is_some_and(|word| !word.starts_with('-') && !Path::new(word).exists()),
        ),
        _ => 0,
    }
}

/// A filter of several words written unquoted, `-f tags has todo`: `-f`
/// takes one word, and the others would be read as paths. Said where a word
/// after it names no file and is one of the filter language's own — an
/// operator, `has`, `contains` — so that a mistyped path is never taken for
/// one.
fn unquoted_filter(words: &[String]) -> Option<String> {
    const OWN: &[&str] = &[
        "has",
        "contains",
        "starts_with",
        "in",
        "is",
        "where",
        "missing",
        "present",
        "==",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "&&",
        "||",
    ];
    let mut at = 0;
    while at < words.len() {
        let word = &words[at];
        if word == "--" {
            return None;
        }
        if (word == "-f" || word == "--filter") && at + 1 < words.len() {
            let rest: Vec<&String> = words[at + 2..]
                .iter()
                .take_while(|next| {
                    !next.starts_with('-') && !is_command(next) && names_nothing(next)
                })
                .collect();
            if let Some(own) = rest.iter().find(|next| OWN.contains(&next.as_str())) {
                let whole = std::iter::once(&words[at + 1])
                    .chain(rest.iter().copied())
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                return Some(format!(
                    "{own}: no such file — a filter of several words is quoted: {word} {}",
                    shell_word(&whole)
                ));
            }
        }
        at += 1;
    }
    None
}

/// Whether an option written alone, `-f` or `--filter`, takes the word after
/// it as its value — asked of every command's options, which are what can
/// stand between the paths and the command.
fn takes_a_value(word: &str) -> bool {
    if word.contains('=') {
        return false;
    }
    let root = Cli::command();
    let commands = std::iter::once(&root).chain(root.get_subcommands());
    let mut arguments = commands.flat_map(clap::Command::get_arguments);
    if let Some(long) = word.strip_prefix("--") {
        return arguments.any(|argument| {
            argument.get_long() == Some(long) && argument.get_action().takes_values()
        });
    }
    // `-f expr`, never `-fexpr` nor `-vq`, which hold their value or are flags.
    let mut letters = word[1..].chars();
    match (letters.next(), letters.next()) {
        (Some(short), None) => arguments.any(|argument| {
            argument.get_short() == Some(short) && argument.get_action().takes_values()
        }),
        _ => false,
    }
}

fn command_words() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .chain(std::iter::once("help".to_string()))
        .collect()
}

fn looks_like_a_path(written: &str) -> bool {
    written.contains('/')
        || written.starts_with('.')
        || written.starts_with('~')
        || written.ends_with(".md")
}

/// The collection a file target belongs to: the directory it lives in, or `None`
/// for a directory target, which is its own collection.
///
/// **A vocabulary belongs to the collection, not to the target.** A sample
/// without `comment` is data. Checking a view against that one file warned *no
/// sample has 'comment'* above a rendering that already showed the gap, and a
/// column or a filter would have been refused outright.
fn siblings_of(target: &Path) -> Option<SampleList> {
    // Several targets are their own vocabulary.
    if !target.is_file() || EXTRA_TARGETS.with(|extra| !extra.borrow().is_empty()) {
        return None;
    }
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    list::from_directory(parent).ok()
}

/// Loads a collection from a directory or a single file.
///
/// A file finds its project by the same upward search a directory does, so a
/// sample validated on its own resolves its units and precision exactly as it
/// does inside its collection.
fn loaded(target: &Path) -> Result<SampleList, Fail> {
    let extra = EXTRA_TARGETS.with(|extra| extra.borrow().clone());
    let collection = if extra.is_empty() {
        loaded_one(target)?
    } else {
        loaded_targets(target, &extra)?
    };
    for config in configurations_of(&collection) {
        bring_history_from_above(config)?;
    }
    // The project's colours, over several the root's, as its tables are.
    render::set_palette(render::Palette::of(collection.config(), coloured()));
    said_loaded(&collection);
    Ok(collection)
}

/// The name beside a missing path nearest to it, `pieces/alpha-titanium.md` for
/// `pieces/alpha-ipaa.md`: a typo is told the file it meant.
fn nearest_path(target: &Path) -> Option<PathBuf> {
    let written = target.file_name()?.to_string_lossy().into_owned();
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let names: Vec<String> = std::fs::read_dir(parent.unwrap_or(Path::new(".")))
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    let near = samplekit::core::identifier::nearest(&written, names.iter().map(String::as_str))?;
    Some(parent.map_or_else(|| PathBuf::from(&near), |parent| parent.join(&near)))
}

/// One target, read as it is, saying nothing.
fn loaded_one(target: &Path) -> Result<SampleList, Fail> {
    if !target.exists() {
        // `exists` follows links, so a symbolic loop answers *false* and used
        // to be reported as *no such file or directory* — which is not what
        // happened, and sends the reader looking for a missing file that is
        // right there. `symlink_metadata` sees the link itself, so the real
        // reason can be asked for and said. A scan already says it correctly;
        // only a target named outright did not.
        if target.symlink_metadata().is_ok()
            && let Err(reason) = std::fs::metadata(target)
        {
            return Err(Fail::io(format!("{}: {reason}", target.display())));
        }
        // A word that is no path may be a mistyped command — where it stands
        // in a command's place: inside `tag add x nosuch` it is a file.
        let written = target.to_string_lossy();
        let in_place_of_a_command =
            NO_COMMAND.load(AtomicOrdering::Relaxed) && !looks_like_a_path(&written);
        let nearest = in_place_of_a_command
            .then(|| nearest_command(&written))
            .flatten();
        let hint = if !in_place_of_a_command {
            String::new()
        } else {
            match &nearest {
                Some(command) => format!("\n  nor a command: did you mean '{command}'?"),
                None => format!("\n  nor a command: {}", listed_commands().join(", ")),
            }
        };
        let hint = if hint.is_empty() {
            nearest_path(target)
                .map(|near| format!("\n  did you mean '{}'?", near.display()))
                .unwrap_or_default()
        } else {
            hint
        };
        let message = format!("{}: no such file or directory{hint}", target.display());
        // A command mistyped is a usage error, 1: nothing on the disk failed.
        if nearest.is_some() {
            return Err(Fail::usage(message));
        }
        return Err(Fail::io(message));
    }
    let forced = forced_configuration();
    let result = match (&forced, target.is_dir()) {
        (Some((_, config)), true) => list::from_directory_with(target, Some(config.clone())),
        (Some((_, config)), false) => {
            list::from_files_with(&[target.to_path_buf()], Some(config.clone()))
        }
        (None, true) => list::from_directory(target),
        (None, false) => list::from_files(&[target.to_path_buf()]),
    };
    let collection = result.map_err(|error| match error {
        // A file that could not be *read* is an I/O error, exit 3 — a missing
        // path already exits 3, and a permission refused is the same class of
        // thing. It is also the one case where the migration hint is nonsense:
        // nothing about the file's contents is known.
        list::ListError::Document {
            error: document::DocumentError::Io { .. },
            ..
        } => Fail::io(error.to_string()),
        // A file that will not parse may predate schema 1: a data error whose
        // cause is worth naming, even where the remedy is not this tool's. A
        // configuration error is not that, and gets no such hint.
        list::ListError::Document {
            error: document::DocumentError::MissingFrontmatter,
            ..
        } => Fail::data(not_a_sample(target)),
        list::ListError::Document { .. } if error.to_string().contains("newer SampleKit") => {
            Fail::data(error.to_string())
        }
        list::ListError::Document { .. } => Fail::data(error.to_string()),
        other => Fail::data(other.to_string()),
    })?;
    Ok(collection)
}

/// Whether a file of what this command loaded could not be read.
static UNREAD: AtomicBool = AtomicBool::new(false);
/// Whether one was read and understood: a file the filesystem refused alone
/// exits 3, as a path named directly does; a malformed one is the data's, 2.
static MALFORMED: AtomicBool = AtomicBool::new(false);

/// What every command says about what it read.
fn said_loaded(collection: &SampleList) {
    // A copy a sync conflict left is no sample, and counts in nothing: said by
    // every command, since it waits for its owner to compare the two.
    for skipped in collection.skipped() {
        if matches!(
            skipped.reason,
            samplekit::config::discovery::SkipReason::SyncConflict { .. }
        ) {
            warn(&format!(
                "{} was left out: {}",
                shown_path(&skipped.path),
                skipped.reason
            ));
        }
    }
    // A file set aside unread is said by every command, not only with -v;
    // several are one line, each named under -v.
    let unread: Vec<_> = collection
        .skipped()
        .iter()
        .filter(|skipped| is_unread(skipped))
        .collect();
    if !unread.is_empty() {
        UNREAD.store(true, AtomicOrdering::Relaxed);
    }
    if unread.iter().any(|skipped| {
        !matches!(
            skipped.reason,
            samplekit::config::discovery::SkipReason::Unreadable { .. }
        )
    }) {
        MALFORMED.store(true, AtomicOrdering::Relaxed);
    }
    if REPORTING_UNREAD.load(AtomicOrdering::Relaxed) {
        return;
    }
    match unread.as_slice() {
        [] => {}
        [one] => warn(&unread_message(one)),
        // `list skipped` shows more than this count: it also names the files
        // that are **not samples** — an `.md` with no frontmatter — which this
        // banner deliberately leaves out, because in a vault of notes it would
        // be noise on every command. The wording used to promise that the list
        // named *them*, and it named more.
        several => warn(&format!(
            "{} not read — -v shows them; samplekit list skipped adds what is not a sample",
            counted_as(several.len(), "file was", "files were")
        )),
    }
    // That a selection spans configurations is said under -v.
    if let Some((path, _)) = forced_configuration() {
        warn_ignored_configurations(collection, &path);
    }
}

/// Several targets, as a shell glob gives them: files and directories, each
/// sample described by its nearest configuration.
fn loaded_targets(first: &Path, extra: &[PathBuf]) -> Result<SampleList, Fail> {
    let mut paths = vec![first.to_path_buf()];
    paths.extend(extra.iter().cloned());
    for path in &paths {
        if !path.exists() {
            // A word that is no path may be a mistyped command before targets.
            let written = path.to_string_lossy();
            if !looks_like_a_path(&written)
                && let Some(command) = samplekit::core::identifier::nearest(
                    &written,
                    listed_commands().iter().map(String::as_str),
                )
            {
                let rest: Vec<String> = paths
                    .iter()
                    .filter(|other| *other != path)
                    .map(|other| other.display().to_string())
                    .collect();
                return Err(Fail::usage(format!(
                    "'{written}' is not a command: did you mean 'samplekit {command} {}'?",
                    rest.join(" ")
                )));
            }
            return Err(Fail::io(format!(
                "{}: no such file or directory",
                path.display()
            )));
        }
    }
    // A target given twice, as overlapping globs give it, is read once.
    let given = paths.len();
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(dunce::canonicalize(path).unwrap_or_else(|_| path.clone())));
    if paths.len() < given {
        warn(&format!(
            "{} given more than once, read once",
            counted_as(given - paths.len(), "target was", "targets were")
        ));
    }
    let mut lists = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    for path in &paths {
        if path.is_file() && !is_markdown(path) {
            ignored.push(format!(
                "{} is not a Markdown sample file, and was ignored",
                path.display()
            ));
            continue;
        }
        lists.push(loaded_one(path)?);
    }
    warn_several(
        &format!(
            "{} targets are not Markdown sample files, and were ignored",
            ignored.len()
        ),
        &ignored,
    );
    if lists.is_empty() {
        return Err(Fail::usage(
            "no target is a Markdown sample file or a directory".to_string(),
        ));
    }
    Ok(list::merged(lists))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "md" | "MD" | "markdown"))
}

/// *None was found* is false of a configuration that was found and set aside:
/// the warning above names it, and this must not send the reader looking for a
/// file that is there.
fn needs_configuration(collection: &SampleList, what: &str) -> String {
    let set_aside = collection.skipped().iter().any(|skipped| {
        matches!(
            skipped.reason,
            samplekit::config::discovery::SkipReason::Configuration { .. }
        )
    });
    if set_aside {
        format!(
            "{what} needs a .samplekitrc, and the one found here was not read — samplekit validate says why"
        )
    } else {
        format!("{what} needs a .samplekitrc, and none was found above this directory")
    }
}

fn warn_skipped(collection: &SampleList, options: &Options) {
    if !options.verbose {
        return;
    }
    note_configuration(collection);
    for warning in collection.warnings() {
        let samplekit::config::discovery::Warning::MixedConfigurations { roots } = warning;
        errln!(
            "this selection spans {} configurations, each sample described by the nearest one:",
            roots.len()
        );
        for root in roots {
            errln!("  {}", root.display());
        }
    }
    let unread: Vec<_> = collection
        .skipped()
        .iter()
        .filter(|skipped| is_unread(skipped))
        .collect();
    if unread.len() > 1 {
        for skipped in unread {
            warn(&unread_message(skipped));
        }
    }
    for skipped in collection
        .skipped()
        .iter()
        .filter(|skipped| !is_unread(skipped))
    {
        warn(&format!(
            "skipped {}: {}",
            skipped.path.display(),
            skipped.reason
        ));
    }
    // What the include patterns leave out is no candidate, and said here as
    // `list skipped` says it: a pattern written too narrowly left samples out
    // of every table without a word, `-v` included.
    for config in configurations_of(collection) {
        for path in
            samplekit::config::discovery::outside_include(config.root(), config.collection())
        {
            warn(&format!(
                "skipped {}: matches no include pattern",
                relative_to_here(&path)
            ));
        }
    }
}

fn is_unread(skipped: &samplekit::config::discovery::Skipped) -> bool {
    matches!(
        skipped.reason,
        samplekit::config::discovery::SkipReason::Malformed { .. }
            | samplekit::config::discovery::SkipReason::Unreadable { .. }
            | samplekit::config::discovery::SkipReason::Configuration { .. }
    )
}

/// Why a file was not read.
fn unread_message(skipped: &samplekit::config::discovery::Skipped) -> String {
    // A configuration's own error already opens with its path and line: said
    // under *was not read: not read as a configuration:* it named the file
    // three times before saying what was wrong with it.
    match &skipped.reason {
        samplekit::config::discovery::SkipReason::Configuration { message } => {
            format!("configuration not read, and the samples read without it — {message}")
        }
        reason => format!("{} was not read: {reason}", shown_path(&skipped.path)),
    }
}

/// Warnings go to stderr and do not change the exit code.
fn warn(message: &str) {
    if QUIET.load(AtomicOrdering::Relaxed) {
        return;
    }
    // Said once in a run: a preview answered yes, run again to write, said
    // each of its warnings twice.
    static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut said) = SAID.lock() {
        if said.iter().any(|held| held == message) {
            return;
        }
        said.push(message.to_string());
    }
    // One warning is one `warning:`, its further lines indented beneath it. A
    // prefix on every line split one warning into several, and a blank line
    // of a reason became a `warning:` saying nothing.
    let mut lines = message.lines().filter(|line| !line.trim().is_empty());
    let Some(first) = lines.next() else {
        return;
    };
    // In the `message` colour on a terminal, as every line `errln!` writes.
    errln!("warning: {first}");
    for line in lines {
        if line.starts_with(' ') {
            errln!("{line}");
        } else {
            errln!("  {line}");
        }
    }
}

/// What ends a line counting several warnings of one kind.
const SEVERAL: &str = "-v shows them";

/// Several warnings of one kind, said once: one alone as it is; more, as one
/// plain line counting them — `summary` — and, under `-v`, each indented
/// beneath it.
fn warn_several(summary: &str, each: &[String]) {
    match each {
        [] => {}
        [one] => warn(one),
        several if VERBOSE.load(AtomicOrdering::Relaxed) => {
            let mut said = format!("{summary}:");
            for one in several {
                for (at, line) in one.lines().enumerate() {
                    said.push_str(if at == 0 { "\n    " } else { "\n      " });
                    said.push_str(line.trim_start());
                }
            }
            warn(&said);
        }
        _ => warn(&format!("{summary} — {SEVERAL}")),
    }
}

/// One kind of warning a command may give several times from several places
/// — once per configuration, once per file written — gathered as it runs and
/// said as it ends, by [`said_gathered`].
struct Gathered {
    kind: &'static str,
    /// The line counting them, given how many.
    summary: fn(usize) -> String,
    each: Vec<String>,
    /// How many times one was said, alike or not.
    times: usize,
}

static GATHERED: std::sync::Mutex<Vec<Gathered>> = std::sync::Mutex::new(Vec::new());

/// A warning of `kind`, said with the others of its kind when the command ends:
/// `summary` counts them where there are several. One said twice is said once.
fn warn_gathered(kind: &'static str, summary: fn(usize) -> String, message: String) {
    let Ok(mut gathered) = GATHERED.lock() else {
        warn(&message);
        return;
    };
    match gathered.iter_mut().find(|held| held.kind == kind) {
        Some(held) => {
            // Counted each time, said once: three alike of one sample were
            // counted one, eight said for twelve logged.
            held.times += 1;
            if !held.each.contains(&message) {
                held.each.push(message);
            }
        }
        None => gathered.push(Gathered {
            kind,
            summary,
            each: vec![message],
            times: 1,
        }),
    }
}

/// Every warning gathered, each kind once, in the order the kinds were met.
fn said_gathered() {
    let gathered = match GATHERED.lock() {
        Ok(mut gathered) => std::mem::take(&mut *gathered),
        Err(_) => return,
    };
    for kind in gathered {
        warn_several(&(kind.summary)(kind.times), &kind.each);
    }
}

// ---------------------------------------------------------------- selection

/// A selection is a base and a narrowing. **`--query` and `-f` compose**: both
/// predicates, conjoined, because they are the same kind of operation.
fn select(collection: &SampleList, options: &Options, target: &Path) -> Result<SampleList, Fail> {
    // `state` is read only where a filter, a query or a column names it, and
    // the model only where a word or the column needs it.
    let stated;
    let collection = match states_wanted(collection, options) {
        Some(with_model) => {
            stated = with_states(collection, with_model)?;
            &stated
        }
        None => collection,
    };
    if options.query.is_empty() || !collection.spans_configurations() {
        return select_with(collection, options, target, &options.query);
    }
    // Over several configurations, a query selects among the samples of a
    // configuration declaring it, and no others.
    let narrowed = select_with(collection, options, target, &[])?;
    let mut kept: std::collections::HashSet<*const samplekit::core::sample::Sample> =
        std::collections::HashSet::new();
    let mut declared = vec![false; options.query.len()];
    let mut undeclared = Vec::new();
    for part in collection.by_configuration(&narrowed) {
        let Some(config) = part.config else {
            undeclared.push((part.file.clone(), part.samples.len()));
            continue;
        };
        let mut expressions = Vec::new();
        for (position, name) in options.query.iter().enumerate() {
            if let Ok(query) = named_queries::named(config, name) {
                declared[position] = true;
                // Checked as written, before the conjunction below is built: an
                // error pointed into `(diameter >)`, a string nobody wrote, and
                // named neither the query nor its file.
                query_parses(name, &query.filter, part.file.as_deref())?;
                expressions.push(format!("({})", query.filter));
            }
        }
        if expressions.len() < options.query.len() {
            undeclared.push((part.file.clone(), part.samples.len()));
            continue;
        }
        let joined = expressions.join(" && ");
        let parsed = filter::parse(&joined).map_err(|error| {
            Fail::usage(filter::caret(&joined, &error).unwrap_or_else(|| error.to_string()))
        })?;
        let matched = part
            .samples
            .filter(&parsed)
            .map_err(|error| Fail::data(error.to_string()))?;
        kept.extend(
            matched
                .iter()
                .map(|entry| entry.sample.as_ptr().cast_const()),
        );
    }
    if let Some(position) = declared.iter().position(|declared| !declared) {
        let name = &options.query[position];
        let everything = collection.filter_by(|_| true);
        let message = collection
            .by_configuration(&everything)
            .into_iter()
            .find_map(|part| {
                part.config
                    .and_then(|config| named_queries::named(config, name).err())
            })
            .map_or_else(
                || needs_configuration(collection, &format!("--query {name}")),
                |error| query_error_said(&error),
            );
        return Err(Fail::usage(message));
    }
    warn_undeclared(
        &undeclared,
        &format!("query '{}'", options.query.join("', '")),
    );
    Ok(narrowed.filter_by(|sample| kept.contains(&std::ptr::from_ref(sample))))
}

/// Whether the selection or its columns name `state`, and then whether the
/// model's plan is needed for it: `Some(false)` for `state == failed`, which
/// the files answer, `Some(true)` for a word only the model could deny, or for
/// the column, which shows every word.
fn states_wanted(collection: &SampleList, options: &Options) -> Option<bool> {
    let mut written: Vec<String> = options.filter.clone();
    let configs: Vec<&ProjectConfig> = match collection.config() {
        Some(config) => vec![config],
        None => collection
            .by_configuration(&collection.filter_by(|_| true))
            .into_iter()
            .filter_map(|part| part.config)
            .collect(),
    };
    for name in &options.query {
        for config in &configs {
            if let Ok(query) = named_queries::named(config, name) {
                written.push(query.filter.clone());
            }
        }
    }
    let mut words = Vec::new();
    for expression in &written {
        if let Ok(parsed) = filter::parse(expression) {
            words.extend_from_slice(filter::state_words(&parsed));
        }
    }
    let shown = |columns: &[ColumnSpec]| columns.iter().any(|column| column.field == "state");
    let column = shown(&options.columns)
        || options.profile.as_deref().is_some_and(|name| {
            configs.iter().any(|config| {
                config
                    .profile(name)
                    .is_ok_and(|profile| shown(&profile.columns))
            })
        });
    if !column && words.is_empty() {
        return None;
    }
    Some(column || words.iter().any(|word| word.needs_the_model()))
}

/// The list, each sample carrying its states: what its files say, and, with
/// `with_model`, what its model owes, planned as `status` plans it — nothing
/// asked, no formula run. A sample whose model could not be read is answered
/// from its files, and said.
fn with_states(collection: &SampleList, with_model: bool) -> Result<SampleList, Fail> {
    let mut states = validation::states(collection);
    if with_model {
        let mut lines = Vec::new();
        let unread = inspect::never_computed(collection, &mut lines)?;
        let counts = name_counts(
            collection
                .iter()
                .map(|entry| entry.sample.borrow().name().map(str::to_string)),
        );
        for (entry, states) in collection.iter().zip(states.iter_mut()) {
            let label = labelled(&counts, entry.path.as_deref(), &entry.sample.borrow());
            let read = !unread.values().any(|labels| labels.contains(&label));
            let mut held = states.held().to_vec();
            held.extend(
                lines
                    .iter()
                    .filter(|line| line[0] == label)
                    .map(|line| planned_state(&line[2])),
            );
            *states = fields::States::new(held, read);
        }
        let unread_count: usize = unread.values().map(Vec::len).sum();
        if unread_count > 0 {
            // Each reason's first line: one may go on to say where to fix it,
            // which `status` says in full.
            let why: Vec<&str> = unread
                .keys()
                .map(|reason| runtime::reason_line(reason))
                .collect();
            warn(&format!(
                "the model was not read for {} ({}): its state is what its files say, and \
                 what it has never computed is not known — {} says why",
                counted_as(unread_count, "sample", "samples"),
                why.join("; "),
                advised("status")
            ));
        }
    }
    Ok(collection.with_states(states))
}

/// A value the model's plan lists, as `state` names it: what `status` says
/// of it, read back.
fn planned_state(said: &str) -> fields::State {
    if said.starts_with("waits for") {
        fields::State::Waiting
    } else if said.starts_with("record missing") {
        fields::State::Edited
    } else if said.contains("never computed") {
        fields::State::NeverComputed
    } else {
        // A formula changed since it computed the value, or one it reads.
        fields::State::Stale
    }
}

/// A declared query's filter, parsed as it is written in its file, and an error
/// said of the query it belongs to.
fn query_parses(name: &str, written: &str, file: Option<&Path>) -> Outcome {
    filter::parse(written).map(|_| ()).map_err(|error| {
        let pointed = filter::caret(written, &error).unwrap_or_else(|| error.to_string());
        let whose = match file {
            Some(file) => format!("query '{name}' in {}", file.display()),
            None => format!("query '{name}'"),
        };
        Fail::usage(format!("{whose}: {pointed}"))
    })
}

fn select_with(
    collection: &SampleList,
    options: &Options,
    target: &Path,
    queries: &[String],
) -> Result<SampleList, Fail> {
    let mut expressions = Vec::new();
    for name in queries {
        let config = collection.config().ok_or_else(|| {
            Fail::usage(needs_configuration(collection, &format!("--query {name}")))
        })?;
        let query = named_queries::named(config, name)
            .map_err(|error| Fail::usage(query_error_said(&error)))?;
        let file = config.root().join(".samplekitrc");
        query_parses(name, &query.filter, Some(&file))?;
        expressions.push(query.filter.clone());
    }
    for written in &options.filter {
        expressions.push(written.clone());
    }
    // Each expression is checked as it was written, so that an error points at
    // the character typed rather than into the conjunction built below.
    for expression in &expressions {
        filter::parse(expression).map_err(|error| {
            Fail::usage(filter::caret(expression, &error).unwrap_or_else(|| error.to_string()))
        })?;
    }
    let mut selected = match expressions.len() {
        0 => collection.filter_by(|_| true),
        // Conjoined by parenthesising each half, so that `a || b` and `c || d`
        // do not silently become `a || (b && c) || d`.
        _ => {
            let joined = expressions
                .iter()
                .map(|expression| format!("({expression})"))
                .collect::<Vec<_>>()
                .join(" && ");
            let parsed = filter::parse(&joined).map_err(|error| {
                // The caret is the message: a position on its own is a number.
                Fail::usage(filter::caret(&joined, &error).unwrap_or_else(|| error.to_string()))
            })?;
            // Checked against the collection the target belongs to, never the
            // file alone: a field its siblings have is not a typo.
            let mut siblings = siblings_of(target);
            // A file among its siblings reads their states too.
            let words = filter::state_words(&parsed);
            if !words.is_empty()
                && let Some(all) = &siblings
            {
                let with_model = words.iter().any(|word| word.needs_the_model());
                siblings = Some(with_states(all, with_model)?);
            }
            let vocabulary = siblings.as_ref().unwrap_or(collection);
            let guards: Vec<_> = vocabulary.iter().map(|e| e.sample.borrow()).collect();
            let samples: Vec<&samplekit::core::sample::Sample> =
                guards.iter().map(|guard| &**guard).collect();
            // Over no sample, no field is known and none is a typo: the
            // selection is empty, and said so, where every field was *unknown*
            // with *0 fields available*.
            let diagnostics = filter::check(&parsed, &samples);
            if !diagnostics.is_empty() && !samples.is_empty() {
                return Err(Fail::usage(report(&diagnostics, vocabulary)));
            }
            if samples.is_empty() {
                warn(&format!(
                    "no sample in {}: nothing for the selection to select",
                    shell_word(&target.display().to_string())
                ));
            }
            // A text no sample holds, one close to it held: the empty
            // answer's likely cause.
            for unmatched in &diagnostics.unmatched_texts {
                warn(&unmatched.to_string());
            }
            for unmatched in &diagnostics.unmatched_numbers {
                warn(unmatched);
            }
            for aside in &diagnostics.set_aside {
                warn_gathered(
                    "left aside",
                    |count| {
                        format!(
                            "{count} comparisons left samples aside: they hold text there, \
                             which a comparison of numbers cannot compare"
                        )
                    },
                    aside.said(),
                );
            }
            drop(guards);
            match &siblings {
                // A file is evaluated among its siblings too, so that a field
                // they have and it lacks is absent — `porosity is missing` — rather
                // than unknown. The answer is then whether the file is kept.
                Some(all) => {
                    let matched = all
                        .filter(&parsed)
                        .map_err(|error| Fail::data(error.to_string()))?;
                    // Each file kept that matched among its siblings, which
                    // several file targets need as much as one.
                    let canonical = |entry: &list::Entry| {
                        entry
                            .path
                            .as_ref()
                            .and_then(|path| dunce::canonicalize(path).ok())
                    };
                    let wanted: std::collections::HashSet<PathBuf> =
                        matched.iter().filter_map(canonical).collect();
                    let kept: std::collections::HashSet<*const samplekit::core::sample::Sample> =
                        collection
                            .iter()
                            .filter(|entry| {
                                canonical(entry).is_some_and(|path| wanted.contains(&path))
                            })
                            .map(|entry| entry.sample.as_ptr().cast_const())
                            .collect();
                    collection.filter_by(move |sample| kept.contains(&std::ptr::from_ref(sample)))
                }
                None => collection
                    .filter(&parsed)
                    .map_err(|error| Fail::data(error.to_string()))?,
            }
        }
    };
    if let Some(spec) = sort_spec(collection, options, &[])? {
        selected.sort(&spec).map_err(|error| match error {
            // The same words an unknown column gets: a sort key is a field.
            list::ListError::Order(ordering::OrderError::UnknownField(error)) => {
                Fail::usage(with_suggestion(*error, collection))
            }
            list::ListError::Field(error) => Fail::usage(with_suggestion(error, collection)),
            other => Fail::usage(other.to_string()),
        })?;
    }
    Ok(selected)
}

/// A query's error, an unknown name said as a profile's, an export's and a
/// figure's are: *no query named 'x'*, where the collection's own words wrote
/// *no named query 'x'*.
fn query_error_said(error: &named_queries::QueryError) -> String {
    match error {
        named_queries::QueryError::UnknownQuery {
            name,
            available,
            suggestion,
        } => {
            let mut said = format!("no query named '{name}'");
            if let Some(suggestion) = suggestion {
                said.push_str(&format!("\n  did you mean: '{suggestion}'?"));
            }
            if !available.is_empty() {
                said.push_str(&format!("\n  available: {}", available.join(", ")));
            }
            said
        }
        other => other.to_string(),
    }
}

/// Every problem in one pass, which is what `check` is for: clearing them one
/// run at a time is the defect one-error-at-a-time diagnostics always are.
fn report(diagnostics: &filter::Diagnostics, collection: &SampleList) -> String {
    let mut out = String::new();
    let groups = field_groups(collection);
    // Said as a column's unknown field is, and closed by the same footer: the
    // filter's own list left out name, path, filename and project, and ended
    // on a different pointer than -c's.
    let mut unknown = false;
    let mut without_suggestion = false;
    for error in &diagnostics.unknown_fields {
        match error {
            fields::FieldError::UnknownProperty {
                name, suggestion, ..
            } => {
                // The nearest field held, where the filter found none: a
                // column misspelt had advice, a filter's field none.
                let suggestion = suggestion.clone().or_else(|| {
                    let known: Vec<String> = collection
                        .available_fields()
                        .iter()
                        .map(fields::describe)
                        .collect();
                    samplekit::core::identifier::nearest(name, known.iter().map(String::as_str))
                });
                let (head, a_column) = unknown_field_head(name, suggestion.as_deref(), &groups);
                out.push_str(&format!("{head}\n"));
                if !a_column {
                    unknown = true;
                    without_suggestion |= suggestion.is_none();
                }
            }
            other => out.push_str(&format!("{}\n", field_error_body(other, &groups))),
        }
    }
    for conflict in &diagnostics.type_conflicts {
        out.push_str(&format!(
            "{}\n",
            filter::FilterError::TypeConflict(Box::new(conflict.clone()))
        ));
    }
    for misuse in &diagnostics.wrong_operators {
        out.push_str(&format!("{}\n", misuse.error()));
    }
    for tag in &diagnostics.unknown_tags {
        out.push_str(&format!("no sample carries the tag '{}'", tag.tag));
        if let Some(suggestion) = &tag.suggestion {
            out.push_str(&format!("\n  did you mean: '{suggestion}'?"));
        }
        out.push('\n');
    }
    // The command that answers the question asked: the fields where a field
    // went wrong, the tags where only a tag did.
    let about_fields = !diagnostics.unknown_fields.is_empty()
        || !diagnostics.type_conflicts.is_empty()
        || !diagnostics.wrong_operators.is_empty();
    if unknown {
        let known: Vec<String> = collection
            .available_fields()
            .iter()
            .map(fields::describe)
            .collect();
        out.push_str(&fields_footer(&known, without_suggestion));
    } else if about_fields {
        out.push_str(&format!(
            "  run '{}' to see every addressable field",
            advised_here("list fields")
        ));
    }
    if !diagnostics.unknown_tags.is_empty() {
        if about_fields {
            out.push('\n');
        }
        out.push_str(&format!(
            "  run '{}' to see the tags in use",
            advised_here("list tags")
        ));
    }
    out
}

/// The order asked for: `--sort`, else the `--profile`'s, else `declared` —
/// an export's own profile's — and `-r` reversing whichever it is.
fn sort_spec(
    collection: &SampleList,
    options: &Options,
    declared: &[String],
) -> Result<Option<SortSpec>, Fail> {
    let mut keys: Vec<String> = Vec::new();
    if !options.sort.is_empty() {
        keys.extend(options.sort.iter().cloned());
    } else if let Some(profile) = stored_profile(collection, options) {
        keys.extend(profile.sort.iter().cloned());
    } else {
        keys.extend(declared.iter().cloned());
    }
    if keys.is_empty() {
        // Reversing nothing is a request that did nothing, silently.
        if options.reverse {
            // The example sorts by a column the command already names.
            let field = options
                .columns
                .iter()
                .map(|column| column.field.as_str())
                .find(|field| *field != "name")
                .unwrap_or("<field>");
            return Err(Fail::usage(format!(
                "-r reverses a sort, and there is none: name one with --sort, \
                 as in -s {field} -r"
            )));
        }
        return Ok(None);
    }
    let spec = ordering::parse_spec(&keys).map_err(|error| Fail::usage(error.to_string()))?;
    // Refused before any sample is read, in the command line's own forms, as
    // Python refuses `sorted("state")` in its own.
    if spec
        .keys()
        .iter()
        .any(|key| key.field == Field::Reserved(fields::ReservedField::State))
    {
        return Err(Fail::usage(
            "'state' cannot be sorted by: a sample can hold several states, which have \
             no order — select by one with -f 'state == failed', or show them as a \
             column with -c name,state"
                .to_string(),
        ));
    }
    if !options.reverse {
        return Ok(Some(spec));
    }
    // **`--reverse` flips every key**, not only the first: a flag that
    // reversed the primary key and left the tie-break alone would produce an
    // order nobody asked for.
    let flipped: Vec<SortKey> = spec
        .keys()
        .iter()
        .map(|key| SortKey {
            field: key.field.clone(),
            descending: !key.descending,
        })
        .collect();
    Ok(Some(
        SortSpec::new(flipped).map_err(|error| Fail::usage(error.to_string()))?,
    ))
}

fn stored_profile<'a>(collection: &'a SampleList, options: &Options) -> Option<&'a Profile> {
    // Over several projects a profile is each one's, and so is its order:
    // each project's table sorts its own, where the first project's sorted
    // them all.
    if collection.spans_configurations() {
        return None;
    }
    let name = options.profile.as_ref()?;
    collection.config()?.profile(name).ok()
}

// -------------------------------------------------------------- the columns

/// The profile a table renders: a named one, or the anonymous one `-c` writes.
///
/// `-srp diameter,ductility` *is* a profile — columns, an order, a direction — so a
/// named profile is what you write when you tire of retyping it rather than a
/// second mechanism.
fn profile_of(
    collection: &SampleList,
    options: &Options,
    name: Option<&str>,
) -> Result<Profile, Fail> {
    profile_in(collection.config(), collection, options, name)
}

/// [`profile_of`], the profile named looked up in `config`: a project's own,
/// where a list spans several.
fn profile_in(
    config: Option<&ProjectConfig>,
    collection: &SampleList,
    options: &Options,
    name: Option<&str>,
) -> Result<Profile, Fail> {
    if let Some(name) = name {
        let config = config
            .ok_or_else(|| Fail::usage(needs_configuration(collection, &format!("'{name}'"))))?;
        let mut profile = config
            .profile(name)
            .cloned()
            .map_err(|error| Fail::usage(error.to_string()))?;
        // A profile declares, and what follows overrides the part it names :
        // `-c` the columns, keeping the profile's order.
        if !options.columns.is_empty() {
            profile.columns = anonymous(&options.columns, None).columns;
        }
        if let Some(precision) = &options.precision {
            for column in &mut profile.columns {
                if !is_count(&column.field) {
                    column.precision = Some(precision.clone());
                }
            }
        }
        return Ok(profile);
    }
    let columns = options.columns.clone();
    if columns.is_empty() {
        return Err(Fail::usage(
            "nothing to show: pass --columns or --profile".to_string(),
        ));
    }
    Ok(anonymous(&columns, options.precision.as_ref()))
}

/// The profile `-c` writes, with `--precision` filling every column that names
/// no precision of its own.
fn is_count(field: &str) -> bool {
    fields::parse(field).is_ok_and(|parsed| {
        fields::channel(&parsed) == fields::Channel::Stat(fields::Statistic::Count)
    })
}

fn anonymous(columns: &[ColumnSpec], global_precision: Option<&PrecisionSchema>) -> Profile {
    profiles::anonymous(
        columns
            .iter()
            .map(|column| ColumnSpec {
                // A count stays whole whatever --precision says.
                precision: column.precision.clone().or_else(|| {
                    (!is_count(&column.field))
                        .then(|| global_precision.cloned())
                        .flatten()
                }),
                // One name for whatever is written: the label a reader sees,
                // and the header a data file's columns are named by.
                header: column.header.clone().or_else(|| column.label.clone()),
                ..column.clone()
            })
            .collect(),
    )
}

/// **No column is rendered before it is checked.** `-c porosity,vfy` must not print
/// a column of dashes and exit 0, which is the v1 defect this project exists to
/// remove.
///
/// The check runs against `loaded` — the collection — and never against the
/// selection: a vocabulary belongs to a collection, and checking a filtered
/// result makes a well-formed question with an empty answer into a false error.
fn correct_column_typos(loaded: &SampleList, profile: &mut Profile) -> Outcome {
    if profile.name != "-c" || !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(());
    }
    let warnings = list::check_profile(loaded, profile);
    let mut corrected = Vec::new();
    for warning in warnings {
        let FieldWarning::Unknown {
            field,
            suggestion: Some(suggestion),
        } = warning
        else {
            continue;
        };
        if corrected.contains(&field) {
            continue;
        }
        if confirm_field_correction(&field, &suggestion)? {
            for column in &mut profile.columns {
                if column.field == field {
                    column.field.clone_from(&suggestion);
                }
            }
        }
        corrected.push(field);
    }
    Ok(())
}

fn confirm_field_correction(field: &str, suggestion: &str) -> Result<bool, Fail> {
    loop {
        errp!("unknown field '{field}'; use '{suggestion}'? [Y/n] ");
        io::stderr()
            .flush()
            .map_err(|error| Fail::io(format!("could not write correction prompt: {error}")))?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|error| Fail::io(format!("could not read correction answer: {error}")))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => errln!("  answer y or n"),
        }
    }
}

fn check_columns(loaded: &SampleList, profile: &Profile) -> Outcome {
    // A property the model declares is no misspelling before it is first
    // computed: the collection's vocabulary holds it.
    let warnings = list::check_profile(loaded, profile);
    if warnings.is_empty() {
        return Ok(());
    }
    let known: Vec<String> = loaded
        .available_fields()
        .iter()
        .map(fields::describe)
        .collect();
    let mut message = String::new();
    let groups = field_groups(loaded);
    let mut has_unknown = false;
    let mut has_unknown_without_suggestion = false;
    for warning in &warnings {
        match warning {
            FieldWarning::Unknown { field, suggestion } => {
                let (head, a_column) = unknown_field_head(field, suggestion.as_deref(), &groups);
                // A declared profile's own key is its mistake, said with where
                // it is written: the command line named no such field.
                // The file declaring the profile: imported, it is that file's,
                // which the project reading it is then named beside.
                let written_in = || {
                    let Some(config) = loaded.config() else {
                        return ".samplekitrc".to_string();
                    };
                    let own = config.root().join(".samplekitrc");
                    match config.origin(DeclarationKind::Profile, &profile.name) {
                        DeclaredIn::Imported(file) => {
                            format!("{}, imported by {}", file.display(), own.display())
                        }
                        DeclaredIn::Local | DeclaredIn::Over(_) => own.display().to_string(),
                    }
                };
                // A column of it too: `unknown field 'attenuaton'` said
                // nothing of where the name was written.
                if profile.name != "-c"
                    && profile.columns.iter().any(|column| column.field == *field)
                {
                    message.push_str(&format!(
                        "profile '{}' in {} shows '{field}':\n",
                        profile.name,
                        written_in()
                    ));
                } else if profile.name != "-c"
                    && profile
                        .sort
                        .iter()
                        .chain(&profile.group)
                        .any(|key| key.trim_start_matches('-') == field)
                {
                    message.push_str(&format!(
                        "profile '{}' in {} orders its rows by '{field}':\n",
                        profile.name,
                        written_in()
                    ));
                }
                message.push_str(&head);
                message.push('\n');
                if !a_column {
                    has_unknown = true;
                    has_unknown_without_suggestion |= suggestion.is_none();
                }
            }
            FieldWarning::TableNeedsCell { table } => {
                message.push_str(&format!(
                    "'{table}' names a table, not one scalar field; choose a column and index\n"
                ));
                if let Some(group) = groups.tables.iter().find(|group| group.name == *table) {
                    message.push_str(&table_group_listing(group, 100));
                }
            }
            FieldWarning::NoItem { field, .. } => {
                message.push_str(&format!(
                    "no sample has the item '{field}': items are numbered from #0\n"
                ));
            }
            FieldWarning::NoRow { field, table } => {
                message.push_str(&format!("no sample has the row '{field}'\n"));
                // The rows there are, and the nearest, as `set` offers it :
                // `the rows of 'inspection': samplekit list fields` sent the
                // reader to another command for the answer.
                match groups.tables.iter().find(|group| group.name == *table) {
                    Some(group) if !group.rows.is_empty() => {
                        message.push_str(&format!(
                            "  the rows of '{table}', by {}: {}\n",
                            group.index.join(", "),
                            group.rows.join(", ")
                        ));
                        let rows: Vec<Value> =
                            group.rows.iter().map(|row| row_value(row)).collect();
                        if let Some(nearest) = nearest_row_address(field, &rows) {
                            message.push_str(&format!("  did you mean: '{nearest}'?\n"));
                        }
                    }
                    _ => message
                        .push_str(&format!("  the rows of '{table}': samplekit list fields\n")),
                }
            }
        }
    }
    if has_unknown
        && loaded.is_empty()
        && UNREAD.load(AtomicOrdering::Relaxed)
        && !MALFORMED.load(AtomicOrdering::Relaxed)
    {
        // Nothing was read because the filesystem refused it: that is the
        // answer, exit 3 as the same file met over the project exits, and
        // not a folder named wrongly.
        return Err(Fail::io(
            "no sample could be read here: what is named above could not be read, so no field \
             can be found — give it back its permissions, then ask again"
                .to_string(),
        ));
    }
    if has_unknown && loaded.is_empty() {
        // Not a misspelling: nothing was read to hold the field. Run from a
        // folder whose samples sit below it, it named every column unknown.
        message = "no sample was read here, so no field can be found: name the folder \
                   that holds the samples"
            .to_string();
    } else if has_unknown && known.is_empty() {
        message.push_str("  no sample read holds a field");
    } else if has_unknown {
        message.push_str(&fields_footer(&known, has_unknown_without_suggestion));
    }
    // A file set aside unread may hold the field — or, a configuration, say
    // where the samples are: the cause is the unread file, said above, and the
    // answer is incomplete rather than the question wrong.
    if has_unknown && UNREAD.load(AtomicOrdering::Relaxed) {
        message.push_str(
            "
  a file above was not read, and may hold it: repair it, then ask again",
        );
        return Err(Fail::data(message.trim_end().to_string()));
    }
    Err(Fail::usage(message.trim_end().to_string()))
}

// ------------------------------------------------------------------ output

/// A terminal and a pipe want different output, so they are different targets.
/// A pipe is never truncated and never carries a placeholder: a dash in a CSV
/// is a *value*, and one that poisons a numeric column.
fn output_target(options: &Options) -> Target {
    if options
        .output
        .as_ref()
        .is_some_and(|path| path != Path::new("-"))
    {
        return Target::Pipe;
    }
    if let Some(width) = options.width {
        return Target::Terminal { width };
    }
    if !std::io::stdout().is_terminal() {
        return Target::Pipe;
    }
    // A column count is the reader's terminal, not a format this project
    // defines: `$COLUMNS` when the shell exports it, else what the terminal
    // reports — a shell rarely exports it — and only then a width wide enough
    // that narrowing is the exception.
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|written| written.parse().ok())
        .filter(|columns| *columns > 20)
        .or_else(|| reported_columns().filter(|columns| *columns > 20))
        .unwrap_or(120);
    Target::Terminal { width }
}

/// Where a command's output goes: stdout, a stream, or a file — and a file is a
/// change, so it is previewed and written with `--write` like every other
/// change the tool makes.
///
/// A stream replaces nothing, so `-o -` writes at once.
fn emit_output(text: &str, options: &Options, from: &SampleList) -> Outcome {
    emit_output_behind(text, options, from, None)
}

/// The same, saying `behind` — what the file is written from that is not
/// current — where a file leaves the terminal that showed each state: in the
/// preview, and on stderr once written.
fn emit_output_behind(
    text: &str,
    options: &Options,
    from: &SampleList,
    behind: Option<(String, exports::Behind)>,
) -> Outcome {
    let Some(path) = options.output.as_deref() else {
        out!("{text}");
        return Ok(());
    };
    let said_behind = || {
        if let Some((said, _)) = &behind {
            for line in said.lines() {
                errln!("{}", line.trim_start());
            }
        }
    };
    // Written from values not current, a failure all the same, as an export
    // is, unless `--accept` names their states.
    let accepted = |written: &str| match &behind {
        Some((_, counted)) => tables::accepted_or_failed(written, counted, &options.accept),
        None => Ok(()),
    };
    if path == Path::new("-") {
        out!("{text}");
        said_behind();
        return accepted("the table");
    }
    if !options.write {
        // Opened as every preview is: what, where, how much.
        outln!(
            "table · {} · {}",
            relative_to_here(path),
            counted_as(text.lines().count(), "line", "lines")
        );
        if let Some(refused) = exports::refusal(path) {
            outln!("  --write refuses it: {refused}");
        } else if path.exists() {
            outln!("  the file is there, and is replaced");
        } else if let Some(directory) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
        {
            // Made by --write, as every destination's folder is: said now,
            // before it happens.
            outln!(
                "  {} does not exist, and is made",
                relative_to_here(directory)
            );
        }
        if let Some((said, _)) = &behind {
            outln!();
            for line in said.lines() {
                outln!("  {line}");
            }
        }
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    // Read, and again just before the write: a file another writer changed
    // meanwhile is not replaced. The write makes a missing folder.
    let held = exports::held_at(path);
    // The preview announced the replacement, so --write is the answer to it.
    exports::write_replacing(text, path, held.as_deref()).map_err(export_write_failed)?;
    keep_output(path, from);
    // On stdout, as an export says it: a deliverable written from a
    // stale number is not hidden by -q nor a redirected stderr.
    if let Some((said, _)) = &behind {
        for line in said.lines() {
            outln!("{}", line.trim_start());
        }
    }
    outln!("written: {}", relative_to_here(path));
    accepted(&relative_to_here(path))
}

/// A written file refused: changed by another writer since the command read it
/// is the data's, exit 2, as a sample's save is; the rest is the filesystem's.
fn export_write_failed(error: exports::ExportError) -> Fail {
    match error {
        // Another writer's, as a sample being saved is: exit 2, as for a
        // sample, where the system refused nothing.
        exports::ExportError::ChangedSince { .. } | exports::ExportError::Busy { .. } => {
            Fail::data(error.to_string())
        }
        // SampleKit's own refusals, exit 1: *the system refused* said a
        // sample, a lock or the configuration was the filesystem's to keep.
        exports::ExportError::IsASample { .. }
        | exports::ExportError::ProjectFile { .. }
        | exports::ExportError::ReadOnly { .. } => Fail::usage(error.to_string()),
        other => Fail::io(other.to_string()),
    }
}

/// The command line this run was given, for what the history says wrote.
static COMMAND_LINE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// What a script's `input()` asked and was answered, said after the command in
/// its snapshot's message: ` — answered 'Limit? ' 15`.
static ANSWERED: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// A command reading the project as a snapshot kept it: the project, the
/// snapshot, and where it was written out.
struct At {
    config: ProjectConfig,
    /// Where the command was run, before it moved to the project as it was.
    here: PathBuf,
    snapshot: String,
    real_root: PathBuf,
    temporary_root: PathBuf,
}

static AT: std::sync::OnceLock<At> = std::sync::OnceLock::new();

/// The commands `--at` reads with: none writes to the project.
const READ_AT: [&str; 6] = ["list", "status", "view", "export", "plot", "validate"];

/// Runs a reading command over the project as the snapshot `--at` names
/// kept it, written out apart: its paths read there, `-o` written here.
fn run_at(arguments: &[String], at: usize) -> Outcome {
    let (named, rest) = match arguments[at].strip_prefix("--at=") {
        Some(named) => (named.to_string(), 1),
        None => match arguments.get(at + 1) {
            Some(named) => (named.clone(), 2),
            None => return Err(Fail::usage("--at wants a state: a number from samplekit log, the beginning of a snapshot's id, or a date".to_string())),
        },
    };
    let arguments: Vec<String> = arguments[..at]
        .iter()
        .chain(&arguments[at + rest..])
        .cloned()
        .collect();
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    let named_command = is_command(command)
        || [
            "set", "new", "tag", "compute", "init", "log", "diff", "open",
        ]
        .contains(&command);
    if named_command && !READ_AT.contains(&command) {
        return Err(Fail::usage(format!(
            "--at reads the project as it was, and {command} {}",
            match command {
                "log" | "diff" => "reads the history itself: --from and --to name states",
                "open" => "opens today's files",
                _ => "writes to it, which only today's project takes",
            }
        )));
    }
    let output = arguments
        .iter()
        .any(|word| !matches!(output_in(word), OutputWord::None));
    // Its preview too: it announced a file in the folder removed at the end.
    if command == "export" && !output {
        return Err(Fail::usage(
            "with --at, an export writes where -o says: its declared file holds today's"
                .to_string(),
        ));
    }
    // The project read is the targets', never the one `-o` writes into: a
    // file already there, in a project without a history, was taken for the
    // project read, and refused.
    let values = option_values(&arguments);
    let mut after_output = false;
    let places: Vec<PathBuf> = std::iter::once(PathBuf::from("."))
        .chain(
            arguments
                .iter()
                .enumerate()
                .filter(|(at, _)| !values[*at])
                .map(|(_, word)| word)
                .filter(|word| {
                    let output = std::mem::take(&mut after_output);
                    match output_in(word) {
                        OutputWord::Next => {
                            after_output = true;
                            false
                        }
                        OutputWord::Attached { .. } => false,
                        OutputWord::None => !output,
                    }
                })
                .map(PathBuf::from)
                .filter(|path| path.exists()),
        )
        .collect();
    use samplekit::config::version_control;
    let Some(config) = version_control::projects_of(&places)
        .into_iter()
        .next_back()
    else {
        return Err(Fail::usage(
            "--at reads a project's history, and no .samplekitrc is here".to_string(),
        ));
    };
    let entries = version_control::entries(&config).map_err(|error| Fail::io(error.to_string()))?;
    // A folder above several projects: a snapshot is one project's, and its
    // number names another moment in each.
    let below = history::projects_below(config.root());
    if entries.is_empty() && !below.is_empty() {
        let folders: Vec<String> = below
            .iter()
            .map(|project| relative_to_here(project.root()))
            .collect();
        let named_state = &named;
        return Err(Fail::usage(format!(
            "--at names a snapshot of one project's history, and here are {}: {} — name one, \
             samplekit {} … --at {named_state}",
            folders.len(),
            folders.join(", "),
            folders[0]
        )));
    }
    if entries.is_empty() {
        return Err(Fail::usage(
            "no history is kept here yet: it begins with the first change SampleKit writes"
                .to_string(),
        ));
    }
    let _ = COMMAND_LINE.set(command_line(
        &arguments
            .iter()
            .cloned()
            .chain(["--at".to_string(), named.clone()])
            .collect::<Vec<_>>(),
    ));
    let (number, entry) = match history::state_at(&named, &entries)? {
        Some(at) => (at + 1, entries[at].clone()),
        None => return run(&arguments),
    };
    // A folder of its own name, removed when it is dropped.
    let held = tempfile::Builder::new()
        .prefix("samplekit-at-")
        .tempdir()
        .map_err(|error| Fail::io(format!("a temporary folder: {error}")))?;
    let base = held.path().join("project");
    let temporary_root = version_control::materialize(&config, &entry.id, &base)
        .map_err(|error| Fail::io(error.to_string()))?;
    let real_root =
        dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
    let here = std::env::current_dir().map_err(|error| Fail::io(error.to_string()))?;
    let inside = dunce::canonicalize(&here)
        .ok()
        .and_then(|here| here.strip_prefix(&real_root).ok().map(Path::to_path_buf));
    // Paths the command reads, moved to the project as it was; what it
    // writes stays here. A relative path within the project is left as it was
    // typed, the command running from the same folder of the project as it
    // was: it reads the past, and says its paths as they were typed.
    let moved = |word: &str| -> String {
        let path = Path::new(word);
        let Ok(full) = dunce::canonicalize(path) else {
            return word.to_string();
        };
        match full.strip_prefix(&real_root) {
            Ok(_) if path.is_relative() && inside.is_some() => word.to_string(),
            Ok(relative) => temporary_root.join(relative).to_string_lossy().into_owned(),
            Err(_) if path.is_relative() && inside.is_some() => {
                here.join(word).to_string_lossy().into_owned()
            }
            Err(_) => word.to_string(),
        }
    };
    // `-` is the standard output, and no file of that name.
    let written_to = |word: &str| -> String {
        if word == "-" {
            word.to_string()
        } else {
            here.join(word).to_string_lossy().into_owned()
        }
    };
    let mut rewritten = Vec::with_capacity(arguments.len());
    let mut after_output = false;
    for (at, word) in arguments.iter().enumerate() {
        // A value of an option is no path, though it opens with `-`: `-s
        // -drop` sorts, and holds no `-o`.
        if values[at] {
            rewritten.push(word.clone());
            continue;
        }
        if after_output {
            rewritten.push(written_to(word));
            after_output = false;
            continue;
        }
        match output_in(word) {
            OutputWord::Next => {
                rewritten.push(word.clone());
                after_output = true;
                continue;
            }
            OutputWord::Attached { flag, path } => {
                rewritten.push(format!("{flag}{}", written_to(path)));
                continue;
            }
            OutputWord::None => {}
        }
        if word.starts_with('-') || is_command(word) {
            rewritten.push(word.clone());
        } else {
            rewritten.push(moved(word));
        }
    }
    if let Some(relative) = inside {
        let there = temporary_root.join(relative);
        let _ = std::fs::create_dir_all(&there);
        std::env::set_current_dir(&there).map_err(|error| Fail::io(error.to_string()))?;
    }
    // Nothing read from the past writes history of its own; it reads the
    // project's own, which knows the formulas that computed each value.
    // SAFETY: set before any thread is started, and read by the children.
    unsafe { std::env::set_var("SAMPLEKIT_HISTORY", "off") };
    version_control::read_history_of(&temporary_root, &real_root);
    // The model as it was, described: what each value waits for and reads is
    // said from the description, which a snapshot keeps none of — a value
    // waiting was called current.
    if let Some(file) = samplekit::config::project_config::find(&temporary_root)
        && let Ok(then) = samplekit::config::project_config::load(&file)
    {
        edit::describe_now(&then);
    }
    errln!(
        "as kept at #{number} · {} · {}",
        history::when_said(&entry),
        entry.message
    );
    let _ = AT.set(At {
        config,
        here: here.clone(),
        snapshot: entry.id.clone(),
        real_root,
        temporary_root: temporary_root.clone(),
    });
    let outcome = run(&rewritten);
    let _ = std::env::set_current_dir(&here);
    drop(held);
    outcome
}

/// Where one word of a command line says `-o`'s path, in each form clap
/// reads: `-o FILE`, `--output FILE`, `--output=FILE`, `-oFILE`, `-o=FILE`,
/// and `-o` closing a cluster of flags, `-qo FILE`. Only the first two forms
/// were seen, and with `--at` the others wrote into the temporary project,
/// removed when the command ended, while the command said it had written.
enum OutputWord<'a> {
    None,
    /// The path is the next word.
    Next,
    /// The path is in this word, after `flag`.
    Attached {
        flag: &'a str,
        path: &'a str,
    },
}

/// Which words are the value of the option before them — `-s -drop`, `-f
/// 'diameter > 1'` — other than a path: the file `-o` writes, and the paths
/// `--into` and `--target` give.
fn option_values(words: &[String]) -> Vec<bool> {
    let mut values = vec![false; words.len()];
    let mut at = 0;
    while at < words.len() {
        let word = &words[at];
        if word == "--" {
            break;
        }
        let path = matches!(word.as_str(), "--into" | "--target")
            || !matches!(output_in(word), OutputWord::None);
        if word.starts_with('-') && word.len() > 1 && !path && takes_a_value(word) {
            if let Some(value) = values.get_mut(at + 1) {
                *value = true;
            }
            at += 1;
        }
        at += 1;
    }
    values
}

fn output_in(word: &str) -> OutputWord<'_> {
    if word == "--output" {
        return OutputWord::Next;
    }
    if let Some(path) = word.strip_prefix("--output=") {
        return OutputWord::Attached {
            flag: "--output=",
            path,
        };
    }
    let Some(cluster) = word.strip_prefix('-').filter(|rest| !rest.starts_with('-')) else {
        return OutputWord::None;
    };
    // The first short option of a cluster that takes a value takes the rest
    // of the word: `-cog` is `-c radius`, and holds no `-o`.
    for (at, option) in cluster.char_indices() {
        match option {
            'o' => {
                let rest = &cluster[at + 1..];
                let flag_end = 1 + at + 1;
                return match rest.strip_prefix('=') {
                    _ if rest.is_empty() => OutputWord::Next,
                    Some(path) => OutputWord::Attached {
                        flag: &word[..flag_end + 1],
                        path,
                    },
                    None => OutputWord::Attached {
                        flag: &word[..flag_end],
                        path: rest,
                    },
                };
            }
            'c' | 'f' | 's' | 'x' | 'y' | 'p' => return OutputWord::None,
            _ => {}
        }
    }
    OutputWord::None
}

/// A file written from the project as it was, recorded in its history
/// against that snapshot, its samples named as the project names them.
fn keep_output_at(written: &Path, samples: &[PathBuf]) {
    let Some(at) = AT.get() else {
        return;
    };
    let temporary =
        dunce::canonicalize(&at.temporary_root).unwrap_or_else(|_| at.temporary_root.clone());
    let mapped: Vec<PathBuf> = samples
        .iter()
        .filter_map(|path| {
            let path = dunce::canonicalize(path).ok()?;
            path.strip_prefix(&temporary)
                .ok()
                .map(|relative| at.real_root.join(relative))
        })
        .collect();
    let said = COMMAND_LINE.get().map_or("samplekit", String::as_str);
    if let Err(error) = samplekit::config::version_control::tag_output(
        &at.config,
        &at.snapshot,
        written,
        said,
        &mapped,
        &at.here,
    ) {
        warn_gathered(
            "history does not record",
            |count| format!("the history does not record {count} files written"),
            format!("the history does not record {}: {error}", written.display()),
        );
    }
}

/// A file written from samples, tied in their project's history to the snapshot
/// it was made from. What goes wrong is a warning.
fn keep_output(written: &Path, from: &SampleList) {
    let paths: Vec<PathBuf> = from.iter().filter_map(|entry| entry.path.clone()).collect();
    if AT.get().is_some() {
        keep_output_at(written, &paths);
        return;
    }
    let said = COMMAND_LINE.get().map_or("samplekit", String::as_str);
    for (root, error) in samplekit::config::version_control::keep_output_from(written, said, &paths)
    {
        warn_gathered(
            "history does not record",
            |count| format!("the history does not record {count} files written"),
            format!(
                "the history of {} does not record {}: {error}",
                root.display(),
                written.display()
            ),
        );
    }
}

fn table_style(collection: &SampleList, options: &Options) -> Result<Style, Fail> {
    // One table over several projects is drawn as every project draws its
    // own, else as none declares: the first project named drew it.
    if collection.spans_configurations() && options.table_style.is_none() {
        let mut styles = Vec::new();
        for part in collection.by_configuration(collection) {
            let style = style_of(options, part.config)?;
            if !styles.contains(&style) {
                styles.push(style);
            }
        }
        return Ok(match styles.as_slice() {
            [one] => *one,
            _ => Style::default(),
        });
    }
    style_of(options, collection.config())
}

/// The table style: `--table-style`, then `[render] table`, then the default.
/// One the configuration declares and no table draws is said with its file,
/// which is where it is mended.
fn style_of(options: &Options, config: Option<&ProjectConfig>) -> Result<Style, Fail> {
    let available = || Style::names().join(", ");
    if let Some(name) = &options.table_style {
        return Style::parse(name).ok_or_else(|| {
            Fail::usage(format!(
                "'{name}' is not a table style\n  available: {}",
                available()
            ))
        });
    }
    match config.and_then(|config| config.render().table.clone().map(|name| (config, name))) {
        None => Ok(Style::default()),
        Some((config, name)) => Style::parse(&name).ok_or_else(|| {
            Fail::usage(format!(
                "[render] table = '{name}' in {} is not a table style\n  available: {}",
                config.root().join(".samplekitrc").display(),
                available()
            ))
        }),
    }
}

fn render_style<'a>(
    collection: &SampleList,
    options: &'a Options,
) -> Result<Option<&'a str>, Fail> {
    let Some(style) = options.style.as_deref() else {
        return Ok(None);
    };
    // Over several projects a style is looked up in each: one project
    // declaring it is enough, and each that does not is said, its samples
    // shown in its own. The first project named alone decided.
    let parts = if collection.spans_configurations() {
        collection.by_configuration(collection)
    } else {
        Vec::new()
    };
    let mut available: Vec<&str> = collection
        .config()
        .map(ProjectConfig::render_style_names)
        .unwrap_or_else(styles_without_a_project);
    for part in &parts {
        for name in part
            .config
            .map(ProjectConfig::render_style_names)
            .unwrap_or_else(styles_without_a_project)
        {
            if !available.contains(&name) {
                available.push(name);
            }
        }
    }
    if available.contains(&style) {
        let lacking: Vec<String> = parts
            .iter()
            .filter(|part| {
                !part
                    .config
                    .map(ProjectConfig::render_style_names)
                    .unwrap_or_else(styles_without_a_project)
                    .contains(&style)
            })
            .map(|part| {
                format!(
                    "{} declares no style '{style}': its samples are shown in its own",
                    tables::configuration_title(part.file.as_deref())
                )
            })
            .collect();
        warn_several(
            &format!(
                "{} projects declare no style '{style}': their samples are shown in their own",
                lacking.len()
            ),
            &lacking,
        );
        return Ok(Some(style));
    }
    let mut message = format!("'{style}' is not a render style");
    if let Some(suggestion) = identifier::nearest(style, available.iter().copied()) {
        message.push_str(&format!("\n  did you mean: '{suggestion}'?"));
    }
    message.push_str(&format!("\n  available: {}", available.join(", ")));
    Err(Fail::usage(message))
}

// ----------------------------------------------------------------- commands

/// The widest a sample's identity is shown before it is cut in the middle.
const IDENTITY_WIDTH: usize = 32;

/// Whether the identity column may be greyed: a terminal reading colour. A
/// command the reader is invited to run, at the foot of a report.
fn advice(text: &str) -> String {
    // Painted where it is written, as every line is: the advice in it muted,
    // what it says of the samples left as it is.
    text.to_string()
}

/// Whether what is written to stdout may carry colour: a terminal is reading,
/// and nothing turned it off (`--no-color`, `NO_COLOR`, `TERM=dumb`).
///
/// Decided here rather than at each call site, and never for a file: an export
/// is written straight to its path, where an escape would be data.
fn coloured() -> bool {
    std::io::stdout().is_terminal() && COLOR_ENABLED.load(AtomicOrdering::Relaxed)
}

use samplekit::config::project_setup;
use samplekit::presentation::summaries::{self, SummaryRow};

/// Every derived value of a sample that is not current, as `editing` lists
/// it: a property by its name, a table column once with its rows.
fn not_current(sample: &samplekit::core::sample::Sample) -> Vec<(String, Freshness, usize)> {
    samplekit::collection::editing::not_current(sample)
        .into_iter()
        .map(|entry| (entry.name, entry.state, entry.rows))
        .collect()
}

fn with_rows(state: &Freshness, rows: usize) -> String {
    match rows {
        0 => describe(state),
        _ => format!("{} ({})", describe(state), counted_as(rows, "row", "rows")),
    }
}

/// The inputs a stale value moved on from that now hold nothing: its value
/// then waits for them, as the model says (`editing::emptied_inputs`).
fn emptied_inputs(sample: &samplekit::core::sample::Sample, state: &Freshness) -> Vec<String> {
    samplekit::collection::editing::emptied_inputs(sample, state)
}

/// A value's state as every command says it: outdated, unless what it moved on
/// from was emptied, and then waiting for it.
fn state_of(sample: &samplekit::core::sample::Sample, state: &Freshness, rows: usize) -> String {
    let emptied = emptied_inputs(sample, state);
    if emptied.is_empty() {
        return with_rows(state, rows);
    }
    let said = format!("waits for {}", emptied.join(", "));
    match rows {
        0 => said,
        _ => format!("{said} ({})", counted_as(rows, "row", "rows")),
    }
}

/// A path as the shell would reach it from the current folder: `out/x.csv`,
/// `../other/x.csv` — never the absolute path a configuration resolved it
/// to, nor `./x.md`. Where the two share nothing but the root, as written.
/// A path with its `.` and `..` resolved by their words, for one that does not
/// exist yet.
fn lexically(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut held, part| {
        match part {
            std::path::Component::ParentDir => {
                held.pop();
            }
            std::path::Component::CurDir => {}
            part => held.push(part),
        }
        held
    })
}

fn relative_to_here(path: &Path) -> String {
    // With --at the command runs in the project as it was: `here` is still
    // where it was typed.
    let here = match AT.get() {
        Some(at) => at.here.clone(),
        None => match std::env::current_dir() {
            Ok(here) => here,
            Err(_) => return shown_path(path),
        },
    };
    let here = dunce::canonicalize(&here).unwrap_or(here);
    // A file not written yet has no canonical form: its folder has, and a
    // folder not made yet is resolved by its words — `data/ana/../../exports`
    // said as `exports`.
    let absolute =
        dunce::canonicalize(path).unwrap_or_else(|_| match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                dunce::canonicalize(parent)
                    .map_or_else(|_| lexically(&here.join(path)), |p| p.join(name))
            }
            _ => lexically(&here.join(path)),
        });
    if !absolute.is_absolute() {
        return shown_path(&absolute);
    }
    // A file of the project read as it was is said where it is in the project.
    let absolute = as_here(&absolute);
    let theirs: Vec<_> = absolute.components().collect();
    let ours: Vec<_> = here.components().collect();
    let shared = theirs.iter().zip(&ours).take_while(|(a, b)| a == b).count();
    // Only the root in common: `../../../../tmp/x` says less than `/tmp/x`.
    if shared <= 1 {
        return absolute.display().to_string();
    }
    let mut relative = PathBuf::new();
    for _ in shared..ours.len() {
        relative.push("..");
    }
    for part in &theirs[shared..] {
        relative.push(part);
    }
    if relative.as_os_str().is_empty() {
        return ".".to_string();
    }
    relative.display().to_string()
}

/// A path of the project read from its history, `--at`, where it is in the
/// project: the folder the snapshot is written out in is removed when the
/// command ends, and its path said nothing to whoever read it.
fn as_here(path: &Path) -> PathBuf {
    let Some(at) = AT.get() else {
        return path.to_path_buf();
    };
    let temporary =
        dunce::canonicalize(&at.temporary_root).unwrap_or_else(|_| at.temporary_root.clone());
    let full = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match full.strip_prefix(&temporary) {
        Ok(relative) => at.real_root.join(relative),
        Err(_) => path.to_path_buf(),
    }
}

/// A path as given, without a leading `./` (`.\` on Windows): `pieces/x.md` in
/// every message, where a scan from `.` said `./pieces/x.md`.
fn shown_path(path: &Path) -> String {
    let shown = path.display().to_string();
    match shown
        .strip_prefix("./")
        .or_else(|| shown.strip_prefix(".\\"))
    {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => shown,
    }
}

/// Whether the command writes, `--write` given.
static WRITING: AtomicBool = AtomicBool::new(false);

/// A project reached: where a folder made a project, the snapshots the
/// project above kept of its files brought into its own history, and said;
/// where a write is under way, what it holds kept for the write's snapshot.
fn bring_history_from_above(config: &ProjectConfig) -> Outcome {
    if let Some(said) = samplekit::config::version_control::bring_from_above(config) {
        errln!("{said}");
    }
    samplekit::config::version_control::reached_by_write(config);
    Ok(())
}

/// The target the command was given, for the advice that names a command
/// to run next: run from elsewhere, one naming none read another folder.
static ADVICE_TARGET: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The selection the command was given, `-f …` and `--query …`, as words: the
/// advice that names a command keeps it, where `compute --force` without it
/// gave back to their formulas the overrides of the whole project.
static ADVICE_SELECTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn selection_words(arguments: &[String]) -> String {
    let mut words = Vec::new();
    let mut at = 0;
    while at < arguments.len() {
        let word = &arguments[at];
        if word == "--" {
            break;
        }
        if matches!(word.as_str(), "-f" | "--filter" | "--query")
            && let Some(value) = arguments.get(at + 1)
        {
            words.push(format!("{word} {}", shell_word(value)));
            at += 2;
            continue;
        }
        if word.starts_with("--filter=") || word.starts_with("--query=") {
            words.push(shell_word(word));
        }
        at += 1;
    }
    words.iter().map(|word| format!(" {word}")).collect()
}

/// `samplekit TARGETS COMMAND`, without the selection: what a listing names,
/// where the selection may be the question's mistake.
fn advised_here(command: &str) -> String {
    let target = ADVICE_TARGET
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    format!("samplekit {}{command}", targets_first_words(&target))
}

/// `samplekit TARGETS COMMAND SELECTION`, the command named on what this one
/// read.
fn advised(command: &str) -> String {
    let target = ADVICE_TARGET
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    format!(
        "samplekit {}{command}{}",
        targets_first_words(&target),
        ADVICE_SELECTION
            .get()
            .map(String::as_str)
            .unwrap_or_default()
    )
}

/// The overrides a command leaves alone, said one way by `status` and
/// `compute`: they were worded two ways, one of them *their formula* of two.
fn overrides_kept(count: usize) -> String {
    format!(
        "{} over {} {} kept — {} gives {} back to {}",
        counted_as(count, "value typed", "values typed"),
        if count == 1 {
            "its formula"
        } else {
            "their formulas"
        },
        if count == 1 { "is" } else { "are" },
        advised("compute --force"),
        if count == 1 { "it" } else { "them" },
        if count == 1 {
            "its formula"
        } else {
            "their formulas"
        }
    )
}

/// What a command wrote, as every one says it: `written: <path>` for one
/// file, `written: N files` and their paths beneath for several, ten at most.
fn written_lines(paths: &[String]) -> Vec<String> {
    match paths {
        [] => vec!["nothing written".to_string()],
        [one] => vec![format!("written: {one}")],
        several => {
            let mut lines = vec![format!("written: {} files", several.len())];
            lines.extend(several.iter().take(10).map(|path| format!("  {path}")));
            if several.len() > 10 {
                lines.push(format!("  … {} more", several.len() - 10));
            }
            lines
        }
    }
}

fn counted_as(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

const KINDS: [&str; 9] = [
    "fields", "tags", "queries", "profiles", "exports", "figures", "scripts", "files", "skipped",
];

/// What `list fields` prints, grouped so that a table reads as a table.
#[derive(Default)]
struct FieldGroups {
    properties: Vec<String>,
    attributes: Vec<String>,
    tables: Vec<TableGroup>,
}

struct TableGroup {
    name: String,
    index: Vec<String>,
    columns: Vec<String>,
    rows: Vec<String>,
}

// -------------------------------------------------------------------- tags

enum Edit {
    Add,
    Remove,
    Rename,
}

// ------------------------------------------------------ the configuration

thread_local! {
    /// The file targets after the first, read with it.
    static EXTRA_TARGETS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    /// `--rc`: the configuration every sample is read with, and its file.
    static FORCED: RefCell<Option<(PathBuf, ProjectConfig)>> = const { RefCell::new(None) };
}

fn force_configuration(path: &Path) -> Outcome {
    if !path.is_file() {
        return Err(Fail::io(if path.exists() {
            format!("{}: not a file, and a configuration is one", path.display())
        } else {
            format!("{}: no such configuration file", path.display())
        }));
    }
    let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let config = samplekit::config::project_config::load(&path)
        .map_err(|error| Fail::data(error.to_string()))?;
    FORCED.with(|forced| *forced.borrow_mut() = Some((path, config)));
    Ok(())
}

fn forced_configuration() -> Option<(PathBuf, ProjectConfig)> {
    FORCED.with(|forced| forced.borrow().clone())
}

/// A sample `--rc` reads whose nearest configuration is another one: said once,
/// with how many, since what describes them is not what they would find.
fn warn_ignored_configurations(collection: &SampleList, forced: &Path) {
    let mut ignored: Vec<PathBuf> = Vec::new();
    let mut samples = 0usize;
    for entry in collection.iter() {
        let Some(path) = entry.path.as_deref() else {
            continue;
        };
        if let Some(own) = samplekit::config::project_config::find(path)
            && own != forced
        {
            samples += 1;
            if !ignored.contains(&own) {
                ignored.push(own);
            }
        }
    }
    if samples > 0 {
        let said = format!(
            "--rc {} describes every sample: {} their own configuration, ignored",
            forced.display(),
            counted_as(samples, "sample has", "samples have"),
        );
        // One plain line; the files ignored under -v, or named where one is.
        match ignored.as_slice() {
            [one] => warn(&format!("{said}: {}", one.display())),
            _ if VERBOSE.load(AtomicOrdering::Relaxed) => {
                let listed: Vec<String> = ignored
                    .iter()
                    .map(|path| format!("  {}", path.display()))
                    .collect();
                warn(&format!("{said}:\n{}", listed.join("\n")));
            }
            several => warn(&format!("{said}, from {} files — {SEVERAL}", several.len())),
        }
    }
}

/// `-v`: which configuration answered, and the model it names.
fn note_configuration(collection: &SampleList) {
    let (file, config) = match forced_configuration() {
        Some((path, config)) => (Some(path), Some(config)),
        None => (
            collection.config().map(|config| {
                config
                    .root()
                    .join(samplekit::config::project_config::FILENAME)
            }),
            collection.config().cloned(),
        ),
    };
    match &file {
        Some(path) => errln!("configuration: {}", as_here(path).display()),
        None => errln!(
            "configuration: none found from {}",
            collection
                .root()
                .map_or_else(|| ".".to_string(), |root| root.display().to_string())
        ),
    }
    if file.is_some() {
        errln!("model: {}", model_of(config.as_ref()));
    }
}

fn model_of(config: Option<&ProjectConfig>) -> String {
    match config.and_then(runtime::template_of) {
        Some(template) => match template.class() {
            Some(class) => format!("{} (class {class})", as_here(template.path()).display()),
            None => as_here(template.path()).display().to_string(),
        },
        None => "none declared".to_string(),
    }
}

/// The examples a help page ends with, by command path: a command line, and
/// what it does beneath it.
fn examples(page: &str) -> &'static [(&'static str, &'static str)] {
    const REFERENCE: &str = "samplekit samples/ tag add reference -f 'material == steel'";
    match page {
        "" => &[
            (
                "samplekit samples/ -f 'elongation > 20'",
                "Print the path of every sample a filter selects",
            ),
            (
                "samplekit samples/ -c material,tensile_strength -s -tensile_strength",
                "A table of chosen fields, sorted",
            ),
            (
                "samplekit samples/ -c width,thickness --group material",
                "A table per material, each headed by it",
            ),
            (
                "samplekit samples/ --profile overview",
                "A table declared in .samplekitrc",
            ),
            (
                "samplekit samples/ status",
                "Computed values that are not current, and why",
            ),
            (
                "samplekit samples/ compute --write",
                "Compute what is not current, and write it",
            ),
            (
                "samplekit help compute",
                "A command's own page, with its examples",
            ),
        ],
        "init" => &[
            (
                "samplekit init",
                "At a terminal, a few questions, then what it will write, on a yes",
            ),
            (
                "samplekit init --write",
                "An empty project, its model to fill in, and an environment",
            ),
            (
                "samplekit example/ init --example --write",
                "The example project, one formula of each kind, in another folder",
            ),
        ],
        "new" => &[
            (
                "samplekit new steel-02",
                "What the file would hold: what you give here, named by its file",
            ),
            (
                "samplekit new steel-02 material=steel width=12.5 --write",
                "Write it with values already filled in",
            ),
            (
                "samplekit samples/ new steel-02 --keep samples/steel-01.md width,grade --write",
                "Take a value another specimen measured, with its readings, and an attribute; nothing else",
            ),
        ],
        "set" => &[
            (
                "samplekit samples/steel-01.md set thickness.readings=1.991,1.994,1.989",
                "What the change would make outdated; nothing is written",
            ),
            (
                "samplekit samples/steel-01.md set thickness.readings=1.991,1.994,1.989 --write",
                "Write it",
            ),
            (
                "samplekit samples/steel-01.md set grade=S275 direction=T --write",
                "Several fields at once, written together or not at all",
            ),
            (
                "samplekit samples/steel-01.md set 'curve.force[0.2]=9.52' --write",
                "One cell of a table, addressed by its index",
            ),
            (
                "samplekit samples/steel-01.md set --add-row curve strain=0.1 force=5.13 --write",
                "A new measurement: one row, the model computes its other columns",
            ),
        ],
        "list" => &[
            (
                "samplekit samples/ list",
                "What the collection holds and declares",
            ),
            (
                "samplekit samples/ list fields",
                "Every field a filter, a column or a sort can name",
            ),
            ("samplekit samples/ list tags", "Every tag in use"),
            (
                "samplekit samples/ list skipped",
                "The files the scan skipped, and why",
            ),
        ],
        "status" => &[
            (
                "samplekit samples/ status",
                "Values outdated, failed, edited or never computed, and why",
            ),
            (
                "samplekit samples/ status -f 'material == steel'",
                "Only in the samples a filter selects",
            ),
            (
                "samplekit samples/ status --exit-code",
                "Exit 2 when anything is not current: a check for a script",
            ),
        ],
        "compute" => &[
            (
                "samplekit samples/ compute",
                "List what would run, and why; asked, compute it, then asked, write it",
            ),
            (
                "samplekit samples/ compute --write",
                "Compute what is outdated or never computed, and write it",
            ),
            (
                "samplekit samples/ compute -p area,curve.stress --write",
                "Only these values, with the inputs they need",
            ),
            (
                "samplekit samples/ compute --rerun --write",
                "Also the values that are current: after a formula changed",
            ),
            (
                "samplekit samples/ compute --force --write",
                "Also the values typed over their formula, given back to it",
            ),
            (
                "samplekit samples/ compute --rerun --force --write",
                "Every formula of the selected samples",
            ),
            (
                "samplekit samples/ compute --write --show-output",
                "Also show what the model prints, as it prints it",
            ),
        ],
        "explain" => &[
            (
                "samplekit samples/steel-01.md explain area",
                "Where a value came from: its inputs and their states",
            ),
            (
                "samplekit samples/steel-01.md explain 'curve.stress[0.2]'",
                "The same for one table cell",
            ),
        ],
        "log" => &[
            (
                "samplekit log",
                "Every snapshot of the project's history, newest first",
            ),
            (
                "samplekit samples/steel-01.md log",
                "Those that changed one sample",
            ),
            (
                "samplekit log --export -o HISTORY.md --write",
                "An account of the history in Markdown, to share beside the files",
            ),
            (
                "samplekit log --script 2",
                "The Python script that made snapshot #2 of the list, as it ran",
            ),
        ],
        "restore" => &[
            (
                "samplekit restore",
                "What taking back the last change kept would do; nothing is written",
            ),
            (
                "samplekit samples/steel-01.md restore --write",
                "Take back the last change kept of one sample",
            ),
            (
                "samplekit samples/ restore --at 3 --write",
                "Put the samples back as snapshot #3 of samplekit log kept them",
            ),
        ],
        "rename" => &[
            (
                "samplekit samples/steel-01.md rename steel-04",
                "What renaming one sample would do, its links with it; nothing is written",
            ),
            (
                "samplekit samples/steel-01.md rename steel-04 --write",
                "Rename it, its own files and every link leading to it",
            ),
        ],
        "remove" => &[
            (
                "samplekit samples/steel-01.md remove",
                "What deleting one sample would do; nothing is written",
            ),
            (
                "samplekit samples/ remove -f 'tags has scrapped' --write",
                "Delete the samples a filter selects; samplekit restore gives them back",
            ),
        ],
        "diff" => &[
            (
                "samplekit diff",
                "The last change SampleKit kept, value by value",
            ),
            (
                "samplekit samples/steel-01.md diff",
                "The last change kept of one sample",
            ),
            (
                "samplekit diff --from 1",
                "What changed since, outside SampleKit",
            ),
            (
                "samplekit diff --from 2026-09-01 --to 2026-09-12",
                "Between two dates, value by value",
            ),
        ],
        "open" => &[
            (
                "samplekit samples/steel-01.md open '*.jpg'",
                "Where [collection] files is declared: the sample's file whose name matches, \
                 in the system's viewer",
            ),
            (
                "samplekit samples/steel-01.md open --navigate",
                "Where [collection] files is declared: the folder holding its files, in the \
                 file manager",
            ),
            (
                "samplekit samples/steel-01.md open --edit",
                "The sample's own file, with the system's application for it",
            ),
            (
                "samplekit samples/ list files",
                "Where [collection] files is declared: every sample's files, and those \
                 belonging to none",
            ),
        ],
        "view" => &[
            (
                "samplekit samples/steel-01.md view",
                "The sample whole: its values, then its tables unfolded",
            ),
            (
                "samplekit samples/ view --query ductile --note",
                "Each sample a saved query selects, with its note",
            ),
            (
                "samplekit samples/steel-01.md view curve -o curve.csv --write",
                "One table alone, its rows written to a file",
            ),
        ],
        "export" => &[
            (
                "samplekit export paper",
                "What a declared export would write, and where; --write writes it",
            ),
            ("samplekit export paper -o -", "Print it instead"),
            (
                "samplekit export paper -o results.tex --write",
                "Write it elsewhere, replacing that file",
            ),
        ],
        "plot" => &[
            (
                "samplekit samples/ plot curves",
                "Open a declared figure, or the model's, in a matplotlib window",
            ),
            (
                "samplekit samples/ plot -x yield_strength -y tensile_strength --group material",
                "Draw two fields against each other, one series per material",
            ),
            (
                "samplekit samples/ plot curves --query ductile --group name",
                "A declared figure of the samples a saved query selects, one series per sample",
            ),
            (
                "samplekit samples/ plot curves -o curves.pdf --write",
                "Write it to a file instead, as over SSH",
            ),
        ],
        "validate" => &[
            (
                "samplekit samples/ validate",
                "The defects and notes of every sample; nothing is changed",
            ),
            (
                "samplekit samples/steel-01.md validate",
                "The same for one sample",
            ),
        ],
        "tag" => &[
            (
                REFERENCE,
                "Show which samples would gain the tag; nothing is written",
            ),
            (
                "samplekit samples/steel-01.md tag remove reference --write",
                "Remove a tag from one sample",
            ),
            (
                "samplekit samples/ tag rename old_name new_name --write",
                "Rename a tag across the collection",
            ),
        ],
        "tag add" => &[
            (
                REFERENCE,
                "Show which samples would gain the tag; nothing is written",
            ),
            (
                "samplekit samples/ tag add reference -f 'material == steel' --write",
                "Add it",
            ),
        ],
        "tag remove" => &[
            (
                "samplekit samples/ tag remove reference",
                "Show which samples would lose the tag; nothing is written",
            ),
            (
                "samplekit samples/steel-01.md tag remove reference --write",
                "Remove it from one sample",
            ),
        ],
        "tag rename" => &[
            (
                "samplekit samples/ tag rename old_name new_name",
                "Show where the tag would be renamed; nothing is written",
            ),
            (
                "samplekit samples/ tag rename old_name new_name --write",
                "Rename it across the collection",
            ),
        ],
        "tui" => &[
            (
                "samplekit samples/ tui",
                "The TUI on a folder: its samples, their values and states",
            ),
            (
                "samplekit",
                "At a terminal, the start page: the project here, the recent ones, a new one",
            ),
        ],
        "run" => &[
            (
                "samplekit list scripts",
                "The model's scripts, what each acts on, and its options",
            ),
            (
                "samplekit samples/ run classify --set limit=15",
                "Run a script on each sample, an option set; what it changes is shown, then asked",
            ),
            (
                "samplekit samples/ run classify --write",
                "Write what it changes without asking",
            ),
            (
                "samplekit run ranking",
                "A script of the selection, run once: what it prints is shown",
            ),
        ],
        "completions" => &[
            (
                "samplekit completions zsh",
                "Print the zsh bridge, to install once in a completion directory",
            ),
            ("samplekit completions bash", "The same for bash"),
        ],
        _ => &[],
    }
}

/// A help page's footer: its examples, then where the configuration was found.
fn help_footer(page: &str, configuration: &str) -> String {
    let rows = examples(page);
    if rows.is_empty() {
        return configuration.to_string();
    }
    let mut text = String::from("Examples:\n");
    for (command, what) in rows {
        text.push_str(&format!("  {command}\n      {what}\n"));
    }
    format!("{text}\n{configuration}")
}

/// The footer of every help page: a target elsewhere finds its own, so the page
/// says where this one was found from.
fn with_configuration_footer(command: clap::Command) -> clap::Command {
    let footer = match samplekit::config::project_config::find(Path::new(".")) {
        Some(path) => format!(
            "Configuration here: {}\n  found from the current directory; a target elsewhere finds its own",
            path.display()
        ),
        None => "Configuration here: none found from the current directory".to_string(),
    };
    command
        .after_help(help_footer("", &footer))
        .after_long_help(help_footer("", &footer))
        .mut_subcommands(|subcommand| {
            let name = subcommand.get_name().to_string();
            subcommand
                .after_help(help_footer(&name, &footer))
                .after_long_help(help_footer(&name, &footer))
                .mut_subcommands(|nested| {
                    let page = format!("{name} {}", nested.get_name());
                    nested
                        .after_help(help_footer(&page, &footer))
                        .after_long_help(help_footer(&page, &footer))
                })
        })
}

/// The path a command reads, for `--show-rc`.
fn target_of(cli: &Cli) -> PathBuf {
    let target = match &cli.command {
        None => cli.query.targets.first().cloned(),
        Some(Commands::List(args)) => args.targets.first().cloned().or_else(|| {
            args.kind_or_target
                .as_ref()
                .filter(|written| !KINDS.contains(&written.as_str()))
                .map(PathBuf::from)
        }),
        Some(Commands::Status(args)) => args.targets.first().cloned(),
        Some(Commands::Compute(args)) => args.targets.first().cloned(),
        Some(Commands::Explain(args)) => args.before.first().cloned().or_else(|| {
            args.words
                .iter()
                .find(|word| !names_nothing(word))
                .map(PathBuf::from)
        }),
        Some(Commands::Log(args)) => args.targets.first().cloned(),
        Some(Commands::Diff(args)) => args.targets.first().cloned(),
        Some(Commands::Restore(args)) => args.targets.first().cloned(),
        Some(Commands::Remove(args)) => args.targets.first().cloned(),
        Some(Commands::Rename(args)) => args.targets.first().cloned(),
        Some(Commands::Open(args)) => args.before.first().cloned().or_else(|| {
            args.words
                .iter()
                .find(|word| !names_nothing(word))
                .map(PathBuf::from)
        }),
        // `view curve`: the table's name is no target.
        Some(Commands::View(args)) => args.targets.iter().find(|target| target.exists()).cloned(),
        Some(Commands::Export(args)) => args.targets.first().cloned(),
        Some(Commands::Plot(args)) => match (&args.x, &args.figure) {
            (Some(_), Some(first)) => Some(PathBuf::from(first)),
            _ => args.targets.first().cloned(),
        },
        Some(Commands::Run(args)) => args.targets.first().cloned(),
        Some(Commands::Validate(args)) => args.targets.first().cloned(),
        Some(Commands::Tag(args)) => match &args.edit {
            TagEdit::Add(one) | TagEdit::Remove(one) => one.target.targets.first().cloned(),
            TagEdit::Rename(rename) => rename.target.targets.first().cloned(),
        },
        Some(Commands::Init(args)) => args.directory.clone(),
        Some(Commands::New(args)) => args
            .into
            .clone()
            .or_else(|| args.keep.first().map(PathBuf::from)),
        Some(Commands::Set(args)) => args.before.first().cloned().or_else(|| {
            args.words
                .iter()
                .find(|word| !word.contains('='))
                .map(PathBuf::from)
        }),
        Some(Commands::Completions(_)) => None,
        Some(Commands::Tui(args)) => args.directory.clone(),
    };
    target.unwrap_or_else(|| PathBuf::from("."))
}

/// The configuration, the model and the interpreter a command would use, and
/// nothing else. `--show-rc`: every configuration a directory spans, each with
/// its model and interpreter.
fn show_rc(target: &Path, merged: bool) -> Outcome {
    if forced_configuration().is_none()
        && target.is_dir()
        && let Ok(collection) = list::from_directory(target)
        && let Some(samplekit::config::discovery::Warning::MixedConfigurations { roots }) =
            collection.warnings().first()
    {
        for (at, file) in roots.iter().enumerate() {
            if at > 0 {
                outln!();
            }
            let directory = file
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(target);
            show_one_rc(directory, merged)?;
        }
        return Ok(());
    }
    show_one_rc(target, merged)
}

fn show_one_rc(target: &Path, merged: bool) -> Outcome {
    let (file, config) = match forced_configuration() {
        Some((path, config)) => (Some(path), Some(config)),
        None => {
            let file = samplekit::config::project_config::find(target);
            let config = match &file {
                Some(path) => Some(
                    samplekit::config::project_config::load(path)
                        .map_err(|error| Fail::data(error.to_string()))?,
                ),
                None => None,
            };
            (file, config)
        }
    };
    match &file {
        Some(path) => outln!("configuration  {}", as_here(path).display()),
        None => outln!(
            "configuration  none found from {}",
            std::path::absolute(target)
                .unwrap_or_else(|_| target.to_path_buf())
                .display()
        ),
    }
    // What it takes from above, nearest first.
    for imported in config.iter().flat_map(|config| config.imports()) {
        outln!("imports        {}", imported.display());
    }
    outln!("model          {}", model_of(config.as_ref()));
    if let Some(config) = config
        .as_ref()
        .filter(|config| config.model().is_some() || config.python().is_some())
    {
        match runtime::interpreter_for(config, target) {
            Ok(python) => outln!("interpreter    {}", python.display()),
            Err(ModelError::NoInterpreter { reason }) => {
                outln!(
                    "interpreter    none: {}",
                    reason.lines().next().unwrap_or_default()
                )
            }
            Err(other) => outln!("interpreter    none: {other}"),
        }
    } else if config.is_some() {
        // Figures are drawn in a Python too, a model or none.
        outln!(
            "interpreter    none declared: [model] python names the one that computes and \
             draws the figures"
        );
    }
    if merged && let (Some(file), Some(config)) = (&file, &config) {
        outln!();
        let width = merged_keys(file, config)
            .iter()
            .map(|(key, value, _)| render::width(key) + 3 + render::width(value))
            .max()
            .unwrap_or(0);
        for (key, value, from) in merged_keys(file, config) {
            let written = format!("{key} = {value}");
            outln!(
                "{written}{}   {}",
                " ".repeat(width.saturating_sub(render::width(&written))),
                from.display()
            );
        }
    }
    Ok(())
}

/// Every key the configuration at `file` holds once its imports are merged —
/// `property.radius.precision` — with its value as written and the file it comes
/// from: the nearest that writes it, its imports behind it. `[model] path` and
/// `class`, never imported, only from `file`.
fn merged_keys(file: &Path, config: &ProjectConfig) -> Vec<(String, String, PathBuf)> {
    fn flatten(prefix: &str, value: &toml::Value, into: &mut Vec<(String, String)>) {
        match value {
            toml::Value::Table(table) => {
                for (key, value) in table {
                    let quoted = if key
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    {
                        key.clone()
                    } else {
                        format!("\"{key}\"")
                    };
                    let name = if prefix.is_empty() {
                        quoted
                    } else {
                        format!("{prefix}.{quoted}")
                    };
                    flatten(&name, value, into);
                }
            }
            other => into.push((prefix.to_string(), other.to_string())),
        }
    }
    // Farthest first, so that a nearer file's value replaces it.
    let mut files: Vec<PathBuf> = config.imports().iter().rev().cloned().collect();
    files.push(file.to_path_buf());
    let mut merged: Vec<(String, String, PathBuf)> = Vec::new();
    for (at, path) in files.iter().enumerate() {
        let Some(table) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            continue;
        };
        let mut keys = Vec::new();
        flatten("", &toml::Value::Table(table), &mut keys);
        let own = at == files.len() - 1;
        for (key, value) in keys {
            if key == "import" || (!own && (key == "model.path" || key == "model.class")) {
                continue;
            }
            match merged.iter_mut().find(|(held, _, _)| *held == key) {
                Some(entry) => *entry = (key, value, path.clone()),
                None => merged.push((key, value, path.clone())),
            }
        }
    }
    merged
}

// --------------------------------------------------------------- computing

struct ComputeFlags {
    names: Vec<String>,
    rerun: bool,
    force: bool,
    /// How long one formula may run.
    timeout: Option<std::time::Duration>,
    dry_run: bool,
    write: bool,
    show_output: bool,
}

#[derive(Default)]
struct Totals {
    computed: usize,
    failed: usize,
    /// Samples changed by another writer while they were computed, and not
    /// saved: no formula failed, and no traceback is owed.
    not_saved: Vec<String>,
    samples: usize,
    /// Samples whose file a `--write` changed: a value computed again to the
    /// same number rewrites nothing, and was counted as written.
    written: usize,
    /// Their files, from here, as the outcome names them.
    written_paths: Vec<String>,
    planned: Vec<Vec<String>>,
    /// The cells each planned value counts: a column its rows, a value and
    /// its uncertainty of two formulas two.
    planned_cells: Vec<usize>,
    /// Why a listing could not read a project's model: said after what the
    /// files record, and the command's failure.
    unread_models: Vec<Fail>,
    /// Samples asked for named values, and those that had none of them.
    asked: usize,
    unknown: usize,
    unknown_messages: Vec<String>,
    /// The last line of every traceback printed: a repeated error prints once.
    tracebacks: std::collections::HashSet<String>,
    /// Whose failure printed each traceback first: a later one the same is
    /// said as that sample's, `the same error as s1's above` — *above* read
    /// as the line just above, another error.
    traced_by: std::collections::HashMap<String, String>,
    /// Samples whose model raised while loading them.
    unloaded: usize,
    /// Values a run narrowed by names left stale.
    pending: usize,
    /// Values not run for want of an input, and the inputs they wait for.
    waiting: usize,
    waited_for: Vec<String>,
    waiting_values: Vec<String>,
    /// Values whose uncertainty formula waits for their own value.
    own: Vec<String>,
    /// What each value was before and after: sample, value, before, after.
    changes: Vec<[String; 4]>,
    /// The cells each of `changes` counts, in its order.
    change_cells: Vec<usize>,
    /// Samples computed by `--try`, by worker, for a write that may follow.
    tried: Vec<(usize, PathBuf, Template)>,
    /// Samples left out of the run because their records cannot be followed.
    unplannable: Vec<String>,
    /// The records each sample held before the run, by its path, and the
    /// configuration describing it: what the trace keeps is told by them.
    before: std::collections::HashMap<PathBuf, Vec<samplekit::config::version_control::HeldRecord>>,
    configurations: std::collections::HashMap<PathBuf, Option<ProjectConfig>>,
    /// What the writes keep in each project's trace.
    tracing: samplekit::collection::computation::Tracing,
}

/// Set by an interruption, so that a computation stops and says what it kept.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The workers' processes, which an interruption interrupts too.
static WORKER_PIDS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

// Samples by the configuration that describes them: each group has one
// model and one interpreter.
use samplekit::collection::computation::Group;

/// A computation's progress: one line on stderr when it is a terminal, redrawn in
/// place, which every report line and the model's own output are written above.
/// Nothing when stderr is not a terminal.
struct Progress;

struct Bar {
    width: usize,
    drawn: bool,
    samples: usize,
    sample_at: usize,
    sample: String,
    planned: usize,
    done: usize,
    value: Option<String>,
    /// When the value in progress started, for its own time.
    value_started: Option<Instant>,
    started: Instant,
}

/// The bar, shared with the thread that relays the model's output.
static BAR: std::sync::Mutex<Option<Bar>> = std::sync::Mutex::new(None);

impl Bar {
    fn draw(&mut self) {
        let within = if self.planned == 0 {
            0.0
        } else {
            self.done.min(self.planned) as f64 / self.planned as f64
        };
        let fraction =
            (self.sample_at.saturating_sub(1) as f64 + within) / self.samples.max(1) as f64;
        let mut parts = Vec::new();
        if self.samples > 1 {
            parts.push(format!("sample {}/{}", self.sample_at, self.samples));
        }
        if self.planned > 0 {
            parts.push(format!("{}/{}", self.done, self.planned));
        }
        if let Some(value) = &self.value {
            let took = self
                .value_started
                .map(|at| at.elapsed().as_secs_f64())
                .filter(|seconds| *seconds >= 1.0);
            parts.push(match took {
                Some(seconds) => format!("{value} {}", duration(seconds)),
                None => value.clone(),
            });
        }
        parts.push(self.sample.clone());
        let line = render::progress_line(
            fraction,
            &parts.join(" · "),
            &duration(self.started.elapsed().as_secs_f64()),
            self.width,
        );
        errp!("\r\x1b[2K{line}");
        let _ = io::stderr().flush();
        self.drawn = true;
    }

    fn erase(&mut self) {
        if self.drawn {
            errp!("\r\x1b[2K");
            let _ = io::stderr().flush();
            self.drawn = false;
        }
    }
}

/// What the model printed during a computation, and what it logged: kept in a
/// file of the machine's state, shown only under `--show-output`.
struct RunLog {
    show: bool,
    directory: Option<PathBuf>,
    /// The project computed, whose log this is: one a project, the last
    /// run's, replaced by the next.
    project: PathBuf,
    file: Option<(PathBuf, std::fs::File)>,
    lines: usize,
    label: String,
    value: String,
    /// The last lines of the value in progress, shown when it fails.
    recent: std::collections::VecDeque<String>,
}

static RUN_LOG: std::sync::Mutex<Option<RunLog>> = std::sync::Mutex::new(None);

impl RunLog {
    fn new(show: bool, project: &Path) -> RunLog {
        let directory = runtime::state_directory().map(|state| state.join("logs"));
        RunLog {
            show,
            directory,
            project: std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf()),
            file: None,
            lines: 0,
            label: String::new(),
            value: String::new(),
            recent: std::collections::VecDeque::new(),
        }
    }

    /// Writes a line, and returns the file's path when this line opened it.
    fn write(&mut self, label: &str, value: &str, text: &str) -> Option<PathBuf> {
        let mut opened = None;
        if self.file.is_none() {
            self.file = self.open();
            opened = self.file.as_ref().map(|(path, _)| path.clone());
        }
        if let Some((_, file)) = self.file.as_mut() {
            let prefix = if value.is_empty() {
                format!("[{label}]")
            } else {
                format!("[{label}] {value}:")
            };
            let _ = writeln!(file, "{prefix} {text}");
        }
        self.lines += 1;
        self.recent.push_back(text.to_string());
        if self.recent.len() > 10 {
            self.recent.pop_front();
        }
        opened
    }

    /// The run's file, created with its first line over the project's last:
    /// the last run's alone is kept, whole.
    fn open(&self) -> Option<(PathBuf, std::fs::File)> {
        use std::hash::{Hash, Hasher};
        let directory = self.directory.as_ref()?;
        std::fs::create_dir_all(directory).ok()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.project.hash(&mut hasher);
        let name = self
            .project
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = directory.join(format!("compute-{name}-{:016x}.log", hasher.finish()));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .ok()?;
        Some((path, file))
    }
}

impl Progress {
    fn new(samples: usize) -> Progress {
        if io::stderr().is_terminal() {
            *bar() = Some(Bar {
                width: terminal_columns(),
                drawn: false,
                samples,
                sample_at: 0,
                sample: String::new(),
                planned: 0,
                done: 0,
                value: None,
                value_started: None,
                started: Instant::now(),
            });
            // The command waits on the worker's next message, so the clock is
            // redrawn from here while a formula computes in silence.
            std::thread::spawn(|| {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let mut held = bar();
                    match held.as_mut() {
                        Some(bar) if bar.drawn => bar.draw(),
                        Some(_) => {}
                        None => break,
                    }
                }
            });
        }
        Progress
    }

    fn update(&self, change: impl FnOnce(&mut Bar)) {
        if let Some(bar) = bar().as_mut() {
            change(bar);
            bar.draw();
        }
    }

    fn next_sample(&self, sample: &str) {
        self.update(|bar| {
            bar.sample_at += 1;
            bar.sample = sample.to_string();
            bar.planned = 0;
            bar.done = 0;
            bar.value = None;
            bar.value_started = None;
        });
    }

    /// A plan names what is left in the sample.
    fn planned(&self, count: usize) {
        self.update(|bar| bar.planned = bar.done + count);
    }

    fn at(&self, value: &str) {
        self.update(|bar| {
            bar.value = Some(value.to_string());
            bar.value_started = Some(Instant::now());
        });
    }

    fn finished(&self) {
        self.update(|bar| {
            bar.done += 1;
            bar.value = None;
            bar.value_started = None;
        });
    }

    /// Writes above the bar: the bar is erased, `write` runs, the bar comes back.
    fn above(&self, write: impl FnOnce()) {
        let mut held = bar();
        if let Some(bar) = held.as_mut() {
            bar.erase();
        }
        write();
        if let Some(bar) = held.as_mut() {
            bar.draw();
        }
    }

    fn finish(&self) {
        let mut held = bar();
        if let Some(bar) = held.as_mut() {
            bar.erase();
        }
        *held = None;
    }
}

// ----------------------------------------------------------------- helpers

/// How many samples hold each name.
fn name_counts(names: impl Iterator<Item = Option<String>>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for name in names.flatten() {
        *counts.entry(name).or_insert(0) += 1;
    }
    counts
}

/// A sample's name, and its file beside it where another sample shares the
/// name.
fn labelled(
    counts: &HashMap<String, usize>,
    path: Option<&Path>,
    sample: &samplekit::core::sample::Sample,
) -> String {
    let base = file_of(path, sample);
    // A name two samples hold is told apart by the path read: by its file
    // alone, two projects' `alpha-titanium (alpha-titanium.md)` were still one.
    match (sample.name(), path) {
        (Some(name), Some(path)) if counts.get(name).is_some_and(|count| *count > 1) => {
            let shown = path.strip_prefix(".").unwrap_or(path);
            format!("{base} ({})", shown.display())
        }
        _ => base,
    }
}

fn file_of(path: Option<&Path>, sample: &samplekit::core::sample::Sample) -> String {
    if let Some(name) = sample.name() {
        return name.to_string();
    }
    path.and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "—".to_string())
}

/// An unknown name is never an empty result: it is reported with the nearest
/// existing one, and with the command that lists them all.
fn with_suggestion(error: fields::FieldError, collection: &SampleList) -> String {
    let groups = field_groups(collection);
    let known: Vec<String> = collection
        .available_fields()
        .iter()
        .map(fields::describe)
        .collect();
    match &error {
        fields::FieldError::UnknownProperty {
            name, suggestion, ..
        } => {
            // A sort key or a filter's field misspelt had no advice, where a
            // column had one: the nearest known field is offered alike.
            let suggestion = suggestion.clone().or_else(|| {
                samplekit::core::identifier::nearest(name, known.iter().map(String::as_str))
            });
            let (head, a_column) = unknown_field_head(name, suggestion.as_deref(), &groups);
            if a_column || known.is_empty() {
                head
            } else {
                format!("{head}\n{}", fields_footer(&known, suggestion.is_none()))
            }
        }
        other => {
            let mut message = field_error_body(other, &groups);
            if !message.contains("did you mean") && !known.is_empty() {
                message.push_str(&format!(
                    "\n  run '{}' to see every addressable field",
                    advised_here("list fields")
                ));
            }
            message
        }
    }
}

/// An unknown field's first lines, as every option naming a field says them: a
/// table's column without its index is said as one, with the table as `list
/// fields` shows it, since that is what it is. The flag is true when it was a
/// column, which needs no list of fields beneath it.
fn unknown_field_head(
    name: &str,
    suggestion: Option<&str>,
    groups: &FieldGroups,
) -> (String, bool) {
    if let Some((table, column)) = name.split_once('.')
        && let Some(group) = groups.tables.iter().find(|group| group.name == table)
        && group
            .columns
            .iter()
            .chain(&group.index)
            .any(|known| known == column)
    {
        return (
            format!(
                "'{name}' is a column of the table '{table}', and a column has one cell per row: \
                 address one, {name}[<index>]\n{}",
                table_group_listing(group, 100).trim_end()
            ),
            true,
        );
    }
    let mut head = format!("unknown field '{name}'");
    if let Some(suggestion) = suggestion {
        head.push_str(&format!("\n  did you mean: '{suggestion}'?"));
    }
    (head, false)
}

/// What closes a message about unknown fields: the first of them where no
/// suggestion was found, and how many there are, with the command listing them.
fn fields_footer(known: &[String], show_some: bool) -> String {
    let mut footer = String::new();
    if show_some {
        let shown: Vec<&str> = known.iter().take(12).map(String::as_str).collect();
        footer.push_str(&format!("  available: {}", shown.join(", ")));
        if known.len() > shown.len() {
            footer.push_str(", …");
        }
        footer.push('\n');
    }
    footer.push_str(&format!(
        "  {} fields available — {}",
        known.len(),
        advised_here("list fields")
    ));
    footer
}

/// An index value as `list fields` writes it, read back as the value it is:
/// `20` a number, `Lea` a text.
fn row_value(written: &str) -> Value {
    samplekit::collection::editing::value_of(written).unwrap_or_else(|_| Value::text(written))
}

/// The address of the row nearest the one `field` names, among `rows` — the
/// index values of a table with one index column — as `set` offers it :
/// `inspection.score[Lee]` is `inspection.score[Lea]`, the same letters in another
/// case first, a number by proximity. A composite index is said per column, by
/// the table's own error.
fn nearest_row_address(field: &str, rows: &[Value]) -> Option<String> {
    let open = field.find('[')?;
    let close = open + field[open..].find(']')?;
    let inner = field[open + 1..close].trim();
    if inner.contains(',') {
        return None;
    }
    let wanted = row_value(inner.trim_matches(['\'', '"']));
    let nearest = samplekit::core::table::nearest_index(rows, &wanted)?;
    if samplekit::core::value::equals(&nearest, &wanted) {
        return None;
    }
    Some(format!(
        "{}[{}]{}",
        &field[..open],
        samplekit::collection::editing::shown_value(&nearest),
        &field[close + 1..]
    ))
}

/// A field error other than an unknown name, with what the collection holds
/// where the error alone does not say it: a row no index names lists the rows
/// there are, which the error's own words leave out.
fn field_error_body(error: &fields::FieldError, groups: &FieldGroups) -> String {
    let said = error.to_string().trim_end().to_string();
    match error {
        fields::FieldError::UnknownIndex { table, .. } => {
            match groups
                .tables
                .iter()
                .find(|group| group.name == table.as_str())
            {
                Some(group) if !group.rows.is_empty() => {
                    // The per-column lines say *matched* of a column no row
                    // matched on its own; the rows there are answer it. The
                    // nearest is kept, as `set` offers it.
                    let head = said.lines().next().unwrap_or(&said).to_string();
                    let mut body = format!(
                        "{head}\n  the rows of '{table}', by {}: {}",
                        group.index.join(", "),
                        group.rows.join(", ")
                    );
                    for line in said.lines().skip(1) {
                        if line.contains("did you mean") || line.contains("nearest is") {
                            body.push_str(&format!("\n{line}"));
                        }
                    }
                    body
                }
                _ => said,
            }
        }
        _ => said,
    }
}

/// The words a state is written in, wearing its colour where colour is allowed.
/// The mark in a table and these words take the same colour for the same state,
/// or the two teach different things.
fn describe(freshness: &Freshness) -> String {
    render::painted_as(&describe_plain(freshness), freshness, coloured())
}

fn describe_plain(freshness: &Freshness) -> String {
    samplekit::presentation::changes::state_said(freshness)
}

/// `--timeout SECONDS`: a number of seconds above zero, `0.5` or `30`.
fn seconds(written: &str) -> Result<std::time::Duration, String> {
    match written.trim().parse::<f64>() {
        Ok(seconds) if seconds > 0.0 && seconds.is_finite() => {
            Ok(std::time::Duration::from_secs_f64(seconds))
        }
        _ => Err(format!(
            "'{written}' is no number of seconds above zero: --timeout 30, or 0.5"
        )),
    }
}

/// What `--accept` lets pass, for `status --exit-code` and for a file written
/// from values not current alike.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accepted {
    /// Every edited value: `edited`.
    pub edited: bool,
    /// The edited values named: `edited:area,young_modulus`.
    pub edited_names: Vec<String>,
    /// `outdated`.
    pub outdated: bool,
    /// `failed`.
    pub failed: bool,
    /// `never_computed`.
    pub never_computed: bool,
}

impl Accepted {
    /// Whether a value in `state` named `value` passes the gate.
    pub fn passes(&self, state: &str, value: &str) -> bool {
        match state {
            "waiting" => true,
            "edited" => self.edited || self.edited_names.iter().any(|name| name == value),
            "outdated" => self.outdated,
            "failed" => self.failed,
            "never computed" | "never_computed" => self.never_computed,
            _ => false,
        }
    }
}

/// `edited`, `waiting`, `edited:modulus,volume` — the names after `edited:` run to
/// the next state word, a comma between each.
fn accepted_states(written: &str) -> Result<Accepted, String> {
    let mut accepted = Accepted::default();
    let mut naming = false;
    for word in written
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
    {
        match word.split_once(':') {
            Some(("edited", first)) => {
                naming = true;
                if !first.trim().is_empty() {
                    accepted.edited_names.push(first.trim().to_string());
                }
            }
            None if word == "edited" => {
                accepted.edited = true;
                naming = false;
            }
            None if word == "waiting" => {
                return Err(
                    "'waiting' is no state --accept takes: a value waiting for an input fails \
                     nothing, so there is nothing to accept"
                        .to_string(),
                );
            }
            None if word == "outdated" => {
                accepted.outdated = true;
                naming = false;
            }
            None if word == "failed" => {
                accepted.failed = true;
                naming = false;
            }
            // One spelling written, the others still read.
            None if matches!(word, "never_computed" | "never-computed" | "never computed") => {
                accepted.never_computed = true;
                naming = false;
            }
            None if naming => accepted.edited_names.push(word.to_string()),
            _ => {
                return Err(format!(
                    "'{word}' is no state --accept takes: outdated, edited, edited:NAME,… for \
                     those values alone, failed or never_computed, a comma between"
                ));
            }
        }
    }
    Ok(accepted)
}
