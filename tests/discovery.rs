//! The tests of `discovery`.

use std::fs;
use std::path::{Path, PathBuf};

use samplekit::config::discovery::{self, SkipReason, Warning};
use samplekit::config::project_config;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-keg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    fn sample(&self, relative: &str) -> PathBuf {
        self.file(
            relative,
            "---\nschema_version: 1\nproperties:\n  malt: 1.0\n---\nA note.\n",
        )
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn config(&self, relative: &str, body: &str) -> PathBuf {
        let name = if relative.is_empty() {
            ".samplekitrc".to_string()
        } else {
            format!("{relative}/.samplekitrc")
        };
        self.file(&name, body)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The included file names, relative to the root.
fn names(collection: &discovery::Collection) -> Vec<String> {
    collection
        .paths
        .iter()
        .map(|path| {
            path.strip_prefix(&collection.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn discover(root: &Path, config: Option<&project_config::ProjectConfig>) -> discovery::Collection {
    discovery::discover(root, config).unwrap()
}

// ------------------------------------------------------------------ rules

#[test]
fn default_is_non_recursive() {
    // A sample directory commonly sits next to README.md and generated/, and a
    // default that swept them would make the common case surprising.
    let scratch = Scratch::new("non-recursive");
    scratch.sample("a.md");
    scratch.sample("runs/b.md");
    let found = discover(&scratch.0, None);
    assert_eq!(names(&found), ["a.md"]);
}

#[test]
fn recursive_includes_subdirectories() {
    let scratch = Scratch::new("recursive");
    scratch.sample("a.md");
    scratch.sample("runs/b.md");
    let path = scratch.config("", "schema_version = 1\n[collection]\nrecursive = true\n");
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.md", "runs/b.md"]);
}

#[test]
fn a_path_pattern_is_relative_to_its_configuration() {
    // `exclude = ["sub/*.md"]` held for `samplekit .` and not for
    // `samplekit sub`, which matched it from `sub`.
    let scratch = Scratch::new("pattern-base");
    scratch.sample("sub/a.md");
    scratch.sample("sub/sub/b.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nrecursive = true\nexclude = [\"sub/*.md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    assert_eq!(
        names(&discover(&scratch.0, Some(&config))),
        ["sub/sub/b.md"]
    );
    let below = discover(&scratch.0.join("sub"), Some(&config));
    assert_eq!(names(&below), ["sub/b.md"]);
    assert!(below.skipped.iter().any(|skipped| {
        skipped.path.ends_with("sub/a.md")
            && skipped.reason
                == SkipReason::Excluded {
                    pattern: "sub/*.md".to_string(),
                }
    }));
}

#[test]
fn include_patterns_select_by_filename() {
    // A pattern without `/` matches the name.
    let scratch = Scratch::new("include-name");
    scratch.sample("a.sample.md");
    scratch.sample("b.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\ninclude = [\"*.sample.md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.sample.md"]);
}

#[test]
fn include_patterns_with_slash_match_relative_paths() {
    // And not the bare filename.
    let scratch = Scratch::new("include-path");
    scratch.sample("runs/a.md");
    scratch.sample("a.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nrecursive = true\ninclude = [\"runs/*.md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["runs/a.md"]);
}

#[test]
fn exclude_beats_include() {
    // A file matching both is excluded, and the pattern is named.
    let scratch = Scratch::new("exclude");
    scratch.sample("a.md");
    scratch.sample("draft-b.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nexclude = [\"draft-*\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.md"]);
    assert!(
        found.skipped.iter().any(|s| matches!(
            &s.reason,
            SkipReason::Excluded { pattern } if pattern == "draft-*"
        )),
        "{:?}",
        found.skipped
    );
}

// ------------------------------------------------------------- membership

#[test]
fn readme_is_skipped_not_reported_as_malformed() {
    // With `NoFrontmatter`.
    let scratch = Scratch::new("readme");
    scratch.sample("a.md");
    scratch.file("README.md", "# Notes\n\nNothing structured here.\n");
    let found = discover(&scratch.0, None);
    assert_eq!(names(&found), ["a.md"]);
    let skipped = found.skipped.iter().find(|s| s.path.ends_with("README.md"));
    assert_eq!(skipped.map(|s| &s.reason), Some(&SkipReason::NoFrontmatter));
}

#[test]
fn malformed_sample_is_skipped_with_a_reason() {
    // And the reason carries the parse message.
    let scratch = Scratch::new("malformed");
    scratch.file("bad.md", "---\nproperties: [unclosed\n---\nnote\n");
    let found = discover(&scratch.0, None);
    assert!(found.paths.is_empty());
    let SkipReason::Malformed { message } = &found.skipped[0].reason else {
        panic!("{:?}", found.skipped);
    };
    assert!(!message.is_empty());
}

#[test]
fn skipped_files_are_reported_not_discarded() {
    // The count of *candidates* equals included plus skipped.
    let scratch = Scratch::new("accounted");
    scratch.sample("a.md");
    scratch.sample("b.md");
    scratch.file("README.md", "# Notes\n");
    scratch.file("c.md", "---\nbroken: [\n---\n");
    let found = discover(&scratch.0, None);
    assert_eq!(found.paths.len() + found.skipped.len(), 4);
}

#[test]
fn an_unmatched_file_is_not_a_skipped_candidate() {
    // A figure beside the samples was never examined as one; reporting it
    // would bury the skips that matter. An *excluded* file is different,
    // because the author wrote the pattern.
    let scratch = Scratch::new("unmatched");
    scratch.sample("a.md");
    scratch.file("figure.png", "not a sample");
    scratch.file("a.md.bak-0", "a migration backup");
    let found = discover(&scratch.0, None);
    assert_eq!(names(&found), ["a.md"]);
    assert!(found.skipped.is_empty(), "{:?}", found.skipped);

    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nexclude = [\"draft-*\"]\n",
    );
    scratch.sample("draft-b.md");
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(found.skipped.len(), 1, "{:?}", found.skipped);
}

#[test]
fn discovery_does_not_fully_parse() {
    // A sample with a malformed table is still discovered, since the failure is
    // not structural to the file: dropping it here would remove a real sample
    // from a query for a reason the query never mentions.
    let scratch = Scratch::new("shallow");
    scratch.file(
        "a.md",
        "---\nschema_version: 1\ntables:\n  mashing:\n    index: nope\n    columns: {}\n    rows: []\n---\nnote\n",
    );
    let found = discover(&scratch.0, None);
    assert_eq!(names(&found), ["a.md"]);
    // And it is indeed refused when actually loaded.
    assert!(samplekit::format::document::load_sample(&found.paths[0]).is_err());
}

// ------------------------------------------------------------------ order

#[test]
fn order_is_deterministic() {
    // Two scans of the same directory agree, on any platform.
    let scratch = Scratch::new("deterministic");
    for name in ["c.md", "a.md", "b.md"] {
        scratch.sample(name);
    }
    let first = names(&discover(&scratch.0, None));
    assert_eq!(first, ["a.md", "b.md", "c.md"]);
    for _ in 0..5 {
        assert_eq!(names(&discover(&scratch.0, None)), first);
    }
}

#[test]
fn order_does_not_depend_on_creation_sequence() {
    // Files created in reverse order still sort the same. This is the defect
    // behind the reported "sort does nothing": the visible order was the
    // directory's, and nothing said so.
    let forward = Scratch::new("forward");
    for name in ["a.md", "b.md", "c.md"] {
        forward.sample(name);
    }
    let backward = Scratch::new("backward");
    for name in ["c.md", "b.md", "a.md"] {
        backward.sample(name);
    }
    assert_eq!(
        names(&discover(&forward.0, None)),
        names(&discover(&backward.0, None))
    );
}

// ------------------------------------------------------------- boundaries

#[test]
#[cfg(unix)]
fn symlinked_directory_is_not_followed() {
    // Including a link to an ancestor, which must terminate.
    let scratch = Scratch::new("symlink");
    scratch.sample("a.md");
    let inner = scratch.dir("inner");
    scratch.sample("inner/b.md");
    // A link pointing at an ancestor: following it would never end.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&scratch.0, inner.join("up")).unwrap();

    let path = scratch.config("", "schema_version = 1\n[collection]\nrecursive = true\n");
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.md", "inner/b.md"]);
}

#[test]
fn single_file_yields_a_collection_of_one() {
    // The file and directory paths are the same code path.
    let scratch = Scratch::new("single");
    let path = scratch.sample("a.md");
    scratch.sample("b.md");
    let found = discover(&path, None);
    assert_eq!(found.paths, [path]);
}

#[test]
fn missing_root_is_an_error() {
    // Unlike a missing individual file, which is a `Skipped`.
    let scratch = Scratch::new("missing");
    let error = discovery::discover(&scratch.0.join("nowhere"), None).unwrap_err();
    assert!(error.to_string().contains("does not exist"), "{error}");
}

// ---------------------------------------------------- crossing vintages

#[test]
fn crossing_into_another_configuration_warns() {
    // A subdirectory with its own `.samplekitrc` is reported, not silently
    // absorbed: there that is precisely where the template differs, so the
    // collection holds casks described by different models.
    let scratch = Scratch::new("mixed");
    scratch.sample("a.md");
    scratch.sample("other/b.md");
    let path = scratch.config("", "schema_version = 1\n[collection]\nrecursive = true\n");
    scratch.config("other", "schema_version = 1\n");
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));

    assert_eq!(names(&found), ["a.md", "other/b.md"]);
    let Some(Warning::MixedConfigurations { roots }) = found.warnings.first() else {
        panic!("{:?}", found.warnings);
    };
    assert_eq!(roots.len(), 2);
    // And it says which configuration each sample was found under.
    assert_eq!(found.configurations.len(), 2);
}

#[test]
fn one_configuration_warns_about_nothing() {
    // The ordinary case stays quiet.
    let scratch = Scratch::new("single-config");
    scratch.sample("a.md");
    scratch.sample("runs/b.md");
    let path = scratch.config("", "schema_version = 1\n[collection]\nrecursive = true\n");
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.md", "runs/b.md"]);
    assert!(found.warnings.is_empty(), "{:?}", found.warnings);
    assert!(found.configurations.is_empty());
}

#[test]
fn a_skip_reason_says_what_was_wrong() {
    // Four causes with four different fixes is why the type exists; a caller
    // that has to write those four sentences itself will write three of them.
    assert_eq!(
        SkipReason::Excluded {
            pattern: "draft-*".to_string()
        }
        .to_string(),
        "excluded by the pattern 'draft-*'"
    );
    assert!(
        SkipReason::NoFrontmatter
            .to_string()
            .contains("begins with a '---' line")
    );
    assert!(
        SkipReason::Unreadable {
            message: "permission denied".to_string()
        }
        .to_string()
        .contains("permission denied")
    );
    assert!(
        SkipReason::Malformed {
            message: "line 3".to_string()
        }
        .to_string()
        .contains("line 3")
    );
}

#[test]
fn explicit_rules_replace_the_declared_ones() {
    let scratch = Scratch::new("explicit-rules");
    scratch.sample("a.md");
    scratch.sample("b.sample.md");
    scratch.sample("c.sample.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n\n[collection]\ninclude = [\"*.md\"]\nexclude = [\"a.md\", \"c.sample.md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    let names = |found: &discovery::Collection| -> Vec<String> {
        found
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };

    let mut rules = config.collection().clone();
    rules.include = vec!["*.sample.md".to_string()];
    let found = discovery::discover_with(&scratch.0, &rules).unwrap();
    // The include was replaced, and the declared excludes still apply.
    assert_eq!(names(&found), ["b.sample.md"]);
    let declared = discovery::discover(&scratch.0, Some(&config)).unwrap();
    assert_eq!(names(&declared), ["b.sample.md"]);
    let _ = Path::new(".");
}

#[test]
fn hidden_directories_and_environments_are_not_walked() {
    let scratch = Scratch::new("hidden");
    scratch.sample("a.md");
    scratch.sample(".git/b.md");
    scratch.sample("env/lib/c.md");
    scratch.file("env/pyvenv.cfg", "home = /usr/bin\n");
    scratch.sample("sub/d.md");
    scratch.config("", "schema_version = 1\n[collection]\nrecursive = true\n");
    let config = project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["a.md", "sub/d.md"]);
}

#[test]
fn an_empty_file_is_set_aside_and_said() {
    let scratch = Scratch::new("empty-file");
    scratch.sample("a.md");
    scratch.file("empty.md", "");
    let found = discover(&scratch.0, None);
    assert_eq!(names(&found), ["a.md"]);
    let skipped = found
        .skipped
        .iter()
        .find(|skipped| skipped.path.ends_with("empty.md"))
        .unwrap_or_else(|| panic!("{:?}", found.skipped));
    assert!(
        matches!(&skipped.reason, SkipReason::Malformed { message } if message.contains("empty")),
        "{:?}",
        skipped.reason
    );
}

#[test]
fn markdown_extensions_are_read_by_default() {
    let scratch = Scratch::new("extensions");
    scratch.sample("a.md");
    scratch.sample("b.MD");
    scratch.sample("c.markdown");
    scratch.file("d.txt", "---\nschema_version: 1\n---\n");
    assert_eq!(
        names(&discover(&scratch.0, None)),
        ["a.md", "b.MD", "c.markdown"]
    );
}

#[test]
fn classify_distinguishes_its_three_refusals() {
    // Unreadable, no frontmatter and malformed are three answers, because they
    // have three different fixes.
    let scratch = Scratch::new("classify-three");
    assert!(discovery::classify(&scratch.sample("good.md")).is_ok());
    assert_eq!(
        discovery::classify(&scratch.file("plain.md", "# Notes\n")),
        Err(SkipReason::NoFrontmatter)
    );
    let unclosed = discovery::classify(&scratch.file("open.md", "---\nschema_version: 1\n"));
    assert!(
        matches!(unclosed, Err(SkipReason::Malformed { .. })),
        "{unclosed:?}"
    );
    let missing = discovery::classify(&scratch.0.join("absent.md"));
    assert!(
        matches!(missing, Err(SkipReason::Unreadable { .. })),
        "{missing:?}"
    );
}

#[test]
fn a_sample_without_its_version_is_discovered() {
    // Read as the current version, and noted by validate.
    let scratch = Scratch::new("versionless");
    let versionless =
        discovery::classify(&scratch.file("noversion.md", "---\nname: A\n---\nnote\n"));
    assert!(versionless.is_ok(), "{versionless:?}");
}

#[test]
fn patterns_read_as_globs_with_double_stars_and_alternatives() {
    // Globset's globs. `*` stays within a segment; `**` crosses them, `{a,b}`
    // is either, `[0-9]` one of a set.
    let scratch = Scratch::new("globset");
    scratch.sample("runs/2026/a.md");
    scratch.sample("runs/b.md");
    scratch.sample("kept/c1.md");
    scratch.sample("kept/cx.md");
    scratch.sample("other/d.md");
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         include = [\"runs/**/*.md\", \"{kept,none}/c[0-9].md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    let found = discover(&scratch.0, Some(&config));
    assert_eq!(names(&found), ["kept/c1.md", "runs/2026/a.md", "runs/b.md"]);
    // `*` alone does not cross a `/`.
    let path = scratch.config(
        "",
        "schema_version = 1\n[collection]\nrecursive = true\ninclude = [\"runs/*.md\"]\n",
    );
    let config = project_config::load(&path).unwrap();
    assert_eq!(names(&discover(&scratch.0, Some(&config))), ["runs/b.md"]);
}

/// Each sample's own files, by the sample's file name and the files' paths
/// below the scratch, and those of none.
fn owned_files(scratch: &Scratch, files: &str) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let rc = scratch.config(
        "",
        &format!("schema_version = 1\n[collection]\nfiles = {files}\n"),
    );
    let config = project_config::load(&rc).unwrap();
    let samples = discover(&scratch.0, Some(&config)).paths;
    let found = discovery::files_by_sample(&config, &samples).unwrap();
    let relative = |path: &PathBuf| {
        path.strip_prefix(&scratch.0)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    };
    (
        found
            .owned
            .iter()
            .map(|(sample, files)| {
                (
                    sample.file_stem().unwrap().to_string_lossy().into_owned(),
                    files.iter().map(relative).collect(),
                )
            })
            .collect(),
        found.unowned.iter().map(relative).collect(),
    )
}

#[test]
fn a_files_pattern_names_a_samples_files_by_its_name() {
    let scratch = Scratch::new("files-pattern");
    scratch.sample("C1.md");
    scratch.sample("C12.md");
    for file in [
        "images/C1.jpg",
        "images/C1-top.png",
        "images/C1.tif",
        "images/C12.jpg",
        "raw/C1/run1/a.dat",
        "raw/C1/b.dat",
        "raw/C9/c.dat",
    ] {
        scratch.file(file, "x");
    }
    let (owned, unowned) =
        owned_files(&scratch, r#"["images/{name}*.{jpg,png}", "raw/{name}/**"]"#);
    assert_eq!(
        owned,
        [
            (
                "C1".to_string(),
                vec![
                    "images/C1-top.png".to_string(),
                    "images/C1.jpg".to_string(),
                    "raw/C1/b.dat".to_string(),
                    "raw/C1/run1/a.dat".to_string(),
                ]
            ),
            // `C1*` matches it too: the longest name owns it.
            ("C12".to_string(), vec!["images/C12.jpg".to_string()]),
        ]
    );
    // The pattern's shape with no sample's name in it is nobody's; what is
    // not the pattern's — a TIFF — is not said at all.
    assert_eq!(unowned, ["raw/C9/c.dat"]);
}

#[test]
fn a_name_is_no_glob_in_a_files_pattern() {
    let scratch = Scratch::new("files-literal-name");
    scratch.sample("a[1].md");
    scratch.file("img/a[1].jpg", "x");
    scratch.file("img/a1.jpg", "x");
    let (owned, unowned) = owned_files(&scratch, r#"["img/{name}.jpg"]"#);
    assert_eq!(
        owned,
        [("a[1]".to_string(), vec!["img/a[1].jpg".to_string()])]
    );
    assert_eq!(unowned, ["img/a1.jpg"]);
}

#[test]
fn an_imported_files_pattern_finds_each_collections_files() {
    let scratch = Scratch::new("files-imported");
    scratch.config(
        "",
        "schema_version = 1\n[collection]\n\
         files = [\"{collection}/photos/{name}*\", \"images/{collection}/{name}*\"]\n",
    );
    for collection in ["c1", "c2"] {
        scratch.config(collection, "schema_version = 1\nimport = \"..\"\n");
    }
    let a = scratch.sample("c1/A.md");
    let b = scratch.sample("c2/B.md");
    for file in [
        "c1/photos/A.jpg",
        "images/c1/A-top.png",
        "c2/photos/B.jpg",
        "images/c2/B.png",
        // A's name, in the other collection's folder: not A's.
        "images/c2/A.png",
    ] {
        scratch.file(file, "x");
    }
    let root = dunce::canonicalize(&scratch.0).unwrap();
    let files_of = |sample: &Path| -> Vec<String> {
        let config = project_config::load_for(sample).unwrap().unwrap();
        let mut found: Vec<String> = discovery::sample_files(&config, sample, None)
            .unwrap()
            .iter()
            .map(|file| {
                dunce::canonicalize(file)
                    .unwrap()
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        found.sort();
        found
    };
    assert_eq!(files_of(&a), ["c1/photos/A.jpg", "images/c1/A-top.png"]);
    assert_eq!(files_of(&b), ["c2/photos/B.jpg", "images/c2/B.png"]);
}

#[test]
fn a_files_entry_without_name_keeps_the_rule_by_name() {
    let scratch = Scratch::new("files-by-name");
    scratch.sample("C1.md");
    scratch.file("reports/C1_report.pdf", "x");
    scratch.file("reports/C12_report.pdf", "x");
    scratch.file("photos/C1-a.jpg", "x");
    scratch.file("photos/C1-b.png", "x");
    scratch.file("x[1]/C1.txt", "x");
    let (owned, unowned) = owned_files(&scratch, r#"["reports", "photos/*.jpg", "x[1]"]"#);
    assert_eq!(
        owned,
        [(
            "C1".to_string(),
            vec![
                "photos/C1-a.jpg".to_string(),
                "reports/C1_report.pdf".to_string(),
                // An existing folder is a folder, whatever its name holds.
                "x[1]/C1.txt".to_string(),
            ]
        )]
    );
    assert_eq!(unowned, ["reports/C12_report.pdf"]);
}
