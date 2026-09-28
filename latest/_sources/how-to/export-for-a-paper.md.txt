# An export for a paper

This recipe makes the tables and figures of a paper from your samples: with
the labels and digits the paper wants, written by a command you can run again,
and made again as they were when a reviewer asks, months later.

The commands run in the tutorial's [step 6](../tutorial/06-figures.md); use
your own names in your project.

## 1. Declare the table as a profile

A **profile** says which columns, with which titles and digits, in which
order, and optionally in which groups. In `.samplekitrc`:

```toml
[profile.paper]
columns = [
  { field = "name", label = "Brew", header = "brew" },
  { field = "style", label = "Style", header = "style" },
  { field = "og", label = "Original gravity", header = "OG", precision = ".3f" },
  { field = "abv", label = "Alcohol", header = "ABV" },
  { field = "attenuation", label = "Attenuation", header = "AA" },
]
sort = ["-abv"]
group = ["style"]
```

- `label` titles the column in the terminal; `header` names it in a CSV or
  JSON file, where a short name suits the tool that reads it.
- `precision` sets the digits of this column; without it, the property's own
  precision applies. The file gets the digits you see on screen.
- `group` gives one table per style in the terminal; in a file it stays one
  table, each group's rows together.

Look at it before writing anything:

<!-- run: 06-figures -->
```console
$ samplekit brews --profile alcohol
$ samplekit brews --profile alcohol --group style
$ samplekit brews --profile alcohol --status --csv
```

`--status` adds a `state` column, `current` or what is not: a table for a
paper should hold only current values. `samplekit status` says why one is not,
and `samplekit compute --write` brings it up to date.

## 2. Declare the export

An **export** writes a profile to a fixed file:

```toml
[export.paper]
profile = "paper"
format = "csv"
output = "out/paper.csv"
```

`format` is `csv`, `tsv` or `json`. An export may also name a `query`, to take
only some samples.

<!-- run: 06-figures -->
```console
$ samplekit export tasting brews                # a preview: rows, file, values not current
$ samplekit export tasting brews --write
```

For a LaTeX report, a style gives each unit and symbol a LaTeX spelling
(`[unit."%"] math = '\%'`), and `--style math` uses it:
`samplekit brews --profile alcohol --style math`.

## 3. The figures

Declare each figure of the paper under `[figure.NAME]`, with its axes, kind,
group and labels, and write it to a file:

<!-- run: 06-figures -->
```console
$ samplekit plot score brews -o figures/score.pdf --write
$ samplekit plot attenuation_curve brews -o 'figures/{name}.pdf' --write
```

`--write` makes the folder if it does not exist. A figure the model draws
for one sample writes one file per sample when the name holds `{name}`.
Sizes are in centimetres: `figsize = [8.5, 6]` in the declaration fits one
column of most journals.

## 4. Make them again as they were

Every export and figure written is recorded in the project's history.
`explain` finds a file again, even copied into your paper's folder, says
whether what it was made from has changed since, and gives the command that
makes it again exactly as it was:

<!-- run: 06-figures -->
```console
$ samplekit export tasting brews --write
$ samplekit explain out/tasting.csv
$ samplekit log
$ samplekit export tasting brews --at 1 -o tasting-then.csv --write
```

`--at` reads the project as a snapshot of the history kept it — its samples,
its configuration and its model then — and writes where `-o` says; your
project is not changed. An export made again this way is the same file, byte
for byte. A figure is drawn from the same data and settings; a newer
matplotlib may draw it slightly differently.

## See also

- The tutorial's [step 3](../tutorial/03-selecting.md) and
  [step 6](../tutorial/06-figures.md).
- [Profiles, exports, figures and styles in `.samplekitrc`](../reference/configuration.md),
  [`export`, `plot`, `explain`, `--at`](../reference/cli.md).
- [The history](../explanations/history.md).
