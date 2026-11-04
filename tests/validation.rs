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
    let mut diameter = stated(&[12.4, 12.5, 12.6], 5.0);
    diameter.set_records(declares(Convention::StandardError));
    let report = validate::run(&list::from_samples(vec![sample("A", "diameter", diameter)]));
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
    assert_eq!(quantity, "diameter");
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
    let mut diameter = stated(&[12.4, 12.5, 12.6], 5.0);
    let mut records = declares(Convention::StandardError);
    records.computed = Some(indexmap::IndexMap::from([(
        samplekit::core::property::InputName::Named(samplekit::format::fingerprint::readings_key()),
        samplekit::core::property::InputRecord::Digest(then),
    )]));
    diameter.set_records(records);
    let report = validate::run(&list::from_samples(vec![sample("A", "diameter", diameter)]));
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
    let mut diameter = stated(&[12.4, 12.5, 12.6], 0.1 / 3.0_f64.sqrt());
    diameter.set_records(declares(Convention::StandardError));
    let report = validate::run(&list::from_samples(vec![sample("A", "diameter", diameter)]));
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
    let collection = list::from_samples(vec![sample("A", "diameter", stated(&[12.5], 0.2))]);
    let report = validate::run(&collection);
    assert!(report.findings.is_empty(), "{:?}", details(&report));
}

#[test]
fn a_property_without_readings_is_not_checked() {
    let mut property = Property::stored(Value::number(12.5).unwrap());
    property.set_uncertainty(Some(Uncertainty::new(0.2).unwrap()));
    let collection = list::from_samples(vec![sample("A", "diameter", property)]);
    assert!(validate::run(&collection).findings.is_empty());
}

