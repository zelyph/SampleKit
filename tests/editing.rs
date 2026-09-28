//! The tests of `editing`.

use samplekit::collection::editing::{self, Became};
use samplekit::core::identifier::Identifier;
use samplekit::core::value::Value;
use samplekit::format::document;
use samplekit::format::fingerprint::Freshness;

/// A sample whose `headspace` a formula computed, read without the model, and
/// whose `malt` holds readings, and a table with a derived column.
const SAMPLE: &str = "---\nschema_version: 1\nname: E\nproperties:\n  \
    ibu:\n    v: 12.0\n    u: 0.005773502691896258\n    unit: mL\n    \
    computed: {u: {}}\n    fingerprint: 18debc327c6f\n  \
    headspace:\n    v: 1.1309733552923256\n    unit: hL\n    \
    computed: {v: {ibu: fce15bfbafe1}}\n    fingerprint: 5bcbffb74128\n  \
    malt: {v: 14.21, readings: [14.21, 14.23, 14.19], u: 0.01, unit: mg}\n---\nN.\n";

fn sample() -> samplekit::core::sample::Sample {
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-editing-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::write(&path, SAMPLE).unwrap();
    let (sample, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    sample
}

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

#[test]
fn an_override_keeps_its_record_on_every_surface() {
    // Through `apply`, as `set` and the workbench call it.
    let mut edited = sample();
    let change =
        editing::apply(&mut edited, "headspace", Value::number(1.5).unwrap(), None).unwrap();
    assert_eq!(change.became, Became::Override);
    let records = edited.property(&id("headspace")).unwrap().records();
    assert!(records.computed.is_some(), "the formula's record stays");
    assert!(records.fingerprint.is_some_and(|digest| digest.is_edited()));

    // Through `set_by_hand`, as Python's setter calls it: the same record.
    let by_hand = sample();
    editing::set_by_hand(
        &by_hand.property(&id("headspace")).unwrap(),
        Value::number(1.5).unwrap(),
    );
    assert_eq!(
        by_hand.property(&id("headspace")).unwrap().records(),
        edited.property(&id("headspace")).unwrap().records()
    );
}

#[test]
fn a_typed_text_is_the_value_it_plainly_is() {
    assert_eq!(editing::value_of("4").unwrap(), Value::integer(4));
    assert_eq!(
        editing::value_of("4.5").unwrap(),
        Value::number(4.5).unwrap()
    );
    assert_eq!(editing::value_of("true").unwrap(), Value::boolean(true));
    assert!(matches!(
        editing::value_of("2026-03-02").unwrap(),
        Value::Date(_)
    ));
    assert_eq!(
        editing::value_of("altbier").unwrap(),
        Value::text("altbier")
    );
    assert!(editing::value_of("").unwrap().is_absent());
    let list = editing::value_of("[1, 2]").unwrap_err();
    // Readings are a channel of their own.
    assert!(list.message.contains("fg.readings=[1, 2]"), "{list}");
    assert!(editing::split_assignment("malt").is_err());
}

#[test]
fn text_where_a_number_belongs_is_refused() {
    let mut edited = sample();
    let refused = editing::apply(&mut edited, "ibu", Value::text("abc"), None).unwrap_err();
    assert!(refused.message.contains("measured in mL"), "{refused}");
    assert_eq!(
        editing::value_at(&edited, "ibu"),
        Some(Value::number(12.0).unwrap())
    );
}

#[test]
fn a_value_beside_readings_keeps_them() {
    let mut edited = sample();
    let change = editing::apply(&mut edited, "malt", Value::number(14.0).unwrap(), None).unwrap();
    let kept = change.kept.expect("the readings are named");
    assert_eq!(kept.readings, [14.21, 14.23, 14.19]);
    assert_eq!(kept.uncertainty, Some(0.01));
    assert!(edited.property(&id("malt")).unwrap().readings().is_some());
}

#[test]
fn not_current_lists_a_column_once() {
    let mut edited = sample();
    assert!(editing::not_current(&edited).is_empty());
    editing::apply(&mut edited, "ibu", Value::number(13.0).unwrap(), None).unwrap();
    let behind = editing::not_current(&edited);
    assert_eq!(behind.len(), 1, "{behind:?}");
    assert_eq!(behind[0].name, "headspace");
    assert!(
        matches!(behind[0].state, Freshness::Stale { .. }),
        "{behind:?}"
    );
}

/// A table with a derived column, read without the model.
const WITH_TABLE: &str = "---\nschema_version: 1\nname: T\ntables:\n  conditioning:\n    index: day\n    \
    columns:\n      day: {}\n      co2: {unit: vol}\n      efficiency: {unit: \"%\"}\n    rows:\n      \
    - day: 1\n        co2: 2.0\n        efficiency: {v: 90.0, computed: {row.co2: aaaaaaaaaaaa}, \
    fingerprint: bbbbbbbbbbbb}\n      - day: 2\n        co2: 1.9\n---\n";

fn tabled() -> samplekit::core::sample::Sample {
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-editing-table-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::write(&path, WITH_TABLE).unwrap();
    let (sample, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    sample
}

#[test]
fn a_cell_takes_a_number_and_an_index_names_one_row() {
    let mut edited = tabled();
    // Text where the column holds numbers is refused, as for a quantity.
    let error = editing::apply(&mut edited, "conditioning.co2[1]", Value::text("abc"), None)
        .unwrap_err()
        .message;
    assert!(
        error.contains("measured in vol") && error.contains("is text"),
        "{error}"
    );
    // An index another row holds would name two rows.
    let error = editing::apply(&mut edited, "conditioning.day[1]", Value::integer(2), None)
        .unwrap_err()
        .message;
    assert!(error.contains("already has a row at (2)"), "{error}");
    // Moved to an index nobody holds, it is a change.
    let change =
        editing::apply(&mut edited, "conditioning.day[1]", Value::integer(3), None).unwrap();
    assert_eq!(change.became, Became::Changed);
    // The row answers to its new index, and the old one is free again.
    let error = editing::apply(&mut edited, "conditioning.day[2]", Value::integer(3), None)
        .unwrap_err()
        .message;
    assert!(error.contains("already has a row at (3)"), "{error}");
    editing::apply(
        &mut edited,
        "conditioning.co2[3]",
        Value::number(2.1).unwrap(),
        None,
    )
    .unwrap();
    editing::apply(&mut edited, "conditioning.day[2]", Value::integer(1), None).unwrap();
    // An index is never emptied, and carries no uncertainty.
    for (field, value) in [
        ("conditioning.day[3]", Value::absent()),
        ("conditioning.day[3].u", Value::number(0.1).unwrap()),
    ] {
        let error = editing::apply(&mut edited, field, value, None)
            .unwrap_err()
            .message;
        assert!(error.contains("is the table's index"), "{error}");
    }
    // A row given a column twice, or text where numbers go, is refused.
    let twice = editing::add_row(
        &mut tabled(),
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(9)),
            ("co2".to_string(), Value::number(1.0).unwrap()),
            ("co2".to_string(), Value::number(2.0).unwrap()),
        ],
        |_| None,
    );
    assert!(twice.is_err_and(|error| error.message.contains("given twice")));
    let text = editing::add_row(
        &mut tabled(),
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(9)),
            ("co2".to_string(), Value::text("x")),
        ],
        |_| None,
    );
    assert!(text.is_err_and(|error| error.message.contains("is text")));
}

