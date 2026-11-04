//! A lock file: creating it is taking the lock — `create_new` fails where it
//! exists — and dropping it releases it. It says who holds it: the process,
//! its host and its user, so that a lock whose writer is gone is known for
//! one, and a lock held elsewhere — another account on a shared server,
//! another computer on a shared folder — is never taken for one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A lock held, released when dropped.
#[derive(Debug)]
pub struct Lock(PathBuf);

/// Who holds a lock, as its file says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: Option<u32>,
    pub host: Option<String>,
    pub user: Option<String>,
}

impl Holder {
    /// As a refusal says it: `process 4242 of tom on lab-server`.
    pub fn said(&self) -> String {
        let mut said = match self.pid {
            Some(pid) => format!("process {pid}"),
            None => "a process".to_string(),
        };
        if let Some(user) = &self.user {
            said.push_str(&format!(" of {user}"));
        }
        if let Some(host) = &self.host {
            said.push_str(&format!(" on {host}"));
        }
        said
    }

    /// Whether it is this host and this user's: only such a lock can be
    /// judged by its process. One naming neither — caught before it was
    /// written whole — is judged by its age, as one naming nobody.
    fn here(&self) -> bool {
        self.host.as_deref() == Some(own_host().as_str())
            && self.user.as_deref() == Some(own_user().as_str())
    }

    /// Whether it names where it is held: its host and its user.
    fn placed(&self) -> bool {
        self.host.is_some() && self.user.is_some()
    }
}

/// Why a lock was not taken.
#[derive(Debug)]
pub enum Refused {
    /// Another holds it; `None` where its file says nothing yet.
    Held(Option<Holder>),
    /// The folder refused the file.
    Io(io::Error),
}

/// How long a lock naming nobody — one caught between its creation and its
/// first byte — is taken as held.
const UNNAMED_LOCK_HELD: std::time::Duration = std::time::Duration::from_secs(60);

impl Lock {
    /// The lock at `at`, taken. One its holder can no longer release — this
    /// host's and user's, its process gone — is taken back once, `left`
    /// called with that process's id first, for what it left beside it.
    pub fn take(at: &Path, left: impl FnOnce(Option<u32>)) -> Result<Lock, Refused> {
        Lock::take_saying(at, "", left)
    }

    /// [`Lock::take`], `saying` written after its holder: what it is held for.
    pub fn take_saying(
        at: &Path,
        saying: &str,
        left: impl FnOnce(Option<u32>),
    ) -> Result<Lock, Refused> {
        let create = || fs::OpenOptions::new().write(true).create_new(true).open(at);
        let mut created = create();
        if matches!(&created, Err(error) if error.kind() == io::ErrorKind::AlreadyExists)
            && let Some(dead) = abandoned(at)
        {
            // Taken back by moving the very file judged abandoned aside: two
            // processes judging it at once, the second removed the lock the
            // first had just made, and both held it.
            let stale = fs::read(at).unwrap_or_default();
            let aside = at.with_extension(format!("stale-{}", std::process::id()));
            if fs::rename(at, &aside).is_ok() {
                if fs::read(&aside).unwrap_or_default() == stale {
                    left(dead);
                    let _ = fs::remove_file(&aside);
                    created = create();
                } else {
                    // Another's lock, made meanwhile, moved: put back where
                    // nothing took its place.
                    let _ = fs::hard_link(&aside, at);
                    let _ = fs::remove_file(&aside);
                }
            }
        }
        match created {
            Ok(mut file) => {
                use std::io::Write as _;
                // Best effort: a lock without its holder is still a lock.
                let _ = write!(
                    file,
                    "{}\nhost {}\nuser {}\n",
                    std::process::id(),
                    own_host(),
                    own_user()
                );
                if !saying.is_empty() {
                    let _ = writeln!(file, "for {saying}");
                }
                Ok(Lock(at.to_path_buf()))
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(Refused::Held(holder(at)))
            }
            Err(error) => Err(Refused::Io(error)),
        }
    }

    /// Where the lock is.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// What the lock at `at` is held for, as [`Lock::take_saying`] wrote it.
pub fn held_for(at: &Path) -> Option<String> {
    fs::read_to_string(at)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("for ").map(str::to_string))
}

/// Who holds the lock at `at`, as its file says it.
pub fn holder(at: &Path) -> Option<Holder> {
    let held = fs::read_to_string(at).ok()?;
    let mut lines = held.lines();
    let pid = lines
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok());
    let mut holder = Holder {
        pid,
        host: None,
        user: None,
    };
    for line in lines {
        if let Some(host) = line.strip_prefix("host ") {
            holder.host = Some(host.trim().to_string());
        } else if let Some(user) = line.strip_prefix("user ") {
            holder.user = Some(user.trim().to_string());
        }
    }
    (holder.pid.is_some() || holder.host.is_some()).then_some(holder)
}

/// Whether the lock at `at` was left by a writer that is gone: `Some` with its
/// process id where it names one of this host and user no longer running,
/// `Some(None)` for one naming nobody and older than a lock is held, `None`
/// for a lock held — or held elsewhere, which cannot be known gone.
pub fn abandoned(at: &Path) -> Option<Option<u32>> {
    match holder(at) {
        Some(holder) if holder.placed() => match holder.pid {
            Some(pid) if holder.here() => (!running(pid)).then_some(Some(pid)),
            _ => None,
        },
        _ => {
            let age = fs::metadata(at).ok()?.modified().ok()?.elapsed().ok()?;
            (age > UNNAMED_LOCK_HELD).then_some(None)
        }
    }
}

/// How long a journal another computer left beside a synchronised history
/// is taken for one under way, since it was last written: each file a write
/// finishes is noted in it, and none of a day is a write that stopped.
pub const JOURNAL_ELSEWHERE: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

/// Whether a write's journal at `at` was left by a writer that is gone: as a
/// lock is, or, left by another computer or account — which cannot be asked
/// whether it runs — unwritten for [`JOURNAL_ELSEWHERE`]. Held for ever, the
/// edits made here meanwhile were kept by no snapshot, then credited to it.
pub fn journal_abandoned(at: &Path) -> bool {
    if abandoned(at).is_some() {
        return true;
    }
    holder(at).is_some_and(|holder| !holder.here())
        && fs::metadata(at)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > JOURNAL_ELSEWHERE)
}

/// This computer's name, as a lock says it.
pub fn own_host() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .trim()
        .to_string()
}

/// This account's name, as a lock says it.
pub fn own_user() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Whether a process of this id runs, asked of the system's `kill -0`: the
/// standard library signals no process but its own children.
#[cfg(unix)]
pub fn running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    // Refused the signal, a process of another account runs all the same:
    // taken for gone, a root process's id in a lock let it be taken back.
    // Said in the C locale, which the refusal is read in.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .env("LC_ALL", "C")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        // Unable to ask, the lock is taken as held: refusing is the safe side.
        .map_or(true, |output| {
            output.status.success()
                || String::from_utf8_lossy(&output.stderr)
                    .to_lowercase()
                    .contains("not permitted")
        })
}

/// The same on Windows, asked of `tasklist`: always *running*, a lock or a
/// journal a command killed there left was never taken back.
#[cfg(windows)]
pub fn running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        // Unable to ask, the lock is taken as held: refusing is the safe side.
        .map_or(true, |output| {
            !output.status.success()
                || String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
        })
}

#[cfg(not(any(unix, windows)))]
pub fn running(_: u32) -> bool {
    true
}
