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
    // Three decimals regardless of what the default convention would have said;
    // the uncertainty rounded up, never understated.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), None);
    assert_eq!(format_value(&number(12.5), &resolved), "12.500");
    assert_eq!(
        format_quantity(&number(12.4987), Some(&uncertainty(0.0523)), &resolved),
        "12.499 \u{b1} 0.053"
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
        "12.499 \u{b1} 0.053"
    );
    // An uncertainty is rounded up, never understated: 0.0520 stays, and the
    // arithmetic's last digits are no measurement, 0.1 + 0.2 is 0.30.
    assert_eq!(
        format_quantity(&number(12.4987), Some(&uncertainty(0.052)), &resolved),
        "12.499 \u{b1} 0.052"
    );
    assert_eq!(
        format_quantity(&number(1.0), Some(&uncertainty(0.1 + 0.2)), &resolved),
        "1.00 \u{b1} 0.30"
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
        property: None,
        uncertainty: None,
        unitless: None,
        unit_alone: None,
        unit_in_form: None,
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
        property: Some("{value} \\pm {uncertainty} {unit}".to_string()),
        uncertainty: None,
        unitless: None,
        unit_alone: None,
        unit_in_form: None,
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
    assert!(resolved.property.is_none());
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
        format_value(&Value::text("dried billet"), &resolved),
        "dried billet"
    );
    assert_eq!(
        format_quantity(
            &Value::text("dried billet"),
            Some(&uncertainty(0.05)),
            &resolved
        ),
        "dried billet"
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
    // Files, exports, templates and screens alike keep the declaration exactly:
    // an uncertainty below its last place is rounded up to it, never written
    // as zero, and an exact zero stays one.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), Some("g"));
    assert_eq!(
        format_quantity(&number(3.47326), Some(&uncertainty(0.00006)), &resolved),
        "3.473 \u{b1} 0.001 g"
    );
    assert_eq!(
        format_quantity(&number(3.47326), Some(&uncertainty(0.0)), &resolved),
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
    assert_eq!(format_value(&number(-3e-5), &resolved), "0.000");
    assert_eq!(format_value(&number(0.0), &resolved), "0.000");
    assert_eq!(
        format_uncertainty(&uncertainty(0.00006), &resolved),
        "0.001"
    );
    // An uncertainty in a column of its own is rounded up too.
    let alone = plain(Some(Precision::both(".3f").unwrap().of_uncertainty()), None);
    assert_eq!(format_value(&number(0.00006), &alone), "0.001");
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

#[test]
fn a_quantity_past_what_a_double_holds_is_written_in_scientific_notation() {
    // Two significant figures of the uncertainty, the value to the same place:
    // written whole, 6.02e23 ± 3e20 read 601999999999999995805696, digits no
    // measurement gave.
    let resolved = plain(None, None);
    assert_eq!(
        format_quantity(&number(6.02e23), Some(&uncertainty(3e20)), &resolved),
        "6.0200e+23 \u{b1} 3.0e+20"
    );
    assert_eq!(
        format_quantity(&number(123456.0), Some(&uncertainty(250.0)), &resolved),
        "123460 \u{b1} 250"
    );
}

#[test]
fn rounding_is_half_up_on_the_shortest_decimal_writing() {
    // Two results equal in decimal are written alike: 6.9125 is 6.913, whether
    // the binary value falls just below it or just above.
    let resolved = plain(Some(Precision::both(".3f").unwrap()), None);
    for x in [
        6.9125,
        13.825 / 2.0,
        6.9125_f64.next_up(),
        1.0005,
        2.0005,
        0.0005,
    ] {
        let decimal: f64 = format!("{x}").parse().unwrap();
        assert_eq!(
            format_value(&number(x), &resolved),
            format_value(&number(decimal), &resolved),
            "{x}"
        );
    }
    assert_eq!(format_value(&number(6.9125), &resolved), "6.913");
    assert_eq!(format_value(&number(1.0005), &resolved), "1.001");
    assert_eq!(format_value(&number(-1.0005), &resolved), "-1.001");
    assert_eq!(format_value(&number(0.0005), &resolved), "0.001");
    assert_eq!(format_value(&number(0.0004), &resolved), "0.000");
    assert_eq!(format_value(&number(9.9995), &resolved), "10.000");
    // A number past the next place is no half: 0.00006 at three decimals.
    assert_eq!(format_value(&number(0.00006), &resolved), "0.000");
    // An uncertainty is rounded up, never understated, on the same writing;
    // the arithmetic's digits past the fifteenth are dropped first.
    let two = plain(Some(Precision::both(".2f").unwrap()), None);
    assert_eq!(format_uncertainty(&uncertainty(0.121), &two), "0.13");
    assert_eq!(format_uncertainty(&uncertainty(0.125), &two), "0.13");
    assert_eq!(format_uncertainty(&uncertainty(0.025), &two), "0.03");
    assert_eq!(format_uncertainty(&uncertainty(0.12), &two), "0.12");
    assert_eq!(format_uncertainty(&uncertainty(0.1 + 0.2), &two), "0.30");
    assert_eq!(format_uncertainty(&uncertainty(0.0), &two), "0.00");
    assert_eq!(format_uncertainty(&uncertainty(0.991), &two), "1.00");
    // Scientific and general forms round the same way.
    let scientific = plain(Some(Precision::both(".2e").unwrap()), None);
    assert_eq!(format_value(&number(1.2345e-5), &scientific), "1.23e-05");
    assert_eq!(format_value(&number(1.2355e-5), &scientific), "1.24e-05");
    assert_eq!(format_value(&number(9.995e3), &scientific), "1.00e+04");
    let general = plain(Some(Precision::both(".3g").unwrap()), None);
    assert_eq!(format_value(&number(6.9125), &general), "6.91");
    assert_eq!(format_value(&number(0.0012345), &general), "0.00123");
    assert_eq!(format_value(&number(1234500.0), &general), "1.23e+06");
}

#[test]
fn every_digit_of_an_uncertainty_is_kept_where_the_precision_keeps_them() {
    // The arithmetic's digits are dropped before rounding up only where the
    // precision cuts among them: `.17g` wrote 0.8787328442349305 as
    // 0.878732844234931.
    let every = plain(Some(Precision::both(".17g").unwrap()), None);
    assert_eq!(
        format_uncertainty(&uncertainty(0.8787328442349305), &every),
        "0.8787328442349305"
    );
    let two = plain(Some(Precision::both(".2f").unwrap()), None);
    assert_eq!(format_uncertainty(&uncertainty(0.1 + 0.2), &two), "0.30");
}

#[test]
fn a_zero_rounded_from_a_negative_number_has_no_sign() {
    // `-0.000`, as Python's `format()` writes it, read as a mistake: the sign
    // of a zero rounded is no information.
    let at = |spec: &str| plain(Some(Precision::both(spec).unwrap()), None);
    assert_eq!(format_value(&number(-3e-5), &at(".3f")), "0.000");
    assert_eq!(format_value(&number(-3e-5), &at(".0f")), "0");
    assert_eq!(format_value(&number(-3e-4), &at(".1e")), "-3.0e-04");
    assert_eq!(format_value(&number(-0.0), &plain(None, None)), "0");
    assert_eq!(format_value(&number(-0.4), &at(".0f")), "0");
    assert_eq!(format_value(&number(-0.6), &at(".0f")), "-1");
}

#[test]
fn a_property_is_written_in_named_pieces() {
    // One line for every value: the uncertainty's piece where there is one,
    // the form with no unit where there is none, no character reserved.
    use samplekit::core::formatting::{StyleForms, filled_property};
    let forms = StyleForms {
        property: Some("\\SI{{value}{uncertainty}}{{unit}}"),
        uncertainty: Some(" \\pm {uncertainty}"),
        unitless: Some("\\num{{value}{uncertainty}}"),
    };
    let filled =
        |value, uncertainty, unit| filled_property(&forms, value, uncertainty, unit, None).unwrap();
    assert_eq!(
        filled("12.5", Some("0.05"), Some("\\gram")),
        "\\SI{12.5 \\pm 0.05}{\\gram}"
    );
    assert_eq!(filled("3", None, Some("\\gram")), "\\SI{3}{\\gram}");
    assert_eq!(
        filled("1.049", Some("0.0003"), None),
        "\\num{1.049 \\pm 0.0003}"
    );
    assert_eq!(filled("1.049", None, None), "\\num{1.049}");
    // A bracket and a bar are text.
    let bars = StyleForms {
        property: Some("[{value}|{uncertainty}]"),
        ..StyleForms::default()
    };
    assert_eq!(
        filled_property(&bars, "1", Some("2"), None, None).unwrap(),
        "[1|2]"
    );
    // Without the piece, {uncertainty} is the number; a value with none,
    // and no piece to leave out, is the caller's to write.
    let whole = StyleForms {
        property: Some("\\SI{{value} \\pm {uncertainty}}{{unit}}"),
        ..StyleForms::default()
    };
    assert_eq!(
        filled_property(&whole, "3", Some("0.1"), Some("g"), None).unwrap(),
        "\\SI{3 \\pm 0.1}{g}"
    );
    assert_eq!(filled_property(&whole, "3", None, Some("g"), None), None);
    assert_eq!(
        filled_property(&StyleForms::default(), "3", None, None, None),
        None
    );
}
