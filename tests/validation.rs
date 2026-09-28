//! The tests of `validation`.

use std::fs;
use std::path::PathBuf;

use samplekit::collection::sample_list as list;
use samplekit::collection::validation::{self as validate, Detail, Severity};
use samplekit::config::project_config as config;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::Property;
use samplekit::core::sample::Sample;
use samplekit::core::uncertainty::{Convention, Uncertainty};
use samplekit::core::value::{Readings, Value, ValueKind};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn readings(values: &[f64]) -> Readings {
    Readings::new(values.to_vec()).unwrap()
}

/// A sample holding one property, named so a finding can be read.
fn sample(name: &str, quantity: &str, property: Property) -> Sample {
    let mut sample = Sample::new();
    sample.set_name(Some(name.to_string()));
    sample.set_property(id(quantity), property).unwrap();
    sample
}

fn with_unit(mut property: Property, unit: &str) -> Property {
    property.set_presentation(Presentation {
        unit: Some(unit.to_string()),
        symbol: None,
        precision: None,
    });
    property
}

/// Readings with an uncertainty nobody derived: the hand-edit shape.
fn stated(values: &[f64], uncertainty: f64) -> Property {
    let mut property = Property::measured(readings(values), None);
    property.set_uncertainty(Some(Uncertainty::new(uncertainty).unwrap()));
    property
}

/// What a file states about where an uncertainty came from. Judging reads this
/// and never the property: a convention a model declares in a session says what
/// a number *should* be, and only a file saying so stands in for the model a
/// reader does not have.
fn declares(convention: Convention) -> samplekit::core::property::Records {
    samplekit::core::property::Records {
        statistics: Some(samplekit::core::property::DeclaredStatistics {
            value: None,
            uncertainty: Some(convention),
        }),
        ..Default::default()
    }
}

fn details(report: &validate::Report) -> Vec<&Detail> {
    report.findings.iter().map(|f| &f.detail).collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-validate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn write(&self, file: &str, body: &str) -> PathBuf {
        let path = self.0.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_declared_statistic_that_disagrees_is_a_defect() {
    // The file states which statistic the channel came from, so a number that
    // is not it is evidence rather than a guess — the condition set for it.
    let mut malt = stated(&[12.4, 12.5, 12.6], 5.0);
    malt.set_records(declares(Convention::StandardError));
    let report = validate::run(&list::from_samples(vec![sample("A", "malt", malt)]));
    assert!(validate::has_defects(&report), "{:?}", details(&report));
    let Some(Detail::DeclaredStatisticDisagrees {
        quantity,
        channel,
        declared,
        stored,
        expected,
    }) = details(&report).first().cloned()
    else {
        panic!("{:?}", details(&report));
    };
    assert_eq!(quantity, "malt");
    assert_eq!(*channel, "uncertainty");
    assert_eq!(*declared, "standard_error");
    assert!(stored.starts_with('5'), "the file's number: {stored}");
    assert!(
        expected.starts_with("0.057"),
        "what the readings give: {expected}"
    );
    assert_eq!(report.findings[0].severity, Severity::Defect);
}

#[test]
fn a_statistic_its_readings_moved_under_is_stale_not_a_defect() {
    // New readings after the statistic was taken — `set --readings`, used as
    // documented — leave the old readings' number beside the new ones: stale,
    // which status says, and not a disagreement. Called a defect, it failed
    // `validate` over the documented workflow.
    let then =
        samplekit::format::fingerprint::of_readings(&samplekit::format::schema::PropertySchema {
            readings: Some(vec![1.0, 2.0, 3.0]),
            ..Default::default()
        })
        .unwrap();
    let mut malt = stated(&[12.4, 12.5, 12.6], 5.0);
    let mut records = declares(Convention::StandardError);
    records.computed = Some(indexmap::IndexMap::from([(
        samplekit::core::property::InputName::Named(samplekit::format::fingerprint::readings_key()),
        samplekit::core::property::InputRecord::Digest(then),
    )]));
    malt.set_records(records);
    let report = validate::run(&list::from_samples(vec![sample("A", "malt", malt)]));
    assert!(
        !details(&report)
            .iter()
            .any(|detail| matches!(detail, Detail::DeclaredStatisticDisagrees { .. })),
        "{:?}",
        details(&report)
    );
}

#[test]
fn a_declared_statistic_that_agrees_is_no_finding() {
    // Nothing to say: the file stated what the number should be, and it is.
    let mut malt = stated(&[12.4, 12.5, 12.6], 0.1 / 3.0_f64.sqrt());
    malt.set_records(declares(Convention::StandardError));
    let report = validate::run(&list::from_samples(vec![sample("A", "malt", malt)]));
    assert!(!validate::has_defects(&report), "{:?}", details(&report));
    assert!(
        details(&report).is_empty(),
        "not even the note that names a match: {:?}",
        details(&report)
    );
}

#[test]
fn a_single_reading_is_not_checked() {
    // No convention derives an uncertainty from one reading, so a number stored
    // beside one came from somewhere this module cannot see.
    let collection = list::from_samples(vec![sample("A", "malt", stated(&[12.5], 0.2))]);
    let report = validate::run(&collection);
    assert!(report.findings.is_empty(), "{:?}", details(&report));
}

#[test]
fn a_property_without_readings_is_not_checked() {
    let mut property = Property::stored(Value::number(12.5).unwrap());
    property.set_uncertainty(Some(Uncertainty::new(0.2).unwrap()));
    let collection = list::from_samples(vec![sample("A", "malt", property)]);
    assert!(validate::run(&collection).findings.is_empty());
}

#[test]
fn a_unit_that_differs_across_the_collection_is_reported() {
    // `g/L` on two casks and `kg/hL` on one, without knowing what
    // either means: a unit is an uninterpreted string.
    let collection = list::from_samples(vec![
        sample(
            "A",
            "brix",
            with_unit(Property::stored(Value::number(2.8).unwrap()), "g/L"),
        ),
        sample(
            "B",
            "brix",
            with_unit(Property::stored(Value::number(2.7).unwrap()), "g/L"),
        ),
        sample(
            "C",
            "brix",
            with_unit(Property::stored(Value::number(2800.0).unwrap()), "kg/hL"),
        ),
    ]);
    let report = validate::run(&collection);
    let Some(Detail::UnitDisagrees {
        quantity,
        spellings,
    }) = details(&report).first().cloned()
    else {
        panic!("{:?}", details(&report));
    };
    assert_eq!(quantity, "brix");
    assert_eq!(
        spellings,
        &[("g/L".to_string(), 2), ("kg/hL".to_string(), 1)]
    );
    // One finding for the quantity, not one per file.
    assert_eq!(report.findings.len(), 1);
    // And it points at the rarest spelling, which is the file to open first.
    assert_eq!(report.findings[0].sample, "C");
}

#[test]
fn a_missing_unit_is_not_a_disagreement() {
    // Absence is already visible in the file, and a cask not yet measured
    // is ordinary.
    let collection = list::from_samples(vec![
        sample(
            "A",
            "brix",
            with_unit(Property::stored(Value::number(2.8).unwrap()), "g/L"),
        ),
        sample("B", "brix", Property::stored(Value::Absent)),
    ]);
    assert!(validate::run(&collection).findings.is_empty());
}

#[test]
fn table_columns_are_checked_like_properties() {
    // A column is a quantity too, and its unit is declared once for the column.
    let scratch = Scratch::new("table-columns");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\ntables:\n  mashing:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      wort: {unit: lintner}\n    rows:\n      \
         - {temperature: 293.0, wort: 104.2}\n---\nNotes.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\ntables:\n  mashing:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      wort: {unit: klintner}\n    rows:\n      \
         - {temperature: 293.0, wort: 0.1042}\n---\nNotes.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let disagreements: Vec<&Detail> = report
        .findings
        .iter()
        .map(|finding| &finding.detail)
        .filter(|detail| matches!(detail, Detail::UnitDisagrees { .. }))
        .collect();
    assert_eq!(disagreements.len(), 1, "{:?}", details(&report));
    assert!(
        matches!(
            disagreements[0],
            Detail::UnitDisagrees { quantity, .. } if quantity == "mashing.wort"
        ),
        "{:?}",
        disagreements[0]
    );
}

/// A file contradicting the unit the project declares is a defect, and the
/// message does not say whose mistake it is.
///
/// It replaces a check that measured every unit against the keys of
/// `[unit.*]`, which made writing one display form turn every other unit in
/// the collection into a defect.
#[test]
fn a_unit_that_contradicts_the_declaration_is_reported() {
    let scratch = Scratch::new("contradiction");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.brix]\nunit = \"g/L\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  brix: {v: 2.8, unit: g/dl}\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let Some(Detail::UnitContradictsDeclaration {
        quantity,
        written,
        declared,
        ..
    }) = details(&report)
        .into_iter()
        .find(|detail| matches!(detail, Detail::UnitContradictsDeclaration { .. }))
    else {
        panic!("{:?}", details(&report));
    };
    assert_eq!(quantity, "brix");
    assert_eq!(written, "g/dl");
    assert_eq!(declared, "g/L");
}

