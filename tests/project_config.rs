//! The tests of `project_config`.

use std::fs;
use std::path::{Path, PathBuf};

use samplekit::config::project_config::{self as config, ConfigError, FigureKind, Format};
use samplekit::core::formatting::{Precision, Presentation};
use samplekit::format::schema::PrecisionSchema;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-cfg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// A `.samplekitrc` in `relative`, which is created.
    fn config(&self, relative: &str, body: &str) -> PathBuf {
        let dir = self.dir(relative);
        let path = dir.join(".samplekitrc");
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A project declaring one of everything.
const FULL: &str = r#"
schema_version = 1

[model]
path = "model.py"
class = "Model"

[render]
style = "math"
precision = ".3f"
table = "boxed"

[collection]
recursive = true
include = ["*.sample.md"]
exclude = ["draft-*"]

[unit."g/L"]
plain = "g/L"
math = "\\mathrm{g\\,L^{-1}}"

[property.brix]
symbol = "Bx"
symbol_math = "\\brix"
precision = ".4f"
unit = "g/L"

[property."fermentation.rate"]
symbol_math = "r_{\\mathrm{warm}}"

[property."fermentation_cold.rate"]
symbol_math = "r_{\\mathrm{cold}}"

[style.math]
separator = "\\pm"

[query.approved]
filter = 'status == "approved"'
directory = "transport/"

[profile.platos]
sort = ["beer", "-plato"]
columns = [
  {field = "beer"},
  {field = "plato", label = "Plato", precision = ".4f"},
]

[export.approved_plato]
profile = "platos"
format = "csv"
output = "generated/approved-plato.tsv"
"#;

// ------------------------------------------------------------------ lookup

#[test]
fn finds_nearest_upward() {
    // A nested directory uses the closest configuration.
    let scratch = Scratch::new("nearest");
    scratch.config("", "schema_version = 1\n");
    let inner = scratch.config("vintage", "schema_version = 1\n");
    let deep = scratch.dir("vintage/runs");

    assert_eq!(config::find(&deep).as_deref(), Some(inner.as_path()));
    let outer_only = scratch.dir("other");
    assert_eq!(
        config::find(&outer_only)
            .unwrap()
            .parent()
            .unwrap()
            .file_name(),
        scratch.0.file_name()
    );
}

#[test]
fn does_not_merge_configurations() {
    // A nested file fully replaces the parent: no key inheritance.
    let scratch = Scratch::new("no-merge");
    scratch.config(
        "",
        "schema_version = 1\n[collection]\nrecursive = true\ninclude = [\"*.sample.md\"]\n",
    );
    scratch.config("vintage", "schema_version = 1\n");
    let inner = config::load_for(&scratch.dir("vintage")).unwrap().unwrap();
    // The parent said recursive; the nearest one says nothing, so the default
    // applies rather than the parent's value.
    assert!(!inner.collection().recursive);
    assert_eq!(inner.collection().include, ["*.md", "*.MD", "*.markdown"]);
}

#[test]
fn absence_is_not_an_error() {
    // A bare directory loads as `None`.
    let scratch = Scratch::new("absent");
    let bare = scratch.dir("samples");
    assert!(config::find(&bare).is_none());
    assert!(config::load_for(&bare).unwrap().is_none());
}

#[test]
fn paths_resolve_relative_to_the_file() {
    // Not to the working directory, so a project moves as a whole.
    let scratch = Scratch::new("relative");
    let path = scratch.config("project", FULL);
    let loaded = config::load(&path).unwrap();
    assert_eq!(
        loaded.resolve("generated/out.tsv"),
        path.parent().unwrap().join("generated/out.tsv")
    );
    // An absolute path is left alone.
    let absolute = if cfg!(windows) {
        "C:\\etc\\hosts"
    } else {
        "/etc/hosts"
    };
    assert_eq!(loaded.resolve(absolute), Path::new(absolute));
}

// -------------------------------------------------------------- versioning

#[test]
fn unsupported_version_is_refused() {
    let scratch = Scratch::new("version");
    let path = scratch.config("", "schema_version = 9\n");
    let error = config::load(&path).unwrap_err();
    let ConfigError::UnsupportedVersion { found, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(*found, 9);
    // Naming both versions.
    let message = error.to_string();
    assert!(
        message.contains('9') && message.contains("schema_version 1"),
        "{message}"
    );
}

#[test]
fn document_and_config_versions_are_independent() {
    // A version-1 config reads version-1 documents without coupling: nothing
    // here consults the sample schema's version.
    let scratch = Scratch::new("independent");
    let path = scratch.config("", "schema_version = 1\n");
    assert_eq!(config::load(&path).unwrap().schema_version(), 1);
    assert_eq!(samplekit::format::schema::version(), 1);
}

#[test]
fn unknown_section_is_refused() {
    // Rather than ignored: a silently inert section is principle 3's failure
    // in configuration form.
    let scratch = Scratch::new("unknown-section");
    let path = scratch.config("", "schema_version = 1\n[bogus]\nkey = 1\n");
    let error = config::load(&path).unwrap_err();
    assert!(matches!(error, ConfigError::Parse { .. }), "{error:?}");
    assert!(error.to_string().contains("bogus"), "{error}");
}

#[test]
fn a_parse_error_names_its_line() {
    // An error starting a line was said on the line above it.
    let scratch = Scratch::new("error-line");
    let path = scratch.config("", "schema_version = 1\n\nbogus = 2\n");
    match config::load(&path).unwrap_err() {
        ConfigError::Parse { line, .. } => assert_eq!(line, Some(3)),
        other => panic!("expected a parse error, got {other:?}"),
    }
    let path = scratch.config("", "schema_version = 1\n[collection]\nrecursive = maybe\n");
    match config::load(&path).unwrap_err() {
        ConfigError::Parse { line, .. } => assert_eq!(line, Some(3)),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

// ------------------------------------------------------------- validation

#[test]
fn all_profiles_are_validated_at_load() {
    // A broken export fails the load even when only a view is requested: the
    // typo in something used once a month is reported the next time anything
    // in the project is loaded.
    let scratch = Scratch::new("eager");
    let path = scratch.config(
        "",
        "schema_version = 1\n\
         [profile.good]\ncolumns = [{field = \"malt\"}]\n\
         [export.broken]\nprofile = \"good\"\nformat = \"xml\"\noutput = \"out\"\n",
    );
    let error = config::load(&path).unwrap_err();
    assert!(error.to_string().contains("xml"), "{error}");
}

#[test]
fn a_filter_is_loaded_as_written() {
    // Held verbatim: parsing one is Layer 4's and this is Layer 3.
    let scratch = Scratch::new("verbatim");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.q]\nfilter = 'status == \"approved\" and ('\n",
    );
    // Malformed as an expression, and accepted here without complaint.
    let loaded = config::load(&path).unwrap();
    assert_eq!(
        loaded.query("q").unwrap().filter,
        "status == \"approved\" and ("
    );
}

#[test]
fn unknown_profile_suggests_nearest_name() {
    // And lists what exists.
    let scratch = Scratch::new("suggest");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let error = loaded.profile("pltaos").unwrap_err();
    let ConfigError::UnknownProfile {
        suggestion,
        available,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("platos"));
    assert_eq!(available, &["platos"]);
}

#[test]
fn duplicate_field_in_a_profile_is_refused() {
    // Naming the field and the profile.
    let scratch = Scratch::new("dup-field");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\n\
         columns = [{field = \"malt\"}, {field = \"malt\", label = \"Again\"}]\n",
    );
    let error = config::load(&path).unwrap_err();
    let ConfigError::DuplicateField { profile, field } = &error else {
        panic!("{error:?}");
    };
    assert_eq!((profile.as_str(), field.as_str()), ("p", "malt"));
}

#[test]
fn duplicate_header_is_refused() {
    // Naming both fields that collided, because nothing could tell the two
    // columns apart in the exported file.
    let scratch = Scratch::new("dup-header");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\n\
         columns = [{field = \"malt\", header = \"m\"}, {field = \"plato\", header = \"m\"}]\n",
    );
    let error = config::load(&path).unwrap_err();
    let ConfigError::DuplicateHeader { header, fields, .. } = &error else {
        panic!("{error:?}");
    };
    // The emitted name, not the declared one: `m_value` is what would appear
    // twice in the exported file, and naming that is what a reader can act on.
    assert_eq!(header, "m_value");
    assert_eq!(fields, &["malt", "plato"]);
}

#[test]
fn empty_label_is_refused() {
    // Its own message: omit the key to fall back to the field, rather than
    // naming it nothing.
    let scratch = Scratch::new("empty-label");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\", label = \"\"}]\n",
    );
    let error = config::load(&path).unwrap_err();
    assert!(
        matches!(error, ConfigError::EmptyHeader { .. }),
        "{error:?}"
    );
}

#[test]
fn an_export_naming_an_undeclared_profile_is_refused() {
    // At load, with the available names.
    let scratch = Scratch::new("export-profile");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.platos]\ncolumns = [{field = \"malt\"}]\n\
         [export.e]\nprofile = \"pltaos\"\nformat = \"csv\"\noutput = \"out.csv\"\n",
    );
    let error = config::load(&path).unwrap_err();
    let ConfigError::UnknownProfile {
        suggestion,
        available,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("platos"));
    assert_eq!(available, &["platos"]);
}

#[test]
fn an_export_may_name_a_query() {
    // Read into the target, and an undeclared one refused at load.
    let scratch = Scratch::new("export-query");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.light]\nfilter = \"malt < 12\"\n\
         [profile.platos]\ncolumns = [{field = \"malt\"}]\n\
         [export.e]\nprofile = \"platos\"\nformat = \"csv\"\noutput = \"out.csv\"\nquery = \"light\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.export("e").unwrap().query.as_deref(), Some("light"));
    let broken = scratch.config(
        "b",
        "schema_version = 1\n[query.light]\nfilter = \"malt < 12\"\n\
         [profile.platos]\ncolumns = [{field = \"malt\"}]\n\
         [export.e]\nprofile = \"platos\"\nformat = \"csv\"\noutput = \"out.csv\"\nquery = \"lighr\"\n",
    );
    let error = config::load(&broken).unwrap_err();
    let ConfigError::UnknownProfile {
        suggestion,
        available,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("light"));
    assert_eq!(available, &["light"]);
}

#[test]
fn a_query_base_is_refused_naming_directory() {
    // The key was renamed to say what it is, and the old one is refused by name
    // with the migration that rewrites it.
    let scratch = Scratch::new("query-base");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.approved]\nfilter = \"status == approved\"\nbase = \"transport/\"\n",
    );
    let error = config::load(&path).unwrap_err();
    assert!(
        matches!(&error, ConfigError::RenamedKey { section, old, new, .. } if section == "query.approved" && *old == "base" && *new == "directory"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("'base' is now 'directory'"), "{message}");
    assert!(
        message.contains("no command rewrites a configuration"),
        "{message}"
    );
    let renamed = scratch.config(
        "r",
        "schema_version = 1\n[query.approved]\nfilter = \"status == approved\"\ndirectory = \"transport/\"\n",
    );
    let config = config::load(&renamed).unwrap();
    assert_eq!(
        config.query("approved").unwrap().directory.as_deref(),
        Some(std::path::Path::new("transport/"))
    );
}

// ----------------------------------------------------------------- model

#[test]
fn reading_a_config_does_not_import_the_model() {
    // Asserted with a model whose import would have an observable effect: the
    // file does not exist, and loading the configuration still succeeds.
    let scratch = Scratch::new("no-import");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let declared = loaded.model().unwrap();
    assert_eq!(declared.path, PathBuf::from("model.py"));
    assert_eq!(declared.class.as_deref(), Some("Model"));
    assert!(!loaded.resolve("model.py").exists());
}

#[test]
fn model_path_resolves_against_the_config_file() {
    // Not the working directory: a path moves with the project.
    let scratch = Scratch::new("model-path");
    let path = scratch.config("project", FULL);
    let loaded = config::load(&path).unwrap();
    let declared = loaded.model().unwrap();
    assert_eq!(
        loaded.resolve(declared.path.to_str().unwrap()),
        path.parent().unwrap().join("model.py")
    );
}

// ------------------------------------------------- properties and units

#[test]
fn a_property_is_declared_once_for_the_project() {
    // `[property.brix]` supplies the symbol and precision no file repeats.
    let scratch = Scratch::new("property");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let brix = loaded.property("brix").unwrap();
    assert_eq!(brix.symbol.as_deref(), Some("Bx"));
    assert_eq!(
        brix.symbol_variants.get("symbol_math").map(String::as_str),
        Some("\\brix")
    );
    assert_eq!(brix.unit.as_deref(), Some("g/L"));
    assert_eq!(
        brix.precision,
        Some(PrecisionSchema::Both(".4f".to_string()))
    );
}

#[test]
fn a_qualified_property_key_is_one_key() {
    // A dotted TOML key nests instead of naming: quoted, the two variants of
    // one quantity stay two entries rather than becoming a table.
    let scratch = Scratch::new("qualified");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let standard = loaded.property("fermentation.rate").unwrap();
    let cold = loaded.property("fermentation_cold.rate").unwrap();
    assert_ne!(
        standard.symbol_variants.get("symbol_math"),
        cold.symbol_variants.get("symbol_math")
    );
    assert!(loaded.property("fermentation").is_none(), "the key nested");
}

#[test]
fn a_table_style_is_carried_and_not_interpreted() {
    // `[render] table` reaches the caller as written. Which names are valid is
    // `terminal_rendering::Style`'s to say, and this module is Layer 3.
    let scratch = Scratch::new("table-style");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.render().table.as_deref(), Some("boxed"));
    // A name this module never heard of is carried just the same: reporting it
    // is the renderer's job, and it is the renderer that knows the list.
    let odd = scratch.config(
        "odd",
        "schema_version = 1
[render]
table = \"carved\"\n",
    );
    assert_eq!(
        config::load(&odd).unwrap().render().table.as_deref(),
        Some("carved")
    );
    // And a project declaring nothing declares no style.
    let bare = scratch.config("bare-table", "schema_version = 1\n");
    assert!(config::load(&bare).unwrap().render().table.is_none());
}

