# Several projects

This guide is for samples that belong together but not in one project: two
series of specimens, two people's measurements, two instruments. Each series
stays a project of its own, with its folder, its model and its history; what
they share is declared once above them; and every command reads them
together.

The commands run in the fifth step of the tensile demo ([the guides'
examples](index.md)): the rolled specimens of the [home page](../index.md),
and cast ones beside them.

## One root, one project per series

```text
05-projects/
├── .samplekitrc        what both share: precisions, a query, a profile
├── model/
│   └── specimen.py     one model, for both
├── rolled/
│   ├── .samplekitrc    import = "..", and its model
│   └── samples/
└── cast/
    ├── .samplekitrc    import = "..", and its model
    └── samples/
```

The root's `.samplekitrc` declares what both projects share. Each project's
own imports it, and names its model: `[model] path` and `class` are never
imported, since they say what a project's samples are, while the root's
`[model] python`, the environment the models run in, is imported ([organise a
project's settings](organise-settings.md)):

```{literalinclude} ../../examples/tensile/05-projects/rolled/.samplekitrc
:language: toml
:caption: rolled/.samplekitrc
```

A key a project writes replaces the root's; the rest is the root's. A path
the root declares reads from the root, and `{project}` stands for each
project's folder, so that one `[collection] files = ["{project}/photos"]`, or
one export's `output = "exports/{project}/overview.csv"`, serves both.

## Read them together

From the root, a command reads every project below it, each sample with its
own project's configuration. A table is split by project:

<!-- run: tensile/05-projects -->
```console
$ samplekit --profile overview
cast
name           material        E [GPa]   Rp0.2 [MPa]   Rm [MPa]   A [%]
───────────────────────────────────────────────────────────────────────
aluminium-c1   aluminium    71.3 ± 0.2           218        273     6.0
iron-c1        iron        169.8 ± 0.9           276        416    18.0

rolled
name           material        E [GPa]   Rp0.2 [MPa]   Rm [MPa]   A [%]
───────────────────────────────────────────────────────────────────────
aluminium-01   aluminium    69.2 ± 0.3           285        318    12.0
copper-01      copper      117.2 ± 0.9            76        221    44.0
steel-01       steel       206.3 ± 1.1           387        518    24.0

5 of 5 samples
```

A query or a profile the root declares selects across both:

<!-- run: tensile/05-projects -->
```console
$ samplekit --query ductile -c material,elongation
rolled
name        material   elongation [%]
─────────────────────────────────────
copper-01   copper               44.0
steel-01    steel                24.0

2 of 5 samples
```

Name a project to read it alone, `samplekit cast`, or several, `samplekit
cast rolled`. The `project` column says which project each sample follows,
for a file that mixes them: `-c project,name,elongation --csv`.

## What each project offers

`list profiles`, `queries`, `exports` and `figures` list what the folder
you name declares, with what it imports, and say where each comes from:

<!-- run: tensile/05-projects -->
```console
$ samplekit cast list profiles
overview   name, material, young_modulus, yield_strength, tensile_strength, elongation · .samplekitrc
```

A name declared in the project itself is `local`; one it redeclares in part,
`local over` the file it imports. `samplekit --show-rc` names the files a
project's configuration imports, and `-v` lists each setting with the file it
comes from.

## Check and compute across them

`validate`, `status` and `compute` take the same folders. From the root, each
project's model computes its own samples:

<!-- run: tensile/05-projects -->
```console
$ samplekit validate
5 samples · 0 defects · 0 notes
$ samplekit status
every computed value in 5 samples is current
```

`validate` also notes a declaration a project repeats from the root, word for
word: a copy to remove.

## Each its own history

Each project keeps its own history. From the root, `log` lists each one's,
headed by its project, and `diff` and `restore` act in each; a snapshot's
number is one project's, so `--at`, `--from` and `--to` are refused there,
naming the projects to choose from:

```console
$ samplekit diff --from 1
error: --from names a snapshot of one project's history, and here are 2: cast, rolled — name one, samplekit cast diff --from 1
```

## In the terminal interface

`samplekit . tui` at the root opens the TUI on both projects, each part
headed by its project ([work in the terminal interface](terminal-interface.md)). It
offers the figures and exports of the project the samples shown belong to;
over projects of different models, the root's alone.

## From Python

`sk.load` of the root reads both projects, each sample with its own
project's configuration and model, as the command line reads it; `project`
says which one a sample follows:

```{literalinclude} ../../examples/tensile/05-projects/scripts/projects.py
:language: python
:caption: scripts/projects.py
```

<!-- run: tensile/05-projects -->
```console
$ python scripts/projects.py
rolled copper-01 44.0 %
rolled steel-01 24.0 %
cast iron-c1 18.0 %
rolled aluminium-01 12.0 %
cast aluminium-c1 6.0 %
```

## Two people, two projects

Two people each keeping their own project, side by side, rarely write the
same file, and their histories stay apart: one project each is how two
people share a root without stepping on each other ([share a
project](share-a-project.md)).

## See also

- [Projects](../concepts/projects.md): the nearest configuration, imports,
  and what the current folder offers.
- [Organise a project's settings](organise-settings.md).
