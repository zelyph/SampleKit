# Values, uncertainty and units

A measurement is more than a number. It has an uncertainty, a unit, and a
number of digits that are worth showing. This page says how SampleKit holds
each of them, which of them live in the sample file and which in the project,
and why a missing value is not zero.

## A quantity in a file

```yaml
properties:
  og: {v: 1.058, u: 0.001}
  volume: {v: 19.5, u: 0.3, unit: L}
  grain_mass: 5.6
```

| Field | What it is |
| --- | --- |
| `v` | the **value** |
| `u` | its **uncertainty**: a standard uncertainty, absolute, in the same unit as the value |
| `unit` | the **unit**, as you write it |
| `readings` | repeated measurements behind the value ([Readings and statistics](readings-and-statistics.md)) |

A quantity with only a value can be written as a bare number:
`grain_mass: 5.6`. `v` and `u` are the only spellings, and you use them
everywhere else too: `og.v` is the value alone and `og.u` the uncertainty
alone, in a filter, a column, a `set` command or a Python index
(`brew["og.u"]`). In Python they are also `brew.og.value` and
`brew.og.uncertainty`.

## What goes in the file, and what goes in the project

One question decides it: **is this needed to read the file by eye, without
SampleKit?**

- The value, the uncertainty, the readings and the unit are: a reader who
  opens the file needs them to know what was measured. They are **in the
  file**.
- A symbol for a figure's axis and the number of digits to show are not.
  They are **declared once in the project**, in `.samplekitrc` under
  `[property.NAME]`, instead of being repeated in every file.

```toml
[property.og]
symbol = "OG"
precision = [".3f", ".4f"]
```

A unit can also be declared there, for the files that give none: `samplekit
set` and `samplekit new` then write it for you. A unit written in a file wins
over the declared one, and `validate` reports the disagreement.

## Units are labels

SampleKit **never converts a unit and never interprets one**. `g/L` is
text that the file carries and the screen shows. There is no unit algebra, no
dimensional check, no conversion between `mL` and `L`. What you type is what
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
`[unit.*]` names is shown as written.

What SampleKit does check is **agreement**. When every sample of a column has
the same unit, the unit moves to the header: `volume [L]`. When one sample has
a value without a unit, the header shows none rather than a wrong one. When
two samples write different units for one quantity, the table is refused and
`validate` names the files: a column in `L` and `mL` is two tables, and
putting them side by side would compare numbers that do not compare.

## Uncertainty

An uncertainty is always **absolute** and in the value's unit: `{v: 19.5,
u: 0.3, unit: L}` is 19.5 ± 0.3 L. A relative uncertainty, a percentage, is
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

1. a column of a profile, or a column on the command line (`-c 'og:.4f'`),
   or `--precision` for one command;
2. the quantity, `[property.og] precision`;
3. the whole project, `[render] precision`.

With none declared, SampleKit shows the uncertainty to **two significant
digits** and rounds the value to the same decimal place:

```text
stored 12.3456789 ± 0.01234   shown 12.346 ± 0.012
```

A value with neither an uncertainty nor a precision is shown in a short
general form, about six significant digits.

### Shown is not stored

**Rounding for display never changes the stored number.** The file keeps
every digit a measurement or a formula gave. Three consequences follow, and
they catch people:

- **A declared precision is applied as written, on the screen as in an
  export.** A terminal, a CSV, a JSON file and a figure round the same way,
  so what you export is what you saw. If `.3f` rounds an uncertainty of
  0.00006 to `0.000`, that is what is shown; `validate` notes a precision
  that writes a zero, so that you can declare more digits.
- **With no precision declared, an export writes the full stored number**
  (`12.3456789`), where the screen shows the rounded one. Declare a precision
  for the quantities you export.
- **A filter compares the stored number**, not the one on screen.
  `fg == 1.012` selects nothing when the file holds `1.0116666`, even though
  the table shows `1.012`. Compare with a range: `fg > 1.0115 && fg < 1.0125`.

A preview of a change (`set`, `compute --try`, `restore`) shows a value at
its declared precision, as a table does, where the digits a computation
leaves (`6.081250000000001`) would only be noise. A value with no precision
declared is shown with every digit, and two values the precision shows alike
are shown whole, since they are still a change.

## Symbols

A **symbol** is the quantity's name in a formula or on a figure's axis: `OG`,
`T_mash`, `\eta`. It is declared in `[property.NAME] symbol`, with a form per
style if needed (`symbol_math`, `symbol_figure`); a table's column is
`[property."TABLE.COLUMN"]`. It never changes what the data is, and nothing
is computed from it.

A model declares no symbol, as it declares no precision: a model says how
a value is computed, and the project how it is written. `sk.Property(symbol=...)`
and `sk.Column(symbol=...)` are refused, with a message saying where the
symbol goes.

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

**`n/a` is an answer, where absent is a gap.** A drink never fermented has no
final gravity to measure: `fg: n/a` says so, and nothing waits for it.

- A value computed from an `n/a` is `n/a` too, without running its formula.
- It has no uncertainty: a spread of a number that is not there means
  nothing. Setting a value to `n/a` drops an uncertainty written beside it.
- A filter treats it as no value: `fg is missing` finds it, `fg > 1.010` does
  not.
- A summary counts it apart: `10/11 (1 n/a)`.
- A CSV writes `n/a`; JSON writes `null`.

Write it `fg: n/a` in the file, `samplekit set FILE fg=n/a` on the command
line, or `brew.fg.value = sk.NA` in Python. In Python an absent value
reads as `None`, and `n/a` as `samplekit.NA`.

## Numbers and text

A value is one of: a number (an integer stays an integer: batch `3` is not
`3.0`), a text, a date (`2026-03-14`), a date and time (`2026-03-14T10:30`),
or true/false. YAML decides by how it is written:

- `yes` and `no` are **text**, not true and false. Write `true` and `false`.
- A number between quotes, `"12.5"`, is **text**.
- A decimal comma is not a number: `1,05` is refused where a number is
  expected. Write `1.05`.

**A property holds a number**, with a unit or without. Text and true/false are
refused there, from the command line, from Python and in the TUI, and
`validate` reports text found in a quantity. An attribute may hold text.

Why refuse rather than accept: a quantity that is sometimes `1.05` and
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
  quantity across a project.
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
- The fields a filter or a column can read (`og.v`, `og.u`, `og.unit`):
  [Command line reference](../reference/cli.md).
- Practised in tutorial steps [2](../tutorial/02-measuring.md) and
  [3](../tutorial/03-selecting.md); [Export a table for a
  paper](../how-to/export-for-a-paper.md).
