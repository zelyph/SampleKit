# Share a project

This guide is for working on one project from two computers, or with a
colleague: the same samples, the same model, one history. A project is a
folder of text files, so git, or a synchronised folder, carries it as it is.

## 1. Say that the history is shared

SampleKit keeps a history of every change, in `.samplekit/history/`. By
default it hides that folder from git; to share it with the samples, add to
the project's `.samplekitrc`:

```toml
[history]
shared = true
```

SampleKit then lets git see what must travel, and keeps out what each
computer makes for itself. A project that imports this configuration shares
its history too.

## 2. Put the project in git

In the project's folder:

```sh
git init
echo ".venv/" > .gitignore
git add -A
git commit -m "The samples as they stand"
```

| Committed | Left out |
| --- | --- |
| the sample files, `.samplekitrc`, the model | `.venv/`, the Python environment: each computer makes its own |
| the history, `.samplekit/history/` | what SampleKit makes on each computer, which it leaves out itself |
| a sample's own files (photos, reports), if they are small enough for git | exports and figures, which a command makes again; or commit them, if a reader needs them without SampleKit |

From then on, `git add -A` takes your samples and the history together. Each
computer writes only its own part of the history, so `git pull` merges it
without a conflict, and the next change after a pull joins the other
computer's ([The history](../concepts/history.md)). SampleKit never
reads or changes your own repository.

A folder synchronised by Syncthing or a shared drive needs nothing of this:
the whole project folder, `.samplekit/history/` included, is carried as it
is. Where two computers changed one file at once, the synchroniser keeps the
other version beside it, `steel-01.sync-conflict-….md` or `steel-01
(conflicted copy …).md`, and SampleKit leaves that copy out of every command,
saying so each time: compare the two, keep one, and remove the other. OneDrive
names its copy after the computer, `steel-01-LAPTOP.md`, which no rule tells
from a sample's own name: such a copy is read as a sample of its own, and
`validate` finds its name twice.

To give an account of the history to someone without SampleKit, write it as
Markdown:

<!-- run: tensile/04-results -->
```console
$ samplekit log --export -o HISTORY.md --write
written: HISTORY.md
```

The account lists each snapshot with its date, what wrote it, every file it
changed, and the script that made it when a script did.

## 3. On the other computer

Clone the project before SampleKit writes anything there, so that the
computer's history starts from the one git brings rather than from a first
snapshot of its own.

```sh
git clone YOUR-REPOSITORY NAME
cd NAME
python -m venv .venv
.venv/bin/pip install samplekit==VERSION    # what samplekit --version says
samplekit samples status
```

`status` judges each computed value by the inputs its file records, so it
tells what is current on any computer. Which version of the formula computed
each value is kept in the project's history: with the history shared as in
step 2, a formula a colleague edited is said *formula changed* on your
computer as on theirs, and the values they computed again are current.

Without the history shared, a value another computer computed is judged by
its inputs alone, and `status --exit-code` says how many values it cannot
vouch for this way. When a change to the model arrives from someone else, compute them again
on purpose:

```console
$ samplekit samples compute --rerun --write
```

A formula edited on your own computer needs none of this: SampleKit notices
the edit, and `compute --write` recomputes what that formula gave.

## Working at the same time

Two people changing the same sample file make a git conflict, as with any
text file. Conflicts stay small when each person keeps to their own samples,
or their own project, side by side under one root and read together
([several projects](several-projects.md)).

**A change to the model is computed once, by one person.** A formula edited
makes `compute --write` rewrite every sample that formula computes: a
measurement a colleague took meanwhile on one of those samples then conflicts
with the recomputed file. Commit the model, let everyone pull it, and have one
person compute and push the result, or compute on one computer only, and let
the others pull.

## See also

- [The history](../concepts/history.md), and what it keeps.
- [Several projects](several-projects.md), and [projects](../concepts/projects.md).
- [`log`, `diff`, `restore`](../reference/cli.md).
