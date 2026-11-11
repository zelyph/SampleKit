//! Which files in this directory are samples, and in what order. The entry
//! point of every collection operation.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::project_config::{self as config, CollectionRules, ProjectConfig};

// ------------------------------------------------------------------- types

#[derive(Debug, Clone)]
pub struct Collection {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub skipped: Vec<Skipped>,
    /// Which configuration each path was found under, when more than one was.
    pub configurations: Vec<(PathBuf, PathBuf)>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// Four causes with four different fixes, which is why `classify` answers with
/// one of these rather than with a boolean.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    /// Excluded by a pattern the author wrote — reported, because they may
    /// have written it too widely. A file matching no *include* pattern has no
    /// variant here at all: it is a file in the directory, not a candidate.
    Excluded {
        pattern: String,
    },
    NoFrontmatter,
    /// Not a regular file — a pipe, a device, a socket, a link to one — and
    /// so no sample, set aside unread: opened, a pipe stopped every command.
    NotAFile,
    /// A frontmatter that declares no `schema_version`: no sample, as a
    /// README is none — a sample says which shape it is.
    NoVersion,
    /// A copy a synchroniser left beside `original` when two machines changed
    /// it at once — Syncthing's `.sync-conflict-`, Dropbox's and Nextcloud's
    /// `(conflicted copy …)`, Nextcloud's `_conflict-`: never a sample
    /// anyone meant, and counted, it doubled one in every mean.
    SyncConflict {
        original: PathBuf,
    },
    Unreadable {
        message: String,
    },
    Malformed {
        message: String,
    },
    /// A `.samplekitrc` that will not read: set aside like an unreadable file,
    /// and the samples it describes read without it.
    Configuration {
        message: String,
    },
    /// A link to a folder the scan is in already — an ancestor, or one
    /// another link reached: not entered again, so that the scan ends and no
    /// sample is read twice.
    LinkBack,
}

