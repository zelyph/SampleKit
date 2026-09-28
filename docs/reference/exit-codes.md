# Exit codes

Every `samplekit` command ends with one of these codes. Messages go to the
error stream (stderr); the data a command was asked for goes alone to the
output stream (stdout).

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | A usage error: what was asked does not exist or cannot be done |
| 2 | A data error: something in the data is wrong |
| 3 | The system refused to read or write a file |
| 130 | Interrupted with Ctrl-C |

## 0 · Success

- A selection that matches nothing.
- An export of an empty selection: its header is written.
- A command with nothing to do, which says *unchanged*: removing a tag no
  sample carries, renaming a tag to itself, a `restore` with nothing to take
  back.
- A command that printed warnings, such as a file in the folder that is not a
  sample.

## 1 · Usage error

- An unknown option, command, field, query, profile, export or figure. The
  message suggests the nearest name.
- A filter that does not parse.
- A name `new` cannot give a sample, or an existing sample `new` would
  overwrite.
- `log --script` on a snapshot that no script made.

## 2 · Data error

- A file named on the command line is not a sample SampleKit can read. Over a
  folder, the samples that could be read are still shown, the others are
  named, and the code is 2.
- A formula raised an error during `compute`.
- `validate` found a defect.
- A figure has nothing to draw, such as no sample holding both axes.
- A file changed on disk between the moment SampleKit read it and the moment
  it was about to write it: a sample, an export, the `.samplekitrc` that
  `init` writes. Nothing is written.
- `status --exit-code` found a value that is not current. A value waiting for
  an input nobody entered does not count; with `--accept edited`, neither does
  a value edited by hand.

## 3 · The system refused

The operating system refused a file or a folder: it does not exist, the
permission to read or write it is missing, or a link cannot be followed. The
message names the file. A file that exists but is not a valid sample is a
data error, 2.

## 130 · Interrupted

Ctrl-C stops the command. What a computation finished before is kept; the
value in progress is not written.