#[test]
fn a_unit_key_is_the_spelling_a_file_writes() {
    // `[unit."g/L"]` is found from `unit: g/L`.
    let scratch = Scratch::new("unit-key");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let declared = loaded.unit("g/L").unwrap();
    assert_eq!(declared.get("plain").map(String::as_str), Some("g/L"));
    // And an undeclared spelling has no entry: it renders as itself.
    assert!(loaded.unit("g/l").is_none());
}

#[test]
fn an_undeclared_unit_is_reported_not_corrected() {
    // `g/dl` beside a declared `g/L` is two units as far as this format is
    // concerned. The vocabulary is what lets a caller say so.
    let scratch = Scratch::new("vocabulary");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.unit_vocabulary(), ["g/L"]);
    assert!(loaded.unit("g/dl").is_none());
    // A project declaring none is measured against none.
    let bare = scratch.config("bare", "schema_version = 1\n");
    assert!(config::load(&bare).unwrap().unit_vocabulary().is_empty());
}

#[test]
fn a_declaration_fills_what_a_file_omits() {
    // 50 quantities declared once instead of 39 090 times. The file still wins
    // where it speaks.
    let scratch = Scratch::new("resolved");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();

    let bare = Presentation {
        unit: None,
        symbol: None,
        precision: None,
    };
    // FULL declares `style = "math"`, so the math variants are the literals.
    let resolved = loaded.resolved(&bare, "brix").unwrap();
    assert_eq!(resolved.symbol.as_deref(), Some("\\brix"));
    assert_eq!(
        resolved.precision.as_ref().map(|p| p.value().as_str()),
        Some(".4f")
    );
    assert_eq!(resolved.unit.as_deref(), Some("\\mathrm{g\\,L^{-1}}"));

    let written = Presentation {
        unit: None,
        symbol: Some("r".to_string()),
        precision: Some(Precision::both(".1f").unwrap()),
    };
    let file_wins = loaded.resolved(&written, "brix").unwrap();
    assert_eq!(
        file_wins.precision.as_ref().map(|p| p.value().as_str()),
        Some(".1f")
    );
    // The symbol is the exception, and not a broken rule: under `math` the
    // declared `symbol_math` is the math literal for this quantity, and a file
    // writing a plain `r` says nothing about it.
    assert_eq!(file_wins.symbol.as_deref(), Some("\\brix"));
    // Under `plain` the file's own symbol wins, as it always did.
    let plain = scratch.config(
        "plain-symbol",
        "schema_version = 1\n[property.brix]\nsymbol = \"Bx\"\n",
    );
    assert_eq!(
        config::load(&plain)
            .unwrap()
            .resolved(&written, "brix")
            .unwrap()
            .symbol
            .as_deref(),
        Some("r")
    );

    // A quantity the project says nothing about resolves to what the file says.
    let unknown = loaded.resolved(&written, "no_such_quantity").unwrap();
    assert_eq!(unknown.symbol.as_deref(), Some("r"));
    assert!(unknown.unit.is_none());
}

