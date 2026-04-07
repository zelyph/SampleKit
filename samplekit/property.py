"""Property — a scalar scientific quantity: value ± uncertainty [unit]."""

from __future__ import annotations

import statistics
from typing import Any, Callable, TYPE_CHECKING

if TYPE_CHECKING:
    from .sample import Sample


class Property:
    """
    Scientific property with value, uncertainty, unit, and optional computation.

    Modes
    -----
    - Static:   Property(value=25.0)
    - Measured:  Property(value=[25.1, 24.9, 25.0])     → auto mean ± std
    - Computed:  Property(compute=self._calc_rho)         → lazy, cached, invalidated

    Parameters
    ----------
    value : float, str, list[float], or None
        Static value or list of measurements (auto mean ± std).
    uncertainty : float or None
        Static uncertainty (overrides auto-std when value is a list).
    unit : str
        Plain-text unit (e.g. "°C", "kPa").
    unit_math : str, optional
        Math-mode unit for MathJax/LaTeX (defaults to unit).
    symbol : str, optional
        Text/unicode symbol for CLI/TUI display (defaults to name).
    symbol_math : str, optional
        Math-mode symbol for MathJax/LaTeX (defaults to symbol).
    precision : str
        Format spec for value (default "").
    precision_unc : str, optional
        Format spec for uncertainty (defaults to precision).
    compute : callable, optional
        Function() → float. Called lazily when value is accessed.
    compute_unc : callable, optional
        Function() → float. Called lazily when uncertainty is accessed.
    depends_on : list[Property], optional
        Properties that this one depends on. When they change,
        this property's cache is invalidated.
    """

    def __init__(
        self,
        value: float | str | list[float] | None = None,
        uncertainty: float | None = None,
        unit: str = "",
        unit_math: str | None = None,
        symbol: str | None = None,
        symbol_math: str | None = None,
        precision: str = "",
        precision_unc: str | None = None,
        compute: Callable[[], Any] | None = None,
        compute_unc: Callable[[], float] | None = None,
        depends_on: list[Property] | None = None,
    ):
        # Identity (set by Sample.__setattr__)
        self._name: str | None = None
        self._parent: Sample | None = None

        # Display
        self.symbol = symbol
        self.symbol_math = symbol_math
        self.unit = unit
        self.unit_math = unit_math or unit
        self.precision = precision
        self.precision_unc = precision_unc or precision

        # Dependencies
        self._depends_on_refs: list[Property] = depends_on or []
        self._dependents: list[Property] = []

        # Value backends
        self._compute = compute
        self._compute_unc = compute_unc
        self._data: list[float] | None = None
        self._static_value: float | str | None = None
        self._static_uncertainty: float | None = None

        # Cache (for computed properties)
        self._value_cached: Any = None
        self._unc_cached: float | None = None
        self._value_cache_valid = False
        self._unc_cache_valid = False

        # Initialize from constructor args
        if compute is None:
            if isinstance(value, list):
                self._data = list(value)
            else:
                self._static_value = value

        if compute_unc is None:
            self._static_uncertainty = uncertainty

    # ── Name ─────────────────────────────────────────────

    @property
    def name(self) -> str | None:
        return self._name

    # ── Value ────────────────────────────────────────────

    @property
    def value(self) -> float | str | None:
        if self._compute is not None:
            if not self._value_cache_valid:
                self._value_cached = self._compute()
                self._value_cache_valid = True
            return self._value_cached
        if self._data is not None:
            return statistics.mean(self._data) if self._data else None
        return self._static_value

    @value.setter
    def value(self, val):
        """Set value directly (clears compute if any)."""
        self._compute = None
        if isinstance(val, list):
            self._data = list(val)
            self._static_value = None
        else:
            self._data = None
            self._static_value = val
        self._value_cache_valid = False
        self._invalidate_dependents()

    # ── Uncertainty ──────────────────────────────────────

    @property
    def uncertainty(self) -> float | None:
        if self._compute_unc is not None:
            if not self._unc_cache_valid:
                self._unc_cached = self._compute_unc()
                self._unc_cache_valid = True
            return self._unc_cached
        if self._static_uncertainty is not None:
            return self._static_uncertainty
        # Auto std from measurement list
        if self._data is not None and len(self._data) > 1:
            return statistics.stdev(self._data)
        return None

    @uncertainty.setter
    def uncertainty(self, val):
        self._compute_unc = None
        self._static_uncertainty = val
        self._unc_cache_valid = False
        self._invalidate_dependents()

    # ── Data (raw measurements) ──────────────────────────

    @property
    def data(self) -> list[float] | None:
        return list(self._data) if self._data is not None else None

    @property
    def is_computed(self) -> bool:
        return self._compute is not None

    # ── Cache management ────────────────────────────────

    def invalidate(self):
        """Manually invalidate this property's cache and propagate."""
        self._value_cache_valid = False
        self._unc_cache_valid = False
        self._invalidate_dependents()

    def _invalidate_dependents(self, _seen: set[int] | None = None):
        if _seen is None:
            _seen = set()
        for dep in self._dependents:
            dep_id = id(dep)
            if dep_id not in _seen:
                _seen.add(dep_id)
                dep._value_cache_valid = False
                dep._unc_cache_valid = False
                dep._invalidate_dependents(_seen)

    def _wire_dependencies(self):
        """Register self as a dependent of each dependency."""
        for dep in self._depends_on_refs:
            if isinstance(dep, Property) and self not in dep._dependents:
                dep._dependents.append(self)

    def _seed_cache(self, value=None, uncertainty=None):
        """Seed the cache for a computed property (used during hydration)."""
        if value is not None:
            self._value_cached = value
            self._value_cache_valid = True
        if uncertainty is not None:
            self._unc_cached = uncertainty
            self._unc_cache_valid = True

    # ── Display ─────────────────────────────────────────

    @property
    def text(self) -> str:
        return self.format()

    def format(self, unit: bool = True) -> str:
        """Format as plain text.

        Parameters
        ----------
        unit : bool
            Include unit in output.
        """
        v = self.value
        if v is None:
            return "N/A"

        # String values (e.g. material name)
        if isinstance(v, str):
            if unit and self.unit and self.unit != "-":
                return f"{v} {self.unit}"
            return v

        u = self.uncertainty
        v_str = f"{v:{self.precision}}"

        if u is not None and u != 0:
            u_str = f"{u:{self.precision_unc}}"
            val_part = f"{v_str} ± {u_str}"
        else:
            val_part = v_str
        if unit and self.unit and self.unit != "-":
            return f"{val_part} {self.unit}"
        return val_part

    def __repr__(self):
        return f"Property({self._name}: {self.text})"

    def __str__(self):
        return self.text

    # ── Serialization ───────────────────────────────────

    def to_yaml(self) -> Any:
        """Convert to a YAML-friendly value (scalar, dict, or None)."""
        v = self.value
        u = self.uncertainty
        unit = self.unit
        data = self.data

        # String with no extra metadata → bare string
        if isinstance(v, str) and u is None and not unit:
            return v

        d: dict[str, Any] = {}
        if v is not None:
            d["value"] = v
        if data is not None:
            d["data"] = data
        if u is not None:
            d["uncertainty"] = u
        if unit:
            d["unit"] = unit
        if self.unit_math and self.unit_math != unit:
            d["unit_math"] = self.unit_math
        sym = self.symbol
        if sym and sym != self._name:
            d["symbol"] = sym
        if self.symbol_math and self.symbol_math != (sym or self._name):
            d["symbol_math"] = self.symbol_math
        if self.precision:
            d["precision"] = self.precision
        if self.precision_unc and self.precision_unc != self.precision:
            d["precision_unc"] = self.precision_unc

        if not d:
            return None

        # Only metadata and no actual data → skip
        data_keys = {"value", "data", "uncertainty"}
        if not (set(d.keys()) & data_keys):
            return None

        # Only value and it's numeric → bare scalar
        if list(d.keys()) == ["value"] and isinstance(v, (int, float)):
            return v

        return d

    def from_yaml(self, raw: Any):
        """Populate this Property from a YAML value (scalar or dict)."""
        if isinstance(raw, dict):
            data = {
                "value": raw.get("value"),
                "uncertainty": raw.get("uncertainty"),
                "unit": raw.get("unit"),
                "data": raw.get("data"),
            }
            # Metadata fields
            if raw.get("unit_math") is not None:
                self.unit_math = raw["unit_math"]
            if raw.get("symbol") is not None:
                self.symbol = raw["symbol"]
            if raw.get("symbol_math") is not None:
                self.symbol_math = raw["symbol_math"]
            if raw.get("precision") is not None:
                self.precision = raw["precision"]
            if raw.get("precision_unc") is not None:
                self.precision_unc = raw["precision_unc"]
        else:
            data = {"value": raw}

        # Data list takes priority (value/uncertainty will be computed from it)
        if data.get("data") is not None:
            self.value = data["data"]
        elif data.get("value") is not None:
            if self.is_computed:
                self._seed_cache(value=data["value"])
            else:
                self.value = data["value"]

        if data.get("uncertainty") is not None:
            if self._compute_unc is not None:
                self._seed_cache(uncertainty=data["uncertainty"])
            else:
                self.uncertainty = data["uncertainty"]

        if data.get("unit") is not None:
            self.unit = data["unit"]
            if self.unit_math == self.unit or not self.unit_math:
                self.unit_math = data["unit"]
