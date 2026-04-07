# SampleKit

**Lightweight Python framework for documenting scientific samples with bidirectional Markdown I/O.**

---

## Origin

SampleKit was born during my PhD research — I needed a structured way to document and process scientific samples without leaving Python or relying on proprietary formats. The first version was written entirely by hand. The current rewrite was **vibecoded** with [GitHub Copilot](https://github.com/features/copilot) (Claude) because maintaining a side-project framework alongside a thesis is, frankly, unsustainable otherwise.

---

## What it does

SampleKit sits between the **lab notebook** and the **analysis script**. It gives you a structured, human-readable record of measurements that stays fully exploitable in code — no proprietary format, no binary files.

Each sample is a plain **Markdown file** with a YAML header for the raw data and a body for formatted tables and notes. You can generate it from code, hand-edit it, and reload it — the round-trip is lossless.

### Core concepts

| Concept | Purpose |
|---|---|
| **Property** | A scalar scientific quantity: value ± uncertainty, unit, symbols (text & LaTeX). Pass a list of measurements → auto mean ± stdev. Define a `compute` callback → lazy evaluation with dependency-based cache invalidation. |
| **Table** | Tabular scientific data indexed by a primary variable (temperature, time, …). Each cell is a `Property` — value, uncertainty, unit, formatting. Supports row-wise and column-wise computed columns. |
| **Column** | Metadata descriptor for a Table column: unit, symbol, precision. |
| **Sample** | A named container of Properties and Tables. Subclass it, declare data in `__init__`, optionally override `template()` for a custom Markdown layout. Reads/writes `.md` files with lossless round-trip. |
| **SampleList** | A collection of Samples loaded from a directory, file list, or built programmatically. Supports filtering, sorting, batch saving, and pandas export. |

### Key features

- **Bidirectional Markdown I/O** — Write a `.md` file from code, edit it by hand, reload it. YAML frontmatter stores raw data, the body is regenerated from `template()`.
- **Auto statistics** — Pass a list of measurements and SampleKit computes the mean and sample standard deviation automatically.
- **Computed properties** — Define a `compute` callback for lazy evaluation. Declare `depends_on` and dependent caches are invalidated on change.
- **Tables** — Index-based tabular data with per-cell uncertainty. Row-wise and column-wise computed columns. Full YAML round-trip.
- **Dual symbol system** — `symbol` (text/unicode for CLI/TUI) and `symbol_math` (LaTeX for reports). Automatic fallback: `symbol_math` → `symbol` → attribute name.
- **Text & math rendering** — Properties and tables render in `text` mode (plain text) or `math` mode (inline LaTeX `$...$`), ready for Pandoc conversion to PDF.
- **Pandas integration** — Export a single sample or an entire collection to a DataFrame.
- **Pure Python** — Only depends on PyYAML and pandas. `pip install` and go.

---

## Installation

> **Requires** Python ≥ 3.10

```bash
pip install samplekit
```

---

## Quick start

### Property

```python
from samplekit import Property

# Static value
length = Property(value=12.5, uncertainty=0.1, unit="mm", symbol="L")
print(length)  # 12.5 ± 0.1 mm

# Measured value — auto mean ± stdev
readings = Property(value=[101.1, 101.3, 101.5], unit="kPa", symbol="P")
print(readings.value)        # 101.3
print(readings.uncertainty)  # ~0.2
```

### Table

```python
from samplekit import Table, Column

measurements = Table(
    title="Resistance vs Temperature",
    columns={
        "T": Column(unit="C", symbol="T"),
        "R": Column(unit="ohm", symbol="R", precision=".2f"),
    },
)

measurements.add(T=20, R=100.5)
measurements.add(T=40, R=102.3)
measurements.add(T=60, R=(105.1, 0.3))  # value ± uncertainty

print(measurements(40).R.value)      # 102.3
print(measurements.R.values)         # [100.5, 102.3, 105.1]
```

### Sample

```python
from samplekit import Sample, Property, Table, Column, report


class Experiment(Sample):
    def __init__(self, name=None, filepath=None):
        super().__init__(name, filepath)

        self.temperature = Property(
            value=25.0, uncertainty=0.5,
            unit="C", symbol="T",
        )
        self.pressure = Property(
            value=[101.1, 101.3, 101.5],
            unit="kPa", symbol="P",
        )
        self.ratio = Property(
            unit="kPa/C", symbol="R",
            compute=self._calc_ratio,
            depends_on=[self.pressure, self.temperature],
        )
        self.readings = Table(
            title="Sensor readings",
            columns={
                "t": Column(unit="s"),
                "signal": Column(unit="mV", precision=".2f"),
            },
        )

    def _calc_ratio(self):
        return self.pressure.value / self.temperature.value

    def template(self, style="math"):
        parts = []
        parts.append(report.heading(f"Experiment: {self.name}"))
        parts.append(report.properties_table(
            self, ["temperature", "pressure", "ratio"], style=style,
        ))
        if self.readings:
            parts.append(report.heading("Readings"))
            parts.append(report.table_to_markdown(self.readings, style=style))
        return "\n\n".join(parts)


exp = Experiment("EXP_001")
exp.readings.add(t=0, signal=12.34)
exp.readings.add(t=1, signal=12.51)

# Save to Markdown, then reload (round-trip is lossless)
exp.save("EXP_001.md")
loaded = Experiment.load("EXP_001.md")
```

**Generated file (`EXP_001.md`)**:

```markdown
---
name: EXP_001
temperature: {value: 25.0, uncertainty: 0.5, unit: C, precision: .1f}
pressure: {value: 101.3, data: [101.1, 101.3, 101.5], uncertainty: 0.2, unit: kPa}
ratio: {value: 4.052, unit: kPa/C}
readings:
  _title: Sensor readings
  _index: t
  _columns:
    - {name: t, unit: s}
    - {name: signal, unit: mV, precision: '.2f'}
  _rows:
    - {t: 0, signal: 12.34}
    - {t: 1, signal: 12.51}
---

## Experiment: EXP_001

| Property | Value | Unit |
| :---: | :---: | :---: |
| $T$ | $25.0 \pm 0.5$ | $C$ |
| $P$ | $101.3 \pm 0.2$ | $kPa$ |
| $R$ | $4.052$ | $kPa/C$ |

## Readings

| $t$ ($s$) | $signal$ ($mV$) |
| :---: | :---: |
| 0 | 12.34 |
| 1 | 12.51 |
```

### SampleList

```python
from samplekit import SampleList

# Load all samples from a directory
samples = SampleList("data/", sample_class=Experiment)

# Filter and sort
hot = samples.filter(lambda s: s.temperature.value > 30)
ordered = hot.sort("pressure")

# Multi-key sort
by_group = samples.sort(
    [lambda s: s.group.value, "temperature"],
    reverse=[False, True],
)

# Batch save
samples.save_all("output/", overwrite=True)

# Export to pandas DataFrame
df = samples.to_dataframe()
```

---

## File format

SampleKit uses Markdown files with YAML frontmatter:

```yaml
---
name: SAMPLE_001

# Scalar properties
label: Test coupon A
length: {value: 50.12, data: [50.12, 50.08, 50.15], uncertainty: 0.035, unit: mm}
width: {value: 25.0, unit: mm}
area: 1252.9

# Table
measurements:
  _title: Resistance vs Temperature
  _index: T
  _columns:
    - {name: T, unit: C}
    - {name: R, unit: ohm, precision: '.2f'}
  _rows:
    - {T: 20, R: 100.5}
    - {T: 40, R: 102.3}
---
```

**Properties:**
- Bare number if only a value: `area: 1252.9`
- Bare string for text: `label: Test coupon A`
- Dict with metadata: `{value, data, uncertainty, unit, symbol, symbol_math, precision, ...}`
- Conditional storage — redundant fields are omitted (e.g. `symbol_math` if same as `symbol`)

**Tables:**
- `_title`: display title
- `_index`: name of the index column
- `_columns`: list of column descriptors `[{name, unit, symbol, precision, ...}]`
- `_rows`: list of row dicts `[{col: value_or_{value, uncertainty}}]`

---

## API reference

### `Property`

```python
Property(
    value=None,               # float, str, list[float], or None
    uncertainty=None,          # float — overrides auto-stdev if set
    unit="",                   # display unit (text)
    unit_math=None,            # LaTeX unit (defaults to unit)
    symbol=None,               # text symbol (defaults to attr name)
    symbol_math=None,          # LaTeX symbol (defaults to symbol)
    precision="",              # format spec, e.g. ".2f", ".1e"
    precision_unc=None,        # format spec for uncertainty (defaults to precision)
    compute=None,              # callable → lazy, cached value
    compute_unc=None,          # callable → lazy uncertainty
    depends_on=None,           # list[Property] → auto-invalidation
)
```

| Member | Description |
|---|---|
| `.value` | Get/set the value. Lists → auto mean. Setting clears `compute`. |
| `.uncertainty` | Get/set uncertainty. Lists → auto stdev. Setting clears `compute_unc`. |
| `.data` | Raw measurement list (read-only copy), or `None`. |
| `.text` | Shortcut for `.format()` → `"25.0 ± 0.5 mm"`. |
| `.format(unit=True)` | Plain-text representation. |
| `.is_computed` | `True` if the value comes from a `compute` callback. |
| `.invalidate()` | Clear cache and propagate to dependents. |

### `Table`

```python
Table(
    columns=None,              # dict[str, Column] — first key is the index
    index=None,                # list — pre-populate index values
    data=None,                 # dict — static data: {idx: {col: val}}
    compute=None,              # dict — column-wise: {col: fn(index_vals) → list}
    compute_unc=None,          # dict — column-wise uncertainty
    compute_row=None,          # dict — row-wise: {col: (fn, [dep_cols])}
    title=None,                # display title
)
```

| Member | Description |
|---|---|
| `.add(**kwargs)` | Add a row. Tuples `(value, unc)` for uncertainty. |
| `table(idx)` | Access row by index value → `RowView`. |
| `table[i]` | Access row by position → `RowView`. |
| `table.col` | Access column → `ColumnView` (`.values`, `.uncertainties`). |
| `len(table)` | Number of rows. |
| `bool(table)` | `True` if any rows exist. |
| `.index_values` | Sorted list of index values. |
| `.data_columns` | List of non-index column names. |

### `Column`

```python
Column(
    unit="",                   # display unit
    unit_math=None,            # LaTeX unit
    symbol=None,               # text symbol
    symbol_math=None,          # LaTeX symbol
    precision="",              # format spec
    precision_unc=None,        # format spec for uncertainty
)
```

### `Sample`

```python
Sample(name=None, filepath=None)
```

Subclass it and declare Properties and Tables in `__init__`. Assignment auto-registers them and wires names, symbols, and dependencies.

| Member | Description |
|---|---|
| `.props` | `dict[str, Property]` — all registered properties. |
| `.tables` | `dict[str, Table]` — all registered tables. |
| `.save(filepath, style="math")` | Write YAML frontmatter + template body to `.md`. |
| `.load(filepath)` | classmethod — load from `.md` file. |
| `.template(style="math")` | Override for custom Markdown body. |
| `.to_dict()` | Export all data as a plain dict. |
| `.to_dataframe()` | Export scalar properties as a single-row DataFrame. |

### `SampleList`

```python
SampleList(source=None, sample_class=Sample, pattern="*.md")
```

`source` can be a directory path, a list of file paths, or a list of Sample objects.

| Member | Description |
|---|---|
| `.filter(func)` | New SampleList with matching samples. |
| `.sort(key, reverse=False)` | Sort by property name, callable, or list (multi-key). |
| `.save_all(directory, overwrite=False)` | Save each sample as `{name}.md`. |
| `.append(sample)` | Add a sample. |
| `samples[i]`, `samples["name"]`, `samples[a:b]` | Access by index, name, or slice. |
| `.to_dataframe()` | Concatenated DataFrame, one row per sample. |
| `.stats(prop_name)` | Descriptive statistics for a property across all samples. |

### `report`

| Function | Description |
|---|---|
| `heading(text, level=2)` | Markdown heading. |
| `format_property(prop, style, unit=True)` | Format a Property in `"text"` or `"math"` mode. |
| `properties_table(sample, names, style="math")` | Render selected properties as a markdown table. |
| `table_to_markdown(table, style="math")` | Render a Table as a markdown table. |
| `markdown_table(rows, headers, align)` | Generic markdown table builder. |

### `converters`

| Function | Description |
|---|---|
| `sample_to_dict(sample)` | Sample → plain dict. |
| `sample_to_dataframe(sample)` | Sample → single-row DataFrame. |
| `samplelist_to_dataframe(slist)` | SampleList → DataFrame (one row per sample). |
| `samplelist_stats(slist, prop)` | Descriptive statistics (via `pd.Series.describe()`). |

These are also accessible as methods: `sample.to_dict()`, `samples.to_dataframe()`, etc.

---

## Roadmap

- [ ] **CLI** — Terminal access to samples: read, compare, export.
- [ ] **TUI** — Interactive terminal explorer with filtering and keyboard navigation.
- [ ] **Plotting** — Matplotlib integration with automatic axis labels from metadata and error bars.
- [ ] **LaTeX report** — Direct PDF generation from sample data.
- [ ] **Additional export formats** — CSV, JSON, LaTeX tables.

---

## Authors

**Thomas Lavie** ([@zelyph](https://github.com/zelyph)) — design, original implementation, PhD research context

**GitHub Copilot** (Claude) — v0.1 rewrite, architecture, documentation

---

## License

[MIT](LICENSE)


