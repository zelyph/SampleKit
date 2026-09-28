//! The tests of `field_addressing`.

use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{Compute, ComputeError, Property};
use samplekit::core::sample::{AttributeValue, Sample};
use samplekit::core::table::{ColumnMeta, Table};
use samplekit::core::uncertainty::Uncertainty;
use samplekit::core::value::{Readings, Value};
use samplekit::query::field_addressing::{
    self as fields, Channel, Field, FieldError, Resolution, Subject, Vocabulary,
};

/// A slice of references, which is what the collection functions take: a real
/// caller holds samples behind an `Rc` and can only lend them.
fn refs(samples: &[Sample]) -> Vec<&Sample> {
    samples.iter().collect()
}

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn columns(names: &[&str]) -> IndexMap<Identifier, ColumnMeta> {
    names
        .iter()
        .map(|name| (id(name), ColumnMeta::default()))
        .collect()
}

/// A cask with a dosed malt, an attribute, tags and a mashing boil.
fn cask() -> Sample {
    let mut sample = Sample::new();
    sample.set_name(Some("Keg 42".to_string()));
    sample.set_tags(vec![id("reference"), id("cold_crashed")]);

    let mut malt = Property::stored(number(12.5));
    malt.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    malt.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        precision: None,
    });
    sample.set_property(id("malt"), malt).unwrap();

    let foam = Property::measured(
        Readings::new(vec![2.011, 2.017, 2.013, 2.015]).unwrap(),
        None,
    );
    sample.set_property(id("foam"), foam).unwrap();
    sample
        .set_property(id("volume"), Property::stored(Value::absent()))
        .unwrap();
    sample
        .set_attribute(id("cold_crashed"), Value::boolean(true))
        .unwrap();

    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature", "wort"]),
        Vec::new(),
    )
    .unwrap();
    for (t, r) in [(65.0, 12.5), (78.5, 20.0)] {
        let mut cell = Property::stored(number(r));
        cell.set_uncertainty(Some(Uncertainty::new(0.2).unwrap()));
        cell.set_presentation(Presentation {
            unit: Some("lintner".to_string()),
            symbol: None,
            precision: None,
        });
        table
            .add_row(vec![
                (id("temperature"), Property::stored(number(t))),
                (id("wort"), cell),
            ])
            .unwrap();
    }
    sample.set_table(id("mashing"), table).unwrap();
    sample
}

/// A table whose index is two columns, and a textual one with a space.
fn wide() -> Sample {
    let mut sample = Sample::new();
    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature"), id("dextrin")],
        columns(&["temperature", "dextrin", "wort"]),
        Vec::new(),
    )
    .unwrap();
    for (t, f, r) in [(65.0, 2.0e9, 12.5), (65.0, 4.0e9, 13.0)] {
        table
            .add_row(vec![
                (id("temperature"), Property::stored(number(t))),
                (id("dextrin"), Property::stored(number(f))),
                (id("wort"), Property::stored(number(r))),
            ])
            .unwrap();
    }
    sample.set_table(id("mashing"), table).unwrap();

    let mut process = Table::new(
        id("process"),
        vec![id("stage")],
        columns(&["stage", "duration"]),
        Vec::new(),
    )
    .unwrap();
    process
        .add_row(vec![
            (id("stage"), Property::stored(Value::text("stage 2"))),
            (id("duration"), Property::stored(number(45.0))),
        ])
        .unwrap();
    sample.set_table(id("process"), process).unwrap();
    sample
}

fn subject<'a>(sample: &'a Sample, vocabulary: &'a Vocabulary) -> Subject<'a> {
    Subject {
        sample,
        path: None,
        vocabulary,
        states: None,
    }
}

fn resolve(path: &str, sample: &Sample) -> Result<Resolution, FieldError> {
    let vocabulary = Vocabulary::of(sample);
    fields::resolve(&fields::parse(path).unwrap(), &subject(sample, &vocabulary))
}

fn scalar(path: &str, sample: &Sample) -> Option<Value> {
    match resolve(path, sample).unwrap() {
        Resolution::Scalar(value) => value,
        Resolution::List(_) => panic!("{path} resolved to a list"),
        Resolution::Tags(_) => panic!("{path} resolved to tags"),
    }
}

// ------------------------------------------------------------------ parsing

#[test]
fn short_form_equals_value_channel() {
    assert_eq!(
        fields::parse("malt").unwrap(),
        fields::parse("malt.v").unwrap()
    );
}

