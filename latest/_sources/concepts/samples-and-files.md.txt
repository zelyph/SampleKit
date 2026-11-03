# Samples and their files

A **sample** is one thing you keep a record of: a specimen pulled until it
breaks, a batch of bread, a soil core. In SampleKit a sample is **one
Markdown file**. Everything SampleKit knows about the sample is in that file,
and the file is the only store: there is no database beside it, no index to
rebuild, nothing to export before you can read your data again.

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

```{code-block} sample
:caption: samples/steel-01.md

---
schema_version: 1
name: steel-01
tags: [reference]
material: steel
grade: S355
tested: 2026-03-04
properties:
  width: {v: 12.513, u: 0.010, unit: mm}
  thickness: {v: 2.0, unit: mm}
---
Tested at 2 mm/min, along the rolling direction.
```

- The **frontmatter** is the block between the two `---` lines, written in
  YAML. It holds the data: what was measured, what was computed from it, and
  the facts that describe the sample. SampleKit reads it, and rewrites it
  when you ask it to write.
- The **note** is everything after the second `---`. It is yours. SampleKit
  shows it, never reads it as data, and never changes it unless you replace
  it yourself, with `set note=…`, the TUI's `N` or Python's `sample.note`:
  not a character, not a line ending. Remarks, observations, a procedure, a photo link: they go
  in the note.

**A sample may be a symbolic link** to a file elsewhere: it is read as the
file it leads to. A symbolic link leading out of its project is never written
through, since
its file would change in another folder, with none of this project's history;
a command that would write it says so: change the file where it is, or copy
it into the project.

`schema_version` is the version of the file's format, `1` today, and every
sample writes it: a Markdown file without it is no sample, left out as a
`README.md` is, and `validate` notes it. A file written by SampleKit 0.x
declares no `schema_version`, so it is left out the same way: `validate`
counts such files and `samplekit list skipped` names them ([Upgrade from an
older version](../guides/upgrade.md)).

## What the frontmatter holds

A sample holds four kinds of thing. The difference between them is what each
one can carry.

| Kind | Where in the file | What it carries | Example |
| --- | --- | --- | --- |
| **Property** | under `properties:` | a value with the apparatus of a measurement: uncertainty, readings, unit, and for a computed one a record of where it came from | `width`, `thickness`, `area` |
| **Attribute** | any other top-level key | one value, or a list of values, and nothing else: no unit, no uncertainty | `material`, `tested`, `operators` |
| **Table** | under `tables:` | a series of rows, each cell holding a value as a property does | the force read at each strain of a test |
| **Tags** | `tags:` | labels recording a deliberate membership | `reference`, `to_redo` |

**A property is a value you measured or computed**: use one for anything
with a unit or an uncertainty, anything a formula reads or gives.
[Values and uncertainty](values-and-uncertainty.md) and [Readings and
statistics](readings-and-statistics.md) explain what a property can hold.

