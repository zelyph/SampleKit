# Glossary

The words SampleKit uses, each in a sentence or two, with the page that
explains it. The examples are the tensile test of the [home page](../index.md).

```{glossary}
:sorted:

absent
   A value nobody entered: no key in the file, or a key with nothing after it.
   It is never read as zero: a filter leaves it out, a table shows `—`, and a
   formula that needs it {term}`waits <waiting>`. See [no value, not applicable,
   zero](../concepts/values-and-uncertainty.md#no-value-not-applicable-zero).

attribute
   A fact about a sample, written at the top of its file: `material: steel`,
   `tested: 2026-03-04`, `operators: [ana, tom]`. It holds a value, or a list of
   values of one kind, and no unit, uncertainty or readings. See [what the
   frontmatter holds](../concepts/samples-and-files.md#what-the-frontmatter-holds).

basket
   In the TUI, the samples put aside with `Space`, which `C`, `p`, `t` and `D`
   then act on alone. See [work in the terminal interface](../guides/terminal-interface.md).

cell
   One value of a {term}`table`, named by its column and its row's
   {term}`index`: `curve.force[0.2]`.

collection
   The samples one command reads: a folder, files, what a filter keeps. It may
   span several {term}`projects <project>`, each sample keeping its own
   project's rules. See [projects](../concepts/projects.md).

configuration
   A project's `.samplekitrc`, in TOML: how its values are shown, which files
   are samples, its model, queries, profiles, exports and figures. The nearest
   one above a sample applies. See [configuration](configuration.md).

computed value
   A value a {term}`formula` of the {term}`model` gives, rather
   than one typed. It carries a {term}`record` of what it was computed
   from. See [models and formulas](../concepts/models-and-formulas.md).

convention
   How a quantity is written where no {term}`precision` is declared:
   the uncertainty to two significant digits, rounded up, and the value to the
   same decimal place, `25.056 ± 0.053`. See [precision: how many
   digits](../concepts/values-and-uncertainty.md#precision-how-many-digits).

current
   A computed value that is what its formula gives from the file as it now is.
   See [current and outdated values](../concepts/current-and-outdated.md).

defective
   A sample whose file `validate` reports a defect in: text where a number is
   expected, two rows with one index, a unit that disagrees.

demo
   The tensile test in six steps that SampleKit carries, written on demand from
   the TUI's start page (`g`), each step a project to try commands on.

digest
   A short code of a content, twelve hexadecimal digits: the same content always
   gives the same code, any change a different one. See
   [fingerprints](fingerprints.md).

edited
   A computed value typed over its formula. `compute` leaves it alone;
   `compute --force` gives it back to its formula. See [values typed over
   their formula, and
   `--force`](../concepts/current-and-outdated.md).

export
   A file declared in `.samplekitrc` under `[export.NAME]`, a table from a
   {term}`profile` or a {term}`figure`, written again by
   `samplekit export NAME --write`. See [tables and figures for a
   paper](../guides/export-for-a-paper.md), and [written
   files](written-files.md) for what a file holds.

failed
   A computed value whose formula raised an error when last run. The file keeps
   the last value it gave and the error's type; `explain` prints the traceback.

field
   The name of anything a sample holds, as a filter, a column, a sort or Python
   names it: `width`, `width.u`, `material`, `curve.force[0.2]`, `state`. See
   [sample files](sample-files.md#fields).

figure
   A plot of samples, drawn by matplotlib: declared in `.samplekitrc` under
   `[figure.NAME]`, drawn by the model with `@sk.figure`, or asked for on the
   command line with `plot -x … -y …`. See [figures](../index.md#figures).

filter
   A condition on fields that keeps the samples meeting it: `-f 'material ==
   steel'`. See [filters](filters.md).

fingerprint
   The {term}`digest` a value carries of itself, which tells whether a
   hand changed it since it was computed. See [fingerprints](fingerprints.md).

formula
   A method of the {term}`model` that gives one value, declared with the
   property: `sk.Property(compute=self._area, depends_on=["width",
   "thickness"])`.

frontmatter
   The part of a sample file between the two `---` lines, in YAML: its data.

group
   A split of the samples shown into one table per value of a field:
   `--group material`. Computed from the data each time, never saved as a list.

history
   Every change SampleKit writes in a project, kept as
   {term}`snapshots <snapshot>` in `.samplekit/history/`, a git repository that
   stays readable without SampleKit. See [history](../concepts/history.md).

