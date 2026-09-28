//! The tests of `value`.

use std::cmp::Ordering;

use samplekit::core::value::{
    Date, DateTime, Readings, Value, ValueError, ValueKind, compare, contains, equals, starts_with,
    total_order,
};

// ---------------------------------------------------------------- generation

/// A deterministic generator: a failing case is reproducible from this file
/// alone, with no recorded seed and no shrinking machinery.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Every kind, both signs, the boundaries of `i64`, the boundary of exact
/// integer representation in `f64`, and a spread of generated values.
fn corpus() -> Vec<Value> {
    let mut values = vec![
        Value::absent(),
        Value::boolean(false),
        Value::boolean(true),
        Value::text(""),
        Value::text("Dunkel 57"),
        Value::text("dc 57"),
        Value::text("\u{3b8}"),
        Value::integer(0),
        Value::integer(-1),
        Value::integer(3),
        Value::integer(i64::MIN),
        Value::integer(i64::MAX),
        Value::integer(9_007_199_254_740_993),
        Value::number(0.0).unwrap(),
        Value::number(-0.0).unwrap(),
        Value::number(3.0).unwrap(),
        Value::number(2.5).unwrap(),
        Value::number(-1e300).unwrap(),
        Value::number(1e300).unwrap(),
        Value::number(9_007_199_254_740_992.0).unwrap(),
        Value::date(Date::new(2026, 3, 14).unwrap()),
        Value::date(Date::new(1970, 1, 1).unwrap()),
        Value::date_time(DateTime::new(2026, 3, 14, 10, 30, 0).unwrap()),
    ];
    let mut generator = Generator(0x5EED);
    for _ in 0..24 {
        let value = match generator.next() % 5 {
            0 => Value::integer((generator.next() % 1000) as i64 - 500),
            1 => Value::number((generator.next() % 20_000) as f64 / 100.0 - 100.0).unwrap(),
            2 => Value::text(format!("t{}", generator.next() % 50)),
            3 => Value::boolean(generator.next().is_multiple_of(2)),
            _ => Value::date(
                Date::new(
                    2000 + (generator.next() % 30) as i32,
                    1 + (generator.next() % 12) as u32,
                    1 + (generator.next() % 28) as u32,
                )
                .unwrap(),
            ),
        };
        values.push(value);
    }
    values
}

// -------------------------------------------------------------- construction

#[test]
fn number_rejects_nan() {
    assert_eq!(
        Value::number(f64::NAN),
        Err(ValueError::NotFinite { position: None })
    );
}

#[test]
fn number_rejects_infinity() {
    assert_eq!(
        Value::number(f64::INFINITY),
        Err(ValueError::NotFinite { position: None })
    );
    assert_eq!(
        Value::number(f64::NEG_INFINITY),
        Err(ValueError::NotFinite { position: None })
    );
}

#[test]
fn absent_is_not_zero() {
    let absent = Value::absent();
    let zero = Value::number(0.0).unwrap();
    assert_ne!(absent.kind(), zero.kind());
    assert!(!equals(&absent, &zero));
    assert!(absent.is_absent());
    assert!(!zero.is_absent());
}

#[test]
fn absent_is_not_empty_text() {
    let absent = Value::absent();
    let empty = Value::text("");
    assert_ne!(absent.kind(), empty.kind());
    assert!(!equals(&absent, &empty));
}

#[test]
fn observations_reject_empty() {
    assert_eq!(Readings::new(vec![]), Err(ValueError::EmptyReadings));
}

#[test]
fn observations_reject_non_finite() {
    let error = Readings::new(vec![1.0, f64::NAN, 3.0]).unwrap_err();
    assert_eq!(error, ValueError::NotFinite { position: Some(1) });
    // The position is in the message, not only in the variant.
    assert!(error.to_string().contains('1'), "{error}");
}

// ------------------------------------------------------------------ ordering

#[test]
fn total_order_is_total() {
    let values = corpus();
    for a in &values {
        for b in &values {
            // Antisymmetry: reversing the arguments reverses the answer.
            assert_eq!(
                total_order(a, b),
                total_order(b, a).reverse(),
                "antisymmetry failed for {a:?} and {b:?}"
            );
        }
    }
    for a in &values {
        for b in &values {
            if total_order(a, b) == Ordering::Greater {
                continue;
            }
            for c in &values {
                if total_order(b, c) == Ordering::Greater {
                    continue;
                }
                assert_ne!(
                    total_order(a, c),
                    Ordering::Greater,
                    "transitivity failed: {a:?} <= {b:?} <= {c:?}"
                );
            }
        }
    }
}

