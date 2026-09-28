//! One named quantity: where its value comes from, whether that value is
//! currently trustworthy, and how it should be displayed.
//!
//! It does not know its own name, does not know what it depends on, and **does
//! not know Python exists**: computation reaches this module as a trait object,
//! never as an interpreter object.
//!

use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use serde::de::{Deserialize, Deserializer, Error as DeError};

use crate::core::formatting::Presentation;
use crate::core::identifier::Identifier;
use crate::core::statistics::Location;
use crate::core::uncertainty::{Convention, Uncertainty, from_readings};
use crate::core::value::{Readings, Value, ValueKind};

/// A content hash of a quantity's values.
///
/// The type lives here and the algorithm does not: a record is *carried* by a
/// property across a load, an edit and a save, while producing and checking one
/// needs canonicalization, two layers up.
///
/// A digest can be marked edited: the digest of a value no formula produced,
/// which travels with the mark so that a record says both what an input was and
/// that it was an override.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint {
    hex: String,
    edited: bool,
}

impl Fingerprint {
    pub fn new(hex: impl Into<String>) -> Fingerprint {
        Fingerprint {
            hex: hex.into(),
            edited: false,
        }
    }

    /// The digest of an override.
    pub fn edited(hex: impl Into<String>) -> Fingerprint {
        Fingerprint {
            hex: hex.into(),
            edited: true,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.hex
    }

    pub fn is_edited(&self) -> bool {
        self.edited
    }

    pub fn marked_edited(self) -> Fingerprint {
        Fingerprint {
            edited: true,
            ..self
        }
    }

    /// Whether two digests hash the same content, whatever either says about
    /// how that content came about. Every freshness comparison is this one.
    pub fn same_digest(&self, other: &Fingerprint) -> bool {
        self.hex == other.hex
    }
}

/// The digest alone: the mark is written by the format, not by the digest.
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex)
    }
}

/// Where one input of a derivation came from.
///
/// Not an `Identifier`, because `row.wort` is not one: it names a scope
/// and a column, and a name admits no dot. One type declares a derivation's
/// inputs and keys the record it produces, so the two cannot drift.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputName {
    /// `foam` — a property or an attribute of the sample.
    Named(Identifier),
    /// `row.wort` — this row's cell, in a column of this table.
    Cell(Identifier),
    /// `mashing.wort` — every cell of one column.
    Column {
        table: Identifier,
        column: Identifier,
    },
}

impl fmt::Display for InputName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputName::Named(name) => write!(f, "{name}"),
            InputName::Cell(column) => write!(f, "row.{column}"),
            InputName::Column { table, column } => write!(f, "{table}.{column}"),
        }
    }
}

/// How one input stood when a value was derived from it.
///
/// The variants name the **form**, not the kind: a property and a whole column
/// both hash, and only an attribute fits. *Hash what does not fit, store what
/// does.*
#[derive(Debug, Clone, PartialEq)]
pub enum InputRecord {
    Digest(Fingerprint),
    Literal(Value),
}

/// What a property claims about where its value came from.
///
/// Two independent records. `computed` being `Some` — including `Some` of an
/// empty map — means the property is calculated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeclaredStatistics {
    pub value: Option<Location>,
    pub uncertainty: Option<Convention>,
}

impl DeclaredStatistics {
    pub fn is_empty(&self) -> bool {
        self.value.is_none() && self.uncertainty.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Records {
    pub fingerprint: Option<Fingerprint>,
    pub computed: Option<IndexMap<InputName, InputRecord>>,
    /// Which channel the formula produced, where exactly one did.
    ///
    /// `None` means the quantity as a whole: both numbers are a formula's, or
    /// nothing is known. It is set **here**, while the record is built, and
    /// not where the file is written — by then `save_computed` has
    /// materialized the sample and no property says it is computed any more.
    pub produced: Option<Produced>,
    /// The message of a failure a file recorded where the digest would be.
    pub failure: Option<String>,
    /// Which statistic **the file states** stood for each channel.
    ///
    /// Here and not on the property, because this is the file's own testimony,
    /// as `computed` and `fingerprint` are. A convention a model declares in a
    /// session says what a number *should* be; only a file saying so replaces
    /// the model for a reader who does not have one, and that is the whole
    /// condition set for the finding to be evidence.
    pub statistics: Option<DeclaredStatistics>,
    /// Which inputs one channel's formula alone reads.
    ///
    /// Empty for the property almost every model writes, whose two formulas —
    /// where it has two — read the same things. What is named here is written
    /// under its channel and the rest at the quantity's level, so that a digest
    /// two formulas share is stated once and cannot diverge.
    pub channel_only: IndexMap<InputName, Produced>,
}

/// The channel one formula produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Produced {
    Value,
    Uncertainty,
}

impl Produced {
    /// The one spelling of a channel, in a declaration, a record and a field
    /// address alike.
    pub fn key(self) -> &'static str {
        match self {
            Produced::Value => "v",
            Produced::Uncertainty => "u",
        }
    }

    pub fn parse(text: &str) -> Option<Produced> {
        match text {
            "v" => Some(Produced::Value),
            "u" => Some(Produced::Uncertainty),
            _ => None,
        }
    }
}

/// What each of a property's two formulas declared it reads.
///
/// The graph holds their union, because an input moving stales the property
/// whichever formula reads it. This holds the split, which only the record
/// needs: it is what lets `u` name what the uncertainty alone reads — its own
/// value, first of all, which is no edge of any graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelInputs {
    pub value: Vec<InputName>,
    pub uncertainty: Vec<InputName>,
}

impl ChannelInputs {
    /// Which channel names this input alone, where one does.
    pub fn only(&self, input: &InputName) -> Option<Produced> {
        match (self.value.contains(input), self.uncertainty.contains(input)) {
            (true, false) => Some(Produced::Value),
            (false, true) => Some(Produced::Uncertainty),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty() && self.uncertainty.is_empty()
    }
}

impl Records {
    /// A property outside every calculation carries neither.
    pub fn none() -> Records {
        Records::default()
    }