#[test]
fn an_explicit_value_channel_is_detectable() {
    assert!(!fields::has_explicit_channel("malt"));
    assert!(fields::has_explicit_channel("malt.v"));
    assert!(fields::has_explicit_channel("malt.v"));
    assert!(!fields::has_explicit_channel("mashing.wort[65]"));
    assert!(fields::has_explicit_channel("mashing.wort[65].u"));
}

#[test]
fn uncertainty_channel_resolves() {
    assert_eq!(scalar("malt.u", &cask()), Some(number(0.05)));
}

#[test]
fn unit_channel_resolves_as_text() {
    assert_eq!(scalar("malt.unit", &cask()), Some(Value::text("g")));
}

#[test]
fn cell_path_resolves_by_index() {
    assert_eq!(scalar("mashing.wort[78.5]", &cask()), Some(number(20.0)));
}

#[test]
fn cell_path_with_channel_resolves() {
    assert_eq!(scalar("mashing.wort[65].u", &cask()), Some(number(0.2)));
}

#[test]
fn quoted_textual_index_with_space() {
    assert_eq!(
        scalar("process.duration[\"stage 2\"]", &wide()),
        Some(number(45.0))
    );
}

#[test]
fn a_composite_index_parses_and_resolves() {
    assert_eq!(
        scalar("mashing.wort[65, 4.0e9]", &wide()),
        Some(number(13.0))
    );
}

#[test]
fn the_wrong_component_count_is_its_own_error() {
    // Count the columns is a different fix from that row does not exist.
    let error = resolve("mashing.wort[65]", &wide()).unwrap_err();
    let message = error.to_string();
    assert!(message.contains('2') && message.contains('1'), "{message}");
}

#[test]
fn an_ordinal_path_parses_and_resolves() {
    // `[#1]` is the second row; `[78.5]` is the row indexed 78.5.
    let sample = cask();
    assert_eq!(scalar("mashing.wort[#1]", &sample), Some(number(20.0)));
    assert_eq!(scalar("mashing.wort[65]", &sample), Some(number(12.5)));
}

#[test]
fn a_row_is_counted_from_the_end() {
    // `#-1` the last row, `#-2` the one before.
    let sample = cask();
    let rows = sample
        .table(&samplekit::core::identifier::Identifier::new("mashing").unwrap())
        .unwrap()
        .rows()
        .count();
    assert_eq!(
        scalar("mashing.wort[#-1]", &sample),
        scalar(&format!("mashing.wort[#{}]", rows - 1), &sample)
    );
    assert_eq!(
        scalar("mashing.wort[#-2]", &sample),
        scalar(&format!("mashing.wort[#{}]", rows - 2), &sample)
    );
    // Past the first row, as past the last with `#99`, the sample has no row:
    // absent in it, as a row another sample may hold is (a filter over
    // samples of unequal tables ran on the others), and a mistake only where
    // no sample holds it.
    assert!(matches!(
        resolve("mashing.wort[#-99]", &sample),
        Ok(Resolution::Scalar(None))
    ));
    let past = fields::held_nowhere(&fields::parse("mashing.wort[#-99]").unwrap(), &[&sample])
        .expect("no sample holds it");
    assert!(past.to_string().contains("no row #-99"), "{past}");
    assert!(fields::parse("mashing.wort[#-0]").is_err());
}

#[test]
fn an_ordinal_path_cannot_be_serialized() {
    // A position names a different row as soon as one is inserted, and this is
    // the one place field paths are written.
    let field = fields::parse("mashing.wort[#3]").unwrap();
    assert!(fields::serialize(&field).is_err());
    let by_index = fields::parse("mashing.wort[78.5]").unwrap();
    assert_eq!(fields::serialize(&by_index).unwrap(), "mashing.wort[78.5]");
}

#[test]
fn column_without_index_is_an_error() {
    // Not the first row.
    let error = fields::parse("mashing.wort").unwrap_err();
    assert!(
        matches!(error, FieldError::MissingIndex { .. }),
        "{error:?}"
    );
}

#[test]
fn a_table_name_needs_a_column_and_index() {
    let error = resolve("mashing", &cask()).unwrap_err();
    let FieldError::TableNeedsCell { table, columns } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(table.as_str(), "mashing");
    assert!(columns.iter().any(|column| column == "wort"));
    let message = error.to_string();
    assert!(message.contains("names a table"), "{message}");
    assert!(message.contains("mashing.<column>[<index>]"), "{message}");
}

// -------------------------------------------------- absent against unknown