#[test]
fn the_plain_style_uses_the_declared_plain_form() {
    // `[unit."g/L"] plain = "g/L"` must not be the one declaration that
    // never applies — and under `style = "math"` the math form is used instead.
    let scratch = Scratch::new("plain-style");
    let plain = scratch.config(
        "plain",
        "schema_version = 1\n[unit.\"g/L\"]\nplain = \"g/L\"\nmath = \"\\\\mathrm{g}\"\n",
    );
    let presentation = Presentation {
        unit: Some("g/L".to_string()),
        symbol: None,
        precision: None,
    };
    let loaded = config::load(&plain).unwrap();
    let resolved = loaded.resolved(&presentation, "brix").unwrap();
    assert_eq!(resolved.unit.as_deref(), Some("g/L"));
    assert_eq!(resolved.separator, "\u{b1}");

    let math = scratch.config(
        "math",
        "schema_version = 1\n[render]\nstyle = \"math\"\n[style.math]\nseparator = \"\\\\pm\"\n\
         [unit.\"g/L\"]\nplain = \"g/L\"\nmath = \"\\\\mathrm{g}\"\n",
    );
    let under_math = config::load(&math)
        .unwrap()
        .resolved(&presentation, "brix")
        .unwrap();
    assert_eq!(under_math.unit.as_deref(), Some("\\mathrm{g}"));
    assert_eq!(under_math.separator, "\\pm");

    // An undeclared spelling renders as itself: it is already legible.
    let itself = Presentation {
        unit: Some("kg/hL".to_string()),
        symbol: None,
        precision: None,
    };
    assert_eq!(
        loaded.resolved(&itself, "brix").unwrap().unit.as_deref(),
        Some("kg/hL")
    );
}

#[test]
fn a_style_naming_nothing_declared_is_an_error() {
    // Not a silent fallback to plain: that renders a whole manuscript table
    // wrong without a word.
    let scratch = Scratch::new("unknown-style");
    let path = scratch.config(
        "",
        "schema_version = 1\n[style.math]\nseparator = \"\\\\pm\"\n",
    );
    let loaded = config::load(&path).unwrap();
    let error = loaded
        .resolved_as(&Presentation::default(), "brix", Some("mathh"))
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("mathh"), "{message}");
    assert!(message.contains("plain, math"), "{message}");
}

#[test]
fn an_explicit_render_style_overrides_without_mutating() {
    let scratch = Scratch::new("transient-style");
    let path = scratch.config(
        "",
        "schema_version = 1\n[render]\nstyle = \"plain\"\n\
         [style.math]\nseparator = \"\\\\pm\"\n\
         [unit.lintner]\nplain = \"°L\"\nmath = \"\\\\Omega\"\n",
    );
    let loaded = config::load(&path).unwrap();
    let presentation = Presentation {
        unit: Some("lintner".to_string()),
        symbol: None,
        precision: None,
    };

    let math = loaded
        .resolved_as(&presentation, "wort", Some("math"))
        .unwrap();
    assert_eq!(math.unit.as_deref(), Some("\\Omega"));
    assert_eq!(math.separator, "\\pm");

    let stored = loaded.resolved(&presentation, "wort").unwrap();
    assert_eq!(stored.unit.as_deref(), Some("°L"));
    assert_eq!(stored.separator, "±");
    assert_eq!(loaded.render().style.as_deref(), Some("plain"));
}