/// **A sentence naming the fix.** Four causes with four different fixes is why
/// this type exists; a caller that has to write those four sentences itself
/// will write three of them.
impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipReason::Excluded { pattern } => {
                write!(f, "excluded by the pattern '{pattern}'")
            }
            SkipReason::NoFrontmatter => write!(
                f,
                "no frontmatter: a sample begins with a '---' line, so this is not one"
            ),
            SkipReason::NotAFile => write!(
                f,
                "not a regular file — a pipe, a device or a socket — so not a sample, and \
                 left unread"
            ),
            SkipReason::SyncConflict { original } => write!(
                f,
                "a copy left by a sync conflict of {}: compare the two, keep one, and remove \
                 the other",
                original.file_name().unwrap_or_default().to_string_lossy()
            ),
            SkipReason::NoVersion => write!(
                f,
                "no schema_version: a sample says which shape it is, 'schema_version: 1', so \
                 this is not one"
            ),
            SkipReason::Unreadable { message } => write!(f, "could not be read: {message}"),
            // A newer format is no malformed one: its version says it all.
            SkipReason::Malformed { message }
                if message.starts_with("schema_version") || message == NOT_TEXT =>
            {
                write!(f, "{message}")
            }
            SkipReason::Malformed { message } => write!(f, "malformed frontmatter: {message}"),
            SkipReason::Configuration { message } => {
                write!(f, "not read as a configuration: {message}")
            }
            SkipReason::LinkBack => write!(
                f,
                "a link back into what is scanned: its folder is read once, where it was \
                 met first"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Warning {
    /// A collection spanning several configurations holds samples described
    /// by different models. Not refused — comparing across projects is a real
    /// thing to want — but nobody should do it without being told.
    MixedConfigurations { roots: Vec<PathBuf> },
}

// ---------------------------------------------------------------- functions

/// Membership and order. Skipped files are **returned, not discarded**: a scan
/// that quietly omits three malformed files produces a query over 197 of 200
/// samples, with a result that is confidently wrong.
pub fn discover(root: &Path, config: Option<&ProjectConfig>) -> Result<Collection, DiscoveryError> {
    let rules = config
        .map(|config| config.collection().clone())
        .unwrap_or_default();
    // A pattern holding a `/` is the configuration's, written from its folder:
    // `exclude = ["sub/*.md"]` matched for `samplekit .` and not for
    // `samplekit sub`, which read it from `sub`.
    let offset = config
        .and_then(|config| below(config.root(), root))
        .unwrap_or_default();
    let own = config.and_then(|config| dunce::canonicalize(config.root()).ok());
    scan(root, &offset, &rules, Some(own))
}

/// The same scan under rules the caller states — a project's, with its include
/// patterns replaced as Python's `pattern=` asks — rather than the declared ones.
/// Patterns are relative to `root`.
/// Every sample of the project `config` is the configuration of: in every
/// folder below its own, whatever `recursive` says — a command reaches a
/// subfolder's samples by naming it, and they are the project's too — and
/// none of a project inside it, which keeps its own.
pub fn samples_of_project(config: &ProjectConfig) -> Result<Vec<PathBuf>, DiscoveryError> {
    let root = config.root();
    let rules = CollectionRules {
        recursive: true,
        ..config.collection().clone()
    };
    let own = dunce::canonicalize(root.join(config::FILENAME)).ok();
    let found = discover_with(root, &rules)?;
    // A sample that no longer reads — its YAML broken, a merge's conflict
    // markers left in it — is still the project's: kept as it is, it was
    // taken for deleted, and a restore would have replaced the edit unkept.
    let broken = found.skipped.into_iter().filter_map(|skipped| {
        let kept = match &skipped.reason {
            // Not one unread: a pipe or a device is no file to keep, and
            // reading it stopped the command.
            SkipReason::Malformed { .. } => true,
            SkipReason::NoFrontmatter => std::fs::read(&skipped.path).is_ok_and(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .any(|line| line.starts_with("<<<<<<<"))
            }),
            _ => false,
        };
        kept.then_some(skipped.path)
    });
    let mut paths: Vec<PathBuf> = found
        .paths
        .into_iter()
        .chain(broken)
        .filter(|path| {
            config::find(path).is_none_or(|found| dunce::canonicalize(found).ok() == own)
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// The files of the project's that do not read as samples now, and might
/// have: one whose `schema_version` was taken out, one the system would not
/// let be read — a lock on Windows, a permission. The history keeps one it
/// held before as it is, or as it last held it, where it was taken for
/// deleted.
pub fn unread_of_project(config: &ProjectConfig) -> Result<Vec<PathBuf>, DiscoveryError> {
    let root = config.root();
    let rules = CollectionRules {
        recursive: true,
        ..config.collection().clone()
    };
    let own = dunce::canonicalize(root.join(config::FILENAME)).ok();
    let found = discover_with(root, &rules)?;
    Ok(found
        .skipped
        .into_iter()
        .filter(|skipped| {
            matches!(
                skipped.reason,
                SkipReason::NoVersion | SkipReason::Unreadable { .. }
            )
        })
        .map(|skipped| skipped.path)
        .filter(|path| {
            config::find(path).is_none_or(|found| dunce::canonicalize(found).ok() == own)
        })
        .collect())
}

/// The Markdown files below `root` that `rules`' include patterns leave out —
/// where they are not the default, which takes every one: no candidates, so
/// listed nowhere, they are named by `list skipped`, which said nothing of a
/// pattern written too narrowly. Those the exclude patterns leave out are not
/// among them: they are skipped, and listed.
pub fn outside_include(root: &Path, rules: &CollectionRules) -> Vec<PathBuf> {
    let markdown = CollectionRules::default().include;
    if rules.include == markdown {
        return Vec::new();
    }
    let broad = CollectionRules {
        include: markdown,
        ..rules.clone()
    };
    let (Ok(all), Ok(found)) = (discover_with(root, &broad), discover_with(root, rules)) else {
        return Vec::new();
    };
    let known: std::collections::BTreeSet<&PathBuf> = found
        .paths
        .iter()
        .chain(found.skipped.iter().map(|skipped| &skipped.path))
        .collect();
    // What the exclude patterns leave out is skipped, under the include
    // patterns too: no part of what they leave out.
    all.paths
        .into_iter()
        .chain(
            all.skipped
                .into_iter()
                .filter(|skipped| !matches!(skipped.reason, SkipReason::Excluded { .. }))
                .map(|skipped| skipped.path),
        )
        .filter(|path| !known.contains(path))
        .collect()
}

pub fn discover_with(root: &Path, rules: &CollectionRules) -> Result<Collection, DiscoveryError> {
    scan(root, Path::new(""), rules, None)
}

/// Where `folder` lies below `base`, both resolved; `None` outside it.
fn below(base: &Path, folder: &Path) -> Option<PathBuf> {
    let base = dunce::canonicalize(base).ok()?;
    let folder = dunce::canonicalize(folder).ok()?;
    folder.strip_prefix(&base).ok().map(Path::to_path_buf)
}

/// The scan, a pattern holding a `/` matched against a path relative to
/// `root` with `offset` before it: where `root` lies below the folder the
/// patterns were written from. `own`, where given, is the folder of the
/// project the rules are — `None` inside it for no project: a file of
/// another project inside the scan is judged by that project's rules, from
/// its folder, so that a project's samples are the same whichever folder a
/// command names.
fn scan(
    root: &Path,
    offset: &Path,
    rules: &CollectionRules,
    own: Option<Option<PathBuf>>,
) -> Result<Collection, DiscoveryError> {
    if !root.exists() {
        return Err(DiscoveryError::NotFound {
            path: root.to_path_buf(),
        });
    }
    // Passing a single file yields a collection of one, without a separate
    // code path for every command that accepts a directory.
    if root.is_file() {
        let mut collection = Collection {
            root: root.parent().unwrap_or(Path::new(".")).to_path_buf(),
            paths: Vec::new(),
            skipped: Vec::new(),
            configurations: Vec::new(),
            warnings: Vec::new(),
        };
        admit(root, &mut collection);
        return Ok(collection);
    }
    if !root.is_dir() {
        return Err(DiscoveryError::NotADirectory {
            path: root.to_path_buf(),
        });
    }

    let mut collection = Collection {
        root: root.to_path_buf(),
        paths: Vec::new(),
        skipped: Vec::new(),
        configurations: Vec::new(),
        warnings: Vec::new(),
    };
    let mut candidates = Vec::new();
    walk(root, offset, rules, own, &mut candidates, &mut collection)?;
    // What waits to be written counts: a sample made is found before its
    // file is, one removed is no longer.
    if crate::format::stage::staging(root) {
        candidates.retain(|candidate| crate::format::stage::exists(candidate));
        for made in crate::format::stage::made_under(root) {
            if made.extension().is_some_and(|extension| extension == "md")
                && matches_below(&made, root, offset, rules)
                && !candidates.contains(&made)
            {
                candidates.push(made);
            }
        }
    }
    // Explicit sort, never filesystem order: that varies by platform, by
    // filesystem and by the history of a directory, and a collection reporting
    // its samples in a different order on two machines makes an export
    // irreproducible.
    candidates.sort();
    // Each candidate classified on every core at once, kept in their order.
    let verdicts =
        crate::format::document::in_parallel(&candidates, |candidate| classify(candidate));
    for (candidate, verdict) in candidates.into_iter().zip(verdicts) {
        match verdict {
            Ok(()) => collection.paths.push(candidate),
            Err(reason) => collection.skipped.push(Skipped {
                path: candidate,
                reason,
            }),
        }
    }
    note_configurations(root, &mut collection);
    Ok(collection)
}

/// A file whose bytes are no UTF-8 text: what it holds, not what the
/// filesystem refused, and no frontmatter to call malformed.
const NOT_TEXT: &str = "not UTF-8 text, which a sample is written in: saved as UTF-16 by an editor, or not text at all";

/// The file a synchroniser's conflict copy copies, by its name, where `path`
/// is one: `piece.sync-conflict-20260929-101500-ABCDEF7.md`,
/// `piece (conflicted copy 2026-09-29).md`, `piece (Tom's conflicted copy
/// 2026-09-29).md`, `piece_conflict-20260929-101500.md`. OneDrive's copies
/// carry only the computer's name, and cannot be told from a sample's own.
pub fn conflict_copy_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_string(), format!(".{extension}")),
        None => (name.clone(), String::new()),
    };
    let original = if let Some(at) = stem.find(".sync-conflict-") {
        stem[..at].to_string()
    } else if let Some(at) = stem.find(" (")
        && stem.ends_with(')')
        && stem[at..].contains("conflicted copy")
    {
        stem[..at].to_string()
    } else if let Some(at) = stem.rfind("_conflict-")
        && stem[at + "_conflict-".len()..]
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-')
    {
        stem[..at].to_string()
    } else {
        return None;
    };
    (!original.is_empty()).then(|| path.with_file_name(format!("{original}{extension}")))
}

