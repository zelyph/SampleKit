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
    },
    /// `[shown](path)`: a file, by its path from the sample's own folder, as
    /// Markdown reads a link; a sample where it is one. Spaces written `%20`
    /// or the path between `<` and `>`.
    Path { path: String, shown: Option<String> },
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
            Link::Sample { target, shown } => shown.as_deref().unwrap_or(target),
            Link::Path { path, shown } => shown.as_deref().unwrap_or(path),
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
        // A heading names a place in a note; a sample is opened whole.
        let target = target.split('#').next().unwrap_or_default().trim();
        if target.is_empty() {
            return None;
        }
        return Some(Link::Sample {
            target: target.to_string(),
            shown: shown.map(str::to_string),
        });
    }
    if let Some((shown, target)) = markdown(text) {
        let shown = Some(shown.trim().to_string()).filter(|shown| !shown.is_empty());
        if is_address(&target) {
            return Some(Link::Url {
                address: target,
                shown,
            });
        }
        return Some(Link::Path {
            path: target,
            shown,
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
    let target = decoded(&target);
    (!target.is_empty()).then_some((shown, target))
}

fn is_address(text: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .any(|scheme| text.starts_with(scheme) && text.len() > scheme.len())
        && !text.contains(char::is_whitespace)
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
pub fn path_from(folder: &Path, path: &str) -> PathBuf {
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
    /// No sample has that file name, nor that name.
    Nowhere,
    /// Several do, which no reader can choose between.
    Several(Vec<PathBuf>),
}

pub fn resolve<'s>(
    target: &str,
    samples: impl IntoIterator<Item = (&'s Path, Option<&'s str>)>,
) -> Resolved {
    let samples: Vec<(&Path, Option<&str>)> = samples.into_iter().collect();
    let by = |found: Vec<&Path>| match found.as_slice() {
        [] => None,
        [one] => Some(Resolved::One(one.to_path_buf())),
        several => Some(Resolved::Several(
            several.iter().map(|path| path.to_path_buf()).collect(),
        )),
    };
    let stems = samples
        .iter()
        .filter(|(path, _)| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.strip_suffix(".sample").unwrap_or(stem) == target)
        })
        .map(|(path, _)| *path)
        .collect();
    if let Some(resolved) = by(stems) {
        return resolved;
    }
    let names = samples
        .iter()
        .filter(|(_, name)| *name == Some(target))
        .map(|(path, _)| *path)
        .collect();
    by(names).unwrap_or(Resolved::Nowhere)
}