#[test]
fn absent_property_resolves_to_none() {
    // When other samples have it: absent is data, and a query may ask about it.
    let dosed = cask();
    let mut undosed = Sample::new();
    undosed
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let collection = [dosed, undosed];
    let vocabulary = fields::vocabulary_of(&refs(&collection));

    let field = fields::parse("malt").unwrap();
    let resolved = fields::resolve(&field, &subject(&collection[1], &vocabulary)).unwrap();
    assert_eq!(resolved, Resolution::Scalar(None));
}

#[test]
fn unknown_property_is_an_error() {
    // When no sample has it. An empty result is a legitimate answer to a
    // well-formed question and a dangerous one to a malformed question.
    let error = resolve("nonexistent", &cask()).unwrap_err();
    assert!(
        matches!(error, FieldError::UnknownProperty { .. }),
        "{error:?}"
    );
}

#[test]
fn absent_and_unknown_are_decided_by_the_vocabulary() {
    // The same name, the same sample, two collections, two answers.
    // `Sample` is not `Clone` — an identity, not a value — so it is built twice.
    let bare = || {
        let mut sample = Sample::new();
        sample
            .set_property(id("foam"), Property::stored(number(2.0)))
            .unwrap();
        sample
    };

    let alone = [bare()];
    let vocabulary = fields::vocabulary_of(&refs(&alone));
    let field = fields::parse("malt").unwrap();
    assert!(fields::resolve(&field, &subject(&alone[0], &vocabulary)).is_err());

    let among = [bare(), cask()];
    let vocabulary = fields::vocabulary_of(&refs(&among));
    let resolved = fields::resolve(&field, &subject(&among[0], &vocabulary)).unwrap();
    assert_eq!(resolved, Resolution::Scalar(None));
}

#[test]
fn unknown_property_suggests_nearest() {
    let mut sample = cask();
    sample
        .set_property(id("plato"), Property::stored(number(2.8)))
        .unwrap();
    let error = resolve("plto", &sample).unwrap_err();
    let FieldError::UnknownProperty { suggestion, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("plato"));
}

#[test]
fn unknown_column_lists_available_columns() {
    // The `Q` case, verbatim.
    let mut sample = Sample::new();
    let mut table = Table::new(
        id("measurements"),
        vec![id("temperature")],
        columns(&["temperature", "ph", "srm", "wort"]),
        Vec::new(),
    )
    .unwrap();
    table
        .add_row(vec![(id("temperature"), Property::stored(number(20.0)))])
        .unwrap();
    sample.set_table(id("measurements"), table).unwrap();

    let error = resolve("measurements.carbonation_level[20]", &sample).unwrap_err();
    let FieldError::UnknownColumn {
        available,
        suggestion,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(available, &["temperature", "ph", "srm", "wort"]);
    assert_eq!(suggestion.as_deref(), None, "nothing is near enough");
    let message = error.to_string();
    assert!(
        message.contains("unknown column 'carbonation_level'"),
        "{message}"
    );
    assert!(message.contains("in table 'measurements'"), "{message}");
}

#[test]
fn unknown_channel_lists_the_channels() {
    // Caught at parse: a channel is grammar, and needs no sample.
    let error = fields::parse("malt.unt").unwrap_err();
    let FieldError::UnknownChannel { available, .. } = &error else {
        panic!("{error:?}");
    };
    assert!(available.iter().any(|c| c == "u"), "{available:?}");
    assert!(
        available.iter().any(|c| c.starts_with("stats")),
        "{available:?}"
    );
    assert!(error.to_string().contains("did you mean: unit?"), "{error}");
}

/// A spelling that **was** a channel is told what it is called now.
///
/// Read as a table's column instead, `malt.value` answered *a column has one
/// cell per row, address one* — nonsense to someone typing what the
/// documentation said last week. And `uncertainty` is too far from `u` for a
/// nearest-name suggestion to reach.
#[test]
fn a_retired_spelling_is_told_what_it_is_called_now() {
    for (was, now) in [("value", "v"), ("uncertainty", "u")] {
        let error = fields::parse(&format!("malt.{was}")).unwrap_err();
        assert!(
            matches!(error, FieldError::UnknownChannel { .. }),
            "{was}: {error:?}"
        );
        let said = error.to_string();
        assert!(
            said.contains(&format!("'{was}' was renamed '{now}'")),
            "{said}"
        );
    }
}

// ------------------------------------------------------ statistics and tags

#[test]
fn a_statistic_resolves_under_stats() {
    let mean = scalar("foam.stats.mean", &cask()).unwrap();
    let Value::Number(mean) = mean else {
        panic!("{mean:?}")
    };
    assert!((mean - 2.014).abs() < 1e-9, "{mean}");
}

#[test]
fn stats_exposes_every_summary_field() {
    // All ten of them, under their own names, unabbreviated.
    let sample = cask();
    for name in [
        "count",
        "minimum",
        "maximum",
        "mean",
        "median",
        "first_quartile",
        "third_quartile",
        "sample_stdev",
        "population_stdev",
        "standard_error",
    ] {
        let path = format!("foam.stats.{name}");
        assert!(
            scalar(&path, &sample).is_some(),
            "{path} resolved to nothing"
        );
    }
}

#[test]
fn stats_alone_is_an_error_listing_its_fields() {
    // It names a group, not a value a filter could compare.
    let error = fields::parse("foam.stats").unwrap_err();
    let FieldError::UnknownChannel { available, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(available.len(), 10, "{available:?}");
    assert!(
        available.iter().any(|name| name == "median"),
        "{available:?}"
    );
}

#[test]
fn an_unknown_statistic_is_caught() {
    // `stdev` does not say whether it divides by n or n - 1.
    let error = fields::parse("foam.stats.stdev").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("sample_stdev"), "{message}");
    assert!(message.contains("population_stdev"), "{message}");
}

#[test]
fn stats_is_absent_without_observations() {
    // A single stored value has nothing to summarize: absent, distinct from a
    // misspelled channel.
    assert_eq!(scalar("malt.stats.median", &cask()), None);
}

#[test]
fn a_failing_formula_is_an_error_not_an_absence() {
    // Discarding it would make a broken model look like a collection of
    // unmeasured casks.
    struct Raising;
    impl Compute for Raising {
        fn compute(&self) -> Result<Value, ComputeError> {
            Err(ComputeError::failed(Boom))
        }
    }
    #[derive(Debug)]
    struct Boom;
    impl std::fmt::Display for Boom {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the formula raised")
        }
    }
    impl std::error::Error for Boom {}

    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Raising)))
        .unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let field = fields::parse("plato").unwrap();
    let error = fields::resolve(&field, &subject(&sample, &vocabulary)).unwrap_err();
    assert!(matches!(error, FieldError::Compute { .. }), "{error:?}");
    assert!(error.to_string().contains("plato"), "{error}");
}

