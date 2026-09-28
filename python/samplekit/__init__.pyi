"""The machine-readable contract of samplekit.

Checked against the runtime surface in both directions by the package's tests:
every public runtime attribute is declared here, and every declaration exists.
"""

from collections.abc import Callable, ItemsView, Iterable, Iterator, KeysView, Mapping, Sequence
from datetime import date, datetime
from os import PathLike
from pathlib import Path
from typing import Any, Generic, Literal, Self, TypeAlias, TypeVar, overload

_T = TypeVar("_T")

# -------------------------------------------------------------- aliases

Scalar: TypeAlias = bool | int | float | str | date | datetime | None
AttributeScalar: TypeAlias = bool | int | float | str | date | datetime
AttributeList: TypeAlias = (
    BoundList[bool] | BoundList[int] | BoundList[float]
    | BoundList[str] | BoundList[date] | BoundList[datetime]
)
Attribute: TypeAlias = Scalar | AttributeList
IndexScalar: TypeAlias = bool | int | float | str | date | datetime
Index: TypeAlias = IndexScalar | tuple[IndexScalar, ...]
CellInput: TypeAlias = Scalar | Sequence[float] | Property
Pathish: TypeAlias = str | PathLike[str]
Precision: TypeAlias = str | tuple[str, str]
Compute: TypeAlias = Callable[[], Scalar]
UncertaintyCompute: TypeAlias = Callable[[], float | None]
QuantityCompute: TypeAlias = Callable[[], tuple[Scalar, float | None]]
RowCompute: TypeAlias = Callable[[RowView], CellInput | Mapping[str, CellInput]]
ColumnCompute: TypeAlias = Callable[
    [Mapping[str, ColumnView]],
    Sequence[CellInput] | Mapping[str, Sequence[CellInput]],
]
OutputNames: TypeAlias = str | Sequence[str]
RowDerivation: TypeAlias = tuple[OutputNames, Sequence[str], RowCompute]
ColumnDerivation: TypeAlias = tuple[OutputNames, Sequence[str], ColumnCompute]
FieldKey: TypeAlias = str | Field
# What a formula reads: names, or by channel — {"v": [...], "u": [...]}.
Declared: TypeAlias = Sequence[str] | Mapping[str, Sequence[str]]
# One end of an axis: a number, or free — None, or "auto" as .samplekitrc writes it.
AxisBound: TypeAlias = float | Literal["auto"] | None
# What a column's cells hold, read as a list: numbers, text or dates, as its
# rows hold them, which a checker cannot tell from the column's name. Typed
# as `Scalar`, a model's arithmetic over a column — a sum, a slope, a mean —
# is refused; the code reading the list knows what its column holds.
CellValue: TypeAlias = Any

# -------------------------------------------------------------- data


__version__: str

class BoundList(list[_T]):
    """A list bound to a sample: its changes are the sample's.

    A sample's tags, a list attribute and a property's readings are read as
    one. ``append``, ``remove``, an item assigned and every other change is
    checked and made in the sample too, and a refused item leaves both
    unchanged. ``list(...)`` or ``.copy()`` is a plain copy, bound to
    nothing, and so is a list read before the attribute was assigned again.

    Example:
        >>> ipa.tags.append("medal")
    """


