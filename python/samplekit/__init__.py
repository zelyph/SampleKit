"""SampleKit: sample data in plain Markdown — computed, current, tracked.

A sample is a Markdown file whose frontmatter holds its values. In Python a
sample is read from its file, read and changed through its attributes, and
saved back::

    import samplekit as sk

    brew = sk.load("brews/citra-ipa.md")
    brew.volume.value = 21
    brew.compute()
    brew.save()

``load`` reads a folder as a ``SampleList``, which filters, sorts, groups and
exports as the command line does.

The examples of this package run in the demo's ``08-python`` folder, after::

    import samplekit as sk

    brews = sk.load("brews")
    ipa = brews["citra-ipa"]
"""

# This file is the package's Python, and it writes only what the native module
# cannot: BoundList, a real list bound to a sample, and the class of Sample,
# which loads a file after the class's __init__ returns. Every rule either
# applies is a call into samplekit._native.


class NotApplicable:
    """A value that does not apply to a sample: ``fg: n/a`` in its file.

    There is one, ``samplekit.NA``. It is false in a test and prints as
    ``n/a``; a property set to it is saved as ``n/a``, and a formula reading
    it gives ``NA`` too, without running.

    Example:
        >>> ipa.fg.value = sk.NA
    """

    _one = None

    def __new__(cls):
        if cls._one is None:
            cls._one = super().__new__(cls)
        return cls._one

    def __repr__(self):
        return "samplekit.NA"

    def __str__(self):
        return "n/a"

    def __bool__(self):
        return False

    def __reduce__(self):
        return (NotApplicable, ())


NA = NotApplicable()

from samplekit import _native  # noqa: E402
from samplekit._native import (
    Collection,
    Column,
    ColumnSpec,
    ColumnView,
    Export,
    Field,
    FigureDeclaration,
    Model,
    Names,
    Profile,
    Project,
    Property,
    PropertyDeclaration,
    Query,
    Render,
    RowView,
    SampleList,
    Statistic,
    Summary,
    Table,
    Unit,
)

from samplekit._native import load, stats
from samplekit._figures import figure, plot

__version__ = _native.__version__


def keep(message=None):
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
    if message is not None and not isinstance(message, str):
        raise TypeError(f"keep() takes a message as text, not {type(message).__name__}")
    if message is not None:
        # One line: the history's log shows a message a row each.
        message = " ".join(message.split()) or None
    _native._history_flush(message)


# One snapshot for the whole script, when the interpreter ends.
import atexit as _atexit  # noqa: E402

_atexit.register(_native._history_flush)

__all__ = [
    "BoundList",
    "Collection",
    "Column",
    "ColumnSpec",
    "ColumnView",
    "Export",
    "Field",
    "FigureDeclaration",
    "Model",
    "NA",
    "Names",
    "NotApplicable",
    "Profile",
    "Project",
    "Property",
    "PropertyDeclaration",
    "Query",
    "Render",
    "RowView",
    "Sample",
    "SampleList",
    "Statistic",
    "Summary",
    "Table",
    "Unit",
    "figure",
    "keep",
    "load",
    "plot",
    "stats",
]


class BoundList(list):
    """A list bound to a sample: its changes are the sample's.

    A sample's tags, a list attribute and a property's readings are read as
    one. ``append``, ``remove``, an item assigned and every other change is
    checked and made in the sample too, and a refused item leaves both
    unchanged. ``list(...)`` or ``.copy()`` is a plain copy, bound to
    nothing, and so is a list read before the attribute was assigned again.

    Example:
        >>> ipa.tags.append("medal")
    """

    __slots__ = ("_owner", "_name", "_epoch", "__weakref__")

    def _change(self, mutate):
        candidate = list(self)
        result = mutate(candidate)
        committed = self._owner._commit_list(self._name, self._epoch, candidate)
        list.__setitem__(self, slice(None), candidate if committed is None else committed)
        return result

    def append(self, item):
        self._change(lambda items: items.append(item))

    def extend(self, items):
        self._change(lambda current: current.extend(items))

    def insert(self, index, item):
        self._change(lambda items: items.insert(index, item))

    def pop(self, index=-1):
        return self._change(lambda items: items.pop(index))

    def remove(self, item):
        self._change(lambda items: items.remove(item))

    def clear(self):
        self._change(lambda items: items.clear())

    def sort(self, *, key=None, reverse=False):
        self._change(lambda items: items.sort(key=key, reverse=reverse))

    def reverse(self):
        self._change(lambda items: items.reverse())

    def __setitem__(self, index, value):
        self._change(lambda items: items.__setitem__(index, value))

    def __delitem__(self, index):
        self._change(lambda items: items.__delitem__(index))

    def __iadd__(self, other):
        self._change(lambda items: items.__iadd__(other))
        return self

    def __imul__(self, count):
        self._change(lambda items: items.__imul__(count))
        return self

    def copy(self):
        return list(self)

    def __reduce_ex__(self, protocol):
        return (list, (list(self),))


def _bind(owner, name, epoch, items):
    bound = BoundList(items)
    bound._owner = owner
    bound._name = name
    bound._epoch = epoch
    return bound


def _arguments(path=None, name=None, model=None):
    return path, name, model


class _Loading(type(_native.Sample)):
    """Constructs a sample, then loads its file.

    A file is read after the class's ``__init__`` returns, so that the
    ``Property`` a model declares is filled by the file rather than overwriting
    what was read; and ``Sample(path)`` with a project's model constructs that
    model instead. Only a metaclass runs at both moments.
    """

    def __call__(cls, *args, **kwargs):
        if cls is Sample:
            path, name, model = _arguments(*args, **kwargs)
            chosen = _native._model_for(path, model)
            if chosen is not None:
                sample = chosen(path)
                sample._declare_name(name)
                return sample
        sample = super().__call__(*args, **kwargs)
        sample._load(args, kwargs)
        return sample

    def __getattr__(cls, name):
        # `name=None`: the message names the replacement, and Python adding a
        # nearest name of its own wrote a second suggestion after it.
        raise AttributeError(_native._missing_class_member(cls, name), name=None)


class Sample(_native.Sample, metaclass=_Loading):
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

    @classmethod
    def new(cls, path, name=None, model=None):
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
        _native._check_new(path)
        chosen = cls
        if cls is Sample:
            chosen = _native._model_for(path, model) or Sample
        sample = chosen()
        sample._create_at(path)
        # Named by its file, and writing no `name:` unless one is given.
        if name is not None:
            sample._declare_name(name)
        return sample


_native._register(Sample, _bind)
