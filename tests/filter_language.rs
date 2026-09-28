//! The tests of `filter_language`.

use indexmap::IndexMap;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::Property;
use samplekit::core::sample::{AttributeValue, Sample};
use samplekit::core::table::{ColumnMeta, Table};
use samplekit::core::value::{Date, DateTime, Value};
use samplekit::query::field_addressing::{self as fields, Subject};
use samplekit::query::filter_language::{self as filter, FilterError, Truth};

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

/// Three casks: one approved and heavy, one reviewed and light, one
/// unweighed. Enough for absence, for text and for a table cell.
fn collection() -> Vec<Sample> {
    let mut samples = Vec::new();

    let mut a = Sample::new();
    a.set_name(Some("A".to_string()));
    a.set_tags(vec![id("reference")]);
    a.set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    a.set_attribute(id("status"), Value::text("approved"))
        .unwrap();
    a.set_attribute(id("beer"), Value::text("Dunkel 57"))
        .unwrap();
    a.set_attribute(id("cold_crashed"), Value::boolean(true))
        .unwrap();
    a.set_attribute(id("batch"), Value::integer(7)).unwrap();
    a.set_attribute(
        id("brewed_on"),
        Value::date(Date::parse("2026-03-14").unwrap()),
    )
    .unwrap();
    let mut mashing = Table::new(
        id("mashing"),
        vec![id("temperature")],
        ["temperature", "wort"]
            .iter()
            .map(|n| (id(n), ColumnMeta::default()))
            .collect::<IndexMap<_, _>>(),
        Vec::new(),
    )
    .unwrap();
    mashing
        .add_row(vec![
            (id("temperature"), Property::stored(number(78.5))),
            (id("wort"), Property::stored(number(104.2))),
        ])
        .unwrap();
    a.set_table(id("mashing"), mashing).unwrap();
    samples.push(a);

    let mut b = Sample::new();
    b.set_name(Some("B".to_string()));
    b.set_tags(vec![id("reference"), id("broken")]);
    b.set_property(id("malt"), Property::stored(number(2.0)))
        .unwrap();
    b.set_attribute(id("status"), Value::text("reviewed"))
        .unwrap();
    b.set_attribute(id("beer"), Value::text("altbier")).unwrap();
    b.set_attribute(id("cold_crashed"), Value::boolean(false))
        .unwrap();
    b.set_attribute(id("batch"), number(7.0)).unwrap();
    b.set_attribute(
        id("brewed_on"),
        Value::date(Date::parse("2025-01-01").unwrap()),
    )
    .unwrap();
    samples.push(b);

    // Unweighed: `malt` is absent here and present in the others.
    let mut c = Sample::new();
    c.set_name(Some("C".to_string()));
    c.set_attribute(id("status"), Value::text("approved"))
        .unwrap();
    c.set_attribute(id("beer"), Value::text("Dunkel 57"))
        .unwrap();
    samples.push(c);

    samples
}

/// The names each sample selects, in collection order.
fn selects(source: &str) -> Vec<String> {
    let samples = collection();
    let parsed = filter::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    samples
        .iter()
        .filter(|sample| {
            let subject = Subject {
                sample,
                path: None,
                vocabulary: &vocabulary,
                states: None,
            };
            filter::evaluate(&parsed, &subject)
                .unwrap_or_else(|e| panic!("{source}: {e}"))
                .selects()
        })
        .map(|sample| sample.name().unwrap().to_string())
        .collect()
}

