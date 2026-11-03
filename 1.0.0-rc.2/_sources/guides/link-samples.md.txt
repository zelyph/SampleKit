# Link samples

This guide is for a sample that names another: a specimen compared with a
reference one, a piece cut from a plate, the file its machine wrote, the
standard it was tested to. A link is an attribute, so it is written, read,
filtered and exported as one; SampleKit also shows where it leads, follows it
from either side, checks it, and keeps it when a sample is renamed.

The commands run in the last step of the [home page's](../index.md) tensile
example, where the aluminium and copper specimens name the steel one they are
compared with:

```yaml
compared_with: "[[steel-01]]"
```

## 1. Write a link

A link leads to one of three places ([every form](../reference/sample-files.md#links)):

| Written | Leads to |
| --- | --- |
| `"[[steel-01]]"`, `"[[steel-01\|the steel]]"` | a sample of the project: the one whose file is `steel-01.md`, or else the one whose `name` is `steel-01` |
| `"[the curve](../machine/steel-01/curve.csv)"` | a file, a sample of another project among them, by its path from the sample's own folder |
| `"https://…"`, `"[ISO 6892-1](https://…)"` | a web page |

These are the forms Obsidian and Markdown write, so a project opened in
either reads its links the same way. **The quotes are needed** in the file:
unquoted, `[[steel-01]]` reads as a list, and the file is refused at that
line, with the quoted form to write.

`set` writes them quoted. Here the steel specimen gets the file its testing
machine wrote, beside the samples' folder, and the standard it was tested to:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md set 'raw=[the curve](../machine/steel-01/curve.csv)' 'standard=https://www.iso.org/standard/78322.html' --write
samples/steel-01.md · 2 changes

  raw        —  →  [the curve](../machine/steel-01/curve.csv)
             new
  standard   —  →  https://www.iso.org/standard/78322.html
             new

  nothing becomes not current

written: samples/steel-01.md
```

After `[[`, `Tab` offers the project's samples by their file's name, on the
command line as in the terminal interface. Several links typed one after
another, `set 'parents=[[aluminium-01]],[[copper-01]]'`, write a list of two.

## 2. See where it leads

In a terminal, a link is shown as Markdown shows one: its text alone,
`steel-01` for `[[steel-01]]` and `the curve` for the machine's file,
underlined in the `link` colour ([colours](../reference/configuration.md#colors)).
Nothing marks a link to a sample or a file; a web page is marked `↗`, and a
link that leads nowhere `✗`, in the error colour. So in a table, in `view`
and in the terminal interface.

What is written to a file or a pipe keeps the text as written, so that the
program reading it finds the link whole. A table written as a document,
Markdown, HTML or LaTeX, holds it as text too, not as a link of that format:

<!-- run: tensile/04-results -->
```console
$ samplekit -c name,compared_with --csv
name,compared_with
aluminium-01,[[steel-01]]
copper-01,[[steel-01]]
steel-01,
```

In the terminal interface, `Enter` or `o` on a link goes where it leads: a
sample opens on its screen, even one the filter leaves out or one of another
project, `Esc` coming back; a file opens in its application, a web page in
the browser. On a list of links, `Enter` lists them, and `Enter` or `o`
follows the one under the cursor.

## 3. The samples linking to one

A sample's screen in the terminal interface ends with the samples whose links
lead to it, *linked from*, each followed by `Enter`. On the command line, the
filter `links to NAME` selects them, `FIELD links to NAME` for one attribute:

<!-- run: tensile/04-results -->
```console
$ samplekit -f 'links to steel-01'
samples/aluminium-01.md
samples/copper-01.md
```

The filter reads the links as they are written, `[[steel-01]]` or
`[…](steel-01.md)`: a link written with a sample's `name` rather than its
file's, `[[Steel A]]`, is found by `links to "Steel A"`, and a name no link
writes selects nothing. *Linked from*, in the terminal interface, finds every
link leading to the sample, however it names it.

## 4. A link that leads nowhere

A sample renamed by hand or removed, a name mistyped: `validate` reports a
link that leads to no sample, or to several, and a path where nothing is,
and exits with code 2. A web page is not checked. `remove` lists, before it
removes a sample, the links leading to it, and leaves them as written:
`validate` finds them after. Here
the aluminium specimen's link was set to `[[steel-1]]`:

```console
$ samplekit samples validate
  aluminium-01           compared_with: [[steel-1]] leads to no sample — no file of the project is named steel-1, and no sample's name is

3 samples · 1 defect · 0 notes
error: 1 defect in 1 file
```

## 5. Rename a sample, and its links with it

`samplekit samples/steel-01.md rename steel-02` renames the steel specimen
and writes again the two links leading to it, `compared_with: "[[steel-02]]"`
in the aluminium and copper files; a Markdown link, `[text](steel-01.md)`,
is written again the same way. Its own files, a folder named after it, and
its `name` where it was the file's name, are renamed with it, and a link to
one of those files, `[the curve](../machine/steel-01/curve.csv)`, is written
to follow it. It is
previewed first, and `--write` makes it ([rename a
sample](edit-samples.md#rename-a-sample)); in the terminal interface, `R` on
a sample.

## 6. From Python

`linked` reads where a link leads: the sample, for a link to a sample; a
`pathlib.Path`, for a file that is no sample; a list, for a list of links. A
web address is no place to read, and raises `ValueError`; the attribute
holds it as text.
`filter` takes `links to NAME` as `-f` does, and `rename` renames a sample
and the links leading to it ([work from Python](work-from-python.md)):

```{literalinclude} ../../examples/tensile/04-results/scripts/links.py
:language: python
:caption: scripts/links.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/links.py
steel-01 518 MPa
['aluminium-01', 'copper-01']
```

## 7. In Obsidian

A project opened in Obsidian as a vault finds the same links: Obsidian reads
a Markdown link's path from the note's own folder, as SampleKit does, and
opens a file outside the vault in its application. Among a note's
properties, Obsidian makes only `[[…]]` clickable; a Markdown link stays text
there, and SampleKit still follows it.

## See also

- [Links in a sample file](../reference/sample-files.md#links), every form.
- [The filter `links to`](../reference/filters.md).
- [Work in the terminal interface](terminal-interface.md).