#[test]
fn a_declaration_without_a_unit_takes_text() {
    // `[property.style]` naming only how a figure's legend says it: `style=ipa`
    // was refused with *write a number*.
    use samplekit::config::project_config::PropertyDeclaration;
    let symbols = PropertyDeclaration {
        symbol: Some("Style".to_string()),
        ..PropertyDeclaration::default()
    };
    let mut edited = sample();
    let change = editing::apply(&mut edited, "style", Value::text("ipa"), Some(&symbols)).unwrap();
    assert!(
        matches!(change.became, Became::NewAttribute { .. }),
        "{:?}",
        change.became
    );
    // A number is still the quantity it declares.
    let change = editing::apply(
        &mut edited,
        "bitterness",
        Value::integer(40),
        Some(&symbols),
    )
    .unwrap();
    assert_eq!(change.became, Became::NewQuantity);
    // With a unit, text is refused.
    let measured = PropertyDeclaration {
        unit: Some("g".to_string()),
        ..PropertyDeclaration::default()
    };
    let error = editing::apply(&mut edited, "grain", Value::text("heavy"), Some(&measured))
        .unwrap_err()
        .message;
    assert!(error.contains("write a number"), "{error}");
}

#[test]
fn a_new_rows_hand_value_in_a_derived_column_is_an_override() {
    // Stored bare, it read *source* without the model while the preview said
    // an override: it carries the column's records, marked edited, as a cell
    // set by hand does.
    let mut edited = tabled();
    let added = editing::add_row(
        &mut edited,
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(3)),
            ("co2".to_string(), Value::number(1.8).unwrap()),
            ("efficiency".to_string(), Value::number(88.0).unwrap()),
        ],
        |_| None,
    )
    .unwrap();
    assert!(
        added
            .given
            .iter()
            .any(|(column, _, overriding)| column.as_str() == "efficiency" && *overriding)
    );
    let freshness = samplekit::format::fingerprint::check_cell(
        &edited,
        &id("conditioning"),
        &id("efficiency"),
        &[Value::integer(3)],
    )
    .unwrap();
    assert_eq!(freshness, Freshness::Edited);
    // A measured column given by hand is no override.
    let co2 = samplekit::format::fingerprint::check_cell(
        &edited,
        &id("conditioning"),
        &id("co2"),
        &[Value::integer(3)],
    )
    .unwrap();
    assert_eq!(co2, Freshness::Source);
}