/// The case nothing caught before: the project declares one unit and **every**
/// file writes another, so the spellings agree with each other and disagree
/// with the project.
#[test]
fn a_whole_collection_contradicting_the_declaration_is_reported() {
    let scratch = Scratch::new("all-contradict");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.malt]\nunit = \"g\"\n",
    );
    for name in ["a", "b", "c"] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {{v: 1.0, unit: kg}}\n---\nN.\n"
            ),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        details(&report).iter().any(|detail| matches!(
            detail,
            Detail::UnitContradictsDeclaration { written, samples, .. }
                if written == "kg" && *samples == 3
        )),
        "{:?}",
        details(&report)
    );
}

/// A unit nowhere declared is ordinary, not a defect and not a note: a
/// declaration is optional everywhere.
#[test]
fn without_a_declared_unit_only_homogeneity_is_checked() {
    let scratch = Scratch::new("no-declaration");
    // A display form for one unit, which used to close the vocabulary.
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[unit.\"g/L\"]\nplain = \"g/L\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  brix: {v: 2.8, unit: g/dl}\n  \
         malt: {v: 1.0, unit: g}\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    assert!(report.findings.is_empty(), "{:?}", details(&report));
}

#[test]
fn a_precision_that_cannot_apply_is_reported() {
    // `.3f` on a date is a declaration about a number, made where no number is.
    // A precision is the project's, so that is where it is declared.
    let scratch = Scratch::new("cannot-apply");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.brewed_on]\nprecision = \".3f\"\n\
         [property.brix]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  brewed_on: 2026-09-12\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(validate::has_defects(&report));
    assert!(
        matches!(
            details(&report).first(),
            Some(Detail::PrecisionCannotApply {
                kind: ValueKind::Date,
                ..
            })
        ),
        "{:?}",
        details(&report)
    );
    // An unmeasured quantity is ordinary, and its declaration is a statement
    // about the number it will hold.
    fs::remove_file(scratch.0.join("a.md")).unwrap();
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  brix: {unit: g/dl}\n---\nN.\n",
    );
    let absent = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(absent.findings.is_empty(), "{:?}", details(&absent));
}