#[test]
fn render_style_names_start_with_plain() {
    let scratch = Scratch::new("style-names");
    let path = scratch.config(
        "",
        "schema_version = 1\n[style.math]\nseparator = \"\\\\pm\"\n\
         [style.ascii]\nseparator = \"+/-\"\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.render_style_names(), ["plain", "math", "ascii"]);
}

#[test]
fn a_relative_start_still_walks_upward() {
    // `.` has no parent to pop, so a relative start would stop at the first
    // directory: the project above a sample directory must stay visible to
    // anyone who has `cd`-ed into it.
    let scratch = Scratch::new("relative");
    let root = scratch.config("", FULL);
    let parent = root.parent().unwrap().to_path_buf();
    let nested = parent.join("deep/deeper");
    fs::create_dir_all(&nested).unwrap();
    let here = std::env::current_dir().unwrap();
    std::env::set_current_dir(&nested).unwrap();
    let found = config::find(Path::new("."));
    std::env::set_current_dir(here).unwrap();
    assert_eq!(
        found.map(|path| dunce::canonicalize(path).unwrap()),
        Some(dunce::canonicalize(&root).unwrap())
    );
}

#[test]
fn declared_quantities_are_listed_in_declaration_order() {
    let scratch = Scratch::new("property-names");
    let path = scratch.config(
        "",
        "schema_version = 1\n\n[property.brix]\nunit = \"g/L\"\n\n[property.malt]\nunit = \"g\"\n\n[property.ebc]\nprecision = \".2f\"\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.property_names(), ["brix", "malt", "ebc"]);
}

#[test]
fn model_python_resolves_against_the_config_file() {
    let scratch = Scratch::new("model-python");
    let path = scratch.config(
        "project",
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"env/bin/python\"\n",
    );
    let loaded = config::load(&path).unwrap();
    let declared = loaded.model().unwrap();
    assert_eq!(declared.python, Some(PathBuf::from("env/bin/python")));
    assert_eq!(declared.class, None);
    // Named, and started by nothing: the interpreter does not even exist.
    let python = loaded.resolve("env/bin/python");
    assert_eq!(python, path.parent().unwrap().join("env/bin/python"));
    assert!(!python.exists());
}

#[test]
fn a_render_style_naming_nothing_declared_is_refused_at_load() {
    let scratch = Scratch::new("render-style-at-load");
    let path = scratch.config(
        "",
        "schema_version = 1\n[render]\nstyle = \"fancy\"\n[style.math]\nseparator = \"\\\\pm\"\n",
    );
    let Err(error) = config::load(&path) else {
        panic!("a render style naming nothing was accepted");
    };
    let message = error.to_string();
    assert!(message.contains("fancy"), "{message}");
    assert!(message.contains("plain, math"), "{message}");
}

#[test]
fn an_invalid_property_precision_is_refused_at_load() {
    let scratch = Scratch::new("property-precision");
    let path = scratch.config(
        "",
        "schema_version = 1\n[property.pressure]\nprecision = \".99z\"\n",
    );
    let Err(error) = config::load(&path) else {
        panic!("an invalid precision was accepted");
    };
    let message = error.to_string();
    assert!(message.contains("property.pressure"), "{message}");
}

#[test]
fn an_invalid_column_precision_is_refused_at_load() {
    // A profile's or a view's column, and a specifier inside a template, are
    // checked as a `[property.*]` precision is — accepted, each was dropped at
    // the first rendering and the default shown in its place.
    let scratch = Scratch::new("column-precision");
    for (body, names) in [
        (
            "[profile.latex]\ncolumns = [{ field = \"brix\", precision = \".3z\" }]\n",
            ["profile.latex", "'brix'", ".3z"],
        ),
        (
            "[profile.latex]\ncolumns = [{ field = \"brix\", template = \"\\\\num{{value:.3z}}\" }]\n",
            ["profile.latex", "'brix'", "{value:.3z}"],
        ),
        (
            "[profile.latex]\ncolumns = [{ field = \"brix\", template = \"{unit:.3f}\" }]\n",
            ["profile.latex", "'brix'", "takes no specifier"],
        ),
    ] {
        let path = scratch.config("", &format!("schema_version = 1\n{body}"));
        let Err(error) = config::load(&path) else {
            panic!("accepted: {body}");
        };
        let message = error.to_string();
        for name in names {
            assert!(message.contains(name), "{name}: {message}");
        }
    }
    // A LaTeX brace, a channel without a specifier and a valid one all load.
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.latex]\ncolumns = [{ field = \"brix\", \
         template = \"\\\\num{{value:.3f}} \\\\pm {u:.1e} \\\\si{{unit}} {symbol}\" }]\n",
    );
    config::load(&path).unwrap();
}

#[test]
fn an_export_flag_is_named_after_the_field_it_adds() {
    // `name`, `filename` and `path` are ordinary fields a profile may name as
    // columns, and the two shorthands on an export are spelt as the fields they
    // add. `include_filename` was the v1 spelling of one thing under two names.
    let scratch = Scratch::new("export-flags");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.malts]\ncolumns = [{ field = \"malt\" }]\n\
         [export.malts]\nprofile = \"malts\"\nformat = \"csv\"\n\
         output = \"out/malts.csv\"\nfilename = true\npath = true\n",
    );
    let config = config::load(&path).unwrap();
    let target = config.export("malts").unwrap();
    assert!(target.filename);
    assert!(target.path);
    // The destination is `output`, and the flag beside it names a column.
    assert!(target.output.ends_with("malts.csv"));

    let refused = scratch.config(
        "",
        "schema_version = 1\n[profile.malts]\ncolumns = [{ field = \"malt\" }]\n\
         [export.malts]\nprofile = \"malts\"\nformat = \"csv\"\n\
         output = \"out/malts.csv\"\ninclude_filename = true\n",
    );
    let Err(error) = config::load(&refused) else {
        panic!("the include_ spelling was accepted");
    };
    let message = error.to_string();
    for name in ["include_filename", "filename", "path"] {
        assert!(message.contains(name), "{name}: {message}");
    }
}

#[test]
fn an_unknown_key_in_a_property_declaration_is_refused() {
    let scratch = Scratch::new("property-key");
    let accepted = scratch.config(
        "",
        "schema_version = 1\n[property.pressure]\nprecision = \".2f\"\nsymbol_math = \"P\"\n",
    );
    assert!(config::load(&accepted).is_ok());
    let path = scratch.config(
        "",
        "schema_version = 1\n[property.pressure]\nprecison = \".2f\"\n",
    );
    let Err(error) = config::load(&path) else {
        panic!("an unknown key was accepted");
    };
    let message = error.to_string();
    assert!(
        message.contains("precison") && message.contains("did you mean 'precision'?"),
        "{message}"
    );
}

#[test]
fn a_profile_without_columns_is_refused() {
    let scratch = Scratch::new("empty-profile");
    let path = scratch.config("", "schema_version = 1\n[profile.empty]\ncolumns = []\n");
    let Err(error) = config::load(&path) else {
        panic!("a profile without columns was accepted");
    };
    let message = error.to_string();
    assert!(
        message.contains("empty") && message.contains("no column"),
        "{message}"
    );
}

#[test]
fn render_identify_is_name_filename_or_none() {
    let scratch = Scratch::new("identify");
    let bare = scratch.config("identify-bare", "schema_version = 1\n");
    assert_eq!(
        config::load(&bare).unwrap().render().identify,
        config::Identify::Name
    );
    let file = scratch.config(
        "identify-file",
        "schema_version = 1\n[render]\nidentify = \"filename\"\n",
    );
    assert_eq!(
        config::load(&file).unwrap().render().identify,
        config::Identify::Filename
    );
    let odd = scratch.config(
        "identify-odd",
        "schema_version = 1\n[render]\nidentify = \"path\"\n",
    );
    let error = config::load(&odd).unwrap_err().to_string();
    assert!(error.contains("name, filename or none"), "{error}");
}

#[test]
fn render_bounds_is_no_longer_a_key() {
    // A declared precision rounds alike on every surface, so a screen has no
    // second form to ask for.
    let scratch = Scratch::new("bounds");
    let asked = scratch.config(
        "bounds-asked",
        "schema_version = 1\n[render]\nbounds = true\n",
    );
    let error = config::load(&asked).unwrap_err().to_string();
    assert!(error.contains("bounds was withdrawn"), "{error}");
    assert!(error.contains("upgrading guide"), "{error}");
}

// ------------------------------------------------------------------ figures

#[test]
fn a_figure_declares_its_kind_its_axes_its_group_and_its_query() {
    // What a figure draws, read in declaration order, and a kind left out is a
    // scatter.
    let scratch = Scratch::new("figure");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.light]\nfilter = \"malt < 12\"\n\
         [figure.vfi]\nkind = \"line\"\nx = \"period\"\ny = \"vfi\"\ngroup = \"beer\"\nquery = \"light\"\n\
         [figure.bare]\nx = \"malt\"\ny = \"brix\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.figure_names(), ["vfi", "bare"]);
    let vfi = config.figure("vfi").unwrap();
    assert_eq!(vfi.kind, FigureKind::Line);
    assert_eq!((vfi.x.as_str(), vfi.y.as_str()), ("period", "vfi"));
    assert_eq!(vfi.group.as_deref(), Some("beer"));
    assert_eq!(vfi.query.as_deref(), Some("light"));
    let bare = config.figure("bare").unwrap();
    assert_eq!(bare.kind, FigureKind::Scatter);
    assert_eq!(bare.group, None);
    assert!(config.figure("absent").is_none());
}

#[test]
fn a_figure_kind_that_is_not_one_is_refused_naming_the_kinds() {
    let scratch = Scratch::new("figure-kind");
    let path = scratch.config(
        "",
        "schema_version = 1\n[figure.f]\nkind = \"pie\"\nx = \"a\"\ny = \"b\"\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    assert!(error.contains("'pie'"), "{error}");
    assert!(error.contains("scatter, line, step, bar or box"), "{error}");
    assert!(error.contains("figure.f"), "{error}");
}

#[test]
fn a_figure_key_that_is_not_one_is_refused() {
    // A fit and a reference line are the model's to draw.
    let scratch = Scratch::new("figure-key");
    for key in ["fit = \"linear\"", "reference = \"y = x\""] {
        let path = scratch.config(
            "",
            &format!("schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n{key}\n"),
        );
        let error = config::load(&path).unwrap_err().to_string();
        let word = key.split(' ').next().unwrap();
        assert!(error.contains(word), "{error}");
    }
}

#[test]
fn a_figure_naming_an_undeclared_query_is_refused() {
    let scratch = Scratch::new("figure-query");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.light]\nfilter = \"malt < 12\"\n\
         [figure.f]\nx = \"a\"\ny = \"b\"\nquery = \"lighr\"\n",
    );
    let error = config::load(&path).unwrap_err();
    let ConfigError::UnknownProfile { suggestion, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("light"));
}