#[test]
fn a_derived_cell_left_as_it_was_is_not_an_override() {
    let mut edited = tabled();
    // Its uncertainty cleared where it has none: nothing changes, nothing is
    // marked.
    let change = editing::apply(
        &mut edited,
        "conditioning.efficiency[1].u",
        Value::absent(),
        None,
    )
    .unwrap();
    assert_eq!(change.became, Became::Unchanged);
    let table = edited.table(&id("conditioning")).unwrap();
    let row = table.rows().next().unwrap();
    let records = row.cell(&id("efficiency")).unwrap().records().clone();
    assert!(!records.fingerprint.is_some_and(|digest| digest.is_edited()));
    // Its value set by hand is an override, the formula's record kept.
    let change = editing::apply(
        &mut edited,
        "conditioning.efficiency[1]",
        Value::number(50.0).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(change.became, Became::Override);
}

#[test]
fn an_uncertainty_for_a_cell_that_is_not_there_says_what_is() {
    let mut edited = tabled();
    let error = editing::apply(
        &mut edited,
        "conditioning.c2[1].u",
        Value::number(0.1).unwrap(),
        None,
    )
    .unwrap_err()
    .message;
    assert!(error.contains("co2"), "{error}");
    let error = editing::apply(
        &mut edited,
        "conditioning.co2[99].u",
        Value::number(0.1).unwrap(),
        None,
    )
    .unwrap_err()
    .message;
    assert!(error.contains("99"), "{error}");
}

#[test]
fn a_column_is_listed_once_for_each_way_its_rows_stand() {
    // An edited first row said the whole column was edited, and the stale
    // rows under it were counted as overrides.
    let text = "---\nschema_version: 1\nname: T\ntables:\n  conditioning:\n    index: day\n    \
    columns:\n      day: {}\n      co2: {unit: vol}\n      efficiency: {unit: \"%\"}\n    rows:\n      \
    - day: 1\n        co2: 2.0\n        efficiency: {v: 90.0, computed: {row.co2: aaaaaaaaaaaa}, \
    fingerprint: bbbbbbbbbbbb}\n      - day: 2\n        co2: 1.9\n        efficiency: {v: 80.0, \
    computed: {row.co2: aaaaaaaaaaaa}}\n      - day: 3\n        co2: 1.9\n        efficiency: \
    {v: 80.0, computed: {row.co2: aaaaaaaaaaaa}}\n---\n";
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-ways-{}.md", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let (sample, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    let found: Vec<(String, bool, usize)> = editing::not_current(&sample)
        .into_iter()
        .map(|entry| {
            (
                entry.name,
                matches!(entry.state, Freshness::Stale { .. }),
                entry.rows,
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("conditioning.efficiency".to_string(), false, 1),
            ("conditioning.efficiency".to_string(), true, 2)
        ]
    );
}

#[test]
fn readings_are_refused_for_a_computed_value() {
    let mut edited = sample();
    let readings = editing::readings_of("headspace", "1.0,1.1").unwrap();
    let error = editing::set_readings(&mut edited, &id("headspace"), readings)
        .unwrap_err()
        .message;
    assert!(error.contains("computed by the model"), "{error}");
    assert!(
        edited
            .property(&id("headspace"))
            .unwrap()
            .readings()
            .is_none()
    );
    // A statistic of readings is recorded as computed, and takes new ones.
    let text = "---\nschema_version: 1\nname: S\nproperties:\n  malt: {v: 2.0, readings: [1.0, 3.0], \
                statistics: {v: mean}, computed: {readings: aaaaaaaaaaaa}, fingerprint: \
                bbbbbbbbbbbb}\n---\n";
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-stat-{}.md", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let (mut measured, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    let readings = editing::readings_of("malt", "4.0,5.0").unwrap();
    editing::set_readings(&mut measured, &id("malt"), readings).unwrap();
}

#[test]
fn a_new_name_is_no_path_and_no_other_samples_in_its_project() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("cellar")).unwrap();
    std::fs::write(root.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    std::fs::write(root.join("A.md"), "---\nschema_version: 1\nname: A\n---\n").unwrap();
    std::fs::write(root.join("cellar/.samplekitrc"), "schema_version = 1\n").unwrap();
    std::fs::write(
        root.join("cellar/B.md"),
        "---\nschema_version: 1\nname: B\n---\n",
    )
    .unwrap();
    let collection = samplekit::collection::sample_list::from_directory(&root).unwrap();
    let project = collection
        .config()
        .map(|config| config.root().to_path_buf());
    let refused = |name: &str| editing::refuses_name(name, &collection, project.as_deref());
    assert!(refused("cells/C").is_some_and(|error| error.message.contains("is a path")));
    assert!(refused("../C").is_some());
    assert!(refused("A").is_some_and(|error| error.message.contains("already a sample's name")));
    // Another project's sample, nested below, keeps its own name.
    assert!(refused("B").is_none());
    assert!(refused("C").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_readings_leave_a_written_value_standing() {
    // New evidence, not an erasure — the value entered stays, written, and so
    // does the uncertainty entered beside it.
    let text =
        "---\nschema_version: 1\nname: S\nproperties:\n  malt: {v: 12.0, u: 0.1, unit: g}\n---\n";
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-stand-{}.md", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let (mut measured, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    let readings = editing::readings_of("malt", "11.9,12.2").unwrap();
    editing::set_readings(&mut measured, &id("malt"), readings).unwrap();
    let malt = measured.property(&id("malt")).unwrap();
    assert_eq!(malt.readings().unwrap().as_slice(), [11.9, 12.2]);
    let (written, spread, unit) = malt.peek(|property| {
        (
            property.written_value().cloned(),
            property.peek_uncertainty().flatten().map(|u| u.magnitude()),
            property.presentation().unit.clone(),
        )
    });
    assert_eq!(written, Some(Value::number(12.0).unwrap()));
    assert_eq!(spread, Some(0.1));
    assert_eq!(unit.as_deref(), Some("g"));
    // A value beside readings with no record is a value written, whoever wrote
    // it — the mean an earlier SampleKit wrote included — and stays too.
    let text = "---\nschema_version: 1\nname: S\nproperties:\n  malt: {v: 9.0, readings: [8.0, 10.0], unit: g}\n---\n";
    std::fs::write(&path, text).unwrap();
    let (mut measured, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    let readings = editing::readings_of("malt", "11,13").unwrap();
    editing::set_readings(&mut measured, &id("malt"), readings).unwrap();
    let malt = measured.property(&id("malt")).unwrap();
    assert_eq!(
        malt.peek(|property| property.written_value().cloned()),
        Some(Value::number(9.0).unwrap())
    );
    assert_eq!(malt.value().unwrap(), Value::number(9.0).unwrap());
}

#[test]
fn readings_that_look_like_decimal_commas_are_warned_about() {
    // `og.readings=1,050,1,051` read as four readings, mean 25.75, with
    // nothing said. Said now, with the spelling that cannot be misread
    //  — and not refused, since four whole numbers can be readings.
    let warned = editing::decimal_commas("og.readings", "1,050,1,051").unwrap();
    assert!(warned.contains("og.readings=[1.050, 1.051]"), "{warned}");
    let warned = editing::decimal_commas("t.readings", "[20,5,21,5]").unwrap();
    assert!(warned.contains("t.readings=[20.5, 21.5]"), "{warned}");
    assert!(warned.contains("4 readings"), "{warned}");
    assert_eq!(
        editing::readings_of("og.readings", "1,050,1,051")
            .unwrap()
            .as_slice(),
        &[1.0, 50.0, 1.0, 51.0]
    );
    // Readings written with points, and a plain zero, are readings, and
    // nothing is said of them.
    assert!(editing::decimal_commas("og.readings", "1.050, 1.051, 0").is_none());
    assert!(editing::decimal_commas("og.readings", "12, 130, 14").is_none());
}

#[test]
fn readings_are_a_channel_with_or_without_brackets() {
    // `og.readings=1.05,1.06` and `og.readings=[1.05, 1.06]` are one list; one
    // number is a value, and is refused as readings.
    for written in ["1.05,1.06", "[1.05, 1.06]", " [1.05,1.06] "] {
        assert_eq!(
            editing::readings_of("og.readings", written)
                .unwrap()
                .as_slice(),
            &[1.05, 1.06],
            "{written}"
        );
    }
    let one = editing::readings_of("og.readings", "[1.05]").unwrap_err();
    assert!(one.message.contains("og=1.05"), "{one}");
    assert!(editing::readings_of("og.readings", "[]").is_err());
    assert!(editing::gives_readings("og.readings"));
    assert!(editing::gives_readings("mouthfeel.sweetness[40].readings"));
    assert!(!editing::gives_readings("og"));
    assert!(!editing::gives_readings("og.u"));
}

#[test]
fn a_cells_readings_keep_what_was_written_beside_them() {
    // In a cell: new readings keep the value the cell held,
    // written beside them; a value written over readings keeps them; an index
    // takes none.
    let mut edited = tabled();
    let row = samplekit::core::table::RowAddress::Index(vec![Value::integer(1)]);
    let before = edited
        .table(&id("conditioning"))
        .unwrap()
        .at(&row, &id("co2"))
        .unwrap()
        .value()
        .unwrap();
    let readings = editing::readings_of("x.readings", "1.0,2.0").unwrap();
    editing::set_cell_readings(&mut edited, &id("conditioning"), &row, &id("co2"), readings)
        .unwrap();
    let cell = |sample: &samplekit::core::sample::Sample| {
        let held = sample.table(&id("conditioning")).unwrap();
        let cell = held.at(&row, &id("co2")).unwrap();
        (
            cell.readings().map(|readings| readings.as_slice().to_vec()),
            cell.value().unwrap(),
        )
    };
    assert_eq!(cell(&edited), (Some(vec![1.0, 2.0]), before));
    editing::apply(
        &mut edited,
        "conditioning.co2[1]",
        Value::number(1.7).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        cell(&edited),
        (Some(vec![1.0, 2.0]), Value::number(1.7).unwrap())
    );
    let index = editing::set_cell_readings(
        &mut edited,
        &id("conditioning"),
        &row,
        &id("day"),
        editing::readings_of("x.readings", "1,2").unwrap(),
    )
    .unwrap_err();
    assert!(index.message.contains("index"), "{index}");
}

#[test]
fn a_row_takes_readings_in_a_cell() {
    // `--add-row conditioning day=9 co2.readings=1.0,2.0`.
    let mut edited = tabled();
    let added = editing::add_row_with_readings(
        &mut edited,
        "conditioning",
        vec![("day".to_string(), Value::integer(9))],
        vec![(
            "co2".to_string(),
            editing::readings_of("co2.readings", "1.0,2.0").unwrap(),
        )],
        |_| None,
    )
    .unwrap();
    assert_eq!(added.readings, vec![(id("co2"), vec![1.0, 2.0])]);
    let held = edited.table(&id("conditioning")).unwrap();
    let cell = held
        .at(
            &samplekit::core::table::RowAddress::Index(vec![Value::integer(9)]),
            &id("co2"),
        )
        .unwrap();
    assert_eq!(cell.readings().unwrap().as_slice(), &[1.0, 2.0]);
    // No statistic is declared for the column: the readings give no value.
    assert!(cell.value().unwrap().is_absent());
    let index = editing::add_row_with_readings(
        &mut tabled(),
        "conditioning",
        Vec::new(),
        vec![(
            "day".to_string(),
            editing::readings_of("day.readings", "1,2").unwrap(),
        )],
        |_| None,
    )
    .unwrap_err();
    assert!(index.message.contains("index"), "{index}");
}

#[test]
fn a_yes_or_no_is_refused_where_a_number_belongs() {
    // `og=true` was written `v: true` over a quantity holding a number, where
    // `og=abc` was refused.
    let mut edited = sample();
    let error = editing::apply(&mut edited, "malt", Value::boolean(true), None)
        .unwrap_err()
        .message;
    assert!(
        error.contains("measured in mg") && error.contains("a yes-or-no, not a number"),
        "{error}"
    );
    let mut tabled = tabled();
    let error = editing::apply(
        &mut tabled,
        "conditioning.co2[1]",
        Value::boolean(false),
        None,
    )
    .unwrap_err()
    .message;
    assert!(error.contains("a yes-or-no"), "{error}");
}

#[test]
fn several_numbers_in_a_cell_are_refused_as_readings() {
    // `sweetness=12.1,12.3,12.2` was stored as text in a column with a unit, and
    // the hint of `--add-row` named the address `set` takes.
    let comma = editing::add_row(
        &mut tabled(),
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(9)),
            ("co2".to_string(), Value::text("1,011")),
        ],
        |_| None,
    )
    .unwrap_err()
    .message;
    assert!(
        comma.contains("write the number alone: co2=1.011"),
        "{comma}"
    );
    let several = editing::add_row(
        &mut tabled(),
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(9)),
            ("co2".to_string(), Value::text("12.1,12.3,12.2")),
        ],
        |_| None,
    )
    .unwrap_err()
    .message;
    // Named as the readings they are, never as their mean.
    assert!(
        several.contains("co2.readings=12.1,12.3,12.2") && !several.contains("12.2\n"),
        "{several}"
    );
    // A table borrowed for its first row holds no cell: its column's unit
    // still refuses text.
    let mut bare = sample();
    let lent = {
        let source = tabled();
        let held = source.table(&id("conditioning")).unwrap();
        let columns = held
            .column_names()
            .into_iter()
            .map(|column| {
                (
                    column.clone(),
                    samplekit::core::table::ColumnMeta {
                        presentation: held
                            .presentation_of(
                                column,
                                &samplekit::core::table::RowAddress::ordinal(0),
                            )
                            .unwrap(),
                        statistics: Default::default(),
                    },
                )
            })
            .collect();
        samplekit::core::table::Table::new(
            id("conditioning"),
            held.index_columns().to_vec(),
            columns,
            Vec::new(),
        )
        .unwrap()
    };
    let first = editing::add_row(
        &mut bare,
        "conditioning",
        vec![
            ("day".to_string(), Value::integer(1)),
            ("co2".to_string(), Value::text("12.1,12.3,12.2")),
        ],
        |_| {
            Some(editing::Lent {
                from: std::path::PathBuf::from("other.md"),
                table: lent,
                derived: Vec::new(),
                records: Vec::new(),
            })
        },
    );
    assert!(first.is_err_and(|error| error.message.contains("measured in vol")));
}

