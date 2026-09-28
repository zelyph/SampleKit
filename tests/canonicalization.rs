//! The tests of `canonicalization`.

use indexmap::IndexMap;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{Fingerprint, InputName};
use samplekit::core::sample::AttributeValue;
use samplekit::core::value::{Date, DateTime, Value};
use samplekit::format::canonicalization::{is_canonical, value_form, write};
use samplekit::format::schema::{
    self, ColumnSchema, Computed, PropertySchema, Recorded, SampleSchema, TableSchema,
};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn parse(source: &str) -> SampleSchema {
    serde_yaml_ng::from_str(source).unwrap_or_else(|e| panic!("{e}\n---\n{source}"))
}

fn empty() -> SampleSchema {
    SampleSchema {
        schema_version: 1,
        name: None,
        tags: Vec::new(),
        attributes: IndexMap::new(),
        properties: IndexMap::new(),
        tables: IndexMap::new(),
    }
}

fn value_only(value: Value) -> PropertySchema {
    PropertySchema {
        statistics: None,
        value: Some(value),
        ..PropertySchema::default()
    }
}

fn with(properties: &[(&str, PropertySchema)]) -> SampleSchema {
    let mut schema = empty();
    for (name, property) in properties {
        schema.properties.insert(id(name), property.clone());
    }
    schema
}

fn malt() -> PropertySchema {
    PropertySchema {
        statistics: None,
        value: Some(number(12.5)),
        uncertainty: Some(0.05),
        unit: Some("g".to_string()),
        ..PropertySchema::default()
    }
}

/// A `computed` record naming no channel: the quantity as a whole, which is
/// what every fixture here means.
fn record(pairs: &[(InputName, Value)]) -> Computed {
    inputs(pairs).into()
}

fn inputs(pairs: &[(InputName, Value)]) -> IndexMap<InputName, Recorded> {
    pairs
        .iter()
        .map(|(name, value)| (name.clone(), Recorded::Value(value.clone())))
        .collect()
}

// ------------------------------------------------------------- determinism

#[test]
fn output_is_deterministic() {
    let schema = with(&[
        ("malt", malt()),
        ("finish", value_only(Value::text("unfiltered"))),
    ]);
    assert_eq!(write(&schema), write(&schema));
}

#[test]
fn write_is_idempotent() {
    // write(parse(write(x))) == write(x), over a schema with one of everything.
    let mut schema = with(&[
        ("malt", malt()),
        ("finish", value_only(Value::text("unfiltered"))),
        ("batch", value_only(Value::integer(3))),
        ("cold_crashed", value_only(Value::boolean(true))),
        ("temperature", value_only(number(20.0))),
    ]);
    schema.name = Some("Keg 42".to_string());
    schema.tags = vec![
        samplekit::format::schema::Tag::Usable(id("reference")),
        samplekit::format::schema::Tag::Usable(id("cold_crashed_run")),
    ];
    schema
        .attributes
        .insert(id("lot"), Value::integer(7).into());
    schema.attributes.insert(
        id("brewed_on"),
        Value::date(Date::new(2026, 3, 14).unwrap()).into(),
    );

    let once = write(&schema);
    let twice = write(&parse(&once));
    assert_eq!(
        once, twice,
        "\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );
    assert!(is_canonical(&once, &schema));
    assert!(!is_canonical("schema_version: 1\n", &schema));
}

#[test]
fn field_order_is_fixed() {
    // Regardless of construction order.
    let mut schema = empty();
    schema.tables.insert(
        id("mashing"),
        TableSchema {
            title: None,
            index: vec![id("temperature")],
            columns: [(id("temperature"), ColumnSchema::default())]
                .into_iter()
                .collect(),
            // A table without a row is not written.
            rows: vec![
                [(id("temperature"), value_only(number(20.0)))]
                    .into_iter()
                    .collect(),
            ],
        },
    );
    schema.properties.insert(id("malt"), malt());
    schema
        .attributes
        .insert(id("batch"), Value::integer(3).into());
    schema.tags = vec![samplekit::format::schema::Tag::Usable(id("reference"))];
    schema.name = Some("A".to_string());

    let out = write(&schema);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "schema_version: 1");
    assert_eq!(lines[1], "name: A");
    assert_eq!(lines[2], "tags: [reference]");
    assert_eq!(lines[3], "batch: 3");
    assert_eq!(lines[4], "properties:");
    assert!(lines.contains(&"tables:"));
}

#[test]
fn records_are_written_last() {
    // `computed` then `fingerprint`, after every presentation field.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(3.05)),
        unit: Some("g/dl".to_string()),
        computed: Some(record(&[(
            InputName::Named(id("malt")),
            Value::text("9f2c14ab77d0"),
        )])),
        fingerprint: Some(Fingerprint::new("5b2c9e1f0a34")),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("plato", property)]));
    let order: Vec<usize> = ["v:", "unit:", "computed:", "fingerprint:"]
        .iter()
        .map(|field| {
            out.find(field)
                .unwrap_or_else(|| panic!("{field} missing\n{out}"))
        })
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{out}");
}