#[test]
fn a_figure_without_both_axes_is_refused() {
    let scratch = Scratch::new("figure-axes");
    let path = scratch.config("", "schema_version = 1\n[figure.f]\nx = \"a\"\n");
    let error = config::load(&path).unwrap_err().to_string();
    assert!(error.contains("figure.f"), "{error}");
    assert!(error.contains("x and y"), "{error}");
}

#[test]
fn a_figure_may_name_its_title_its_labels_and_its_style() {
    // How its axes read, and a style that names nothing refused.
    let scratch = Scratch::new("figure-labels");
    let path = scratch.config(
        "",
        "schema_version = 1\n[style.figure]\n[figure.f]\nx = \"a\"\ny = \"b\"\n\
         title = \"T\"\nx_label = \"A\"\ny_label = \"$b$\"\nstyle = \"figure\"\n",
    );
    let config = config::load(&path).unwrap();
    let figure = config.figure("f").unwrap();
    assert_eq!(figure.title.as_deref(), Some("T"));
    assert_eq!(figure.x_label.as_deref(), Some("A"));
    assert_eq!(figure.y_label.as_deref(), Some("$b$"));
    assert_eq!(figure.style.as_deref(), Some("figure"));
    let broken = scratch.config(
        "b",
        "schema_version = 1\n[style.figure]\n[figure.f]\nx = \"a\"\ny = \"b\"\nstyle = \"figur\"\n",
    );
    let error = config::load(&broken).unwrap_err().to_string();
    assert!(error.contains("figur"), "{error}");
    assert!(error.contains("figure"), "{error}");
}

#[test]
fn a_matplotlib_section_is_read_by_matplotlibs_names() {
    // Quoted or nested, a setting is the dotted name matplotlib gives it.
    let scratch = Scratch::new("matplotlib");
    let path = scratch.config(
        "",
        "schema_version = 1\n[matplotlib]\n\"font.size\" = 13\naxes.grid = true\n\
         \"figure.figsize\" = [7, 5]\n",
    );
    let config = config::load(&path).unwrap();
    let names: Vec<&str> = config.matplotlib().keys().map(String::as_str).collect();
    assert_eq!(names, ["font.size", "axes.grid", "figure.figsize"]);
    assert_eq!(config.matplotlib()["font.size"].as_integer(), Some(13));
    let dated = scratch.config(
        "d",
        "schema_version = 1\n[matplotlib]\n\"date.epoch\" = 1970-01-01\n",
    );
    let error = config::load(&dated).unwrap_err().to_string();
    assert!(error.contains("date.epoch"), "{error}");
    // Written twice, one of them would be dropped without a word.
    let twice = scratch.config(
        "t",
        "schema_version = 1\n[matplotlib]\n\"font.size\" = 13\nfont.size = 14\n",
    );
    let error = config::load(&twice).unwrap_err().to_string();
    assert!(error.contains("font.size is written twice"), "{error}");
    // Where a figure opens is samplekit's choice, and MPLBACKEND's.
    let backend = scratch.config("k", "schema_version = 1\n[matplotlib]\nbackend = \"pdf\"\n");
    let error = config::load(&backend).unwrap_err().to_string();
    assert!(error.contains("MPLBACKEND"), "{error}");
}

