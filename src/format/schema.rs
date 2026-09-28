//! The portable shape of a sample: the exact set of fields a file may contain,
//! what each means, and which schema versions this build understands.
//!
//! It knows nothing about YAML syntax or about Markdown. It **derives
//! `Deserialize` and not `Serialize`**, which makes it structural: this
//! shape can be read by serde and cannot be written by it, so there is no
//! second emitter to drift from `canonicalization`.
//!

use std::fmt;
use std::ops::RangeInclusive;

use indexmap::IndexMap;
use serde::Deserialize;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, SeqAccess, Visitor};

use crate::core::formatting::{FormatError, Precision, Presentation};
use crate::core::identifier::{Identifier, IdentifierError};
use crate::core::property::DeclaredStatistics;
use crate::core::property::{Fingerprint, InputName, InputRecord, Produced, Property, Records};
use crate::core::sample::{AttributeValue, Sample, SampleError};
use crate::core::statistics::Location;
use crate::core::table::{ColumnMeta, RowAddress, Table};
use crate::core::uncertainty::Convention;
use crate::core::uncertainty::Uncertainty;
use crate::core::value::{Readings, Value, recognize_text};

/// The version this build writes.
pub fn version() -> u32 {
    1
}

fn supported() -> RangeInclusive<u32> {
    1..=1
}

pub fn supports(version: u32) -> bool {
    supported().contains(&version)
}

/// **An unsupported version is refused, not interpreted.** Unknown fields might
/// carry meaning that changes how known fields should be read, and a plausible
/// misreading of someone's records is worse than a refusal to open it.
pub fn check_version(version: u32) -> Result<(), SchemaError> {
    if supports(version) {
        Ok(())
    } else {
        Err(SchemaError::UnsupportedVersion {
            found: version,
            supported: supported(),
        })
    }
}

// ------------------------------------------------------------------- shape

/// A tag as a file writes it: an identifier, or text that is not one, which is
/// kept, reported, and repaired by a rename or a migration.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Usable(Identifier),
    Unusable(String),
}

impl Tag {
    pub fn of(written: &str) -> Tag {
        match Identifier::new(written) {
            Ok(tag) => Tag::Usable(tag),
            Err(_) => Tag::Unusable(written.to_string()),
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tag::Usable(tag) => write!(f, "{tag}"),
            Tag::Unusable(text) => f.write_str(text),
        }
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Tag, D::Error> {
        struct TagVisitor;
        impl serde::de::Visitor<'_> for TagVisitor {
            type Value = Tag;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a tag")
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Tag, E> {
                Ok(Tag::of(text))
            }
            fn visit_i64<E: serde::de::Error>(self, number: i64) -> Result<Tag, E> {
                Ok(Tag::of(&number.to_string()))
            }
            fn visit_u64<E: serde::de::Error>(self, number: u64) -> Result<Tag, E> {
                Ok(Tag::of(&number.to_string()))
            }
            fn visit_f64<E: serde::de::Error>(self, number: f64) -> Result<Tag, E> {
                Ok(Tag::of(&number.to_string()))
            }
            fn visit_bool<E: serde::de::Error>(self, flag: bool) -> Result<Tag, E> {
                Ok(Tag::of(&flag.to_string()))
            }
        }
        deserializer.deserialize_any(TagVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SampleSchema {
    /// A file without it is read as the current version, and the next write
    /// adds it.
    #[serde(default = "version")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub properties: IndexMap<Identifier, PropertySchema>,
    #[serde(default)]
    pub tables: IndexMap<Identifier, TableSchema>,
    /// Unknown top-level keys are attributes, which is why the root cannot deny
    /// them. What keeps an unknown field refused there is the **shape**: an
    /// attribute is one `Value`, so a mistyped `propertys:` fails because its
    /// value is a mapping.
    #[serde(flatten)]
    pub attributes: IndexMap<Identifier, AttributeValue>,
}

impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(AttributeVisitor)
    }
}

struct AttributeVisitor;

impl<'de> Visitor<'de> for AttributeVisitor {
    type Value = AttributeValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an attribute scalar or a homogeneous list of attribute scalars")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::boolean(value).into())
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::integer(value).into())
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        i64::try_from(value)
            .map(Value::integer)
            .map(AttributeValue::from)
            .map_err(|_| E::custom(format!("{value} is too large to be an integer here")))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Value::number(value)
            .map(AttributeValue::from)
            .map_err(E::custom)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        recognize_text(value)
            .map(AttributeValue::from)
            .map_err(E::custom)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::absent().into())
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::absent().into())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<Value>()? {
            values.push(value);
        }
        AttributeValue::list(values).map_err(de::Error::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
        Err(de::Error::custom(
            "a mapping is not an attribute: use one scalar or one homogeneous list",
        ))
    }
}

/// A property is written as a **bare scalar** when it carries only a value, and
/// as a mapping otherwise. Both are this type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropertySchema {
    /// `None` *is* `Value::Absent`: a file omits what it does not record.
    pub value: Option<Value>,
    pub readings: Option<Vec<f64>>,
    pub uncertainty: Option<f64>,
    pub unit: Option<String>,
    pub symbol: Option<String>,
    /// Raw here, `InputRecord` at runtime: which form an entry takes is decided
    /// by the **name**, and a parser does not have the sample's namespace to
    /// decide with. Which statistic of the readings stood for each channel.
    pub statistics: Option<Statistics>,
    pub computed: Option<Computed>,
    pub fingerprint: Option<Fingerprint>,
    /// `fingerprint: {failed: "…"}`: a value whose formula raised, and why.
    /// Never beside a `fingerprint` digest or a `value`.
    pub failure: Option<String>,
}

