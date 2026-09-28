//! Recognising a file this build cannot read, and saying what is different
//! about it.
//!
//! **It writes nothing, and has no write path at all**. Converting a collection
//! between two versions of SampleKit is its owner's, and documented; what
//! remains here is the reading that lets every command say *this predates
//! schema 1* or *this configuration was written for an earlier samplekit*
//! instead of `unknown field`.
//!

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::format::document::{self, Origin};
use crate::format::schema;

/// A pre-1 file claims `schema_version: 1` and is a different shape, so the
/// number cannot separate them. These three fields can.
/// What version 1 refuses, and therefore what says a file predates it. `data`
/// is a marker like the other three: being only a rename does not make a
/// file that carries nothing else any less pre-1.
const LEGACY_FIELDS: [&str; 4] = ["unit_math", "symbol_math", "precision_unc", "data"];

/// Whatever the version line of a pre-1 file says, it is *before this format*.
const BEFORE: u32 = 0;

#[derive(Debug, Clone, PartialEq)]
pub struct MigrationPlan {
    pub entries: Vec<MigrationEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MigrationEntry {
    pub origin: Origin,
    pub from_version: u32,
    pub to_version: u32,
    pub action: Action,
    pub alterations: Vec<Alteration>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Upgrade,
    Canonicalize,
    NoChange,
    /// A first-class outcome, not an error that aborts the run: one unreadable
    /// file among two hundred must not prevent the rest from being reported.
    Blocked {
        reason: String,
    },
}

/// What a migration would do to one file, in terms a reader can check.
///
/// Not `Change`: `dependency-graph` owns that name for what a mutation *is*.
#[derive(Debug, Clone, PartialEq)]
pub enum Alteration {
    Dropped {
        field: String,
        because: String,
    },
    Renamed {
        from: String,
        to: String,
    },
    Reformatted {
        what: String,
    },
    /// YAML comments a canonical write does not keep.
    DroppedComments {
        count: usize,
    },
}

impl fmt::Display for Alteration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Alteration::Dropped { field, because } => {
                write!(f, "drop '{field}' — {because}")
            }
            Alteration::Renamed { from, to } => write!(f, "rename '{from}' to '{to}'"),
            Alteration::Reformatted { what } => write!(f, "rewrite {what} canonically"),
            Alteration::DroppedComments { count } => write!(
                f,
                "drop {} — a write keeps none; remarks belong in the note",
                if *count == 1 {
                    "1 YAML comment".to_string()
                } else {
                    format!("{count} YAML comments")
                }
            ),
        }
    }
}

// -------------------------------------------------------------- planning

/// **Planning cannot write.** There is no write path here — not guarded by a
/// flag, not conditional on an option. A dry run that can write under some
/// combination of arguments is not a dry run.
pub fn plan(paths: &[PathBuf]) -> MigrationPlan {
    MigrationPlan {
        entries: paths.iter().map(|path| plan_one(path)).collect(),
    }
}

fn plan_one(path: &Path) -> MigrationEntry {
    let blocked = |origin: Origin, reason: String| MigrationEntry {
        origin,
        from_version: BEFORE,
        to_version: schema::version(),
        action: Action::Blocked { reason },
        alterations: Vec::new(),
    };
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return blocked(
                Origin {
                    path: path.to_path_buf(),
                    digest: crate::core::property::Fingerprint::new(""),
                },
                error.to_string(),
            );
        }
    };
    let origin = Origin {
        path: path.to_path_buf(),
        digest: document::digest_of(&source),
    };

    match document::parse(&source) {
        Ok(document) => {
            // A file whose frontmatter parses and whose sample does not is no
            // candidate: its preview must not say it is fine.
            if let Err(error) = schema::into_sample(document.schema.clone()) {
                return blocked(origin, error.to_string());
            }
            let version = document.schema.schema_version;
            // Reordered keys are detected without being rewritten.
            let canonical = document::write(&document);
            let mut repairing = document.schema.clone();
            let repairs = repair_tags(&mut repairing);
            if canonical == source && repairs.is_empty() {
                MigrationEntry {
                    origin,
                    from_version: version,
                    to_version: schema::version(),
                    action: Action::NoChange,
                    alterations: Vec::new(),
                }
            } else {
                let mut alterations = Vec::new();
                if canonical != source {
                    alterations.push(Alteration::Reformatted {
                        what: if frontmatter_lacks_version(&source) {
                            "the frontmatter, adding schema_version".to_string()
                        } else {
                            "the frontmatter".to_string()
                        },
                    });
                }
                alterations.extend(repairs.into_iter().map(|(from, to)| Alteration::Renamed {
                    from: format!("tag '{from}'"),
                    to: format!("'{to}'"),
                }));
                let count = comments_in(&source);
                if count > 0 {
                    alterations.push(Alteration::DroppedComments { count });
                }
                MigrationEntry {
                    origin,
                    from_version: version,
                    to_version: schema::version(),
                    action: Action::Canonicalize,
                    alterations,
                }
            }
        }
        Err(error) => {
            // In the frontmatter, never in the note: a researcher's prose may
            // say `precision_unc:` while explaining why the field went away,
            // and a merely invalid file must not be called pre-1 because of a
            // sentence underneath it.
            let normalized = Layout::of(&source).strip(&source);
            let frontmatter = split(&normalized).map(|(frontmatter, _)| frontmatter);
            let held: Vec<&str> = LEGACY_FIELDS
                .iter()
                .copied()
                .filter(|field| frontmatter.is_some_and(|text| text.contains(&format!("{field}:"))))
                .collect();
            if held.is_empty() {
                return blocked(origin, error.to_string());
            }
            let mut alterations = Vec::new();
            for field in ["unit_math", "symbol_math"] {
                if held.contains(&field) {
                    alterations.push(Alteration::Dropped {
                        field: field.to_string(),
                        because: "math variants are declared once, in .samplekitrc's [unit.*] \
                                  and [property.*], and a per-property copy would be a second \
                                  source of truth"
                            .to_string(),
                    });
                }
            }
            if held.contains(&"data") {
                alterations.push(Alteration::Renamed {
                    from: "data".to_string(),
                    to: "readings".to_string(),
                });
            }
            alterations.push(Alteration::Renamed {
                from: "value' and 'uncertainty".to_string(),
                to: "v' and 'u".to_string(),
            });
            if held.contains(&"precision_unc") {
                alterations.push(Alteration::Dropped {
                    field: "precision' and 'precision_unc".to_string(),
                    because: "a precision is declared once for the project, in .samplekitrc's \
                              [property.*], and a file holds none"
                        .to_string(),
                });
            }
            // No reordering is announced, because none happens: the property
            // order a file has is the order it keeps. The message that used to
            // stand here fired on every v0 file — including one with a single
            // property — and described a rewrite the code never performed.
            // Ordering by what a model declares would need the model, which is
            // a layer above this one, and the owner's verdict is that the
            // feature is not worth having.
            let count = comments_in(&source);
            if count > 0 {
                alterations.push(Alteration::DroppedComments { count });
            }
            MigrationEntry {
                origin,
                from_version: BEFORE,
                to_version: schema::version(),
                action: Action::Upgrade,
                alterations,
            }
        }
    }
}

