//! The tests of `ordering`.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{Compute, ComputeError, Property};
use samplekit::core::sample::{AttributeValue, Sample};
use samplekit::core::table::{ColumnMeta, Table};
use samplekit::core::value::Value;
use samplekit::query::field_addressing::{self as fields, Subject, Vocabulary};
use samplekit::query::ordering::{self, OrderError, SortKey, SortSpec};

/// A slice of references, which is what the collection functions take: a real
/// caller holds samples behind an `Rc` and can only lend them.
fn refs(samples: &[Sample]) -> Vec<&Sample> {
    samples.iter().collect()
}

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

/// One cask: a beer, a plato when it was measured, and a mashing
/// boil so that a cell can be a sort key.
fn cask(beer: &str, plato: Option<f64>) -> Sample {
    let mut sample = Sample::new();
    sample.set_attribute(id("beer"), Value::text(beer)).unwrap();
    if let Some(plato) = plato {
        let mut property = Property::stored(number(plato));
        property.set_uncertainty(Some(
            samplekit::core::uncertainty::Uncertainty::new(plato / 100.0).unwrap(),
        ));
        sample.set_property(id("plato"), property).unwrap();
    }
    let mut mashing = Table::new(
        id("mashing"),
        vec![id("temperature")],
        ["temperature", "wort"]
            .iter()
            .map(|n| (id(n), ColumnMeta::default()))
            .collect::<IndexMap<_, _>>(),
        Vec::new(),
    )
    .unwrap();
    mashing
        .add_row(vec![
            (id("temperature"), Property::stored(number(78.5))),
            (
                id("wort"),
                Property::stored(number(plato.unwrap_or(1.0) * 10.0)),
            ),
        ])
        .unwrap();
    sample.set_table(id("mashing"), mashing).unwrap();
    sample
}

/// Samples with paths, so that the final tie-break has something to compare.
struct Corpus {
    samples: Vec<Sample>,
    paths: Vec<Option<PathBuf>>,
    vocabulary: Vocabulary,
}

impl Corpus {
    fn of(entries: Vec<(&str, Option<f64>, Option<&str>)>) -> Corpus {
        let samples: Vec<Sample> = entries
            .iter()
            .map(|(beer, plato, _)| cask(beer, *plato))
            .collect();
        let paths = entries
            .iter()
            .map(|(_, _, path)| path.map(PathBuf::from))
            .collect();
        let vocabulary = fields::vocabulary_of(&refs(&samples));
        Corpus {
            samples,
            paths,
            vocabulary,
        }
    }

    fn subjects(&self) -> Vec<Subject<'_>> {
        self.samples
            .iter()
            .zip(&self.paths)
            .map(|(sample, path)| Subject {
                sample,
                path: path.as_deref(),
                vocabulary: &self.vocabulary,
                states: None,
            })
            .collect()
    }

    /// The beers, in the order a spec puts them.
    fn order(&self, spec: &str) -> Vec<String> {
        let spec = ordering::parse_spec(&[spec.to_string()]).unwrap();
        self.order_by(&spec)
    }

    fn order_by(&self, spec: &SortSpec) -> Vec<String> {
        let subjects = self.subjects();
        ordering::sorted(&subjects, spec)
            .unwrap()
            .into_iter()
            .map(|at| beer_of(&self.samples[at]))
            .collect()
    }
}

fn beer_of(sample: &Sample) -> String {
    match sample.attribute(&id("beer")).unwrap().as_scalar().unwrap() {
        Value::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    }
}

fn spec(keys: &[(&str, bool)]) -> SortSpec {
    SortSpec::new(
        keys.iter()
            .map(|(path, descending)| SortKey {
                field: fields::parse(path).unwrap(),
                descending: *descending,
            })
            .collect(),
    )
    .unwrap()
}

// ------------------------------------------------------------------ sorting

