//! The tests of `schema`.

use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{
    Compute, ComputeError, Fingerprint, InputName, InputRecord, Property, Records,
};
use samplekit::core::sample::{AttributeValue, Sample, SampleError};
use samplekit::core::uncertainty::Uncertainty;
use samplekit::core::value::{Date, DateTime, Readings, Value};
use samplekit::format::schema::{self, PrecisionSchema, PropertySchema, SampleSchema, SchemaError};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn parse(source: &str) -> Result<SampleSchema, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(source)
}

fn read(sample: &Sample, name: &str) -> Value {
    sample.property(&id(name)).unwrap().value().unwrap()
}

/// A formula, for the two tests about what a file does with one.
struct Fixed(f64);

impl Compute for Fixed {
    fn compute(&self) -> Result<Value, ComputeError> {
        Ok(number(self.0))
    }
}

/// A cask with one of everything the shape can carry.
fn cask() -> Sample {
    let mut sample = Sample::new();
    sample.set_name(Some("Keg 42".to_string()));
    sample.set_tags(vec![id("reference"), id("cold_crashed")]);
    sample
        .set_attribute(id("batch"), Value::integer(7))
        .unwrap();
    sample
        .set_attribute(
            id("brewed_on"),
            Value::date(Date::new(2026, 3, 14).unwrap()),
        )
        .unwrap();

    let mut malt = Property::stored(number(12.487));
    malt.set_uncertainty(Some(Uncertainty::new(0.005).unwrap()));
    malt.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        precision: None,
    });
    sample.set_property(id("malt"), malt).unwrap();

    sample
        .set_property(
            id("foam"),
            Property::measured(Readings::new(vec![2.011, 2.017, 2.013]).unwrap(), None),
        )
        .unwrap();
    sample
        .set_property(id("finish"), Property::stored(Value::text("unfiltered")))
        .unwrap();
    sample
        .set_property(id("haze"), Property::stored(Value::absent()))
        .unwrap();
    sample
}

fn round_trip(sample: &Sample) -> Sample {
    schema::into_sample(schema::from_sample(sample).unwrap()).unwrap()
}

// -------------------------------------------------------------- round trips

#[test]
fn round_trip_preserves_values() {
    let back = round_trip(&cask());
    assert_eq!(read(&back, "malt"), number(12.487));
    assert_eq!(read(&back, "finish"), Value::text("unfiltered"));
    assert_eq!(back.name(), Some("Keg 42"));
    assert_eq!(back.attribute(&id("batch")).unwrap(), &Value::integer(7));
    assert_eq!(
        back.attribute(&id("brewed_on")).unwrap(),
        &Value::date(Date::new(2026, 3, 14).unwrap())
    );
    let malt = back.property(&id("malt")).unwrap();
    assert_eq!(malt.uncertainty().unwrap().unwrap().magnitude(), 0.005);
}

#[test]
fn round_trip_preserves_observations() {
    // Raw readings survive, not only their mean.
    let back = round_trip(&cask());
    let readings = back.property(&id("foam")).unwrap().readings().unwrap();
    assert_eq!(readings.as_slice(), [2.011, 2.017, 2.013]);
}

#[test]
fn an_equal_precision_pair_is_read_as_one() {
    // Two equal precisions must compare equal: `[.3f, .3f]` and `.3f` say one
    // thing. A precision is declared in the project, so it is read there.
    #[derive(serde::Deserialize)]
    struct Declared {
        malt: PrecisionSchema,
        volume: PrecisionSchema,
    }
    let parsed: Declared =
        toml::from_str("malt = [\".3f\", \".3f\"]\nvolume = [\".3f\", \".1e\"]\n").unwrap();
    assert_eq!(parsed.malt, PrecisionSchema::Both(".3f".to_string()));
    assert_eq!(
        parsed.volume,
        PrecisionSchema::Split(".3f".to_string(), ".1e".to_string())
    );
}