#[test]
fn not_applicable_drops_the_uncertainty_and_keeps_the_readings() {
    // `fg=n/a` kept `u: 0.0004` and its readings' statistic went on giving one:
    // a value that does not apply has no spread.
    let mut edited = sample();
    editing::apply(&mut edited, "malt", Value::NotApplicable, None).unwrap();
    let handle = edited.property(&id("malt")).unwrap();
    assert_eq!(
        handle.peek(|property| property.peek_uncertainty()),
        Some(None)
    );
    assert!(handle.readings().is_some(), "the readings stay");
    assert!(
        handle
            .peek(|property| property.peek_value())
            .is_some_and(|value| value.is_not_applicable())
    );
}

#[test]
fn tags_are_set_by_their_own_command() {
    let error = editing::apply(&mut sample(), "tags", Value::text("a,b"), None)
        .unwrap_err()
        .message;
    assert!(error.contains("samplekit tag add"), "{error}");
    // A quantity and a word that is none of its channels: said so, not that
    // it named a column.
    let error = editing::apply(&mut sample(), "malt.x", Value::integer(1), None)
        .unwrap_err()
        .message;
    assert!(error.contains("x is no channel of malt"), "{error}");
}

#[test]
fn a_name_the_collection_holds_as_a_quantity_is_said_how() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    std::fs::write(
        root.join("a.md"),
        "---\nschema_version: 1\nname: a\nbatch: B1\nproperties:\n  og: 1.05\n  volume: {v: 20, unit: L}\n  malt: {v: 1, unit: g}\n---\n",
    )
    .unwrap();
    std::fs::write(
        root.join("b.md"),
        "---\nschema_version: 1\nname: b\nproperties:\n  volume: {v: 21, unit: L}\n  malt: {v: 1000, unit: mg}\n---\n",
    )
    .unwrap();
    let collection = samplekit::collection::sample_list::from_directory(&root).unwrap();
    let og = editing::held_as(&collection, "og").unwrap();
    assert!(og.numeric && og.unit.is_none() && og.units.is_empty());
    let volume = editing::held_as(&collection, "volume").unwrap();
    assert_eq!(volume.unit.as_deref(), Some("L"));
    let malt = editing::held_as(&collection, "malt").unwrap();
    assert_eq!(malt.unit, None);
    assert_eq!(malt.units, ["g", "mg"]);
    // An attribute is no quantity, and a name nobody holds is none.
    assert!(editing::held_as(&collection, "batch").is_none());
    assert!(editing::held_as(&collection, "nosuch").is_none());
    let refused = editing::not_a_number_for("og", None, &Value::text("1,050")).message;
    assert!(
        refused.contains("'og' holds a number, and '1,050' is text")
            && refused.contains("og=1.050"),
        "{refused}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_a_command_line_would_split_is_refused() {
    // A name is typed after `samplekit set` and in filters: a space splits it
    // into two words, and a leading dash reads as an option. On every surface,
    // since `new` and the workbench both ask here.
    let collection = samplekit::collection::sample_list::empty();
    let refused = |name: &str| editing::refuses_name(name, &collection, None);
    assert!(refused("pale ale").is_some_and(|error| error.message.contains("write pale-ale")));
    assert!(refused("pale\tale").is_some());
    assert!(refused("-pale").is_some_and(|error| error.message.contains("write pale instead")));
    assert!(refused("--").is_some_and(|error| error.message.contains("a letter or a digit")));
    assert!(refused("pale-ale").is_none());
    assert!(refused("pale_ale").is_none());
}

