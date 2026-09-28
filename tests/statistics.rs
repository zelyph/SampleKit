//! The tests of `statistics`.

use samplekit::core::statistics::{Summary, summarize, summarize_slice};
use samplekit::core::value::Readings;

/// The same deterministic generator `tests/value.rs` uses, for the same reason.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn shuffle(&mut self, values: &mut [f64]) {
        for index in (1..values.len()).rev() {
            let target = (self.next() % (index as u64 + 1)) as usize;
            values.swap(index, target);
        }
    }
}

/// The reference series: eight readings, hand-checkable, with a repeated value
/// and an even count so the quartiles interpolate.
const REFERENCE: [f64; 8] = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];

fn summarize_reference() -> Summary {
    summarize_slice(&REFERENCE).unwrap()
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-12 * left.abs().max(right.abs()).max(1.0)
}

#[test]
fn matches_reference_values() {
    let summary = summarize_reference();
    assert_eq!(summary.count.get(), 8);
    assert_eq!(summary.minimum, 2.0);
    assert_eq!(summary.maximum, 9.0);
    assert!(close(summary.mean, 5.0), "{}", summary.mean);
    assert!(close(summary.population_stdev, 2.0));
    assert!(close(summary.sample_stdev.unwrap(), 2.138_089_935_299_395));
    assert!(close(
        summary.standard_error.unwrap(),
        0.755_928_946_018_454_4
    ));
    assert_eq!(summary.median, 4.5);
    assert_eq!(summary.first_quartile, 4.0);
    assert_eq!(summary.third_quartile, 5.5);
}

#[test]
fn single_observation_has_no_spread() {
    // Never zero: one reading provides no information about spread, and zero
    // would claim perfect precision from it.
    let summary = summarize(&Readings::new(vec![12.5]).unwrap());
    assert_eq!(summary.sample_stdev, None);
    assert_eq!(summary.standard_error, None);
}

#[test]
fn single_observation_has_a_mean() {
    let summary = summarize(&Readings::new(vec![12.5]).unwrap());
    assert_eq!(summary.count.get(), 1);
    assert_eq!(summary.mean, 12.5);
    assert_eq!(summary.minimum, 12.5);
    assert_eq!(summary.maximum, 12.5);
    assert_eq!(summary.median, 12.5);
    assert_eq!(summary.first_quartile, 12.5);
    assert_eq!(summary.third_quartile, 12.5);
}

#[test]
fn single_observation_has_a_zero_population_deviation() {
    // 0.0, not None: a one-element series genuinely has no spread. Refusing to
    // call that an *uncertainty* is `uncertainty`'s job, not this module's.
    let summary = summarize(&Readings::new(vec![12.5]).unwrap());
    assert_eq!(summary.population_stdev, 0.0);
}

#[test]
fn standard_error_is_stdev_over_root_n() {
    // Exactly, not approximately.
    let summary = summarize_reference();
    let expected = summary.sample_stdev.unwrap() / (summary.count.get() as f64).sqrt();
    assert_eq!(summary.standard_error.unwrap(), expected);
}

#[test]
fn stable_for_clustered_large_values() {
    // The textbook formula sum(x^2) - sum(x)^2/n returns exactly 0.0 here.
    let clustered = [1e8 + 1.0, 1e8 + 2.0, 1e8 + 3.0];
    let summary = summarize_slice(&clustered).unwrap();
    assert!(
        close(summary.sample_stdev.unwrap(), 1.0),
        "sample_stdev was {:?}",
        summary.sample_stdev
    );
    assert!(
        close(summary.population_stdev, 0.816_496_580_927_726),
        "population_stdev was {}",
        summary.population_stdev
    );
    assert!(close(summary.mean, 1e8 + 2.0), "mean was {}", summary.mean);
}

#[test]
fn order_statistics_are_monotonic() {
    let mut generator = Generator(0xC0FFEE);
    for size in 1..40usize {
        let series: Vec<f64> = (0..size)
            .map(|_| (generator.next() % 100_000) as f64 / 100.0 - 500.0)
            .collect();
        let summary = summarize_slice(&series).unwrap();
        assert!(summary.minimum <= summary.first_quartile, "{summary:?}");
        assert!(summary.first_quartile <= summary.median, "{summary:?}");
        assert!(summary.median <= summary.third_quartile, "{summary:?}");
        assert!(summary.third_quartile <= summary.maximum, "{summary:?}");
        if let Some(sample_stdev) = summary.sample_stdev {
            // Follows from the divisors: n - 1 is smaller than n.
            assert!(summary.population_stdev <= sample_stdev, "{summary:?}");
        }
    }
}

#[test]
fn independent_of_input_order() {
    // The order statistics are bit-identical because they are read from a
    // sorted copy. The mean and the deviations accumulate, so they agree to
    // within rounding rather than exactly; see the note.
    let expected = summarize_reference();
    let mut generator = Generator(0xBEEF);
    for _ in 0..200 {
        let mut shuffled = REFERENCE.to_vec();
        generator.shuffle(&mut shuffled);
        let summary = summarize_slice(&shuffled).unwrap();
        assert_eq!(summary.count, expected.count);
        assert_eq!(summary.minimum, expected.minimum);
        assert_eq!(summary.maximum, expected.maximum);
        assert_eq!(summary.median, expected.median);
        assert_eq!(summary.first_quartile, expected.first_quartile);
        assert_eq!(summary.third_quartile, expected.third_quartile);
        assert!(close(summary.mean, expected.mean), "{shuffled:?}");
        assert!(
            close(summary.population_stdev, expected.population_stdev),
            "{shuffled:?}"
        );
        assert!(
            close(
                summary.sample_stdev.unwrap(),
                expected.sample_stdev.unwrap()
            ),
            "{shuffled:?}"
        );
        assert!(
            close(
                summary.standard_error.unwrap(),
                expected.standard_error.unwrap()
            ),
            "{shuffled:?}"
        );
    }
}

#[test]
fn quartiles_match_declared_convention() {
    // Positioned at p * (n - 1), interpolated linearly. At least nine other
    // conventions exist, and a collection summarized by two of them silently
    // disagrees.
    let odd = summarize_slice(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
    assert_eq!(odd.first_quartile, 2.0);
    assert_eq!(odd.median, 3.0);
    assert_eq!(odd.third_quartile, 4.0);

    let even = summarize_slice(&[10.0, 20.0, 30.0, 40.0]).unwrap();
    assert_eq!(even.first_quartile, 17.5);
    assert_eq!(even.median, 25.0);
    assert_eq!(even.third_quartile, 32.5);
}

#[test]
fn median_interpolates_on_an_even_count() {
    // The midpoint of the middle two, not the lower one.
    let summary = summarize_slice(&[1.0, 2.0, 3.0, 4.0]).unwrap();
    assert_eq!(summary.median, 2.5);
}

#[test]
fn empty_slice_yields_none() {
    // Emptiness is reported rather than answered with zeros.
    assert!(summarize_slice(&[]).is_none());
}

#[test]
fn single_value_slice_summarizes() {
    let from_slice = summarize_slice(&[12.5]).unwrap();
    let from_readings = summarize(&Readings::new(vec![12.5]).unwrap());
    assert_eq!(from_slice, from_readings);
}