// ---------------------------------------------------------------- shapes

#[test]
fn value_only_property_is_a_bare_scalar() {
    let out = write(&with(&[("finish", value_only(Value::text("unfiltered")))]));
    assert!(out.contains("  finish: unfiltered\n"), "{out}");
    assert!(!out.contains('{'), "{out}");
}

#[test]
fn property_with_metadata_is_a_flow_mapping() {
    // And stays on one line.
    let out = write(&with(&[("malt", malt())]));
    assert!(
        out.contains("  malt: {v: 12.5, u: 0.05, unit: g}\n"),
        "{out}"
    );
}

#[test]
fn derived_property_is_a_block_mapping() {
    // And an entered one beside it stays a flow mapping.
    let derived = PropertySchema {
        statistics: None,
        value: Some(number(3.05)),
        computed: Some(record(&[(
            InputName::Named(id("malt")),
            Value::text("9f2c14ab77d0"),
        )])),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("malt", malt()), ("plato", derived)]));
    assert!(out.contains("  plato:\n    v: 3.05\n"), "{out}");
    assert!(out.contains("  malt: {v:"), "{out}");
}

#[test]
fn an_empty_computed_map_is_written() {
    // `computed: {}` marks a formula with no inputs, and is not absence.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(0.05)),
        computed: Some(Computed::from(IndexMap::new())),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("nominal", property)]));
    assert!(out.contains("computed: {}"), "{out}");
    // `computed: {}` is a formula with no inputs, and not the absence of a
    // record: the quantity is named, its input map is empty.
    assert_eq!(
        parse(&out).properties[&id("nominal")].computed,
        Some(Computed::from(IndexMap::new()))
    );
}

#[test]
fn a_long_flow_mapping_wraps_at_a_hundred_characters() {
    let long = PropertySchema {
        statistics: None,
        value: Some(number(12.5)),
        uncertainty: Some(0.05),
        unit: Some("kilogram_per_cubic_metre".to_string()),
        symbol: Some("brix_effective_measured_at_room_temperature".to_string()),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("malt", malt()), ("plato_of_the_keg", long)]));
    for line in out.lines() {
        assert!(
            line.chars().count() <= 100,
            "{} chars: {line}",
            line.chars().count()
        );
    }
    // And a shorter one does not wrap, so the exception stays the exception.
    assert!(
        out.contains("  malt: {v: 12.5, u: 0.05, unit: g}\n"),
        "{out}"
    );
    // It is still readable back.
    let back = parse(&out);
    assert_eq!(
        back.properties[&id("plato_of_the_keg")].symbol.as_deref(),
        Some("brix_effective_measured_at_room_temperature")
    );
}

// ---------------------------------------------------------------- omission

#[test]
fn none_is_omitted_not_written_as_null() {
    let out = write(&with(&[("finish", value_only(Value::text("x")))]));
    assert!(!out.contains("null"), "{out}");
    assert!(!out.contains("name:"), "{out}");
}

#[test]
fn empty_tags_are_omitted() {
    // No `tags: []` is ever written.
    let out = write(&empty());
    assert!(!out.contains("tags"), "{out}");
}

#[test]
fn empty_string_is_written() {
    // A deliberate empty value is distinct from absence.
    let out = write(&with(&[("note", value_only(Value::text("")))]));
    assert!(out.contains("  note: \"\"\n"), "{out}");
    assert_eq!(
        parse(&out).properties[&id("note")].value,
        Some(Value::text(""))
    );
}

#[test]
fn a_value_beside_readings_is_written_as_it_stands() {
    // The written value is the value, and a write never replaces it with a
    // statistic of the readings beside it.
    let written = PropertySchema {
        statistics: None,
        value: Some(number(2.014)),
        readings: Some(vec![2.011, 2.017, 2.013, 2.015]),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("foam", written)]));
    assert!(out.contains("v: 2.014,"), "{out}");
    assert!(!out.contains("2.0140000000000002"), "{out}");
    // And writing it again changes nothing.
    assert_eq!(write(&parse(&out)), out);
}

#[test]
fn readings_with_no_value_are_written_alone() {
    // No mean is written on the reader's behalf. Which statistic stands for
    // readings is the model's to say, and a mean the writer supplied read back
    // as a value somebody chose.
    let unwritten = PropertySchema {
        statistics: None,
        readings: Some(vec![2.0, 2.5, 3.0]),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("foam", unwritten)]));
    assert!(out.contains("foam: {readings: [2.0, 2.5, 3.0]}"), "{out}");
    assert!(!out.contains("v:"), "{out}");
    assert_eq!(write(&parse(&out)), out);
}