#[test]
fn tags_resolve_to_their_own_shape() {
    // Not a joined string, and not a refusal.
    let resolved = resolve("tags", &cask()).unwrap();
    assert_eq!(
        resolved,
        Resolution::Tags(vec![id("reference"), id("cold_crashed")])
    );
}

#[test]
fn an_attribute_takes_no_channel() {
    // Refused at resolution, where the kind is known, saying what `cold_crashed` is.
    let sample = cask();
    let field = fields::parse("cold_crashed.u").unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let error = fields::resolve(&field, &subject(&sample, &vocabulary)).unwrap_err();
    assert!(error.to_string().contains("attribute"), "{error}");
    // And its value resolves without one.
    assert_eq!(scalar("cold_crashed", &sample), Some(Value::boolean(true)));
}

/// A channel has one spelling, and the long one is not read.
///
/// `malt.value` and `malt.v` used to be the same address. Two ways of writing
/// one thing is what the file format and the grammar both stopped doing.
#[test]
fn a_channel_has_one_spelling() {
    let sample = cask();
    assert_eq!(scalar("malt.v", &sample), Some(number(12.5)));
    assert_eq!(scalar("malt.u", &sample), Some(number(0.05)));
    for refused in ["malt.value", "malt.uncertainty"] {
        let error = fields::parse(refused).unwrap_err();
        let FieldError::UnknownChannel { available, .. } = &error else {
            panic!("{refused}: {error:?}");
        };
        // And what it offers instead is what it accepts.
        assert!(available.iter().any(|c| c == "v"), "{available:?}");
        assert!(available.iter().any(|c| c == "u"), "{available:?}");
        assert!(!available.iter().any(|c| c == "value"), "{available:?}");
    }
}

#[test]
fn unit_has_no_single_letter_alias() {
    // `u` is the first letter of both, and it goes to uncertainty, following
    // the GUM's u(x).
    let sample = cask();
    assert_eq!(scalar("malt.u", &sample), Some(number(0.05)));
    assert_ne!(scalar("malt.u", &sample), scalar("malt.unit", &sample));
}

#[test]
fn reserved_fields_resolve() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let path = PathBuf::from("/data/samples/keg-42.md");
    let located = Subject {
        sample: &sample,
        path: Some(&path),
        vocabulary: &vocabulary,
        states: None,
    };
    let read = |text: &str| match fields::resolve(&fields::parse(text).unwrap(), &located).unwrap()
    {
        Resolution::Scalar(value) => value,
        other => panic!("{other:?}"),
    };
    assert_eq!(read("name"), Some(Value::text("Keg 42")));
    assert_eq!(read("path"), Some(Value::text("/data/samples/keg-42.md")));
    assert_eq!(read("filename"), Some(Value::text("keg-42.md")));
}

