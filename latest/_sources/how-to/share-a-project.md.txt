# Share a project

This recipe puts a project in git, so that a colleague, or you on another
computer, works on the same samples, with one history. SampleKit needs little
for it: a project is a folder of text files, and git carries them as they
are.

## 1. What goes in the repository

| Commit | Leave out |
| --- | --- |
| the sample files | `.venv/`, the Python environment: each computer makes its own |
| `.samplekitrc` | `.samplekit/failures/`, the records of failures, and `.samplekit/model.json`, the model's description: kept per computer, and made again from the model where it is missing |
| the model and the files it imports | exports and figures, which a command makes again — or commit them, if a reader needs them without SampleKit |
| a sample's own files (photos, reports), if they are small enough for git | |
| `.samplekit/history/`, SampleKit's history, once step 2 is done | |

In the project's folder:

```sh
git init
printf '.venv/\n.samplekit/failures/\n.samplekit/model.json\n' > .gitignore
git add .gitignore .samplekitrc model samples
git commit -m "The samples as they stand"
```

SampleKit never reads or changes your repository.

## 2. Share the history too

The history, `.samplekit/history/`, hides itself from git with a file of one
line, `.samplekit/history/.gitignore`, which holds `*`. Replace that line, and
git carries the history with the samples:

```sh
printf '.tmp*\n*.lock\n' > .samplekit/history/.gitignore
git add .samplekit/history
git commit -m "The history too"
```

The two patterns left out are the files SampleKit has open while it writes.
From then on, commit `.samplekit/history/` with your samples: `git add -A`
takes both. Each computer writes only its own part of the history, and every
other file in it is named by its content and never changed, so `git pull`
merges it without a conflict. After a pull, the next change you make joins
the other computer's snapshots ([The history](../explanations/history.md)).

A folder synchronised by Syncthing or a shared drive needs nothing of this:
the whole project folder, `.samplekit/history/` included, is carried as it
is.

To give an account of the history to someone without SampleKit, write it as
Markdown:

<!-- run: 04-computing -->
```console
$ samplekit log --export                        # a preview, printed
$ samplekit log --export -o HISTORY.md --write
```

The account lists each snapshot with its date, what wrote it, every file it
changed, and the script that made it when a script did.

## 3. On the other computer

Clone the project before SampleKit writes anything there, so that the
computer's history starts from the one git brings rather than from a first
snapshot of its own.

```sh
git clone YOUR-REPOSITORY project
cd project
python3 -m venv .venv
.venv/bin/pip install samplekit==VERSION    # what samplekit --version says
samplekit status samples
```

`status` judges each computed value by the inputs its file records, so it
tells what is current on any computer. One thing it cannot tell for a value
another computer computed: which version of the formula computed it, since
that is recorded on the computer that ran it, not in the files. When a change
to the model arrives from someone else, compute its values again on purpose:

```console
$ samplekit compute samples --rerun --write
```

A formula edited on your own computer needs none of this: SampleKit notices
the edit, and `compute --write` recomputes what that formula gave.

## Working at the same time

Two people changing the same sample file make a git conflict, as with any
text file. Conflicts stay small when each person keeps to their own samples,
or their own project: the tutorial's [step 7](../tutorial/07-two-brewers.md)
keeps two projects side by side with one model between them, and reads them
together.

## See also

- [The history](../explanations/history.md), and what it keeps.
- [Projects, and a collection over several](../explanations/projects.md).
- [`log`, `diff`, `restore`](../reference/cli.md).