fn truth(source: &str, which: usize) -> Truth {
    let samples = collection();
    let parsed = filter::parse(source).unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subject = Subject {
        sample: &samples[which],
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    filter::evaluate(&parsed, &subject).unwrap()
}

// ------------------------------------------------------------- comparisons

#[test]
fn equality_on_text() {
    assert_eq!(selects("beer == \"Dunkel 57\""), ["A", "C"]);
}

#[test]
fn bare_word_operand_is_text() {
    assert_eq!(selects("status == approved"), ["A", "C"]);
}

#[test]
fn quoted_operand_with_space() {
    // `"Dunkel 57"` survives tokenization, which whitespace splitting broke.
    assert_eq!(selects("beer == \"Dunkel 57\""), ["A", "C"]);
}

#[test]
fn numeric_comparison() {
    assert_eq!(selects("malt > 3"), ["A"]);
    assert_eq!(selects("malt >= 2"), ["A", "B"]);
    assert_eq!(selects("malt < 3"), ["B"]);
    assert_eq!(selects("malt <= 2"), ["B"]);
}

#[test]
fn contains_is_case_insensitive() {
    assert_eq!(selects("beer contains \"dunk\""), ["A", "C"]);
}

#[test]
fn equality_is_case_sensitive() {
    assert!(selects("beer == \"dc 57\"").is_empty());
}

#[test]
fn in_operator() {
    assert_eq!(selects("status in (approved, reviewed)"), ["A", "B", "C"]);
    assert_eq!(selects("status in (reviewed)"), ["B"]);
}

#[test]
fn and_binds_tighter_than_or() {
    // `a and b or c` groups as `(a and b) or c`.
    assert_eq!(
        selects("malt > 100 && status == approved || status == reviewed"),
        ["B"]
    );
}

#[test]
fn parentheses_override_precedence() {
    assert_eq!(
        selects("malt > 1 && (status == reviewed || status == approved)"),
        ["A", "B"]
    );
}

#[test]
fn not_negates_one_predicate() {
    // And binds tighter than `and`.
    assert_eq!(selects("!status == approved && malt > 1"), ["B"]);
}

#[test]
fn table_cell_field_in_a_filter() {
    assert_eq!(selects("mashing.wort[78.5] > 100"), ["A"]);
}

#[test]
fn a_quoted_index_stays_in_its_field() {
    // The index of a cell may hold a space, and is then quoted: the filter cut
    // the field at its first quote, where a column read it whole.
    let mut sample = Sample::new();
    sample.set_name(Some("P".to_string()));
    let mut process = Table::new(
        id("process"),
        vec![id("stage")],
        ["stage", "duration"]
            .iter()
            .map(|n| (id(n), ColumnMeta::default()))
            .collect::<IndexMap<_, _>>(),
        Vec::new(),
    )
    .unwrap();
    process
        .add_row(vec![
            (id("stage"), Property::stored(Value::text("stage 2"))),
            (id("duration"), Property::stored(number(30.0))),
        ])
        .unwrap();
    sample.set_table(id("process"), process).unwrap();
    let samples = vec![sample];
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subject = Subject {
        sample: &samples[0],
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    for source in [
        "process.duration[\"stage 2\"] > 20",
        "process.duration[\"stage 2\"] > 20 && process.duration[\"stage 2\"] < 40",
    ] {
        let parsed = filter::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert!(
            filter::evaluate(&parsed, &subject).unwrap().selects(),
            "{source}"
        );
    }
}

#[test]
fn boolean_literal_is_not_a_bare_word() {
    // Without the literal form, this compares Text("true") to Boolean(true)
    // and answers "no sample is cold_crashed" on a collection where one is.
    assert_eq!(selects("cold_crashed == true"), ["A"]);
}

#[test]
fn date_literal_parses_unquoted() {
    assert_eq!(selects("brewed_on > 2026-01-01"), ["A"]);
}

#[test]
fn quoting_forces_text() {
    // An attribute genuinely holding the word `true` needs this.
    assert!(selects("cold_crashed == \"true\"").is_empty());
}

#[test]
fn integer_literal_matches_either_numeric_form() {
    // `batch == 7` finds a stored 7 and a stored 7.0.
    assert_eq!(selects("batch == 7"), ["A", "B"]);
}

#[test]
fn attribute_and_property_filter_alike() {
    assert_eq!(selects("cold_crashed == true && malt > 12"), ["A"]);
}

// ------------------------------------------------------ three-valued logic

#[test]
fn absent_value_yields_unknown_not_false() {
    // `Truth::Unknown`, distinct from `False`, and the sample is excluded.
    assert_eq!(truth("malt > 3", 2), Truth::Unknown);
    assert_eq!(truth("malt > 3", 1), Truth::False);
    assert_eq!(selects("malt > 3"), ["A"]);
}

#[test]
fn negated_unknown_stays_excluded() {
    // A sample whose malt is unknown is not known to be light. Deciding
    // otherwise means one cask satisfies a predicate and its negation.
    assert_eq!(truth("!(malt > 3)", 2), Truth::Unknown);
    assert_eq!(selects("!(malt > 3)"), ["B"]);
}

#[test]
fn unknown_in_or_can_still_select() {
    // The other branch decides.
    assert_eq!(truth("malt > 3 || status == approved", 2), Truth::True);
    assert_eq!(selects("malt > 3 || status == approved"), ["A", "C"]);
}

#[test]
fn is_missing_selects_absent_values() {
    // The only predicate true on absence, which is what makes absence askable.
    assert_eq!(selects("malt is missing"), ["C"]);
}

#[test]
fn is_present_is_its_complement() {
    // Together they partition the collection.
    let present = selects("malt is present");
    let missing = selects("malt is missing");
    assert_eq!(present, ["A", "B"]);
    assert_eq!(missing, ["C"]);
    assert_eq!(present.len() + missing.len(), collection().len());
    // And `is not missing` is `is present`.
    assert_eq!(selects("malt is not missing"), present);
}

#[test]
fn unknown_is_countable_by_the_caller() {
    // Excluded because false and excluded because unanswerable are two
    // different facts, and a summary saying 58 / 70 needs the second.
    let samples = collection();
    let parsed = filter::parse("malt > 3").unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let mut counts = (0, 0, 0);
    for sample in &samples {
        let subject = Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        };
        match filter::evaluate(&parsed, &subject).unwrap() {
            Truth::True => counts.0 += 1,
            Truth::False => counts.1 += 1,
            Truth::Unknown => counts.2 += 1,
        }
    }
    assert_eq!(counts, (1, 1, 1));
}

// ------------------------------------------------------------------- tags

#[test]
fn has_matches_a_tag_exactly() {
    // `tags has ref` does not match `reference`.
    assert_eq!(selects("tags has reference"), ["A", "B"]);
    assert!(selects("tags has ref").is_empty());
}

#[test]
fn contains_on_tags_is_refused() {
    // And the message names `has`.
    let samples = collection();
    let parsed = filter::parse("tags contains ref").unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subject = Subject {
        sample: &samples[0],
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let error = filter::evaluate(&parsed, &subject).unwrap_err();
    assert!(
        matches!(error, FilterError::WrongOperator { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("has"), "{error}");
}

#[test]
fn has_on_a_non_tag_field_is_refused() {
    // The operator is not general, and the error is `WrongOperator` rather
    // than a kind mismatch.
    let samples = collection();
    let parsed = filter::parse("beer has reference").unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subject = Subject {
        sample: &samples[0],
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let error = filter::evaluate(&parsed, &subject).unwrap_err();
    assert!(
        matches!(error, FilterError::WrongOperator { .. }),
        "{error:?}"
    );
    // It names the operator text takes, as `contains` on a list names `has`.
    assert!(error.to_string().contains("contains"), "{error}");
    // A number has no such operator: `contains` is not offered for it.
    let parsed = filter::parse("malt has 1").unwrap();
    let error = filter::evaluate(&parsed, &subject).unwrap_err();
    assert!(!error.to_string().contains("contains"), "{error}");
}

#[test]
fn an_unknown_tag_suggests_the_nearest() {
    // Tags are enumerable, so a typo is catchable.
    let samples = collection();
    let parsed = filter::parse("tags has referenc").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert!(
        diagnostics
            .unknown_tags
            .iter()
            .any(|t| t.suggestion.as_deref() == Some("reference")),
        "{diagnostics:?}"
    );
}

// ------------------------------------------------------------ diagnostics

#[test]
fn unknown_field_is_an_error_not_an_empty_result() {
    // The central case. An empty result is a legitimate answer to a
    // well-formed question and a dangerous one to a malformed question.
    let samples = collection();
    let parsed = filter::parse("nonexistent == 5").unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subject = Subject {
        sample: &samples[0],
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    assert!(filter::evaluate(&parsed, &subject).is_err());
    assert!(!filter::check(&parsed, &refs(&samples)).is_empty());
}

#[test]
fn unknown_field_suggests_nearest_name() {
    let samples = collection();
    let parsed = filter::parse("bere == x").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    let rendered = format!("{:?}", diagnostics.unknown_fields);
    assert!(rendered.contains("beer"), "{rendered}");
}

#[test]
fn check_reports_every_problem_at_once() {
    // Three mistakes yield three diagnostics from one run.
    let samples = collection();
    let parsed = filter::parse("bere == x && nonexistent == 5 && malt > \"heavy\"").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert_eq!(diagnostics.unknown_fields.len(), 2, "{diagnostics:?}");
    assert_eq!(diagnostics.type_conflicts.len(), 1, "{diagnostics:?}");
}

#[test]
fn type_conflict_is_reported() {
    // `malt > "heavy"` errors rather than matching nothing.
    let samples = collection();
    let parsed = filter::parse("malt > \"heavy\"").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert_eq!(diagnostics.type_conflicts.len(), 1);
    let message =
        FilterError::TypeConflict(Box::new(diagnostics.type_conflicts[0].clone())).to_string();
    assert!(message.contains("heavy"), "{message}");
}

// ----------------------------------------------------------- syntax errors

#[test]
fn unbalanced_parenthesis_reports_the_opening_position() {
    // Not the end of input: the reader needs to see which one was never closed.
    let source = "status == approved && (plato > 2";
    let error = filter::parse(source).unwrap_err();
    let FilterError::UnbalancedParenthesis { position } = error else {
        panic!("{error:?}");
    };
    assert_eq!(position, source.find('(').unwrap());
    let rendered = filter::caret(source, &FilterError::UnbalancedParenthesis { position }).unwrap();
    assert!(
        rendered.lines().nth(2).unwrap().ends_with('^'),
        "{rendered}"
    );
}

#[test]
fn unterminated_string_is_reported() {
    let source = "beer == \"Dunkel 57";
    let error = filter::parse(source).unwrap_err();
    let FilterError::UnterminatedString { position } = error else {
        panic!("{error:?}");
    };
    assert_eq!(position, source.find('"').unwrap());
}

#[test]
fn filter_declares_its_fields() {
    // `fields()` lists exactly what is referenced.
    let parsed = filter::parse("malt > 3 && status == approved && malt < 100").unwrap();
    let named: Vec<String> = filter::fields(&parsed)
        .iter()
        .map(fields::describe)
        .collect();
    assert_eq!(named, ["malt", "status"]);
}

#[test]
fn parse_does_not_need_a_sample() {
    // A filter validates against no data, which is what lets a config hold one.
    assert!(filter::parse("nonexistent == 5").is_ok());
    assert!(filter::parse("mashing.wort[65] > 1").is_ok());
}

#[test]
fn has_tests_membership_of_a_list_attribute() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    let vocabulary = fields::Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    assert_eq!(
        filter::evaluate(&filter::parse("temperatures has 30").unwrap(), &subject).unwrap(),
        Truth::True
    );
    assert_eq!(
        filter::evaluate(&filter::parse("temperatures has 40").unwrap(), &subject).unwrap(),
        Truth::False
    );
}

#[test]
fn scalar_operators_on_a_whole_list_are_refused() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20)]).unwrap(),
        )
        .unwrap();
    let vocabulary = fields::Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let error =
        filter::evaluate(&filter::parse("temperatures == 20").unwrap(), &subject).unwrap_err();
    assert!(matches!(error, FilterError::WrongOperator { .. }));
    assert!(error.to_string().contains("has"), "{error}");
}

