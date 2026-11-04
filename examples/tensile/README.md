# SampleKit's demo: a tensile test

Three metal specimens pulled until they break, in five steps: from their
sizes written by hand to their results, tables, figures and two projects. It
is the example SampleKit's home page walks through, a step a folder: each
folder works on its own, and its `README.md` says what it holds, the commands
to try, and where the home page explains them.

| Step | What it holds |
| --- | --- |
| [1 · Measured](01-measured/README.md) | three specimens, their sizes written by hand |
| [2 · Readings](02-readings/README.md) | each size measured three times, a model taking the mean |
| [3 · Curve](03-curve/README.md) | the test itself: a curve of force against strain, the stress |
| [4 · Results](04-results/README.md) | what the curve gives, tables for a paper, figures, scripts |
| [5 · Projects](05-projects/README.md) | rolled and cast specimens, two projects read together |

Run each step's commands from its folder. Reading, selecting and exporting
need nothing but `samplekit`. Computing, figures and scripts, from step 2 on,
run with the Python of the `.venv/` folder here, above the steps.

**Where this demo comes from.** SampleKit's start page, `samplekit` alone,
then `g` and *The demo*, writes it into SampleKit's data folder the first
time you choose it, makes that `.venv/` with SampleKit's Python package in
it, and opens step 1. After that, *The demo* is a page of its steps: its
number, or `Enter`, opens one, and `q` in a step comes back to that page.
*Reset the demo*, at its foot, writes it again as it came. In SampleKit's
repository it is `examples/tensile/`: there, copy it elsewhere and make the
`.venv/` yourself, so that your changes stay out of the repository:

```console
$ python -m venv .venv
$ .venv/bin/pip install samplekit==VERSION     # what samplekit --version says
```

The whole documentation is at
[zelyph.github.io/SampleKit](https://zelyph.github.io/SampleKit/latest/index.html).
