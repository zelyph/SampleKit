# Start a project

This guide is for setting up a folder of your own for SampleKit: the files it
needs, the Python environment your formulas run in, and a first sample. It
assumes SampleKit is installed ([install SampleKit](install.md)).

A **project** is a folder with a `.samplekitrc` in it: the file that says
what the samples have in common, which model computes them, and how values
are shown. Its samples are Markdown files in it, usually in `samples/`
([projects](../concepts/projects.md)).

## Set it up with `init`

In a folder of your own, or naming a new one:

```console
$ samplekit tensile-tests init
init · tensile-tests

  .samplekitrc               the project: its model and its collection, and in comments what else it may say
  model/main.py              the model, empty: it runs, and says in comments how to declare each kind
  samples/                   where the samples go, empty: N in the TUI, or samplekit new, makes one
  .venv/                     the environment the model runs in, with samplekit 1.0.0rc2 installed

  completion, by hand        add to ~/.zshrc:  source <(samplekit completions zsh)

apply these changes? [y/N]
```

Like every command that writes, `init` shows what it would write and asks;
`--write` answers yes beforehand. In a terminal it first asks a few
questions, each with its default marked: what to start from, which model,
and whether to make the environment. The options answer them on the command
line:

| Option | What it does |
| --- | --- |
| `--no-model` | a project without a model: every value typed in the samples, none computed |
| `--model PATH` | a model file you already have, named where it is: nothing is copied |
| `--no-venv` | no Python environment, which reading and writing samples do not need; while a model is declared, `validate` reports the missing environment as a defect |
| `--example` | the example project: a model with one formula of each kind, to read and copy from |

Run in a project that exists, `init` offers only what is missing, such as the
environment.

**In the terminal interface**, `samplekit` alone opens the start page, where
`n` sets up a new project by the same questions, one at a time, and opens it.
**From Python**, a project is the folder and its `.samplekitrc`: `sk.load` reads
it once `init` has written it, and needs nothing else.

## What it writes

- **`.samplekitrc`**, which names the model and its environment, `python =
  ".venv"`, and lists in comments every section the file may hold. Each is
  optional, and explained in [the configuration
  reference](../reference/configuration.md).
- **`model/main.py`**, a model that runs as it is and says in comments how to
  declare each kind of property and formula ([write a model](first-model.md)).
- **`samples/`**, empty.
- **`.venv/`**, a Python environment with SampleKit's package in it, at the
  version of the command line: the only Python SampleKit runs for this project
  ([the Python package](install.md#the-python-package)).

The tab completion line is said, never written: the file it goes in is yours.

## A first sample

`new` makes a sample in the folder you name, from the fields you give. A
field the model declares, or `.samplekitrc` under `[property.NAME]`, is
written as a property, with its unit; any other is written as an attribute
([samples and their files](../concepts/samples-and-files.md)).
The `model/main.py` that `init` writes declares nothing yet, so declare the
width and the thickness first ([write a model](first-model.md), or [bring
existing data in](move-existing-data.md) for `[property.NAME]`). Before
that, the command below writes them as bare numbers, attributes, and warns:
*2 names are a property nowhere here, so they are written as attributes*.

```console
$ samplekit samples new steel-01 material=steel width=12.5 thickness=2.0 --write
```

A sample is also a text file you can write in an editor: two `---` lines,
the data between them, a free note below ([one sample, one
file](../index.md#one-sample-one-file)). `samplekit validate` then checks
every file, and `samplekit` alone opens the [terminal
interface](terminal-interface.md), whose start page sets up a project too,
with `n`.

## The example project

`samplekit example init --example --write`, run beside your project rather
than inside it, writes a small project in a folder `example/`: one tensile specimen and its model, with one formula of each kind, a
figure and a script, and a configuration with a query, a profile, an export
and the units and precisions of every property, each section explained in a
comment, to read and copy from. It prints the commands to try, in order.

## Next

- [Bring existing data in](move-existing-data.md), from a spreadsheet or CSV
  files.
- [Write a model](first-model.md): the values your formulas compute.
- The [home page](../index.md) walks through a project from its first files
  to its figures.