#[test]
fn a_date_operand_selects_every_time_on_that_day() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("measured_at"),
            Value::date_time(DateTime::parse("2026-03-14T10:30").unwrap()),
        )
        .unwrap();
    let vocabulary = fields::Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    for expression in ["measured_at == 2026-03-14", "measured_at >= 2026-03-14"] {
        assert_eq!(
            filter::evaluate(&filter::parse(expression).unwrap(), &subject).unwrap(),
            Truth::True,
            "{expression}"
        );
    }
    assert_eq!(
        filter::evaluate(
            &filter::parse("measured_at < 2026-03-14").unwrap(),
            &subject
        )
        .unwrap(),
        Truth::False
    );
}

#[test]
fn check_reports_a_wrong_operator_with_its_reason() {
    let samples = collection();
    let parsed = filter::parse("tags contains reference").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert!(diagnostics.type_conflicts.is_empty(), "{diagnostics:?}");
    assert_eq!(diagnostics.wrong_operators.len(), 1, "{diagnostics:?}");
    assert!(!diagnostics.is_empty());
    let message = diagnostics.wrong_operators[0].error().to_string();
    assert!(message.contains("'has'"), "{message}");
    assert!(!message.contains("nothing"), "{message}");
}

#[test]
fn the_words_and_or_and_not_are_names() {
    // The connectives are `&&`, `||` and `!`. The words are refused where an
    // operator stood, naming the symbol to write; as a field's name each is an
    // ordinary word, and `is not missing` keeps its `not`.
    for (source, symbol, at) in [
        ("status == approved and malt > 5", "'&&'", 20),
        ("status == reviewed OR batch == 7", "'||'", 20),
        ("not status == approved", "'!'", 1),
        ("not (malt > 5)", "'!'", 1),
    ] {
        let message = filter::parse(source).unwrap_err().to_string();
        assert!(message.contains(&format!("expected {symbol}")), "{message}");
        assert!(message.contains("is a name"), "{message}");
        assert!(
            message.contains(&format!("at character {at}:")),
            "{message}"
        );
    }
    for source in ["not > 3", "and == x && or has y", "not is not missing"] {
        let parsed = filter::parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert!(!filter::fields(&parsed).is_empty());
    }
    assert_eq!(selects("!(status == approved || malt > 100)"), ["B"]);
    assert_eq!(selects("status != approved"), ["B"]);
}

