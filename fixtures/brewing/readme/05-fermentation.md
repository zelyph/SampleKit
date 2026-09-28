# Step 5 · Fermentation

In this step each brew carries two **tables**: `fermentation`, a gravity and a
temperature for each day, and `tasting`, a score from each taster. You read
single cells, filter on them, and see the model fill a column row by row and
compute values over a whole table.

Run every command from this folder, `05-fermentation`.

## What changed

A table is written in the frontmatter, under `tables:`. Before the model
computed anything, the Citra IPA's fermentation began:

```yaml
tables:
  fermentation:
    title: Gravity and temperature, day by day
    index: day
    columns:
      day: {unit: d}
      gravity: {}
      temperature: {unit: degC}
    rows:
      - {day: 1, gravity: 1.041, temperature: 20.7}
      - {day: 2, gravity: 1.029, temperature: 20.7}
      - {day: 3, gravity: 1.021, temperature: 20.5}
```

- `index` is the column that names a row: here the day. The `tasting` table
  is indexed by the taster's name.
- Every cell is a quantity, like a property: a value, and optionally an
  uncertainty, readings and the column's unit.

The model declares the table, and fills two of its columns:

```python
self.fermentation = sk.Table(
    {
        "day": sk.Column(unit="d"),
        "gravity": sk.Column(),
        "temperature": sk.Column(unit="degC"),
        "apparent": sk.Column(unit="%"),
        "rate": sk.Column(unit="pt/d"),
    },
    "day",
    compute_rows=[(["apparent"], ["row.gravity", "og"], self._apparent)],
    compute_columns=[("rate", ["fermentation.day", "fermentation.gravity", "og"], self._rate)],
)
```

- `compute_rows` runs once per row: `apparent`, the attenuation reached that
  day, reads the row's own gravity and the brew's `og`.
- `compute_columns` runs once for the whole column, when a row needs the
  others: `rate`, how fast the gravity fell since the row before, in brewers'
  points per day (one point is 0.001 of gravity).

Two properties read a whole table: `drop`, a straight line fitted to the first
three days, with its standard error; and `score`, the tasters' mean. Each says
which columns it reads, `depends_on=["tasting.score"]`, so a changed score
makes it outdated.

## Try it

```console
$ samplekit view brews/citra-ipa.md
$ samplekit brews -c 'name,fermentation.gravity[3],fermentation.apparent[3]'
$ samplekit brews -f 'tasting.score[Lea] > 44'
$ samplekit brews --profile fermentation
$ samplekit brews --profile tasting --query best
$ samplekit export tasting brews --write
```

## What you see

`view` shows the brew's values, then each table. `apparent` and `rate` are the
model's:

```text
  fermentation · 8 rows
  day [d]   gravity   temperature [°C]   apparent [%]   rate [pt/day]
  ───────────────────────────────────────────────────────────────────
        1     1.041               20.7             36            22.7
        2     1.029               20.7             54            12.0
        3     1.021               20.5             67             8.0
        4     1.017               19.9             73             4.0
  …
```

A cell is named `table.column[index]`: `fermentation.gravity[3]` is the
gravity of the row whose day is 3, and `tasting.score[Lea]` Lea's score.
`[#0]` is the first row whatever its index, `[#-1]` the last. A cell works
everywhere a field does: in a filter, a column, a sort, a profile.

```text
Brew               Yeast          Original gravity   Day 1   Day 3   Attenuation, day 3 [%]   Drop [pt/day]
───────────────────────────────────────────────────────────────────────────────────────────────────────────
double-ipa         US-05            1.078 ± 0.0007   1.052   1.027                       66      17.0 ± 2.8
citra-ipa          US-05            1.064 ± 0.0007   1.041   1.021                       67      14.0 ± 2.4
smoked-porter      S-04             1.066 ± 0.0003   1.044   1.027                       59      13.0 ± 2.8
…
```

The smoked porter has no tasting yet, so its `score` waits, as a formula with
a missing input does.

## Adding a row

```console
$ samplekit set brews/citra-ipa.md --add-row fermentation day=16 gravity=1.011 temperature=18.9
$ samplekit set brews/citra-ipa.md 'fermentation.gravity[3]=1.022'
```

The first previews a new row and names the columns the model will fill,
`apparent` and `rate`; the second changes one cell. Add `--write` to write
them, then `samplekit compute brews --write` fills what they made not current.

## Why

A series of measurements belongs to its sample, not in a spreadsheet beside
it. Kept as a table in the file, each cell is a quantity like any other, and
what is computed from it knows when a cell changed.

- To understand: [a model and its formulas](https://zelyph.github.io/SampleKit/latest/explanations/models-and-formulas.html),
  [readings and their statistic](https://zelyph.github.io/SampleKit/latest/explanations/readings-and-statistics.html),
  which a cell can hold too.
- To look up: [a table in a sample file](https://zelyph.github.io/SampleKit/latest/reference/sample-files.html),
  [`sk.Table` and `sk.Column`](https://zelyph.github.io/SampleKit/latest/reference/python.html).

Next: [Step 6 · Figures](../06-figures/README.md), where the brews are drawn.
