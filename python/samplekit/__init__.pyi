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
# What a table formula gives for one cell: a number, or (value, uncertainty),
# as a property's formula does; a Property carrying both is taken too.
CellOutput: TypeAlias = Scalar | tuple[Scalar, float | None] | Property
Pathish: TypeAlias = str | PathLike[str]
Precision: TypeAlias = str | tuple[str, str]
# A formula of a property, of no argument; on a table's column, of the row.
Compute: TypeAlias = Callable[[], Scalar | tuple[Scalar, float | None]] | Callable[[RowView], CellOutput]
ValueCompute: TypeAlias = Callable[[], Scalar] | Callable[[RowView], Scalar]
UncertaintyCompute: TypeAlias = Callable[[], float | None] | Callable[[RowView], float | None]
RowCompute: TypeAlias = Callable[[RowView], CellOutput | Mapping[str, CellOutput]]
ColumnCompute: TypeAlias = Callable[
    [Mapping[str, ColumnView]],
    Sequence[CellOutput] | Mapping[str, Sequence[CellOutput]],
]
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
        >>> steel.tags.append("reference")
    """


class Property:
    """A property: a value, its uncertainty and its unit.

    A property stands for its value in arithmetic and comparisons, so
    ``steel.area * 2`` and ``steel.area > 25`` work on the number, and it prints
    as the whole property, ``25.056 ± 0.053 mm²``. A model declares its properties in its
    ``__init__``; a property a model does not declare is made the same way and
    assigned to the sample.

    A property is one of four kinds, by what is given: a value entered by hand;
    readings, whose value and uncertainty a statistic gives; a value a formula
    computes (``compute``, or ``compute_value`` and ``compute_uncertainty``);
    or nothing yet.

    A ``Table`` declares its columns with it too: there a property describes
    every cell of its column, with the same keywords, and its formula is given
    the row, ``f(row)``. A column takes no value nor uncertainty: each cell
    holds its own, in the file.

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
        compute: A function of no argument returning the property: a number,
            or ``(value, uncertainty)``.
        compute_value: A function of no argument returning the value alone,
            beside an uncertainty entered, given by readings or computed apart.
        compute_uncertainty: A function of no argument returning the
            uncertainty, beside a value entered or computed apart.
        compute_column: For a table's column only: a function given the
            columns it reads, by name, and returning a list, one cell per row,
            each a number or ``(value, uncertainty)``.
        depends_on: What the formula reads: a list of names — ``["width", "thickness"]``,
            ``"curve.force"`` for a column, ``"row.force"`` for a
            cell of the same row — or, where the value and the uncertainty have
            a formula each, ``{"v": [...], "u": [...]}``. ``[]`` says it reads
            nothing of the sample. It can also be declared later with
            ``Sample.set_dependencies``.

    Raises:
        ValueError: Two arguments give the same thing — ``value`` and a
            formula, ``compute`` and ``compute_value``, ``compute_column`` and
            another — or a value is not a number where a unit is given.
        TypeError: A formula is not callable, or a symbol or a precision is
            given.

    Example:
        >>> import samplekit as sk
        >>> sk.Property(3.2, 0.1, unit="kg")
        Property(value=3.2, uncertainty=0.1, unit='kg')
        >>> width = sk.Property(unit="mm", value=sk.stats.mean,
        ...                     uncertainty=sk.stats.standard_error)
    """

    def __init__(
        self,
        value: Scalar | Sequence[float] | Property | Statistic = None,
        uncertainty: float | Statistic | None = None,
        unit: str | None = None,
        compute: Compute | None = None,
        compute_value: ValueCompute | None = None,
        compute_uncertainty: UncertaintyCompute | None = None,
        compute_column: ColumnCompute | None = None,
        # What this formula reads, so that a change to one of them makes it
        # outdated. Optional here rather than required: a model may declare the
        # same thing with `Sample.set_dependencies`, and a checker cannot see
        # that from the call. A computed property that declares neither is refused once the
        # model is built — `depends_on=[]` says it reads nothing.
        depends_on: Declared | None = None,
    ) -> None: ...
    # A property's value is read as a number, so that a model's arithmetic
    # type-checks: a formula runs only once its inputs hold one, and waits
    # otherwise. Outside a formula it may hold text, a date, NA or nothing,
    # which the code reading it checks. Assigned, it takes all of them, and
    # a list of readings.
    @property
    def value(self) -> float:
        """The value — a number, text, a boolean, a date, ``samplekit.NA``, or
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
        """The value — the same as ``value``, spelt as a field path spells it
        (``area.v``).
        """
    @v.setter
    def v(self, value: Scalar | NotApplicable | Sequence[float] | Property) -> None: ...
    uncertainty: float | None
    """The absolute standard uncertainty, in the value's unit, or ``None``."""
    u: float | None
    """The uncertainty — the same as ``uncertainty``, spelt as a field path spells
    it (``area.u``).
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
        """The property's symbol — the one this sample's file writes, else the one
        the project declares in ``.samplekitrc`` under ``[property.NAME]``, else
        ``None``. Read only: a symbol is the project's, declared there, never
        set by a model or a script.
        """
    @property
    def is_computed(self) -> bool:
        """Whether a formula gives the value."""
    @property
    def is_outdated(self) -> bool:
        """Whether the value is outdated — an input changed since it was computed."""
    @property
    def failure(self) -> str | None:
        """What the formula raised when it last ran, or ``None`` if it did not fail."""
    @property
    def is_edited(self) -> bool:
        """Whether the value is edited — typed over its formula, or a computed table
        cell whose record is missing.
        """
    @property
    def edited_upstream(self) -> list[str]:
        """The edited values this one was computed from, directly or further up."""
    @property
    def state(self) -> str:
        """Where the value stands, in one word.

        One of ``"entered"`` (typed in, no formula), ``"absent"`` (no formula,
        and nothing typed in), ``"current"``,
        ``"outdated"`` (an input changed since it was computed), ``"waiting"``
        (an input it reads was emptied since: the number is the one computed
        before), ``"never_computed"``, ``"edited"`` (a value typed over its
        formula) or ``"failed"`` (its formula raised): the words a filter's
        ``state`` takes.
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
        """Format the property at its declared precision.

        Args:
            unit: Include the unit.

        Returns:
            The property as text, such as ``"25.056 ± 0.053 mm²"``. For another
            precision, use a format specifier: ``f"{steel.area:.2f}"``.
        """
    def to_text(self, style: str | None = None) -> str:
        r"""The property as a style writes it: its units, its symbols, its form
        of a property — ``\SI{12.5 \pm 0.05}{\gram}`` in a style declaring
        ``property = '\SI{{value} \pm {uncertainty}}{{unit}}'``.

        Args:
            style: A style's name: ``"plain"``, one SampleKit gives —
                ``"latex"``, ``"markdown"``, ``"html"`` — or one ``.samplekitrc``
                declares. Without it, ``[render] style``'s, as ``str()`` gives.

        Returns:
            The text, ``n/a`` where the value does not apply, and the style's
            ``missing`` where there is none.

        Raises:
            ValueError: No style has that name.

        Example:
            >>> steel.area.to_text("latex")
            '$25.056 \\pm 0.053$ mm²'
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


class RowView:
    """One row of a table, as a column's formula receives it.

    A cell is read by its column's name, as an attribute or an item:
    ``row.force`` or ``row["force"]``, each a ``Property``.
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
    """One column of a table, as a ``compute_column`` formula receives it, and
    as ``table.column_name`` reads it.

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


class ComputedColumns:
    """Several columns of a table filled by one formula: given each row, or the
    columns it reads whole.

    A column filled alone declares its formula on its ``Property``; this is for
    one computation that gives several columns at once, a fit's two parameters.

    Args:
        columns: The columns it fills: a name, or a list of names.
        compute: A function given each row, a ``RowView``, and returning a dict:
            one cell for each column it fills, a number or
            ``(value, uncertainty)``.
        compute_column: A function given the columns it reads, by name, and
            returning a dict: one list of cells for each column it fills, one
            cell per row.
        depends_on: What the formula reads: ``"row.force"`` for a cell of the
            same row, ``"curve.force"`` for a whole column, ``"area"`` for a
            property of the sample; ``[]`` for nothing.

    Raises:
        ValueError: Neither formula is given, or both, or ``depends_on`` is not.
        TypeError: A formula is not callable.

    Example:
        >>> bounds = sk.ComputedColumns(
        ...     ["low", "high"], compute=lambda row: {"low": 1.0, "high": 2.0},
        ...     depends_on=[],
        ... )
    """

    def __init__(
        self,
        columns: str | Sequence[str],
        *,
        compute: RowCompute | None = None,
        compute_column: ColumnCompute | None = None,
        # What the formula reads: `"row.force"`, `"curve.force"`, `"area"`.
        depends_on: Sequence[str] | None = None,
    ) -> None: ...


class Table:
    """A table of a sample: named columns, and rows identified by their index.

    The index is the column — or the columns — whose values identify a row: a
    day, a temperature, a inspector. A cell is read by its index value, never by
    its position: ``table.at(0.2, "force")``.

    A column is declared with ``Property``: its unit, the statistics of its
    cells' readings, or the formula that computes each cell, given the row
    (``compute``, ``compute_value``, ``compute_uncertainty``), or the whole
    column at once (``compute_column``). A formula filling several columns at
    once is a ``ComputedColumns``, given in ``computed``.

    Args:
        columns: A dict of ``Property`` by name, in the order they are shown.
        index: The name of the index column, or a list of names; always named.
        title: A title for the table.
        rows: The first rows: each a dict of cells by column name.
        computed: The ``ComputedColumns`` of the table, each filling several
            columns.

    Raises:
        TypeError: A column is not a ``Property``, or is given a value; the
            index is not named.
        ValueError: The index names no column, a row does not fit the columns,
            or a column is filled by two formulas.

    Example:
        >>> inspection = sk.Table(
        ...     {"inspector": sk.Property(), "score": sk.Property()},
        ...     index="inspector",
        ...     rows=[{"inspector": "Ana", "score": 41}],
        ... )
        >>> inspection.at("Ana", "score").value
        41
    """

    def __init__(
        self,
        columns: Mapping[str, Property],
        *,
        index: str | Sequence[str],
        title: str | None = None,
        rows: Iterable[Mapping[str, CellInput]] | None = None,
        computed: Iterable[ComputedColumns] | None = None,
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
            >>> steel.curve.at(0.2, "force").value
            9.516
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
            >>> steel.curve.values("force")[:3]
            [0.0, 1.281, 2.585]
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
    def to_text(self, style: str | None = None) -> str:
        """The table as ``samplekit view`` unfolds it: its columns headed with
        their units, its index first, each cell in a style's forms — those of
        its sample's project, or of the project the current folder is in for a
        table of no sample.

        Args:
            style: A style's name: ``"plain"``, one SampleKit gives, or one
                ``.samplekitrc`` declares. Without it, ``[render] style``'s.

        Returns:
            The text.

        Raises:
            ValueError: No style has that name.

        Example:
            >>> print(steel.curve.to_text())
        """
    def to_csv(self) -> str:
        """The table as rows of CSV, as ``samplekit FILE view TABLE --csv``
        prints it: its index first, each column a value and its uncertainty
        headed with the unit, at the precision its columns declare.

        Returns:
            The text.

        Raises:
            ValueError: The table belongs to no sample yet.

        Example:
            >>> text = steel.curve.to_csv()
        """
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
    ``steel.area`` or ``steel["area"]``, and any other field path as an item:
    ``steel["curve.force[0.2]"]`` is that cell's value. What is assigned
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
        >>> steel = sk.Sample("samples/steel-01.md")
        >>> steel.material
        'steel'
        >>> steel["curve.force[0.2]"]
        9.516
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
            >>> specimen = sk.Sample.new("samples/steel-02.md")
            >>> specimen.material = "steel"
            >>> specimen.save()
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
        """The project the sample's file belongs to — what its ``.samplekitrc``
        declares.
        """
    name: str | None
    """The sample's name, or ``None``."""
    tags: BoundList[str]
    """The sample's tags, as a list whose changes are saved with the sample.

    Each tag is an identifier, and none appears twice.
    """
    note: str
    """The note — the Markdown text below the frontmatter, kept as written."""
    def to_text(self, style: str | None = None) -> str:
        """The sample as ``samplekit view`` shows it — its values, then its
        tables — each value in a style's forms.

        Args:
            style: A style's name: ``"plain"``, one SampleKit gives, or one
                ``.samplekitrc`` declares. Without it, ``[render] style``'s.

        Returns:
            The text.

        Raises:
            ValueError: No style has that name.

        Example:
            >>> print(steel.to_text())
        """
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
                Also: another process is saving the same file at this moment.
            FileExistsError: ``path`` exists already.
            FileNotFoundError: The folder does not exist, or the sample's file was
                removed or moved since it was read.
            IsADirectoryError: ``path`` is a folder.
            OSError: The file changed on disk since it was read.
            PermissionError: The file is read-only.

        Example:
            >>> steel.grade = "S275"
            >>> steel.save()
        """
    def linked(self, field: str) -> Sample | Path | list[Sample | Path]:
        """The sample an attribute links to, read as ``sk.load`` reads it, or
        the file it leads to.

        A link is an attribute written ``"[[NAME]]"``, as Obsidian writes
        one: the sample of the project whose file is NAME.md, else whose
        ``name`` is NAME; or a Markdown link to a file,
        ``"[the second](NAME.md)"``, its path read from this sample's folder.
        A sample's file gives the sample; any other file its path. A list of
        links gives a list, in order.

        Args:
            field: The attribute holding the link, or the links.

        Returns:
            The sample it names, or a ``pathlib.Path`` for a file that is no
            sample; for a list, a list of them, in order.

        Raises:
            KeyError: The sample holds no such attribute.
            ValueError: The attribute is no link: text, or a web address; or
                several samples answer to it.
            FileNotFoundError: No sample answers to it, or nothing is at its
                path.

        Example:
            >>> steel.linked("reference").name
            'aluminium-01'
        """
    def rename(self, new: str) -> Path:
        """Rename the sample: its file, its own files, and every link of its
        project leading to it.

        The file becomes ``NEW.md`` beside the old one, its own files under
        ``[collection] files`` renamed with it, its ``name`` too where it was
        the file's name; each link of the project that led to it, ``[[OLD]]``
        or ``[text](OLD.md)``, is written to lead to it again. The history
        keeps the samples; renamed back, the files and the links go back too.

        Args:
            new: The new name: the file's, without its folder or ``.md``.

        Returns:
            The sample's new path.

        Raises:
            ValueError: The sample has no file, the name is no file's name, or
                the script runs under ``samplekit run``, whose preview cannot
                hold a rename.
            FileExistsError: Another sample, or one of its files, has the name.
            OSError: The file changed on disk since it was read.

        Example:
            >>> sk.Sample("samples/steel-01.md").rename("steel-04")
            PosixPath('samples/steel-04.md')
        """
        ...

    def remove(self) -> None:
        """Delete the sample's file.

        The project's history keeps it, and ``samplekit FILE restore --write``
        gives it back; the sample's own files, under ``[collection] files``,
        stay. The sample keeps its values: ``save()`` writes it again.

        Raises:
            ValueError: The sample has no file, or the script runs under
                ``samplekit run``, whose preview cannot hold a removal.
            FileNotFoundError: Its file was removed or moved since it was read.
            OSError: The file changed on disk since it was read.

        Example:
            >>> sk.Sample("samples/steel-01.md").remove()
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
            RuntimeError: Over SSH without a display — ``DISPLAY`` and
                ``WAYLAND_DISPLAY`` unset — there is nothing to open them on.
                Elsewhere the system's opener is started, and what it does
                without a display is its own.
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
            >>> class Specimen(sk.Sample):
            ...     def __init__(self, path=None, name=None):
            ...         super().__init__(path, name=name)
            ...         self.area = sk.Property(unit="mm²", compute_value=self._area)
            ...         self.set_dependencies("area", depends_on=["width", "thickness"])
            ...
            ...     def _area(self):
            ...         return self.width.value * self.thickness.value
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
            >>> steel.dependencies("area")
            ['thickness', 'width']
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
            >>> steel.affected_by_change("thickness")
            ['area', 'curve.stress', 'tensile_strength', 'yield_strength', 'young_modulus']
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
            >>> steel.compute()
            >>> steel.save()
        """
    def outdated(self) -> list[str]:
        """The outdated values — those an input changed under since they were
        computed.

        Returns:
            The names of the outdated properties, and ``table.column`` for a
            column with an outdated cell.
        """
    def edited(self) -> list[str]:
        """The edited values — those typed over their formula.

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
            ``"outdated"``, ``"formula changed"``, ``"edited"``, ``"failed"``,
            ``"never computed"`` or ``"waits for notch"``.

        Example:
            >>> steel.not_current()
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
    is a sample, by position or by name — ``specimens["steel-01"]`` — and a slice
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
        >>> specimens = sk.SampleList("samples")
        >>> len(specimens)
        3
        >>> specimens["steel-01"].material
        'steel'
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
            predicate: A filter expression — ``"elongation > 20 && material == steel"`` — or a
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
            >>> [specimen.name for specimen in specimens.filter("tensile_strength > 300")]
            ['aluminium-01', 'steel-01']
        """
    def sort(self, key: FieldKey | Sequence[FieldKey] | Callable[[Sample], Any]) -> None:
        """Sort the list in place, and return ``None``.

        Args:
            key: A field — ``"area"``, or ``"-area"`` for descending — several in a
                list or comma-separated, or a function of a sample as ``sorted``
                takes one.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            TypeError: A sort by ``state``, which a sample holds several of.
        """
    def sorted(self, key: FieldKey | Sequence[FieldKey] | Callable[[Sample], Any]) -> SampleList:
        """A new list in sorted order; this one is unchanged.

        Args:
            key: A field — ``"area"``, or ``"-area"`` for descending — several in a
                list or comma-separated, or a function of a sample as ``sorted``
                takes one.

        Returns:
            A new list.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            TypeError: A sort by ``state``, which a sample holds several of.

        Example:
            >>> specimens.sorted("-tensile_strength")[0].name
            'steel-01'
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
            >>> len(specimens.query("ductile"))
            2
        """
    def stats(self, field: FieldKey) -> Summary:
        """The statistics of one field over the list, as ``--summary`` gives them.

        Args:
            field: A field: ``"area"``, ``"area.u"``, ``"curve.force[0.2]"``.

        Returns:
            A ``Summary``. Its mean is weighted by 1/u² where every value has an
            uncertainty above zero.

        Raises:
            KeyError: An unknown field; the message names the nearest.
            ValueError: No sample of the list holds a number there.

        Example:
            >>> round(specimens.stats("tensile_strength").mean, 1)
            352.3
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
            >>> list(specimens.group_by("material"))
            ['aluminium', 'copper', 'steel']
        """
    def save_all(self, directory: Pathish | None = None, overwrite: bool = False) -> list[Path]:
        """Write every sample of the list: back where each was read, or into a folder.

        Without a folder, each sample is written to its own file, as ``save()``
        writes it: a file changed since it was read — by another object, or an
        editor — is refused rather than written over. Into a folder, each is
        written under its file's name, or its name, and nothing is written if
        any file is refused.

        Args:
            directory: The folder, which must exist; each sample's own file when
                left out.
            overwrite: Replace existing files in the folder.

        Returns:
            The paths written.

        Raises:
            FileNotFoundError: The folder does not exist.
            NotADirectoryError: It is a file.
            FileExistsError: A file exists and ``overwrite`` is false.
            ValueError: Two samples would be written to the same file, or,
                without a folder, a sample has no file of its own.
            OSError: Without a folder, a file changed on disk since it was read.
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
        """The project of the list's samples — what their ``.samplekitrc`` declares.

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
            >>> columns = specimens.to_dict(columns="name,area")
            >>> list(columns)
            ['name', 'area_value', 'area_uncertainty']
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
            >>> text = specimens.to_csv(columns="name,area")
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
            >>> text = specimens.to_csv(columns="name,area")
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
            >>> text = specimens.to_tsv(columns="name,area")
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
            >>> text = specimens.to_tsv(columns="name,area")
        """
    @overload
    def to_text(
        self,
        style: str,
        path: None = None,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> str:
        """The list as a document in a style: ``"latex"``, ``"markdown"``,
        ``"html"``, or one ``[style.NAME]`` declares, as ``--style NAME -o -``
        writes it.

        Args:
            style: The style's name.
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: No style writing documents has that name, or neither
                ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> table = specimens.to_text("latex", columns="name,area")
        """
    @overload
    def to_text(
        self,
        style: str,
        path: Pathish,
        columns: str | Sequence[FieldKey] | None = None,
        profile: str | Profile | None = None,
        overwrite: bool = False,
    ) -> None:
        """The list as a document in a style: ``"latex"``, ``"markdown"``,
        ``"html"``, or one ``[style.NAME]`` declares, as ``--style NAME -o -``
        writes it.

        Args:
            style: The style's name.
            path: The file to write. Without it, the text is returned.
            columns: The fields to include: a list, or comma-separated as ``-c``
                writes them, each ``FIELD[:PRECISION][=LABEL]``.
            profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
                instead of ``columns``.
            overwrite: Replace an existing file.

        Returns:
            The text, when no ``path`` is given; otherwise ``None``.

        Raises:
            ValueError: No style writing documents has that name, or neither
                ``columns`` nor ``profile`` is given, or both.
            KeyError: An unknown field or profile; the message names the nearest.
            FileExistsError: ``path`` exists and ``overwrite`` is false.

        Example:
            >>> table = specimens.to_text("latex", columns="name,area")
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
            >>> text = specimens.to_json(columns="name,area")
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
            >>> text = specimens.to_json(columns="name,area")
        """
    def export(
        self, export: str | Export, output: Pathish | None = None, overwrite: bool = False
    ) -> Path:
        """Write an export declared in ``.samplekitrc``, as ``samplekit export
        --write`` does.

        The export's own query, if it declares one, narrows the list first. An
        export of a figure draws it as ``sk.plot`` does, to its file.

        Args:
            export: The export's name, or an ``Export``.
            output: Write here instead of the declared file.
            overwrite: Replace an existing file.

        Returns:
            The path written, absolute.

        Raises:
            KeyError: No export has that name, or its profile or figure is not
                declared.
            FileExistsError: The file exists and ``overwrite`` is false.
            ValueError: ``output``'s extension does not match the export's format.

        Example:
            >>> specimens.export("paper", output="paper/results.tex").name
            'results.tex'
        """


class Summary:
    """The statistics of a set of values: readings, or a field over a list.

    Given by ``Property.stats`` and ``SampleList.stats``.
    """

    def to_text(self, style: str | None = None) -> str:
        """The summary as ``--summary`` writes a row of it: the count, the
        mean, the deviation ``s``, its standard error ``sem``, the median and
        the extremes.

        Args:
            style: Accepted for every ``to_text``; a summary's numbers are
                written alike in every style.

        Returns:
            The text.
        """
    @property
    def count(self) -> int:
        """How many values were summarised."""
    @property
    def considered(self) -> int | None:
        """How many samples the value applies to, those holding no number
        included and those saying *not applicable* left out: the denominator of
        ``count``, as ``--summary`` writes ``3/4``. ``None`` for readings.
        """
    @property
    def not_applicable(self) -> int:
        """How many samples say the value is not applicable, counted apart —
        neither summarised nor missing.
        """
    @property
    def minimum(self) -> float:
        """The smallest value."""
    @property
    def maximum(self) -> float:
        """The largest value."""
    @property
    def mean(self) -> float:
        """The mean — weighted by 1/u² where every value has an uncertainty above zero,
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
        """The standard error of the mean — 1/√Σw beside a weighted mean, the
        sample standard deviation over √n beside an arithmetic one; ``None``
        for one value unweighted.
        """


class Field:
    """A field of a list, as ``SampleList.fields`` gives it, and as ``-c`` and
    ``-s`` take it.

    ``str(field)`` is its path, and a field is accepted wherever a field's name
    is: ``specimens.sorted(specimens.fields.area)``.
    """

    @property
    def path(self) -> str:
        """The field's path — ``"area"``, ``"curve.force[0.2]"``."""
    def __str__(self) -> str: ...


# ------------------------------------------------------------ project


class Names(Generic[_T]):
    """The entries a project declares, read-only, by name.

    An entry is read as an attribute or an item — ``project.queries.ductile`` or
    ``project.queries["ductile"]`` — and iterating gives the names. An unknown
    name raises ``AttributeError`` or ``KeyError`` naming the nearest.
    """

    def __getattr__(self, name: str) -> _T: ...
    def __getitem__(self, name: str) -> _T: ...
    def __iter__(self) -> Iterator[str]: ...
    def __len__(self) -> int: ...
    def __contains__(self, name: object) -> bool: ...


class Project:
    """A project: what its ``.samplekitrc`` declares, read-only.

    Read from a sample or a list — ``steel.project``, ``specimens.project``. Outside
    any project every collection of entries is empty.

    Example:
        >>> list(specimens.project.queries)
        ['ductile']
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
        """The matplotlib settings every figure is drawn with, ``[matplotlib]`` —
        a dict by matplotlib's own dotted names.
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
    @property
    def matplotlib(self) -> dict[str, Any]:
        """Its own matplotlib settings, over the project's ``[matplotlib]``, by
        matplotlib's dotted names.
        """


class Export:
    """An export, ``[export.*]``: a profile written to a file in a format, or
    a figure drawn to a file.
    """

    @property
    def name(self) -> str:
        """The export's name."""
    @property
    def profile(self) -> str | None:
        """The name of the profile it writes, or ``None`` for a figure's export."""
    @property
    def figure(self) -> str | None:
        """The name of the figure it draws, or ``None`` for a table's export."""
    @property
    def format(self) -> Literal["csv", "tsv", "json"] | None:
        """The data format — ``"csv"``, ``"tsv"``, ``"json"`` — or ``None`` for
        a document its style writes, or a figure.
        """
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
        """Its name, as ``Summary`` spells it — ``"median"``."""


class stats:
    """The statistics a model can declare for readings.

    Six give a value — ``mean``, ``median``, ``minimum``, ``maximum``,
    ``first_quartile``, ``third_quartile`` — and three give an uncertainty —
    ``standard_error``, ``sample_stdev``, ``population_stdev``.

    Example:
        >>> width = sk.Property(unit="mm", value=sk.stats.mean,
        ...                     uncertainty=sk.stats.standard_error)
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
        r"""The symbol in each style, by style name:
        ``{"figure": "$\\sigma$"}``.
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
        """The unit as the files write it — ``"degC"``."""
    @property
    def variants(self) -> Mapping[str, str]:
        """The unit in each style, by style name — ``{"plain": "°C"}``."""


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
    @property
    def files(self) -> tuple[str, ...]:
        """Where a sample's own files are looked for, ``[collection] files`` —
        the folders searched by the sample's name, and the patterns naming it,
        as written.
        """


class NotApplicable:
    """A value that does not apply to a sample — ``yield_strength: n/a`` in its file.

    There is one, ``samplekit.NA``. It is false in a test and prints as
    ``n/a``; a property set to it is saved as ``n/a``, and a formula reading
    it gives ``NA`` too, without running.

    Example:
        >>> steel.yield_strength.value = sk.NA
    """

    def __bool__(self) -> bool: ...

NA: NotApplicable

# One path is a sample for a file and a list for a folder, which a type
# cannot tell apart: the script knows which it named, and a union would
# refuse `for specimen in sk.load("samples")` until narrowed. Several paths are
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
        >>> specimens = sk.load("samples")
        >>> steel = sk.load("samples/steel-01.md")
        >>> steel.area
        Property(value=25.05586444444445, uncertainty=0.05217296159913354, unit='mm²')
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
        >>> specimens = sk.load("samples")
        >>> steel = sk.load("samples/steel-01.md")
        >>> steel.area
        Property(value=25.05586444444445, uncertainty=0.05217296159913354, unit='mm²')
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

    Warns:
        UserWarning: Nothing was saved in a project since the last snapshot,
            so nothing is kept.

    Raises:
        TypeError: The message is not text.

    Example:
        >>> steel.save()
        >>> sk.keep("thickness measured again")
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
        matplotlib: matplotlib's settings for this figure alone, by their
            names, over the project's ``[matplotlib]``:
            ``{"lines.marker": "None"}``.

    Returns:
        The method, marked.

    Raises:
        ValueError: ``subplots`` is not two positive integers.

    Example:
        >>> class Specimen(sk.Sample):
        ...     @sk.figure
        ...     def stress_strain(self, ax):
        ...         ax.plot(self.curve.values("strain"),
        ...                 self.curve.values("stress"))
    """
@overload
def figure(
    method: None = None,
    *,
    subplots: tuple[int, int] | None = None,
    figsize: tuple[float, float] | None = None,
    matplotlib: dict[str, Any] | None = None,
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
        matplotlib: matplotlib's settings for this figure alone, by their
            names, over the project's ``[matplotlib]``:
            ``{"lines.marker": "None"}``.

    Returns:
        The method, marked.

    Raises:
        ValueError: ``subplots`` is not two positive integers.

    Example:
        >>> class Specimen(sk.Sample):
        ...     @sk.figure
        ...     def stress_strain(self, ax):
        ...         ax.plot(self.curve.values("strain"),
        ...                 self.curve.values("stress"))
    """
# A script is the model's. `script` marks a method — an instance method acts
# on one sample, a class method on the selection — and `samplekit run` runs it.
def script(method: _Method) -> _Method:
    """Mark a method of a model as a script.

    ``samplekit run NAME`` runs it, and ``r`` in the workbench: a method acts
    on one sample, each selected in turn; placed above ``@classmethod``, it
    receives the selection. Its keyword parameters are the command's options,
    ``--set offset=0.002``, read as their annotation or their default says;
    its docstring is its description. What it changes in the samples is
    shown, and written once the change is accepted; what it returns is shown.

    Args:
        method: The method.

    Returns:
        The method, marked.

    Raises:
        TypeError: The method is a static method, or takes ``*args`` or
            ``**kwargs``.

    Example:
        >>> class Specimen(sk.Sample):
        ...     @sk.script
        ...     def classify(self, limit: float = 20.0):
        ...         "Class the specimen's ductility by its elongation at break."
        ...         self.ductility = "high" if self.elongation.value >= limit else "low"
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
        >>> figures = sk.plot(specimens, "curves", output="paper/curves.pdf")
        >>> figures = sk.plot(specimens, x="yield_strength", y="tensile_strength",
        ...                   group="material", output="strengths.png")
    """
