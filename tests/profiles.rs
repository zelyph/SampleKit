//! The tests of `profiles`.

use std::fs;
use std::path::PathBuf;

use samplekit::config::project_config::{self as config, ConfigError};
use samplekit::core::formatting::{Precision, Presentation};
use samplekit::core::identifier::Identifier;

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn write(name: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-prof-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".samplekitrc");
    fs::write(&path, body).unwrap();
    (dir, path)
}

fn loaded(name: &str, body: &str) -> config::ProjectConfig {
    let (dir, path) = write(name, body);
    let config = config::load(&path).unwrap();
    let _ = fs::remove_dir_all(&dir);
    config
}

fn refused(name: &str, body: &str) -> ConfigError {
    let (dir, path) = write(name, body);
    let error = config::load(&path).unwrap_err();
    let _ = fs::remove_dir_all(&dir);
    error
}

fn property(precision: Option<&str>) -> Presentation {
    Presentation {
        unit: Some("g/L".to_string()),
        symbol: Some("Bx".to_string()),
        precision: precision.map(|spec| Precision::both(spec).unwrap()),
    }
}

const PLATOS: &str = r#"
schema_version = 1

[profile.platos]
sort = ["beer", "-plato"]
columns = [
  {field = "beer"},
  {field = "malt", label = "Malt"},
  {field = "plato", label = "Plato (g/L)", precision = ".4f"},
  {field = "plato.u", header = "plato_unc"},
]
"#;

#[test]
fn column_order_is_declaration_order() {
    // Not alphabetical, not the order the properties occur in a file.
    let config = loaded("order", PLATOS);
    let profile = config.profile("platos").unwrap();
    let fields: Vec<&str> = profile.columns().iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["beer", "malt", "plato", "plato.u"]);
}

#[test]
fn label_renames_only_the_output() {
    // The identifier still addresses the field.
    let config = loaded("label", PLATOS);
    let profile = config.profile("platos").unwrap();
    assert_eq!(profile.label_for(&profile.columns()[2]), "Plato (g/L)");
    assert_eq!(profile.columns()[2].field, "plato");
}

#[test]
fn missing_label_falls_back_to_the_identifier() {
    // Partial renaming works.
    let config = loaded("fallback", PLATOS);
    let profile = config.profile("platos").unwrap();
    assert_eq!(profile.label_for(&profile.columns()[0]), "beer");
}

#[test]
fn export_header_ignores_the_label() {
    // A column labelled *Plato (g/L)* exports as `plato`, so that a
    // reader who sees a header can round-trip it back to a field.
    let config = loaded("header-ignores", PLATOS);
    let profile = config.profile("platos").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[2], true),
        ["plato_value", "plato_uncertainty"]
    );
}

#[test]
fn declared_header_is_used_only_in_exports() {
    // The terminal still shows the label.
    let config = loaded("header-only-export", PLATOS);
    let profile = config.profile("platos").unwrap();
    let column = &profile.columns()[3];
    // A declared header is the stem even here: on a channel column,
    // `plato_unc` emits `plato_unc_uncertainty`. A header that were the
    // final name here and a stem elsewhere would make a column's name depend
    // on how it was asked for.
    assert_eq!(profile.headers_for(column, true), ["plato_unc_uncertainty"]);
    assert_eq!(profile.label_for(column), "plato.u");
}

#[test]
fn quantity_flattens_into_two_columns() {
    // `malt` yields `malt_value` and `malt_uncertainty` where the format
    // cannot carry a pair.
    let config = loaded("flatten", PLATOS);
    let profile = config.profile("platos").unwrap();
    let malt = &profile.columns()[1];
    assert_eq!(
        profile.headers_for(malt, true),
        ["malt_value", "malt_uncertainty"]
    );
    // And one column where the destination can show a pair.
    assert_eq!(profile.headers_for(malt, false), ["malt"]);
}

#[test]
fn declared_header_is_the_stem_of_both_columns() {
    // `header = "m"` yields `m_value` and `m_uncertainty`.
    let config = loaded(
        "stem",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\", header = \"m\"}]\n",
    );
    let profile = config.profile("p").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[0], true),
        ["m_value", "m_uncertainty"]
    );
}

#[test]
fn channel_column_and_flattened_column_agree() {
    // `malt.v` and the value half of `malt` share one name, so a column's name
    // does not depend on how it was asked for.
    let config = loaded(
        "agree",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt.v\"}]\n",
    );
    let profile = config.profile("p").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[0], true),
        ["malt_value"]
    );
}