#[test]
fn a_precision_that_writes_a_value_as_zero_is_noted() {
    // A screen shows `0.000030` or `<0.001`, but a file, an export and a
    // template keep the declared form — so this is where a reader learns it is
    // the declaration, not the measurement, that says nothing.
    let scratch = Scratch::new("writes-zero");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.foaming]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  foaming: 3.0e-5\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(!validate::has_defects(&report));
    let Some(Detail::PrecisionWritesZero {
        quantity,
        specifier,
        value,
    }) = details(&report).first().copied()
    else {
        panic!("{:?}", details(&report));
    };
    assert_eq!(quantity, "foaming");
    assert_eq!(specifier, ".3f");
    assert!(value.contains("3"), "{value}");
    assert_eq!(report.findings[0].severity, Severity::Note);

    // An exact zero is a measurement, and a value the specifier keeps a digit
    // of is nothing to report.
    fs::remove_file(scratch.0.join("a.md")).unwrap();
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  foaming: 0.0\n---\nN.\n",
    );
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  foaming: 1.25\n---\nN.\n",
    );
    let quiet = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(quiet.findings.is_empty(), "{:?}", details(&quiet));
}

#[test]
fn a_project_declaration_the_files_shadow_is_noted() {
    // A file wins over the project, and nothing said so. The owner declared a
    // precision, saw nothing change, and concluded the syntax was wrong. A file
    // holds no precision any more; a symbol is the one declaration a file can
    // still shadow.
    let scratch = Scratch::new("shadowed");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.grist]\nsymbol = \"m\"\n\
         [property.height]\nsymbol = \"h\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  grist: {v: 12.5, symbol: w}\n  \
         collar: {v: 4.0, symbol: h}\n---\nNotes.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  grist: {v: 13.5, symbol: w}\n  \
         collar: {v: 4.5}\n---\nNotes.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let shadowed: Vec<&Detail> = report
        .findings
        .iter()
        .map(|finding| &finding.detail)
        .filter(|detail| matches!(detail, Detail::DeclarationIsShadowed { .. }))
        .collect();
    // `collar` agrees where it is written and is absent where it is not:
    // neither is a shadow. `grist` is one note for both files.
    assert_eq!(shadowed.len(), 1, "{:?}", details(&report));
    let Detail::DeclarationIsShadowed {
        quantity,
        channel,
        declared,
        written,
        samples,
    } = shadowed[0]
    else {
        panic!("{:?}", shadowed[0]);
    };
    assert_eq!(quantity, "grist");
    assert_eq!(*channel, "symbol");
    assert_eq!(declared, "m");
    assert_eq!(written, "w");
    assert_eq!(*samples, 2);
    assert!(!validate::has_defects(&report));
}

#[test]
fn a_file_that_is_not_canonical_is_a_note_not_a_defect() {
    // A file a migration would rewrite is not an invalid file.
    let scratch = Scratch::new("canonical");
    scratch.write(
        "a.md",
        "---\nname: A\nschema_version: 1\nproperties:\n  malt: {v: 12.5}\n---\nNotes.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    assert_eq!(details(&report), [&Detail::NotCanonical]);
    assert_eq!(report.findings[0].severity, Severity::Note);
    assert!(!validate::has_defects(&report));
    assert!(report.findings[0].path.is_some());
}

#[test]
fn stale_values_are_one_note_per_sample() {
    // A stale value is work to do, not something wrong in the data — one note
    // counting them, never a defect.
    let scratch = Scratch::new("stale");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt:\n    v: 12.5\n  \
         plato:\n    v: 2.8\n    computed:\n      malt: 0123456789ab\n  \
         haze:\n    v: 0.1\n    computed:\n      malt: 0123456789ab\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let stale: Vec<&Detail> = details(&report)
        .into_iter()
        .filter(|detail| matches!(detail, Detail::NotCurrent { .. }))
        .collect();
    assert_eq!(
        stale,
        [&Detail::NotCurrent { values: 2 }],
        "{:?}",
        details(&report)
    );
    assert!(!validate::has_defects(&report));
}

#[test]
fn records_forming_a_cycle_are_a_defect() {
    // No write produces a cycle, so the file says something impossible; one
    // defect naming the path, and the sample is still validated.
    let scratch = Scratch::new("cycle");
    // The digests match, as they do in a file whose values nobody touched: a
    // record that differs is stale before the walk ever reaches the cycle.
    let one = samplekit::format::fingerprint::of(&samplekit::format::schema::PropertySchema {
        statistics: None,
        value: Some(Value::Integer(1)),
        ..Default::default()
    });
    scratch.write(
        "a.md",
        &format!(
            "---\nschema_version: 1\nname: A\nproperties:\n  \
             a:\n    v: 1\n    computed:\n      b: {one}\n  \
             b:\n    v: 1\n    computed:\n      a: {one}\n---\nN.\n"
        ),
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let found: Vec<&Detail> = details(&report)
        .into_iter()
        .filter(|detail| matches!(detail, Detail::RecordsCannotBeFollowed { .. }))
        .collect();
    let [Detail::RecordsCannotBeFollowed { reason, others, .. }] = found.as_slice() else {
        panic!("{:?}", details(&report));
    };
    assert!(reason.contains('\u{2192}'), "{reason}");
    assert_eq!(*others, 1);
    assert!(validate::has_defects(&report));
}

#[test]
fn a_clean_collection_produces_no_findings() {
    // The check is not a generator of noise: a file that is right produces
    // nothing at all.
    let scratch = Scratch::new("clean");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  \
         malt: {v: 12.5, unit: g}\n---\nNotes.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    assert!(report.findings.is_empty(), "{:?}", details(&report));
    assert_eq!(report.samples, 1);
}

#[test]
fn a_report_counts_what_it_looked_at() {
    // A finding count without a denominator is not a result.
    let collection = list::from_samples(vec![
        sample("A", "malt", Property::stored(Value::number(1.0).unwrap())),
        sample("B", "malt", Property::stored(Value::number(2.0).unwrap())),
    ]);
    assert_eq!(validate::run(&collection).samples, 2);
    assert_eq!(validate::run(&list::empty()).samples, 0);
}

#[test]
fn each_sample_is_checked_against_its_own_project_units() {
    let scratch = Scratch::new("nested-units");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[property.malt]\nunit = \"g\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 2.8, unit: g}\n---\nN.\n",
    );
    fs::create_dir_all(scratch.0.join("cellar")).unwrap();
    scratch.write(
        "cellar/.samplekitrc",
        "schema_version = 1\n[property.pressure]\nunit = \"bar\"\n",
    );
    scratch.write(
        "cellar/p.md",
        "---\nschema_version: 1\nname: P\nproperties:\n  pressure: {v: 3.6, unit: bar}\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 2);
    let report = validate::run(&collection);
    assert!(
        !details(&report)
            .iter()
            .any(|detail| matches!(detail, Detail::UnitContradictsDeclaration { .. })),
        "{:?}",
        details(&report)
    );
    scratch.write(
        "cellar/p.md",
        "---\nschema_version: 1\nname: P\nproperties:\n  pressure: {v: 3.6, unit: mbar}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        details(&report).iter().any(
            |detail| matches!(detail, Detail::UnitContradictsDeclaration { written, .. } if written == "mbar")
        ),
        "{:?}",
        details(&report)
    );
}

#[test]
fn an_unreadable_file_is_a_defect() {
    let scratch = Scratch::new("unreadable");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  brix: {v: 2.8}\n---\nN.\n",
    );
    scratch.write("b.md", "---\nschema_version: 1\nname: [oops\n---\nN.\n");
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    assert!(
        details(&report)
            .iter()
            .any(|detail| matches!(detail, Detail::Unreadable { .. })),
        "{:?}",
        details(&report)
    );
    assert!(validate::has_defects(&report));
}

