//! The tests of `formatting`.

use samplekit::core::formatting::{
    FormatError, Precision, Presentation, Resolved, Spec, format_quantity, format_uncertainty,
    format_value,
};
use samplekit::core::uncertainty::Uncertainty;
use samplekit::core::value::{Date, DateTime, Value};

fn plain(precision: Option<Precision>, unit: Option<&str>) -> Resolved {
    Resolved::plain(&Presentation {
        unit: unit.map(str::to_string),
        symbol: None,
        precision,
    })
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn uncertainty(x: f64) -> Uncertainty {
    Uncertainty::new(x).unwrap()
}

// --------------------------------------------------------------- declaration

#[test]
fn declared_precision_is_honored() {
    // Three decimals regardless of what the default convention would have said.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), None);
    assert_eq!(format_value(&number(12.5), &resolved), "12.500");
    assert_eq!(
        format_quantity(&number(12.4987), Some(&uncertainty(0.0523)), &resolved),
        "12.499 \u{b1} 0.052"
    );
}

#[test]
fn one_specifier_covers_value_and_uncertainty() {
    // One declaration, both numbers, and no second field to forget.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    assert_eq!(
        format_quantity(&number(12.5), Some(&uncertainty(0.05)), &resolved),
        "12.500 \u{b1} 0.050 g"
    );
}

#[test]
fn a_pair_separates_them_deliberately() {
    // Reachable, and reaching it is a decision rather than an oversight.
    let resolved = plain(Some(Precision::split(".3f", ".1f").unwrap()), None);
    assert_eq!(
        format_quantity(&number(12.5), Some(&uncertainty(0.1)), &resolved),
        "12.500 \u{b1} 0.1"
    );
}

#[test]
fn uniform_precision_reports_itself_uniform() {
    // What lets a round trip write one specifier instead of two.
    assert!(Precision::both(".3f").unwrap().is_uniform());
    assert!(Precision::split(".3f", ".3f").unwrap().is_uniform());
    assert!(!Precision::split(".3f", ".1f").unwrap().is_uniform());
}

// ------------------------------------------------------------ the convention

#[test]
fn default_rounds_uncertainty_to_two_significant_digits() {
    let resolved = plain(None, None);
    assert_eq!(
        format_quantity(&number(12.4987), Some(&uncertainty(0.0523)), &resolved),
        "12.499 \u{b1} 0.052"
    );
}

#[test]
fn default_aligns_value_to_uncertainty() {
    // Both at the same decimal place, which is what makes the pair readable.
    let resolved = plain(None, None);
    assert_eq!(
        format_quantity(&number(1284.3), Some(&uncertainty(27.0)), &resolved),
        "1284 \u{b1} 27"
    );
}

#[test]
fn default_without_uncertainty_uses_general_form() {
    // Not sixteen digits, which would claim a precision the measurement does
    // not support.
    let resolved = plain(None, None);
    assert_eq!(
        format_value(&number(12.498700000000001), &resolved),
        "12.4987"
    );
    assert_eq!(
        format_quantity(&number(0.000012345), None, &resolved),
        "1.2345e-05"
    );
    assert_eq!(
        format_quantity(&number(1234567.0), None, &resolved),
        "1.23457e+06"
    );
}

#[test]
fn project_default_applies_where_nothing_is_declared() {
    // Resolution happens at Layer 3; this module sees one Option<Precision>
    // and never learns where it came from. What it must honour is that a
    // precision it is handed applies even when the presentation declares none,
    // and that a declared one is what `plain` carries down.
    let project_default = Resolved {
        unit: None,
        symbol: None,
        separator: "\u{b1}".to_string(),
        precision: Some(Precision::both(".2f").unwrap()),
    };
    assert_eq!(format_value(&number(12.4987), &project_default), "12.50");

    let declared_on_the_property = plain(Some(Precision::both(".4f").unwrap()), None);
    assert_eq!(
        format_value(&number(12.4987), &declared_on_the_property),
        "12.4987"
    );
}

// ------------------------------------------------------------------- layout

#[test]
fn unit_follows_the_whole_quantity() {
    // `12.50 g +- 0.05` would invite the reader to wonder about the second
    // number's unit.
    let resolved = plain(Some(Precision::both(".2f").unwrap()), Some("g"));
    let rendered = format_quantity(&number(12.5), Some(&uncertainty(0.05)), &resolved);
    assert_eq!(rendered, "12.50 \u{b1} 0.05 g");
    assert!(!rendered.contains("g \u{b1}"), "{rendered}");
}

