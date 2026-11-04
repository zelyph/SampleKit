# Filters

`-f` (or `--filter`) keeps the samples an expression selects, on every
command that works on samples. A query saved in `.samplekitrc`, `--query
NAME`, is the same language. The examples are the tensile specimens of the
[home page](../index.md), in the last step of its example:

<!-- run: tensile/04-results -->
```console
$ samplekit -f 'material == steel || elongation > 30' -c material,elongation
name        material   elongation [%]
─────────────────────────────────────
copper-01   copper               44.0
steel-01    steel                24.0

2 of 3 samples
```

Quote the whole expression, as above: the shell would otherwise read `&&`,
`||`, `!`, `<` and `>` itself.

## Comparisons

| Expression | Selects the samples where |
| --- | --- |
| `elongation > 20` | `elongation` is above 20; also `>=`, `<`, `<=` |
| `material == steel` | `material` is `steel`; `!=` for the others |
| `material == "cast iron"` | a text holding a space, in quotes |
| `tensile_strength > yield_strength` | one field above another, in one unit |
| `grade contains "355"` | the text holds `355`, whatever the case; a number, quoted |
| `name starts_with steel` | the text starts with `steel`, whatever the case |
| `material in (steel, copper)` | `material` is one of them |
| `elongation is missing` | the sample has no `elongation`; `is present` for those that have one |

- **A bare word is text**, `material == steel`, unless it names a field:
  `tensile_strength > yield_strength` compares two fields, and after `<`,
  `<=`, `>` or `>=` a bare word is always a field. In quotes, a word is always
  text.
- **`12`, `1.050`, `true`, `2026-01-24` are a number, a boolean and a date**;
  quoted, `"12"` is text.
- **`==` and `!=` respect the case; `contains` and `starts_with` do not.** An
  ordered comparison of texts, `name > "d"`, follows the order `-s` sorts in.
- **A number compared with a text is refused**, never answered *nothing*:
  `elongation == high` says so. Two fields in different units are refused
  too: `width > area` compares millimetres with square millimetres.
- **A sample without the field is not selected**, even under `!`:
  `!(elongation > 20)` leaves out a specimen without `elongation`.
  `elongation is missing` asks for those.
- **`==` on a number compares it exactly**: a value shown `69.2` may hold
  `69.1759`, and the filter says so, with the range to write instead,
  `young_modulus >= 69.15 && young_modulus < 69.25`.

## Lists, tags and tables

| Expression | Selects the samples where |
| --- | --- |
| `tags has reference` | the sample carries the tag |
| `operators has ana` | the list attribute holds `ana` |
| `curve.strain has 24` | one row of that column holds 24 |
| `curve has 24` | one cell of the table holds 24 |
| `curve where (strain > 20 && stress < 450)` | one row meets both conditions |
| `curve.stress[10] > 500` | the cell of the row whose index is 10 |
| `links to plate-03` | an attribute holds a link to that sample |
| `cut_from links to plate-03` | that attribute holds a link to that sample |

`has` is membership: `tags contains ref` is refused, since it would also
match `reference`. In `where (…)`, a name that is a column of the table is
that row's cell, the same row for every name; another name is the sample's.
`links to` names the sample as its links write it: a wikilink's target,
`[[plate-03]]`, or a Markdown link's file name, `[P3](plate-03.md)`. A link
written with a sample's `name`, `[[Plate 3]]`, is found by that name,
`links to "Plate 3"`; a name no link writes selects nothing, and is not an
error. The TUI's *linked from* finds the samples a link leads to, however it
names them.

## How a sample stands

`state` holds the words `status` and `validate` say of a sample:

| Expression | Selects the samples |
| --- | --- |
| `state == outdated` | one of whose values is outdated |
| `state == not_current` | one of whose values is not current, for whatever reason |
| `state in (failed, never_computed)` | holding either |
| `state == edited` | one of whose computed values was written by hand |

## Combining

`&&` (and), `||` (or) and `!` (not), with parentheses: `&&` binds before
`||`, so `a && b || c` is `(a && b) || c`.

```text
material == steel && !(tags has reference)
(material == steel || material == copper) && elongation > 20
```

`and`, `or` and `not` are not keywords: a field may be named so, and
`material == steel and elongation > 20` is refused, naming `&&`. `-f` given twice, or with
`--query`, keeps the samples every one selects.

## Mistakes are named

A field no sample holds is an error that names the nearest one, never an
empty answer: `elongaton > 20` says *did you mean elongation?*. A text no
sample holds is a warning that suggests the nearest one, `material == steal`
*did you mean steel?*, beside an empty table. A tag no sample carries,
`tags has reference`, is an error, which points to `samplekit list tags`. In
the TUI, `/` filters, and `Tab` completes the fields, the operators and the
values.