#[test]
fn a_name_held_by_two_files_is_a_defect() {
    let scratch = Scratch::new("duplicate-name");
    scratch.write("a.md", "---\nschema_version: 1\nname: BR-01\n---\nN.\n");
    scratch.write("b.md", "---\nschema_version: 1\nname: BR-01\n---\nN.\n");
    scratch.write("c.md", "---\nschema_version: 1\nname: BR-02\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let duplicates: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::DuplicateName { .. }))
        .collect();
    assert_eq!(duplicates.len(), 1, "{:?}", report.findings);
    assert_eq!(duplicates[0].severity, Severity::Defect);
    let Detail::DuplicateName { name, files } = &duplicates[0].detail else {
        unreachable!()
    };
    assert_eq!(name, "BR-01");
    assert_eq!(files.len(), 2, "{files:?}");
}

#[test]
fn a_name_two_files_give_alike_is_a_defect() {
    // A name is the file's where none is written, and two samples of one
    // project named alike by it are as ambiguous as two `name:`.
    let scratch = Scratch::new("duplicate-file-name");
    fs::create_dir_all(scratch.0.join("x")).unwrap();
    fs::create_dir_all(scratch.0.join("y")).unwrap();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write("x/s-1.md", "---\nschema_version: 1\n---\n");
    scratch.write("y/s-1.md", "---\nschema_version: 1\n---\n");
    scratch.write("a.md", "---\nschema_version: 1\nname: s-2\n---\n");
    scratch.write("s-2.md", "---\nschema_version: 1\n---\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let mut duplicates: Vec<(String, usize)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DuplicateName { name, files } => Some((name.clone(), files.len())),
            _ => None,
        })
        .collect();
    duplicates.sort();
    assert_eq!(
        duplicates,
        [("s-1".to_string(), 2), ("s-2".to_string(), 2)],
        "{:?}",
        report.findings
    );
}

#[test]
fn text_in_a_numeric_quantity_is_a_defect() {
    let scratch = Scratch::new("kind-disagrees");
    for (file, value) in [("a.md", "2.0"), ("b.md", "3"), ("c.md", "\"ninety\"")] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  malt: {{v: {value}}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::KindDisagrees { .. }))
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(finding.severity, Severity::Defect);
    assert!(finding.path.as_ref().unwrap().ends_with("c.md"));
    assert_eq!(
        finding.detail,
        Detail::KindDisagrees {
            quantity: "malt".to_string(),
            kinds: vec![("number".to_string(), 2), ("text".to_string(), 1)],
        }
    );
}

#[test]
fn an_invalid_date_among_dates_is_a_defect() {
    let scratch = Scratch::new("date-kinds");
    for (file, made) in [
        ("a.md", "2026-03-01"),
        ("b.md", "2026-04-01"),
        ("c.md", "2026-13-45"),
    ] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nmade: {made}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::KindDisagrees { .. }))
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert!(finding.path.as_ref().unwrap().ends_with("c.md"));
    assert_eq!(
        finding.detail,
        Detail::KindDisagrees {
            quantity: "made".to_string(),
            kinds: vec![("date".to_string(), 2), ("text".to_string(), 1)],
        }
    );
}

