//! A model's script, run by its Python.
//!
//! A method of the model marked `@sk.script` acts on one sample, or on the
//! selection. This module hands `python -m samplekit._script` the script's
//! name, the files and the options as one JSON object; the script prints as
//! it runs, what it returns is printed after it, and what it would save is
//! held, not written, and comes back here for the command to show, ask about
//! and write.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::json;

/// One run: the script, the sample files it acts on, its options as typed —
/// read by the script's signature on the Python side — and the style what it
/// returns is written in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptRequest {
    pub script: String,
    pub samples: Vec<PathBuf>,
    pub options: BTreeMap<String, String>,
    pub style: Option<String>,
    /// Past it the script is stopped, and nothing is written: `--timeout`.
    pub timeout: Option<std::time::Duration>,
}

/// A file the script would change: what it holds now — `None` where it does
/// not exist — and what it would hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

/// Why a script did not end with its changes held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// The interpreter could not be started, or the request not handed over.
    NotStarted { python: PathBuf, reason: String },
    /// It exited with this code: 1 a refusal, 2 the script failing, 130
    /// interrupted, each said on its standard error.
    Exited { code: u8 },
    /// A signal other than Ctrl-C ended it.
    Killed { signal: i32 },
    /// It ran past `--timeout` and was stopped.
    TimedOut { seconds: String },
    /// It ended well and left no reply that reads.
    NoReply { reason: String },
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::NotStarted { python, reason } => write!(
                f,
                "the model's Python, {}, could not be started: {reason}",
                python.display()
            ),
            ScriptError::Exited { code } => write!(f, "the script exited with code {code}"),
            ScriptError::Killed { signal } => {
                write!(f, "the script was ended by signal {signal}")
            }
            ScriptError::NoReply { reason } => {
                write!(
                    f,
                    "the script ended and said nothing of its changes: {reason}"
                )
            }
            ScriptError::TimedOut { seconds } => write!(
                f,
                "the script ran past --timeout {seconds} s and was stopped: nothing is written"
            ),
        }
    }
}

/// The script stopped, as a worker past its limit is.
#[cfg(unix)]
fn stop(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn stop(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

impl std::error::Error for ScriptError {}

/// `--set NAME=VALUE`, each: the options by name. A word with no `=`, or an
/// option given twice, is refused naming it.
pub fn options_of(words: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    for word in words {
        let Some((name, value)) = word.split_once('=') else {
            return Err(format!(
                "--set {word}: an option is NAME=VALUE, --set {word}=…"
            ));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("--set {word}: the option's name is missing"));
        }
        if options
            .insert(name.to_string(), value.to_string())
            .is_some()
        {
            return Err(format!("--set {name} is given twice"));
        }
    }
    Ok(options)
}

/// The request as the Python side reads it, the reply to be written at
/// `reply`.
pub fn request_json(request: &ScriptRequest, reply: &Path) -> String {
    json!({
        "version": crate::config::model_runtime::VERSION,
        "script": request.script,
        "samples": request
            .samples
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>(),
        "options": request.options,
        "style": request.style,
        "reply": reply.to_string_lossy(),
    })
    .to_string()
}

/// A question a script asked with `input()`, and the line that answered it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Answer {
    pub prompt: String,
    pub answer: String,
}

/// A script that ended well: what it printed — kept where it was not shown —
/// the files it would change, and the questions it asked, answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub printed: String,
    pub held: Vec<Held>,
    pub answers: Vec<Answer>,
}

/// What a script started beside a screen says: what it prints, as it prints
/// it, then how it ended — a failure with the last line of its standard
/// error.
#[derive(Debug)]
pub enum ScriptEvent {
    Printed(String),
    Ended(Result<Ran, (ScriptError, String)>),
}

/// Runs the script in the terminal: what it prints shown as it prints it, and
/// a question it asks with `input()` read from the terminal, as the script
/// run by Python itself would.
pub fn run(python: &Path, request: &ScriptRequest) -> Result<Ran, ScriptError> {
    let (child, folder) = spawn(python, request, false).map_err(|(error, _)| error)?;
    let pid = child.id();
    let watch = watch(request.timeout, pid);
    let output = child.wait_with_output();
    ended(output, watch, request, &folder, String::new()).map_err(|(error, _)| error)
}