#[test]
fn a_precision_in_a_file_is_an_unknown_field() {
    // A precision is the project's: in a property, a column or a cell it is
    // refused like any field the format does not have, never dropped.
    for source in [
        "schema_version: 1\nproperties:\n  malt: {v: 12.5, precision: .3f}\n",
        "schema_version: 1\ntables:\n  conditioning:\n    index: day\n    columns:\n      \
         day: {}\n      volume_left: {unit: vol, precision: .2f}\n    rows:\n      \
         - {day: 1, volume_left: 2.5}\n",
        "schema_version: 1\ntables:\n  conditioning:\n    index: day\n    columns:\n      \
         day: {}\n      volume_left: {}\n    rows:\n      \
         - {day: 1, volume_left: {v: 2.5, precision: .2f}}\n",
    ] {
        let file = format!("---\n{source}---\n");
        let error = samplekit::format::document::parse(&file)
            .unwrap_err()
            .to_string();
        assert!(error.contains("holds no precision"), "{error}");
        assert!(error.contains(".samplekitrc"), "{error}");
        assert!(!error.contains("expected one of"), "{error}");
    }
}

#[test]
fn round_trip_preserves_display_metadata() {
    let back = round_trip(&cask());
    let presentation = back.property(&id("malt")).unwrap().presentation();
    assert_eq!(presentation.unit.as_deref(), Some("g"));
    assert_eq!(presentation.symbol.as_deref(), Some("m"));
}

#[test]
fn round_trip_preserves_declaration_order() {
    let back = round_trip(&cask());
    let order: Vec<String> = back
        .property_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    assert_eq!(order, ["malt", "foam", "finish", "haze"]);
    let attributes: Vec<String> = back
        .attribute_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    assert_eq!(attributes, ["batch", "brewed_on"]);
}

#[test]
fn round_trip_preserves_tags_and_their_order() {
    // Neither sorted nor deduplicated.
    let back = round_trip(&cask());
    assert_eq!(back.tags(), [id("reference"), id("cold_crashed")]);
}

#[test]
fn round_trip_preserves_absent_values() {
    // Absent does not become 0 or empty text.
    let back = round_trip(&cask());
    assert_eq!(read(&back, "haze"), Value::absent());
    assert!(read(&back, "haze").is_absent());
    // And the shape omits it rather than writing a placeholder.
    let shape = schema::from_sample(&cask()).unwrap();
    assert_eq!(shape.properties[&id("haze")].value, None);
}

#[test]
fn round_trip_drops_formulas() {
    // Their absence is specified, not accidental.
    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    sample.materialize().unwrap();
    let back = round_trip(&sample);
    assert_eq!(read(&back, "plato"), number(3.05));
    assert!(!back.property(&id("plato")).unwrap().is_computed());
}

#[test]
fn unresolved_value_cannot_be_saved() {
    // A lazy property refuses conversion, naming itself.
    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Fixed(3.05))))
        .unwrap();
    assert_eq!(
        schema::from_sample(&sample).unwrap_err(),
        SchemaError::UnresolvedValue {
            property: id("plato")
        }
    );
}

// ----------------------------------------------------------------- records

fn with_records() -> Sample {
    let mut sample = cask();
    let plato = {
        let mut computed = IndexMap::new();
        computed.insert(
            InputName::Named(id("malt")),
            InputRecord::Digest(Fingerprint::new("9f2c14ab77d0")),
        );
        computed.insert(
            InputName::Named(id("batch")),
            InputRecord::Literal(Value::integer(7)),
        );
        let mut property = Property::stored(number(3.05));
        property.set_records(Records {
            produced: None,
            channel_only: Default::default(),
            statistics: None,
            failure: None,
            fingerprint: Some(Fingerprint::new("5b2c9e1f0a34")),
            computed: Some(computed),
        });
        property
    };
    sample.set_property(id("plato"), plato).unwrap();
    // An input carries a fingerprint and no `computed`.
    let malt = sample.property(&id("malt")).unwrap();
    malt.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::new("9f2c14ab77d0")),
        computed: None,
    });
    sample
}

