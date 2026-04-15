"""Sample — container for scientific properties with structured I/O."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml

from .property import Property
from .table import Table


# ════════════════════════════════════════════════════════════
# YAML configuration — compact leaf dicts, block nested dicts
# ════════════════════════════════════════════════════════════

class _RootDict(dict):
    """Marker so the dumper always uses block style for the root mapping."""
    pass


class _Dumper(yaml.SafeDumper):
    pass


def _is_leaf(v: Any) -> bool:
    if isinstance(v, (int, float, str, bool)) or v is None:
        return True
    return isinstance(v, list) and all(
        isinstance(x, (int, float, str, bool)) or x is None for x in v
    )


_Dumper.add_representer(_RootDict, lambda d, data:
    d.represent_mapping('tag:yaml.org,2002:map', data, flow_style=False))

_Dumper.add_representer(dict, lambda d, data:
    d.represent_mapping('tag:yaml.org,2002:map', data,
                        flow_style=bool(data and all(_is_leaf(v) for v in data.values()))))

def _repr_float(d: yaml.Dumper, v: float) -> yaml.Node:
    if v != v:     return d.represent_scalar('tag:yaml.org,2002:float', '.nan')
    if v ==  float('inf'):  return d.represent_scalar('tag:yaml.org,2002:float', '.inf')
    if v == -float('inf'):  return d.represent_scalar('tag:yaml.org,2002:float', '-.inf')
    if v == int(v):
        return d.represent_int(int(v))
    s = f"{v:.6e}" if (v != 0 and (abs(v) < 0.001 or abs(v) >= 1e7)) else f"{v:.10g}"
    return d.represent_scalar('tag:yaml.org,2002:float', s)

_Dumper.add_representer(float, _repr_float)


def _parse_frontmatter(content: str) -> tuple[dict, str]:
    """Split ``---`` YAML frontmatter and Markdown body."""
    if not content.startswith("---"):
        return {}, content
    end = content.find("\n---", 3)
    if end == -1:
        return {}, content
    yaml_data = yaml.safe_load(content[4:end]) or {}
    body = content[end + 4:].lstrip("\n")
    return yaml_data, body


class Sample:
    """
    Named container of Properties and Tables.

    Subclass and define properties in __init__. Properties and Tables
    are auto-registered when assigned as instance attributes.

    Examples
    --------
    >>> class MySample(Sample):
    ...     def __init__(self, name=None, filepath=None):
    ...         super().__init__(name, filepath)
    ...         self.temperature = Property(value=25.0, unit="°C", symbol_math="T")
    ...         self.pressure = Property(value=101.3, unit="kPa", symbol_math="P")
    ...
    ...     def template(self, style="math"):
    ...         from .report import properties_table
    ...         return properties_table(self, ["temperature", "pressure"], style=style)
    >>>
    >>> sample = MySample("EXP_001")
    >>> sample.save("sample.md")
    >>> loaded = MySample.load("sample.md")
    """

    def __init__(self, name: str | None = None, filepath: str | Path | None = None):
        # Use object.__setattr__ to bypass our custom __setattr__
        object.__setattr__(self, '_props', {})
        object.__setattr__(self, '_tables', {})
        object.__setattr__(self, '_order', [])
        fp = Path(filepath) if filepath else None
        object.__setattr__(self, '_filepath', fp)
        n = name if name is not None else (fp.stem if fp else "Unnamed")
        object.__setattr__(self, 'name', n)
        object.__setattr__(self, '_hydrating', False)
        if type(self) is Sample:
            self._auto_hydrate()

    def _auto_hydrate(self):
        """Load data from filepath if the file exists."""
        fp = object.__getattribute__(self, '_filepath')
        if fp is not None and fp.exists():
            object.__setattr__(self, '_hydrating', True)
            try:
                self._hydrate_from_file(fp)
            finally:
                object.__setattr__(self, '_hydrating', False)

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        original_init = cls.__init__

        def _wrapped_init(self, *args, **kw):
            original_init(self, *args, **kw)
            if type(self) is cls and not object.__getattribute__(self, '_hydrating'):
                self._auto_hydrate()

        cls.__init__ = _wrapped_init

    # ── Auto-registration ───────────────────────────────

    def __setattr__(self, name: str, value):
        object.__setattr__(self, name, value)
        if name.startswith('_') or name == 'name':
            return
        if isinstance(value, Property):
            props = object.__getattribute__(self, '_props')
            order = object.__getattribute__(self, '_order')
            props[name] = value
            value._name = name
            value._parent = self
            if value.symbol is None:
                value.symbol = name
            if value.symbol_math is None:
                value.symbol_math = value.symbol
            if name not in order:
                order.append(name)
            value._wire_dependencies()
        elif isinstance(value, Table):
            tables = object.__getattribute__(self, '_tables')
            order = object.__getattribute__(self, '_order')
            tables[name] = value
            value._name = name
            value._parent = self
            if name not in order:
                order.append(name)

    # ── Access ──────────────────────────────────────────

    @property
    def props(self) -> dict[str, Property]:
        """All registered Properties (ordered)."""
        return dict(object.__getattribute__(self, '_props'))

    @property
    def tables(self) -> dict[str, Table]:
        """All registered Tables (ordered)."""
        return dict(object.__getattribute__(self, '_tables'))

    def __getitem__(self, key: str) -> Property | Table:
        """Access property or table by name."""
        props = object.__getattribute__(self, '_props')
        if key in props:
            return props[key]
        tables = object.__getattribute__(self, '_tables')
        if key in tables:
            return tables[key]
        raise KeyError(key)

    def __contains__(self, key: str) -> bool:
        props = object.__getattribute__(self, '_props')
        tables = object.__getattribute__(self, '_tables')
        return key in props or key in tables

    # ── Template ────────────────────────────────────────

    def template(self, style: str = "math") -> str:
        """Override in subclass for custom markdown layout.

        Parameters
        ----------
        style : "math" or "text"
        """
        return ""

    # ── I/O ─────────────────────────────────────────────

    def _build_yaml_data(self) -> dict:
        """Build the complete YAML dict for this sample."""
        data: dict[str, Any] = _RootDict(name=self.name)

        props = object.__getattribute__(self, '_props')
        tables = object.__getattribute__(self, '_tables')
        order = object.__getattribute__(self, '_order')

        for key in order:
            if key in props:
                serialized = props[key].to_yaml()
                if serialized is not None:
                    data[key] = serialized
            elif key in tables:
                serialized = tables[key].to_yaml()
                if serialized:
                    data[key] = serialized

        return data

    def _hydrate_from_yaml(self, yaml_data: dict):
        """Populate registered Properties and Tables from YAML data."""
        props = object.__getattribute__(self, '_props')
        tables = object.__getattribute__(self, '_tables')

        for key, raw in yaml_data.items():
            if key in props:
                props[key].from_yaml(raw)
            elif key in tables and isinstance(raw, dict):
                tables[key].from_yaml(raw)
            elif isinstance(raw, dict) and "_rows" in raw:
                # Detected an unregistered table → create dynamically
                new_table = Table()
                new_table.from_yaml(raw)
                setattr(self, key, new_table)
            else:
                # Unknown key → create a dynamic Property
                new_prop = Property()
                new_prop.from_yaml(raw)
                setattr(self, key, new_prop)

    def _hydrate_from_file(self, filepath: Path):
        """Read and hydrate from a Markdown file."""
        content = filepath.read_text(encoding="utf-8")
        yaml_data, _body = _parse_frontmatter(content)
        name = yaml_data.pop("name", None)
        if name is not None:
            object.__setattr__(self, 'name', name)
        self._hydrate_from_yaml(yaml_data)

    def save(self, filepath: str | Path | None = None, style: str = "math") -> Path:
        """Save to markdown file with YAML frontmatter.

        Parameters
        ----------
        filepath : path, optional
            Defaults to the filepath used at construction.
        style : "math" or "text"
            Controls math rendering in the body.
        """
        fp = Path(filepath) if filepath else self._filepath
        if fp is None:
            raise ValueError("No filepath specified")

        yaml_data = self._build_yaml_data()
        yaml_str = yaml.dump(
            yaml_data,
            Dumper=_Dumper,
            sort_keys=False,
            allow_unicode=True,
            default_flow_style=False,
        )

        body = self.template(style=style)

        content = f"---\n{yaml_str}---\n"
        if body:
            content += f"\n{body}\n"

        fp.parent.mkdir(parents=True, exist_ok=True)
        fp.write_text(content, encoding="utf-8")
        self._filepath = fp
        return fp

    @classmethod
    def load(cls, filepath: str | Path) -> Sample:
        """Load from a markdown file with YAML frontmatter.

        Creates an instance of *cls* (calling its __init__ to define
        properties), then hydrates from the YAML data via _auto_hydrate.
        """
        fp = Path(filepath)
        # _auto_hydrate (called by __init_subclass__ wrapper) will read
        # the file and hydrate since fp exists on disk.
        return cls(filepath=fp)

    # ── Converters (delegated to samplekit.converters) ──

    def __getattr__(self, name: str):
        if name.startswith('_'):
            raise AttributeError(name)
        from . import converters
        fn = getattr(converters, f"sample_{name}", None)
        if fn is not None:
            return lambda *args, **kw: fn(self, *args, **kw)
        raise AttributeError(f"'{type(self).__name__}' has no attribute {name!r}")

    # ── Display ─────────────────────────────────────────

    def __str__(self):
        lines = [f"Sample: {self.name}"]
        props = object.__getattribute__(self, '_props')
        tables = object.__getattribute__(self, '_tables')
        order = object.__getattribute__(self, '_order')

        for key in order:
            if key in props:
                prop = props[key]
                if prop.value is not None:
                    lines.append(f"  {key}: {prop.text}")
            elif key in tables:
                table = tables[key]
                lines.append(f"  {key}: {len(table)} rows × {len(table.columns)} columns")

        return "\n".join(lines)

    def __repr__(self):
        props = object.__getattribute__(self, '_props')
        tables = object.__getattribute__(self, '_tables')
        return (f"<Sample '{self.name}' with "
                f"{len(props)} properties, {len(tables)} tables>")