/// Is this a sample, and if not, why not. The check is exactly as deep as the
/// question: frontmatter, and a version this build reads. Nothing below.
pub fn classify(path: &Path) -> Result<(), SkipReason> {
    if let Some(original) = conflict_copy_of(path) {
        return Err(SkipReason::SyncConflict { original });
    }
    // Only a regular file is read, a link followed: a FIFO named `.md` stopped
    // every command, and a link to /dev/zero read until memory ran out.
    if std::fs::metadata(path).is_ok_and(|meta| !meta.is_file()) {
        return Err(SkipReason::NotAFile);
    }
    let text = crate::format::document::read_text(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidData {
            SkipReason::Malformed {
                message: NOT_TEXT.to_string(),
            }
        } else {
            SkipReason::Unreadable {
                message: error.to_string(),
            }
        }
    })?;
    let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
    // An empty sample file was emptied by mistake more often than written so:
    // set aside and said, not skipped in silence.
    if body.trim().is_empty() {
        return Err(SkipReason::Malformed {
            message: "the file is empty".to_string(),
        });
    }
    let mut lines = body.lines();
    match lines.next() {
        Some(first) if first.trim_end_matches('\r') == "---" => {}
        _ => return Err(SkipReason::NoFrontmatter),
    }
    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end_matches('\r') == "---" {
            closed = true;
            break;
        }
        frontmatter.push_str(&line.replace("\r\n", "\n"));
        frontmatter.push('\n');
    }
    if !closed {
        return Err(SkipReason::Malformed {
            message: "the frontmatter is never closed".to_string(),
        });
    }
    verdict_of(frontmatter)
}

/// The verdicts on the frontmatters read already, by their text: a terminal
/// interface reads its project again after each write, and four thousand
/// files classified again were a pause after each. Kept by the text, never by
/// a file's time, which a USB key keeps to two seconds.
type Verdicts = std::sync::Mutex<std::collections::HashMap<String, Result<(), SkipReason>>>;

/// Verdicts kept before the store is emptied: the frontmatters of several
/// projects of thousands of samples.
const VERDICTS_KEPT: usize = 50_000;

fn verdict_of(frontmatter: String) -> Result<(), SkipReason> {
    static VERDICTS: std::sync::OnceLock<Verdicts> = std::sync::OnceLock::new();
    let verdicts = VERDICTS.get_or_init(Default::default);
    if let Some(verdict) = verdicts
        .lock()
        .ok()
        .and_then(|held| held.get(&frontmatter).cloned())
    {
        return verdict;
    }
    let verdict = judge(&frontmatter);
    if let Ok(mut held) = verdicts.lock() {
        if held.len() >= VERDICTS_KEPT {
            held.clear();
        }
        held.insert(frontmatter, verdict.clone());
    }
    verdict
}

/// A closed frontmatter's verdict: its version, read alone.
fn judge(frontmatter: &str) -> Result<(), SkipReason> {
    // Only the version is read. A sample with an unreadable property or a table
    // that violates its own declaration **is discovered**, and fails where it
    // is used — dropping it here would remove a real sample from a query for a
    // reason the query never mentions.
    #[derive(serde::Deserialize)]
    struct JustTheVersion {
        schema_version: Option<u32>,
    }
    // Refused before the parser reads it, which took a time growing as its
    // square over brackets nested deep.
    if crate::format::document::nested_too_deep(frontmatter).is_some() {
        return Err(SkipReason::Malformed {
            message: format!(
                "brackets or braces nest deeper than {}, as no sample's fields do",
                crate::format::document::MAX_NESTING
            ),
        });
    }
    // A link written unquoted, said where it is: YAML stopped at it as a
    // key it did not find.
    if let Some((line, said)) = crate::format::document::unquoted_link_said(frontmatter) {
        return Err(SkipReason::Malformed {
            message: format!("line {line}: {said}"),
        });
    }
    let parsed: JustTheVersion =
        serde_yaml_ng::from_str(frontmatter).map_err(|error| SkipReason::Malformed {
            message: crate::format::document::frontmatter_refused(frontmatter, &error.to_string()),
        })?;
    match parsed.schema_version {
        // No sample, as a README with a frontmatter is none.
        None => Err(SkipReason::NoVersion),
        Some(version) if version != crate::format::schema::version() => {
            Err(SkipReason::Malformed {
                message: format!(
                    "schema_version {version}, and this build reads schema_version {}",
                    crate::format::schema::version()
                ),
            })
        }
        Some(_) => Ok(()),
    }
}

