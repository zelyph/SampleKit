# Values, uncertainty and units

A measurement is more than a number. It has an uncertainty, a unit, and a
number of digits that are worth showing. This page says how SampleKit holds
each of them, which of them live in the sample file and which in the project,
and why a missing value is not zero.

## A property in a file

```yaml
properties:
  width: {v: 12.513, u: 0.010, unit: mm}
  gauge_length: {v: 50.0, u: 0.1, unit: mm}
  thickness: 2.0
```

| Field | What it is |
| --- | --- |
| `v` | the **value** |
| `u` | its **uncertainty**: a standard uncertainty, absolute, in the same unit as the value |
| `unit` | the **unit**, as you write it |
| `readings` | repeated measurements behind the value ([Readings and statistics](readings-and-statistics.md)) |

A property with only a value can be written as a bare number:
`thickness: 2.0`. `v` and `u` are the only spellings, and you use them
everywhere else too: `width.v` is the value alone and `width.u` the
uncertainty alone, in a filter, a column, a `set` command or a Python index
(`steel["width.u"]`). In Python they are also `steel.width.value` and
`steel.width.uncertainty`.

## What goes in the file, and what goes in the project

One question decides it: **is this needed to read the file by eye, without
SampleKit?**

- The value, the uncertainty, the readings and the unit are: a reader who
  opens the file needs them to know what was measured. They are **in the
  file**.
