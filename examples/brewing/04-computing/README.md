# Step 4 · Computing

In this step a **model** computes some values from others — the alcohol from
the two gravities, for one — and SampleKit tells you which computed values are
not current, and why. You compute them, look at where one came from, and
correct a reading.

Computing runs Python: this step, and every step after it, needs the Python
of the `.venv/` folder above the steps, with SampleKit's Python package in it.
The demo written by the TUI's start page has it; in a copy of the repository,
the [tutorial's first page](../README.md) says how to make it.

Run every command from this folder, `04-computing`.

## What changed

`.samplekitrc` names the model:

```toml
[model]
path = "model/brew.py"
class = "Brew"
```

`model/brew.py` is a Python class. It declares each quantity, and how the
computed ones are computed (shortened here):

```python
class Brew(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times: the value is the mean of the readings,
        # the uncertainty their standard error.
        self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)

        # Computed, each saying what it reads.
        self.abv = sk.Property(unit="%", compute_quantity=self._abv)
        self.attenuation = sk.Property(unit="%", compute=self._attenuation)
        self.efficiency = sk.Property(unit="%", compute=self._efficiency)
        self.set_dependencies("abv", depends_on=["og", "fg"])
        self.set_dependencies("attenuation", depends_on=["og", "fg"])
        self.set_dependencies("efficiency", depends_on=["og", "volume", "grain_mass"])

    def _attenuation(self):
        return 100.0 * (self.og.value - self.fg.value) / (self.og.value - 1.0)
```

The gravities are now **read three times** each, and the files keep every
reading: `og: {readings: [1.064, 1.065, 1.063]}`. Readings have a value only
because the model says which statistic of them is the value: here their mean,
and their standard error for the uncertainty. SampleKit never takes a mean on
its own.

The brews were left in the states a real notebook ends up in:

- the **blonde saison** was never computed — not even the mean of its
  gravities' readings, which a table shows as `—` until `compute` takes it;
- the **dry stout**'s grain was weighed again after the computation, so its
  efficiency is **outdated**;
- the **double IPA**'s alcohol was copied from its label by hand: an
  **override**, kept as written;
- the **smoked porter** has no final gravity yet: its alcohol **waits**.

## Try it

```console
$ samplekit status brews                # what is not current, and why
$ samplekit compute brews               # what would run
$ samplekit compute brews --write       # runs the model and writes the results
$ samplekit explain brews/dry-stout.md efficiency
$ samplekit brews --profile alcohol
```

## What you see

`status` lists every computed value that is not current, with the reason:

```text
10 values are not current

sample          value         state
──────────────────────────────────────────────────────────
blonde-saison   abv           never computed
blonde-saison   attenuation   never computed
blonde-saison   efficiency    never computed
blonde-saison   fg            never computed
blonde-saison   og            never computed
blonde-saison   volume        uncertainty never computed
double-ipa      abv           edited since it was computed
dry-stout       efficiency    outdated — grain_mass
smoked-porter   abv           waits for fg
smoked-porter   attenuation   waits for fg

samplekit compute lists what would run; --write computes and writes
1 value edited by hand is kept — samplekit compute --force gives it back to its formula
```

`compute` alone lists what would run and runs nothing; `--try` computes and
writes nothing; `--write` computes and writes. Each run first names the
configuration and the model file it uses. A value waiting for an input nobody
entered is not a failure: it runs once you enter `fg`. The override is kept.

`explain` shows one value, what it was computed from, and the state of each
input:

```text
dry-stout · efficiency = 70 %

  computed from
    grain_mass       4.30 kg            entered
    og               1.043 ± 0.0003     current
    volume           20.5 ± 0.3 L       current

  this value is outdated — grain_mass
```

In a table in the terminal, a value that is not current is marked `⚠`, a value
written over its formula `✎`, and a formula that failed `✗`.

## A reading corrected

```console
$ samplekit set brews/citra-ipa.md fg.readings=1.011,1.010,1.011 --write
$ samplekit status brews/citra-ipa.md
$ samplekit compute brews/citra-ipa.md --write
```

The preview of `set` already says what the new readings make not current: `fg`
itself, whose mean must be taken again, and `abv` and `attenuation`, which
read it. `compute` then runs them in the order the inputs require.

## An override given back

```console
$ samplekit set brews/citra-ipa.md abv=7.2              # a preview: an override
$ samplekit compute brews --force --write               # every override back to its formula
```

A computed value you write by hand becomes an **override**: `compute` leaves
it alone, and `status` lists it as *edited since it was computed*, until
`--force` gives it back to its formula.

## The history

Every `--write` is kept, as a snapshot, in `.samplekit/` beside
`.samplekitrc`: on your computer only, with no git needed.

```console
$ samplekit set brews/citra-ipa.md fg.readings=1.011,1.010,1.011 --write
$ samplekit compute brews/citra-ipa.md --write
$ samplekit log                         # the snapshots, newest first
$ samplekit diff                        # the last change, value by value
$ samplekit restore brews/citra-ipa.md  # a preview of taking it back
```

```text
from  #2 · 2026-09-27 17:29 · samplekit set brews/citra-ipa.md fg.readings=1.011,1.010,1.011 --write
to    #1 · 2026-09-27 17:29 · samplekit compute brews/citra-ipa.md --write

brews/citra-ipa.md
  fg            1.012 ± 0.0003  →  1.011 ± 0.0003
  abv           6.8 ± 0.1 %  →  7.0 ± 0.1 %
  attenuation   82 %  →  83 %
```

## Why

Each computed value records a short code of every input it read, so SampleKit
knows from the files alone when an input has changed since. A value you can
no longer trust is said to be not current, and never shown as if it were.

- To understand: [a model and its formulas](https://zelyph.github.io/SampleKit/latest/explanations/models-and-formulas.html),
  [current and outdated](https://zelyph.github.io/SampleKit/latest/explanations/current-and-outdated.html),
  [readings and their statistic](https://zelyph.github.io/SampleKit/latest/explanations/readings-and-statistics.html),
  [the history](https://zelyph.github.io/SampleKit/latest/explanations/history.html).
- To look up: [`status`, `compute`, `explain`, `log`, `diff`, `restore`](https://zelyph.github.io/SampleKit/latest/reference/cli.html),
  [what a model can declare](https://zelyph.github.io/SampleKit/latest/reference/python.html).
- To do: [a first model of your own](https://zelyph.github.io/SampleKit/latest/how-to/first-model.html).

Next: [Step 5 · Fermentation](../05-fermentation/README.md), where a brew holds
tables.
