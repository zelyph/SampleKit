//! The tests of `changes`.

use std::fs;
use std::path::PathBuf;

use samplekit::presentation::changes;

fn project(name: &str) -> (PathBuf, samplekit::config::project_config::ProjectConfig) {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-changes-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n[property.malt]\nprecision = \".2f\"\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    (root, config)
}

const BEFORE: &str = "---\nschema_version: 1\nname: C1\ntags: [a]\nbeer: schwarz\n\
                      properties:\n  malt: {v: 12.0, u: 0.1, unit: g}\n---\nA note.\n";

#[test]
fn a_sample_is_compared_field_by_field() {
    let (_, config) = project("fields");
    let after = BEFORE
        .replace("12.0", "12.5")
        .replace("[a]", "[a, b]")
        .replace("schwarz", "bock")
        .replace("A note.", "A note.\nAnother line.");
    let lines = changes::changes_of(
        "c1.md",
        Some(&BEFORE.as_bytes().to_vec()),
        Some(&after.into_bytes()),
        &config,
    );
    assert_eq!(lines[0], "c1.md");
    assert!(
        lines.contains(&"  tags       a  →  a, b".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"  beer       schwarz  →  bock".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"  malt       12.00 ± 0.10 g  →  12.50 ± 0.10 g".to_string()),
        "{lines:?}"
    );
    assert_eq!(lines.last().unwrap(), "  the note   changed, +1 −0 lines");
}

#[test]
fn a_file_is_new_removed_or_changed_by_its_lines() {
    let (_, config) = project("files");
    let bytes = BEFORE.as_bytes().to_vec();
    assert_eq!(
        changes::changes_of("c1.md", None, Some(&bytes), &config),
        ["c1.md   new sample"]
    );
    assert_eq!(
        changes::changes_of("c1.md", Some(&bytes), None, &config),
        ["c1.md   removed"]
    );
    let rc = b"a = 1\nb = 2\n".to_vec();
    let rc2 = b"a = 1\nb = 3\nc = 4\n".to_vec();
    assert_eq!(
        changes::changes_of(".samplekitrc", Some(&rc), Some(&rc2), &config),
        [".samplekitrc   changed, +2 −1 lines"]
    );
    // Only its records: no value changed.
    let recorded = BEFORE.replace("unit: g}", "unit: g, fingerprint: 0123456789ab}");
    let lines = changes::changes_of("c1.md", Some(&bytes), Some(&recorded.into_bytes()), &config);
    assert_eq!(
        lines[1],
        "  changed only what SampleKit records about its values"
    );
    assert_eq!(
        changes::changes_of("c1.md", Some(&bytes), Some(&bytes), &config),
        Vec::<String>::new()
    );
}

#[test]
fn a_snapshot_is_said_briefly() {
    let names: Vec<String> = [
        "a.md",
        "b.md",
        "c.md",
        "d.md",
        ".samplekitrc",
        "model/m.py",
        "model/h.py",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    let all: Vec<&String> = names.iter().collect();
    assert_eq!(
        changes::changed_said(&all),
        "4 samples, .samplekitrc, the model (2 files)"
    );
    assert_eq!(changes::changed_said(&all[..2]), "a, b");
}

#[test]
fn a_value_shown_the_same_on_both_sides_is_no_change() {
    // A property held without a value on one side and absent from the other
    // is `—` on both: said `fg   —  →  —`, it was no change at all.
    let (_, config) = project("same");
    let before = BEFORE.as_bytes().to_vec();
    let after = BEFORE
        .replace("unit: g}\n", "unit: g}\n  fg: {unit: g}\n")
        .into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&after), &config);
    assert!(!lines.iter().any(|line| line.contains("fg")), "{lines:?}");
    assert_eq!(
        lines,
        ["c1.md".to_string(), format!("  {}", changes::RECORDS_ONLY)]
    );
}

