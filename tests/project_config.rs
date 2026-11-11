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

[property.porosity]
symbol = "Bx"
symbol_math = "\\porosity"
precision = ".4f"
unit = "g/L"

[property."ageing.rate"]
symbol_math = "r_{\\mathrm{warm}}"

[property."ageing_cold.rate"]
symbol_math = "r_{\\mathrm{cold}}"

[style.math]
property = '{value} \pm {uncertainty} {unit}'

[query.approved]
filter = 'status == "approved"'
directory = "transport/"

[profile.ductilities]
sort = ["alloy", "-ductility"]
columns = [
  {field = "alloy"},
  {field = "ductility", label = "Ductility", precision = ".4f"},
]

[export.approved_ductility]
profile = "ductilities"
format = "csv"
output = "generated/approved-ductility.tsv"
"#;

// ------------------------------------------------------------------ lookup

#[test]
fn finds_nearest_upward() {
    // A nested directory uses the closest configuration.
    let scratch = Scratch::new("nearest");
    scratch.config("", "schema_version = 1\n");
    let inner = scratch.config("casting", "schema_version = 1\n");
    let deep = scratch.dir("casting/runs");

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
        "schema_version = 1\n[collection]\nrecursive = false\ninclude = [\"*.sample.md\"]\n",
    );
    scratch.config("casting", "schema_version = 1\n");
    let inner = config::load_for(&scratch.dir("casting")).unwrap().unwrap();
    // The parent said not recursive; the nearest one says nothing, so the
    // default applies rather than the parent's value.
    assert!(inner.collection().recursive);
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
         [profile.good]\ncolumns = [{field = \"diameter\"}]\n\
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
    let error = loaded.profile("ductilites").unwrap_err();
    let ConfigError::UnknownProfile {
        suggestion,
        available,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(suggestion.as_deref(), Some("ductilities"));
    assert_eq!(available, &["ductilities"]);
}

#[test]
fn duplicate_field_in_a_profile_is_refused() {
    // Naming the field and the profile.
    let scratch = Scratch::new("dup-field");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\n\
         columns = [{field = \"diameter\"}, {field = \"diameter\", label = \"Again\"}]\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    assert!(error.contains("'p' declares 'diameter' twice"), "{error}");
}

#[test]
fn duplicate_header_is_refused() {
    // Naming both fields that collided, because nothing could tell the two
    // columns apart in the exported file.
    let scratch = Scratch::new("dup-header");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\n\
         columns = [{field = \"diameter\", header = \"m\"}, {field = \"ductility\", header = \"m\"}]\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    // The emitted name, not the declared one: `m_value` is what would appear
    // twice in the exported file, and naming that is what a reader can act on.
    assert!(
        error.contains("'p' would emit the header 'm_value' twice, from diameter and ductility"),
        "{error}"
    );
}

#[test]
fn empty_label_is_refused() {
    // Its own message: omit the key to fall back to the field, rather than
    // naming it nothing.
    let scratch = Scratch::new("empty-label");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\", label = \"\"}]\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    assert!(
        error.contains("'p' declares an empty name for 'diameter'"),
        "{error}"
    );
}

#[test]
fn an_export_naming_an_undeclared_profile_is_refused() {
    // At load, with the available names.
    let scratch = Scratch::new("export-profile");
    let path = scratch.config(
        "",
        "schema_version = 1\n[profile.ductilities]\ncolumns = [{field = \"diameter\"}]\n\
         [export.e]\nprofile = \"ductilites\"\nformat = \"csv\"\noutput = \"out.csv\"\n",
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
    assert_eq!(suggestion.as_deref(), Some("ductilities"));
    assert_eq!(available, &["ductilities"]);
}

#[test]
fn an_export_may_name_a_query() {
    // Read into the target, and an undeclared one refused at load.
    let scratch = Scratch::new("export-query");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.light]\nfilter = \"diameter < 12\"\n\
         [profile.ductilities]\ncolumns = [{field = \"diameter\"}]\n\
         [export.e]\nprofile = \"ductilities\"\nformat = \"csv\"\noutput = \"out.csv\"\nquery = \"light\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(
        config.table_export("e").unwrap().query.as_deref(),
        Some("light")
    );
    let broken = scratch.config(
        "b",
        "schema_version = 1\n[query.light]\nfilter = \"diameter < 12\"\n\
         [profile.ductilities]\ncolumns = [{field = \"diameter\"}]\n\
         [export.e]\nprofile = \"ductilities\"\nformat = \"csv\"\noutput = \"out.csv\"\nquery = \"lighr\"\n",
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
    // `[property.porosity]` supplies the symbol and precision no file repeats.
    let scratch = Scratch::new("property");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    let porosity = loaded.property("porosity").unwrap();
    assert_eq!(porosity.symbol.as_deref(), Some("Bx"));
    assert_eq!(
        porosity
            .symbol_variants
            .get("symbol_math")
            .map(String::as_str),
        Some("\\porosity")
    );
    assert_eq!(porosity.unit.as_deref(), Some("g/L"));
    assert_eq!(
        porosity.precision,
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
    let standard = loaded.property("ageing.rate").unwrap();
    let cold = loaded.property("ageing_cold.rate").unwrap();
    assert_ne!(
        standard.symbol_variants.get("symbol_math"),
        cold.symbol_variants.get("symbol_math")
    );
    assert!(loaded.property("ageing").is_none(), "the key nested");
}

#[test]
fn a_table_style_naming_nothing_is_refused_as_the_file_loads() {
    // `table = "fancy"` passed the load, stopped the first table drawn, naming
    // the file importing it, and every other command used the configuration.
    let scratch = Scratch::new("table-style");
    let path = scratch.config("", FULL);
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.render().table.as_deref(), Some("boxed"));
    let odd = scratch.config("odd", "schema_version = 1\n[render]\ntable = \"carved\"\n");
    let error = config::load(&odd).unwrap_err().to_string();
    assert!(
        error.contains(".samplekitrc:3: [render] table is plain or boxed, not 'carved'"),
        "{error}"
    );
    // Imported, it is the imported file's, named with its line.
    let importing = scratch.config("odd/inner", "schema_version = 1\nimport = \"..\"\n");
    let error = config::load(&importing).unwrap_err().to_string();
    assert!(
        error
            .replace('\\', "/")
            .contains("odd/.samplekitrc:3: [render] table"),
        "{error}"
    );
    // And a project declaring nothing declares no style.
    let bare = scratch.config("bare-table", "schema_version = 1\n");
    assert!(config::load(&bare).unwrap().render().table.is_none());
}

#[test]
fn an_error_of_a_declaration_names_its_file_and_line() {
    // Without them, three levels of import left the reader to guess which
    // file to correct.
    let scratch = Scratch::new("error-lines");
    for (name, body, said) in [
        (
            "identify",
            "schema_version = 1\n\n[render]\nidentify = \"path\"\n",
            ":4: [render] identify",
        ),
        (
            "style",
            "schema_version = 1\n[render]\nstyle = \"nope\"\n",
            ":3: [render] style",
        ),
        (
            "profile",
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"radius\"},\n  {field = \"radius\"}]\n",
            ":3: 'p' declares 'radius' twice",
        ),
        (
            "sort",
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"radius\"}]\nsort = [\"radius\", \"-radius\"]\n",
            ":2: 'p': sort names 'radius' twice",
        ),
        (
            "group",
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"radius\"}]\ngroup = [\"s\", \"s\"]\n",
            "group names 's' twice",
        ),
        (
            "label",
            "schema_version = 1\n[profile.p]\ncolumns = [{field = \"a\", label = \"x\"}, {field = \"b\", label = \"x\"}]\n",
            "two columns are labelled 'x'",
        ),
    ] {
        let path = scratch.config(name, body);
        let error = config::load(&path).unwrap_err().to_string();
        assert!(error.contains(said), "{name}: {error}");
        assert!(error.contains(".samplekitrc"), "{name}: {error}");
    }
    // A file importing another still says its own version.
    let versionless = scratch.config("versionless", "import = \"../identify\"\n");
    let error = config::load(&versionless).unwrap_err().to_string();
    assert!(error.contains("has no schema_version"), "{error}");
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
    let resolved = loaded.resolved(&bare, "porosity").unwrap();
    assert_eq!(resolved.symbol.as_deref(), Some("\\porosity"));
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
    let file_wins = loaded.resolved(&written, "porosity").unwrap();
    assert_eq!(
        file_wins.precision.as_ref().map(|p| p.value().as_str()),
        Some(".1f")
    );
    // The symbol is the exception, and not a broken rule: under `math` the
    // declared `symbol_math` is the math literal for this quantity, and a file
    // writing a plain `r` says nothing about it.
    assert_eq!(file_wins.symbol.as_deref(), Some("\\porosity"));
    // Under `plain` the file's own symbol wins, as it always did.
    let plain = scratch.config(
        "plain-symbol",
        "schema_version = 1\n[property.porosity]\nsymbol = \"Bx\"\n",
    );
    assert_eq!(
        config::load(&plain)
            .unwrap()
            .resolved(&written, "porosity")
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
    let resolved = loaded.resolved(&presentation, "porosity").unwrap();
    assert_eq!(resolved.unit.as_deref(), Some("g/L"));
    assert!(resolved.property.is_none());

    let math = scratch.config(
        "math",
        "schema_version = 1\n[render]\nstyle = \"math\"\n[style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n\
         [unit.\"g/L\"]\nplain = \"g/L\"\nmath = \"\\\\mathrm{g}\"\n",
    );
    let under_math = config::load(&math)
        .unwrap()
        .resolved(&presentation, "porosity")
        .unwrap();
    assert_eq!(under_math.unit.as_deref(), Some("\\mathrm{g}"));
    // The style's property.
    assert_eq!(
        under_math.property.as_deref(),
        Some("{value} \\pm {uncertainty} {unit}")
    );

    // An undeclared spelling renders as itself: it is already legible.
    let itself = Presentation {
        unit: Some("kg/hL".to_string()),
        symbol: None,
        precision: None,
    };
    assert_eq!(
        loaded
            .resolved(&itself, "porosity")
            .unwrap()
            .unit
            .as_deref(),
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
        "schema_version = 1\n[style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n",
    );
    let loaded = config::load(&path).unwrap();
    let error = loaded
        .resolved_as(&Presentation::default(), "porosity", Some("mathh"))
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("mathh"), "{message}");
    assert!(message.contains("math"), "{message}");
}

#[test]
fn an_explicit_render_style_overrides_without_mutating() {
    let scratch = Scratch::new("transient-style");
    let path = scratch.config(
        "",
        "schema_version = 1\n[render]\nstyle = \"plain\"\n\
         [style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n\
         [unit.lintner]\nplain = \"°L\"\nmath = \"\\\\Omega\"\n",
    );
    let loaded = config::load(&path).unwrap();
    let presentation = Presentation {
        unit: Some("lintner".to_string()),
        symbol: None,
        precision: None,
    };

    let math = loaded
        .resolved_as(&presentation, "tension", Some("math"))
        .unwrap();
    assert_eq!(math.unit.as_deref(), Some("\\Omega"));
    assert_eq!(
        math.property.as_deref(),
        Some("{value} \\pm {uncertainty} {unit}")
    );

    let stored = loaded.resolved(&presentation, "tension").unwrap();
    assert_eq!(stored.unit.as_deref(), Some("°L"));
    assert!(stored.property.is_none());
    assert_eq!(loaded.render().style.as_deref(), Some("plain"));
}

#[test]
fn render_style_names_start_with_plain() {
    let scratch = Scratch::new("style-names");
    let path = scratch.config(
        "",
        "schema_version = 1\n[style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n\
         [style.ascii]\nproperty = '{value} +/- {uncertainty} {unit}'\n",
    );
    let loaded = config::load(&path).unwrap();
    // The given styles, then the project's in the order it writes them.
    assert_eq!(
        loaded.render_style_names(),
        ["plain", "html", "latex", "markdown", "math", "ascii"]
    );
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
        "schema_version = 1\n\n[property.porosity]\nunit = \"g/L\"\n\n[property.diameter]\nunit = \"g\"\n\n[property.hv]\nprecision = \".2f\"\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.property_names(), ["porosity", "diameter", "hv"]);
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
fn python_is_kept_without_a_model() {
    let scratch = Scratch::new("python-alone");
    let path = scratch.config(
        "project",
        "schema_version = 1\n[model]\npython = \".venv\"\n",
    );
    let loaded = config::load(&path).unwrap();
    assert!(loaded.model().is_none());
    assert_eq!(loaded.python(), Some(std::path::Path::new(".venv")));
}

#[test]
fn a_render_style_naming_nothing_declared_is_refused_at_load() {
    let scratch = Scratch::new("render-style-at-load");
    let path = scratch.config(
        "",
        "schema_version = 1\n[render]\nstyle = \"fancy\"\n[style.math]\nproperty = '{value} \\pm {uncertainty} {unit}'\n",
    );
    let Err(error) = config::load(&path) else {
        panic!("a render style naming nothing was accepted");
    };
    let message = error.to_string();
    assert!(message.contains("fancy"), "{message}");
    assert!(message.contains("math"), "{message}");
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
fn the_render_precision_is_every_undeclared_numbers() {
    // `[render] precision` was read and never applied, and a specifier that
    // `[property.*]` refuses was accepted there in silence.
    let scratch = Scratch::new("render-precision");
    let path = scratch.config(
        "",
        "schema_version = 1\n[render]\nprecision = \".1e\"\n[property.porosity]\nprecision = \".2f\"\n",
    );
    let loaded = config::load(&path).unwrap();
    let bare = Presentation {
        unit: None,
        symbol: None,
        precision: None,
    };
    let of = |quantity: &str| {
        loaded
            .resolved(&bare, quantity)
            .unwrap()
            .precision
            .map(|precision| precision.value().as_str().to_string())
    };
    assert_eq!(of("pressure").as_deref(), Some(".1e"));
    assert_eq!(of("porosity").as_deref(), Some(".2f"));
    let refused = scratch.config("", "schema_version = 1\n[render]\nprecision = \"3f\"\n");
    let Err(error) = config::load(&refused) else {
        panic!("an invalid [render] precision was accepted");
    };
    assert!(error.to_string().contains("render"), "{error}");
}

#[test]
fn an_invalid_column_precision_is_refused_at_load() {
    // A profile's or a view's column, and a specifier inside a template, are
    // checked as a `[property.*]` precision is — accepted, each was dropped at
    // the first rendering and the default shown in its place.
    let scratch = Scratch::new("column-precision");
    for (body, names) in [
        (
            "[profile.latex]\ncolumns = [{ field = \"porosity\", precision = \".3z\" }]\n",
            ["profile.latex", "'porosity'", ".3z"],
        ),
        (
            "[profile.latex]\ncolumns = [{ field = \"porosity\", template = \"\\\\num{{value:.3z}}\" }]\n",
            ["profile.latex", "'porosity'", "{value:.3z}"],
        ),
        (
            "[profile.latex]\ncolumns = [{ field = \"porosity\", template = \"{unit:.3f}\" }]\n",
            ["profile.latex", "'porosity'", "takes no specifier"],
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
        "schema_version = 1\n[profile.latex]\ncolumns = [{ field = \"porosity\", \
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
        "schema_version = 1\n[profile.diameters]\ncolumns = [{ field = \"diameter\" }]\n\
         [export.diameters]\nprofile = \"diameters\"\nformat = \"csv\"\n\
         output = \"out/diameters.csv\"\nfilename = true\npath = true\n",
    );
    let config = config::load(&path).unwrap();
    let target = config.table_export("diameters").unwrap();
    assert!(target.filename);
    assert!(target.path);
    // The destination is `output`, and the flag beside it names a column.
    assert!(target.output.ends_with("diameters.csv"));

    let refused = scratch.config(
        "",
        "schema_version = 1\n[profile.diameters]\ncolumns = [{ field = \"diameter\" }]\n\
         [export.diameters]\nprofile = \"diameters\"\nformat = \"csv\"\n\
         output = \"out/diameters.csv\"\ninclude_filename = true\n",
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

// ------------------------------------------------------------------ figures

#[test]
fn a_figure_declares_its_kind_its_axes_its_group_and_its_query() {
    // What a figure draws, read in declaration order, and a kind left out is a
    // scatter.
    let scratch = Scratch::new("figure");
    let path = scratch.config(
        "",
        "schema_version = 1\n[query.light]\nfilter = \"diameter < 12\"\n\
         [figure.vfi]\nkind = \"line\"\nx = \"period\"\ny = \"vfi\"\ngroup = \"alloy\"\nquery = \"light\"\n\
         [figure.bare]\nx = \"diameter\"\ny = \"porosity\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.figure_names(), ["vfi", "bare"]);
    let vfi = config.figure("vfi").unwrap();
    assert_eq!(vfi.kind, FigureKind::Line);
    assert_eq!((vfi.x.as_str(), vfi.y.as_str()), ("period", "vfi"));
    assert_eq!(vfi.group.as_deref(), Some("alloy"));
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
        "schema_version = 1\n[query.light]\nfilter = \"diameter < 12\"\n\
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
    let scratch = Scratch::new("tui-colors");
    let path = scratch.config(
        "",
        "schema_version = 1\n[colors]\noutdated = \"#ff8800\"\nmuted = \"grey\"\n\
         table = \"dark-gray\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.color("outdated"), Some("#ff8800"));
    // British and American spellings of grey are one colour.
    assert_eq!(config.color("muted"), Some("grey"));
    assert_eq!(config.color("table"), Some("dark-gray"));
    assert_eq!(config.color("failed"), Some("red"));
    // A role nobody draws, and a colour nothing can, are refused at load.
    let role = scratch.config("r", "schema_version = 1\n[colors]\nstaled = \"red\"\n");
    let error = config::load(&role).unwrap_err().to_string();
    assert!(
        error.contains("no role 'staled'") && error.contains("outdated"),
        "{error}"
    );
    let colour = scratch.config("c", "schema_version = 1\n[colors]\noutdated = \"orange\"\n");
    let error = config::load(&colour).unwrap_err().to_string();
    assert!(
        error.contains("not a colour") && error.contains("#rrggbb"),
        "{error}"
    );
    let key = scratch.config("k", "schema_version = 1\n[workbench]\ncolours = {}\n");
    assert!(config::load(&key).is_err());
    // A value and a unit are roles too, and the TUI's own section is no more.
    assert_eq!(config.color("value"), Some("default"));
    assert_eq!(config.color("unit"), Some("cyan"));
    let tui = scratch.config(
        "t",
        "schema_version = 1\n[tui.colors]\noutdated = \"red\"\n",
    );
    assert!(config::load(&tui).is_err());
}

#[test]
fn a_project_names_its_theme_beside_its_roles() {
    let scratch = Scratch::new("colors-theme");
    let path = scratch.config(
        "",
        "schema_version = 1\n[colors]\ntheme = \"nord\"\noutdated = \"#ff8800\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.theme(), Some("nord"));
    // A name, not a role: only `outdated` is laid over the theme.
    assert_eq!(
        config.colors_named().keys().collect::<Vec<_>>(),
        ["outdated"]
    );
    assert_eq!(config.color("theme"), None);
    // Any name loads: one's own theme, named by a project shared, is not
    // its reader's, which the TUI says.
    let own = scratch.config("o", "schema_version = 1\n[colors]\ntheme = \"my-own\"\n");
    assert_eq!(config::load(&own).unwrap().theme(), Some("my-own"));
    let number = scratch.config("n", "schema_version = 1\n[colors]\ntheme = 3\n");
    assert!(config::load(&number).is_err());
    // None named, none.
    let none = scratch.config("x", "schema_version = 1\n");
    assert_eq!(config::load(&none).unwrap().theme(), None);
}

#[test]
fn the_background_and_the_text_are_roles_left_to_the_terminal() {
    let scratch = Scratch::new("colors-background");
    let path = scratch.config(
        "",
        "schema_version = 1\n[colors]\nbackground = \"#1e1e2e\"\n",
    );
    let config = config::load(&path).unwrap();
    assert_eq!(config.color("background"), Some("#1e1e2e"));
    assert_eq!(config.color("foreground"), Some("default"));
    let plain = scratch.config("p", "schema_version = 1\n");
    assert_eq!(
        config::load(&plain).unwrap().color("background"),
        Some("default")
    );
    let navy = scratch.config("n", "schema_version = 1\n[colors]\nforeground = \"navy\"\n");
    let error = config::load(&navy).unwrap_err().to_string();
    assert!(
        error.contains("not a colour") && error.contains("light-blue"),
        "{error}"
    );
}

#[test]
fn a_theme_file_is_its_base_and_its_colors() {
    let path = Path::new("/themes/mine.toml");
    let file = config::theme_file(
        path,
        "# Mine: nord, its accent pinker.\nbase = \"nord\"\n\n[colors]\naccent = \"#ff79c6\"\n",
    )
    .unwrap();
    assert_eq!(file.base.as_deref(), Some("nord"));
    assert_eq!(file.colors["accent"], "#ff79c6");
    let alone = config::theme_file(path, "[colors]\nbackground = \"#000000\"\n").unwrap();
    assert_eq!(alone.base, None);
    // Each refused, naming the file.
    for (text, said) in [
        ("[colors]\nstaled = \"red\"\n", "no role 'staled'"),
        ("[colors]\naccent = \"pinkish\"\n", "not a colour"),
        ("base = \"nord\"\nname = \"mine\"\n", "name"),
        ("[colors]\ntheme = \"nord\"\n", "base = \"NAME\""),
        ("[colors\n", ""),
    ] {
        let error = config::theme_file(path, text).unwrap_err().to_string();
        assert!(
            error.contains("mine.toml") && error.contains(said),
            "{text}: {error}"
        );
    }
}

#[test]
fn the_themes_folder_is_in_the_configuration_folder() {
    let scratch = Scratch::new("config-dir");
    // SAFETY: no other test of this file reads the configuration folder.
    unsafe { std::env::set_var("SAMPLEKIT_CONFIG_DIR", &scratch.0) };
    assert_eq!(
        config::config_directory().as_deref(),
        Some(scratch.0.as_path())
    );
    assert_eq!(config::themes_folder(), Some(scratch.0.join("themes")));
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
    let profile = loaded.profile("ductilities").unwrap();
    assert_eq!(profile.sort, ["alloy", "-ductility"]);
    let export = loaded.table_export("approved_ductility").unwrap();
    assert_eq!(export.format, Format::Csv);
    assert_eq!(
        loaded.style("math").unwrap().property.as_deref(),
        Some("{value} \\pm {uncertainty} {unit}")
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
         [profile.zulu]\ncolumns = [{field = \"diameter\"}]\n\
         [profile.alpha]\ncolumns = [{field = \"diameter\"}]\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.profile_names(), ["zulu", "alpha"]);
}

#[test]
fn a_profile_declares_its_groups() {
    // `group` is a list of fields, as `sort` is, held in its order.
    let scratch = Scratch::new("profile-group");
    let path = scratch.config(
        "",
        "schema_version = 1\n\
         [profile.grouped]\ncolumns = [{field = \"diameter\"}]\ngroup = [\"alloy\", \"status\"]\n\
         [profile.flat]\ncolumns = [{field = \"diameter\"}]\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(
        loaded.profile("grouped").unwrap().group,
        ["alloy", "status"]
    );
    assert!(loaded.profile("flat").unwrap().group.is_empty());
    // One text is refused as `sort`'s is: a list is asked.
    let text = scratch.config(
        "t",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\"}]\ngroup = \"alloy\"\n",
    );
    let error = config::load(&text).unwrap_err().to_string();
    assert!(error.contains("where a list belongs"), "{error}");
    let sort = scratch.config(
        "s",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\"}]\nsort = \"alloy\"\n",
    );
    let sorted = config::load(&sort).unwrap_err().to_string();
    assert!(sorted.contains("where a list belongs"), "{sorted}");
    // A field left empty groups by nothing anyone can name.
    let empty = scratch.config(
        "e",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\"}]\ngroup = [\"\"]\n",
    );
    let error = config::load(&empty).unwrap_err().to_string();
    assert!(error.contains("empty field"), "{error}");
    // A saved query is a filter, and takes no group.
    let query = scratch.config(
        "q",
        "schema_version = 1\n[query.q]\nfilter = \"diameter > 1\"\ngroup = [\"alloy\"]\n",
    );
    let error = config::load(&query).unwrap_err().to_string();
    assert!(error.contains("unknown key `group`"), "{error}");
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
files = ["images/{project}/{name}*", "{project}/photos/{name}*"]
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
output = "exports/{project}/all.csv"

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
fn a_file_imported_changed_is_read_again() {
    // What is kept of a configuration read is kept by its text and by the
    // text of every file it imports, the farthest too.
    let scratch = Scratch::new("import-changed");
    let root = scratch.config("", COMMON);
    scratch.config("data", "schema_version = 1\nimport = \"..\"\n");
    let own = scratch.config("data/c1", "schema_version = 1\nimport = \"..\"\n");
    let unit = || {
        config::load(&own)
            .unwrap()
            .property("collar")
            .unwrap()
            .unit
            .clone()
    };
    assert_eq!(unit().as_deref(), Some("cL"));
    let common = std::fs::read_to_string(&root).unwrap();
    std::fs::write(&root, common.replace("\"cL\"", "\"mL\"")).unwrap();
    assert_eq!(unit().as_deref(), Some("mL"));
    std::fs::remove_file(&root).unwrap();
    assert!(config::load(&own).is_err());
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
    let nothing = scratch.config("c3", "schema_version = 1\nimport = \"../nowhere\"\n");
    let said = config::load(&nothing).unwrap_err().to_string();
    assert!(said.contains("is no folder nor file"), "{said}");
    let number = scratch.config("c4", "schema_version = 1\nimport = 3\n");
    let said = config::load(&number).unwrap_err().to_string();
    assert!(
        said.contains("import names a folder or a file, or a list"),
        "{said}"
    );
}

#[test]
fn an_import_takes_a_file_and_a_list() {
    // A project's settings in files of their own, the main file importing the
    // root and them; a later one wins over an earlier one, and the importing
    // file over all.
    let scratch = Scratch::new("import-list");
    scratch.config(
        "",
        "schema_version = 1\n[render]\nprecision = \".1f\"\ntable = \"boxed\"\n\
         [unit.\"g\"]\nplain = \"gram\"\n",
    );
    let project = scratch.dir("p");
    fs::write(
        project.join("figures.samplekitrc"),
        "schema_version = 1\n[matplotlib]\n\"axes.grid\" = true\nstyle = \"look.mplstyle\"\n\
         [render]\nprecision = \".2f\"\n",
    )
    .unwrap();
    fs::write(
        project.join("units.samplekitrc"),
        "schema_version = 1\n[render]\nprecision = \".3f\"\n",
    )
    .unwrap();
    let own = scratch.config(
        "p",
        "schema_version = 1\nimport = [\"..\", \"figures.samplekitrc\", \"units.samplekitrc\"]\n\
         [render]\ntable = \"plain\"\n",
    );
    let config = config::load(&own).unwrap();
    assert_eq!(config.matplotlib()["axes.grid"].as_bool(), Some(true));
    let style = config.matplotlib()["style"].as_array().unwrap()[0]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        normal(Path::new(&style)),
        normal(&project.join("look.mplstyle"))
    );
    // The last listed over the ones before; the importing file over all.
    let precision = format!("{:?}", config.render().precision);
    assert!(precision.contains(".3f"), "{precision}");
    assert_eq!(config.render().table.as_deref(), Some("plain"));
    let shown = config.imports();
    let names: Vec<String> = shown
        .iter()
        .map(|file| file.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["units.samplekitrc", "figures.samplekitrc", ".samplekitrc"]
    );
    // A file listed twice is merged once.
    let twice = scratch.config("q", "schema_version = 1\nimport = [\"..\", \"..\"]\n");
    assert_eq!(config::load(&twice).unwrap().imports().len(), 1);
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
    let output = config.resolve(&config.table_export("all").unwrap().output.to_string_lossy());
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
        config.table_export("all").unwrap().output,
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

#[test]
fn a_configuration_error_says_what_to_write() {
    // Messages that named no key (`duplicate key`), no near name (`imports`,
    // `yelow`), a Rust type (`expected u32`), no declaration (`no profile
    // named 'nope'`), or not what to write where the version is missing.
    let scratch = Scratch::new("error-words");
    for (body, expected) in [
        ("imports = \"..\"\n", "did you mean `import`?"),
        (
            "[property.radius]\nsymbol = \"a\"\nsymbol = \"b\"\n",
            "duplicate key, in `symbol = \"b\"`",
        ),
        (
            "[collection]\nrecursive = \"yes\"\n",
            "in `recursive = \"yes\"`",
        ),
        ("[colors]\noutdated = \"yelow\"\n", "did you mean 'yellow'?"),
        ("[colors]\noutdate = \"red\"\n", "did you mean 'outdated'?"),
        (
            "[profile.overview]\ncolumns = [{ field = \"name\" }]\n[export.e]\nprofile = \"nope\"\nformat = \"csv\"\noutput = \"x.csv\"\n",
            "'export.e': no profile named 'nope'",
        ),
    ] {
        let path = scratch.config("", &format!("schema_version = 1\n{body}"));
        let Err(error) = config::load(&path) else {
            panic!("{body} was accepted");
        };
        let message = error.to_string();
        assert!(message.contains(expected), "{expected}: {message}");
        assert!(!message.contains("u32"), "{message}");
    }
    let path = scratch.config("", "[render]\n");
    let Err(error) = config::load(&path) else {
        panic!("a configuration without a version was accepted");
    };
    assert!(
        error.to_string().contains("write schema_version = 1"),
        "{error}"
    );
}

#[test]
fn a_shared_folder_may_declare_only_the_interpreter() {
    // `[model] python` alone, in the folder above projects that import it,
    // was refused as `missing field 'path'` — and said to be an earlier
    // samplekit's — and every project importing it lost its configuration.
    let scratch = Scratch::new("python-alone");
    scratch.config(
        "",
        "schema_version = 1\n[model]\npython = \".venv/bin/python\"\n",
    );
    let bare = scratch.config("bare", "schema_version = 1\nimport = \"..\"\n");
    let loaded = config::load(&bare).unwrap();
    assert!(loaded.model().is_none());
    let modelled = scratch.config(
        "modelled",
        "schema_version = 1\nimport = \"..\"\n[model]\npath = \"model.py\"\n",
    );
    let loaded = config::load(&modelled).unwrap();
    let model = loaded.model().unwrap();
    assert!(model.python.as_ref().unwrap().ends_with(".venv/bin/python"));
}

#[test]
fn a_range_is_two_numbers_the_lower_first() {
    let scratch = Scratch::new("range");
    let path = scratch.config(
        "project",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\nrange = [0, 50]\n\
         [property.modulus]\nrange = [0.0, inf]\n",
    );
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.property("volume").unwrap().range, Some((0.0, 50.0)));
    assert_eq!(
        loaded.property("modulus").unwrap().range,
        Some((0.0, f64::INFINITY))
    );
    for (written, said) in [("[50, 0]", "is above"), ("[1]", "takes two")] {
        let path = scratch.config(
            "project",
            &format!("schema_version = 1\n[property.volume]\nrange = {written}\n"),
        );
        let refused = config::load(&path).unwrap_err().to_string();
        assert!(
            refused.contains("[property.volume] range") && refused.contains(said),
            "{refused}"
        );
    }
}

#[test]
fn a_text_format_is_declared_as_a_style_is() {
    // A style says how a table is a document: what it does not say is the
    // default; `latex`, `markdown` and `html` are given; a key it does not
    // know is refused at load.
    let scratch = Scratch::new("text-format");
    let path = scratch.config(
        "",
        "schema_version = 1\n[style.paper]\n\
         [style.list]\nrow = \"- {cells}\"\nheader = false\nproperty = \"{value} +- {uncertainty}\"\n",
    );
    let config = config::load(&path).unwrap();
    let list = config.text_format("list").unwrap();
    assert_eq!(list.row, "- {cells}");
    assert_eq!(list.header, None);
    assert_eq!(list.property.as_deref(), Some("{value} +- {uncertainty}"));
    assert_eq!(list.extension, "list");
    // A style of units and symbols alone writes no document.
    assert!(config.text_format("paper").is_none());
    assert_eq!(
        config.text_format_names(),
        ["html", "latex", "markdown", "list"]
    );
    assert_eq!(config.text_format("latex").unwrap().extension, "tex");
    let broken = scratch.config(
        "unknown-key",
        "schema_version = 1\n[style.list]\nrows = \"x\"\n",
    );
    let error = config::load(&broken).unwrap_err().to_string();
    assert!(error.contains("did you mean `row`"), "{error}");
}

#[test]
fn a_precision_refused_is_said_with_its_line() {
    // `[property.radius] precision: '.3x' …` named the section, never the line, as
    // a parse error of the same file does.
    let scratch = Scratch::new("precision-line");
    let path = scratch.config(
        "",
        "schema_version = 1\n\n[property.radius]\nsymbol = \"RADIUS\"\nprecision = \".3x\"\n\
         [property.notch]\nrange = [2, 1]\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    assert!(
        error.contains(".samplekitrc:5: [property.radius] precision"),
        "{error}"
    );
    fs::write(
        &path,
        "schema_version = 1\n[property.notch]\n\nrange = [2, 1]\n",
    )
    .unwrap();
    let error = config::load(&path).unwrap_err().to_string();
    assert!(
        error.contains(".samplekitrc:4: [property.notch] range"),
        "{error}"
    );
}

#[test]
fn history_shared_is_read_and_imported() {
    let scratch = Scratch::new("history-shared");
    let root = scratch.config("", "schema_version = 1\n[history]\nshared = true\n");
    assert_eq!(config::load(&root).unwrap().history_shared(), Some(true));
    // Imported as any key: a project below shares as the root does.
    let below = scratch.config("below", "schema_version = 1\nimport = \"..\"\n");
    assert_eq!(config::load(&below).unwrap().history_shared(), Some(true));
    let alone = scratch.config("alone", "schema_version = 1\n");
    assert_eq!(config::load(&alone).unwrap().history_shared(), None);
    let wrong = scratch.config("wrong", "schema_version = 1\n[history]\nshared = \"yes\"\n");
    assert!(config::load(&wrong).is_err());
}

#[test]
fn style_sheets_and_fonts_are_read_from_the_file_declaring_them() {
    // A sheet's name stays a name; a .mplstyle file and a font are paths, from
    // the file that declares them, an imported one's included.
    let scratch = Scratch::new("matplotlib-sheets");
    scratch.config(
        "",
        "schema_version = 1\n[matplotlib]\nstyle = [\"ggplot\", \"paper.mplstyle\"]\n\
         fonts = \"fonts/*.otf\"\n",
    );
    let below = scratch.config("sub", "schema_version = 1\nimport = \"..\"\n");
    let config = config::load(&below).unwrap();
    let root = below.parent().unwrap().parent().unwrap();
    let listed = |key: &str| -> Vec<String> {
        config.matplotlib()[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(listed("style")[0], "ggplot");
    assert_eq!(
        std::path::Path::new(&listed("style")[1]),
        root.join("paper.mplstyle")
    );
    assert_eq!(
        std::path::Path::new(&listed("fonts")[0]),
        root.join("fonts/*.otf")
    );
    let number = scratch.config("n", "schema_version = 1\n[matplotlib]\nstyle = 3\n");
    let error = config::load(&number).unwrap_err().to_string();
    assert!(
        error.contains("[matplotlib] style: a style sheet's name"),
        "{error}"
    );
}

#[test]
fn a_figure_carries_its_own_matplotlib_settings() {
    let scratch = Scratch::new("figure-matplotlib");
    let path = scratch.config(
        "",
        "schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n\
         matplotlib = { \"lines.marker\" = \"s\", font.size = 9 }\n",
    );
    let config = config::load(&path).unwrap();
    let own: Vec<&str> = config
        .figure("f")
        .unwrap()
        .matplotlib
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(own, ["lines.marker", "font.size"]);
    // Style sheets and fonts are the project's.
    let sheets = scratch.config(
        "s",
        "schema_version = 1\n[figure.f]\nx = \"a\"\ny = \"b\"\n\
         matplotlib = { style = \"ggplot\" }\n",
    );
    let error = config::load(&sheets).unwrap_err().to_string();
    assert!(error.contains("style is the project's"), "{error}");
}

#[test]
fn the_projects_folder_is_written_project() {
    // `{collection}` named the project's folder until 1.0.0-rc.2; kept, it
    // would make a folder of that name.
    let scratch = Scratch::new("old-placeholder");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nfiles = [\"{collection}/photos\"]\n",
    );
    let error = config::load(&path).unwrap_err().to_string();
    assert!(
        error.contains("{collection} is written {project} now"),
        "{error}"
    );
}
