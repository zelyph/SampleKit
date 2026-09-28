# Changelog

What changed in SampleKit from one version to the next, newest first. Each
version says what was added, changed, removed and fixed, for someone who uses
SampleKit rather than works on it.

A version ending in `-rc.N` is a release candidate: tried before the version
of that number is published, and replaced by it.

## [1.0.0-rc.1] — 2026-09-28

The first release candidate of SampleKit 1.0, and a new SampleKit. Version
0.2.0 was a pure Python package for writing samples from a script. 1.0 keeps
its idea — a sample is a Markdown file, with its data at the top — and is
built around a command line and a full-screen terminal interface, written in
Rust, with a Python package beside them for your formulas and your scripts.
Both come from one source and carry one version (`1.0.0rc1` for Python).

It does not read the files 0.2.0 wrote, and no command converts them: see
[Upgrading from 0.2.0](#upgrading-from-020) below.

### Added

**The command line**, `samplekit`, a program with no Python inside. Reading,
selecting, showing, checking and exporting samples never start Python.

- `samplekit FOLDER` prints the path of each sample; with `-c` it shows a
  table of the columns you name, and with `--profile` a table declared in
  `.samplekitrc`. `-f` filters (`-f 'og > 1.050 && tags has medal'`), `-s`
  sorts, `--group` makes a table per value of a field, `--summary` summarises
  each column. `--csv`, `--tsv` and `--json` print the same table as data,
  and `-o FILE --write` writes it.
- `list` enumerates what a collection holds and declares: its fields, tags,
  queries, profiles, exports, figures, a sample's own files, and the files it
  skipped and why.
- `view` shows a sample whole: its values, then its tables, and its note with
  `--note`.
- `status` lists every computed value that is not current, and why;
  `status --exit-code` makes it a check for continuous integration.
- `compute` runs the model's formulas: alone it says what would run, with
  `--try` it computes and shows the results without writing, and with
  `--write` it writes each sample as soon as it is done. `explain FILE FIELD`
  says where one value came from, and prints a formula's error.
- `validate` reports the defects of sample files and changes nothing.
- `new` creates a sample from what the project already knows, `set` changes
  its values (`set brew.md og=1.052 og.u=0.001`, readings, table cells, the
  name), and `tag add`, `tag remove` and `tag rename` change its tags. Each
  shows what it would change and writes only with `--write`.
- `export NAME` writes a table declared in `.samplekitrc` as CSV, TSV or
  JSON, after a preview; `--status` adds a column holding each row's state.
- `plot` draws a figure with matplotlib, in a window or to a file: one
  declared in `.samplekitrc` or by the model, or one given by its axes
  (`-x`, `-y`), over properties, attributes or a table's columns.
- `open` opens a sample's own files — photos, reports, raw data — which
  `[collection] files` says where to find.
- `init` sets a project up: its `.samplekitrc`, a model to fill in or the
  one you name with `--model PATH`, and a Python environment, `.venv/`, with
  the SampleKit package installed in it. At a terminal it asks; `--example`
  writes a project that runs, one formula of every kind.
- `tui` opens the full-screen interface on a folder, and `samplekit` alone,
  in a terminal, opens its start page.
- `completions` sets up tab completion for bash, zsh, fish, elvish and
  PowerShell.
- Every command has `--help`, with examples, and `-h` for a short summary.
  Errors name what was asked, what exists instead, and the nearest name. Exit
  codes tell a usage error (1) from a data error (2) and a file the system
  refused (3).

**A history of every change.** Before SampleKit writes to a project, it keeps
what the project held, in `.samplekit/history/`, beside `.samplekitrc`: the
sample files, the configuration and the model's source. There is nothing to
set up, and no Git to install.

- `log` lists the snapshots, `diff` compares two states value by value
  (`abv 6.8 ± 0.1 % → 7.2 ± 0.1 %`), and `restore` puts files back: the last
  change taken back, or the state of a given snapshot.
- `--at` reads the whole project as it was at a snapshot or on a date:
  `samplekit brews -c name,abv --at 2026-09-12`. An export made again this
  way is the same file, byte for byte.
- A change made in your text editor is kept as a snapshot of its own, so it
  is never credited to the command that follows.
- A Python script's writes are one snapshot, with the script's source: `log
  --script N` prints the script as it ran, and `keep()` marks a snapshot of
  its own from inside a script.
- Every export and figure SampleKit writes is recorded. `explain FILE` finds
  it again, even copied into a manuscript's folder and renamed: what made it,
  from which samples, whether it is still current, and the command that makes
  it again.
- `log --export` writes the history as Markdown, to share beside the samples.
- One history over several computers: each writes its own branch in
  `.samplekit/history/`, and the next snapshot joins another computer's once
  your files hold its state — no command merges. `log` shows a `machine`
  column where more than one wrote, and an undo takes back only this
  computer's change. A synchronised folder shares it as it is; git, once the
  history's own `.gitignore` stops ignoring everything.

**Current and outdated values.** Every computed value carries a short record
of what it was computed from. SampleKit compares it with the file as it is
now, and knows — from the files alone, without running Python — which values
are *outdated* (an input changed), *edited* (typed by hand over the formula),
*failed* (the formula raised an error) or *never computed*. Your computer also
keeps a digest of each formula, so that editing a formula makes outdated only
the values it computed, and a comment or a docstring changes nothing. A value
whose input nobody entered yet *waits* for it, and is not an error.

- The states appear in the same words everywhere: `status`, marks in tables
  (`⚠` outdated, `✎` edited, `✗` failed), the TUI, Python. `state` is also a
  field: `-f 'state == outdated'`, `-c name,state`.
- A value typed over its formula is an **override**: `compute` leaves it
  alone until `compute --force` gives it back to its formula.
- A formula that fails does not stop the run: the other values are computed,
  the file records the error's type, and the full traceback is kept for
  `explain`. An interrupted run loses only the sample in progress.

**The full-screen interface, the TUI**, for a project you work in every day.

- A **start page**: recent projects, a new project set up by a few questions,
  a folder opened or configured, and the guide and demo.
- **The collection**: filter, sort, group, choose columns, summarise, and
  save what it shows as a query, a profile or an export of the project.
- **A sample**: its values, tables, readings and note, each value editable;
  its own files opened; a value computed, or everything not current.
- **The history**: the snapshots, what each changed, and `u` to take the last
  change back.
- **The configuration** edited from the TUI, and the colours set by role in
  `[tui.colors]`.
- **Editing as in a text editor**, wherever you type — the filter, a value,
  a name, a setting, the note: the mouse selects, by word on a double click;
  `Shift` and the arrows select; `Ctrl` and the arrows move by word;
  `Ctrl+Backspace` and `Ctrl+Delete` take a word out; `Ctrl+Z` undoes,
  `Ctrl+Shift+Z` redoes; `Ctrl+C` copies a selection, `Ctrl+X` cuts, and a
  paste goes where the cursor is. In the note, Markdown by keys: `*`, `_` or
  `~` around a selection, `Alt+1` to `Alt+6` a heading, `Alt+L` a link,
  `Alt+I` an image, `Alt+C` a code block.
- `?` lists every key of the screen you are on.

**A demo**: a home brewer's notebook in eight steps, from three brews written
by hand to a model, tables, figures, two projects and Python. The start page
writes it for you (`g`), and it is also in the source, in `examples/brewing/`.

**Several projects in one command.** `samplekit status ana/brews tom/brews`
reads both; each sample keeps its own project's units, precisions, model,
queries and profiles, and the terminal shows one table per project.

**New in the sample file** (the format itself is under *Changed*):

- **`n/a`**, not applicable: a value that does not apply to this sample, as
  distinct from one nobody has measured yet. A formula reading it gives `n/a`
  too, without running.
- **Attributes**: facts about a sample — a style, a supplier, a date, a list
  of hops — with no unit and no uncertainty, filtered and shown like
  properties.
- **Tags**, a deliberate label on a sample (`tags: [medal]`), reached by a
  filter (`tags has medal`).
- **A sample's own files** — photos, reports, raw data — found by the
  sample's name where `[collection] files` says, and opened from the command
  line, the TUI or Python.

**A project's configuration**, `.samplekitrc`, in the project's folder: the
model; queries, profiles, exports and figures by name; how a unit is shown in
a table, in LaTeX and on a figure (`[unit.*]`); the precision, symbol and unit
of each quantity, declared once (`[property.*]`); matplotlib's settings;
which files are samples. Every key is checked: a misspelt one is an error
that names the line and the nearest valid key.

- **`import = ".."`** takes the configuration of the folder named, merged key
  by key beneath this one: what several collections share is declared once,
  at their common root, and each collection keeps its model and what is its
  own. Imports chain; a loop is refused. A relative path reads from the file
  that declares it, and `{collection}` names a sample's collection folder in
  `[collection] files` and an export's `output`.
- **What the current folder offers**: `list profiles`, `queries`, `exports`
  and `figures`, the completion and the TUI's `f` show the configuration of
  the folder a command runs from, with what it imports, each name marked
  `local` or with the file it comes from. A sample still follows its own
  collection's configuration wherever the command runs from.
- `validate` notes a declaration identical to the one imported, a copy to
  remove, and `--show-rc` names the files imported.

**One warning a command.** A command that would warn several times of one
thing — samples a profile or an export several projects lack, targets that
are not Markdown, histories not kept, what a model logged — says it once, in
one line counting them and ending *-v shows them*; `-v` lists each.

**The documentation**, in four parts: a tutorial that follows the demo;
explanations, a page per notion; how-to guides for a task you have in mind;
and a reference — every command and option, the Python API, the TUI's keys,
every key of `.samplekitrc`, the sample file format, the exit codes.

### Changed

**The note below the data is yours.** 0.2.0 wrote the body of a file from the
model's `template()` at every save. SampleKit now never writes, reformats or
reads the note: not a character, not a line ending. Tables for a reader are
what the command line, the TUI and exports show.

**The sample file.** A sample is still one Markdown file with its data in a
YAML block at the top, written differently:

```yaml
---
schema_version: 1
style: stout
properties:
  og: {v: 1.058, u: 0.001, readings: [1.057, 1.058, 1.059]}
  volume: {v: 19.5, unit: L}
tables:
  fermentation:
    index: day
    columns:
      day: {unit: d}
      gravity: {}
    rows:
      - {day: 1, gravity: 1.041}
---
```

- `schema_version: 1` heads the file. Quantities go under `properties:`,
  tables under `tables:`, and any other top-level key is an attribute.
- A quantity is a bare number or a short mapping: `v` for the value (was
  `value`), `u` for the standard uncertainty in the value's unit (was
  `uncertainty`), `readings` for the repeated measurements (was `data`), and
  `unit`. `og.v` and `og.u` are also how a filter, a column, `set` and Python
  name the two numbers.
