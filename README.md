# SampleKit

**Sample data in plain Markdown: computed, current, tracked.**

Which value was computed from which measurement? Was this table made before or
after the thickness was measured again? Is the figure in the draft still
right? SampleKit answers these questions for a folder of plain text files:
one sample, one Markdown file, readable in any editor, in Obsidian, and in
git.

```yaml
---
schema_version: 1
name: copper-01
material: copper
compared_with: "[[steel-01]]"
properties:
  width: {readings: [12.509, 12.498, 12.506], unit: mm}
  thickness: {readings: [1.995, 1.988, 2.007], unit: mm}
---
Tested at 2 mm/min, across the rolling direction.
```

Above the second `---` are the sample's data: attributes, values with their
uncertainties and units, readings, tables of measurements. Below is a note
that is yours, kept as you write it. A folder of such files is a project,
with no database and nothing to set up.

## What it does

- **Computed**: the values that derive from your measurements, an area, a
  modulus, a whole column of stress, come from formulas you write in Python,
  uncertainties included.
- **Current**: SampleKit knows which results still rest on the measurements
  they were computed from, and says which do not, and why.
- **Tracked**: every change it writes is kept in a history of the project's
  own, with the command that made it, and no git to install. `--at` reads the
  project as it was, and makes a table or a figure again exactly as it was.
- **Tables and figures**: select, sort and group samples, then write a CSV, a
  LaTeX or Markdown table, or a matplotlib figure, in one style for the whole
  project, by one command you run again at each draft.
- **Linked**: a sample names another, a file or a web page, as Obsidian and
  Markdown write a link; SampleKit follows it both ways, checks it, and keeps
  it when a sample is renamed.
- **Three ways in**: a command line, a full-screen terminal interface, and a
  Python package reading the same files with the same names.

## A taste

From the last step of the demo, three tensile specimens:

```console
$ samplekit -c name,material,tensile_strength -s -tensile_strength   # a table, sorted
$ samplekit -f 'elongation > 20'                  # the specimens a filter selects
$ samplekit status                                # what is not current, and why
$ samplekit compute --write                       # run the project's Python model
$ samplekit export paper --write                  # a LaTeX table for a paper
$ samplekit samples/steel-01.md view curve --csv  # a sample's curve, as CSV
$ samplekit samples/steel-01.md log               # every change kept, and what made it
$ samplekit                                       # the terminal interface, full screen
```

```python
import samplekit as sk

for specimen in sk.load("samples").filter("elongation > 20"):
    print(specimen.name, specimen.young_modulus.value, specimen.young_modulus.uncertainty)
```

## Install

The command line is one file: download it for Linux, macOS or Windows from
[the releases](https://github.com/zelyph/SampleKit/releases), or build it with
Rust:

```console
$ cargo install samplekit --locked --version VERSION
```

The Python package goes into each project's `.venv/`, at the version of the
command line, `VERSION` being what `samplekit --version` says; `samplekit
init` makes that environment and installs it for you:

```console
$ pip install samplekit==VERSION
```

> **Until 1.0, name the version.** `pip install samplekit` alone gives 0.2.0,
> the previous version, whose API is not this one; likewise the *Latest*
> release on GitHub is 0.2.0, and this version's archives are under its own
> release, marked *pre-release*.

[Install SampleKit](https://zelyph.github.io/SampleKit/latest/guides/install.html)
says more, system by system. **From an earlier version**, 0.x or 1.0.0-rc.1,
`samplekit-migrate.pyz`, in each release, carries a project into a new
folder this version reads, and says what to check by hand
([upgrade](https://zelyph.github.io/SampleKit/latest/guides/upgrade.html)).
It never writes in the folder it reads; back your project up all the same.

## Try the demo

`samplekit` alone opens its start page; `g`, then *The demo*, writes a
tensile test in five steps, from three specimens measured by hand to their
curves, results, a paper's tables and figures, and opens the first.
[The documentation](https://zelyph.github.io/SampleKit/latest/) walks
through it, then goes further: guides a task each, the concepts, and the
reference.

## Where things are

|                                                                                                 |                                                                                                                                                                                                                                                                                                  |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [`docs/`](https://zelyph.github.io/SampleKit/latest/index.html)                                 | The documentation: the home page's walkthrough of the demo, [guides](https://zelyph.github.io/SampleKit/latest/guides/index.html), [concepts](https://zelyph.github.io/SampleKit/latest/concepts/index.html) and the [reference](https://zelyph.github.io/SampleKit/latest/reference/index.html) |
| `src/`, `tests/`                                                                                | The Rust crate: the core, the command line, the terminal interface and the Rust half of the Python package; a test file per source file                                                                                                                                                          |
| `python/samplekit/`                                                                             | The Python half of the Python package                                                                                                                                                                                                                                                            |
| [`examples/tensile/`](https://github.com/zelyph/SampleKit/blob/main/examples/tensile/README.md) | The demo, in five steps, generated from `fixtures/tensile/`; the start page writes it for you                                                                                                                                                                                                    |
| `fixtures/tensile/`                                                                             | What the demo is generated from: its model, scripts and READMEs, and the generator                                                                                                                                                                                                               |
| [`migrate/`](https://github.com/zelyph/SampleKit/blob/main/migrate/README.md)                   | The migration tool, `samplekit-migrate.pyz` in each release                                                                                                                                                                                                                                      |

## Building

```console
$ cargo build --release                  # the command line: target/release/samplekit
$ python -m venv .venv && source .venv/bin/activate
$ pip install maturin
$ maturin develop --release              # the Python package, into .venv
$ tools/check.sh                         # everything a commit must pass
```

The documentation is a Sphinx site, built from `docs/` into `target/site`,
after `maturin develop`, since the Python reference reads the package:

```console
$ .venv/bin/pip install -r docs/requirements.txt
$ .venv/bin/sphinx-build docs target/site
```

The command line needs no Python to read, select, render, check or export;
Python runs a project's model, and matplotlib draws its figures.

## Status

Version `1.0.0-rc.2` (`1.0.0rc2` as Python spells it), the second release
candidate of `1.0.0`. It follows `0.2.0`, the previous implementation, in
Python alone, published under the same name. What changed is in
[CHANGELOG.md](https://github.com/zelyph/SampleKit/blob/main/CHANGELOG.md).

## Roadmap

In order of priority:

- [ ] **1.0**: this release candidate, tried, then published as `1.0.0`;
  from then on, `pip install samplekit` gives it without naming a version.
- [ ] **Designs of experiments**: full and
  fractional factorials, central composite, Box-Behnken, definitive
  screening and mixture designs, with centre points, replicates, blocks and
  a randomised order. SampleKit lays out a plan's runs, makes a sample of
  each, takes in the samples already made, says which runs are still to
  make, and analyses what they measured: a model per response, reduced and
  tested term by term, its residuals examined, then used to predict,
  optimise and draw; in the terminal interface and from Python too.
- [ ] **An Obsidian plugin**, so that a project opened as a vault shows its
  properties and tables as SampleKit does, hides what only SampleKit reads,
  such as `schema_version`, and edits them.

## Licence

MIT: see [LICENSE](https://github.com/zelyph/SampleKit/blob/main/LICENSE).
Keep the notice with any copy; everything else is permitted, including
commercial use.