#[test]
fn a_unit_that_differs_across_the_collection_is_reported() {
    // `g/L` on two slabs and `kg/hL` on one, without knowing what
    // either means: a unit is an uninterpreted string.
    let collection = list::from_samples(vec![
        sample(
            "A",
            "porosity",
            with_unit(Property::stored(Value::number(2.8).unwrap()), "g/L"),
        ),
        sample(
            "B",
            "porosity",
            with_unit(Property::stored(Value::number(2.7).unwrap()), "g/L"),
        ),
        sample(
            "C",
            "porosity",
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
    assert_eq!(quantity, "porosity");
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
    // Absence is already visible in the file, and a slab not yet measured
    // is ordinary.
    let collection = list::from_samples(vec![
        sample(
            "A",
            "porosity",
            with_unit(Property::stored(Value::number(2.8).unwrap()), "g/L"),
        ),
        sample("B", "porosity", Property::stored(Value::Absent)),
    ]);
    assert!(validate::run(&collection).findings.is_empty());
}

#[test]
fn table_columns_are_checked_like_properties() {
    // A column is a quantity too, and its unit is declared once for the column.
    let scratch = Scratch::new("table-columns");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\ntables:\n  preheating:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      tension: {unit: lintner}\n    rows:\n      \
         - {temperature: 293.0, tension: 104.2}\n---\nNotes.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\ntables:\n  preheating:\n    index: [temperature]\n    \
         columns:\n      temperature: {unit: degC}\n      tension: {unit: klintner}\n    rows:\n      \
         - {temperature: 293.0, tension: 0.1042}\n---\nNotes.\n",
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
            Detail::UnitDisagrees { quantity, .. } if quantity == "preheating.tension"
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
        "schema_version = 1\n[property.porosity]\nunit = \"g/L\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  porosity: {v: 2.8, unit: g/dl}\n---\nN.\n",
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
    assert_eq!(quantity, "porosity");
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
        "schema_version = 1\n[property.diameter]\nunit = \"g\"\n",
    );
    for name in ["a", "b", "c"] {
        scratch.write(
            &format!("{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: 1.0, unit: kg}}\n---\nN.\n"
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
        "---\nschema_version: 1\nname: A\nproperties:\n  porosity: {v: 2.8, unit: g/dl}\n  \
         diameter: {v: 1.0, unit: g}\n---\nN.\n",
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
        "schema_version = 1\n[property.poured_on]\nprecision = \".3f\"\n\
         [property.porosity]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  poured_on: 2026-09-12\n---\nN.\n",
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
        "---\nschema_version: 1\nname: B\nproperties:\n  porosity: {unit: g/dl}\n---\nN.\n",
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
        "schema_version = 1\n[property.wearing]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  wearing: 3.0e-5\n---\nN.\n",
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
    assert_eq!(quantity, "wearing");
    assert_eq!(specifier, ".3f");
    assert!(value.contains("3"), "{value}");
    assert_eq!(report.findings[0].severity, Severity::Note);

    // An exact zero is a measurement, and a value the specifier keeps a digit
    // of is nothing to report.
    fs::remove_file(scratch.0.join("a.md")).unwrap();
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  wearing: 0.0\n---\nN.\n",
    );
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  wearing: 1.25\n---\nN.\n",
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
        "schema_version = 1\n[property.swarf]\nsymbol = \"m\"\n\
         [property.height]\nsymbol = \"h\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  swarf: {v: 12.5, symbol: w}\n  \
         collar: {v: 4.0, symbol: h}\n---\nNotes.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  swarf: {v: 13.5, symbol: w}\n  \
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
    // neither is a shadow. `swarf` is one note for both files.
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
    assert_eq!(quantity, "swarf");
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
        "---\nname: A\nschema_version: 1\nproperties:\n  diameter: {v: 12.5}\n---\nNotes.\n",
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter:\n    v: 12.5\n  \
         ductility:\n    v: 2.8\n    computed:\n      diameter: 0123456789ab\n  \
         roughness:\n    v: 0.1\n    computed:\n      diameter: 0123456789ab\n---\nN.\n",
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
         diameter: {v: 12.5, unit: g}\n---\nNotes.\n",
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
        sample(
            "A",
            "diameter",
            Property::stored(Value::number(1.0).unwrap()),
        ),
        sample(
            "B",
            "diameter",
            Property::stored(Value::number(2.0).unwrap()),
        ),
    ]);
    assert_eq!(validate::run(&collection).samples, 2);
    assert_eq!(validate::run(&list::empty()).samples, 0);
}

#[test]
fn each_sample_is_checked_against_its_own_project_units() {
    let scratch = Scratch::new("nested-units");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[property.diameter]\nunit = \"g\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 2.8, unit: g}\n---\nN.\n",
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
        "---\nschema_version: 1\nname: A\nproperties:\n  porosity: {v: 2.8}\n---\nN.\n",
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
fn text_in_a_quantity_the_model_declares_is_a_defect() {
    // One sample alone: the model's description says a number belongs there.
    let scratch = Scratch::new("model-number");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model.py\"\n",
    );
    scratch.write(
        "model.py",
        "import samplekit as sk\n\nclass Billet(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.diameter = sk.Property()\n",
    );
    scratch.write(
        "c.md",
        "---\nschema_version: 1\nname: c\nproperties:\n  diameter: {v: \"ninety\"}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::NumberExpected { .. }))
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(finding.severity, Severity::Defect);
    assert!(finding.path.as_ref().unwrap().ends_with("c.md"));
}

#[test]
fn other_samples_do_not_decide_what_a_value_is() {
    // Text among numbers, and a date among dates, where neither the model nor
    // the project declares anything: nothing is said of what the others hold.
    let scratch = Scratch::new("no-kinds");
    for (file, value, made) in [
        ("a.md", "2.0", "2026-03-01"),
        ("b.md", "3", "2026-04-01"),
        ("c.md", "\"ninety\"", "2026-13-45"),
    ] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nmade: {made}\nproperties:\n  diameter: {{v: {value}}}\n---\nN.\n"
            ),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity != Severity::Defect),
        "{:?}",
        report.findings
    );
}