class Property:
    """A quantity: a value, its uncertainty and its unit.

    A property stands for its value in arithmetic and comparisons, so
    ``ipa.abv * 2`` and ``ipa.abv > 6`` work on the number, and it prints as
    the whole quantity, ``6.8 ± 0.1 %``. A model declares its properties in its
    ``__init__``; a property a model does not declare is made the same way and
    assigned to the sample.

    A property is one of four kinds, by what is given: a value entered by hand;
    readings, whose value and uncertainty a statistic gives; a value a formula
    computes (``compute``, ``compute_uncertainty`` or ``compute_quantity``); or
    nothing yet.

    Args:
        value: The value — a number, text, a boolean, a date, ``samplekit.NA``
            — or a list of readings, or a statistic from ``samplekit.stats``
            that gives the value from the readings.
        uncertainty: The absolute standard uncertainty, in the value's unit, or
            a spread from ``samplekit.stats``.
        unit: The unit, as the file writes it: ``"g"``, ``"degC"``.
        symbol: Not taken here: a symbol is declared in ``.samplekitrc``,
            under ``[property.NAME]``.
        precision: Not taken here: a precision is declared in ``.samplekitrc``,
            under ``[property.NAME]``.
        compute: A function of no argument returning the value.
        compute_uncertainty: A function of no argument returning the
            uncertainty, beside a value entered or computed apart.
        compute_quantity: A function of no argument returning
            ``(value, uncertainty)`` from one run.
        depends_on: What the formula reads: a list of names — ``["og", "fg"]``,
            ``"fermentation.gravity"`` for a column — or, where the value and
            the uncertainty have a formula each, ``{"v": [...], "u": [...]}``.
            ``[]`` says it reads nothing of the sample. It can also be declared
            later with ``Sample.set_dependencies``.

    Raises:
        ValueError: Two arguments give the same thing — ``value`` and a
            formula, ``compute`` and ``compute_quantity`` — or a value is not a
            number where a unit is given.
        TypeError: A formula is not callable, or a symbol or a precision is
            given.

    Example:
        >>> import samplekit as sk
        >>> sk.Property(3.2, 0.1, unit="kg")
        Property(value=3.2, uncertainty=0.1, unit='kg')
        >>> og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
    """

    def __init__(
        self,
        value: Scalar | Sequence[float] | Property | Statistic = None,
        uncertainty: float | Statistic | None = None,
        unit: str | None = None,
        compute: Compute | None = None,
        compute_uncertainty: UncertaintyCompute | None = None,
        compute_quantity: QuantityCompute | None = None,
        # What this formula reads, so that a change to one of them makes it
        # outdated. Optional here rather than required: a model may declare the
        # same thing with `Sample.set_dependencies`, and a checker cannot see
        # that from the call. A computed property that declares neither is refused once the
        # model is built — `depends_on=[]` says it reads nothing.
        depends_on: Declared | None = None,
    ) -> None: ...
    # A quantity's value is read as a number, so that a model's arithmetic
    # type-checks: a formula runs only once its inputs hold one, and waits
    # otherwise. Outside a formula it may hold text, a date, NA or nothing,
    # which the code reading it checks. Assigned, it takes all of them, and
    # a list of readings.
    @property
    def value(self) -> float:
        """The value: a number, text, a boolean, a date, ``samplekit.NA``, or
        ``None`` when there is none.

        Assigning a value to a computed property overrides its formula: the
        property reads *edited* until ``compute(force=True)`` gives it back to the
        formula. Assigning a list gives the property readings. A property with a
        unit takes numbers only, and raises ``ValueError`` otherwise.
        """
    @value.setter
    def value(self, value: Scalar | NotApplicable | Sequence[float] | Property) -> None: ...
    @property
    def v(self) -> float:
        """The value: the same as ``value``, spelt as a field path spells it
        (``abv.v``).
        """
    @v.setter
    def v(self, value: Scalar | NotApplicable | Sequence[float] | Property) -> None: ...
    uncertainty: float | None
    """The absolute standard uncertainty, in the value's unit, or ``None``."""
    u: float | None
    """The uncertainty: the same as ``uncertainty``, spelt as a field path spells
    it (``abv.u``).
    """
    readings: list[float] | None
    """The readings, or ``None`` for a value not measured several times.

    The list is bound to the property: ``append``, ``remove`` or an item
    assigned changes the property, as assigning a whole list does. Readings are
    refused on a value a formula computes.
    """
    @property
    def stats(self) -> Summary | None:
        """The statistics of the readings, as a ``Summary``, or ``None`` without
        readings.
        """
    unit: str | None
    """The unit, as the file writes it — ``"g"``, ``"degC"`` — or ``None``."""
    @property
    def symbol(self) -> str | None:
        """The quantity's symbol: the one this sample's file writes, else the one
        the project declares in ``.samplekitrc`` under ``[property.NAME]``, else
        ``None``. Read only: a symbol is the project's, declared there, never
        set by a model or a script.
        """
    @property
    def text(self) -> str:
        """The quantity as a terminal shows it — value, uncertainty and unit, at the
        declared precision: what ``str()`` gives.
        """
    @property
    def is_computed(self) -> bool:
        """Whether a formula gives the value."""
    def invalidate(self) -> None:
        """Discard the computed value.

        The value reads *never computed* (a statistic of readings, *outdated*), and
        every value computed from it reads *outdated*, until ``compute()`` runs
        them again.
        """
    @property
    def is_outdated(self) -> bool:
        """Whether the value is outdated: an input changed since it was computed."""
    # `is_outdated` under its former name, still answered.
    @property
    def is_stale(self) -> bool:
        """The same as ``is_outdated``, under its former name."""
    @property
    def failure(self) -> str | None:
        """What the formula raised when it last ran, or ``None`` if it did not fail."""
    @property
    def is_edited(self) -> bool:
        """Whether the value is edited: typed over its formula, or a computed table
        cell whose record is missing.
        """
    @property
    def edited_upstream(self) -> list[str]:
        """The edited values this one was computed from, directly or further up."""
    @property
    def state(self) -> str:
        """Where the value stands, in one word.

        One of ``"entered"`` (typed in, no formula), ``"current"``,
        ``"outdated"`` (an input changed since it was computed),
        ``"never computed"``, ``"edited"`` (a value typed over its formula) or
        ``"failed"`` (its formula raised).
        """
    def compute(self, rerun: bool = False, force: bool = False) -> None:
        """Compute the value, after the inputs it needs.

        Only a value that is outdated or never computed runs, unless asked
        otherwise. The file is not written: ``save()`` does that.

        Args:
            rerun: Run it even when it is current.
            force: Run it even when it is edited, giving it back to its formula.

        Raises:
            ValueError: The value has no formula.
            Exception: Whatever the formula raised, with a note naming the value.
        """
    def format(self, unit: bool = True) -> str:
        """Format the quantity at its declared precision.

        Args:
            unit: Include the unit.

        Returns:
            The quantity as text, such as ``"6.8 ± 0.1 %"``. For another
            precision, use a format specifier: ``f"{ipa.abv:.2f}"``.
        """
    def __str__(self) -> str: ...
    def __format__(self, spec: str) -> str: ...
    def __repr__(self) -> str: ...
    def __float__(self) -> float: ...
    def __int__(self) -> int: ...
    def __bool__(self) -> bool: ...
    def __add__(self, other: Any) -> Any: ...
    def __radd__(self, other: Any) -> Any: ...
    def __sub__(self, other: Any) -> Any: ...
    def __rsub__(self, other: Any) -> Any: ...
    def __mul__(self, other: Any) -> Any: ...
    def __rmul__(self, other: Any) -> Any: ...
    def __truediv__(self, other: Any) -> Any: ...
    def __rtruediv__(self, other: Any) -> Any: ...
    def __floordiv__(self, other: Any) -> Any: ...
    def __rfloordiv__(self, other: Any) -> Any: ...
    def __mod__(self, other: Any) -> Any: ...
    def __rmod__(self, other: Any) -> Any: ...
    def __pow__(self, other: Any, modulo: Any = None) -> Any: ...
    def __rpow__(self, other: Any, modulo: Any = None) -> Any: ...
    def __neg__(self) -> Any: ...
    def __pos__(self) -> Any: ...
    def __abs__(self) -> Any: ...
    def __round__(self, ndigits: int | None = None) -> Any: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...
    def __lt__(self, other: Any) -> bool: ...
    def __le__(self, other: Any) -> bool: ...
    def __gt__(self, other: Any) -> bool: ...
    def __ge__(self, other: Any) -> bool: ...
    __hash__: None  # type: ignore[assignment]


class Column:
    """One column of a ``Table``: its unit, and the statistics of its cells'
    readings.

    Args:
        unit: The unit of every cell, as the file writes it.
        symbol: Not taken here: a symbol is declared in ``.samplekitrc``,
            under ``[property."TABLE.COLUMN"]``.
        precision: Not taken here: a precision is declared in ``.samplekitrc``,
            under ``[property.TABLE.COLUMN]``.
        value: A statistic from ``samplekit.stats`` that gives each cell's value
            from the readings it holds.
        uncertainty: A spread from ``samplekit.stats`` that gives each cell's
            uncertainty from its readings.

    Raises:
        TypeError: A symbol or a precision is given, or ``value`` or
            ``uncertainty`` is not a statistic.
        ValueError: A spread is given as the value.

    Example:
        >>> day = sk.Column(unit="d")
    """

    def __init__(
        self,
        unit: str | None = None,
        symbol: None = None,
        precision: None = None,
        value: Statistic | None = None,
        uncertainty: Statistic | None = None,
    ) -> None: ...
    @property
    def unit(self) -> str | None:
        """The unit of every cell, or ``None``."""


class RowView:
    """One row of a table, as a function in ``compute_rows`` receives it.

    A cell is read by its column's name, as an attribute or an item:
    ``row.gravity`` or ``row["gravity"]``, each a ``Property``.
    """

    @property
    def position(self) -> int:
        """The row's position in the table, from 0."""
    def __getitem__(self, column: str) -> Property: ...
    def __getattr__(self, column: str) -> Property: ...
    def __contains__(self, column: object) -> bool: ...
    def __iter__(self) -> Iterator[str]: ...
    def keys(self) -> KeysView[str]:
        """The names of the row's columns."""
    def items(self) -> ItemsView[str, Property]:
        """Each column name with its cell."""


class ColumnView:
    """One column of a table, as a function in ``compute_columns`` receives it,
    and as ``table.column_name`` reads it.

    Iterating gives each cell as a ``Property``, in row order, and an item is
    the cell of a row, by its index value: ``column[3]``.
    """

    @property
    def values(self) -> list[CellValue]:
        """The value of each cell, in row order; ``None`` where a cell has none."""
    @property
    def uncertainties(self) -> list[float | None]:
        """The uncertainty of each cell, in row order; ``None`` where a cell has none."""
    def __getitem__(self, index: Index) -> Property: ...
    def __iter__(self) -> Iterator[Property]: ...
    def __len__(self) -> int: ...


