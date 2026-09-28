# A first model

This recipe adds a **model** to a project: a Python class that says which
values are computed, from what, and how. SampleKit then computes them, and
tells you when one is no longer current.

The example goes on from [Move existing data in](move-existing-data.md): brews
whose original and final gravities are each read three times with a
hydrometer, and whose volume is read on the fermenter's scale. The model
computes the alcohol, and from it the grams of alcohol in the batch.

## 1. A Python environment

The model runs with the Python of the nearest `.venv/` folder above your
samples, where SampleKit's Python package must be installed. In the project's
folder:

```console
$ samplekit init --write
```

In a project that exists, `init` only offers what is missing: here the
environment, with the package installed in it from the Python Package Index.

## 2. Name the model

In `.samplekitrc`:

```toml
[model]
path = "model/main.py"
class = "Model"
```

The path is relative to `.samplekitrc`. A project set up with a model by
`samplekit init` already has this section, and a `model/main.py` that
runs as it is and says in comments how to declare each kind of formula.
Where you already have a model file, `samplekit init`'s model question — *use
a model file I already have* — asks its path and writes this section naming
it where it is, without `class`: the one subclass of `sk.Sample` the file
defines is used, and `class` is needed only where it defines several.

## 3. Write the model

`model/main.py`:

```python
import samplekit as sk


class Model(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Measured. Each gravity is read three times: its value is the mean
        # of the readings, its uncertainty their standard error.
        self.og = sk.Property(value=sk.stats.mean,
                              uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean,
                              uncertainty=sk.stats.standard_error)
        self.volume = sk.Property(unit="L")

        # Computed, each saying what it reads.
        self.abv = sk.Property(unit="%", compute=self._abv,
                               depends_on=["og", "fg"])
        self.alcohol = sk.Property(unit="g", compute=self._alcohol,
                                   depends_on=["abv", "volume"])

    def _abv(self):
        # The usual home-brewing rule: 131.25 per unit of gravity lost.
        return (self.og.value - self.fg.value) * 131.25

    def _alcohol(self):
        # Ethanol weighs 0.789 g per mL.
        return 7.89 * self.abv.value * self.volume.value
```

Three things matter:

- **Every computed value says what it reads**, with `depends_on`. That list
  is how SampleKit knows that `abv` is outdated when `fg` changes. It
  cannot see what a formula actually reads: a name left out is a change
  never noticed. A formula that reads nothing of the sample says
  `depends_on=[]`. A computed value that says nothing is refused before
  anything runs.
- **Readings have a value only through a statistic you choose**:
  `value=sk.stats.mean, uncertainty=sk.stats.standard_error`. Without it, the
  gravities' readings have no value, and nothing is computed from them.
- `compute=` gives a value. `compute_uncertainty=` gives the uncertainty of a
  value you type, and `compute_quantity=` a value and its uncertainty at once,
  as a pair.

`new` and `set` read the names and units the model declares from its source,
without running it: `volume=20.5` is written as a quantity in L. A
`[property.*]` in `.samplekitrc`, as in [Move existing data
in](move-existing-data.md), still sets the digits, and its unit comes first.

## 4. Compute

```console
$ samplekit status samples              # what the model would compute, and why
$ samplekit compute samples --try       # computes, shows the results, writes nothing
$ samplekit compute samples --write     # computes and writes
$ samplekit samples -c name,og,fg,abv,alcohol
```

Then change an input and watch what follows:

```console
$ samplekit set samples/B-01.md fg.readings=1.011,1.012,1.011 --write
$ samplekit status samples
```

```text
3 values are not current

sample   value     state
──────────────────────────────────────────────────────
B-01     abv       outdated — fg
B-01     alcohol   outdated — abv (itself not current)
B-01     fg        outdated — readings
```

`compute --write` brings all three up to date, in the order the inputs require.

A model that fails to load, or a formula that raises, is reported with its
Python traceback; the other values are still computed. `samplekit explain
FILE VALUE` shows where one value came from.

## Try it on an example

`init --example` writes a small project — a brew, as in the demo — with one
formula of each kind, to read and copy from:

<!-- run: 02-measuring -->
```console
$ samplekit init --example example --no-venv --write
$ cd example
$ samplekit status samples
$ samplekit compute samples --try
$ samplekit compute samples --write
$ samplekit samples --profile overview
```

## See also

- The tutorial's [step 4](../tutorial/04-computing.md), a model at work, and
  [step 5](../tutorial/05-fermentation.md), formulas over tables.
- [A model and its formulas](../explanations/models-and-formulas.md),
  [current and outdated](../explanations/current-and-outdated.md),
  [readings and their statistic](../explanations/readings-and-statistics.md).
- [Everything a model can declare](../reference/python.md).