#[test]
fn a_template_brace_near_a_channel_is_noted() {
    // `{valeu}` prints `{valeu}` in a manuscript table. A command's argument is
    // LaTeX's, and left alone; a word standing alone far from every channel is
    // noted without a channel to suggest.
    let scratch = Scratch::new("template-brace");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\", template = \
         \"\\\\num{{value:.3f}} \\\\text{valeur} {valeu} {untis} {table} \\\\si{{unit}}\"}]\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 2.0}\n---\nN.\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let noted: Vec<(String, Option<String>)> = report
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
            ("valeu".to_string(), Some("value".to_string())),
            ("untis".to_string(), Some("unit".to_string())),
            ("table".to_string(), None)
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
                "---\nschema_version: 1\nname: {file}\nproperties:\n  diameter: {{v: 2.0}}\n---\nN.\n"
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
         [figure.good]\nx = \"m.T\"\ny = \"m.hv\"\ngroup = \"tags\"\n\
         [figure.typo]\nx = \"diametr\"\ny = \"diameter\"\ngroup = \"alloi\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nalloy: cobalt\nproperties:\n  diameter: {v: 2.0}\n\
         tables:\n  m:\n    index: T\n    columns:\n      T: {}\n      hv: {}\n    rows:\n      \
         - {T: 20.0, hv: 5.1}\n---\nN.\n",
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
            "figure 'typo': no sample has 'diametr' — did you mean 'diameter'?",
            "figure 'typo': no sample has 'alloi' — did you mean 'alloy'?",
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
        "---\nschema_version: 1\nname: A\nprocess_strain: UT05\nprocess_strain: T04\nproperties:\n  diameter: {v: 1.0}\n  diameter: {v: 2.0}\n---\nN.\n",
    );
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nprocess_strain: UT05\n---\nN.\n",
    );
    // Not read at all: which of its values is meant cannot be told, and every
    // command read the last without a word.
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 1);
    let report = validate::run(&collection);
    let said: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::Unreadable { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert!(
        said.len() == 1 && said[0].contains("'process_strain' is written twice"),
        "{said:?}"
    );
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
         properties:\n  diameter: {v: 1.0}\n---\nN.\n",
    );
    // A block scalar ends where its indentation does: a key repeated after it
    // is still found.
    scratch.write(
        "b.md",
        "---\nschema_version: 1\nname: B\nprotocol: |\n  step: a\nprocess_strain: UT05\nprocess_strain: T04\n---\nN.\n",
    );
    // The text is read; the key written twice after the block is not.
    let collection = list::from_directory(&scratch.0).unwrap();
    let read: Vec<String> = collection
        .iter()
        .filter_map(|entry| entry.sample.borrow().name().map(str::to_string))
        .collect();
    assert_eq!(read, ["A"]);
    assert!(
        collection.skipped().iter().any(|skipped| skipped
            .reason
            .to_string()
            .contains("'process_strain' is written twice")),
        "{:?}",
        collection.skipped()
    );
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
        ("b.md", "X", "\"2,5\""),
        ("c.md", "Y", "3.0"),
    ] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: {value}}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::NumberExpected { .. }))
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
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  diameter: {{v: {value}, unit: mg}}\n---\nN.\n"),
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
    scratch.write("c.md", "---\ntitle: notes\n---\nN.\n");
    // No version, no sample: left out as a README is, and said once.
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(collection.len(), 1);
    let report = validate::run(&collection);
    let notes: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::MissingVersion { files: 2 }))
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", report.findings);
    assert_eq!(notes[0].severity, Severity::Note);
    assert!(
        validate::described(notes[0]).contains("this file and 1 other declare no schema_version"),
        "{}",
        validate::described(notes[0])
    );
}

#[test]
fn a_defect_in_a_part_is_reported_and_the_rest_validated() {
    let mut defective = Sample::new();
    defective.set_name(Some("A".to_string()));
    defective.set_unusable_tags(vec!["my tag".to_string()]);
    defective.set_aside_table(id("tempering"), "duplicate index".to_string());
    let collection = list::from_samples(vec![
        defective,
        sample("B", "diameter", stated(&[12.4, 12.5, 12.6], 5.0)),
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
fn an_override_is_not_counted_as_work_to_do() {
    // The invariant the note states and nothing tested: `NotCurrent` counts
    // stale, broken and failed values, never one a hand wrote over a formula
    // — counting it would invite a `compute` over a deliberate correction.
    let scratch = Scratch::new("override-not-work");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter:\n    v: 12.5\n  \
         ductility:\n    v: 2.8\n    computed:\n      diameter: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n  \
         roughness:\n    v: 0.1\n    computed:\n      diameter: 0123456789ab\n---\nN.\n",
    );
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let counted: Vec<&Detail> = details(&report)
        .into_iter()
        .filter(|detail| matches!(detail, Detail::NotCurrent { .. }))
        .collect();
    // roughness is stale; ductility is an override, and is not counted.
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter:\n    v: 12.5\n  \
         ductility:\n    v: 2.8\n    computed:\n      diameter: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n  \
         volume:\n    v: 4.4\n    computed:\n      diameter: 0123456789ab\n    \
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
         self.porosity = sk.Property(compute_value=self._porosity, depends_on=[])\n        \
         super().__init__(path)\n\n    def _porosity(self):\n        return 1.0\n",
    );
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[model]\npath = \"model/cell.py\"\n\
         [query.strong]\nfilter = \"porosity > 3\"\n[profile.p]\ncolumns = [{field = \"porosity\"}, {field = \"porosityy\"}]\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nproperties:\n  diameter: {{v: 2.0}}\n---\nN.\n"
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
    assert!(declared[0].contains("porosityy"), "{declared:?}");
}

#[test]
fn a_finding_names_the_value_it_is_about() {
    let about = Detail::NumberExpected {
        quantity: "diameter".to_string(),
        text: "heavy".to_string(),
    };
    assert_eq!(about.quantity(), Some("diameter"));
    let table = Detail::TableSetAside {
        table: "tempering".to_string(),
        reason: "two rows at one index".to_string(),
    };
    assert_eq!(table.quantity(), Some("tempering"));
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
        "diameter",
        stated(&[12.4, 12.5, 12.6], 5.0),
    )]));
    assert!(report.findings.is_empty(), "{:?}", details(&report));
}