#[test]
fn a_cells_readings_are_compared() {
    // A reading of a cell corrected was said *changed only what SampleKit
    // records about its values*.
    let (_, config) = project("cell-readings");
    let before = "---\nschema_version: 1\nname: C1\ntables:\n  lagering:\n    index: t\n    \
                  columns:\n      t: {}\n      brix: {}\n    rows:\n      - t: 1\n        \
                  brix: {readings: [1.0, 2.0]}\n---\n";
    let after = before.replace("[1.0, 2.0]", "[1.0, 3.0]");
    let lines = changes::changes_of(
        "c1.md",
        Some(&before.as_bytes().to_vec()),
        Some(&after.into_bytes()),
        &config,
    );
    assert!(
        lines.iter().any(
            |line| line.contains("lagering.brix[1].readings") && line.contains("1, 2  →  1, 3")
        ),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains(changes::RECORDS_ONLY)),
        "{lines:?}"
    );
}

#[test]
fn a_value_is_said_at_its_precision_and_whole_without_one() {
    // At the precision the project declares, as a table shows it; a value that
    // moved below it said whole, never *changed only what SampleKit records*;
    // and every digit where none is declared.
    let (_, config) = project("digits");
    let before = BEFORE.as_bytes().to_vec();
    let moved = BEFORE.replace("12.0", "12.5").into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&moved), &config);
    assert!(
        lines.contains(&"  malt   12.00 ± 0.10 g  →  12.50 ± 0.10 g".to_string()),
        "{lines:?}"
    );
    let hidden = BEFORE.replace("12.0", "12.001234").into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&hidden), &config);
    assert!(
        lines.contains(&"  malt   12 ± 0.1 g  →  12.001234 ± 0.1 g".to_string()),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains(changes::RECORDS_ONLY)),
        "{lines:?}"
    );
    let undeclared = BEFORE.replace("malt", "volume");
    let computed = undeclared.replace("12.0", "6.081250000000001").into_bytes();
    let lines = changes::changes_of(
        "c1.md",
        Some(&undeclared.into_bytes()),
        Some(&computed),
        &config,
    );
    assert!(
        lines.contains(&"  volume   12 ± 0.1 g  →  6.081250000000001 ± 0.1 g".to_string()),
        "{lines:?}"
    );
}

#[test]
fn a_name_written_is_a_field_compared() {
    // The name is edited as any attribute, and compared as one.
    let (_, config) = project("name");
    let renamed = BEFORE.replace("name: C1", "name: Cask C1").into_bytes();
    let lines = changes::changes_of(
        "c1.md",
        Some(&BEFORE.as_bytes().to_vec()),
        Some(&renamed),
        &config,
    );
    assert!(
        lines.contains(&"  name   C1  →  Cask C1".to_string()),
        "{lines:?}"
    );
}

#[test]
fn among_says_only_the_fields_asked_about() {
    // What an export writes, and what that is computed from, and nothing else
    // of its samples.
    let (_, config) = project("among");
    let before = BEFORE.as_bytes().to_vec();
    let after = BEFORE
        .replace("12.0", "12.5")
        .replace("schwarz", "bock")
        .into_bytes();
    let malt = changes::changes_among("c1.md", Some(&before), Some(&after), &config, &|field| {
        field == "malt"
    });
    assert!(malt.iter().any(|line| line.contains("malt")), "{malt:?}");
    assert!(!malt.iter().any(|line| line.contains("beer")), "{malt:?}");
    let brix = changes::changes_among("c1.md", Some(&before), Some(&after), &config, &|field| {
        field == "brix"
    });
    assert!(brix.is_empty(), "{brix:?}");
    assert_eq!(
        changes::field_base("fermentation.gravity[3]"),
        "fermentation"
    );
    assert_eq!(changes::field_base("-og.u"), "og");
    let computed = "---\nschema_version: 1\nname: C1\nproperties:\n  volume: {v: 4.0}\n  \
                    brix: {v: 3.0, computed: {volume: 000000000000}}\n---\n";
    assert_eq!(
        changes::computed_from(computed.as_bytes()),
        vec![("brix".to_string(), "volume".to_string())]
    );
}
