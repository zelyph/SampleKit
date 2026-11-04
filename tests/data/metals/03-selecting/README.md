# Step 3 · Selecting

In this step you have twelve pieces, a season's worth, and you choose among
them: a filter keeps some, columns show what you ask, a sort orders them. Then
you give the filters and tables you use often a name in `.samplekitrc`, and
write a table to a file.

The gamma zinc is still ageing: it has no final hardness yet.

Run every command from this folder, `03-selecting`.

## Filters, columns, sorting

```console
$ samplekit pieces -f 'style == titanium'
$ samplekit pieces -f 'style == nickel || radius > 1.060' -c name,style,radius
$ samplekit pieces -f 'notch is missing'
$ samplekit pieces -c style,radius -s -radius
$ samplekit pieces -c name,radius,notch --group style
$ samplekit pieces -c radius,notch --summary
```

- `-f` keeps the pieces that match. `==`, `!=`, `<`, `>`, `has` for tags and
  lists, `contains` for text, `is missing` and `is present`, combined with
  `&&` (and), `||` (or), `!` (not) and parentheses. Several `-f` must all
  hold.
- `-c` names the columns. A column can also say its digits and its title:
  `-c 'style,radius:.4f=Original hardness'`. Written to a CSV, TSV or JSON file
  with `-o`, the title names its columns: `radius=RADIUS` gives `RADIUS_value` and
  `RADIUS_uncertainty`. A title holding a comma is quoted inside:
  `-c "radius='Hardness, original'"`.
- `-s` sorts; a `-` before a field sorts from high to low.
- `--group` prints one table per value of a field.
- `--summary` counts, averages and spreads each column instead of listing the
  pieces.

The summary counts `11/12` final hardnesses, because one piece has none:

```text
     n       mean (1/u²)       s      sem   median     min     max
──────────────────────────────────────────────────────────────────
radius   12/12         1.055   0.010   0.0003    1.053   1.042   1.078
notch   11/12         1.011   0.004   0.0003    1.011   1.003   1.017
```

Every value has an uncertainty here, so the mean is weighted by 1/u²: a
precise value counts more, and the header says so. `s` is the spread of the
values, their sample standard deviation; `sem` is how well the mean is known,
its standard error — 1/√Σ(1/u²) beside a weighted mean, `s`/√n beside a plain
one.

## A mistake is said, never guessed

A filter that cannot be read, or that names something no piece has, stops with
an error that says where and suggests the nearest name. It never returns an
empty table that looks like an answer:

```text
$ samplekit pieces -f 'tags has prizes'
error: no sample carries the tag 'prizes'
  did you mean: 'prize'?
  run 'samplekit list tags' to see the tags in use
```

A comparison on a field a piece does not have is never true, even under `!`:
`radius > 1.060` and `!(radius > 1.060)` both leave out a piece without `radius`. Ask for
those with `radius is missing`.

## Queries, profiles and exports

What you ask often gets a name in `.samplekitrc`:

```toml
[query.titaniums]                 # which pieces
filter = 'style == titanium'

[profile.overview]           # which columns, in which order
columns = [
  { field = "name", label = "Piece" },
  { field = "style", label = "Style" },
  { field = "process", label = "Process" },
  { field = "radius", label = "Original hardness" },
  { field = "notch", label = "Final hardness" },
  { field = "volume", label = "Volume" },
]
sort = ["style", "-radius"]

[export.overview]            # a profile written to a file
profile = "overview"
format = "csv"
output = "out/overview.csv"
```

```console
$ samplekit pieces --query titaniums
$ samplekit pieces --profile overview
$ samplekit pieces --profile overview --query strong
$ samplekit pieces --profile preheat --csv
$ samplekit pieces export overview             # a preview
$ samplekit pieces export overview --write     # writes out/overview.csv
$ samplekit pieces list queries
```

The profile gives its columns their titles, and the unit moves to the header:

```text
Piece               Style      Process          Original hardness    Final hardness   Volume [L]
───────────────────────────────────────────────────────────────────────────────────────────
double-titanium         titanium        UT-05            1.078 ± 0.0010   1.014 ± 0.0010         19.0
alpha-titanium          titanium        UT-05            1.064 ± 0.0010   1.011 ± 0.0010         20.5
session-titanium        titanium        UT-05            1.042 ± 0.0010   1.009 ± 0.0010         20.0
beta-bronze          bronze_alloy   Vacuum          1.054 ± 0.0010   1.012 ± 0.0010         20.0
…
```

In a CSV file, a property is two columns, its value and its uncertainty, with
the unit in the header. Numbers are written with the digits you saw on
screen. An option given beside `--profile` replaces that part of it: `-c` the
columns, `-s` the order.

`export` previews first — how many rows, where the file goes, and which values
are not current — and writes with `--write`. The export is recorded in the
project's history, so that `samplekit out/overview.csv explain` can later say
where it came from.

## Why

A table in a paper should be made by a command you can run again, not by
copying cells. A named query, profile or export is that command, kept beside
the data it reads.

- To understand: [values, units and uncertainty](https://zelyph.github.io/SampleKit/latest/explanations/values-and-uncertainty.html),
  for the digits and the weighted mean.
- To look up: [the filter language, the options of every command](https://zelyph.github.io/SampleKit/latest/reference/cli.html),
  [queries, profiles and exports in `.samplekitrc`](https://zelyph.github.io/SampleKit/latest/reference/configuration.html).
- To do: [an export for a paper](https://zelyph.github.io/SampleKit/latest/how-to/export-for-a-paper.html).

Next: [Step 4 · Computing](../04-computing/README.md), where some values are
computed from others.
