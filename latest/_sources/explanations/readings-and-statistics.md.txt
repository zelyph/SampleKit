# Readings and statistics

A quantity measured several times, three hydrometer readings, five thermometer readings,
has **readings**: every result, kept as it was taken. This page says how
SampleKit keeps them, how a value comes to stand for them, and why it never
takes their mean on its own.

## Readings are the evidence

```yaml
properties:
  og: {readings: [1.064, 1.065, 1.063]}
```

The readings are kept in the file, all of them, in the order written. They
are never replaced by their mean. A statistic of them can always be
recomputed, a suspect reading can always be found again, and a different
convention (the median rather than the mean, say) can be applied later. A
mean with the readings thrown away can do none of that.

## A value stands for its readings only if someone chose how

Which number represents three readings? The mean is common, but not the only
answer: the median resists an outlier, the maximum is what a safety limit
wants. **That choice is yours**, and SampleKit does not make it for you.

So the value of a quantity with readings is, in this order:

1. **A value written beside them**, `{v: 1.064, readings: [...]}`, typed by a
   person. It is the value, and it stays when the readings change.
2. Otherwise, **the statistic the model declares** for this quantity: the
   project's model says, for example, that the value is the mean of the
   readings and the uncertainty their standard error.
3. Otherwise, **no value**. The readings are there; nothing says what stands
   for them, and SampleKit says so rather than guessing.

A mean taken where nobody chose it would be a number nobody decided on,
written into files and later read back as if someone had. That is the silent
kind of error this tool exists to prevent, so readings without a declared
statistic have no value, and every surface says it: the TUI marks the value
`·`, a table shows `—`, Python reads `None`, and `samplekit set` says that
the readings give no value until a statistic is declared or a value written.

## Declaring the statistic

The model declares it on the property:

```python
self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
```

`sk.stats` offers, for a value, `mean`, `median`, `minimum`, `maximum`,
`first_quartile` and `third_quartile`; for an uncertainty,
`standard_error`, `sample_stdev` and `population_stdev`.
[Models and formulas](models-and-formulas.md) explains the model itself.

Once computed, the file carries the statistic's result beside the readings,
with a note of which statistic it is and a record of the readings it was
taken from:

```yaml
og:
  v: 1.0433333333333332
  readings: [1.044, 1.043, 1.043]
  u: 3.3333333333332443e-4
  statistics: {v: mean, u: standard_error}
  computed: {readings: 0af8bd4decf0}
  fingerprint: 5962c1426074
```

`statistics:` is there so that a reader without the model knows what the two
numbers are. `computed:` and `fingerprint:` are the record every computed
value carries ([Current and outdated](current-and-outdated.md)).

## When a reading changes

A value taken from the readings is a **computed value like any other**.
Correct a reading, and the value is *outdated — readings* until you compute
again; everything computed from that value is outdated in turn. `samplekit
compute --write` (or `C` in the TUI, or `compute()` in Python) takes the
statistic again.

A value **written by hand** beside readings is different. It outranks the
statistic, and new readings do not replace it: the preview of the change says
that your value stays. If the model declares a statistic, a typed value there
is an **override**, listed as *edited since it was computed*, and `compute
--force` gives the quantity back to its statistic. Emptying the value
(`samplekit set FILE og=`) does the same without computing.

Files written by earlier versions of SampleKit hold the mean next to every
list of readings. That mean is read as a value written by hand, since nothing
in the file tells it apart from one typed, and your files are not rewritten.
Where the model now declares a statistic, `compute --force` hands the value
over to it.

## Giving readings

Readings are the quantity's `readings`, with or without brackets:

```console
$ samplekit set brews/citra-ipa.md fg.readings=1.011,1.010,1.011 --write
$ samplekit set brews/citra-ipa.md 'fg.readings=[1.011, 1.010, 1.011]' --write
```

