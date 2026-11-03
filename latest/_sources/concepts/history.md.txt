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

**Each file is written once and never changed.** A snapshot writes each
version of a file whole, named by its content, and removes nothing: that is
what lets git or a synchroniser carry the history between computers without
a conflict. A year of daily work on a collection of seventy samples is about
35 MB; where git holds the history, it packs it in its own repository.

## Reading it without SampleKit

Your data never depends on SampleKit being there to read it. The samples
are Markdown files any editor opens, and the history is an ordinary Git
repository: should SampleKit one day stop running on your computer, or stop
existing, `git` reads every snapshot, and gives back any state the project
was in.

- **`.samplekit/history/` is a bare Git repository**: its files are git's
  own, and no copy of your samples sits beside them.
- **A snapshot is a commit.** Its files are the project's at that moment,
  `.samplekitrc`, the samples and the model's source, at the paths they have
  in the project; its message is the command that made it, or *changed
  outside SampleKit*; its date is when it was taken.
- **Each computer's snapshots are a branch**, named by the computer's
  identifier, so that `--all` reads every computer's.
- Tags named `trace/…` hold the record of which formula computed which value;
  nothing in your samples needs them to be read.

From the project's folder, with git installed:

```console
$ git --git-dir=.samplekit/history log --all --format="%h  %ad  %s" --date=short
5452402  2026-10-06  samplekit compute --write
0d4bf40  2026-10-06  samplekit samples/steel-01.md set thickness.readings=1.991,1.994,1.989 --write
246d92a  2026-10-06  samplekit samples compute --write
e84eab2  2026-10-06  the project as SampleKit first kept it
```

Each line is a snapshot: its identifier, the start of the one `samplekit log`
gives, its day, and the command that made it. With it:

| To | Run |
| --- | --- |
| see what a snapshot changed | `git --git-dir=.samplekit/history show --stat 0d4bf40` |
| compare two snapshots, line by line | `git --git-dir=.samplekit/history diff 246d92a 5452402` |
| read one file as it was | `git --git-dir=.samplekit/history show 0d4bf40:samples/steel-01.md` |
| get a whole snapshot back, in a folder of its own | `git clone .samplekit/history recovered`, then `git -C recovered checkout 0d4bf40` |

A clone holds the files of the snapshot checked out, as the project had them:
the samples, `.samplekitrc` and the model. Nothing in it is SampleKit's own
format. A computed value is in its sample file as SampleKit wrote it, beside
a short record of what it was computed from, which you can ignore: the
numbers are there, with their uncertainties and their units.

`samplekit log --export -o HISTORY.md --write`, while SampleKit runs, writes
the same account as a Markdown file anyone reads: each snapshot, its date,
its computer, the command, and the files it changed.

## When a snapshot is taken

- **After each write**: a command run with `--write`, a change written in the
  TUI, a restore. Its message is the command line as you typed it, or the
  TUI's action.
- **A change made outside SampleKit**, in your text editor for example, is
  kept as a snapshot of its own, *changed outside SampleKit*, just before the
  next write. So an edit is never credited to the command that follows it.
- **A change to the model** is a snapshot of its own before that one, *the
  model changed*, naming the formulas that changed when a computation follows
  (*the model changed: young_modulus*). The snapshot keeps the model as it was, so what
  an earlier version of a formula computed can be read, and computed again,
  from it.