/// Starts the script beside a screen: its output comes as events, as it
/// prints it, and what `input()` asks is answered by writing a line to the
/// handle returned.
pub fn start(
    python: &Path,
    request: &ScriptRequest,
) -> Result<
    (
        std::process::ChildStdin,
        std::sync::mpsc::Receiver<ScriptEvent>,
    ),
    ScriptError,
> {
    use std::io::Read as _;
    let (mut child, folder) = spawn(python, request, true).map_err(|(error, _)| error)?;
    let input = child.stdin.take().expect("stdin is piped");
    let (sender, receiver) = std::sync::mpsc::channel();
    let pid = child.id();
    let request = request.clone();
    // Each stream read as it comes, a question's prompt printed without its
    // line's end included.
    let reader = |stream: Option<Box<dyn std::io::Read + Send>>,
                  sender: std::sync::mpsc::Sender<ScriptEvent>| {
        std::thread::spawn(move || {
            let mut kept = String::new();
            let Some(mut stream) = stream else {
                return kept;
            };
            let mut buffer = [0u8; 4096];
            while let Ok(read) = stream.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                kept.push_str(&text);
                let _ = sender.send(ScriptEvent::Printed(text));
            }
            kept
        })
    };
    let stdout = reader(
        child
            .stdout
            .take()
            .map(|out| Box::new(out) as Box<dyn std::io::Read + Send>),
        sender.clone(),
    );
    let stderr = reader(
        child
            .stderr
            .take()
            .map(|err| Box::new(err) as Box<dyn std::io::Read + Send>),
        sender.clone(),
    );
    std::thread::spawn(move || {
        let watch = watch(request.timeout, pid);
        let status = child.wait();
        let printed = stdout.join().unwrap_or_default();
        let errors = stderr.join().unwrap_or_default();
        let output = status.map(|status| std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: errors.into_bytes(),
        });
        let _ = sender.send(ScriptEvent::Ended(ended(
            output, watch, &request, &folder, printed,
        )));
    });
    Ok((input, receiver))
}

/// The same run with what it prints kept rather than shown, and nothing to
/// answer a question: for a surface that owns the terminal and a test. A
/// failure carries the last line it said on its standard error.
pub fn run_quietly(python: &Path, request: &ScriptRequest) -> Result<Ran, (ScriptError, String)> {
    let (input, events) = start(python, request).map_err(|error| (error, String::new()))?;
    drop(input);
    loop {
        match events.recv() {
            Ok(ScriptEvent::Printed(_)) => {}
            Ok(ScriptEvent::Ended(ended)) => return ended,
            Err(_) => {
                return Err((
                    ScriptError::NoReply {
                        reason: "the script's thread ended without a word".to_string(),
                    },
                    String::new(),
                ));
            }
        }
    }
}

/// The interpreter started on the script's module, the request in a file
/// whose path it is given: its standard input stays the script's, for
/// `input()`. Beside a screen, its streams are piped and unbuffered.
fn spawn(
    python: &Path,
    request: &ScriptRequest,
    piped: bool,
) -> Result<(std::process::Child, tempfile::TempDir), (ScriptError, String)> {
    let not_started = |reason: String| {
        (
            ScriptError::NotStarted {
                python: python.to_path_buf(),
                reason,
            },
            String::new(),
        )
    };
    let folder = tempfile::tempdir().map_err(|error| not_started(error.to_string()))?;
    let reply = folder.path().join("reply.json");
    let asked = folder.path().join("request.json");
    std::fs::File::create(&asked)
        .and_then(|mut file| file.write_all(request_json(request, &reply).as_bytes()))
        .map_err(|error| not_started(error.to_string()))?;
    let stream = || {
        if piped {
            Stdio::piped()
        } else {
            Stdio::inherit()
        }
    };
    let mut command = Command::new(python);
    command
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env(REQUEST_VARIABLE, &asked)
        // Its warnings in the command line's colours, where its stderr is.
        .envs(crate::presentation::terminal_rendering::child_colours())
        .args(["-c", BOOTSTRAP])
        .stdin(stream())
        .stdout(stream())
        .stderr(stream());
    if piped {
        command.env("PYTHONUNBUFFERED", "1");
    }
    let child = command
        .spawn()
        .map_err(|error| not_started(error.to_string()))?;
    Ok((child, folder))
}