/// Whether a path is included by the rules, before it is opened. Exclusion is
/// applied after inclusion and always wins.
pub fn matches(path: &Path, root: &Path, rules: &CollectionRules) -> bool {
    matches_below(path, root, Path::new(""), rules)
}

/// [`matches`], `offset` before the path relative to `root`.
fn matches_below(path: &Path, root: &Path, offset: &Path, rules: &CollectionRules) -> bool {
    rules
        .include
        .iter()
        .any(|pattern| matched(pattern, path, root, offset))
        && !rules
            .exclude
            .iter()
            .any(|pattern| matched(pattern, path, root, offset))
}

/// A pattern holding a `/` against the path from where the patterns were
/// written, `/` between its parts on every system; another against the file's
/// name.
fn matched(pattern: &str, path: &Path, root: &Path, offset: &Path) -> bool {
    if pattern.contains('/') {
        let relative = offset.join(path.strip_prefix(root).unwrap_or(path));
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        glob(pattern, &parts.join("/"))
    } else {
        glob(
            pattern,
            &path.file_name().unwrap_or_default().to_string_lossy(),
        )
    }
}

/// A sample's own files — an image, a measurement report — found by its name in
/// the directories `[collection] files` declares, and nothing written in the
/// sample. A file is the sample's when its name begins with the sample file's,
/// and what follows is no letter or digit: `B2` does not take
/// `B25`'s. Where several samples' names begin one file's, the longest
/// is its owner — `B24-2` over `B24`. `pattern`, a glob on the
/// file's name, narrows them. In the order of their paths.
pub fn sample_files(
    config: &ProjectConfig,
    sample: &Path,
    pattern: Option<&str>,
) -> Result<Vec<PathBuf>, DiscoveryError> {
    let mut samples = discover(config.root(), Some(config))?.paths;
    if !samples.iter().any(|path| stem_of(path) == stem_of(sample)) {
        samples.push(sample.to_path_buf());
    }
    let owned = files_by_sample(config, &samples)?;
    let own = stem_of(sample);
    Ok(owned
        .owned
        .into_iter()
        .filter(|(owner, _)| stem_of(owner) == own)
        .flat_map(|(_, files)| files)
        .filter(|file| {
            pattern.is_none_or(|pattern| {
                file.file_name()
                    .is_some_and(|name| glob(pattern, &name.to_string_lossy()))
            })
        })
        .collect())
}

/// Every file under `[collection] files`, given to the sample that owns it, and
/// those no sample owns — what `list files` shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SampleFiles {
    /// Each sample with its files, in the samples' order; a sample with none
    /// is left out.
    pub owned: Vec<(PathBuf, Vec<PathBuf>)>,
    /// Files whose name begins with no sample's.
    pub unowned: Vec<PathBuf>,
}