#[test]
fn sorted_returns_a_total_permutation() {
    // A shuffled collection yields an order, and every index appears exactly
    // once — the regression test for the original defect, where `sort("vfi")`
    // left the collection in directory order and said nothing.
    let corpus = Corpus::of(vec![
        ("c", Some(3.0), Some("/c.md")),
        ("a", Some(1.0), Some("/a.md")),
        ("b", Some(2.0), Some("/b.md")),
    ]);
    let subjects = corpus.subjects();
    let order = ordering::sorted(&subjects, &spec(&[("plato", false)])).unwrap();
    let mut seen = order.clone();
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2], "not a permutation: {order:?}");
    assert_eq!(corpus.order("plato"), ["a", "b", "c"]);
}

#[test]
fn sort_by_numeric_field() {
    // Numeric, not lexicographic: 9 before 10.
    let corpus = Corpus::of(vec![
        ("ten", Some(10.0), Some("/a.md")),
        ("nine", Some(9.0), Some("/b.md")),
    ]);
    assert_eq!(corpus.order("plato"), ["nine", "ten"]);
}

#[test]
fn sort_by_table_cell_field() {
    let corpus = Corpus::of(vec![
        ("high", Some(3.0), Some("/a.md")),
        ("low", Some(1.0), Some("/b.md")),
    ]);
    assert_eq!(corpus.order("mashing.wort[78.5]"), ["low", "high"]);
}

#[test]
fn sort_by_uncertainty_channel() {
    // Channels are sortable.
    let corpus = Corpus::of(vec![
        ("big", Some(500.0), Some("/a.md")),
        ("small", Some(1.0), Some("/b.md")),
    ]);
    assert_eq!(corpus.order("plato.u"), ["small", "big"]);
}

#[test]
fn descending_reverses_present_values() {
    let corpus = Corpus::of(vec![
        ("a", Some(1.0), Some("/a.md")),
        ("b", Some(2.0), Some("/b.md")),
        ("c", Some(3.0), Some("/c.md")),
    ]);
    assert_eq!(corpus.order("-plato"), ["c", "b", "a"]);
}

// -------------------------------------------------------------- absence

#[test]
fn absent_sorts_last_ascending() {
    let corpus = Corpus::of(vec![
        ("unmeasured", None, Some("/u.md")),
        ("light", Some(1.0), Some("/l.md")),
        ("heavy", Some(3.0), Some("/h.md")),
    ]);
    assert_eq!(corpus.order("plato"), ["light", "heavy", "unmeasured"]);
}

#[test]
fn absent_sorts_last_descending() {
    // Still at the end: descending is not a plain reversal, because reversing
    // a sort to see the largest values first must not fill the screen with
    // blanks.
    let corpus = Corpus::of(vec![
        ("unmeasured", None, Some("/u.md")),
        ("light", Some(1.0), Some("/l.md")),
        ("heavy", Some(3.0), Some("/h.md")),
    ]);
    assert_eq!(corpus.order("-plato"), ["heavy", "light", "unmeasured"]);
}

// ------------------------------------------------------------- multi-key

#[test]
fn multi_key_breaks_ties_in_order() {
    let corpus = Corpus::of(vec![
        ("altbier", Some(3.0), Some("/a.md")),
        ("altbier", Some(1.0), Some("/b.md")),
        ("zwickel", Some(2.0), Some("/c.md")),
    ]);
    let order = corpus.order_by(&spec(&[("beer", false), ("plato", false)]));
    assert_eq!(order, ["altbier", "altbier", "zwickel"]);
    let platos = corpus.order_by(&spec(&[("beer", false), ("plato", false)]));
    assert_eq!(platos.len(), 3);
    // The second key decides between the two helless: 1.0 before 3.0.
    let subjects = corpus.subjects();
    let permutation =
        ordering::sorted(&subjects, &spec(&[("beer", false), ("plato", false)])).unwrap();
    assert_eq!(permutation[0], 1);
}