    pub fn is_empty(&self) -> bool {
        self.fingerprint.is_none()
            && self.computed.is_none()
            && self.failure.is_none()
            && self.statistics.is_none_or(|declared| declared.is_empty())
    }
}

/// The core's entire knowledge of computation: something that produces a value
/// or fails.
pub trait Compute {
    fn compute(&self) -> Result<Value, ComputeError>;
}

/// And something that produces a value and its uncertainty together, or fails.
pub trait ComputeQuantity {
    fn compute(&self) -> Result<(Value, Option<Uncertainty>), ComputeError>;
}

type Quantity = (Value, Option<Uncertainty>);

/// Where a value comes from. Four origins, mutually exclusive.
enum Source {
    Stored(Value),
    Measured(Readings),
    Computed {
        formula: Rc<dyn Compute>,
        cache: RefCell<Cache<Value>>,
    },
    Joint {
        formula: Rc<dyn ComputeQuantity>,
        cache: RefCell<Cache<Quantity>>,
    },
}

/// Where an uncertainty comes from.
enum UncertaintySource {
    None,
    Explicit(Uncertainty),
    /// A marker: the uncertainty is read from the readings on demand, so
    /// that correcting a reading cannot leave a stale one behind.
    Derived(Convention),
    Computed {
        formula: Rc<dyn Compute>,
        cache: RefCell<Cache<Value>>,
    },
    /// A marker, for the same reason `Derived` is: it is read from
    /// `Source::Joint`'s one cache.
    Joint,
}

/// Generic over what a run produces, because the laziness, the re-entrancy
/// guard and the failure caching are the same rules whichever it holds.
enum Cache<T> {
    Empty,
    /// The re-entrancy guard. A read that finds it has re-entered its own
    /// computation.
    Running,
    Valid(T),
    /// An override: a value assigned to a computed property, which reads like
    /// `Valid` and which no invalidation clears.
    Edited(T),
    Failed(ComputeError),
}

impl<T> Cache<T> {
    fn is_valid(&self) -> bool {
        matches!(self, Cache::Valid(_) | Cache::Edited(_))
    }
}

/// What a failing formula keeps of its last success.
#[derive(Debug, Clone)]
pub struct LastGood {
    pub value: Value,
    pub uncertainty: Option<Uncertainty>,
    pub records: Records,
}

/// One named quantity of a sample.
pub struct Property {
    source: Source,
    uncertainty: UncertaintySource,
    presentation: Presentation,
    records: Records,
    /// A value a file supplied for a formula with no record of how: an override
    /// until `--force` gives it back to its formula.
    record_missing: bool,
    /// The value a formula last gave, with its uncertainty and records, kept
    /// for while a later run of it fails.
    last_good: Option<LastGood>,
    /// The value a file wrote beside its readings.
    written: Option<Value>,
    /// Whether the record beside that value vouches for it: the statistic's own
    /// product, current or stale, and not a hand's.
    written_recorded: bool,
    /// The statistic of its readings a model declared as its value.
    location: Option<Location>,
    /// The spread a model declared as its uncertainty. Kept **beside** the
    /// source rather than inside it: a file may hold a stored uncertainty *and*
    /// a declaration of what it should be, and turning the source into
    /// `Derived` would discard the file's number and re-derive it, changing a
    /// measured quantity on load without a word.
    convention: Option<Convention>,
}

/// Debug says where a value comes from and whether it has been produced —
/// never the formula, which has nothing printable.
impl fmt::Debug for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match &self.source {
            Source::Stored(value) => format!("Stored({value:?})"),
            Source::Measured(readings) => format!("Measured({} readings)", readings.len()),
            Source::Computed { .. } => "Computed".to_string(),
            Source::Joint { .. } => "Joint".to_string(),
        };
        let uncertainty = match &self.uncertainty {
            UncertaintySource::None => "None".to_string(),
            UncertaintySource::Explicit(uncertainty) => {
                format!("Explicit({})", uncertainty.magnitude())
            }
            UncertaintySource::Derived(convention) => format!("Derived({convention:?})"),
            UncertaintySource::Computed { .. } => "Computed".to_string(),
            UncertaintySource::Joint => "Joint".to_string(),
        };
        f.debug_struct("Property")
            .field("source", &source)
            .field("uncertainty", &uncertainty)
            .field("resolved", &self.is_resolved())
            .field("records", &self.records)
            .finish()
    }
}

impl Property {
    pub fn stored(value: Value) -> Property {
        Property::with(Source::Stored(value), UncertaintySource::None)
    }

    /// A convention arrives **with** the readings and never on its own: it
    /// names a statistic of them, so it means nothing without them.
    pub fn measured(readings: Readings, convention: Option<Convention>) -> Property {
        let mut property = Property::with(
            Source::Measured(readings),
            match convention {
                Some(convention) => UncertaintySource::Derived(convention),
                None => UncertaintySource::None,
            },
        );
        // Recorded as a declaration too, so that what a file states about the
        // origin of its number does not depend on how the property was built.
        property.convention = convention;
        property
    }

    pub fn computed(formula: Rc<dyn Compute>) -> Property {
        Property::with(
            Source::Computed {
                formula,
                cache: RefCell::new(Cache::Empty),
            },
            UncertaintySource::None,
        )
    }

    /// One formula producing both numbers. The pair is set together and cleared
    /// together; neither half occurs without the other.
    pub fn joint(formula: Rc<dyn ComputeQuantity>) -> Property {
        Property::with(
            Source::Joint {
                formula,
                cache: RefCell::new(Cache::Empty),
            },
            UncertaintySource::Joint,
        )
    }

    fn with(source: Source, uncertainty: UncertaintySource) -> Property {
        Property {
            record_missing: false,
            last_good: None,
            written: None,
            written_recorded: false,
            location: None,
            convention: None,
            source,
            uncertainty,
            presentation: Presentation::default(),
            records: Records::none(),
        }
    }

    // ------------------------------------------------------------- reading

    pub fn value(&self) -> Result<Value, ComputeError> {
        match &self.source {
            Source::Stored(value) => Ok(value.clone()),
            Source::Measured(readings) => Ok(self.measured_value(readings)),
            Source::Computed { formula, cache } => {
                not_computed_is_absent(run(cache, || formula.compute()), Value::absent())
            }
            Source::Joint { formula, cache } => not_computed_is_absent(
                run(cache, || formula.compute()).map(|(value, _)| value),
                Value::absent(),
            ),
        }
    }