#[test]
fn round_trip_preserves_both_records() {
    let back = round_trip(&with_records());
    let records = back.property(&id("plato")).unwrap().records();
    assert_eq!(records.fingerprint, Some(Fingerprint::new("5b2c9e1f0a34")));
    let computed = records.computed.unwrap();
    // A property input keeps its digest; an attribute input keeps its value.
    assert_eq!(
        computed[&InputName::Named(id("malt"))],
        InputRecord::Digest(Fingerprint::new("9f2c14ab77d0"))
    );
    assert_eq!(
        computed[&InputName::Named(id("batch"))],
        InputRecord::Literal(Value::integer(7))
    );
}

#[test]
fn an_input_carries_a_fingerprint_without_being_computed() {
    // The two records are on different sets.
    let back = round_trip(&with_records());
    let malt = back.property(&id("malt")).unwrap().records();
    assert_eq!(malt.fingerprint, Some(Fingerprint::new("9f2c14ab77d0")));
    assert_eq!(malt.computed, None);
}

#[test]
fn computed_naming_an_absent_property_is_accepted() {
    // The shape does not validate references: `fingerprint` reports it broken,
    // and a file that could not be loaded could not be repaired.
    let shape = parse(
        r#"
schema_version: 1
properties:
  plato:
    v: 3.05
    computed: {vanished: 9f2c14ab77d0}
"#,
    )
    .unwrap();
    let sample = schema::into_sample(shape).unwrap();
    let computed = sample
        .property(&id("plato"))
        .unwrap()
        .records()
        .computed
        .unwrap();
    assert!(computed.contains_key(&InputName::Named(id("vanished"))));
    assert!(!sample.has_property(&id("vanished")));
}

// ---------------------------------------------------------------- parsing

#[test]
fn short_aliases_parse_to_the_long_fields() {
    let shape =
        parse("schema_version: 1\nproperties:\n  malt: {v: 12.5, u: 0.05, unit: g}\n").unwrap();
    let malt = &shape.properties[&id("malt")];
    assert_eq!(malt.value, Some(number(12.5)));
    assert_eq!(malt.uncertainty, Some(0.05));
    assert_eq!(malt.unit.as_deref(), Some("g"));
}

#[test]
fn a_long_spelling_is_refused_naming_its_short_form() {
    // `value` and `uncertainty` are not read — in a property, a cell, a
    // statistic or a computed record — and the refusal names the key and what
    // it is written now, rather than listing every field a quantity has.
    for (source, key, now) in [
        (
            "schema_version: 1\nproperties:\n  malt: {value: 12.5, unit: g}\n",
            "value",
            "v",
        ),
        (
            "schema_version: 1\nproperties:\n  malt: {v: 12.5, uncertainty: 0.05}\n",
            "uncertainty",
            "u",
        ),
        (
            "schema_version: 1\ntables:\n  conditioning:\n    index: day\n    columns:\n      \
             day: {}\n      volume_left: {}\n    rows:\n      \
             - {day: 1, volume_left: {value: 2.5}}\n",
            "value",
            "v",
        ),
        (
            "schema_version: 1\nproperties:\n  malt: {readings: [1, 2], \
             statistics: {v: mean, uncertainty: standard_error}}\n",
            "uncertainty",
            "u",
        ),
        (
            "schema_version: 1\nproperties:\n  malt: {v: 2, computed: {value: {a: 1}}}\n",
            "value",
            "v",
        ),
    ] {
        let file = format!("---\n{source}---\n");
        let error = samplekit::format::document::parse(&file)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("'{key}' is written '{now}'")),
            "{error}"
        );
        assert!(error.contains("rename it"), "{error}");
        assert!(!error.contains("expected one of"), "{error}");
    }
    // A key that merely contains one is not one.
    assert!(parse("schema_version: 1\nvalues: [1, 2]\n").is_ok());
}

