//! A file changed outside SampleKit, told by the system: inotify on Linux,
//! FSEvents on macOS, ReadDirectoryChangesW on Windows, kqueue on the BSDs.
//!
//! The terminal interface looked at every file of its project once a second
//! and after each key, to see whether one changed: on a project of thousands
//! of samples, a pause after each key. Told by the system, it looks only when
//! something changed, and once in a while all the same, should the system say
//! nothing where it should: a network drive, a folder no watch reaches. Where
//! no watch can be made, it looks as before.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use notify::Watcher;

/// How long the files go unlooked at, the system having said nothing.
pub const LOOKED_AT_LEAST_EVERY: Duration = Duration::from_secs(30);

pub struct Watch {
    /// Set by the system's word, taken by the next look.
    stirred: Arc<AtomicBool>,
    /// None where no watch could be made, or one failed: then every look is due.
    watcher: Option<notify::RecommendedWatcher>,
    watched: BTreeSet<PathBuf>,
    looked: Instant,
}

impl Watch {
    /// A watch over nothing yet; none where the system offers none.
    pub fn new() -> Watch {
        let stirred = Arc::new(AtomicBool::new(false));
        let told = Arc::clone(&stirred);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            // A file read is no change: reading the project, as each look
            // does, would otherwise stir the next look, and that the next.
            let reading = event
                .as_ref()
                .is_ok_and(|event| matches!(event.kind, notify::EventKind::Access(_)));
            if !reading {
                told.store(true, Ordering::Relaxed);
            }
        })
        .ok();
        Watch {
            stirred,
            watcher,
            watched: BTreeSet::new(),
            looked: Instant::now(),
        }
    }

    /// Whether a watch holds: the system tells of a change.
    pub fn told(&self) -> bool {
        self.watcher.is_some()
    }

    /// The folders watched made `folders`, each alone: a project's folders
    /// are few, where watching its tree whole reached into its `.venv` and
    /// its thousands of folders. A folder that cannot be watched gives the
    /// watch up, and the files are looked at as before.
    pub fn follow(&mut self, folders: BTreeSet<PathBuf>) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        for gone in self.watched.difference(&folders) {
            let _ = watcher.unwatch(gone);
        }
        for added in folders.difference(&self.watched) {
            if watcher
                .watch(added, notify::RecursiveMode::NonRecursive)
                .is_err()
            {
                self.watcher = None;
                self.watched.clear();
                return;
            }
        }
        self.watched = folders;
    }

    /// Whether the files are to be looked at now: the system told of a
    /// change, the last look is old, or there is no watch.
    pub fn due(&mut self) -> bool {
        if self.watcher.is_none() || self.looked.elapsed() >= LOOKED_AT_LEAST_EVERY {
            self.stirred.store(false, Ordering::Relaxed);
            self.looked = Instant::now();
            return true;
        }
        if self.stirred.swap(false, Ordering::Relaxed) {
            self.looked = Instant::now();
            return true;
        }
        false
    }
}

impl Default for Watch {
    fn default() -> Watch {
        Watch::new()
    }
}

/// The folders a project's changes happen in: its root and every folder
/// below it a sample may lie in, as the scan enters them, a hidden folder and
/// a virtual environment left out; and the folders of `more`, the files read
/// outside them.
pub fn folders_of(root: &Path, more: impl IntoIterator<Item = PathBuf>) -> BTreeSet<PathBuf> {
    let mut folders: BTreeSet<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !(entry.file_name().to_string_lossy().starts_with('.')
                    || entry.path().join("pyvenv.cfg").is_file())
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.into_path())
        .collect();
    folders.extend(
        more.into_iter()
            .filter_map(|file| Some(file.parent()?.to_path_buf()))
            .filter(|folder| folder.is_dir()),
    );
    folders
}
