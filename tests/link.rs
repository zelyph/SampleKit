//! The tests of `link`.

use std::path::{Path, PathBuf};

use samplekit::core::link::{self, Link, Resolved};

#[test]
fn a_whole_wikilink_or_address_is_a_link_and_anything_else_text() {
    let sample = |target: &str, shown: Option<&str>| Link::Sample {
        target: target.to_string(),
        shown: shown.map(str::to_string),
    };
    assert_eq!(link::parse("[[steel-02]]"), Some(sample("steel-02", None)));
    assert_eq!(
        link::parse(" [[steel-02|the second]] "),
        Some(sample("steel-02", Some("the second")))
    );
    // A heading names a place in a note: the sample is opened whole.
    assert_eq!(
        link::parse("[[steel-02#Fracture]]"),
        Some(sample("steel-02", None))
    );
    assert_eq!(
        link::parse("https://example.org/a"),
        Some(Link::Url {
            address: "https://example.org/a".to_string(),
            shown: None
        })
    );
    for text in [
        "steel-02",
        "see [[steel-02]]",
        "[[]]",
        "[[a]] and [[b]]",
        "https://",
        "https://example.org/a b",
        "ftp://example.org",
    ] {
        assert_eq!(link::parse(text), None, "{text}");
    }
    assert_eq!(
        link::parse("[[steel-02|the second]]").unwrap().shown(),
        "the second"
    );
}

#[test]
fn a_target_is_found_by_its_file_then_by_its_name() {
    let a = PathBuf::from("samples/steel-02.md");
    let b = PathBuf::from("samples/b.md");
    let c = PathBuf::from("other/steel-02.md");
    let samples =
        |list: &[(&PathBuf, Option<&'static str>)]| -> Vec<(PathBuf, Option<&'static str>)> {
            list.iter()
                .map(|(path, name)| ((*path).clone(), *name))
                .collect()
        };
    let resolve = |target: &str, held: &[(PathBuf, Option<&'static str>)]| {
        link::resolve(
            target,
            held.iter().map(|(path, name)| (path.as_path(), *name)),
        )
    };
    let held = samples(&[(&a, None), (&b, Some("steel-02"))]);
    // The file's name wins over another sample's `name:`.
    assert_eq!(resolve("steel-02", &held), Resolved::One(a.clone()));
    let held = samples(&[(&b, Some("bar-07"))]);
    assert_eq!(resolve("bar-07", &held), Resolved::One(b.clone()));
    assert_eq!(resolve("bar-08", &held), Resolved::Nowhere);
    let held = samples(&[(&a, None), (&c, None)]);
    assert_eq!(resolve("steel-02", &held), Resolved::Several(vec![a, c]));
    let dotted = [(Path::new("x/bar-09.sample.md"), None)];
    assert_eq!(
        link::resolve("bar-09", dotted),
        Resolved::One(PathBuf::from("x/bar-09.sample.md"))
    );
}

#[test]
fn a_markdown_link_names_a_path_or_a_page() {
    let path = |path: &str, shown: Option<&str>| Link::Path {
        path: path.to_string(),
        shown: shown.map(str::to_string),
    };
    assert_eq!(
        link::parse("[the second](steel-02.md)"),
        Some(path("steel-02.md", Some("the second")))
    );
    // Spaces as Markdown writes them, `%20` or between angle brackets; a
    // title after the path is the reader's tooltip, and dropped.
    assert_eq!(
        link::parse("[report](../reports/test%20report.pdf)"),
        Some(path("../reports/test report.pdf", Some("report")))
    );
    assert_eq!(
        link::parse("[report](<../reports/test report.pdf>)"),
        Some(path("../reports/test report.pdf", Some("report")))
    );
    assert_eq!(
        link::parse(r#"[report](report.pdf "the March test")"#),
        Some(path("report.pdf", Some("report")))
    );
    assert_eq!(
        link::parse("[datasheet](https://example.org/s355)"),
        Some(Link::Url {
            address: "https://example.org/s355".to_string(),
            shown: Some("datasheet".to_string())
        })
    );
    assert_eq!(
        link::parse("[](steel-02.md)").unwrap().shown(),
        "steel-02.md"
    );
    for text in ["[a](b) and more", "see [a](b)", "[a]()", "[a](b", "[a] (b)"] {
        assert_eq!(link::parse(text), None, "{text}");
    }
    let folder = Path::new("/data/bars");
    assert_eq!(
        link::path_from(folder, "../reports/r.pdf"),
        PathBuf::from("/data/bars/../reports/r.pdf")
    );
    assert_eq!(
        link::path_from(folder, "/srv/r.pdf"),
        PathBuf::from("/srv/r.pdf")
    );
}