    pub fn uncertainty(&self) -> Result<Option<Uncertainty>, ComputeError> {
        match &self.uncertainty {
            UncertaintySource::None => Ok(None),
            UncertaintySource::Explicit(uncertainty) => Ok(Some(*uncertainty)),
            // A value written `n/a` beside readings does not apply, and has no
            // spread: the readings stay, their statistic is not its.
            UncertaintySource::Derived(_) if self.written_not_applicable() => Ok(None),
            UncertaintySource::Derived(convention) => match &self.source {
                Source::Measured(readings) => Ok(from_readings(readings, *convention)),
                // A convention only reaches a measured property; nothing else
                // has readings for it to name a statistic of.
                _ => Ok(None),
            },
            UncertaintySource::Computed { formula, cache } => {
                match not_computed_is_absent(run(cache, || formula.compute()).map(Some), None)? {
                    Some(value) => as_uncertainty(&value),
                    None => Ok(None),
                }
            }
            UncertaintySource::Joint => match &self.source {
                Source::Joint { formula, cache } => not_computed_is_absent(
                    run(cache, || formula.compute()).map(|(_, uncertainty)| uncertainty),
                    None,
                ),
                // Unreachable while the invariant holds; answering `None` rather
                // than panicking keeps a reader from paying for a defect here.
                _ => Ok(None),
            },
        }
    }

    pub fn readings(&self) -> Option<&Readings> {
        match &self.source {
            Source::Measured(readings) => Some(readings),
            _ => None,
        }
    }

    pub fn presentation(&self) -> &Presentation {
        &self.presentation
    }

    pub fn records(&self) -> &Records {
        &self.records
    }

    /// A formula's value given as *not applicable* without running it, as an
    /// input it reads is: its own value, its uncertainty none. An override is
    /// left as it is.
    pub fn settle_not_applicable(&self) {
        match &self.source {
            Source::Computed { cache, .. } => {
                if !matches!(*cache.borrow(), Cache::Edited(_)) {
                    *cache.borrow_mut() = Cache::Valid(Value::NotApplicable);
                }
            }
            Source::Joint { cache, .. } => {
                if !matches!(*cache.borrow(), Cache::Edited(_)) {
                    *cache.borrow_mut() = Cache::Valid((Value::NotApplicable, None));
                }
            }
            Source::Stored(_) | Source::Measured(_) => {}
        }
        if let UncertaintySource::Computed { cache, .. } = &self.uncertainty
            && !matches!(*cache.borrow(), Cache::Edited(_))
        {
            *cache.borrow_mut() = Cache::Valid(Value::NotApplicable);
        }
    }

    /// True for `Computed` and for `Joint` alike: both are a value a formula
    /// produces.
    pub fn is_computed(&self) -> bool {
        matches!(self.source, Source::Computed { .. } | Source::Joint { .. })
    }

    /// An uncertainty a formula gives beside the value, whatever the value's
    /// source: what a computation owes a run beside the computed values.
    pub fn has_uncertainty_formula(&self) -> bool {
        matches!(self.uncertainty, UncertaintySource::Computed { .. })
    }

    /// Both numbers from one call — `compute_quantity`.
    ///
    /// It is the quantity that is computed, not a channel of it: the pair
    /// cannot be recomputed by halves, so a record about it names no channel.
    pub fn is_joint(&self) -> bool {
        matches!(self.source, Source::Joint { .. })
    }

    /// Whether a read would be free **as far as this property can see**. It
    /// cannot see whether its inputs have moved, because it does not know their
    /// names; `PropertyHandle::is_resolved` answers the whole question.
    pub fn is_resolved(&self) -> bool {
        let value = match &self.source {
            Source::Stored(_) | Source::Measured(_) => true,
            Source::Computed { cache, .. } => cache.borrow().is_valid(),
            Source::Joint { cache, .. } => cache.borrow().is_valid(),
        };
        // Both halves. A value and an uncertainty can be computed
        // independently, so a stored value with a formula beside it has nothing
        // lazy about its source and a formula that has never run.
        let uncertainty = match &self.uncertainty {
            UncertaintySource::Computed { cache, .. } => cache.borrow().is_valid(),
            _ => true,
        };
        value && uncertainty
    }

    /// The value as it stands, running nothing: `None` when only a formula
    /// could give it — never run, running, or failed.
    pub fn peek_value(&self) -> Option<Value> {
        match &self.source {
            Source::Stored(value) => Some(value.clone()),
            Source::Measured(readings) => Some(self.measured_value(readings)),
            Source::Computed { cache, .. } => match &*cache.borrow() {
                Cache::Valid(value) | Cache::Edited(value) => Some(value.clone()),
                _ => None,
            },
            Source::Joint { cache, .. } => match &*cache.borrow() {
                Cache::Valid((value, _)) | Cache::Edited((value, _)) => Some(value.clone()),
                _ => None,
            },
        }
    }

    /// The uncertainty as it stands, running nothing: `None` when only a
    /// formula could give it, `Some(None)` when there is none. Whether the
    /// value written beside readings is `n/a`.
    fn written_not_applicable(&self) -> bool {
        self.written.as_ref().is_some_and(Value::is_not_applicable)
    }

    pub fn peek_uncertainty(&self) -> Option<Option<Uncertainty>> {
        match &self.uncertainty {
            UncertaintySource::None => Some(None),
            UncertaintySource::Explicit(uncertainty) => Some(Some(*uncertainty)),
            UncertaintySource::Derived(_) if self.written_not_applicable() => Some(None),
            UncertaintySource::Derived(convention) => Some(match &self.source {
                Source::Measured(readings) => from_readings(readings, *convention),
                _ => None,
            }),
            UncertaintySource::Computed { cache, .. } => match &*cache.borrow() {
                Cache::Valid(value) | Cache::Edited(value) => as_uncertainty(value).ok(),
                _ => None,
            },
            UncertaintySource::Joint => match &self.source {
                Source::Joint { cache, .. } => match &*cache.borrow() {
                    Cache::Valid((_, uncertainty)) | Cache::Edited((_, uncertainty)) => {
                        Some(*uncertainty)
                    }
                    _ => None,
                },
                _ => Some(None),
            },
        }
    }

    // ------------------------------------------------------------- writing