#[test]
fn total_order_agrees_with_equality() {
    for a in &corpus() {
        for b in &corpus() {
            assert_eq!(
                total_order(a, b) == Ordering::Equal,
                equals(a, b),
                "total_order and equals disagree on {a:?} and {b:?}"
            );
        }
    }
}

#[test]
fn cross_kind_order_follows_the_declaration() {
    // Integer / Number  <  Text  <  Boolean  <  Date  <  Absent
    let ladder = [
        Value::number(1e9).unwrap(),
        Value::text("zzz"),
        Value::boolean(false),
        Value::date(Date::new(1970, 1, 1).unwrap()),
        Value::date_time(DateTime::new(1970, 1, 1, 0, 0, 0).unwrap()),
        Value::absent(),
    ];
    for (i, left) in ladder.iter().enumerate() {
        for (j, right) in ladder.iter().enumerate() {
            assert_eq!(
                total_order(left, right),
                i.cmp(&j),
                "position {i} against position {j}"
            );
        }
    }
    // An integer shares the first position rather than holding one of its own.
    assert_eq!(
        total_order(&Value::integer(5), &Value::text("")),
        Ordering::Less
    );
}

#[test]
fn boolean_orders_false_before_true() {
    assert_eq!(
        total_order(&Value::boolean(false), &Value::boolean(true)),
        Ordering::Less
    );
    // Defined for sorting, and nothing else relies on it: a boolean compares to
    // a boolean and to nothing else, exactly as a date does.
    assert_eq!(
        compare(&Value::boolean(true), &Value::integer(1)),
        Err(ValueError::IncomparableKinds {
            left: ValueKind::Boolean,
            right: ValueKind::Integer
        })
    );
    assert_eq!(
        compare(&Value::boolean(false), &Value::boolean(true)),
        Ok(Ordering::Less)
    );
}

