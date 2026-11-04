# Work from Python

This guide is for reading, analysing and changing your samples from Python: a
script of your own, a notebook, or a few lines typed at the prompt. The
`samplekit` package reads the same files as the command line, with the same
names and the same model, and changes them the same way: nothing is written
until you save.

The examples run in the last step of the [home page's](../index.md) tensile
example, whose `scripts/` folder holds them.

## The Python to use

The package is installed in the project's Python environment, the one `[model]
python` names in `.samplekitrc`: `.venv/` beside the samples, which `samplekit
init` makes ([install SampleKit](install.md#the-python-package)). Run a script
with that Python, `.venv/bin/python script.py`, or activate the environment
first, `source .venv/bin/activate`, after which `python` is that one; on
Windows, `.venv\Scripts\activate`. A notebook uses it as its kernel. Another
Python says *No module named 'samplekit'*.

## Reading

`sk.Sample(path)` reads one sample with its project's model; `sk.load(folder)`
reads a folder, as the command line does, and gives a `SampleList`. A
property is a value, an uncertainty and a unit; `.value` is a number to
compute with. Any field the command line reads is read by its address too,
`sample["curve.stress[10]"]`:

```{literalinclude} ../../examples/tensile/04-results/scripts/read.py
:language: python
:caption: scripts/read.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/read.py
steel-01 steel
25.056 ± 0.053 mm²
25.05586444444445 0.05217296159913354 mm²
[1.995, 2.004, 2.008] 2.0023333333333335
504.3130732135541
[0, 0.025, 0.05, 0.2]
['copper-01', 'steel-01']
26.666666666666668
{'name': ['aluminium-01', 'copper-01', 'steel-01'], 'tensile_strength_value': [318.0726127367075, 221.0919691412931, 517.8428398975834], 'tensile_strength_uncertainty': [None, None, None]}
```

- `filter`, `sorted` and `query` take what `-f`, `-s` and `--query` take, and
  give a new list.
- `stats` gives the statistics `--summary` gives: of a property's readings, or
  of a field over a list.
- `to_dict` gives each column as a list, as `--csv` names it. A value missing
  is `None`, and pandas reads the dict as it is, `pd.DataFrame(…)` (pandas is
  not installed with SampleKit: `pip install pandas` in the environment).
- `to_csv`, `to_json` and `to_text("latex")` write a table as the command line
  writes it, with the precision the project declares.
  A sample's own table has its `to_csv` too, `steel.curve.to_csv()`, its rows
  as `view curve --csv` prints them.

In a notebook, `dir(sample)` and `help(sample)` list what an object offers; a
mistake raises a standard exception naming the nearest name: `steel.aera` is
an `AttributeError` that says `area`.

## Changing, computing, saving

A sample changes as an object does: `steel.width.readings = [12.49, 12.53,
12.52]`, `steel.grade = "S355J2"`, `steel.ductility = None` to remove an
attribute. `not_current()` says what the change made out of date, `compute()`
runs the model's formulas, and `save()` writes the file. Nothing is written
before it:

```{literalinclude} ../../examples/tensile/04-results/scripts/remeasure.py
:language: python
:caption: scripts/remeasure.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/remeasure.py
{'width': 'outdated', 'area': 'outdated', 'young_modulus': 'outdated', 'yield_strength': 'outdated', 'tensile_strength': 'outdated', 'curve.stress': 'outdated'}
25.056 ± 0.054 mm² 518 MPa
```

A write is safe as a command's is: it refuses a file someone changed since it
was read. The files a script saved are kept in the project's history as one
change when it ends, with the script beside them; `sk.keep("message")` keeps
what was saved so far as a change of its own, so that the history shows the
script's steps apart ([the history](../concepts/history.md)).

`sk.Sample.new("samples/steel-02.md")` makes a sample where none is; set its
fields, then `save()`.
`steel.note = "…"` replaces the note below the data, written by `save()`.
`steel.remove()` deletes the file, which the history keeps: `samplekit
restore` gives it back. `steel.rename("steel-04")` renames it, its own files
and the links of the project leading to it. An attribute that links to another sample,
`reference: "[[aluminium-01]]"` or `"[the plate](../plates/P3.md)"`, is read as
text, and `steel.linked("reference")` reads the sample it names, the path of
a file that is no sample, a list of links a list
([link samples](link-samples.md#6-from-python)).

## Figures and exports

`sk.plot(samples, "curves", output="curves.pdf")` draws a figure, declared or
the model's, as `plot` does; with `x=` and `y=` it draws one from two fields.
`samples.export("paper")` writes a declared export, a table or a figure, as
`samplekit export paper --write` does. Unlike `--write`, neither replaces a
file that exists: each raises `FileExistsError`, and `overwrite=True`
replaces it. Both are recorded in the history, so that `explain` finds the
file again.

## A script of the model

A function you run yourself is a script of your own. A method of the model
marked `@sk.script` is a script of the project: `samplekit run` runs it on the
samples selected, previews what it changes, and the TUI offers it ([run a
script of the model](run-a-script.md)).

## See also

- [The Python API](../reference/python.md), every class and method.
- [Values and uncertainty](../concepts/values-and-uncertainty.md): `None`
  for a value nobody entered, `sk.NA` for one that does not apply.
