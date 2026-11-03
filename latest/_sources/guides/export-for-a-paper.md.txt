# Tables and figures for a paper

This guide is for the tables and figures of a paper or a report: with the
labels and digits the paper wants, written by a command you run again at each
draft, and made again as they were when a reviewer asks, months later.

The commands run in the last step of the [home page's](../index.md) tensile
example, which shows the same steps more briefly: [tables written to
files](../index.md#tables-written-to-files) and [figures](../index.md#figures).
A table written once, for a spreadsheet or a script, needs no more than `-o`
([write the table to a file](find-and-select.md#write-the-table-to-a-file)).

## 1. The table, as a profile

A **profile** says which columns, with which titles and digits, in which
order, and in which groups. The tensile example's:

```toml
[profile.overview]
columns = [
  {field = "name"},
  {field = "material"},
  {field = "young_modulus", label = "E", precision = ".1f"},
  {field = "yield_strength", label = "Rp0.2"},
  {field = "tensile_strength", label = "Rm"},
  {field = "elongation", label = "A"},
]
sort = ["material", "name"]
```

Each column takes these keys, besides its `field` ([every
key](../reference/configuration.md#profilename)):

- `label` titles the column in a terminal and in a document; `header` names
  it in a CSV or JSON file, where a short name suits the program that reads
  it. A property is two columns in a CSV, `E_value [GPa]` and
  `E_uncertainty [GPa]`, the suffixes and the unit saying what each holds.
- `precision` sets the digits of the column, over the property's own. A file
  is written at the precision declared, as a table shows it; `.17g` writes
  every digit a number carries. Two, `[".1f", ".2g"]`, give the uncertainty
  digits of its own: an uncertainty is rounded up, never understated, so at
  `.1f` one of 0.012 would be written `0.1`, eight times its size.
- `template` writes the cell as text, `'\num{{value:.1f}}'`: a CSV holds that
  text, for a LaTeX package that reads CSV, and JSON keeps the numbers.

`group = ["material"]` gives a table per material in a terminal; in a file it
stays one table, each group's rows together.

Look at it before writing anything, with `--status`, which adds a `state`
column: a table for a paper should hold only current values.

<!-- run: tensile/04-results -->
```console
$ samplekit --profile overview --status --csv
note: yield_strength_uncertainty is left out: it is empty for every sample
note: tensile_strength_uncertainty is left out: it is empty for every sample
note: elongation_uncertainty is left out: it is empty for every sample
name,material,young_modulus_value [GPa],young_modulus_uncertainty [GPa],yield_strength_value [MPa],tensile_strength_value [MPa],elongation_value [%],state
aluminium-01,aluminium,69.2,0.3,285,318,12.0,current
copper-01,copper,117.2,0.9,76,221,44.0,current
steel-01,steel,206.3,1.1,387,518,24.0,current
```

## 2. The table, as a document

A **style** writes the table as a document holds it: `latex`, a `booktabs`
table with its units in the headers, `markdown` and `html` are given. `-o -`
prints it, `-o table.tex --write` writes it, the extension naming the style:

<!-- run: tensile/04-results -->
```console
$ samplekit --profile overview --style markdown -o -
| name | material | E (GPa) | Rp0.2 (MPa) | Rm (MPa) | A (%) |
| --- | --- | --- | --- | --- | --- |
| aluminium-01 | aluminium | 69.2 ± 0.3 | 285 | 318 | 12.0 |
| copper-01 | copper | 117.2 ± 0.9 | 76 | 221 | 44.0 |
| steel-01 | steel | 206.3 ± 1.1 | 387 | 518 | 24.0 |
```

To change how a style writes, declare it again under the same name, with only
what changes; the rest is kept ([every key of a
style](../reference/configuration.md#stylename)). A LaTeX table written with
`siunitx`, each value `\SI{…}{…}` with its unit in the cell:

```toml
[style.latex]
property = '\SI{{value}{uncertainty}}{{unit}}'
uncertainty = ' \pm {uncertainty}'
unitless = '\num{{value}{uncertainty}}'
header_unit = false

[unit."%"]
latex = '\percent'
```

A unit given no spelling in the style is written as its text, escaped:
`\SI{24.0}{\%}`. Where `.samplekitrc` already has a section, add the key to it
rather than write the section again: a section written twice makes the file
unreadable, *duplicate key*.

## 3. The figures

A figure for a paper is declared once, its axes, kind and group, and its look
is the project's: `[matplotlib]` for the font and the size, `symbol_figure`
for mathematics in the labels ([one style for the whole
project](../index.md#one-style-for-the-whole-project)). Sizes are in
centimetres: `"figure.figsize" = [8.5, 6]` fits one column of most journals.
A figure the model draws for one sample writes a file per sample where its
path holds `{name}`.

## 4. Written again by one command

An **export** names what a file holds and where it goes: a profile with a
style, or a figure. One command then writes it again at each draft, with no
`-o` to remember:

```toml
[export.paper]
profile = "overview"
style = "latex"
output = "paper/results.tex"

[export.curves]
figure = "curves"
output = "paper/curves.pdf"
```

<!-- run: tensile/04-results -->
```console
$ samplekit list exports
paper    paper/results.tex · profile overview
curves   paper/curves.pdf · figure curves
$ samplekit export paper --write
export paper · paper/results.tex · 3 rows
written: paper/results.tex, 3 rows
$ samplekit export curves --write
export curves · figure curves · paper/curves.pdf · 3 samples
written: paper/curves.pdf
```

A data file, `format = "csv"`, `tsv` or `json`, is exported the same way
([every key of an export](../reference/configuration.md#exportname)). A table
shown with `-c` or `--profile` declares one from what it shows, `--save
export=NAME`, previewed before it writes `.samplekitrc` ([saved
shapes](find-and-select.md#saved-shapes)).

**Written from values that are not current**, outdated, edited, failed or
never computed, an export writes its file all the same, says which values, and
exits with code 2, so that a script writing the paper stops there; a table
written with `-o` and a figure do the same. Compute them, or name their states
after `--accept`, `--accept outdated,failed`, if the file is meant as it
stands: the words are `outdated`, `edited` (or `edited:NAME,…` for those
values alone), `failed` and `never_computed`. A failed value is written as its
last number, its state saying not to trust it. A data file gives each such column its states,
`elongation_state` ([values out of date](../index.md#values-out-of-date)).
Where a sample could not be read, nothing is written: its rows would be
missing.

In the terminal interface, `p` offers the exports, previewed before `y`
writes them; its first line, *what is shown*, writes the screen's table once
to a file you name, and `W` declares what is shown as an export ([work in the
terminal interface](terminal-interface.md)).

## 5. From Python

From Python, `to_text` writes a table in a style, as `--style NAME -o -`
does, its `profile=` naming the columns; `export` writes a declared export, a
table or a figure, and returns the path written; `sk.plot(samples, "curves",
output="curves.pdf")` draws a figure that no export declares. Each is
recorded in the history, as the command's is. Unlike `--write`, `export` and
`sk.plot` never replace a file that exists: they raise `FileExistsError`, and
`overwrite=True`, `export("paper", overwrite=True)`, replaces the file, as
the script below does, so that it runs again at each draft:

```{literalinclude} ../../examples/tensile/04-results/scripts/paper.py
:language: python
:caption: scripts/paper.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/paper.py
| name | material | E (GPa) | Rp0.2 (MPa) | Rm (MPa) | A (%) |
| --- | --- | --- | --- | --- | --- |
| aluminium-01 | aluminium | 69.2 ± 0.3 | 285 | 318 | 12.0 |
| copper-01 | copper | 117.2 ± 0.9 | 76 | 221 | 44.0 |
| steel-01 | steel | 206.3 ± 1.1 | 387 | 518 | 24.0 |

results.tex
curves.pdf
```

## 6. Made again as they were

Every export and figure written is recorded in the project's history.
`explain` finds a file again, even copied into the paper's folder, says
whether what it was made from has changed since, and gives the command that
makes it again exactly as it was:

```console
$ samplekit paper/results.tex explain
paper/results.tex
  made      2026-10-06 19:16, by samplekit export paper --write
  written   paper/results.tex
  from      snapshot #1, 3 samples

nothing it was made from has changed since: it is up to date
made again as it was, from the project's folder: samplekit export paper --write -o paper/results.tex --at 246d92a
```

`--at` reads the project as the history kept it then, its samples, its
configuration and its model, and writes where `-o` says; the project is not
changed. A table made again this way is the same file, byte for byte; a
figure is drawn from the same data and settings, and a newer matplotlib may
draw it slightly differently.

## See also

- [Profiles, exports, figures and styles in `.samplekitrc`](../reference/configuration.md).
- [`export`, `plot`, `explain`, `--at`](../reference/cli.md).
- [The history](../concepts/history.md).
- [Written files](../reference/written-files.md): the columns and cells of a
  data file.
