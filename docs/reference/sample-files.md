# Sample files

The format of a sample file, key by key.

## The file

A sample is one Markdown file, `.md`, `.MD` or `.markdown`:

```{code-block} sample
:caption: samples/steel-01.md

---
schema_version: 1
name: steel-01
tags: [reference]
material: steel
operators: [ana, tom]
tested: 2026-03-04
properties:
  width: {v: 12.513, u: 0.01, unit: mm}
  thickness: {v: 2.0, unit: mm}
tables:
  curve:
    index: strain
    columns:
      strain: {unit: "%"}
      force: {unit: kN}
    rows:
      - strain: 0
        force: 0.0
      - strain: 0.2
        force: 9.516
---
# steel-01

The note: free Markdown.
```

| Part | What it is |
| --- | --- |
| frontmatter | The YAML between the first two `---` lines: the sample's data |
| note | Everything after the second `---`. Never read as data; changed only when replaced, `set note=…` |

## Top-level keys

| Key | Holds | Written |
| --- | --- | --- |
| `schema_version` | The format's version: `1`. A Markdown file without it is no sample: it is left out, as a README is, and `validate` says how many were | Always |
| `name` | The sample's name; without it, the file name without its extension | When set |
| `tags` | A list of tags | When not empty |
| any other key | An attribute | When set |
| `properties` | The properties, by name | When not empty |
| `tables` | The tables, by name | When not empty |

Keys are written in this order: `schema_version`, `name`, `tags`, the
attributes in their order, `properties`, `tables`.

A version this build does not support is refused, naming both versions. An
unknown key inside a property, a table or a column is refused.

## Names

A name, of a property, a table, a column, an attribute or a tag, starts with
a letter (or `_`, for a tag alone) and continues with letters, digits and `_`. Letters of every
alphabet count: `température`, `épaisseur`. A hyphen, a dot or a space is not
allowed, nor a digit first.

- A property, a table and an attribute share one set of names: one name is
  one of them.
- A property, table, column or attribute name cannot begin with `_`: a
  leading `_` is kept for Python's private state.
- Reserved, and refused as a property, table or attribute name: `name`,
  `tags`, `path`, `filename`, `readings`, `project`, `state`, `note` (the
  text below the frontmatter: `set note=` and Python's `sample.note` write
  it).

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
material: steel
tested: 2026-03-04
operators: [ana, tom]
```

## Links

A text attribute may lead elsewhere: to another sample, to a file, or to a
web page. Two forms are read, the one Obsidian writes and the one Markdown
does:

```yaml
reference: "[[aluminium-01]]"
twin: "[[copper-01|the copper]]"
cut_from: "[plate P3](../plates/P3.md)"
certificate: "[certificate](../certificates/S355-0425.pdf)"
datasheet: "[datasheet](https://example.org/s355)"
standard: https://example.org/iso-6892
parents: ["[[aluminium-01]]", "[[copper-01]]"]
```

- **`[[NAME]]`** names a sample of the project: the one whose file is
  `NAME.md`, or, where no file is named so, the one whose `name` is NAME.
  After `|` comes what Obsidian shows in its place, and a heading after `#`
  is ignored.
- **`[text](path)`** names a file by its path from the sample's own folder,
  as Markdown reads a link: a sample, of this project or another, or any
  file, a certificate, a raw data file, a folder. A space in the path is
  written `%20`, or the path between `<` and `>`.
- **`https://…`**, alone or as `[text](https://…)`, names a web page.
- **A list of links** names several: the samples a specimen was made from.

**The quotes are needed**: unquoted, a link reads as a list, and the file is
refused at that line, with the quoted form to write. `set
reference=[[aluminium-01]]` writes them, and `set
parents=[[aluminium-01]],[[copper-01]]` writes a list of two; after `[[`, Tab
offers the project's samples.

A link is text a filter and an export read as written. **It is shown as
Markdown shows one**, in the terminal interface and in a table drawn in a
terminal: its text alone, `the copper` for `[[copper-01|the copper]]`,
underlined in the `link` colour; so does `view` in a terminal. A web page is
marked `↗`; a link leading nowhere is marked `✗`, in the error colour. Typed, a link is the text as
written; a table written to a file or a pipe keeps it so.