#[test]
fn text_in_a_column_with_a_unit_is_a_defect() {
    // `stiffness=12.1,12.3,12.2` stored as text in a column measured in cP was
    // reported by nothing.
    let mut sample = Sample::new();
    sample.set_name(Some("V".to_string()));
    let mut meta = samplekit::core::table::ColumnMeta::default();
    meta.presentation.unit = Some("cP".to_string());
    let columns: indexmap::IndexMap<Identifier, samplekit::core::table::ColumnMeta> = [
        (id("T"), samplekit::core::table::ColumnMeta::default()),
        (id("stiffness"), meta),
    ]
    .into_iter()
    .collect();
    let mut table =
        samplekit::core::table::Table::new(id("toughness"), vec![id("T")], columns, Vec::new())
            .unwrap();
    table
        .add_row(vec![
            (id("T"), Property::stored(Value::integer(20))),
            (
                id("stiffness"),
                Property::stored(Value::text("12.1,12.3,12.2")),
            ),
        ])
        .unwrap();
    sample.set_table(id("toughness"), table).unwrap();
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: 1.0\n---\n",
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
        "schema_version = 1\n[property.notch]\nprecision = \".3f\"\n",
    );
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: A\nproperties:\n  notch: n/a\n---\n",
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
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 2.0}\n  \
         volume: {computed: {diameter: bbbbbbbbbbbb}, fingerprint: {failed: ZeroDivisionError}}\n\
         ---\nN.\n",
    );
    scratch.write(
        "b-stale.md",
        "---\nschema_version: 1\nname: B\nproperties:\n  diameter: {v: 2.0}\n  \
         ductility: {v: 2.8, computed: {diameter: 0123456789ab}}\n---\nN.\n",
    );
    scratch.write(
        "c-edited.md",
        "---\nschema_version: 1\nname: C\nproperties:\n  diameter: {v: 2.0}\n  \
         ductility:\n    v: 2.8\n    computed:\n      diameter: 0123456789ab\n    \
         fingerprint: {edited: 0123456789ab}\n---\nN.\n",
    );
    scratch.write(
        "d-clean.md",
        "---\nschema_version: 1\nname: D\nproperties:\n  diameter: {v: 2.0}\n---\nN.\n",
    );
    scratch.write(
        "e-waiting.md",
        "---\nschema_version: 1\nname: E\nproperties:\n  diameter: {}\n  \
         ductility: {v: 2.8, computed: {diameter: 0123456789ab}}\n---\nN.\n",
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
        "---\nschema_version: 1\nname: A\nproperties:\n  carbon: {v: \"2.0\"}\n  \
         diameter: {v: 2.0}\n  ductility: {v: 2.8, computed: {diameter: 0123456789ab}}\n---\nN.\n",
    );
    // A file without schema_version is no sample, and no defect of one.
    scratch.write("b.md", "---\nname: B\nprocess_strain: UT05\n---\nN.\n");
    let collection = list::from_directory(&scratch.0).unwrap();
    assert_eq!(words_of(&collection), [vec!["defective", "outdated"]]);
}

