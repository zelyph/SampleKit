# Upgrade from an older SampleKit

This version knows its own files, its own configuration and its own names
alone: nothing in it recognises, converts or cleans up what an earlier
version wrote. A tool beside it, `samplekit-migrate.pyz`, does most of an
upgrade for you, in a copy of your project; what an upgrade asks is also said
here, to do by hand, once. Upgrading from 1.0.0-rc.1 is a matter of a few
renames and a clean-up; upgrading from 0.2 means moving your samples, further
down.

## The migration tool

**Nothing guarantees that the tool carries your data correctly.** It cannot
fit every use, and it will miss some. It works on a copy, so that nothing is
lost: it never writes in the folder it reads. **Before anything, keep a
backup** of your project: three copies, on two kinds of media, one of them
elsewhere (the 3-2-1 rule), and, better still, a git repository.

`samplekit-migrate.pyz` is one file, attached to each
[release](https://github.com/zelyph/SampleKit/releases) beside the command
line. It reads a project of 0.1.0, 0.1.1, 0.1.2, 0.2.0 or 1.0.0-rc.1 and
writes a new folder this version reads. It installs nothing: run it with the
Python of an environment where this version's package is installed, the
version `samplekit --version` says.

```text
$ python -m venv .venv
$ .venv/bin/python -m pip install samplekit==VERSION
$ .venv/bin/python samplekit-migrate.pyz old --to new --model old/specimen.py
```

Without `--write`, it reads everything and says what it would write. Here, a
tensile project of 0.2, its model in `old/specimen.py`:

```text
SampleKit migration tool, for SampleKit 1.0.0-rc.2.

Nothing guarantees that this tool carries your data correctly: it cannot fit
every use, and it will miss some. It works on a copy, so that nothing is lost:
it never writes in the folder it reads.

Before anything, keep a backup of your project: three copies, on two kinds of
media, one of them elsewhere (3-2-1), and, better still, a git repository.

Would write 5 files to new:
  .samplekitrc
  specimen.py
  samples/aluminium-01.md
  samples/copper-01.md
  samples/steel-01.md

Read as SampleKit 0.x (0.1.0 to 0.2.0).

## Carried

- the text below each frontmatter, as the note, as it is: what 0.x wrote there from template() is a copy of the values then, to remove where it is one
- material: no number, written as an attribute of each sample, and out of the model
- operator: no number, written as an attribute of each sample, and out of the model
- width: readings, given 0.x's statistic, their mean and sample standard deviation
- thickness: readings, given 0.x's statistic, their mean and sample standard deviation

## Not carried: computed again

- area, curve.stress, tensile_strength

## Left behind

- the model's default values, gauge_length = 50.0: 0.x wrote them in every file it saved, and they are carried from there; this version's model declares none
- specimen.py: template(), taken out of the model

## To check by hand

- samples/copper-01.md: thickness, an uncertainty typed over its readings, 0.003: carried beside them, and compute writes the statistic's in its place; to keep it, set it again after computing
- specimen.py: the methods _area, _area_unc, _tensile_strength, _stress, copied as 0.x wrote them, each marked # TODO(migrate). A formula is too varied to be translated: read each against this version's API (a table's columns are read self.curve.values("stress"), a row's cells row.force.value)

## What to do next

1. Make the project's environment, in the new folder: `python -m venv .venv`, then `.venv/bin/python -m pip install samplekit==1.0.0-rc.2`.
2. Read the model and every line marked `# TODO(migrate)`.
3. `samplekit . validate`, then `samplekit . compute --write`, which computes again what was not carried.
4. Compare a few samples with the earlier version's by hand before trusting the rest.

Nothing written: --write writes it.
```

`--write` writes the new folder, and in it `MIGRATION.md`, the same report,
with what this version's `validate` and `status` say of the copy. Then do
what it says: make the environment, read every line marked
`# TODO(migrate)`, and compute.

- **From 0.x**, `--model` names the module of your class: a file of 0.x does
  not say which of its values a formula computed, and the model does. The
  model is read, never run. A project of this version has one class: where
  the module holds several classes of samples, `--class` names the one your
  samples were saved with, and samples of another class are migrated apart.
  Its `__init__` is written again in this version's API, a value that is no
  number (text, a date, a flag) becoming an attribute; every other method is copied as it was and marked, since a formula is
  too varied to be translated. What a formula computed is not carried, and
  `compute --write` gives it again; `--keep-computed` carries it, as values
  typed over their formulas, which `compute --force` gives back to them.
  `--verify`, with `--write`, computes the copy and compares each value with
  the one 0.x wrote.
- **From 1.0.0-rc.1**, the sample files are this version's already: they are
  copied as they are, with the history, and the configuration and the model
  are renamed as the section below says. Bring every value up to date with
  rc.1 first, as it says too.

`samplekit-migrate.pyz --help` lists its options. The sections below say what
the tool does, step by step, for those who would rather do it by hand, or
check what it did.

## From 1.0.0-rc.1

The sample files are the same, schema 1: nothing in them changes. What
changes is how the command line is written, a few keys of `.samplekitrc`, a
few names in Python, and where SampleKit keeps which formula computed each
value.

**Upgrade every computer that shares a project together**, as for 0.2: once
one has written with this version, a computer still on rc.1 does not read the
project's configuration, nor its history, and what it writes then is kept
nowhere.

### Before you upgrade: bring every value up to date, with rc.1

rc.1 recorded which formula computed each value on the computer, in its state
folder. This version keeps it in each project's history, `.samplekit/history`,
and does not read what rc.1 recorded. At its first write in a project, it
takes every value the files hold as computed by the formulas as they are then,
and says how many.
**A formula changed between your last rc.1 computation and that first write
would go unseen.** So, with rc.1 still installed, in each project:

```text
$ samplekit status samples/
$ samplekit compute samples/ --write
```

until `status` names no value *outdated* or *formula changed* (a value that
waits for an input nobody entered yet may stay). Then change no formula until
this version has written once in the project.

**If you upgraded first**, or are not sure no formula changed, compute again
the values of a formula you changed, or every value, with this version:

```text
$ samplekit samples/ compute -p area --rerun --write
$ samplekit samples/ compute --rerun --write
```

The second takes as long as computing the project from nothing.

### Install the command line and the package together

The command line and the Python package in each project's environment are
one version: install both, the command line as [Install
SampleKit](install.md) says, and the package with
`.venv/bin/python -m pip install samplekit==VERSION`, the version
`samplekit --version` says. `.venv/bin/pip show samplekit` then names it. The
model runs in **the Python `[model] python` names**, an environment folder or
an interpreter; rc.1 found the project's `.venv` by itself, and this version
looks for none: add the line.

```toml
[model]
path = "model/specimen.py"
class = "Specimen"
python = ".venv"
```

### The command line: paths anywhere

Paths and options are read before the command and after it: `samplekit
samples/ status` and `samplekit status samples/` read the same, and the help,
this guide and what SampleKit says write the path first. The first word
naming a command is the command: a folder of that name is written `status/`.
A file SampleKit wrote, copied outside its project, is found by `samplekit
FILE explain` in the projects around it (the one holding it, the current
folder's, those below the current folder, those the TUI opened last):
`samplekit FILE explain FOLDER` is gone, a folder after `explain` being its
samples. A preview asks `apply these changes? [y/N]`: a script answers with
`--write`. `compute --try` is gone, refused as any unknown option: `compute`
asks before it computes, and before it writes.

### The configuration

A key this version does not have is refused, and the configuration is not
read until it is renamed:

| rc.1 | Now |
| --- | --- |
| `[style.NAME] separator = "\pm"` | `property = '{value} \pm {uncertainty} {unit}'` |
| `[workbench.colors]` | `[colors]` |
| the colour role `stale` | `outdated` |
| `{collection}`, in `[collection] files` and `[export.*] output` | `{project}` |

`samplekit DIR validate` names the first it meets, with its line: run it
again after each rename. A configuration written for a build made before rc.1
may hold other keys those builds read and this version does not:
`[format.NAME]`, a style's `quantity`, `unit` or `property_exact`, a query's
`base`: each is refused the same way, and the [configuration
reference](../reference/configuration.md) gives the keys there are.

- **A project reads its subfolders unless it says otherwise**: a
  `.samplekitrc` with no `recursive` now reads every folder below its own. A
  project that kept samples it did not want read in a subfolder writes
  `recursive = false` under `[collection]`.

### Python scripts

- **A property's formulas are renamed**, in this order, so that the second
  does not catch what the first wrote: `compute=` becomes `compute_value=`,
  then `compute_quantity=` becomes `compute=`. `compute=` is now the
  property's formula, returning a number or `(value, uncertainty)`;
  `compute_value=` gives the value alone, beside an uncertainty typed, given
  by readings or by `compute_uncertainty=`. Renamed this way, no value is
  outdated. A value formula left as `compute=` still runs, but the property
  is then the formula's whole: an uncertainty typed beside it is no longer
  kept, and a `compute=` given with `uncertainty=` is refused.
- **A table's columns are properties**: `sk.Column(` becomes
  `sk.Property(`, and the index is named, `sk.Table(columns, index="strain")`.
  An entry of `compute_rows` filling one column becomes that column's
  `compute=` and `depends_on=`, `("stress", ["row.force", "area"], f)`
  giving `"stress": sk.Property(unit="MPa", compute=f, depends_on=["row.force",
  "area"])`; an entry of `compute_columns` likewise its `compute_column=`. An
  entry filling several columns becomes a `sk.ComputedColumns(["low",
  "high"], compute=f, depends_on=[…])` in the table's `computed=`, or
  `compute_column=f` for one of `compute_columns`. The functions are
  unchanged, and no value is outdated.
- **Reading computes nothing**: where the model declares a statistic of
  readings, `width.value` is `None` until `compute()` takes it, as the
  command line shows it.
- `stale()` and `is_stale` are `outdated()` and `is_outdated`.
- `Property.invalidate()` is gone: `compute(force=True)` gives a value back to
  its formula. `Property.text` is `str()`.

### A history kept by rc.1

rc.1 named each computer's branch in a project's history after its host; this
version names it after an identifier drawn once, kept in the state folder's
`machine` file. Nothing is renamed: the first write of this version joins the
branch rc.1 kept, and `log` shows rc.1's snapshots as *SampleKit*'s, the name
rc.1 signed them with, and that write as *you, joins HOST*. Nothing is lost.

- **A history shared through git under rc.1** committed files each computer
  keeps for itself, and the model's description, `.samplekit/model.json`.
  Take them out of the repository once, and declare the sharing, `[history]
  shared = true`, as [Share a project](share-a-project.md) says:
  `git rm -r --cached .samplekit/history/HEAD .samplekit/history/config
  .samplekit/history/description .samplekit/history/hooks
  .samplekit/model.json`, then `git add -A`.
- **A value restored from a snapshot rc.1 took**, by `restore --at` or `--at`
  a snapshot from before the upgrade, carries no record of the formula that
  computed it, and is taken as current: compute it again,
  `compute -p NAME --rerun --write`, if its formula changed since.

### Clean the state folder

The state folder holds what SampleKit keeps on this computer alone:
`$XDG_STATE_HOME/samplekit`, or `~/.local/state/samplekit`, on Linux;
`~/Library/Application Support/samplekit` on macOS;
`%LOCALAPPDATA%\samplekit` on Windows; `$SAMPLEKIT_STATE_DIR` where it is
set. This version keeps there its run logs, `logs/compute-PROJECT-….log`,
the workbench's state, `workbench/`, and the computer's `machine`. What rc.1
and the builds before it left there is read by nothing, and can be deleted:
`computed.json`, `computed`, `consent`, `accepted/`, and the logs named by
run, `logs/compute-YYYY-MM-DDT…Z-PID.log`. The migration tool lists them,
`samplekit-migrate.pyz --clean-state`, and removes them with `--write`,
nothing else of the folder.

## From SampleKit 0.2

This recipe moves samples written by SampleKit 0.2 into files this version
reads, by hand: what [the migration tool](#the-migration-tool) does, for those
who would rather do it themselves or check what it did. The program itself
converts nothing. Either way, the formulas of the model are yours to read
again: the tool copies them as they were, marked.

**Take your time over the model and the configuration**: write them, and try
them on copies of a few samples, before the collection's files are moved.
**Then move the files together**, everyone who shares them: SampleKit 0.2
cannot read a moved file, and a colleague still on 0.2 who opens one gets
errors, or values that are silently empty.

### Find the files to move

This version knows its own files alone: it does not recognise a file an older
version wrote, nor say that it is one. A sample of SampleKit 0.2 declares no
`schema_version`, so it is **no sample** here, left out as a README is; no
command reads it, and none writes it. `samplekit samples/ validate` counts
them, *this file declares no schema_version*, with the first one's name: one
run finds them all.

### Move the samples

1. **Install both versions**, each in its own Python environment, since two
   versions of one package cannot share one: `python -m venv old` and
   `python -m venv .venv`. SampleKit 0.2 is on the Python Package Index:
   `old/bin/pip install samplekit==0.2.0`; this one is
   `.venv/bin/pip install samplekit==VERSION`, the version `samplekit
   --version` says. **Install the command line** too ([Install
   SampleKit](install.md)): it is what reads, checks and computes
   the new files. The new environment is `.venv/`, beside the project, and
   the project's `.samplekitrc` names it, `python = ".venv"` under `[model]`:
   `compute` runs the model with that Python, and looks for no other.
2. **Write the new model**: its class, its properties and its formulas, as
   this version declares them ([write a model](first-model.md)). Then
   `samplekit init --model model/specimen.py --write` writes the project's
   `.samplekitrc`, naming the model and the `.venv/` already there.
3. **Load the samples with the old version** and its old model, and write what
   they hold to a JSON file, which both versions read.
4. **Read the JSON with the new version**, set the values on new samples, and
   **save** them, into a new folder, never over the originals.

**Carry everything a sample holds**: each property's value, uncertainty,
readings and unit, its text values, and its tables. A copy
that keeps only the values gives files in which `validate` finds nothing
wrong, since every number it kept is right. Compare a few files by hand before
trusting the rest.

```python
# ── in the old environment, with SampleKit 0.2 and the old model ──
import json
from pathlib import Path

import old_model   # the module that declares your 0.2 class

carried = []
for path in sorted(Path("samples").glob("*.md")):
    sample = old_model.Specimen(path)
    properties, attributes, tables = {}, {}, {}
    # 0.2 held everything as a Property: a number is a property here, and
    # anything else, text or a date, an attribute. A value never given is
    # carried as nothing.
    for name, held in sample.props.items():
        if held.value is None:
            continue
        if isinstance(held.value, bool) or not isinstance(held.value, (int, float)):
            attributes[name] = held.value
        else:
            # Readings are `data` in 0.2: carried, or the new file holds
            # their mean and nothing it was taken from.
            readings = getattr(held, "data", None)
            properties[name] = {"v": held.value, "u": held.uncertainty,
                                "unit": held.unit,
                                "readings": list(readings) if readings else None}
    # Each table as its rows, each row its cells by column.
    for name, table in sample.tables.items():
        tables[name] = table.to_yaml()
    carried.append({"file": path.name, "name": sample.name,
                    "properties": properties, "attributes": attributes,
                    "tables": tables})
Path("carried.json").write_text(json.dumps(carried, indent=1, default=str))
```

```python
# ── in the new environment, with the new SampleKit and the new model ──
import datetime
import json
from pathlib import Path

import new_model
import samplekit as sk


def restored(value):
    """A date the JSON holds as its text, a date again: saved as text, a date
    is refused, since the file would read it back as a date."""
    if isinstance(value, str):
        try:
            return datetime.date.fromisoformat(value)
        except ValueError:
            pass
    return value


Path("new-samples").mkdir(exist_ok=True)
for held in json.loads(Path("carried.json").read_text()):
    target = Path("new-samples") / held["file"]
    # Run again after an interruption, it goes on where it stopped.
    if target.exists():
        continue
    fresh = new_model.Specimen.new(target)
    fresh.name = held["name"]
    for name, numbers in held["properties"].items():
        declared = getattr(fresh, name)
        # What the new model computes is left to it: copied, each value
        # would be a value typed over its formula, which it never replaces.
        if declared.is_computed:
            continue
        if numbers["unit"]:
            declared.unit = numbers["unit"]
        if numbers["readings"]:
            # The readings, and the statistic the model declares takes the
            # value from them: a value set over them is typed over its formula.
            declared.readings = numbers["readings"]
        else:
            declared.value = numbers["v"]
            declared.uncertainty = numbers["u"]
    for name, value in held["attributes"].items():
        setattr(fresh, name, restored(value))
    # Each table's rows, a cell 0.2 wrote with its uncertainty given with it.
    for name, table in held["tables"].items():
        rows = [{column: sk.Property(value=cell.get("value"),
                                     uncertainty=cell.get("uncertainty"))
                 if isinstance(cell, dict) else cell
                 for column, cell in row.items()}
                for row in table["_rows"]]
        if rows:
            getattr(fresh, name).extend(rows)
    fresh.save()
```

Tables are carried in the JSON as 0.2 wrote them, and their rows added to the
tables the new model declares; a column's precision and symbol, which 0.2
wrote in each file, are declared once, `[property."curve.force"]`.
Readings are carried with their property, and the statistic the new model
declares gives the value from them. **What the new model computes is not
carried**: `compute --write` computes it, from the values carried. **0.2 had no tags and no note of your
own**: the text below its frontmatter was written by `template()` from the
values when the file was last saved. The migration tool keeps it as the note,
as it was; this recipe carries nothing of it: copy what you want to keep, and
write the note the new files should have. Then:

```console
$ samplekit new-samples/ validate
$ samplekit new-samples/ compute --write
```

### Scripts written for 0.2

- **`sort()` sorts the list in place and returns `None`**, as a Python list's
  `sort()` does: `ordered = specimens.sort("area")` leaves `ordered` at
  `None`. Write `ordered = specimens.sorted("area")` for a sorted copy.
- **`stats()` gives a weighted mean** where every uncertainty is known and
  none is 0, each value weighted by 1/u²: a script of 0.2 gets another
  number. Its `weighted` says which mean it took, and `standard_error` is
  the weighted mean's, 1/√Σw, beside it.
- **`invalidate()` is gone**: what is outdated is known by itself now, and
  `compute(force=True)` gives a value written over its formula back to it.
- **The names 0.2 had and this version has not** are unknown attributes or
  keywords, answered as any typing mistake, with the nearest name:

  | 0.2 | Now |
  | --- | --- |
  | `s.get_property("x")`, `s.get_table("x")` | `s["x"]`, or `s.x` |
  | `s.set_property("x", p)`, `s.set_table("x", t)` | `s["x"] = p` |
  | `s.has_property("x")` | `"x" in s` |
  | `s.remove_property("x")`, `s.remove_table("x")` | `del s["x"]` |
  | `s.set_value(…)`, `set_uncertainty`, `set_unit`, `set_symbol` | `s.x.value = …`, `.uncertainty`, `.unit`, `.symbol` |
  | `s.property_names()`, `s.table_names()` | `list(s)`, or `s.keys()` |
  | `Sample.load(path)`, `load_data` | `Sample(path)` |
  | `s.notes` | `s.note` |
  | `s.render_view()`, `render_report()` | `samplekit FILE view` |
  | `samples.append(…)` and the other changes to a list | `samples + other`, `SampleList([...])` |
  | `samples.names()` | `[s.name for s in samples]` |
  | `samples.query_names()` | `samples.project.queries` |
  | `samples.sort_multi(…)`, `sort(…, reverse=True)` | `sort(["first", "-second"])` |
  | `samples.to_records()`, `to_dataframe()` | `samples.to_dict(columns=[...])` |
  | `table.cell(i, c)`, `table.column(c)` | `table.at(i, c)`, `table.values(c)` |
  | `p.data` | `p.readings` |
  | `p.unit_math`, `symbol_math`, `precision_unc` | `[unit]` and `[property]` in `.samplekitrc` |
  | `p.is_valid` | `p.state != "failed"`, or `p.failure`, the error it raised; reading `p.value` gives its last value, with a warning |
  | `Property(compute_unc=…)` | `compute_uncertainty=` |
  | `sk.load(…, sample_class=Specimen)` | `Specimen(path)` |
- **Readings give their value once computed**: where the model declares a
  statistic, `width.value` is `None` until `compute()` takes it, as the
  command line shows it.

### What 0.2 wrote in each file, and this version declares once

- **Digits, symbols and LaTeX spellings**, 0.2's `precision`,
  `precision_unc`, `symbol_math` and `unit_math`, written into every file,
  are declared once for the project, `[property.width] precision = ".4f"`,
  `symbol_math = …`, and `[unit."mm²"] math = …`, in `.samplekitrc`
  ([configuration](../reference/configuration.md)). A file holds none of them.
- **The LaTeX forms need their style**: `symbol_math` and `math` are what
  `--style math` writes, and nothing without it. Declare the style once,
  `[style.math]`, and write a table in it: `samplekit samples/ --style math`.
- **Numbers shown to a digit apart**: at the same precision, a few values and
  uncertainties are shown differently from 0.2: `1.0401` where 0.2 showed
  `1.0400` for 1.04005, `3.54 ± 0.000 %` where it showed no uncertainty of
  0, `—` where it showed `N/A`. The numbers the files hold are the same: this
  version rounds a value to the nearest, a half up, and an uncertainty up,
  never understated, on their shortest decimal writing ([Values and
  uncertainty](../concepts/values-and-uncertainty.md)), where 0.2 used
  Python's `format()`.
- **The uncertainty of readings**: 0.2 gave the readings' sample standard
  deviation. [Write a model](first-model.md) declares their standard error,
  √n times smaller; to keep 0.2's numbers, declare
  `uncertainty=sk.stats.sample_stdev`.

### Keys renamed since 0.2

A 0.2 file spelt a property's numbers `value` and `uncertainty`, and its
repeated measurements `data`. They are `v`, `u` and `readings` now, and the
old spellings are not read. The recipe above needs no change for it:
`save()` writes the new ones. A file that already says `schema_version: 1` and
still holds a long spelling is refused for it, *unknown field `value`*: rename
the key.

### What needs nothing

**A file written differently from SampleKit's own form** is valid, and is
read. The next command that writes it rewrites it in the fixed form;
`validate` notes it, and there is nothing to run.