    /// On a computed property, an override: the formula stays and the value is
    /// held until a computation is asked for by name, with the records of the
    /// formula's last value. A joint pair is held with no uncertainty. Anywhere
    /// else the value replaces the source, and the records go.
    pub fn set_value(&mut self, value: Value) {
        match &self.source {
            Source::Computed { cache, .. } => {
                *cache.borrow_mut() = Cache::Edited(value);
                return;
            }
            Source::Joint { cache, .. } => {
                *cache.borrow_mut() = Cache::Edited((value, None));
                return;
            }
            // A value written beside readings is a **trial, not an erasure**.
            // The readings are the evidence and stay; the value outranks the
            // statistic they declare; the records stay too, so that the
            // quantity's own digest no longer matching reports it edited, and
            // `restore_formula` gives the statistic back.
            Source::Measured(_) => {
                self.written = Some(value);
                self.written_recorded = false;
                return;
            }
            Source::Stored(_) => {}
        }
        let was_joint = matches!(self.source, Source::Joint { .. });
        // **A record the uncertainty's formula owns is not the value's to
        // clear**. Where the value is entered and the uncertainty computed —
        // the instrument rated at a percentage of its reading — the record says
        // what that formula read, the value among it, and dropping it would
        // leave a corrected reading beside an uncertainty computed from the old
        // one, with nothing saying so.
        let uncertainty_owns = matches!(self.uncertainty, UncertaintySource::Computed { .. });
        self.source = Source::Stored(value);
        self.written = None;
        if was_joint || matches!(self.uncertainty, UncertaintySource::Derived(_)) {
            self.uncertainty = UncertaintySource::None;
        }
        if !uncertainty_owns {
            self.records = Records::none();
        }
    }

    pub fn set_readings(&mut self, readings: Readings, convention: Option<Convention>) {
        self.source = Source::Measured(readings);
        // **New readings replace no writing**. They are new evidence, so a
        // value written beside the old ones still stands and still outranks the
        // statistic; what moved is the statistic, which the record now reports
        // stale.
        self.uncertainty = match convention {
            Some(convention) => UncertaintySource::Derived(convention),
            None => UncertaintySource::None,
        };
        // A statistic the model declared keeps its record, which then says
        // *stale — readings* until it is taken again — as `set --readings`
        // leaves it. Dropped, the old value read as entered, and compute
        // never took the new statistic.
        if !self.has_declared_statistic() {
            self.records = Records::none();
        }
    }

    /// An explicit uncertainty overrides a derived, computed or joint one. The
    /// value's source is untouched: on a joint property the formula still
    /// produces both, and the half that was overridden stops being read.
    pub fn set_uncertainty(&mut self, uncertainty: Option<Uncertainty>) {
        self.uncertainty = match uncertainty {
            Some(uncertainty) => UncertaintySource::Explicit(uncertainty),
            None => UncertaintySource::None,
        };
    }

    /// The only way a computed uncertainty comes about. Separate from the
    /// value's formula because the two are independent: a value entered by hand
    /// may still have an uncertainty an instrument specification computes.
    pub fn set_uncertainty_formula(&mut self, formula: Rc<dyn Compute>) {
        self.uncertainty = UncertaintySource::Computed {
            formula,
            cache: RefCell::new(Cache::Empty),
        };
    }

    /// The value a file wrote beside its readings: the value, unless a model
    /// declared a statistic of them.
    pub fn set_written_value(&mut self, value: Option<Value>) {
        self.written = value.filter(|value| !matches!(value, Value::Absent));
        self.written_recorded = false;
    }

    /// The record beside the written value vouches for it: it is what the
    /// declared statistic gave, not what a hand wrote over it. The property
    /// cannot tell the two apart — the evidence is a digest, which
    /// `document::load_into` compares once, as it loads.
    pub fn confirm_written_as_recorded(&mut self) {
        self.written_recorded = self.written.is_some();
    }

    pub fn written_value(&self) -> Option<&Value> {
        self.written.as_ref()
    }

    /// Declares which statistic of the readings is the value, and which the
    /// uncertainty.
    pub fn declare_statistics(&mut self, value: Option<Location>, uncertainty: Option<Convention>) {
        if value.is_some() {
            self.location = value;
        }
        if let Some(convention) = uncertainty {
            self.convention = Some(convention);
            // **Only where the file supplied none.** An uncertainty the file
            // holds is what the file records; the declaration says what it
            // should be, and the two disagreeing is exactly what `validation`
            // must be able to see. Overwriting the source here would erase the
            // disagreement before anything could report it.
            if matches!(self.uncertainty, UncertaintySource::None) {
                self.uncertainty = UncertaintySource::Derived(convention);
            }
        }
    }

    pub fn declared_location(&self) -> Option<Location> {
        self.location
    }

    /// The spread a model declared, whether or not the uncertainty currently
    /// comes from it: a file's own number does not withdraw the declaration.
    /// `convention` answers the narrower question — whether the uncertainty is
    /// *presently* derived from the readings.
    pub fn declared_convention(&self) -> Option<Convention> {
        self.convention
    }

    /// Whether a value written beside readings stands over the statistic
    /// declared for them — the trial `--force` gives back.
    ///
    /// Deliberately **not** `is_edited`: freshness must not read this, because
    /// the stored value is the statistic of the readings *as they were*, so a
    /// corrected reading would make it differ with nobody having touched it.
    /// `fingerprint` answers that question over the quantity without its
    /// observations; this one answers only *is there something to restore*.
    pub fn holds_written_override(&self) -> bool {
        matches!(self.source, Source::Measured(_))
            && self.written.is_some()
            && !self.written_recorded
            && self.has_declared_statistic()
    }

    /// Whether a model declared a statistic of its readings for either channel.
    /// Such a property is derived, and records like any other.
    pub fn has_declared_statistic(&self) -> bool {
        self.location.is_some() || self.convention.is_some()
    }

    /// The convention an uncertainty is derived from readings under, if it is.
    pub fn convention(&self) -> Option<Convention> {
        match self.uncertainty {
            UncertaintySource::Derived(convention) => Some(convention),
            _ => None,
        }
    }

    /// A measured value: the value written beside the readings, else the
    /// statistic a model declared. **Else none**: no mean is taken on its own.
    /// Which statistic stands for readings is the model's to say, and a mean
    /// supplied where it said nothing was a confident number nobody chose — the
    /// state *readings with no statistic*, said as such wherever the value is
    /// asked for.
    fn measured_value(&self, readings: &Readings) -> Value {
        if let Some(written) = &self.written {
            return written.clone();
        }
        let Some(location) = self.location else {
            return Value::Absent;
        };
        let summary = crate::core::statistics::summarize(readings);
        let number = location.of(&summary);
        // Finite readings can still overflow a mean — `1.7e308, -1.7e308` —
        // and a statistic that is no number is no value, never a panic.
        Value::number(number).unwrap_or(Value::Absent)
    }

    /// Changes nothing about the quantity, which is the line `dependency-graph`
    /// relies on to avoid invalidating half a collection over a unit label.
    pub fn set_presentation(&mut self, presentation: Presentation) {
        self.presentation = presentation;
    }