class Table:
    """A table of a sample: named columns, and rows identified by their index.

    The index is the column — or the columns — whose values identify a row: a
    day, a temperature, a taster. A cell is read by its index value, never by
    its position: ``table.at(3, "gravity")``.

    Some columns can be computed: ``compute_rows`` fills cells row by row, and
    ``compute_columns`` fills a whole column at once. Each entry is a triple
    ``(outputs, inputs, function)``: the column or columns it fills, what it
    reads (``"row.gravity"`` for a cell of the same row,
    ``"fermentation.gravity"`` for a whole column, ``"og"`` for a property of
    the sample), and the function. A row function takes a ``RowView`` and
    returns a cell, or a dict of cells by column; a column function takes a
    dict of ``ColumnView`` by name and returns a list of cells, one per row,
    or a dict of such lists.

    Args:
        columns: A dict of ``Column`` by name, in the order they are shown.
        index: The name of the index column, or a list of names.
        title: A title for the table.
        rows: The first rows: each a dict of cells by column name.
        compute_rows: The columns computed row by row.
        compute_columns: The columns computed whole.

    Raises:
        TypeError: A column is not a ``Column``.
        ValueError: The index names no column, or a row does not fit the
            columns.

    Example:
        >>> tasting = sk.Table(
        ...     {"taster": sk.Column(), "score": sk.Column()},
        ...     "taster",
        ...     rows=[{"taster": "Ana", "score": 41}],
        ... )
        >>> tasting.at("Ana", "score").value
        41
    """

    def __init__(
        self,
        columns: Mapping[str, Column],
        index: str | Sequence[str],
        *,
        title: str | None = None,
        rows: Iterable[Mapping[str, CellInput]] | None = None,
        compute_rows: Iterable[RowDerivation] | None = None,
        compute_columns: Iterable[ColumnDerivation] | None = None,
    ) -> None: ...
    def add(self, **cells: CellInput) -> None:
        """Add a row at the end.

        Args:
            **cells: The row's cells by column name: a value, a list of readings,
                or a ``Property``. The index columns are required.

        Raises:
            KeyError: A column the table does not have.
            ValueError: A row with that index exists already.
        """
    def extend(self, rows: Iterable[Mapping[str, CellInput]]) -> None:
        """Add several rows at the end.

        Args:
            rows: An iterable of dicts, each a row's cells by column name.

        Raises:
            KeyError: A column the table does not have.
            ValueError: A row with that index exists already.
        """
    def update(self, index: Index, **cells: CellInput) -> None:
        """Change cells of one row.

        Values computed from those cells become outdated. A computed cell changed
        here is edited: kept by ``compute()`` until ``compute(force=True)``.

        Args:
            index: The row's index value; a tuple for an index of several columns.
            **cells: The new cells, by column name.

        Raises:
            KeyError: No row has that index, or a column is unknown; the message
                names the nearest.
        """
    def at(self, index: Index, column: str) -> Property:
        """The cell of a row, by the row's index value.

        Args:
            index: The row's index value; a tuple for an index of several columns.
            column: The column's name.

        Returns:
            The cell, as a ``Property``.

        Raises:
            KeyError: No row has that index, or the column is unknown; the message
                names the nearest.

        Example:
            >>> ipa.fermentation.at(2, "gravity").value
            1.029
        """
    def values(self, column: str) -> list[CellValue]:
        """The values of a column, in row order.

        Args:
            column: The column's name.

        Returns:
            A list of values, ``None`` where a cell has none.

        Raises:
            KeyError: The column is unknown; the message lists the columns.

        Example:
            >>> ipa.fermentation.values("gravity")[:3]
            [1.041, 1.029, 1.021]
        """
    def uncertainties(self, column: str) -> list[float | None]:
        """The uncertainties of a column, in row order.

        Args:
            column: The column's name.

        Returns:
            A list of uncertainties, ``None`` where a cell has none.

        Raises:
            KeyError: The column is unknown; the message lists the columns.
        """
    @property
    def column_names(self) -> list[str]:
        """The names of the columns, in the order they were declared."""
    @property
    def index_values(self) -> list[Index]:
        """The index value of each row, in row order; a tuple each for an index of
        several columns.
        """
    @property
    def data_columns(self) -> list[str]:
        """The names of the columns that are not the index."""
    @property
    def index(self) -> str | list[str]:
        """The name of the index column, or a list of names for an index of several
        columns.
        """
    @property
    def index_unit(self) -> str | list[str | None] | None:
        """The unit of the index column, or a list of units for an index of several
        columns.
        """
    @property
    def title(self) -> str | None:
        """The table's title, or ``None``."""
    def __getitem__(self, position: int) -> RowView: ...
    def __getattr__(self, column: str) -> ColumnView: ...
    def __contains__(self, index: object) -> bool: ...
    def __iter__(self) -> Iterator[RowView]: ...
    def __len__(self) -> int: ...