#[test]
fn value_is_written_alongside_data() {
    // A value beside readings is written beside them, so a reader never has
    // to average to learn it; the field is `readings`.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(2.5)),
        readings: Some(vec![2.0, 2.5, 3.0]),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("foam", property)]));
    assert!(out.contains("v: 2.5"), "{out}");
    assert!(out.contains("readings: [2.0, 2.5, 3.0]"), "{out}");
}

// ---------------------------------------------------------------- scalars

#[test]
fn the_shorter_number_form_is_written() {
    // Shortest means shortest on the page, and a tie goes to the positional
    // form, which is the one a reader does not have to decode.
    for (x, expected) in [
        (0.001, "0.001"),   // 5 against 6
        (0.0001, "0.0001"), // 6 each: the tie
        (0.00001, "1.0e-5"),
        (1e20, "1.0e+20"),
        (20.0, "20.0"),
        (0.0, "0.0"),
    ] {
        let out = write(&with(&[("v", value_only(number(x)))]));
        assert!(out.contains(&format!("  v: {expected}\n")), "{x}: {out}");
    }
}

#[test]
fn an_exponent_carries_its_point_and_its_sign() {
    // `1e-5` is not a number to a YAML 1.1 reader, it is the string `1e-5`.
    let out = write(&with(&[("v", value_only(number(5e-7)))]));
    assert!(out.contains("  v: 5.0e-7\n"), "{out}");
    let back = parse(&out).properties[&id("v")].value.clone().unwrap();
    assert_eq!(back, number(5e-7));
}

#[test]
fn numbers_round_trip_exactly() {
    // Property test: parse(write(x)) is bit-identical for generated floats.
    let mut seed = 0x1234_5678u64;
    let mut next = move || {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut generated: Vec<f64> = vec![
        0.1 + 0.2,
        1e-300,
        1e300,
        12.498_700_000_000_001,
        -0.0,
        f64::MIN_POSITIVE,
    ];
    for _ in 0..200 {
        let bits = next();
        let candidate = f64::from_bits(bits);
        if candidate.is_finite() {
            generated.push(candidate);
        }
    }
    for x in generated {
        let out = write(&with(&[("v", value_only(number(x)))]));
        let back = parse(&out).properties[&id("v")].value.clone().unwrap();
        match back {
            Value::Number(n) => assert_eq!(n.to_bits(), x.to_bits(), "{x} through {out}"),
            Value::Integer(i) => assert_eq!(i as f64, x, "{x} through {out}"),
            other => panic!("{x} came back as {other:?} through {out}"),
        }
    }
}

#[test]
fn float_keeps_its_type() {
    // `20.0` does not become `20`.
    let out = write(&with(&[("t", value_only(number(20.0)))]));
    assert!(out.contains("  t: 20.0\n"), "{out}");
    assert!(matches!(
        parse(&out).properties[&id("t")].value,
        Some(Value::Number(_))
    ));
}

#[test]
fn an_integer_round_trips_without_becoming_a_float() {
    // A batch number `3` is written `3`, never `3.0`.
    let out = write(&with(&[("batch", value_only(Value::integer(3)))]));
    assert!(out.contains("  batch: 3\n"), "{out}");
    assert_eq!(
        parse(&out).properties[&id("batch")].value,
        Some(Value::integer(3))
    );
}

#[test]
fn a_boolean_round_trips_unquoted() {
    let out = write(&with(&[("cold_crashed", value_only(Value::boolean(true)))]));
    assert!(out.contains("  cold_crashed: true\n"), "{out}");
    assert_eq!(
        parse(&out).properties[&id("cold_crashed")].value,
        Some(Value::boolean(true))
    );
}

#[test]
fn a_date_round_trips_as_iso() {
    // Unquoted, and read back as a Date rather than as text.
    let date = Date::new(2026, 3, 14).unwrap();
    let out = write(&with(&[("brewed_on", value_only(Value::date(date)))]));
    assert!(out.contains("  brewed_on: 2026-03-14\n"), "{out}");
    assert_eq!(
        parse(&out).properties[&id("brewed_on")].value,
        Some(Value::date(date))
    );
}

#[test]
fn a_date_time_is_written_in_canonical_local_form() {
    let mut schema = empty();
    schema.attributes.insert(
        id("measured_at"),
        Value::date_time(DateTime::parse("2026-03-14 10:30:15").unwrap()).into(),
    );
    let out = write(&schema);
    assert!(out.contains("measured_at: 2026-03-14T10:30:15\n"), "{out}");
    assert_eq!(parse(&out), schema);
}

#[test]
fn a_list_attribute_is_a_flow_sequence_in_order() {
    let mut schema = empty();
    schema.attributes.insert(
        id("temperatures"),
        AttributeValue::list(vec![Value::integer(20), number(30.5), Value::integer(20)]).unwrap(),
    );
    let out = write(&schema);
    assert!(out.contains("temperatures: [20, 30.5, 20]\n"), "{out}");
    assert_eq!(parse(&out), schema);
}

#[test]
fn empty_and_absent_attributes_are_omitted() {
    let mut schema = empty();
    schema
        .attributes
        .insert(id("comment"), Value::absent().into());
    schema.attributes.insert(
        id("temperatures"),
        AttributeValue::list(Vec::new()).unwrap(),
    );
    assert_eq!(write(&schema), "schema_version: 1\n");
}

#[test]
fn ambiguous_strings_are_quoted() {
    // Through this format's own reader and through a YAML 1.1 one.
    for text in [
        "no", "yes", "null", "true", "1.5", "3", "on", "off", " padded",
    ] {
        let out = write(&with(&[("status", value_only(Value::text(text)))]));
        assert!(
            out.contains(&format!("\"{text}\"")),
            "{text} unquoted in {out}"
        );
        assert_eq!(
            parse(&out).properties[&id("status")].value,
            Some(Value::text(text)),
            "{text} did not survive"
        );
    }
}

#[test]
fn unicode_is_not_escaped() {
    // `é` stays `é`.
    let out = write(&with(&[(
        "symbol_name",
        value_only(Value::text("été_max é 中文")),
    )]));
    assert!(out.contains("été_max é 中文"), "{out}");
    assert!(!out.contains("\\u"), "{out}");
}

#[test]
fn display_precision_does_not_round_storage() {
    // A `.2f` precision does not truncate the stored value.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(12.498_700_000_000_001)),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("malt", property)]));
    assert!(out.contains("12.498700000000001"), "{out}");
}

/// A channel has **one** spelling, in the file, in an address, and in the bytes
/// that are hashed.
///
/// `value` and `uncertainty` are not read at all, and a file holding them is
/// refused naming its short form: `schema`'s test says so.
#[test]
fn the_written_form_is_short() {
    let written = "schema_version: 1\nproperties:\n  malt: {v: 12.5, u: 0.05}\n";
    let out = write(&parse(written));
    assert!(out.contains("v: 12.5"), "{out}");
    assert!(out.contains("u: 0.05"), "{out}");
    assert!(!out.contains("value:"), "{out}");
    assert!(!out.contains("uncertainty:"), "{out}");
    // The pre-image spells them the same way: one vocabulary in the bytes a
    // reader sees and in the bytes that are hashed.
    let property = parse(written).properties.values().next().unwrap().clone();
    assert_eq!(value_form(&property), "{v: 12.5, u: 0.05}");
}

#[test]
fn tags_are_a_flow_sequence_in_declaration_order() {
    // Not sorted, not block style.
    let mut schema = empty();
    schema.tags = vec![
        samplekit::format::schema::Tag::Usable(id("zeta")),
        samplekit::format::schema::Tag::Usable(id("alpha")),
        samplekit::format::schema::Tag::Usable(id("mu")),
    ];
    let out = write(&schema);
    assert!(out.contains("tags: [zeta, alpha, mu]\n"), "{out}");
}

// ---------------------------------------------------------------- records

#[test]
fn an_attribute_input_is_written_as_its_value() {
    // And a property input beside it as its digest, in one map.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(3.05)),
        computed: Some(record(&[
            (InputName::Named(id("malt")), Value::text("9f2c14ab77d0")),
            (InputName::Named(id("batch")), Value::integer(7)),
        ])),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("plato", property)]));
    assert!(
        out.contains("computed: {malt: 9f2c14ab77d0, batch: 7}"),
        "{out}"
    );
}