#[test]
fn a_date_shaped_scalar_is_a_date() {
    // Quoted or not, and every other scalar keeps its kind.
    let shape = parse(
        r#"
schema_version: 1
plain: 2026-03-14
quoted: "2026-03-14"
flag: true
count: 7
measure: 1.5
label: unfiltered
also_text: "1.5"
"#,
    )
    .unwrap();
    let date = Value::date(Date::new(2026, 3, 14).unwrap());
    assert_eq!(shape.attributes[&id("plain")], date);
    assert_eq!(shape.attributes[&id("quoted")], date);
    assert_eq!(shape.attributes[&id("flag")], Value::boolean(true));
    assert_eq!(shape.attributes[&id("count")], Value::integer(7));
    assert_eq!(shape.attributes[&id("measure")], number(1.5));
    assert_eq!(shape.attributes[&id("label")], Value::text("unfiltered"));
    assert_eq!(shape.attributes[&id("also_text")], Value::text("1.5"));
}

#[test]
fn a_date_time_shaped_scalar_is_a_date_time() {
    let shape = parse("schema_version: 1\nmeasured_at: 2026-03-14 10:30:15\n").unwrap();
    assert_eq!(
        shape.attributes[&id("measured_at")],
        AttributeValue::scalar(Value::date_time(
            DateTime::parse("2026-03-14T10:30:15").unwrap()
        ))
    );
    assert!(parse("schema_version: 1\nmeasured_at: 2026-03-14T10:30Z\n").is_err());
}

#[test]
fn round_trip_preserves_list_attributes() {
    let mut sample = Sample::new();
    let values = vec![Value::integer(20), number(30.5), Value::integer(20)];
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(values.clone()).unwrap(),
        )
        .unwrap();
    let back = round_trip(&sample);
    assert_eq!(
        back.attribute(&id("temperatures")).unwrap().as_list(),
        Some(values.as_slice())
    );
}

#[test]
fn a_mixed_or_nested_attribute_list_is_refused() {
    let mixed = parse("schema_version: 1\nvalues: [1, text]\n").unwrap_err();
    assert!(mixed.to_string().contains("one kind"), "{mixed}");
    let nested = parse("schema_version: 1\nvalues: [[1], [2]]\n").unwrap_err();
    assert!(nested.to_string().contains("scalar"), "{nested}");
}

#[test]
fn a_leading_underscore_field_is_refused_at_conversion() {
    let shape = parse("schema_version: 1\n_hidden: 1\n").unwrap();
    let error = schema::into_sample(shape).unwrap_err();
    assert!(
        error.to_string().contains("private Python state"),
        "{error}"
    );
}

#[test]
fn future_version_is_refused() {
    // Naming both versions.
    let error = schema::check_version(2).unwrap_err();
    assert_eq!(
        error,
        SchemaError::UnsupportedVersion {
            found: 2,
            supported: 1..=1
        }
    );
    let message = error.to_string();
    assert!(message.contains('2') && message.contains('1'), "{message}");
    assert!(schema::supports(1));
    assert_eq!(schema::version(), 1);
}

#[test]
fn unknown_field_is_refused() {
    // An extra key inside a property fails rather than being dropped.
    let error = parse("schema_version: 1\nproperties:\n  malt: {v: 1.0, formula: malt/volume}\n")
        .unwrap_err();
    assert!(error.to_string().contains("formula"), "{error}");
}

#[test]
fn an_unknown_top_level_mapping_is_refused() {
    // `propertys:` fails on its shape, while `batch: 7` becomes an attribute.
    let error = parse("schema_version: 1\npropertys:\n  malt: 1.0\n").unwrap_err();
    assert!(error.to_string().contains("mapping"), "{error}");
    let fine = parse("schema_version: 1\nbatch: 7\n").unwrap();
    assert_eq!(fine.attributes[&id("batch")], Value::integer(7));
}