#[test]
fn path_and_filename_come_from_the_subject() {
    // A sample never written has no path: absent, not an invented empty string.
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let unwritten = subject(&sample, &vocabulary);
    let read = |text: &str| fields::resolve(&fields::parse(text).unwrap(), &unwritten).unwrap();
    assert_eq!(read("path"), Resolution::Scalar(None));
    assert_eq!(read("filename"), Resolution::Scalar(None));
}

// -------------------------------------------------------------- enumeration

#[test]
fn available_enumerates_every_cell() {
    // Eleven indexes by four columns yields forty-four fields.
    let mut sample = Sample::new();
    let mut table = Table::new(
        id("boil"),
        vec![id("temperature")],
        columns(&["temperature", "wort", "clarity", "ph"]),
        Vec::new(),
    )
    .unwrap();
    for step in 0..11 {
        table
            .add_row(vec![(
                id("temperature"),
                Property::stored(number(f64::from(step))),
            )])
            .unwrap();
    }
    sample.set_table(id("boil"), table).unwrap();

    let cells = fields::available(&sample)
        .into_iter()
        .filter(|field| matches!(field, Field::Cell { .. }))
        .count();
    assert_eq!(cells, 44);
}

#[test]
fn enumeration_does_not_expand_channels() {
    // Thirty properties yield thirty entries, not a hundred and fifty.
    let mut sample = Sample::new();
    for n in 0..30 {
        sample
            .set_property(id(&format!("p{n}")), Property::stored(number(1.0)))
            .unwrap();
    }
    let named = fields::available(&sample)
        .into_iter()
        .filter(|field| matches!(field, Field::Named { .. }))
        .count();
    // Thirty properties and `tags`, and not one `.value` among them.
    assert_eq!(named, 31);
}

#[test]
fn the_vocabulary_knows_the_tags_in_use() {
    // Tags are values, not fields, and the vocabulary is where "nobody uses
    // this" is answered — the same question as "nobody has this name".
    let mut other = Sample::new();
    other.set_tags(vec![id("cold_crashed"), id("broken")]);
    let collection = [cask(), other];
    let vocabulary = fields::vocabulary_of(&refs(&collection));
    assert_eq!(
        vocabulary.tags(),
        [id("reference"), id("cold_crashed"), id("broken")]
    );
    assert!(vocabulary.has_tag(&id("broken")));
    assert!(!vocabulary.has_tag(&id("referenc")));
    assert_eq!(
        vocabulary.nearest_tag("referenc").as_deref(),
        Some("reference")
    );
}

#[test]
fn tags_is_enumerated_once_without_channels() {
    // It is an attribute, not a quantity.
    let listed: Vec<Field> = fields::available(&cask())
        .into_iter()
        .filter(|field| matches!(field, Field::Named { name, .. } if name.as_str() == "tags"))
        .collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0],
        Field::Named {
            name: id("tags"),
            channel: Channel::Value
        }
    );
}

#[test]
fn with_channel_keeps_the_quantity() {
    // An export reads one column on two channels; re-parsing a string would
    // mean building a path in order to take it apart again.
    let malt = fields::parse("malt").unwrap();
    assert_eq!(
        fields::with_channel(&malt, Channel::Uncertainty),
        fields::parse("malt.u").unwrap()
    );
    let cell = fields::parse("mashing.wort[65]").unwrap();
    assert_eq!(
        fields::with_channel(&cell, Channel::Unit),
        fields::parse("mashing.wort[65].unit").unwrap()
    );
    // A reserved field has no channel to set.
    let name = fields::parse("name").unwrap();
    assert_eq!(fields::with_channel(&name, Channel::Uncertainty), name);
}

#[test]
fn a_parsed_field_equals_an_enumerated_one() {
    // One has seen a sample and the other has not; the same name is the same
    // field, or the two property tests below compare nothing.
    let sample = cask();
    let listed = fields::available(&sample);
    for path in ["cold_crashed", "malt", "tags", "name"] {
        let parsed = fields::parse(path).unwrap();
        assert!(
            listed.contains(&parsed),
            "{path} is not what available says"
        );
    }
}