#[test]
fn a_template_brace_near_a_channel_is_noted() {
    // `{valeu}` prints `{valeu}` in a manuscript table. A command's argument and
    // a word far from every channel are LaTeX's, and are left alone.
    let scratch = Scratch::new("template-brace");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\", template = \
         \"\\\\num{{value:.3f}} \\\\text{valeur} {valeu} {untis} {table} \\\\si{{unit}}\"}]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let noted: Vec<(String, String)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::TemplateBraceNearAChannel {
                written, channel, ..
            } => {
                assert_eq!(finding.severity, Severity::Note);
                Some((written.clone(), channel.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        noted,
        [
            ("valeu".to_string(), "value".to_string()),
            ("untis".to_string(), "unit".to_string())
        ]
    );
    assert!(!validate::has_defects(&report));
}

#[test]
fn a_declaration_naming_no_field_is_a_defect() {
    let scratch = Scratch::new("declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[query.ghost]\nfilter = \"ghost_field > 3\"\n[profile.p]\ncolumns = [{field = \"nme\"}]\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nproperties:\n  malt: {{v: 2.0}}\n---\nN.\n"
            ),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let declared: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DeclarationNamesNoField { declaration, .. } => Some(declaration.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(declared, ["query 'ghost'", "profile 'p'"]);
    assert!(validate::has_defects(&report));
}

#[test]
fn a_figure_naming_no_field_is_a_defect() {
    // A figure is a declaration like a profile: its axes and its group are
    // checked, a whole column of a table included, with one sample as with many.
    let scratch = Scratch::new("figure-declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n\
         [figure.good]\nx = \"m.T\"\ny = \"m.ebc\"\ngroup = \"tags\"\n\
         [figure.typo]\nx = \"mal\"\ny = \"malt\"\ngroup = \"beet\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nbeer: schwarz\nproperties:\n  malt: {v: 2.0}\n\
         tables:\n  m:\n    index: T\n    columns:\n      T: {}\n      ebc: {}\n    rows:\n      \
         - {T: 20.0, ebc: 5.1}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let declared: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DeclarationNamesNoField {
                declaration,
                reason,
            } => Some(format!("{declaration}: {reason}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        declared,
        [
            "figure 'typo': no sample has 'mal' — did you mean 'malt'?",
            "figure 'typo': no sample has 'beet' — did you mean 'beer'?",
        ]
    );
    assert!(validate::has_defects(&report));
}

#[test]
fn a_model_that_cannot_be_found_is_a_defect() {
    let scratch = Scratch::new("model-unusable");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model/missing.py\"\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        details(&report)
            .iter()
            .any(|detail| matches!(detail, Detail::ModelUnusable { reason } if reason.contains("does not exist"))),
        "{:?}",
        details(&report)
    );
    fs::create_dir_all(scratch.0.join("model")).unwrap();
    scratch.write("model/missing.py", "class Other:\n    pass\n");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model/missing.py\"\nclass = \"NoSuchCell\"\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        details(&report)
            .iter()
            .any(|detail| matches!(detail, Detail::ModelUnusable { reason } if reason.contains("NoSuchCell"))),
        "{:?}",
        details(&report)
    );
}

#[test]
fn a_key_written_twice_is_a_defect() {
    let scratch = Scratch::new("duplicate-key");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nyeast_strain: US05\nyeast_strain: S04\nproperties:\n  malt: {v: 1.0}\n  malt: {v: 2.0}\n---\nN.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nyeast_strain: US05\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let keys: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DuplicateKey { key } => Some(key.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(keys, ["yeast_strain", "properties.malt"]);
}

#[test]
fn a_key_repeated_inside_text_is_no_defect() {
    // A protocol's steps in a block scalar, a flow list and a quoted scalar
    // over several lines are text, not keys.
    let scratch = Scratch::new("keys-in-text");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nprotocol: |\n  step: a\n  step: b\n\n  step: c\n\
         notes: >-\n  step: d\n  step: e\nsteps:\n  - |\n    step: f\n    step: g\n\
         flow: [\n  'step: h',\n  'step: i']\nquoted: \"step: j\n  step: k\"\n\
         properties:\n  malt: {v: 1.0}\n---\nN.\n",
    );
    // A block scalar ends where its indentation does: a key repeated after it
    // is still found.
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nprotocol: |\n  step: a\nyeast_strain: US05\nyeast_strain: S04\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let keys: Vec<(String, String)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DuplicateKey { key } => Some((finding.sample.clone(), key.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(keys, [("B".to_string(), "yeast_strain".to_string())]);
}

#[test]
fn one_name_in_two_projects_is_no_defect() {
    // A nested project keeps its own names, as `new` allows; within one
    // project a name shared is still a defect.
    let scratch = Scratch::new("name-per-project");
    fs::create_dir_all(scratch.0.join("nested")).unwrap();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nname: X\n---\n");
    scratch.write("nested/.samplekitrc", "schema_version = 1\n");
    scratch.write("nested/b.md", "---\nschema_version: 1\nname: X\n---\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| matches!(finding.detail, Detail::DuplicateName { .. })),
        "{:?}",
        report.findings
    );
    scratch.write("nested/c.md", "---\nschema_version: 1\nname: X\n---\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let files: Vec<usize> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DuplicateName { files, .. } => Some(files.len()),
            _ => None,
        })
        .collect();
    assert_eq!(files, [2]);
}

#[test]
fn a_name_two_files_share_is_shown_with_its_file() {
    let scratch = Scratch::new("shared-name");
    for (file, name, value) in [
        ("a.md", "X", "2.0"),
        ("b.md", "X", "\"heavy\""),
        ("c.md", "Y", "3.0"),
    ] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {{v: {value}}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::KindDisagrees { .. }))
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(finding.sample, "X (b.md)");
}

#[test]
fn a_partial_target_reports_declarations_as_notes() {
    let scratch = Scratch::new("partial-target");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[query.ghost]\nfilter = \"ghost_field > 3\"\n",
    );
    fs::create_dir_all(scratch.0.join("sub")).unwrap();
    for file in ["sub/a.md", "sub/b.md"] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0.join("sub")).unwrap());
    let found: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::DeclarationNamesNoField { .. }))
        .collect();
    assert!(!found.is_empty(), "{:?}", report.findings);
    assert!(
        found
            .iter()
            .all(|finding| finding.severity == Severity::Note)
    );
}