class Sample:
    """One sample, read from and saved to a Markdown file.

    ``Sample(path)`` reads the file with the model its project declares;
    ``Sample(name=...)`` makes a sample with no file, which ``save(path)``
    gives one. A model is a subclass of ``Sample`` whose ``__init__``
    declares its properties and tables, and passes ``path`` on to
    ``super().__init__``.

    A property, a table or an attribute is read as an attribute or an item,
    ``brew.abv`` or ``brew["abv"]``, and any other field path as an item:
    ``brew["fermentation.gravity[2]"]`` is that cell's value. What is assigned
    decides what a name becomes: a ``Property`` or a ``Table`` is one; a
    ``bool``, ``int``, ``float``, ``str``, ``date``, ``datetime``, or a list
    of one of them, is an attribute; and a name with a leading ``_`` is
    ordinary Python state, never saved. **Nothing assigned without a leading
    ``_`` is lost on save**: it is saved, or refused.

    Args:
        path: The sample's file.
        name: The sample's name; a name the file holds wins over it.
        model: The class to read the file with, or ``False`` to read the
            data alone. By default, the class the project's ``[model]``
            declares.

    Raises:
        FileNotFoundError: The file does not exist; the message names the
            nearest. ``Sample.new`` makes a sample for a new file.
        IsADirectoryError: The path is a folder: ``sk.load`` reads a folder.
        ValueError: The file is not a sample.
        AttributeError: An unknown field, read as an attribute; the message
            names the nearest.
        KeyError: An unknown field, read as an item.
        TypeError: A value of a kind that cannot be saved; a leading ``_``
            keeps it in Python only.

    Example:
        >>> brew = sk.Sample("brews/citra-ipa.md")
        >>> brew.style
        'ipa'
        >>> brew["fermentation.gravity[2]"]
        1.029
    """

    def __init__(
        self,
        path: Pathish | None = None,
        name: str | None = None,
        model: type[Sample] | Literal[False] | None = None,
    ) -> None: ...
    @classmethod
    def new(
        cls,
        path: Pathish,
        name: str | None = None,
        model: type[Sample] | Literal[False] | None = None,
    ) -> Self:
        """Make a new sample for a file that does not exist yet.

        The sample is of the model the project of that folder declares — or of
        this class, when called on a model — and is named by the file, which
        writes no ``name:`` unless ``name`` is given.
        ``save()`` writes the file.

        Args:
            path: The file to create.
            name: The sample's name, instead of the file's.
            model: The class to make it with, or ``False`` for a plain
                ``Sample``.

        Returns:
            The new sample, not saved yet.

        Raises:
            FileExistsError: The file exists already: ``Sample(path)`` reads it.
            FileNotFoundError: The folder does not exist.

        Example:
            >>> brew = sk.Sample.new("brews/red-ale.md")
            >>> brew.style = "red_ale"
            >>> brew.save()
        """
    @property
    def path(self) -> Path | None:
        """The file the sample was read from or last saved to, or ``None`` for a
        sample never saved. ``save(path)`` changes it.
        """
    @property
    def filename(self) -> str | None:
        """The name of the sample's file, without its folder, or ``None``."""
    @property
    def project(self) -> Project:
        """The project the sample's file belongs to: what its ``.samplekitrc``
        declares.
        """
    name: str | None
    """The sample's name, or ``None``."""
    tags: BoundList[str]
    """The sample's tags, as a list whose changes are saved with the sample.

    Each tag is an identifier, and none appears twice.
    """
    note: str
    """The note: the Markdown text below the frontmatter, kept as written."""
    def save(self, path: Pathish | None = None, overwrite: bool = False) -> None:
        """Write the sample to its file.

        Nothing is overwritten unless asked: a file changed on disk since it was
        read, or an existing file at a new path, is refused. Saving runs no
        formula: a value never computed is written without a value.

        Args:
            path: Where to write it instead. The sample then belongs to that file:
                a later ``save()`` writes there.
            overwrite: Write even over a file changed since it was read, or over an
                existing file at ``path``.

        Raises:
            ValueError: The sample has no file yet and no ``path`` is given.
            FileExistsError: ``path`` exists already.
            FileNotFoundError: The folder does not exist, or the sample's file was
                removed or moved since it was read.
            IsADirectoryError: ``path`` is a folder.
            OSError: The file changed on disk since it was read.
            PermissionError: The file is read-only.

        Example:
            >>> ipa.volume.value = 21
            >>> ipa.save()
        """
    def files(self, pattern: str | None = None) -> list[Path]:
        """The sample's own files: images, reports, anything found by the sample's
        name in the folders ``[collection] files`` declares.

        Args:
            pattern: A glob on the files' names, such as ``"*.png"``.

        Returns:
            The files' paths.

        Raises:
            ValueError: The sample has no file, or belongs to no project.
        """
    def open(
        self, pattern: str | None = None, navigate: bool = False, many: bool = False
    ) -> list[Path]:
        """Open the sample's own files with the system's application.

        Args:
            pattern: A glob on the files' names, such as ``"*.pdf"``.
            navigate: Open the folders holding the files, in the file manager,
                instead of the files.
            many: Allow opening more than five.

        Returns:
            The paths opened.

        Raises:
            FileNotFoundError: No file of the sample matches.
            ValueError: More than five would open and ``many`` is false.
            RuntimeError: There is no display to open them on.
        """
    def set_dependencies(self, output: str, *, depends_on: Declared) -> None:
        """Declare what a formula reads.

        A change to one of the inputs makes the value outdated, and a computation
        runs the inputs first. A later declaration replaces an earlier one.

        Args:
            output: The value the formula gives: a property's name, or
                ``table.column``.
            depends_on: What it reads: a list of names, or ``{"v": [...],
                "u": [...]}`` where the value and the uncertainty have a formula
                each. ``[]`` says it reads nothing of the sample.

        Raises:
            KeyError: An unknown name, once the model is built.
            ValueError: The declaration makes a cycle, or names a channel with no
                formula.

        Example:
            >>> class Brew(sk.Sample):
            ...     def __init__(self, path=None, name=None):
            ...         super().__init__(path, name=name)
            ...         self.abv = sk.Property(unit="%", compute=self._abv)
            ...         self.set_dependencies("abv", depends_on=["og", "fg"])
            ...
            ...     def _abv(self):
            ...         return (self.og.value - self.fg.value) * 131.25
        """
    def dependencies(self, output: str) -> list[str]:
        """What a value's formula is declared to read.

        Args:
            output: A property's name, or ``table.column``.

        Returns:
            The input names, sorted.

        Raises:
            KeyError: An unknown name; the message names the nearest.

        Example:
            >>> ipa.dependencies("abv")
            ['fg', 'og']
        """
    def dependents(self, input: str) -> list[str]:
        """The values whose formula reads this one directly.

        Args:
            input: A property's name, or ``table.column``.

        Returns:
            Their names, sorted.

        Raises:
            KeyError: An unknown name; the message names the nearest.
        """
    def affected_by_change(self, input: str) -> list[str]:
        """Every value a change to this one would make outdated, directly or through
        other values.

        Args:
            input: A property's name, or ``table.column``.

        Returns:
            Their names, sorted.

        Raises:
            KeyError: An unknown name; the message names the nearest.

        Example:
            >>> ipa.affected_by_change("og")
            ['abv', 'attenuation', 'drop', 'efficiency', 'fermentation.apparent', 'fermentation.rate']
        """
    def compute(self, *names: str, rerun: bool = False, force: bool = False) -> None:
        """Compute the sample's values that are not current, in dependency order.

        Without names, every value a formula gives is considered; only those
        outdated or never computed run. A value whose inputs nobody entered waits,
        with a warning saying for what, and a formula that raises is recorded as
        failed while the others run. The file is not written: ``save()`` does
        that.

        Args:
            *names: The values to compute: properties, ``table.column``, or a
                table's name for all its computed columns. Their inputs run first
                when needed.
            rerun: Run them even when they are current.
            force: Run them even when they are edited, giving them back to their
                formula.

        Raises:
            KeyError: An unknown name; the message names the nearest.
            ValueError: A named value has no formula.
            Exception: The first formula that raised, with a note naming the value
                and the others that failed.

        Example:
            >>> ipa.compute()
            >>> ipa.save()
        """
    def outdated(self) -> list[str]:
        """The outdated values: those an input changed under since they were
        computed.

        Returns:
            The names of the outdated properties, and ``table.column`` for a
            column with an outdated cell.
        """
    # `outdated()` under its former name, still answered.
    def stale(self) -> list[str]:
        """The same as ``outdated()``, under its former name."""
    def edited(self) -> list[str]:
        """The edited values: those typed over their formula.

        Returns:
            The names of the edited properties, and ``table.column`` for a column
            with an edited cell.
        """
    def not_current(self) -> dict[str, str]:
        """Every value that is not current, and why: what ``samplekit status``
        reports.

        ``outdated()`` and ``edited()`` answer one question each; this one also
        sees values that failed, were never computed, or wait for an input.

        Returns:
            A dict from each name — a property, or ``table.column`` — to a reason:
            ``"outdated"``, ``"edited"``, ``"failed"``, ``"never computed"`` or
            ``"waits for fg"``.

        Example:
            >>> ipa.not_current()
            {}
        """
    def keys(self) -> KeysView[str]:
        """The names of the sample's attributes, properties and tables, in the order
        they were declared.
        """
    def items(self) -> ItemsView[str, Property | Table | Attribute]:
        """Each name with its attribute, property or table, in the order they were
        declared.
        """
    def __getitem__(self, name: str) -> Property | Table | Attribute | list[Scalar]: ...
    # A field's type is the data's, which no checker can know from the class.
    def __getattr__(self, name: str) -> Any: ...
    def __setattr__(self, name: str, value: Any) -> None: ...
    def __delattr__(self, name: str) -> None: ...
    def __setitem__(self, name: str, value: Property | Table | Attribute) -> None: ...
    def __delitem__(self, name: str) -> None: ...
    def __contains__(self, name: object) -> bool: ...
    def __iter__(self) -> Iterator[str]: ...