#[test]
fn a_lone_ampersand_or_bar_is_refused() {
    for (source, expected, at) in [
        ("malt > 5 & batch == 7", "'&&'", 10),
        ("malt > 5 | batch == 7", "'||'", 10),
    ] {
        let error = filter::parse(source).unwrap_err();
        let message = error.to_string();
        assert!(message.contains(expected), "{message}");
        assert!(message.contains(&format!("at character {at}")), "{message}");
    }
}

#[test]
fn has_on_a_sample_without_the_list_is_unknown() {
    let mut with = Sample::new();
    with.set_attribute(
        id("adjuncts"),
        AttributeValue::list(vec![Value::text("oats")]).unwrap(),
    )
    .unwrap();
    let samples = vec![with, Sample::new()];
    let parsed = filter::parse("adjuncts has oats").unwrap();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let truth = |sample: &Sample| {
        filter::evaluate(
            &parsed,
            &Subject {
                sample,
                path: None,
                vocabulary: &vocabulary,
                states: None,
            },
        )
        .unwrap()
    };
    assert_eq!(truth(&samples[0]), Truth::True);
    assert_eq!(truth(&samples[1]), Truth::Unknown);
    assert!(filter::check(&parsed, &refs(&samples)).is_empty());
}

#[test]
fn a_row_no_sample_holds_is_an_unknown_field() {
    let samples = collection();
    let parsed = filter::parse("mashing.wort[500] > 1").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert_eq!(diagnostics.unknown_fields.len(), 1, "{diagnostics:?}");
}

