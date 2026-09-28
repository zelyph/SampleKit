//! The tests of `exports`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::exports::{self, ExportError};
use samplekit::collection::sample_list as list;
use samplekit::config::project_config::{self as project_config, Format};
use samplekit::presentation::export_formats as formats;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-exp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    /// A sample with a malt (value and uncertainty) and an optional plato.
    fn sample(&self, file: &str, name: &str, malt: f64, plato: Option<f64>) -> PathBuf {
        let mut body = format!("---\nschema_version: 1\nname: {name}\n");
        body.push_str(&format!(
            "properties:\n  malt: {{v: {malt}, u: 0.05, unit: g}}\n"
        ));
        match plato {
            Some(plato) => body.push_str(&format!("  plato: {{v: {plato}}}\n")),
            None => body.push_str("  plato: {}\n"),
        }
        body.push_str("---\nnote\n");
        let path = self.0.join(file);
        fs::write(&path, body).unwrap();
        path
    }

    fn config(&self, body: &str) -> project_config::ProjectConfig {
        let path = self.0.join(".samplekitrc");
        fs::write(&path, body).unwrap();
        project_config::load(&path).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CONFIG: &str = r#"
schema_version = 1

[profile.transport]
columns = [
  {field = "name"},
  {field = "malt", label = "Malt (g)"},
  {field = "plato.v", header = "brix"},
]

[export.transport]
profile = "transport"
format = "csv"
output = "out/transport.csv"

[export.located]
profile = "transport"
format = "csv"
output = "out/located.csv"
filename = true
path = true
"#;

fn project(name: &str) -> (Scratch, project_config::ProjectConfig, list::SampleList) {
    let scratch = Scratch::new(name);
    scratch.sample("a.md", "A", 12.5, Some(2.8));
    scratch.sample("b.md", "B", 9.0, None);
    let config = scratch.config(CONFIG);
    let collection = list::from_directory(&scratch.0).unwrap();
    (scratch, config, collection)
}

fn csv(name: &str) -> (Scratch, String) {
    let (scratch, config, collection) = project(name);
    let target = config.export("transport").unwrap();
    let profile = config.profile("transport").unwrap();
    let dataset = exports::run(target, profile, &collection).unwrap();
    (scratch, formats::serialize(&dataset, Format::Csv).unwrap())
}

// ------------------------------------------------------------------ shape

#[test]
fn row_order_matches_the_profile() {
    let (_scratch, out) = csv("order");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[1].starts_with("A,"), "{out}");
    assert!(lines[2].starts_with("B,"), "{out}");
}

#[test]
fn headers_rename_only_the_output() {
    // A label is the terminal's; the file takes the field or the header.
    let (_scratch, out) = csv("headers");
    // `brix_value` and not `brix`: a declared header is the stem, always, which
    // is what keeps a column's name independent of how it was asked for.
    assert_eq!(
        out.lines().next().unwrap(),
        "name,malt_value [g],malt_uncertainty [g],brix_value"
    );
    assert!(!out.contains("Malt (g)"), "{out}");
}