#[test]
fn the_workbench_colours_are_named_by_role() {
    // A role the project leaves out keeps the terminal's colour.
    let scratch = Scratch::new("workbench-colors");
    let path = scratch.config(
        "",
        "schema_version = 1\n[workbench.colors]\noutdated = \"#ff8800\"\nmuted = \"grey\"\n\
         table = \"dark-gray\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.workbench_color("outdated"), Some("#ff8800"));
    // British and American spellings of grey are one colour.
    assert_eq!(config.workbench_color("muted"), Some("grey"));
    assert_eq!(config.workbench_color("table"), Some("dark-gray"));
    assert_eq!(config.workbench_color("failed"), Some("red"));
    // A role nobody draws, and a colour nothing can, are refused at load.
    let role = scratch.config(
        "r",
        "schema_version = 1\n[workbench.colors]\nstaled = \"red\"\n",
    );
    let error = config::load(&role).unwrap_err().to_string();
    assert!(
        error.contains("no role 'staled'") && error.contains("outdated"),
        "{error}"
    );
    let colour = scratch.config(
        "c",
        "schema_version = 1\n[workbench.colors]\noutdated = \"orange\"\n",
    );
    let error = config::load(&colour).unwrap_err().to_string();
    assert!(
        error.contains("not a colour") && error.contains("#rrggbb"),
        "{error}"
    );
    let key = scratch.config("k", "schema_version = 1\n[workbench]\ncolours = {}\n");
    assert!(config::load(&key).is_err());
}

#[test]
fn the_outdated_colour_is_still_read_under_its_former_name() {
    // `stale` became `outdated`; a project's colour written under the former
    // name keeps colouring what is outdated, and the new name wins where both
    // are written.
    let scratch = Scratch::new("workbench-stale");
    let former = scratch.config(
        "f",
        "schema_version = 1\n[workbench.colors]\nstale = \"#ff8800\"\n",
    );
    let config = config::load(&former).unwrap();
    assert_eq!(config.workbench_color("outdated"), Some("#ff8800"));
    let both = scratch.config(
        "b",
        "schema_version = 1\n[workbench.colors]\nstale = \"red\"\noutdated = \"#ff8800\"\n",
    );
    let config = config::load(&both).unwrap();
    assert_eq!(config.workbench_color("outdated"), Some("#ff8800"));
    let colour = scratch.config(
        "c",
        "schema_version = 1\n[workbench.colors]\nstale = \"orange\"\n",
    );
    let error = config::load(&colour).unwrap_err().to_string();
    assert!(error.contains("not a colour"), "{error}");
}

#[test]
fn a_figure_declares_its_limits_its_scales_and_its_aspect() {
    // A free bound is `auto`; a scale or an aspect that is not one is refused,
    // and so is a log axis starting at or below zero.
    let scratch = Scratch::new("figure-axes-settings");
    let path = scratch.config(
        "",
        "schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n\
         x_limits = [0, 100]\ny_limits = [1, \"auto\"]\ny_scale = \"log\"\naspect = \"equal\"\n",
    );
    let config = config::load(&path).unwrap();
    let figure = config.figure("f").unwrap();
    assert_eq!(figure.x_limits, Some((Some(0.0), Some(100.0))));
    assert_eq!(figure.y_limits, Some((Some(1.0), None)));
    assert_eq!(figure.y_scale.as_deref(), Some("log"));
    assert_eq!(figure.aspect.as_deref(), Some("equal"));
    // Where the legend goes, and the figure's size.
    let placed = scratch.config(
        "placed",
        "schema_version = 1\n[render]\nfigure_style = \"figure\"\n[style.figure]\n\
         [collection]\nfiles = [\"../images\"]\n\
         [figure.f]\nkind = \"box\"\nx = \"a\"\ny = \"b\"\nlegend = \"upper left\"\nfigsize = [7, 5]\n",
    );
    let config = config::load(&placed).unwrap();
    let figure = config.figure("f").unwrap();
    assert_eq!(figure.kind, config::FigureKind::Box);
    assert_eq!(figure.legend.as_deref(), Some("upper left"));
    assert_eq!(figure.figsize, Some((7.0, 5.0)));
    assert_eq!(config.render().figure_style.as_deref(), Some("figure"));
    assert_eq!(config.collection().files, ["../images"]);
    let unknown = scratch.config(
        "unknown-style",
        "schema_version = 1\n[render]\nfigure_style = \"figur\"\n",
    );
    let error = config::load(&unknown).unwrap_err().to_string();
    assert!(error.contains("figure_style"), "{error}");
    for (written, said) in [
        ("x_scale = \"logarithmic\"", "linear, log, symlog"),
        ("aspect = \"square\"", "equal, auto"),
        ("x_limits = [0]", "two bounds"),
        ("x_limits = [0, \"cent\"]", "'cent'"),
        ("y_scale = \"log\"\ny_limits = [0, 10]", "at or below zero"),
        (
            "y_scale = \"log\"\ny_limits = [\"auto\", -1]",
            "at or below zero",
        ),
        ("x_limits = [\"nan\", 5]", "finite"),
        ("x_limits = [\"auto\", inf]", "finite"),
        ("x_limits = [3, 3]", "no range"),
        ("legend = \"somewhere\"", "legend = 'somewhere'"),
        ("figsize = [7]", "figsize"),
        ("figsize = [7, -5]", "both positive"),
    ] {
        let broken = scratch.config(
            "b",
            &format!("schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n{written}\n"),
        );
        let error = config::load(&broken).unwrap_err().to_string();
        assert!(error.contains(said), "{written}: {error}");
    }
}

#[test]
fn a_declared_profile_can_be_read_back() {
    // Every section has an accessor, so nothing can be written into the file
    // and never read out of it.
    let scratch = Scratch::new("read-back");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();

    assert_eq!(
        loaded.query("approved").unwrap().directory,
        Some(PathBuf::from("transport/"))
    );
    let profile = loaded.profile("platos").unwrap();
    assert_eq!(profile.sort, ["beer", "-plato"]);
    let export = loaded.export("approved_plato").unwrap();
    assert_eq!(export.format, Format::Csv);
    assert_eq!(
        loaded.style("math").unwrap().separator.as_deref(),
        Some("\\pm")
    );
    assert_eq!(loaded.render().style.as_deref(), Some("math"));
}

#[test]
fn declaration_order_is_preserved() {
    // Profiles, queries and exports come back in file order.
    let scratch = Scratch::new("order");
    let path = scratch.config(
        "",
        "schema_version = 1\n\
         [profile.zulu]\ncolumns = [{field = \"malt\"}]\n\
         [profile.alpha]\ncolumns = [{field = \"malt\"}]\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.profile_names(), ["zulu", "alpha"]);
}

#[test]
fn the_tui_colours_are_read_under_the_sections_former_name_too() {
    // `[tui.colors]` is the section; `[workbench.colors]`, its name before, is
    // read as it stands, and `[tui.colors]` wins a role both write.
    let scratch = Scratch::new("tui-colours");
    let new = scratch.config(
        "n",
        "schema_version = 1\n[tui.colors]\nfailed = \"#ff0000\"\n",
    );
    let config = config::load(&new).unwrap();
    assert_eq!(config.workbench_color("failed"), Some("#ff0000"));
    let former = scratch.config(
        "f",
        "schema_version = 1\n[workbench.colors]\nmuted = \"grey\"\n",
    );
    let config = config::load(&former).unwrap();
    assert_eq!(config.workbench_color("muted"), Some("grey"));
    let both = scratch.config(
        "b",
        "schema_version = 1\n[workbench.colors]\nfailed = \"red\"\nmuted = \"grey\"\n\
         [tui.colors]\nfailed = \"#ff0000\"\n",
    );
    let config = config::load(&both).unwrap();
    assert_eq!(config.workbench_color("failed"), Some("#ff0000"));
    // A role only the former section writes keeps its colour.
    assert_eq!(config.workbench_color("muted"), Some("grey"));
    // A role written wrongly is named in the section it was written in.
    let role = scratch.config("r", "schema_version = 1\n[tui.colors]\nstaled = \"red\"\n");
    let error = config::load(&role).unwrap_err().to_string();
    assert!(
        error.contains("[tui.colors] has no role 'staled'"),
        "{error}"
    );
    let role = scratch.config(
        "w",
        "schema_version = 1\n[workbench.colors]\nstaled = \"red\"\n",
    );
    let error = config::load(&role).unwrap_err().to_string();
    assert!(
        error.contains("[workbench.colors] has no role 'staled'"),
        "{error}"
    );
}

#[test]
fn a_profile_declares_its_groups() {
    // `group` is a list of fields, as `sort` is, held in its order.
    let scratch = Scratch::new("profile-group");
    let path = scratch.config(
        "",
        "schema_version = 1\n\
         [profile.grouped]\ncolumns = [{field = \"malt\"}]\ngroup = [\"beer\", \"status\"]\n\
         [profile.flat]\ncolumns = [{field = \"malt\"}]\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.profile("grouped").unwrap().group, ["beer", "status"]);
    assert!(loaded.profile("flat").unwrap().group.is_empty());
    // One text is refused as `sort`'s is: a list is asked.
    let text = scratch.config(
        "t",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\"}]\ngroup = \"beer\"\n",
    );
    let error = config::load(&text).unwrap_err().to_string();
    assert!(error.contains("sequence"), "{error}");
    let sort = scratch.config(
        "s",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\"}]\nsort = \"beer\"\n",
    );
    let sorted = config::load(&sort).unwrap_err().to_string();
    assert!(sorted.contains("sequence"), "{sorted}");
    // A field left empty groups by nothing anyone can name.
    let empty = scratch.config(
        "e",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\"}]\ngroup = [\"\"]\n",
    );
    let error = config::load(&empty).unwrap_err().to_string();
    assert!(error.contains("empty field"), "{error}");
    // A saved query is a filter, and takes no group.
    let query = scratch.config(
        "q",
        "schema_version = 1\n[query.q]\nfilter = \"malt > 1\"\ngroup = [\"beer\"]\n",
    );
    let error = config::load(&query).unwrap_err().to_string();
    assert!(error.contains("unknown field `group`"), "{error}");
}

// ------------------------------------------------------------ imports

/// A root declaring what is common, as the owner's collection would.
const COMMON: &str = r#"
schema_version = 1

[model]
path = "root_model.py"
class = "Root"
python = "env/bin/python"

[collection]
files = ["images/{collection}/{name}*", "{collection}/photos/{name}*"]
exclude = ["draft-*"]

[property.collar]
unit = "cL"
symbol = "h"
precision = ".2f"

[query.tall]
filter = "collar > 2"
directory = "data"

[profile.physical]
columns = [{field = "name"}, {field = "collar"}]
sort = ["collar"]

[export.all]
profile = "physical"
format = "csv"
output = "exports/{collection}/all.csv"

[figure.collars]
x = "name"
y = "collar"
"#;

#[test]
fn an_import_merges_key_by_key() {
    let scratch = Scratch::new("import-merges");
    scratch.config("", COMMON);
    let own = scratch.config(
        "c1",
        "schema_version = 1\nimport = \"..\"\n\
         [collection]\nexclude = [\"old-*\"]\n\
         [property.collar]\nprecision = \".4f\"\n\
         [profile.physical]\nsort = [\"-collar\"]\n\
         [profile.own]\ncolumns = [{field = \"name\"}]\n",
    );
    let config = config::load(&own).unwrap();
    let collar = config.property("collar").unwrap();
    assert_eq!(collar.unit.as_deref(), Some("cL"));
    assert_eq!(collar.symbol.as_deref(), Some("h"));
    assert_eq!(
        collar.precision,
        Some(PrecisionSchema::Both(".4f".to_string()))
    );
    let physical = config.profile("physical").unwrap();
    assert_eq!(physical.sort, ["-collar"]);
    assert_eq!(physical.columns.len(), 2, "the columns imported stay");
    // An array replaces whole.
    assert_eq!(config.collection().exclude, ["old-*"]);
    assert_eq!(config.profile_names(), ["physical", "own"]);
    assert_eq!(config.query_names(), ["tall"]);
    assert_eq!(config.export_names(), ["all"]);
    assert_eq!(config.figure_names(), ["collars"]);
}

#[test]
fn imports_chain() {
    let scratch = Scratch::new("import-chain");
    let root = scratch.config("", COMMON);
    let data = scratch.config(
        "data",
        "schema_version = 1\nimport = \"..\"\n[query.middle]\nfilter = \"collar > 1\"\n\
         [property.collar]\nsymbol = \"H\"\n",
    );
    let own = scratch.config(
        "data/c1",
        "schema_version = 1\nimport = \"..\"\n[property.collar]\nprecision = \".1f\"\n",
    );
    let config = config::load(&own).unwrap();
    assert_eq!(config.query_names(), ["tall", "middle"]);
    let collar = config.property("collar").unwrap();
    assert_eq!(collar.symbol.as_deref(), Some("H"), "the nearer wins");
    assert_eq!(collar.unit.as_deref(), Some("cL"));
    assert_eq!(
        collar.precision,
        Some(PrecisionSchema::Both(".1f".to_string()))
    );
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap();
    assert_eq!(config.imports(), [canonical(&data), canonical(&root)]);
}

#[test]
fn an_import_loop_is_refused_naming_the_files() {
    let scratch = Scratch::new("import-loop");
    let one = scratch.config("one", "schema_version = 1\nimport = \"../two\"\n");
    let two = scratch.config("two", "schema_version = 1\nimport = \"../one\"\n");
    let error = config::load(&one).unwrap_err();
    let ConfigError::ImportLoop { files } = &error else {
        panic!("a loop: {error}");
    };
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap();
    assert_eq!(files, &[canonical(&one), canonical(&two), canonical(&one)]);
    let said = error.to_string().replace('\\', "/");
    assert!(said.contains("imports itself"), "{said}");
    assert!(said.contains("two/.samplekitrc"), "{said}");
}

#[test]
fn an_import_not_found_is_refused() {
    let scratch = Scratch::new("import-not-found");
    scratch.dir("empty");
    let own = scratch.config("c1", "schema_version = 1\nimport = \"../empty\"\n");
    let error = config::load(&own).unwrap_err();
    assert!(
        matches!(&error, ConfigError::ImportNotFound { import, .. } if import == "../empty"),
        "{error}"
    );
    // A path as Unix writes it, whatever the system said it with.
    let said = error.to_string().replace('\\', "/");
    assert!(said.contains("c1/.samplekitrc"), "{said}");
    assert!(said.contains("holds no .samplekitrc"), "{said}");
    // A file, rather than the folder holding it.
    scratch.config("", "schema_version = 1\n");
    let file = scratch.config("c2", "schema_version = 1\nimport = \"../.samplekitrc\"\n");
    let said = config::load(&file).unwrap_err().to_string();
    assert!(said.contains("import = \"..\""), "{said}");
    let nothing = scratch.config("c3", "schema_version = 1\nimport = \"../nowhere\"\n");
    let said = config::load(&nothing).unwrap_err().to_string();
    assert!(said.contains("is no folder"), "{said}");
}

#[test]
fn the_model_is_not_imported() {
    let scratch = Scratch::new("import-model");
    scratch.config("", COMMON);
    let bare = scratch.config("c1", "schema_version = 1\nimport = \"..\"\n");
    assert!(config::load(&bare).unwrap().model().is_none());
    let own = scratch.config(
        "c2",
        "schema_version = 1\nimport = \"..\"\n[model]\npath = \"model.py\"\n",
    );
    let config = config::load(&own).unwrap();
    let model = config.model().unwrap();
    assert_eq!(model.path, PathBuf::from("model.py"));
    assert_eq!(model.class, None, "the class is the root's own");
    // `python` is imported, read from the root.
    let python = config.resolve(&model.python.as_ref().unwrap().to_string_lossy());
    assert_eq!(normal(&python), normal(&scratch.0.join("env/bin/python")));
}

/// A path with `.` and `..` taken out by the words, to compare two spellings
/// of one place.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            part => out.push(part),
        }
    }
    out
}

