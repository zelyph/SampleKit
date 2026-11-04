# Step 7 · Two testers

In this step Ana and Tom each keep their pieces in a project of their own, with
one model between them. You read both projects in one command, then do in the
TUI, with the keyboard, what the earlier steps did with commands.

## What is here

```text
07-two-testers/
  ana/
    .samplekitrc        Ana's project: [model] path = "../shared/piece.py"
    pieces/
  tom/
    .samplekitrc        Tom's project, the same model
    pieces/
  shared/
    piece.py             the model both use
    durometer.py
```

This folder is no project: it only holds the two. A sample follows the nearest
`.samplekitrc` above it, so each piece keeps its own project's units, digits,
queries and profiles.

The states of step 4 are here again, split between the two, and Tom's English
bronze alloy was never computed.

## Try it

Run these from this folder, `07-two-testers`:

```console
$ samplekit ana/pieces tom/pieces status          # both, each by its own rules
$ samplekit ana/pieces tom/pieces --profile modulus
$ samplekit ana/pieces tom/pieces -c project,name,modulus --csv
```

In the terminal, a command over two projects prints one table per project,
headed by its folder:

```text
ana
Piece               Style      Original hardness    Final hardness   Modulus [%]   Reduction [%]
───────────────────────────────────────────────────────────────────────────────────────────────
farmhouse-steel   steel       1.051 ± 0.0003   1.004 ± 0.0003     6.2 ± 0.1                92
…

tom
Piece            Style      Original hardness    Final hardness   Modulus [%]   Reduction [%]
────────────────────────────────────────────────────────────────────────────────────────────
double-titanium      titanium          1.078 ± 0.0007   1.014 ± 0.0006     8.5 ± 0.2                82
…
```

A CSV file is one table: the `project` column tells the rows apart.

## The TUI

```console
$ cd ana
$ samplekit                  # the start page: Enter opens Ana's project
$ samplekit ../tom tui       # Tom's project, directly
```

The TUI opens on the **collection**, one row per piece. Try, in this order:

| Key | What it does |
| --- | --- |
| `/` | type a filter, as after `-f`; `Tab` completes, `Esc` clears it |
| `s` | sort by a column; `r` reverses |
| `c` | choose the columns |
| `g` | group by a field |
| `f` | apply a saved query or profile |
| `Enter` | open the piece under the cursor |
| `N` | a new piece, empty, beside the one under the cursor |
| `Space` | put the piece in the basket: `C`, `p`, `t` then act on the basket alone |
| `e` | in a piece, change the value under the cursor |
| `A` | in a piece, add an attribute: its name, then its value |
| `N` | in a piece, write its note; `Esc` shows the change |
| `→` | in a piece, open the table under the cursor |
| `c` | in a piece, compute the value under the cursor |
| `C` | compute what is not current |
| `v` | the control screen: everything not current or wrong |
| `H` | the history, change by change |
| `u` | take back the last change |
| `p` | a script, a figure or an export — what is shown, to a file |
| `W` | save what is shown as a query, a profile or an export |
| `?` | every key of the screen you are on |
| `q` | back to the start page; `q` again quits |

Whatever you type — a filter, a value, a name, the note — is edited as in a
text editor; a value opens selected, so that what you type replaces it, and an
arrow or `End` keeps it to change: the mouse or `Shift` and the arrows select, `Ctrl` and the arrows
move by word, `Ctrl+Backspace` takes a word back, `Ctrl+Z` undoes. In the note,
`Alt+1` to `Alt+6` make a heading, `Alt+L` a link, and `*` put around a
selection makes it emphasis.

Every change you type is shown before it is written, as the commands' previews
are, and `y` writes it. A computation, `c` or `C`, writes at once: it is kept
in the history, and `u` takes it back. Marks tell a value's state: `⚠` not current, `✎` written over its
formula, `✗` failed, `∅` never computed or waiting.

The TUI remembers, for each folder, the filter, the sort, the grouping, the
columns and the basket, and opens there next time: `x` empties the basket.

## Why

A project is a folder with its own configuration and its own model. Keeping
people or experiments in separate projects lets each keep its rules, and a
command can still read them together without mixing them up.

- To understand: [projects, and a collection over several](https://zelyph.github.io/SampleKit/latest/explanations/projects.html).
- To look up: [every key of the TUI](https://zelyph.github.io/SampleKit/latest/reference/tui.html),
  [which configuration applies](https://zelyph.github.io/SampleKit/latest/reference/configuration.html).
- To do: [share a project](https://zelyph.github.io/SampleKit/latest/how-to/share-a-project.html).

Next: [Step 8 · Python](../08-python/README.md), the same pieces from a script.