#[test]
fn a_stray_closing_parenthesis_closes_nothing() {
    let error = filter::parse("yeast_strain == US05)").unwrap_err();
    assert!(
        matches!(error, FilterError::UnexpectedClose { position: 20 }),
        "{error:?}"
    );
    assert!(error.to_string().contains("closes nothing"), "{error}");
}

#[test]
fn single_quotes_quote_text_too() {
    assert_eq!(
        selects("status == 'approved'"),
        selects("status == \"approved\"")
    );
    assert!(matches!(
        filter::parse("status == 'approved"),
        Err(FilterError::UnterminatedString { .. })
    ));
}

#[test]
fn a_number_too_large_is_refused_not_read_as_text() {
    let error = filter::parse("loading > 1e999").unwrap_err();
    assert!(error.to_string().contains("a finite number"), "{error}");
    // A word that is no number stays text.
    assert!(filter::parse("yeast_strain == nan").is_ok());
}

#[test]
fn a_quoted_date_is_a_date() {
    assert_eq!(
        selects("brewed_on >= \"2026-01-01\""),
        selects("brewed_on >= 2026-01-01")
    );
    assert_eq!(selects("brewed_on >= \"2026-01-01\""), ["A"]);
}

#[test]
fn a_number_compared_with_text_is_an_error() {
    let samples = collection();
    let parsed = filter::parse("malt == \"heavy\"").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert_eq!(diagnostics.type_conflicts.len(), 1, "{diagnostics:?}");
}

#[test]
fn text_compared_with_a_number_is_an_error_too() {
    // A type conflict is symmetric. `beer == 3` used to answer *nothing
    // matched*, where `malt == "heavy"` was refused.
    let samples = collection();
    for source in ["beer == 3", "beer != 3", "beer == 2.5"] {
        let parsed = filter::parse(source).unwrap();
        let diagnostics = filter::check(&parsed, &refs(&samples));
        assert_eq!(
            diagnostics.type_conflicts.len(),
            1,
            "{source}: {diagnostics:?}"
        );
        let message =
            FilterError::TypeConflict(Box::new(diagnostics.type_conflicts[0].clone())).to_string();
        assert!(message.contains("beer"), "{message}");
        assert!(message.contains("text"), "{message}");
    }
}

