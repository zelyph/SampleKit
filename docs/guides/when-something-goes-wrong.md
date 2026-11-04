# When something goes wrong

This page is for when a command did not do what you expected: it refused,
warned, or showed a value you did not trust. Find the symptom below; each
says what SampleKit is telling you, what to do, and where the reason is
explained.

SampleKit's rule is to **refuse rather than guess**. A misspelt name, a
condition that does not read, a file it cannot read, a value it cannot vouch
for: each is said, by name, never filled in quietly. The message usually
holds the fix.

## Getting help

- `samplekit --help` lists the commands; `samplekit set --help` explains one,
  with examples; `-h` gives a shorter summary.
- In the terminal interface, `?` lists the keys of the screen you are on.
- [The command line](../reference/cli.md) and [the TUI's
  keys](../reference/tui.md) list everything, generated from the version you
  run.
- Every command's **exit code** says how it ended: 0 done, 1 a mistake in the
  command, 2 something wrong in the data, 3 the system refused ([exit
  codes](../reference/exit-codes.md)).

## A name is refused

*unknown field 'tensile_strenght'*, *no profile named …*: the message names
the nearest name that exists, and in a terminal asks whether to use it. Check
the names a project uses with `samplekit list fields`, and `list queries`,
`profiles`, `exports`, `figures`, `scripts` for the rest. A name holds
letters, digits and `_`, never a hyphen: `gauge_length`, not `gauge-length`.

## A filter is refused

The message points at the character where it stopped reading: `material =
steel` wants `==`. Quote the whole filter for the shell, `-f 'elongation >
20'`. A text compared with a number, `grade > 3`, is refused where every
sample holds text ([find and select samples](find-and-select.md), [every
form](../reference/filters.md)).

## A file was not read

```text
warning: samples/broken.md was not read: malformed frontmatter: did not find expected ',' or ']' at line 5 column 1, while parsing a flow sequence at line 4 column 7
```

A file that looks like a sample and cannot be read is never skipped in
silence: the command names it, reads the rest, and exits with code 2.
`samplekit list skipped` lists every file set aside, and why. Open the file
at the line named: a bracket not closed, a tab, a quote. A link written
without its quotes, `reference: [[steel-02]]` or `report: [report](r.pdf)`,
is refused at its line, with the quoted form to write instead,
`reference: "[[steel-02]]"`. A Markdown file
without the `---` lines at its top is not a sample at all: no command warns of
it, as of a README, and `list skipped` names it with its reason ([a collection](../concepts/samples-and-files.md#a-collection)).

## No sample is found

*no sample here*, followed by the folders below that hold samples: outside a
project, a folder is read alone, without the folders below it. Name the
folder that holds the samples, or make the folder a project with `samplekit
init`, whose folders are all read ([which folders a command
reads](../concepts/projects.md#which-folders-a-command-reads)).

## `validate` reports a defect

`validate` names the file and what is wrong: two units in one column, a
value outside the range the project declares, a text where a number belongs,
a pattern of `[collection] files` that cannot work, a link to a sample or a
file that does not exist ([links](../reference/sample-files.md#links)). It changes nothing; fix
the file with `set` or an editor, and run it again ([checking
them](../index.md#checking-them)). A defect makes it exit with code 2, which
stops a continuous integration job ([check a project in continuous
integration](continuous-integration.md)).

## A value is not current

`samplekit status` lists every value that is not current, with the reason:

| State | Means | Do |
| --- | --- | --- |
| *outdated* | an input changed since it was computed | `compute --write` |
| *formula changed* | the model's formula was edited since | `compute --write` |
| *never computed* | the model declares it, and it was never computed | `compute --write` |
| *edited* | typed over its formula | keep it, or `compute --force --write` |
| *waits for …* | an input nobody entered | enter the input; nothing is wrong |
| *failed* | its formula raised an error | read the error, fix the formula or the data |

A table marks each in its cell, `⚠` `✎` `✗` `∅`, and `samplekit FILE explain
FIELD` says, for one value, what it was computed from and why it is in its
state ([current and outdated](../concepts/current-and-outdated.md)).

## A formula fails

`compute` shows the error with the formula's Python traceback, and computes
the rest. The value is marked *failed*, and its old number is never shown as
current. `explain` gives the error again. A formula that returned nothing,
a `return` forgotten, is said so ([when a formula cannot
run](../concepts/models-and-formulas.md#when-a-formula-cannot-run)).

## The model does not run

```text
error: the model cannot run: [model] python names nothing that exists
```

The model runs only in the Python environment `[model] python` names in
`.samplekitrc`, with SampleKit's package installed in it at the version of
the command line. The message names what is missing, and the command that
makes it: `python -m venv .venv`, then `.venv/bin/pip install
samplekit==VERSION`; or `samplekit init`, which offers it. A model that
imports a package you use, numpy or scipy, needs it installed in that same
environment ([where the model
runs](../concepts/models-and-formulas.md#where-the-model-runs)).

Reading, filtering, `validate` and the exports of tables never need Python:
only computing, scripts and figures do, a figure's export among them.
`validate` still checks that the environment exists, and reports it missing
as a defect.

## A script stops at a question

A script that asks with `input()` reads the answer from the terminal. Run
where nothing can answer, in a pipe or a scheduled job, it stops, naming the
question: give the script its value with `--set NAME=VALUE` instead, an
option the script declares ([run a script](run-a-script.md)).

## A figure does not open

A figure opens in a window only where there is a display, and with a window
toolkit, tkinter or Qt, in the project's environment; `plot` says which is
missing. `-o curves.pdf --write` writes it to a file instead, which needs
neither.

## A write is refused

- *changed since it was read*: someone, an editor or another command, changed
  the file between the preview and the write. Nothing is written; run the
  command again.
- *read-only*: the file cannot be written by you.
- A question answered `n`, or no answer from a script, writes nothing and
  exits 0: `--write` answers yes beforehand.

## A copy appears beside a sample

A synchronised folder that met two changes to one file at once keeps the
other version beside it, `steel-01.sync-conflict-….md` or `steel-01
(conflicted copy …).md`: SampleKit leaves it out of every command and says
so. Compare the two, keep one, and remove the other ([share a
project](share-a-project.md)).

## A change must be taken back

`samplekit restore` takes the last change back, previewed first;
`restore --at` puts files back as they were at a change or on a day, and
`--at` on any command reads the project as it was then ([the
history](../concepts/history.md)).

## Common mistakes, by subject

Each concept page ends with the mistakes it prevents:
[samples and files](../concepts/samples-and-files.md#common-mistakes),
[values and uncertainty](../concepts/values-and-uncertainty.md#common-mistakes),
[readings](../concepts/readings-and-statistics.md#common-mistakes),
[models](../concepts/models-and-formulas.md#common-mistakes),
[current and outdated](../concepts/current-and-outdated.md#common-mistakes),
[the history](../concepts/history.md#common-mistakes),
[projects](../concepts/projects.md#common-mistakes).