#[test]
fn a_copy_of_an_imported_declaration_is_a_note() {
    let scratch = Scratch::new("imported-copy");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.height]\nunit = \"cL\"\nsymbol = \"h\"\n\
         [property.diameter]\nunit = \"g\"\nprecision = \".1f\"\n",
    );
    scratch.write(
        "c1/.samplekitrc",
        "schema_version = 1\nimport = \"..\"\n\
         [property.height]\nunit = \"cL\"\nsymbol = \"h\"\n\
         [property.diameter]\nunit = \"g\"\nprecision = \".3f\"\n",
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
            (Severity::Note, "[property.diameter] unit".to_string()),
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
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"diameter\"}]\ngroup = [\"alloi\"]\n",
    );
    for file in ["a.md", "b.md"] {
        scratch.write(
            file,
            &format!(
                "---\nschema_version: 1\nname: {file}\nalloy: cobalt\n\
                 properties:\n  diameter: {{v: 2.0}}\n---\nN.\n"
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
        said[0].1.contains("alloi") && said[0].1.contains("alloy"),
        "{said:?}"
    );
    assert!(validate::has_defects(&report));
}

#[test]
fn a_value_outside_its_declared_range_is_a_defect() {
    let scratch = Scratch::new("range");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[property.volume]\nunit = \"L\"\nrange = [0, 50]\n",
    );
    for (file, volume) in [("a.md", "20"), ("b.md", "-5")] {
        scratch.write(
            file,
            &format!("---\nschema_version: 1\nname: {file}\nproperties:\n  volume: {{v: {volume}, unit: L}}\n---\nN.\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let outside: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::OutsideRange { .. }))
        .collect();
    assert_eq!(outside.len(), 1, "{:?}", report.findings);
    assert_eq!(outside[0].severity, Severity::Defect);
    assert!(outside[0].path.as_ref().unwrap().ends_with("b.md"));
    assert!(
        validate::described(outside[0]).contains("volume = -5 L is outside its range, 0 to 50 L"),
        "{}",
        validate::described(outside[0])
    );
}

#[test]
fn names_alike_but_for_case_or_accents_are_a_note() {
    // `Ab` and `ab`, `Café` written whole and as `e` with its accent: one file
    // on macOS and Windows, and no finding said so.
    let scratch = Scratch::new("names-alike");
    scratch.write("a.md", "---\nschema_version: 1\nname: Ab\n---\n");
    scratch.write("b.md", "---\nschema_version: 1\nname: ab\n---\n");
    scratch.write("c.md", "---\nschema_version: 1\nname: \"Caf\u{e9}\"\n---\n");
    scratch.write(
        "d.md",
        "---\nschema_version: 1\nname: \"Cafe\u{301}\"\n---\n",
    );
    scratch.write("e.md", "---\nschema_version: 1\nname: other\n---\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let alike: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::NamesAlike { .. }))
        .collect();
    assert_eq!(alike.len(), 2, "{:?}", report.findings);
    assert!(
        alike
            .iter()
            .all(|finding| finding.severity == Severity::Note)
    );
    assert!(!validate::has_defects(&report));
}

#[test]
fn the_demos_steps_note_nothing_of_their_own_configuration() {
    // Steps 04 and 05 declared a unit's `math` form with no `math` style: each
    // raised notes on a configuration the learner did not write.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/tensile");
    for step in ["02-readings", "03-curve", "04-results"] {
        let collection = list::from_directory(&root.join(step)).unwrap();
        let report = validate::run(&collection);
        let styles: Vec<String> = report
            .findings
            .iter()
            .map(|finding| format!("{:?}", finding.detail))
            .filter(|said| said.contains("no style"))
            .collect();
        assert!(styles.is_empty(), "{step}: {styles:?}");
    }
}

#[test]
fn a_field_one_sample_alone_holds_a_slip_from_another_is_a_note() {
    // `set radiuss=1.070` was refused with *did you mean 'radius'?*, written anyway,
    // and `validate` never said it again.
    let scratch = Scratch::new("field-slip");
    for (name, field) in [("A", "radius"), ("B", "radius"), ("C", "radiuss")] {
        fs::write(
            scratch.0.join(format!("{name}.md")),
            format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  {field}: {{v: 1.05}}\n---\n"
            ),
        )
        .unwrap();
    }
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let slips: Vec<&validate::Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::FieldAlike { .. }))
        .collect();
    assert_eq!(slips.len(), 1, "{:?}", details(&report));
    assert_eq!(slips[0].severity, Severity::Note);
    assert!(
        matches!(&slips[0].detail, Detail::FieldAlike { field, near, others: 2 } if field == "radiuss" && near == "radius"),
        "{:?}",
        slips[0].detail
    );
}

#[test]
fn a_unit_declared_for_a_reserved_field_is_a_note() {
    // `[property.name]` with a unit passed in silence: a name is a text.
    let scratch = Scratch::new("reserved-unit");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[property.name]\nsymbol = \"N\"\nunit = \"g\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("A.md"),
        "---\nschema_version: 1\nname: A\nproperties:\n  radius: {v: 1.05}\n---\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| format!("{:?}", finding.detail)
                .contains("unit is declared: 'name' is SampleKit's own field")),
        "{:?}",
        details(&report)
    );
}