#[test]
fn ordered_comparison_on_text_ignores_case() {
    // The sort orders `altbier` before `Dunkel 57`, and the filter agrees.
    assert_eq!(selects("beer > \"b\""), ["A", "C"]);
    assert_eq!(selects("beer < \"E\""), ["A", "B", "C"]);
    assert_eq!(selects("beer < \"b\""), ["B"]);
    assert_eq!(selects("beer >= \"dc 57\""), ["A", "C"]);
}

// ------------------------------------------------------------- completion

/// Every candidate the collection offers, unioned in collection order. One
/// subject answers for one sample; a caller holding a collection unions them,
/// which is how the menu comes to show what the collection holds.
fn completes(source: &str) -> Vec<String> {
    let samples = collection();
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let mut offered: Vec<String> = Vec::new();
    for sample in &samples {
        let subject = Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        };
        for candidate in filter::complete(source, &subject) {
            if !offered.contains(&candidate) {
                offered.push(candidate);
            }
        }
    }
    offered
}

#[test]
fn completion_offers_fields_where_a_predicate_begins() {
    let empty = completes("");
    assert!(empty.contains(&"malt".to_string()), "{empty:?}");
    assert!(empty.contains(&"beer".to_string()), "{empty:?}");
    // And after a connective, which opens a new predicate.
    let after = completes("status == approved && ");
    assert!(
        after.contains(&"status == approved && malt".to_string()),
        "{after:?}"
    );
}

#[test]
fn completion_offers_operators_after_a_complete_field() {
    let offered = completes("malt ");
    assert!(offered.contains(&"malt == ".to_string()), "{offered:?}");
    assert!(
        offered.contains(&"malt contains ".to_string()),
        "{offered:?}"
    );
    // `has` belongs to tags and lists, and completion cannot propose what
    // `check` would refuse.
    assert!(!offered.contains(&"malt has ".to_string()), "{offered:?}");
    assert!(completes("tags ").contains(&"tags has ".to_string()));
}

#[test]
fn completion_offers_values_after_an_operator() {
    let offered = completes("beer == ");
    assert!(
        offered.contains(&"beer == altbier".to_string()),
        "{offered:?}"
    );
    // What the collection holds, never every string there could be.
    assert!(
        offered
            .iter()
            .all(|candidate| candidate.starts_with("beer == ")),
        "{offered:?}"
    );
}

#[test]
fn completion_offers_connectives_after_a_complete_predicate() {
    assert_eq!(
        completes("malt > 12 "),
        ["malt > 12 && ".to_string(), "malt > 12 || ".to_string()]
    );
}

#[test]
fn completion_finds_a_table_column_by_its_own_name() {
    // `wo` begins no field's full name, and is the column `mashing.wort`:
    // the filter offers its cells as the field completion does.
    let offered = completes("wo");
    assert!(
        offered.contains(&"mashing.wort[78.5]".to_string()),
        "{offered:?}"
    );
    let after = completes("malt > 1 && wo");
    assert!(
        after.contains(&"malt > 1 && mashing.wort[78.5]".to_string()),
        "{after:?}"
    );
}

#[test]
fn completion_carries_what_precedes_it() {
    let offered = completes("malt > 12 && bee");
    assert!(
        offered.contains(&"malt > 12 && beer".to_string()),
        "{offered:?}"
    );
    // The fragment alone would replace the whole expression in a shell.
    assert!(!offered.contains(&"beer".to_string()), "{offered:?}");
}

#[test]
fn completion_inside_an_unclosed_quote_offers_values() {
    // `parse` never sees a valid expression here: the string is unterminated.
    assert!(filter::parse("beer == \"Du").is_err());
    let offered = completes("beer == \"Du");
    assert!(
        offered.contains(&"beer == \"Dunkel 57\"".to_string()),
        "{offered:?}"
    );
}

#[test]
fn completion_quotes_a_value_holding_a_space() {
    let offered = completes("beer == ");
    assert!(
        offered.contains(&"beer == \"Dunkel 57\"".to_string()),
        "{offered:?}"
    );
}

#[test]
fn completion_never_fails_on_a_half_written_expression() {
    let whole = "malt > 12 && beer == \"Dunkel 57\"";
    for end in 0..=whole.len() {
        if whole.is_char_boundary(end) {
            let _ = completes(&whole[..end]);
        }
    }
    // A head that does not tokenize leaves completion silent rather than wrong.
    assert!(completes("malt > 12 & ").is_empty());
}

