# samplekit-migrate

Carries a project written by an earlier SampleKit into a new folder that this
version reads: 0.1.0, 0.1.1, 0.1.2, 0.2.0 and 1.0.0-rc.1, the versions
published on the Python Package Index.

## No guarantee

**Nothing guarantees that this tool carries your data correctly.** It cannot
fit every use, and it will miss some. It works on a copy, so that nothing is
lost: it never writes in the folder it reads.

**Before anything, keep a backup** of your project: three copies, on two kinds
of media, one of them elsewhere (the 3-2-1 rule), and, better still, a git
repository.

## Run it

It is one file, `samplekit-migrate.pyz`, attached to each release. It installs
nothing: run it with the Python of an environment where this version's
`samplekit` is installed, the version of your command line.

```console
$ python -m venv .venv
$ .venv/bin/python -m pip install samplekit==VERSION
$ .venv/bin/python samplekit-migrate.pyz OLD --to NEW --model OLD/model.py
$ .venv/bin/python samplekit-migrate.pyz OLD --to NEW --model OLD/model.py --write
```

`OLD` is the earlier project's folder, read and never written; `NEW` a folder
that does not exist yet, or is empty. Without `--write`, the tool reads
everything and says what it would write; with it, it writes `NEW`, and in it
`MIGRATION.md`: what was carried, what was left, what to check by hand, and
what to run next.

| Option | What it does |
| --- | --- |
| `--model FILE` | 0.x: the module that declares your class of samples. A 0.x file does not say which of its values a formula computed: the model does |
| `--no-model` | 0.x without its model: every value carried as entered |
| `--class NAME` | the class of the samples, where the module holds several |
| `--from VERSION` | the version that wrote the project, where it is not recognised |
| `--keep-computed` | 0.x: carry the values a formula computed, as values typed over it, which `compute --force` gives back to it |
| `--write` | write `NEW` |
| `--clean-state` | list what earlier versions left in this computer's state folder, and remove it with `--write`; nothing else of the folder is touched. It runs alone, `samplekit-migrate.pyz --clean-state` |
| `--verify` | 0.x, with `--write`: compute the copy with the migrated model and compare each value with the one 0.x computed |

## What it does

**From 0.x**: the values entered, the readings and the tables are carried; a
value a formula computed is not, and `samplekit compute --write` computes it
again. The units, symbols and precisions 0.x wrote in every file are declared
once in `.samplekitrc`. The text below each frontmatter is kept as the note,
as it was, what 0.x wrote there from `template()` included: remove it where
it is only a copy of the values. The model's `__init__` is written in this version's
API, readings keeping 0.x's statistic (their mean and sample standard
deviation); every other method is copied as it was and marked
`# TODO(migrate)`: a formula is too varied to be translated, and is to be read
by hand.

**From 1.0.0-rc.1**: the sample files are this version's already, and are
copied as they are, with the history. The configuration and the model are
renamed where this version's names differ.

The [upgrade guide](https://zelyph.github.io/SampleKit/latest/guides/upgrade.html)
says what changed between the versions, and how to do it all by hand.

## Licence

MIT, as SampleKit. The archive holds a copy of PyYAML, under its own MIT
licence, `yaml/LICENSE`.