#[test]
fn integer_orders_against_number_numerically() {
    // They interleave rather than grouping by kind.
    let mut values = [
        Value::number(2.5).unwrap(),
        Value::integer(3),
        Value::number(1.5).unwrap(),
        Value::integer(2),
    ];
    values.sort_by(total_order);
    let rendered: Vec<String> = values
        .iter()
        .map(|v| match v {
            Value::Integer(i) => format!("i{i}"),
            Value::Number(n) => format!("n{n}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(rendered, ["n1.5", "i2", "n2.5", "i3"]);
    // In both operations: `compare` is the one a filter reaches, and it must
    // not refuse the single cross-kind pair that has an answer.
    assert_eq!(
        compare(&Value::integer(3), &Value::number(2.5).unwrap()),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        compare(&Value::number(2.5).unwrap(), &Value::integer(3)),
        Ok(Ordering::Less)
    );
    assert_eq!(
        compare(&Value::integer(3), &Value::number(3.0).unwrap()),
        Ok(Ordering::Equal)
    );
}

#[test]
fn large_integer_compares_exactly() {
    // 2^53 + 1 is not representable as an f64: widening it would report these
    // two as equal.
    let integer = Value::integer(9_007_199_254_740_993);
    let number = Value::number(9_007_199_254_740_992.0).unwrap();
    assert_eq!(total_order(&integer, &number), Ordering::Greater);
    assert_eq!(total_order(&number, &integer), Ordering::Less);
    assert!(!equals(&integer, &number));
}

#[test]
fn date_compares_only_to_date() {
    let date = Value::date(Date::new(2026, 3, 14).unwrap());
    assert_eq!(
        compare(&date, &Value::text("2026-03-14")),
        Err(ValueError::IncomparableKinds {
            left: ValueKind::Date,
            right: ValueKind::Text
        })
    );
    assert_eq!(
        compare(&date, &Value::number(2026.0).unwrap()),
        Err(ValueError::IncomparableKinds {
            left: ValueKind::Date,
            right: ValueKind::Number
        })
    );
    assert_eq!(
        compare(&date, &Value::date(Date::new(2026, 3, 15).unwrap())),
        Ok(Ordering::Less)
    );
}

// ---------------------------------------------------------------- comparison

#[test]
fn compare_rejects_mixed_kinds() {
    let error = compare(&Value::number(3.0).unwrap(), &Value::text("heavy")).unwrap_err();
    assert_eq!(
        error,
        ValueError::IncomparableKinds {
            left: ValueKind::Number,
            right: ValueKind::Text
        }
    );
    // The message names both kinds rather than saying "invalid comparison".
    let message = error.to_string();
    assert!(
        message.contains("number") && message.contains("text"),
        "{message}"
    );
}

#[test]
fn compare_rejects_absent() {
    assert_eq!(
        compare(&Value::absent(), &Value::number(3.0).unwrap()),
        Err(ValueError::ComparisonWithAbsent)
    );
    assert_eq!(
        compare(&Value::number(3.0).unwrap(), &Value::absent()),
        Err(ValueError::ComparisonWithAbsent)
    );
    // Two absences are as unanswerable as one.
    assert_eq!(
        compare(&Value::absent(), &Value::absent()),
        Err(ValueError::ComparisonWithAbsent)
    );
}

#[test]
fn equality_across_kinds_is_false_not_an_error() {
    // Equality is total where ordering is not.
    assert!(!equals(&Value::number(3.0).unwrap(), &Value::text("3")));
    assert!(!equals(&Value::boolean(true), &Value::integer(1)));
    assert!(!equals(
        &Value::date(Date::new(2026, 3, 14).unwrap()),
        &Value::text("2026-03-14")
    ));
}

#[test]
fn equals_is_reflexive_including_absent() {
    for value in corpus() {
        assert!(equals(&value, &value), "{value:?} does not equal itself");
    }
    assert!(equals(&Value::absent(), &Value::absent()));
}

#[test]
fn integer_equals_number_of_the_same_magnitude() {
    // `batch == 3` matches a stored 3 and a stored 3.0, or the integer/number
    // distinction becomes a trap instead of a convenience.
    assert!(equals(&Value::integer(3), &Value::number(3.0).unwrap()));
    assert!(equals(&Value::number(3.0).unwrap(), &Value::integer(3)));
    assert!(!equals(&Value::integer(3), &Value::number(3.5).unwrap()));
}

#[test]
fn text_equality_is_case_sensitive() {
    // "Dunkel 57" and "dc 57" are different labels.
    assert!(!equals(&Value::text("Dunkel 57"), &Value::text("dc 57")));
    assert!(equals(&Value::text("Dunkel 57"), &Value::text("Dunkel 57")));
}

// ------------------------------------------------------------ text operators

#[test]
fn contains_is_case_insensitive() {
    assert_eq!(contains(&Value::text("Dunkel 57"), "du"), Ok(true));
    assert_eq!(contains(&Value::text("du 57"), "DU"), Ok(true));
    assert_eq!(contains(&Value::text("Dunkel 57"), "xy"), Ok(false));
}

#[test]
fn starts_with_is_case_insensitive() {
    assert_eq!(starts_with(&Value::text("Dunkel 57"), "du"), Ok(true));
    assert_eq!(starts_with(&Value::text("du 57"), "DU"), Ok(true));
    assert_eq!(starts_with(&Value::text("Dunkel 57"), "57"), Ok(false));
}

#[test]
fn text_operators_reject_numbers() {
    // An error, not a stringified match: the text form of a number depends on
    // formatting, which is not visible from here.
    assert_eq!(
        contains(&Value::number(1234.0).unwrap(), "23"),
        Err(ValueError::NotText {
            found: ValueKind::Number
        })
    );
    assert_eq!(
        starts_with(&Value::integer(1234), "12"),
        Err(ValueError::NotText {
            found: ValueKind::Integer
        })
    );
}

#[test]
fn text_operators_reject_every_non_text_kind() {
    let kinds = [
        (Value::integer(1), ValueKind::Integer),
        (Value::number(1.0).unwrap(), ValueKind::Number),
        (Value::boolean(true), ValueKind::Boolean),
        (
            Value::date(Date::new(2026, 3, 14).unwrap()),
            ValueKind::Date,
        ),
        (
            Value::date_time(DateTime::new(2026, 3, 14, 10, 30, 0).unwrap()),
            ValueKind::DateTime,
        ),
        (Value::absent(), ValueKind::Absent),
    ];
    for (value, kind) in kinds {
        assert_eq!(
            contains(&value, "x"),
            Err(ValueError::NotText { found: kind }),
            "contains on {kind}"
        );
        assert_eq!(
            starts_with(&value, "x"),
            Err(ValueError::NotText { found: kind }),
            "starts_with on {kind}"
        );
    }
}

// ---------------------------------------------------------------------- date

#[test]
fn date_rejects_an_impossible_day() {
    // The 31st of February is refused, not silently shifted to March.
    let error = Date::new(2026, 2, 31).unwrap_err();
    assert!(matches!(error, ValueError::InvalidDate { .. }));
    assert!(Date::parse("2026-02-31").is_err());
    // And a real leap day is accepted.
    assert!(Date::new(2024, 2, 29).is_ok());
    assert!(Date::new(2026, 2, 29).is_err());
}

#[test]
fn date_parses_only_iso() {
    assert_eq!(Date::parse("2026-03-14").unwrap().iso(), "2026-03-14");
    assert!(Date::parse("14/03/2026").is_err());
    // Not a lenient ISO either: `iso` is the exact inverse of `parse`.
    assert!(Date::parse("2026-3-14").is_err());
    assert!(Date::parse("2026-03-14T00:00:00").is_err());
    assert!(Date::parse("").is_err());
    // And the round trip holds in the other direction.
    let date = Date::new(2026, 3, 14).unwrap();
    assert_eq!(Date::parse(&date.iso()).unwrap(), date);
}

#[test]
fn date_time_accepts_the_four_local_forms() {
    let forms = [
        ("2026-03-14T10:30", "2026-03-14T10:30"),
        ("2026-03-14 10:30", "2026-03-14T10:30"),
        ("2026-03-14T10:30:15", "2026-03-14T10:30:15"),
        ("2026-03-14 10:30:15", "2026-03-14T10:30:15"),
    ];
    for (written, canonical) in forms {
        assert_eq!(DateTime::parse(written).unwrap().iso(), canonical);
    }
    assert_eq!(
        DateTime::new(2026, 3, 14, 10, 30, 0).unwrap().iso(),
        "2026-03-14T10:30"
    );
}

#[test]
fn date_and_date_time_compare_as_two_precisions() {
    let day = Value::date(Date::parse("2026-03-14").unwrap());
    let morning = Value::date_time(DateTime::parse("2026-03-14T10:30").unwrap());
    let tomorrow = Value::date_time(DateTime::parse("2026-03-15T00:00").unwrap());
    assert_eq!(compare(&morning, &day), Ok(Ordering::Equal));
    assert_eq!(compare(&day, &morning), Ok(Ordering::Equal));
    // They are different stored values. The filter layer gives a date operand
    // its whole-day predicate meaning without weakening `Value`'s `Eq` law.
    assert!(!equals(&morning, &day));
    assert_eq!(total_order(&day, &morning), Ordering::Less);
    assert_eq!(compare(&morning, &tomorrow), Ok(Ordering::Less));
}

#[test]
fn date_time_refuses_timezones_and_a_fraction_without_seconds() {
    for written in [
        "2026-03-14T10:30Z",
        "2026-03-14T10:30+02:00",
        "2026-03-14T10:30:15.2Z",
        "2026-03-14T10:30.5",
        "2026-03-14T10:30:15.",
        "2026-03-14T10:30:15.1234567890",
        "2026-03-14T24:00",
    ] {
        assert!(
            matches!(
                DateTime::parse(written),
                Err(ValueError::InvalidDateTime { .. })
            ),
            "{written}"
        );
    }
}

#[test]
fn date_time_keeps_a_fraction_of_a_second() {
    let forms = [
        ("2026-01-24T09:42:17.204931", "2026-01-24T09:42:17.204931"),
        ("2026-01-24 09:42:53.20", "2026-01-24T09:42:53.2"),
        ("2026-01-24T09:42:00.5", "2026-01-24T09:42:00.5"),
        ("2026-01-24T09:42:53.000", "2026-01-24T09:42:53"),
        (
            "2026-01-24T09:42:00.000000001",
            "2026-01-24T09:42:00.000000001",
        ),
    ];
    for (written, canonical) in forms {
        assert_eq!(DateTime::parse(written).unwrap().iso(), canonical);
    }
}

#[test]
fn a_long_text_is_refused_as_a_date_time_without_a_panic() {
    for written in [
        "Bière de garde, douze degrés",
        "2026-01-24T09:42:53é",
        "2026-01-24T09:42:5é.12",
    ] {
        assert!(DateTime::parse(written).is_err(), "{written}");
    }
}
