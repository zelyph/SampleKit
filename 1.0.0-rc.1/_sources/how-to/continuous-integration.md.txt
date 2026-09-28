# Check a project in continuous integration

This recipe adds a job to your project's continuous integration that fails
when a sample file is wrong, or when a computed value is not current — before
it reaches a figure or a paper.

## The two checks

<!-- run: 08-python -->
```console
$ samplekit validate brews
$ samplekit status brews --exit-code
```

Here `brews` is the tutorial's folder of samples, in its step 8, where both
pass; put your own folder in its place.

- `validate` reads every file and exits with code 2 on a **defect**: a file it
  cannot read, a column in two units, a value that contradicts its readings.
  It changes nothing and starts no Python.
- `status --exit-code` exits with code 2 when a computed value is **not
  current**: outdated, failed, never computed, or written by hand over its
  formula. A value waiting for an input nobody has entered yet does not fail
  it.

Both exit 0 when all is well, and 1 on a mistake in the command itself.

## Choose what passes

A value typed over its formula on purpose — an **override**, such as a value
copied from a certificate — can be let through:

```console
$ samplekit status samples --exit-code --accept edited
```

The job then fails only on what is outdated, failed or never computed. For a
report the job can keep, `--json` writes one object per value not current,
and `samplekit validate --json` the same for defects.

## The environment

`status` reads the model, so the job needs the model's Python: a `.venv/`
above the samples, with SampleKit's Python package and whatever your model
imports. Without it, `status --exit-code` fails, saying that the model was not
read. `validate` needs no Python.

A job, as a shell script any CI runs:

```sh
set -e
# One version for the command line and the package: the release's tag
# without its `v`.
VERSION=1.0.0-rc.1
curl -sSfL "https://github.com/zelyph/SampleKit/releases/download/v$VERSION/samplekit-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin samplekit

# The model's environment, in the project.
python3 -m venv .venv
.venv/bin/pip install "samplekit==$VERSION"   # and what your model imports

samplekit validate samples
samplekit status samples --exit-code
```

The version is pinned, so that a new release changes the job only when you
change it. `cargo install samplekit --locked --version "$VERSION"` builds the
same command line from its source instead, where Rust is installed.

Nothing asks for a confirmation: SampleKit runs a project's model as `python`
runs a script you give it, so the job runs unattended.

## What a fresh machine cannot see

`status` judges each value by its inputs, which the files record. Which
version of a formula computed a value is recorded on the computer that
computed it, not in the files. So on a CI machine, an edited formula does not
make its values outdated. To check the formulas too, compute everything again
without writing, and read what would change:

```console
$ samplekit compute samples --rerun --try
```

Its summary counts the values that would change; `unchanged` everywhere
means the files hold what the formulas give. This runs every formula, which
takes as long as computing the whole project.

## See also

- [Exit codes](../reference/exit-codes.md), every case.
- [`status`, `validate`, `compute`](../reference/cli.md).
- [Current and outdated](../explanations/current-and-outdated.md).
