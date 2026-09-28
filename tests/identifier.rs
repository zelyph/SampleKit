//! The tests of `identifier`.

use samplekit::core::identifier::{self, Identifier, IdentifierError};
use samplekit::core::value::Value;

#[test]
fn accepts_ordinary_names() {
    for name in ["malt", "carbonation_level", "_internal", "T2", "a", "_"] {
        let identifier = Identifier::new(name)
            .unwrap_or_else(|error| panic!("'{name}' should be a valid name: {error}"));
        assert_eq!(identifier.as_str(), name);
    }
}

#[test]
fn accepts_unicode_letters() {
    for name in ["\u{e9}t\u{e9}_max", "bi\u{e8}re", "\u{3b2}"] {
        let identifier = Identifier::new(name)
            .unwrap_or_else(|error| panic!("'{name}' should be a valid name: {error}"));
        assert_eq!(identifier.as_str(), name);
        // And a Unicode name is still reachable as a Python attribute.
        assert!(identifier.is_python_attribute());
    }
}

#[test]
fn rejects_empty() {
    assert_eq!(Identifier::new(""), Err(IdentifierError::Empty));
    assert_eq!(Identifier::new("   "), Err(IdentifierError::Empty));
    assert_eq!(Identifier::new("\t"), Err(IdentifierError::Empty));
}

#[test]
fn rejects_leading_digit() {
    // Indistinguishable from a number in a filter expression.
    let error = Identifier::new("2theta").unwrap_err();
    assert_eq!(
        error,
        IdentifierError::LeadingDigit {
            name: "2theta".to_string()
        }
    );
    // A digit anywhere else is fine.
    assert!(Identifier::new("theta2").is_ok());
}

#[test]
fn rejects_dot() {
    let error = Identifier::new("sample.malt").unwrap_err();
    assert_eq!(
        error,
        IdentifierError::ReservedCharacter {
            name: "sample.malt".to_string(),
            character: '.',
            position: 6,
        }
    );
    // The message explains what a dot *means*, which is why the variant is
    // separate from `DisallowedCharacter`.
    let message = error.to_string();
    assert!(message.contains("malt.u"), "{message}");
}

#[test]
fn rejects_brackets() {
    for (name, character, position) in [("q[1]", '[', 1), ("q1]", ']', 2)] {
        let error = Identifier::new(name).unwrap_err();
        assert_eq!(
            error,
            IdentifierError::ReservedCharacter {
                name: name.to_string(),
                character,
                position,
            }
        );
        // And the message explains what they delimit.
        let message = error.to_string();
        assert!(message.contains("fermentation.gravity[3]"), "{message}");
    }
}

#[test]
fn rejects_whitespace() {
    // Space and tab, at any position.
    assert_eq!(
        Identifier::new("quality factor"),
        Err(IdentifierError::Whitespace {
            name: "quality factor".to_string(),
            position: 7,
        })
    );
    assert_eq!(
        Identifier::new(" malt"),
        Err(IdentifierError::Whitespace {
            name: " malt".to_string(),
            position: 0,
        })
    );
    assert_eq!(
        Identifier::new("malt\tg"),
        Err(IdentifierError::Whitespace {
            name: "malt\tg".to_string(),
            position: 4,
        })
    );
}

#[test]
fn rejects_a_hyphen_as_disallowed_not_reserved() {
    // A hyphen means nothing, so the message states the rule instead of
    // claiming a meaning. One message could not serve both cases: an author
    // who tried `sample.malt` and then `sample-malt` must not be refused twice
    // by the same sentence.
    let hyphen = Identifier::new("sample-malt").unwrap_err();
    assert_eq!(
        hyphen,
        IdentifierError::DisallowedCharacter {
            name: "sample-malt".to_string(),
            character: '-',
            position: 6,
        }
    );
    let dot = Identifier::new("sample.malt").unwrap_err();
    assert_ne!(hyphen.to_string(), dot.to_string());
    // And it suggests something that would work.
    assert!(hyphen.to_string().contains("sample_malt"), "{hyphen}");
}

