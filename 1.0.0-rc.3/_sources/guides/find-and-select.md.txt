# Find and select samples

This guide is for answering a question from your samples: which ones, which
values, in which order, grouped how, and the answer written to a file. Every
command on this page only reads, but for a file written with `--write`.

The commands run on the [home page's](../index.md) tensile example, in its
last step.

## Which samples a command reads

A command reads the folder or the files you name, `samples/` or
`samples/steel-01.md`; naming nothing, the folder you are in. In a project,
a folder is read with the folders below it ([which folders a command
reads](../concepts/projects.md#which-folders-a-command-reads)). Given
nothing else, a command lists the samples' files:

<!-- run: tensile/04-results -->
```console
$ samplekit .
samples/aluminium-01.md
samples/copper-01.md
samples/steel-01.md
```

## Columns

`-c` names the columns, by the names the files use. A column may also be one
part of a value, or one cell of a table:

| Column | Is |
| --- | --- |
| `width` | the value, with its uncertainty and its unit |
| `width.u` | its uncertainty alone |
| `thickness.readings` | its readings |
| `'curve.stress[10]'` | the stress in the row whose index, the strain, is 10 |
| `name`, `path`, `filename`, `project`, `tags`, `state` | what every sample has |

<!-- run: tensile/04-results -->
```console
$ samplekit -c width.u,thickness.readings,'curve.stress[10]'
name           width.u [mm]   thickness.readings [mm]   curve.stress[10] [MPa]
──────────────────────────────────────────────────────────────────────────────
aluminium-01     0.00375648   1.993, 1.98, 2.007                         318.1
copper-01        0.00328296   1.995, 1.988, 2.007                        179.9
steel-01         0.0100885    1.995, 2.004, 2.008                        504.3

3 of 3 samples
```

`samplekit list fields` lists every name the samples use, properties,
attributes and tables apart, with an example of a cell's address.

## Filters

`-f` keeps the samples a condition selects. It compares a field with `==`,
`!=`, `<`, `<=`, `>`, `>=`; combines with `&&` (and), `||` (or) and `!`
(not); and knows a few words: `has` for a tag, `in (a, b)` for a list,
`is missing` for a value nobody entered, `links to steel-01` for the samples
linking to one ([link samples](link-samples.md)). Quote the whole condition, so that
the shell leaves it alone ([every form](../reference/filters.md)).

A table is searched with `where`: the samples one of whose rows holds what the
condition says. Here, the specimens that stretched past 30 %:

<!-- run: tensile/04-results -->
```console
$ samplekit -f 'curve where (strain > 30)' -c material
name        material
────────────────────
copper-01   copper

1 of 3 samples
```

Several `-f` all hold. A filter used often is saved in `.samplekitrc` as a
query, `[query.ductile]` with `filter = "elongation > 20"`, and `--query
ductile` selects with it ([the home page](../index.md#profiles-and-summaries)).

## Order and groups

`-s` sorts by a field, a `-` before it for the highest first; several,
comma-separated, sort by the first, then the next. `-r` reverses the order.
`--group` gives a table per value of a field:

<!-- run: tensile/04-results -->
```console
$ samplekit -c elongation -s -elongation -f 'elongation > 20'
name        elongation [%]
──────────────────────────
copper-01             44.0
steel-01              24.0

2 of 3 samples
$ samplekit -c material,elongation --group material
material = aluminium
name           material    elongation [%]
─────────────────────────────────────────
aluminium-01   aluminium             12.0

material = copper
name        material   elongation [%]
─────────────────────────────────────
copper-01   copper               44.0

material = steel
name       material   elongation [%]
────────────────────────────────────
steel-01   steel                24.0

3 of 3 samples
```

`--summary` gives each column's statistics instead of its rows, and
`--group` with it, the statistics of each group.

## Write the table to a file

Every table on this page can go to a file. `--csv`, `--tsv` and `--json`
print it as data for another program: a value and its uncertainty in two
columns, each headed with its unit, and a column empty in every sample left
out, with a note saying so:

<!-- run: tensile/04-results -->
```console
$ samplekit -c name,material,tensile_strength=Rm -s -tensile_strength --csv
note: Rm_uncertainty is left out: it is empty for every sample
name,material,Rm_value [MPa]
steel-01,steel,518
aluminium-01,aluminium,318
copper-01,copper,221
```

`-o` names the file, and its extension says what it holds: `.csv`, `.tsv` or
`.json` a data file, `.tex`, `.md` or `.html` a table for a document, in that
style. It is previewed, and `--write` writes it:

<!-- run: tensile/04-results -->
```console
$ samplekit -c name,tensile_strength=Rm -s -tensile_strength -o strongest.md --write
written: strongest.md
```

```{code-block} text
:caption: strongest.md

| name | Rm (MPa) |
| --- | --- |
| steel-01 | 518 |
| aluminium-01 | 318 |
| copper-01 | 221 |
```

A file is written at the precision a table shows, and `--precision .17g`
writes every digit; a value not current is written all the same, said, and
the command exits with code 2, unless `--accept` names its state. A table
written again at each draft is better declared once, as an export: `--save
export=NAME`, below, declares one from what the table shows, and [tables and
figures for a paper](export-for-a-paper.md) goes further, labels, styles and
the history of each file written.

### A sample's own table

A sample's own table, its curve, is written the same way, a row of the curve
a row of the file. `view` followed by the table's name shows that table
alone, and takes `--csv`, `--tsv`, `--json` and `-o`:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md view curve -o curve.csv --write
note: strain_uncertainty is left out: it is empty in every row
note: force_uncertainty is left out: it is empty in every row
note: stress_uncertainty is left out: it is empty in every row
written: curve.csv
```

```{code-block} text
:caption: curve.csv (its first rows)

strain_value [%],force_value [kN],stress_value [MPa]
0,0.0,0.0
0.025,1.281,51.1
0.05,2.585,103.2
```

Its columns are written as a table of samples writes a property, at the
precision declared for each, every digit stored where none is. Over several
samples, `samplekit samples view curve --csv`, a column `name` comes first
and each sample's rows follow one another. From Python,
`steel.curve.to_csv()` gives the same text.

## Saved shapes

A profile saves columns, labels, an order and groups; a query saves a filter;
both are declared in `.samplekitrc`, and listed:

<!-- run: tensile/04-results -->
```console
$ samplekit list queries
ductile   elongation > 20
$ samplekit list profiles
overview   name, material, young_modulus, yield_strength, tensile_strength, elongation
data       name, young_modulus, yield_strength
```

`--save` declares what a table shows, under a name: `query=NAME` its filter,
`profile=NAME` its columns, order and groups, `export=NAME` both and an export
to `out/NAME.csv`. It shows the lines it adds to `.samplekitrc`, and asks:

<!-- run: tensile/04-results -->
```console
$ samplekit -c name,tensile_strength=Rm -s -tensile_strength --save profile=strongest
save in .samplekitrc
  +  45  [profile.strongest]
  +  46  columns = [{ field = "name" }, { field = "tensile_strength", label = "Rm", header = "Rm" }]
  +  47  sort = ["-tensile_strength"]
  +  48

apply these changes? [y/N] n
nothing written
```

## In the terminal interface

On the list of samples, each question is a key, and the table changes as you
type ([work in the terminal interface](terminal-interface.md)):

| Key | Does what the command line does with |
| --- | --- |
| `/` | `-f`: the same language, `Tab` completing fields, operators and values |
| `s` | `-s`: choose the columns to sort by; `r` on one sorts it highest first |
| `c` | `-c`: tick the columns to show |
| `g` | `--group`: a heading above each group |
| `f` | `--query`, `--profile`: apply a saved one |
| `S` | `--summary`: the columns' statistics, or the samples again |
| `W` | save what is shown in `.samplekitrc`, as a query, a profile or an export |

## From Python

The same questions from Python: `filter`, `query` and `sorted` take what
`-f`, `--query` and `-s` take, and give a new list; `stats` gives what
`--summary` gives ([work from Python](work-from-python.md)):

```{literalinclude} ../../examples/tensile/04-results/scripts/selecting.py
:language: python
:caption: scripts/selecting.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/selecting.py
['copper-01']
copper-01 44.0 %
steel-01 24.0 %
26.666666666666668
```

## A mistake is said, never guessed

SampleKit refuses what it would have to guess, and says why:

- **A name misspelt** is refused naming the nearest one; in a terminal, it
  asks whether to use it, `use 'tensile_strength'? [Y/n]`.
- **A condition that does not read** is refused at the character where it
  stops:

```console
$ samplekit -f 'material = steel'
error: at character 10: expected a comparison operator, found '='
  material = steel
           ^
```

- **A text compared with a number**, `grade > 3`, is refused where every
  sample holds text; where one sample holds text among numbers, the others are
  compared and the one left aside is named.
- **A file that cannot be read** is never skipped in silence: the command
  names it, reads the rest, and exits with code 2. `samplekit list skipped`
  lists every file set aside, and why.

[When something goes wrong](when-something-goes-wrong.md) gathers these and
the others.

## See also

- [Filters](../reference/filters.md), every form.
- [Queries, profiles and groups](../concepts/projects.md#queries-profiles-and-groups).
- Tables written to a file: [above](#write-the-table-to-a-file), [the home
  page](../index.md#tables-written-to-files), and [tables and figures for a
  paper](export-for-a-paper.md); what each file holds, column by column:
  [written files](../reference/written-files.md).