// -------------------------------------------------------------- applying

/// A tag that is not an identifier, repaired to the nearest one that is; one no
/// repair reaches is kept as it is. Returns what was repaired to what.
fn repair_tags(schema: &mut schema::SampleSchema) -> Vec<(String, String)> {
    let mut repaired = Vec::new();
    let mut tags: Vec<schema::Tag> = Vec::new();
    for tag in std::mem::take(&mut schema.tags) {
        let tag = match tag {
            schema::Tag::Unusable(text) => {
                match crate::core::identifier::Identifier::repaired(&text) {
                    Some(fixed) => {
                        repaired.push((text, fixed.to_string()));
                        schema::Tag::Usable(fixed)
                    }
                    None => schema::Tag::Unusable(text),
                }
            }
            usable => usable,
        };
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    schema.tags = tags;
    repaired
}

/// How many YAML comments a file's frontmatter holds: what a canonical write
/// drops. A `#` counts when it starts a line or follows a space outside quotes.
pub fn comments_in(source: &str) -> usize {
    let normalized = Layout::of(source).strip(source);
    let Some((frontmatter, _)) = split(&normalized) else {
        return 0;
    };
    frontmatter
        .lines()
        .filter(|line| {
            let mut quote: Option<char> = None;
            let mut previous = ' ';
            for character in line.chars() {
                match (quote, character) {
                    (Some(open), c) if c == open => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(character),
                    (None, '#') if previous.is_whitespace() => return true,
                    _ => {}
                }
                previous = character;
            }
            false
        })
        .count()
}

fn frontmatter_lacks_version(source: &str) -> bool {
    let normalized = Layout::of(source).strip(source);
    split(&normalized).is_some_and(|(frontmatter, _)| {
        !frontmatter
            .lines()
            .any(|line| line.starts_with("schema_version:"))
    })
}

fn split(source: &str) -> Option<(&str, &str)> {
    let body = source.strip_prefix('\u{feff}').unwrap_or(source);
    let after_open = body.strip_prefix("---\n")?;
    let close = after_open.find("\n---\n")?;
    Some((&after_open[..close + 1], &after_open[close + 5..]))
}

/// What a file's bytes say about themselves. The frontmatter is rebuilt from
/// scratch, so both have to be carried across by hand or the conversion quietly
/// changes a file it was only meant to convert.
struct Layout {
    crlf: bool,
}

impl Layout {
    fn of(source: &str) -> Layout {
        Layout {
            crlf: source.contains("\r\n"),
        }
    }

    /// LF, no BOM: what `split` and the YAML reader work in. A file's own
    /// conventions are never rewritten here — nothing in this module writes —
    /// so reading is all they have to survive.
    fn strip(&self, source: &str) -> String {
        let body = source.strip_prefix('\u{feff}').unwrap_or(source);
        if self.crlf {
            body.replace("\r\n", "\n")
        } else {
            body.to_string()
        }
    }
}

// ------------------------------------------------------- configurations

/// Whether a `.samplekitrc` is the previous implementation's: a `[model]
/// python` key, a bare `python`, or one of its plural sections. Said where it
/// is met, and converted by nobody: its parser is on `main`, `src/config.rs`,
/// and the configuration guide names the sections this one reads.
pub fn is_earlier_configuration(source: &str) -> bool {
    let Ok(parsed) = source.parse::<toml::Table>() else {
        return false;
    };
    let old_model = parsed
        .get("model")
        .and_then(|model| model.get("python"))
        .or_else(|| parsed.get("python"))
        .is_some();
    old_model
        || ["views", "queries", "exports", "reports"]
            .iter()
            .any(|section| parsed.contains_key(*section))
}
