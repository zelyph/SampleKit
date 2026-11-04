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
    // On Windows, the path whole and in its own separators: a link's
    // `samples\../reports/r.pdf` reached the shell as written.
    let whole;
    // A web address is no path: made whole, `https://…` became a file under
    // the current folder, which the shell could not open.
    let address = path.to_string_lossy().contains("://");
    let path = if cfg!(windows) && !address {
        whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        whole.as_path()
    } else {
        path
    };
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
    // Windows's file manager is told to show the file in its folder, as its
    // own *Show in folder* does; opening the folder by its association was
    // refused, *the system's opener could not be started*.
    if cfg!(windows) {
        if let Some(refused) = refusal(path) {
            return Err(refused);
        }
        explorer(path)?;
        return Ok(folder);
    }
    open(&folder)?;
    Ok(folder)
}

/// What Windows's `explorer.exe` is given to show `path`: a file selected in
/// its folder, `/select,"C:\…\a.png"`, or a folder, `"C:\…\photos"`, the path
/// whole and in Windows's separators. As one argument, written as it is: the
/// file manager reads `/select,` and its quotes itself.
pub fn explorer_line(path: &Path) -> String {
    let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = whole.display().to_string().replace('/', "\\");
    if path.is_dir() {
        format!("\"{text}\"")
    } else {
        format!("/select,\"{text}\"")
    }
}

#[cfg(windows)]
fn explorer(path: &Path) -> Result<(), OpenError> {
    use std::os::windows::process::CommandExt as _;
    // Its exit code says nothing: it is 1 where the window opened.
    Command::new("explorer.exe")
        .raw_arg(explorer_line(path))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| OpenError::NotStarted {
            program: "explorer.exe".to_string(),
            reason: error.to_string(),
        })
}

#[cfg(not(windows))]
fn explorer(path: &Path) -> Result<(), OpenError> {
    open(&folder_of(path))
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
/// `git commit` waits; with neither, the system's application for it, and on
/// Windows Notepad, waited for too: the application Windows gives a `.py` is
/// Python, which ran the model rather than showing it.
pub fn edit(path: &Path) -> Result<(), OpenError> {
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty());
    let editor = match editor {
        Some(editor) => editor,
        None if cfg!(windows) => "notepad.exe".to_string(),
        None => return open(path),
    };
    // `code --wait` and `emacsclient -t` are one variable with arguments,
    // read as a shell reads it: `"C:\Program Files\…\code.exe" --wait`, a
    // path holding a space, was cut in two.
    let split = editor_words(&editor);
    let mut words = split.iter().map(String::as_str);
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

/// The editor [`edit`] starts, named as a reader knows it, where it opens a
/// window of its own: Notepad, `code --wait`, the system's application. `None`
/// for one that runs in the terminal, `vim`, `nano`, `emacs -nw`, which the
/// terminal is given to while it runs.
pub fn windowed_editor() -> Option<String> {
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty());
    let Some(editor) = editor else {
        return Some(if cfg!(windows) {
            "Notepad".to_string()
        } else {
            "the file's application".to_string()
        });
    };
    let words = editor_words(&editor);
    let program = words.first()?;
    let name = Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    const IN_TERMINAL: &[&str] = &[
        "vi", "vim", "nvim", "view", "vis", "nano", "pico", "micro", "hx", "helix", "kak", "joe",
        "jed", "ne", "mg", "ed", "ex", "mcedit", "amp", "kibi", "tilde", "edit",
    ];
    let tty = |flags: &[&str]| words[1..].iter().any(|word| flags.contains(&word.as_str()));
    let in_terminal = IN_TERMINAL.contains(&name.as_str())
        || (name == "emacs" && tty(&["-nw", "--no-window-system", "-t", "--tty"]))
        || (name == "emacsclient" && tty(&["-nw", "-t", "--tty"]));
    if in_terminal {
        return None;
    }
    Some(match name.as_str() {
        "notepad" => "Notepad".to_string(),
        "code" => "VS Code".to_string(),
        _ => name,
    })
}

/// An editor's command as its words: as a shell reads it, and on Windows,
/// whose paths hold `\`, a first word in double quotes, the others between
/// spaces.
fn editor_words(editor: &str) -> Vec<String> {
    if cfg!(windows) {
        let trimmed = editor.trim();
        if let Some(rest) = trimmed.strip_prefix('"')
            && let Some((program, after)) = rest.split_once('"')
        {
            return std::iter::once(program.to_string())
                .chain(after.split_whitespace().map(str::to_string))
                .collect();
        }
        return trimmed.split_whitespace().map(str::to_string).collect();
    }
    shell_words::split(editor)
        .unwrap_or_else(|_| editor.split_whitespace().map(str::to_string).collect())
}
