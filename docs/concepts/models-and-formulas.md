# Models and formulas

Some values come from others: a cross-section from a width and a thickness,
a stress from a force and that cross-section, a modulus fitted over a table's
columns. In
SampleKit such values are computed by **your own Python formulas**, gathered
in a project's **model**. This page says what a model is, how SampleKit runs
it, and what happens when a formula cannot run.

## A model is a Python class

A project names its model in `.samplekitrc`:

```toml
[model]
path = "model/specimen.py"
class = "Specimen"
python = ".venv"
```

The model is a subclass of `samplekit.Sample`. Its `__init__` declares each
property of the project and, for the computed ones, the formula that gives
them:

```python
import math

import samplekit as sk

class Specimen(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.width = sk.Property(unit="mm", value=sk.stats.mean,
                                 uncertainty=sk.stats.standard_error)
        self.thickness = sk.Property(unit="mm", value=sk.stats.mean,
                                     uncertainty=sk.stats.standard_error)
        self.area = sk.Property(unit="mm²", compute=self._area,
                                depends_on=["width", "thickness"])

    def _area(self):
        w, t = self.width, self.thickness
        value = w.value * t.value
        return value, math.hypot(t.value * w.uncertainty, w.value * t.uncertainty)
```

It is ordinary Python: it can import other files beside it and any package
installed in its environment, such as numpy or scipy.

**One model per project.** The properties and formulas of one kind of
sample are rarely those of another, so a project has one model and another
kind is another project ([Projects](projects.md)). Two projects can share
one model file, by pointing `path` at it.

**A project without a model works.** Every value is then typed in the files.
Reading, filtering, sorting, checking and exporting never need Python; only
computing and drawing figures do.

## One formula per value

Each computed value has its formula, declared where the value is:

| Declaration | The formula gives |
| --- | --- |
| `sk.Property(compute=f)` | the property; `f()` returns a number, or `(value, uncertainty)` |
| `sk.Property(compute_value=f)` | the value alone, beside an uncertainty given apart |
| `sk.Property(compute_uncertainty=f)` | the uncertainty, beside a value you enter or compute apart |
| `sk.Property(value=sk.stats.mean, uncertainty=…)` | both, as statistics of the readings ([Readings and statistics](readings-and-statistics.md)) |
| `sk.Property(compute=f, depends_on=[…])`, as a table's column | the column, one row at a time; `f(row)` returns a number, or `(value, uncertainty)` |
| `sk.Property(compute_column=f, depends_on=[…])`, as a table's column | the whole column at once; `f(columns)` returns one cell per row |
| `sk.ComputedColumns([…], compute=f, depends_on=[…])`, in a table's `computed` | several columns at once |

Why a formula per value rather than one function computing everything: so
that SampleKit can tell which value is out of date, compute only that one,
and know which values a changed formula affects. A collection can take hours
to compute; recomputing only what is outdated is what makes that bearable.

Write each formula as a **method** of the class (`self._area`). A `lambda`
that uses `self` keeps every sample in memory as long as your script runs.

### Formulas over a table

A table's columns are declared with `sk.Property`, as the sample's
properties are, and a column's formula is declared on it in the same words.
It can be computed in two ways:

- **Row by row**, `sk.Property(compute=f, depends_on=["row.force", "area"])`:
  the formula runs once per row, receives the row, can read the sample
  through `self`, and returns the cell. Use it when a cell depends only on
  its own row, and on the sample's properties. `compute_value` and
  `compute_uncertainty` give one number of each cell, as for a property: an
  instrument rated at 0.5 % of its reading is `sk.Property(unit="kN",
  compute_uncertainty=f, depends_on=["row.force.v"])`, the value entered in
  each row and its uncertainty computed from it.
- **The whole column at once**, `sk.Property(compute_column=f,
  depends_on=["curve.force", "curve.strain"])`: the formula receives the
  columns it reads, by name, and returns one cell per row. Use it when a row's
  value depends on other rows: a slope from one row to the next, the energy
  absorbed so far.

A formula that fills several columns at once, the two parameters of a fit,
is declared on the table, `computed=[sk.ComputedColumns(["low", "high"],
compute=f, depends_on=[…])]`, and returns a dict, one cell (or one list of
cells, with `compute_column=`) for each column it fills.