#[test]
fn invalid_key_is_refused() {
    // A key containing `@` fails at parse time, with the grammar in the message.
    let error = parse("schema_version: 1\nproperties:\n  mashing@293: 1.0\n").unwrap_err();
    let message = error.to_string();
    assert!(message.contains('@'), "{message}");
    assert!(
        message.contains("letters, digits and underscores"),
        "{message}"
    );
}

#[test]
fn a_property_colliding_with_an_attribute_is_refused_at_load() {
    // Through the sample's own message rather than a second one.
    let shape = parse("schema_version: 1\nbatch: 7\nproperties:\n  batch: 3\n").unwrap();
    let error = schema::into_sample(shape).unwrap_err();
    let SchemaError::Sample(SampleError::NameCollision { name, .. }) = &error else {
        panic!("expected a name collision, got {error:?}");
    };
    assert_eq!(name, &id("batch"));
    assert!(error.to_string().contains("attribute"), "{error}");
}

#[test]
fn no_field_can_hold_a_callable() {
    // A structural assertion: the shape refuses a field it does not have, so no
    // future change can add one without amending the note first.
    for field in ["formula", "callback", "compute", "python", "import_path"] {
        let source =
            format!("schema_version: 1\nproperties:\n  malt:\n    v: 1.0\n    {field}: x\n");
        assert!(parse(&source).is_err(), "{field} was accepted");
    }
    // And the eight fields it does have are exactly these.
    let shape = PropertySchema {
        statistics: None,
        failure: None,
        value: Some(number(1.0)),
        readings: None,
        uncertainty: None,
        unit: None,
        symbol: None,
        computed: None,
        fingerprint: None,
    };
    assert_eq!(shape.value, Some(number(1.0)));
}

#[test]
fn a_sample_written_as_it_stands_runs_no_formula() {
    use samplekit::core::property::{Compute, ComputeError};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Once(Cell<usize>);

    impl Compute for Once {
        fn compute(&self) -> Result<Value, ComputeError> {
            self.0.set(self.0.get() + 1);
            Ok(Value::number(2.5).unwrap())
        }
    }

    let formula = Rc::new(Once(Cell::new(0)));
    let mut sample = Sample::new();
    let name = Identifier::new("plato").unwrap();
    sample
        .set_property(name.clone(), Property::computed(formula.clone()))
        .unwrap();
    let unread = schema::from_sample_as_is(&sample).unwrap();
    assert_eq!(unread.properties[&name].value, None);
    assert_eq!(formula.0.get(), 0);
    assert!(schema::from_sample(&sample).is_err());
    sample.property(&name).unwrap().value().unwrap();
    let read = schema::from_sample_as_is(&sample).unwrap();
    assert_eq!(
        read.properties[&name].value,
        Some(Value::number(2.5).unwrap())
    );
    assert_eq!(formula.0.get(), 1);
}

#[test]
fn an_edited_mark_round_trips() {
    let mut sample = Sample::new();
    let mut grist = Property::stored(number(3.2));
    grist.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::edited("7c01e2b3a9f4")),
        computed: None,
    });
    sample.set_property(id("grist"), grist).unwrap();
    let mut computed = IndexMap::new();
    computed.insert(
        InputName::Named(id("grist")),
        InputRecord::Digest(Fingerprint::edited("3f2a1b09c4d1")),
    );
    let mut brix = Property::stored(number(1.52));
    brix.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::new("5b2c9e1f0a34")),
        computed: Some(computed),
    });
    sample.set_property(id("brix"), brix).unwrap();

    let back = round_trip(&sample);
    for name in ["grist", "brix"] {
        assert_eq!(
            back.property(&id(name)).unwrap().records(),
            sample.property(&id(name)).unwrap().records(),
            "{name}"
        );
    }
}