class SampleList(Sequence[Sample]):
    """An ordered list of samples, read like a Python list.

    A list is read from a folder, or built from samples already read. An item
    is a sample, by position or by name — ``brews["citra-ipa"]`` — and a slice
    is a list. Lists join with ``+``. ``filter``, ``query``, ``sorted`` and
    ``group_by`` make new lists; the samples in them are the same objects.

    Args:
        source: A folder, whose samples are read as the project declares, or
            an iterable of samples.
        pattern: A glob choosing the folder's files, instead of what
            ``[collection]`` declares.
        model: The model class to read the samples with, or ``False`` to read
            the data alone. By default, the project's model.

    Raises:
        TypeError: The iterable holds something other than samples, such as
            paths: ``sk.load`` reads paths.
        ValueError: A sample appears twice, or ``pattern`` or ``model`` is
            given without a folder.
        FileNotFoundError: The folder does not exist.

    Example:
        >>> brews = sk.SampleList("brews")
        >>> len(brews)
        12
        >>> brews["citra-ipa"].style
        'ipa'
    """

    def __init__(
        self,
        source: Pathish | Iterable[Sample] | None = None,
        pattern: str | None = None,
        model: type[Sample] | Literal[False] | None = None,
    ) -> None: ...
    def __len__(self) -> int: ...
    @overload
    def __getitem__(self, key: int) -> Sample: ...
    @overload
    def __getitem__(self, key: slice) -> SampleList: ...
    @overload
    def __getitem__(self, key: str) -> Sample: ...
    def __contains__(self, item: object) -> bool: ...
    def __iter__(self) -> Iterator[Sample]: ...
    def __reversed__(self) -> Iterator[Sample]: ...
    def __add__(self, other: SampleList) -> SampleList: ...
    def index(self, value: object, start: int = 0, stop: int = ...) -> int:
        """The position of a sample in the list.

        Args:
            value: A sample, or a sample's name.
            start: Where to start looking.
            stop: Where to stop looking.

        Raises:
            ValueError: It is not in the list.
        """
    def count(self, value: object) -> int:
        """How many times a sample is in the list: 0 or 1 for a sample, and, for a
        name, the number of samples of that name.
        """
    def filter(self, predicate: str | Callable[[Sample], bool]) -> SampleList:
        """The samples an expression selects, as ``-f`` selects them.

        Args:
            predicate: A filter expression — ``"abv > 6 && style == ipa"`` — or a
                function taking a sample and returning whether to keep it.

        Returns:
            A new list, in this list's order.

        Raises:
            KeyError: The expression names an unknown field; the message names
                the nearest.
            ValueError: The expression does not parse, or names a tag no sample
                carries.
            TypeError: The expression compares a field in a way its values do not
                allow.

        Example:
            >>> [brew.name for brew in brews.filter("abv > 6")]
            ['blonde-saison', 'citra-ipa', 'double-ipa', 'farmhouse-saison']
        """
    def sort(self, key: FieldKey | Sequence[FieldKey] | Callable[[Sample], Any]) -> None:
        """Sort the list in place, and return ``None``.

        Args:
            key: A field — ``"abv"``, or ``"-abv"`` for descending — several in a
                list or comma-separated, or a function of a sample as ``sorted``
                takes one.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            TypeError: A sort by ``state``, which a sample holds several of.
        """
    def sorted(self, key: FieldKey | Sequence[FieldKey] | Callable[[Sample], Any]) -> SampleList:
        """A new list in sorted order; this one is unchanged.

        Args:
            key: A field — ``"abv"``, or ``"-abv"`` for descending — several in a
                list or comma-separated, or a function of a sample as ``sorted``
                takes one.

        Returns:
            A new list.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            TypeError: A sort by ``state``, which a sample holds several of.

        Example:
            >>> brews.sorted("-abv")[0].name
            'double-ipa'
        """
    def query(self, name: str | Query) -> SampleList:
        """The samples a saved query selects, as ``-Q`` selects them.

        Args:
            name: The query's name in ``.samplekitrc``, or a ``Query``.

        Returns:
            A new list, in this list's order.

        Raises:
            KeyError: No query has that name; the message names the nearest.
            ValueError: The list spans several projects that declare it
                differently.

        Example:
            >>> len(brews.query("ipas"))
            3
        """
    def stats(self, field: FieldKey) -> Summary:
        """The statistics of one field over the list, as ``--summary`` gives them.

        Args:
            field: A field: ``"abv"``, ``"abv.u"``, ``"fermentation.gravity[1]"``.

        Returns:
            A ``Summary``. Its mean is weighted by 1/u² where every value has an
            uncertainty above zero.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            ValueError: No sample of the list holds a number there.

        Example:
            >>> round(brews.stats("abv").mean, 2)
            5.67
        """
    def group_by(self, fields: FieldKey | Sequence[FieldKey]) -> dict[Any, SampleList]:
        """Split the list into groups by the values of fields, as ``--group`` does.

        Args:
            fields: A field, or several — a list, or comma-separated as
                ``--group`` writes them.

        Returns:
            A dict from each value — a tuple of values for several fields — to
            the list of samples holding it. The groups come in the order their
            values sort; for a sorted list, in the order of their first samples.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            ValueError: The field is a whole table.
            RuntimeError: The field is ``state``, which a sample holds several of.

        Example:
            >>> list(brews.group_by("style"))
            ['ipa', 'pale_ale', 'porter', 'saison', 'stout', 'wheat']
        """
    def save_all(self, directory: Pathish, overwrite: bool = False) -> list[Path]:
        """Write every sample of the list into a folder.

        Each sample is written under its file's name, or its name. Nothing is
        written if any file is refused.

        Args:
            directory: The folder, which must exist.
            overwrite: Replace existing files.

        Returns:
            The paths written.

        Raises:
            FileNotFoundError: The folder does not exist.
            NotADirectoryError: It is a file.
            FileExistsError: A file exists and ``overwrite`` is false.
            ValueError: Two samples would be written to the same file.
        """
    def compute(self, *names: str, rerun: bool = False, force: bool = False) -> None:
        """Compute every sample of the list, as ``Sample.compute`` does.

        Every sample is computed even when one fails; the failures are raised at
        the end. A ``KeyboardInterrupt`` stops at once.

        Args:
            *names: The values to compute; every value that is not current by
                default.
            rerun: Run them even when they are current.
            force: Run them even when they are edited.

        Raises:
            Exception: The one failure, with a note naming the sample and the
                value.
            ExceptionGroup: Several failures, in the order they happened.
        """
    @property
    def fields(self) -> Names[Field]:
        """Every field the samples of the list can be addressed by, as
        ``samplekit list fields`` lists them.
        """
    @property
    def project(self) -> Project:
        """The project of the list's samples: what their ``.samplekitrc`` declares.

        Raises:
            ValueError: The samples belong to several projects.
        """
    def to_dict(
        self,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
    ) -> dict[str, list[Scalar | list[AttributeScalar]]]:
        """The list as a dict of columns, one list of values per column heading.

        Values are the stored numbers, unrounded.

        Args:
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.

        Returns:
            A dict from each heading to its values, one per sample.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.

        Example:
            >>> columns = brews.to_dict(columns="name,abv")
            >>> list(columns)
            ['name', 'abv_value', 'abv_uncertainty']
        """
    # Without a path the text is returned; with one it is written, and
    # nothing is returned.
    @overload
    def to_csv(
        self,
        path: None = None,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> str:
        """The list as CSV, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_csv(columns="name,abv")
        """
    @overload
    def to_csv(
        self,
        path: Pathish,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> None:
        """The list as CSV, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_csv(columns="name,abv")
        """
    @overload
    def to_tsv(
        self,
        path: None = None,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> str:
        """The list as TSV, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_tsv(columns="name,abv")
        """
    @overload
    def to_tsv(
        self,
        path: Pathish,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> None:
        """The list as TSV, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_tsv(columns="name,abv")
        """
    @overload
    def to_json(
        self,
        path: None = None,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> str:
        """The list as JSON, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_json(columns="name,abv")
        """
    @overload
    def to_json(
        self,
        path: Pathish,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> None:
        """The list as JSON, as the command line exports it.

        Args:
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: Neither ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> text = brews.to_json(columns="name,abv")
        """
    def export(
        self, export: str | Export, output: Pathish | None = None, overwrite: bool = False
    ) -> Path:
        """Write an export declared in ``.samplekitrc``, as ``samplekit export
        --write`` does.

        The export's own query, if it declares one, narrows the list first.

        Args:
            export: The export's name, or an ``Export``.
            output: Write here instead of the declared file.
            overwrite: Replace an existing file.

        Returns:
            The path written.

        Raises:
            KeyError: No export has that name, or its profile is not declared.
            FileExistsError: The file exists and ``overwrite`` is false.
            ValueError: ``output``'s extension does not match the export's format.

        Example:
            >>> brews.export("overview", output="out/overview.csv")
            PosixPath('out/overview.csv')
        """


class Summary:
    """The statistics of a set of values: readings, or a field over a list.

    Given by ``Property.stats`` and ``SampleList.stats``.
    """

    @property
    def count(self) -> int:
        """How many values were summarised."""
    @property
    def minimum(self) -> float:
        """The smallest value."""
    @property
    def maximum(self) -> float:
        """The largest value."""
    @property
    def mean(self) -> float:
        """The mean: weighted by 1/u² where every value has an uncertainty above zero,
        the arithmetic mean otherwise. ``weighted`` says which.
        """
    @property
    def weighted(self) -> bool:
        """Whether ``mean`` is weighted by 1/u², as the command line's summary heads
        it ``mean (1/u²)``.
        """
    @property
    def median(self) -> float:
        """The median."""
    @property
    def first_quartile(self) -> float:
        """The first quartile."""
    @property
    def third_quartile(self) -> float:
        """The third quartile."""
    @property
    def sample_stdev(self) -> float | None:
        """The sample standard deviation, dividing by n − 1; ``None`` for one value."""
    @property
    def population_stdev(self) -> float:
        """The population standard deviation, dividing by n."""
    @property
    def standard_error(self) -> float | None:
        """The standard error of the mean: the sample standard deviation over √n;
        ``None`` for one value.
        """


class Field:
    """A field of a list, as ``SampleList.fields`` gives it, and as ``-c`` and
    ``-s`` take it.

    ``str(field)`` is its path, and a field is accepted wherever a field's name
    is: ``brews.sorted(brews.fields.abv)``.
    """

    @property
    def path(self) -> str:
        """The field's path: ``"abv"``, ``"fermentation.gravity[1]"``."""
    def __str__(self) -> str: ...


# ------------------------------------------------------------ project


class Names(Generic[_T]):
    """The entries a project declares, read-only, by name.

    An entry is read as an attribute or an item — ``project.queries.ipas`` or
    ``project.queries["ipas"]`` — and iterating gives the names. An unknown
    name raises ``AttributeError`` or ``KeyError`` naming the nearest.
    """

    def __getattr__(self, name: str) -> _T: ...
    def __getitem__(self, name: str) -> _T: ...
    def __iter__(self) -> Iterator[str]: ...
    def __len__(self) -> int: ...
    def __contains__(self, name: object) -> bool: ...


class Project:
    """A project: what its ``.samplekitrc`` declares, read-only.

    Read from a sample or a list — ``brew.project``, ``brews.project``. Outside
    any project every collection of entries is empty.

    Example:
        >>> list(brews.project.queries)[:3]
        ['ipas', 'strong', 'medals']
    """

    @property
    def root(self) -> Path | None:
        """The folder holding ``.samplekitrc``, or ``None`` outside a project."""
    @property
    def queries(self) -> Names[Query]:
        """The saved queries, ``[query.*]``, by name, each a ``Query``."""
    @property
    def profiles(self) -> Names[Profile]:
        """The profiles, ``[profile.*]``, by name, each a ``Profile``."""
    @property
    def exports(self) -> Names[Export]:
        """The exports, ``[export.*]``, by name, each an ``Export``."""
    @property
    def figures(self) -> Names[FigureDeclaration]:
        """The figures, ``[figure.*]``, by name, each a ``FigureDeclaration``."""
    @property
    def matplotlib(self) -> dict[str, object]:
        """The matplotlib settings every figure is drawn with, ``[matplotlib]``: a
        dict by matplotlib's own dotted names.
        """
    @property
    def properties(self) -> Names[PropertyDeclaration]:
        """The declared properties, ``[property.*]``, by name, each a
        ``PropertyDeclaration``.
        """
    @property
    def units(self) -> Names[Unit]:
        """The declared units, ``[unit.*]``, by spelling, each a ``Unit``."""
    @property
    def render(self) -> Render:
        """How tables and figures are rendered, ``[render]``, as a ``Render``."""
    @property
    def model(self) -> Model | None:
        """The model, ``[model]``, or ``None`` when the project declares none."""
    @property
    def collection(self) -> Collection:
        """Which files are samples, ``[collection]``, as a ``Collection``."""


class Query:
    """A saved query, ``[query.*]``: a filter under a name."""

    @property
    def name(self) -> str:
        """The query's name."""
    @property
    def filter(self) -> str:
        """The filter expression, as ``-f`` takes it."""
    @property
    def directory(self) -> Path | None:
        """The folder the query searches by default, or ``None`` for the project's."""


class ColumnSpec:
    """One column of a profile: its field, its headings and its precision."""

    @property
    def field(self) -> str:
        """The field the column shows."""
    @property
    def label(self) -> str | None:
        """The heading of the column in a terminal table, or ``None`` for the field."""
    @property
    def header(self) -> str | None:
        """The heading of the column in an exported file, or ``None``."""
    @property
    def precision(self) -> Precision | None:
        """The column's precision — ``".3f"``, or a pair for the value and the
        uncertainty — or ``None``.
        """
    @property
    def template(self) -> str | None:
        """The text each cell is written as, such as ``"{value:.3f}"``, or ``None``."""


class Profile:
    """A profile, ``[profile.*]``: the columns of a table, and the order of its
    rows.
    """

    @property
    def name(self) -> str:
        """The profile's name."""
    @property
    def columns(self) -> tuple[ColumnSpec, ...]:
        """The columns, each a ``ColumnSpec``, in order."""
    @property
    def sort(self) -> tuple[str, ...]:
        """The sort keys, in order of precedence; a leading ``-`` sorts descending."""


class FigureDeclaration:
    """A figure declared in ``.samplekitrc``, ``[figure.*]``: what it draws, and
    how its axes are set.
    """

    @property
    def name(self) -> str:
        """The figure's name."""
    @property
    def kind(self) -> Literal["scatter", "line", "step", "bar", "box"]:
        """``"scatter"``, ``"line"``, ``"step"``, ``"bar"`` or ``"box"``."""
    @property
    def x(self) -> str:
        """The field on the x axis."""
    @property
    def y(self) -> str:
        """The field on the y axis."""
    @property
    def group(self) -> str | None:
        """The field, or comma-separated fields, whose values split the samples into
        series; ``None`` for one series.
        """
    @property
    def query(self) -> str | None:
        """The saved query that selects its samples, or ``None``."""
    @property
    def title(self) -> str | None:
        """The title, or ``None``."""
    @property
    def x_label(self) -> str | None:
        """The x axis label, or ``None`` for the field's symbol and unit."""
    @property
    def y_label(self) -> str | None:
        """The y axis label, or ``None`` for the field's symbol and unit."""
    @property
    def style(self) -> str | None:
        """The style whose symbols and units label the axes, or ``None``."""
    @property
    def x_limits(self) -> tuple[float | None, float | None] | None:
        """The x axis bounds, ``None`` for a free one; ``None`` when not declared."""
    @property
    def y_limits(self) -> tuple[float | None, float | None] | None:
        """The y axis bounds, ``None`` for a free one; ``None`` when not declared."""
    @property
    def x_scale(self) -> str | None:
        """``"linear"``, ``"log"`` or ``"symlog"``, or ``None``."""
    @property
    def y_scale(self) -> str | None:
        """``"linear"``, ``"log"`` or ``"symlog"``, or ``None``."""
    @property
    def aspect(self) -> str | None:
        """``"equal"`` or ``"auto"``, or ``None``."""
    @property
    def legend(self) -> str | None:
        """Where the legend goes — ``"best"``, ``"outside"``, ``"none"`` or one of
        matplotlib's places — or ``None``.
        """
    @property
    def figsize(self) -> tuple[float, float] | None:
        """The figure's width and height in centimetres, or ``None``."""


class Export:
    """An export, ``[export.*]``: a profile written to a file in a format."""

    @property
    def name(self) -> str:
        """The export's name."""
    @property
    def profile(self) -> str:
        """The name of the profile it writes."""
    @property
    def format(self) -> Literal["csv", "tsv", "json"]:
        """The format: ``"csv"``, ``"tsv"`` or ``"json"``."""
    @property
    def output(self) -> Path:
        """The file it writes."""
    @property
    def filename(self) -> bool:
        """Whether the export adds a column with each sample's file name."""
    @property
    def path(self) -> bool:
        """Whether the export adds a column with each sample's path."""
    @property
    def query(self) -> str | None:
        """The saved query that selects its samples, or ``None``."""


class Statistic:
    """A statistic of a property's readings, which a model declares to give the
    value or the uncertainty.

    The statistics are in ``samplekit.stats``.

    Example:
        >>> sk.Property(value=sk.stats.median, uncertainty=sk.stats.standard_error)
        Property(value=None)
    """

    @property
    def name(self) -> str:
        """Its name, as ``Summary`` spells it: ``"median"``."""


class stats:
    """The statistics a model can declare for readings.

    Six give a value — ``mean``, ``median``, ``minimum``, ``maximum``,
    ``first_quartile``, ``third_quartile`` — and three give an uncertainty —
    ``standard_error``, ``sample_stdev``, ``population_stdev``.

    Example:
        >>> og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
    """

    mean: Statistic
    median: Statistic
    minimum: Statistic
    maximum: Statistic
    first_quartile: Statistic
    third_quartile: Statistic
    standard_error: Statistic
    sample_stdev: Statistic
    population_stdev: Statistic


class PropertyDeclaration:
    """A property declared in ``.samplekitrc``, ``[property.*]``: its unit, symbol
    and precision, for every sample of the project.
    """

    @property
    def unit(self) -> str | None:
        """The unit, or ``None``."""
    @property
    def symbol(self) -> str | None:
        """The symbol, or ``None``."""
    @property
    def symbol_variants(self) -> Mapping[str, str]:
        """The symbol in each style, by style name:
        ``{"figure": "Original gravity"}``.
        """
    @property
    def precision(self) -> Precision | None:
        """The precision — ``".3f"``, or a pair for the value and the uncertainty —
        or ``None``.
        """


class Unit:
    """A unit declared in ``.samplekitrc``, ``[unit.*]``: how it is written in
    each style.
    """

    @property
    def spelling(self) -> str:
        """The unit as the files write it: ``"degC"``."""
    @property
    def variants(self) -> Mapping[str, str]:
        """The unit in each style, by style name: ``{"plain": "°C"}``."""


class Render:
    """How the project renders tables and figures, ``[render]``."""

    @property
    def style(self) -> str | None:
        """The style tables are rendered in, or ``None``."""
    @property
    def precision(self) -> Precision | None:
        """The precision of every number without its own, or ``None``."""
    @property
    def table(self) -> str | None:
        """How a table is drawn — ``"plain"`` or ``"boxed"`` — or ``None``."""
    @property
    def figure_style(self) -> str | None:
        """The style figures label their axes in when they name none, or ``None``."""


class Model:
    """The project's model, ``[model]``: the Python file and class its samples are
    read with.
    """

    @property
    def path(self) -> Path:
        """The model's Python file."""
    @property
    def class_name(self) -> str | None:
        """The class in that file, or ``None`` when the file declares only one."""


class Collection:
    """Which files of the project are samples, ``[collection]``."""

    @property
    def recursive(self) -> bool:
        """Whether subfolders are searched."""
    @property
    def include(self) -> tuple[str, ...]:
        """The globs a sample's file must match."""
    @property
    def exclude(self) -> tuple[str, ...]:
        """The globs of files that are not samples."""


class NotApplicable:
    """A value that does not apply to a sample: ``fg: n/a`` in its file.

    There is one, ``samplekit.NA``. It is false in a test and prints as
    ``n/a``; a property set to it is saved as ``n/a``, and a formula reading
    it gives ``NA`` too, without running.

    Example:
        >>> ipa.fg.value = sk.NA
    """

    def __bool__(self) -> bool: ...

NA: NotApplicable

# One path is a sample for a file and a list for a folder, which a type
# cannot tell apart: the script knows which it named, and a union would
# refuse `for brew in sk.load("brews")` until narrowed. Several paths are
# always one list.
@overload
def load(path: Pathish, /) -> Any:
    """Read samples as the command line reads its targets.

    A folder is read as a ``SampleList`` of its samples, as its project
    declares them, and a file as one ``Sample``; each is read with its
    project's model. Several paths are one ``SampleList``, in the order given,
    each sample once.

    Args:
        *paths: Folders and sample files.

    Returns:
        A ``Sample`` for one file, otherwise a ``SampleList``.

    Raises:
        FileNotFoundError: A path does not exist; the message names the
            nearest.
        ValueError: A file is not a sample.

    Example:
        >>> brews = sk.load("brews")
        >>> ipa = sk.load("brews/citra-ipa.md")
        >>> ipa.abv
        Property(value=6.825000000000035, uncertainty=0.14510233457805027, unit='%')
    """
@overload
def load(first: Pathish, second: Pathish, /, *paths: Pathish) -> SampleList:
    """Read samples as the command line reads its targets.

    A folder is read as a ``SampleList`` of its samples, as its project
    declares them, and a file as one ``Sample``; each is read with its
    project's model. Several paths are one ``SampleList``, in the order given,
    each sample once.

    Args:
        *paths: Folders and sample files.

    Returns:
        A ``Sample`` for one file, otherwise a ``SampleList``.

    Raises:
        FileNotFoundError: A path does not exist; the message names the
            nearest.
        ValueError: A file is not a sample.

    Example:
        >>> brews = sk.load("brews")
        >>> ipa = sk.load("brews/citra-ipa.md")
        >>> ipa.abv
        Property(value=6.825000000000035, uncertainty=0.14510233457805027, unit='%')
    """
def keep(message: str | None = None) -> None:
    """Keep what the script has saved so far as one snapshot of the history.

    A script's saves are kept as one snapshot when it ends, with the
    script's source beside it; ``keep`` takes that snapshot now, so that the
    history shows the script's steps apart.

    Args:
        message: What the snapshot says; the script's command line when not
            given. A blank message says nothing, and several lines are kept
            on one.

    Raises:
        TypeError: The message is not text.

    Example:
        >>> ipa.save()
        >>> sk.keep("volumes corrected")
    """


# A figure is matplotlib's. `figure` marks a model's method — an
# instance method draws one sample, a class method a collection — and `plot`
# draws it, a declared one, or the axes given.
_Method = TypeVar("_Method")


@overload
def figure(method: _Method) -> _Method:
    """Mark a method of a model as a figure.

    The method draws into the matplotlib axes it receives, and returns
    nothing; ``samplekit.plot`` and ``samplekit plot`` open it in a window or
    write it to a file. A method draws one sample; placed above
    ``@classmethod``, it draws a list of them.

    Args:
        method: The method, when used without arguments: ``@sk.figure``.
        subplots: ``(rows, columns)``: the method receives the array of axes
            ``plt.subplots`` makes, instead of one.
        figsize: The figure's width and height, in centimetres.

    Returns:
        The method, marked.

    Raises:
        ValueError: ``subplots`` is not two positive integers.

    Example:
        >>> class Brew(sk.Sample):
        ...     @sk.figure
        ...     def fermentation_curve(self, ax):
        ...         ax.plot(self.fermentation.values("day"),
        ...                 self.fermentation.values("gravity"))
    """
@overload
def figure(
    method: None = None,
    *,
    subplots: tuple[int, int] | None = None,
    figsize: tuple[float, float] | None = None,
) -> Callable[[_Method], _Method]:
    """Mark a method of a model as a figure.

    The method draws into the matplotlib axes it receives, and returns
    nothing; ``samplekit.plot`` and ``samplekit plot`` open it in a window or
    write it to a file. A method draws one sample; placed above
    ``@classmethod``, it draws a list of them.

    Args:
        method: The method, when used without arguments: ``@sk.figure``.
        subplots: ``(rows, columns)``: the method receives the array of axes
            ``plt.subplots`` makes, instead of one.
        figsize: The figure's width and height, in centimetres.

    Returns:
        The method, marked.

    Raises:
        ValueError: ``subplots`` is not two positive integers.

    Example:
        >>> class Brew(sk.Sample):
        ...     @sk.figure
        ...     def fermentation_curve(self, ax):
        ...         ax.plot(self.fermentation.values("day"),
        ...                 self.fermentation.values("gravity"))
    """
def plot(
    target: Sample | SampleList | Iterable[Sample],
    figure: str | None = None,
    *,
    x: str | None = None,
    y: str | None = None,
    # A field, or several whose combinations are the groups.
    group: str | Sequence[str] | None = None,
    kind: Literal["scatter", "line", "step", "bar", "box"] | None = None,
    title: str | None = None,
    x_label: str | None = None,
    y_label: str | None = None,
    style: str | None = None,
    x_limits: tuple[AxisBound, AxisBound] | None = None,
    y_limits: tuple[AxisBound, AxisBound] | None = None,
    x_scale: Literal["linear", "log", "symlog"] | None = None,
    y_scale: Literal["linear", "log", "symlog"] | None = None,
    aspect: Literal["equal", "auto"] | None = None,
    legend: str | None = None,
    figsize: tuple[float, float] | None = None,
    output: Pathish | None = None,
    overwrite: bool = False,
    project_style: bool = True,
) -> list[Any]:
    """Draw a figure of a sample or a list of samples, with matplotlib.

    The figure is a model's figure, one declared in ``.samplekitrc``, or
    axes given here: ``x`` and ``y``, drawn as a declared figure would be.
    It opens in a window, or is written to ``output``. The project's
    ``[matplotlib]`` settings apply to every figure.

    Args:
        target: A sample, a ``SampleList``, or an iterable of samples.
        figure: The name of a model's figure or of a declared one.
        x: The field on the x axis, for a figure not declared.
        y: The field on the y axis, for a figure not declared.
        group: A field, or several, whose values split the samples into
            series, each in its own colour.
        kind: ``"scatter"`` (the default), ``"line"``, ``"step"``, ``"bar"``
            or ``"box"``.
        title: The title; ``""`` draws none.
        x_label: The x axis label, instead of the field's symbol and unit.
        y_label: The y axis label, instead of the field's symbol and unit.
        style: The declared style whose symbols and units label the axes;
            ``[render] figure_style`` by default.
        x_limits: The x axis bounds, ``(low, high)``; ``None`` or ``"auto"``
            leaves one free.
        y_limits: The y axis bounds, ``(low, high)``; ``None`` or ``"auto"``
            leaves one free.
        x_scale: ``"linear"``, ``"log"`` or ``"symlog"``.
        y_scale: ``"linear"``, ``"log"`` or ``"symlog"``.
        aspect: ``"equal"`` or ``"auto"``.
        legend: Where the legend goes: ``"best"``, ``"outside"``, ``"none"``
            or one of matplotlib's places.
        figsize: The figure's width and height, in centimetres.
        output: The file to write, in the format its extension names:
            ``.png``, ``.pdf``, ``.svg``… ``{name}`` in it writes one file per
            sample, for a figure of one sample. A folder it names that does
            not exist is made.
        overwrite: Replace an existing file.
        project_style: Apply the project's ``[matplotlib]`` settings.

    Returns:
        The matplotlib figures drawn.

    Raises:
        TypeError: Neither ``figure`` nor ``x`` and ``y`` is given, or both
            are, or axes options are given to a model's figure.
        KeyError: An unknown figure or field; the message names the nearest.
        ValueError: No sample to draw, or an output format matplotlib
            cannot write.
        FileExistsError: ``output`` exists and ``overwrite`` is false.
        RuntimeError: No ``output`` is given and there is no display.

    Example:
        >>> figures = sk.plot(brews, "alcohol_by_style", output="out/abv.png")
        >>> figures = sk.plot(brews, x="abv", y="score", group="style",
        ...                   output="out/score.png")
    """