A property can also read a whole column: a fit over a table's rows, the
highest stress of a curve. It declares the columns it reads (`curve.stress`),
and a change to any cell of them makes it outdated.

## Every formula says what it reads

**A computed value declares its inputs**, with `depends_on`:

```python
self.area = sk.Property(unit="mm²", compute=self._area,
                        depends_on=["width", "thickness"])
# or, later in __init__:
self.set_dependencies("area", depends_on=["width", "thickness"])
```

This is how SampleKit knows that `area` is outdated when `width` changes, and
in which order to compute: `width` before `area`. A model that forgets a declaration
is refused before any formula runs, with a message naming the value.

- A formula that reads nothing of the sample, a constant scale for example,
  declares `depends_on=[]`.
- A formula that reads the value beside its own uncertainty names the value
  channel: `depends_on=["gauge_length.v"]`.
- A property with two formulas reading different inputs gives one list per
  part: `depends_on={"v": ["width", "thickness"], "u": ["width", "thickness",
  "area.v"]}`.
- A table's formulas name their inputs in their `depends_on`
  (`"row.force"`, `"area"`, `"curve.force"`), and need no
  `set_dependencies`.

**SampleKit trusts the declaration.** It cannot see what a formula actually
reads. If `area` reads `thickness` but declares only `width`, a change to
`thickness` does not make `area` outdated, and nothing warns you. This is the one rule
that no check can enforce for you: declare every value a formula reads.

## Where the model runs

The `samplekit` command line is a program with no Python inside. To compute,
it starts a Python interpreter in a separate process, and talks to it:

**the Python `[model] python` names** in `.samplekitrc`: an environment
folder, `python = ".venv"` (its `bin/python`, or `Scripts\python.exe` on
Windows, so that one file serves both), or an interpreter itself. `samplekit
init` makes the project's `.venv/` and writes that line. **Nothing is searched
for**: a `.venv/` in a folder above, which belongs to another project or
which a colleague's clone would not have, is never used, and a project that
declares no interpreter computes nothing and says the line to write.

The SampleKit Python package must be installed there, **in the same version
as the command line**; a different version is refused, naming both.
`samplekit --show-rc FOLDER` prints which configuration, model and Python a
command would use.

From a Python script, `compute()` runs the formulas in the script's own
process instead, with the same rules.

Nothing about the model is written into the samples. Which version of each
formula computed which value is kept in the project's history, which travels
with the project ([Current and outdated](current-and-outdated.md)).

### The model's description

Each time Python imports the model, SampleKit writes down what it declares in
`.samplekit/model.json`, beside `.samplekitrc`: every property with its unit,
which ones a formula computes and what each formula reads, which take their
value from a statistic of readings, the tables with their columns and the
rows the model gives them itself, the attributes and the figures, with a
fingerprint of the model's files.

That description is what lets most commands answer **without starting
Python**: `status` and a `state` filter list what the model owes each sample,
`new` and `set` know that a name is one of the model's properties and its
unit, `list figures` names the model's figures, and the TUI marks what is
never computed. When you edit the model, the fingerprint no longer matches;
the next of those commands starts Python once to describe the model again,
and the ones after it start none. A description that no longer matches the
model is never used.

You never write or edit this file, and it does not belong in git: it is made
again from the model whenever it is needed.

## Running without asking

SampleKit runs a project's model **without asking**, as `python` runs a
script you give it: a model is code you wrote or chose. What it does instead
is say what it runs. Every computation begins by naming the model file, and
with `-v` the configuration before it:

```text
model  model/specimen.py
```

and computing is always in three steps you choose between:

| Command | What it does |
| --- | --- |
| `samplekit FOLDER compute` | lists what would run and why, then asks `compute these N values? [y/N]` |
| answered `y` | computes and shows every result, then asks `apply these changes? [y/N]` |
| `samplekit FOLDER compute --write` | computes the values and writes them, asking nothing |

Answering `n` to the second question is how you watch a new or edited model
run before it touches a file. The answers are read from the standard input:
`yes | samplekit FOLDER compute` answers both, and a script given no input
answers no, writing nothing. In the TUI, `C` shows what is not current and computes it once you answer
`y`, and `c` computes the value under the cursor; in Python, `compute()` then `save()`.

