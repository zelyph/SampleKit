"""Table — tabular scientific data indexed by a parameter."""

from __future__ import annotations

from typing import Any, Callable, TYPE_CHECKING

if TYPE_CHECKING:
    from .sample import Sample

from .property import Property


class Column:
    """Column metadata for a Table.

    Parameters
    ----------
    unit : str
        Plain-text unit.
    unit_math : str, optional
        Math-mode unit for MathJax/LaTeX (defaults to unit).
    symbol : str, optional
        Text/unicode symbol for CLI/TUI display (defaults to column name).
    symbol_math : str, optional
        Math-mode symbol for MathJax/LaTeX (defaults to symbol).
    precision : str
        Format spec for values (default "").
    precision_unc : str, optional
        Format spec for uncertainties (defaults to precision).
    """

    def __init__(
        self,
        unit: str = "",
        unit_math: str | None = None,
        symbol: str | None = None,
        symbol_math: str | None = None,
        precision: str = "",
        precision_unc: str | None = None,
    ):
        self.unit = unit
        self.unit_math = unit_math or unit
        self.symbol = symbol
        self.symbol_math = symbol_math
        self.precision = precision
        self.precision_unc = precision_unc or precision

    def __repr__(self):
        parts = []
        if self.symbol:
            parts.append(f"symbol={self.symbol!r}")
        if self.symbol_math and self.symbol_math != self.symbol:
            parts.append(f"symbol_math={self.symbol_math!r}")
        if self.unit:
            parts.append(f"unit={self.unit!r}")
        return f"Column({', '.join(parts)})"


# ════════════════════════════════════════════════════════════
# RowView — lightweight proxy for row-level access
# ════════════════════════════════════════════════════════════

class RowView:
    """Read-only view of a single row in a Table.

    Access cell Properties via attribute: ``row.f``, ``row.Q``.
    """

    def __init__(self, table: Table, index_val: Any):
        object.__setattr__(self, '_table', table)
        object.__setattr__(self, '_index_val', index_val)

    def __getattr__(self, name: str) -> Property:
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        row = table._data.get(idx)
        if row is not None and name in row:
            return row[name]
        raise AttributeError(f"Row has no column {name!r}")

    def __getitem__(self, name: str) -> Property:
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        row = table._data.get(idx)
        if row is not None and name in row:
            return row[name]
        raise KeyError(name)

    def __contains__(self, name: str) -> bool:
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        row = table._data.get(idx)
        return row is not None and name in row

    def __iter__(self):
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        return iter(table._data.get(idx, {}))

    def keys(self) -> list[str]:
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        return list(table._data.get(idx, {}))

    def items(self):
        table = object.__getattribute__(self, '_table')
        idx = object.__getattribute__(self, '_index_val')
        return table._data.get(idx, {}).items()

    def __repr__(self):
        idx = object.__getattribute__(self, '_index_val')
        table = object.__getattribute__(self, '_table')
        row = table._data.get(idx, {})
        parts = []
        for name, prop in row.items():
            if name == table._index_name:
                continue
            v, u = prop.value, prop.uncertainty
            parts.append(f"{name}=({v}, {u})" if u is not None else f"{name}={v!r}")
        return f"Row({table._index_name}={idx}, {', '.join(parts)})"


# ════════════════════════════════════════════════════════════
# ColumnView — lazy proxy for column-level access
# ════════════════════════════════════════════════════════════

class ColumnView:
    """Read-only view of a single column across all rows.

    Access via ``table.f`` where ``f`` is a column name.
    """

    def __init__(self, table: Table, col_name: str):
        self._table = table
        self._col_name = col_name

    @property
    def values(self) -> list:
        """Values for this column, ordered by index."""
        return [
            self._table._data[idx][self._col_name].value
            if self._col_name in self._table._data[idx]
            else None
            for idx in self._table.index_values
        ]

    @property
    def uncertainties(self) -> list:
        """Uncertainties for this column, ordered by index."""
        return [
            self._table._data[idx][self._col_name].uncertainty
            if self._col_name in self._table._data[idx]
            else None
            for idx in self._table.index_values
        ]

    def __getitem__(self, index_val) -> Property:
        """Get the Property for this column at a given index value."""
        row = self._table._data.get(index_val)
        if row is None:
            raise KeyError(f"No row at index {index_val!r}")
        prop = row.get(self._col_name)
        if prop is None:
            raise KeyError(f"No column {self._col_name!r} at index {index_val!r}")
        return prop

    def __iter__(self):
        """Iterate over Property objects for this column, ordered by index."""
        for idx in self._table.index_values:
            prop = self._table._data[idx].get(self._col_name)
            if prop is not None:
                yield prop

    def __repr__(self):
        return f"ColumnView({self._col_name!r}, {len(self._table)} rows)"


