//! Adds, removes or renames a tag over a selection: what each sample would
//! carry, decided before anything is written, and then the writes.
//!
//! It was the body of `samplekit tag`. It is a module because the workbench
//! writes a sample in exactly this way, and a second implementation would be a
//! second set of rules for which samples change and what a failure leaves.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::collection::sample_list::SampleList;
use crate::core::identifier::Identifier;
use crate::format::document::{self, Destination};

// ------------------------------------------------------------------- types

/// What is asked. The tags are identifiers already: a text that is not one is
/// refused by whoever read it, before there is anything to plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Add(Identifier),
    Remove(Identifier),
    Rename { from: Identifier, to: Identifier },
}

/// What one sample would become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub before: Vec<Identifier>,
    pub after: Vec<Identifier>,
}

/// Every sample of the selection that came from a file, on one side or the
/// other. A sample built in memory has nowhere to be written and is in neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub edit: Edit,
    pub changes: Vec<Change>,
    pub unchanged: Vec<PathBuf>,
}

// --------------------------------------------------------------- functions

/// The tags a sample would carry. **One pass**, so there is no intermediate
/// state where a sample carries neither spelling, and a renamed tag keeps the
/// position it already occupied.
pub fn retagged(before: &[Identifier], edit: &Edit) -> Vec<Identifier> {
    let mut after: Vec<Identifier> = match edit {
        Edit::Add(tag) => {
            let mut all = before.to_vec();
            if !all.contains(tag) {
                all.push(tag.clone());
            }
            all
        }
        Edit::Remove(tag) => before.iter().filter(|held| *held != tag).cloned().collect(),
        Edit::Rename { from, to } => before
            .iter()
            .map(|held| {
                if held == from {
                    to.clone()
                } else {
                    held.clone()
                }
            })
            .collect(),
    };
    // **A sample that already carries the new tag keeps one**, in the position
    // it already occupied: renaming must not produce `[reference, reference]`.
    let mut seen = Vec::new();
    after.retain(|tag| {
        if seen.contains(tag) {
            return false;
        }
        seen.push(tag.clone());
        true
    });
    after
}

/// What each sample of the selection would become. Reads nothing, writes
/// nothing.
pub fn plan(selection: &SampleList, edit: Edit) -> Plan {
    let mut changes = Vec::new();
    let mut unchanged = Vec::new();
    for entry in selection.iter() {
        let Some(path) = entry.path.clone() else {
            continue;
        };
        let before = entry.sample.borrow().tags().to_vec();
        let after = retagged(&before, &edit);
        if after == before {
            unchanged.push(path);
        } else {
            changes.push(Change {
                path,
                before,
                after,
            });
        }
    }
    Plan {
        edit,
        changes,
        unchanged,
    }
}

/// Writes the plan's changes, one file at a time, and returns the files
/// written.
///
/// **Each write is atomic**, so a failure halfway leaves the files before it
/// changed and the files after it untouched — and the error names exactly
/// which. Each file is read again, so that its save carries an origin and a
/// concurrent edit is refused rather than overwritten; and its tags are
/// computed again from what it holds now, so that a tag another hand added
/// since the plan was made is kept.
pub fn apply(plan: &Plan) -> Result<Vec<PathBuf>, TaggingError> {
    let mut written = Vec::new();
    for change in &plan.changes {
        if let Err(reason) = write_one(&change.path, &plan.edit) {
            return Err(TaggingError {
                path: change.path.clone(),
                reason,
                written,
            });
        }
        written.push(change.path.clone());
    }
    Ok(written)
}

fn write_one(path: &Path, edit: &Edit) -> Result<(), String> {
    let (mut sample, origin) = document::load_sample(path).map_err(|error| error.to_string())?;
    let after = retagged(sample.tags(), edit);
    sample.set_tags(after);
    document::save_sample(&mut sample, &Destination::Origin(origin))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

// ------------------------------------------------------------------ errors

/// A file that could not be written, and every file written before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggingError {
    pub path: PathBuf,
    pub reason: String,
    pub written: Vec<PathBuf>,
}

impl fmt::Display for TaggingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.reason)?;
        if !self.written.is_empty() {
            let names: Vec<String> = self
                .written
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            write!(
                f,
                "\n  {} changed before this one: {}",
                if names.len() == 1 {
                    "1 file was".to_string()
                } else {
                    format!("{} files were", names.len())
                },
                names.join(", ")
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for TaggingError {}