pub fn files_by_sample(
    config: &ProjectConfig,
    samples: &[PathBuf],
) -> Result<SampleFiles, DiscoveryError> {
    let stems: Vec<(String, &PathBuf)> = samples
        .iter()
        .filter_map(|path| stem_of(path).map(|stem| (stem, path)))
        .collect();
    let mut owned: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    let mut unowned = Vec::new();
    // By its name, or by a folder named after the sample it lies in below where
    // it was looked for: `images/dry-nickel/a.jpg` is dry-nickel's. The longest
    // name wins.
    let by_name = |file: &Path, below: &Path| -> Option<PathBuf> {
        let name = file.file_name()?.to_string_lossy().into_owned();
        let folders: Vec<String> = file
            .strip_prefix(below)
            .ok()
            .and_then(Path::parent)
            .map(|parent| {
                parent
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        stems
            .iter()
            .filter(|(stem, _)| {
                begins_with_name(&name, stem) || folders.iter().any(|folder| folder == stem)
            })
            .max_by_key(|(stem, _)| stem.len())
            .map(|(_, path)| (*path).clone())
    };
    for written in &config.collection().files {
        let entry = files_entry(config, written);
        let below = match &entry {
            FilesEntry::Folder(folder) => folder.clone(),
            FilesEntry::Pattern { base, .. } => base.clone(),
        };
        if !below.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        every_file(&below, &mut files)?;
        files.sort();
        // A pattern naming the sample is one glob per sample, matched at once;
        // one without the name selects, and the name gives.
        let per_sample = match &entry {
            FilesEntry::Pattern {
                rest, named: true, ..
            } => Some(named_globs(rest, &stems)),
            _ => None,
        };
        // A pattern without the name selects, and the name gives; one with
        // it selects by its shape, any name standing in it, what is then
        // nobody's where no sample's name fits.
        let selecting = match &entry {
            FilesEntry::Pattern { rest, .. } => compiled(&rest.replace(PLACEHOLDER, "*")),
            FilesEntry::Folder(_) => None,
        };
        for file in files {
            let relative = relative_parts(&file, &below);
            let owner = match (&entry, &per_sample) {
                (FilesEntry::Folder(_), _) => by_name(&file, &below),
                (FilesEntry::Pattern { named: true, .. }, Some(set)) => {
                    let matched = set.matches(&relative);
                    if matched.is_empty() {
                        // Not the pattern's: neither a sample's nor nobody's.
                        if !selecting
                            .as_ref()
                            .is_some_and(|glob| glob.is_match(&relative))
                        {
                            continue;
                        }
                        None
                    } else {
                        matched
                            .into_iter()
                            .map(|at| &stems[at])
                            .max_by_key(|(stem, _)| stem.len())
                            .map(|(_, path)| (*path).clone())
                    }
                }
                _ => {
                    if !selecting
                        .as_ref()
                        .is_some_and(|glob| glob.is_match(&relative))
                    {
                        continue;
                    }
                    by_name(&file, &below)
                }
            };
            match owner {
                Some(owner) => match owned.iter_mut().find(|(held, _)| *held == owner) {
                    Some((_, held)) => held.push(file),
                    None => owned.push((owner, vec![file])),
                },
                None => unowned.push(file),
            }
        }
    }
    owned.sort_by_key(|(sample, _)| samples.iter().position(|path| path == sample));
    for (_, files) in &mut owned {
        files.sort();
        files.dedup();
    }
    // Two entries may reach one file: a sample's by one of them is nobody
    // else's, and nobody's is said once.
    unowned.sort();
    unowned.dedup();
    unowned.retain(|file| !owned.iter().any(|(_, files)| files.contains(file)));
    Ok(SampleFiles { owned, unowned })
}

/// A `[collection] files` entry, read: a folder searched by the sample's name,
/// or a glob, naming the sample with `{name}` or not.
#[derive(Debug, Clone, PartialEq)]
enum FilesEntry {
    Folder(PathBuf),
    Pattern {
        /// The folders the pattern names before its first wildcard, where
        /// it is looked for.
        base: PathBuf,
        /// The rest of it, matched against the path below `base`.
        rest: String,
        named: bool,
    },
}

const PLACEHOLDER: &str = "{name}";

/// An entry that is an existing folder is a folder, whatever it holds: a
/// configuration written before patterns means what it meant.
fn files_entry(config: &ProjectConfig, written: &str) -> FilesEntry {
    let whole = config.root().join(written);
    let wildcard = |part: &str| part.contains(['*', '?', '[', '{']);
    if whole.is_dir() || !wildcard(written) {
        return FilesEntry::Folder(whole);
    }
    let parts: Vec<&str> = written.split('/').collect();
    let first = parts
        .iter()
        .position(|part| wildcard(part))
        .unwrap_or(parts.len());
    let mut base = config.root().to_path_buf();
    for part in &parts[..first] {
        base.push(part);
    }
    let rest = parts[first..].join("/");
    FilesEntry::Pattern {
        base,
        named: rest.contains(PLACEHOLDER),
        rest,
    }
}

/// A path below `base`, its parts joined by `/` on every system, as a
/// pattern is written.
fn relative_parts(file: &Path, base: &Path) -> String {
    file.strip_prefix(base)
        .unwrap_or(file)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A glob as the collection's patterns read one: `*` within one folder, `**`
/// across them.
fn compiled(pattern: &str) -> Option<globset::GlobMatcher> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .ok()
        .map(|glob| glob.compile_matcher())
}

/// One glob per sample, `{name}` written as the name stands — a name holding
/// `[` names itself — in the samples' order.
fn named_globs(rest: &str, stems: &[(String, &PathBuf)]) -> globset::GlobSet {
    let mut set = globset::GlobSetBuilder::new();
    for (stem, _) in stems {
        let written = rest.replace(PLACEHOLDER, &globset::escape(stem));
        let glob = globset::GlobBuilder::new(&written)
            .literal_separator(true)
            .build()
            // What does not read matches nothing; `validate` says why.
            .unwrap_or_else(|_| globset::Glob::new("\u{0}").expect("a literal is a glob"));
        set.add(glob);
    }
    set.build().unwrap_or_else(|_| globset::GlobSet::empty())
}

/// What is wrong with a `[collection] files` entry, for `validate` : a defect
/// where it cannot find what it means to, and a note where the folder it names
/// is not there yet.
pub fn files_entry_problems(config: &ProjectConfig) -> Vec<FilesProblem> {
    let mut problems = Vec::new();
    // `include` and `exclude` are read as globs by discovery, which set aside
    // one that does not read without a word: every file then matched nothing.
    let rules = config.collection();
    for (key, patterns) in [("include", &rules.include), ("exclude", &rules.exclude)] {
        for pattern in patterns {
            if let Err(error) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                problems.push(FilesProblem {
                    entry: format!("{key} '{pattern}'"),
                    defect: true,
                    reason: format!("the pattern does not read: {}", error.kind()),
                });
            } else if key == "include" && pattern.contains('/') && !rules.recursive {
                problems.push(FilesProblem {
                    entry: format!("{key} '{pattern}'"),
                    defect: false,
                    reason: "it names a folder, and a collection that is not recursive reads \
                             none: recursive = true reads the folders below"
                        .to_string(),
                });
            }
        }
    }
    for written in &config.collection().files {
        let entry = files_entry(config, written);
        let (folder, rest) = match &entry {
            FilesEntry::Folder(folder) => (folder, None),
            FilesEntry::Pattern { base, rest, .. } => (base, Some(rest)),
        };
        if let Some(rest) = rest {
            // A brace holding one word is read by a glob as that word:
            // `{nmae}` finds `nmae`, never a sample's name.
            for word in single_braces(rest) {
                if word != "name" {
                    problems.push(FilesProblem {
                        entry: written.clone(),
                        defect: true,
                        reason: format!(
                            "{{{word}}} is no placeholder, and a glob reads it as the word \
                             {word}: only {{name}} stands for the sample's name"
                        ),
                    });
                }
            }
            let probe = rest.replace(PLACEHOLDER, "name");
            if let Err(error) = globset::GlobBuilder::new(&probe)
                .literal_separator(true)
                .build()
            {
                problems.push(FilesProblem {
                    entry: written.clone(),
                    defect: true,
                    reason: format!("the pattern does not read: {}", error.kind()),
                });
            }
        }
        if !folder.is_dir() {
            problems.push(FilesProblem {
                entry: written.clone(),
                defect: false,
                reason: format!(
                    "{} does not exist: no file is found there",
                    folder.display()
                ),
            });
        }
    }
    problems
}

/// One problem of a `[collection] files` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct FilesProblem {
    pub entry: String,
    /// A defect, or a note: a folder not made yet is no mistake.
    pub defect: bool,
    pub reason: String,
}