#[test]
fn a_digest_shaped_attribute_value_round_trips() {
    // The name decides the form, so a value that looks like a hash survives.
    let property = PropertySchema {
        statistics: None,
        value: Some(number(3.05)),
        computed: Some(record(&[(
            InputName::Named(id("serial")),
            Value::text("9f2c14ab77d0"),
        )])),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("plato", property)]));
    let back = parse(&out);
    assert_eq!(
        back.properties[&id("plato")]
            .computed
            .as_ref()
            .unwrap()
            .inputs()[&InputName::Named(id("serial"))],
        Recorded::Value(Value::text("9f2c14ab77d0"))
    );
}

#[test]
fn a_cell_record_marks_the_scope_of_each_input() {
    // `row.wort` and `foam` in one map, and a column and a property
    // sharing a name stay apart.
    let computed = record(&[
        (InputName::Cell(id("wort")), Value::text("a91f42c8f0d1")),
        (InputName::Named(id("foam")), Value::text("3f2a1b09c4d5")),
    ]);
    let cell = PropertySchema {
        statistics: None,
        value: Some(number(0.08)),
        computed: Some(computed),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("clarity", cell)]));
    assert!(out.contains("row.wort: a91f42c8f0d1"), "{out}");
    assert!(out.contains("foam: 3f2a1b09c4d5"), "{out}");
    let back = parse(&out).properties[&id("clarity")]
        .computed
        .clone()
        .unwrap()
        .inputs();
    assert!(back.contains_key(&InputName::Cell(id("wort"))));
    assert!(back.contains_key(&InputName::Named(id("foam"))));
}

