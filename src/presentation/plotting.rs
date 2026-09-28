//! A figure, drawn by matplotlib through the project's Python.
//!
//! This module decides what to draw and on which files, and hands both to
//! `python -m samplekit._figure` as one JSON object. The drawing, the window
//! and every diagnostic are that process's; its exit code is the command's.

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value as Json, json};

use crate::config::project_config::{FigureDeclaration, Limits};

/// What the Python side is asked to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum FigureChoice {
    /// `[figure.<name>]` when `model` is false, the model's figure otherwise:
    /// it decides whether the model is loaded at all.
    Named { name: String, model: bool },
    /// Axes given on the command line: a declaration nobody wrote down.
    AdHoc(Box<FigureDeclaration>),
}

/// What the command line changes of a declared or given figure: each replaces
/// what the declaration says, and `None` leaves it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FigureOverrides {
    pub title: Option<String>,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub style: Option<String>,
    pub group: Option<String>,
    pub kind: Option<String>,
    pub x_limits: Option<Limits>,
    pub y_limits: Option<Limits>,
    pub x_scale: Option<String>,
    pub y_scale: Option<String>,
    pub aspect: Option<String>,
    pub legend: Option<String>,
    pub figsize: Option<(f64, f64)>,
}

/// One drawing: the figure, the sample files it draws, and where to write it
/// — `None` opens the window.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureRequest {
    pub choice: FigureChoice,
    pub samples: Vec<PathBuf>,
    pub output: Option<PathBuf>,
    pub overrides: FigureOverrides,
    /// Whether `[matplotlib]` shapes the figure; `--no-project-style` sets it
    /// aside.
    pub project_style: bool,
    /// What the history says wrote a file this draws: the command line. Python
    /// names itself where it is `None`.
    pub said: Option<String>,
    /// The snapshot a figure drawn from the project as it was carries in its
    /// metadata, where the history is not written from the drawing.
    pub snapshot: Option<String>,
}

#[derive(Debug)]
pub enum PlotError {
    /// The interpreter could not be started.
    NotStarted { python: PathBuf, reason: String },
    /// The Python side refused or failed, having said why on stderr.
    Exited { code: u8 },
    /// The Python side was killed by a signal other than Ctrl-C's — a crash
    /// in matplotlib or its toolkit, or the system out of memory.
    Killed { signal: i32 },
}

impl fmt::Display for PlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlotError::NotStarted { python, reason } => {
                write!(f, "{} could not be started: {reason}", python.display())
            }
            PlotError::Exited { code } => write!(f, "the figure exited with code {code}"),
            PlotError::Killed { signal } => write!(
                f,
                "the figure's Python was killed by signal {signal}{}, and nothing it drew \
                 was kept",
                match signal {
                    6 => " (SIGABRT)",
                    9 => " (SIGKILL — out of memory, or killed)",
                    11 => " (SIGSEGV — a crash in matplotlib or its window toolkit)",
                    15 => " (SIGTERM)",
                    _ => "",
                }
            ),
        }
    }
}

impl std::error::Error for PlotError {}

/// The request as the one JSON object `python -m samplekit._figure` reads.
pub fn request_json(request: &FigureRequest) -> String {
    let samples: Vec<String> = request
        .samples
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let output = request
        .output
        .as_ref()
        .map(|path| Json::String(path.to_string_lossy().into_owned()))
        .unwrap_or(Json::Null);
    let overrides = &request.overrides;
    let mut body = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "samples": samples,
        "output": output,
        "project_style": request.project_style,
        "said": request.said,
        "snapshot": request.snapshot,
        "overrides": {
            "title": overrides.title,
            "x_label": overrides.x_label,
            "y_label": overrides.y_label,
            "style": overrides.style,
            "group": overrides.group,
            "kind": overrides.kind,
            "x_limits": overrides.x_limits.map(|(low, high)| [low, high]),
            "y_limits": overrides.y_limits.map(|(low, high)| [low, high]),
            "x_scale": overrides.x_scale,
            "y_scale": overrides.y_scale,
            "aspect": overrides.aspect,
            "legend": overrides.legend,
            "figsize": overrides.figsize.map(|(width, height)| [width, height]),
        },
    });
    match &request.choice {
        FigureChoice::Named { name, model } => {
            body["figure"] = json!(name);
            body["model"] = json!(model);
        }
        FigureChoice::AdHoc(declaration) => {
            body["model"] = json!(false);
            body["axes"] = json!({
                "kind": declaration.kind.as_str(),
                "x": declaration.x,
                "y": declaration.y,
                "group": declaration.group,
            });
        }
    }
    body.to_string()
}