    /// The loading path. Nothing else writes a record.
    pub fn set_records(&mut self, records: Records) {
        self.records = records;
    }

    /// Clears both caches. Every automatic path calls this and only this: the
    /// graph declares edges between names, not between channels.
    pub fn invalidate(&mut self) {
        self.invalidate_value();
        self.invalidate_uncertainty();
    }

    /// Clears the value's cache alone — a deliberate act, never the graph
    /// speaking. On a joint property one run fills one cache, so this clears
    /// the pair.
    pub fn invalidate_value(&mut self) {
        self.keep_last_good();
        let cleared = match &self.source {
            Source::Computed { cache, .. } => clear(cache),
            Source::Joint { cache, .. } => clear(cache),
            Source::Stored(_) | Source::Measured(_) => false,
        };
        self.forget_records_if(cleared);
    }

    pub fn invalidate_uncertainty(&mut self) {
        self.keep_last_good();
        let cleared = match (&self.uncertainty, &self.source) {
            // Never a held one, as the value's `clear` never clears an override.
            (UncertaintySource::Computed { cache, .. }, _) => clear(cache),
            (UncertaintySource::Joint, Source::Joint { cache, .. }) => clear(cache),
            _ => false,
        };
        self.forget_records_if(cleared);
    }

    /// A formula whose cache was cleared will produce the next value, and the
    /// records described the one before it — possibly a value a file supplied.
    fn forget_records_if(&mut self, cleared: bool) {
        if cleared {
            self.records = Records::none();
        }
    }

    /// Whether this property holds an override: a value held over its formula,
    /// or — where the uncertainty is the only formula — an uncertainty held
    /// over its own, or a value written over the statistic its readings
    /// declare.
    pub fn is_edited(&self) -> bool {
        let held_uncertainty = match &self.uncertainty {
            UncertaintySource::Computed { cache, .. } => {
                matches!(*cache.borrow(), Cache::Edited(_))
            }
            _ => false,
        };
        match &self.source {
            Source::Computed { cache, .. } => matches!(*cache.borrow(), Cache::Edited(_)),
            Source::Joint { cache, .. } => matches!(*cache.borrow(), Cache::Edited(_)),
            // A value written beside readings that **differs** from the
            // statistic declared for it is a trial: the
            // readings stand, the writing outranks what they give, and
            // `restore_formula` — `--force` — gives it back. Saying so here is
            // what makes `edited` list it and `compute --force` target it.
            //
            // A value written beside readings is **not** judged here. What a
            // hand wrote cannot be told apart from what the readings moved
            // underneath: the stored value is the statistic of the readings as
            // they *were*, so any correction makes it differ from what they
            // give now, with nobody having touched it. `fingerprint` answers
            // that question instead, over the quantity without its
            // observations, where the two are separable.
            Source::Measured(_) => held_uncertainty,
            Source::Stored(_) => held_uncertainty,
        }
    }

    /// The message of the failure a value holds — its formula's last run, or
    /// the one its file recorded — as one line of at most 200 characters. What
    /// a formula raised, as it was raised, where this session's cache holds it:
    /// the value's, else the uncertainty's. A failure read from a file carries
    /// only its text, and has none.
    pub fn failure_source(&self) -> Option<Rc<dyn Error>> {
        fn source_of<T>(cache: &Cache<T>) -> Option<Rc<dyn Error>> {
            match cache {
                Cache::Failed(ComputeError::Failed { source }) => Some(Rc::clone(source)),
                _ => None,
            }
        }
        let value = match &self.source {
            Source::Computed { cache, .. } => source_of(&cache.borrow()),
            Source::Joint { cache, .. } => source_of(&cache.borrow()),
            Source::Stored(_) | Source::Measured(_) => None,
        };
        value.or_else(|| match &self.uncertainty {
            UncertaintySource::Computed { cache, .. } => source_of(&cache.borrow()),
            _ => None,
        })
    }

    pub fn peek_failure(&self) -> Option<String> {
        let value = match &self.source {
            Source::Computed { cache, .. } => failure_of(&cache.borrow()),
            Source::Joint { cache, .. } => failure_of(&cache.borrow()),
            Source::Stored(_) | Source::Measured(_) => None,
        };
        value.or_else(|| match &self.uncertainty {
            UncertaintySource::Computed { cache, .. } => failure_of(&cache.borrow()),
            _ => None,
        })
    }

    /// Whether its formula's last run failed, so that no value stands.
    pub fn has_failed(&self) -> bool {
        let value = match &self.source {
            Source::Computed { cache, .. } => matches!(*cache.borrow(), Cache::Failed(_)),
            Source::Joint { cache, .. } => matches!(*cache.borrow(), Cache::Failed(_)),
            Source::Stored(_) | Source::Measured(_) => false,
        };
        let uncertainty = match &self.uncertainty {
            UncertaintySource::Computed { cache, .. } => {
                matches!(*cache.borrow(), Cache::Failed(_))
            }
            _ => false,
        };
        value || uncertainty
    }

    /// Drops an override for its formula: the cache empties and the records of
    /// the formula's last value go, so the next read runs it. Whether there
    /// was one.
    pub fn restore_formula(&mut self) -> bool {
        let restored = match &self.source {
            Source::Computed { cache, .. } => release(cache),
            Source::Joint { cache, .. } => release(cache),
            // The uncertainty is one hold here; on a measured property the
            // value written over its declared statistic is the other, and
            // `--force` is how it is given back.
            Source::Measured(_) => {
                let released = match &self.uncertainty {
                    UncertaintySource::Computed { cache, .. } => release(cache),
                    _ => false,
                };
                let written = self.written.take().is_some();
                self.written_recorded = false;
                released || written
            }
            Source::Stored(_) => match &self.uncertainty {
                UncertaintySource::Computed { cache, .. } => release(cache),
                _ => false,
            },
        };
        self.forget_records_if(restored);
        if restored {
            self.record_missing = false;
        }
        restored
    }

    /// The value its formula last gave, while a later run of it has failed.
    pub fn last_good(&self) -> Option<&LastGood> {
        let failed = match &self.source {
            Source::Computed { cache, .. } => matches!(*cache.borrow(), Cache::Failed(_)),
            Source::Joint { cache, .. } => matches!(*cache.borrow(), Cache::Failed(_)),
            Source::Stored(_) | Source::Measured(_) => false,
        };
        if failed {
            self.last_good.as_ref()
        } else {
            None
        }
    }