#[test]
fn a_comma_is_named_as_a_list_separator() {
    // `-s energy,period` once suggested `energy_period`: the usual repair glued
    // two names into one that exists nowhere.
    let comma = Identifier::new("energy,period").unwrap_err();
    assert_eq!(
        comma,
        IdentifierError::DisallowedCharacter {
            name: "energy,period".to_string(),
            character: ',',
            position: 6,
        }
    );
    let message = comma.to_string();
    assert!(message.contains("separates names"), "{message}");
    assert!(!message.contains("energy_period"), "{message}");
}

#[test]
fn rejects_an_at_sign() {
    // Freed from delimiting an index and still not a name character.
    assert_eq!(
        Identifier::new("mashing@293"),
        Err(IdentifierError::DisallowedCharacter {
            name: "mashing@293".to_string(),
            character: '@',
            position: 7,
        })
    );
    for name in ["a/b", "a+b", "a$b"] {
        assert!(
            matches!(
                Identifier::new(name),
                Err(IdentifierError::DisallowedCharacter { .. })
            ),
            "'{name}' should be disallowed, not reserved"
        );
    }
}

#[test]
fn error_reports_character_position() {
    let error = Identifier::new("quality-factor").unwrap_err();
    let IdentifierError::DisallowedCharacter { position, .. } = error else {
        panic!("expected a disallowed character, got {error:?}");
    };
    // Counted from 0 in the error, from 1 where it is written: the eighth
    // character.
    assert_eq!(position, 7);
    assert!(error.to_string().contains("at character 8"), "{error}");
}

#[test]
fn python_keyword_is_valid_but_not_an_attribute() {
    // `class` is a perfectly good property name; it is only attribute access
    // that cannot reach it, and `sample["class"]` always works.
    let keyword = Identifier::new("class").unwrap();
    assert!(!keyword.is_python_attribute());
    assert!(!Identifier::new("None").unwrap().is_python_attribute());
    assert!(!Identifier::new("lambda").unwrap().is_python_attribute());
    // Soft keywords are ordinary identifiers: `sample.match` is legal Python.
    assert!(Identifier::new("match").unwrap().is_python_attribute());
    assert!(Identifier::new("type").unwrap().is_python_attribute());
    assert!(Identifier::new("malt").unwrap().is_python_attribute());
}

#[test]
fn index_values_accept_whitespace() {
    // Index values are data, not names, and are deliberately unrestricted.
    // Names are chosen by whoever designs the data; index values describe the
    // physical world and cannot be constrained.
    assert!(Identifier::new("stage 2").is_err());
    let index = Value::text("stage 2");
    assert_eq!(index, Value::text("stage 2"));
    assert!(matches!(index, Value::Text(_)));
}

// ------------------------------------------------------------- suggestions

#[test]
fn nearest_offers_a_close_candidate() {
    let names = ["malt", "plato", "volume", "foam"];
    assert_eq!(
        identifier::nearest("plto", names),
        Some("plato".to_string())
    );
    assert_eq!(identifier::nearest("foa", names), Some("foam".to_string()));
}

#[test]
fn nearest_offers_nothing_when_nothing_is_close() {
    // The least bad candidate sends the reader somewhere else, which is worse
    // than saying nothing: the author concludes the name does not exist.
    let columns = ["temperature", "ph", "srm", "wort"];
    assert_eq!(identifier::nearest("carbonation_level", columns), None);
    // A very short name gets no suggestion at all: within a third of two
    // characters, nothing is near enough, and one letter for another is not a
    // typo anybody can identify.
    assert_eq!(identifier::nearest("R", ["ph", "T"]), None);
    // And a tie breaks by name rather than by iteration order.
    assert_eq!(
        identifier::nearest("fermentaton", ["fermentations", "fermentation"]),
        Some("fermentation".to_string())
    );
}

#[test]
fn a_quote_in_a_name_says_fields_are_unquoted() {
    let error = Identifier::new("\"conditioning").unwrap_err();
    assert!(error.to_string().contains("without quotes"), "{error}");
}