#[test]
fn colliding_flattened_headers_are_refused() {
    // `malt` and `malt.u` together produce `malt_uncertainty` twice.
    let error = refused(
        "collide",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\"}, {field = \"malt.u\"}]\n",
    );
    assert!(
        matches!(error, ConfigError::DuplicateHeader { .. }),
        "{error:?}"
    );
}

#[test]
fn column_precision_overrides_property_precision() {
    // The composition order, over the metadata the caller supplies.
    let config = loaded("precision", PLATOS);
    let profile = config.profile("platos").unwrap();
    let composed = profile.presentation_for(&profile.columns()[2], &property(Some(".1f")));
    assert_eq!(composed.precision, Some(Precision::both(".4f").unwrap()));
    // And the property's own survives where the column says nothing.
    let kept = profile.presentation_for(&profile.columns()[1], &property(Some(".1f")));
    assert_eq!(kept.precision, Some(Precision::both(".1f").unwrap()));
}

#[test]
fn new_property_does_not_appear_in_an_existing_profile() {
    // Explicit selection holds.
    let config = loaded("explicit", PLATOS);
    let profile = config.profile("platos").unwrap();
    assert!(profile.covers(&id("plato")));
    assert!(!profile.covers(&id("haze")));
}

#[test]
fn sort_is_held_as_text_until_applied() {
    // Resolved against the collection it runs on: parsing a key is Layer 4's.
    let config = loaded("sort-text", PLATOS);
    let profile = config.profile("platos").unwrap();
    assert_eq!(profile.sort, ["beer", "-plato"]);
}

#[test]
fn several_sort_keys_keep_their_order() {
    // `["beer", "-plato"]` is one spec, not two settings.
    let config = loaded("sort-order", PLATOS);
    assert_eq!(config.profile("platos").unwrap().sort, ["beer", "-plato"]);
}

#[test]
fn a_minus_prefix_reverses_one_key() {
    // And only that key. This layer holds the text; the prefix is read where
    // the spec is parsed, so what is asserted here is that it survives intact.
    let config = loaded("minus", PLATOS);
    let sort = &config.profile("platos").unwrap().sort;
    assert!(!sort[0].starts_with('-'));
    assert!(sort[1].starts_with('-'));
}

// ------------------------------------------------------- a written column

#[test]
fn a_written_column_is_a_field_a_precision_and_a_label() {
    use samplekit::config::profiles;
    use samplekit::format::schema::PrecisionSchema;
    let column = profiles::parse_column("brix:.3f=Plato").unwrap();
    assert_eq!(column.field, "brix");
    assert_eq!(
        column.precision,
        Some(PrecisionSchema::Both(".3f".to_string()))
    );
    assert_eq!(column.label.as_deref(), Some("Plato"));
    let bare = profiles::parse_column("brix").unwrap();
    assert_eq!(bare.field, "brix");
    assert_eq!(bare.precision, None);
    assert_eq!(bare.label, None);
    let _ = id("brix");
}

#[test]
fn a_comma_inside_an_index_does_not_split_a_column_list() {
    use samplekit::config::profiles;
    let columns = profiles::parse_columns("mashing.r[65, 2.0e9],malt").unwrap();
    let fields: Vec<&str> = columns.iter().map(|column| column.field.as_str()).collect();
    assert_eq!(fields, ["mashing.r[65, 2.0e9]", "malt"]);
}

#[test]
fn an_empty_part_of_a_written_column_is_refused() {
    use samplekit::config::profiles::{self, ColumnError};
    assert_eq!(
        profiles::parse_columns("malt,,brix"),
        Err(ColumnError::Empty)
    );
    assert_eq!(profiles::parse_column(":.3f"), Err(ColumnError::Empty));
    assert_eq!(
        profiles::parse_column("malt="),
        Err(ColumnError::EmptyLabel {
            column: "malt=".to_string()
        })
    );
    assert_eq!(
        profiles::parse_column("malt:"),
        Err(ColumnError::EmptyPrecision {
            column: "malt:".to_string()
        })
    );
    assert!(matches!(
        profiles::parse_column("malt:>10"),
        Err(ColumnError::Precision { .. })
    ));
}

#[test]
fn a_grouped_profile_carries_its_groups_as_first_columns() {
    // A file has no place for a heading, so a field grouped by is a column,
    // first where the profile does not show it; one shown keeps its place.
    let profile = samplekit::config::profiles::anonymous(
        samplekit::config::profiles::parse_columns("malt,plato").unwrap(),
    );
    let carrying = profile.carrying(&["status".to_string(), "malt".to_string()]);
    let fields: Vec<&str> = carrying
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["status", "malt", "plato"]);
    // Nothing grouped by, nothing added.
    assert_eq!(profile.carrying(&[]), profile);
}