import
   A line of `.samplekitrc` that merges another configuration into this one:
   `import = ".."`. See [importing what several projects
   share](../concepts/projects.md#importing-what-several-projects-share).

index
   The column, or columns, whose values name a {term}`table's <table>` rows:
   `strain` in the curve. A cell is read by its index value,
   `curve.force[0.2]`, or by its row's position, `curve.force[#3]`.

input
   A value a formula reads, named in its `depends_on`. A change to an input
   makes the value {term}`outdated`.

link
   A text attribute leading elsewhere: `"[[steel-02]]"`, another sample of the
   project as Obsidian writes a link; `"[certificate](../certificates/c.pdf)"`,
   a file by its path, from the sample's folder, as Markdown writes one; or a
   web address. A terminal shows its text alone; the TUI opens it, `validate`
   reports one that leads nowhere, and `rename` writes it again. See
   [links](sample-files.md#links) and [link samples](../guides/link-samples.md).

linked from
   The samples whose links lead to a sample, listed at the foot of its screen
   in the TUI. The filter `links to NAME` selects the samples whose links
   write NAME.

migration tool
   `samplekit-migrate.pyz`, attached to each release: a project of an earlier
   SampleKit carried into a new folder this version reads, on a copy, and with
   no guarantee. See [upgrade from an older SampleKit](../guides/upgrade.md).

model
   The Python class of a project, a subclass of `samplekit.Sample`, that
   declares its properties and tables and holds its formulas, figures and
   scripts. Named in `.samplekitrc` under `[model]`. See [models and
   formulas](../concepts/models-and-formulas.md).

n/a
   Not applicable: an answer saying there is nothing to measure for this
   sample, where {term}`absent` is a gap. A value computed from one is
   `n/a` too.

never computed
   A value the model declares and the file does not hold yet.

not current
   Any of {term}`outdated`, {term}`edited`,
   {term}`failed` and {term}`never computed`: what
   `status --exit-code` fails on.

note
   Everything after a sample file's second `---`: free Markdown that SampleKit
   shows and never reads, changed only when you replace it, `set note=…`.

outdated
   A computed value whose input, readings or formula changed since it was
   computed. `compute --write` computes it again.

precision
   How many digits a number is written with, `".3f"` or `[".1f", ".2f"]` for the
   value and the uncertainty, declared in the project, never in a sample file. A
   value is rounded to the nearest, an uncertainty up. See [precision: how many
   digits](../concepts/values-and-uncertainty.md#precision-how-many-digits).

profile
   A named set of columns, with their labels, digits, sort and groups, declared
   under `[profile.NAME]` and shown by `--profile NAME`.

project
   A folder with a `.samplekitrc` in it, and the samples below it: what they
   have in common, which model computes them, how their values are shown. See
   [projects](../concepts/projects.md).

property
   A measured or computed value of a sample, under `properties:` in its file:
   a value with, where it has them, an uncertainty, readings and a unit. See
   [values, uncertainty and units](../concepts/values-and-uncertainty.md).

query
   A {term}`filter` saved by name, under `[query.NAME]`, and used by
   `--query NAME`.

readings
   The results of a property measured several times, kept as they were taken:
   `thickness: {readings: [1.995, 2.004, 2.008]}`. A {term}`statistic`
   the model declares gives the value from them. See [readings and
   statistics](../concepts/readings-and-statistics.md).

record
   What a computed value carries beside its number: `computed:`, the digest of
   each input it read, and its {term}`fingerprint`.

sample
   One thing kept a record of, one specimen, in one Markdown file. See [samples
   and their files](../concepts/samples-and-files.md).

script
   A method of the model marked `@sk.script`, run on the samples selected by
   `samplekit run NAME` or `p` in the TUI, what it changes shown before it is
   written. See [run a script of the model](../guides/run-a-script.md).

snapshot
   One state of a project kept in its {term}`history`, taken at each
   write, listed by `samplekit log`, and read back by `--at`.

state
   A {term}`field` saying how a sample stands: `current`, `outdated`, `edited`,
   `failed`, `never_computed`, `waiting`, `defective`, and `not_current` for
   any of outdated, edited, failed and never computed. A filter reads it:
   `-f 'state == outdated'`. See [the
   states](../concepts/current-and-outdated.md#the-states).

statistic
   What the model declares stands for a property's readings: `sk.stats.mean`
   for the value, `sk.stats.standard_error` for the uncertainty, recorded in the
   file as `statistics:`. Readings with none declared and none recorded have no
   value.

style
   A way of writing units, symbols and quantities: `plain` on a terminal,
   `latex`, `markdown` and `html` given, others declared under `[style.NAME]`.

summary
   Statistics of a column across samples, `--summary`: how many have a value,
   their mean, spread and extremes. Not the statistics of one property's
   {term}`readings`. See [readings are not a
   summary](../concepts/readings-and-statistics.md#readings-are-not-a-summary).

symbol
   A property's name in a formula or on a figure's axis, `\sigma`, declared in
   `[property.NAME]`. It changes nothing in the data.

table
   A series of rows inside a sample, its columns and rows written in the file,
   and its columns declared by the model too, where there is one: the force
   read at each strain. See [a table inside a
   sample](../index.md#a-table-inside-a-sample).

tag
   A word that puts a sample in a set someone chose, `reference`, written by
   `samplekit tag add` and selected by `tags has reference`.

TUI
   The terminal interface, opened by `samplekit` alone: the same work as the
   command line, from one full-screen view, by the keyboard. See [work in the
   terminal interface](../guides/terminal-interface.md).

uncertainty
   The standard uncertainty of a value, absolute and in its unit, `u` in the
   file. SampleKit never invents one: it is typed, given by readings, or
   returned by a formula.

unit
   A label a value carries, `mm`, written as typed and never converted. Its
   forms in each style are declared under `[unit."mm"]`.

waiting
   A computed value an input of which nobody entered yet: its formula does not
   run, what reads it waits too, and it is not counted as {term}`not current`.
```
