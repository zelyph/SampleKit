# Write a model

This guide is for adding a **model** to a project: a Python class that says
what a sample holds, which values are computed, from what, and how.
SampleKit then computes those values, and knows when one no longer holds.
The [home page](../index.md#the-model-what-a-sample-holds) builds one step by
step on the tensile example; this guide gathers what writing your own takes.

## 1. The Python it runs in

A model runs in the Python environment that `[model] python` names in
`.samplekitrc`, where SampleKit's package is installed at the version of the
command line, and nowhere else. `samplekit init`, in the project's folder,
offers to make it, `.venv/`, when it is missing ([start a
project](start-a-project.md)). Whatever your formulas import, numpy or scipy,
is installed in that same environment: `.venv/bin/pip install scipy`.

## 2. Name it

`.samplekitrc` names the model's file and its class:

```toml
[model]
path = "model/specimen.py"
class = "Specimen"
python = ".venv"
```

Both paths read from `.samplekitrc`'s folder. A project set up by `init` with
a model already has this section, and a `model/main.py` that runs as it is
and says in comments how to declare each kind of value. A model file you
already have is named where it is, `samplekit init --model PATH`, without
`class`: the one subclass of `sk.Sample` it defines is used. A model may
import modules of its own beside it, a `helpers/` package for instance.

## 3. Declare what a sample holds

In the class's `__init__`, each property is an `sk.Property`, by kind:

| Kind | Declared as |
| --- | --- |
| measured once, typed in each file | `sk.Property(unit="mm")` |
| measured several times | `sk.Property(unit="mm", value=sk.stats.mean, uncertainty=sk.stats.standard_error)` |
| computed from others | `sk.Property(unit="mm²", compute=self._area, depends_on=["width", "thickness"])` |
| computed, value and uncertainty apart | `compute_value=` and `compute_uncertainty=` |
| a table, its columns | `sk.Table({"strain": sk.Property(unit="%"), …}, index="strain")` |
| a column computed row by row | `sk.Property(unit="MPa", compute=self._stress, depends_on=["row.force", "area"])` |
| a text each file gives, read by a formula | named in the formula's `depends_on`; `self.material = "steel"` gives a default |

The home page shows each in place: [readings and their
statistics](../index.md#readings-and-their-statistics), [values computed from
others](../index.md#values-computed-from-others), [a table inside a
sample](../index.md#a-table-inside-a-sample). Two rules hold for every
computed value:

- **It says what it reads**, with `depends_on`. That list is how SampleKit
  knows the value is out of date when an input changes: it cannot see what a
  formula reads, and a name left out is a change never noticed. A formula
  that reads nothing of the sample says `depends_on=[]`.
- **Readings have a value only through the statistic you choose.** Without
  `value=`, readings have no value, and nothing is computed from them
  ([readings and statistics](../concepts/readings-and-statistics.md)).

A formula returns a number, or a value and its uncertainty as a pair; it
propagates uncertainties only as you write it. Every keyword is in the
reference of {py:class}`sk.Property <samplekit.Property>`. The model can also
draw figures and run scripts, methods marked `@sk.figure` and `@sk.script`
([figures](../index.md#drawn-by-the-model), [scripts](../index.md#scripts)).

## 4. Declare their digits

A file keeps every digit a formula gives, `25.05586444444445`; what tables,
exports and figures show is the precision the project declares. Declare one
for each value a formula gives, or a table shows about six significant
digits and an export every digit:

```toml
[property.area]
precision = ".2f"
```

A value with an uncertainty is shown to its uncertainty, and needs none
([precision](../concepts/values-and-uncertainty.md#precision-how-many-digits)).

## 5. Compute

`status` lists what is not current and why; `compute` alone shows what it
would compute and asks, then shows the results and asks again; `--write`
computes and writes without asking. Here, after a width measured again:

<!-- run: tensile/02-readings -->
```console
$ samplekit samples/copper-01.md set width.readings=12.511,12.497,12.505 --write
samples/copper-01.md · 1 change

  width.readings   12.509, 12.498, 12.506  →  12.511, 12.497, 12.505
                   3 readings: 12.511, 12.497, 12.505, their mean 12.5043
                   they replace the 3 readings it held
                   its value and its uncertainty, their mean and standard error, wait for samplekit compute

  this makes 1 value not current
    width   outdated — readings

  samplekit samples/copper-01.md compute --write recomputes them

written: samples/copper-01.md
$ samplekit status
1 value is not current

sample      value   state
───────────────────────────────────────
copper-01   width   outdated — readings

samplekit compute lists what would run; --write computes and writes
$ samplekit compute --write
model  model/specimen.py
  ✓ copper-01     width

copper-01
value                before                 after
─────────────────────────────────────────────────
width   12.5043 ± 0.0033 mm   12.5043 ± 0.0041 mm

computed 1 value in 1 sample
written: samples/copper-01.md
```

Only what depends on the change is computed. A formula edited in the model
makes what it computed *formula changed*, and `compute --write` computes it
again; `compute --rerun` runs every formula, whatever is current, and says
what would change.

**In the terminal interface**, `C` shows what is not current, of the basket
(the samples `Space` put aside) or of the samples shown, and computes it on
`y`; `c` on a value computes it again, even current; `v` shows every value not current, with its reason ([work in the
terminal interface](terminal-interface.md)). **From Python**, `compute()` on
a sample or a list computes what is not current, `not_current()` says what
is, and `save()` writes ([work from Python](work-from-python.md)).

## 6. When a formula cannot run

A value whose input nobody entered **waits**, and is said so; a formula that
raises **fails**, its Python traceback shown and the rest computed;
`samplekit FILE explain FIELD` says, for one value, what it was computed from
and why it is in its state ([when a formula cannot
run](../index.md#when-a-formula-cannot-run), [models and
formulas](../concepts/models-and-formulas.md)).

## An example to copy from

`samplekit example init --example --write` writes a small project in a
folder `example/`, from the folder you run it in: run it beside your
project, not inside it, so that the two stay apart. It has one formula of
each kind, working, and the commands to try.

## See also

- [Models and formulas](../concepts/models-and-formulas.md): where a model
  runs, what it may print, and why every formula says what it reads.
- [Current and outdated](../concepts/current-and-outdated.md).
- [The Python API](../reference/python.md), every keyword.