    /// Keeps a produced value aside before its cache is cleared, for a run that
    /// fails to fall back on.
    fn keep_last_good(&mut self) {
        let held = match &self.source {
            Source::Computed { cache, .. } => match &*cache.borrow() {
                Cache::Valid(value) => Some((value.clone(), None)),
                _ => None,
            },
            Source::Joint { cache, .. } => match &*cache.borrow() {
                Cache::Valid((value, uncertainty)) => Some((value.clone(), Some(*uncertainty))),
                _ => None,
            },
            Source::Stored(_) | Source::Measured(_) => None,
        };
        if let Some((value, joint)) = held {
            let uncertainty = joint.unwrap_or_else(|| self.peek_uncertainty().flatten());
            self.last_good = Some(LastGood {
                value,
                uncertainty,
                records: self.records.clone(),
            });
        }
    }

    /// A value its file supplied with no record of its formula.
    pub fn is_record_missing(&self) -> bool {
        self.record_missing
    }

    /// Holds the value a file supplied as an override: what loading does with a
    /// computed value whose digest no longer matches. A formula never run has
    /// nothing to hold. Where the uncertainty is the only formula, it is the
    /// uncertainty that is held.
    pub fn hold_as_edited(&mut self) {
        match &self.source {
            Source::Computed { cache, .. } => hold(cache),
            Source::Joint { cache, .. } => hold(cache),
            Source::Stored(_) | Source::Measured(_) => {
                if let UncertaintySource::Computed { cache, .. } = &self.uncertainty {
                    hold(cache);
                }
            }
        }
    }

    /// Holds a value its file supplied for a formula with no record of how it
    /// came as an override, never taken for current. Says whether it did: an
    /// override, a failure or a record already speak for themselves.
    pub fn hold_record_missing(&mut self) -> bool {
        if self.is_edited() || self.records.computed.is_some() || self.records.failure.is_some() {
            return false;
        }
        self.hold_as_edited();
        self.record_missing = self.is_edited();
        self.record_missing
    }

    /// Replaces a computed source with the plain value it currently yields,
    /// keeping any records it carries. Left unchanged when it fails.
    pub fn materialize(&mut self) -> Result<(), ComputeError> {
        // An independently computed uncertainty is materialized on its own,
        // whether or not the value beside it is computed: nothing else ever
        // runs that formula, so a stored value would otherwise reach a file
        // with its uncertainty missing and no error.
        let independent = matches!(self.uncertainty, UncertaintySource::Computed { .. });
        if !self.is_computed() && !independent {
            return Ok(());
        }
        // Both formulas run before either source is replaced, so a failure in
        // one leaves the property exactly as it was.
        let value = self.is_computed().then(|| self.value()).transpose()?;
        // A joint source also writes the uncertainty it yielded: dropping the
        // formula would otherwise leave the marker pointing at nothing.
        let settled = match self.uncertainty {
            UncertaintySource::Joint | UncertaintySource::Computed { .. } => {
                Some(self.uncertainty()?)
            }
            _ => None,
        };
        if let Some(value) = value {
            self.source = Source::Stored(value);
        }
        if let Some(uncertainty) = settled {
            self.uncertainty = match uncertainty {
                Some(uncertainty) => UncertaintySource::Explicit(uncertainty),
                None => UncertaintySource::None,
            };
        }
        Ok(())
    }

    // ------------------------------------------------------------- filling

    /// A declaration filled by what a file recorded: the file wins wherever it
    /// speaks, and the formula is kept, its cache holding the file's value so
    /// that loading runs nothing. Refused before anything changes.
    pub fn fill_from(&mut self, file: Property) -> Result<(), FillConflict> {
        if let Some(conflict) = self.conflict_with(&file) {
            return Err(conflict);
        }
        let Property {
            source: file_source,
            uncertainty: file_uncertainty,
            presentation: file_presentation,
            records: file_records,
            written: file_written,
            ..
        } = file;
        let file_value = match &file_source {
            // Kept when not applicable: an answer, not an absence.
            Source::Stored(value) if !matches!(value, Value::Absent) => Some(value.clone()),
            _ => None,
        };
        let file_explicit = match &file_uncertainty {
            UncertaintySource::Explicit(uncertainty) => Some(*uncertainty),
            _ => None,
        };
        let file_readings = matches!(file_source, Source::Measured(_));
        let marked = file_records
            .fingerprint
            .as_ref()
            .is_some_and(Fingerprint::is_edited);
        // A failure the file recorded stands in the formula's cache, as a value
        // would.
        let failure = file_records.failure.clone();

        // Whether the value has a formula, before the source is replaced: a
        // mark on a property whose only formula is its uncertainty holds the
        // uncertainty.
        let value_computed = self.is_computed();
        let source = match std::mem::replace(&mut self.source, Source::Stored(Value::absent())) {
            Source::Computed { formula, .. } => Source::Computed {
                formula,
                cache: RefCell::new(match &file_value {
                    Some(value) if marked => Cache::Edited(value.clone()),
                    // A failure beside a value: the value is the last good one.
                    Some(_) if failure.is_some() => failed_cache(&failure),
                    Some(value) => Cache::Valid(value.clone()),
                    None => failed_cache(&failure),
                }),
            },
            Source::Joint { formula, .. } => Source::Joint {
                formula,
                cache: RefCell::new(match &file_value {
                    Some(value) if marked => Cache::Edited((value.clone(), file_explicit)),
                    Some(_) if failure.is_some() => failed_cache(&failure),
                    Some(value) => Cache::Valid((value.clone(), file_explicit)),
                    None => failed_cache(&failure),
                }),
            },
            Source::Stored(_) | Source::Measured(_) => file_source,
        };
        let uncertainty = match std::mem::replace(&mut self.uncertainty, UncertaintySource::None) {
            UncertaintySource::Joint => UncertaintySource::Joint,
            UncertaintySource::Computed { formula, .. } => UncertaintySource::Computed {
                formula,
                cache: RefCell::new(match file_explicit {
                    Some(uncertainty) => {
                        let held = Value::number(uncertainty.magnitude())
                            .expect("an uncertainty is finite");
                        if marked && !value_computed {
                            Cache::Edited(held)
                        } else {
                            Cache::Valid(held)
                        }
                    }
                    None => Cache::Empty,
                }),
            },
            UncertaintySource::Derived(convention) if file_readings => {
                UncertaintySource::Derived(convention)
            }
            _ => file_uncertainty,
        };
        // The two markers name where an uncertainty is read from, and each is
        // meaningful only beside the source it reads.
        self.uncertainty = match (&uncertainty, &source) {
            (UncertaintySource::Joint, Source::Joint { .. })
            | (UncertaintySource::Derived(_), Source::Measured(_)) => uncertainty,
            (UncertaintySource::Joint, _) | (UncertaintySource::Derived(_), _) => {
                UncertaintySource::None
            }
            _ => uncertainty,
        };
        self.source = source;

        let declared = std::mem::take(&mut self.presentation);
        self.presentation = Presentation {
            unit: file_presentation.unit.or(declared.unit),
            symbol: file_presentation.symbol.or(declared.symbol),
            // Neither holds one: a precision is the project's.
            precision: None,
        };
        self.written = file_written;
        self.written_recorded = false;
        self.last_good = match (&file_value, &failure) {
            (Some(value), Some(_)) if !marked && self.is_computed() => Some(LastGood {
                value: value.clone(),
                uncertainty: file_explicit,
                records: Records {
                    failure: None,
                    ..file_records.clone()
                },
            }),
            _ => None,
        };
        self.records = file_records;
        self.record_missing = false;
        Ok(())
    }

