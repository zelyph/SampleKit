# Check a project in continuous integration

This guide is for a project kept in git with a continuous integration
service, GitHub Actions, GitLab CI or any other: a job that fails when a
sample file is wrong, or when a computed value is not current, before either
reaches a figure or a paper.

## The two checks

<!-- run: tensile/04-results -->
```console
$ samplekit samples validate
3 samples · 0 defects · 0 notes
$ samplekit samples status --exit-code
every computed value in 3 samples is current
```

Here `samples` is the [home page's](../index.md) tensile example, in its last
step, where both pass; put your own folder in its place.

- `validate` reads every file and exits with code 2 on a **defect**: a file it
  cannot read, a column in two units, a value that contradicts its readings.
  It changes nothing and starts no Python, though a missing environment is
  one of its defects.
- `status --exit-code` exits with code 2 when a computed value is **not
  current**: outdated, failed, never computed, or written by hand over its
  formula. A value waiting for an input nobody has entered yet does not fail
  it.

Both exit 0 when all is well, and 1 on a mistake in the command itself.

## Choose what passes

A value typed over its formula on purpose, an **edited** value such as one
copied from a certificate, can be let through:

```console
$ samplekit samples status --exit-code --accept edited
```

The job then fails only on what is outdated, failed or never computed. To let
through only the edited values you know of, name them: `--accept
edited:area,elongation` passes those values typed over their formula, and fails on any
other, a typo typed over a formula among them. For a
report the job can keep, `--json` writes one object per value not current,
and `samplekit validate --json` the same for defects.

## The environment

`status` reads the model, so the job needs the model's Python: the `.venv/`
that `[model] python` names, made in the job, with SampleKit's Python package
and whatever your model imports. Without it, `status --exit-code` fails, saying that the model was not
read. `validate` runs no Python, but checks that the environment `[model]
python` names exists: without it, `validate` too exits with code 2, on that
defect.

A job, as a shell script any CI runs:

```sh
set -e
# One version for the command line and the package: the release's tag
# without its `v`.
VERSION=1.0.0-rc.2
curl -sSfL "https://github.com/zelyph/SampleKit/releases/download/v$VERSION/samplekit-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin samplekit

# The model's environment, in the project.
python3 -m venv .venv
.venv/bin/pip install "samplekit==$VERSION"   # and what your model imports

samplekit samples validate
samplekit samples status --exit-code
```

The version is pinned, so that a new release changes the job only when you
change it. `cargo install samplekit --locked --version "$VERSION"` builds the
same command line from its source instead, where Rust is installed.

Nothing asks for a confirmation: SampleKit runs a project's model as `python`
runs a script you give it, so the job runs unattended.

## What the job sees of the formulas

Which version of a formula computed each value is kept in the project's
history, `.samplekit/history/`. SampleKit hides that folder from git unless
`.samplekitrc` says `[history] shared = true`; shared, it is committed with
the samples ([share a project](share-a-project.md)). So a clone on a CI
machine sees it: a formula edited since its values were computed makes
them *formula changed*, and `status --exit-code` fails on them. To check that
the files hold what the formulas give today, whatever was recorded, compute
everything again without writing, and read what would change:

```console
$ printf 'y\nn\n' | samplekit samples compute --rerun
```

The first answer computes, the second declines to write.

Its summary counts the values that would change; `unchanged` everywhere
means the files hold what the formulas give. This runs every formula, which
takes as long as computing the whole project.

## Figures and exports

A figure records the snapshot of the history it was drawn from, so that
`explain` can say what it was made from. Two drawings of the same data at two
snapshots therefore differ in that identifier, and a job comparing figures
byte for byte would fail with nothing changed. Compare the exports instead,
which hold the values alone, and ask `explain` about each figure, which says
whether it is still what its command makes and what changed since:

```console
$ samplekit paper/curves.pdf explain
```

## See also

- [Exit codes](../reference/exit-codes.md), every case.
- [`status`, `validate`, `compute`](../reference/cli.md).
- [Current and outdated](../concepts/current-and-outdated.md).