#[test]
fn enumeration_order_is_deterministic() {
    let sample = cask();
    let first: Vec<String> = fields::available(&sample)
        .iter()
        .map(fields::describe)
        .collect();
    let again: Vec<String> = fields::available(&sample)
        .iter()
        .map(fields::describe)
        .collect();
    assert_eq!(first, again);
    // Declaration order: properties before attributes before cells.
    let malt = first.iter().position(|f| f == "malt").unwrap();
    let cold_crashed = first.iter().position(|f| f == "cold_crashed").unwrap();
    let cell = first.iter().position(|f| f.contains('[')).unwrap();
    assert!(malt < cold_crashed && cold_crashed < cell, "{first:?}");
}

#[test]
fn every_available_field_resolves() {
    // One direction of the pair: nothing is advertised and broken.
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    // `state` resolves where its reader read the states, as every caller that
    // offers it does.
    let states = samplekit::query::field_addressing::States::new(Vec::new(), true);
    let here = Subject {
        states: Some(&states),
        ..subject(&sample, &vocabulary)
    };
    for field in fields::available(&sample) {
        let described = fields::describe(&field);
        assert!(
            fields::resolve(&field, &here).is_ok(),
            "{described} is enumerated and does not resolve"
        );
    }
}

#[test]
fn every_resolvable_field_is_available() {
    // The other direction: nothing is reachable and undiscoverable. Every
    // enumerated path is re-parsed and must come back as the same field.
    let sample = cask();
    let listed = fields::available(&sample);
    for field in &listed {
        let described = fields::describe(field);
        let parsed = fields::parse(&described)
            .unwrap_or_else(|error| panic!("{described} does not parse back: {error}"));
        assert!(
            listed.contains(&parsed),
            "{described} parses to something not enumerated"
        );
    }
}

// -------------------------------------------------------------- completion

#[test]
fn completion_offers_channels_after_the_separator() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    let channels = fields::complete("malt.", &here);
    assert!(channels.contains(&"malt.v".to_string()), "{channels:?}");
    assert!(channels.contains(&"malt.u".to_string()), "{channels:?}");
    assert!(
        channels.contains(&"malt.symbol".to_string()),
        "{channels:?}"
    );
}

#[test]
fn an_exact_quantity_completion_opens_its_channels() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    let channels = fields::complete("malt", &here);
    assert!(channels.contains(&"malt.v".to_string()), "{channels:?}");
    assert!(channels.contains(&"malt.u".to_string()), "{channels:?}");
    assert!(
        !channels.contains(&"malt.stats.".to_string()),
        "{channels:?}"
    );
}

#[test]
fn completion_walks_a_table_path_segment_by_segment() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    assert_eq!(
        fields::complete("mash", &here),
        vec!["mashing.".to_string()]
    );
    let columns = fields::complete("mashing.", &here);
    assert!(
        columns.contains(&"mashing.wort[".to_string()),
        "{columns:?}"
    );
    let indexes = fields::complete("mashing.wort[", &here);
    assert!(
        indexes.contains(&"mashing.wort[65]".to_string()),
        "{indexes:?}"
    );
    let channels = fields::complete("mashing.wort[65].", &here);
    assert!(
        channels.contains(&"mashing.wort[65].u".to_string()),
        "{channels:?}"
    );
}

#[test]
fn a_strong_single_match_is_offered_directly() {
    // Without waiting for the separator: the point is to shorten the path to a
    // name, not to enforce a ceremony.
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);
    assert_eq!(fields::complete("fo", &here), vec!["foam".to_string()]);
}

#[test]
fn an_exact_completion_match_wins_over_longer_prefixes() {
    let mut sample = Sample::new();
    for name in ["measurements", "measurements_cold"] {
        let mut table =
            Table::new(id(name), vec![id("T")], columns(&["T", "ph"]), Vec::new()).unwrap();
        table
            .add_row(vec![
                (id("T"), Property::stored(number(20.0))),
                (id("ph"), Property::stored(number(1.0))),
            ])
            .unwrap();
        sample.set_table(id(name), table).unwrap();
    }
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);
    assert_eq!(
        fields::complete("measurements", &here),
        vec!["measurements.".to_string()]
    );
}

#[test]
fn completion_expands_stats_one_level_at_a_time() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    assert_eq!(
        fields::complete("foam.stats", &here),
        vec!["foam.stats.".to_string()]
    );
    let statistics = fields::complete("foam.stats.", &here);
    assert_eq!(statistics.len(), 10, "{statistics:?}");
    assert!(statistics.contains(&"foam.stats.mean".to_string()));
    assert!(!statistics.iter().any(|name| name.contains('<')));
}

#[test]
fn completion_offers_stats_only_for_readings() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    let stored = fields::complete("malt", &here);
    assert!(!stored.contains(&"malt.stats.".to_string()), "{stored:?}");
    assert!(fields::complete("malt.stats", &here).is_empty());

    let measured = fields::complete("foam", &here);
    assert!(
        measured.contains(&"foam.stats.".to_string()),
        "{measured:?}"
    );
}

