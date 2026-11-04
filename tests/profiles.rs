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

const DUCTILITIES: &str = r#"
schema_version = 1

[profile.ductilities]
sort = ["alloy", "-ductility"]
columns = [
  {field = "alloy"},
  {field = "diameter", label = "Diameter"},
  {field = "ductility", label = "Ductility (g/L)", precision = ".4f"},
  {field = "ductility.u", header = "ductility_unc"},
]
"#;

#[test]
fn column_order_is_declaration_order() {
    // Not alphabetical, not the order the properties occur in a file.
    let config = loaded("order", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    let fields: Vec<&str> = profile.columns().iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["alloy", "diameter", "ductility", "ductility.u"]);
}

#[test]
fn label_renames_only_the_output() {
    // The identifier still addresses the field.
    let config = loaded("label", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    assert_eq!(profile.label_for(&profile.columns()[2]), "Ductility (g/L)");
    assert_eq!(profile.columns()[2].field, "ductility");
}

#[test]
fn missing_label_falls_back_to_the_identifier() {
    // Partial renaming works.
    let config = loaded("fallback", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    assert_eq!(profile.label_for(&profile.columns()[0]), "alloy");
}

#[test]
fn export_header_ignores_the_label() {
    // A column labelled *Ductility (g/L)* exports as `ductility`, so that a
    // reader who sees a header can round-trip it back to a field.
    let config = loaded("header-ignores", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[2], true),
        ["ductility_value", "ductility_uncertainty"]
    );
}

#[test]
fn declared_header_is_used_only_in_exports() {
    // The terminal still shows the label.
    let config = loaded("header-only-export", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    let column = &profile.columns()[3];
    // A declared header is the stem even here: on a channel column,
    // `ductility_unc` emits `ductility_unc_uncertainty`. A header that were the
    // final name here and a stem elsewhere would make a column's name depend
    // on how it was asked for.
    assert_eq!(
        profile.headers_for(column, true),
        ["ductility_unc_uncertainty"]
    );
    assert_eq!(profile.label_for(column), "ductility.u");
}

#[test]
fn quantity_flattens_into_two_columns() {
    // `diameter` yields `diameter_value` and `diameter_uncertainty` where the format
    // cannot carry a pair.
    let config = loaded("flatten", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    let diameter = &profile.columns()[1];
    assert_eq!(
        profile.headers_for(diameter, true),
        ["diameter_value", "diameter_uncertainty"]
    );
    // And one column where the destination can show a pair.
    assert_eq!(profile.headers_for(diameter, false), ["diameter"]);
}

#[test]
fn declared_header_is_the_stem_of_both_columns() {
    // `header = "m"` yields `m_value` and `m_uncertainty`.
    let config = loaded(
        "stem",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\", header = \"m\"}]\n",
    );
    let profile = config.profile("p").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[0], true),
        ["m_value", "m_uncertainty"]
    );
}

#[test]
fn channel_column_and_flattened_column_agree() {
    // `diameter.v` and the value half of `diameter` share one name, so a column's name
    // does not depend on how it was asked for.
    let config = loaded(
        "agree",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter.v\"}]\n",
    );
    let profile = config.profile("p").unwrap();
    assert_eq!(
        profile.headers_for(&profile.columns()[0], true),
        ["diameter_value"]
    );
}

#[test]
fn colliding_flattened_headers_are_refused() {
    // `diameter` and `diameter.u` together produce `diameter_uncertainty` twice.
    let error = refused(
        "collide",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\"}, {field = \"diameter.u\"}]\n",
    );
    assert!(
        error
            .to_string()
            .contains("would emit the header 'diameter_uncertainty' twice"),
        "{error}"
    );
}

#[test]
fn column_precision_overrides_property_precision() {
    // The composition order, over the metadata the caller supplies.
    let config = loaded("precision", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    let composed = profile.presentation_for(&profile.columns()[2], &property(Some(".1f")));
    assert_eq!(composed.precision, Some(Precision::both(".4f").unwrap()));
    // And the property's own survives where the column says nothing.
    let kept = profile.presentation_for(&profile.columns()[1], &property(Some(".1f")));
    assert_eq!(kept.precision, Some(Precision::both(".1f").unwrap()));
}

#[test]
fn new_property_does_not_appear_in_an_existing_profile() {
    // Explicit selection holds.
    let config = loaded("explicit", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    assert!(profile.covers(&id("ductility")));
    assert!(!profile.covers(&id("roughness")));
}

#[test]
fn sort_is_held_as_text_until_applied() {
    // Resolved against the collection it runs on: parsing a key is Layer 4's.
    let config = loaded("sort-text", DUCTILITIES);
    let profile = config.profile("ductilities").unwrap();
    assert_eq!(profile.sort, ["alloy", "-ductility"]);
}

#[test]
fn several_sort_keys_keep_their_order() {
    // `["alloy", "-ductility"]` is one spec, not two settings.
    let config = loaded("sort-order", DUCTILITIES);
    assert_eq!(
        config.profile("ductilities").unwrap().sort,
        ["alloy", "-ductility"]
    );
}

#[test]
fn a_minus_prefix_reverses_one_key() {
    // And only that key. This layer holds the text; the prefix is read where
    // the spec is parsed, so what is asserted here is that it survives intact.
    let config = loaded("minus", DUCTILITIES);
    let sort = &config.profile("ductilities").unwrap().sort;
    assert!(!sort[0].starts_with('-'));
    assert!(sort[1].starts_with('-'));
}

// ------------------------------------------------------- a written column

#[test]
fn a_written_column_is_a_field_a_precision_and_a_label() {
    use samplekit::config::profiles;
    use samplekit::format::schema::PrecisionSchema;
    let column = profiles::parse_column("porosity:.3f=Ductility").unwrap();
    assert_eq!(column.field, "porosity");
    assert_eq!(
        column.precision,
        Some(PrecisionSchema::Both(".3f".to_string()))
    );
    assert_eq!(column.label.as_deref(), Some("Ductility"));
    let bare = profiles::parse_column("porosity").unwrap();
    assert_eq!(bare.field, "porosity");
    assert_eq!(bare.precision, None);
    assert_eq!(bare.label, None);
    let _ = id("porosity");
}

#[test]
fn a_comma_inside_an_index_does_not_split_a_column_list() {
    use samplekit::config::profiles;
    let columns = profiles::parse_columns("preheating.r[65, 2.0e9],diameter").unwrap();
    let fields: Vec<&str> = columns.iter().map(|column| column.field.as_str()).collect();
    assert_eq!(fields, ["preheating.r[65, 2.0e9]", "diameter"]);
}

#[test]
fn an_empty_part_of_a_written_column_is_refused() {
    use samplekit::config::profiles::{self, ColumnError};
    assert_eq!(
        profiles::parse_columns("diameter,,porosity"),
        Err(ColumnError::Empty)
    );
    assert_eq!(profiles::parse_column(":.3f"), Err(ColumnError::Empty));
    assert_eq!(
        profiles::parse_column("diameter="),
        Err(ColumnError::EmptyLabel {
            column: "diameter=".to_string()
        })
    );
    assert_eq!(
        profiles::parse_column("diameter:"),
        Err(ColumnError::EmptyPrecision {
            column: "diameter:".to_string()
        })
    );
    assert!(matches!(
        profiles::parse_column("diameter:>10"),
        Err(ColumnError::Precision { .. })
    ));
}

#[test]
fn a_grouped_profile_carries_its_groups_as_first_columns() {
    // A file has no place for a heading, so a field grouped by is a column,
    // first where the profile does not show it; one shown keeps its place.
    let profile = samplekit::config::profiles::anonymous(
        samplekit::config::profiles::parse_columns("diameter,ductility").unwrap(),
    );
    let carrying = profile.carrying(&["status".to_string(), "diameter".to_string()]);
    let fields: Vec<&str> = carrying
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["status", "diameter", "ductility"]);
    // Nothing grouped by, nothing added.
    assert_eq!(profile.carrying(&[]), profile);
}

#[test]
fn a_precision_pair_on_the_command_line_is_said_to_be_a_profiles() {
    // `radius:[.3f,.4f]` was refused as a thousands separator.
    let error = samplekit::config::profiles::parse_column("radius:[.3f,.4f]").unwrap_err();
    let said = error.to_string();
    assert!(
        said.contains("a pair, one for each, is a profile's"),
        "{said}"
    );
    assert!(!said.contains("thousands"), "{said}");
}