- A symbol for a figure's axis and the number of digits to show are not.
  They are **declared once in the project**, in `.samplekitrc` under
  `[property.NAME]`, instead of being repeated in every file; a file may
  still give one value its own `symbol`, which wins
  ([properties](../reference/sample-files.md#properties)).

```toml
[property.width]
symbol = "w"
precision = [".3f", ".4f"]
```

A unit can also be declared there, for the files that give none: `samplekit
set` and `samplekit new` then write it for you. A unit written in a file wins
over the declared one, and `validate` reports the disagreement.

## Units are labels

SampleKit **never converts a unit and never interprets one**. `MPa` is
text that the file carries and the screen shows. There is no unit algebra, no
dimensional check, no conversion between `MPa` and `N/mm²`. What you type is what
you get, which is also why a unit is exact: nothing rewrites it behind your
back.

How a unit is *shown* can differ from how it is *written*. The file writes
`degC`, which is easy to type; the project can say to show it as `°C` in a
table, `^\circ\mathrm{C}` in LaTeX and `°C` on a figure:

```toml
[unit."degC"]
plain = "°C"
math = '^\circ\mathrm{C}'
figure = '°C'
```

The written unit is the key; the forms are only display. A unit no
`[unit.*]` names is shown as written. `plain` is the terminal's style, and
`latex`, `markdown` and `html` are given; `math` and `figure` here are styles
the project declares, `[style.math]` and `[style.figure]`, without which
`--style math` is refused ([configuration](../reference/configuration.md)).

What SampleKit does check is **agreement**. When every sample of a column has
the same unit, the unit moves to the header: `width [mm]`. When one sample has
a value without a unit, the header shows none rather than a wrong one. When
two samples write different units for one property, the table is refused and
`validate` names the files: a column in `mm` and `µm` is two tables, and
putting them side by side would compare numbers that do not compare.

## Uncertainty

An uncertainty is always **absolute** and in the value's unit: `{v: 50.0,
u: 0.1, unit: mm}` is 50.0 ± 0.1 mm. A relative uncertainty, a percentage, is
something you compute and write as absolute.

An uncertainty comes from you, or from a formula you wrote. **SampleKit never
invents one**: it does not propagate uncertainties through your formulas on
its own. A formula that should give an uncertainty returns one, computed the
way you choose ([Models and formulas](models-and-formulas.md)). A value
computed from uncertain inputs without such a formula simply has no
uncertainty.

Where the uncertainty comes from differs by case:

- written in the file, beside a value you entered;
- computed by a formula of the model, beside a value you entered (an
  instrument's rated accuracy, for example);
- computed with the value by the same formula;
- a statistic of the readings, such as their standard error.

`0` is an uncertainty: the value is exactly known. It is shown as `± 0`, and
is not the same as no uncertainty at all.

## Precision: how many digits

A **precision** says how a number is written: `".3f"` is three decimals,
`".2e"` scientific notation with two. A list of two, `[".3f", ".4f"]`, gives
the value's and then the uncertainty's.

**A precision is the project's, never the file's.** Sample files and models
hold none. It is declared at one of three levels, the first found wins:

1. a column of a profile, or a column on the command line (`-c 'width:.4f'`),
   or `--precision` for one command;
2. the property, `[property.width] precision`;
3. the whole project, `[render] precision`.

With none declared, SampleKit shows the uncertainty to **two significant
digits** and rounds the value to the same decimal place:

```text
stored 12.3456789 ± 0.01234   shown 12.346 ± 0.013
```

**How a number is rounded.** A value is rounded to the nearest, and a half
up: `6.9125` at three decimals is `6.913`. **An uncertainty is rounded up,
never understated**: `0.01234` to two digits is `0.013`, `0.121` at two
decimals `0.13`, and an exact zero stays `0`. A standard error in a summary is
an uncertainty, rounded up too. Both are rounded on the number as it is
written in decimal, not on the binary number the computer holds, so two
results equal in decimal are always shown alike; the last digits an addition
leaves, `0.30000000000000004` for 0.1 + 0.2, are dropped first, so that
uncertainty is `0.30`, not `0.31`. The screen, Python and every file
SampleKit writes show the same digits.

A value with neither an uncertainty nor a precision is shown in a short
general form, about six significant digits.

### Shown is not stored

**Rounding for display never changes the stored number.** The file keeps
every digit a measurement or a formula gave. Three consequences follow, and
they catch people:

- **A declared precision is applied as written, on the screen as in an
  export.** A terminal, a CSV, a JSON file and a figure round the same way,
  so what you export is what you saw. At `.3f` an uncertainty of 0.00006 is
  shown `0.001`, the smallest it can be written without understating it:
  declare more digits to see it as it is.
- **With no precision declared, an export writes the full stored number**
  (`12.3456789`), where the screen shows the rounded one. Declare a precision
  for the properties you export.
- **A filter compares the stored number**, not the one on screen.
  `thickness == 2.002` selects nothing when the file holds `2.0023333`, even
  though the table shows `2.002`. Compare with a range: `thickness > 2.0015 &&
  thickness < 2.0025`.

A preview of a change (`set`, `compute`, `restore`, `diff`) shows a value
as a table does, at its declared precision or by the rule above where none
is declared, where the digits a computation leaves (`6.081250000000001`)
would only be noise. A number you type is shown as the table will show it,
`25.0` as `25.000` beside an uncertainty of three decimals; where you typed
more digits than the table shows, `25.0123`, every one is kept. Two values
shown alike are told apart, since they are still a change.

## Symbols

A **symbol** is the property's name in a formula or on a figure's axis: `E`,
`R_m`, `\sigma`. It is declared in `[property.NAME] symbol`, with a form per
style if needed (`symbol_math`, `symbol_figure`); a table's column is
`[property."TABLE.COLUMN"]`. It never changes what the data is, and nothing
is computed from it.

A model declares no symbol, as it declares no precision: a model says how
a value is computed, and the project how it is written. `sk.Property(symbol=...)`
is refused, for a property as for a table's column, with a message saying
where the symbol goes.

## No value, not applicable, zero

Three different answers, and SampleKit keeps them apart:

| | In the file | Shown | Means |
| --- | --- | --- | --- |
| **Zero** | `0` | `0` | a measurement whose result is zero |
| **Absent** | nothing, or a key with no value | `—` | not recorded: nobody entered it yet |
| **Not applicable** | `n/a` | `n/a` | there is nothing to measure: the question does not apply to this sample |

**An absent value is never read as zero.** Treating "not recorded" as a
measurement of zero is how a missing reading turns into a wrong mean. So an
absent value is left out of a filter, shown `—` in a column, and not counted
in a summary; a formula that needs it **waits** instead of running
([Models and formulas](models-and-formulas.md)).

**`n/a` is an answer, where absent is a gap.** A specimen tested without an
extensometer has no gauge length to give: `gauge_length: n/a` says so, and
nothing waits for it.

- A value computed from an `n/a` is `n/a` too, without running its formula.
- It has no uncertainty: a spread of a number that is not there means
  nothing. Setting a value to `n/a` drops an uncertainty written beside it.
- A filter treats it as no value: `gauge_length is missing` finds it,
  `gauge_length > 40` does not.
- A summary counts it apart: over three samples, one holding `45`, one `n/a`
  and one with nothing, the count is `1/2 (1 n/a)`, one value of the two
  samples it applies to.
- A CSV writes `n/a`; JSON writes `null`.

Write it `gauge_length: n/a` in the file, `samplekit FILE set
gauge_length=n/a` on the command line, or `steel.gauge_length.value = sk.NA`
in Python. In Python an absent value
reads as `None`, and `n/a` as `samplekit.NA`.

## Numbers and text

A value is one of: a number, a text, a date (`2026-03-14`), a date and time
(`2026-03-14T10:30`), or true/false. YAML decides by how it is written:

- `yes` and `no` are **text**, not true and false. Write `true` and `false`.
- A number between quotes, `"12.5"`, is **text**.
- A decimal comma is not a number: `1,05` is refused where a number is
  expected. Write `1.05`.

**A number is written in its shortest form** when SampleKit writes the file:
leading and trailing zeros are dropped, `1.050` is written `1.05`, and an
uncertainty or a list of readings is written as decimals, `u: 12` as `12.0`. A
whole number standing alone, batch `3`, stays `3`. Neither changes a result (a
formula computes the same with `12` as with `12.0`) nor what is shown: the
digits a table, an export or a figure shows come from the precision the
project declares ([configuration](../reference/configuration.md)), never from
the digits in the file. A reading's resolution is declared there, once.

**A property holds a number**, with a unit or without. Text and true/false are
refused there, from the command line, from Python and in the TUI, and so is a
formula of a property that gives text: its value is *failed*. A table's
column with a unit holds numbers too; a column without one may hold text.
`validate` reports text, a date or true/false found in a property the model
declares, or one with a unit, by what the model says and never by what the
other samples hold. An attribute may hold text.

Why refuse rather than accept: a property that is sometimes `1.05` and
sometimes `"high"` cannot be sorted, averaged or plotted, and every one of
those would have to guess what to do with the text. So the guess is refused
where the data is written, not made silently where it is read:

- a filter comparing a field that holds text in every sample with a number
  (`code > 3`) is an error;
- where one sample holds text in a field the others hold as a number, the
  filter selects among the others and **names the one it left aside**;
- a summary leaves a column of text out, and says so.

## Common mistakes

- **Expecting SampleKit to convert units.** It will not. Write one unit per
  property across a project.
- **Writing a relative uncertainty in `u`.** `u` is absolute, in the value's
  unit.
- **Comparing a rounded number in a filter.** Filters see the stored number;
  use a range.
- **Exporting without a declared precision** and getting sixteen digits.
  Declare `[property.NAME] precision`.
- **Writing `0` for "not measured".** Leave it absent, or write `n/a` if it
  does not apply.
- **Writing a precision in a sample file.** It is refused: precision is the
  project's.

## Where to go next

- The keys `[unit.*]`, `[property.*]` and `[render]`: [Configuration
  reference](../reference/configuration.md).
- The fields a filter or a column can read (`width.v`, `width.u`, `width.unit`):
  [Fields](../reference/sample-files.md#fields).
- Practised on the [home page](../index.md#readings-and-their-statistics), and
  in [edit samples](../guides/edit-samples.md) and [tables and figures for a
  paper](../guides/export-for-a-paper.md).
