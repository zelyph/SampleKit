# Bring existing data in

This guide is for measurements you already have, in a spreadsheet, CSV files
or a notebook, to turn into sample files: one Markdown file per sample, in a
project. Your original data is only read; SampleKit writes new files beside
it.

The example is the [home page's](../index.md) tensile specimens: a material,
a width and a thickness per specimen, and the force read at each strain,
which the testing machine wrote to a CSV file per specimen.

## 1. Make the project

```console
$ samplekit tensile-tests init --no-model --no-venv --write
```

This writes `.samplekitrc` and an empty `samples/` folder. `--no-model`
leaves the model for later, and `--no-venv` makes no Python environment,
which reading and writing samples do not need; `init` without them asks
([start a project](start-a-project.md)).

## 2. Declare your properties

A measured quantity with a unit is a **property**; a fact without one, the
material or the grade, is an **attribute** ([samples and their
files](../concepts/samples-and-files.md)). Declare each property in
`.samplekitrc`, with its unit and its digits, before writing any sample:

```toml
[property.width]
unit = "mm"
precision = ".2f"

[property.thickness]
unit = "mm"
precision = ".2f"
```

A name `.samplekitrc` declares is then written as a property, with its unit,
and so is one the project's model declares, once there is one. A name nothing
declares is written as an attribute, and `new` and `set` warn you when that
name is given a number.

## 3. Write the samples

**One sample** at a time, from the command line:

```console
$ samplekit samples new steel-01 material=steel grade=S355 width=12.51 thickness=2.00 --write
```

`width.u=0.01` gives an uncertainty, and `width.readings=12.494,12.528,12.518`
repeated measurements. Write decimals with a point, never a comma ([edit
samples](edit-samples.md)).

**From a CSV file**, a shell loop gives each row to `new`. With
`specimens.csv` holding `name,material,grade,width,thickness`, a row per
specimen:

```sh
tail -n +2 specimens.csv | while IFS=, read -r name material grade width thickness; do
  samplekit samples new "$name" material="$material" grade="$grade" \
      width="$width" thickness="$thickness" --write
done
```

From a spreadsheet, save the sheet as CSV first. A field that holds a comma
needs a real CSV reader: Python's `csv` module, as below.

**A table per sample**, such as each test's curve, is simpler from Python.
With `machine/steel-01/curve.csv` holding `strain [%],force [kN]` for each
specimen, and its sample written above:

```python
import csv
from pathlib import Path

import samplekit as sk

for folder in sorted(Path("machine").iterdir()):
    sample = sk.Sample(Path("samples") / f"{folder.name}.md")
    with (folder / "curve.csv").open(newline="") as exported:
        rows = [{"strain": float(row["strain [%]"]), "force": float(row["force [kN]"])}
                for row in csv.DictReader(exported)]
    sample.curve = sk.Table(
        {"strain": sk.Property(unit="%"), "force": sk.Property(unit="kN")},
        index="strain",
        rows=rows,
    )
    sample.save()
```

Run it with the Python where SampleKit's package is installed ([work from
Python](work-from-python.md)). Step 1 made none: `samplekit init`, run again
in the project, offers to make it, `.venv/`, or make it yourself ([the Python
package](install.md#the-python-package)). The CSV files stay where they are, and
`[collection] files = ["machine/{name}/*"]` keeps them attached to their
sample ([files](../index.md#files)).

**By hand**, write one file in a text editor, then the next ones from it:
`new --keep` takes the fields it names from a sample, and nothing else:

```console
$ samplekit samples new steel-02 --keep samples/steel-01.md material,grade --write
```

**In the terminal interface**, `N` on the list makes a new sample beside the
one under the cursor: its name, then `field=value` for each value, and
`keep:material,grade` to take fields from the one under the cursor. `e` on its
screen fills a value, and `A` adds an attribute; each change is shown before
`y` writes it ([work in the terminal
interface](terminal-interface.md)).

## 4. Check

`list fields` shows each name under *properties* or *attributes*: a property
listed as an attribute was not declared in step 2. `validate` reports what
is wrong in a file, and changes nothing. Then compare a few files with your
original data by eye before trusting the rest:

<!-- run: tensile/02-readings -->
```console
$ samplekit list fields
properties (2)
  width  thickness

attributes (4)
  material  grade  direction  tested

always
  name  path  filename  project  state  tags
$ samplekit validate
3 samples · 0 defects · 0 notes
```

## Next

- [Write a model](first-model.md): what your formulas compute from these
  values.
- Data from several series, or several people: [several
  projects](several-projects.md).
