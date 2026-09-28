# Move existing data in

This recipe turns measurements you already have — in a spreadsheet, a CSV
file, or a notebook — into sample files: one Markdown file per sample, in a
project of its own. Your original data is only read; SampleKit writes new
files beside it.

The example is a series of brews, each with a brewer, an original gravity and
a volume. Replace the names with yours.

## 1. Make the project

In an empty folder:

```console
$ mkdir notebook
$ cd notebook
$ samplekit init --no-model --no-venv --write
```

This writes `.samplekitrc` and an empty `samples/` folder. `--no-model`
leaves the model for later ([A first model](first-model.md)), and
`--no-venv` makes no Python environment, which reading and writing samples do
not need. `samplekit init` alone asks these questions one at a time, each
answer saying what it does and the default marked: what to start from, which
model — one to fill in, a model file you already have, or none for now — and
whether to make the environment.

## 2. Declare your quantities

Open `.samplekitrc` and declare each measured quantity with its unit and its
digits:

```toml
[property.og]
precision = [".3f", ".4f"]      # the value's digits, then the uncertainty's

[property.volume]
unit = "L"
precision = ".1f"
```

Do this before writing any sample. A name `.samplekitrc` declares is written
as a quantity, with its unit, and so is one a project's model declares, once
it has one ([A first model](first-model.md)). A name nothing declares is
written as an **attribute** — text with no unit — and `new` and `set` warn
you when that name is given a number. `brewer`, which is not a quantity,
needs no declaration.

## 3. Write the samples

One sample, from the command line:

```console
$ samplekit new B-01 brewer=ana og=1.052 og.u=0.001 volume=20.5 --into samples --write
```

`og.u` is the uncertainty of `og`. Repeated measurements are its readings:
`og.readings=1.052,1.053,1.052`. Write decimals with a point, never a comma.

**From a CSV file.** A shell loop gives each row to `new`. With
`brews.csv` holding `name,brewer,og,og_u,volume` and a row per brew:

```sh
tail -n +2 brews.csv | while IFS=, read -r name brewer og og_u volume; do
  samplekit new "$name" brewer="$brewer" og="$og" og.u="$og_u" volume="$volume" --into samples --write
done
```

From a spreadsheet, save the sheet as CSV first. A field that holds a comma
needs a CSV reader: use Python's `csv` module, as below.

**Series of measurements** — a table per sample — are simpler from Python.
With `logs/B-01.csv` holding `day,gravity` for each brew:

```python
import csv
from pathlib import Path

import samplekit as sk

for path in sorted(Path("logs").glob("*.csv")):
    sample = sk.Sample(Path("samples") / f"{path.stem}.md")
    with path.open(newline="") as log:
        rows = [{"day": int(row["day"]), "gravity": float(row["gravity"])}
                for row in csv.DictReader(log)]
    sample.fermentation = sk.Table(
        {"day": sk.Column(unit="d"), "gravity": sk.Column()},
        "day",
        rows=rows,
    )
    sample.save()
```

Run it with a Python where SampleKit's package is installed.

**By hand.** Write one file in a text editor, then shape the next ones on it:
`new --like` copies its attributes and leaves its measured values empty.

```console
$ samplekit new B-04 --like samples/B-01.md --write
$ samplekit set samples/B-04.md og=1.047 og.u=0.001 volume=21.0 brewer=tom --write
```

## 4. Check

```console
$ samplekit samples -c name,brewer,og,volume
$ samplekit list fields samples
$ samplekit validate samples
```

`list fields` shows each name under *properties* or *attributes*: a quantity
listed as an attribute was not declared in step 2. `validate` reports what is
wrong — a file it cannot read, two units in one column — and changes nothing.
Compare a few files with your original data by eye before trusting the rest.

The same steps, tried on the demo's step 2:

<!-- run: 02-measuring -->
```console
$ samplekit new pilsner og=1.048 og.u=0.001 volume=20 --into brews --write
$ samplekit new bitter --like brews/oatmeal-stout.md --write
$ samplekit set brews/bitter.md og=1.040 og.u=0.001 grain_mass=3.9 --write
$ samplekit brews -c name,og,volume,grain_mass
$ samplekit validate brews
```

## 5. Several collections, one common root

When your data comes as several collections — one per season, per style or
per person — each with a model of its own but the same quantities, declare
what they share once, at their common root, and let each collection import
it:

```text
notebook/
  .samplekitrc          [property.og], [property.volume], shared profiles
  ales/.samplekitrc     import = "..", its [model], what only ales have
  lagers/.samplekitrc   import = "..", its [model], what only lagers have
```

In `ales/.samplekitrc`:

```toml
schema_version = 1
import = ".."                 # the root's .samplekitrc

[model]
path = "ales.py"

[property.og]
precision = ".4f"             # finer here; the rest comes from the root
```

A key the collection writes replaces the root's; the rest is the root's. A
path the root declares reads from the root, and `{collection}` names each
collection's folder, so that one declaration serves all of them:

```toml
# notebook/.samplekitrc
[collection]
files = ["{collection}/photos/{name}*"]

[export.overview]
profile = "overview"
format = "csv"
output = "exports/{collection}/overview.csv"
```

Then check what each collection is given, and what it repeats:

```console
$ samplekit --show-rc ales
$ samplekit list profiles ales
$ samplekit validate
```

`--show-rc` names the file imported; `list profiles` marks each profile
`local`, `../.samplekitrc` or `local over` it; `validate` notes a
declaration that is a copy of the root's, which you can remove.

## See also

- [Projects](../explanations/projects.md): which configuration a sample
  follows, and what an import brings.
- [What a sample file can hold](../reference/sample-files.md), to write files
  by hand.
- [`init`, `new`, `set`](../reference/cli.md).
- [A sample and its file](../explanations/samples-and-files.md).
