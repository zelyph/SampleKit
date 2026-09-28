# Models and formulas

Some values come from others: an alcohol content from two gravities, a
brewhouse efficiency from a gravity, a volume and a grain mass, a slope fitted over a table's column. In
SampleKit such values are computed by **your own Python formulas**, gathered
in a project's **model**. This page says what a model is, how SampleKit runs
it, and what happens when a formula cannot run.

## A model is a Python class

A project names its model in `.samplekitrc`:

```toml
[model]
path = "model/brew.py"
class = "Brew"
```

The model is a subclass of `samplekit.Sample`. Its `__init__` declares each
quantity of the project and, for the computed ones, the formula that gives
them:

```python
import samplekit as sk

class Brew(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.abv = sk.Property(unit="%", compute_quantity=self._abv,
                               depends_on=["og", "fg"])

    def _abv(self):
        value = (self.og.value - self.fg.value) * 131.25
        return value, 0.1
```

It is ordinary Python: it can import other files beside it and any package
installed in its environment, such as numpy or scipy.

**One model per project.** The quantities and formulas of one kind of
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
| `sk.Property(compute=f)` | the value; `f()` returns a number |
| `sk.Property(compute_uncertainty=f)` | the uncertainty, beside a value you enter |
| `sk.Property(compute_quantity=f)` | both; `f()` returns `(value, uncertainty)` |
| `sk.Property(value=sk.stats.mean, uncertainty=…)` | both, as statistics of the readings ([Readings and statistics](readings-and-statistics.md)) |
| `sk.Table(…, compute_rows=[…])` | table cells, one row at a time |
| `sk.Table(…, compute_columns=[…])` | a whole table column at once |

Why a formula per value rather than one function computing everything: so
that SampleKit can tell which value is out of date, compute only that one,
and know which values a changed formula affects. A collection can take hours
to compute; recomputing only what is outdated is what makes that bearable.

Write each formula as a **method** of the class (`self._abv`). A `lambda`
that uses `self` keeps every sample in memory as long as your script runs.

### Formulas over a table

A table's columns can be computed in two ways:

- **Row by row**, `compute_rows`: the formula runs once per row, receives the
  row, and can read the sample through `self`. Use it when a cell depends only
  on its own row, and on the sample's properties.
- **The whole column at once**, `compute_columns`: the formula receives the
  table's input columns and returns one value per row. Use it when a row's
  value depends on other rows: a rate from one day to the next, a running
  sum.

A property can also read a whole column: a fit over a table's rows, a mean of
the tasters' scores. It declares the columns it reads (`tasting.score`), and a
change to any cell of them makes it outdated.

## Every formula says what it reads

**A computed value declares its inputs**, with `depends_on`:

```python
self.abv = sk.Property(unit="%", compute_quantity=self._abv, depends_on=["og", "fg"])
# or, later in __init__:
self.set_dependencies("abv", depends_on=["og", "fg"])
```

This is how SampleKit knows that `abv` is outdated when `og` changes, and in
which order to compute: `og` before `abv`. A model that forgets a declaration
is refused before any formula runs, with a message naming the value.

- A formula that reads nothing of the sample, a constant scale for example,
  declares `depends_on=[]`.
- A formula that reads the value beside its own uncertainty names the value
  channel: `depends_on=["volume.v"]`.
- A property with two formulas reading different inputs gives one list per
  part: `depends_on={"v": ["og"], "u": ["og", "abv.v"]}`.
- A table's formulas name their inputs in their `compute_rows` and
  `compute_columns` entries (`"row.gravity"`, `"og"`,
  `"fermentation.gravity"`), and need no `set_dependencies`.

**SampleKit trusts the declaration.** It cannot see what a formula actually
reads. If `calories` reads `fg` but declares only `og`, a change to `fg` does
not make `calories` outdated, and nothing warns you. This is the one rule
that no check can enforce for you: declare every value a formula reads.

## Where the model runs

The `samplekit` command line is a program with no Python inside. To compute,
it starts a Python interpreter in a separate process, and talks to it:

- by default, the Python of **the nearest `.venv/` folder above the
  samples**: the project's own environment, which `samplekit init` makes;
- or the interpreter `[model] python` names.

The SampleKit Python package must be installed there, **in the same version
as the command line**; a different version is refused, naming both.
`samplekit --show-rc FOLDER` prints which configuration, model and Python a
command would use.

