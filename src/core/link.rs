//! An attribute that leads elsewhere: a wikilink to a sample, `[[steel-02]]`,
//! as Obsidian writes one; a Markdown link, `[the second](steel-02.md)`, to a
//! file beside the sample or a web page; or a web address alone. The value
//! stays text, held and compared as written; this module only says what it
//! leads to, and to which of a project's samples a wikilink does. It reads no
//! file.

use std::path::{Path, PathBuf};

/// Where an attribute's text leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// `[[target]]`, `[[target|shown]]` or `[[target#heading]]`: a sample of
    /// the project, by its file's name or its `name:`.
    Sample {
        target: String,
        /// What the alias after `|` shows in its place, where there is one.
        shown: Option<String>,
        /// What follows `#`: a heading, a block `^id`, a PDF's `page=12`.
        fragment: Option<String>,
    },
    /// `[shown](path)`: a file, by its path from the sample's own folder, as
    /// Markdown reads a link; a sample where it is one. Spaces written `%20`
    /// or the path between `<` and `>`; `#…` after it a place in the file.
    Path {
        path: String,
        shown: Option<String>,
        fragment: Option<String>,
    },
    /// `https://…` or `http://…`, alone or as `[shown](https://…)`.
    Url {
        address: String,
        shown: Option<String>,
    },
}

impl Link {
    /// What a reader sees of it: the alias or the text, else the target.
    pub fn shown(&self) -> &str {
        match self {
            Link::Sample { target, shown, .. } => shown.as_deref().unwrap_or(target),
            Link::Path { path, shown, .. } => shown.as_deref().unwrap_or(path),
            Link::Url { address, shown } => shown.as_deref().unwrap_or(address),
        }
    }
}

/// The link a whole text is, or `None`: text that only holds one is text.
pub fn parse(text: &str) -> Option<Link> {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix("[[")
        .and_then(|rest| rest.strip_suffix("]]"))
    {
        if inner.contains(['[', ']', '\n']) {
            return None;
        }
        let (target, shown) = match inner.split_once('|') {
            Some((target, shown)) => (target, Some(shown.trim()).filter(|s| !s.is_empty())),
            None => (inner, None),
        };
        // A heading names a place in a note, and a sample is opened whole;
        // a PDF's page is where it opens.
        let (target, fragment) = match target.split_once('#') {
            Some((target, fragment)) => (target, Some(fragment.trim()).filter(|f| !f.is_empty())),
            None => (target, None),
        };
        let target = target.trim();
        if target.is_empty() {
            return None;
        }
        return Some(Link::Sample {
            target: target.to_string(),
            shown: shown.map(str::to_string),
            fragment: fragment.map(str::to_string),
        });
    }
    if let Some((shown, target)) = markdown(text) {
        let shown = Some(shown.trim().to_string()).filter(|shown| !shown.is_empty());
        if is_address(&target) {
            return Some(Link::Url {
                address: decoded(&target),
                shown,
            });
        }
        // The place in the file taken off before the path is decoded: a `#`
        // of the file's name is written `%23`.
        let (path, fragment) = match target.split_once('#') {
            Some((path, fragment)) => (path, Some(decoded(fragment)).filter(|f| !f.is_empty())),
            None => (target.as_str(), None),
        };
        let path = decoded(path);
        if path.is_empty() {
            return None;
        }
        return Some(Link::Path {
            path,
            shown,
            fragment,
        });
    }
    is_address(text).then(|| Link::Url {
        address: text.to_string(),
        shown: None,
    })
}

/// `[shown](target)`, the whole text: the text shown and the target, its
/// `<…>` taken off, a title after it dropped, `%XX` decoded.
fn markdown(text: &str) -> Option<(&str, String)> {
    let rest = text.strip_prefix('[')?;
    let (shown, after) = rest.split_once("](")?;
    let inside = after.strip_suffix(')')?;
    if shown.contains(['[', ']', '\n']) || inside.contains('\n') {
        return None;
    }
    let inside = inside.trim();
    let target = match inside.strip_prefix('<') {
        Some(rest) => rest.split_once('>')?.0.to_string(),
        // `(path "a title")`: the title is Markdown's tooltip.
        None => inside.split_whitespace().next()?.to_string(),
    };
    (!target.is_empty()).then_some((shown, target))
}

/// The addresses a link may be, beyond a file: the web's, and those a
/// researcher's other tools answer to — a reference in Zotero, a note in
/// Obsidian, a mail. Named one by one: `ratio:2` is no address.
const SCHEMES: [&str; 7] = [
    "https://",
    "http://",
    "ftp://",
    "file://",
    "mailto:",
    "zotero://",
    "obsidian://",
];