#[test]
fn a_whole_column_input_always_names_its_table() {
    // Never shortened, because a bare name is the cask's.
    let computed = record(&[(
        InputName::Column {
            table: id("mashing"),
            column: id("wort"),
        },
        Value::text("a91f42c8f0d1"),
    )]);
    let property = PropertySchema {
        statistics: None,
        value: Some(number(1.0)),
        computed: Some(computed),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("mean_wort", property)]));
    assert!(out.contains("mashing.wort: a91f42c8f0d1"), "{out}");
    let back = parse(&out).properties[&id("mean_wort")]
        .computed
        .clone()
        .unwrap()
        .inputs();
    assert!(back.contains_key(&InputName::Column {
        table: id("mashing"),
        column: id("wort")
    }));
}

// ----------------------------------------------------------------- tables

fn boil(index: Vec<Identifier>) -> TableSchema {
    let mut columns = IndexMap::new();
    columns.insert(id("temperature"), ColumnSchema::default());
    if index.len() > 1 {
        columns.insert(id("dextrin"), ColumnSchema::default());
    }
    columns.insert(
        id("wort"),
        ColumnSchema {
            unit: Some("lintner".to_string()),
            symbol: None,
            statistics: None,
        },
    );
    // A table without a row is not written: one row, one value per column.
    let row = columns
        .keys()
        .map(|column| (column.clone(), value_only(number(1.0))))
        .collect();
    TableSchema {
        title: None,
        index,
        columns,
        rows: vec![row],
    }
}

#[test]
fn a_single_index_column_is_written_bare() {
    // `index: temperature`, never `index: [temperature]`.
    let mut schema = empty();
    schema
        .tables
        .insert(id("mashing"), boil(vec![id("temperature")]));
    let out = write(&schema);
    assert!(out.contains("    index: temperature\n"), "{out}");
    assert_eq!(
        parse(&out).tables[&id("mashing")].index,
        vec![id("temperature")]
    );
}

#[test]
fn a_composite_index_is_written_as_a_sequence() {
    let mut schema = empty();
    schema
        .tables
        .insert(id("mashing"), boil(vec![id("temperature"), id("dextrin")]));
    let out = write(&schema);
    assert!(out.contains("    index: [temperature, dextrin]\n"), "{out}");
    assert_eq!(
        parse(&out).tables[&id("mashing")].index,
        vec![id("temperature"), id("dextrin")]
    );
}

#[test]
fn cells_are_written_in_column_order() {
    // The owner's pre-1 collection sorted every row's cells alphabetically
    // while declaring its columns in experiment order. One table, one order.
    let mut table = boil(vec![id("temperature")]);
    table.rows.clear();
    let mut row = IndexMap::new();
    row.insert(id("wort"), value_only(number(12.5)));
    row.insert(id("temperature"), value_only(number(65.0)));
    // Not a declared column: written last rather than lost, for the next read
    // to refuse by name.
    row.insert(id("stray"), value_only(number(1.0)));
    table.rows.push(row);
    let mut schema = empty();
    schema.tables.insert(id("mashing"), table);

    let out = write(&schema);
    let written: Vec<&str> = out
        .lines()
        .skip_while(|line| !line.contains("rows:"))
        .skip(1)
        .filter_map(|line| line.trim_start_matches(['-', ' ']).split(':').next())
        .filter(|name| !name.is_empty())
        .collect();
    assert_eq!(written, ["temperature", "wort", "stray"], "{out}");
}

#[test]
fn a_derived_cell_carries_its_own_record() {
    // Each row's cell holds its own `computed` and `fingerprint`.
    let mut table = boil(vec![id("temperature")]);
    table.rows.clear();
    table.columns.insert(id("clarity"), ColumnSchema::default());
    for (t, r, c, digest) in [
        (65.0, 12.5, 0.08, "aaaa11112222"),
        (78.5, 20.0, 0.05, "bbbb33334444"),
    ] {
        let mut row = IndexMap::new();
        row.insert(id("temperature"), value_only(number(t)));
        row.insert(id("wort"), value_only(number(r)));
        row.insert(
            id("clarity"),
            PropertySchema {
                statistics: None,
                value: Some(number(c)),
                computed: Some(record(&[(
                    InputName::Cell(id("wort")),
                    Value::text(digest),
                )])),
                ..PropertySchema::default()
            },
        );
        table.rows.push(row);
    }
    let mut schema = empty();
    schema.tables.insert(id("mashing"), table);
    let out = write(&schema);
    assert!(out.contains("aaaa11112222"), "{out}");
    assert!(out.contains("bbbb33334444"), "{out}");
    let back = parse(&out);
    let rows = &back.tables[&id("mashing")].rows;
    assert_eq!(rows.len(), 2);
    assert!(rows[0][&id("clarity")].computed.is_some());
    assert_eq!(rows[1][&id("temperature")].value, Some(number(78.5)));
}