/// Where the script's module finds its request.
pub const REQUEST_VARIABLE: &str = "SAMPLEKIT_SCRIPT_REQUEST";

/// A script running past its limit stopped: one looping for ever held the
/// command, which no option could end. Whether it was, once it has ended.
fn watch(
    limit: Option<std::time::Duration>,
    pid: u32,
) -> (
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(limit) = limit {
        let (done, stopped) = (done.clone(), stopped.clone());
        std::thread::spawn(move || {
            let until = std::time::Instant::now() + limit;
            while std::time::Instant::now() < until {
                if done.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if !done.load(std::sync::atomic::Ordering::SeqCst) {
                stopped.store(true, std::sync::atomic::Ordering::SeqCst);
                stop(pid);
            }
        });
    }
    (done, stopped)
}

/// How a run ended: its exit, and its reply read.
fn ended(
    output: std::io::Result<std::process::Output>,
    (done, stopped): (
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ),
    request: &ScriptRequest,
    folder: &tempfile::TempDir,
    printed: String,
) -> Result<Ran, (ScriptError, String)> {
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let output = output.map_err(|error| {
        (
            ScriptError::NoReply {
                reason: error.to_string(),
            },
            String::new(),
        )
    })?;
    if stopped.load(std::sync::atomic::Ordering::SeqCst) {
        return Err((
            ScriptError::TimedOut {
                seconds: request
                    .timeout
                    .unwrap_or_default()
                    .as_secs_f64()
                    .to_string(),
            },
            String::new(),
        ));
    }
    let said = String::from_utf8_lossy(&output.stderr)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string();
    let failed = |error: ScriptError| (error, said.clone());
    match output.status.code() {
        Some(0) => {}
        Some(code) => {
            return Err(failed(ScriptError::Exited {
                code: u8::try_from(code).unwrap_or(2),
            }));
        }
        None => {
            return Err(failed(match signal_of(&output.status) {
                Some(2) | None => ScriptError::Exited { code: 130 },
                Some(signal) => ScriptError::Killed { signal },
            }));
        }
    }
    let text = std::fs::read_to_string(folder.path().join("reply.json")).map_err(|error| {
        failed(ScriptError::NoReply {
            reason: error.to_string(),
        })
    })?;
    let (held, answers) =
        reply_of(&text).map_err(|reason| failed(ScriptError::NoReply { reason }))?;
    Ok(Ran {
        printed,
        held,
        answers,
    })
}

/// The reply's held saves, and the questions the script asked.
pub fn reply_of(reply: &str) -> Result<(Vec<Held>, Vec<Answer>), String> {
    #[derive(Deserialize)]
    struct Reply {
        held: Vec<Written>,
        #[serde(default)]
        answers: Vec<Answer>,
    }
    #[derive(Deserialize)]
    struct Written {
        path: PathBuf,
        before: Option<String>,
        after: String,
    }
    let reply: Reply = serde_json::from_str(reply).map_err(|error| error.to_string())?;
    let held = reply
        .held
        .into_iter()
        .map(|written| Held {
            path: written.path,
            before: written.before.map(String::into_bytes),
            after: written.after.into_bytes(),
        })
        .collect();
    Ok((held, reply.answers))
}

/// Starts the script's module, saying so where the environment has no
/// SampleKit, rather than Python's exit 1, which reads as a refusal.
const BOOTSTRAP: &str = "import sys
try:
    from samplekit._script import main
except ModuleNotFoundError as error:
    print(f'error: this environment ({sys.executable}) has no {error.name}: a script is '
          'run by the samplekit installed in it', file=sys.stderr)
    sys.exit(3)
sys.exit(main())
";

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<i32> {
    None
}