#[test]
fn a_blank_value_removes_only_what_is_there() {
    // A blank value removes the value there is, an edit; on a name the sample
    // holds nowhere it has nothing to remove.
    let mut edited = sample();
    let change = editing::apply(&mut edited, "style", Value::absent(), None).unwrap();
    assert_eq!(change.became, Became::Unchanged);
    assert!(!edited.has_attribute(&id("style")));
    editing::apply(&mut edited, "style", Value::text("ipa"), None).unwrap();
    let removed = editing::apply(&mut edited, "style", Value::absent(), None).unwrap();
    assert_ne!(removed.became, Became::Unchanged);
    assert_eq!(removed.before, Some(Value::text("ipa")));
}

#[test]
fn a_shape_writes_no_name() {
    // A new sample is named by its file, and writes no `name:` — nor carries
    // its pattern's.
    let bare = editing::shaped_like("C-02", None, &[]).unwrap();
    assert_eq!(bare.sample.name(), Some("C-02"));
    assert_eq!(bare.sample.written_name(), None);
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-editing-unnamed-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::write(
        &path,
        "---\nschema_version: 1\nname: C-01\nbatch: B7\n\
         properties:\n  malt: {v: 4.7, unit: g}\n---\n",
    )
    .unwrap();
    let (pattern, _) = document::load_sample(&path).expect("the pattern reads");
    let _ = std::fs::remove_file(&path);
    let shaped = editing::shaped_like("C-02", Some(&pattern), &[]).unwrap();
    assert_eq!(shaped.sample.name(), Some("C-02"));
    assert_eq!(shaped.sample.written_name(), None);
    assert!(
        !shaped.attributes.iter().any(|(name, _)| name == "name"),
        "{:?}",
        shaped.attributes
    );
}

