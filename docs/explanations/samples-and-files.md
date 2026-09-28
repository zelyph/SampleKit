# Samples and their files

A **sample** is one thing you keep a record of: a brew, a batch of bread,
a bottle of cider. In SampleKit a sample is **one Markdown file**. Everything SampleKit
knows about the sample is in that file, and the file is the only store:
there is no database beside it, no index to rebuild, nothing to export before
you can read your data again.

This page says what a file holds, how SampleKit tells a sample from another
Markdown file, and what it writes into a file and what it never touches.

## Why one file per sample

A collection of measurements outlives the program that reads it. A Markdown
file opens in any text editor, shows its changes line by line in git, and can
be read by a colleague who has never heard of SampleKit. So the file is the
record, and SampleKit is a tool that reads it and, when you ask, writes it.

The price is some verbosity at the top of each file. It is accepted
knowingly: the alternative is a database, and a database is exactly what a
folder of files avoids.

## The two parts of a file

```text
---
schema_version: 1
name: oatmeal-stout
tags: [medal]
style: stout
brewed: 2026-01-10
properties:
  og: {v: 1.058, u: 0.001}
  volume: {v: 19.5, unit: L}
---
# Oatmeal stout

Flaked oats in the mash for the body.
```

- The **frontmatter** is the block between the two `---` lines, written in
  YAML. It holds the data: what was measured, what was computed from it, and
  the facts that describe the sample. SampleKit reads it, and rewrites it
  when you ask it to write.
- The **note** is everything after the second `---`. It is yours. SampleKit
  shows it, and never reads it as data and never changes it: not a character,
  not a line ending. Remarks, observations, a procedure, a photo link: they go
  in the note.

`schema_version` is the version of the file's format. A file without it is
read as version 1, the current one. A file written by an older SampleKit is
not read, and every command says so ([Upgrade from an older
version](../how-to/upgrade.md)).

## What the frontmatter holds

A sample holds four kinds of thing. The difference between them is what each
one can carry.

| Kind | Where in the file | What it carries | Example |
| --- | --- | --- | --- |
| **Property** | under `properties:` | a value with the apparatus of a measurement: uncertainty, readings, unit, and for a computed one a record of where it came from | `og`, `volume`, `abv` |
| **Attribute** | any other top-level key | one value, or a list of values, and nothing else: no unit, no uncertainty | `style`, `brewed`, `hops` |
| **Table** | under `tables:` | a series of rows, each cell a quantity like a property | a gravity every day of a fermentation |
| **Tags** | `tags:` | labels recording a deliberate membership | `medal`, `dry_hopped` |

**A property is a quantity.** Use one for anything you measured or computed,
anything with a unit or an uncertainty, anything a formula reads or gives.
[Values and uncertainty](values-and-uncertainty.md) and [Readings and
statistics](readings-and-statistics.md) explain what a property can hold.

**An attribute is a fact about the sample.** A style, a supplier, a batch
number, a date, a list of hops. It holds a number, a text, a date, `true` or
`false`, or a list of one kind of them. You filter, sort and show attributes
exactly as you do properties; they simply carry nothing else.

**A table is a series of measurements** that belong together: a quantity
measured against a temperature, a score from each taster. Each table has an
**index**, the column whose values name its rows (`day`, `taster`), and each
cell is a quantity. A cell is read as `table.column[index]`:
`fermentation.gravity[3]` is the gravity on day 3.

**A tag is a deliberate label.** It says that someone decided this sample
belongs to a set: a favourite, one that won a medal, one poured away.
Tags are written by a person and never derived from a measurement. A set you
want to come back to is a tag on each of its samples, reached by a filter
(`tags has medal`), rather than a list of files kept somewhere else: a list of
files breaks the day a file is renamed, a tag travels with the sample.

Properties, tables and attributes share one set of names in a sample: `og`
cannot be both a property and an attribute. `name`, `tags`, `path` and
`filename` are SampleKit's own.

## Names

The name of a property, an attribute, a table, a column or a tag starts with a
letter or `_` and continues with letters, digits and `_`. Letters with accents
and other alphabets are allowed: `température`, `bière`. A name holds no
hyphen, no dot and no space, and does not start with a digit: `dry_hopped` is
a valid tag, `dry-hopped` is not.

Why so strict: the same name is typed in a filter, a column list, a Python
attribute and a TUI prompt, and the dot and brackets already have a meaning
there. `og.u` is the uncertainty of `og`; `fermentation.gravity[3]` is a cell.
A name that could contain a dot would make `og.u` ambiguous.

The **sample's name**, `name:` in the frontmatter, is different: it is a label
for people, shown in tables. It is optional, and it is not how SampleKit
identifies a sample: the file's path is. Two samples of one project named
alike — by `name:` or by their files' names — cannot be told apart by a
filter or a report, so `samplekit validate` reports them. **A sample without a `name:` is named by its file**, the file's name
without its extension: `s-1.md` is `s-1` in a `name` column, in a filter
(`name == s-1`), in a sort, in Python's `sample.name`, in the TUI and in a
figure's legend. A `name:` written wins.