#[test]
fn a_sub_project_declaration_is_checked_against_its_own_samples() {
    let scratch = Scratch::new("sub-project-declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[query.root_query]\nfilter = \"special > 1\"\n[profile.root_profile]\ncolumns = [{field = \"extra\"}]\n",
    );
    fs::create_dir_all(scratch.0.join("nested")).unwrap();
    scratch.write(
        "nested/.samplekitrc",
        "schema_version = 1\n[query.nested_query]\nfilter = \"special > 1\"\n[profile.nested_profile]\ncolumns = [{field = \"extra\"}]\n",
    );
    for (file, property) in [
        ("a.md", "special"),
        ("b.md", "special"),
        ("nested/c.md", "extra"),
        ("nested/d.md", "extra"),
    ] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  {property}: {{v: 2.0}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let mut declared: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DeclarationNamesNoField { declaration, .. } => Some(declaration.clone()),
            _ => None,
        })
        .collect();
    declared.sort();
    declared.dedup();
    assert_eq!(declared, ["profile 'root_profile'", "query 'nested_query'"]);
}

#[test]
fn a_text_that_reads_as_a_number_is_a_defect() {
    let scratch = Scratch::new("number-expected");
    for (file, value) in [("a.md", "\"12,5\""), ("b.md", "13.0")] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  malt: {{v: {value}, unit: mg}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        report.findings.iter().any(|finding| matches!(
            &finding.detail,
            Detail::NumberExpected { text, .. } if text == "12,5"
        )),
        "{:?}",
        report.findings
    );
}

#[test]
fn a_sample_without_its_version_is_a_note() {
    let scratch = Scratch::new("missing-version");
    scratch.write("a.md", "---\nname: a\n---\nN.\n");
    scratch.write("b.md", "---\nschema_version: 1\nname: b\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let notes: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::MissingVersion))
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", report.findings);
    assert_eq!(notes[0].severity, Severity::Note);
}

#[test]
fn a_defect_in_a_part_is_reported_and_the_rest_validated() {
    let mut defective = Sample::new();
    defective.set_name(Some("A".to_string()));
    defective.set_unusable_tags(vec!["my tag".to_string()]);
    defective.set_aside_table(id("conditioning"), "duplicate index".to_string());
    let collection = list::from_samples(vec![
        defective,
        sample("B", "malt", stated(&[12.4, 12.5, 12.6], 5.0)),
    ]);
    let report = validate::run(&collection);
    assert!(validate::has_defects(&report));
    let found = details(&report);
    assert!(
        found
            .iter()
            .any(|detail| matches!(detail, Detail::UnusableTag { tag, .. } if tag == "my tag")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|detail| matches!(detail, Detail::TableSetAside { .. })),
        "{found:?}"
    );
}

#[test]
fn a_configuration_given_instead_of_the_nearest_reports_declarations_as_defects() {
    let scratch = Scratch::new("forced-configuration");
    fs::create_dir_all(scratch.0.join("rc")).unwrap();
    fs::create_dir_all(scratch.0.join("samples")).unwrap();
    scratch.write(
        "rc/.samplekitrc",
        "schema_version = 1\n[query.ghost]\nfilter = \"ghost_field > 3\"\n",
    );
    for file in ["samples/a.md", "samples/b.md"] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\n---\nN.\n"),
        );
    }
    let given = config::load(&scratch.0.join("rc/.samplekitrc")).unwrap();
    let report =
        validate::run(&list::from_directory_with(&scratch.0.join("samples"), Some(given)).unwrap());
    let found: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::DeclarationNamesNoField { .. }))
        .collect();
    assert!(!found.is_empty(), "{:?}", report.findings);
    assert!(
        found
            .iter()
            .all(|finding| finding.severity == Severity::Defect)
    );
}

#[test]
fn an_uncertainty_a_precision_writes_as_zero_is_noted() {
    // The value channel alone was looked at: `u: 0.0004` under `.2f` is
    // written `0.0` in every file and export — a claim of a perfect
    // measurement — and beside a value of exactly zero nothing was said.
    let scratch = Scratch::new("uncertainty-zero");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.tiny]\nprecision = \".2f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  tiny: {v: 0.0, u: 0.0004}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        details(&report).iter().any(|detail| matches!(
            detail,
            Detail::PrecisionWritesZero { quantity, .. } if quantity == "tiny.u"
        )),
        "{:?}",
        details(&report)
    );
}

#[test]
fn an_override_is_not_counted_as_work_to_do() {
    // The invariant the note states and nothing tested: `NotCurrent` counts
    // stale, broken and failed values, never one a hand wrote over a formula
    // — counting it would invite a `compute` over a deliberate correction.
    let scratch = Scratch::new("override-not-work");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt:\n    v: 12.5\n  \
         plato:\n    v: 2.8\n    computed:\n      malt: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n  \
         haze:\n    v: 0.1\n    computed:\n      malt: 0123456789ab\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let counted: Vec<&Detail> = details(&report)
        .into_iter()
        .filter(|detail| matches!(detail, Detail::NotCurrent { .. }))
        .collect();
    // haze is stale; plato is an override, and is not counted.
    assert_eq!(
        counted,
        [&Detail::NotCurrent { values: 1 }],
        "{:?}",
        details(&report)
    );
}

#[test]
fn an_override_is_noted_and_is_not_a_defect() {
    // On a collection whose every value was an override, `validate` said
    // nothing while `status` listed 2 774 values. Each is counted, apart from
    // the work to do, and none makes the report fail.
    let scratch = Scratch::new("override-noted");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt:\n    v: 12.5\n  \
         plato:\n    v: 2.8\n    computed:\n      malt: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n  \
         volume:\n    v: 4.4\n    computed:\n      malt: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let noted: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::Overridden { .. }))
        .collect();
    assert_eq!(noted.len(), 1, "{:?}", details(&report));
    assert_eq!(noted[0].detail, Detail::Overridden { values: 2 });
    assert_eq!(noted[0].severity, Severity::Note);
    assert!(!validate::has_defects(&report));
}

