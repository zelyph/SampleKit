//! A sample renamed, and its links with it: its file named again beside the
//! old one, its own files with it, its `name:` where it was the file's name,
//! and every link of its project that led to it written to lead to it again.
//! Planned first, so that what it does is said before anything is written.

use std::path::{Path, PathBuf};

use crate::config::discovery;
use crate::config::project_config::ProjectConfig;
use crate::core::identifier::Identifier;
use crate::core::link::{self, Link, Resolved};
use crate::core::sample::{AttributeValue, Sample};
use crate::core::value::Value;
use crate::format::document::{self, Destination};

/// A link written again: in which file, which attribute, which item of a
/// list, and the text before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChange {
    pub file: PathBuf,
    pub field: String,
    pub item: Option<usize>,
    pub before: String,
    pub after: String,
}

/// What renaming a sample does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Its own files, each from and to.
    pub own: Vec<(PathBuf, PathBuf)>,
    /// Its `name:`, where it was the file's name: before and after.
    pub name: Option<(String, String)>,
    pub links: Vec<LinkChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameError(pub String);

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RenameError {}

/// A file's name without `.md`, and `.sample` where it has one.
fn stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.strip_suffix(".sample")
        .map(str::to_string)
        .unwrap_or(stem)
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What renaming the sample at `from` to `new` would do, nothing written.
pub fn plan(config: Option<&ProjectConfig>, from: &Path, new: &str) -> Result<Plan, RenameError> {
    let new = new.trim();
    let new = new.strip_suffix(".md").unwrap_or(new);
    let new = new.strip_suffix(".sample").unwrap_or(new);
    if new.is_empty() || new.contains(['/', '\\']) || new.starts_with('.') {
        return Err(RenameError(format!(
            "'{new}' is no sample's name: a file's name, without its folder, \
             such as steel-04"
        )));
    }
    let old = stem(from);
    if new == old {
        return Err(RenameError(format!(
            "nothing to change — the sample is named {old} already"
        )));
    }
    let file_name = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = if file_name.ends_with(".sample.md") {
        ".sample.md"
    } else {
        ".md"
    };
    let to = from.with_file_name(format!("{new}{suffix}"));
    if to.exists() {
        return Err(RenameError(format!(
            "{} exists: another sample has that name",
            to.display()
        )));
    }
    let (sample, _) = document::load_sample(from)
        .map_err(|error| RenameError(format!("{}: {error}", from.display())))?;
    // Its own files: a folder named after it, or a file whose name begins
    // with its own.
    let mut own = Vec::new();
    if let Some(config) = config {
        let old_file = from
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let new_file = to
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        for file in discovery::sample_files(config, from, None).unwrap_or_default() {
            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // In a folder named after the sample, `machine/steel-01/`: the
            // folder is renamed, once for all it holds, whatever their names.
            let folder = file.ancestors().skip(1).find(|folder| {
                folder.starts_with(config.root())
                    && folder.file_name().is_some_and(|name| {
                        name.to_string_lossy() == old || name.to_string_lossy() == old_file
                    })
            });
            let (file, moved) = match folder {
                Some(folder) => {
                    let folder = folder.to_path_buf();
                    if own
                        .iter()
                        .any(|(held, _): &(PathBuf, PathBuf)| *held == folder)
                    {
                        continue;
                    }
                    let named_as_file = folder
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy() == old_file);
                    let moved = folder.with_file_name(if named_as_file { &new_file } else { new });
                    (folder, moved)
                }
                // Else by the file's name it begins with.
                None => {
                    let renamed = match name.strip_prefix(&old_file) {
                        Some(rest) => format!("{new_file}{rest}"),
                        None => match name.strip_prefix(&old) {
                            Some(rest) => format!("{new}{rest}"),
                            None => continue,
                        },
                    };
                    let moved = file.with_file_name(renamed);
                    (file, moved)
                }
            };
            if moved.exists() {
                return Err(RenameError(format!(
                    "{} exists: the sample's file {} cannot take its name",
                    moved.display(),
                    file.display()
                )));
            }
            own.push((file, moved));
        }
    }
    let name =
        (sample.written_name() == Some(old.as_str())).then(|| (old.clone(), new.to_string()));
    // Every link of the project leading to it.
    let samples = discovery::linkable(config, from, &[]);
    let this = canonical(from);
    let new_file_name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut links = Vec::new();
    for (path, _) in &samples {
        let Ok((held, _)) = document::load_sample(path) else {
            continue;
        };
        for field in held.attribute_names() {
            let Ok(value) = held.attribute(field) else {
                continue;
            };
            let items: Vec<(Option<usize>, &Value)> = match value {
                AttributeValue::Scalar(value) => vec![(None, value)],
                AttributeValue::List(values) => values
                    .iter()
                    .enumerate()
                    .map(|(at, value)| (Some(at), value))
                    .collect(),
            };
            for (item, value) in items {
                let Value::Text(text) = value else {
                    continue;
                };
                let after = match link::parse(text) {
                    Some(Link::Sample { target, .. })
                        if called(&target, &old)
                            && matches!(
                                link::resolve(
                                    &target,
                                    samples
                                        .iter()
                                        .map(|(path, name)| (path.as_path(), name.as_deref()))
                                ),
                                Resolved::One(found) if canonical(&found) == this
                            ) =>
                    {
                        wikilink_renamed(text, new)
                    }
                    Some(Link::Path { path: written, .. }) => {
                        let folder = path.parent().unwrap_or(Path::new("."));
                        let target = canonical(&link::path_from(folder, &written));
                        if target == this {
                            markdown_renamed(text, &file_name, &new_file_name)
                        } else if let Some((from, to)) = own
                            .iter()
                            .find(|(from, _)| target.starts_with(canonical(from)))
                        {
                            // A link to one of its own files, or into the
                            // folder named after it, follows the file moved.
                            let name = |path: &Path| {
                                path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            };
                            component_renamed(text, &name(from), &name(to))
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };
                if let Some(after) = after {
                    links.push(LinkChange {
                        file: path.clone(),
                        field: field.to_string(),
                        item,
                        before: text.clone(),
                        after,
                    });
                }
            }
        }
    }
    Ok(Plan {
        from: from.to_path_buf(),
        to,
        own,
        name,
        links,
    })
}

/// A link of the project leading to a sample: the file holding it, its
/// attribute, and its text as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub file: PathBuf,
    pub field: String,
    pub text: String,
}