**An attribute is a fact about the sample.** A material, a supplier, a batch
number, a date, the list of people who ran the test. It holds a number, a text, a date, `true` or
`false`, or a list of one kind of them. You filter, sort and show attributes
exactly as you do properties; they simply carry nothing else. A text
attribute may also lead elsewhere: `"[[steel-02]]"` links to another sample,
as Obsidian writes a link, `"[certificate](../certificates/c.pdf)"` to a file,
as Markdown writes one, and `https://…` to a web page
([links](../reference/sample-files.md#links)).

**A table is a series of measurements** that belong together: a value
measured against a strain, a reading at each temperature. Each table has an
**index**, the column whose values name its rows (`strain`, `temperature`),
and each cell holds a value as a property does. A cell is read as
`table.column[index]`: `curve.force[0.2]` is the force at 0.2 % of strain.

**A tag is a deliberate label.** It says that someone decided this sample
belongs to a set: a reference specimen, one to test again, one set aside.
Tags are written by a person and never derived from a measurement. A set you
want to come back to is a tag on each of its samples, reached by a filter
(`tags has reference`), rather than a list of files kept somewhere else: a list of
files breaks the day a file is renamed, a tag travels with the sample.

Properties, tables and attributes share one set of names in a sample: `width`
cannot be both a property and an attribute. `name`, `tags`, `path`,
`filename`, `project`, `state`, `readings` and `note` are SampleKit's own:
`note` is the text below the frontmatter, so a remark kept as an attribute is
named `remarks` or `comment`.

## Names

The name of a property, an attribute, a table, a column or a tag starts with a
letter and continues with letters, digits and `_`; only a tag may also start
with `_`, which the others keep for Python's private state. Letters with accents
and other alphabets are allowed: `température`, `épaisseur`. A name holds no
hyphen, no dot and no space, and does not start with a digit: `to_redo` is a
valid tag, `to-redo` is not.

Why so strict: the same name is typed in a filter, a column list, a Python
attribute and a TUI prompt, and the dot and brackets already have a meaning
there. `width.u` is the uncertainty of `width`; `curve.force[0.2]` is a cell.
A name that could contain a dot would make `width.u` ambiguous.

The **sample's name**, `name:` in the frontmatter, is different: it is a label
for people, shown in tables. It is optional, and it is not how SampleKit
identifies a sample: the file's path is. Two samples of one project named
alike, by `name:` or by their files' names, cannot be told apart by a filter
or a report, so `samplekit validate` reports them. **A sample without a `name:` is named by its file**, the file's name
without its extension: `s-1.md` is `s-1` in a `name` column, in a filter
(`name == s-1`), in a sort, in Python's `sample.name`, in the TUI and in a
figure's legend. A `name:` written wins.

The name is edited like any attribute: `samplekit s-1.md set "name=Steel
A"`, `sample.name = "Steel A"` in Python, or `e` on the name's line in the
TUI. It changes what the file says, never the file's name. An empty name,
`name=`, removes `name:`, and the file's name stands for it again.
`samplekit new` names the file by the name you give and writes no `name:`,
since the file's name stands for it; it refuses a space or a leading `-`
there. A name you set afterwards is a label and may hold spaces.

## A collection

A **collection** is the samples SampleKit finds in the folder you name. Not
every Markdown file in it is a sample:

- the files ending in `.md`, `.MD` or `.markdown`, in that folder and, in a
  project, in its subfolders too;
- of those, only the files that **begin with a `---` frontmatter** declaring
  `schema_version`. A `README.md` or a page of notes without one is simply not
  a sample, and is not reported as a problem.

A project's configuration can change this: read its folder alone
(`recursive = false`), read other file names (`include`), or skip some
(`exclude`). A folder outside any project is read alone.
[Projects](projects.md) explains the configuration, and
[the configuration reference](../reference/configuration.md) lists the keys.

A file that looks like a candidate and cannot be read (a frontmatter that is
not valid YAML, a `schema_version` other than the one this version reads) is
**never skipped in silence**.
The command says how many files it could not read (`-v` names them), reads
the rest, and exits with code 2; `samplekit list skipped` lists every file set
aside and why. A query over 197
samples of 200 that says nothing about the other three gives a confident wrong
answer; that is what this rule prevents.

## A sample's own files

Photos of a fracture, the testing machine's raw output, a certificate: files
that belong to a sample but are not samples. SampleKit does not store them and does not read
them. It can find them, so that you can open them from a command, from Python
or from the TUI.

You say where they live in the project's configuration:

```toml
[collection]
files = ["photos", "machine/{name}/*", "reports/{name}*.{pdf,txt}"]
```

- **A folder alone** (`photos`) is searched by name: a file whose name starts
  with the sample file's name belongs to it (`steel-01-fracture.jpg` belongs
  to `steel-01.md`), and so does every file in a subfolder named after the
  sample (`photos/steel-01/side.jpg`).
- **A pattern with `{name}`** says exactly which files belong to a sample.
  `{name}` stands for the sample's file name without `.md`. `*` matches within
  one folder, `**` across folders, and `{jpg,png}` means either.
- When two sample names start the same way, **the longest match wins**:
  `photos/s-10.png` belongs to `s-10.md`, not to `s-1.md`.

`samplekit list files` lists each sample's files, `samplekit open` opens them,
`files()` and `open()` do the same in Python, and `o` in the TUI's sample
screen. `samplekit validate` reports a pattern that cannot work, such as a
misspelled `{nmae}`.

## Links between samples

A specimen compared with a reference one, a piece cut from a plate, a
certificate kept elsewhere: a relation a sample states is an attribute that
leads somewhere, a **link**. It is written in the forms Obsidian and Markdown
write, `"[[steel-01]]"` for a sample of the project and
`"[text](../plates/P3.md)"` for a file by its path, so that a project opened
in either finds the same links ([every form](../reference/sample-files.md#links)).

- **A link is text, kept as written.** A filter, a sort, an export and a
  file written read it so; only a terminal shows it as Markdown shows one,
  its text alone, marked `↗` for a web page and `✗` where it leads nowhere.
- **A path is read from the sample's own folder**, as Markdown and Obsidian
  read it: `../plates/P3.md` from `samples/` is `plates/P3.md` in the project.
  `[[NAME]]` needs no path: it names a file of the project, or else a
  sample's `name`.
- **It is followed from either side.** The terminal interface opens where a
  link leads and lists, on a sample's screen, the samples linking to it; the
  filter `links to NAME` selects the samples whose links write NAME.
- **It is kept.** `validate` reports a link that leads nowhere, and `rename`
  writes again every link leading to the sample it renames.

A sample's own files need no link: they are found by their name. A link is
for a relation no name gives ([link samples](../guides/link-samples.md)).

## What SampleKit writes, and what it never touches

**It writes only when you ask.** Every command that changes a file first shows
what it would change, and asks `apply these changes? [y/N]`: `y` (or `yes`) writes, and `n`, an empty
line or no input at all (a script given none) writes nothing. `--write`
writes without asking. In the TUI, a window
shows the change and `y` writes it. In Python, `save()` writes. Reading,
filtering, showing, checking and exporting to the screen change nothing.

**The note is kept as you wrote it.** Not reformatted, not regenerated, not
read as data; it changes only when you replace it yourself, `set note=…`.

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
- **A hyphen in a tag or a property name.** `to-redo` is refused; write
  `to_redo`.
- **A measured property written as an attribute.** `width: 12.5` at the top
  level is an attribute: it can take no unit and no uncertainty, and a model
  that declares `width` as a property will not find it there. Put it under
  `properties:`. `samplekit set` and `samplekit new` write a name as a
  property when the configuration, the model or the other samples hold it as
  one.
- **Expecting a Markdown file without frontmatter to be a sample.** It is
  not; start it with `---`.
- **Keeping a list of "the good samples" in a separate file.** Tag them
  instead.

## Where to go next

- The exact keys of a sample file: [Sample files](../reference/sample-files.md).
- Writing a first file and reading it: the [home
  page](../index.md#samples-and-projects), and [edit samples](../guides/edit-samples.md).
- Bringing a folder of existing data in: [Bring existing data
  in](../guides/move-existing-data.md).
- Samples that name one another: [link samples](../guides/link-samples.md).
