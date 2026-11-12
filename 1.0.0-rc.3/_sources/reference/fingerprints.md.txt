# Fingerprints

A sample file records, beside each computed value, what its inputs held when
it was computed: `computed: {width: 25920047999d}`. And it records, on the value
itself, what it held then: `fingerprint: 17b7fdf04105`. Both are
**fingerprints**: twelve hexadecimal digits that change whenever the numbers
they cover change.

Nothing in them is secret or SampleKit's own. This page gives the exact recipe,
so that any fingerprint of a file can be computed again by hand, with
`sha256sum` or three lines of Python, and checked without SampleKit.

## The recipe

A fingerprint is **the first twelve characters of the SHA-256 of a short
text**, written in lowercase hexadecimal. The text is the numbers of a
property, between braces:

```{code-block} console
:caption: a terminal

$ printf '%s' '{v: 1.05}' | sha256sum | cut -c1-12
760c72163408
```

`printf '%s'` writes the text with nothing after it: `echo` would add a
newline, and the newline would be hashed too. In Python:

```{code-block} python
:caption: Python

import hashlib

hashlib.sha256("{v: 1.05}".encode()).hexdigest()[:12]   # '760c72163408'
```

## The text that is hashed

```text
{v: VALUE, readings: [R1, R2, R3], u: UNCERTAINTY}
```

- **The fields a property has, in this order**: `v`, then `readings`, then
  `u`. A field the property does not have is left out with its comma: `{v:
  20.5, u: 0.2886751345948129}`, `{v: 1.05}`.
- **Separated by a comma and a space**, each written `name: number`, inside
  braces, on one line.
- **The numbers as the file spells them**: copy them from the file,
  `6.666666666666673e-4` included. One exception: a whole number is hashed
  as its decimal, `v: 65` as `{v: 65.0}`, so that writing `65.0` in its place
  changes nothing.
- **Nothing else**: not the unit, the symbol, the statistics, the records,
  nor the name. A unit corrected from `g` to `kg` without its number makes
  nothing outdated, and a value renamed keeps its fingerprint.

## Which text, for which record

| Record | Text hashed |
| --- | --- |
| `fingerprint` of a value nothing computes | the whole property: `{v, readings, u}`, those it has |
| An input in `computed: {…}` | the input whole, the same text |
| `fingerprint` of a computed value | only what its formula gives: `{v, u}` for `compute`, `{v}` for `compute_value`, `{u}` for `compute_uncertainty` |
| `fingerprint` of a value its readings give | `{v}`: the readings are recorded apart, and an uncertainty they give is theirs |
| `computed: {readings: …}` | the readings alone: `{readings: [R1, R2, R3]}` |
| An input read by one of its channels, `force.v` | that channel alone: `{v: 9.5}` |
| A whole column, `curve.strain` | each cell's text followed by a newline, in the order of the rows, hashed together |
| The same row's cell, `row.force` | the cell's text: `{v: 9.516}` |
| An attribute, `heat: 7` | not hashed: its value, written as it is |
| A list attribute | its list as the file writes it: `[ana, tom]` |
| A value written by hand over its formula | `{edited: …}`: the fingerprint of the value as it stands, marked |

A value's own `fingerprint` answers *has a hand changed what the formula
gave?*, which is why it covers only what the formula gave. The records in
`computed` answer *have the inputs moved?*, which is why they cover each input
whole.

## Checking a file by hand

`examples/tensile/04-results/samples/steel-01.md`, the steel specimen of the
[home page](../index.md), holds, among its properties:

```{code-block} sample
:caption: samples/steel-01.md

properties:
  width:
    v: 12.513333333333334
    readings: [12.494, 12.528, 12.518]
    u: 0.01008849730028156
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: b1b4d9b56ab1}
    fingerprint: f099c3bf0938
  area:
    v: 25.05586444444445
    u: 0.05217296159913354
    unit: mm²
    computed: {thickness: 2b1079ab857e, width: 25920047999d}
    fingerprint: 17b7fdf04105
  young_modulus:
    v: 206.33891963549178
    u: 1.0599559413170074
    unit: GPa
    computed: {curve.strain: ed9e87a6f01a, curve.stress: 82a1e45e269c}
    fingerprint: 1e199aaf2a11
  elongation:
    v: 24
    unit: "%"
    computed: {curve.strain: ed9e87a6f01a}
    fingerprint: d4a65cab1618
```

and, in its table, the curve's first row:

```{code-block} sample
:caption: samples/steel-01.md

      - strain: 0
        force: 0.0
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: 17b7fdf04105}, fingerprint: d3e1083d25fb}
```

Each digest, computed again:

```{code-block} console
:caption: a terminal

$ f() { printf '%s' "$1" | sha256sum | cut -c1-12; }
$ f '{readings: [12.494, 12.528, 12.518]}'
b1b4d9b56ab1
$ f '{v: 12.513333333333334}'
f099c3bf0938
$ f '{v: 12.513333333333334, readings: [12.494, 12.528, 12.518], u: 0.01008849730028156}'
25920047999d
$ f '{v: 25.05586444444445, u: 0.05217296159913354}'
17b7fdf04105
$ f '{v: 206.33891963549178, u: 1.0599559413170074}'
1e199aaf2a11
$ f '{v: 24.0}'
d4a65cab1618
$ f '{v: 0.0}'
d3e1083d25fb
```

- `width`'s readings give its value and its uncertainty: its `fingerprint`
  covers the value, and `computed: {readings: …}` the readings.
- `area` records `width` whole, `25920047999d`, which is not `width`'s own
  `fingerprint`: the area's formula read the value, the readings and the
  uncertainty. Its own `fingerprint` covers what its formula gave, the value
  and its uncertainty.
- `elongation`, a whole number in the file, `24`, is hashed as a decimal,
  `{v: 24.0}`.
- A cell of a table is a value of its own: the stress of the first row
  records the force of its row, `row.force`, and the area it read.

A whole column hashes its cells one per line. In Python, for the curve's
`strain`, thirteen rows, as `young_modulus` and `elongation` record it:

```{code-block} python
:caption: Python

import hashlib

strain = [0, 0.025, 0.05, 0.2, 0.3, 0.4, 0.6, 1, 2, 5, 10, 20, 24]
text = "".join(f"{{v: {float(s)}}}\n" for s in strain)
hashlib.sha256(text.encode()).hexdigest()[:12]   # 'ed9e87a6f01a'
```

## Attributes, channels and values written by hand

A specimen whose width is corrected by the offset of the callipers of the
people who measured it, `operators`, for the heat of steel it was cut from,
`heat`; its area computed from that corrected width; and its force given an
uncertainty of 0.5 % of its own value:

```{code-block} sample
:caption: s.md

heat: 7
operators: [ana, tom]
properties:
  width: {v: 12.5, unit: mm, fingerprint: 87c43f2ca298}
  corrected_width:
    v: 12.4
    unit: mm
    computed: {heat: 7, operators: d68eb8bfbb21, width: 87c43f2ca298}
    fingerprint: {edited: 0c1743bc5b20}
  area:
    v: 24.8
    unit: mm²
    computed: {corrected_width: {edited: 0c1743bc5b20}}
    fingerprint: 0f03d747f724
  force:
    v: 9.5
    u: 0.0475
    unit: kN
    computed: {u: {force.v: 20403669cede}}
    fingerprint: 699d0feb180e
```

- `heat` is an attribute: `corrected_width` records its value, `7`, which a
  reader compares by eye. `operators` is a list: `corrected_width` records
  `f '[ana, tom]'`, `d68eb8bfbb21`.
- `corrected_width` was written by hand over its formula, `samplekit s.md set
  corrected_width=12.4`: its `fingerprint` is `{edited: f '{v: 12.4}'}`, and
  `area`, computed from it, carries the mark too.
- `force`'s uncertainty is 0.5 % of its own value, so it reads `force.v`, the
  value alone: `f '{v: 9.5}'`, `20403669cede`. Its own `fingerprint` covers
  what its formula gives, the uncertainty: `f '{u: 0.0475}'`.

## What decides that a value is outdated

A computed value is **current** when each input, hashed as it stands now,
gives the fingerprint its record holds, and each computed input is current
too. An input whose fingerprint differs makes it *outdated*, and so does an
input that is itself outdated. A value whose own fingerprint no longer matches
its numbers was changed by hand: it is *edited*, and is kept.

SampleKit never compares two stored fingerprints: it always computes the
input's again. A file edited by hand changes the numbers and neither record,
and only computing again sees it. The `fingerprint` written on an input is
there for a reader: equal to what a value records means unchanged since.

## A formula's digest is something else

When a formula itself changes, its inputs have not moved, and no fingerprint
of the file can tell. SampleKit keeps, in the project's history
(`.samplekit/history`) and not in the files, a **digest of each formula**: the
SHA-256 of a description the model's Python takes of it, made of the keyword it
is given under (`compute`, `compute_value`, `compute_uncertainty`,
`compute_column`, a statistic), its unit, what a table's formula fills, the source of every function of
the model's own code that it reaches, without its comments or docstrings, and the
names it declares in `depends_on`. A formula whose digest changed makes its
values outdated, *formula changed*.

That digest is taken by running the model's code, and is not meant to be
computed by hand: it is what says *the code changed*, where the fingerprints
say *the numbers changed*. Checking a file's numbers never needs it.
