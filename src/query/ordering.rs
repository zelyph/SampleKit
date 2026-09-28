//! Sorting a collection, deterministically. A small module with one hard
//! requirement — that asking for an order and not getting one is impossible.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::core::value::{self, Value};
use crate::query::field_addressing::{self as fields, Field, FieldError, Resolution, Subject};

// ------------------------------------------------------------------- types

#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub field: Field,
    pub descending: bool,
}

/// Non-empty by construction. A sort with no keys is a request that cannot be
/// honored, and refusing it here is cheaper than discovering later that nothing
/// happened.
#[derive(Debug, Clone, PartialEq)]
pub struct SortSpec {
    keys: Vec<SortKey>,
}

impl SortSpec {
    pub fn new(keys: Vec<SortKey>) -> Result<SortSpec, OrderError> {
        if keys.is_empty() {
            return Err(OrderError::EmptySpec);
        }
        Ok(SortSpec { keys })
    }

    pub fn keys(&self) -> &[SortKey] {
        &self.keys
    }
}

/// `["beer", "-plato"]`: keys in order, `-` reversing the one it
/// prefixes. Held as text in a profile, parsed here, where the layer allows.
pub fn parse_spec(source: &[String]) -> Result<SortSpec, OrderError> {
    let mut keys = Vec::new();
    for written in source {
        let text = written.trim();
        let (path, descending) = match text.strip_prefix('-') {
            Some(rest) => (rest.trim(), true),
            None => (text, false),
        };
        if path.is_empty() {
            return Err(OrderError::Syntax {
                source: written.clone(),
                reason: "a sort key needs a field, and '-' alone is not one".to_string(),
            });
        }
        let field =
            fields::parse(path).map_err(|error| OrderError::UnknownField(Box::new(error)))?;
        keys.push(SortKey { field, descending });
    }
    SortSpec::new(keys)
}

// ---------------------------------------------------------------- ordering

/// The order, as a permutation of the input. This module reorders nothing:
/// applying it belongs to whatever owns the collection.
///
/// Either a total permutation comes back or an error does, and there is no path
/// where the call succeeds having answered nothing — which is the whole of why
/// this module exists.
pub fn sorted(subjects: &[Subject], spec: &SortSpec) -> Result<Vec<usize>, OrderError> {
    // Validation precedes extraction: a run that fails halfway has produced no
    // permutation at all, so there is no half-order for a caller to apply.
    let extracted = extract(subjects, spec)?;
    let mut order: Vec<usize> = (0..subjects.len()).collect();
    order.sort_by(|a, b| compare_extracted(&extracted[*a], &extracted[*b], spec));
    Ok(order)
}

/// Two subjects under one spec, for a caller that needs the comparison without
/// the sort. Resolves each time, so `sorted` is what a collection uses.
pub fn compare(a: &Subject, b: &Subject, spec: &SortSpec) -> Result<Ordering, OrderError> {
    let left = keys_of(a, spec)?;
    let right = keys_of(b, spec)?;
    Ok(compare_extracted(&left, &right, spec))
}

/// One subject's key values and its path, in spec order.
struct Extracted {
    values: Vec<Option<Value>>,
    path: Option<PathBuf>,
}

/// Keys are extracted once per subject, then the extractions are sorted — not
/// the subjects with resolution happening inside the comparator. A
/// comparison-time resolution runs a formula O(n log n) times instead of n, and
/// a formula that is not pure would produce an ordering that is silently wrong.
fn extract(subjects: &[Subject], spec: &SortSpec) -> Result<Vec<Extracted>, OrderError> {
    let mut extracted = Vec::with_capacity(subjects.len());
    for subject in subjects {
        extracted.push(keys_of(subject, spec)?);
    }
    Ok(extracted)
}

fn keys_of(subject: &Subject, spec: &SortSpec) -> Result<Extracted, OrderError> {
    let mut values = Vec::with_capacity(spec.keys.len());
    for key in &spec.keys {
        values.push(key_value(&key.field, subject)?);
    }
    Ok(Extracted {
        values,
        path: subject.path.map(Path::to_path_buf),
    })
}

