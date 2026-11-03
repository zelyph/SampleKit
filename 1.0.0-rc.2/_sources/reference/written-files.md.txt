# Written files

The shape of the files SampleKit writes from a table: what their columns are
called, what a cell holds, and how a value not current is said. The
[guides](../guides/find-and-select.md#write-the-table-to-a-file) show how to
write them; this page says what is in them.

## What writes one

| Written by | Holds |
| --- | --- |
| `samplekit -c … --csv`, `--tsv`, `--json`, or `-o FILE` | the table of samples shown, a row per sample |
| `samplekit export NAME` | an export `.samplekitrc` declares: a profile's table, or a figure |
| `samplekit FILE view TABLE --csv`, or `-o FILE` | a sample's own table, a row per row of it |
| Python's `to_csv`, `to_json`, `to_text`, `export` | the same files, from a list of samples; `Table.to_csv()` for a sample's table |

`-o` names the format by its extension: `.csv`, `.tsv` and `.json` are data
files, `.tex`, `.md` and `.html` documents in the style of that name. A file
is previewed, and written by `--write`, which replaces it; each one written
is kept in the project's history, which `explain FILE` reads
([the history](../concepts/history.md)).

## Columns

A data file names its columns after the fields, never after the labels a
terminal shows, so that a program reading it finds them by name:

| Column | Header |
| --- | --- |
| an attribute, `name`, `tags` | the field: `material` |
| a property | two columns, `FIELD_value [UNIT]` and `FIELD_uncertainty [UNIT]` |
| a property named in `-c`, `young_modulus=E`, or by a profile's `header` | `E_value [GPa]`, `E_uncertainty [GPa]` |
| a part of a value, `thickness.readings`, `width.u` | the part as named, with its unit |
| a cell, `'curve.stress[10]'` | the cell as named, with its unit |
| `--status` | `state`, last |

The unit is written as the sample files spell it, `degC` rather than `°C`,
unless `--style`, or the export's `style`, names another. An uncertainty
column empty in every row is left out, and a note says so. Two columns
writing one header are refused.

## Cells

<!-- run: tensile/04-results -->
```console
$ samplekit -c name,young_modulus:.1f=E,thickness.readings,compared_with --csv
name,E_value [GPa],E_uncertainty [GPa],thickness.readings [mm],compared_with
aluminium-01,69.2,0.3,1.993;1.98;2.007,[[steel-01]]
copper-01,117.2,0.9,1.995;1.988;2.007,[[steel-01]]
steel-01,206.3,1.1,1.995;2.004;2.008,
```

- **A number** is written at the precision declared for its column or its
  property, here `.1f`; where none is declared, every digit stored. An
  uncertainty is rounded up, never understated.
- **Nothing entered** is an empty cell; a value that does not apply, `n/a`,
  is written `n/a`.
- **A list**, readings or a list attribute, and **tags** are their items
  joined by `;`.
- **A link** is its text as written, `[[steel-01]]`, in a data file and in a
  document alike.
- Lines end with a line feed on every system, so that a file written on two
  computers is the same file.

TSV is CSV with tabs between the cells.

## JSON

A JSON file is a list of objects, one per row, each keyed by the columns'
fields, or by the name `-c` or `header` gives them. A property is an object
holding its value, its uncertainty, its unit and its state; a list and the
tags are arrays, and `n/a` is `null`:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md -c name,young_modulus:.1f=E,thickness.readings,tags --json
[
  {
    "name": "steel-01",
    "E": {
      "value": 206.3,
      "uncertainty": 1.1,
      "unit": "GPa",
      "state": "current"
    },
    "thickness.readings": [
      1.995,
      2.004,
      2.008
    ],
    "tags": []
  }
]
```

## Values not current

A column holding a value not current gets a column of its states beside it,
`FIELD_state`, empty where the value is current: `outdated`, `edited`,
`failed`, `never_computed`. JSON says it in each property's `state`. Written
to a file with `-o`, or by an export, the file is written all the same, the
values not current are said, and the command exits with code 2, unless
`--accept` names their states. Here after the steel specimen's width was
measured again, its area not yet computed:

```console
$ samplekit -c name,area:.3f --csv
name,area_value [mm²],area_uncertainty [mm²],area_state
aluminium-01,24.922,0.098,
copper-01,24.967,0.070,
steel-01,25.056,0.053,outdated
$ samplekit -c name,area:.3f -o area.csv --write
the table is written from values that are not current: 1 outdated in 1 sample
steel-01 area
samplekit status says why, and compute --write brings them up to date
written: area.csv
error: area.csv is written from values not current — --accept outdated exits 0 for it
```

## A sample's own table

`view TABLE` writes a row of the file per row of the table: its index first,
then each column, a property's two columns as above. Over several samples,
a column `name` comes first, and each sample's rows follow one another:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md view curve --csv
note: strain_uncertainty is left out: it is empty in every row
note: force_uncertainty is left out: it is empty in every row
note: stress_uncertainty is left out: it is empty in every row
strain_value [%],force_value [kN],stress_value [MPa]
0,0.0,0.0
0.025,1.281,51.1
…
```

## Documents

A document, `.tex`, `.md` or `.html`, is the table as the style writes it:
its columns headed by their labels, a property in one cell with its
uncertainty, its units spelled as the style spells them. [Tables and figures
for a paper](../guides/export-for-a-paper.md) shows the styles given and how
to declare one ([`[style.NAME]`](configuration.md#stylename)).

## See also

- [`-o`, `--csv`, `--status`, `--accept`, `export`, `view`](cli.md).
- [`[profile.NAME]` and `[export.NAME]`](configuration.md#exportname).
- [Exit codes](exit-codes.md).