/// Which statistic of its readings stood for each channel of a property. A file
/// states it so that a reader with no model knows what the two numbers were
/// meant to be; absent where nothing declared one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Statistics {
    pub value: Option<Location>,
    pub uncertainty: Option<Convention>,
}

/// The inputs of one formula, as a file records them.
pub type Inputs = IndexMap<InputName, Recorded>;

/// What a formula produced, and from what.
///
/// A record naming **no channel** — `computed: {malt: 1bfc…}` — means the
/// quantity as a whole, and that one shape serves three cases that share one
/// meaning: a file written before this record had channels, a
/// `compute_quantity` that returns a pair from one call and therefore cannot
/// be recomputed by halves, and *not known*.
///
/// A record naming channels — `computed: {v: {…}, u: {…}}` — says which number
/// each formula produced and what it read. Without it the core cannot tell a
/// value that was computed from one that was entered beside a computed
/// uncertainty, and answered *edited since it was computed* about a value
/// nothing had ever computed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Computed {
    pub quantity: Option<Inputs>,
    pub value: Option<Inputs>,
    pub uncertainty: Option<Inputs>,
}

impl Computed {
    /// The whole quantity's inputs, for a caller asking only *what does this
    /// read* — which is most of them.
    pub fn inputs(&self) -> Inputs {
        let mut all = Inputs::new();
        for held in [&self.quantity, &self.value, &self.uncertainty]
            .into_iter()
            .flatten()
        {
            for (name, record) in held {
                all.entry(name.clone()).or_insert_with(|| record.clone());
            }
        }
        all
    }

    /// Whether any channel is named: a record that says which number it made.
    pub fn names_channels(&self) -> bool {
        self.value.is_some() || self.uncertainty.is_some()
    }
}

impl<'de> Deserialize<'de> for Computed {
    /// Both shapes, told apart by what a key holds rather than by what it is
    /// called.
    ///
    /// `{v: {malt: 1bfc…}}` names a channel: its value is a *map of inputs*.
    /// `{malt: 1bfc…}` names an input: its value is a digest, or the
    /// `{edited: …}` mark of an override. So a record whose every key is `v`
    /// or `u` **and** whose every value is an input map is the keyed form, and
    /// everything else is the quantity's.
    ///
    /// The one shape that cannot be written is a channel whose formula reads an
    /// input literally called `edited`, since `{v: {edited: …}}` is read as an
    /// input named `v` carrying an override mark — which is what it meant
    /// before this decision, and compatibility outranks a name RESERVED refuses
    /// anyway.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Computed, D::Error> {
        use serde::de::Error as _;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Entry {
            /// Tried first: a digest, or `{edited: …}`.
            Record(Recorded),
            /// A channel's inputs.
            Inputs(Inputs),
        }
        let raw: IndexMap<String, Entry> = IndexMap::deserialize(d)?;
        let mut computed = Computed::default();
        let mut common = Inputs::new();
        for (key, entry) in raw {
            match (key.as_str(), entry) {
                // A channel: a key spelt `v` or `u` holding a map of inputs.
                ("v", Entry::Inputs(inputs)) => computed.value = Some(inputs),
                ("u", Entry::Inputs(inputs)) => computed.uncertainty = Some(inputs),
                // Anything else is an input this property reads, common to
                // every formula it has.
                (_, Entry::Record(record)) => {
                    common.insert(InputName::parse(&key).map_err(D::Error::custom)?, record);
                }
                (_, Entry::Inputs(_)) => {
                    // `value` and `uncertainty` were a channel's names once:
                    // said as such rather than as a misplaced map.
                    retired_spelling::<D::Error>(&key)?;
                    return Err(D::Error::custom(format!(
                        "'{key}' holds a map of inputs, which only a channel does: \
                         name it 'v' or 'u', or record a digest"
                    )));
                }
            }
        }
        // An empty record is a formula that reads nothing, and says so — which
        // is not the same as a record that names only channels.
        if !common.is_empty() || !computed.names_channels() {
            computed.quantity = Some(common);
        }
        Ok(computed)
    }
}

/// A record under the channel its formula produced, where one did, and what one
/// formula alone reads under its own.
///
/// **The file factors what a declaration repeats.** A digest written twice is
/// a digest that can diverge, so an input both formulas read is stated once,
/// at the quantity's level, and only what belongs to one channel is keyed.
fn keyed(produced: Option<Produced>, inputs: Inputs, channel_only: &ChannelOnly) -> Computed {
    match produced {
        // One formula produced one number: everything it read is that
        // channel's, and there is nothing to factor.
        Some(Produced::Value) => Computed {
            value: Some(inputs),
            ..Computed::default()
        },
        Some(Produced::Uncertainty) => Computed {
            uncertainty: Some(inputs),
            ..Computed::default()
        },
        None if channel_only.is_empty() => Computed::from(inputs),
        None => {
            let mut computed = Computed::default();
            let mut common = Inputs::new();
            for (name, record) in inputs {
                match channel_only.get(&name) {
                    Some(Produced::Value) => {
                        computed.value.get_or_insert_default().insert(name, record);
                    }
                    Some(Produced::Uncertainty) => {
                        computed
                            .uncertainty
                            .get_or_insert_default()
                            .insert(name, record);
                    }
                    None => {
                        common.insert(name, record);
                    }
                }
            }
            computed.quantity = Some(common);
            computed
        }
    }
}

