# Step 8 · Python

In this step you read the pieces of step 6 from Python, with the same rules as
the command line: two scripts, then a few lines typed by hand. A script can
also change values, compute and save.

Run every command from this folder, `08-python`, with the Python of the
`.venv/` above the steps, where SampleKit's Python package is installed.
Activate it first — `source ../.venv/bin/activate`, or
`..\.venv\Scripts\activate` on Windows — and `python` is that one; without
it, `python` is another Python, which says *No module named 'samplekit'*.
Called by a path through `..`, `../.venv/bin/python`, Python 3.14 warns that
its prefix is not what it expected: activating the environment avoids it.

## Try it

```console
$ python scripts/piece_report.py
$ python scripts/process_comparison.py
```

- `piece_report.py` prints what is not current, the three best pieces, and
  writes the season to `season.csv` — without `reduction`'s uncertainty,
  which no piece has, as the warning says: a column empty for every sample is
  left out of a file;
- `process_comparison.py` compares reduction and hardness drop by process, and
  draws a figure the project does not declare, `processes.png`.

```text
piece_report.py:26: UserWarning: reduction_uncertainty is left out: it is empty for every sample
gamma-zinc: modulus, reduction, score

blonde-steel       44.8 ± 0.5   6.1 %
english-bronze        44.7 ± 0.9   4.8 %
alpha-titanium           42.7 ± 0.9   6.8 %

wrote season.csv
```

## By hand

Start `python` in this folder, and type:

```python
>>> import samplekit as sk
>>> piece = sk.Sample("pieces/alpha-titanium.md")       # read with its project's model
>>> piece.modulus
Property(value=6.825000000000035, uncertainty=0.14510233457805027, unit='%')
>>> piece.modulus.value
6.825000000000035
>>> piece["ageing.hardness[3]"]               # any field the command line reads
1.021
>>> pieces = sk.load("pieces")                      # a folder: a list of samples
>>> pieces.filter("style == titanium").sorted("-modulus")   # as -f and -s
>>> print(pieces.to_csv(columns=["name", "modulus"]))  # as --csv
```

`dir(piece)` and `help(piece)` list what an object offers. A mistake raises a
standard exception with the nearest name: `piece.modluus` is an `AttributeError`.

`to_dict` gives each column as a list, which pandas reads as it is. A value
that does not apply, `n/a`, is `None` there, which pandas reads as `NaN`
(pandas does not come with samplekit: `pip install pandas` in the
environment), and a sample's tags are a list, which `explode` turns into one
row per tag:

```python
>>> import pandas as pd
>>> frame = pd.DataFrame(pieces.to_dict(columns=["name", "notch", "tags"]))
>>> frame.explode("tags").groupby("tags")["notch_value"].mean()
```

## Changing, computing, saving

```python
>>> piece = sk.Sample("pieces/dry-nickel.md")
>>> piece.ore_mass.value = 4.5
>>> piece.not_current()
{'recovery': 'outdated'}
>>> piece.compute()            # what is not current, in your own Python
>>> piece.save()               # writes the file
```

Reading never computes: a value that is not current is returned as the file
holds it, with a warning. `save()` refuses to overwrite a file that changed
since it was read. A script's saves are kept as one snapshot of the project's
history, with the script itself:

```console
$ python scripts/process_comparison.py
$ samplekit log
```

The script wrote a figure and saved no piece, and it is still a snapshot, since
the figure came from it; `samplekit log --script 1` prints the script as it ran.

## Why

The command line answers the questions you ask often; Python answers the one
you have not asked yet. Both read the same files by the same rules, so a
script never disagrees with a table.

- To understand: [the history](https://zelyph.github.io/SampleKit/latest/explanations/history.html),
  and what a script leaves in it.
- To look up: [the Python package](https://zelyph.github.io/SampleKit/latest/reference/python.html).

This is the last step. The [how-to guides](https://zelyph.github.io/SampleKit/latest/how-to/index.html)
start your own project: [moving existing data in](https://zelyph.github.io/SampleKit/latest/how-to/move-existing-data.html),
[a first model](https://zelyph.github.io/SampleKit/latest/how-to/first-model.html).