#[test]
fn a_value_the_model_declares_is_not_a_misspelling() {
    // A profile or a query naming a value the model's source declares, which no
    // sample holds yet, is not a defect; a typo still is.
    let scratch = Scratch::new("model-declares");
    fs::create_dir_all(scratch.0.join("model")).unwrap();
    scratch.write(
        "model/cell.py",
        "import samplekit as sk\n\nclass Cell(sk.Sample):\n    def __init__(self, path=None):\n        \
         self.brix = sk.Property(compute=self._brix, depends_on=[])\n        \
         super().__init__(path)\n\n    def _brix(self):\n        return 1.0\n",
    );
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model/cell.py\"\n\
         [query.strong]\nfilter = \"brix > 3\"\n[profile.p]\ncolumns = [{field = \"brix\"}, {field = \"brixx\"}]\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nproperties:\n  malt: {{v: 2.0}}\n---\nN.\n"
            ),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let declared: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DeclarationNamesNoField {
                declaration,
                reason,
            } => Some(format!("{declaration}: {reason}")),
            _ => None,
        })
        .collect();
    assert_eq!(declared.len(), 1, "{declared:?}");
    assert!(declared[0].contains("brixx"), "{declared:?}");
}

#[test]
fn a_finding_names_the_value_it_is_about() {
    let about = Detail::NumberExpected {
        quantity: "malt".to_string(),
        text: "heavy".to_string(),
    };
    assert_eq!(about.quantity(), Some("malt"));
    let table = Detail::TableSetAside {
        table: "conditioning".to_string(),
        reason: "two rows at one index".to_string(),
    };
    assert_eq!(table.quantity(), Some("conditioning"));
    let file = Detail::Unreadable {
        reason: "not YAML".to_string(),
    };
    assert_eq!(file.quantity(), None);
}

#[test]
fn a_number_beside_readings_is_not_second_guessed() {
    // With nothing declared, an uncertainty or a value beside readings that no
    // statistic gives is the model's or a hand's, and not a finding.
    let report = validate::run(&list::from_samples(vec![sample(
        "A",
        "malt",
        stated(&[12.4, 12.5, 12.6], 5.0),
    )]));
    assert!(report.findings.is_empty(), "{:?}", details(&report));
}

#[test]
fn text_in_a_column_with_a_unit_is_a_defect() {
    // `sweetness=12.1,12.3,12.2` stored as text in a column measured in cP was
    // reported by nothing.
    let mut sample = Sample::new();
    sample.set_name(Some("V".to_string()));
    let mut meta = samplekit::core::table::ColumnMeta::default();
    meta.presentation.unit = Some("cP".to_string());
    let columns: indexmap::IndexMap<Identifier, samplekit::core::table::ColumnMeta> = [
        (id("T"), samplekit::core::table::ColumnMeta::default()),
        (id("sweetness"), meta),
    ]
    .into_iter()
    .collect();
    let mut table =
        samplekit::core::table::Table::new(id("mouthfeel"), vec![id("T")], columns, Vec::new())
            .unwrap();
    table
        .add_row(vec![
            (id("T"), Property::stored(Value::integer(20))),
            (
                id("sweetness"),
                Property::stored(Value::text("12.1,12.3,12.2")),
            ),
        ])
        .unwrap();
    sample.set_table(id("mouthfeel"), table).unwrap();
    let report = validate::run(&list::from_samples(vec![sample]));
    let found = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::NumberExpected { .. }))
        .expect("the text is found");
    assert_eq!(found.severity, Severity::Defect);
    let said = validate::described(found);
    assert!(said.contains("several readings"), "{said}");
}

#[test]
fn files_named_one_by_one_are_judged_alone() {
    // A query naming a field one file lacks is no finding about that file: it
    // cannot answer for the rest. Over the folder it is one.
    let scratch = Scratch::new("files-alone");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[query.bad]\nfilter = \"nosuch > 1\"\n",
    );
    let file = scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: 1.0\n---\n",
    );
    let alone = validate::run_on_files(&list::from_directory(&file).unwrap());
    assert!(
        !alone
            .findings
            .iter()
            .any(|finding| matches!(finding.detail, Detail::DeclarationNamesNoField { .. })),
        "{:?}",
        details(&alone)
    );
    let whole = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        whole
            .findings
            .iter()
            .any(|finding| matches!(finding.detail, Detail::DeclarationNamesNoField { .. })),
        "{:?}",
        details(&whole)
    );
}

#[test]
fn not_applicable_takes_no_precision() {
    // `n/a` has no digits for a precision to shape: `.3f` over it was a defect,
    // *cannot apply to a NotApplicable value*.
    let scratch = Scratch::new("na-precision");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.fg]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  fg: n/a\n---\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(!validate::has_defects(&report), "{:?}", details(&report));
}

/// The words `validate::states` gives each sample, in the list's order.
fn words_of(collection: &list::SampleList) -> Vec<Vec<&'static str>> {
    validate::states(collection)
        .iter()
        .map(|states| states.held().iter().map(|state| state.word()).collect())
        .collect()
}

