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
        "schema_version = 1\n[property.diameter]\nprecision = \".2f\"\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    (root, config)
}

const BEFORE: &str = "---\nschema_version: 1\nname: C1\ntags: [a]\nalloy: cobalt\n\
                      properties:\n  diameter: {v: 12.0, u: 0.1, unit: g}\n---\nA note.\n";

#[test]
fn a_sample_is_compared_field_by_field() {
    let (_, config) = project("fields");
    let after = BEFORE
        .replace("12.0", "12.5")
        .replace("[a]", "[a, b]")
        .replace("cobalt", "chrome")
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
        lines.contains(&"  alloy      cobalt  →  chrome".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"  diameter   12.00 ± 0.10 g  →  12.50 ± 0.10 g".to_string()),
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
    // is `—` on both: said `notch   —  →  —`, it was no change at all.
    let (_, config) = project("same");
    let before = BEFORE.as_bytes().to_vec();
    let after = BEFORE
        .replace("unit: g}\n", "unit: g}\n  notch: {unit: g}\n")
        .into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&after), &config);
    assert!(
        !lines.iter().any(|line| line.contains("notch")),
        "{lines:?}"
    );
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
    let before = "---\nschema_version: 1\nname: C1\ntables:\n  annealing:\n    index: t\n    \
                  columns:\n      t: {}\n      porosity: {}\n    rows:\n      - t: 1\n        \
                  porosity: {readings: [1.0, 2.0]}\n---\n";
    let after = before.replace("[1.0, 2.0]", "[1.0, 3.0]");
    let lines = changes::changes_of(
        "c1.md",
        Some(&before.as_bytes().to_vec()),
        Some(&after.into_bytes()),
        &config,
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("annealing.porosity[1].readings")
                && line.contains("1, 2  →  1, 3")),
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
fn a_value_is_said_at_its_precision_or_as_a_table_shows_it() {
    // At the precision the project declares, as a table shows it; a value that
    // moved below it said whole, never *changed only what SampleKit records*;
    // and as a table shows it where none is declared, where every digit said
    // the noise a computation leaves.
    let (_, config) = project("digits");
    let before = BEFORE.as_bytes().to_vec();
    let moved = BEFORE.replace("12.0", "12.5").into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&moved), &config);
    assert!(
        lines.contains(&"  diameter   12.00 ± 0.10 g  →  12.50 ± 0.10 g".to_string()),
        "{lines:?}"
    );
    let hidden = BEFORE.replace("12.0", "12.001234").into_bytes();
    let lines = changes::changes_of("c1.md", Some(&before), Some(&hidden), &config);
    assert!(
        lines.contains(&"  diameter   12.000 ± 0.1 g  →  12.001 ± 0.1 g".to_string()),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains(changes::RECORDS_ONLY)),
        "{lines:?}"
    );
    let undeclared = BEFORE.replace("diameter", "volume");
    let computed = undeclared.replace("12.0", "6.081250000000001").into_bytes();
    let lines = changes::changes_of(
        "c1.md",
        Some(&undeclared.into_bytes()),
        Some(&computed),
        &config,
    );
    assert!(
        lines.contains(&"  volume   12.00 ± 0.10 g  →  6.08 ± 0.10 g".to_string()),
        "{lines:?}"
    );
}

