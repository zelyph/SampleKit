# Step 4 · Computing

In this step a **model** computes some values from others — the modulus from
the two hardnesses, for one — and SampleKit tells you which computed values are
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
path = "model/piece.py"
class = "Piece"
# The environment the model runs in, with samplekit installed in it: every
# project with a model names one.
python = "../.venv"
```

`model/piece.py` is a Python class. It declares each property, and how the
computed ones are computed (shortened here):

```python
class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times: the value is the mean of the readings,
        # the uncertainty their standard error.
        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.notch = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)

        # Computed: `compute` gives the property, a value or a value with
        # its uncertainty; each says below what it reads.
        self.modulus = sk.Property(unit="%", compute=self._modulus)
        self.reduction = sk.Property(unit="%", compute=self._reduction)
        self.recovery = sk.Property(unit="%", compute=self._recovery)
        self.set_dependencies("modulus", depends_on=["radius", "notch"])
        self.set_dependencies("reduction", depends_on=["radius", "notch"])
        self.set_dependencies("recovery", depends_on=["radius", "volume", "ore_mass"])

    def _reduction(self):
        return 100.0 * (self.radius.value - self.notch.value) / (self.radius.value - 1.0)
```

`compute` returns the property: a number, as `reduction`'s, or a value and
its uncertainty together, as `modulus`'s, propagated from the hardnesses'. Where the
value and the uncertainty have a formula each, `compute_value` gives the value
and `compute_uncertainty` the uncertainty.

The hardnesses are now **read three times** each, and the files keep every
reading: `radius: {readings: [1.064, 1.065, 1.063]}`. Readings have a value only
because the model says which statistic of them is the value: here their mean,
and their standard error for the uncertainty. SampleKit never takes a mean on
its own.

The pieces were left in the states a real notebook ends up in:

- the **blonde steel** was never computed — not even the mean of its
  hardnesses' readings, which a table shows as `—` until `compute` takes it;
- the **dry nickel**'s ore was weighed again after the computation, so its
  recovery is **outdated**;
- the **double TITANIUM**'s modulus was copied from its label by hand: an
  **override**, kept as written;
- the **gamma zinc** has no final hardness yet: its modulus **waits**.

## Try it

```console
$ samplekit pieces status                # what is not current, and why
$ samplekit pieces/dry-nickel.md explain recovery   # one of them, before computing
$ samplekit pieces compute               # what would run
$ samplekit pieces compute --write       # runs the model and writes the results
$ samplekit pieces --profile modulus
```

## What you see

`status` lists every computed value that is not current, with the reason:

```text
8 values are not current, and 2 wait for an input nobody entered

sample          value         state
──────────────────────────────────────────────────────────
blonde-steel   modulus           never computed
blonde-steel   reduction   never computed
blonde-steel   recovery    never computed
blonde-steel   notch            never computed
blonde-steel   radius            never computed
blonde-steel   volume        uncertainty never computed
double-titanium      modulus           edited since it was computed
dry-nickel       recovery    outdated — ore_mass
gamma-zinc   modulus           waits for notch
gamma-zinc   reduction   waits for notch

samplekit compute lists what would run; --write computes and writes
1 value edited by hand is kept — samplekit compute --force gives it back to its formula
```

The first `--write` in your copy of the demo also says that it takes the
values the files hold as computed by the model as it is now: a copy comes
without the history that records which formula computed them, and SampleKit
starts that record here, once.

`compute` alone lists what would run, then asks `compute these values?
[y/N]`; computed, it shows before and after and asks `apply these changes?
[y/N]`. `--write` computes and writes without asking. Each run first names the
model file it uses, and with `-v` the configuration too. A value waiting for an input nobody
entered is not a failure: it runs once you enter `notch`. The override is kept.

`explain` shows one value, what it was computed from, and the state of each
input:

```text
dry-nickel · recovery = 70 %

  computed from
    ore_mass       4.30 kg            entered
    radius               1.043 ± 0.0003     current
    volume           20.5 ± 0.3 L       current

  this value is outdated — ore_mass
```

After `compute --write`, the same command says *this value is current*, at
67 %.

In a table in the terminal, a value that is not current is marked `⚠`, a value
written over its formula `✎`, and a formula that failed `✗`.

## A reading corrected

```console
$ samplekit pieces/alpha-titanium.md set notch.readings=1.011,1.010,1.011 --write
$ samplekit pieces/alpha-titanium.md status
$ samplekit pieces/alpha-titanium.md compute --write
```

The preview of `set` already says what the new readings make not current: `notch`
itself, whose mean must be taken again, and `modulus` and `reduction`, which
read it. `compute` then runs them in the order the inputs require.

## An override given back

```console
$ samplekit pieces/alpha-titanium.md set modulus=7.2              # a preview: an override
$ samplekit pieces compute --force --write               # every override back to its formula
```

A computed value you write by hand becomes an **override**: `compute` leaves
it alone, and `status` lists it as *edited since it was computed*, until
`--force` gives it back to its formula.

## The history

Every `--write` is kept, as a snapshot, in `.samplekit/` beside
`.samplekitrc`: on your computer only, with no git needed.

```console
$ samplekit pieces/alpha-titanium.md set notch.readings=1.012,1.012,1.013 --write
$ samplekit pieces/alpha-titanium.md compute --write
$ samplekit log                         # the snapshots, newest first
$ samplekit diff                        # the last change, value by value
$ samplekit pieces/alpha-titanium.md restore  # a preview of taking it back
```

```text
from  #2 · 2026-09-27 17:29 · samplekit pieces/alpha-titanium.md set notch.readings=1.012,1.012,1.013 --write
to    #1 · 2026-09-27 17:29 · samplekit pieces/alpha-titanium.md compute --write

pieces/alpha-titanium.md
  notch            1.011 ± 0.0003  →  1.012 ± 0.0003
  modulus           7.0 ± 0.1 %  →  6.7 ± 0.1 %
  reduction   83 %  →  81 %
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

Next: [Step 5 · Ageing](../05-ageing/README.md), where a piece holds
tables.
