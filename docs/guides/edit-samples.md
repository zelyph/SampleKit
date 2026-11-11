# Edit samples

This guide is for changing what your samples hold: a value measured again, an
attribute corrected, a tag, a row of a table, a new sample. Each change is
shown before it is written, says what it makes out of date, and is kept in
the project's history.

The commands run on the [home page's](../index.md) tensile example, in its
last step: three specimens, their readings, their curves and their results.

## Change a value

`set` names the field, an `=`, and the new value. Without `--write` it only
shows the change; with it, it writes:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md set grade=S355J2 --write
samples/steel-01.md · 1 change

  grade   S355  →  S355J2

  nothing becomes not current

written: samples/steel-01.md
```

The preview says what the change makes out of date, here nothing: no formula
reads the grade. Several fields change at once, `set grade=S355J2
tested=2026-03-05`, written together or not at all. A filter changes several
samples, each previewed, and asks once: `samplekit samples set
supplier=acme -f 'material == steel'`.

What a field holds decides how it is written:

| To write | Write |
| --- | --- |
| a number, in the unit the project declares | `area=25.0` |
| its uncertainty | `area.u=0.05` |
| repeated measurements | `thickness.readings=1.991,1.994,1.989` |
| a text, a date | `grade=S355J2`, `tested=2026-03-05` |
| not applicable | `direction=n/a` |
| nothing: the field removed | `direction=` |

A decimal is written with a point, never a comma. A unit is never converted:
`set` writes the number in the unit the project declares for the field
([units are labels](../concepts/values-and-uncertainty.md#units-are-labels)).
`n/a` says there is nothing to measure, where an empty field says nobody
entered it ([no value, not applicable,
zero](../concepts/values-and-uncertainty.md#no-value-not-applicable-zero)).

A value that a formula computes can be written too: it is then edited, typed
over its formula,
which `compute` keeps until `--force` gives it back to its formula ([written
by hand](../index.md#written-by-hand)).

## Measured several times

Readings replace one another as a list: the three readings written are the
three the property holds. Their value and their uncertainty are the
statistics the model declares, and wait for `compute` ([readings and
statistics](../concepts/readings-and-statistics.md)):

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md set thickness.readings=1.991,1.994,1.989 --write
samples/steel-01.md · 1 change

  thickness.readings   1.995, 2.004, 2.008  →  1.991, 1.994, 1.989
                       3 readings: 1.991, 1.994, 1.989, their mean 1.9913
                       they replace the 3 readings it held
                       its value and its uncertainty, their mean and standard error, wait for samplekit compute

  this makes 18 values not current
    area               outdated — thickness
    tensile_strength   outdated — curve.stress (itself not current)
    thickness          outdated — readings
    yield_strength     outdated — young_modulus (itself not current), curve.stress (itself not current)
    young_modulus      outdated — curve.stress (itself not current)
    curve.stress       outdated — area (itself not current) (13 rows)

  samplekit samples/steel-01.md compute --write recomputes them

written: samples/steel-01.md
```

One number is not a list of readings: `thickness.readings=1.99` is refused,
saying to write `thickness=1.99`.

## A row of a table

A cell is named by its table, its column and its row's index, between
brackets; quote it, since a shell reads brackets:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/aluminium-01.md set 'curve.force[10]=7.95' --write
samples/aluminium-01.md · 1 change

  curve.force[10]   7.927 kN  →  7.95 kN

  this makes 4 values not current
    tensile_strength   outdated — curve.stress (itself not current)
    yield_strength     outdated — young_modulus (itself not current), curve.stress (itself not current)
    young_modulus      outdated — curve.stress (itself not current)
    curve.stress       outdated — curve.force (1 row)

  samplekit samples/aluminium-01.md compute --write recomputes them

written: samples/aluminium-01.md
```

`--add-row` adds a row, each assignment naming one of its columns. What the
model computes in a row, here the stress, is computed by `compute`:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/aluminium-01.md set --add-row curve strain=11 force=7.8 --write
samples/aluminium-01.md · a row in curve

  curve.strain   —  →  11
  curve.force    —  →  7.8
  computed by the model   stress

  this makes 4 values not current
    elongation         outdated — curve.strain
    tensile_strength   outdated — curve.stress
    yield_strength     outdated — curve.strain, curve.stress, young_modulus (itself not current)
    young_modulus      outdated — curve.strain, curve.stress

written: samples/aluminium-01.md

  samplekit samples/aluminium-01.md compute --write   fills what the model derives from it
```

## Tags

A tag is a word that groups samples across their attributes: a reference
specimen, a test to redo. `tag add`, `tag remove` and `tag rename` change the
samples selected, and `tags has` selects by them:

<!-- run: tensile/04-results -->
```console
$ samplekit samples tag add reference -f 'material == steel' --write
tag +reference · 1 of 1 sample

  samples/steel-01.md   +reference

written: samples/steel-01.md
$ samplekit -f 'tags has reference' -c grade,tags
name       grade   tags
────────────────────────────
steel-01   S355    reference

1 of 3 samples
```

`samplekit list tags` lists the tags in use.

## A new sample

`new` makes a sample in the folder named, from the fields given. A name the
project or its model declares as a property is written as one, with its unit:

<!-- run: tensile/04-results -->
```console
$ samplekit samples new steel-02 material=steel grade=S355 direction=T --write
samples/steel-02.md · new sample

  material    —  →  steel
  grade       —  →  S355
  direction   —  →  T

written: samples/steel-02.md

  samplekit samples/steel-02.md set <field>=<value>   fill a value in
  samplekit samples/steel-02.md compute --write        run the model on it
```

`--keep` takes fields from a sample already there, and nothing else:
`samplekit samples new steel-03 --keep samples/steel-02.md material,grade
--write`. From Python, `sk.Sample.new(path)` makes one, and `save()` writes
it ([work from Python](work-from-python.md)).

## The note

The text below a sample's frontmatter is its note, read as it is written.
`set note=…` replaces it, and `note=` empties it; an editor changes it as
well:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/copper-01.md set 'note=Necked at mid-length, broke in the gauge.' --write
samples/copper-01.md · 1 change

  note   Tested at 2 mm/min, across the rolling direction.  →  Necked at mid-length, broke in the gauge.

  nothing becomes not current

written: samples/copper-01.md
```

## Remove a sample

`remove` deletes the sample files named, or those a filter selects in a
folder; a folder alone is refused. Their own files, the photographs and
the machine's files, stay. The history keeps what is removed, and
`samplekit restore` gives it back:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/copper-01.md remove
remove · 1 sample file

  samples/copper-01.md

their own files stay: 2 files under [collection] files
samplekit restore gives it back, as it was

apply these changes? [y/N] n
nothing written
```

The links of the project leading to a sample removed are listed in its
preview and left as written; `validate` reports them after ([a link that
leads nowhere](link-samples.md#4-a-link-that-leads-nowhere)). From Python,
`sample.note` is the note, set as any attribute, and `sample.remove()`
deletes the file.

## Rename a sample

`rename` gives a sample a new file's name and keeps what leads to it: its own
files are renamed with it, a folder named after it too, its `name` where it
was the file's name, and every link of the project naming it is written to
name it again. In the last step, the aluminium and copper specimens link to
the steel one, `steel-01`, which is renamed:

<!-- run: tensile/04-results -->
```console
$ samplekit samples/steel-01.md rename steel-02
samples/steel-01.md · rename to steel-02.md

  file         samples/steel-01.md  →  samples/steel-02.md
  name         steel-01  →  steel-02
  files        machine/steel-01  →  machine/steel-02
  files        photos/steel-01-fracture.jpg  →  photos/steel-02-fracture.jpg

  2 links leading to it, written again:
  samples/aluminium-01.md
    compared_with  [[steel-01]]  →  [[steel-02]]
  samples/copper-01.md
    compared_with  [[steel-01]]  →  [[steel-02]]

apply these changes? [y/N] n
nothing written
```

With `--write`, it is made. The history keeps the samples as they were, and
`restore` gives them back; their own files are not in the history, so
renaming back, `samplekit samples/steel-02.md rename steel-01 --write`, is
what takes the whole change back. In the terminal interface, `R`; from
Python, `sample.rename("steel-02")`. Links have [a guide of their
own](link-samples.md).

## In a text editor

A sample is a text file, and an editor may change it: SampleKit reads it as
it is at the next command. Three things to know:

- **What a change makes out of date is judged by the values**, wherever the
  change came from: `samplekit status` says it after an edit as after `set`.
- **`validate` finds a mistake typed by hand**: a frontmatter that does not
  read, a unit that contradicts the other files, a value outside its range
  ([checking them](../index.md#checking-them)).
- **The next write puts the frontmatter in one fixed form**, and drops a
  comment between the `---` lines; the note below them is kept as written
  ([what SampleKit writes](../concepts/samples-and-files.md#what-samplekit-writes-and-what-it-never-touches)).

The history keeps such an edit too, as *changed outside SampleKit*, at the
next command that writes.

## In the terminal interface

On a sample's screen, `e` enters the typing mode, where what you type
replaces the cell under the cursor, as in a spreadsheet: a value with its
uncertainty, `12.6 +- 0.02`, an attribute's name or value, a table's cell; the
arrows go from cell to cell and `Esc` leaves. `i` opens a value's card: what
it reads and what reads it, each typed in there too. `A` adds an attribute, `N` edits
the note, and `→` unfolds the table under the cursor, where the empty row
adds one. The list of samples is typed in the same way, a column for several
samples at once. There, `N` makes a new sample, `t` tags the samples `Space`
put in the basket, or renames a tag, `old=new`, `R` renames a sample and `D`
removes one. Nothing is written at once: every change waits, shown as it will
be, until `w` writes them all, one change of the history; `u` takes back the
last change waiting ([work in the terminal interface](terminal-interface.md),
[the TUI's keys](../reference/tui.md)).

## From Python

The same changes from Python, where a sample changes as an object does: an
attribute set, readings given as a list, a tag appended, a row added to a
table with `add`. `not_current()` says what the change made out of date,
`compute()` runs the model's formulas, and nothing is written before `save()`
([work from Python](work-from-python.md)):

```{literalinclude} ../../examples/tensile/04-results/scripts/edit.py
:language: python
:caption: scripts/edit.py
```

<!-- run: tensile/04-results -->
```console
$ python scripts/edit.py
{'thickness': 'outdated', 'area': 'outdated', 'young_modulus': 'outdated', 'yield_strength': 'outdated', 'tensile_strength': 'outdated', 'curve.stress': 'outdated'}
12.0 % 318 MPa
```

## Take a change back

`samplekit restore` takes the last change back, and `restore --at` puts files
back as the history kept them at a change or on a day; both preview first
([the history](../concepts/history.md)).

## See also

- [`set`, `new`, `tag`, `remove`](../reference/cli.md), every option.
- [What a sample file may hold](../reference/sample-files.md).