#[test]
fn a_shape_leaves_the_patterns_dates_and_status() {
    // The rule the workbench's `N` kept for itself, in the shape both surfaces
    // take — a date, a list of dates and `status` are the pattern's own; a
    // batch and an operator describe the cask.
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-editing-own-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::write(
        &path,
        "---\nschema_version: 1\nname: P\nbatch: B7\nmade: 2026-09-01\n\
         dosed: [2026-09-02, 2026-09-03]\nstatus: approved\nmaltster: sam\n\
         properties:\n  malt: {v: 4.7, unit: g}\n---\n",
    )
    .unwrap();
    let (pattern, _) = document::load_sample(&path).expect("the pattern reads");
    let _ = std::fs::remove_file(&path);
    let shaped = editing::shaped_like("ph", Some(&pattern), &[]).unwrap();
    assert_eq!(shaped.not_carried, ["made", "dosed", "status"]);
    let carried: Vec<&str> = shaped
        .attributes
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(carried, ["batch", "maltster"]);
    for own in ["made", "dosed", "status"] {
        assert!(shaped.sample.attribute(&id(own)).is_err(), "{own} carried");
    }
    assert!(shaped.sample.attribute(&id("batch")).is_ok());
}

/// The model's description, written as the worker writes it, for a model no
/// Python here imports: `properties` and `tables` as the description holds
/// them, the rest taken from the project's configuration and files.
fn describe(root: &std::path::Path, properties: serde_json::Value, tables: serde_json::Value) {
    use samplekit::config::model_runtime as runtime;
    let config = samplekit::config::project_config::load_for(root)
        .unwrap()
        .expect("a configuration");
    let template = runtime::template_of(&config).expect("a model");
    let digest = runtime::digest_of(&template).unwrap();
    let description: runtime::ModelDescription = serde_json::from_value(serde_json::json!({
        "format": runtime::FORMAT,
        "samplekit": runtime::VERSION,
        "model": {
            "path": runtime::described_path(config.root(), template.path()),
            "class": template.class().unwrap_or("Model"),
            "digest": digest.as_str(),
        },
        "properties": properties,
        "tables": tables,
        "attributes": {},
        "figures": [],
    }))
    .unwrap();
    runtime::write_description(config.root(), &description).unwrap();
}