#[test]
fn completion_continues_an_operator_being_typed() {
    // The tokenizer reads a lone `!` as the connective `not`. The position does
    // not, because the reader is still writing the operator.
    let offered = completes("malt !");
    assert!(offered.contains(&"malt != ".to_string()), "{offered:?}");
    assert!(!offered.contains(&"malt beer".to_string()), "{offered:?}");
    assert!(completes("malt >").contains(&"malt >= ".to_string()));
}

#[test]
fn completion_offers_values_inside_a_membership_list() {
    let offered = completes("status in (");
    assert!(
        offered.contains(&"status in (approved".to_string()),
        "{offered:?}"
    );
    // The parenthesis holds operands; it does not open another predicate.
    assert!(
        !offered.contains(&"status in (beer".to_string()),
        "{offered:?}"
    );
}

#[test]
fn completion_offers_presence_after_is() {
    assert_eq!(
        completes("malt is "),
        [
            "malt is missing ".to_string(),
            "malt is present ".to_string()
        ]
    );
}

#[test]
fn completion_leaves_a_space_after_a_word_operator() {
    // Against a closing quote, `has` without its space cannot be continued.
    assert!(completes("tags ").contains(&"tags has ".to_string()));
    assert!(completes("status ").contains(&"status in ".to_string()));
}

#[test]
fn completion_omits_operators_the_field_would_refuse() {
    let offered = completes("tags ");
    assert!(offered.contains(&"tags has ".to_string()), "{offered:?}");
    // `check` refuses a scalar operator on a whole list, and a menu proposing
    // what the checker refuses is worse than a shorter menu.
    assert!(
        !offered.contains(&"tags contains ".to_string()),
        "{offered:?}"
    );
    assert!(!offered.contains(&"tags == ".to_string()), "{offered:?}");
    assert!(!offered.contains(&"tags > ".to_string()), "{offered:?}");
}

#[test]
fn a_sample_whose_field_holds_text_is_left_aside_and_named() {
    // One brew's `og` written `high` refused `og > 1.06` over the whole
    // collection; it is left aside, named, and never selected, even under `!`.
    // Text everywhere is still a question that cannot be asked.
    let mut samples = Vec::new();
    for (name, og) in [
        ("a", number(1.05)),
        ("b", Value::text("high")),
        ("c", number(1.07)),
    ] {
        let mut sample = Sample::new();
        sample.set_name(Some(name.to_string()));
        sample.set_property(id("og"), Property::stored(og)).unwrap();
        samples.push(sample);
    }
    let parsed = filter::parse("og > 1.06").unwrap();
    let diagnostics = filter::check(&parsed, &refs(&samples));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(diagnostics.set_aside.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics.set_aside[0].samples, vec!["b".to_string()]);
    assert_eq!(
        diagnostics.set_aside[0].said(),
        "b left aside: 'og' holds text there, which > cannot compare"
    );
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let truth = |source: &str, sample: &Sample| {
        let subject = Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        };
        filter::evaluate_leaving_aside(&filter::parse(source).unwrap(), &subject).unwrap()
    };
    assert_eq!(truth("og > 1.06", &samples[2]), Truth::True);
    assert_eq!(truth("og > 1.06", &samples[1]), Truth::Unknown);
    assert_eq!(truth("!(og > 1.06)", &samples[1]), Truth::Unknown);
    // Two comparisons of one field are one sentence.
    let both = filter::check(&filter::parse("og > 1 && og < 2").unwrap(), &refs(&samples));
    assert_eq!(both.set_aside.len(), 1, "{both:?}");
    assert!(both.set_aside[0].said().contains("> and <"), "{both:?}");
    // Text in every sample: refused, as ever.
    let words = filter::check(&filter::parse("og > 1").unwrap(), &refs(&samples[1..2]));
    assert_eq!(words.type_conflicts.len(), 1, "{words:?}");
}

/// One sample's answer to a filter, its states given.
fn asked(source: &str, states: Option<&fields::States>) -> Result<Truth, FilterError> {
    let sample = Sample::new();
    let vocabulary = fields::Vocabulary::of(&sample);
    let parsed = filter::parse(source)?;
    filter::evaluate(
        &parsed,
        &Subject {
            sample: &sample,
            path: None,
            vocabulary: &vocabulary,
            states,
        },
    )
}

fn states_of(held: &[fields::State], model_read: bool) -> fields::States {
    fields::States::new(held.to_vec(), model_read)
}

