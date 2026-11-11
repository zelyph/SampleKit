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

/// How a PDF was shown at its page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageShown {
    /// In the web browser, at the page.
    AtPage,
    /// No browser found: the file opened as it is, at its start.
    Whole,
}

/// A PDF shown at `page`, in the system's web browser: a PDF reader opened
/// by the system's opener is given no page on any system, and every browser
/// reads a file's `#page=`. Where no browser is found, the file is opened as
/// it is, and said.
pub fn open_page(file: &Path, page: u32) -> Result<PageShown, OpenError> {
    if let Some(refused) = refusal(file) {
        return Err(refused);
    }
    let whole = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let url = file_url(&whole, Some(page));
    if let Some(line) = browser_line(&url)
        && let Some((program, arguments)) = line.split_first()
    {
        let mut command = Command::new(program);
        #[cfg(windows)]
        {
            // The registry's line as it writes it: its quotes are the
            // browser's to read.
            use std::os::windows::process::CommandExt;
            for argument in arguments {
                command.raw_arg(argument);
            }
        }
        #[cfg(not(windows))]
        command.args(arguments);
        if let Ok(mut child) = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(PageShown::AtPage);
        }
    }
    open(file).map(|()| PageShown::Whole)
}

/// `file:///…` for an absolute path, `#page=N` after it: every character
/// but the unreserved ones and `/` escaped, Windows's `C:\…` written
/// `/C:/…`.
pub fn file_url(path: &Path, page: Option<u32>) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = if text.starts_with('/') {
        text
    } else {
        format!("/{text}")
    };
    let mut url = String::from("file://");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    if let Some(page) = page {
        url.push_str(&format!("#page={page}"));
    }
    url
}

/// The command that shows `url` in the system's web browser, or `None`.
fn browser_line(url: &str) -> Option<Vec<String>> {
    // Who named a browser named it for this.
    if let Some(browser) = std::env::var("BROWSER")
        .ok()
        .and_then(|browsers| browsers.split(':').next().map(str::to_string))
        .filter(|browser| !browser.trim().is_empty())
    {
        return Some(vec![browser, url.to_string()]);
    }
    if cfg!(target_os = "macos") {
        return mac_browser()
            .map(|app| vec!["open".to_string(), "-a".to_string(), app, url.to_string()]);
    }
    if cfg!(windows) {
        return windows_browser().map(|command| windows_line(&command, url));
    }
    if under_wsl() {
        return None;
    }
    let desktop = linux_browser()?;
    let exec = desktop_exec(&desktop)?;
    exec_line(&exec, url)
}

/// The output of a command, trimmed, where it ran and said something.
fn said(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// Linux's default browser, as its desktop file: what `xdg-settings` says,
/// else the handler of `https` addresses.
fn linux_browser() -> Option<PathBuf> {
    let id = said("xdg-settings", &["get", "default-web-browser"])
        .or_else(|| said("xdg-mime", &["query", "default", "x-scheme-handler/https"]))?;
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    home.into_iter()
        .chain(dirs.split(':').map(PathBuf::from))
        .map(|dir| dir.join("applications").join(&id))
        .find(|file| file.is_file())
}

/// The `Exec=` line of a desktop file's own entry.
fn desktop_exec(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if inside && let Some(exec) = line.strip_prefix("Exec=") {
            return Some(exec.to_string());
        }
    }
    None
}

/// A desktop file's `Exec=` line given `url`: its `%u`, `%U`, `%f` or `%F`
/// made the address, its other field codes dropped, and the address put last
/// where it names none.
pub fn exec_line(exec: &str, url: &str) -> Option<Vec<String>> {
    let mut placed = false;
    let mut line = Vec::new();
    for word in shell_words::split(exec).ok()? {
        match word.as_str() {
            "%u" | "%U" | "%f" | "%F" => {
                if !placed {
                    line.push(url.to_string());
                    placed = true;
                }
            }
            "%i" | "%c" | "%k" => {}
            _ => line.push(word.replace("%%", "%")),
        }
    }
    if !placed {
        line.push(url.to_string());
    }
    (!line.is_empty()).then_some(line)
}

/// macOS's default browser, as the application that opens an `https`
/// address.
fn mac_browser() -> Option<String> {
    said(
        "osascript",
        &[
            "-l",
            "JavaScript",
            "-e",
            "ObjC.import('AppKit'); $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString('https://example.org')).path.js",
        ],
    )
}

/// Windows's default browser, as the command its registry gives to open an
/// `https` address.
fn windows_browser() -> Option<String> {
    let choice = said(
        "reg",
        &[
            "query",
            r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            "/v",
            "ProgId",
        ],
    )?;
    let id = choice.lines().find_map(|line| {
        line.trim()
            .strip_prefix("ProgId")?
            .split_whitespace()
            .last()
            .map(str::to_string)
    })?;
    let key = format!(r"HKCR\{id}\shell\open\command");
    let command = said("reg", &["query", &key, "/ve"])?;
    command
        .lines()
        .find_map(|line| {
            line.split_once("REG_SZ")
                .or_else(|| line.split_once("REG_EXPAND_SZ"))
        })
        .map(|(_, command)| command.trim().to_string())
}

/// The registry's command line given `url`: the program, as the line names
/// it, quoted or not, and the rest, its `%1` the address in quotes, or the
/// address added where it has none.
pub fn windows_line(command: &str, url: &str) -> Vec<String> {
    let command = command.trim();
    let (program, rest) = match command.strip_prefix('"') {
        Some(rest) => match rest.split_once('"') {
            Some((program, rest)) => (program.to_string(), rest.trim().to_string()),
            None => (rest.to_string(), String::new()),
        },
        None => match command.split_once(' ') {
            Some((program, rest)) => (program.to_string(), rest.trim().to_string()),
            None => (command.to_string(), String::new()),
        },
    };
    let quoted = format!("\"{url}\"");
    let rest = if rest.contains("\"%1\"") {
        rest.replace("\"%1\"", &quoted)
    } else if rest.contains("%1") {
        rest.replace("%1", &quoted)
    } else if rest.is_empty() {
        quoted
    } else {
        format!("{rest} {quoted}")
    };
    vec![program, rest]
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