/// Which channel a written record names, where it names one.
///
/// **Only where the record names nothing else**: a record holding common inputs
/// beside a channel's says that both formulas ran and one read more than the
/// other, not that one channel alone was produced.
pub fn channel_of_record(computed: &Computed) -> Option<Produced> {
    if computed.quantity.is_some() {
        return None;
    }
    match (computed.value.is_some(), computed.uncertainty.is_some()) {
        (true, false) => Some(Produced::Value),
        (false, true) => Some(Produced::Uncertainty),
        _ => None,
    }
}

/// Which inputs of a record belong to one channel's formula alone.
type ChannelOnly = IndexMap<InputName, Produced>;

fn channels_of_record(computed: &Computed) -> ChannelOnly {
    let mut only = ChannelOnly::new();
    // Where it names one channel and nothing else, `produced` says so and
    // every input is that formula's: there is no split to keep. Anywhere else
    // there is — including two channels with nothing common, which this once
    // skipped, so that the next save flattened a record that had said which
    // formula read what.
    if channel_of_record(computed).is_some() {
        return only;
    }
    for (channel, inputs) in [
        (Produced::Value, &computed.value),
        (Produced::Uncertainty, &computed.uncertainty),
    ] {
        for name in inputs.iter().flatten().map(|(name, _)| name) {
            only.insert(name.clone(), channel);
        }
    }
    only
}

impl From<Inputs> for Computed {
    fn from(inputs: Inputs) -> Computed {
        Computed {
            quantity: Some(inputs),
            ..Computed::default()
        }
    }
}

/// One entry of a `computed` record, as a file writes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    /// A digest or an attribute's value: which is the name's to decide.
    Value(Value),
    /// `{edited: …}`, the digest of an override, whatever the name.
    Edited(Fingerprint),
}

impl From<Value> for Recorded {
    fn from(value: Value) -> Recorded {
        Recorded::Value(value)
    }
}

impl<'de> Deserialize<'de> for Recorded {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Recorded, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Marked(Marked),
            Plain(Value),
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Marked {
            edited: crate::core::property::DigestText,
        }
        Ok(match Written::deserialize(d)? {
            Written::Marked(Marked { edited }) => {
                Recorded::Edited(Fingerprint::edited(edited.into_text()))
            }
            Written::Plain(value) => Recorded::Value(value),
        })
    }
}

/// A statistic's name, refused when it is not one. Dropping it would be the
/// lossy round trip `schema` forbids: the file would come out saying nothing
/// about where its number came from.
struct StatisticName<T>(T);