fn is_address(text: &str) -> bool {
    SCHEMES.iter().any(|scheme| {
        text.len() > scheme.len()
            && text
                .get(..scheme.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    }) && !text.contains(char::is_whitespace)
}

/// The page a fragment names, `page=12` as a PDF reader reads it.
pub fn page_of(fragment: &str) -> Option<u32> {
    fragment
        .split('&')
        .find_map(|part| part.trim().strip_prefix("page="))
        .and_then(|page| page.parse().ok())
        .filter(|page| *page > 0)
}

/// `%20` and the other escapes a link may write a path's characters as.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(hex) = text.get(at + 1..at + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// A Markdown link's path read from the folder of the file that holds it, as
/// Markdown reads one; an absolute path, or one starting with `~`, as it is.
/// A path as a project's files write it, read the same on every system: `\`
/// is a separator there, as `/` is, so that a `.samplekitrc` or a link
/// written on Windows reads on Linux and macOS, where `\` would be part of a
/// file's name.
pub fn portable(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '\\' {
        path.to_string()
    } else {
        path.replace('\\', "/")
    }
}

pub fn path_from(folder: &Path, path: &str) -> PathBuf {
    let path = &portable(path);
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        folder.join(path)
    }
}

/// The sample a wikilink's target names among `samples`, each its path and
/// its `name:`: by its file's name first, as Obsidian finds a note, then by
/// its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    One(PathBuf),
    /// A file of the project that is no sample: `[[curve.csv]]`.
    File(PathBuf),
    /// No sample has that file name, nor that name, and no file is named so.
    Nowhere,
    /// Several do, which no reader can choose between.
    Several(Vec<PathBuf>),
}

pub fn resolve<'s>(
    target: &str,
    samples: impl IntoIterator<Item = (&'s Path, Option<&'s str>)>,
) -> Resolved {
    // One pass, as [`Index::resolve`] answers: a link shown in a table is
    // resolved alone, and an index of the whole project for each was more
    // than the search.
    let target = target.trim();
    let named = without_markdown(target);
    let folded = named.to_lowercase();
    let parts = parts_of(named);
    let mut found: [Vec<PathBuf>; 4] = Default::default();
    for (path, name) in samples {
        if parts.len() > 1 {
            if ends_with(path, &parts) {
                found[0].push(path.to_path_buf());
            }
            continue;
        }
        let stem = stem_of(path);
        if stem == Some(named) {
            found[0].push(path.to_path_buf());
        }
        if name == Some(named) {
            found[1].push(path.to_path_buf());
        }
        if stem.is_some_and(|stem| stem.to_lowercase() == folded) {
            found[2].push(path.to_path_buf());
        }
        if name.is_some_and(|name| name.to_lowercase() == folded) {
            found[3].push(path.to_path_buf());
        }
    }
    match found.into_iter().find(|found| !found.is_empty()) {
        Some(found) if found.len() == 1 => Resolved::One(found[0].clone()),
        Some(found) => Resolved::Several(found),
        None => Resolved::Nowhere,
    }
}

/// A target's parts, `folder/name`, in lower case.
fn parts_of(target: &str) -> Vec<String> {
    target
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_lowercase)
        .collect()
}

/// Whether `path` ends with `parts`, its last part its name as a wikilink
/// calls it.
fn ends_with(path: &Path, parts: &[String]) -> bool {
    let Some((last, folders)) = parts.split_last() else {
        return false;
    };
    stem_of(path).is_some_and(|stem| stem.to_lowercase() == *last) && folders_end(path, folders)
}

/// Whether the folders above `path` end with `folders`, in any case.
fn folders_end(path: &Path, folders: &[String]) -> bool {
    let above: Vec<String> = path
        .parent()
        .into_iter()
        .flat_map(|folder| folder.components())
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    above.ends_with(folders)
}

/// The samples a wikilink may name, by their files' names and their `name:`:
/// made once, and asked for each link. Searched along the samples, every link
/// of a project was a search through all of it. The project's other files
/// may be given too, which a wikilink names with their extension.
#[derive(Debug, Default)]
pub struct Index {
    stems: std::collections::HashMap<String, Vec<PathBuf>>,
    names: std::collections::HashMap<String, Vec<PathBuf>>,
    /// The same two by their lower case: Obsidian finds `[[Steel-02]]`.
    stems_folded: std::collections::HashMap<String, Vec<PathBuf>>,
    names_folded: std::collections::HashMap<String, Vec<PathBuf>>,
    /// The other files, by their whole name in lower case.
    files: std::collections::HashMap<String, Vec<PathBuf>>,
}

