//! The tests of `summaries`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::sample_list as list;
use samplekit::config::profiles;
use samplekit::config::project_config::ColumnSpec;
use samplekit::core::value::Value;
use samplekit::presentation::summaries;

fn scratch(name: &str) -> PathBuf {
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-summaries-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    for (name, malt, beer) in [
        ("A", "12.0", "schwarz"),
        ("B", "10.0", "bock"),
        ("C", "", "schwarz"),
    ] {
        let malt = if malt.is_empty() {
            String::new()
        } else {
            format!("properties:\n  malt: {{v: {malt}, unit: g}}\n")
        };
        fs::write(
            path.join(format!("{name}.md")),
            format!("---\nschema_version: 1\nname: {name}\nbeer: {beer}\n{malt}---\n"),
        )
        .unwrap();
    }
    path
}

fn profile(fields: &[&str]) -> samplekit::config::project_config::Profile {
    profiles::anonymous(
        fields
            .iter()
            .map(|field| ColumnSpec {
                field: field.to_string(),
                label: None,
                header: None,
                precision: None,
                template: None,
            })
            .collect(),
    )
}

#[test]
fn a_count_carries_its_denominator() {
    let path = scratch("count");
    let collection = list::from_directory(&path).unwrap();
    let (rows, _) =
        summaries::rows(&collection, &profile(&["malt"]), &collection, None, false).unwrap();
    let cells = summaries::written(&rows[0]);
    assert_eq!(cells[0], "2/3");
    assert_eq!(cells[1], "11.0", "{cells:?}");
    assert_eq!(rows[0].unit.as_deref(), Some("g"));
    // A selection holding none of it still says of how many: 0/1, not 0.
    let none = collection.filter_by(|sample| sample.name() == Some("C"));
    let (rows, _) = summaries::rows(&none, &profile(&["malt"]), &collection, None, false).unwrap();
    assert_eq!(summaries::written(&rows[0])[0], "0/1");
    let _ = fs::remove_dir_all(path);
}

#[test]
fn text_is_left_out_and_named() {
    let path = scratch("text");
    let collection = list::from_directory(&path).unwrap();
    let (rows, left_out) = summaries::rows(
        &collection,
        &profile(&["malt", "beer"]),
        &collection,
        None,
        false,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(left_out, ["beer"]);
    let _ = fs::remove_dir_all(path);
}

#[test]
fn the_state_is_left_out_as_text_is() {
    // `state` is words; summarised, it refused the whole summary.
    let path = scratch("state");
    let collection = list::from_directory(&path).unwrap();
    let states = samplekit::collection::validation::states(&collection);
    let stated = collection.with_states(states);
    let (rows, left_out) =
        summaries::rows(&stated, &profile(&["malt", "state"]), &stated, None, false).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(left_out, ["state"]);
    let _ = fs::remove_dir_all(path);
}

#[test]
fn a_number_is_written_to_two_figures_of_its_spread() {
    assert_eq!(summaries::decimals_of(1.414), 1);
    assert_eq!(summaries::decimals_of(0.0123), 3);
    assert_eq!(summaries::decimals_of(250.0), 0);
}

#[test]
fn a_large_mean_is_rounded_to_its_spread_not_written_whole() {
    // `{:.0}` of a mean near 1e20 wrote the float's own digits,
    // 100000000000000131072, as though they were measured.
    assert_eq!(summaries::to_the_deviation(1234.567, 23.0), "1235");
    assert_eq!(summaries::to_the_deviation(1234.567, 230.0), "1230");
    let large = summaries::to_the_deviation(1.000_000_000_000_001_3e20, 2.3e18);
    assert_eq!(large, "1.000e20", "{large}");
    assert!(!large.contains("131072"), "{large}");
}

#[test]
fn a_summary_is_made_per_group_and_each_group_is_headed_by_its_values() {
    // A summary per group, in the groups' order, and a heading that names each
    // field beside its value.
    let path = scratch("grouped");
    let collection = list::from_directory(&path).unwrap();
    let beer = samplekit::query::field_addressing::parse("beer").unwrap();
    let (parts, left_out) = summaries::grouped(
        &collection,
        &profile(&["malt"]),
        &collection,
        None,
        false,
        &[beer],
        samplekit::collection::sample_list::GroupOrder::Values,
    )
    .unwrap();
    assert!(left_out.is_empty());
    let keys: Vec<String> = parts
        .iter()
        .map(|(key, _)| summaries::group_value(&key[0]))
        .collect();
    assert_eq!(keys, ["bock", "schwarz"]);
    assert_eq!(summaries::written(&parts[1].1[0])[0], "1/2");
    let names = ["beer".to_string()];
    assert_eq!(
        summaries::group_heading(&names, &[Value::text("schwarz")]),
        "beer = schwarz"
    );
    assert_eq!(
        summaries::group_heading(&names, &[Value::Absent]),
        "beer = (none)"
    );
    assert_eq!(summaries::group_value(&Value::NotApplicable), "n/a");
}