// -------------------------------------------------------------- value form

#[test]
fn value_form_ignores_presentation_fields() {
    // Adding a unit or a symbol leaves the hashed bytes identical.
    let bare = PropertySchema {
        statistics: None,
        value: Some(number(12.5)),
        uncertainty: Some(0.05),
        ..PropertySchema::default()
    };
    let dressed = PropertySchema {
        statistics: None,
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        fingerprint: Some(Fingerprint::new("5b2c9e1f0a34")),
        ..bare.clone()
    };
    assert_eq!(value_form(&bare), value_form(&dressed));
    assert_eq!(value_form(&bare), "{v: 12.5, u: 0.05}");
}

#[test]
fn value_form_never_uses_the_scalar_shorthand() {
    // A value-only property and a value-plus-unit property hash the same.
    let plain = value_only(number(12.5));
    let with_unit = PropertySchema {
        statistics: None,
        unit: Some("g".to_string()),
        ..plain.clone()
    };
    assert_eq!(value_form(&plain), "{v: 12.5}");
    assert_eq!(value_form(&plain), value_form(&with_unit));
    assert!(value_form(&plain).starts_with('{'));
}

// ------------------------------------------------------------ full circle

#[test]
fn writing_is_lossless() {
    // parse(write(x)) == x: stronger than idempotence, which only says two
    // writes agree. This says nothing was dropped between them.
    let sample = {
        let mut schema = with(&[("malt", malt())]);
        schema.name = Some("Keg 42".to_string());
        schema.tags = vec![samplekit::format::schema::Tag::Usable(id("reference"))];
        schema
            .attributes
            .insert(id("batch"), Value::integer(7).into());
        schema
    };
    let out = write(&sample);
    let back = parse(&out);
    assert_eq!(back, sample);
    assert_eq!(schema::version(), back.schema_version);
}

/// A record mixes what the property reads with what one formula reads, and the
/// two are told apart key by key.
#[test]
fn a_record_mixes_common_inputs_with_a_channel_s_own() {
    let mixed = parse(
        "schema_version: 1\nproperties:\n  p:\n    v: 200.0\n    u: 1.0\n    computed: {src: 1bfcd9d65091, u: {p.v: 9d6aff8f12cd}}\n",
    );
    let computed = mixed.properties[&id("p")].computed.clone().unwrap();
    // What both formulas read stays plain; what the uncertainty alone reads is
    // under its channel.
    assert!(
        computed
            .quantity
            .as_ref()
            .unwrap()
            .contains_key(&InputName::Named(id("src")))
    );
    assert!(computed.value.is_none());
    let own = computed.uncertainty.as_ref().unwrap();
    assert_eq!(own.len(), 1, "{own:?}");
    // And `inputs()` still answers the whole question, for the callers that
    // only ask *what does this read*.
    assert_eq!(computed.inputs().len(), 2, "{computed:?}");

    // The common ones are written first, then each channel's own, and the
    // whole survives a round trip.
    let out = write(&mixed);
    assert!(
        out.contains("computed: {src: 1bfcd9d65091, u: {p.v: 9d6aff8f12cd}}"),
        "{out}"
    );
    assert_eq!(write(&parse(&out)), out, "{out}");
}

/// A record tells a channel from an input by what the key **holds**, not by
/// what it is called.
#[test]
fn a_record_names_a_channel_or_an_input() {
    // A channel's key holds a map of inputs.
    let keyed = parse(
        "schema_version: 1\nproperties:\n  d:\n    v: 20.0\n    computed: {v: {src: 1bfcd9d65091}, u: {src: 1bfcd9d65091}}\n",
    );
    let computed = keyed.properties[&id("d")].computed.clone().unwrap();
    assert!(computed.names_channels(), "{computed:?}");
    assert!(computed.value.is_some() && computed.uncertainty.is_some());
    assert!(computed.quantity.is_none());
    assert_eq!(computed.inputs().len(), 1, "{computed:?}");

    // One channel alone: the value is entered, the uncertainty computed.
    let one = parse("schema_version: 1\nproperties:\n  a:\n    v: 1.0\n    computed: {u: {}}\n");
    let computed = one.properties[&id("a")].computed.clone().unwrap();
    assert!(computed.value.is_none() && computed.uncertainty.is_some());

    // An input's key holds a digest, and `v` is a name a property may have.
    let flat = parse(
        "schema_version: 1\nproperties:\n  e:\n    v: 30.0\n    computed: {v: 1bfcd9d65091}\n",
    );
    let computed = flat.properties[&id("e")].computed.clone().unwrap();
    assert!(!computed.names_channels(), "{computed:?}");
    assert!(computed.inputs().contains_key(&InputName::Named(id("v"))));

    // And both shapes survive a round trip.
    for written in [
        "schema_version: 1\nproperties:\n  d:\n    v: 20.0\n    computed: {v: {src: 1bfcd9d65091}}\n",
        "schema_version: 1\nproperties:\n  e:\n    v: 30.0\n    computed: {src: 1bfcd9d65091}\n",
    ] {
        let out = write(&parse(written));
        assert_eq!(write(&parse(&out)), out, "{out}");
    }
}

