//! The tests of `terminal_rendering`.

use std::fs;
use std::path::PathBuf;

use samplekit::config::project_config::{self as config, ColumnSpec, Profile, ProjectConfig};
use samplekit::core::formatting::{Precision, Presentation};
use samplekit::core::identifier::Identifier;
use samplekit::core::property::Property;
use samplekit::core::sample::{AttributeValue, Sample};
use samplekit::core::table::{ColumnMeta, Table};
use samplekit::core::uncertainty::Uncertainty;
use samplekit::core::value::Value;
use samplekit::presentation::terminal_rendering::{self as render, Style, Target};
use samplekit::query::field_addressing::{Subject, Vocabulary};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn wide() -> Target {
    Target::Terminal { width: 200 }
}

fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
    cells
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect()
}

fn headers(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

/// The column each row occupies, so alignment can be asserted rather than
/// eyeballed.
fn column_starts(rendered: &str, needle: &str) -> Vec<usize> {
    rendered
        .lines()
        .filter_map(|line| line.find(needle).map(|at| render::width(&line[..at])))
        .collect()
}

// ------------------------------------------------------------------ widths

#[test]
fn columns_align_with_unicode_content() {
    // `été_max` and `réort` produce aligned columns: byte width would make
    // them ragged for exactly the users this program has.
    let out = render::table(
        headers(&["name", "été_max"]),
        rows(&[&["réort", "1.0"], &["abcdefghij", "2.0"]]),
        wide(),
        Style::Plain,
    );
    let widths: Vec<usize> = out.lines().skip(2).map(render::width).collect();
    assert_eq!(widths[0], widths[1], "{out}");
}

#[test]
fn cjk_characters_count_as_two_columns() {
    assert_eq!(render::width("試料"), 4);
    assert_eq!(render::width("ab"), 2);
    let out = render::table(
        headers(&["name", "v"]),
        rows(&[&["試料", "1.0"], &["abcd", "2.0"]]),
        wide(),
        Style::Plain,
    );
    let lines: Vec<&str> = out.lines().skip(2).collect();
    assert_eq!(render::width(lines[0]), render::width(lines[1]), "{out}");
}

#[test]
fn combining_marks_count_as_zero() {
    // `e` followed by a combining acute is one column, not two.
    let decomposed = "e\u{301}";
    assert_eq!(render::width(decomposed), 1);
    assert_eq!(render::width("é"), 1);
}

#[test]
fn truncation_does_not_split_a_grapheme() {
    // No broken character at the boundary.
    let text = "試料試料試料";
    let cut = render::truncate(text, 5);
    assert!(cut.ends_with('…'), "{cut}");
    assert!(render::width(&cut) <= 5, "{cut} is {}", render::width(&cut));
    // And a decomposed accent stays with its base letter.
    let accented = "e\u{301}e\u{301}e\u{301}";
    let cut = render::truncate(accented, 2);
    assert!(!cut.starts_with('\u{301}'), "{cut:?}");
}

#[test]
fn truncation_is_marked() {
    // Never a silent cut.
    let cut = render::truncate("a very long beer name", 10);
    assert!(cut.ends_with('…'), "{cut}");
    assert_eq!(render::truncate("short", 10), "short");
}

// ------------------------------------------------------------- terminal vs pipe

#[test]
fn pipe_output_has_no_escapes() {
    // Not even when colour would be enabled: a script parsing this must not
    // receive an escape sequence.
    let out = render::table(
        headers(&["name", "brix"]),
        rows(&[&["A", "2.8"]]),
        Target::Pipe,
        Style::Plain,
    );
    assert!(!out.contains('\u{1b}'), "{out:?}");
}

#[test]
fn pipe_output_is_not_truncated() {
    // Regardless of terminal width: output truncated for display and then
    // parsed is data loss.
    let long = "a".repeat(200);
    let out = render::table(
        headers(&["name"]),
        rows(&[&[&long]]),
        Target::Pipe,
        Style::Plain,
    );
    assert!(out.contains(&long), "the cell was cut");
    // A narrow terminal wraps it, and keeps every character.
    let narrow = render::table(
        headers(&["name"]),
        rows(&[&[&long]]),
        Target::Terminal { width: 40 },
        Style::Plain,
    );
    assert!(!narrow.contains(&long));
    assert!(!narrow.contains('…'), "{narrow}");
    assert_eq!(narrow.matches('a').count(), 200 + 1, "{narrow}");
}

#[test]
fn narrow_terminal_does_not_reduce_precision() {
    // A `.6f` column stays `.6f`: display precision is a scientific statement
    // and the window size has no bearing on it.
    let cell = "12.487000";
    let out = render::table(
        headers(&["name", "malt"]),
        rows(&[&["a-very-long-cask-name-here", cell]]),
        Target::Terminal { width: 30 },
        Style::Plain,
    );
    // Whole, or its column hidden and said: never cut.
    assert!(
        out.contains(cell) || out.contains("1 column hidden"),
        "{out}"
    );
    assert!(!out.contains("12.4…"), "{out}");
}

#[test]
fn decimal_alignment_lines_up_points() {
    // Mixed magnitudes align on the point.
    let out = render::table(
        headers(&["name", "v"]),
        rows(&[&["a", "12.5"], &["b", "1284.25"], &["c", "3.125"]]),
        wide(),
        Style::Plain,
    );
    let points = column_starts(&out, ".");
    assert!(
        points.windows(2).all(|pair| pair[0] == pair[1]),
        "{out}\n{points:?}"
    );
}

#[test]
fn a_style_adds_a_border_and_never_a_digit() {
    // Boxed and plain differ in their frame and agree on every cell.
    let plain = render::table(
        headers(&["name", "brix"]),
        rows(&[&["A", "2.8014"], &["B", "2.7991"]]),
        wide(),
        Style::Plain,
    );
    let boxed = render::table(
        headers(&["name", "brix"]),
        rows(&[&["A", "2.8014"], &["B", "2.7991"]]),
        wide(),
        Style::Boxed,
    );
    assert!(boxed.contains('┌') && !plain.contains('┌'));
    for cell in ["A", "B", "2.8014", "2.7991"] {
        assert!(plain.contains(cell) && boxed.contains(cell), "{cell}");
    }
    // And a style is named, so a setting and a flag can choose it.
    assert_eq!(Style::parse("boxed"), Some(Style::Boxed));
    assert_eq!(Style::parse("fancy"), None);
    assert_eq!(Style::names().len(), 2);
}

// -------------------------------------------------------------------- views

struct Fixture {
    sample: Sample,
    vocabulary: Vocabulary,
    path: PathBuf,
    directory: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// One cask with a malt, a volume it has no value for, and a mashing
/// table — enough for every view kind.
fn fixture(name: &str) -> Fixture {
    let directory = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-render-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();

    let mut sample = Sample::new();
    sample.set_name(Some("Keg 42".to_string()));
    sample.set_note("Measured after drying for 24 hours.\n".to_string());

    let mut malt = Property::stored(number(12.5));
    malt.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    malt.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        precision: Some(Precision::both(".2f").unwrap()),
    });
    sample.set_property(id("malt"), malt).unwrap();

    let mut wort = Property::stored(number(104.221));
    wort.set_presentation(Presentation {
        unit: Some("lintner".to_string()),
        symbol: None,
        precision: Some(Precision::both(".3f").unwrap()),
    });
    let mut mashing = Table::new(
        id("mashing"),
        vec![id("temperature")],
        ["temperature", "wort"]
            .iter()
            .map(|n| (id(n), ColumnMeta::default()))
            .collect::<indexmap::IndexMap<_, _>>(),
        Vec::new(),
    )
    .unwrap();
    mashing
        .add_row(vec![
            (id("temperature"), Property::stored(number(78.5))),
            (id("wort"), wort),
        ])
        .unwrap();
    sample.set_table(id("mashing"), mashing).unwrap();

    let path = directory.join("keg.md");
    let vocabulary = Vocabulary::of(&sample);
    Fixture {
        sample,
        vocabulary,
        path,
        directory,
    }
}