#[test]
fn completion_finds_table_cells_by_unqualified_column() {
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);

    let folded = fields::complete("", &here);
    assert!(!folded.iter().any(|name| name.contains('[')), "{folded:?}");

    let cells = fields::complete("wo", &here);
    assert_eq!(
        cells,
        vec![
            "mashing.wort[65]".to_string(),
            "mashing.wort[78.5]".to_string(),
        ]
    );
}

#[test]
fn the_vocabulary_holds_every_available_name() {
    // `available` lists a field, so the vocabulary knows its name: otherwise a
    // column a caller *can* ask for is reported as a name nobody has, which
    // produced "unknown field 'tags'; did you mean 'tags'?".
    let sample = cask();
    let vocabulary = Vocabulary::of(&sample);
    for field in fields::available(&sample) {
        if let Field::Named { name, .. } = &field {
            assert!(
                vocabulary.has_name(name),
                "{} is available and not in the vocabulary",
                fields::describe(&field)
            );
        }
    }
    assert!(vocabulary.has_name(&id("tags")));
}

#[test]
fn a_list_attribute_and_its_items_have_distinct_shapes() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    assert_eq!(
        resolve("temperatures", &sample).unwrap(),
        Resolution::List(vec![Value::integer(20), Value::integer(30)])
    );
    assert_eq!(
        scalar("temperatures[#1]", &sample),
        Some(Value::integer(30))
    );
    assert!(fields::available(&sample).contains(&fields::parse("temperatures[#0]").unwrap()));
}

#[test]
fn completion_offers_list_positions_after_the_bracket() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let here = subject(&sample, &vocabulary);
    assert_eq!(
        fields::complete("temperatures[", &here),
        ["temperatures[#0]", "temperatures[#1]"]
    );
}

#[test]
fn an_index_a_sample_lacks_is_absent() {
    assert_eq!(
        resolve("mashing.wort[300]", &cask()).unwrap(),
        Resolution::Scalar(None)
    );
}

#[test]
fn an_index_no_sample_holds_names_the_nearest() {
    let sample = cask();
    let missed = fields::parse("mashing.wort[80]").unwrap();
    let error = fields::held_nowhere(&missed, &[&sample]).unwrap();
    let FieldError::UnknownIndex { per_column, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(per_column[0].nearest, Some(number(78.5)));
    assert!(error.to_string().contains("78.5"), "{error}");
    let held = fields::parse("mashing.wort[78.5]").unwrap();
    assert!(fields::held_nowhere(&held, &[&sample]).is_none());
}

#[test]
fn a_list_item_a_sample_lacks_is_absent() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20)]).unwrap(),
        )
        .unwrap();
    assert_eq!(
        resolve("temperatures[#1]", &sample).unwrap(),
        Resolution::Scalar(None)
    );
}

