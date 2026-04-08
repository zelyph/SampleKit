"""Rendering utilities — markdown tables, property tables, Table rendering."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .property import Property
    from .sample import Sample
    from .table import Table

# Characters that need escaping inside $...$ (LaTeX math mode)
_LATEX_SPECIAL = str.maketrans({"#": r"\#", "%": r"\%", "&": r"\&", "_": r"\_"})

import re
_SCI_RE = re.compile(r'^([+-]?\d+(?:\.\d+)?)[eE]([+-]?\d+)$')

def _math_sci(s: str) -> str:
    """Convert '9e-07' → '9 \\times 10^{-7}' for math mode."""
    m = _SCI_RE.match(s.strip())
    if not m:
        return s
    mantissa, exp = m.group(1), int(m.group(2))
    return f"{mantissa} \\times 10^{{{exp}}}"


# ════════════════════════════════════════════════════════════
# Property formatting
# ════════════════════════════════════════════════════════════

def format_property(prop: Property, style: str = "text", unit: bool = True) -> str:
    """Format a Property for display.

    Parameters
    ----------
    prop : Property
    style : "text" or "math"
        "text" → plain text (e.g. ``25.0 ± 0.5 °C``)
        "math" → inline math (e.g. ``$25.0 \\pm 0.5$ $°C$``)
    unit : bool
        Include unit in output.
    """
    if style == "math":
        v = prop.value
        if v is None:
            return "N/A"
        if isinstance(v, str):
            if unit and prop.unit and prop.unit != "-":
                return f"{v} {prop.unit}"
            return v
        u = prop.uncertainty
        v_str = _math_sci(f"{v:{prop.precision}}")
        if u is not None and u != 0:
            u_str = _math_sci(f"{u:{prop.precision_unc}}")
            val_part = f"${v_str} \\pm {u_str}$"
        else:
            val_part = f"${v_str}$"
        if unit and prop.unit and prop.unit != "-":
            return f"{val_part} ${prop.unit_math.translate(_LATEX_SPECIAL)}$"
        return val_part
    return prop.format(unit=unit)


# ════════════════════════════════════════════════════════════
# Generic markdown table
# ════════════════════════════════════════════════════════════

def markdown_table(
    rows: list[list[str]],
    headers: list[str],
    align: list[str] | None = None,
) -> str:
    """Generate a markdown table.

    Parameters
    ----------
    rows : list of list of str
        Table data.
    headers : list of str
        Column headers.
    align : list of str, optional
        Per-column alignment: "l", "c", "r". Defaults to center.
    """
    ncols = len(headers)
    align = align or ["c"] * ncols

    header_row = "| " + " | ".join(headers) + " |"
    sep_map = {"l": ":---", "r": "---:", "c": ":---:"}
    sep_row = "| " + " | ".join(sep_map.get(a, ":---:") for a in align) + " |"
    data_rows = "\n".join("| " + " | ".join(row) + " |" for row in rows)

    return f"{header_row}\n{sep_row}\n{data_rows}"


# ════════════════════════════════════════════════════════════
# Property table
# ════════════════════════════════════════════════════════════

def properties_table(
    sample: Sample,
    names: list[str],
    headers: list[str] | None = None,
    style: str = "math",
    align: list[str] | None = None,
) -> str:
    """Render selected scalar properties as a markdown table.

    Parameters
    ----------
    sample : Sample
    names : list of property names to include
    headers : column headers (default: ["Property", "Value", "Unit"])
    style : "math" or "text"
    """
    headers = headers or ["Property", "Value", "Unit"]
    math = style == "math"
    props = sample.props

    rows = []
    for name in names:
        prop = props.get(name)
        if prop is None or prop.value is None:
            continue
        if math:
            sym = prop.symbol_math or prop.symbol or name
            sym_cell = f"${sym}$"
        else:
            sym = prop.symbol or name
            sym_cell = sym
        val_cell = format_property(prop, style, unit=False)
        unit_cell = f"${prop.unit_math}$" if math and prop.unit else prop.unit
        rows.append([sym_cell, val_cell, unit_cell])

    return markdown_table(rows, headers, align or ["c"] * len(headers))


# ════════════════════════════════════════════════════════════
# Table rendering
# ════════════════════════════════════════════════════════════

def _col_header(col_name: str, col, style: str) -> str:
    """Build a column header string (symbol + unit) for a given style."""
    math = style == "math"
    if col:
        if math:
            sym = col.symbol_math or col.symbol or col_name
        else:
            sym = col.symbol or col_name
    else:
        sym = col_name

    h = f"${sym}$" if math else sym

    if col and col.unit and col.unit != "-":
        unit_str = f"${col.unit_math.translate(_LATEX_SPECIAL)}$" if math else col.unit
        h += f" ({unit_str})"
    return h


def table_to_markdown(
    table: Table,
    style: str = "math",
    columns: list[str] | None = None,
    index_label: str | None = None,
    align: list[str] | None = None,
) -> str:
    """Render a Table as a markdown table.

    Parameters
    ----------
    table : Table
    style : "math" or "text"
    columns : list of column names to include (default: all)
    index_label : override for the index column header
    """
    math = style == "math"
    cols = columns or table.data_columns

    # Index header
    if index_label:
        idx_header = index_label
    else:
        idx_col = table.columns.get(table.index)
        idx_header = _col_header(table.index, idx_col, style)

    # Column headers
    headers = [idx_header]
    for col_name in cols:
        col = table.columns.get(col_name)
        headers.append(_col_header(col_name, col, style))

    # Data rows
    rows = []
    for idx in table.index_values:
        row_cells = [f"${idx}$" if math else str(idx)]
        row = table(idx)
        for col_name in cols:
            try:
                cell = row[col_name]
                row_cells.append(format_property(cell, style, unit=False))
            except (KeyError, AttributeError):
                row_cells.append("N/A")
        rows.append(row_cells)

    return markdown_table(rows, headers, align or ["c"] * len(headers))


# ════════════════════════════════════════════════════════════
# Heading helper
# ════════════════════════════════════════════════════════════

def heading(text: str, level: int = 2) -> str:
    """Generate a markdown heading."""
    return f"{'#' * level} {text}\n"