#[test]
fn an_imported_path_reads_from_the_file_declaring_it() {
    let scratch = Scratch::new("import-paths");
    scratch.config("", COMMON);
    let own = scratch.config("data/c1", "schema_version = 1\nimport = \"../..\"\n");
    let config = config::load(&own).unwrap();
    let output = config.resolve(&config.export("all").unwrap().output.to_string_lossy());
    assert_eq!(
        normal(&output),
        normal(&scratch.0.join("exports/data/c1/all.csv"))
    );
    let directory = config.query("tall").unwrap().directory.clone().unwrap();
    assert_eq!(
        normal(&config.resolve(&directory.to_string_lossy())),
        normal(&scratch.0.join("data"))
    );
    assert_eq!(
        config.collection().files,
        ["../../images/data/c1/{name}*", "photos/{name}*"]
    );
    // Patterns on the collection's own files, imported as written.
    assert_eq!(config.collection().exclude, ["draft-*"]);
}

#[test]
fn collection_stands_for_the_collection_folder() {
    let scratch = Scratch::new("collection-placeholder");
    let root = scratch.config("", COMMON);
    // In the root's own file, the collection is its own folder.
    let config = config::load(&root).unwrap();
    assert_eq!(
        config.collection().files,
        ["images/{name}*", "photos/{name}*"]
    );
    assert_eq!(
        config.export("all").unwrap().output,
        PathBuf::from("exports/all.csv")
    );
    // From the root, `data/c1`, then read from the collection.
    let own = scratch.config("data/c1", "schema_version = 1\nimport = \"../..\"\n");
    let config = config::load(&own).unwrap();
    assert_eq!(config.collection().files[0], "../../images/data/c1/{name}*");
}