fn subject(fixture: &Fixture) -> Subject<'_> {
    Subject {
        sample: &fixture.sample,
        path: Some(&fixture.path),
        vocabulary: &fixture.vocabulary,
        states: None,
    }
}

fn entry(
    field: &str,
    label: Option<&str>,
    precision: Option<&str>,
    template: Option<&str>,
) -> ColumnSpec {
    ColumnSpec {
        field: field.to_string(),
        label: label.map(str::to_string),
        header: None,
        precision: precision
            .map(|spec| samplekit::format::schema::PrecisionSchema::Both(spec.to_string())),
        template: template.map(str::to_string),
    }
}

/// A profile of the columns given: the declaration a cell is rendered under.
fn definition(entries: Vec<ColumnSpec>) -> Profile {
    samplekit::config::profiles::anonymous(entries)
}

/// Each column of `view` rendered for one sample, a line each.
fn rendered(
    subject: &Subject,
    view: &Profile,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> String {
    view.columns()
        .iter()
        .map(|entry| {
            render::cell(
                entry,
                subject,
                render::Declaration::Profile(view),
                config,
                style,
                true,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_explicit_channel_renders_only_that_channel() {
    let fixture = fixture("explicit-channels");
    let profile = config::Profile {
        name: "channels".to_string(),
        columns: ["malt", "malt.v", "malt.u", "malt.unit", "malt.symbol"]
            .into_iter()
            .map(|field| entry(field, None, None, None))
            .collect(),
        sort: Vec::new(),
        group: Vec::new(),
    };
    let subject = subject(&fixture);

    assert_eq!(
        render::row(&profile, &subject, None, None),
        ["12.50 ± 0.05", "12.50", "0.05", "g", "m"]
    );
    assert_eq!(
        render::columns(&profile, &[subject], None, None),
        [
            "malt [g]",
            "malt.v [g]",
            "malt.u [g]",
            "malt.unit",
            "malt.symbol",
        ]
    );
}

#[test]
fn a_profile_row_renders_in_column_order() {
    // Not in the order the cells resolved: a row ordered by when each value
    // came back is a table that reads correctly and means something else.
    let fixture = fixture("profile-row");
    let profile = config::Profile {
        name: "p".to_string(),
        columns: vec![
            entry("wort", None, None, None),
            entry("malt", None, None, None),
            entry("volume", None, None, None),
        ],
        sort: Vec::new(),
        group: Vec::new(),
    };
    let cells = render::row(&profile, &subject(&fixture), None, None);
    assert_eq!(cells.len(), 3);
    // `wort` is a table column, not a property: a bare name addresses
    // the property, so this sample has nothing under it.
    assert_eq!(cells[0], "\u{2014}");
    // One cell for the whole quantity, and no unit in it.
    assert_eq!(cells[1], "12.50 \u{b1} 0.05");
    assert_eq!(cells[2], "\u{2014}");
}

#[test]
fn a_header_carries_the_unit_when_the_samples_agree() {
    // The unit cannot change down a column, so it is written once. And when it
    // *does* change, the header says nothing rather than something wrong.
    let grams = fixture("header-unit");
    let profile = config::Profile {
        name: "p".to_string(),
        columns: vec![entry("malt", Some("Malt"), None, None)],
        sort: Vec::new(),
        group: Vec::new(),
    };
    let headers = render::columns(&profile, &[subject(&grams)], None, None);
    assert_eq!(headers, ["Malt [g]"]);

    let mut other = fixture("header-unit-other");
    let mut kilograms = Property::stored(number(0.0125));
    kilograms.set_presentation(Presentation {
        unit: Some("kg".to_string()),
        symbol: None,
        precision: None,
    });
    other.sample.set_property(id("malt"), kilograms).unwrap();
    let disagreeing = render::columns(&profile, &[subject(&grams), subject(&other)], None, None);
    assert_eq!(disagreeing, ["Malt"]);
}

#[test]
fn a_value_with_no_unit_takes_the_unit_off_the_header() {
    // `5` beside rows in kg was shown under `Malt [kg]`, and read as five
    // kilograms whatever it was written in. A row with no value is no
    // evidence either way.
    let grams = fixture("header-unit-missing");
    let profile = config::Profile {
        name: "p".to_string(),
        columns: vec![entry("malt", Some("Malt"), None, None)],
        sort: Vec::new(),
        group: Vec::new(),
    };
    let mut bare = fixture("header-unit-bare");
    bare.sample
        .set_property(id("malt"), Property::stored(number(5.0)))
        .unwrap();
    let headers = render::columns(&profile, &[subject(&grams), subject(&bare)], None, None);
    assert_eq!(headers, ["Malt"]);
    let mut empty = fixture("header-unit-empty");
    empty.sample.remove_property(&id("malt")).ok();
    let headers = render::columns(&profile, &[subject(&grams), subject(&empty)], None, None);
    assert_eq!(headers, ["Malt [g]"]);
}

#[test]
fn a_tag_set_renders_as_a_cell() {
    // A tag set is not a scalar and still has to appear in a cell someone asked
    // for. A dash here would be the column of dashes this project exists to
    // remove.
    let mut fixture = fixture("tags-cell");
    fixture
        .sample
        .set_tags(vec![id("reference"), id("cold_crashed")]);
    let vocabulary = Vocabulary::of(&fixture.sample);
    let subject = Subject {
        sample: &fixture.sample,
        path: Some(&fixture.path),
        vocabulary: &vocabulary,
        states: None,
    };
    let profile = config::Profile {
        name: "p".to_string(),
        columns: vec![entry("tags", None, None, None)],
        sort: Vec::new(),
        group: Vec::new(),
    };
    assert_eq!(
        render::row(&profile, &subject, None, None),
        ["reference cold_crashed"]
    );
}

#[test]
fn a_list_item_renders_as_a_scalar_cell() {
    let mut fixture = fixture("list-cell");
    fixture
        .sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    let vocabulary = Vocabulary::of(&fixture.sample);
    let subject = Subject {
        sample: &fixture.sample,
        path: Some(&fixture.path),
        vocabulary: &vocabulary,
        states: None,
    };
    let profile = config::Profile {
        name: "p".to_string(),
        columns: vec![entry("temperatures[#1]", None, None, None)],
        sort: Vec::new(),
        group: Vec::new(),
    };
    assert_eq!(render::row(&profile, &subject, None, None), ["30"]);
}

#[test]
fn absent_renders_as_a_placeholder_in_a_terminal() {
    // And is distinguishable from an empty text value.
    let out = render::table(
        headers(&["name", "v"]),
        rows(&[&["a", render::ABSENT], &["b", ""]]),
        wide(),
        Style::Plain,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[2].contains('—'), "{out}");
    assert!(!lines[3].contains('—'), "{out}");
}

// ---------------------------------------------------------------- templates

// ---------------------------------------------------------- one quantity

#[test]
fn one_quantity_renders_with_its_unit_beside_it() {
    let dir = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-quantity-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(".samplekitrc");
    fs::write(
        &path,
        "schema_version = 1\n\n[unit.\"g/L\"]\nplain = \"g/L\"\n\n[property.brix]\nprecision = \".3f\"\n",
    )
    .unwrap();
    let project = config::load(&path).unwrap();
    let _ = fs::remove_dir_all(&dir);

    let presentation = Presentation {
        unit: Some("g/L".to_string()),
        ..Presentation::default()
    };
    let value = number(2.1523);
    let uncertainty = Uncertainty::new(0.0004).unwrap();
    // The project's unit variant and precision, the uncertainty rounded as
    // declared like the value.
    assert_eq!(
        render::quantity(
            &value,
            Some(&uncertainty),
            &presentation,
            "brix",
            Some(&project),
            None
        ),
        "2.152 ± 0.000 g/L"
    );
    // A column precision overrides the project's.
    assert_eq!(
        render::quantity(
            &value,
            Some(&uncertainty),
            &presentation,
            "brix",
            Some(&project),
            Some(&Precision::both(".2f").unwrap())
        ),
        "2.15 ± 0.00 g/L"
    );
    // A text value ignores a precision, and an absent one is left to the caller.
    assert_eq!(
        render::quantity(
            &Value::text("Maris Otter"),
            None,
            &Presentation::default(),
            "beer",
            None,
            Some(&Precision::both(".2f").unwrap())
        ),
        "Maris Otter"
    );
    assert_eq!(
        render::quantity(
            &Value::absent(),
            None,
            &Presentation::default(),
            "energy",
            None,
            None
        ),
        ""
    );
}

#[test]
fn a_text_column_header_reads_from_the_left() {
    let out = render::table(
        headers(&["name", "state", "value"]),
        rows(&[&["A", "stale", "12.5"], &["B", "current", "1284.25"]]),
        wide(),
        Style::Plain,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0].find("state"), lines[2].find("stale"), "{out}");
    // Over numbers, the header ends where the widest number does.
    let header_end = lines[0].find("value").unwrap() + "value".len();
    assert_eq!(header_end, lines[3].trim_end().len(), "{out}");
}

#[test]
fn truncate_middle_keeps_both_ends() {
    use samplekit::presentation::terminal_rendering::truncate_middle;
    assert_eq!(truncate_middle("BR-04", 10), "BR-04");
    let cut = truncate_middle("vintage-2026-brewday-04", 11);
    assert!(cut.starts_with("vinta") && cut.ends_with("ay-04"), "{cut}");
    assert!(cut.contains('\u{2026}'), "{cut}");
}

#[test]
fn the_identity_column_is_greyed_whole() {
    use samplekit::presentation::terminal_rendering::{Style, Target, table_identified};
    let out = table_identified(
        vec!["name".to_string(), "malt".to_string()],
        vec![
            vec!["A".to_string(), "1.0".to_string()],
            vec!["B".to_string(), "2.0".to_string()],
        ],
        Target::Pipe,
        Style::Plain,
        // One column added by the terminal, and colour on. The renderer needs
        // the count, not a flag: `project` and `name` can both be added, and a
        // narrow table must know how many of the leading columns it may drop.
        1,
        true,
    );
    // The header and both rows; the rule beneath the header is not a cell.
    assert_eq!(out.matches("\u{1b}[2m").count(), 3, "{out:?}");
    assert!(!out.contains("\u{1b}[2m1.0"), "{out:?}");
}

#[test]
fn a_long_label_wraps_before_a_column_is_hidden() {
    let out = render::table(
        headers(&["name", "an extremely long label [g per L]"]),
        rows(&[&["A", "1.0"]]),
        Target::Terminal { width: 24 },
        Style::Plain,
    );
    assert!(out.contains("1.0"), "{out}");
    assert!(!out.contains("hidden"), "{out}");
    assert!(!out.contains('…'), "{out}");
    // Every word of the label whole, over several lines, the unit unbroken.
    for word in ["extremely", "long", "label", "[g per L]"] {
        assert!(out.contains(word), "{word}: {out}");
    }
    let rule = out.lines().position(|line| line.starts_with('─')).unwrap();
    assert!(rule >= 2, "the header wraps: {out}");
}

#[test]
fn the_columns_a_table_keeps_wrap_rather_than_hide() {
    let out = render::table_keeping(
        headers(&["sample", "value", "state"]),
        rows(&[&["english-pale", "efficiency", "edited since it was computed"]]),
        Target::Terminal { width: 30 },
        Style::Plain,
        3,
    );
    assert!(!out.contains("hidden"), "{out}");
    assert!(out.lines().next().unwrap().contains("state"), "{out}");
    assert!(out.contains("edited"), "{out}");
    assert!(out.lines().all(|line| render::width(line) <= 30), "{out}");
}

#[test]
fn a_number_is_never_truncated_columns_are_hidden_instead() {
    // Numbers alone too wide: a text column would have wrapped first.
    let out = render::table(
        headers(&["name", "malt", "brix"]),
        rows(&[&["S1", "12.487000", "2.8014"]]),
        Target::Terminal { width: 18 },
        Style::Plain,
    );
    assert!(out.contains("12.487000"), "{out}");
    assert!(!out.contains("2.80"), "{out}");
    assert!(
        out.contains("1 column hidden — --width shows them"),
        "{out}"
    );
}

/// `table` is the layout decision followed by drawing it, and nothing else. A
/// screen consumes the first half; if the two ever came apart, the screen would
/// fit its columns by a rule only it obeyed.
#[test]
fn a_table_is_its_layout_drawn() {
    let at = |width: usize| {
        let target = Target::Terminal { width };
        let head = headers(&["name", "beer", "malt", "brix"]);
        let body = rows(&[
            &["S1", "altbier strain", "12.487000", "2.8014"],
            &["S2", "gueuze", "3.10", "5.6800"],
        ]);
        let drawn = render::draw(
            &render::lay_out(head.clone(), body.clone(), target, Style::Plain, 0, false),
            Style::Plain,
        );
        assert_eq!(render::table(head, body, target, Style::Plain), drawn);
    };
    // Roomy, tight enough to narrow the text column, tight enough to hide one.
    for width in [120, 44, 26, 18] {
        at(width);
    }
}

/// What a screen needs out of the decision: the widths to place cells at, the
/// alignment to place them with, and how many columns did not fit — without
/// parsing them back out of a string.
#[test]
fn the_layout_says_what_it_dropped_and_how_wide() {
    let layout = render::lay_out(
        headers(&["name", "malt", "brix"]),
        rows(&[&["S1", "12.487000", "2.8014"]]),
        Target::Terminal { width: 18 },
        Style::Plain,
        0,
        false,
    );
    assert_eq!(layout.hidden, 1, "{layout:?}");
    assert_eq!(layout.headers, ["name", "malt"], "{layout:?}");
    assert_eq!(layout.widths.len(), 2, "{layout:?}");
    assert_eq!(layout.alignment.len(), 2, "{layout:?}");
    // A number is never narrowed, so its column keeps what it holds.
    assert_eq!(layout.widths[1], 9, "{layout:?}");
    // The drawn table and the layout agree about what was dropped.
    let drawn = render::draw(&layout, Style::Plain);
    assert!(!drawn.contains("2.80"), "{drawn}");
    assert!(drawn.contains("1 column hidden"), "{drawn}");
}

/// Placing the mark, painting it, and leaving an absent value unmarked unless
/// it failed are one step, in one place: a table, a report and a screen show a
/// state the same way or they teach different things.
#[test]
fn a_mark_is_composed_once() {
    use samplekit::format::fingerprint::Freshness;

    let stale = Freshness::Stale {
        changed: vec![samplekit::core::property::InputName::Named(id("malt"))],
        upstream: Vec::new(),
    };
    assert_eq!(render::with_mark("1.93", &stale, false), "1.93 ⚠");
    assert_eq!(
        render::with_mark("13.00", &Freshness::Edited, false),
        "13.00 ✎"
    );
    // Current carries no mark, so nothing is appended and nothing is padded.
    assert_eq!(
        render::with_mark("1.93", &Freshness::Current, false),
        "1.93"
    );
    // An absent value carries no mark — the mark would be all the reader sees.
    assert_eq!(render::with_mark("—", &stale, false), "—");
    assert_eq!(render::mark_beside("—", &stale), None);
    // Unless it failed: there the absence *is* what is being reported.
    let failed = Freshness::Failed {
        message: "division by zero".to_string(),
    };
    assert_eq!(render::with_mark("—", &failed, false), "— ✗");
    // With colour allowed the mark is painted and the value is not.
    let painted = render::with_mark("1.93", &stale, true);
    assert!(painted.starts_with("1.93 \u{1b}["), "{painted}");
    assert_eq!(
        render::width(&painted),
        render::width("1.93 ⚠"),
        "{painted}"
    );
}

#[test]
fn a_cell_rounds_at_its_declared_precision() {
    // A table on a screen rounds as its export does. At `.0f` the fixture's
    // `0.05` is `0`, in a cell and in a `.u` column alike.
    let fixture = fixture("rounded");
    let profile = config::Profile {
        name: "rounded".to_string(),
        columns: vec![
            entry("malt", None, Some(".0f"), None),
            entry("malt.u", None, Some(".0f"), None),
        ],
        sort: Vec::new(),
        group: Vec::new(),
    };
    let row = render::row(&profile, &subject(&fixture), None, None);
    assert!(row[0].ends_with("\u{b1} 0"), "{row:?}");
    assert_eq!(row[1], "0");
}

#[test]
fn a_progress_line_never_exceeds_the_terminal_width() {
    use samplekit::presentation::terminal_rendering::{progress_line, width};
    let text = "sample 12/75 · 30/41 · fermentation_cold.rate_fit · oatmeal-stout-second-batch";
    for columns in [24, 40, 60, 80, 200] {
        let line = progress_line(0.4, text, "3m09s", columns);
        assert!(width(&line) < columns, "{columns}: {line}");
    }
    let cut = progress_line(0.4, text, "3m09s", 60);
    assert!(cut.contains('…'), "{cut}");
}

#[test]
fn a_progress_line_keeps_the_bar_and_the_time() {
    use samplekit::presentation::terminal_rendering::progress_line;
    let wide = progress_line(0.5, "12/41 · drop · B38", "3m09s", 120);
    assert_eq!(wide, "[██████████░░░░░░░░░░] 12/41 · drop · B38 · 3m09s");
    let long = progress_line(1.0, &"x".repeat(200), "1.5s", 60);
    assert!(long.starts_with("[████████████████████] "), "{long}");
    assert!(long.ends_with(" · 1.5s"), "{long}");
    let narrow = progress_line(0.0, "12/41 · drop", "3m09s", 22);
    assert!(narrow.starts_with("[░░░░"), "{narrow}");
}

#[test]
fn a_text_column_gives_way_before_a_column_is_hidden() {
    // The long beer narrows and wraps, and the number stays shown.
    let out = render::table(
        headers(&["name", "beer", "brix"]),
        rows(&[&["keg-01", "altbier ninety nine point five percent", "2.8014"]]),
        Target::Terminal { width: 40 },
        Style::Plain,
    );
    assert!(!out.contains("hidden"), "{out}");
    assert!(out.contains("2.8014"), "{out}");
    assert!(out.contains("percent"), "{out}");
    assert!(!out.contains('…'), "{out}");
}

#[test]
fn a_text_cell_wraps_instead_of_being_cut() {
    // What SampleKit says is never cut: a long state wraps under itself, and
    // the other columns are not repeated.
    let state = "stale — fermentation.gravity (itself stale), drop (itself stale) (8 rows)";
    let out = render::table(
        headers(&["sample", "value", "state"]),
        rows(&[&["citra-ipa-first-batch", "fermentation.rate", state]]),
        Target::Terminal { width: 90 },
        Style::Plain,
    );
    assert!(!out.contains('…'), "{out}");
    assert!(!out.contains("hidden"), "{out}");
    let body: Vec<&str> = out.lines().skip(2).collect();
    assert!(body.len() >= 2, "{out}");
    assert_eq!(
        body.iter()
            .filter(|line| line.contains("citra-ipa-first-batch"))
            .count(),
        1,
        "{out}"
    );
    let said = body
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    for word in state.split(' ') {
        assert!(said.contains(word), "{word}: {out}");
    }
    assert!(body.iter().all(|line| render::width(line) <= 90), "{out}");
}

#[test]
fn every_added_column_is_greyed() {
    // `name` and `project` are both there because the terminal put them there.
    // One of them reading as asked-for is the confusion the grey exists to
    // prevent.
    use samplekit::presentation::terminal_rendering::{Style, Target, table_identified};
    let out = table_identified(
        vec![
            "name".to_string(),
            "project".to_string(),
            "malt".to_string(),
        ],
        vec![
            vec!["A".to_string(), "one".to_string(), "1.0".to_string()],
            vec!["B".to_string(), "two".to_string(), "2.0".to_string()],
        ],
        Target::Pipe,
        Style::Plain,
        2,
        true,
    );
    // Two added columns over a header and two rows.
    assert_eq!(out.matches("\u{1b}[2m").count(), 6, "{out:?}");
    assert_eq!(out.matches("\u{1b}[0m").count(), 6, "{out:?}");
    // And the column that was asked for stays plain.
    assert!(!out.contains("\u{1b}[2m1.0"), "{out:?}");
}

#[test]
fn a_style_takes_no_room_in_the_layout() {
    // A cell carrying colour must measure as what a terminal draws, or its
    // column widens by the age of an escape nobody sees — and truncation
    // could cut one in half.
    use samplekit::presentation::terminal_rendering::width;
    let style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
    let painted = format!("{}stale{}", style.render(), style.render_reset());
    assert_eq!(width(&painted), width("stale"));
    assert_eq!(width(&painted), 5);
    // And what is drawn is still measured in display columns, not bytes.
    let wide = format!("{}été_max{}", style.render(), style.render_reset());
    assert_eq!(width(&wide), width("été_max"));
}

#[test]
fn a_list_attribute_renders_its_items() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("adjuncts"),
            samplekit::core::sample::AttributeValue::list(vec![
                Value::text("oats"),
                Value::text("FEC"),
            ])
            .unwrap(),
        )
        .unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let view = definition(Vec::new());
    let shown = render::cell(
        &entry("adjuncts", None, None, None),
        &subject,
        render::Declaration::Profile(&view),
        None,
        None,
        false,
    );
    assert_eq!(shown, "oats, FEC");
}

#[test]
fn a_count_ignores_the_quantity_precision() {
    let mut sample = Sample::new();
    let mut foam = Property::measured(
        samplekit::core::value::Readings::new(vec![2.011, 2.017, 2.013]).unwrap(),
        None,
    );
    foam.set_presentation(Presentation {
        unit: Some("mm".to_string()),
        symbol: None,
        precision: Some(Precision::both(".2f").unwrap()),
    });
    sample.set_property(id("foam"), foam).unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let view = definition(Vec::new());
    let shown = |field: &str, precision: Option<&str>| {
        render::cell(
            &entry(field, None, precision, None),
            &subject,
            render::Declaration::Profile(&view),
            None,
            None,
            false,
        )
    };
    assert_eq!(shown("foam.stats.count", None), "3");
    assert_eq!(shown("foam.stats.count", Some(".1f")), "3.0");
    assert!(shown("foam.stats.mean", None).starts_with("2.01"));
}

#[test]
fn a_template_does_not_alter_the_stored_value() {
    // Presentation stays non-destructive.
    let fixture = fixture("non-destructive");
    let view = definition(vec![entry("malt", None, None, Some("{value:.0f}"))]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("13") || out.contains("12"), "{out}");
    let stored = fixture
        .sample
        .property(&id("malt"))
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(stored, number(12.5));
}

#[test]
fn a_template_uses_the_resolved_variant() {
    // A template in a math-styled project gets `\Omega` without asking.
    let directory = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-render-style-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(".samplekitrc");
    fs::write(
        &path,
        "schema_version = 1\n[render]\nstyle = \"math\"\n\
         [unit.g]\nmath = \"\\\\mathrm{g}\"\n[style.math]\nseparator = \"\\\\pm\"\n",
    )
    .unwrap();
    let loaded = config::load(&path).unwrap();

    let fixture = fixture("resolved");
    let view = definition(vec![entry("malt", None, None, Some("{value:.2f} {unit}"))]);
    let out = rendered(&subject(&fixture), &view, Some(&loaded), None);
    assert!(out.contains("\\mathrm{g}"), "{out}");
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_brace_that_names_nothing_is_text() {
    // A template is full of braces that are not placeholders: `\mathrm{g}`
    // has one, and so does a mistyped name. Both come out as written.
    let fixture = fixture("literal-braces");
    let view = definition(vec![entry(
        "malt",
        None,
        None,
        Some("{value:.1f}\\,\\mathrm{g} {oops}"),
    )]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("\\mathrm{g}"), "{out}");
    assert!(out.contains("{oops}"), "{out}");
    assert!(out.contains("12.5"), "{out}");
}

#[test]
fn a_template_keeps_the_declared_rounding() {
    // A template is pasted into a document, where `<1` is not a number: it
    // gets the rounding it asked for.
    let fixture = fixture("template-rounding");
    let view = definition(vec![entry(
        "malt",
        None,
        None,
        Some("{value:.2f} +- {uncertainty:.0f}"),
    )]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("12.50 +- 0"), "{out}");
    assert!(!out.contains('<'), "{out}");
}

#[test]
fn a_template_fills_value_and_uncertainty() {
    // The nominal LaTeX case.
    let fixture = fixture("latex");
    let view = definition(vec![entry(
        "malt",
        None,
        None,
        Some("\\num{{value:.2f}} \\pm \\num{{uncertainty:.2f}}"),
    )]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("\\num{12.50} \\pm \\num{0.05}"), "{out}");
}

#[test]
fn a_template_replaces_the_default_rendering() {
    // And only for the column that declares one.
    let fixture = fixture("template");
    let view = definition(vec![
        entry("malt", None, None, Some("[{value:.1f}]")),
        entry("volume", None, None, None),
    ]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("[12.5]"), "{out}");
    assert!(!out.contains("12.50 ± 0.05"), "{out}");
    assert!(out.contains('—'), "the other column is untouched: {out}");
}

#[test]
fn a_missing_property_is_an_absent_cell() {
    // A column the sample has nothing for renders as absent, without an error.
    let fixture = fixture("missing");
    let view = definition(vec![entry("volume", None, None, None)]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains('—'), "{out}");
}

#[test]
fn a_quantity_is_one_cell_at_a_terminal() {
    // `12.50 ± 0.05` in one column; the split into two is the export's.
    let fixture = fixture("quantity");
    let view = definition(vec![entry("malt", None, None, None)]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert!(out.contains("12.50 ± 0.05"), "{out}");
}

#[test]
fn a_column_with_nothing_behind_it_renders_as_absent() {
    // Rather than being skipped, so the layout does not shift.
    let fixture = fixture("absent-entry");
    let view = definition(vec![
        entry("malt", None, None, None),
        entry("volume", None, None, None),
    ]);
    let out = rendered(&subject(&fixture), &view, None, None);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(out.contains('—'), "{out}");
}

#[test]
fn a_sample_is_shown_whole_its_note_on_request() {
    // Values with their states, then each table unfolded with its units; the
    // note only when asked for, byte for byte.
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-sheet-{}.md", std::process::id()));
    fs::write(
        &path,
        "---\nschema_version: 1\nname: W\nbeer: bock\nproperties:\n  malt: {v: 9.0, u: 0.1, unit: g}\n\
         tables:\n  runs:\n    index: run\n    columns:\n      run: {}\n      load: {unit: N}\n    rows:\n      \
         - run: 1\n        load: 3.5\n---\nHopped  twice.\n",
    )
    .unwrap();
    let (sample, _) = samplekit::format::document::load_sample(&path).unwrap();
    let _ = fs::remove_file(&path);
    let vocabulary = Vocabulary::of(&sample);
    let subject = Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    let shown = render::sheet(&subject, None, None, None, Target::Pipe, false);
    let lines: Vec<&str> = shown.lines().collect();
    assert_eq!(lines[0], "W", "{shown}");
    let beer = lines.iter().position(|line| line.contains("bock")).unwrap();
    let malt = lines
        .iter()
        .position(|line| line.contains("± 0.1"))
        .unwrap();
    let table = lines
        .iter()
        .position(|line| line.contains("runs · 1 row"))
        .unwrap();
    assert!(beer < malt && malt < table, "{shown}");
    assert!(
        shown.contains("load [N]") && shown.contains("3.5"),
        "{shown}"
    );
    assert!(!shown.contains("Hopped"), "{shown}");
    let noted = render::sheet(&subject, None, None, None, Target::Pipe, true);
    assert!(noted.ends_with("Hopped  twice.\n"), "{noted}");
}

#[test]
fn an_added_column_gives_way_before_an_asked_one_is_hidden() {
    // A `project` column the terminal added, kept whole while columns asked
    // for were hidden, laid the table out for the terminal's sake.
    let headers = headers(&["project", "name", "yeast_strain", "loading", "malt"]);
    let rows = rows(&[&["cellar/kegs", "BR-01", "WLP001", "8.40", "9.50 ± 0.014"]]);
    let narrow = render::lay_out(
        headers.clone(),
        rows.clone(),
        Target::Terminal { width: 40 },
        Style::Boxed,
        1,
        false,
    );
    assert_ne!(narrow.headers[0], "project", "{:?}", narrow.headers);
    // With the room, it stays.
    let wide = render::lay_out(
        headers,
        rows,
        Target::Terminal { width: 120 },
        Style::Boxed,
        1,
        false,
    );
    assert_eq!(wide.headers[0], "project");
    assert_eq!(wide.hidden, 0);
}

/// The column a text begins at, on the line holding it.
fn column_of(out: &str, text: &str) -> usize {
    let line = out
        .lines()
        .find(|line| line.contains(text))
        .unwrap_or_else(|| panic!("{text} in {out}"));
    render::width(&line[..line.find(text).unwrap()])
}

#[test]
fn only_a_number_is_aligned_as_one() {
    // `12 samples, dry-stout` was a number because its first word was, and
    // its column aligned on a point it did not have.
    let out = render::table(
        headers(&["#", "changed"]),
        rows(&[
            &["1", "12 samples, .samplekitrc"],
            &["2", "dry-stout"],
            &["3", "4 samples"],
        ]),
        wide(),
        Style::Plain,
    );
    assert_eq!(
        column_of(&out, "12 samples"),
        column_of(&out, "dry-stout"),
        "{out}"
    );
    assert_eq!(
        column_of(&out, "4 samples"),
        column_of(&out, "dry-stout"),
        "{out}"
    );
    // The point is the value's: `12 ± 3.5` lines up with `1.5 ± 0.2` on
    // the point of 1.5, not on its uncertainty's.
    let out = render::table(
        headers(&["name", "x"]),
        rows(&[&["a", "1.5 ± 0.2"], &["b", "12 ± 3.5"], &["c", "—"]]),
        wide(),
        Style::Plain,
    );
    assert_eq!(column_of(&out, "1.5"), column_of(&out, "12 ±") + 1, "{out}");
    // A placeholder ends where the column ends, as the numbers do.
    let line = |text: &str| {
        out.lines()
            .find(|line| line.contains(text))
            .unwrap()
            .trim_end()
            .to_string()
    };
    assert_eq!(
        render::width(&line("—")),
        render::width(&line("1.5 ±")),
        "{out}"
    );
}

#[test]
fn a_numeric_first_column_is_aligned_as_numbers() {
    // The first column was always a name's, left-aligned: `-c og,name`
    // asked for a number first and got it read as text.
    let out = render::table(
        headers(&["og", "name"]),
        rows(&[&["1.049", "blonde-saison"], &["12.100", "dry-stout"]]),
        wide(),
        Style::Plain,
    );
    assert_eq!(
        column_of(&out, "1.049"),
        column_of(&out, "12.100") + 1,
        "{out}"
    );
}

#[test]
fn a_column_of_several_quantities_ends_its_numbers_at_its_right() {
    // compute's *before* column holds a value per row, each its own
    // quantity: points aligned across them meant nothing and read ragged.
    let out = render::table(
        headers(&["value", "before"]),
        rows(&[
            &["abv", "6.8 ± 0.1 %"],
            &["attenuation", "82 %"],
            &["drop", "14.0 ± 2.4 pt/day"],
        ]),
        wide(),
        Style::Plain,
    );
    let ends: Vec<usize> = out
        .lines()
        .skip(2)
        .map(|line| render::width(line.trim_end()))
        .collect();
    assert!(ends.windows(2).all(|pair| pair[0] == pair[1]), "{out}");
}

#[test]
fn a_hyphenated_name_is_never_split_at_its_hyphen() {
    // textwrap hyphenates by default: `farmhouse-saison` broke at its
    // hyphen while its column could hold it whole.
    let lines = render::wrap("farmhouse-saison and blonde-saison", 20);
    assert!(
        lines.iter().any(|line| line.contains("farmhouse-saison")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("blonde-saison")),
        "{lines:?}"
    );
}

#[test]
fn a_long_unbreakable_name_wraps_before_a_column_is_hidden() {
    // A name no space breaks hid every asked-for column at a width that
    // could show them all, wrapped.
    let out = render::table(
        headers(&["name", "og", "fg"]),
        rows(&[&["黒ビール燻製ポーター特別醸造版", "1.066", "1.012"]]),
        Target::Terminal { width: 30 },
        Style::Plain,
    );
    assert!(!out.contains("hidden"), "{out}");
    assert!(out.contains("1.066") && out.contains("1.012"), "{out}");
}