- A table has `index`, `columns` as a mapping and `rows` (was `_index`,
  `_columns` as a list, `_rows`), an index of one column or several, and
  cells that are quantities. `fermentation.gravity[3]` names the gravity on
  day 3.
- A precision and a LaTeX spelling are no longer written in each file
  (`precision`, `precision_unc`, `symbol_math`, `unit_math`): they are
  declared once in `.samplekitrc`, where a quantity's unit and symbol can be
  too. A formula is never written in a file.
- `name:` is optional: without it, the file's name without its extension
  stands for the sample's name.
- A computed value carries a short record of what it was computed from
  (`computed:`, `fingerprint:`), which is how SampleKit tells it is current.
- Names of properties, attributes, tables, columns and tags start with a
  letter or `_` and hold letters, digits and `_`, in any alphabet: `température` and
  `bière` are names, `dry-hopped` is not.
- The data is written in one fixed form: the first write to a file written
  by hand may change more lines than the value, and after that only what
  changed. A write goes to a temporary file first, and is refused if the file
  changed on disk since it was read.

**Readings and their statistic.** In 0.2.0, a list of measurements gave the
mean and the sample standard deviation on its own. The value of readings is
now **the statistic the model declares** — mean, median, minimum, maximum or
a quartile, and standard error or a standard deviation for the uncertainty —
or a value written beside them. Readings with neither have no value, and
every surface says so rather than taking a mean nobody chose. The file notes
which statistic gave the numbers (`statistics: {v: mean, u: standard_error}`).

