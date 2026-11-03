# Current and outdated values

A computed value is only as good as the inputs it was computed from. Correct
a thickness reading, and the stress computed from it last month is now wrong, though
nothing in its number says so. SampleKit tells you, for every computed value,
whether it is still what its formula gives from the file as it now is, and
if not, why. This page says how it knows, what the states mean, and what to do
about each.

## The record a computed value carries

When SampleKit computes a value, it writes beside it a record of what it was
computed from:

```yaml
area:
  v: 25.05586444444445
  u: 0.05217296159913354
  unit: mm²
  computed: {thickness: 2b1079ab857e, width: 25920047999d}
  fingerprint: 17b7fdf04105
```

- `computed:` names each input the formula read, with a short code (a
  **digest**) of that input's content at the time.
- `fingerprint:` is the same kind of digest, of the value itself as the
  formula gave it.

A digest is a short summary of content: the same numbers always give the same
code, and any change to them gives a different one. It covers what a
property *is*, its value, uncertainty and readings, and not how it is shown:
changing a unit's spelling or a precision makes nothing outdated.

With these two records, two questions are answered from the files alone, in
milliseconds, with no Python and nothing run:

- **Has an input changed since?** SampleKit computes the digest of
  `thickness` as it is now and compares it with the one recorded in `area`'s
  `computed`. Different: `area` is **outdated**.
- **Was this value changed by hand since?** It computes `area`'s own digest
  and compares it with its `fingerprint`. Different: `area` was **edited**.

Stored codes are never compared with each other, only with what the content
gives now. A hand edit changes the content and neither code, so comparing
two stored codes would agree about something no longer true.

A computed value whose input is itself not current is not current either: the
problem is further up the chain, and `status` says so (*outdated — area
(itself not current)*).

### What the files cannot know: the formula

A file records what a value read, never the code that computed it. **The
project's history records that instead**, in `.samplekit/history/`: for each
value SampleKit computes and writes, a digest of the formula that computed it
(its code, read without comments and blank lines, what it declares, and the
functions and constants of your model it uses), kept by what the value's
record says rather than by the file it is in. When you edit a formula, the
values it computed are *formula changed*, and the next `compute` runs them
again. What reads them is computed again only if their value actually moved.

**A project's first write takes the values it holds as computed by the model
as it is then**, where no history records what computed them: a project
copied or received without its `.samplekit/` folder. It says how many: a
model edited before that first command is not seen changed, and `samplekit
compute --rerun --write` runs them again, to be sure.

Anything else you change in the model makes nothing outdated: a comment, a
docstring, a new figure, a helper no formula uses. This matters when a full
recompute takes hours.

Because this record is kept by the value and travels with the history:

- a value you restore from an earlier snapshot is judged by the formula that
  computed it then;
- a project you move, copy or clone keeps it;
- a value a colleague computed is judged by what their history says of it,
  once their history has arrived ([Share a
  project](../guides/share-a-project.md)).

A value whose formula the history does not know, computed on a computer
whose history is not here or by a SampleKit before this one, is judged by
its inputs alone, and `status` says how many there are:
`samplekit compute --rerun` computes them again.

## The states

| State | What it means | What to do |
| --- | --- | --- |
| **current** | the value is what its formula gives from the file as it is | nothing |
| **outdated** | an input changed after the value was computed (*outdated — thickness*), a reading was corrected (*outdated — readings*), or the formula was edited (*formula changed*) | `compute --write` |
| **edited** | someone typed this value over its formula (*edited — typed over its formula*). A computed-looking value with no record at all (*record missing*) is treated the same way | keep it, or `compute --force` to give it back to its formula |
| **failed** | the formula raised an error when last run; the file keeps the last value it gave | read the error (`explain`), fix the data or the formula, compute again |
| **never computed** | the model declares this value and the file does not hold it yet (or, beside a value you entered, *uncertainty never computed*) | `compute --write` |
| **waiting** | an input is empty: nobody entered it (*waits for thickness*) | enter the input; the next `compute` runs it |

**Not current** is the word for any of outdated, edited, failed and never
computed: the values `status --exit-code` fails on. A **waiting** value is not
among them: a sample halfway through its measurements is not in error. What
reads a waiting value waits too, *waits for area, which waits*, rather than
being called outdated or failed. **A
value whose input was emptied since it was computed keeps its number**, the
one computed before, and says that it waits: a table shows it in brackets,
`(69)`, on the terminal as in a pipe, JSON writes `"state": "waiting"`, and
Python's `state` is `"waiting"`. **A CSV gives a column of states to each
property one of whose values is not current or waits**, `area_state`,
after its own: `outdated`, `edited`, `failed`, `waiting`…, empty where the
value is current.

**Values not current are counted by cells**, wherever a number of them is
given: a column of a table outdated in its 13 rows counts 13, as `view` lists
13. A value and its uncertainty count once when one formula gives both, and
twice when each has its own formula, since each can be outdated without the
other. Under a table, the count is of the cells the table shows.

One more word describes a whole sample rather than a value: **defective**,
when `validate` reports a defect in its file (a value that is text where a
number is expected, two rows with the same index, a unit that disagrees). A
defective sample can still have only current values.

## Seeing the states

The same states appear on every surface, in the same words:

