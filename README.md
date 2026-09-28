# SampleKit

Sample data in plain Markdown: computed, current, tracked.

One sample is one `.md` file: a YAML frontmatter holding values, uncertainties,
units, readings and tables, and below it a note that is yours and that SampleKit
never touches. A folder of such files is a collection: a notebook that any
text editor and git can read, with no database and nothing to set up.
SampleKit computes what derives from what you measured, knows which results
are no longer current and why, and keeps every change.

```console
$ samplekit brews/ -c name,style,og,abv -s -abv       a table, sorted
$ samplekit brews/ -f 'abv > 6 && tags has medal'     the brews a filter selects
$ samplekit status brews/                             which derived values are not current, and why
$ samplekit compute brews/ --write                    run the project's Python model
$ samplekit export overview brews/                    a CSV a colleague can open
$ samplekit log brews/citra-ipa.md                    every change kept, and what made it
$ samplekit diff                                      the last change, value by value
$ samplekit                                           the TUI: all of it, full screen
```

Every change SampleKit writes is kept in a history of the project's own, with
no git to install: `log` and `diff` read it, `--at` reads the project as it
was, and an export or a figure made again that way is the file it was.

From Python, the same files and the same names:

```python
import samplekit as sk

for brew in sk.load("brews").filter("abv > 6"):
    print(brew.name, brew.abv.value, brew.abv.uncertainty)
```

## Installing

The command line is one file: download it for Linux, macOS or Windows from
[the releases](https://github.com/zelyph/SampleKit/releases), or build it with
`cargo install samplekit --locked --version VERSION`. The Python package goes
into each project's `.venv/`, at the version of the command line —
`pip install samplekit==VERSION`, `VERSION` being what `samplekit --version`
says; `samplekit init` makes that environment and installs it for you.

[The documentation](https://zelyph.github.io/SampleKit/latest/) starts with a
tutorial on a home-brewing demo, which `samplekit` alone, then `g` and *The
demo*, writes and opens.

## Where things are

| | |
| --- | --- |
| [`docs/`](https://zelyph.github.io/SampleKit/latest/index.html) | The documentation, in four parts: a [tutorial](https://zelyph.github.io/SampleKit/latest/tutorial/index.html) on the demo, [explanations](https://zelyph.github.io/SampleKit/latest/explanations/index.html), [how-to guides](https://zelyph.github.io/SampleKit/latest/how-to/index.html), and the [reference](https://zelyph.github.io/SampleKit/latest/reference/index.html) — the commands, the Python API, the TUI's keys, `.samplekitrc`, the file format, the exit codes |
| `src/`, `tests/` | The Rust crate: the core, the command line, the TUI and the extension's Rust half; a test file per source file |
| `python/samplekit/` | The Python half of the extension |
| [`examples/brewing/`](https://github.com/zelyph/SampleKit/blob/main/examples/brewing/README.md) | The demo the tutorial walks through, generated from `fixtures/brewing/`; the TUI's start page writes it for you |
| `fixtures/` | Collections to try it on, and what the demo is generated from |

## Building

```console
$ cargo build --release                  # the command line: target/release/samplekit
$ python -m venv .venv && source .venv/bin/activate
$ pip install maturin
$ maturin develop --release              # the Python package, into .venv
$ tools/check.sh                         # everything a commit must pass
```

The documentation is a Sphinx site, built from `docs/` into `target/site`:

```console
$ .venv/bin/pip install -r docs/requirements.txt
$ .venv/bin/sphinx-build docs target/site   # after maturin develop: the Python reference reads the package
```

The command line needs no Python to read, select, render, check or export; Python
is needed to run a project's model, and to draw a figure, which matplotlib
draws.

## Status

Version `1.0.0-rc.1` — `1.0.0rc1` as Python spells it — the first release
candidate of `1.0.0`, following the `0.2.0` the previous implementation, in
Python alone, reached under this name. What changed is in
[CHANGELOG.md](https://github.com/zelyph/SampleKit/blob/main/CHANGELOG.md).

## Licence

MIT — see [LICENSE](https://github.com/zelyph/SampleKit/blob/main/LICENSE). Keep the notice with any copy; everything else is
permitted, including commercial use.