**Units, symbols and precision** are the project's. A unit is a label, never
converted; `[unit."degC"]` says how it is shown in a table (`°C`), in LaTeX
and on a figure. Two samples writing different units for one quantity are
reported rather than put side by side. A declared precision rounds alike on
the screen and in an export.

**The model and computing.** A project names its model, a subclass of
`samplekit.Sample`, in `.samplekitrc` (`[model] path` and `class`), and every
tool uses it. Each computed value has a formula of its own and names what it
reads:

```python
self.abv = sk.Property(unit="%", compute_quantity=self._abv, depends_on=["og", "fg"])
```

- `compute=` gives the value, `compute_uncertainty=` (was `compute_unc`) the
  uncertainty beside a value you enter, and `compute_quantity=` both. A
  table's cells are computed a row at a time (`compute_rows`) or a column at
  once (`compute_columns`).
- `depends_on` names the inputs by name (`"og"`, `"fermentation.gravity"`),
  not as `Property` objects, and it is required: a model with a formula
  that declares nothing is refused before any formula runs.
- **A model declares no symbol and no precision**: both are the project's,
  in `[property.*]`, as the unit's display is. `sk.Property` and `sk.Column`
  refuse `symbol=` and `precision=`, saying where each is declared.
- **Nothing is computed when a value is read.** Values are computed when you
  ask — `samplekit compute`, `C` or `c` in the TUI, `compute()` in Python —
  and only what is not current, in the order the inputs require. A change to
  an input marks what reads it as outdated; it does not recompute it.
