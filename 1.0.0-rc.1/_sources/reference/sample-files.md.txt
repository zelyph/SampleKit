# Sample files

The format of a sample file, key by key.

## The file

A sample is one Markdown file, `.md`, `.MD` or `.markdown`:

```text
---
schema_version: 1
name: citra-ipa
tags: [dry_hopped]
style: ipa
hops: [Citra, Mosaic]
brewed: 2026-01-24
properties:
  og: {v: 1.064, u: 0.001}
  volume: {v: 20.5, unit: L}
tables:
  fermentation:
    index: day
    columns:
      day: {unit: d}
      gravity: {}
    rows:
      - {day: 1, gravity: 1.041}
      - {day: 2, gravity: 1.029}
---
# Citra IPA

The note: free Markdown.
```

| Part | What it is |
| --- | --- |
| frontmatter | The YAML between the first two `---` lines: the sample's data |
| note | Everything after the second `---`. Never read as data, never changed |

## Top-level keys

| Key | Holds | Written |
| --- | --- | --- |
| `schema_version` | The format's version: `1`. A file without it is read as version 1 | Always |
| `name` | The sample's name; without it, the file name without its extension | When set |
| `tags` | A list of tags | When not empty |
| any other key | An attribute | When set |
| `properties` | The quantities, by name | When not empty |
| `tables` | The tables, by name | When not empty |

Keys are written in this order: `schema_version`, `name`, `tags`, the
attributes in their order, `properties`, `tables`.

A version this build does not support is refused, naming both versions. An
unknown key inside a property, a table or a column is refused.

## Names

A name — of a property, a table, a column, an attribute or a tag — starts with
a letter or `_` and continues with letters, digits and `_`. Letters of every
alphabet count: `température`, `bière`. A hyphen, a dot or a space is not
allowed, nor a digit first.

- A property, a table and an attribute share one set of names: one name is
  one of them.
- A property, table or attribute name cannot begin with `_`.
- Reserved, and refused as a property, table or attribute name: `name`,
  `tags`, `path`, `filename`, `readings`, `project`, `state`.

## Scalars

| Written | Read as |
| --- | --- |
| `12.5`, `-3`, `1.0e-5` | A number |
| `true`, `false` | A boolean |
| `2026-03-14`, quoted or not | A date |
| `2026-03-14T10:30`, `2026-03-14 10:30:15` | A date and time, written with `T` |
| `n/a` | Not applicable: the value does not apply to this sample |
| anything else | Text |

`yes`, `no` and a quoted number, `"12.5"`, are text.

## Attributes

An attribute is one scalar, or a list of scalars of one kind (numbers, dates,
text or booleans; integers and decimals mix, as do dates and date-times). It
has no unit, uncertainty or readings. A mapping is not an attribute: a
top-level key holding one is refused.

```yaml
style: ipa
brewed: 2026-01-24
hops: [Citra, Mosaic]
```

## Tags

A list of names, in the order written.

```yaml
tags: [medal, dry_hopped]
```

## Properties

A property is a quantity. With a value alone it is a bare scalar; otherwise
a mapping:

```yaml
properties:
  batch_size: 20
  og: {v: 1.064, u: 0.001}
  volume: {v: 20.5, unit: L}
  fg: n/a
```

| Key | Holds |
| --- | --- |
| `v` | The value: a scalar |
| `readings` | Repeated measurements of the quantity: a list of numbers |
| `u` | The standard uncertainty, in the value's unit: a number |
| `unit` | The unit, as written; never converted |
| `symbol` | The symbol, over the one `[property.*]` declares |
| `statistics` | Which statistic of the readings is the value, and which the uncertainty |
| `computed` | For a computed value: what it was computed from |
| `fingerprint` | A digest of the property's value, uncertainty and readings |

- Keys are written in this order.
- A file holds no precision: `precision` is refused, and declared in
  `.samplekitrc`'s `[property.*]`.
- `v` and `u` are the only spellings. `value` and `uncertainty` — in a
  property, a cell, `statistics` or `computed` — are refused, the file not
  read: `'value' is written 'v' since schema 1: rename it`.
- `n/a` has no uncertainty.
- Readings with neither `v` nor a statistic have no value.
- A `v` beside readings is the value. With a `computed` record naming
  `readings`, the statistic gave it; without one, it was written by hand.

### `statistics`