    /// Why `file` cannot fill this declaration, if it cannot. One rule, read by
    /// every caller that must refuse before anything moves.
    pub(crate) fn conflict_with(&self, file: &Property) -> Option<FillConflict> {
        (self.is_computed() && file.readings().is_some())
            .then_some(FillConflict::ReadingsUnderFormula)
    }
}

/// Run a formula once, guarding against re-entry and caching the outcome —
/// including a failure, because re-running a formula that raised is not free
/// and two reads must see the same error.
/// A failure a file recorded, standing where the formula's error would.
#[derive(Debug)]
struct RecordedFailure(String);

impl fmt::Display for RecordedFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for RecordedFailure {}

fn failed_cache<T>(failure: &Option<String>) -> Cache<T> {
    match failure {
        Some(message) => Cache::Failed(ComputeError::failed(RecordedFailure(message.clone()))),
        None => Cache::Empty,
    }
}

fn failure_of<T>(cache: &Cache<T>) -> Option<String> {
    let Cache::Failed(error) = cache else {
        return None;
    };
    let message = error.to_string();
    let line = message.lines().next().unwrap_or_default();
    Some(if line.chars().count() > 200 {
        let mut cut: String = line.chars().take(199).collect();
        cut.push('…');
        cut
    } else {
        line.to_string()
    })
}

/// Empties a cache an invalidation may clear: never an override.
fn clear<T>(cache: &RefCell<Cache<T>>) -> bool {
    let mut slot = cache.borrow_mut();
    if matches!(*slot, Cache::Edited(_)) {
        return false;
    }
    *slot = Cache::Empty;
    true
}

fn release<T>(cache: &RefCell<Cache<T>>) -> bool {
    let mut slot = cache.borrow_mut();
    if !matches!(*slot, Cache::Edited(_)) {
        return false;
    }
    *slot = Cache::Empty;
    true
}

fn hold<T>(cache: &RefCell<Cache<T>>) {
    let mut slot = cache.borrow_mut();
    if let Cache::Valid(_) = &*slot
        && let Cache::Valid(value) = std::mem::replace(&mut *slot, Cache::Empty)
    {
        *slot = Cache::Edited(value);
    }
}

/// A value no computation has produced, read where reads do not compute: absent,
/// as every reader of an absent value already expects.
fn not_computed_is_absent<T>(
    outcome: Result<T, ComputeError>,
    absent: T,
) -> Result<T, ComputeError> {
    match outcome {
        Err(ComputeError::NotComputed) => Ok(absent),
        other => other,
    }
}

/// What reading does on every thread, until a computation asked for says
/// otherwise on its own.
///
/// **Process-wide, not per thread.** It was a thread-local set once, by the
/// Python extension, on the thread that imported it — and a `Thread` or a
/// `ThreadPoolExecutor` read with the default, so a filter or an export there
/// ran formulas that take minutes, which this exists to prevent.
static READS_COMPUTE_BY_DEFAULT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

thread_local! {
    /// Set while a computation asked for runs on this thread: `Some(true)`.
    static COMPUTING: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether reading a value may run its formula. It may by default; the Python
/// extension turns it off, and only a computation asked for turns it back on.
pub fn reads_compute() -> bool {
    COMPUTING
        .with(std::cell::Cell::get)
        .unwrap_or_else(|| READS_COMPUTE_BY_DEFAULT.load(std::sync::atomic::Ordering::Relaxed))
}

/// Sets what reading does on every thread of the process.
pub fn set_reads_compute(on: bool) {
    READS_COMPUTE_BY_DEFAULT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Runs `body` with reads computing, as a computation asked for does, and
/// restores what was set before — **however `body` ends**. Restored by a
/// guard rather than after the call: a panic unwinding through a formula left
/// the flag on, and every read afterwards, in every sample, ran its formula.
pub fn computing<T>(body: impl FnOnce() -> T) -> T {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMPUTING.with(|flag| flag.set(self.0));
        }
    }
    let _restore = Restore(COMPUTING.with(std::cell::Cell::get));
    COMPUTING.with(|flag| flag.set(Some(true)));
    body()
}

fn run<T: Clone>(
    cache: &RefCell<Cache<T>>,
    formula: impl FnOnce() -> Result<T, ComputeError>,
) -> Result<T, ComputeError> {
    {
        let mut slot = cache.borrow_mut();
        match &*slot {
            Cache::Valid(value) | Cache::Edited(value) => return Ok(value.clone()),
            Cache::Failed(error) => return Err(error.clone()),
            // A property does not know its own name, so the path is empty here
            // and completed by each frame that does, on the way out.
            Cache::Running => return Err(ComputeError::Cycle { path: Vec::new() }),
            // Where reads do not compute, a value never produced stays so.
            Cache::Empty if !reads_compute() => return Err(ComputeError::NotComputed),
            Cache::Empty => *slot = Cache::Running,
        }
    }
    // The borrow is released before the formula runs: it may read this very
    // property, and must find `Running` rather than a panic.
    let outcome = formula();
    let mut slot = cache.borrow_mut();
    *slot = match &outcome {
        Ok(value) => Cache::Valid(value.clone()),
        // Stopped, not failed: the value is as it was before the run, and never
        // computed is what it was — the run invalidated it first.
        Err(ComputeError::Interrupted { .. }) => Cache::Empty,
        Err(error) => Cache::Failed(error.clone()),
    };
    outcome
}

