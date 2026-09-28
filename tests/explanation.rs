//! The tests of `explanation`.

use samplekit::collection::explanation::{
    self, ColumnKind, ExplainError, Explanation, InputState, Origin,
};
use samplekit::core::identifier::Identifier;
use samplekit::format::document;
use samplekit::format::fingerprint::Freshness;
use samplekit::query::field_addressing as fields;

const SAMPLE: &str = "---\nschema_version: 1\nname: E\nproperties:\n  \
    ibu:\n    v: 12.0\n    u: 0.005773502691896258\n    unit: mL\n    \
    computed: {u: {}}\n    fingerprint: 18debc327c6f\n  \
    headspace:\n    v: 1.1309733552923256\n    unit: hL\n    \
    computed: {v: {ibu: fce15bfbafe1}}\n    fingerprint: 5bcbffb74128\n  \
    malt: {v: 14.21, readings: [14.21, 14.23, 14.19], unit: mg}\n\
    tables:\n  conditioning:\n    index: day\n    columns:\n      day: {}\n      co2: {unit: vol}\n    \
    rows:\n      - {day: 1, co2: 2.1}\n---\nN.\n";

fn explained(field: &str) -> Result<Explanation, ExplainError> {
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-explanation-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
    std::fs::write(&path, SAMPLE).unwrap();
    let (sample, _) = document::load_sample(&path).expect("the fixture reads");
    let _ = std::fs::remove_file(&path);
    let vocabulary = fields::vocabulary_of(&[&sample]);
    explanation::explain(&sample, None, None, field, &vocabulary)
}

#[test]
fn a_computed_value_names_its_inputs_and_their_states() {
    let Explanation::Value(value) = explained("headspace").unwrap() else {
        panic!("a value");
    };
    let Origin::Inputs { inputs, .. } = value.origin else {
        panic!("inputs: {:?}", value.origin);
    };
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].name, "ibu");
    assert_eq!(inputs[0].field.as_deref(), Some("ibu"));
    assert!(matches!(inputs[0].state, Some(InputState::Of(_))));
    assert_eq!(value.state, Some(Freshness::Current));
}

#[test]
fn an_entered_value_and_a_table_are_explained() {
    let Explanation::Value(value) = explained("malt.u").unwrap() else {
        panic!("a value");
    };
    assert!(
        matches!(value.origin, Origin::Entered | Origin::Nothing),
        "{:?}",
        value.origin
    );
    let Explanation::Table(table) = explained("conditioning").unwrap() else {
        panic!("a table");
    };
    assert_eq!(table.name, Identifier::new("conditioning").unwrap());
    assert_eq!(table.rows, 1);
    assert_eq!(table.columns[0].kind, ColumnKind::Index);
    assert_eq!(table.columns[1].kind, ColumnKind::Measured);
    assert_eq!(table.columns[1].unit.as_deref(), Some("vol"));
}

#[test]
fn a_list_is_not_one_value() {
    assert_eq!(
        explained("malt.readings").unwrap_err(),
        ExplainError::NotOneValue
    );
    assert!(matches!(
        explained("maltt").unwrap_err(),
        ExplainError::Unknown(_)
    ));
}