fn key_value(field: &Field, subject: &Subject) -> Result<Option<Value>, OrderError> {
    // `state` is a set of words, not a list of items: the list's advice,
    // `[#n]`, sent the reader to address an item that does not exist. Said
    // before resolving, so that a caller that read no states is told the same.
    if *field == Field::Reserved(fields::ReservedField::State) {
        return Err(OrderError::Unsortable {
            field: Box::new(field.clone()),
            reason: "a sample can hold several states, which have no order — \
                     select by one, or show them as a column"
                .to_string(),
        });
    }
    match fields::resolve(field, subject) {
        // A value not applicable sorts as none does, last in both directions.
        Ok(Resolution::Scalar(Some(Value::NotApplicable))) => Ok(None),
        Ok(Resolution::Scalar(value)) => Ok(value),
        // A tag set has no order. Sorting by `tags` is a mistake, and saying so
        // beats inventing a rule nobody asked for.
        Ok(Resolution::Tags(_)) => Err(OrderError::Unsortable {
            field: Box::new(field.clone()),
            reason: "'tags' holds several identifiers and has no order".to_string(),
        }),
        Ok(Resolution::List(_)) => Err(OrderError::Unsortable {
            field: Box::new(field.clone()),
            reason: "a list attribute has no whole-list order; address one item with [#n]"
                .to_string(),
        }),
        Err(FieldError::Compute { field, reason }) => Err(OrderError::Compute {
            field,
            subject: subject.path.map(Path::to_path_buf),
            reason,
        }),
        Err(error) => Err(OrderError::UnknownField(Box::new(error))),
    }
}

fn compare_extracted(a: &Extracted, b: &Extracted, spec: &SortSpec) -> Ordering {
    for (at, key) in spec.keys.iter().enumerate() {
        let ordering = compare_key(&a.values[at], &b.values[at], key.descending);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    // The final tie-break is the path, which is unique within a collection.
    // Without it, two samples equal on every key take the input order — which
    // is deterministic only because `discovery` made it so, and would change
    // silently if a file were renamed. A sample never written sorts last, for
    // the same reason an absent value does.
    match (&a.path, &b.path) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Absent sorts last in **both** directions, which is why this is not
/// `reverse()` on the ascending order. Reversing a sort to see the largest
/// values first should not fill the screen with blanks.
fn compare_key(a: &Option<Value>, b: &Option<Value>, descending: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        // Text ignores case, so `activated carbon` does not follow `MnO2`; the
        // written case only breaks a tie.
        (Some(Value::Text(left)), Some(Value::Text(right))) => {
            let ordering = left
                .to_lowercase()
                .cmp(&right.to_lowercase())
                .then_with(|| left.cmp(right));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
        (Some(left), Some(right)) => {
            let ordering = value::total_order(left, right);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
    }
}

// ------------------------------------------------------------------ errors

#[derive(Debug, Clone, PartialEq)]
pub enum OrderError {
    EmptySpec,
    UnknownField(Box<FieldError>),
    /// Names the subject as well as the field: one failing formula among two
    /// hundred samples is otherwise a search.
    Compute {
        field: String,
        subject: Option<PathBuf>,
        reason: String,
    },
    /// A field that resolves but has no order.
    Unsortable {
        field: Box<Field>,
        reason: String,
    },
    Syntax {
        source: String,
        reason: String,
    },
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderError::EmptySpec => write!(
                f,
                "a sort needs at least one key: a spec with none is a request \
                 that cannot be honored"
            ),
            OrderError::UnknownField(error) => write!(f, "{error}"),
            OrderError::Compute {
                field,
                subject,
                reason,
            } => match subject {
                Some(path) => write!(
                    f,
                    "sorting by '{field}' ran a formula that failed on {}: {reason}",
                    path.display()
                ),
                None => write!(
                    f,
                    "sorting by '{field}' ran a formula that failed on a sample \
                     that has no path: {reason}"
                ),
            },
            OrderError::Unsortable { field, reason } => write!(
                f,
                "'{}' cannot be sorted by: {reason}",
                fields::describe(field.as_ref())
            ),
            OrderError::Syntax { source, reason } => {
                write!(f, "'{source}' is not a sort key: {reason}")
            }
        }
    }
}

impl std::error::Error for OrderError {}