From a Python script, `compute()` runs the formulas in the script's own
process instead, with the same rules.

Nothing about the model is written into the samples. Which version of each
formula computed which value is recorded on your computer, in SampleKit's own
state folder ([Current and outdated](current-and-outdated.md)).

### The model's description

Each time Python imports the model, SampleKit writes down what it declares in
`.samplekit/model.json`, beside `.samplekitrc`: every property with its unit,
which ones a formula computes and what each formula reads, which take their
value from a statistic of readings, the tables with their columns and the
rows the model gives them itself, the attributes and the figures — with a
fingerprint of the model's files.

That description is what lets most commands answer **without starting
Python**: `status` and a `state` filter list what the model owes each sample,
`new` and `set` know that a name is one of the model's quantities and its
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
is say what it runs. Every computation begins by naming the configuration and
the model file:

```text
configuration  …/04-computing/.samplekitrc
model          …/04-computing/model/brew.py (class Brew)
```

and computing is always in three steps you choose between:

| Command | What it does |
| --- | --- |
| `samplekit compute FOLDER` | lists what would run and why; **runs no formula** |
| `samplekit compute FOLDER --try` | computes and shows every result; **writes nothing** |
| `samplekit compute FOLDER --write` | computes and writes each sample as soon as it is done |

`--try` is how you watch a new or edited model run before it touches a
file. In the TUI, `C` computes what is not current and `c` the value under the
cursor; in Python, `compute()` then `save()`.

What runs by default is what is **not current**, in the order the inputs
require: values never computed, outdated values, values whose formula
changed. `-p NAME` narrows it to named values and the inputs they need;
`--rerun` adds the current ones; `--force` adds the values typed by hand over
their formula ([Current and outdated](current-and-outdated.md)).

Because each sample is written as soon as it is done, **an interrupted run
loses only the sample in progress**. Ctrl+C stops it; the next `compute`
starts from what is still not current.

## When a formula cannot run

### A missing input: the value waits

If an input is **absent**, nobody entered it yet, the formula is not run and
the value **waits**: `abv · waits for fg`. This is not a failure. It is the
normal state of a sample halfway through its measurements; once you enter
`fg`, the next `compute` runs `abv`. A waiting value does not fail
`status --exit-code`.

If an input is **`n/a`**, the question does not apply, and the value is `n/a`
too, without running the formula
([Values and uncertainty](values-and-uncertainty.md)).

### An error: the value failed

A formula that raises an error **does not stop the run**: the other values
are computed. The failed value keeps the last value it gave, and the file
records the failure by the error's type:

```yaml
efficiency:
  v: 67.07439846974715
  computed: {grain_mass: 24be04b9eefb, og: a8ea7995583e, volume: 34b353a78c5c}
  fingerprint: {failed: ZeroDivisionError}
```

The full traceback goes to `.samplekit/failures/` in the project;
`samplekit explain FILE VALUE` prints it, and so does `Enter` on the value in
the TUI. The command exits with code 2. In Python, `compute()` finishes the
other values and then raises the first failure, naming the others. Reading a
failed value gives its last value, with a warning that it failed.

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
- a **warning** (`log.warning(...)`, `warnings.warn(...)`) is shown during
  the run, with its sample and value: that is how a formula tells you
  something while it runs;
- `--show-output` shows every line as it comes.

## Common mistakes

- **Forgetting an input in `depends_on`.** The value is never marked outdated
  when that input changes. Declare everything a formula reads.
- **Expecting a formula to propagate uncertainties by itself.** It gives what
  you return; return an uncertainty if you want one.
- **Installing SampleKit in another environment than the project's.** The
  command line uses the nearest `.venv/`; `--show-rc` says which.
- **Taking a waiting value for a failure.** It waits for an input nobody
  entered; enter it.
- **Running `--write` on a new model straight away.** Run `--try` first.

## Where to go next

- A first model, step by step: [Write a first model](../how-to/first-model.md).
- Practised in tutorial steps [4](../tutorial/04-computing.md) (properties)
  and [5](../tutorial/05-fermentation.md) (tables).
- `sk.Property`, `sk.Table`, `sk.Column`, `sk.stats`: [Python
  reference](../reference/python.md); `compute` and its options: [Command line
  reference](../reference/cli.md); `[model]`: [Configuration
  reference](../reference/configuration.md).