#[test]
fn each_declaration_says_where_it_comes_from() {
    use config::{DeclarationKind, DeclaredIn};
    let scratch = Scratch::new("import-origin");
    let root = scratch.config("", COMMON);
    let own = scratch.config(
        "c1",
        "schema_version = 1\nimport = \"..\"\n[profile.physical]\nsort = [\"name\"]\n\
         [profile.own]\ncolumns = [{field = \"name\"}]\n",
    );
    let config = config::load(&own).unwrap();
    let root = dunce::canonicalize(root).unwrap();
    assert_eq!(
        config.origin(DeclarationKind::Profile, "own"),
        DeclaredIn::Local
    );
    assert_eq!(
        config.origin(DeclarationKind::Profile, "physical"),
        DeclaredIn::Over(root.clone())
    );
    assert_eq!(
        config.origin(DeclarationKind::Query, "tall"),
        DeclaredIn::Imported(root.clone())
    );
    assert_eq!(
        config.origin(DeclarationKind::Figure, "collars"),
        DeclaredIn::Imported(root)
    );
}

#[test]
fn a_copy_of_an_imported_declaration_is_kept_for_validate() {
    let scratch = Scratch::new("import-copies");
    let root = scratch.config("", COMMON);
    let own = scratch.config(
        "c1",
        "schema_version = 1\nimport = \"..\"\n\
         [property.collar]\nunit = \"cL\"\nsymbol = \"h\"\n\
         [query.tall]\nfilter = \"collar > 2\"\ndirectory = \"elsewhere\"\n",
    );
    let config = config::load(&own).unwrap();
    let keys: Vec<&str> = config
        .copies()
        .iter()
        .map(|copy| copy.key.as_str())
        .collect();
    assert_eq!(keys, ["[property.collar]", "[query.tall] filter"]);
    let root = dunce::canonicalize(root).unwrap();
    assert!(config.copies().iter().all(|copy| copy.from == root));
}