#[test]
fn multi_key_supports_mixed_directions() {
    // Ascending then descending in one spec.
    let corpus = Corpus::of(vec![
        ("altbier", Some(1.0), Some("/a.md")),
        ("altbier", Some(3.0), Some("/b.md")),
        ("zwickel", Some(2.0), Some("/c.md")),
    ]);
    let parsed = ordering::parse_spec(&["beer".to_string(), "-plato".to_string()]).unwrap();
    let subjects = corpus.subjects();
    let permutation = ordering::sorted(&subjects, &parsed).unwrap();
    assert_eq!(permutation, [1, 0, 2]);
}

#[test]
fn path_breaks_remaining_ties() {
    // Two samples equal on every key order by path.
    let corpus = Corpus::of(vec![
        ("same", Some(1.0), Some("/b.md")),
        ("same", Some(1.0), Some("/a.md")),
    ]);
    let subjects = corpus.subjects();
    let permutation = ordering::sorted(&subjects, &spec(&[("plato", false)])).unwrap();
    assert_eq!(permutation, [1, 0]);
}

#[test]
fn a_sample_without_a_path_sorts_last() {
    // The tie-break has nothing to compare, and says so rather than guessing.
    let corpus = Corpus::of(vec![
        ("unwritten", Some(1.0), None),
        ("saved", Some(1.0), Some("/a.md")),
    ]);
    let subjects = corpus.subjects();
    let permutation = ordering::sorted(&subjects, &spec(&[("plato", false)])).unwrap();
    assert_eq!(permutation, [1, 0]);
}

#[test]
fn sort_is_stable() {
    // Equal elements keep their relative order.
    let corpus = Corpus::of(vec![
        ("first", Some(1.0), None),
        ("second", Some(1.0), None),
        ("third", Some(1.0), None),
    ]);
    let subjects = corpus.subjects();
    let permutation = ordering::sorted(&subjects, &spec(&[("plato", false)])).unwrap();
    assert_eq!(permutation, [0, 1, 2]);
}

#[test]
fn sort_is_deterministic_across_runs() {
    let corpus = Corpus::of(vec![
        ("c", Some(2.0), Some("/c.md")),
        ("a", Some(2.0), Some("/a.md")),
        ("b", Some(1.0), Some("/b.md")),
    ]);
    let first = corpus.order("plato");
    for _ in 0..5 {
        assert_eq!(corpus.order("plato"), first);
    }
}

// -------------------------------------------------------------- refusals

#[test]
fn unknown_field_is_an_error() {
    // Not a silent no-op, which is the defect this module exists for.
    let corpus = Corpus::of(vec![("a", Some(1.0), Some("/a.md"))]);
    let subjects = corpus.subjects();
    let error = ordering::sorted(&subjects, &spec(&[("vfi", false)])).unwrap_err();
    assert!(matches!(error, OrderError::UnknownField(_)), "{error:?}");
}

#[test]
fn unknown_field_yields_no_permutation() {
    // Validation precedes extraction, so there is no half-order to apply.
    let corpus = Corpus::of(vec![
        ("a", Some(1.0), Some("/a.md")),
        ("b", Some(2.0), Some("/b.md")),
    ]);
    let subjects = corpus.subjects();
    assert!(ordering::sorted(&subjects, &spec(&[("vfi", false)])).is_err());
    // And the good spec still works afterwards: nothing was consumed.
    assert_eq!(corpus.order("plato"), ["a", "b"]);
}

#[test]
fn empty_spec_is_rejected() {
    // At construction.
    assert!(matches!(
        SortSpec::new(Vec::new()),
        Err(OrderError::EmptySpec)
    ));
    assert!(matches!(
        ordering::parse_spec(&[]),
        Err(OrderError::EmptySpec)
    ));
}

