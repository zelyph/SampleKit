# SampleKit's tutorial: a home brewer's notebook

An invented home brewer's notebook, in eight steps: from three brews written
by hand to two projects with a model, tables, figures and Python. Each step is
a folder that works on its own, and its `README.md` is the tutorial's page for
that step: what you do, the commands to type, what you see, and why.

| Step | What you do |
| --- | --- |
| [1 · First brews](01-first-brews/README.md) | Read three brews written by hand |
| [2 · Measuring](02-measuring/README.md) | Units, uncertainties and tags; change a value |
| [3 · Selecting](03-selecting/README.md) | Filter, sort, name a query and a profile, export |
| [4 · Computing](04-computing/README.md) | A model: what is outdated, overridden, waiting; the history |
| [5 · Fermentation](05-fermentation/README.md) | Tables: the fermentation day by day, the tasting |
| [6 · Figures](06-figures/README.md) | Figures, and the styles that label them |
| [7 · Two brewers](07-two-brewers/README.md) | Two projects side by side; the TUI |
| [8 · Python](08-python/README.md) | The brews read from Python |

Run each step's commands from its folder. Reading, selecting and exporting
need nothing but `samplekit`. Computing and figures, from step 4 on, run with
the Python of the `.venv/` folder here, above the steps.

**Where this demo comes from.** SampleKit's start page — `samplekit` alone,
then `g` and *The demo* — writes it into SampleKit's data folder the first
time you choose it, makes that `.venv/` with SampleKit's Python package in it,
and opens step 1. After that, *The demo* is a page of its steps: its number,
or `Enter`, opens one, and `q` in a step comes back to that page. *Reset the
demo*, at its foot, writes it again as it came. In SampleKit's
repository it is `examples/brewing/`: there, copy it elsewhere and make the
`.venv/` yourself, so that your changes stay out of the repository:

```console
$ python -m venv .venv
$ .venv/bin/pip install samplekit==VERSION     # what samplekit --version says
```

The rest of SampleKit's documentation — how-to guides, explanations and the
reference — is at
[zelyph.github.io/SampleKit](https://zelyph.github.io/SampleKit/latest/index.html).