#[test]
fn a_list_position_no_sample_holds_is_named() {
    let list = |items: Vec<Value>| {
        let mut sample = Sample::new();
        sample
            .set_attribute(id("temperatures"), AttributeValue::list(items).unwrap())
            .unwrap();
        sample
    };
    let short = list(vec![Value::integer(20)]);
    let long = list(vec![Value::integer(20), Value::integer(30)]);
    let second = fields::parse("temperatures[#1]").unwrap();
    assert!(fields::held_nowhere(&second, &[&short, &long]).is_none());
    let third = fields::parse("temperatures[#2]").unwrap();
    let error = fields::held_nowhere(&third, &[&short, &long]).unwrap();
    assert!(
        matches!(
            error,
            FieldError::ListIndexOutOfRange {
                position: 2,
                length: 2,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("numbered from #0"), "{error}");
}

#[test]
fn readings_resolve_as_the_list_they_are() {
    // The evidence itself, where `stats` answers its summary. Answering the
    // mean here would be the tool replying to a question nobody put to it.
    let sample = cask();
    let Resolution::List(values) = resolve("foam.readings", &sample).unwrap() else {
        panic!("readings are a list");
    };
    assert!(values.len() > 1, "{values:?}");
    let mean = scalar("foam.stats.mean", &sample).unwrap();
    assert!(
        !values
            .iter()
            .all(|value| samplekit::core::value::equals(value, &mean)),
        "the readings answered their own summary: {values:?}"
    );
}

#[test]
fn readings_are_empty_on_a_stored_value() {
    // Absence, as `stats` answers there — not an error, and above all not the
    // "column missing its index" this used to be diagnosed as.
    let Resolution::List(values) = resolve("malt.readings", &cask()).unwrap() else {
        panic!("readings are a list even when there are none");
    };
    assert!(values.is_empty(), "{values:?}");
}

#[test]
fn a_cell_unit_is_its_column_unit_unless_it_overrides() {
    // A cell holding no unit of its own answers with its column's, as the
    // table's header does: `measurements.f[20].unit` was empty.
    use samplekit::core::table::ColumnMeta;
    let mut sample = Sample::new();
    let mut declared = columns(&["temperature", "f"]);
    declared[&id("f")] = ColumnMeta {
        presentation: Presentation {
            unit: Some("brix".to_string()),
            symbol: None,
            precision: None,
        },
        statistics: Default::default(),
    };
    let mut table = Table::new(id("m"), vec![id("temperature")], declared, Vec::new()).unwrap();
    let mut own = Property::stored(number(9.0));
    own.set_presentation(Presentation {
        unit: Some("ppm".to_string()),
        symbol: None,
        precision: None,
    });
    table
        .add_row(vec![
            (id("temperature"), Property::stored(number(20.0))),
            (id("f"), Property::stored(number(8.8))),
        ])
        .unwrap();
    table
        .add_row(vec![
            (id("temperature"), Property::stored(number(30.0))),
            (id("f"), own),
        ])
        .unwrap();
    sample.set_table(id("m"), table).unwrap();
    assert_eq!(scalar("m.f[20].unit", &sample), Some(Value::text("brix")));
    assert_eq!(scalar("m.f[30].unit", &sample), Some(Value::text("ppm")));
}

#[test]
fn a_name_and_a_word_are_no_channel_or_a_column_without_its_row() {
    // `abv.x` was told it named a column, beside a quantity `abv`.
    let error = fields::parse("abv.x").unwrap_err().to_string();
    assert!(
        error.contains("if abv is a quantity, x is no channel of it")
            && error.contains("abv.x[<index>]"),
        "{error}"
    );
}

#[test]
fn state_resolves_to_the_words_a_sample_holds() {
    use samplekit::query::field_addressing::{
        ReservedField, Resolution, State, States, Vocabulary,
    };
    let parsed = fields::parse("state").unwrap();
    assert_eq!(parsed, Field::Reserved(ReservedField::State));
    let sample = Sample::new();
    assert!(fields::available(&sample).contains(&parsed));
    let vocabulary = Vocabulary::of(&sample);
    let resolved = |states: &States| {
        fields::resolve(
            &parsed,
            &Subject {
                sample: &sample,
                path: None,
                vocabulary: &vocabulary,
                states: Some(states),
            },
        )
        .unwrap()
    };
    // The gravest first, `current` added where the model was read and no
    // value is otherwise.
    assert_eq!(
        resolved(&States::new(vec![State::Defective, State::Failed], true)),
        Resolution::List(vec![Value::text("failed"), Value::text("defective")])
    );
    assert_eq!(
        resolved(&States::new(vec![State::Defective], true)),
        Resolution::List(vec![Value::text("defective"), Value::text("current")])
    );
    assert_eq!(
        resolved(&States::new(Vec::new(), false)),
        Resolution::Scalar(None)
    );
}

#[test]
fn state_without_its_states_is_an_error() {
    let sample = Sample::new();
    let vocabulary = samplekit::query::field_addressing::Vocabulary::of(&sample);
    let error = fields::resolve(
        &fields::parse("state").unwrap(),
        &Subject {
            sample: &sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        },
    )
    .unwrap_err();
    assert_eq!(error, fields::FieldError::StatesNotRead);
}

#[test]
fn a_state_only_the_model_could_deny_is_unknown_without_it() {
    use samplekit::query::field_addressing::{State, StateWord, States};
    let word = |written: &str| StateWord::parse(written).unwrap();
    let files_only = States::new(vec![State::Stale], false);
    assert_eq!(files_only.answers(word("outdated")), Some(true));
    assert_eq!(files_only.answers(word("not_current")), Some(true));
    assert_eq!(files_only.answers(word("failed")), Some(false));
    assert_eq!(files_only.answers(word("current")), Some(false));
    let nothing = States::new(Vec::new(), false);
    for only_the_model in ["never_computed", "waiting", "current", "not_current"] {
        assert_eq!(
            nothing.answers(word(only_the_model)),
            None,
            "{only_the_model}"
        );
    }
    let read = States::new(Vec::new(), true);
    for written in StateWord::WORDS {
        assert_eq!(
            read.answers(word(written)),
            Some(written == "current"),
            "{written}"
        );
    }
}
