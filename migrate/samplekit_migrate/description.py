"""What a project means, in no version's format.

Each version's reader fills these; the writer writes them. Nothing here names
a key of a file: a reader says what a value is, entered, measured several
times or computed, and the writer chooses how this version writes it.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import PurePosixPath
from typing import Any

ENTERED = "entered"
READINGS = "readings"
COMPUTED = "computed"


@dataclass
class Quantity:
    """A value of a sample or a cell of a table, and where it came from."""

    value: Any = None
    uncertainty: float | None = None
    unit: str | None = None
    readings: list[float] | None = None
    origin: str = ENTERED
    # An uncertainty typed by hand over readings, where the earlier version
    # let it stand for their spread.
    typed_uncertainty: bool = False


@dataclass
class Table:
    """A table of a sample: its index, its columns' units, its rows."""

    index: list[str]
    columns: dict[str, str | None]
    rows: list[dict[str, Quantity]]
    title: str | None = None


@dataclass
class Sample:
    """One sample: where its file goes, and what it holds."""

    path: PurePosixPath
    name: str | None
    attributes: dict[str, Any] = field(default_factory=dict)
    properties: dict[str, Quantity] = field(default_factory=dict)
    tables: dict[str, Table] = field(default_factory=dict)
    # The text below the frontmatter, carried as it is.
    note: str = ""


@dataclass
class Shown:
    """How a property, or a table's column, is shown: what an earlier version
    wrote in every file, and this one declares once."""

    unit: str | None = None
    symbol: str | None = None
    symbol_math: str | None = None
    precision: str | None = None
    precision_uncertainty: str | None = None


@dataclass
class Formula:
    """A formula of the model, as the model's source writes it."""

    # The callable, as written in the source: `self._area`.
    source: str
    # What it reads, by this version's names; None where the earlier version
    # named nothing and nothing was found.
    depends_on: list[str] | None = None
    # Found in the formula's body rather than declared: to check.
    inferred: bool = False


@dataclass
class Declaration:
    """A property, or a table's column, as the model declares it."""

    name: str
    unit: str | None = None
    value: Formula | None = None
    uncertainty: Formula | None = None
    # A formula given the row, for a table's column.
    row: bool = False
    # A formula given the columns whole, for a table's column.
    column: bool = False
    # The statistic the earlier version took of readings, implicitly.
    readings_statistic: bool = False


@dataclass
class TableDeclaration:
    name: str
    index: list[str]
    columns: dict[str, Declaration]
    title: str | None = None


@dataclass
class Model:
    """The model: its file, its class, and what its __init__ declares."""

    path: PurePosixPath
    class_name: str
    properties: dict[str, Declaration] = field(default_factory=dict)
    tables: dict[str, TableDeclaration] = field(default_factory=dict)
    # The methods copied as they are, to check by hand.
    copied: list[str] = field(default_factory=list)
    # The model's file, rewritten.
    text: str = ""


@dataclass
class Project:
    """A whole project, as the reader understood it."""

    version: str
    samples: list[Sample] = field(default_factory=list)
    shown: dict[str, Shown] = field(default_factory=dict)
    # A unit's math form, by its spelling.
    unit_math: dict[str, str] = field(default_factory=dict)
    model: Model | None = None
    # The configuration of a 0.x project, made for it.
    configuration: str = ""
    # Files of the project written again: rc.1's configurations and models.
    rewritten: dict[PurePosixPath, str] = field(default_factory=dict)
    # Files copied as they are, by their path in the project.
    copied: list[PurePosixPath] = field(default_factory=list)
    # Sample files copied as they are, this version reading them already.
    samples_as_they_are: list[PurePosixPath] = field(default_factory=list)
    # What the report says, by heading.
    said: dict[str, list[str]] = field(default_factory=dict)

    def say(self, heading: str, line: str) -> None:
        lines = self.said.setdefault(heading, [])
        if line not in lines:
            lines.append(line)