What runs by default is what is **not current**, in the order the inputs
require: values never computed, outdated values, values whose formula
changed. `-p NAME` narrows it to named values and the inputs they need;
`--rerun` adds the current ones; `--force` adds the values typed by hand over
their formula ([Current and outdated](current-and-outdated.md)).

**An interrupted run loses only the value in progress**: Ctrl+C stops it,
and every value finished before is written, those of the sample under way
among them. The next `compute` starts from what is still not current.

## When a formula cannot run

### A missing input: the value waits

If an input is **absent**, nobody entered it yet, the formula is not run and
the value **waits**: `area · waits for thickness`. This is not a failure. It is
the normal state of a sample halfway through its measurements; once you enter
`thickness`, the next `compute` runs `area`. A waiting value does not fail
`status --exit-code`.

If an input is **`n/a`**, the question does not apply, and the value is `n/a`
too, without running the formula
([Values and uncertainty](values-and-uncertainty.md)).

### An error: the value failed

A formula that raises an error **does not stop the run**: the other values
are computed. The failed value keeps the last value it gave, and the file
records the failure by the error's type:

```yaml
young_modulus:
  v: 206.33891963549178
  u: 1.0599559413170074
  unit: GPa
  computed: {curve.strain: ed9e87a6f01a, curve.stress: 82a1e45e269c}
  fingerprint: {failed: ZeroDivisionError}
```

Here the fit of the modulus had two rows left below 0.05 % of strain, and
divided by zero.

The full traceback goes to `.samplekit/failures/` in the project;
`samplekit FILE explain VALUE` prints it, and so does `Enter` on the value in
the TUI. The command exits with code 2. In Python, `compute()` finishes the
other values and then raises the first failure, naming the others. Reading a
failed value gives its last value, with a warning that it failed.

**A formula that never returns** is stopped by `compute --timeout SECONDS`:
its value fails as `TimedOut`, and the others run: in a continuous
integration, a formula caught in a loop no longer holds the job until its
limit. Without `--timeout`, a formula runs as long as it takes, and `Ctrl+C`
stops the run.

When one command covers several projects and one of them cannot run at all
(its model missing, say), the error names that project and the others are
still computed.

### The model cannot be read

If the model file is missing, there is no Python environment, or the model
raises as it is imported, `status` and `compute` say so, and give what the
files alone can tell: outdated, edited
and failed values are known from the files; the values the model has
**never computed** are known only to the model, and are said to be unknown
rather than left out in silence.

## What the model prints

The model runs in its own process during `compute`, so its output does not
clutter the listing:

- `print(...)` and `logging` below warning go to a log file, whose path is
  printed at the end of the run;
- a **warning** (`log.warning(...)`, `warnings.warn(...)`) is shown at the
  end of the run, with its sample and value; several are counted in one line,
  which `-v` turns into the list of them: that is how
  a formula tells you something from inside a run;
- `--show-output` shows every line as it comes.

A formula reads nothing either: an `input()` left in it fails, since a
computation runs with nobody before it. A script of the model may ask, in
the terminal or in the TUI's window ([run a
script](../guides/run-a-script.md)).

## Common mistakes

- **Forgetting an input in `depends_on`.** The value is never marked outdated
  when that input changes. Declare everything a formula reads.
- **Expecting a formula to propagate uncertainties by itself.** It gives what
  you return; return an uncertainty if you want one.
- **Installing SampleKit in another environment than the project's.** The
  command line uses the one `[model] python` names; `--show-rc` says which.
- **Taking a waiting value for a failure.** It waits for an input nobody
  entered; enter it.
- **Running `--write` on a new model straight away.** Run `compute` alone
  first, and answer `n` when it asks to apply.

## Where to go next

- A model, step by step: [Write a model](../guides/first-model.md).
- Practised on the [home page](../index.md#values-computed-from-others), for
  properties and [for tables](../index.md#a-table-inside-a-sample).
- `sk.Property`, `sk.Table`, `sk.ComputedColumns`, `sk.stats`: [Python
  reference](../reference/python.md); `compute` and its options: [Command line
  reference](../reference/cli.md); `[model]`: [Configuration
  reference](../reference/configuration.md).