- **A Python script's writes** are one snapshot, kept when the script ends,
  named by its command line, **with the script's source**: `samplekit log
  --script N` prints the script as it ran. `sk.keep("message")` keeps what the
  script has written so far as a snapshot of its own. A script that writes an
  export or a figure file is a snapshot too; one that only prints, or only
  shows a figure in a window, is not.
- **A script of the model run by `samplekit run`** is a snapshot named by its
  command, with what it asked with `input()` and the answers it was given:
  *samplekit run scale --write — answered 'Factor?' 0.5*.
- **A write that changed nothing** leaves no trace.
- **Each command keeps its own files.** Two commands writing at once, a
  long `compute --write` and a `set` in another terminal, each keep a
  snapshot of the files they wrote, under their own names.
- **A write that was interrupted**, the command killed or the power cut, is
  kept by the next write under its own name, marked *interrupted*. Ctrl-C
  during `tag` or `restore` ends the file being written and stops there,
  saying which files were written.

A snapshot that fails is a warning, and the write it follows stands: the
history never stands in the way of the data.

## Looking back

| To… | Command line | TUI | Python |
| --- | --- | --- | --- |
| list the snapshots | `samplekit [FILE or FOLDER] log` | `H` | |
| see what changed, value by value | `samplekit diff [--from A] [--to B]` | `H`, then the snapshot; `d` on two, compared | |
| put files back | `samplekit PATH restore [--at A] --write` | `u`; `r` in `H` | |
| read the project as it was | any reading command with `--at A` | | |
| trace an exported file | `samplekit FILE explain` | | |

**`log`** lists the snapshots, newest first and numbered from 1: when, what
wrote, which files changed. Given a file or a folder, it lists only the
snapshots that changed it, with the same numbers. From a folder above several
projects, it lists each project's history under its name; a number is then
one project's, so `--at`, `--from` and `--to` name the project too, `samplekit
cast log --at 1` ([several projects](../guides/several-projects.md)).

**`diff`** compares two states **value by value**, in the project's units
and precisions, not line by line: `area 25.056 ± 0.053 mm² → 24.918 ± 0.028
mm²`. Alone,
it shows the last change kept; `diff --from 1` shows what changed since the
last snapshot, such as an edit in your editor.

A state is named by `now`, a number from `log`, the start of a snapshot's id,
or a date: `2026-09-12` (the end of that day) or `'2026-09-12 14:30'`. A date
names the last snapshot at or before it. Tab completes them.

**`restore`** puts files back. Without `--at`, it **takes back the last
change SampleKit wrote on your computer** in what you name, value by value:
what was changed since, by hand or by a colleague whose work you pulled,
stays. Where one of those later changes touched a value that change wrote,
nothing is taken back, and `restore` names the value. A file you name that you
deleted, or changed outside SampleKit, comes back as it was last kept. With
`--at`, it puts the files back as that snapshot held them, every change since
included. Like every command that writes, it shows the change
first and asks before writing it; `--write` writes without asking. A restore is itself a snapshot, so it can be
taken back too.

In the TUI, `u` takes back the last change of the session, and once the
session's changes are all taken back, goes on with the project's history: the
last change SampleKit kept, even from yesterday, or from the command line.

## The project as it was

**`--at`** runs a reading command over the project as a snapshot held it: its
samples, its `.samplekitrc` and its model then.

```console
$ samplekit -c material,tensile_strength --at 2026-10-06
$ samplekit export paper --at 3 -o results-then.tex --write
$ samplekit samples/steel-01.md explain area --at 3
```

Your project is not changed: what the command writes goes where `-o` says.
An export made again this way is the same file, byte for byte. A figure is
drawn from the same data, settings and model, and with the same matplotlib
it is the same file too; matplotlib itself may have changed since, so it can
differ in small ways. Commands that change the
project (`set`, `new`, `tag`, `remove`, `rename`, `compute`, `run`, `init`)
refuse `--at`.

## Where a file came from

An export, a table written with `-o`, or a figure written to a file is
**recorded** in the history when it is written: what wrote it, when, where,
and from which samples. The file itself is not kept, only a digest of its
exact content. So `samplekit explain` finds it again, even copied into a
manuscript's folder and renamed:

```console
$ samplekit ../paper/figure-3.pdf explain
../paper/figure-3.pdf is kept by the project in .
../paper/figure-3.pdf
  made      2026-10-06 20:36, by samplekit plot curves -o curves.pdf --write
  written   curves.pdf
  from      snapshot #3, 3 samples

since then
  samples/steel-01.md
    curve.force[24]    11.07 kN  →  11.2 kN
    curve.stress[24]   441.8 MPa  →  447.0 MPa