/// The words held alone between braces: `{name}` and `{nmae}`, never
/// `{jpg,png}`.
fn single_braces(pattern: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let inside = &after[..close];
        if !inside.is_empty()
            && !inside.contains([',', '{', '*', '?', '['])
            && !found.iter().any(|held| held == inside)
        {
            found.push(inside.to_string());
        }
        rest = &after[close + 1..];
    }
    found
}

fn stem_of(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
}

/// `name` begins with `stem`, and what follows is no letter or digit.
fn begins_with_name(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

/// Every file under `at`, hidden directories set aside.
fn every_file(at: &Path, into: &mut Vec<PathBuf>) -> Result<(), DiscoveryError> {
    let entries = walkdir::WalkDir::new(at)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            !(entry.file_type().is_dir() && entry.file_name().to_string_lossy().starts_with('.'))
        });
    // `at` unreadable is said; below it, what cannot be read is passed over,
    // as a file no sample owns would be.
    std::fs::read_dir(at).map_err(|source| DiscoveryError::Io {
        path: at.to_path_buf(),
        source,
    })?;
    for entry in entries.flatten() {
        if entry.path().is_file() {
            into.push(entry.path().to_path_buf());
        }
    }
    Ok(())
}

fn admit(path: &Path, collection: &mut Collection) {
    match classify(path) {
        Ok(()) => collection.paths.push(path.to_path_buf()),
        Err(reason) => collection.skipped.push(Skipped {
            path: path.to_path_buf(),
            reason,
        }),
    }
}

fn walk(
    at: &Path,
    offset: &Path,
    rules: &CollectionRules,
    own: Option<Option<PathBuf>>,
    candidates: &mut Vec<PathBuf>,
    collection: &mut Collection,
) -> Result<(), DiscoveryError> {
    let scanned = dunce::canonicalize(at).unwrap_or_else(|_| at.to_path_buf());
    // The projects inside the scan, each with its rules: loaded once.
    let mut inner: Vec<(PathBuf, CollectionRules)> = Vec::new();
    // The configuration each folder's files answer to.
    let mut found_in: std::collections::HashMap<PathBuf, Option<PathBuf>> =
        std::collections::HashMap::new();
    // By `walkdir`. **A folder linked into the project is the project's**:
    // data on another disk, linked in, is read, computed and written under
    // the path the link gives it. Each folder is entered once, by what it
    // resolves to: a link back to an ancestor, or a second link to a folder
    // read already, is passed over and said, so that the scan ends and no
    // sample is read twice.
    let seen = std::cell::RefCell::new(std::collections::HashSet::from([scanned.clone()]));
    let looped = std::cell::RefCell::new(Vec::new());
    let entries = walkdir::WalkDir::new(at)
        .min_depth(1)
        .max_depth(if rules.recursive { usize::MAX } else { 1 })
        .follow_links(rules.recursive)
        .into_iter()
        // A hidden directory and a virtual environment hold no samples anyone
        // wrote there: `.git`, and `.venv` with its thousands of files.
        .filter_entry(|entry| {
            if entry.file_type().is_dir()
                && (entry.file_name().to_string_lossy().starts_with('.')
                    || entry.path().join("pyvenv.cfg").is_file())
            {
                return false;
            }
            if entry.file_type().is_dir() {
                let resolved = dunce::canonicalize(entry.path())
                    .unwrap_or_else(|_| entry.path().to_path_buf());
                if !seen.borrow_mut().insert(resolved) {
                    looped.borrow_mut().push(entry.path().to_path_buf());
                    return false;
                }
            }
            true
        });
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // A loop the folders above did not catch, said as one.
            Err(error) if error.loop_ancestor().is_some() => {
                if let Some(path) = error.path() {
                    looped.borrow_mut().push(path.to_path_buf());
                }
                continue;
            }
            // A link that does not resolve — a loop of two files, a target
            // gone — is a file of the folder that cannot be read: set aside
            // and said, as a scan of the folder alone says it, where it
            // stopped the whole command.
            Err(error)
                if error.path().is_some_and(|path| {
                    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_symlink())
                        && path.extension().is_some_and(|extension| {
                            matches!(extension.to_str(), Some("md" | "MD" | "markdown"))
                        })
                }) =>
            {
                if let Some(path) = error.path() {
                    candidates.push(path.to_path_buf());
                }
                continue;
            }
            Err(error) => return Err(walk_error(at)(error)),
        };
        let path = entry.path().to_path_buf();
        let kind = entry.file_type();
        if kind.is_dir() || (kind.is_symlink() && path.is_dir()) {
            continue;
        }
        // The project the file is a sample of, where it is another than the
        // rules' own and lies inside the scan.
        let project = own.as_ref().and_then(|own| {
            // Searched once per folder: upward from each file, the search
            // was the same for every file of a folder. A file that is a link
            // is searched for itself, from wherever it leads.
            let found = match path.parent().filter(|_| !entry.path_is_symlink()) {
                Some(folder) => found_in
                    .entry(folder.to_path_buf())
                    .or_insert_with(|| config::find(&path))
                    .clone(),
                None => config::find(&path),
            };
            let folder = found?.parent()?.to_path_buf();
            (Some(&folder) != own.as_ref() && folder.starts_with(&scanned) && folder != scanned)
                .then_some(folder)
        });
        let (base, offset, rules) = match project {
            Some(folder) => {
                let at = match inner.iter().position(|(held, _)| *held == folder) {
                    Some(at) => at,
                    None => {
                        // A configuration that does not load is set aside, as
                        // every command sets it aside: its samples are read
                        // under the defaults.
                        let rules = config::load(&folder.join(config::FILENAME))
                            .map(|config| config.collection().clone())
                            .unwrap_or_default();
                        inner.push((folder, rules));
                        inner.len() - 1
                    }
                };
                let (folder, rules) = &inner[at];
                // Its own `recursive` says whether its subfolders hold samples.
                let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !rules.recursive && canonical.parent() != Some(folder.as_path()) {
                    continue;
                }
                (folder.clone(), PathBuf::new(), rules.clone())
            }
            None => (at.to_path_buf(), offset.to_path_buf(), rules.clone()),
        };
        // Matched from the project's folder, which is resolved: so is the
        // path matched, and the path kept is the one found.
        let probe = if base == at {
            path.clone()
        } else {
            dunce::canonicalize(&path).unwrap_or_else(|_| path.clone())
        };
        if !matches_below(&probe, &base, &offset, &rules) {
            // Only a candidate is excluded: `exclude = ["*"]` listed every
            // `.samplekitrc` as a sample left out.
            let candidate = rules
                .include
                .iter()
                .any(|pattern| matched(pattern, &probe, &base, &offset));
            let excluded = rules
                .exclude
                .iter()
                .filter(|_| candidate)
                .find(|pattern| matched(pattern, &probe, &base, &offset));
            // An exclusion names the pattern that did it. A file matching no
            // include pattern is not reported at all — it was never a
            // candidate, and listing every figure beside the samples would
            // bury the three skips that matter.
            if let Some(pattern) = excluded {
                collection.skipped.push(Skipped {
                    path: path.clone(),
                    reason: SkipReason::Excluded {
                        pattern: pattern.clone(),
                    },
                });
            }
            continue;
        }
        candidates.push(path);
    }
    for path in looped.into_inner() {
        collection.skipped.push(Skipped {
            path,
            reason: SkipReason::LinkBack,
        });
    }
    Ok(())
}

