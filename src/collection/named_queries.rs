//! Reusable **selections**: a named filter defined once in `.samplekitrc` and
//! evaluated identically by the Python API, the CLI and the TUI. One
//! definition, one result, three surfaces.
//!
//! Filed under `30-config/` in the vault because that is where a reader looks
//! for it, but it is Layer 5: it *executes* what `project-config` holds as
//! text.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::collection::sample_list::SampleList;
use crate::config::project_config::{NamedQuery, ProjectConfig};
use crate::query::field_addressing::FieldError;
use crate::query::filter_language::{self as filter, FilterError};

/// The declaration, by name. Here rather than in `project-config` because this
/// is where a missing query can be reported *as a query*: the layer below has
/// no reason to know that a query is the kind of thing a user misspells at a
/// command line.
pub fn named<'a>(config: &'a ProjectConfig, name: &str) -> Result<&'a NamedQuery, QueryError> {
    config.query(name).map_err(|_| {
        let available: Vec<String> = config.query_names().iter().map(|n| n.to_string()).collect();
        QueryError::UnknownQuery {
            suggestion: crate::core::identifier::nearest(
                name,
                available.iter().map(String::as_str),
            ),
            name: name.to_string(),
            available,
        }
    })
}

/// Runs a named query over a collection.
///
/// The filter is parsed **here**, against the collection it runs on, so that a
/// field diagnostic can name what exists in *that* collection. A query parsed
/// at load time would either lose that context or need re-checking anyway.
///
/// **Filtering never sorts.** A selection preserves the order it was given,
/// and any reordering is applied afterwards by whatever presents it. It is
/// also the cheap direction: sorting first costs a key extraction for every
/// sample about to be discarded, which — when a key is a Python formula — is
/// the difference between one second and thirty.
pub fn run(query: &NamedQuery, list: &SampleList) -> Result<SampleList, QueryError> {
    let parsed = filter::parse(&query.filter).map_err(|source| QueryError::Filter {
        query: query.name.clone(),
        source,
    })?;
    list.filter(&parsed).map_err(|error| match error {
        crate::collection::sample_list::ListError::Filter(FilterError::Field(field)) => {
            QueryError::Field {
                query: query.name.clone(),
                source: *field,
            }
        }
        crate::collection::sample_list::ListError::Filter(source) => QueryError::Filter {
            query: query.name.clone(),
            source,
        },
        other => QueryError::Collection {
            query: query.name.clone(),
            reason: other.to_string(),
        },
    })
}

/// Where a query applies when the command line named no target.
///
/// A declared directory is a **default**, not a constraint: an explicit target
/// replaces it. What it removes is the case where nobody said anything and the
/// answer depended on the working directory — which, under this layout, is
/// often the whole of `samples/`, whose subdirectories hold samples described
/// by different templates. The key is `directory`: it names a directory, and
/// the word `base` it replaced read as a parent query.
pub fn directory_of(
    query: &NamedQuery,
    config: &ProjectConfig,
    explicit: Option<&Path>,
) -> PathBuf {
    match explicit {
        Some(target) => target.to_path_buf(),
        None => match &query.directory {
            Some(directory) => config.resolve(&directory.to_string_lossy()),
            None => config.root().to_path_buf(),
        },
    }
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum QueryError {
    UnknownQuery {
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    Filter {
        query: String,
        source: FilterError,
    },
    Field {
        query: String,
        source: FieldError,
    },
    /// The collection refused for a reason that is not about the predicate.
    Collection {
        query: String,
        reason: String,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::UnknownQuery {
                name,
                available,
                suggestion,
            } => {
                write!(f, "no query named '{name}'")?;
                if let Some(suggestion) = suggestion {
                    write!(f, "\n  did you mean: '{suggestion}'?")?;
                }
                if !available.is_empty() {
                    write!(f, "\n  available: {}", available.join(", "))?;
                }
                Ok(())
            }
            // The name is the only thing connecting the message to something
            // the user can edit: they typed `--query approved`, not a field.
            QueryError::Filter { query, source } => {
                write!(f, "in named query '{query}': {source}")
            }
            QueryError::Field { query, source } => {
                write!(f, "in named query '{query}': {source}")
            }
            QueryError::Collection { query, reason } => {
                write!(f, "in named query '{query}': {reason}")
            }
        }
    }
}

impl std::error::Error for QueryError {}
