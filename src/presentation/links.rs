//! A link as a reader sees it, as Markdown shows one: its text alone, marked
//! where it leaves the project or leads nowhere. What is written stays as
//! written; this is how the TUI and a table drawn in a terminal show it.

use std::path::{Path, PathBuf};

use crate::core::link::{self, Link, Resolved};
use crate::core::sample::AttributeValue;
use crate::core::value::Value;

/// Where a link shown leads, which its mark and its colour say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leads {
    /// A sample of the project, or a file: no mark.
    Inside,
    /// A web page: `↗`.
    Web,
    /// No sample, several, or no file: `✗`.
    Nowhere,
}

/// A link shown: its text, its mark after it where it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub leads: Leads,
}

/// `text` shown as the link it is, held by the sample file `from`, a wikilink
/// found among `samples`, each its path and its `name:`; `None` where the
/// text is no link.
pub fn shown(text: &str, from: &Path, samples: &[(PathBuf, Option<String>)]) -> Option<Shown> {
    let parsed = link::parse(text)?;
    let leads = match &parsed {
        Link::Url { .. } => Leads::Web,
        Link::Sample { target, .. } => match link::resolve(
            target,
            samples
                .iter()
                .map(|(path, name)| (path.as_path(), name.as_deref())),
        ) {
            Resolved::One(_) => Leads::Inside,
            // A file of the project by its whole name, `[[curve.csv]]`.
            Resolved::Nowhere if crate::config::discovery::names_a_file(from, target) => {
                Leads::Inside
            }
            _ => Leads::Nowhere,
        },
        Link::Path { path, .. } => {
            let folder = from.parent().unwrap_or(Path::new("."));
            if link::path_from(folder, path).exists() {
                Leads::Inside
            } else {
                Leads::Nowhere
            }
        }
    };
    let mark = match leads {
        Leads::Inside => "",
        Leads::Web => " ↗",
        Leads::Nowhere => " ✗",
    };
    Some(Shown {
        text: format!("{}{mark}", parsed.shown()),
        leads,
    })
}

/// An attribute shown with its links: one link as [`shown`] says it; a list
/// holding links, its items joined by commas, each link shown, and leading
/// nowhere where any of them does. `None` where it holds no link.
pub fn attribute(
    value: &AttributeValue,
    from: &Path,
    samples: &[(PathBuf, Option<String>)],
) -> Option<Shown> {
    fn text_of(value: &Value) -> Option<&str> {
        match value {
            Value::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }
    match value {
        AttributeValue::Scalar(value) => shown(text_of(value)?, from, samples),
        AttributeValue::List(items) => {
            let each: Vec<Option<Shown>> = items
                .iter()
                .map(|item| text_of(item).and_then(|text| shown(text, from, samples)))
                .collect();
            if each.iter().all(Option::is_none) {
                return None;
            }
            let leads = if each.iter().any(|item| {
                item.as_ref()
                    .is_some_and(|item| item.leads == Leads::Nowhere)
            }) {
                Leads::Nowhere
            } else if each
                .iter()
                .all(|item| item.as_ref().is_some_and(|item| item.leads == Leads::Web))
            {
                Leads::Web
            } else {
                Leads::Inside
            };
            let text = items
                .iter()
                .zip(each)
                .map(|(item, shown)| match shown {
                    Some(shown) => shown.text,
                    None => crate::collection::editing::shown_value(item),
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some(Shown { text, leads })
        }
    }
}

/// A table's row, each column a link as [`attribute`] shows it where the
/// column is an attribute holding one, `None` elsewhere.
pub fn row(
    profile: &crate::config::project_config::Profile,
    subject: &crate::query::field_addressing::Subject,
    samples: &[(PathBuf, Option<String>)],
) -> Vec<Option<Shown>> {
    use crate::query::field_addressing::{self as fields, Channel, Field};
    profile
        .columns()
        .iter()
        .map(|column| {
            let from = subject.path?;
            match fields::parse(&column.field).ok()? {
                Field::Named {
                    name,
                    channel: Channel::Value,
                    ..
                } if subject.sample.has_attribute(&name) => {
                    attribute(subject.sample.attribute_if(&name)?, from, samples)
                }
                _ => None,
            }
        })
        .collect()
}