/// A computed uncertainty yields the scalar domain, and not every value is a
/// magnitude.
fn as_uncertainty(value: &Value) -> Result<Option<Uncertainty>, ComputeError> {
    let magnitude = match value {
        // The formula declined to give one.
        Value::Absent => return Ok(None),
        Value::Number(number) => *number,
        Value::Integer(integer) => *integer as f64,
        other => {
            return Err(ComputeError::failed(NotAMagnitude::Kind(other.kind())));
        }
    };
    match Uncertainty::new(magnitude) {
        Ok(uncertainty) => Ok(Some(uncertainty)),
        Err(error) => Err(ComputeError::failed(NotAMagnitude::Invalid(error))),
    }
}

#[derive(Debug)]
enum NotAMagnitude {
    Kind(ValueKind),
    Invalid(crate::core::uncertainty::UncertaintyError),
}

impl fmt::Display for NotAMagnitude {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotAMagnitude::Kind(found) => write!(
                f,
                "an uncertainty formula must return a number, and returned {found}"
            ),
            NotAMagnitude::Invalid(error) => write!(f, "{error}"),
        }
    }
}

impl Error for NotAMagnitude {}

/// Why a file cannot fill a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillConflict {
    /// Two answers for one value: the evidence, and the formula the model says
    /// the value is. Keeping either silently discards the other.
    ReadingsUnderFormula,
}

impl fmt::Display for FillConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FillConflict::ReadingsUnderFormula => f.write_str(
                "the file holds readings for a value the model computes: remove the \
                 formula to keep the readings, or the readings to keep the formula",
            ),
        }
    }
}

impl Error for FillConflict {}

/// The two ways reading a value fails.
#[derive(Debug, Clone)]
pub enum ComputeError {
    Failed {
        source: Rc<dyn Error>,
    },
    Cycle {
        path: Vec<Identifier>,
    },
    /// Read where reads do not compute, before any computation produced it.
    NotComputed,
    /// The formula was stopped from outside — a person pressing Ctrl-C — and
    /// did not finish. **Not a failure**: nothing is cached, so the value is
    /// left as it was before the run began, never `failed`.
    Interrupted {
        source: Rc<dyn Error>,
    },
}

impl ComputeError {
    pub fn failed(source: impl Error + 'static) -> ComputeError {
        ComputeError::Failed {
            source: Rc::new(source),
        }
    }

    pub fn interrupted(source: impl Error + 'static) -> ComputeError {
        ComputeError::Interrupted {
            source: Rc::new(source),
        }
    }

    /// Prepend the name of the property whose formula was running.
    ///
    /// Returns `Failed` unchanged: a path describes a cycle, not a raise.
    pub fn within(self, name: &Identifier) -> ComputeError {
        match self {
            ComputeError::Cycle { mut path } => {
                path.insert(0, name.clone());
                ComputeError::Cycle { path }
            }
            failed => failed,
        }
    }
}

/// Two reads of a failed property return the **same** error, so equality is
/// identity for a raise and content for a cycle.
impl PartialEq for ComputeError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ComputeError::Failed { source: a }, ComputeError::Failed { source: b })
            | (ComputeError::Interrupted { source: a }, ComputeError::Interrupted { source: b }) => {
                Rc::ptr_eq(a, b)
            }
            (ComputeError::Cycle { path: a }, ComputeError::Cycle { path: b }) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for ComputeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComputeError::Failed { source } => write!(f, "{source}"),
            ComputeError::Interrupted { source } => write!(f, "interrupted: {source}"),
            ComputeError::NotComputed => {
                write!(f, "this value was never computed: compute() runs it")
            }
            ComputeError::Cycle { path } => {
                let drawn: Vec<String> = path.iter().map(Identifier::to_string).collect();
                write!(
                    f,
                    "a computation re-entered itself: {}. A formula is reading the \
                     property it is computing",
                    drawn.join(" \u{2192} ")
                )
            }
        }
    }
}

impl Error for ComputeError {}

/// Read permissively: a digest is compared, never interpreted, so a truncated
/// or hand-mangled one simply never matches and the property reports itself
/// stale — which is louder than refusing to open the file it is in.
///
/// `{edited: …}` is the same digest, marked.
impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Fingerprint, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Written {
            Plain(DigestText),
            Marked(Marked),
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Marked {
            edited: DigestText,
        }
        Ok(match Written::deserialize(deserializer)? {
            Written::Plain(hex) => Fingerprint::new(hex.into_text()),
            Written::Marked(Marked { edited }) => Fingerprint::edited(edited.into_text()),
        })
    }
}

/// A digest as a file holds it. Written unquoted, one made only of digits reads
/// as an integer and one like `844409772e92` as a float; both still read, and
/// the integer gets its leading zeros back.
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum DigestText {
    Text(String),
    Whole(u64),
    Float(f64),
}

impl DigestText {
    pub fn into_text(self) -> String {
        match self {
            DigestText::Text(text) => text,
            DigestText::Whole(number) => digits(number),
            // The float lost the text it was read from, so it matches nothing
            // and the value reads stale: computing it again writes it quoted.
            DigestText::Float(number) => number.to_string(),
        }
    }
}

/// The twelve digits of a digest read as an integer.
pub fn digits(number: u64) -> String {
    format!("{number:012}")
}

/// The three spellings: bare for the sample, `row.` for this row's
/// cell, `table.column` for a whole column.
///
/// They cannot be confused because `row` is a reserved column name, so a dotted
/// name beginning with it is never a table.
impl InputName {
    /// The name as a file writes it: `foam`, `row.wort`,
    /// `mashing.wort`.
    pub fn parse(text: &str) -> Result<InputName, String> {
        let named = |part: &str| Identifier::new(part).map_err(|error| error.to_string());
        match text.split_once('.') {
            None => Ok(InputName::Named(named(text)?)),
            Some(("row", column)) => Ok(InputName::Cell(named(column)?)),
            Some((table, column)) => Ok(InputName::Column {
                table: named(table)?,
                column: named(column)?,
            }),
        }
    }
}

impl<'de> Deserialize<'de> for InputName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<InputName, D::Error> {
        InputName::parse(&String::deserialize(deserializer)?).map_err(DeError::custom)
    }
}