# ════════════════════════════════════════════════════════════
# Table
# ════════════════════════════════════════════════════════════

class Table:
    """Tabular data indexed by a parameter (e.g. temperature).

    The **first column** in *columns* is the index. Every cell is a
    :class:`Property` prefilled with Column metadata at grid creation.

    Parameters
    ----------
    columns : dict[str, Column]
        Column definitions. The first key is the index column.
    index : list, optional
        Index values — creates the grid upfront (required for compute).
    data : dict, optional
        ``{index_val: {col: value, ...}}`` — static row values.
        Tuples are interpreted as ``(value, uncertainty)``.
    compute : dict[str, callable], optional
        Column-wise compute: ``{col: fn(index_values) → list[value]}``.
    compute_unc : dict[str, callable], optional
        Column-wise uncertainty: ``{col: fn(index_values) → list[unc]}``.
    compute_row : dict[str, tuple], optional
        Row-wise compute: ``{col: (fn, [dep1, dep2, ...])}``.
        ``fn`` receives dependency values (same row), returns a scalar.

    Examples
    --------
    >>> table = Table(
    ...     columns={"T": Column(unit="°C"), "f": Column(unit="GHz")},
    ...     index=[20, 30, 40],
    ...     compute={"f": lambda Ts: [8.878 - 0.001 * (T - 20) for T in Ts]},
    ... )

    >>> table = Table(
    ...     columns={"T": Column(unit="°C"), "f": Column(unit="GHz"), "Q": Column()},
    ...     data={20: {"f": (8.878, 9e-7), "Q": 24840},
    ...           30: {"f": 8.874, "Q": 24104}},
    ... )

    >>> table.add(T=40, f=8.858, Q=23500)
    """

    def __init__(
        self,
        columns: dict[str, Column] | None = None,
        index: list | None = None,
        data: dict | None = None,
        compute: dict[str, Callable] | None = None,
        compute_unc: dict[str, Callable] | None = None,
        compute_row: dict[str, tuple] | None = None,
        title: str | None = None,
    ):
        self.columns: dict[str, Column] = columns or {}
        self.title: str | None = title
        col_names = list(self.columns.keys())
        self._index_name: str = col_names[0] if col_names else ""

        self._compute_fns: dict[str, Callable] = compute or {}
        self._compute_unc_fns: dict[str, Callable] = compute_unc or {}
        self._compute_row_fns: dict[str, tuple] = compute_row or {}

        # {index_val: {col_name: Property}}  — includes index column
        self._data: dict[Any, dict[str, Property]] = {}

        # Set by Sample.__setattr__
        self._name: str | None = None
        self._parent: Sample | None = None

        # Collect index values from both sources
        idx_vals: list = list(index) if index else []
        if data:
            for k in data:
                if k not in idx_vals:
                    idx_vals.append(k)

        # Create grid (prefilled Properties for every cell)
        for iv in idx_vals:
            self._ensure_row(iv)

        # Static data
        if data:
            for iv, row_data in data.items():
                if isinstance(row_data, dict):
                    self._fill_row(iv, row_data)

        # Column-wise compute
        if idx_vals and self._compute_fns:
            self._run_column_compute(idx_vals)

        # Row-wise compute (after column-wise, so deps are ready)
        if idx_vals and self._compute_row_fns:
            self._run_row_compute(idx_vals)

    # ── Internal helpers ────────────────────────────────

    def _new_cell(self, col_name: str) -> Property:
        """Create a Property prefilled with Column metadata."""
        col = self.columns.get(col_name)
        prop = Property()
        if col:
            self._apply_column_meta(prop, col)
        return prop

    def _ensure_row(self, index_val):
        """Create a row with prefilled Properties for all columns."""
        if index_val in self._data:
            return
        row: dict[str, Property] = {}
        for col_name in self.columns:
            prop = self._new_cell(col_name)
            if col_name == self._index_name:
                prop.value = index_val
            row[col_name] = prop
        self._data[index_val] = row

    def _fill_row(self, index_val, values: dict):
        """Set values on existing Properties in a row."""
        row = self._data[index_val]
        for col_name, val in values.items():
            if col_name == self._index_name:
                continue
            if isinstance(val, Property):
                self._apply_column_meta(val, self.columns.get(col_name))
                row[col_name] = val
                continue
            prop = row.get(col_name)
            if prop is None:
                prop = self._new_cell(col_name)
                row[col_name] = prop
            if isinstance(val, tuple) and len(val) == 2:
                prop.value = val[0]
                prop.uncertainty = val[1]
            else:
                prop.value = val

    def _run_column_compute(self, idx_vals: list):
        """Execute column-wise compute functions."""
        for col_name, fn in self._compute_fns.items():
            values = fn(idx_vals)
            unc_fn = self._compute_unc_fns.get(col_name)
            uncertainties = unc_fn(idx_vals) if unc_fn else [None] * len(idx_vals)
            for idx_val, v, u in zip(idx_vals, values, uncertainties):
                prop = self._data[idx_val].get(col_name)
                if prop is None:
                    prop = self._new_cell(col_name)
                    self._data[idx_val][col_name] = prop
                prop.value = v
                if u is not None:
                    prop.uncertainty = u

    def _run_row_compute(self, idx_vals: list):
        """Execute row-wise compute functions (lazy Properties)."""
        for col_name, (fn, deps) in self._compute_row_fns.items():
            col = self.columns.get(col_name)
            for idx_val in idx_vals:
                row = self._data[idx_val]
                dep_props = [row[d] for d in deps]

                def _make_compute(fn_=fn, deps_=dep_props):
                    return lambda: fn_(*(p.value for p in deps_))

                prop = Property(
                    compute=_make_compute(),
                    depends_on=dep_props,
                )
                if col:
                    self._apply_column_meta(prop, col)
                row[col_name] = prop

    @staticmethod
    def _apply_column_meta(prop: Property, col: Column | None):
        """Copy Column metadata into a Property if not already set."""
        if col is None:
            return
        if not prop.unit and col.unit:
            prop.unit = col.unit
            prop.unit_math = col.unit_math
        if not prop.precision and col.precision:
            prop.precision = col.precision
        if prop.precision_unc == prop.precision and col.precision_unc != col.precision:
            prop.precision_unc = col.precision_unc
        if prop.symbol is None and col.symbol:
            prop.symbol = col.symbol
        if prop.symbol_math is None and col.symbol_math:
            prop.symbol_math = col.symbol_math

    # ── Index ───────────────────────────────────────────

    @property
    def index(self) -> str:
        """Name of the index column."""
        return self._index_name

    @property
    def index_unit(self) -> str:
        """Unit of the index column."""
        col = self.columns.get(self._index_name)
        return col.unit if col else ""

    @property
    def index_values(self) -> list:
        """Sorted list of index values."""
        vals = list(self._data.keys())
        try:
            return sorted(vals)
        except TypeError:
            return vals

    @property
    def data_columns(self) -> list[str]:
        """Non-index column names, in order."""
        return [c for c in self.columns if c != self._index_name]

    # ── Add rows ────────────────────────────────────────

    def add(self, **kwargs):
        """Add a row to the table.

        The first-column keyword is the index value;
        other keywords are column values.
        Tuples are ``(value, uncertainty)``.

        >>> table.add(T=20, f=8.878, Q=24840)
        >>> table.add(T=30, f=(8.874, 9e-7), Q=24104)
        """
        if self._index_name not in kwargs:
            raise ValueError(f"Missing index column {self._index_name!r}")
        index_val = kwargs.pop(self._index_name)
        self._ensure_row(index_val)
        self._fill_row(index_val, kwargs)

    # ── Access by position ──────────────────────────────

    def __getitem__(self, key) -> RowView | list[RowView]:
        """Positional access: ``table[0]``, ``table[-1]``, ``table[0:3]``."""
        ordered = self.index_values
        if isinstance(key, int):
            if key < 0:
                key += len(ordered)
            if key < 0 or key >= len(ordered):
                raise IndexError(f"Table index {key} out of range")
            return RowView(self, ordered[key])
        if isinstance(key, slice):
            return [RowView(self, ordered[i]) for i in range(*key.indices(len(ordered)))]
        raise TypeError(
            f"Use table({key!r}) for index-value access, "
            f"table[int] for positional access"
        )

    # ── Access by index value ───────────────────────────

    def __call__(self, index_val) -> RowView:
        """Value-based access: ``table(20)`` → RowView for index=20."""
        if index_val not in self._data:
            raise KeyError(f"No row at index {index_val!r}")
        return RowView(self, index_val)

    def __contains__(self, index_val) -> bool:
        return index_val in self._data

    def __len__(self) -> int:
        return len(self._data)

    def __iter__(self):
        """Iterate over RowView objects, sorted by index."""
        for idx in self.index_values:
            yield RowView(self, idx)

    def __bool__(self) -> bool:
        return len(self._data) > 0

    # ── Access by column ────────────────────────────────

    def __getattr__(self, name: str):
        if name.startswith('_'):
            raise AttributeError(name)
        col_names = set()
        try:
            col_names.update(object.__getattribute__(self, 'columns'))
        except AttributeError:
            pass
        if name in col_names:
            return ColumnView(self, name)
        raise AttributeError(f"'{type(self).__name__}' has no column {name!r}")

    # ── Serialization ───────────────────────────────────

    @staticmethod
    def _cell_to_yaml(prop: Property):
        """Serialize a cell — value only (+ uncertainty if present).

        Column metadata (unit, precision…) is already in ``columns=``
        so we never duplicate it in the YAML.
        """
        v = prop.value
        if v is None:
            return None
        u = prop.uncertainty
        if u is not None:
            return {"value": v, "uncertainty": u}
        return v

    @staticmethod
    def _cell_from_yaml(prop: Property, raw):
        """Deserialize a cell — scalar or {value, uncertainty}."""
        if isinstance(raw, dict):
            val = raw.get("value")
            unc = raw.get("uncertainty")
        else:
            val, unc = raw, None

        if val is not None:
            if prop.is_computed:
                prop._seed_cache(value=val)
            else:
                prop.value = val
        if unc is not None:
            if prop._compute_unc is not None:
                prop._seed_cache(uncertainty=unc)
            else:
                prop.uncertainty = unc

    def _columns_to_yaml(self) -> list:
        """Serialize column metadata as a list of dicts."""
        cols = []
        for name, col in self.columns.items():
            d: dict[str, Any] = {"name": name}
            if col.unit:
                d["unit"] = col.unit
            if col.unit_math and col.unit_math != col.unit:
                d["unit_math"] = col.unit_math
            sym = col.symbol
            if sym and sym != name:
                d["symbol"] = sym
            if col.symbol_math and col.symbol_math != (sym or name):
                d["symbol_math"] = col.symbol_math
            if col.precision:
                d["precision"] = col.precision
            if col.precision_unc and col.precision_unc != col.precision:
                d["precision_unc"] = col.precision_unc
            cols.append(d)
        return cols

    def to_yaml(self) -> dict:
        """Convert to a YAML-friendly nested dict.

        Includes ``_title``, ``_index``, ``_columns`` metadata,
        then a ``_rows`` list where each row is a dict including the
        index column as a regular value.
        Cells carry only value (+ uncertainty if present).
        """
        result: dict[str, Any] = {}
        if self.title:
            result["_title"] = self.title
        result["_index"] = self._index_name
        result["_columns"] = self._columns_to_yaml()
        rows: list[dict[str, Any]] = []
        for idx_val in self.index_values:
            row = self._data[idx_val]
            entry: dict[str, Any] = {}
            for name, prop in row.items():
                serialized = self._cell_to_yaml(prop)
                if serialized is not None:
                    entry[name] = serialized
            if entry:
                rows.append(entry)
        result["_rows"] = rows
        return result

    def from_yaml(self, data: dict):
        """Populate this Table from a YAML nested dict."""
        # ── Metadata ────────────────────────────────────
        if "_title" in data:
            self.title = data["_title"]

        if "_index" in data:
            self._index_name = data["_index"]

        # Build columns from YAML if none were defined in code
        if "_columns" in data and not self.columns:
            for col_entry in data["_columns"]:
                if not isinstance(col_entry, dict):
                    continue
                col_name = col_entry.get("name", "")
                if not col_name:
                    continue
                self.columns[col_name] = Column(
                    unit=col_entry.get("unit", ""),
                    unit_math=col_entry.get("unit_math"),
                    symbol=col_entry.get("symbol"),
                    symbol_math=col_entry.get("symbol_math"),
                    precision=col_entry.get("precision", ""),
                    precision_unc=col_entry.get("precision_unc"),
                )
            if not self._index_name and self.columns:
                self._index_name = next(iter(self.columns))

        # ── Data rows ───────────────────────────────────
        for row_data in data.get("_rows", []):
            if not isinstance(row_data, dict):
                continue
            idx_val = row_data.get(self._index_name)
            if idx_val is None:
                continue
            self._ensure_row(idx_val)
            row = self._data[idx_val]
            for key, raw in row_data.items():
                if key == self._index_name:
                    continue
                prop = row.get(key)
                if prop is None:
                    prop = self._new_cell(key)
                    row[key] = prop
                self._cell_from_yaml(prop, raw)

    # ── Display ─────────────────────────────────────────

    def __repr__(self):
        cols = ", ".join(self.data_columns)
        return f"Table({self._name}: {len(self)} rows, [{cols}])"

    def __str__(self):
        idx_name = self._index_name or "?"
        lines = [f"Table: {self._name or '?'} ({len(self)} rows)"]
        lines.append(f"  Index: {idx_name} ({self.index_unit})")
        for idx_val in self.index_values:
            row = self._data[idx_val]
            vals = ", ".join(
                f"{k}={p.value}" for k, p in row.items()
                if k != self._index_name and p.value is not None
            )
            lines.append(f"  {idx_val}: {vals}")
        return "\n".join(lines)