In the terminal interface, `Enter` or `o` on a link opens where it leads: a
sample on its screen, even one the filter leaves out or of another project,
a file in its application, a page in the browser; on a list, `Enter` opens
its items, and `Enter` or `o` follows the one under the cursor. A sample's
screen ends with the samples linking to it, *linked from*, each followed by
`Enter`. On the command line, the filter `links to NAME` selects the samples
whose links write NAME ([filters](filters.md)). From
Python, `sample.linked("reference")` reads the sample, a file that is no
sample gives its path, a `pathlib.Path`, and a list of links gives a list.
`validate` reports a link that leads to no sample or to several, and a path
where nothing is; a web page is not checked.

`samplekit FILE rename NEW` renames a sample and keeps its links: the file,
its own files under `[collection] files` (a folder named after it too), its
`name` where it was the file's name, and every link of the project leading to
it, `[[old]]` and `[text](old.md)` alike.

Obsidian makes only `[[…]]` clickable among a note's properties: there, a
Markdown link stays text. [Link samples](../guides/link-samples.md) walks
through them on the tensile example.

## Tags

A list of names, in the order written.

```yaml
tags: [reference, to_redo]
```

## Properties

A property holds a value, and its uncertainty, unit and readings where it has
them. With a value alone it is a bare scalar; otherwise
a mapping:

```yaml
properties:
  specimens_cut: 3
  width: {v: 12.513, u: 0.010, unit: mm}
  thickness: {v: 2.0, unit: mm}
  gauge_length: n/a
```

| Key | Holds |
| --- | --- |
| `v` | The value: a scalar |
| `readings` | Repeated measurements of the property: a list of numbers |
| `u` | The standard uncertainty, in the value's unit: a number |
| `unit` | The unit, as written; never converted |
| `symbol` | The symbol, over the one `[property.*]` declares |
| `statistics` | Which statistic of the readings is the value, and which the uncertainty |
| `computed` | For a computed value: what it was computed from |
| `fingerprint` | A digest of the property's value, uncertainty and readings |

- Keys are written in this order.
- A file holds no precision: `precision` is refused, and declared in
  `.samplekitrc`'s `[property.*]`.
- `v` and `u` are the only spellings. `value` and `uncertainty`, in a
  property, a cell, `statistics` or `computed`, are refused, the file not
  read: `validate` names the key, and the keys expected in its place.
- `n/a` has no uncertainty.
- Readings with neither `v` nor a statistic have no value.
- A `v` beside readings is the value. With a `computed` record naming
  `readings`, the statistic gave it; without one, it was written by hand.

### `statistics`

```yaml
thickness:
  v: 2.0023333333333335
  readings: [1.995, 2.004, 2.008]
  u: 0.003844187531556873
  unit: mm
  statistics: {v: mean, u: standard_error}
  computed: {readings: 3160016f6c6e}
  fingerprint: 1d88e7bb89d3
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
| `computed: {thickness: 2b1079ab857e, width: 25920047999d}` | Computed, value and uncertainty, from these inputs |
| `computed: {}` | Computed from no input |
| `computed: {v: {width: 2592…}}` | Only the value computed, from these inputs |
| `computed: {u: {}}` | Only the uncertainty computed, from no input |
| `computed: {width: 2592…, u: {area: 17b7…}}` | Inputs every formula read, then one channel's own |
| `computed: {readings: 3160016f6c6e}` | The statistic of the property's own readings |
| `computed: {material: steel}` | An attribute input, recorded as its value |
| `computed: {area: {edited: 17b7fdf04105}}` | An input whose value was written by hand over its formula |

Input names:

| Name | Input |
| --- | --- |
| `width` | A property, or an attribute |
| `curve.force` | A whole column of a table |
| `row.force` | In a table cell: the same row's cell of that column |
| `readings` | The property's own readings |

### `fingerprint`

Twelve hexadecimal digits, a digest of the property's value, uncertainty and
readings, not its unit. Written on a property that is computed or that
something is computed from. `fingerprint: {edited: …}` marks a value written
by hand over its formula, and `fingerprint: {failed: ValueError}` a value whose
formula raised that error when last run, the number kept the last it gave.
[Fingerprints](fingerprints.md) gives the recipe, to compute one again by
hand.

## Tables

```yaml
tables:
  curve:
    title: "The force read at each strain"
    index: strain
    columns:
      strain: {unit: "%"}
      force: {unit: kN, statistics: {v: mean, u: standard_error}}
      grips: {}
    rows:
      - strain: 0
        force: 0.0
        grips: wedge
      - strain: 0.2
        force: {readings: [9.51, 9.52]}
        grips: wedge