```yaml
og:
  v: 1.0636666666666668
  readings: [1.063, 1.065, 1.063]
  u: 6.666666666666673e-4
  statistics: {v: mean, u: standard_error}
  computed: {readings: 688071fe6cd4}
  fingerprint: 8cfd5936bf29
```

| Key | Statistics |
| --- | --- |
| `v` | `mean`, `median`, `minimum`, `maximum`, `first_quartile`, `third_quartile` |
| `u` | `standard_error`, `sample_stdev`, `population_stdev` |

### `computed`

A computed property records each input it was read from, with that input's
fingerprint at the time.

| Form | Means |
| --- | --- |
| `computed: {og: 72fc715330d5, fg: 78d6a47f1b63}` | Computed, value and uncertainty, from these inputs |
| `computed: {}` | Computed from no input |
| `computed: {v: {og: 72fc…}}` | Only the value computed, from these inputs |
| `computed: {u: {}}` | Only the uncertainty computed, from no input |
| `computed: {src: 1bfc…, u: {p: 9d6a…}}` | Inputs every formula read, then one channel's own |
| `computed: {readings: 688071fe6cd4}` | The statistic of the property's own readings |
| `computed: {style: ipa}` | An attribute input, recorded as its value |
| `computed: {og: {edited: 72fc715330d5}}` | An input whose value was written by hand over its formula |

Input names:

| Name | Input |
| --- | --- |
| `og` | A property, or an attribute |
| `fermentation.gravity` | A whole column of a table |
| `row.gravity` | In a table cell: the same row's cell of that column |
| `readings` | The property's own readings |

### `fingerprint`

Twelve hexadecimal digits, a digest of the property's value, uncertainty and
readings, not its unit. Written on a property that is computed or that
something is computed from. `fingerprint: {edited: …}` marks a value written
by hand over its formula.

## Tables

```yaml
tables:
  fermentation:
    title: "Gravity and temperature, day by day"
    index: day
    columns:
      day: {unit: d}
      gravity: {statistics: {v: mean, u: standard_error}}
      temperature: {unit: degC}
    rows:
      - day: 1
        gravity: {readings: [1.041, 1.042]}
        temperature: 20.7
      - day: 2
        gravity: 1.029
        temperature: 20.7
```

| Key | Holds |
| --- | --- |
| `title` | A title, as text |
| `index` | The column that tells rows apart, or a list of columns: `index: [day, temperature]` |
| `columns` | Each column, in order: `unit`, `symbol`, `statistics`, all optional; `{}` for none |
| `rows` | The rows, in order: each maps a column to a cell |

- A cell has the form of a property, without `statistics`: its column
  declares that once.
- Rows are written with their cells in column order, one line per cell.
- A table whose index repeats a value is set aside, reported by `validate`,
  and the sample is not written until it is repaired.

## Fields

How a filter, a column, a sort and Python name a part of a sample.

| Field | Means |
| --- | --- |
| `og` | The value of the property `og` |
| `og.v` | The same |
| `og.u` | Its uncertainty |
| `og.unit` | Its unit |
| `og.stats.mean` | A statistic of its readings: `count`, `mean`, `median`, `minimum`, `maximum`, `first_quartile`, `third_quartile`, `sample_stdev`, `population_stdev`, `standard_error` |
| `style` | An attribute |
| `hops[#0]` | The first item of a list attribute; `#-1` the last |
| `fermentation.gravity[3]` | The cell of column `gravity` at index `3` |
| `fermentation.gravity[3].u` | That cell's uncertainty |
| `process.duration["stage 2"]` | A cell at an index holding a space |
| `mash.gravity[2, 65]` | A cell of a table indexed by two columns |
| `fermentation.gravity[#3]` | The fourth row, by position |
| `name`, `tags` | The sample's name and tags |
| `path`, `filename` | Where the sample is stored |
| `project` | The name of the folder of the sample's project |
| `state` | How the sample stands: `failed`, `outdated`, `edited`, `waiting`, `never_computed`, `not_current`, `defective`, `current` |

## Writing

SampleKit writes a file only when asked: a command with `--write`, a change
confirmed in the TUI, `save()` in Python.

- The frontmatter is written in one fixed form: the key order above, numbers
  in one spelling, short mappings on one line, a mapping longer than 100
  characters on several.
- Comments in the frontmatter are not kept.
- The note is kept byte for byte.
- A file is written to a temporary file, then put in place. A file changed
  since it was read, or read-only, is not written. A write that changes
  nothing is not made.
- A file written by SampleKit 0.2 is not read, and no command converts it.
