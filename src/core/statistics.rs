//! Descriptive statistics over a series of finite numbers.
//!
//! One implementation serves both a property's raw readings and a collection
//! column, because a mean computed two ways eventually differs two ways.
//!

use std::num::NonZeroUsize;

use crate::core::value::Readings;

/// Every descriptive statistic of one series.
///
/// **These field names are a public vocabulary.** They are what a filter
/// addresses through `malt.stats.sample_stdev`, what a column emits, and what
/// a template names when it chooses how an uncertainty is obtained. Renaming
/// one renames it in four places at once, which is the point.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub count: NonZeroUsize,
    pub minimum: f64,
    pub maximum: f64,
    pub mean: f64,
    pub median: f64,
    pub first_quartile: f64,
    pub third_quartile: f64,
    /// `None` when `count == 1`: one reading provides no information about
    /// spread, and reporting `0.0` would claim perfect precision from one
    /// reading.
    pub sample_stdev: Option<f64>,
    /// Defined for every count, and `0.0` for one reading — which is the
    /// true description of a one-element series. Turning that into an
    /// *uncertainty* is refused one layer up, by `uncertainty`.
    pub population_stdev: f64,
    /// `None` when `count == 1`, for the same reason as `sample_stdev`.
    pub standard_error: Option<f64>,
}

/// Summarize a property's readings. Cannot fail: the type has already
/// discharged non-emptiness and finiteness.
pub fn summarize(readings: &Readings) -> Summary {
    summarize_slice(readings.as_slice()).expect("Readings is non-empty by construction")
}

/// Summarize a bare slice, for collection columns, which are assembled from
/// many samples and may be empty.
///
/// Non-finite entries are the caller's error to prevent; only emptiness is
/// reported, and it is reported rather than answered with zeros.
pub fn summarize_slice(values: &[f64]) -> Option<Summary> {
    let count = NonZeroUsize::new(values.len())?;
    let n = count.get();

    // Welford's online algorithm, in one pass, for the mean and the sum of
    // squared deviations behind both standard deviations. Naming the algorithm
    // in the specification is what keeps the three candidate formulas — which
    // disagree in their last bits — from being chosen by whoever writes the
    // code. What it excludes is the textbook `sum(x^2) - sum(x)^2/n`, which
    // loses catastrophic precision on readings clustered far from zero.
    let mut mean = 0.0f64;
    let mut sum_squared_deviations = 0.0f64;
    for (index, &x) in values.iter().enumerate() {
        let delta = x - mean;
        mean += delta / (index + 1) as f64;
        sum_squared_deviations += delta * (x - mean);
    }

    let mut sorted = values.to_vec();
    // `total_cmp` rather than `partial_cmp().unwrap()`: a caller that ignored
    // the precondition gets a wrong summary, not a panic in a sort.
    sorted.sort_by(f64::total_cmp);

    let population_stdev = (sum_squared_deviations / n as f64).sqrt();
    let (sample_stdev, standard_error) = if n > 1 {
        let s = (sum_squared_deviations / (n - 1) as f64).sqrt();
        (Some(s), Some(s / (n as f64).sqrt()))
    } else {
        (None, None)
    };

    Some(Summary {
        count,
        minimum: sorted[0],
        maximum: sorted[n - 1],
        mean,
        median: quantile(&sorted, 0.5),
        first_quartile: quantile(&sorted, 0.25),
        third_quartile: quantile(&sorted, 0.75),
        sample_stdev,
        population_stdev,
        standard_error,
    })
}

/// Linear interpolation between order statistics, positioned at `p * (n - 1)`.
///
/// Named in the specification because at least nine quantile conventions exist
/// and a collection summarized by two of them silently disagrees. This is the
/// one NumPy uses by default and the one `statistics.quantiles(method=
/// "inclusive")` implements.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let position = p * (n - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        sorted[lower] + (position - lower as f64) * (sorted[upper] - sorted[lower])
    }
}

/// The statistics that say where a series lies, which a model may declare as a
/// property's value. The spreads a model declares as its uncertainty are
/// `uncertainty::Convention`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Location {
    Mean,
    Median,
    Minimum,
    Maximum,
    FirstQuartile,
    ThirdQuartile,
}

impl Location {
    /// The reverse of `name`: the closed set a file may state.
    pub fn from_name(name: &str) -> Option<Location> {
        Location::ALL.into_iter().find(|one| one.name() == name)
    }

    pub const ALL: [Location; 6] = [
        Location::Mean,
        Location::Median,
        Location::Minimum,
        Location::Maximum,
        Location::FirstQuartile,
        Location::ThirdQuartile,
    ];

    pub fn of(&self, summary: &Summary) -> f64 {
        match self {
            Location::Mean => summary.mean,
            Location::Median => summary.median,
            Location::Minimum => summary.minimum,
            Location::Maximum => summary.maximum,
            Location::FirstQuartile => summary.first_quartile,
            Location::ThirdQuartile => summary.third_quartile,
        }
    }

    /// Its name in `Summary`, which is the public vocabulary.
    pub fn name(&self) -> &'static str {
        match self {
            Location::Mean => "mean",
            Location::Median => "median",
            Location::Minimum => "minimum",
            Location::Maximum => "maximum",
            Location::FirstQuartile => "first_quartile",
            Location::ThirdQuartile => "third_quartile",
        }
    }
}