#[test]
fn a_name_written_is_a_field_compared() {
    // The name is edited as any attribute, and compared as one.
    let (_, config) = project("name");
    let renamed = BEFORE.replace("name: C1", "name: Slab C1").into_bytes();
    let lines = changes::changes_of(
        "c1.md",
        Some(&BEFORE.as_bytes().to_vec()),
        Some(&renamed),
        &config,
    );
    assert!(
        lines.contains(&"  name   C1  →  Slab C1".to_string()),
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
        .replace("cobalt", "chrome")
        .into_bytes();
    let diameter =
        changes::changes_among("c1.md", Some(&before), Some(&after), &config, &|field| {
            field == "diameter"
        });
    assert!(
        diameter.iter().any(|line| line.contains("diameter")),
        "{diameter:?}"
    );
    assert!(
        !diameter.iter().any(|line| line.contains("alloy")),
        "{diameter:?}"
    );
    let porosity =
        changes::changes_among("c1.md", Some(&before), Some(&after), &config, &|field| {
            field == "porosity"
        });
    assert!(porosity.is_empty(), "{porosity:?}");
    assert_eq!(changes::field_base("ageing.hardness[3]"), "ageing");
    assert_eq!(changes::field_base("-radius.u"), "radius");
    let computed = "---\nschema_version: 1\nname: C1\nproperties:\n  volume: {v: 4.0}\n  \
                    porosity: {v: 3.0, computed: {volume: '000000000000'}}\n---\n";
    assert_eq!(
        changes::computed_from(computed.as_bytes()),
        vec![("porosity".to_string(), "volume".to_string())]
    );
}

#[test]
fn a_join_says_apart_what_came_with_it() {
    let (root, config) = project("join");
    for name in ["a", "b"] {
        fs::write(
            root.join(format!("{name}.md")),
            format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
        )
        .unwrap();
    }
    let set = |machine: &str, change: &str| {
        let (file, assignment) = change.split_once(' ').unwrap();
        let arguments = vec![file, "set", assignment, "--write"];
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(&arguments)
            .current_dir(&root)
            .env("SAMPLEKIT_MACHINE", machine)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    set("ana", "a.md diameter=1.5");
    set("tom", "b.md diameter=2.5");
    set("ana", "a.md diameter=1.7");
    let entries = samplekit::config::version_control::entries(&config).unwrap();
    let join = changes::of_join(&config, &entries, 0, &|_| true)
        .unwrap()
        .expect("the newest is a join");
    let own: Vec<&String> = join.own.iter().flat_map(|(_, lines)| lines).collect();
    let came: Vec<&String> = join.came.iter().flat_map(|(_, lines)| lines).collect();
    assert!(
        own.iter()
            .any(|line| line.starts_with("a.md") && line.contains("1.70")),
        "{own:?}"
    );
    assert!(
        came.iter()
            .any(|line| line.starts_with("b.md") && line.contains("2.50")),
        "{came:?}"
    );
    assert_eq!(join.from, ["#2 of tom"]);
    assert!(
        changes::of_join(&config, &entries, 1, &|_| true)
            .unwrap()
            .is_none()
    );
    // Tom's value set back by ana: no change of hers, nothing that stayed.
    set("tom", "a.md diameter=3.5");
    set("ana", "a.md diameter=1.7");
    let entries = samplekit::config::version_control::entries(&config).unwrap();
    let join = changes::of_join(&config, &entries, 0, &|_| true)
        .unwrap()
        .expect("a join");
    assert!(
        join.own.is_empty() && join.came.is_empty(),
        "{:?} {:?}",
        join.own,
        join.came
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_change_and_what_it_makes_not_current_are_said_one_way() {
    // Each surface padded its own way: `set` to 28 columns, the workbench not
    // at all. One block, padded to its widest; notes beneath the values.
    use samplekit::presentation::changes::{FieldChange, change_lines, not_current_lines};
    let mut radius = FieldChange::new("radius", "1.043 ± 0.0003", "1.050 ± 0.0003");
    radius
        .notes
        .push("typed over its formula, which stays".to_string());
    let lines = change_lines(&[
        radius,
        FieldChange::new("ageing.hardness[1]", "1.029", "1.020"),
        FieldChange::said("tags", "+prize"),
    ]);
    assert_eq!(
        lines,
        [
            "  radius               1.043 ± 0.0003  →  1.050 ± 0.0003",
            "                       typed over its formula, which stays",
            "  ageing.hardness[1]   1.029  →  1.020",
            "  tags                 +prize",
        ]
    );
    let block = not_current_lines(
        &[
            ("modulus".to_string(), "outdated — radius".to_string()),
            (
                "ageing.nominal".to_string(),
                "outdated — row.hardness (8 rows)".to_string(),
            ),
        ],
        0,
    );
    // Counted by cells: the column's eight rows, and modulus.
    assert_eq!(
        block,
        [
            "  this makes 9 values not current",
            "    modulus          outdated — radius",
            "    ageing.nominal   outdated — ageing.hardness (8 rows)",
        ]
    );
    assert_eq!(not_current_lines(&[], 0), ["  nothing becomes not current"]);
    assert_eq!(
        not_current_lines(&[], 3),
        ["  nothing more becomes not current — 3 values were already"]
    );
}

#[test]
fn two_values_are_told_apart_at_their_difference() {
    // 207.48 → 207.53, rounded at the value's own two significant digits,
    // was said 207 → 208: a jump of one where the value moved by 0.05.
    let (was, is) = changes::apart("207.48 ± 1.0658 GPa", "207.53 ± 1.0661 GPa");
    assert_eq!(was, "207.48 ± 1.0658 GPa");
    assert_eq!(is, "207.53 ± 1.0661 GPa");
    // Shown 207 → 208, they are said whole: the rounding made a jump of one
    // out of 0.05. A move as large as what is shown is shown as it is.
    assert!(changes::exaggerated(
        "207 ± 1.1 GPa",
        "208 ± 1.1 GPa",
        "207.48 ± 1.0658 GPa",
        "207.53 ± 1.0661 GPa"
    ));
    assert!(changes::exaggerated(
        "2.07e+03", "2.08e+03", "2074.8", "2075.3"
    ));
    // An uncertainty taken again from the same spread differs past twelve
    // significant digits: the same number, not seventeen decimals of it, and
    // rounded up, as an uncertainty is.
    let (was, is) = changes::apart(
        "1.0117 ± 0.00033333333333337",
        "1.0123 ± 0.00033333333333330",
    );
    assert_eq!(
        (was.as_str(), is.as_str()),
        ("1.0117 ± 0.00034", "1.0123 ± 0.00034")
    );
    // A preview's uncertainty is rounded up too: 0.0012345612 is 0.0013.
    let (was, _) = changes::apart("5 ± 0.0010", "5 ± 0.0012345612");
    assert_eq!(was, "5 ± 0.0010");
    let (_, is) = changes::apart("5 ± 0.0010", "5 ± 0.0012345612");
    assert_eq!(is, "5 ± 0.0013");
    assert!(!changes::exaggerated(
        "207 ± 1.1 GPa",
        "209 ± 1.1 GPa",
        "207.4 ± 1.1 GPa",
        "208.6 ± 1.1 GPa"
    ));
    // 6.83 → 6.74 shown 6.8 → 6.7: a step for nearly a step, said as shown.
    assert!(!changes::exaggerated(
        "6.8 ± 0.1 %",
        "6.7 ± 0.1 %",
        "6.83 ± 0.15 %",
        "6.74 ± 0.15 %"
    ));
}

#[test]
fn a_row_input_is_said_by_its_column_in_its_file() {
    // `restore` names each value in its file: the row's input was said
    // `samples/b1.load`, its file's name taken for its table's.
    let lines = changes::not_current_lines(
        &[(
            "samples/b1.md  tests.ratio".to_string(),
            "outdated — row.stress".to_string(),
        )],
        0,
    );
    assert!(lines[1].ends_with("outdated — tests.stress"), "{lines:?}");
}
