//! The grammar of names: what may be called a property, a table or a column.
//!
//! It exists so that a name chosen when data is entered cannot make that data
//! unaddressable later.
//!

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error as DeError};

/// The name of a property, a table or a column.
///
/// Validated once at construction so that every later layer — YAML keys, filter
/// fields, CLI arguments, Python attributes, export headers — can assume it is
/// well formed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier(String);

/// A name starts with a letter or an underscore.
fn is_leading(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_start(c)
}

/// And continues with letters, digits and underscores.
///
/// Narrower than `XID_Continue`, which also admits combining marks and
/// connector punctuation. The grammar says letters, digits and underscores, so
/// that is what this accepts.
fn is_trailing(c: char) -> bool {
    c == '_'
        || unicode_ident::is_xid_start(c)
        || (unicode_ident::is_xid_continue(c) && c.is_numeric())
}

/// Python's hard keywords, which cannot be attribute names.
///
/// The soft keywords — `match`, `case`, `type` and `_` — are deliberately
/// absent: they are ordinary identifiers, and `sample.match` is legal Python.
const PYTHON_KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

impl Identifier {
    /// Validate a name against the grammar.
    ///
    /// Every excluded character is excluded because permitting it would create
    /// a name that can be stored but not retrieved — a value that exists in a
    /// file and cannot be filtered, sorted, exported or read back. That failure
    /// would appear long after the data was entered, which is the worst
    /// possible time.
    pub fn new(name: &str) -> Result<Identifier, IdentifierError> {
        if name.trim().is_empty() {
            return Err(IdentifierError::Empty);
        }
        for (position, character) in name.chars().enumerate() {
            if character.is_whitespace() {
                return Err(IdentifierError::Whitespace {
                    name: name.to_string(),
                    position,
                });
            }
            if matches!(character, '.' | '[' | ']') {
                return Err(IdentifierError::ReservedCharacter {
                    name: name.to_string(),
                    character,
                    position,
                });
            }
            let admitted = if position == 0 {
                is_leading(character)
            } else {
                is_trailing(character)
            };
            if admitted {
                continue;
            }
            // A leading digit is its own diagnosis: the character is fine, its
            // position is not, and the fix is different.
            if position == 0 && character.is_numeric() {
                return Err(IdentifierError::LeadingDigit {
                    name: name.to_string(),
                });
            }
            return Err(IdentifierError::DisallowedCharacter {
                name: name.to_string(),
                character,
                position,
            });
        }
        Ok(Identifier(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `sample.<name>` works in Python.
    ///
    /// A valid Python identifier that is not a Python keyword — the whole of
    /// what this module can know. Whether a name also collides with a method of
    /// `Sample` or `Table` depends on what those classes carry, which is
    /// `python-api`'s to answer. Both cases end the same way: `sample["class"]`
    /// always works, and the mapping form, not this one, is the contract.
    pub fn is_python_attribute(&self) -> bool {
        let mut characters = self.0.chars();
        let Some(first) = characters.next() else {
            return false;
        };
        if !is_leading(first) {
            return false;
        }
        if !characters.all(|c| c == '_' || unicode_ident::is_xid_continue(c)) {
            return false;
        }
        !PYTHON_KEYWORDS.contains(&self.0.as_str())
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One variant per rule, each naming the offending character and its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierError {
    Empty,
    LeadingDigit {
        name: String,
    },
    /// `.`, `[` or `]`: characters that mean something in a field path.
    ReservedCharacter {
        name: String,
        character: char,
        position: usize,
    },
    /// Any other character outside the grammar: `-`, `@`, `/`, `+`.
    DisallowedCharacter {
        name: String,
        character: char,
        position: usize,
    },
    Whitespace {
        name: String,
        position: usize,
    },
}

impl Identifier {
    /// The identifier a written name is repaired to: each offending character
    /// replaced, a leading digit prefixed, as the refusal suggests. `None` when
    /// nothing is left to name.
    pub fn repaired(written: &str) -> Option<Identifier> {
        let trimmed = written.trim();
        let prefixed = match trimmed.chars().next() {
            Some(first) if first.is_numeric() => format!("n{trimmed}"),
            _ => trimmed.to_string(),
        };
        Identifier::new(&suggestion(&prefixed)).ok()
    }
}

/// What the name would be with its offending characters replaced.
///
/// The suggestion is what turns a refusal into an instruction: an author told
/// only "invalid character" tries `sample-malt` after `sample.malt` and gets
/// the same unhelpful answer twice.
fn suggestion(name: &str) -> String {
    let repaired: String = name
        .chars()
        .enumerate()
        .map(|(position, c)| {
            let admitted = if position == 0 {
                is_leading(c)
            } else {
                is_trailing(c)
            };
            if admitted { c } else { '_' }
        })
        .collect();
    repaired
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentifierError::Empty => write!(
                f,
                "a name cannot be empty: an unnamed property is not addressable"
            ),
            IdentifierError::LeadingDigit { name } => write!(
                f,
                "'{name}' starts with a digit, and a name that starts with a digit cannot \
                 be told from a number in a filter expression. Try 'n{name}'"
            ),
            // States what the character *means*, not only that it is forbidden.
            IdentifierError::ReservedCharacter {
                name,
                character,
                position,
            } => {
                let meaning = match character {
                    '.' => "'.' separates a name from its channel, as in malt.u",
                    _ => {
                        "'[' and ']' delimit a column's index value, as in \
                          fermentation.gravity[3]"
                    }
                };
                write!(
                    f,
                    "'{name}' contains '{character}' at character {}, and {meaning}. \
                     Try '{}'",
                    position + 1,
                    suggestion(name)
                )
            }
            // A comma means something on the command line, and the usual repair
            // would join two names into one that exists nowhere.
            IdentifierError::DisallowedCharacter {
                name,
                character: ',',
                position,
            } => write!(
                f,
                "'{name}' contains ',' at character {}, and ',' separates names in a \
                 list, as in -c og,fg: one name is expected here",
                position + 1
            ),
            // A quote belongs to the shell, never to a field.
            IdentifierError::DisallowedCharacter {
                name,
                character: '"' | '\'',
                ..
            } => write!(
                f,
                "'{name}' contains a quote: a field is written without quotes, which only the \
                 shell needs, as -c 'fermentation.gravity[3]'"
            ),
            // Cannot say what the character means — a hyphen means nothing — so
            // it states the rule instead.
            IdentifierError::DisallowedCharacter {
                name,
                character,
                position,
            } => write!(
                f,
                "'{name}' contains '{character}' at character {}: a name is letters, \
                 digits and underscores. Try '{}'",
                position + 1,
                suggestion(name)
            ),
            IdentifierError::Whitespace { name, position } => write!(
                f,
                "'{name}' contains whitespace at character {}, which would need \
                 quoting in every CLI argument and filter expression. Try '{}'",
                position + 1,
                suggestion(name)
            ),
        }
    }
}

impl std::error::Error for IdentifierError {}

/// A name read from a file goes through `Identifier::new`, so a key that is not
/// a name fails at load with the grammar in its message.
///
/// There is deliberately no `Serialize`: writing belongs to `canonicalization`,
/// and a type serde cannot write is that rule made structural.
impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Identifier, D::Error> {
        let text = String::deserialize(deserializer)?;
        Identifier::new(&text).map_err(DeError::custom)
    }
}

/// The nearest of a set of candidates, or nothing when nothing is close.
///
/// Levenshtein distance, offered only within **a third of the written name's
/// length**: `plto` suggests `plato`, and `carbonation_level` suggests nothing
/// among `temperature, ph, srm, wort`. A suggestion that is not nearly
/// right sends the reader somewhere else, which is worse than none.
///
/// It lives here because three modules had written it independently — two with
/// one threshold and one with another — and a threshold that differs by module
/// means the same typo is corrected in one message and not in the next.
pub fn nearest<'a>(written: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let budget = written.chars().count().max(1);
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(candidate, written), candidate))
        .filter(|(distance, _)| distance * 3 <= budget)
        // Ties break by name, so that two equally near candidates do not
        // depend on iteration order.
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, candidate)| candidate.to_string())
}

/// Levenshtein, by `strsim`, which clap already builds for its own suggestions.
pub fn edit_distance(left: &str, right: &str) -> usize {
    strsim::levenshtein(left, right)
}
