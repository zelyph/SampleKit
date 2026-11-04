# Exit codes

Every `samplekit` command ends with one of these codes. Messages go to the
error stream (stderr); the data a command was asked for goes alone to the
output stream (stdout).

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | A usage error: what was asked does not exist or cannot be done |
| 2 | A data error: something in the data is wrong |
| 3 | The system refused to read or write a file, or the machine cannot run the model |
| 130 | Interrupted with Ctrl-C |

## 0 · Success

- A selection that matches nothing.
- An export of an empty selection: its header is written.
- A command with nothing to do, which says `nothing to change` and why:
  removing a tag no sample carries, renaming a tag to itself, a `restore` with
  nothing to take back, a `compute` with nothing to compute, a `set`, `tag` or
  `remove` whose filter selects no sample.
- A command that printed warnings, such as, with `-v`, a file in the folder
  that is not a sample (a Markdown file without frontmatter).
- A question answered no (`n`, an empty line, the end of the input), which
  says `nothing written`: nothing failed. A script that must stop there writes
  with `--write`, or reads what the command printed.

## 1 · Usage error

- An unknown option, command, field, query, profile, export or figure. The
  message suggests the nearest name.
- A filter that does not parse.
- A name `new` cannot give a sample, or an existing sample `new` would
  overwrite.
- `log --script` on a snapshot that no script made.
- `--accept waiting`: a value waiting for an input fails nothing, so there is
  nothing to accept.
- `remove` given a folder alone, or nothing: it deletes the files named, or
  those a filter selects.

## 2 · Data error

- A file named on the command line is not a sample SampleKit can read. Over a
  folder, the samples that could be read are still shown, the others are
  named, and the code is 2.
- The `.samplekitrc` that applies has an error (an unknown key, a misspelled
  section): it is set aside, said in a warning, the samples are read without
  it, and the code is 2.
- A formula raised an error during `compute`.
- A script of the model, run by `run`, raised an error, or asked with
  `input()` where nothing could answer. Nothing is written.
- `validate` found a defect.
- A figure has nothing to draw, such as no sample holding both axes.
- A file changed on disk between the moment SampleKit read it and the moment
  it was about to write it: a sample, an export, the `.samplekitrc` that
  `init` writes. Nothing is written.
- `status --exit-code` found a value that is not current. A value waiting for
  an input nobody entered does not count; with `--accept edited`, neither does
  a value typed over its formula, and with `--accept edited:area,young_modulus` those values
  alone.
- A file was written from values that are not current, by `export --write`,
  a table's `-o FILE --write` or `plot -o FILE --write`: the file is written,
  and the code is 2 so that a script stops there. `--accept` followed by their
  states makes it 0: `outdated`, `edited`, `edited:NAME,…`, `failed`,
  `never_computed`, a comma between.
- `export --write` over a selection where a sample could not be read: nothing
  is written, since its rows would be missing, and the file there is kept.

## 3 · The system refused

The operating system refused a file or a folder: it does not exist, the
permission to read or write it is missing, or a symbolic link cannot be followed. The
message names the file. A file that exists but is not a valid sample is a
data error, 2.

The machine cannot run the model when its environment lacks what it needs:
no Python where `[model] python` says (nothing else is searched for), the `samplekit` package
missing from it, or another version of it than the command line's: the
message names both versions. That is the machine's to mend, as a file refused
is, not the data's.

## 130 · Interrupted

Ctrl-C stops the command, at its question too. What a computation finished
before is kept; the value in progress is not written.
