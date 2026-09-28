# Current and outdated values

A computed value is only as good as the inputs it was computed from. Correct
a hydrometer reading, and the alcohol computed from it last month is now wrong, though
nothing in its number says so. SampleKit tells you, for every computed value,
whether it is still what its formula gives from the file as it now is, and
if not, why. This page says how it knows, what the states mean, and what to do
about each.

## The record a computed value carries

When SampleKit computes a value, it writes beside it a record of what it was
computed from:

```yaml
abv:
  v: 4.375000000000013
  u: 0.13834964763236396
  unit: "%"
  computed: {fg: 100396ebe001, og: a8ea7995583e}
  fingerprint: 56612a681e02
```

- `computed:` names each input the formula read, with a short code (a
  **digest**) of that input's content at the time.
- `fingerprint:` is the same kind of digest, of the value itself as the
  formula gave it.

A digest is a short summary of content: the same numbers always give the same
code, and any change to them gives a different one. It covers what a
quantity *is*, its value, uncertainty and readings, and not how it is shown:
changing a unit's spelling or a precision makes nothing outdated.

With these two records, two questions are answered from the files alone, in
milliseconds, with no Python and nothing run:

- **Has an input changed since?** SampleKit computes the digest of `og` as it
  is now and compares it with the one recorded in `abv`'s `computed`.
  Different: `abv` is **outdated**.
- **Was this value changed by hand since?** It computes `abv`'s own digest and
  compares it with its `fingerprint`. Different: `abv` was **edited**.

Stored codes are never compared with each other, only with what the content
gives now. A hand edit changes the content and neither code, so comparing
two stored codes would agree about something no longer true.

A computed value whose input is itself not current is not current either: the
problem is further up the chain, and `status` says so (*outdated — og (itself
not current)*).

### What the files cannot know: the formula

A file records what a value read, never the code that computed it. **Your
computer records that instead**, in SampleKit's state folder: for each
sample and each formula, a digest of the formula that last computed its value
(its code, read without comments and blank lines, what it declares, and the
functions and constants of your model it uses). When you edit a formula, the
values it computed are *formula changed*, and the next `compute` runs them
again. What reads them is computed again only if their value actually moved.

Anything else you change in the model makes nothing outdated: a comment, a
docstring, a new figure, a helper no formula uses. This matters when a full
recompute takes hours.

This record is **local to the computer** that computed. A value computed on
another computer is judged by its inputs alone; after editing a formula
there, `samplekit compute --rerun` computes everything again.

## The states

| State | What it means | What to do |
| --- | --- | --- |
| **current** | the value is what its formula gives from the file as it is | nothing |
| **outdated** | an input changed after the value was computed (*outdated — grain_mass*), a reading was corrected (*outdated — readings*), or the formula was edited (*formula changed*) | `compute --write` |
| **edited** | someone typed this value over its formula: an **override** (*edited since it was computed*). A computed-looking value with no record at all (*record missing*) is treated the same way | keep it, or `compute --force` to give it back to its formula |
| **failed** | the formula raised an error when last run; the file keeps the last value it gave | read the error (`explain`), fix the data or the formula, compute again |
| **never computed** | the model declares this value and the file does not hold it yet (or, beside a value you entered, *uncertainty never computed*) | `compute --write` |
| **waiting** | an input is empty: nobody entered it (*waits for fg*) | enter the input; the next `compute` runs it |

**Not current** is the word for any of outdated, edited, failed and never
computed: the values `status --exit-code` fails on. A **waiting** value is not
among them: a sample halfway through its measurements is not in error.

One more word describes a whole sample rather than a value: **defective**,
when `validate` reports a defect in its file (a value that is text where a
number is expected, two rows with the same index, a unit that disagrees). A
defective sample can still have only current values.

## Seeing the states

The same states appear on every surface, in the same words:

- `samplekit status FOLDER` lists every value not current or waiting, sample
  by sample, and says what to run. `samplekit explain FILE VALUE` shows one
  value: its inputs, the state of each, and why it is in its state.
- In a table on the terminal, a value is marked `⚠` when outdated, `✎` when
  edited, `✗` when failed. The TUI adds `∅` for never computed or waiting,
  and `·` for readings with no statistic. `?` in the TUI lists every mark.
- `state` is a **field**: `-f 'state == outdated'` keeps the samples with an
  outdated value, and `-c name,state` shows each sample's states. Its words
  are `current`, `outdated`, `edited`, `failed`, `never_computed`, `waiting`,
  `defective`, and `not_current` for any of outdated, edited, failed and
  never computed. A sample can be in several at once.
  `brews.filter("state == failed")` in Python and `/` in the TUI read the
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
- an input that is an override: what reads it rests on it as it stands, and
  becomes outdated only if the override itself changes.

Nothing is recomputed at the moment of a change. A change marks; `compute`
recomputes, when you ask.

## Overrides, and `--force`

Sometimes the right value is not what the formula gives: a figure copied from
a certificate, a correction you trust more than the model. You can type it
over the formula, in the file, with `set`, in the TUI or in Python. It becomes
an **override**:

- it is marked `✎` and listed as *edited since it was computed*;
- **`compute` leaves it alone**, even with `--rerun`: a value you typed is not
  replaced without being asked by name;
- `compute --force` (optionally with `-p NAME`) gives it back to its formula.
  In the TUI, `c` on the value does so, after asking.

The formula stays in the model; only this sample's value is held. An override
counts as not current, so `status --exit-code` fails on it; where overrides
are deliberate, `status --exit-code --accept edited` lets them pass.

## Exports and figures are current or not too

A file SampleKit writes from your samples, an export, a table written with
`-o`, a figure, is **recorded in the project's history**, by its exact
content. `samplekit explain FILE` then says, for that file, even copied
elsewhere or renamed, what made it, from which samples, what changed since in
what it was made from, and so whether it is still current, with the command
that makes it again ([History](history.md)).

An export is judged by the **fields it writes**, and by what those are
computed from: changing a brew's grain mass leaves an export that shows no
grain mass, and nothing computed from it, current.

Before writing, an export's preview says which of the values it would write
are not current. A CSV never carries the marks `⚠ ✎ ✗`; `--status` adds a
`state` column when you want each row's state kept with the data.

## Common mistakes

- **Thinking a value is current because its number looks right.** Look at
  `status`, or the marks.
- **Expecting `compute --rerun` to replace an override.** It does not; that is
  `--force`.
- **Changing a unit and expecting values to be outdated.** A unit is a label;
  change the numbers too if the unit really changed.
- **Editing a formula on one computer and computing on another.** The second
  computer does not know the formula changed; use `--rerun` there.
- **Forgetting that `status --exit-code` fails on overrides.** Add
  `--accept edited` if they are intended.

## Where to go next

- A check that fails when something is not current:
  [Check a project in continuous integration](../how-to/continuous-integration.md).
- Practised in tutorial step [4](../tutorial/04-computing.md).
- `status`, `explain`, `compute` and their options: [Command line
  reference](../reference/cli.md); the exit codes: [Exit
  codes](../reference/exit-codes.md).