#[test]
fn a_text_in_a_column_of_numbers_is_a_defect() {
    // Written by hand in a table's cell, `1,016` passed `validate`, where the
    // same text in a quantity was a defect. A column of remarks holding one
    // number stays one.
    let scratch = Scratch::new("text-in-numbers");
    fs::write(
        scratch.0.join("A.md"),
        "---\nschema_version: 1\nname: A\ntables:\n  ageing:\n    index: day\n    \
         columns:\n      day: {}\n      hardness: {}\n      remark: {}\n    rows:\n      \
         - {day: 1, hardness: 1.05, remark: bubbling}\n      \
         - {day: 2, hardness: '1,016', remark: 3}\n      \
         - {day: 3, hardness: 1.012, remark: calm}\n---\n",
    )
    .unwrap();
    let collection = list::from_directory(&scratch.0).unwrap();
    let report = validate::run(&collection);
    let expected: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match &finding.detail {
            Detail::NumberExpected { quantity, .. } => Some(quantity.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(expected.len(), 1, "{:?}", details(&report));
    assert!(expected[0].starts_with("ageing.hardness"), "{expected:?}");
}

#[test]
fn a_pattern_that_does_not_read_is_said_without_samples() {
    // It may be why no sample is read: said all the same.
    let scratch = Scratch::new("pattern-no-samples");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\ninclude = [\"[a.md\"]\n",
    );
    scratch.write("a.md", "---\nschema_version: 1\nname: A\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Defect
                && matches!(&finding.detail, Detail::FilesPatternUnusable { entry, .. }
                if entry == "include '[a.md'")),
        "{:?}",
        report.findings
    );
}

#[test]
fn a_readme_excluded_is_not_every_sample_excluded() {
    let scratch = Scratch::new("readme-excluded");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nexclude = [\"README.md\"]\n",
    );
    scratch.write("README.md", "# A project\n");
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| format!("{:?}", finding.detail).contains("excludes every sample")),
        "{:?}",
        report.findings
    );
}

#[test]
fn a_files_entry_an_import_declares_is_said_in_its_file() {
    // The file that writes it, and the entry as it writes it.
    let scratch = Scratch::new("files-imported");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nfiles = [\"{project}/photos\"]\n",
    );
    scratch.write("tins/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    scratch.write("tins/a.md", "---\nschema_version: 1\nname: A\n---\nN.\n");
    let report = validate::run(&list::from_directory(&scratch.0.join("tins")).unwrap());
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding.detail, Detail::FilesPatternUnusable { .. }))
        .expect("a note on the files entry");
    assert_eq!(
        finding.path.as_deref(),
        Some(scratch.0.join(".samplekitrc").as_path())
    );
    assert!(
        matches!(&finding.detail, Detail::FilesPatternUnusable { entry, .. }
            if entry == "{project}/photos"),
        "{finding:?}"
    );
}

#[test]
fn a_roots_declarations_are_judged_by_the_projects_importing_it() {
    // A root declaring the columns every project below it holds: noted as
    // held by no sample, the root's own samples alone being asked.
    let scratch = Scratch::new("root-declarations");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n[property.\"m.eps\"]\nprecision = \".2f\"\n",
    );
    scratch.write("own.md", "---\nschema_version: 1\nname: own\n---\n");
    scratch.write("p/.samplekitrc", "schema_version = 1\nimport = \"..\"\n");
    scratch.write(
        "p/a.md",
        "---\nschema_version: 1\nname: a\ntables:\n  m:\n    index: T\n    rows:\n      - {T: 20, eps: 4.1}\n---\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| matches!(&finding.detail, Detail::DeclarationNamesNoField { declaration, .. } if declaration.contains("m.eps"))),
        "{:?}",
        report.findings
    );
}