it is not current — samplekit plot curves -o curves.pdf --write makes it again
made again as it was, from the project's folder: samplekit plot curves -o curves.pdf --write --at 388c00c
```

It says what changed since in what the file was made from, and so whether the
file is still current, and gives the command that makes it again, as it was
(`--at`) or as the project is now. A file changed since, cropped or converted,
no longer matches its digest; a figure also carries its snapshot's number in
its metadata, which survives some of these changes. A figure saved from
matplotlib's window was not written by SampleKit, and is not recorded.

## A folder made a project

A project keeps the history of the files it holds itself: a folder below it
with a `.samplekitrc` of its own is a project of its own, with its own
history. **The snapshots the project above took of that folder's files, from
before it was a project, come with it**: the first command that reaches the
folder (a read, a write, `log`, the TUI opening it, a script saving there)
copies them into its history and says so. Here `samples/aluminium` was given a
`.samplekitrc` of its own:

```console
$ samplekit samples/aluminium log
3 snapshots of samples/aluminium brought from the history of ., a project of its own now
#   when               what                                                                    changed
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
1   2026-10-06 20:37   the folder made a project of its own                                    .samplekitrc, @model/specimen.py
2   2026-10-06 20:37   samplekit samples/aluminium/aluminium-01.md set grade=6082-T6 --write   aluminium-01
3   2026-10-06 20:37   samplekit samples/aluminium/aluminium-01.md tag add reference --write   aluminium-01
4   2026-10-06 20:37   changed outside SampleKit                                               aluminium-01
```

Each copy keeps its date, its message and its computer, and holds that
folder's files alone; one more snapshot, *the folder made a project of its
own*, keeps its configuration as it now is. The project above keeps them all,
and shows none of them any more: `samplekit samples/aluminium log` reads them.
Nothing is asked, and nothing above is rewritten, so that no other computer's
history, no `--at` and no record of a file written changes. A command run
from the project above that changes samples of the one below is kept in the
history of the one below.

## Several machines

A project you work on from two computers, or with a colleague, keeps **one
history**. Each computer writes its snapshots on a line of its own in
`.samplekit/history/`, and never writes another's.
So the history can travel with the project, by a synchronised folder
(Syncthing, a shared drive) or by git ([Share a
project](../guides/share-a-project.md)), without two computers ever writing
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
other. Your own computer is *you*:

```console
$ samplekit log
#   when               machine          what                                                      changed
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
1   2026-10-06 20:36   you, joins ana   samplekit samples/steel-01.md tag add reference --write   steel-01
2   2026-10-06 20:36   ana, joins you   samplekit samples/copper-01.md set grade=C10100 --write   copper-01
3   2026-10-06 20:36   you              samplekit samples/steel-01.md set grade=S275 --write      steel-01
4   2026-10-06 19:58   you              samplekit samples compute --write                         aluminium-01, copper-01, steel-01
5   2026-10-06 19:58   you              the project as SampleKit first kept it                    aluminium-01, copper-01, steel-01, .samplekitrc, model/specimen.py
```

A history only one computer wrote has no such column. The numbers are places
in the list: a snapshot arriving from the other computer takes its place by
date and moves the numbers below it, as every new snapshot does. The start of
a snapshot's id never moves; use it when a number might have. `diff`,
`restore` and `--at` read every computer's snapshots. **Undo** (`u` in the
TUI) and `restore` without `--at` take back only a change made on your
computer; take back another computer's with `restore --at`.

Each computer is told apart by an **identifier** SampleKit draws the first
time it writes, and keeps in its own state folder (`~/.local/state/samplekit/`
on Linux), so that two computers set up from one disk image, which share a
host name, keep two lines. Another computer is shown by its host name
(`toms-macbook` for `Toms-MacBook.local`), or by the name you give it in the
TUI's history (`H`), which asks when a computer it does not know yet has
written: that name stays on your computer. `SAMPLEKIT_MACHINE`, if you set it,
is used instead of the identifier and of the host name. A line rc.1 kept
under your host's name is shown as another computer's, by that name, and
your next write joins it ([Upgrade](../guides/upgrade.md)); nothing in the
history is rewritten.

To give an account of the history to someone without SampleKit, `samplekit
log --export` writes it as readable Markdown: each snapshot with its date, its
computer by host name, what wrote it, the files it changed, and the script that made it.

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

- Practised on the [home page](../index.md#the-history-of-every-change).
- `log`, `diff`, `restore`, `explain`, `--at`: [Command line
  reference](../reference/cli.md); the TUI's history screen: [TUI
  reference](../reference/tui.md).