/// Runs the drawing in `python` and waits for it: a window stays open until it
/// is closed. Its output, its diagnostics and its window are the user's.
pub fn draw(python: &Path, request: &FigureRequest) -> Result<(), PlotError> {
    run(python, &request_json(request), false).map(|_| ())
}

/// The same drawing with what Python says kept rather than shown — for a
/// surface that owns the terminal, where it would be drawn over. A failure
/// carries the last line it said.
pub fn draw_quietly(python: &Path, request: &FigureRequest) -> Result<(), (PlotError, String)> {
    let not_started = |reason: String| {
        (
            PlotError::NotStarted {
                python: python.to_path_buf(),
                reason,
            },
            String::new(),
        )
    };
    let mut child = Command::new(python)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .args(["-c", BOOTSTRAP])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| not_started(error.to_string()))?;
    let written = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(request_json(request).as_bytes());
    let output = child
        .wait_with_output()
        .map_err(|error| not_started(error.to_string()))?;
    let said = String::from_utf8_lossy(&output.stderr)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string();
    match output.status.code() {
        Some(0) => written.map_err(|error| not_started(error.to_string())),
        Some(code) => Err((
            PlotError::Exited {
                code: u8::try_from(code).unwrap_or(2),
            },
            said,
        )),
        // Ctrl-C is an exit, 130, as `run` says it; another signal a kill.
        None => Err((
            match signal_of(&output.status) {
                Some(2) | None => PlotError::Exited { code: 130 },
                Some(signal) => PlotError::Killed { signal },
            },
            said,
        )),
    }
}

/// Starts the figure's module, saying so where the environment has no
/// SampleKit, rather than Python's exit 1, which reads as a refusal.
const BOOTSTRAP: &str = "import sys
try:
    from samplekit._figure import main
except ModuleNotFoundError as error:
    print(f'error: this environment ({sys.executable}) has no {error.name}: a figure is '
          'drawn by the samplekit and matplotlib installed in it', file=sys.stderr)
    sys.exit(3)
sys.exit(main())
";

fn run(python: &Path, body: &str, capture: bool) -> Result<String, PlotError> {
    let not_started = |reason: String| PlotError::NotStarted {
        python: python.to_path_buf(),
        reason,
    };
    // Ctrl-C reaches the Python side too, which stops and exits 130: this
    // side waits for it, rather than dying first and leaving it behind. The
    // handler also takes SIGTERM and SIGHUP — the `termination` feature the
    // command line needs — which reach this process alone: a handler doing
    // nothing made `kill` of a `plot` with a window open do nothing. They are
    // passed on to the Python side, after the moment an interrupted one takes
    // to stop by itself, and this side then ends with it.
    let _ = ctrlc::set_handler(|| {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let child = DRAWING.load(std::sync::atomic::Ordering::SeqCst);
        if child != 0 {
            terminate(child);
        }
    });
    let mut child = Command::new(python)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .args(["-c", BOOTSTRAP])
        .stdin(Stdio::piped())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| not_started(error.to_string()))?;
    DRAWING.store(child.id(), std::sync::atomic::Ordering::SeqCst);
    // A Python side that exits before reading its request closes the pipe:
    // its exit code says why, not the write that failed.
    let written = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(body.as_bytes());
    let output = child.wait_with_output();
    DRAWING.store(0, std::sync::atomic::Ordering::SeqCst);
    let output = output.map_err(|error| not_started(error.to_string()))?;
    match output.status.code() {
        Some(0) => match written {
            Ok(()) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
            Err(error) => Err(not_started(error.to_string())),
        },
        Some(code) => Err(PlotError::Exited {
            code: u8::try_from(code).unwrap_or(2),
        }),
        None => match signal_of(&output.status) {
            Some(2) | None => Err(PlotError::Exited { code: 130 }),
            Some(signal) => Err(PlotError::Killed { signal }),
        },
    }
}

/// The Python side drawing now, by its process id; 0 when none is.
static DRAWING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A termination passed on to the Python side, by the system's `kill`: the
/// standard library signals no process but its own children's by `kill()`,
/// which is SIGKILL, and a handler holds no `Child`.
#[cfg(unix)]
fn terminate(child: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &child.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn terminate(_: u32) {}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<i32> {
    None
}