/// A walk's failure as the scan says it: the path it was reading, and why.
fn walk_error(at: &Path) -> impl Fn(walkdir::Error) -> DiscoveryError + '_ {
    move |error| DiscoveryError::Io {
        path: error.path().unwrap_or(at).to_path_buf(),
        source: error
            .into_io_error()
            .unwrap_or_else(|| std::io::Error::other("a directory loop")),
    }
}

/// Which configuration each sample was found under, and a warning when more
/// than one is involved.
fn note_configurations(root: &Path, collection: &mut Collection) {
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in &collection.paths {
        let found = config::find(path).unwrap_or_else(|| root.join(config::FILENAME));
        if !roots.contains(&found) {
            roots.push(found.clone());
        }
        collection.configurations.push((path.clone(), found));
    }
    if roots.len() > 1 {
        collection
            .warnings
            .push(Warning::MixedConfigurations { roots });
    } else {
        // One configuration warns about nothing, and the per-path record is
        // noise when there is only one thing it could say.
        collection.configurations.clear();
    }
}

/// Whether `text` matches the glob `pattern`, by `globset`: `*` and `?` within
/// one path segment, `**` across them, `[abc]` and `{a,b}`. A pattern that does
/// not read as a glob is matched as the text it is. Each pattern is compiled
/// once.
fn glob(pattern: &str, text: &str) -> bool {
    thread_local! {
        static COMPILED: std::cell::RefCell<std::collections::HashMap<String, globset::GlobMatcher>> =
            std::cell::RefCell::default();
    }
    COMPILED.with(|compiled| {
        compiled
            .borrow_mut()
            .entry(pattern.to_string())
            .or_insert_with(|| {
                let build = |written: &str| {
                    globset::GlobBuilder::new(written)
                        .literal_separator(true)
                        .build()
                };
                build(pattern)
                    .or_else(|_| build(&globset::escape(pattern)))
                    .map(|glob| glob.compile_matcher())
                    .unwrap_or_else(|_| globset::Glob::new("").unwrap().compile_matcher())
            })
            .is_match(text)
    })
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum DiscoveryError {
    NotFound { path: PathBuf },
    NotADirectory { path: PathBuf },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // A missing root is a mistake in the command and stops everything;
            // a single unreadable file is a fact about the collection and is
            // reported alongside the results.
            DiscoveryError::NotFound { path } => {
                write!(f, "{} does not exist", path.display())
            }
            DiscoveryError::NotADirectory { path } => write!(
                f,
                "{} is neither a directory nor a sample file",
                path.display()
            ),
            DiscoveryError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Where an attribute's link leads, from the sample file holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// A sample file: a wikilink's, or a Markdown link's `.md` that reads as
    /// one.
    Sample(PathBuf),
    /// Any other file, or a folder: opened with the system's application.
    File(PathBuf),
    /// A PDF at a page, `#page=12`: opened where it can open there.
    Page { file: PathBuf, page: u32 },
    /// A web page.
    Address(String),
    /// Nothing: no file at that path, or no sample answering to the name —
    /// the path or the name, as the link writes it.
    Nowhere(String),
    /// Several samples answering to one name.
    Several(Vec<PathBuf>),
}

/// Every sample a wikilink of the sample `from` may name: its project's
/// files, by their file names and their `name:`, those `known` gives read
/// already; outside a project, the samples of its folder.
pub fn linkable(
    config: Option<&ProjectConfig>,
    from: &Path,
    known: &[(PathBuf, Option<String>)],
) -> Vec<(PathBuf, Option<String>)> {
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let found = match config {
        Some(config) => discover(config.root(), Some(config)),
        None => discover(from.parent().unwrap_or(Path::new(".")), None),
    }
    .map(|found| found.paths)
    .unwrap_or_default();
    // Each file known by what it resolves to, once: searched along the list,
    // a file was resolved again for each it was compared with.
    let names: std::collections::HashMap<PathBuf, &Option<String>> = known
        .iter()
        .map(|(held, name)| (canonical(held), name))
        .collect();
    let mut listed = std::collections::HashSet::new();
    let mut samples: Vec<(PathBuf, Option<String>)> = found
        .into_iter()
        .map(|path| {
            let resolved = canonical(&path);
            // A file not read yet is read for its name alone.
            let name = match names.get(&resolved) {
                Some(name) => (*name).clone(),
                None => crate::format::document::load_sample(&path)
                    .ok()
                    .and_then(|(sample, _)| sample.name().map(str::to_string)),
            };
            listed.insert(resolved);
            (path, name)
        })
        .collect();
    for (path, name) in known {
        if listed.insert(canonical(path)) {
            samples.push((path.clone(), name.clone()));
        }
    }
    samples
}

/// Where `link`, held by the sample file `from`, leads: a wikilink to the
/// sample, or the file, of the project answering to it, the project's index
/// made only for one; a Markdown path read from `from`'s folder, a sample
/// where it is a `.md` file that reads as one. `known` are the samples read
/// already, as [`linkable`] takes them.
pub fn destination(
    link: &crate::core::link::Link,
    from: &Path,
    config: Option<&ProjectConfig>,
    known: &[(PathBuf, Option<String>)],
) -> Destination {
    destination_in(link, from, || index_of(config, from, known))
}

/// What a wikilink of the sample `from` may name, indexed: its project's
/// samples, as [`linkable`] lists them, and its project's other files.
pub fn index_of(
    config: Option<&ProjectConfig>,
    from: &Path,
    known: &[(PathBuf, Option<String>)],
) -> crate::core::link::Index {
    let samples = linkable(config, from, known);
    let root = match config {
        Some(config) => config.root().to_path_buf(),
        None => from.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    crate::core::link::Index::new(
        samples
            .iter()
            .map(|(path, name)| (path.as_path(), name.as_deref())),
    )
    .with_files(project_files(&root))
}

/// Every file under `root` a project's link may name, a hidden folder and a
/// virtual environment left out, as the scan leaves them.
pub fn project_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !(entry.file_name().to_string_lossy().starts_with('.')
                    || entry.path().join("pyvenv.cfg").is_file())
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect()
}

thread_local! {
    /// Each project's other files, by its root, indexed: until the project
    /// is read again.
    static FILES: std::cell::RefCell<std::collections::HashMap<PathBuf, std::rc::Rc<crate::core::link::Index>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Whether `target`, a wikilink of the sample `from`, names a file of its
/// project that is no sample: what a table shows without a mark. The files
/// are listed once until the project is read again.
pub fn names_a_file(from: &Path, target: &str) -> bool {
    let root = match config::find(from).and_then(|file| file.parent().map(Path::to_path_buf)) {
        Some(root) => root,
        None => from.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    let index = FILES.with(|files| {
        files
            .borrow_mut()
            .entry(root.clone())
            .or_insert_with(|| {
                std::rc::Rc::new(
                    crate::core::link::Index::default().with_files(project_files(&root)),
                )
            })
            .clone()
    });
    matches!(index.resolve(target), crate::core::link::Resolved::File(_))
}

/// The projects' files listed again at the next asking: what each reading of
/// a project does first.
pub fn forget_files() {
    FILES.with(|files| files.borrow_mut().clear());
}

/// A file at the place its fragment names, where it is a PDF's page.
fn placed(file: PathBuf, fragment: Option<&String>) -> Destination {
    let pdf = file
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    match fragment.and_then(|fragment| crate::core::link::page_of(fragment)) {
        Some(page) if pdf => Destination::Page { file, page },
        _ => Destination::File(file),
    }
}

/// [`destination`], the samples a wikilink may name given as their index:
/// made once for every link of a project.
pub fn destination_in<Samples: AsRef<crate::core::link::Index>>(
    link: &crate::core::link::Link,
    from: &Path,
    linkable: impl FnOnce() -> Samples,
) -> Destination {
    use crate::core::link::{self, Link, Resolved};
    match link {
        Link::Url { address, .. } => Destination::Address(address.clone()),
        Link::Sample {
            target, fragment, ..
        } => match linkable().as_ref().resolve(target) {
            Resolved::One(path) => Destination::Sample(path),
            Resolved::File(file) => placed(
                dunce::canonicalize(&file).unwrap_or(file),
                fragment.as_ref(),
            ),
            Resolved::Nowhere => Destination::Nowhere(target.clone()),
            Resolved::Several(paths) => Destination::Several(paths),
        },
        Link::Path { path, fragment, .. } => {
            let folder = from.parent().unwrap_or(Path::new("."));
            let found = link::path_from(folder, path);
            if !found.exists() {
                return Destination::Nowhere(path.clone());
            }
            // Whole, its `..` resolved and in the system's separators: opened,
            // shown and compared as the file it is.
            let found = dunce::canonicalize(&found).unwrap_or(found);
            let markdown = found
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
                });
            if found.is_file() && markdown && crate::format::document::load_sample(&found).is_ok() {
                Destination::Sample(found)
            } else {
                placed(found, fragment.as_ref())
            }
        }
    }
}
