# History

Every time SampleKit writes to a project, it first keeps what the project
held. You can then see what changed and when, put a file back as it was, read
the whole project as it stood on a given day, and find out where an exported
table or a figure came from. This page says what the history keeps, when,
and what it is not.

## Why SampleKit keeps a history of its own

A value changed by mistake, a computation run over the wrong samples, a
figure in a manuscript that nobody can trace back to its data: these are the
losses that cost most, and they happen to careful people. Git would answer
them, but many researchers do not use Git, and those who do do not commit
after every command. So SampleKit keeps the history itself, without asking
you for anything: no Git to install, nothing to set up, and it cannot be
turned off.

## What it is

The history lives in `.samplekit/history/`, beside the project's
`.samplekitrc`. A project, that is a folder with a `.samplekitrc`, is needed:
a folder of samples without one has no history, and a command that asks for
it says so.

A **snapshot** is the state of the project at one moment:

| Kept in a snapshot | Not kept |
| --- | --- |
| `.samplekitrc` | a sample's own files (photos, reports, raw data) |
| every sample file of the project, in every subfolder | exports and figures |
| the model's source files, even when they sit outside the project | the Python environment |

What is not kept is either not data SampleKit reads, or can be large, or can
be made again. Exports and figures are not kept, but they are **recorded**:
see [Where a file came from](#where-a-file-came-from).

The history is a Git repository, so any `git` can read it if you have one.
It is SampleKit's own: your own Git repository, if the project has one, never
sees it (it ignores itself) and is never changed by it.

## When a snapshot is taken

- **After each write**: a command run with `--write`, a change written in the
  TUI, a restore. Its message is the command line as you typed it, or the
  TUI's action.
- **A change made outside SampleKit**, in your text editor for example, is
  kept as a snapshot of its own, *changed outside SampleKit*, just before the
  next write. So an edit is never credited to the command that follows it.
- **A Python script's writes** are one snapshot, kept when the script ends,
  named by its command line, **with the script's source**: `samplekit log
  --script N` prints the script as it ran. `sk.keep("message")` keeps what the
  script has written so far as a snapshot of its own. A script that writes an
  export or a figure file is a snapshot too; one that only prints, or only
  shows a figure in a window, is not.
- **A write that changed nothing** leaves no trace.

A snapshot that fails is a warning, and the write it follows stands: the
history never stands in the way of the data.

## Looking back

| To… | Command line | TUI | Python |
| --- | --- | --- | --- |
| list the snapshots | `samplekit log [FILE or FOLDER]` | `H` | |
| see what changed, value by value | `samplekit diff [--from A] [--to B]` | `H`, then the snapshot | |
| put files back | `samplekit restore PATH [--at A] --write` | `u` | |
| read the project as it was | any reading command with `--at A` | | |
| trace an exported file | `samplekit explain FILE` | | |

**`log`** lists the snapshots, newest first and numbered from 1: when, what
wrote, which files changed. Given a file or a folder, it lists only the
snapshots that changed it, with the same numbers.

**`diff`** compares two states **value by value**, in the project's units
and precisions, not line by line: `abv 6.8 ± 0.1 % → 7.2 ± 0.1 %`. Alone,
it shows the last change kept; `diff --from 1` shows what changed since the
last snapshot, such as an edit in your editor.

A state is named by `now`, a number from `log`, the start of a snapshot's id,
or a date: `2026-09-12` (the end of that day) or `'2026-09-12 14:30'`. A date
names the last snapshot at or before it. Tab completes them.

**`restore`** puts files back. Without `--at`, it takes back the last change
kept of what you name; a file you deleted, or changed outside SampleKit,
comes back as it was last kept. With `--at`, it puts the files back as that
snapshot held them. Like every command that writes, it shows the change
first, and writes with `--write`. A restore is itself a snapshot, so it can be
taken back too.

In the TUI, `u` takes back the last change of the session, and once the
session's changes are all taken back, goes on with the project's history: the
last change SampleKit kept, even from yesterday, or from the command line.

## The project as it was

**`--at`** runs a reading command over the project as a snapshot held it: its
samples, its `.samplekitrc` and its model then.

```console
$ samplekit brews -c name,abv --at 2026-09-12
$ samplekit export overview brews --at 3 -o overview-then.csv --write
$ samplekit explain brews/dry-stout.md efficiency --at 3
```

Your project is not changed: what the command writes goes where `-o` says.
An export made again this way is the same file, byte for byte. A figure is
drawn from the same data, settings and model; matplotlib itself may have
changed since, so it can differ in small ways. Commands that change the
project (`set`, `new`, `tag`, `compute`, `init`) refuse `--at`.

## Where a file came from

An export, a table written with `-o`, or a figure written to a file is
**recorded** in the history when it is written: what wrote it, when, where,
and from which samples. The file itself is not kept, only a digest of its
exact content. So `samplekit explain` finds it again, even copied into a
manuscript's folder and renamed:

```text
$ samplekit explain ../thesis/figure-3.pdf .
../thesis/figure-3.pdf
  made      2026-09-12 14:03, by samplekit plot score brews -o score.pdf --write
  written   score.pdf
  from      snapshot #7, 12 samples

since then
  brews/dry-stout.md
    abv   4.4 ± 0.1 %  →  5.0 ± 0.1 %

it is not current — samplekit plot score brews -o score.pdf --write makes it again
```

It says what changed since in what the file was made from, and so whether the
file is still current, and gives the command that makes it again, as it was
(`--at`) or as the project is now. A file changed since, cropped or converted,
no longer matches its digest; a figure also carries its snapshot's number in
its metadata, which survives some of these changes. A figure saved from
matplotlib's window was not written by SampleKit, and is not recorded.

## Several machines

A project you work on from two computers, or with a colleague, keeps **one
history**. Each computer writes its snapshots on a line of its own in
`.samplekit/history/`, named after the computer, and never writes another's.
So the history can travel with the project, by a synchronised folder
(Syncthing, a shared drive) or by git ([Share a
project](../how-to/share-a-project.md)), without two computers ever writing
the same file of it.

SampleKit never merges samples: whatever carries your files does (git,
Syncthing). It only notices when they have arrived. When your computer takes
its next snapshot and your files already hold what the other computer kept,
the snapshot **joins** the two lines. What came from the other computer is
not recorded again as *changed outside SampleKit*. If the other computer's
snapshots have arrived but its files have not yet, nothing is joined: your
computer goes on writing its own line, and joins at a later snapshot.

`log` then lists every computer's snapshots together, newest first, with a
`machine` column that names the computer and says where one joined the
other:

```text
#   when               machine          what                                  changed
1   2026-09-27 09:02   tom, joins ana   samplekit set brews/dry-stout.md …    dry-stout
2   2026-09-26 18:40   ana              samplekit compute brews --write       blonde-saison, citra-ipa
3   2026-09-26 16:18   tom              samplekit compute brews --write       blonde-saison, dry-stout
```

A history only one computer wrote has no such column. The numbers are places
in the list: a snapshot arriving from the other computer takes its place by
date and moves the numbers below it, as every new snapshot does. The start of
a snapshot's id never moves; use it when a number might have. `diff`,
`restore` and `--at` read every computer's snapshots. **Undo** (`u` in the
TUI) takes back only a change made on your computer; take back another
computer's with `restore`.

A computer is named after its host name (`toms-macbook` for
`Toms-MacBook.local`), or after `SAMPLEKIT_MACHINE` if you set it. A history
kept by an earlier version of SampleKit becomes the line of the first
computer that writes to it; nothing in it is rewritten.

To give an account of the history to someone without SampleKit, `samplekit
log --export` writes it as readable Markdown: each snapshot with its date, its
computer, what wrote it, the files it changed, and the script that made it.

## Common mistakes

- **Expecting a history in a folder without `.samplekitrc`.** Make it a
  project first (`samplekit init`).
- **Expecting photos or exports to be restored.** They are not kept; exports
  can be made again with `--at`.
- **Treating the history as a backup.** It sits beside your samples, on the
  same disk. Back up the whole folder, `.samplekit/` included, as you would
  anything else.
- **Expecting a join before the files arrive.** A join waits until your
  files hold what the other computer kept; until then, `log` lists its
  snapshots, but your computer does not build on them.
- **Changing the history's folder by hand.** Let git or the synchroniser
  carry it whole; never merge or edit its files.

## Where to go next

- Practised in tutorial step [4](../tutorial/04-computing.md).
- `log`, `diff`, `restore`, `explain`, `--at`: [Command line
  reference](../reference/cli.md); the TUI's history screen: [TUI
  reference](../reference/tui.md).