#[test]
fn an_edited_mark_is_a_one_key_mapping() {
    let mut held = inputs(&[(InputName::Named(id("volume")), Value::text("d312b70e4a55"))]);
    held.insert(
        InputName::Named(id("grist")),
        Recorded::Edited(Fingerprint::edited("3f2a1b09c4d1")),
    );
    let computed = Computed::from(held);
    let property = PropertySchema {
        statistics: None,
        value: Some(number(1.52)),
        computed: Some(computed),
        fingerprint: Some(Fingerprint::edited("7c01e2b3a9f4")),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("brix", property)]));
    assert!(
        out.contains("computed: {volume: d312b70e4a55, grist: {edited: 3f2a1b09c4d1}}"),
        "{out}"
    );
    assert!(out.contains("fingerprint: {edited: 7c01e2b3a9f4}"), "{out}");

    let back = &parse(&out).properties[&id("brix")];
    let fingerprint = back.fingerprint.clone().unwrap();
    assert!(fingerprint.is_edited());
    assert_eq!(fingerprint.as_str(), "7c01e2b3a9f4");
    let records = back.computed.as_ref().unwrap().inputs();
    assert_eq!(
        records[&InputName::Named(id("grist"))],
        Recorded::Edited(Fingerprint::edited("3f2a1b09c4d1"))
    );
    assert_eq!(
        records[&InputName::Named(id("volume"))],
        Recorded::Value(Value::text("d312b70e4a55"))
    );
    assert_eq!(write(&parse(&out)), out);
}

#[test]
fn a_digest_that_reads_as_a_number_is_quoted() {
    let mut held = inputs(&[(InputName::Named(id("volume")), Value::text("972327790827"))]);
    held.insert(
        InputName::Named(id("grist")),
        Recorded::Edited(Fingerprint::edited("844409772e92")),
    );
    let computed = Computed::from(held);
    let property = PropertySchema {
        statistics: None,
        value: Some(number(1.52)),
        computed: Some(computed),
        fingerprint: Some(Fingerprint::new("000000000123")),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("brix", property)]));
    assert!(
        out.contains(r#"computed: {volume: "972327790827", grist: {edited: "844409772e92"}}"#),
        "{out}"
    );
    assert!(out.contains(r#"fingerprint: "000000000123""#), "{out}");
    let back = &parse(&out).properties[&id("brix")];
    assert_eq!(back.fingerprint.clone().unwrap().as_str(), "000000000123");
    let records = back.computed.as_ref().unwrap().inputs();
    assert_eq!(
        records[&InputName::Named(id("grist"))],
        Recorded::Edited(Fingerprint::edited("844409772e92"))
    );
    assert_eq!(
        records[&InputName::Named(id("volume"))],
        Recorded::Value(Value::text("972327790827"))
    );
}

#[test]
fn a_digest_written_unquoted_still_reads() {
    let plain = parse(
        "schema_version: 1\nproperties:\n  brix:\n    v: 1.52\n    computed: {volume: d312b70e4a55}\n    fingerprint: 012345678901\n",
    );
    let fingerprint = plain.properties[&id("brix")].fingerprint.clone().unwrap();
    assert_eq!(fingerprint.as_str(), "012345678901");
    let marked = parse(
        "schema_version: 1\nproperties:\n  brix:\n    v: 1.52\n    computed: {grist: {edited: 844409772e92}}\n    fingerprint: {edited: 844409772e92}\n",
    );
    let property = &marked.properties[&id("brix")];
    assert!(property.fingerprint.clone().unwrap().is_edited());
    assert!(matches!(
        property.computed.as_ref().unwrap().inputs()[&InputName::Named(id("grist"))],
        Recorded::Edited(_)
    ));
}

#[test]
fn a_wrapped_record_ending_with_an_edited_mark_reads_back() {
    let mut held = inputs(&[
        (
            InputName::Named(id("mash_malt")),
            Value::text("d312b70e4a55"),
        ),
        (
            InputName::Named(id("kettle_malt")),
            Value::text("a91f42c8f0d1"),
        ),
        (
            InputName::Named(id("fermentable_fraction")),
            Value::text("5b2c9e1f0a34"),
        ),
    ]);
    held.insert(
        InputName::Named(id("fermentable_malt")),
        Recorded::Edited(Fingerprint::edited("3f2a1b09c4d1")),
    );
    let computed = Computed::from(held);
    let property = PropertySchema {
        statistics: None,
        value: Some(number(1.52)),
        computed: Some(computed.clone()),
        fingerprint: Some(Fingerprint::new("7c01e2b3a9f4")),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("expected_volume", property)]));
    let back = &parse(&out).properties[&id("expected_volume")];
    assert_eq!(back.computed.as_ref().unwrap(), &computed, "{out}");
}

