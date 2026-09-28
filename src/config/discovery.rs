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
            SkipReason::Unreadable { message } => write!(f, "could not be read: {message}"),
            // A newer format is no malformed one: its version says it all.
            SkipReason::Malformed { message } if message.starts_with("schema_version") => {
                write!(f, "{message}")
            }
            SkipReason::Malformed { message } => write!(f, "malformed frontmatter: {message}"),
            SkipReason::Configuration { message } => {
                write!(f, "not read as a configuration: {message}")
            }
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
    scan(root, &offset, &rules)
}

/// The same scan under rules the caller states — a project's, with its include
/// patterns replaced as Python's `pattern=` asks — rather than the declared ones.
/// Patterns are relative to `root`.
pub fn discover_with(root: &Path, rules: &CollectionRules) -> Result<Collection, DiscoveryError> {
    scan(root, Path::new(""), rules)
}

/// Where `folder` lies below `base`, both resolved; `None` outside it.
fn below(base: &Path, folder: &Path) -> Option<PathBuf> {
    let base = dunce::canonicalize(base).ok()?;
    let folder = dunce::canonicalize(folder).ok()?;
    folder.strip_prefix(&base).ok().map(Path::to_path_buf)
}

/// The scan, a pattern holding a `/` matched against a path relative to
/// `root` with `offset` before it: where `root` lies below the folder the
/// patterns were written from.
fn scan(root: &Path, offset: &Path, rules: &CollectionRules) -> Result<Collection, DiscoveryError> {
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
    walk(root, offset, rules, &mut candidates, &mut collection)?;
    // Explicit sort, never filesystem order: that varies by platform, by
    // filesystem and by the history of a directory, and a collection reporting
    // its samples in a different order on two machines makes an export
    // irreproducible.
    candidates.sort();
    for candidate in candidates {
        admit(&candidate, &mut collection);
    }
    note_configurations(root, &mut collection);
    Ok(collection)
}

/// Is this a sample, and if not, why not. The check is exactly as deep as the
/// question: frontmatter, and a version this build reads. Nothing below.
pub fn classify(path: &Path) -> Result<(), SkipReason> {
    let text = std::fs::read_to_string(path).map_err(|error| SkipReason::Unreadable {
        message: error.to_string(),
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
    // Only the version is read. A sample with an unreadable property or a table
    // that violates its own declaration **is discovered**, and fails where it
    // is used — dropping it here would remove a real sample from a query for a
    // reason the query never mentions.
    #[derive(serde::Deserialize)]
    struct JustTheVersion {
        schema_version: Option<u32>,
    }
    let parsed: JustTheVersion =
        serde_yaml_ng::from_str(&frontmatter).map_err(|error| SkipReason::Malformed {
            message: error.to_string(),
        })?;
    match parsed.schema_version {
        // Read as the current version; `validate` notes it.
        None => Ok(()),
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
    // it was looked for: `images/dry-stout/a.jpg` is dry-stout's. The longest
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
    candidates: &mut Vec<PathBuf>,
    collection: &mut Collection,
) -> Result<(), DiscoveryError> {
    // By `walkdir`. Directory symlinks are not followed: following them needs
    // cycle detection, and a link to an ancestor turns a scan into an infinite
    // one; a symlinked directory is a link, not a directory, to an entry that
    // does not follow.
    let entries = walkdir::WalkDir::new(at)
        .min_depth(1)
        .max_depth(if rules.recursive { usize::MAX } else { 1 })
        .follow_links(false)
        .into_iter()
        // A hidden directory and a virtual environment hold no samples anyone
        // wrote there: `.git`, and `.venv` with its thousands of files.
        .filter_entry(|entry| {
            !(entry.file_type().is_dir()
                && (entry.file_name().to_string_lossy().starts_with('.')
                    || entry.path().join("pyvenv.cfg").is_file()))
        });
    for entry in entries {
        let entry = entry.map_err(walk_error(at))?;
        let path = entry.path().to_path_buf();
        let kind = entry.file_type();
        if kind.is_dir() || (kind.is_symlink() && path.is_dir()) {
            continue;
        }
        if !matches_below(&path, at, offset, rules) {
            let excluded = rules
                .exclude
                .iter()
                .find(|pattern| matched(pattern, &path, at, offset));
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