/// A file's name without `.md`, or `.sample.md`: what a wikilink calls it.
fn stem_of(path: &Path) -> Option<&str> {
    let stem = path.file_stem()?.to_str()?;
    Some(stem.strip_suffix(".sample").unwrap_or(stem))
}

/// `name.md` and `name.markdown` called `name`, as Obsidian calls a note.
fn without_markdown(target: &str) -> &str {
    let lower = target.to_ascii_lowercase();
    for extension in [".md", ".markdown"] {
        if lower.ends_with(extension) && target.len() > extension.len() {
            return &target[..target.len() - extension.len()];
        }
    }
    target
}

impl Index {
    /// The index of `samples`, each its path and its `name:`.
    pub fn new<'s>(samples: impl IntoIterator<Item = (&'s Path, Option<&'s str>)>) -> Index {
        let mut index = Index::default();
        let put = |map: &mut std::collections::HashMap<String, Vec<PathBuf>>,
                   key: String,
                   path: &Path| {
            let held = map.entry(key).or_default();
            if !held.iter().any(|known| known == path) {
                held.push(path.to_path_buf());
            }
        };
        for (path, name) in samples {
            if let Some(stem) = stem_of(path) {
                put(&mut index.stems, stem.to_string(), path);
                put(&mut index.stems_folded, stem.to_lowercase(), path);
            }
            if let Some(name) = name {
                put(&mut index.names, name.to_string(), path);
                put(&mut index.names_folded, name.to_lowercase(), path);
            }
        }
        index
    }

    /// The same, and the project's other files: what a wikilink names by its
    /// whole name, `[[curve.csv]]`, `[[paper.pdf#page=3]]`.
    pub fn with_files(mut self, files: impl IntoIterator<Item = PathBuf>) -> Index {
        for file in files {
            if let Some(name) = file.file_name().and_then(|name| name.to_str()) {
                let held = self.files.entry(name.to_lowercase()).or_default();
                if !held.contains(&file) {
                    held.push(file);
                }
            }
        }
        self
    }

    /// The sample `target` names: by its file's name first, then by its
    /// name, each as written then in any case; `folder/name` by the end of
    /// its path; else a file of the project by its whole name.
    pub fn resolve(&self, target: &str) -> Resolved {
        let target = target.trim();
        let by = |found: Vec<PathBuf>| match found.as_slice() {
            [] => None,
            [one] => Some(Resolved::One(one.clone())),
            _ => Some(Resolved::Several(found)),
        };
        let named = without_markdown(target);
        let found = if named.contains(['/', '\\']) {
            self.ending_with(named)
        } else {
            [
                self.stems.get(named),
                self.names.get(named),
                self.stems_folded.get(&named.to_lowercase()),
                self.names_folded.get(&named.to_lowercase()),
            ]
            .into_iter()
            .flatten()
            .find(|found| !found.is_empty())
            .cloned()
            .unwrap_or_default()
        };
        if let Some(resolved) = by(found) {
            return resolved;
        }
        self.file(target).unwrap_or(Resolved::Nowhere)
    }

    /// The samples whose path ends with `named`'s parts, `folder/name`: how
    /// Obsidian tells two notes of one name apart.
    fn ending_with(&self, named: &str) -> Vec<PathBuf> {
        let parts = parts_of(named);
        let Some((last, folders)) = parts.split_last() else {
            return Vec::new();
        };
        self.stems_folded
            .get(last)
            .into_iter()
            .flatten()
            .filter(|path| folders_end(path, folders))
            .cloned()
            .collect()
    }

    /// A file of the project named `target`, its folders too where it
    /// writes them.
    fn file(&self, target: &str) -> Option<Resolved> {
        let parts = parts_of(target);
        let (last, folders) = parts.split_last()?;
        let found: Vec<PathBuf> = self
            .files
            .get(last)?
            .iter()
            .filter(|path| folders_end(path, folders))
            .cloned()
            .collect();
        match found.as_slice() {
            [] => None,
            [one] => Some(Resolved::File(one.clone())),
            _ => Some(Resolved::Several(found)),
        }
    }
}

impl AsRef<Index> for Index {
    fn as_ref(&self) -> &Index {
        self
    }
}
