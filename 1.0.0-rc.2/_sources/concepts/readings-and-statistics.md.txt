# Readings and statistics

A property measured several times, three micrometer readings of a thickness,
five of a temperature, has **readings**: every result, kept as it was taken. This page says how
SampleKit keeps them, how a value comes to stand for them, and why it never
takes their mean on its own.

## Readings are the evidence

```yaml
properties:
  thickness: {readings: [1.995, 2.004, 2.008], unit: mm}
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

So the value of a property with readings is, in this order:

1. **A value written beside them**, `{v: 2.0, readings: [...]}`, typed by a
   person. It is the value, and it stays when the readings change.
2. Otherwise, **the statistic the model declares** for this property: the
   project's model says, for example, that the value is the mean of the
   readings and the uncertainty their standard error. Read without a model,
   a file's `statistics:` beside the readings (below) says the same.
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
self.thickness = sk.Property(unit="mm", value=sk.stats.mean,
                             uncertainty=sk.stats.standard_error)
```

`sk.stats` offers, for a value, `mean`, `median`, `minimum`, `maximum`,
`first_quartile` and `third_quartile`; for an uncertainty,
`standard_error`, `sample_stdev` and `population_stdev`.
[Models and formulas](models-and-formulas.md) explains the model itself.

Once computed, the file carries the statistic's result beside the readings,
with a note of which statistic it is and a record of the readings it was
taken from:

```yaml
thickness:
  v: 2.0023333333333335
  readings: [1.995, 2.004, 2.008]
  u: 0.003844187531556873
  unit: mm
  statistics: {v: mean, u: standard_error}
  computed: {readings: 3160016f6c6e}
  fingerprint: 1d88e7bb89d3
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
is **edited**, listed as *edited — typed over its formula*, and `compute
--force` gives the property back to its statistic. Emptying the value
(`samplekit FILE set thickness=`) does the same without computing.

A value beside readings with no statistic recorded, as a file written by
hand may hold, is read the same way: nothing in the file tells it apart from
one typed. Where the model declares a statistic, `compute --force` hands the
value over to it.

## Giving readings

Readings are the property's `readings`, with or without brackets:

```console
$ samplekit samples/steel-01.md set thickness.readings=1.991,1.994,1.989 --write
$ samplekit samples/steel-01.md set 'thickness.readings=[1.991, 1.994, 1.989]' --write
```

The same spelling works in `samplekit new`, for a table cell, and in
`--add-row`. The list you give replaces the old one. In Python,
`steel.thickness.readings = [1.991, 1.994, 1.989]`; a list assigned to a
measured property's value, `steel.thickness.value = [...]`, is its readings
too. In the TUI,
`e` on a property edits its value, uncertainty and readings together.

Two things are refused or warned about, because each is usually a typing
slip:

- **One number is not readings.** `thickness.readings=1.991` is refused: one
  number is a value, `thickness=1.991`.
- **A decimal comma.** `width.readings=12,5,12,6` is read as four readings,
  and a warning says so: write `[12.5, 12.6]`.

A value that a formula computes takes no readings: readings under a formula
are refused, since both cannot be kept and neither may be dropped without a
word. An `n/a` value keeps readings written beside it, but they give it no
uncertainty.

## Statistics of the readings, any time

Whatever the value, the readings can always be described. These are
**fields** you read, not values anyone chose:

| Field | Reads |
| --- | --- |
| `thickness.readings` | the readings, as a list |
| `thickness.stats.count` | how many |
| `thickness.stats.mean`, `thickness.stats.median` | their mean, their median |
| `thickness.stats.minimum`, `thickness.stats.maximum` | the extremes |
| `thickness.stats.sample_stdev`, `thickness.stats.population_stdev` | their spread |
| `thickness.stats.standard_error` | the standard error of their mean |
| `thickness.stats.first_quartile`, `thickness.stats.third_quartile` | the quartiles |

They work in a filter, a column, a profile and a sort, and in Python
(`steel.thickness.stats.count`, `steel["thickness.stats.mean"]`). They need no
model and write nothing. `thickness.stats.mean` is always the mean of the
readings; `thickness` is the value, which is the mean only if someone chose
it.

## Readings in a table cell

A cell of a table can hold readings as a property does: the force read on
three dials at one strain, for example.

```console
$ samplekit samples/steel-01.md set 'curve.force[0.2].readings=9.51,9.52,9.52' --write
$ samplekit samples/steel-01.md set --add-row curve strain=0.1 'force.readings=[5.12, 5.14]' --write
```

The statistic is declared **once for the whole column**, in the model:

```python
"force": sk.Property(unit="kN", value=sk.stats.mean,
                     uncertainty=sk.stats.standard_error),
```

and the file writes it beside the column's unit, so that the table still says
what its numbers are without the model. Each cell then behaves as a property
with readings: its value is computed, recorded, and outdated when a reading
changes; a value typed in a cell outranks the statistic; a column declaring
no statistic leaves its cells' readings without a value.

## Readings are not a summary

Two kinds of statistics are easy to confuse:

- **Statistics of one property's readings**, inside one sample: the mean of
  three micrometer readings of one specimen. They are the declared statistic,
  or the `thickness.stats.*` fields.
- **A summary of a column across samples**: the mean thickness of twelve
  specimens. That is `--summary` on the command line, `stats()` in Python
  and `S` in the TUI. It counts how many samples have a value (`11/12`), and
  when every value has an uncertainty its mean is weighted by 1/u², each value
  counting as much as it is precise; the header then says `mean (1/u²)`.

A summary reads each sample's value, whatever gave it: a typed value, a
declared statistic, or a formula.

## Common mistakes

- **Expecting readings to give a value on their own.** They do not. Declare a
  statistic in the model, or write a value beside them.
- **Writing a value beside readings and then correcting a reading.** Your
  written value stays; the preview says so. Empty it (`thickness=`) to let the
  statistic stand again.
- **Reading `thickness.stats.mean` as "the value".** It is a description of
  the readings. The value is `thickness`.
- **A decimal comma in a list of readings.** Use points.

## Where to go next

- The model's side: [Models and formulas](models-and-formulas.md).
- The [home page](../index.md#readings-and-their-statistics) declares
  statistics for a specimen's width and thickness, and [edit
  samples](../guides/edit-samples.md#measured-several-times) changes readings.
- The exact syntax of `set`: [Command line reference](../reference/cli.md);
  of the fields: [fields](../reference/sample-files.md#fields); `sk.stats`, `sk.Property`: [Python
  reference](../reference/python.md).