- **A value set over a formula is an override**: the formula stays in the
  model, and the value is marked edited, where 0.2.0 dropped the formula.
- The command line runs the model in the Python of the nearest `.venv/`
  above the samples, or the interpreter `[model] python` names, where the
  SampleKit package must be installed in the same version. It runs without
  asking, as `python` runs a script you give it, and names the configuration
  and the model file before it starts.
- The model is described once, in `.samplekit/model.json`, written whenever
  it is imported and again when its files change: what only reads the model's
  declarations — `status`, `new` and `set`, a `state` filter, the TUI — reads
  the description and starts no Python. Python runs to compute and to draw.
- What a model prints goes to a log file; its warnings are shown during the
  run.
- Uncertainties are never propagated on their own: a formula that should give
  one returns it.

**The Python package** is built from the same Rust code as the command line,
and changes its shape:

- `sk.load(path)` reads a sample or a folder, with the project's model
  (was `SampleList(path, sample_class=…)` and `Sample.load`); `sk.load(a, b)`
  reads several folders into one list.
- `Sample.new(path)` makes a sample, `save()` writes it, `compute()` computes
  what is not current, and `not_current()` lists what `status` would.
- A `SampleList` filters with the command line's language
  (`brews.filter("og > 1.050")`) as well as a function, sorts, groups
  (`group_by`), selects with a query of the project, and writes CSV, TSV or
  JSON at the project's precisions (`to_csv`, `to_tsv`, `to_json`,
  `to_dict`).
- `sk.stats` names the statistics readings can stand for, and
  `sk.plot` draws the same figures as the command line.
- Reading a value that is not current returns it, with a warning saying why.
- The package ships type stubs that carry its documentation, so that an
  editor shows both.
- Python 3.11 or newer is required (was 3.10), and neither PyYAML nor pandas
  is needed.

**Performance.** The package is built in Rust. Reading and writing sample
files is 25 to 35 times faster than 0.2.0, and the statistics of readings
about 3 times faster. A value computed costs a little more — some 15 µs per
formula run — for the record of what it read, which is how SampleKit knows
afterwards that it is still current; beside a formula that takes seconds, it
does not show. `status`, `new`, `set` and the TUI start no Python while the
model's description is current.

### Removed

- **Reading 0.2.0 files.** Every command names such a file, says why, reads
  the rest and exits with code 2.
- **The long spellings `value:` and `uncertainty:`.** They are not read, in a
  property, a table's cell or anywhere else in a sample file: such a file is
  not read, and every command names the key and what it is written now —
  `'value' is written 'v' since schema 1: rename it`.
- **`template()` and the `report` module**: the note is no longer generated.
  Tables for a paper are profiles and exports; figures are drawn with `plot`.
- **pandas**: `to_dataframe()`, `stats()` on a list and the `converters`
  module. `to_csv()` or `to_dict()` give data pandas reads, and `--summary`,
  `Summary` and `sk.stats` summarise.
- **Computing on read**, and the cache a changed input cleared.
- **The automatic mean and standard deviation of a list**: a statistic is
  declared.

### Upgrading from 0.2.0

No command converts a 0.2.0 sample: the model has to be rewritten by hand
anyway, and a tool that did half the work would take all the risk of writing
to your files. [Upgrade from an older SampleKit](docs/how-to/upgrade.md)
shows how to carry each sample across — its values, uncertainties, readings,
tables and text — with both versions installed side by side, into a new
folder, and how to check the result.

## [0.2.0]

The previous SampleKit, a Python package published on the Python Package
Index. Its changes are not recorded here.
