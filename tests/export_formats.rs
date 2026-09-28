//! The tests of `export_formats`.

use samplekit::config::project_config::Format;
use samplekit::core::value::{Date, Value};
use samplekit::presentation::export_formats::{Cell, Dataset};

mod formats {
    use samplekit::config::project_config::Format;
    use samplekit::presentation::export_formats::{self, Dataset, ExportFormatError};

    pub fn serialize(dataset: &Dataset, format: Format) -> String {
        export_formats::serialize(dataset, format).unwrap()
    }

    pub fn serialize_with(dataset: &Dataset, format: Format, crlf: bool) -> String {
        export_formats::serialize_with(dataset, format, crlf).unwrap()
    }

    pub fn serialize_result(
        dataset: &Dataset,
        format: Format,
    ) -> Result<String, ExportFormatError> {
        export_formats::serialize(dataset, format)
    }
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn dataset(headers: &[&str], rows: Vec<Vec<Option<Value>>>) -> Dataset {
    Dataset {
        headers: headers.iter().map(|h| h.to_string()).collect(),
        rows: rows
            .into_iter()
            .map(|row| row.into_iter().map(Into::into).collect())
            .collect(),
        ..Default::default()
    }
}

/// One cask with a measured malt, one with none.
fn corpus() -> Dataset {
    dataset(
        &["name", "malt_value", "malt_uncertainty"],
        vec![
            vec![
                Some(Value::text("A")),
                Some(number(12.5)),
                Some(number(0.05)),
            ],
            vec![Some(Value::text("B")), None, None],
        ],
    )
}

// ----------------------------------------------------------------- absence

#[test]
fn absent_is_empty_in_csv() {
    // Never `-`, never `N/A`: each of those is a *value* to a parser, and
    // pandas turns a numeric column into an object column on one dash.
    let out = formats::serialize(&corpus(), Format::Csv);
    assert_eq!(out.lines().nth(2).unwrap(), "B,,");
    assert!(!out.contains('-'), "{out}");
    assert!(!out.contains("N/A"), "{out}");
}

#[test]
fn absent_is_null_in_json() {
    let out = formats::serialize(&corpus(), Format::Json);
    assert!(out.contains("\"malt_value\": null"), "{out}");
}

#[test]
fn json_objects_share_all_keys() {
    // A consumer iterating keys must find the same keys in every object, or a
    // missing measurement becomes a missing field.
    let out = formats::serialize(&corpus(), Format::Json);
    let per_object: Vec<usize> = out
        .split("  {")
        .skip(1)
        .map(|block| block.matches("\": ").count())
        .collect();
    assert_eq!(per_object, [3, 3], "{out}");
}

// ----------------------------------------------------------------- quoting

#[test]
fn delimiter_in_a_field_is_quoted() {
    let out = formats::serialize(
        &dataset(
            &["beer"],
            vec![vec![Some(Value::text("Dunkel 57, rating 2"))]],
        ),
        Format::Csv,
    );
    assert_eq!(out.lines().nth(1).unwrap(), "\"Dunkel 57, rating 2\"");
}

#[test]
fn quote_in_a_field_is_doubled() {
    let out = formats::serialize(
        &dataset(
            &["note"],
            vec![vec![Some(Value::text("a \"quoted\" word"))]],
        ),
        Format::Csv,
    );
    assert_eq!(out.lines().nth(1).unwrap(), "\"a \"\"quoted\"\" word\"");
}

#[test]
fn newline_in_a_field_is_quoted() {
    let out = formats::serialize(
        &dataset(&["note"], vec![vec![Some(Value::text("two\nlines"))]]),
        Format::Csv,
    );
    assert!(out.contains("\"two\nlines\""), "{out:?}");
}

#[test]
fn tab_in_a_tsv_field_is_quoted() {
    // TSV has no standard, so it quotes rather than escapes: escape-based TSV
    // has no agreed convention and quoting is what spreadsheets accept.
    let out = formats::serialize(
        &dataset(&["note"], vec![vec![Some(Value::text("a\tb"))]]),
        Format::Tsv,
    );
    assert_eq!(out.lines().nth(1).unwrap(), "\"a\tb\"");
}

#[test]
fn leading_equals_is_neutralized() {
    // And a negative *number* is not: the prefix applies to text only.
    let out = formats::serialize(
        &dataset(
            &["name", "offset"],
            vec![vec![Some(Value::text("=SUM(A1)")), Some(number(-3.5))]],
        ),
        Format::Csv,
    );
    let row = out.lines().nth(1).unwrap();
    assert!(row.starts_with("'=SUM(A1)"), "{row}");
    assert!(row.ends_with("-3.5"), "{row}");
    // Every trigger character, and only in text.
    for trigger in ["=x", "+x", "-x", "@x"] {
        let out = formats::serialize(
            &dataset(&["v"], vec![vec![Some(Value::text(trigger))]]),
            Format::Csv,
        );
        assert_eq!(out.lines().nth(1).unwrap(), format!("'{trigger}"));
    }
}

// ----------------------------------------------------------------- numbers

#[test]
fn numbers_use_a_decimal_point() {
    // No thousands separators, no locale: a comma-separated file with comma
    // decimal separators is unparseable.
    let out = formats::serialize(
        &dataset(&["v"], vec![vec![Some(number(1234.5))]]),
        Format::Csv,
    );
    assert_eq!(out.lines().nth(1).unwrap(), "1234.5");
}

#[test]
fn numbers_round_trip_exactly() {
    // The shortest decimal that parses back identically.
    for x in [0.1 + 0.2, 2.0140000000000002, 1e-7, -0.0, 12.5] {
        let out = formats::serialize(&dataset(&["v"], vec![vec![Some(number(x))]]), Format::Csv);
        let written = out.lines().nth(1).unwrap();
        assert_eq!(
            written.parse::<f64>().unwrap().to_bits(),
            x.to_bits(),
            "{written}"
        );
    }
}

#[test]
fn declared_precision_is_applied() {
    // A precision that reaches this module is already resolved, and applying
    // it is the caller's: what arrives here is a `Value`, so a rounded export
    // is a rounded value in the dataset — never a second rounding at write
    // time, which would make the file disagree with the terminal.
    let rounded = dataset(&["v"], vec![vec![Some(number(12.487))]]);
    let exact = formats::serialize(&rounded, Format::Csv);
    assert_eq!(exact.lines().nth(1).unwrap(), "12.487");
    let already = dataset(&["v"], vec![vec![Some(number(12.49))]]);
    assert_eq!(
        formats::serialize(&already, Format::Csv)
            .lines()
            .nth(1)
            .unwrap(),
        "12.49"
    );
}

// -------------------------------------------------------------- stability

#[test]
fn line_endings_are_lf_by_default() {
    // On every platform, so a file exported on two machines is identical.
    let out = formats::serialize(&corpus(), Format::Csv);
    assert!(!out.contains('\r'), "{out:?}");
    // And CRLF is a declared setting, not a platform default.
    let windows = formats::serialize_with(&corpus(), Format::Csv, true);
    assert!(windows.contains("\r\n"), "{windows:?}");
}

#[test]
fn output_is_byte_identical_across_runs() {
    // No timestamps, no map iteration order.
    let first = formats::serialize(&corpus(), Format::Json);
    for _ in 0..5 {
        assert_eq!(formats::serialize(&corpus(), Format::Json), first);
    }
}

#[test]
fn a_list_is_a_json_array() {
    let dataset = Dataset {
        headers: vec!["temperatures".to_string()],
        rows: vec![vec![Cell::List(vec![
            Value::integer(20),
            Value::integer(30),
        ])]],
        ..Default::default()
    };
    let out = formats::serialize(&dataset, Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed[0]["temperatures"],
        serde_json::json!([20, 30]),
        "{out}"
    );
}

#[test]
fn a_list_is_joined_in_csv_and_tsv() {
    // As tags are, `;` between items and a `;` or `\` inside escaped.
    let dataset = Dataset {
        headers: vec!["hops".to_string()],
        rows: vec![vec![Cell::List(vec![
            Value::integer(20),
            Value::text("a;b"),
            Value::number(1.5).unwrap(),
        ])]],
        ..Default::default()
    };
    assert_eq!(
        formats::serialize_result(&dataset, Format::Csv).unwrap(),
        "hops\n20;a\\;b;1.5\n"
    );
    assert_eq!(
        formats::serialize_result(&dataset, Format::Tsv).unwrap(),
        "hops\n20;a\\;b;1.5\n"
    );
}

#[test]
fn csv_round_trips_through_a_conforming_parser() {
    // Fields holding a delimiter, a quote and a newline all survive.
    let tricky = dataset(
        &["a", "b"],
        vec![vec![
            Some(Value::text("x, y")),
            Some(Value::text("say \"hi\"\nagain")),
        ]],
    );
    let out = formats::serialize(&tricky, Format::Csv);
    let parsed = parse_csv(&out);
    assert_eq!(parsed[0], ["a", "b"]);
    assert_eq!(parsed[1], ["x, y", "say \"hi\"\nagain"]);
    // And a date keeps its ISO form through the trip.
    let dated = dataset(
        &["on"],
        vec![vec![Some(Value::date(Date::parse("2026-03-14").unwrap()))]],
    );
    assert_eq!(
        parse_csv(&formats::serialize(&dated, Format::Csv))[1],
        ["2026-03-14"]
    );
}

/// A minimal RFC 4180 reader, so that the round trip is checked against the
/// rules rather than against the writer's own idea of them.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted => {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[test]
fn a_number_with_a_point_keeps_it() {
    let data = dataset(
        &["loading", "batch"],
        vec![vec![Some(number(13.0)), Some(Value::integer(2))]],
    );
    let csv = formats::serialize(&data, Format::Csv);
    assert!(csv.contains("13.0,2"), "{csv}");
    let json = formats::serialize(&data, Format::Json);
    assert!(json.contains("13.0") && json.contains(": 2"), "{json}");
}

#[test]
fn a_unit_is_written_beside_its_header() {
    let mut data = corpus();
    data.units = vec![None, Some("g".to_string()), Some("g".to_string())];
    let out = formats::serialize(&data, Format::Csv);
    assert_eq!(
        out.lines().next().unwrap(),
        "name,malt_value [g],malt_uncertainty [g]"
    );
}

#[test]
fn a_quantity_is_one_json_object_with_its_unit() {
    let mut data = corpus();
    data.units = vec![None, Some("g".to_string()), Some("g".to_string())];
    data.quantities = vec![samplekit::presentation::export_formats::QuantityColumns {
        key: "malt".to_string(),
        value: 1,
        uncertainty: Some(2),
        states: Vec::new(),
    }];
    let out = formats::serialize(&data, Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed[0]["malt"],
        serde_json::json!({"value": 12.5, "uncertainty": 0.05, "unit": "g"}),
        "{out}"
    );
    assert!(!out.contains("malt_uncertainty"), "{out}");
}

#[test]
fn tags_are_an_array_in_json_and_joined_in_delimited() {
    let data = Dataset {
        headers: vec!["tags".to_string()],
        rows: vec![vec![Cell::Tags(vec!["a".to_string(), "b;c".to_string()])]],
        ..Default::default()
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&formats::serialize(&data, Format::Json)).unwrap();
    assert_eq!(parsed[0]["tags"], serde_json::json!(["a", "b;c"]));
    assert_eq!(
        formats::serialize(&data, Format::Csv)
            .lines()
            .nth(1)
            .unwrap(),
        r"a;b\;c"
    );
}

#[test]
fn an_uncertainty_column_empty_in_every_row_is_left_out() {
    let mut data = dataset(
        &["name", "malt_value", "malt_uncertainty"],
        vec![
            vec![Some(Value::text("A")), Some(number(12.5)), None],
            vec![Some(Value::text("B")), Some(number(9.0)), None],
        ],
    );
    data.quantities = vec![samplekit::presentation::export_formats::QuantityColumns {
        key: "malt".to_string(),
        value: 1,
        uncertainty: Some(2),
        states: Vec::new(),
    }];
    let (trimmed, dropped) =
        samplekit::presentation::export_formats::without_empty_uncertainties(&data);
    assert_eq!(dropped, ["malt_uncertainty"]);
    assert_eq!(trimmed.headers, ["name", "malt_value"]);
    assert_eq!(trimmed.quantities[0].uncertainty, None);
    let (kept, none) =
        samplekit::presentation::export_formats::without_empty_uncertainties(&corpus());
    assert!(none.is_empty());
    assert_eq!(kept.headers.len(), 3);
}

#[test]
fn a_quantity_carries_its_state_in_json() {
    let mut data = corpus();
    data.quantities = vec![samplekit::presentation::export_formats::QuantityColumns {
        key: "malt".to_string(),
        value: 1,
        uncertainty: Some(2),
        states: vec![Some("stale".to_string()), None],
    }];
    let out = formats::serialize(&data, Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed[0]["malt"]["unit"].is_null() && parsed[0]["malt"]["state"] == "stale",
        "{out}"
    );
    assert_eq!(out.matches("\"state\"").count(), 1, "{out}");
}

#[test]
fn not_applicable_is_written_as_such_in_csv_and_tsv() {
    // An empty field read as a value nobody measured, where this one was
    // answered: `n/a` in CSV and TSV, null in JSON.
    let data = dataset(
        &["name", "malt_value"],
        vec![vec![Some(Value::text("A")), Some(Value::NotApplicable)]],
    );
    assert_eq!(
        formats::serialize(&data, Format::Csv).lines().nth(1),
        Some("A,n/a")
    );
    assert_eq!(
        formats::serialize(&data, Format::Tsv).lines().nth(1),
        Some("A\tn/a")
    );
    assert!(formats::serialize(&data, Format::Json).contains("\"malt_value\": null"));
}