/// The screen showed `12.22` and the file held `12.219999999999999`: the rule
/// was written in this note, in `export-formats`'s, and in the user guide, and
/// implemented in none of them, because the serializer was told not to round
/// and the caller never asked.
#[test]
fn a_declared_precision_reaches_the_file() {
    let scratch = Scratch::new("precision");
    // A value carrying a float's full expansion, which is what a formula
    // leaves behind.
    fs::write(
        scratch.0.join("a.md"),
        "---\nschema_version: 1\nname: A\nproperties:\n  \
         malt: {v: 12.219999999999999, u: 0.05, unit: g}\n  \
         plato: {v: 0.30000000000000004}\n---\n",
    )
    .unwrap();
    let config = scratch.config(
        "schema_version = 1\n\n\
         [property.malt]\nunit = \"g\"\nprecision = \".2f\"\n\n\
         [profile.transport]\ncolumns = [\n  \
         {field = \"name\"},\n  \
         {field = \"malt\"},\n  \
         {field = \"plato.v\", precision = \".1f\"},\n]\n\n\
         [export.transport]\nprofile = \"transport\"\nformat = \"csv\"\noutput = \"out/t.csv\"\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let target = config.export("transport").unwrap();
    let profile = config.profile("transport").unwrap();
    let dataset =
        exports::run_within(target, profile, &collection, Some(&config), &collection).unwrap();
    let out = formats::serialize(&dataset, Format::Csv).unwrap();
    // The property's own `.2f`, and the column's `.1f` where it declares one.
    assert_eq!(out.lines().nth(1).unwrap(), "A,12.22,0.05,0.3", "{out}");
    // JSON keeps them numbers, at the same precision.
    let json = formats::serialize(&dataset, Format::Json).unwrap();
    assert!(json.contains("12.22"), "{json}");
    assert!(!json.contains("12.2199"), "{json}");
    // Without a declared precision, nothing is touched: the file keeps what a
    // formula produced, which is what `numbers-round-trip-exactly` rests on.
    let bare = scratch.config(
        "schema_version = 1\n\n\
         [profile.transport]\ncolumns = [{field = \"name\"}, {field = \"malt\"}]\n\n\
         [export.transport]\nprofile = \"transport\"\nformat = \"csv\"\noutput = \"out/t.csv\"\n",
    );
    let untouched = exports::run_within(
        bare.export("transport").unwrap(),
        bare.profile("transport").unwrap(),
        &collection,
        Some(&bare),
        &collection,
    )
    .unwrap();
    let out = formats::serialize(&untouched, Format::Csv).unwrap();
    assert!(out.contains("12.219999999999999"), "{out}");
}

#[test]
fn a_quantity_exports_both_of_its_numbers() {
    let (_scratch, out) = csv("quantity");
    assert_eq!(out.lines().nth(1).unwrap(), "A,12.5,0.05,2.8");
}

#[test]
fn a_quantity_is_one_whatever_the_selection_holds() {
    // An export has the same columns whatever the selection, so whether a column is a quantity is
    // asked of the collection — never of the selection's first sample.
    let scratch = Scratch::new("quantity-shape");
    scratch.sample("a.md", "A", 12.5, Some(2.8));
    fs::write(
        scratch.0.join("b.md"),
        "---\nschema_version: 1\nname: B\nmaltster: kim\n---\nnote\n",
    )
    .unwrap();
    let config = scratch.config(
        "schema_version = 1\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"maltster\"}, {field = \"malt\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"o.csv\"\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let expected = ["name", "maltster", "malt_value [g]", "malt_uncertainty [g]"];
    let headers = |selection: &list::SampleList| {
        let dataset = exports::run_within(
            config.export("e").unwrap(),
            config.profile("p").unwrap(),
            selection,
            Some(&config),
            &collection,
        )
        .unwrap();
        formats::serialize(&dataset, Format::Csv)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    let everything = headers(&collection);
    assert_eq!(everything, expected.join(","));
    // No sample at all; then only B, which holds neither quantity and is the
    // one sample holding `maltster`; then only A, which lacks `maltster`.
    for kept in ["NOBODY", "B", "A"] {
        let selection = collection.filter_by(|sample| sample.name() == Some(kept));
        assert_eq!(headers(&selection), everything, "selection {kept}");
    }
}

#[test]
fn a_channel_exports_one_column() {
    let scratch = Scratch::new("channel");
    scratch.sample("a.md", "A", 12.5, Some(2.8));
    let config = scratch.config(
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt.v\"}, {field = \"malt.u\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"o.csv\"\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let dataset = exports::run(
        config.export("e").unwrap(),
        config.profile("p").unwrap(),
        &collection,
    )
    .unwrap();
    assert_eq!(dataset.headers, ["malt_value", "malt_uncertainty"]);
    assert_eq!(dataset.rows[0].len(), 2);
}

#[test]
fn a_missing_uncertainty_leaves_an_empty_field() {
    // The column count does not vary: a file whose width changes by row is
    // malformed, and the empty field is what the absence looks like.
    let (_scratch, out) = csv("empty-field");
    let row = out.lines().nth(2).unwrap();
    // `9` and not `9.0`: a CSV column has no per-cell type, so forcing the
    // `.0` the file needs would add a digit carrying no information here.
    assert_eq!(row, "B,9,0.05,");
    assert_eq!(row.split(',').count(), 4);
}

#[test]
fn absent_values_are_emitted_consistently() {
    // Empty in CSV and TSV, `null` in JSON, from the same dataset.
    let (_scratch, config, collection) = project("absent");
    let target = config.export("transport").unwrap();
    let profile = config.profile("transport").unwrap();
    let dataset = exports::run(target, profile, &collection).unwrap();
    assert!(
        formats::serialize(&dataset, Format::Csv)
            .unwrap()
            .contains("B,9,0.05,\n")
    );
    assert!(
        formats::serialize(&dataset, Format::Json)
            .unwrap()
            .contains("\"brix_value\": null")
    );
}

#[test]
fn filename_and_path_columns_are_optional() {
    let (_scratch, config, collection) = project("located");
    let profile = config.profile("transport").unwrap();

    let plain = exports::run(config.export("transport").unwrap(), profile, &collection).unwrap();
    assert_eq!(plain.headers[0], "name");

    let located = exports::run(config.export("located").unwrap(), profile, &collection).unwrap();
    assert_eq!(located.headers[0], "filename");
    assert_eq!(located.headers[1], "path");
    let out = formats::serialize(&located, Format::Csv).unwrap();
    assert!(out.lines().nth(1).unwrap().starts_with("a.md,"), "{out}");
}

#[test]
fn the_same_profile_prints_and_exports_alike() {
    // One declaration, two destinations: the values are the same and only the
    // shape of a quantity differs — one cell at a terminal, two in a file.
    let (_scratch, config, collection) = project("alike");
    let profile = config.profile("transport").unwrap();
    let dataset = exports::run(config.export("transport").unwrap(), profile, &collection).unwrap();
    let terminal: Vec<String> = profile
        .columns()
        .iter()
        .map(|column| profile.label_for(column).to_string())
        .collect();
    assert_eq!(terminal, ["name", "Malt (g)", "plato.v"]);
    // Three columns on screen, four in the file, same three quantities.
    assert_eq!(dataset.headers.len(), 4);
}

#[test]
fn a_templated_column_reaches_the_file() {
    // A template is carried through as a declaration: rendering it is the
    // renderer's, and what this module owes is not to lose it.
    let scratch = Scratch::new("template");
    scratch.sample("a.md", "A", 12.5, Some(2.8));
    let config = scratch.config(
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\", template = \"x\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"o.csv\"\n",
    );
    let profile = config.profile("p").unwrap();
    assert_eq!(profile.columns()[0].template.as_deref(), Some("x"));
    let collection = list::from_directory(&scratch.0).unwrap();
    let dataset = exports::run(config.export("e").unwrap(), profile, &collection).unwrap();
    assert_eq!(dataset.headers, ["malt_value", "malt_uncertainty"]);
}

// -------------------------------------------------------------- stability

#[test]
fn regeneration_is_byte_identical() {
    // Same data, same profile, same bytes — otherwise every regeneration
    // produces a diff and the real changes are invisible among them.
    let (scratch, first) = csv("regen");
    let config = project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    for _ in 0..4 {
        let collection = list::from_directory(&scratch.0).unwrap();
        let dataset = exports::run(
            config.export("transport").unwrap(),
            config.profile("transport").unwrap(),
            &collection,
        )
        .unwrap();
        assert_eq!(formats::serialize(&dataset, Format::Csv).unwrap(), first);
    }
}

#[test]
fn unknown_profile_suggests_nearest() {
    // Caught at configuration load, which is the point: an export naming a
    // profile that does not exist fails when the project is opened.
    let scratch = Scratch::new("unknown-profile");
    let path = scratch.0.join(".samplekitrc");
    fs::write(
        &path,
        "schema_version = 1\n[profile.transport]\ncolumns = [{field = \"malt\"}]\n\
         [export.e]\nprofile = \"transprot\"\nformat = \"csv\"\noutput = \"o.csv\"\n",
    )
    .unwrap();
    let error = project_config::load(&path).unwrap_err();
    assert!(error.to_string().contains("transport"), "{error}");
}

// ----------------------------------------------------------------- writing

#[test]
fn existing_destination_is_not_overwritten() {
    let scratch = Scratch::new("exists");
    let path = scratch.0.join("out.csv");
    fs::write(&path, "in the way").unwrap();
    let error = exports::write("new", &path, false).unwrap_err();
    assert!(
        matches!(error, ExportError::AlreadyExists { .. }),
        "{error:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "in the way");
}

#[test]
fn overwrite_replaces_the_file() {
    let scratch = Scratch::new("overwrite");
    let path = scratch.0.join("out.csv");
    fs::write(&path, "old").unwrap();
    exports::write("new", &path, true).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
}

#[test]
fn an_export_never_replaces_a_sample() {
    // A destination mistyped into the collection replaced a sample's
    // frontmatter, tags and note with a table, exit 0 — even `--write` is no
    // answer to that: an export is made from the data, never over it.
    let scratch = Scratch::new("over-a-sample");
    let path = scratch.sample("BR-01.md", "BR-01", 12.487, None);
    let before = fs::read_to_string(&path).unwrap();
    let error = exports::write("malt\n12.487\n", &path, true).unwrap_err();
    assert!(matches!(error, ExportError::IsASample { .. }), "{error:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    // And a preview knows it before `--write` does.
    assert!(matches!(
        exports::refusal(&path),
        Some(ExportError::IsASample { .. })
    ));
}

#[test]
fn an_export_never_replaces_a_file_opening_with_frontmatter() {
    // A sample of attributes only, or one whose YAML does not parse today,
    // is still data: the guard asked for a name, properties or tables.
    let scratch = Scratch::new("over-frontmatter");
    for (name, text) in [
        (
            "attributes.md",
            "---\nschema_version: 1\nbatch: 3\n---\n\nKept.\n",
        ),
        ("broken.md", "---\nname: [unclosed\n---\n\nKept.\n"),
        ("bom.md", "\u{feff}---\r\nbatch: 3\r\n---\r\n"),
    ] {
        let path = scratch.0.join(name);
        fs::write(&path, text).unwrap();
        let error = exports::write("malt\n12.487\n", &path, true).unwrap_err();
        assert!(matches!(error, ExportError::IsASample { .. }), "{error:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
    // A table the export wrote before is replaced.
    let table = scratch.0.join("table.md");
    fs::write(&table, "| malt |\n| --- |\n").unwrap();
    exports::write("| malt |\n| --- |\n| 1 |\n", &table, true).unwrap();
}

#[test]
#[cfg(unix)]
fn an_export_never_replaces_a_read_only_file() {
    // The atomic write renames over its destination, and a rename asks only
    // the directory: the file's own protection was never consulted.
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("read-only");
    let path = scratch.0.join("published.csv");
    fs::write(&path, "kept").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let error = exports::write("new", &path, true).unwrap_err();
    assert!(matches!(error, ExportError::ReadOnly { .. }), "{error:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "kept");
}

#[test]
#[cfg(unix)]
fn an_export_writes_through_a_link() {
    // `latest.csv -> runs/today.csv`: the link survives and its target is
    // what changes. Writing to the link's own path replaced it with a file.
    let scratch = Scratch::new("through-a-link");
    fs::create_dir_all(scratch.0.join("runs")).unwrap();
    let target = scratch.0.join("runs/today.csv");
    fs::write(&target, "old").unwrap();
    let link = scratch.0.join("latest.csv");
    std::os::unix::fs::symlink("runs/today.csv", &link).unwrap();
    exports::write("new", &link, true).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");

    // A link to nothing has nothing to write through, and says what it names.
    let dangling = scratch.0.join("gone.csv");
    std::os::unix::fs::symlink("runs/missing.csv", &dangling).unwrap();
    let error = exports::write("new", &dangling, true).unwrap_err();
    assert!(
        matches!(error, ExportError::DanglingLink { .. }),
        "{error:?}"
    );
}

#[test]
fn write_is_atomic() {
    // Write-then-rename, as in `document`: a killed process leaves the old
    // file rather than a truncated one, and no temporary survives a success.
    let scratch = Scratch::new("atomic");
    let path = scratch.0.join("nested/out.csv");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    exports::write("content", &path, false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "content");
    let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_missing_directory_is_made() {
    // Whatever named the destination, its folder is made, as a declared
    // export's always was.
    let scratch = Scratch::new("missing-directory");
    let path = scratch.0.join("typo/out.csv");
    exports::write("content", &path, false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "content");
    let deeper = scratch.0.join("again/deeper/out.csv");
    exports::write_replacing("replaced", &deeper, None).unwrap();
    assert_eq!(fs::read_to_string(&deeper).unwrap(), "replaced");
}

#[test]
fn an_export_records_each_column_unit_and_quantity() {
    let (_scratch, config, collection) = project("units");
    let dataset = exports::run(
        config.export("transport").unwrap(),
        config.profile("transport").unwrap(),
        &collection,
    )
    .unwrap();
    assert_eq!(
        dataset.units,
        [None, Some("g".to_string()), Some("g".to_string()), None]
    );
    assert_eq!(dataset.quantities.len(), 1);
    assert_eq!(dataset.quantities[0].key, "malt");
    assert_eq!(dataset.quantities[0].value, 1);
}

#[test]
fn an_export_is_not_written_over_another_writers_change() {
    // The destination as the command read it, or nothing.
    let scratch = Scratch::new("export-changed-since");
    let path = scratch.0.join("out.csv");
    fs::write(&path, "theirs\n").unwrap();
    let refused = exports::write_replacing("mine\n", &path, Some(b"before\n"));
    assert!(
        matches!(refused, Err(ExportError::ChangedSince { .. })),
        "{refused:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
    let held = exports::held_at(&path);
    exports::write_replacing("mine\n", &path, held.as_deref()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine\n");
}

#[test]
fn a_profile_that_groups_writes_each_groups_rows_together() {
    // One table carrying its groups — the field grouped by first, each group's
    // rows together, in the values' order or, sorted, in the sort's.
    let scratch = Scratch::new("grouped");
    for (file, name, status, malt) in [
        ("a.md", "A", "approved", 12.5),
        ("b.md", "B", "rejected", 9.0),
        ("c.md", "C", "approved", 10.0),
    ] {
        fs::write(
            scratch.0.join(file),
            format!(
                "---\nschema_version: 1\nname: {name}\nstatus: {status}\n\
                 properties:\n  malt: {{v: {malt}, unit: g}}\n---\n"
            ),
        )
        .unwrap();
    }
    let config = scratch.config(
        "schema_version = 1\n\
         [profile.by_status]\ncolumns = [{field = \"name\"}, {field = \"malt\"}]\n\
         group = [\"status\"]\n\
         [profile.flat]\ncolumns = [{field = \"name\"}]\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let profile = config.profile("by_status").unwrap();
    let names = |rows: &list::SampleList| -> Vec<String> {
        rows.iter()
            .map(|entry| entry.sample.borrow().name().unwrap().to_string())
            .collect()
    };
    let (carrying, rows) =
        exports::grouped(profile, &collection, list::GroupOrder::Values).unwrap();
    let fields: Vec<&str> = carrying
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["status", "name", "malt"]);
    assert_eq!(names(&rows), ["A", "C", "B"]);
    // Sorted by malt, the rejected B is the lightest, and its group leads.
    let lightest_first = collection
        .sorted(&samplekit::query::ordering::parse_spec(&["malt".to_string()]).unwrap())
        .unwrap();
    let (_, rows) = exports::grouped(profile, &lightest_first, list::GroupOrder::Listed).unwrap();
    assert_eq!(names(&rows), ["B", "C", "A"]);
    // A profile grouping by nothing is unchanged.
    let flat = config.profile("flat").unwrap();
    let (same, rows) = exports::grouped(flat, &collection, list::GroupOrder::Values).unwrap();
    assert_eq!(&same, flat);
    assert_eq!(names(&rows), names(&collection));
}
