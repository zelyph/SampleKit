# Step 1 · First pieces

In this step you look at three pieces, each written by hand in a text file, and
read them with your first commands: a list, a table, one piece whole, a filter.

Run every command from this folder, `01-first-pieces`.

## What is here

```text
01-first-pieces/
  .samplekitrc          marks this folder as a project
  README.md             this page
  pieces/
    alpha-titanium.md        one piece
    farmhouse-steel.md
    oxidised-nickel.md
```

`.samplekitrc` is the project's configuration. For now it says only two
things: read the subfolder `pieces/` too, and skip this `README.md`, which is
not a piece.

```toml
schema_version = 1

[collection]
recursive = true
exclude = ["README.md"]
```

Each piece is one Markdown file. Open `pieces/alpha-titanium.md` in any text editor:

```text
---
schema_version: 1
name: alpha-titanium
style: titanium
process: UT-05
poured: 2026-01-24
properties:
  radius: 1.064
  notch: 1.011
  volume: {v: 20.5, unit: L}
---
# Alpha TITANIUM

Dry treated on day 4 with 100 g of Alpha.
```

- Between the two `---` lines is the **frontmatter**, which SampleKit reads.
- `style`, `process` and `poured` are **attributes**: words, dates or numbers
  that describe the piece.
- `radius` (the original hardness), `notch` (the final hardness) and `volume` are
  **properties**: what was measured. A property is a bare number, like
  `radius`, or a small mapping that adds a unit, like `volume`. `v` is the value.
- After the second `---` is the **note**. It is yours: SampleKit shows it and
  never changes it.

## Try it

```console
$ samplekit pieces                       # the pieces' files
$ samplekit pieces -c style,radius,notch        # a table of what they hold
$ samplekit pieces/alpha-titanium.md view     # one piece, whole
$ samplekit pieces -f 'radius > 1.055'       # the strong ones
$ samplekit pieces list                  # what the project holds
```

## What you see

`samplekit pieces` prints the path of each piece, one per line. `-c` (for
*columns*) turns the list into a table:

```text
style       radius      notch
──────────────────────
titanium      1.064   1.011
steel   1.052   1.004
nickel    1.058   1.016
```

In a terminal, each row also starts with the piece's name, in grey. When the
output goes to a file or to another program, the name is not added: you get
exactly the columns you asked for.

`view` shows one piece whole:

```text
alpha-titanium
  tags     —
  style    titanium
  process    UT-05
  poured   2026-01-24
  radius       1.064
  notch       1.011
  volume   20.5 L
```

`-f` (for *filter*) keeps the pieces that match a condition: here the TITANIUM and
the nickel, whose original hardness is above 1.055. Quote the whole filter with
single quotes, so that the shell leaves it alone. `list` counts what the
project holds, and names the command that lists each kind.

## The TUI

`samplekit` alone, in a terminal, opens the **TUI**: SampleKit in full screen,
driven by the keyboard. Its start page offers the project you are in; press
`Enter` to open it.

```console
$ samplekit
```

Move with the arrow keys, open a piece with `Enter`, go back with `Esc`. `?`
lists the keys of the screen you are on, and `q` goes back to the page you
came from — the start page, or the demo's page of steps — where `q` goes back
again, until it quits. Step 7 tours the TUI.

## Help

```console
$ samplekit --help            # the commands
$ samplekit view --help       # one command, with examples
$ samplekit view -h           # a shorter summary
```

## Why

The files are the only store: no database, nothing hidden. What you measured
stays readable in any editor and in git, and SampleKit only reads it — until
you ask it to write.

- To understand: [a sample and its file](https://zelyph.github.io/SampleKit/latest/explanations/samples-and-files.html).
- To look up: [the commands](https://zelyph.github.io/SampleKit/latest/reference/cli.html),
  [what a sample file can hold](https://zelyph.github.io/SampleKit/latest/reference/sample-files.html).

Next: [Step 2 · Measuring](../02-measuring/README.md), where each number gets
its unit and its uncertainty.