#[test]
fn state_equals_asks_whether_a_sample_holds_the_word() {
    use fields::State::{Defective, Failed};
    let both = states_of(&[Failed, Defective], true);
    let failed = states_of(&[Failed], true);
    let clean = states_of(&[], true);
    // A set asked whether it holds a word, never compared whole.
    for states in [&both, &failed] {
        assert_eq!(asked("state == failed", Some(states)), Ok(Truth::True));
        assert_eq!(asked("state has failed", Some(states)), Ok(Truth::True));
        assert_eq!(asked("state != failed", Some(states)), Ok(Truth::False));
    }
    assert_eq!(asked("state == failed", Some(&clean)), Ok(Truth::False));
    assert_eq!(asked("state != failed", Some(&clean)), Ok(Truth::True));
    assert_eq!(asked("state == current", Some(&clean)), Ok(Truth::True));
    assert_eq!(asked("state == defective", Some(&failed)), Ok(Truth::False));
    assert_eq!(
        asked("state in (outdated, defective)", Some(&both)),
        Ok(Truth::True)
    );
    assert_eq!(
        asked("state in (outdated, edited)", Some(&both)),
        Ok(Truth::False)
    );
    assert_eq!(
        asked("state == not_current", Some(&failed)),
        Ok(Truth::True)
    );
}

#[test]
fn a_state_only_the_model_could_deny_stays_unknown() {
    use fields::State::Stale;
    let unread = states_of(&[], false);
    assert_eq!(
        asked("state == never_computed", Some(&unread)),
        Ok(Truth::Unknown)
    );
    // Its negation too: a sample nobody asked the model about is not known
    // to owe nothing.
    assert_eq!(
        asked("state != never_computed", Some(&unread)),
        Ok(Truth::Unknown)
    );
    assert_eq!(asked("state == outdated", Some(&unread)), Ok(Truth::False));
    let stale = states_of(&[Stale], false);
    assert_eq!(asked("state == outdated", Some(&stale)), Ok(Truth::True));
    assert_eq!(asked("state == not_current", Some(&stale)), Ok(Truth::True));
    assert_eq!(asked("state == current", Some(&stale)), Ok(Truth::False));
}

#[test]
fn an_unknown_state_word_is_refused_at_parse() {
    let error = filter::parse("state == faild").unwrap_err();
    let FilterError::UnknownState {
        position,
        suggestion,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(*position, 9);
    assert_eq!(suggestion.as_deref(), Some("failed"));
    let said = error.to_string();
    assert!(said.contains("available: current, not_current"), "{said}");
    assert!(filter::caret("state == faild", &error).is_some());
    // The word `outdated` replaced is not a state, and is sent to it, though no
    // edit distance would.
    match filter::parse("state == stale") {
        Err(FilterError::UnknownState { suggestion, .. }) => {
            assert_eq!(suggestion.as_deref(), Some("outdated"));
        }
        other => panic!("{other:?}"),
    }
    let quoted = filter::parse("state == \"failed\"").unwrap();
    assert_eq!(filter::state_words(&quoted).len(), 1);
}

#[test]
fn state_refuses_an_order_and_presence() {
    for source in ["state > failed", "state contains fail", "state is missing"] {
        match filter::parse(source) {
            Err(FilterError::WrongOperator { reason, .. }) => {
                assert!(reason.contains("==, has and in"), "{source}: {reason}");
            }
            other => panic!("{source}: {other:?}"),
        }
    }
}

#[test]
fn state_unread_is_an_error_not_an_empty_selection() {
    assert!(matches!(
        asked("state == failed", None),
        Err(FilterError::Field(error)) if *error == fields::FieldError::StatesNotRead
    ));
    let samples = collection();
    let parsed = filter::parse("state == failed").unwrap();
    assert!(filter::check(&parsed, &refs(&samples)).is_empty());
}

#[test]
fn completion_offers_the_states_after_state() {
    let operators = completes("state ");
    for operator in ["==", "!=", "has", "in"] {
        assert!(
            operators.contains(&format!("state {operator} ")),
            "{operators:?}"
        );
    }
    assert!(!operators.contains(&"state > ".to_string()));
    let words = completes("state == ");
    assert_eq!(words.len(), 8, "{words:?}");
    assert!(words.contains(&"state == never_computed".to_string()));
    let listed = completes("state in (");
    assert!(
        listed.contains(&"state in (defective".to_string()),
        "{listed:?}"
    );
}