/// Every link of the project leading to one of `samples`, held by a sample
/// that is not among them: what removing them leaves leading nowhere.
pub fn links_leading_to(config: Option<&ProjectConfig>, samples: &[PathBuf]) -> Vec<Incoming> {
    let Some(first) = samples.first() else {
        return Vec::new();
    };
    let known = discovery::linkable(config, first, &[]);
    let index = link::Index::new(
        known
            .iter()
            .map(|(path, name)| (path.as_path(), name.as_deref())),
    );
    let these: Vec<PathBuf> = samples.iter().map(|path| canonical(path)).collect();
    let mut found = Vec::new();
    for (path, _) in &known {
        if these.contains(&canonical(path)) {
            continue;
        }
        let Ok((held, _)) = document::load_sample(path) else {
            continue;
        };
        for field in held.attribute_names() {
            let Ok(value) = held.attribute(field) else {
                continue;
            };
            let items: Vec<&Value> = match value {
                AttributeValue::Scalar(value) => vec![value],
                AttributeValue::List(values) => values.iter().collect(),
            };
            for value in items {
                let Value::Text(text) = value else {
                    continue;
                };
                let target = match link::parse(text) {
                    Some(Link::Sample { target, .. }) => match index.resolve(&target) {
                        Resolved::One(found) => canonical(&found),
                        _ => continue,
                    },
                    Some(Link::Path { path: written, .. }) => {
                        let folder = path.parent().unwrap_or(Path::new("."));
                        canonical(&link::path_from(folder, &written))
                    }
                    _ => continue,
                };
                if these.contains(&target) {
                    found.push(Incoming {
                        file: path.clone(),
                        field: field.to_string(),
                        text: text.clone(),
                    });
                }
            }
        }
    }
    found
}

/// `[[old|shown]]` or `[[old#heading]]` naming `new`, its heading and its
/// alias kept.
fn wikilink_renamed(text: &str, new: &str) -> Option<String> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("[[")?.strip_suffix("]]")?;
    let (target, shown) = match inner.split_once('|') {
        Some((target, shown)) => (target, Some(shown)),
        None => (inner, None),
    };
    let (target, fragment) = match target.split_once('#') {
        Some((target, fragment)) => (target, Some(fragment)),
        None => (target, None),
    };
    // Its folders and its extension as it wrote them: `[[2024/old.md]]`
    // becomes `[[2024/new.md]]`.
    let (folders, last) = match target.rfind(['/', '\\']) {
        Some(at) => target.split_at(at + 1),
        None => ("", target),
    };
    let extension = [".md", ".markdown"]
        .into_iter()
        .find(|extension| last.to_ascii_lowercase().ends_with(extension))
        .map_or("", |extension| &last[last.len() - extension.len()..]);
    let mut renamed = format!("[[{folders}{new}{extension}");
    if let Some(fragment) = fragment {
        renamed.push('#');
        renamed.push_str(fragment);
    }
    if let Some(shown) = shown {
        renamed.push('|');
        renamed.push_str(shown);
    }
    renamed.push_str("]]");
    Some(renamed)
}

/// Whether a wikilink's target calls a sample `old`: by its last part,
/// `.md` or not, in any case, as Obsidian finds a note.
fn called(target: &str, old: &str) -> bool {
    let last = target.rsplit(['/', '\\']).next().unwrap_or(target).trim();
    let lower = last.to_lowercase();
    let bare = [".md", ".markdown"]
        .into_iter()
        .find_map(|extension| lower.strip_suffix(extension))
        .unwrap_or(&lower);
    bare == old.to_lowercase()
}

