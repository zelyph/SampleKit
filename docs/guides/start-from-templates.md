# Start from templates

This guide is for a project of your own started from files rather than from
nothing: a `.samplekitrc`, a model, a sample and a script, each ready to fill
in, the places for your own choices marked in its comments with `← change`.
Together they make a project that validates and computes as downloaded; a
test of SampleKit builds one from them and computes it, so that they keep
working.

They are files to copy, not the demo: the demo, the home page's tensile
test, is in the command line already ([the guides](index.md)). `samplekit
init` sets a project up too, with a model that declares nothing yet ([start
a project](start-a-project.md)); the templates are the same project with one
value of each kind already declared.

## The configuration

A project's `.samplekitrc`: which files are samples and where a sample's own
files are, the model and its Python, the digits of two values, a query, a
profile, an export and a figure. Save it as `.samplekitrc`, with the leading
dot, in the project's folder. Every key is explained in [the configuration
reference](../reference/configuration.md).

```{literalinclude} ../templates/samplekitrc.toml
:language: toml
:caption: samplekitrc.toml
```

{download}`Download samplekitrc.toml <../templates/samplekitrc.toml>`

## The model

The class that says what a sample holds: two values measured several times,
their mean and standard error as value and uncertainty; an area computed
from them; a table `curve` whose index is `strain`, a column measured and a
column computed in each row; and a script with an option. Save it where
`[model] path` names it, `model/model.py`. [Write a model](first-model.md)
explains each kind.

```{literalinclude} ../templates/model.py
:language: python
:caption: model.py
```

{download}`Download model.py <../templates/model.py>`

## A sample

One sample of that model: two attributes, the readings of the two values
with their units, the table's rows, and a note below the second `---`. The
area and the column `stress` are not written in it: `compute` writes them.
Save it in `samples/` as `specimen-01.md`, the name it holds.

```{literalinclude} ../templates/sample.md
:language: markdown
:caption: sample.md
```

{download}`Download sample.md <../templates/sample.md>`

## A script

A Python script of your own, beside the model's: it reads the samples, calls
the model's script `classify` on each, and prints what it gives. Its
options, `--limit` and `--save`, are its own. Save it in `scripts/` and run
it from the project's folder with the project's Python, `.venv/bin/python
scripts/script.py --limit 150` ([work from Python](work-from-python.md)).

```{literalinclude} ../templates/script.py
:language: python
:caption: script.py
```

{download}`Download script.py <../templates/script.py>`

## Put them together

In a new folder, each file where the configuration expects it:

```text
.samplekitrc            samplekitrc.toml, renamed
model/model.py          model.py
samples/specimen-01.md  sample.md
scripts/script.py       script.py
```

Then, in that folder:

1. `samplekit init` offers what is missing, the Python environment `.venv/`
   with SampleKit's package in it, and writes it once you answer `y`.
2. `samplekit validate` checks every file. It notes that `photos` and `raw`,
   the folders `[collection] files` names, do not exist yet: make them, or
   change that line to your own folders.
3. `samplekit compute --write` computes the area and the column `stress`,
   and writes them in the sample.
4. `samplekit run classify --set limit=150` runs the model's script on each
   sample, shows what it changes and asks before writing it ([run a script
   of the model](run-a-script.md)).
5. `samplekit` opens the project in the [terminal
   interface](terminal-interface.md); `p` runs the same script there.

Then change what is marked `← change`: your values and their units in the
model, the same names in the sample and in the configuration's profile,
query and figure. `samplekit validate` says, with its line, a name that
refers to nothing.

## See also

- [Choose the colours](colours.md): a `[colors]` table for your terminal's
  theme, to add to the configuration.
- [Bring existing data in](move-existing-data.md): samples made from a
  spreadsheet or CSV files rather than written by hand.
