//! A file shown by the application the system gives its kind, the folder
//! holding it shown in the file manager, and a sample's own file in the editor
//! — for the command line, Python and the workbench alike.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Over SSH without a display: nothing opened could be seen.
    NoDisplay { path: PathBuf },
    /// The system's opener, or the editor, could not be started.
    NotStarted { program: String, reason: String },
    /// The editor ran and failed.
    Failed { program: String, code: Option<i32> },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoDisplay { path } => write!(
                f,
                "{}: this SSH session has no display to open it on — copy it, or open it \
                 where you sit",
                path.display()
            ),
            OpenError::NotStarted { program, reason } => {
                write!(f, "{program} could not be started: {reason}")
            }
            OpenError::Failed { program, code } => match code {
                Some(code) => write!(f, "{program} exited with code {code}"),
                None => write!(f, "{program} was killed"),
            },
        }
    }
}

impl std::error::Error for OpenError {}

/// How many files a pattern opens without asking. More opens only after a yes:
/// `'*'` over a sample's forty images is forty windows.
pub const OPENED_WITHOUT_ASKING: usize = 5;

/// The folders holding `files`, each once, in their order.
pub fn folders_of(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for file in files {
        let folder = folder_of(file);
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// Why nothing can be shown here, or `None`: over SSH without a display.
pub fn refusal(path: &Path) -> Option<OpenError> {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    (cfg!(target_os = "linux")
        && (set("SSH_CONNECTION") || set("SSH_TTY"))
        && !set("DISPLAY")
        && !set("WAYLAND_DISPLAY"))
    .then(|| OpenError::NoDisplay {
        path: path.to_path_buf(),
    })
}

/// Shows `path` with the application the system gives its kind, and returns at
/// once: the application is the user's, and its process is reaped in the
/// background. On Linux outside WSL, `xdg-open` is started here, a thread
/// waiting for it; `opener` drops the child it starts without waiting, and each
/// file opened from a long workbench session left a zombie behind. Where
/// `xdg-open` cannot be started, and on every other system, by `opener`: the
/// `xdg-open` it carries, `wslview` under WSL, `open`, `start`.
pub fn open(path: &Path) -> Result<(), OpenError> {
    if let Some(refused) = refusal(path) {
        return Err(refused);
    }
    if cfg!(target_os = "linux")
        && !under_wsl()
        && let Ok(mut child) = Command::new("xdg-open")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    }
    opener::open(path).map_err(|error| OpenError::NotStarted {
        program: "the system's opener".to_string(),
        reason: error.to_string(),
    })
}

/// Whether this Linux is WSL's, where `wslview` shows a file and `opener`
/// knows it.
fn under_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"))
}

/// The folder holding `path` — or `path`, a folder — in the file manager.
pub fn navigate(path: &Path) -> Result<PathBuf, OpenError> {
    let folder = folder_of(path);
    open(&folder)?;
    Ok(folder)
}

/// The folder `navigate` opens: `path`'s, or `path`, a folder.
pub fn folder_of(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }
}

/// `path` in the user's editor — `$VISUAL`, then `$EDITOR` — waited for, as
/// `git commit` waits; with neither, the system's application for it.
pub fn edit(path: &Path) -> Result<(), OpenError> {
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty());
    let Some(editor) = editor else {
        return open(path);
    };
    // `code --wait` and `emacsclient -t` are one variable with arguments.
    let mut words = editor.split_whitespace();
    let program = words.next().unwrap_or_default().to_string();
    let status = Command::new(&program)
        .args(words)
        .arg(path)
        .status()
        .map_err(|error| OpenError::NotStarted {
            program: program.clone(),
            reason: error.to_string(),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(OpenError::Failed {
            program,
            code: status.code(),
        })
    }
}