The name is edited like any attribute: `samplekit set s-1.md "name=Brew
A"`, `sample.name = "Brew A"` in Python, or `e` on the name's line in the
TUI. It changes what the file says, never the file's name. An empty name,
`name=`, removes `name:`, and the file's name stands for it again.
`samplekit new` names the file by the name you give and writes no `name:`,
since the file's name stands for it; it refuses a space or a leading `-`
there. A name you set afterwards is a label and may hold spaces.

## A collection

A **collection** is the samples SampleKit finds in the folder you name. Not
every Markdown file in it is a sample:

- by default, the files ending in `.md`, `.MD` or `.markdown`, in that folder
  and not in its subfolders;
- of those, only the files that **begin with a `---` frontmatter**. A
  `README.md` or a page of notes without one is simply not a sample, and is
  not reported as a problem.

A project's configuration can change this: read the subfolders too
(`recursive`), read other file names (`include`), or skip some (`exclude`).
[Projects](projects.md) explains the configuration, and
[the configuration reference](../reference/configuration.md) lists the keys.

A file that looks like a candidate and cannot be read (a frontmatter that is
not valid YAML, a file from an older version) is **never skipped in silence**.
The command names it, reads the rest, and exits with code 2;
`samplekit list skipped` lists every file set aside and why. A query over 197
samples of 200 that says nothing about the other three gives a confident wrong
answer; that is what this rule prevents.

## A sample's own files

Photos, a tasting sheet, a log from a thermometer: files that belong to a
sample but are not samples. SampleKit does not store them and does not read
them. It can find them, so that you can open them from a command, from Python
or from the TUI.

You say where they live in the project's configuration:

```toml
[collection]
files = ["reports", "photos/{name}*.{jpg,png}", "raw/{name}/**"]
```

- **A folder alone** (`reports`) is searched by name: a file whose name starts
  with the sample file's name belongs to it (`citra-ipa_label.png` belongs to
  `citra-ipa.md`), and so does every file in a subfolder named after the
  sample (`reports/citra-ipa/tasting.pdf`).
- **A pattern with `{name}`** says exactly which files belong to a sample.
  `{name}` stands for the sample's file name without `.md`. `*` matches within
  one folder, `**` across folders, and `{jpg,png}` means either.
- When two sample names start the same way, **the longest match wins**:
  `photos/s-10.png` belongs to `s-10.md`, not to `s-1.md`.

`samplekit list files` lists each sample's files, `samplekit open` opens them,
`files()` and `open()` do the same in Python, and `o` in the TUI's sample
screen. `samplekit validate` reports a pattern that cannot work, such as a
misspelled `{nmae}`.

## What SampleKit writes, and what it never touches

**It writes only when you ask.** Every command that changes a file first shows
what it would change, and writes only with `--write`. In the TUI, a window
shows the change and `y` writes it. In Python, `save()` writes. Reading,
filtering, showing, checking and exporting to the screen change nothing.

**The note is never touched.** Not reformatted, not regenerated, not read as
data.

**The frontmatter is rewritten in one fixed form.** Keys come in a fixed
order, numbers are written one way, short mappings sit on one line. So the
first write to a file you wrote by hand can change more lines than the value
you changed; after that, a write changes only what changed. `validate` notes
the files the next write will reformat.

**Comments in the frontmatter are lost** at the next write: a `# remark`
between the two `---` lines disappears. Put remarks in the note. Likewise a
text that YAML would read otherwise is quoted: `flag: yes` is written
`flag: "yes"`, because it is text, not `true`.

**A computed value carries a record** of what it was computed from: short
codes of its inputs and of itself. [Current and outdated](current-and-outdated.md)
explains them. The record holds names and codes, never code: nothing
executable is ever stored in a file.

**A write is safe.** SampleKit writes to a temporary file and then replaces
the old one, so a crash cannot leave half a file. Just before writing, it
reads the file again, and refuses if someone changed it since it was read
(your editor, another command, a script): your change is never overwritten.
A read-only file is refused. A write that would change nothing writes
nothing.

**Every write is kept** in the project's history, so it can be taken back
([History](history.md)).

## Common mistakes

- **Putting a remark in the frontmatter as a YAML comment.** It is lost at the
  next write. Use the note.
- **A hyphen in a tag or a property name.** `dry-hopped` is refused; write
  `dry_hopped`.
- **A measured quantity written as an attribute.** `volume: 20` at the top
  level is an attribute: it can take no unit and no uncertainty, and a model
  that declares `volume` as a property will not find it there. Put it under
  `properties:`. `samplekit set` and `samplekit new` write a name as a
  property when the configuration, the model or the other samples hold it as
  one.
- **Expecting a Markdown file without frontmatter to be a sample.** It is
  not; start it with `---`.
- **Keeping a list of "the good samples" in a separate file.** Tag them
  instead.

## Where to go next

- The exact keys of a sample file: [Sample files](../reference/sample-files.md).
- Writing a first file and reading it: tutorial steps
  [1](../tutorial/01-first-brews.md) and [2](../tutorial/02-measuring.md).
- Bringing a folder of existing data in: [Move existing data
  in](../how-to/move-existing-data.md).