#[test]
fn resolved_literals_are_used_verbatim() {
    // If this ever needs a project to pass, the lookup has leaked downward.
    let resolved = Resolved {
        unit: Some("\\Omega".to_string()),
        symbol: Some("R".to_string()),
        separator: "\\pm".to_string(),
        precision: Some(Precision::both(".3f").unwrap()),
    };
    assert_eq!(
        format_quantity(&number(12.5), Some(&uncertainty(0.05)), &resolved),
        "12.500 \\pm 0.050 \\Omega"
    );
}

#[test]
fn plain_resolution_uses_the_stored_strings() {
    // A pure Rust reader with no project still renders something exact.
    let presentation = Presentation {
        unit: Some("lintner".to_string()),
        symbol: Some("R".to_string()),
        precision: Some(Precision::both(".3f").unwrap()),
    };
    let resolved = Resolved::plain(&presentation);
    assert_eq!(resolved.unit.as_deref(), Some("lintner"));
    assert_eq!(resolved.separator, "\u{b1}");
    assert_eq!(
        format_quantity(&number(12.5), Some(&uncertainty(0.05)), &resolved),
        "12.500 \u{b1} 0.050 lintner"
    );
}

// -------------------------------------------------------------------- kinds

#[test]
fn absent_renders_as_empty_string() {
    // Not 0, not None, not a dash: the caller decides on a placeholder.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    assert_eq!(format_value(&Value::absent(), &resolved), "");
    assert_eq!(format_quantity(&Value::absent(), None, &resolved), "");
    assert_eq!(
        format_quantity(&Value::absent(), Some(&uncertainty(0.05)), &resolved),
        ""
    );
}

#[test]
fn text_renders_verbatim() {
    // Unaffected by precision and unit metadata, and carrying neither.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    assert_eq!(
        format_value(&Value::text("dried keg"), &resolved),
        "dried keg"
    );
    assert_eq!(
        format_quantity(
            &Value::text("dried keg"),
            Some(&uncertainty(0.05)),
            &resolved
        ),
        "dried keg"
    );
}

#[test]
fn an_integer_renders_without_a_decimal_point() {
    // `3`, never `3.0`, until a precision says otherwise.
    let bare = plain(None, None);
    assert_eq!(format_value(&Value::integer(3), &bare), "3");
    assert_eq!(format_quantity(&Value::integer(3), None, &bare), "3");
    let declared = plain(Some(Precision::both(".3f").unwrap()), None);
    assert_eq!(format_value(&Value::integer(3), &declared), "3.000");
    // And `d` keeps a large integer exact rather than routing it through f64.
    let whole = plain(Some(Precision::both("d").unwrap()), None);
    assert_eq!(
        format_value(&Value::integer(9_007_199_254_740_993), &whole),
        "9007199254740993"
    );
}

#[test]
fn a_date_renders_as_it_is_stored() {
    // The file and the screen show the same text.
    let date = Value::date(Date::new(2026, 3, 14).unwrap());
    let resolved = plain(None, None);
    assert_eq!(format_value(&date, &resolved), "2026-03-14");
    assert_eq!(format_quantity(&date, None, &resolved), "2026-03-14");
}

#[test]
fn a_date_time_renders_as_its_canonical_storage_form() {
    let date_time = Value::date_time(DateTime::parse("2026-03-14 10:30").unwrap());
    assert_eq!(
        format_value(&date_time, &plain(None, None)),
        "2026-03-14T10:30"
    );
}

#[test]
fn a_boolean_renders_as_true_or_false() {
    let resolved = plain(None, None);
    assert_eq!(format_value(&Value::boolean(true), &resolved), "true");
    assert_eq!(format_value(&Value::boolean(false), &resolved), "false");
}

#[test]
fn a_precision_on_a_non_numeric_kind_is_ignored() {
    // Ignored, not refused: this module cannot fail at render time, and a
    // table cell is the worst place to learn that a declaration was wrong.
    // Checking a declaration against what it describes is `cli validate`'s.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    let date = Value::date(Date::new(2026, 3, 14).unwrap());
    assert_eq!(format_quantity(&date, None, &resolved), "2026-03-14");
    assert_eq!(
        format_quantity(&Value::boolean(true), None, &resolved),
        "true"
    );
    assert_eq!(format_quantity(&Value::text("x"), None, &resolved), "x");
}

// ------------------------------------------------------------------ grammar