The same spelling works in `samplekit new`, for a table cell, and in
`--add-row`. The list you give replaces the old one. In Python,
`brew.fg.readings = [1.011, 1.010, 1.011]`; a list assigned to a measured
quantity's value, `brew.og.value = [...]`, is its readings too. In the TUI,
`e` on a quantity edits its value, uncertainty and readings together.

Two things are refused or warned about, because each is usually a typing
slip:

- **One number is not readings.** `fg.readings=1.011` is refused: one number is
  a value, `fg=1.011`.
- **A decimal comma.** `fg.readings=20,5,21,5` is read as four readings, and a
  warning says so: write `[20.5, 21.5]`.

A value that a formula computes takes no readings: readings under a formula
are refused, since both cannot be kept and neither may be dropped without a
word. An `n/a` value keeps readings written beside it, but they give it no
uncertainty.

## Statistics of the readings, any time

Whatever the value, the readings can always be described. These are
**fields** you read, not values anyone chose:

| Field | Reads |
| --- | --- |
| `og.readings` | the readings, as a list |
| `og.stats.count` | how many |
| `og.stats.mean`, `og.stats.median` | their mean, their median |
| `og.stats.minimum`, `og.stats.maximum` | the extremes |
| `og.stats.sample_stdev`, `og.stats.population_stdev` | their spread |
| `og.stats.standard_error` | the standard error of their mean |
| `og.stats.first_quartile`, `og.stats.third_quartile` | the quartiles |

They work in a filter, a column, a profile and a sort, and in Python
(`brew.og.stats.count`, `brew["og.stats.mean"]`). They need no model and write
nothing. `og.stats.mean` is always the mean of the readings; `og` is the value,
which is the mean only if someone chose it.

## Readings in a table cell

A cell of a table is a quantity, and can hold readings as a property does:
three thermometer readings for one day, for example.

```console
$ samplekit set brews/citra-ipa.md 'fermentation.temperature[1].readings=20.6,20.7,20.8' --write
$ samplekit set brews/citra-ipa.md --add-row fermentation day=16 'temperature.readings=[18.8, 18.9]' --write
```

The statistic is declared **once for the whole column**, in the model:

```python
"temperature": sk.Column(unit="degC", value=sk.stats.mean,
                         uncertainty=sk.stats.standard_error),
```

and the file writes it beside the column's unit, so that the table still says
what its numbers are without the model. Each cell then behaves as a property
with readings: its value is computed, recorded, and outdated when a reading
changes; a value typed in a cell outranks the statistic; a column declaring
no statistic leaves its cells' readings without a value.

## Readings are not a summary

Two kinds of statistics are easy to confuse:

- **Statistics of one quantity's readings**, inside one sample: the mean of
  three hydrometer readings of one brew. They are the declared statistic, or
  the `og.stats.*` fields.
- **A summary of a column across samples**: the mean original gravity of
  twelve brews. That is `--summary` on the command line, `stats()` in Python
  and `S` in the TUI. It counts how many samples have a value (`11/12`), and
  when every value has an uncertainty its mean is weighted by 1/u², each value
  counting as much as it is precise; the header then says `mean (1/u²)`.

A summary reads each sample's value, whatever gave it: a typed value, a
declared statistic, or a formula.

## Common mistakes

- **Expecting readings to give a value on their own.** They do not. Declare a
  statistic in the model, or write a value beside them.
- **Writing a value beside readings and then correcting a reading.** Your
  written value stays; the preview says so. Empty it (`og=`) to let the
  statistic stand again.
- **Reading `og.stats.mean` as "the value".** It is a description of the
  readings. The value is `og`.
- **A decimal comma in a list of readings.** Use points.

## Where to go next

- The model's side: [Models and formulas](models-and-formulas.md).
- Tutorial step [4](../tutorial/04-computing.md) declares statistics for the
  gravities, and step [5](../tutorial/05-fermentation.md) works with tables.
- The exact syntax of `set` and of the fields: [Command line
  reference](../reference/cli.md); `sk.stats`, `sk.Column`: [Python
  reference](../reference/python.md).
