# Run a script of the model

This guide is for a task you repeat on samples, written once in the model:
an attribute filled from a value, a reading corrected, a report printed, a
sample made from others. A **script** runs from the command line or the
terminal interface, on the samples you select, and what it changes is shown
before it is written.

The examples are the scripts of the [home page's](../index.md#scripts)
tensile example, in its last step.

## 1. Mark a method `@sk.script`

In the model:

```python
class Specimen(sk.Sample):
    # … its properties and formulas …

    @sk.script
    def classify(self, limit: float = 20.0):
        """Class the specimen's ductility by its elongation at break."""
        self.ductility = "high" if self.elongation.value >= limit else "low"

    @sk.script
    @classmethod
    def ranking(cls, samples, by: str = "tensile_strength"):
        """Print the specimens from the highest value of a field to the lowest."""
        for rank, specimen in enumerate(samples.sorted(f"-{by}"), start=1):
            print(rank, specimen.name, specimen[by])
```

- **A method acts on one sample**, and runs on each sample selected in turn.
  Placed above `@classmethod`, it receives the **selection**, a list of
  samples, once.
- **Its keyword parameters are the command's options**, given with `--set`:
  typed by their annotation, `int`, `float`, `bool` or `str`, or by their
  default. A parameter with no default must be given.
- **Its docstring's first paragraph is its description.**

`list scripts` lists them, with their options:

<!-- run: tensile/04-results -->
```console
$ samplekit list scripts
classify   on each sample
    Class the specimen's ductility by its elongation at break.
    --set limit=NUMBER   20.0 by default
ranking    on the selection
    Print the specimens from the highest value of a field to the lowest.
    --set by=TEXT        tensile_strength by default
```

## 2. Run it

`run` runs a script on the samples a command reads: a folder, files, `-f`, a
query. What it changes in samples, a value set, an attribute, a sample saved
or made, is shown, then written with `--write`, or on `y` when it asks:

<!-- run: tensile/04-results -->
```console
$ samplekit run classify --set limit=15

run classify · 3 files changed

samples/aluminium-01.md
  ductility   —  →  low
samples/copper-01.md
  ductility   —  →  high
samples/steel-01.md
  ductility   —  →  high

  nothing becomes not current

apply these changes? [y/N] n
nothing written
```

What it writes is one change of the project's history, which `restore` takes
back. A file someone changed while the script ran is refused, not
overwritten. What the script writes itself, a figure or a CSV file, is its
own: neither shown nor kept in the history.

## 3. What it prints, and what it returns

What a script prints is shown as it prints it:

<!-- run: tensile/04-results -->
```console
$ samplekit run ranking
1 steel-01 518 MPa
2 aluminium-01 318 MPa
3 copper-01 221 MPa
```

A script may ask with `input()`. On the command line the question is asked in
the terminal, as Python itself would ask it, and the preview lists each
answer, `asked Factor?  0.5`, before anything is written; the history keeps
them with the command, `samplekit run scale --write — answered 'Factor?'
0.5`. With no terminal to answer, in a pipe or a scheduled job, the script
fails and says so: give it its values with `--set` instead. In the terminal
interface the script runs in a window that shows what it prints as it
prints it, and the question is answered on the line at its foot.

What it returns is shown too, each kind as the command line shows it:

| It returns | Shown as |
| --- | --- |
| a text | the text |
| a property | its value, in `--style`'s forms or `[render] style`'s |
| a table | the table, as `samplekit view` unfolds it |
| a sample | the sample, as `samplekit view` shows it |
| a list of samples | their files' paths, one per line |
| a summary | a row as `--summary` writes it |
| a list of these | each, in order |
| anything else | its text, `str()` |

## 4. From Python

A script is a method of the model, and Python calls it as one: on a sample
for a method, on the model's class with a list for a class method. Nothing is
previewed then, and what it changes is written by `save()`, as any change
from Python:

```{literalinclude} ../../examples/tensile/04-results/scripts/model_scripts.py
:language: python
:caption: scripts/model_scripts.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/model_scripts.py
aluminium-01 low
copper-01 high
steel-01 high
1 copper-01 44.0 %
2 steel-01 24.0 %
3 aluminium-01 12.0 %
```

## 5. In the terminal interface

`p` runs a script on the sample open, on the basket, or on the samples shown:
its menu lists the scripts first, then the figures and the exports, `Tab`
from one group to the next. Choose the script and change its options; it
runs in a window that shows what it prints, where a question it asks is
answered on the last line. Then read what it changed before `y` writes it; `u` takes it back ([work in the
terminal interface](terminal-interface.md)).

## See also

- [Work from Python](work-from-python.md): a script of your own, outside the
  model.
- [`run`, `list scripts`](../reference/cli.md); [`sk.script`](../reference/python.md#scripts).