impl<'de> Deserialize<'de> for StatisticName<Location> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Location::from_name(&name).map(StatisticName).ok_or_else(|| {
            <D::Error as serde::de::Error>::custom(format!(
                "'{name}' is not a statistic that says where readings lie;                  the names are {}",
                Location::ALL
                    .iter()
                    .map(|one| one.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }
}

impl<'de> Deserialize<'de> for StatisticName<Convention> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Convention::from_name(&name)
            .map(StatisticName)
            .ok_or_else(|| {
                <D::Error as serde::de::Error>::custom(format!(
                    "'{name}' is not a spread; the names are {}",
                    Convention::ALL
                        .iter()
                        .map(|one| one.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatisticsFields {
    #[serde(default, rename = "v")]
    value: Option<StatisticName<Location>>,
    #[serde(default, rename = "u")]
    uncertainty: Option<StatisticName<Convention>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyFields {
    #[serde(default, rename = "v")]
    value: Option<Value>,
    #[serde(default)]
    readings: Option<Vec<f64>>,
    #[serde(default, rename = "u")]
    uncertainty: Option<f64>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    statistics: Option<OneSpelling<StatisticsFields>>,
    #[serde(default)]
    computed: Option<Computed>,
    #[serde(default)]
    fingerprint: Option<WrittenFingerprint>,
}

/// A mapping read in the one spelling of a channel.
///
/// `value:` and `uncertainty:` are not read, and a file holding them is
/// refused — but not as any unknown field is. Serde would list what is
/// accepted, where `uncertainty` is four edits from `u` and no nearest name
/// reaches it; the key is named with what it is written now instead.
struct OneSpelling<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneSpelling<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Mapping<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Mapping<T> {
            type Value = OneSpelling<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a mapping")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<OneSpelling<T>, A::Error> {
                T::deserialize(MapAccessDeserializer::new(RetiredKeys(map))).map(OneSpelling)
            }
        }
        d.deserialize_map(Mapping(std::marker::PhantomData))
    }
}

/// The key a channel was once written with, refused naming the one it is
/// written with now.
fn retired_spelling<E: de::Error>(key: &str) -> Result<(), E> {
    let now = match key {
        "value" => "v",
        "uncertainty" => "u",
        _ => return Ok(()),
    };
    Err(E::custom(format!(
        "'{key}' is written '{now}' since schema 1: rename it"
    )))
}

/// A map whose every key is checked for a retired spelling before the shape
/// reading it sees it.
struct RetiredKeys<A>(A);

impl<'de, A: MapAccess<'de>> MapAccess<'de> for RetiredKeys<A> {
    type Error = A::Error;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(KeySeed(seed))
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, A::Error> {
        self.0.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct KeySeed<K>(K);

impl<'de, K: de::DeserializeSeed<'de>> de::DeserializeSeed<'de> for KeySeed<K> {
    type Value = K::Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<K::Value, D::Error> {
        self.0.deserialize(KeyDeserializer(d))
    }
}

struct KeyDeserializer<D>(D);

impl<'de, D: serde::Deserializer<'de>> serde::Deserializer<'de> for KeyDeserializer<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_any(KeyVisitor(visitor))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_identifier(KeyVisitor(visitor))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum ignored_any
    }
}

struct KeyVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for KeyVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<V::Value, E> {
        retired_spelling(key)?;
        self.0.visit_str(key)
    }

    fn visit_borrowed_str<E: de::Error>(self, key: &'de str) -> Result<V::Value, E> {
        retired_spelling(key)?;
        self.0.visit_borrowed_str(key)
    }

    fn visit_string<E: de::Error>(self, key: String) -> Result<V::Value, E> {
        retired_spelling(&key)?;
        self.0.visit_string(key)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<V::Value, E> {
        self.0.visit_bool(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<V::Value, E> {
        self.0.visit_i64(v)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<V::Value, E> {
        self.0.visit_u64(v)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<V::Value, E> {
        self.0.visit_f64(v)
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<V::Value, E> {
        self.0.visit_bytes(v)
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_unit()
    }
}

/// A digest, marked edited or not, or a failure in its place.
#[derive(Deserialize)]
#[serde(untagged)]
enum WrittenFingerprint {
    Failed(FailedMark),
    Digest(Fingerprint),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailedMark {
    failed: String,
}

impl From<PropertyFields> for PropertySchema {
    fn from(f: PropertyFields) -> PropertySchema {
        let (fingerprint, failure) = match f.fingerprint {
            Some(WrittenFingerprint::Failed(FailedMark { failed })) => (None, Some(failed)),
            Some(WrittenFingerprint::Digest(digest)) => (Some(digest), None),
            None => (None, None),
        };
        PropertySchema {
            statistics: f.statistics.map(|OneSpelling(s)| Statistics {
                value: s.value.map(|name| name.0),
                uncertainty: s.uncertainty.map(|name| name.0),
            }),
            value: f.value,
            readings: f.readings,
            uncertainty: f.uncertainty,
            unit: f.unit,
            symbol: f.symbol,
            computed: f.computed,
            fingerprint,
            failure,
        }
    }
}

impl<'de> Deserialize<'de> for PropertySchema {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<PropertySchema, D::Error> {
        d.deserialize_any(PropertyVisitor)
    }
}

struct PropertyVisitor;

impl<'de> Visitor<'de> for PropertyVisitor {
    type Value = PropertySchema;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value, or a mapping of the fields a quantity carries")
    }

    /// A mapping goes through the derived shape, so an unknown field is refused
    /// there with serde's own message rather than a reconstruction — but for
    /// the two long spellings of a channel, which are named with what they are
    /// written now.
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<PropertySchema, A::Error> {
        PropertyFields::deserialize(MapAccessDeserializer::new(RetiredKeys(map)))
            .map(PropertySchema::from)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<PropertySchema, E> {
        Ok(scalar(Value::boolean(v)))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<PropertySchema, E> {
        Ok(scalar(Value::integer(v)))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<PropertySchema, E> {
        i64::try_from(v)
            .map(|integer| scalar(Value::integer(integer)))
            .map_err(|_| E::custom(format!("{v} is too large to be an integer here")))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<PropertySchema, E> {
        Value::number(v).map(scalar).map_err(de::Error::custom)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<PropertySchema, E> {
        recognize_text(v).map(scalar).map_err(E::custom)
    }

    fn visit_unit<E: de::Error>(self) -> Result<PropertySchema, E> {
        Ok(PropertySchema::default())
    }
}

fn scalar(value: Value) -> PropertySchema {
    PropertySchema {
        statistics: None,
        value: Some(value),
        ..PropertySchema::default()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSchema {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(deserialize_with = "one_or_more")]
    pub index: Vec<Identifier>,
    pub columns: IndexMap<Identifier, ColumnSchema>,
    #[serde(default)]
    pub rows: Vec<IndexMap<Identifier, PropertySchema>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, from = "Option<ColumnFields>")]
pub struct ColumnSchema {
    pub unit: Option<String>,
    pub symbol: Option<String>,
    /// Which statistic of a cell's readings stands for each channel, said once
    /// for every cell of the column.
    pub statistics: Option<Statistics>,
}

/// A column that says nothing is written `{}`; reading a bare key as nothing is
/// the same tolerance a parser owes a hand-edited file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnFields {
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    statistics: Option<OneSpelling<StatisticsFields>>,
}

impl From<Option<ColumnFields>> for ColumnSchema {
    fn from(fields: Option<ColumnFields>) -> ColumnSchema {
        match fields {
            Some(f) => ColumnSchema {
                unit: f.unit,
                symbol: f.symbol,
                statistics: f.statistics.map(|OneSpelling(s)| Statistics {
                    value: s.value.map(|name| name.0),
                    uncertainty: s.uncertainty.map(|name| name.0),
                }),
            },
            None => ColumnSchema::default(),
        }
    }
}

/// One specifier covering both numbers, or a pair separating them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrecisionSchema {
    Both(String),
    Split(String, String),
}

impl PrecisionSchema {
    /// The `formatting::Precision` this declares.
    ///
    /// **One conversion, in one place.** Both `[property.*]` and a profile's
    /// column turn a written specifier into a precision, and two copies of
    /// this match is how `.3f` eventually means two things. A specifier that
    /// reached a loaded schema was validated at load, so `None` is only
    /// reachable through a value built in memory.
    pub fn precision(&self) -> Option<Precision> {
        match self {
            PrecisionSchema::Both(spec) => Precision::both(spec).ok(),
            PrecisionSchema::Split(value, uncertainty) => Precision::split(value, uncertainty).ok(),
        }
    }
}

impl<'de> Deserialize<'de> for PrecisionSchema {
    /// A pair whose halves are equal is read as `Both`. Letting both values
    /// exist means `==` reports two equal precisions as different, which is how
    /// a migrated file first failed its own verification.
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PrecisionSchema, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Both(String),
            Split(String, String),
            Other(serde::de::IgnoredAny),
        }
        Ok(match Written::deserialize(deserializer)? {
            Written::Other(_) => {
                return Err(<D::Error as serde::de::Error>::custom(
                    "a precision is one specifier, like .3f, or a pair [value, uncertainty], \
                     like [.3f, .1e]",
                ));
            }
            Written::Both(spec) => PrecisionSchema::Both(spec),
            Written::Split(value, uncertainty) if value == uncertainty => {
                PrecisionSchema::Both(value)
            }
            Written::Split(value, uncertainty) => PrecisionSchema::Split(value, uncertainty),
        })
    }
}

/// `index: temperature` and `index: [temperature, dextrin]` are the same
/// field at two lengths, not two shapes.
fn one_or_more<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Identifier>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMore {
        One(Identifier),
        More(Vec<Identifier>),
    }
    Ok(match OneOrMore::deserialize(deserializer)? {
        OneOrMore::One(one) => vec![one],
        OneOrMore::More(more) => more,
    })
}

// -------------------------------------------------------------- conversion

/// Convert one quantity, which is what `from_sample` does repeatedly and what
/// `fingerprint` needs to hash one.
/// The versions a build reads, as a reader says them: `schema_version 1`, not
/// `1..=1`.
pub fn readable_versions(range: &std::ops::RangeInclusive<u32>) -> String {
    if range.start() == range.end() {
        format!("schema_version {}", range.start())
    } else {
        format!("schema_version {} to {}", range.start(), range.end())
    }
}

/// What a model declared as standing for each channel, for the file to state.
/// Read from the property itself: `Records` holds digests, and a statistic's
/// name is not one.
fn statistics_of(property: &Property) -> Option<Statistics> {
    // What the file already states, else what the model declared — so a save
    // states it and a round trip keeps it. The *judging* side reads the records,
    // never this.
    let declared = match property.records().statistics {
        Some(stated) => Statistics {
            value: stated.value,
            uncertainty: stated.uncertainty,
        },
        None => Statistics {
            value: property.declared_location(),
            uncertainty: property.declared_convention(),
        },
    };
    (declared.value.is_some() || declared.uncertainty.is_some()).then_some(declared)
}

pub fn property_from(property: &Property) -> Result<PropertySchema, SchemaError> {
    if !property.is_resolved() {
        // A saved sample holds no promises — but a formula that has *run* is
        // not one. `fingerprint::record` converts a resolved computed property
        // before materialize erases the formula, and refusing on `is_computed`
        // would have made that order impossible.
        return Err(SchemaError::UnresolvedValue {
            property: Identifier::new("_").expect("a placeholder is a name"),
        });
    }
    let value = property
        .value()
        .map_err(|error| SchemaError::MalformedTable {
            table: Identifier::new("_").expect("a placeholder is a name"),
            reason: error.to_string(),
        })?;
    let presentation = property.presentation();
    let records = property.records();
    Ok(PropertySchema {
        // Not applicable is written, as the answer it is.
        value: (!matches!(value, Value::Absent)).then_some(value),
        readings: property.readings().map(|r| r.as_slice().to_vec()),
        // A failed uncertainty is an error, never an absence: discarding it
        // writes a quantity with no uncertainty and no complaint.
        uncertainty: property
            .uncertainty()
            .map_err(|error| SchemaError::Uncertainty {
                property: Identifier::new("_").expect("a placeholder is a name"),
                reason: error.to_string(),
            })?
            .map(|u| u.magnitude()),
        unit: presentation.unit.clone(),
        symbol: presentation.symbol.clone(),
        statistics: statistics_of(property),
        computed: records.computed.as_ref().map(|inputs| {
            keyed(
                records.produced,
                inputs
                    .iter()
                    .map(|(name, record)| (name.clone(), record_from(record)))
                    .collect::<Inputs>(),
                &records.channel_only,
            )
        }),
        fingerprint: records.fingerprint.clone(),
        // A failure the file recorded is carried as the file wrote it : a save
        // that changed another property dropped it, and the failed value was
        // then written with no verdict at all.
        failure: records.failure.clone(),
    })
}

/// A quantity as it stands, running nothing: a value or an uncertainty only a
/// formula could give is written absent. What a save that runs no formula
/// writes, and what a record hashes.
pub fn property_as_is(property: &Property) -> PropertySchema {
    let presentation = property.presentation();
    let records = property.records();
    // A formula's failure is the cache's to say; a file read without its model
    // carries the one it recorded.
    let failure = if property.is_computed() || property.has_uncertainty_formula() {
        property.peek_failure()
    } else {
        records.failure.clone()
    };
    // **Only the exception's type reaches the file.** A message carries whole
    // sentences and absolute paths — one real sample held the owner's own
    // directory sixteen times over, in a file that is data and is versioned —
    // and a file that says `FileNotFoundError` says what a reader needs without
    // carrying somebody's machine. The whole traceback goes to the project's
    // failure log, which `explain` reads.
    let failure = failure.as_deref().map(failure_kind);
    // A failure keeps the value its formula last gave, and that value's record.
    let last = failure.as_ref().and(property.last_good());
    PropertySchema {
        value: property
            .peek_value()
            .filter(|value| !matches!(value, Value::Absent))
            .or_else(|| last.map(|last| last.value.clone())),
        readings: property.readings().map(|r| r.as_slice().to_vec()),
        uncertainty: property
            .peek_uncertainty()
            .flatten()
            .or_else(|| last.and_then(|last| last.uncertainty))
            .map(|uncertainty| uncertainty.magnitude()),
        unit: presentation.unit.clone(),
        symbol: presentation.symbol.clone(),
        statistics: statistics_of(property),
        computed: records
            .computed
            .as_ref()
            .or_else(|| last.and_then(|last| last.records.computed.as_ref()))
            .map(|inputs| {
                keyed(
                    records.produced,
                    inputs
                        .iter()
                        .map(|(name, record)| (name.clone(), record_from(record)))
                        .collect::<Inputs>(),
                    &records.channel_only,
                )
            }),
        fingerprint: if failure.is_some() {
            None
        } else {
            records.fingerprint.clone()
        },
        failure,
    }
}

/// An exception's type, from the line a formula's failure gives.
///
/// `"ZeroDivisionError: division by zero"` is `ZeroDivisionError`. Anything that
/// does not read as a type — the core's own messages, which are sentences — is
/// kept whole: they are short, and cutting them would leave nothing.
fn failure_kind(message: &str) -> String {
    let Some((head, _)) = message.split_once(": ") else {
        return message.to_string();
    };
    let looks_like_a_type = !head.is_empty()
        && head.len() <= 64
        && !head.contains(char::is_whitespace)
        && head.starts_with(char::is_uppercase)
        && head
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '_');
    if looks_like_a_type {
        head.to_string()
    } else {
        message.to_string()
    }
}

fn record_from(record: &InputRecord) -> Recorded {
    match record {
        InputRecord::Digest(digest) if digest.is_edited() => Recorded::Edited(digest.clone()),
        InputRecord::Digest(digest) => Recorded::Value(Value::text(digest.as_str())),
        InputRecord::Literal(value) => Recorded::Value(value.clone()),
    }
}

/// Named so that a failure says which property, which `property_from` alone
/// cannot: a quantity does not know its own name.
fn named_property_from(
    name: &Identifier,
    property: &Property,
) -> Result<PropertySchema, SchemaError> {
    property_from(property).map_err(|error| match error {
        SchemaError::UnresolvedValue { .. } => SchemaError::UnresolvedValue {
            property: name.clone(),
        },
        SchemaError::Uncertainty { reason, .. } => SchemaError::Uncertainty {
            property: name.clone(),
            reason,
        },
        other => named_malformed(name, other),
    })
}

pub fn from_sample(sample: &Sample) -> Result<SampleSchema, SchemaError> {
    schema_of(sample, false)
}

/// The sample as it stands, running nothing: every value a formula has not
/// produced is absent, and every record is the one the value carries.
pub fn from_sample_as_is(sample: &Sample) -> Result<SampleSchema, SchemaError> {
    schema_of(sample, true)
}

fn schema_of(sample: &Sample, as_is: bool) -> Result<SampleSchema, SchemaError> {
    let mut properties = IndexMap::new();
    for name in sample
        .property_names()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
    {
        let handle = sample.property(&name).map_err(SchemaError::Sample)?;
        let shape = if as_is {
            handle.peek(property_as_is)
        } else {
            handle.with(|property| named_property_from(&name, property))?
        };
        properties.insert(name, shape);
    }

    let mut tables = IndexMap::new();
    for name in sample
        .table_names()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
    {
        let table = sample.table(&name).map_err(SchemaError::Sample)?;
        tables.insert(name.clone(), table_from(&name, table, as_is)?);
    }

    Ok(SampleSchema {
        schema_version: version(),
        name: sample.written_name().map(str::to_string),
        tags: sample
            .tags()
            .iter()
            .cloned()
            .map(Tag::Usable)
            .chain(sample.unusable_tags().iter().cloned().map(Tag::Unusable))
            .collect(),
        attributes: sample
            .attribute_names()
            .into_iter()
            .map(|name| {
                let value = sample.attribute(name).expect("just enumerated").clone();
                (name.clone(), value)
            })
            .collect(),
        properties,
        tables,
    })
}

fn table_from(name: &Identifier, table: &Table, as_is: bool) -> Result<TableSchema, SchemaError> {
    let columns = table
        .column_names()
        .into_iter()
        .map(|column| {
            let view = table.column(column).expect("just enumerated");
            let presentation = view.presentation();
            let declared = view.statistics();
            (
                column.clone(),
                ColumnSchema {
                    unit: presentation.unit.clone(),
                    symbol: presentation.symbol.clone(),
                    statistics: (!declared.is_empty()).then_some(Statistics {
                        value: declared.value,
                        uncertainty: declared.uncertainty,
                    }),
                },
            )
        })
        .collect();

    let column_names: Vec<Identifier> = table.column_names().into_iter().cloned().collect();
    let mut rows = Vec::new();
    for row in table.rows() {
        let mut cells = IndexMap::new();
        for column in &column_names {
            let cell = row
                .cell(column)
                .map_err(|error| SchemaError::MalformedTable {
                    table: name.clone(),
                    reason: error.to_string(),
                })?;
            let mut shape = if as_is {
                property_as_is(cell)
            } else {
                named_property_from(column, cell)?
            };
            // Its column states them for every cell: repeated on each, they
            // would be one declaration written forty times over.
            shape.statistics = None;
            cells.insert(column.clone(), shape);
        }
        rows.push(cells);
    }

    Ok(TableSchema {
        title: table.title().map(str::to_string),
        index: table.index_columns().to_vec(),
        columns,
        rows,
    })
}

/// A table as it stands, its values and records and no formula: what a formula
/// reads of another table while its own resolves.
pub fn table_copy(table: &Table) -> Result<Table, SchemaError> {
    let name = table.name().clone();
    table_into(&name, table_from(&name, table, true)?, &[], &[])
}

pub fn into_sample(schema: SampleSchema) -> Result<Sample, SchemaError> {
    check_version(schema.schema_version)?;
    let mut sample = Sample::new();
    sample.set_name(schema.name);
    // A tag that is no identifier is kept aside, reported, and written back.
    let mut usable = Vec::new();
    let mut unusable = Vec::new();
    for tag in schema.tags {
        match tag {
            Tag::Usable(tag) => usable.push(tag),
            Tag::Unusable(text) => unusable.push(text),
        }
    }
    sample.set_tags(usable);
    sample.set_unusable_tags(unusable);

    // Attributes first: a record resolves against the namespace, so it has to
    // exist before a property that names one is built.
    for (name, value) in schema.attributes {
        sample
            .set_attribute(name, value)
            .map_err(SchemaError::Sample)?;
    }
    let attributes: Vec<Identifier> = sample.attribute_names().into_iter().cloned().collect();
    let list_attributes: Vec<Identifier> = attributes
        .iter()
        .filter(|name| {
            sample
                .attribute(name)
                .is_ok_and(|value| value.as_list().is_some())
        })
        .cloned()
        .collect();

    for (name, shape) in schema.properties {
        let property = property_into(shape, &attributes, &list_attributes)
            .map_err(|error| named_malformed(&name, error))?;
        sample
            .set_property(name, property)
            .map_err(SchemaError::Sample)?;
    }

    for (name, shape) in schema.tables {
        match table_into(&name, shape, &attributes, &list_attributes) {
            Ok(table) => sample.set_table(name, table).map_err(SchemaError::Sample)?,
            // A repeated index sets the table aside, and the rest is read.
            Err(SchemaError::DuplicateIndex { table, reason }) => {
                sample.set_aside_table(table, reason)
            }
            Err(error) => return Err(error),
        }
    }
    Ok(sample)
}

/// A property's error, which does not know the property's name, named.
fn named_malformed(name: &Identifier, error: SchemaError) -> SchemaError {
    match error {
        SchemaError::MalformedTable { table, reason } if table.as_str() == "_" => {
            SchemaError::MalformedProperty {
                property: name.clone(),
                reason,
            }
        }
        other => other,
    }
}

fn property_into(
    shape: PropertySchema,
    attributes: &[Identifier],
    list_attributes: &[Identifier],
) -> Result<Property, SchemaError> {
    let malformed = |reason: String| SchemaError::MalformedTable {
        table: Identifier::new("_").expect("a placeholder is a name"),
        reason,
    };
    // A file records no convention: the readings sit beside the value, and a
    // reader with them can compute any of the three.
    //
    // The file's `value` beside them is the value, and the readings give its
    // statistics; neither replaces the other. A value that is no statistic of
    // them is what `validate` reports, and a model that declares a statistic
    // computes it instead.
    let mut property = match shape.readings {
        Some(readings) => {
            let mut measured = Property::measured(
                Readings::new(readings).map_err(|e| malformed(e.to_string()))?,
                None,
            );
            measured.set_written_value(shape.value);
            measured
        }
        None => Property::stored(shape.value.unwrap_or_else(Value::absent)),
    };
    if let Some(magnitude) = shape.uncertainty {
        property.set_uncertainty(Some(
            Uncertainty::new(magnitude).map_err(|e| malformed(e.to_string()))?,
        ));
    }
    // A file states which statistic stood for each channel, so a reader with no
    // model knows what the numbers were meant to be. Declared after the
    // uncertainty, which would otherwise overwrite the convention with the
    // number the file holds.
    if let Some(statistics) = shape.statistics {
        property.declare_statistics(statistics.value, statistics.uncertainty);
    }
    // Held as the file's own testimony as well, which is what `validation`
    // judges against: a convention a model declares in a session says what a
    // number should be, and only a file saying so replaces the model.
    let stated = shape.statistics.map(|statistics| DeclaredStatistics {
        value: statistics.value,
        uncertainty: statistics.uncertainty,
    });
    property.set_presentation(Presentation {
        unit: shape.unit,
        symbol: shape.symbol,
        // Declared in the project and nowhere else.
        precision: None,
    });
    property.set_records(Records {
        statistics: stated,
        failure: shape.failure,
        fingerprint: shape.fingerprint,
        // Read back from the record's own shape.
        produced: shape.computed.as_ref().and_then(channel_of_record),
        channel_only: shape
            .computed
            .as_ref()
            .map(channels_of_record)
            .unwrap_or_default(),
        computed: shape.computed.map(|computed| {
            computed
                .inputs()
                .into_iter()
                .map(|(name, raw)| {
                    let record = record_into(&name, raw, attributes, list_attributes);
                    (name, record)
                })
                .collect()
        }),
    });
    Ok(property)
}

/// A digest a record holds, whatever a reader made of it unquoted.
fn digest_text(raw: Value) -> String {
    match raw {
        Value::Text(text) => text,
        Value::Integer(number) => u64::try_from(number)
            .map(crate::core::property::digits)
            .unwrap_or_else(|_| number.to_string()),
        Value::Number(number) => number.to_string(),
        other => format!("{other:?}"),
    }
}

/// **The name decides, never the shape.** An attribute input records its value;
/// everything else records a digest. Guessing from the text would break on an
/// attribute whose value is literally a digest, which a sample identifier can
/// be.
fn record_into(
    name: &InputName,
    raw: Recorded,
    attributes: &[Identifier],
    list_attributes: &[Identifier],
) -> InputRecord {
    let raw = match raw {
        Recorded::Edited(digest) => return InputRecord::Digest(digest),
        Recorded::Value(value) => value,
    };
    let is_attribute = match name {
        InputName::Named(name) => attributes.contains(name),
        InputName::Cell(_) | InputName::Column { .. } => false,
    };
    let is_list = match name {
        InputName::Named(name) => list_attributes.contains(name),
        InputName::Cell(_) | InputName::Column { .. } => false,
    };
    if is_list {
        InputRecord::Digest(Fingerprint::new(digest_text(raw)))
    } else if is_attribute {
        InputRecord::Literal(raw)
    } else {
        InputRecord::Digest(Fingerprint::new(digest_text(raw)))
    }
}

fn table_into(
    name: &Identifier,
    shape: TableSchema,
    attributes: &[Identifier],
    list_attributes: &[Identifier],
) -> Result<Table, SchemaError> {
    let malformed = |reason: String| SchemaError::MalformedTable {
        table: name.clone(),
        reason,
    };
    let columns: IndexMap<Identifier, ColumnMeta> = shape
        .columns
        .into_iter()
        .map(|(column, meta)| {
            Ok((
                column,
                ColumnMeta {
                    presentation: Presentation {
                        unit: meta.unit,
                        symbol: meta.symbol,
                        precision: None,
                    },
                    statistics: meta
                        .statistics
                        .map(|statistics| DeclaredStatistics {
                            value: statistics.value,
                            uncertainty: statistics.uncertainty,
                        })
                        .unwrap_or_default(),
                },
            ))
        })
        .collect::<Result<_, SchemaError>>()?;

    // A file holds no formulas, so a loaded table declares no derivations.
    let mut table = Table::new(name.clone(), shape.index, columns, Vec::new())
        .map_err(|error| malformed(error.to_string()))?;
    if let Some(title) = shape.title {
        table.set_title(Some(title));
    }
    for cells in shape.rows {
        let mut built = Vec::new();
        for (column, cell) in cells {
            let property =
                property_into(cell, attributes, list_attributes).map_err(|error| match error {
                    SchemaError::MalformedTable { table, reason } if table.as_str() == "_" => {
                        malformed(format!("{column}: {reason}"))
                    }
                    other => other,
                })?;
            built.push((column, property));
        }
        table.add_row(built).map_err(|error| match &error {
            crate::core::table::TableError::DuplicateIndex { .. } => SchemaError::DuplicateIndex {
                table: name.clone(),
                reason: error.to_string(),
            },
            _ => malformed(error.to_string()),
        })?;
    }
    Ok(table)
}

/// Reading a cell back out, for callers that hold a schema rather than a table.
pub fn cell_of<'a>(
    table: &'a TableSchema,
    row: &RowAddress,
    column: &Identifier,
) -> Option<&'a PropertySchema> {
    match row {
        RowAddress::Ordinal(at) => table.rows.get(*at)?.get(column),
        RowAddress::FromEnd(back) => table
            .rows
            .get(table.rows.len().checked_sub(*back)?)?
            .get(column),
        RowAddress::Index(_) => None,
    }
}

// ------------------------------------------------------------------ errors

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    UnsupportedVersion {
        found: u32,
        supported: RangeInclusive<u32>,
    },
    UnknownField {
        field: String,
        context: String,
    },
    UnresolvedValue {
        property: Identifier,
    },
    Uncertainty {
        property: Identifier,
        reason: String,
    },
    MalformedTable {
        table: Identifier,
        reason: String,
    },
    InvalidName {
        name: String,
        source: IdentifierError,
    },
    MissingVersion,
    /// A table whose index repeats: set aside, and the rest read.
    DuplicateIndex {
        table: Identifier,
        reason: String,
    },
    Sample(SampleError),
    Format(FormatError),
    /// A property a file writes that cannot be one: a negative uncertainty.
    MalformedProperty {
        property: Identifier,
        reason: String,
    },
}

impl From<FormatError> for SchemaError {
    fn from(error: FormatError) -> SchemaError {
        SchemaError::Format(error)
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::UnsupportedVersion { found, supported } if found > supported.end() => {
                write!(
                    f,
                    "this file says schema_version {found}, written by a newer SampleKit; \
                     this build reads {}: upgrade SampleKit",
                    readable_versions(supported)
                )
            }
            SchemaError::UnsupportedVersion { found, supported } => write!(
                f,
                "this file says schema_version {found}, and this build reads {}: \
                 no command converts it, and the upgrading guide gives the procedure",
                readable_versions(supported)
            ),
            SchemaError::UnknownField { field, context } => write!(
                f,
                "'{field}' is not a field of {context}: an unknown field is refused \
                 rather than dropped, so a round trip through an older build cannot \
                 delete what a newer one wrote"
            ),
            SchemaError::UnresolvedValue { property } => write!(
                f,
                "'{property}' is still a promise and cannot be saved: materialise the \
                 sample first, so that the file holds a value"
            ),
            SchemaError::Uncertainty { property, reason } => write!(
                f,
                "'{property}' has an uncertainty formula that ran and did not \
                 yield an uncertainty: {reason}"
            ),
            SchemaError::MalformedTable { table, reason } => {
                write!(
                    f,
                    "table '{table}' does not match its own declaration: {reason}"
                )
            }
            SchemaError::MalformedProperty { property, reason } => {
                write!(f, "'{property}': {reason}")
            }
            SchemaError::InvalidName { name, source } => write!(f, "'{name}': {source}"),
            SchemaError::DuplicateIndex { table, reason } => write!(f, "table {table}: {reason}"),
            SchemaError::MissingVersion => write!(
                f,
                "no schema_version: a sample must declare which shape it is. A \
                 hand-written file needs 'schema_version: 1'"
            ),
            SchemaError::Sample(error) => write!(f, "{error}"),
            SchemaError::Format(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SchemaError {}