#[test]
fn a_precision_that_is_neither_one_nor_two_specifiers_says_what_it_is() {
    // Where a precision is declared: the project.
    let error = toml::from_str::<std::collections::BTreeMap<String, PrecisionSchema>>(
        "malt = [\".1f\", \".2f\", \".3f\"]\n",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("a precision is one specifier"),
        "{error}"
    );
}

#[test]
fn a_malformed_property_is_named() {
    let parsed = parse("schema_version: 1\nproperties:\n  malt: {v: 1.0, u: -0.5}\n").unwrap();
    let Err(error) = schema::into_sample(parsed) else {
        panic!("a negative uncertainty was accepted");
    };
    let message = error.to_string();
    assert!(
        message.contains("'malt'") && !message.contains("table '_'"),
        "{message}"
    );
}

#[test]
fn a_missing_version_is_read_as_the_current_one() {
    let schema = parse("name: Keg 42\n").unwrap();
    assert_eq!(schema.schema_version, 1);
}

#[test]
fn a_written_value_beside_readings_is_the_value() {
    let sample = schema::into_sample(
        parse(
            "schema_version: 1\nproperties:\n  foam: {v: 2.5, readings: [2.011, 2.017, 2.013]}\n",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(read(&sample, "foam"), number(2.5));
    let back = round_trip(&sample);
    assert_eq!(read(&back, "foam"), number(2.5));
    let readings = back.property(&id("foam")).unwrap().readings().unwrap();
    assert_eq!(readings.as_slice(), [2.011, 2.017, 2.013]);
}

#[test]
fn an_unusable_tag_is_kept_and_the_rest_read() {
    let sample = schema::into_sample(
        parse("schema_version: 1\ntags: [reference, \"my tag\", 2024]\nbatch: 7\n").unwrap(),
    )
    .unwrap();
    assert_eq!(sample.tags(), [id("reference")]);
    assert_eq!(sample.unusable_tags(), ["my tag", "2024"]);
    assert!(sample.attribute(&id("batch")).is_ok());
    let back = round_trip(&sample);
    assert_eq!(back.unusable_tags(), ["my tag", "2024"]);
}

#[test]
fn a_duplicated_table_index_sets_the_table_aside() {
    let source = "schema_version: 1\nproperties:\n  malt: {v: 1.0}\ntables:\n  conditioning:\n    index: day\n    columns: {day: {}, volume_left: {}}\n    rows:\n      - {day: {v: 1}, volume_left: {v: 2.0}}\n      - {day: {v: 1}, volume_left: {v: 2.1}}\n";
    let sample = schema::into_sample(parse(source).unwrap()).unwrap();
    assert!(sample.table(&id("conditioning")).is_err());
    assert_eq!(sample.set_aside_tables().len(), 1);
    assert_eq!(read(&sample, "malt"), number(1.0));
}

#[test]
fn a_columns_statistics_reach_its_cells() {
    // Read without the model, a cell's readings take the statistic its column
    // states, and the cell carries no statement of its own.
    let source = "schema_version: 1\nname: V\ntables:\n  mouthfeel:\n    index: T\n    \
                  columns:\n      T: {}\n      sweetness: {statistics: {v: mean}}\n    rows:\n      \
                  - T: 40\n        sweetness: {readings: [1.0, 2.0, 3.0]}\n";
    let sample = schema::into_sample(parse(source).unwrap()).unwrap();
    let table = sample.table(&id("mouthfeel")).unwrap();
    let cell = table
        .at(
            &samplekit::core::table::RowAddress::Index(vec![Value::integer(40)]),
            &id("sweetness"),
        )
        .unwrap();
    assert_eq!(cell.value().unwrap(), number(2.0));
    let written = schema::from_sample(&sample).unwrap();
    let table = &written.tables[&id("mouthfeel")];
    assert!(table.columns[&id("sweetness")].statistics.is_some());
    assert!(table.rows[0][&id("sweetness")].statistics.is_none());
}