- `samplekit FOLDER status` lists every value not current or waiting, sample
  by sample, and says what to run. `samplekit FILE explain VALUE` shows one
  value: its inputs, the state of each, and why it is in its state.
- In a table on the terminal, a value is marked `⚠` when outdated, `✎` when
  edited, `✗` when failed. The TUI adds `∅` for never computed or waiting,
  and `·` for readings with no statistic. `?` in the TUI lists every mark.
- `state` is a **field**: `-f 'state == outdated'` keeps the samples with an
  outdated value, and `-c name,state` shows each sample's states. Its words
  are `current`, `outdated`, `edited`, `failed`, `never_computed`, `waiting`,
  `defective`, and `not_current` for any of outdated, edited, failed and
  never computed. A sample can be in several at once.
  `specimens.filter("state == failed")` in Python and `/` in the TUI read the
  same words.
- In Python, reading a value that is not current returns it, with a warning
  saying why; `sample.not_current()` lists everything `status` would report.

`never_computed`, `waiting`, `not_current` and `current` need the model, since
only the model knows which values it owes: SampleKit reads what the model
declares from its description, `.samplekit/model.json`, and runs nothing
([Models and formulas](models-and-formulas.md#the-models-description)); after
an edit to the model, the first such command starts Python once to describe
it again. Where the model cannot be read, those words select
nothing, and a line says why. The other words are read from the files alone.

## What makes a value outdated, and what does not

Outdated:

- a change to an input's value, uncertainty or readings, however it was made:
  `set`, the TUI, Python, or your text editor;
- a corrected reading, for a value computed from readings;
- an edit to the formula itself, or to a function or constant it uses;
- an input that was itself recomputed and moved.

Not outdated:

- a change of unit spelling, symbol or precision (presentation, not content);
- a change to anything the formula does not declare it reads, which is why
  `depends_on` must be complete ([Models and formulas](models-and-formulas.md));
- a comment, a docstring or a new figure in the model;
- the name of a method or of the model's class: renaming `_area` or
  `Specimen` recomputes nothing;
- an input typed over its formula: what reads it rests on it as it stands,
  and becomes outdated only if that value itself changes.

Nothing is recomputed at the moment of a change. A change marks; `compute`
recomputes, when you ask.

A unit the model declares is part of its formula: changed, the values are
*formula changed*, and `compute --write` writes them again in the new unit.

**A value the model no longer computes**, its formula renamed or removed,
keeps its number in the file, and blocks nothing: `status` and `validate`
name it, *the model no longer computes it*, with its two ways out. `samplekit
samples/steel-01.md set hardness=` removes it; removing its `computed` record by hand keeps
it as a value entered, which is how what reads it takes it meanwhile.

## Values typed over their formula, and `--force`

Sometimes the right value is not what the formula gives: a figure copied from
a certificate, a correction you trust more than the model. You can type it
over the formula, in the file, with `set`, in the TUI or in Python. It is
**edited**:

- it is marked `✎` and listed as *edited — typed over its formula*;
- **`compute` leaves it alone**, even with `--rerun`: a value you typed is not
  replaced without being asked by name;
- `compute --force` (optionally with `-p NAME`) gives it back to its formula.
  In the TUI, `c` on the value does so, after asking.

The formula stays in the model; only this sample's value is held. An edited
value counts as not current, so `status --exit-code` fails on it; where such
values are deliberate, `status --exit-code --accept edited` lets them pass.

## Exports and figures are current or not too

A file SampleKit writes from your samples, an export, a table written with
`-o`, a figure, is **recorded in the project's history**, by its exact
content. `samplekit FILE explain` then says, for that file, even copied
elsewhere or renamed, what made it, from which samples, what changed since in
what it was made from, and so whether it is still current, with the command
that makes it again ([History](history.md)).

An export is judged by the **fields it writes**, and by what those are
computed from: changing a specimen's grade leaves an export that shows no
grade, and nothing computed from it, current.

Before writing, an export's preview says which of the values it would write
are not current. A CSV never carries the marks `⚠ ✎ ✗`; `--status` adds a
`state` column when you want each row's state kept with the data.

## Common mistakes

- **Thinking a value is current because its number looks right.** Look at
  `status`, or the marks.
- **Expecting `compute --rerun` to replace a value typed over its formula.**
  It does not; that is `--force`.
- **Changing a unit in a file and expecting values to be outdated.** A unit is
  a label; change the numbers too if the unit really changed. (A unit the
  model declares is different: changed there, it is part of the formula.)
- **Sharing the samples without their history.** A computer that has the
  project's history, `.samplekit/history/` committed beside the samples, knows
  which formula computed each value, and says *formula changed* after an edit
  made elsewhere. One that has the samples alone cannot know: compute there
  with `--rerun`, or share the history
  ([Share a project](../guides/share-a-project.md)).
- **Forgetting that `status --exit-code` fails on edited values.** Add
  `--accept edited` if they are intended.

## Where to go next

- A check that fails when something is not current:
  [Check a project in continuous integration](../guides/continuous-integration.md).
- Practised on the [home page](../index.md#values-out-of-date).
- `status`, `explain`, `compute` and their options: [Command line
  reference](../reference/cli.md); the exit codes: [Exit
  codes](../reference/exit-codes.md).
