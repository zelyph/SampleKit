//! The tests of `link`.

use std::path::{Path, PathBuf};

use samplekit::core::link::{self, Link, Resolved};

#[test]
fn a_whole_wikilink_or_address_is_a_link_and_anything_else_text() {
    let sample = |target: &str, shown: Option<&str>| Link::Sample {
        target: target.to_string(),
        shown: shown.map(str::to_string),
        fragment: None,
    };
    assert_eq!(link::parse("[[steel-02]]"), Some(sample("steel-02", None)));
    assert_eq!(
        link::parse(" [[steel-02|the second]] "),
        Some(sample("steel-02", Some("the second")))
    );
    // A heading names a place in a note: kept, and the sample opened whole.
    assert_eq!(
        link::parse("[[steel-02#Fracture]]"),
        Some(Link::Sample {
            target: "steel-02".to_string(),
            shown: None,
            fragment: Some("Fracture".to_string()),
        })
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
        "ratio:2",
        "C:\\data\\a.csv",
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
        fragment: None,
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

#[test]
fn a_wikilink_finds_its_sample_as_obsidian_does() {
    let a = PathBuf::from("samples/2024/Steel-02.md");
    let b = PathBuf::from("samples/2025/steel-02.md");
    let c = PathBuf::from("samples/copper-01.md");
    let index = link::Index::new([
        (a.as_path(), None),
        (b.as_path(), None),
        (c.as_path(), Some("Copper, annealed")),
    ]);
    // With its extension, as Obsidian writes a link it completes.
    assert_eq!(index.resolve("copper-01.md"), Resolved::One(c.clone()));
    // In any case, where no file is named as written.
    assert_eq!(index.resolve("Copper-01"), Resolved::One(c.clone()));
    assert_eq!(index.resolve("copper, ANNEALED"), Resolved::One(c.clone()));
    // As written first: Steel-02 is one file, steel-02 another.
    assert_eq!(index.resolve("Steel-02"), Resolved::One(a.clone()));
    // Two of one name told apart by their folder.
    assert_eq!(index.resolve("2025/steel-02"), Resolved::One(b.clone()));
    assert_eq!(index.resolve("2024/steel-02.md"), Resolved::One(a.clone()));
    assert_eq!(index.resolve("2023/steel-02"), Resolved::Nowhere);
    // One link alone resolved as the index resolves it.
    let samples = [
        (a.as_path(), None),
        (b.as_path(), None),
        (c.as_path(), Some("Copper, annealed")),
    ];
    for target in [
        "copper-01.md",
        "Copper-01",
        "copper, ANNEALED",
        "Steel-02",
        "steel-02",
        "STEEL-02",
        "2025/steel-02",
        "2024/steel-02.md",
        "2023/steel-02",
    ] {
        assert_eq!(
            link::resolve(target, samples),
            index.resolve(target),
            "{target}"
        );
    }
}

#[test]
fn a_wikilink_names_a_file_of_the_project_by_its_whole_name() {
    let sample = PathBuf::from("samples/steel-02.md");
    let curve = PathBuf::from("data/2024/curve.csv");
    let other = PathBuf::from("data/2025/curve.csv");
    let paper = PathBuf::from("papers/Paper.pdf");
    let index = link::Index::new([(sample.as_path(), None)]).with_files([
        curve.clone(),
        other.clone(),
        paper.clone(),
    ]);
    assert_eq!(index.resolve("paper.pdf"), Resolved::File(paper));
    assert_eq!(
        index.resolve("2024/curve.csv"),
        Resolved::File(curve.clone())
    );
    assert_eq!(
        index.resolve("curve.csv"),
        Resolved::Several(vec![curve, other])
    );
    // A sample first, by its name without extension.
    assert_eq!(index.resolve("steel-02"), Resolved::One(sample));
    assert_eq!(index.resolve("absent.pdf"), Resolved::Nowhere);
}

#[test]
fn a_link_keeps_the_place_it_names_and_a_pdf_its_page() {
    assert_eq!(
        link::parse("[[paper.pdf#page=12]]"),
        Some(Link::Sample {
            target: "paper.pdf".to_string(),
            shown: None,
            fragment: Some("page=12".to_string()),
        })
    );
    assert_eq!(
        link::parse("[the method](paper%20one.pdf#page=3)"),
        Some(Link::Path {
            path: "paper one.pdf".to_string(),
            shown: Some("the method".to_string()),
            fragment: Some("page=3".to_string()),
        })
    );
    // A `#` of the file's name is written `%23`, and stays in it.
    assert_eq!(
        link::parse("[run](run%231.md)"),
        Some(Link::Path {
            path: "run#1.md".to_string(),
            shown: Some("run".to_string()),
            fragment: None,
        })
    );
    assert_eq!(link::page_of("page=12"), Some(12));
    assert_eq!(link::page_of("page=4&zoom=100"), Some(4));
    assert_eq!(link::page_of("Fracture"), None);
    assert_eq!(link::page_of("page=0"), None);
}

#[test]
fn an_address_of_another_tool_is_a_link() {
    for address in [
        "zotero://select/library/items/ABCD1234",
        "obsidian://open?vault=lab&file=steel-02",
        "mailto:lab@example.org",
        "ftp://example.org/data.zip",
        "file:///srv/data/curve.csv",
        "HTTPS://example.org",
    ] {
        assert_eq!(
            link::parse(address),
            Some(Link::Url {
                address: address.to_string(),
                shown: None
            }),
            "{address}"
        );
    }
    assert_eq!(
        link::parse("[the reference](zotero://select/library/items/ABCD1234)"),
        Some(Link::Url {
            address: "zotero://select/library/items/ABCD1234".to_string(),
            shown: Some("the reference".to_string())
        })
    );
}