#[test]
fn an_integer_and_an_equal_number_hash_alike() {
    assert_eq!(
        value_form(&value_only(Value::integer(200))),
        value_form(&value_only(number(200.0)))
    );
    assert_ne!(
        value_form(&value_only(Value::integer(200))),
        value_form(&value_only(number(200.5)))
    );
}

#[test]
fn a_derived_cell_is_written_on_one_line() {
    let written = "---\nschema_version: 1\ntables:\n  mashing:\n    index: T\n    columns:\n      T: {}\n      G: {}\n    rows:\n      - T: 10\n        G:\n          v: 0.5\n          computed: {row.R: d312b70e4a55}\n          fingerprint: 7c01e2b3a9f4\n---\n";
    let frontmatter = &written[4..written.len() - 4];
    let schema = parse(frontmatter);
    let out = write(&schema);
    assert!(
        out.contains(
            "        G: {v: 0.5, computed: {row.R: d312b70e4a55}, fingerprint: 7c01e2b3a9f4}\n"
        ),
        "{out}"
    );
    assert_eq!(parse(&out), schema, "{out}");
}

#[test]
fn a_failure_is_spelled_like_an_override() {
    let property = PropertySchema {
        statistics: None,
        computed: Some(record(&[(
            InputName::Named(id("volume")),
            Value::text("d312b70e4a55"),
        )])),
        failure: Some("ZeroDivisionError: division by zero".to_string()),
        ..PropertySchema::default()
    };
    let out = write(&with(&[("plato", property.clone())]));
    assert!(
        out.contains(r#"fingerprint: {failed: "ZeroDivisionError: division by zero"}"#),
        "{out}"
    );
    assert!(!out.contains("value:"), "{out}");
    assert_eq!(parse(&out).properties[&id("plato")], property, "{out}");
}

#[test]
fn an_empty_property_or_table_is_not_written() {
    let mut schema = with(&[
        ("malt", value_only(number(1.0))),
        ("volume", PropertySchema::default()),
    ]);
    let out = write(&schema);
    assert!(!out.contains("volume"), "{out}");
    let table = parse("schema_version: 1\ntables:\n  mashing:\n    index: T\n    columns:\n      T: {}\n    rows: []\n")
        .tables[&id("mashing")]
        .clone();
    schema.tables.insert(id("mashing"), table);
    let out = write(&schema);
    assert!(!out.contains("mashing"), "{out}");
}

#[test]
fn a_date_time_keeps_its_fraction_of_a_second_through_a_write() {
    let schema = parse("schema_version: 1\nend_time: 2026-01-24T09:42:17.204931\n");
    let out = write(&schema);
    assert!(
        out.contains("end_time: 2026-01-24T09:42:17.204931"),
        "{out}"
    );
    assert_eq!(parse(&out), schema, "{out}");
}

/// A record naming both channels and nothing common says which formula read
/// what, and keeps saying it through a load and a save.
#[test]
fn a_record_of_two_channels_with_nothing_common_survives_a_save() {
    let written = "---\nschema_version: 1\nname: A\nproperties:\n  a: {v: 1.0}\n  b: {v: 2.0}\n  \
        p:\n    v: 2.0\n    u: 0.1\n    computed: {v: {a: 012bac967364}, u: {b: 8babce7201f4}}\n---\n";
    let document = samplekit::format::document::parse(written).unwrap();
    let sample = samplekit::format::schema::into_sample(document.schema).unwrap();
    let records = sample.property(&id("p")).unwrap().records();
    assert_eq!(records.produced, None);
    assert_eq!(records.channel_only.len(), 2, "{:?}", records.channel_only);
    let again = samplekit::format::schema::from_sample(&sample).unwrap();
    let out = write(&again);
    assert!(
        out.contains("computed: {v: {a: 012bac967364}, u: {b: 8babce7201f4}}"),
        "{out}"
    );
}

#[test]
fn a_columns_statistics_are_written_with_it() {
    // A column says once, for every cell, which statistic of a cell's readings
    // stands for each channel — after its presentation, as a property's
    // statistics follow its unit.
    let mut schema = empty();
    let mut table = boil(vec![id("temperature")]);
    table.columns[&id("wort")].statistics = Some(schema::Statistics {
        value: Some(samplekit::core::statistics::Location::Mean),
        uncertainty: Some(samplekit::core::uncertainty::Convention::StandardError),
    });
    schema.tables.insert(id("mashing"), table);
    let out = write(&schema);
    assert!(
        out.contains("wort: {unit: lintner, statistics: {v: mean, u: standard_error}}"),
        "{out}"
    );
    assert_eq!(write(&parse(&out)), out);
}