#[test]
fn states_say_what_the_files_say_of_each_sample() {
    // Without the model, a sample's state is what its files say.
    let scratch = Scratch::new("states");
    scratch.write(
        "a-failed.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  malt: {v: 2.0}\n  \
         volume: {computed: {malt: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    scratch.write(
        "b-stale.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  malt: {v: 2.0}\n  \
         plato: {v: 2.8, computed: {malt: 0123456789ab}}\n---\nN.\n",
    );
    scratch.write(
        "c-edited.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  malt: {v: 2.0}\n  \
         plato:\n    v: 2.8\n    computed:\n      malt: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n---\nN.\n",
    );
    scratch.write(
        "d-clean.md",
        "---\nschema_version: 1\nname: D\nproperties:\n  malt: {v: 2.0}\n---\nN.\n",
    );
    scratch.write(
        "e-waiting.md",
        "---\nschema_version: 1\nname: E\nproperties:\n  malt: {}\n  \
         plato: {v: 2.8, computed: {malt: 0123456789ab}}\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(
        words_of(&collection),
        [
            vec!["failed"],
            vec!["outdated"],
            vec!["edited"],
            vec![],
            vec!["waiting"]
        ]
    );
    // The model was not read: nothing says a clean file is current.
    assert!(
        validate::states(&collection)
            .iter()
            .all(|states| !states.model_read())
    );
}

#[test]
fn a_defect_in_its_file_makes_a_sample_defective() {
    let scratch = Scratch::new("defective");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nyeast_strain: US05\nyeast_strain: S04\nproperties:\n  \
         malt: {v: 2.0}\n  plato: {v: 2.8, computed: {malt: 0123456789ab}}\n---\nN.\n",
    );
    // A note — no schema_version — is no defect.
    scratch.write("b.md", "---\nname: B\nyeast_strain: US05\n---\nN.\n");
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(
        words_of(&collection),
        [vec!["defective", "outdated"], vec![]]
    );
}

#[test]
fn a_copy_of_an_imported_declaration_is_a_note() {
    let scratch = Scratch::new("imported-copy");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.height]\nunit = \"cL\"\nsymbol = \"h\"\n\
         [property.malt]\nunit = \"g\"\nprecision = \".1f\"\n",
    );
    scratch.write(
        "c1/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n\
         [property.height]\nunit = \"cL\"\nsymbol = \"h\"\n\
         [property.malt]\nunit = \"g\"\nprecision = \".3f\"\n",
    );
    scratch.write("c1/a.md", "---\nschema_version: 1\nname: A\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0.join("c1")).unwrap());
    let root = dunce::canonicalize(scratch.0.join(".samplekitrc")).unwrap();
    let found: Vec<(Severity, String)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::CopiedDeclaration { key, from } => {
                assert_eq!(finding.sample, ".samplekitrc");
                assert_eq!(*from, root);
                Some((finding.severity, key.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        found,
        [
            (Severity::Note, "[property.height]".to_string()),
            (Severity::Note, "[property.malt] unit".to_string()),
        ]
    );
    assert!(!validate::has_defects(&report), "{report:?}");
    let said = validate::described(
        report
            .findings
            .iter()
            .find(|finding| matches!(finding.detail, Detail::CopiedDeclaration { .. }))
            .unwrap(),
    );
    assert!(said.contains("a copy to remove"), "{said}");
}

#[test]
fn an_import_not_found_or_a_loop_is_reported() {
    let scratch = Scratch::new("import-reported");
    fs::create_dir_all(scratch.0.join("empty")).unwrap();
    scratch.write(
        "c1/.samplekitrc",
        "schema_version = 1\nimport = \"../empty\"\n",
    );
    scratch.write("c1/a.md", "---\nschema_version: 1\nname: A\n---\nN.\n");
    scratch.write(
        "one/.samplekitrc",
        "schema_version = 1\nimport = \"../two\"\n",
    );
    scratch.write(
        "two/.samplekitrc",
        "schema_version = 1\nimport = \"../one\"\n",
    );
    scratch.write("one/b.md", "---\nschema_version: 1\nname: B\n---\nN.\n");
    for (folder, said) in [("c1", "import = \"../empty\""), ("one", "imports itself")] {
        let report = validate::run(&list::from_directory(&scratch.0.join(folder)).unwrap());
        let reported: Vec<String> = report
            .findings
            .iter()
            .filter(|finding| matches!(finding.detail, Detail::Unreadable { .. }))
            .map(validate::described)
            .collect();
        assert!(
            reported.iter().any(|reported| reported.contains(said)),
            "{folder}: {reported:?}"
        );
    }
}

#[test]
fn a_files_pattern_that_finds_nothing_is_a_defect() {
    let scratch = Scratch::new("files-pattern");
    fs::create_dir_all(scratch.0.join("images")).unwrap();
    fs::create_dir_all(scratch.0.join("raw")).unwrap();
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"images/{nmae}*.jpg\", \"images/[x\", \
         \"nowhere\", \"images/{name}*.{jpg,png}\", \"raw/{name}/**\"]\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nname: A\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let found: Vec<(Severity, String)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::FilesPatternUnusable { entry, .. } => {
                assert_eq!(finding.sample, ".samplekitrc");
                Some((finding.severity, entry.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        found,
        [
            (Severity::Defect, "images/{nmae}*.jpg".to_string()),
            (Severity::Defect, "images/[x".to_string()),
            (Severity::Note, "nowhere".to_string()),
        ]
    );
    let said = validate::described(
        report
            .findings
            .iter()
            .find(|finding| matches!(finding.detail, Detail::FilesPatternUnusable { .. }))
            .unwrap(),
    );
    assert!(said.contains("only {name} stands for"), "{said}");
}

#[test]
fn a_profile_grouping_by_no_field_is_a_defect() {
    // The fields a profile groups by are checked as its columns are.
    let scratch = Scratch::new("group-declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"malt\"}]\ngroup = [\"beet\"]\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nbeer: schwarz\n\
                 properties:\n  malt: {{v: 2.0}}\n---\nN.\n"
            ),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let said: Vec<(String, String)> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::DeclarationNamesNoField {
                declaration,
                reason,
            } => Some((declaration.clone(), reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, "profile 'p'");
    assert!(
        said[0].1.contains("beet") && said[0].1.contains("beer"),
        "{said:?}"
    );
    assert!(validate::has_defects(&report));
}