#[test]
fn the_subset_is_exactly_the_grammar() {
    for accepted in [".3f", ".2e", ".2E", ".4G", "g", "d", "f", ".0f", ".12g"] {
        assert!(
            Spec::new(accepted).is_ok(),
            "'{accepted}' should parse: {:?}",
            Spec::new(accepted)
        );
    }
    for refused in [
        ".2q", "%", "08.2f", ",", "", ".f", ".", "3f", ".3d", "ff", ">10.2f", "+.2f", ".2%", "_",
    ] {
        assert!(Spec::new(refused).is_err(), "'{refused}' should be refused");
    }
}

#[test]
fn unsupported_specifier_is_not_silently_ignored() {
    // v1 answered `.2q` by printing sixteen digits, which looks like a
    // precision choice and is not one.
    assert!(matches!(
        Spec::new(".2q"),
        Err(FormatError::InvalidPrecision { .. })
    ));
}

#[test]
fn invalid_precision_is_rejected_at_construction() {
    // The error names the specifier, and does not surface at render time.
    let error = Precision::both(".2q").unwrap_err();
    let FormatError::InvalidPrecision { specifier, reason } = &error;
    assert_eq!(specifier, ".2q");
    assert!(reason.contains('q'), "{reason}");
    assert!(error.to_string().contains(".2q"), "{error}");
    // A pair fails the same way, on either half.
    assert!(Precision::split(".3f", ".2q").is_err());
    assert!(Precision::split(".2q", ".3f").is_err());
}

// ----------------------------------------------------------------- immutable

#[test]
fn formatting_does_not_mutate_the_value() {
    // Rounding for display never modifies what is stored.
    let value = number(12.498700000000001);
    let before = match &value {
        Value::Number(x) => x.to_bits(),
        _ => unreachable!(),
    };
    let resolved = plain(Some(Precision::both(".2f").unwrap()), Some("g"));
    let _ = format_value(&value, &resolved);
    let _ = format_quantity(&value, Some(&uncertainty(0.05)), &resolved);
    let after = match &value {
        Value::Number(x) => x.to_bits(),
        _ => unreachable!(),
    };
    assert_eq!(before, after);
}

// ------------------------------------------------------------------- display

#[test]
fn the_declared_form_never_writes_a_bound() {
    // Files, exports, templates and screens alike keep the declaration exactly.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    assert_eq!(
        format_quantity(&number(3.47326), Some(&uncertainty(0.00006)), &resolved),
        "3.473 \u{b1} 0.000 g"
    );
    // With nothing declared, the convention never rounds an uncertainty away.
    let convention = plain(None, None);
    assert_eq!(
        format_quantity(&number(3.47326), Some(&uncertainty(0.00006)), &convention),
        "3.473260 \u{b1} 0.000060"
    );
}

#[test]
fn a_declared_precision_rounds_alike_everywhere() {
    // One rounding, the one declared. A screen that rescued what the precision
    // rounds away showed a number no file held.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), None);
    assert_eq!(format_value(&number(3e-5), &resolved), "0.000");
    assert_eq!(format_value(&number(-3e-5), &resolved), "-0.000");
    assert_eq!(format_value(&number(0.0), &resolved), "0.000");
    assert_eq!(
        format_uncertainty(&uncertainty(0.00006), &resolved),
        "0.000"
    );
    // The uncertainty's own specifier decides for it, and an exponential one
    // keeps a significant digit of any nonzero number.
    let split = plain(Some(Precision::split(".3f", ".5f").unwrap()), None);
    assert_eq!(
        format_quantity(&number(3.47326), Some(&uncertainty(0.00006)), &split),
        "3.473 \u{b1} 0.00006"
    );
    let exponential = plain(Some(Precision::both(".1e").unwrap()), None);
    assert_eq!(
        format_uncertainty(&uncertainty(0.00006), &exponential),
        "6.0e-05"
    );
}

#[test]
fn a_precision_past_what_a_number_carries_is_refused_when_read() {
    // `.65536f` in a sample file reached the formatter, whose limit is lower
    // than the count's type, and panicked every command that drew the sample.
    // Refused where every other flaw in a precision is: when it is read.
    assert!(Precision::both(".99f").is_ok());
    for written in [".100f", ".65536f", ".99999999999999999999f"] {
        let refused = Precision::both(written).unwrap_err();
        assert!(refused.to_string().contains("at most 99"), "{refused}");
    }
    // A precision is declared in the project, so that is where it is read and
    // refused.
    let directory = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("sk-precision-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let rc = directory.join(".samplekitrc");
    std::fs::write(
        &rc,
        "schema_version = 1\n[property.a]\nprecision = \".99999f\"\n",
    )
    .unwrap();
    let refused = samplekit::config::project_config::load(&rc);
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(refused.is_err());
}