#[test]
fn an_imported_query_one_project_cannot_answer_is_a_note_of_its_file() {
    // A root's query naming a field one importing project holds and another
    // does not: a defect of the other's file, which does not declare it.
    let scratch = Scratch::new("imported-query");
    scratch.write(
        ".samplekitrc",
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [query.mine]\nfilter = \"tester == ana\"\n",
    );
    for (project, sample, attribute) in [("ana", "a", "tester: ana\n"), ("cider", "c", "")] {
        scratch.write(
            &format!("{project}/.samplekitrc"),
            "schema_version = 1\nimport = \"..\"\n",
        );
        scratch.write(
            &format!("{project}/{sample}.md"),
            &format!("---\nschema_version: 1\nname: {sample}\n{attribute}---\n"),
        );
    }
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let about: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| matches!(&finding.detail, Detail::DeclarationNamesNoField { declaration, .. } if declaration == "query 'mine'"))
        .collect();
    assert!(!about.is_empty(), "{:?}", report.findings);
    assert!(
        about
            .iter()
            .all(|finding| finding.severity == Severity::Note
                && finding.path.as_deref() == Some(scratch.0.join(".samplekitrc").as_path())),
        "{about:?}"
    );
}

#[test]
fn a_link_that_leads_to_no_sample_or_to_several_is_a_defect() {
    let scratch = Scratch::new("links");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    let sample = |name: &str, attributes: &str| {
        format!("---\nschema_version: 1\nname: {name}\n{attributes}---\n")
    };
    scratch.write(
        "bars/bar-01.md",
        &sample(
            "bar-01",
            "twin: \"[[bar-02]]\"\nby_name: \"[[second bar]]\"\nsupplier: https://example.org\n\
             lost: \"[[bar-09|the ninth]]\"\nboth: \"[[bar-03]]\"\n",
        ),
    );
    scratch.write("bars/bar-02.md", &sample("second bar", ""));
    scratch.write("bars/bar-03.md", &sample("bar-03a", ""));
    scratch.write("old/bar-03.md", &sample("bar-03b", ""));
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let links: Vec<String> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::LinkLeadsNowhere { .. }))
        .inspect(|finding| assert_eq!(finding.severity, Severity::Defect))
        .map(validate::described)
        .collect();
    assert_eq!(links.len(), 2, "{links:#?}");
    assert!(
        links.iter().any(|said| said.starts_with(
            "lost: [[bar-09|the ninth]] leads to no sample — no file of the project is named bar-09"
        )),
        "{links:?}"
    );
    assert!(
        links
            .iter()
            .any(|said| said.starts_with("both: [[bar-03]] leads to 2 samples")),
        "{links:?}"
    );
    // A file named alone is judged against its whole project.
    let alone =
        validate::run_on_files(&list::from_files(&[scratch.0.join("bars/bar-01.md")]).unwrap());
    assert_eq!(
        alone
            .findings
            .iter()
            .filter(|finding| matches!(finding.detail, Detail::LinkLeadsNowhere { .. }))
            .count(),
        2
    );
}

#[test]
fn a_markdown_link_to_a_path_where_nothing_is_is_a_defect() {
    let scratch = Scratch::new("markdown-links");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write("reports/bar-01.pdf", "%PDF");
    scratch.write(
        "bars/bar-01.md",
        "---\nschema_version: 1\nname: bar-01\ntwin: \"[the twin](bar-02.md)\"\n\
         report: \"[report](../reports/bar-01.pdf)\"\nlost: \"[report](../reports/bar-09.pdf)\"\n\
         parents: [\"[[bar-02]]\", \"[plate](../plates/p3.md)\"]\n---\n",
    );
    scratch.write(
        "bars/bar-02.md",
        "---\nschema_version: 1\nname: bar-02\n---\n",
    );
    let report = validate::run(&list::from_directory(&scratch.0).unwrap());
    let links: Vec<String> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.detail, Detail::LinkLeadsNowhere { .. }))
        .map(validate::described)
        .collect();
    assert_eq!(
        links,
        [
            "lost: [report](../reports/bar-09.pdf) leads to nothing — no file is at \
             ../reports/bar-09.pdf, from the sample's folder",
            "parents: [plate](../plates/p3.md) leads to nothing — no file is at \
             ../plates/p3.md, from the sample's folder",
        ],
        "{links:#?}"
    );
}
