# Step 5 · Ageing

In this step each piece carries two **tables**: `ageing`, a hardness and a
temperature for each day, and `inspection`, a score from each inspector. You read
single cells, filter on them, and see the model fill a column row by row and
compute values over a whole table.

Run every command from this folder, `05-ageing`.

## What changed

A table is written in the frontmatter, under `tables:`. Before the model
computed anything, the Alpha TITANIUM's ageing began:

```yaml
tables:
  ageing:
    title: Hardness and temperature, day by day
    index: day
    columns:
      day: {unit: d}
      hardness: {}
      temperature: {unit: degC}
    rows:
      - {day: 1, hardness: 1.041, temperature: 20.7}
      - {day: 2, hardness: 1.029, temperature: 20.7}
      - {day: 3, hardness: 1.021, temperature: 20.5}
```

- `index` is the column that names a row: here the day. The `inspection` table
  is indexed by the inspector's name.
- A cell is written as a property is: a number, and optionally an
  uncertainty, readings and the column's unit — or a text, a date or `true`,
  as an attribute is. A column of text — who inspected, under which gas — needs
  no unit, and is filtered and shown as any other.

The model declares the table, each column as a property, and fills two of
them:

```python
self.ageing = sk.Table(
    {
        "day": sk.Property(unit="d"),
        "hardness": sk.Property(),
        "temperature": sk.Property(unit="degC"),
        "nominal": sk.Property(
            unit="%", compute=self._nominal, depends_on=["row.hardness", "radius"]
        ),
        "rate": sk.Property(
            unit="pt/d",
            compute_column=self._rate,
            depends_on=["ageing.day", "ageing.hardness", "radius"],
        ),
    },
    index="day",
)
```

- `nominal`, the reduction reached that day, is computed in each row and
  declared as a property's formula is: `compute=` the function, given the
  row, and `depends_on=` what it reads, the row's own hardness, `row.hardness`,
  and the piece's `radius`.
- `compute_column=` runs once for the whole column, when a row needs the
  others: `rate`, how fast the hardness fell since the row before, in testers'
  points per day (one point is 0.001 of hardness). Its function is given the
  columns it reads, and returns one cell per row.

Two properties read a whole table: `drop`, a straight line fitted to the first
three days, with its standard error; and `score`, the inspectors' mean. Each says
which columns it reads, `depends_on=["inspection.score"]`, so a changed score
makes it outdated.

## Try it

```console
$ samplekit pieces/alpha-titanium.md view
$ samplekit pieces -c 'name,ageing.hardness[3],ageing.nominal[3]'
$ samplekit pieces -f 'inspection.score[Lea] > 44'
$ samplekit pieces --profile ageing
$ samplekit pieces --profile inspection --query best
$ samplekit pieces export inspection --write
```

## What you see

`view` shows the piece's values, then each table. `nominal` and `rate` are the
model's:

```text
  ageing · 8 rows
  day [d]   hardness   temperature [°C]   nominal [%]   rate [pt/day]
  ───────────────────────────────────────────────────────────────────
        1     1.041               20.7             36            22.7
        2     1.029               20.7             54            12.0
        3     1.021               20.5             67             8.0
        4     1.017               19.9             73             4.0
  …
```

A cell is named `table.column[index]`: `ageing.hardness[3]` is the
hardness of the row whose day is 3, and `inspection.score[Lea]` Lea's score.
`[#0]` is the first row whatever its index, `[#-1]` the last. A cell works
everywhere a field does: in a filter, a column, a sort, a profile.

```text
Piece               Process          Original hardness   Day 1   Day 3   Reduction, day 3 [%]   Drop [pt/day]
───────────────────────────────────────────────────────────────────────────────────────────────────────────
double-titanium         UT-05            1.078 ± 0.0007   1.052   1.027                       66      17.0 ± 2.8
alpha-titanium          UT-05            1.064 ± 0.0007   1.041   1.021                       67      14.0 ± 2.4
gamma-zinc      T-04             1.066 ± 0.0003   1.044   1.027                       59      13.0 ± 2.8
…
```

The gamma zinc has no inspection yet, so its `score` waits, as a formula with
a missing input does.

## Adding a row

```console
$ samplekit pieces/alpha-titanium.md set --add-row ageing day=16 hardness=1.011 temperature=18.9
$ samplekit pieces/alpha-titanium.md set 'ageing.hardness[3]=1.022'
```

The first previews a new row and names the columns the model will fill,
`nominal` and `rate`; the second changes one cell. Add `--write` to write
them, then `samplekit pieces compute --write` fills what they made not current.

## Why

A series of measurements belongs to its sample, not in a spreadsheet beside
it. Kept as a table in the file, each cell is a property like any other, and
what is computed from it knows when a cell changed.

- To understand: [a model and its formulas](https://zelyph.github.io/SampleKit/latest/explanations/models-and-formulas.html),
  [readings and their statistic](https://zelyph.github.io/SampleKit/latest/explanations/readings-and-statistics.html),
  which a cell can hold too.
- To look up: [a table in a sample file](https://zelyph.github.io/SampleKit/latest/reference/sample-files.html),
  [`sk.Table` and `sk.Property`](https://zelyph.github.io/SampleKit/latest/reference/python.html).

Next: [Step 6 · Figures](../06-figures/README.md), where the pieces are drawn.