```

| Key | Holds |
| --- | --- |
| `title` | A title, as text |
| `index` | The column that tells rows apart, or a list of columns: `index: [strain, temperature]` |
| `columns` | Each column, in order: `unit`, `symbol`, `statistics`, all optional; `{}` for none |
| `rows` | The rows, in order: each maps a column to a cell |

- A cell has the form of a property: a number, with optionally an
  uncertainty, readings and a unit; or a text, a date, `true` or `false`, as an
  attribute is: `grips: wedge`. One table may hold both, a column of text
  beside a column of forces. Its column declares `statistics` once
  for every cell; a cell may write its own, which it keeps, and which wins
  over its column's.
- Rows are written with their cells in column order, one line per cell.
- A table whose index repeats a value is set aside, reported by `validate`,
  and the sample is not written until it is repaired.

## Fields

How a filter, a column, a sort and Python name a part of a sample.

| Field | Means |
| --- | --- |
| `width` | The value of the property `width` |
| `width.v` | The same |
| `width.u` | Its uncertainty |
| `width.unit` | Its unit |
| `width.stats.mean` | A statistic of its readings: `count`, `mean`, `median`, `minimum`, `maximum`, `first_quartile`, `third_quartile`, `sample_stdev`, `population_stdev`, `standard_error` |
| `material` | An attribute |
| `operators[#0]` | The first item of a list attribute, `#1` the second |
| `curve.force[0.2]` | The cell of column `force` at index `0.2` |
| `curve.force[0.2].u` | That cell's uncertainty |
| `heat.duration["stage 2"]` | A cell at an index holding a space |
| `creep.strain[100, 600]` | A cell of a table indexed by two columns |
| `curve.force has 9.516` | In a filter: a cell of the column, in any row, holds it |
| `curve has 9.516` | In a filter: any cell of the table holds it |
| `curve where (strain > 1 && stress < 300)` | In a filter: one row meets the condition, each name its cell in that row |
| `curve.force[#3]` | The fourth row, by position; `#-1` the last |
| `name`, `tags` | The sample's name and tags |
| `path`, `filename` | Where the sample is stored |
| `project` | The name of the folder of the sample's project |
| `state` | How the sample stands: `failed`, `outdated`, `edited`, `waiting`, `never_computed`, `not_current`, `defective`, `current` |

## Writing

SampleKit writes a file only when asked: a command's change answered `y`, or
written with `--write`; a change confirmed in the TUI; `save()` in Python.

- The frontmatter is written in one fixed form: the key order above, numbers
  in one spelling, a property carrying a `computed` record on several lines,
  one key a line, and any other property, as every cell of a table, on one
  line however long.
- Comments in the frontmatter are not kept.
- What SampleKit reads otherwise than it is written is changed by the next
  write: a key whose value is `null` is removed, a YAML tag (`!!python/…`)
  dropped, an uncertainty beside `n/a` dropped, and a date no calendar has
  (`2026-02-30`) kept as the text it was read as. `validate` notes each, and
  a command's preview says which this write changes.
- The note is kept byte for byte, unless you replace it, `set note=…`.
- A file is written to a temporary file, then put in place. A file changed
  since it was read, or read-only, is not written. A write that changes
  nothing is not made.
- A file written by SampleKit 0.2 is not read, and no command converts it.