#[test]
fn keys_are_extracted_once_per_sample() {
    // A counting formula is called n times, not n log n.
    struct Counting(Rc<Cell<usize>>);
    impl Compute for Counting {
        fn compute(&self) -> Result<Value, ComputeError> {
            self.0.set(self.0.get() + 1);
            Ok(Value::number(self.0.get() as f64).unwrap())
        }
    }
    let calls = Rc::new(Cell::new(0));
    let mut samples = Vec::new();
    for _ in 0..8 {
        let mut sample = Sample::new();
        sample
            .set_property(
                id("derived"),
                Property::computed(Rc::new(Counting(Rc::clone(&calls)))),
            )
            .unwrap();
        samples.push(sample);
    }
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subjects: Vec<Subject> = samples
        .iter()
        .map(|sample| Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        })
        .collect();
    ordering::sorted(&subjects, &spec(&[("derived", false)])).unwrap();
    // Eight samples, eight calls — not the 24 a comparator would cost.
    assert_eq!(calls.get(), 8);
}

#[test]
fn sorted_does_not_mutate() {
    // The input is untouched: it returns a permutation and owns nothing.
    let corpus = Corpus::of(vec![
        ("c", Some(3.0), Some("/c.md")),
        ("a", Some(1.0), Some("/a.md")),
    ]);
    let before: Vec<String> = corpus.samples.iter().map(beer_of).collect();
    let subjects = corpus.subjects();
    ordering::sorted(&subjects, &spec(&[("plato", false)])).unwrap();
    let after: Vec<String> = corpus.samples.iter().map(beer_of).collect();
    assert_eq!(before, after);
}

#[test]
fn sort_by_a_list_item_uses_that_scalar() {
    let mut samples = Vec::new();
    for (name, first) in [("warm", 30), ("cold", 20)] {
        let mut sample = Sample::new();
        sample.set_name(Some(name.to_string()));
        sample
            .set_attribute(
                id("temperatures"),
                AttributeValue::list(vec![Value::integer(first), Value::integer(40)]).unwrap(),
            )
            .unwrap();
        samples.push(sample);
    }
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subjects: Vec<_> = samples
        .iter()
        .map(|sample| Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        })
        .collect();
    assert_eq!(
        ordering::sorted(&subjects, &spec(&[("temperatures[#0]", false)])).unwrap(),
        [1, 0]
    );
}

#[test]
fn sorting_by_a_whole_list_is_refused() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20)]).unwrap(),
        )
        .unwrap();
    let vocabulary = Vocabulary::of(&sample);
    let subjects = [Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    }];
    assert!(matches!(
        ordering::sorted(&subjects, &spec(&[("temperatures", false)])),
        Err(OrderError::Unsortable { .. })
    ));
}

#[test]
fn sorting_by_state_is_refused_as_a_set() {
    // `state` is a set of words, and a list's advice, `[#n]`, would name an
    // item it does not have. Refused whether its states were read.
    let sample = cask("schwarz", Some(7.8));
    let vocabulary = Vocabulary::of(&sample);
    let subjects = [Subject {
        sample: &sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    }];
    let error = ordering::sorted(&subjects, &spec(&[("state", false)])).unwrap_err();
    assert!(matches!(error, OrderError::Unsortable { .. }), "{error:?}");
    let said = error.to_string();
    assert!(said.contains("several states"), "{said}");
    assert!(!said.contains("[#n]"), "{said}");
}

#[test]
fn text_sorts_without_regard_to_case() {
    let samples = vec![
        cask("MnO2", None),
        cask("activated carbon", None),
        cask("CNT", None),
    ];
    let vocabulary = fields::vocabulary_of(&refs(&samples));
    let subjects: Vec<Subject> = samples
        .iter()
        .map(|sample| Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        })
        .collect();
    let spec = ordering::parse_spec(&["beer".to_string()]).unwrap();
    assert_eq!(ordering::sorted(&subjects, &spec).unwrap(), [1, 2, 0]);
}