/// `[shown](folder/old.md)` leading to `new`'s file, its folder kept, as it
/// was written: spaces as `%20` where it wrote them so.
fn markdown_renamed(text: &str, old: &str, new: &str) -> Option<String> {
    let encoded = |name: &str| name.replace(' ', "%20");
    for (from, to) in [
        (old.to_string(), new.to_string()),
        (encoded(old), encoded(new)),
    ] {
        if let Some(at) = text.rfind(&from) {
            return Some(format!("{}{to}{}", &text[..at], &text[at + from.len()..]));
        }
    }
    None
}

/// `[shown](../machine/old/curve.csv)` with the path's component `old`, a
/// folder or a file, named `new`: the last one written whole between
/// separators, so that `old` within a longer name is left alone.
fn component_renamed(text: &str, old: &str, new: &str) -> Option<String> {
    let encoded = |name: &str| name.replace(' ', "%20");
    let opens = |c: char| matches!(c, '/' | '\\' | '(' | '<');
    let closes = |c: char| matches!(c, '/' | '\\' | ')' | '>');
    for (from, to) in [
        (old.to_string(), new.to_string()),
        (encoded(old), encoded(new)),
    ] {
        let found = text.rmatch_indices(&from).find(|(at, _)| {
            text[..*at].chars().next_back().is_some_and(opens)
                && text[at + from.len()..].chars().next().is_some_and(closes)
        });
        if let Some((at, _)) = found {
            return Some(format!("{}{to}{}", &text[..at], &text[at + from.len()..]));
        }
    }
    None
}

/// The plan carried out: the links written again, the sample written under
/// its new name and the old file removed, its own files moved. Each file
/// written is noted, so that the command's snapshot keeps them.
pub fn apply(plan: &Plan) -> Result<Vec<PathBuf>, RenameError> {
    let failed = |path: &Path, error: &dyn std::fmt::Display| {
        RenameError(format!("{}: {error}", path.display()))
    };
    let mut written = Vec::new();
    // The others' links first; the sample's own links to itself with it.
    let mut files: Vec<&PathBuf> = plan.links.iter().map(|change| &change.file).collect();
    files.dedup();
    let this = canonical(&plan.from);
    for file in files {
        if canonical(file) == this {
            continue;
        }
        let (mut sample, origin) =
            document::load_sample(file).map_err(|error| failed(file, &error))?;
        rewrite_links(&mut sample, plan, file).map_err(|error| failed(file, &error))?;
        document::save_sample(&mut sample, &Destination::Origin(origin))
            .map_err(|error| failed(file, &error))?;
        written.push(file.clone());
    }
    // Copied whole, as its file writes it, then changed where it changes.
    if plan.to.exists() {
        return Err(failed(&plan.to, &"exists: another sample has that name"));
    }
    document::note_written(&plan.to);
    std::fs::copy(&plan.from, &plan.to).map_err(|error| failed(&plan.to, &error))?;
    let own_links = plan
        .links
        .iter()
        .any(|change| canonical(&change.file) == this);
    if own_links || plan.name.is_some() {
        let (mut sample, origin) =
            document::load_sample(&plan.to).map_err(|error| failed(&plan.to, &error))?;
        let mut moved = plan.clone();
        for change in &mut moved.links {
            if canonical(&change.file) == this {
                change.file = plan.to.clone();
            }
        }
        rewrite_links(&mut sample, &moved, &plan.to).map_err(|error| failed(&plan.to, &error))?;
        if let Some((_, new)) = &plan.name {
            sample.set_name(Some(new.clone()));
        }
        document::save_sample(&mut sample, &Destination::Origin(origin))
            .map_err(|error| failed(&plan.to, &error))?;
    }
    written.push(plan.to.clone());
    document::note_written(&plan.from);
    crate::format::document::remove_file(&plan.from).map_err(|error| failed(&plan.from, &error))?;
    written.push(plan.from.clone());
    for (from, to) in &plan.own {
        document::note_written(from);
        crate::format::document::move_file(from, to).map_err(|error| failed(from, &error))?;
        document::note_written(to);
        written.push(to.clone());
    }
    Ok(written)
}

/// The plan's changes to the links `file` holds, made in `sample`.
fn rewrite_links(sample: &mut Sample, plan: &Plan, file: &Path) -> Result<(), String> {
    let file = canonical(file);
    for change in plan
        .links
        .iter()
        .filter(|change| canonical(&change.file) == file)
    {
        let name = Identifier::new(&change.field).map_err(|error| error.to_string())?;
        let value = match (
            sample.attribute(&name).map_err(|error| error.to_string())?,
            change.item,
        ) {
            (AttributeValue::Scalar(_), None) => {
                AttributeValue::Scalar(Value::text(change.after.clone()))
            }
            (AttributeValue::List(items), Some(at)) => {
                let mut items = items.clone();
                if let Some(item) = items.get_mut(at) {
                    *item = Value::text(change.after.clone());
                }
                AttributeValue::list(items).map_err(|error| error.to_string())?
            }
            (other, _) => other.clone(),
        };
        sample
            .set_attribute(name, value)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