/// A property the model declares, entered, as a description holds it.
fn entered(unit: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "unit": unit, "value": {"from": "entered"}, "uncertainty": null,
        "reads": [], "formula": null, "default": null,
    })
}

#[test]
fn a_name_is_known_from_the_configuration_then_the_model_then_the_samples() {
    // `ibu` only the model declared was written as an attribute, which the
    // model then refused. The model is read from its description, current or
    // not read at all.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-known-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("model")).unwrap();
    std::fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n\n\
         [property.malt]\nunit = \"g\"\n\n[property.style]\nsymbol = \"S\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("model/keg.py"),
        "import samplekit as sk\n\nclass Keg(sk.Sample):\n    pass\n",
    )
    .unwrap();
    describe(
        &root,
        serde_json::json!({
            "malt": entered(Some("kg")),
            "ibu": entered(Some("mL")),
            "collar": entered(None),
            "style": entered(Some("cL")),
            "headspace": entered(None),
        }),
        serde_json::json!({}),
    );
    std::fs::write(
        root.join("a.md"),
        "---\nschema_version: 1\nname: a\nbatch: B1\nproperties:\n  \
         og: {v: 1.05, unit: SG}\n  collar: {v: 3, unit: cL}\n---\n",
    )
    .unwrap();
    std::fs::write(root.join("b.md"), "---\nschema_version: 1\nname: b\n---\n").unwrap();
    let collection = samplekit::collection::sample_list::from_directory(&root).unwrap();
    let config = collection.config().expect("the project's configuration");
    let (mut sample, _) = document::load_sample(&root.join("b.md")).unwrap();
    let known = |sample: &samplekit::core::sample::Sample, field: &str| {
        editing::known_as(sample, field, Some(config), Some(&collection))
    };

    // The configuration first, its unit before the model's, the two named.
    let malt = known(&sample, "malt").unwrap();
    assert_eq!(malt.from, editing::KnownFrom::Configuration);
    assert_eq!(malt.declaration.unit.as_deref(), Some("g"));
    assert_eq!(malt.disagreement, Some(("g".to_string(), "kg".to_string())));
    // A declaration without a unit takes the model's.
    let style = known(&sample, "style").unwrap();
    assert_eq!(style.from, editing::KnownFrom::Configuration);
    assert_eq!(style.declaration.unit.as_deref(), Some("cL"));
    assert_eq!(style.disagreement, None);
    // The model, from its file.
    let ibu = known(&sample, "ibu").unwrap();
    assert_eq!(
        ibu.from,
        editing::KnownFrom::Model(dunce::canonicalize(root.join("model/keg.py")).unwrap())
    );
    assert_eq!(ibu.declaration.unit.as_deref(), Some("mL"));
    let headspace = known(&sample, "headspace").unwrap();
    assert!(matches!(headspace.from, editing::KnownFrom::Model(_)));
    assert_eq!(headspace.declaration.unit, None);
    // No unit from the model: the samples' stands.
    let collar = known(&sample, "collar").unwrap();
    assert!(matches!(collar.from, editing::KnownFrom::Model(_)));
    assert_eq!(collar.declaration.unit.as_deref(), Some("cL"));
    // Then the samples.
    let og = known(&sample, "og").unwrap();
    assert_eq!(og.from, editing::KnownFrom::Samples);
    assert_eq!(og.declaration.unit.as_deref(), Some("SG"));
    // An attribute elsewhere, a name nobody declares, and a channel are none.
    assert!(known(&sample, "batch").is_none());
    assert!(known(&sample, "nosuch").is_none());
    assert!(known(&sample, "ibu.u").is_none());

    // And `apply` writes it as the quantity it is, with its unit.
    let change = editing::apply(
        &mut sample,
        "ibu",
        Value::number(25.1).unwrap(),
        Some(&ibu.declaration),
    )
    .unwrap();
    assert_eq!(change.became, Became::NewQuantity);
    let handle = sample.property(&id("ibu")).unwrap();
    assert_eq!(handle.presentation().unit.as_deref(), Some("mL"));

    // The model's file edited: the description is stale, and never read.
    std::fs::write(
        root.join("model/keg.py"),
        "import samplekit as sk\n\nclass Keg(sk.Sample):\n    pass  # edited\n",
    )
    .unwrap();
    let (fresh, _) = document::load_sample(&root.join("b.md")).unwrap();
    assert!(known(&fresh, "ibu").is_none());
    assert_eq!(
        known(&fresh, "collar").unwrap().from,
        editing::KnownFrom::Samples
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn text_for_a_property_the_model_declares_is_a_property() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-editing-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("model")).unwrap();
    std::fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n\n[model]\npath = \"model/keg.py\"\nclass = \"Keg\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("model/keg.py"),
        "import samplekit as sk\n\nclass Keg(sk.Sample):\n    pass\n",
    )
    .unwrap();
    describe(
        &root,
        serde_json::json!({"rating": entered(None), "collar": entered(Some("mL"))}),
        serde_json::json!({}),
    );
    std::fs::write(root.join("b.md"), "---\nschema_version: 1\nname: b\n---\n").unwrap();
    let collection = samplekit::collection::sample_list::from_directory(&root).unwrap();
    let config = collection.config().expect("the project's configuration");
    let (mut sample, _) = document::load_sample(&root.join("b.md")).unwrap();
    // Text, where the model gives the name no unit: a property.
    let rating = editing::known_as(&sample, "rating", Some(config), Some(&collection));
    let change =
        editing::apply_known(&mut sample, "rating", Value::text("A"), rating.as_ref()).unwrap();
    assert_eq!(change.became, Became::NewQuantity);
    assert!(sample.has_property(&id("rating")));
    assert!(!sample.has_attribute(&id("rating")));
    // A unit refuses text.
    let collar = editing::known_as(&sample, "collar", Some(config), Some(&collection));
    assert!(
        editing::apply_known(&mut sample, "collar", Value::text("tall"), collar.as_ref()).is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_name_is_edited_as_an_attribute() {
    // What the file writes, never the file's name: a blank one removes `name:`
    // and the file's name stands for it; a path and a line break are refused.
    let mut edited = samplekit::core::sample::Sample::new();
    edited.set_file(Some(std::path::Path::new("brews/s-1.md")));
    let typed = editing::typed("name", "007").unwrap();
    assert_eq!(typed, Value::text("007"));
    let named = editing::apply(&mut edited, "name", Value::text("Cask A"), None).unwrap();
    assert_eq!(named.became, Became::Changed);
    assert_eq!(named.before, Some(Value::text("s-1")));
    assert_eq!(edited.written_name(), Some("Cask A"));
    let removed = editing::apply(&mut edited, "name", Value::absent(), None).unwrap();
    assert_eq!(removed.became, Became::Cleared);
    assert_eq!(removed.after, Some(Value::text("s-1")));
    assert_eq!(edited.written_name(), None);
    let again = editing::apply(&mut edited, "name", Value::absent(), None).unwrap();
    assert_eq!(again.became, Became::Unchanged);
    // A name written that says what the file's name says is still removed.
    editing::apply(&mut edited, "name", Value::text("s-1"), None).unwrap();
    assert_eq!(edited.written_name(), Some("s-1"));
    let same = editing::apply(&mut edited, "name", Value::absent(), None).unwrap();
    assert_eq!(same.became, Became::Cleared);
    for refused in ["a/b", "a\\b", ".hidden", "a\nb", "a\tb"] {
        assert!(
            editing::apply(&mut edited, "name", Value::text(refused), None).is_err(),
            "{refused:?}"
        );
    }
    assert!(editing::refuses_written_name("Cask A, batch 3").is_none());
    assert!(editing::apply(&mut edited, "path", Value::text("x"), None).is_err());
}

#[test]
fn a_preview_shows_a_value_at_its_declared_precision() {
    // At the precision the project declares, its uncertainty's for `.u`, and
    // every digit without one; two values it writes alike said whole.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-editing-precision-{}",
            std::process::id()
        ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n[property.abv]\nprecision = [\".2f\", \".3f\"]\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    let noise = Value::number(6.081250000000001).unwrap();
    assert_eq!(editing::previewed(&noise, "abv", Some(&config)), "6.08");
    assert_eq!(editing::previewed(&noise, "abv.u", Some(&config)), "6.081");
    assert_eq!(
        editing::previewed(&noise, "og", Some(&config)),
        "6.081250000000001"
    );
    assert_eq!(editing::previewed(&noise, "abv", None), "6.081250000000001");
    let moved = Value::number(6.0812).unwrap();
    assert_eq!(
        editing::previewed_change(Some(&noise), Some(&moved), "abv", Some(&config)),
        ("6.081250000000001".to_string(), "6.0812".to_string())
    );
    assert_eq!(
        editing::previewed_change(None, Some(&Value::integer(7)), "abv", Some(&config)),
        ("—".to_string(), "7.00".to_string())
    );
    let _ = std::fs::remove_dir_all(&root);
}
