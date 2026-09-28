# Tutorial

The tutorial is a demo project: a home brewer's notebook, invented for the
purpose. It grows in eight steps, from three brews written by hand to two
projects with a model, tables, figures and Python. Each step is a folder that
works on its own, and each page below is that folder's `README.md`: what you
do, the commands to type, what you see, and why.

Follow the steps in order the first time. This page installs SampleKit and
gets the demo.

## Install

SampleKit has two parts, built from the same source:

- the **command line**, `samplekit`, a program written in Rust. It reads,
  selects, checks and exports your samples, and holds the full-screen
  interface, the TUI;
- the **Python package**, `samplekit`. The command line uses it to run your
  formulas and draw figures, and your own scripts import it.

The command line is one file. Download the archive for your system from
[the releases](https://github.com/zelyph/SampleKit/releases) — Linux,
macOS (Intel or Apple silicon) or Windows — and put `samplekit` (or
`samplekit.exe`) in a folder on your `PATH`, `~/.local/bin` for instance:

On Linux or macOS:

```console
$ tar -xzf samplekit-x86_64-unknown-linux-gnu.tar.gz -C ~/.local/bin samplekit
$ samplekit --version
```

On macOS, a program downloaded by a browser is held back until you say it
may run: `xattr -d com.apple.quarantine ~/.local/bin/samplekit` says so.

On Windows, the archive is a `.zip`: extract `samplekit.exe` into a folder of
your own, `%LOCALAPPDATA%\Programs\samplekit` for instance, add that folder
to your `PATH` (*Edit environment variables for your account*), and open a new
terminal:

```console
> samplekit --version
```

With Rust installed, `cargo install samplekit --locked --version VERSION`
builds it instead, `VERSION` being the release's, `1.0.0-rc.1` for instance.

The Python package is not installed once for the whole machine: it goes into
**each project's own Python environment**, a `.venv/` folder in the project
(or above it). When SampleKit needs Python, it runs the Python of the nearest
`.venv/` above your samples. `samplekit init`, and the start page when it
writes the demo, make that environment and install the package into it from
the Python Package Index, at the version of the command line: the two are
one release, and go together. To do it yourself, with the version
`samplekit --version` says:

```console
$ python -m venv .venv
$ .venv/bin/pip install samplekit==VERSION     # 1.0.0-rc.1, for instance
```

On Windows the environment's programs are in `.venv\Scripts\`:
`.venv\Scripts\pip install samplekit==VERSION`.

Tab completion for your shell is set up with `samplekit completions`; the
[command reference](../reference/cli.md) says how.

## Get the demo

Run `samplekit` alone in a terminal. It opens the TUI's start page. Press `g`,
then choose **The demo**.

The first time, SampleKit writes the demo into its data folder —
`~/.local/share/samplekit/demo` on Linux,
`~/Library/Application Support/samplekit/demo` on macOS,
`%LOCALAPPDATA%\samplekit\demo` on Windows, or `$SAMPLEKIT_DATA_DIR/demo`
when that variable is set — makes its `.venv/`, and opens step 1. After that,
the same entry opens **the demo's page**: its eight steps, one a line with
what it is about. `Enter` on a step, or its number, opens it, and `q` in the
step comes back to that page, so that the next step is a key away. The same
page opens this tutorial in the browser, and **Reset the demo**, at its foot,
writes the demo again as it came, after asking: your changes to it are lost.

The same demo is in SampleKit's source, in `examples/brewing/`. Read it
there; to try it, copy it out of a clone of the repository and make its
`.venv/` beside the steps, so that your changes stay out of the repository:

```console
$ cp -r SampleKit/examples/brewing ~/brewing
$ cd ~/brewing
$ python3 -m venv .venv
$ .venv/bin/pip install samplekit==VERSION     # what samplekit --version says
```

## The steps

| Step | What you do |
| --- | --- |
| [1 · First brews](01-first-brews.md) | Read three brews written by hand: a list, a table, one brew, a filter |
| [2 · Measuring](02-measuring.md) | Units, uncertainties and tags; change a value and a tag |
| [3 · Selecting](03-selecting.md) | Filter, sort, group, summarise; name a query and a profile; export |
| [4 · Computing](04-computing.md) | A model: what is not current and why; compute; the history |
| [5 · Fermentation](05-fermentation.md) | Tables: a series of measurements, and formulas over them |
| [6 · Figures](06-figures.md) | Figures from the command line, from `.samplekitrc` and from the model |
| [7 · Two brewers](07-two-brewers.md) | Two projects side by side; the TUI |
| [8 · Python](08-python.md) | The same brews from a script |

Each step's commands run from its folder, `01-first-brews` and so on.

## After the tutorial

- The [how-to guides](../how-to/index.md) are recipes for your own project:
  moving existing data in, a first model, an export for a paper.
- The [explanations](../explanations/index.md) say what SampleKit's notions
  are and why they are so.
- The [reference](../reference/index.md) lists every command, key and
  setting.

```{toctree}
:hidden:

01-first-brews
02-measuring
03-selecting
04-computing
05-fermentation
06-figures
07-two-brewers
08-python
```
