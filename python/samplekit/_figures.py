"""Figures, drawn by matplotlib.

``@figure`` marks a model's method as a figure — an instance method draws one
sample, a class method a collection — and ``plot`` draws it, or a figure the
project declares, or two axes given by hand. SampleKit makes the figure and its
axes; the method only draws, so that the choice between a window and a file,
and the refusal where no window can be seen, are made here and nowhere else.
"""

import math
import os
import pathlib
import re
import sys
import warnings

# A KeyError that prints its message on its lines rather than as a quoted key.
from samplekit._native import _KeyError as LookupFailed

__all__ = ["figure", "plot"]

KINDS = ("scatter", "line", "step", "bar", "box")
LEGENDS = ("best", "outside", "none", "upper right", "upper left", "lower left",
           "lower right", "right", "center left", "center right", "lower center",
           "upper center")
# More groups than this are read as a colour scale when numeric, and refused as
# text: ten colours repeat, and a legend of more reads as nothing.
MOST_GROUPS = 10


class Figure:
    """A model's method marked ``@sk.figure``. It still behaves as the method
    it marks: ``brew.attenuation(ax)`` draws into ``ax`` as written."""

    def __init__(self, method, subplots, figsize=None):
        if subplots is not None and (
            len(subplots) != 2 or not all(isinstance(n, int) and n > 0 for n in subplots)
        ):
            raise ValueError(f"subplots is (rows, columns), two positive integers: {subplots!r}")
        if isinstance(method, staticmethod):
            raise TypeError(
                "a figure draws a sample or a collection: mark an instance method, or a "
                "class method (@sk.figure above @classmethod), not a static method"
            )
        self.method = method
        self.subplots = tuple(subplots) if subplots is not None else None
        self.figsize = _checked_axes({"figsize": figsize}).get("figsize")
        self.collection = isinstance(method, classmethod)
        self.name = getattr(method, "__name__", None) or getattr(
            getattr(method, "__func__", None), "__name__", None
        )
        self.__doc__ = getattr(method, "__doc__", None)

    def __set_name__(self, owner, name):
        self.name = name

    def __get__(self, instance, owner=None):
        return self.method.__get__(instance, owner)


def figure(method=None, *, subplots=None, figsize=None):
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
    if method is None:
        return lambda marked: Figure(marked, subplots, figsize)
    return Figure(method, subplots, figsize)


def figures_of(cls):
    """The figures a model declares, by name, a subclass's over its base's: a
    subclass that redefines the name as something else takes the figure away."""
    found = {}
    for klass in reversed(cls.__mro__):
        for name, value in vars(klass).items():
            if isinstance(value, Figure):
                found[name] = value
            elif isinstance(value, classmethod) and isinstance(value.__func__, Figure):
                raise TypeError(
                    f"{klass.__name__}.{name}: @sk.figure goes above @classmethod, "
                    "not beneath it"
                )
            elif name in found:
                del found[name]
    return found


class Declared:
    """What ``[figure.*]`` holds, or axes given to ``plot`` by hand."""

    def __init__(self, name, kind, x, y, group=None, query=None, title=None,
                 x_label=None, y_label=None, style=None, axes=None):
        if kind not in KINDS:
            raise ValueError(f"'{kind}' is not a figure kind: scatter, line, step, bar or box")
        self.name, self.kind, self.x, self.y = name, kind, x, y
        self.group, self.query = _group_text(group), query
        self.title, self.x_label, self.y_label, self.style = title, x_label, y_label, style
        self.axes = _checked_axes(axes or {})

    def overridden(self, title=None, x_label=None, y_label=None, style=None, group=None,
                   kind=None, axes=None):
        """The same, with what was given here replacing what was declared — an
        empty title or label included, which draws none."""
        merged = dict(self.axes)
        merged.update({key: value for key, value in (axes or {}).items() if value is not None})

        def given(new, old):
            return old if new is None else new

        return Declared(self.name, given(kind, self.kind), self.x, self.y,
                        given(group, self.group), self.query, given(title, self.title),
                        given(x_label, self.x_label), given(y_label, self.y_label),
                        given(style, self.style), merged)


def _group_text(group):
    """A group as one text, its fields separated by commas as ``--group
    style,yeast`` writes them: a field, several in a list, or ``None``. An
    empty one groups nothing, replacing a figure's own."""
    if group is None or group == "":
        return group
    if isinstance(group, (list, tuple)):
        fields = list(group)
    elif isinstance(group, str):
        fields = group.split(",")
    else:
        raise TypeError(f"group is a field, or several in a list: {group!r}")
    if not all(isinstance(field, str) for field in fields):
        raise TypeError(f"group is a field, or several in a list: {group!r}")
    fields = [field.strip() for field in fields]
    if not fields or not all(fields):
        raise ValueError(f"group names a field, or several separated by commas: {group!r}")
    return ",".join(fields)


def _fields_of(group):
    """The fields a group names: one, or several whose combinations are the
    groups."""
    return group.split(",") if group else []


# Sizes are taken in centimetres and given to matplotlib in inches.
CM_PER_INCH = 2.54

AXIS_KEYS = ("x_limits", "y_limits", "x_scale", "y_scale", "aspect", "legend", "figsize")


def _checked_axes(axes):
    """Limits, scales and aspect, checked before anything is drawn."""
    checked = {}
    for key, value in axes.items():
        if value is None:
            continue
        if key in ("x_limits", "y_limits"):
            if not isinstance(value, (list, tuple)) or len(value) != 2:
                raise ValueError(
                    f"{key} is two bounds, (low, high), a free one None or \"auto\": {value!r}"
                )
            # "auto" as `.samplekitrc` and the command line write a free end.
            value = tuple(None if bound == "auto" else bound for bound in value)
            for bound in value:
                if bound is not None and (
                    isinstance(bound, bool)
                    or not isinstance(bound, (int, float))
                    or not math.isfinite(bound)
                ):
                    raise ValueError(
                        f"{key} bounds are finite numbers, or None or \"auto\" for a free "
                        f"one: {value!r}"
                    )
            if value[0] is not None and value[0] == value[1]:
                raise ValueError(f"{key} is a range, and {value[0]:g} to {value[1]:g} is none")
            checked[key] = tuple(value)
        elif key in ("x_scale", "y_scale"):
            if value not in ("linear", "log", "symlog"):
                raise ValueError(f"{key} is linear, log or symlog, not {value!r}")
            checked[key] = value
        elif key == "aspect":
            if value not in ("equal", "auto"):
                raise ValueError(f"aspect is equal or auto, not {value!r}")
            checked[key] = value
        elif key == "legend":
            if value not in LEGENDS:
                raise ValueError(f"legend is one of {', '.join(LEGENDS)}, not {value!r}")
            checked[key] = value
        elif key == "figsize":
            if (
                not isinstance(value, (list, tuple))
                or len(value) != 2
                or not all(isinstance(side, (int, float)) and not isinstance(side, bool)
                           and math.isfinite(side) and side > 0 for side in value)
            ):
                raise ValueError(
                    f"figsize is a width and a height in centimetres, both positive: {value!r}"
                )
            checked[key] = tuple(float(side) for side in value)
        else:
            raise TypeError(f"{key} is not an axis setting: {', '.join(AXIS_KEYS)}")
    return checked


def plot(target, figure=None, *, x=None, y=None, group=None, kind=None,
         title=None, x_label=None, y_label=None, style=None,
         x_limits=None, y_limits=None, x_scale=None, y_scale=None, aspect=None,
         legend=None, figsize=None, output=None, overwrite=False, project_style=True):
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
    samples = _as_list(target)
    if not len(samples):
        # A figure of nothing is refused, as the command line refuses it : an
        # empty file, or a window of empty axes, reads as data.
        raise _in_the_data(ValueError("no sample to draw: the selection is empty"))
    axes = {"x_limits": x_limits, "y_limits": y_limits, "x_scale": x_scale,
            "y_scale": y_scale, "aspect": aspect, "legend": legend, "figsize": figsize}
    if figure is None:
        if x is None or y is None:
            raise TypeError("plot draws a figure by name, or axes x= and y=")
        chosen = Declared(None, kind or "scatter", x, y, group, None, title, x_label,
                          y_label, style, axes)
    else:
        if x is not None or y is not None:
            raise TypeError(
                "a named figure draws its own axes: pass the name, or x= and y=, not both"
            )
        chosen = _find(samples, figure)
        if isinstance(chosen, Declared):
            chosen = chosen.overridden(title, x_label, y_label, style, group, kind, axes)
        else:
            refused = [name for name, given in (
                ("x_label", x_label), ("y_label", y_label), ("style", style),
                ("group", group), ("kind", kind), *axes.items())
                if given is not None and name != "figsize"]
            if refused:
                raise TypeError(
                    f"'{figure}' is the model's, and draws its own axes: "
                    f"{', '.join(refused)} {'changes' if len(refused) == 1 else 'change'} "
                    "a declared figure, or axes given with x= and y="
                )
    if isinstance(chosen, Declared) and chosen.style is None:
        chosen.style = _default_style(samples)

    per_sample = output is not None and "{name}" in str(output)
    if output is not None:
        output = pathlib.Path(output)
        refusal = output_refusal(output)
        if refusal:
            raise ValueError(f"{output}: {refusal}")
        if per_sample and (isinstance(chosen, Declared) or chosen.collection):
            raise ValueError(
                f"{output}: {{name}} writes a file per sample, and this figure draws "
                "every sample in one: name one file"
            )
        if not per_sample and output.exists() and not overwrite:
            raise FileExistsError(
                f"{output} already exists, and nothing was written: overwrite=True replaces it"
            )
    else:
        refusal = window_refusal()
        if refusal:
            raise RuntimeError(refusal)

    import matplotlib

    _said_across(samples)
    settings = _project_settings(samples) if project_style else {}
    with matplotlib.rc_context(settings):
        return _draw(samples, chosen, output, title, overwrite, figsize)


def _default_style(samples):
    """``[render] figure_style``, the style a figure names none reads in."""
    project = _project_of(samples)
    if project is None:
        return None
    return project.render.figure_style


def output_refusal(output):
    """Why matplotlib would not write the figure at ``output`` as named, or
    ``None``: a path without an extension it would give one of its own, and
    one it does not write it cannot."""
    suffix = output.suffix.lower().lstrip(".")
    if not suffix:
        return (
            "a figure's file says its format by its extension — .pdf, .svg, .png — "
            "and this one has none"
        )
    if suffix not in FORMATS:
        return f"matplotlib writes no '.{suffix}': {', '.join('.' + name for name in FORMATS)}"
    return None


# The formats a figure is written in, the command line's list: a script and
# `samplekit plot` accept the same files, whatever else the installed
# matplotlib could write.
FORMATS = ("eps", "jpeg", "jpg", "pdf", "pgf", "png", "ps", "raw", "rgba", "svg", "svgz",
           "tif", "tiff", "webp")


class FigureRaised(Exception):
    """Marks an exception as the model's figure's own: raised by its code,
    not by SampleKit deciding what to draw."""


def _called(method, *arguments):
    """Calls a model's figure method, marking what it raises as its own, and
    refusing a figure it drew into with ``plt.*`` instead of the axes it was
    given — a figure SampleKit never writes."""
    pyplot = sys.modules.get("matplotlib.pyplot")
    before = set(pyplot.get_fignums()) if pyplot else set()
    try:
        method(*arguments)
    except Exception as error:
        error._samplekit_figure = True
        raise
    pyplot = sys.modules.get("matplotlib.pyplot")
    stray = set(pyplot.get_fignums()) - before if pyplot else set()
    if stray:
        for number in stray:
            pyplot.close(number)
        error = RuntimeError(
            f"{getattr(method, '__qualname__', method)} drew into a figure of pyplot's "
            "own (plt.plot, plt.xlabel…), not into the axes it was given: draw on "
            "them — ax.plot, ax.set_xlabel"
        )
        error._samplekit_figure = True
        raise error


def raised_by_the_figure(error):
    """Whether ``error`` came out of the model's figure code."""
    return getattr(error, "_samplekit_figure", False)


def _in_the_data(error):
    """Marks a refusal as the samples': their values or their models disagree,
    and the request itself was sound. The command line exits 2 for it, as the
    table does for a column of two units, and 1 for a name or an option."""
    error._samplekit_data = True
    return error


def raised_by_the_data(error):
    """Whether ``error`` says something of the samples rather than of what was
    asked: marked so here, or raised by matplotlib itself over the values it
    was given to draw."""
    if getattr(error, "_samplekit_data", False):
        return True
    trace = error.__traceback__
    while trace is not None and trace.tb_next is not None:
        trace = trace.tb_next
    if trace is None:
        return False
    parts = pathlib.Path(trace.tb_frame.f_code.co_filename).parts
    return "matplotlib" in parts or "numpy" in parts


# What the history says wrote a figure's file: the command line when `samplekit
# plot` draws it, else Python, naming the figure.
SAID = None
# Whether `samplekit plot` is drawing: a refusal then names the command's
# options, `-o`, and a script's otherwise, `output=`.
COMMAND_LINE = False


def _option(command_line, python):
    """An option as the one reading the message writes it."""
    return command_line if COMMAND_LINE else python


# The snapshot a figure carries where the command line names it, drawing from
# the project as it was, with no history written here.
CARRY = None

# What a figure carries in its metadata before its snapshot's id, which
# version_control reads back.
CARRIED = "samplekit snapshot "


def _carrying(path, snapshot):
    """The ``savefig`` keywords that write ``snapshot`` into the file's
    metadata, where its format has a place for it."""
    if snapshot is None:
        return {}
    key = {".pdf": "Subject", ".png": "Description", ".svg": "Description"}.get(
        pathlib.Path(str(path)).suffix.lower())
    return {"metadata": {key: CARRIED + snapshot}} if key else {}


def _kept(samples, chosen, files, per_sample, drawn):
    """Writes each figure, tied in the history to the snapshot it was made
    from: a snapshot of each project its samples belong to first, then
    a tag naming the file's hash. A history not kept is a warning."""
    from samplekit import _native

    paths = [str(sample.path) for sample in samples if getattr(sample, "path", None)]
    # A script writing a figure is a snapshot of its own where it has none
    # yet; the command line's drawing leaves the history to its command.
    written = None if COMMAND_LINE else [str(path) for path in files]
    kept = _native._history_before_output(paths, written) if paths else []
    # What ran, as the script's own snapshot names it, and what was drawn:
    # the figure's name, or its axes as `samplekit plot` says them.
    name = getattr(chosen, "name", None)
    drawn_as = name or f"{chosen.y} against {chosen.x}"
    said = SAID or f"{_native._script_said()} · plot {drawn_as}"
    carried = CARRY or (kept[0][1] if kept else None)
    for at, (figure_, path) in enumerate(zip(drawn, files)):
        # An output's folder is made, as every destination's is.
        pathlib.Path(path).parent.mkdir(parents=True, exist_ok=True)
        figure_.savefig(path, **_carrying(path, carried))
        # A figure of one sample, per file, is made from that sample alone.
        made_from = [paths[at]] if per_sample and at < len(paths) else paths
        for root, snapshot in kept:
            _native._history_tag_output(root, snapshot, str(path), said, made_from)


def _draw(samples, chosen, output, title, overwrite=False, figsize=None):
    drawn = []
    files = []
    try:
        if isinstance(chosen, Declared):
            figure_, axes = _new(output, None, drawn, chosen.axes.get("figsize"))
            _draw_declared(samples, chosen, axes)
            files.append(output)
        elif chosen.collection:
            model = _model_of(samples, chosen)
            figure_, axes = _new(output, chosen.subplots, drawn, figsize or chosen.figsize)
            _called(chosen.method.__get__(None, model), samples, axes)
            if title:
                figure_.suptitle(title)
            files.append(output)
        else:
            methods = [_method_of(sample, chosen) for sample in samples]
            per_sample = output is not None and "{name}" in str(output)
            if output is not None and len(samples) != 1 and not per_sample:
                raise ValueError(
                    f"'{chosen.name}' draws one sample, and a file holds one figure: "
                    f"name one sample of the {len(samples)}, or write a file per sample "
                    "with {name} in the file's name"
                )
            for sample, method in zip(samples, methods):
                figure_, axes = _new(output, method.subplots, drawn,
                                     figsize or method.figsize)
                _called(method.method.__get__(sample, type(sample)), axes)
                # Its sample named, so that files written one per sample are
                # told apart once printed.
                if title or len(samples) > 1:
                    figure_.suptitle(title or sample.name or "")
                files.append(_file_of(output, sample) if per_sample else output)
            if per_sample:
                _check_the_files(files, overwrite)

        if output is not None:
            per_sample = "{name}" in str(output) and len(files) == len(samples) > 1
            _kept(samples, chosen, files, per_sample, drawn)
        else:
            import matplotlib.pyplot as plt

            plt.show()
    except BaseException:
        # A figure half drawn is closed, so that pyplot does not show it at
        # the script's next plt.show().
        pyplot = sys.modules.get("matplotlib.pyplot")
        if pyplot is not None:
            for figure_ in drawn:
                pyplot.close(figure_)
        raise
    return drawn


def _file_of(output, sample):
    """``output`` with ``{name}`` replaced by the sample's name, or its file's."""
    name = sample.name or (pathlib.Path(sample.path).stem if sample.path else None)
    if not name:
        raise ValueError(
            f"{output}: {{name}} names a file per sample, and a sample here has neither "
            "a name nor a file"
        )
    return pathlib.Path(str(output).replace("{name}", name))


def _check_the_files(files, overwrite):
    """A file per sample: two samples of one name would write one file, and an
    existing one is replaced only when asked."""
    seen = set()
    for path in files:
        if path in seen:
            raise ValueError(f"{path}: two samples would write this one file")
        seen.add(path)
        if path.exists() and not overwrite:
            raise FileExistsError(
                f"{path} already exists, and nothing was written: overwrite=True replaces it"
            )


# ----------------------------------------------------------------- choosing


def _project_settings(samples):
    """``[matplotlib]``, checked by matplotlib's own validators: a name it does
    not have or a value it refuses is refused here, naming it, where
    matplotlib alone warns and carries on."""
    import difflib

    import matplotlib

    if not len(samples):
        return {}
    project = _project_of(samples)
    if project is None:
        return {}
    written = project.matplotlib
    settings = {}
    for name, value in written.items():
        if name in ("backend", "interactive"):
            raise ValueError(
                f"[matplotlib] {name} is not a figure's: SampleKit chooses between a "
                "window and a file, and MPLBACKEND chooses the window's backend"
            )
        if name not in matplotlib.rcParams:
            near = difflib.get_close_matches(name, list(matplotlib.rcParams), n=1)
            raise ValueError(
                f"[matplotlib] {name} is not a matplotlib setting"
                + (f"\n  did you mean: {near[0]}?" if near else "")
            )
        # In centimetres, as every size SampleKit takes. Written as text too,
        # "15, 10", which matplotlib reads alike.
        if name == "figure.figsize" and isinstance(value, str):
            try:
                value = [float(side) for side in value.split(",")]
            except ValueError:
                pass
        if name == "figure.figsize" and isinstance(value, (list, tuple)):
            value = [side / CM_PER_INCH if isinstance(side, (int, float)) else side
                     for side in value]
        try:
            settings[name] = matplotlib.rcParams.validate[name](value)
        except (ValueError, TypeError) as error:
            raise ValueError(f"[matplotlib] {name} = {value!r}: {error}") from None
    return settings


def _as_list(target):
    import samplekit

    if isinstance(target, samplekit.SampleList):
        return target
    if isinstance(target, samplekit.Sample):
        return samplekit.SampleList([target])
    return samplekit.SampleList(list(target))


def _find(samples, name):
    """The model's figure of that name, or the project's; both is ambiguous,
    and neither names what exists."""
    ours = {}
    for cls in {type(sample) for sample in samples}:
        ours.update(figures_of(cls))
    declared = {}
    across = None
    try:
        figures = samples.project.figures
        declared = {name: figures[name] for name in figures}
    except ValueError:
        # Several projects: a name each declares alike is one figure; declared
        # differently, or by some only, it is refused naming them.
        projects = [project for project in _projects_of(samples) if project is not None]
        held = [(project, project.figures[name] if name in project.figures else None)
                for project in projects]
        shapes = {repr(_figure_shape(figure)) for _, figure in held if figure is not None}
        if all(figure is not None for _, figure in held) and len(shapes) == 1:
            declared = {name: held[0][1]}
        elif any(figure is not None for _, figure in held):
            across = "; ".join(
                f"{project.root} {'declares it differently' if figure else 'does not declare it'}"
                for project, figure in held[1:]
                if figure is None or repr(_figure_shape(figure)) != repr(_figure_shape(held[0][1]))
            )
            across = f"'{name}' is declared differently across these projects: {across}"
    if name in ours and name in declared:
        raise ValueError(
            f"'{name}' is both a figure of the model and one .samplekitrc declares: "
            "rename one of them"
        )
    if name in ours:
        return ours[name]
    if name in declared:
        held = declared[name]
        return Declared(name, held.kind, held.x, held.y, held.group, held.query,
                        held.title, held.x_label, held.y_label, held.style,
                        {key: getattr(held, key) for key in AXIS_KEYS})
    if across is not None:
        raise _in_the_data(ValueError(
            f"{across}\n  declare it alike in each, or draw each project's samples on "
            "their own"
        ))
    import difflib

    # In the order they are declared, the project's then the model's, as the
    # command line lists them.
    known = list(declared) + [figure for figure in ours if figure not in declared]
    near = difflib.get_close_matches(name, known, n=1)
    raise LookupFailed(
        f"no figure named '{name}'"
        + (f"\n  did you mean: {near[0]}?" if near else "")
        + (f"\n  the figures: {', '.join(known)}" if known else ": none is declared")
    )


def _method_of(sample, chosen):
    """The figure ``chosen`` names, as the sample's own model defines it — a
    subclass's over its base's — refused for a sample whose model has none."""
    found = figures_of(type(sample)).get(chosen.name)
    if found is None:
        raise ValueError(
            f"{sample.name or sample.path}: its model declares no figure '{chosen.name}'"
        )
    return found


def _model_of(samples, chosen):
    """The class a collection figure is called on: the most derived one every
    sample is an instance of, which must draw the figure as each sample's own
    model does."""
    methods = {id(_method_of(sample, chosen)) for sample in samples}
    if len(methods) > 1:
        raise _in_the_data(ValueError(
            f"these samples' models draw '{chosen.name}' differently: select the "
            "samples of one model"
        ))
    first = type(samples[0])
    for cls in first.__mro__:
        if all(isinstance(sample, cls) for sample in samples):
            return cls
    return first


# ----------------------------------------------------------------- the window


def window_refusal():
    """Why a window cannot be seen here, or ``None``: over SSH without a
    display, or where matplotlib has no interactive backend."""
    over_ssh = os.environ.get("SSH_CONNECTION") or os.environ.get("SSH_TTY")
    display = (
        os.environ.get("DISPLAY")
        or os.environ.get("WAYLAND_DISPLAY")
        or sys.platform in ("darwin", "win32")
    )
    if over_ssh and not display:
        return (
            "a figure opens in a window, and this SSH session has no display to open "
            f"it on: write it to a file instead ({_option('-o figure.pdf', 'output=')})"
        )
    import matplotlib
    import matplotlib.pyplot  # noqa: F401 — resolves the automatic backend

    backend = matplotlib.get_backend().lower()
    if backend.startswith("module://") or "inline" in backend:
        return None
    try:
        from matplotlib.backends.registry import BackendFilter, backend_registry

        interactive = {
            name.lower() for name in backend_registry.list_builtin(BackendFilter.INTERACTIVE)
        }
    except ImportError:  # matplotlib before 3.9
        from matplotlib import rcsetup

        interactive = {name.lower() for name in getattr(rcsetup, "interactive_bk", [])}
    if backend not in interactive:
        if os.environ.get("MPLBACKEND"):
            return (
                f"MPLBACKEND={os.environ['MPLBACKEND']} chooses a backend that opens no "
                f"window: write the figure to a file ({_option('-o figure.pdf', 'output=')}), "
                "or unset MPLBACKEND"
            )
        return (
            f"matplotlib has no interactive backend here (it chose '{backend}'), so a "
            "window would show nothing: install tkinter or PyQt, or write the figure "
            f"to a file ({_option('-o figure.pdf', 'output=')})"
        )
    return None


def _new(output, subplots, drawn, figsize=None):
    """A figure and its axes, kept in ``drawn``: pyplot's for a window, a
    figure of its own for a file, so that writing one never changes a script's
    pyplot state. Laid out by matplotlib, so that a long tick label never
    covers an axis's label; ``figsize`` in centimetres, converted here
    for matplotlib, which takes inches; `[matplotlib]`'s otherwise."""
    rows, columns = subplots or (1, 1)
    if figsize is not None:
        # Sizes are centimetres: one written in inches, or a slip, would draw a
        # thumbnail or a picture of a billion pixels without a word.
        if any(side < 2 or side > 150 for side in figsize):
            # At the script's line, as every warning of the package is: a
            # fixed level landed in the package when the call came through it.
            _warn(f"a figure of {figsize[0]:g} by {figsize[1]:g} cm: sizes are in centimetres")
        figsize = tuple(side / CM_PER_INCH for side in figsize)
    if output is None:
        import matplotlib.pyplot as plt

        figure_, axes = plt.subplots(rows, columns, layout="constrained", figsize=figsize)
    else:
        from matplotlib.figure import Figure as MatplotlibFigure

        figure_ = MatplotlibFigure(layout="constrained", figsize=figsize)
        axes = figure_.subplots(rows, columns)
    drawn.append(figure_)
    return figure_, axes


# ----------------------------------------------------------------- declared


def _draw_declared(samples, declared, ax):
    """A declared figure, or axes given, drawn into ``ax``; what it left out,
    and what it read not current, said once it is drawn, one summary each —
    before, a refusal would have followed a warning about a figure never
    drawn."""
    global _HELD
    held = _HELD = _NotCurrent()
    try:
        omitted = _drawn_declared(samples, declared, ax)
    finally:
        _HELD = None
    omitted.say()
    held.say(omitted)


def _drawn_declared(samples, declared, ax):
    if declared.query:
        given = len(samples)
        samples = samples.query(declared.query)
        if not len(samples):
            raise ValueError(
                f"the figure's query '{declared.query}' selects none of the {given} "
                "samples: nothing to draw"
            )
    if declared.group:
        _check_group(samples, declared.group)
    x_table, y_table = _table_of(samples, declared.x), _table_of(samples, declared.y)
    if (x_table is None) != (y_table is None):
        column = declared.x if x_table is not None else declared.y
        raise ValueError(
            f"'{column}' is a table's column and the other axis one number per sample: "
            "draw two columns of one table, or two fields"
        )
    project = _project_of(samples)
    groups = _Groups(samples, declared, project)
    if x_table is not None:
        if declared.kind in ("bar", "box"):
            raise ValueError(
                f"a {declared.kind} is drawn over one number per sample, and "
                f"'{declared.x}' and '{declared.y}' are a table's columns: a curve per "
                "sample is a scatter, a line or a step"
            )
        x_read, y_read, omitted = _curves(samples, declared, ax, groups)
    elif declared.kind == "box":
        x_read, y_read, omitted = _boxes(samples, declared, ax)
    else:
        x_read, y_read, omitted = _points(samples, declared, ax, groups)
    x_text = (_axis(project, declared.x, x_read, declared.style, _row_of(samples, declared.x))
              if declared.x_label is None else declared.x_label)
    y_text = (_axis(project, declared.y, y_read, declared.style, _row_of(samples, declared.y))
              if declared.y_label is None else declared.y_label)
    # No title unless one is given: a figure in a report takes its caption
    # there, and a declaration's name is no sentence.
    title = declared.title
    _typeset(x_text, declared.style, "x label", declared.x_label is not None)
    _typeset(y_text, declared.style, "y label", declared.y_label is not None)
    _typeset(title, declared.style, "title", True)
    ax.set_xlabel(x_text)
    ax.set_ylabel(y_text)
    _set_axes(ax, declared.axes)
    if title:
        ax.set_title(title)
    groups.finish(ax)
    _place_legend(ax, declared.axes.get("legend"), groups.title)
    return omitted


def _place_legend(ax, place, title):
    """``best`` by default, beside the axes when more than six series would
    cover the data, where ``legend`` says otherwise, or none."""
    handles = ax.get_legend_handles_labels()[0]
    if not handles or place == "none":
        return
    if place is None:
        place = "outside" if len(handles) > 6 else "best"
    if place == "outside":
        ax.legend(loc="upper left", bbox_to_anchor=(1.02, 1.0), fontsize="small",
                  title=title)
    else:
        ax.legend(loc=place, title=title)


class _Groups:
    """What ``group`` makes of a figure: its values in the
    field's declared precision, a legend title in the figure's style, and a
    colour for each — a colour scale when a numeric group has more values than
    ten colours tell apart, refused for text. Over several fields, a group is
    a combination of their values, named field by field: a category, never a
    scale."""

    def __init__(self, samples, declared, project):
        self.field = declared.group
        self.project = project
        self.style = declared.style
        self.title = None
        self.scale = None
        self.ordered = None
        self.colours = {}
        self.parts = None
        if not self.field:
            return
        fields = _fields_of(self.field)
        keys = {_group_of(sample, self.field) for sample in samples}
        self.title = ", ".join(self._title_of(samples, field) for field in fields)
        _typeset(self.title, self.style, "legend title", False)
        if len(fields) > 1:
            if len(keys) > MOST_GROUPS:
                raise _in_the_data(ValueError(
                    f"'{', '.join(fields)}' make {len(keys)} groups together, and more than "
                    f"{MOST_GROUPS} colours repeat: group by fewer fields, or narrow the "
                    "selection"
                ))
            self.parts = [
                self._texts(field, list({key[at] for key in keys if _is_number(key[at])}))
                for at, field in enumerate(fields)
            ]
            self.texts = {}
            return
        values = [key for key in keys if _is_number(key)]
        # A sample holding no value, or one not applicable, is no value of the
        # field: a numeric group with them is still numeric, and they are
        # drawn grey, named in the legend.
        valued = len(keys - set(NO_VALUE))
        if len(keys) > MOST_GROUPS:
            if len(values) < valued or not values:
                raise _in_the_data(ValueError(
                    f"'{self.field}' makes {len(keys)} groups, and more than "
                    f"{MOST_GROUPS} colours repeat: group by a numeric field for a colour "
                    "scale, or narrow the selection"
                ))
            import matplotlib

            self.scale = (matplotlib.colors.Normalize(min(values), max(values)),
                          matplotlib.colormaps["viridis"])
        elif len(values) >= 2 and len(values) == valued:
            # Numbers keep their order in their colours, each still named in
            # the legend.
            import matplotlib

            self.ordered = (matplotlib.colors.Normalize(min(values), max(values)),
                            matplotlib.colormaps["viridis"])
        self.texts = self._texts(self.field, values) if self.scale is None else {}

    def _title_of(self, samples, field):
        """One field's part of the legend's title: its symbol and unit in the
        figure's style, as an axis is labelled."""
        if field == "tags":
            return field
        units = _Units(field, "group")
        for sample in samples:
            try:
                held = _read(sample, field)
            except KeyError:
                continue
            reading = _reading_of(sample, field) if not hasattr(held, "unit") else (
                held.unit, held.symbol)
            units.add(sample, reading)
        return _axis(self.project, field, units.reading, self.style, _row_of(samples, field))

    def _texts(self, field, values):
        """Each numeric value as the legend writes it: at the field's declared
        precision, unless two values would read alike there — 66 and 66 — and
        then with the fewest more digits that tell every one apart."""
        texts = {}
        for value in values:
            text = None
            if self.project is not None:
                try:
                    text = self.project._value_text(field, value)
                except (TypeError, ValueError):
                    pass
            texts[value] = text if text is not None else str(value)
        if len(set(texts.values())) == len(texts):
            return texts
        for digits in range(0, 16):
            spelled = {value: f"{value:.{digits}f}" for value in values}
            if len(set(spelled.values())) == len(spelled):
                return spelled
        return {value: repr(value) for value in values}

    def label(self, key):
        """A group's value as the legend writes it: none for a value a colour
        bar reads, and the samples without a value named; a combination, each
        field's value in turn."""
        if key is None:
            return None
        if isinstance(key, tuple):
            return ", ".join(
                part if part in NO_VALUE
                else texts.get(part, str(part)) if _is_number(part)
                else str(part)
                for part, texts in zip(key, self.parts)
            )
        if key in NO_VALUE:
            return key
        if self.scale is not None:
            return None
        if _is_number(key):
            return self.texts.get(key, str(key))
        return str(key)

    def colour(self, key):
        if self.scale is not None and _is_number(key):
            norm, colours = self.scale
            return colours(norm(key))
        if self.ordered is not None and _is_number(key):
            norm, colours = self.ordered
            # Short of the scale's pale end, which a white page loses.
            return colours(0.85 * norm(key))
        if self.scale is not None or self.ordered is not None:
            # No value, beside values a scale colours: a colour of the scale
            # would read as one of them.
            return "0.6"
        return None

    def finish(self, ax):
        """The colour bar a scale reads by, labelled as the group's axis."""
        if self.scale is None:
            return
        import matplotlib

        norm, colours = self.scale
        bar = ax.figure.colorbar(matplotlib.cm.ScalarMappable(norm=norm, cmap=colours), ax=ax)
        bar.set_label(self.title)
        self.title = None


def _is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


class _Omitted:
    """What a figure leaves out, and why, said once it is drawn."""

    def __init__(self, total):
        self.total = total
        self.reasons = {}
        # By the name a read's warning gives them.
        self.names = set()

    def add(self, sample, reason):
        self.reasons.setdefault(reason, set()).add(id(sample))
        path = getattr(sample, "path", None)
        name = sample.name or (pathlib.Path(path).stem if path else None)
        if name:
            self.names.add(name)

    def say(self):
        for reason, samples in self.reasons.items():
            _warn(f"{len(samples)} of {self.total} samples not drawn: {reason}")


def _nothing_drawn(samples, omitted, held):
    """The refusal of a figure none of whose samples could be drawn: a fact
    of the samples, not a misspelt name, which a field no sample knows is.
    ``held`` is what a sample would have had to hold."""
    reasons = list(omitted.reasons)
    absent = all(reason.startswith("no ") for reason in reasons)
    if len(samples) == 1:
        said = ("the one sample selected holds " + " and ".join(reasons) if absent and reasons
                else "the one sample selected is not drawn: " + "; ".join(reasons))
    else:
        said = (f"none of the {len(samples)} samples holds {held}" if absent
                else f"none of the {len(samples)} samples can be drawn: " + "; ".join(reasons))
    return _in_the_data(ValueError(f"{said}: nothing to draw"))


def _on_the_axis(value, scale):
    """Whether a log axis can show ``value``."""
    return scale != "log" or value is None or not _is_number(value) or value > 0


def _points(samples, declared, ax, groups):
    """A point per sample, a series per value of ``group``."""
    series = {}
    units = (_Units(declared.x), _Units(declared.y))
    missing = []
    omitted = _Omitted(len(samples))
    for sample in samples:
        read_x = _scalar(sample, declared.x, missing, text=declared.kind == "bar")
        read_y = _scalar(sample, declared.y, missing)
        if read_x is None or read_y is None:
            absent = declared.x if read_x is None else declared.y
            omitted.add(sample, f"no {absent}")
            continue
        for axis, value in (("x", read_x[0]), ("y", read_y[0])):
            if not _on_the_axis(value, declared.axes.get(f"{axis}_scale")):
                omitted.add(sample, f"{getattr(declared, axis)} at or below zero on a log axis")
                break
        else:
            units[0].add(sample, read_x[2])
            units[1].add(sample, read_y[2])
            key = _group_of(sample, declared.group) if declared.group else None
            entry = series.setdefault(key, ([], [], [], [], []))
            entry[0].append(read_x[0])
            entry[1].append(read_y[0])
            entry[2].append(read_x[1])
            entry[3].append(read_y[1])
            entry[4].append(sample)
    if not series:
        # A field no sample knows is a misspelling, said with its suggestion;
        # one some samples lack is an ordinary gap.
        if missing and len(missing) >= len(samples):
            raise missing[0]
        raise _nothing_drawn(samples, omitted, f"both '{declared.x}' and '{declared.y}'")
    keys = sorted(series, key=_group_order)
    if declared.kind == "bar":
        _bars(ax, declared, [(key, series[key]) for key in keys], groups)
    else:
        for key in keys:
            xs, ys, x_errors, y_errors, _ = series[key]
            if declared.kind in ("line", "step"):
                order = sorted(range(len(xs)), key=lambda at: xs[at])
                xs, ys = [xs[at] for at in order], [ys[at] for at in order]
                x_errors = [x_errors[at] for at in order]
                y_errors = [y_errors[at] for at in order]
            _series(ax, declared.kind, xs, ys, x_errors, y_errors, groups.label(key),
                    groups.colour(key))
    return units[0].reading, units[1].reading, omitted


def _bars(ax, declared, series, groups):
    """Bars at each x, a series' beside another's; two of one series at
    one x would hide one, and are refused. A text x is a category."""
    for key, (xs, _, _, _, samples) in series:
        seen = {}
        for x, sample in zip(xs, samples):
            if x in seen:
                raise _in_the_data(ValueError(
                    f"{_named(seen[x])} and {_named(sample)} are two bars at "
                    f"{declared.x} = {x}"
                    + (f" in the series {groups.label(key) or key}" if key is not None else "")
                    + ", and one would hide the other: group them apart, or draw their "
                    f"spread with {_option('--kind box', 'kind=box')}"
                ))
            seen[x] = sample
    every = [x for _, (xs, *_) in series for x in xs]
    categorical = any(isinstance(x, str) for x in every)
    if categorical:
        order = sorted({str(x) for x in every}, key=_group_order)
        place = {name: at for at, name in enumerate(order)}
        position = lambda x: place[str(x)]  # noqa: E731
    else:
        position = lambda x: x  # noqa: E731
    spots = sorted({position(x) for x in every})
    spacing = min((b - a for a, b in zip(spots, spots[1:])), default=1.0) or 1.0
    width = 0.8 * spacing / len(series)
    for at, (key, (xs, ys, x_errors, y_errors, _)) in enumerate(series):
        shift = (at - (len(series) - 1) / 2) * width
        ax.bar([position(x) + shift for x in xs], ys, width=width,
               xerr=None if categorical else _errors(x_errors), yerr=_errors(y_errors),
               capsize=3, label=groups.label(key), color=groups.colour(key))
    if categorical:
        ax.set_xticks(range(len(order)), order)
        # Many or long names lean, rather than write over each other.
        if len(order) > 6 or max((len(name) for name in order), default=0) > 8:
            ax.tick_params(axis="x", labelrotation=40)
            for label in ax.get_xticklabels():
                label.set_horizontalalignment("right")


def _boxes(samples, declared, ax):
    """The values of ``y`` over the samples sharing a value of ``x``, a box
    each: their spread, which a bar would hide. What a box draws is the
    values; their uncertainties are not a box's to show."""
    if declared.group:
        raise ValueError(
            f"a box plot's boxes are the values of '{declared.x}': "
            f"{_option('--group', 'group=')} has nothing left to split"
        )
    missing = []
    boxes = {}
    units = _Units(declared.y)
    omitted = _Omitted(len(samples))
    for sample in samples:
        key = _group_of(sample, declared.x)
        read_y = _scalar(sample, declared.y, missing)
        if read_y is None or key in NO_VALUE:
            omitted.add(sample, f"no {declared.y if read_y is None else declared.x}")
            continue
        if not _on_the_axis(read_y[0], declared.axes.get("y_scale")):
            omitted.add(sample, f"{declared.y} at or below zero on a log axis")
            continue
        units.add(sample, read_y[2])
        boxes.setdefault(key, []).append(read_y[0])
    if not boxes:
        if missing and len(missing) >= len(samples):
            raise missing[0]
        raise _nothing_drawn(samples, omitted, f"'{declared.y}'")
    keys = sorted(boxes, key=_group_order)
    # Each box with its values drawn over it, and how many beneath it: a box
    # of one sample is a line, which the point and its count explain.
    ax.boxplot(
        [boxes[key] for key in keys],
        tick_labels=[f"{key}\n({len(boxes[key])})" for key in keys],
        medianprops={"color": "0.2"},
    )
    for at, key in enumerate(keys, start=1):
        ax.plot([at] * len(boxes[key]), boxes[key], "o", color="0.35", markersize=3, zorder=3)
    return (None, None), units.reading, omitted


def _set_axes(ax, axes):
    """Scales before limits, so that a log axis is given its bounds as one. A
    bound a log axis cannot take is refused, where matplotlib ignores it with a
    warning."""
    for axis in ("x", "y"):
        if axes.get(f"{axis}_scale") != "log":
            continue
        for bound in axes.get(f"{axis}_limits") or ():
            if bound is not None and bound <= 0:
                raise ValueError(
                    f"{axis}_limits has {bound:g}, and a log scale holds no bound at or "
                    "below zero: give a positive one, or auto"
                )
    for key, setter in (("x_scale", ax.set_xscale), ("y_scale", ax.set_yscale)):
        if key in axes:
            setter(axes[key])
    for axis in ("x", "y"):
        bounds = axes.get(f"{axis}_limits")
        if bounds and None not in bounds and bounds[0] > bounds[1]:
            _warn(
                f"{axis}_limits run from {bounds[0]:g} down to {bounds[1]:g}: the {axis} "
                "axis is drawn reversed"
            )
    if "x_limits" in axes:
        ax.set_xlim(*_free_end(ax, "x", axes["x_limits"], axes.get("x_scale")))
    if "y_limits" in axes:
        ax.set_ylim(*_free_end(ax, "y", axes["y_limits"], axes.get("y_scale")))
    if "aspect" in axes:
        ax.set_aspect(axes["aspect"], adjustable="box")


def _free_end(ax, axis, bounds, scale):
    """Two bounds, a free end placed past the data by matplotlib's margin —
    measured over the range from the fixed end, as autoscaling measures it —
    rather than at the last value, where its marker was cut by the frame."""
    low, high = bounds
    if (low is None) == (high is None):
        return bounds
    interval = ax.dataLim.intervalx if axis == "x" else ax.dataLim.intervaly
    least, most = (float(end) for end in interval)
    fixed = low if low is not None else high
    if not (math.isfinite(least) and math.isfinite(most)):
        return bounds
    log = scale == "log"
    if log and (fixed <= 0 or least <= 0):
        return bounds

    def along(value):
        return math.log10(value) if log else value

    def back(value):
        return 10 ** value if log else value

    margin = ax.margins()[0 if axis == "x" else 1]
    if low is not None:
        # The data's far end from the fixed one, whichever side it lies.
        far = most if along(most) >= along(fixed) else least
    else:
        far = least if along(least) <= along(fixed) else most
    if far == fixed:
        return bounds
    free = back(along(far) + margin * (along(far) - along(fixed)))
    return (fixed, free) if low is not None else (free, fixed)


def _group_order(key):
    """Groups in their values' order: numbers as numbers — 80 before 100 —
    then text, then the samples with none; a combination of several fields
    by its first field's value, then the next."""
    if isinstance(key, tuple):
        return (1, tuple(_group_order(part) for part in key), "")
    if key is None or key in NO_VALUE:
        return (2, NO_VALUE.index(key) if key in NO_VALUE else 0, "")
    if _is_number(key):
        return (0, key, "")
    return (1, 0, str(key))


def _curves(samples, declared, ax, groups):
    """A curve per sample, for two columns of one table, coloured by group."""
    units = (_Units(declared.x), _Units(declared.y))
    drawn = 0
    colours = {}
    omitted = _Omitted(len(samples))
    points = 0
    if declared.group:
        # Drawn group by group, so that the legend reads in the groups' order.
        samples = sorted(samples, key=lambda s: _group_order(_group_of(s, declared.group)))
    # A curve per sample past ten colours: twenty, then a scale, never a
    # colour two curves share.
    palette = _palette(len(samples)) if not declared.group else None
    for sample in samples:
        table_x = _table_in(sample, declared.x)
        table_y = _table_in(sample, declared.y)
        if table_x is None or table_y is None:
            omitted.add(sample, f"no {declared.x if table_x is None else declared.y}")
            continue
        (name_x, table, column_x), (name_y, _, column_y) = table_x, table_y
        if name_x != name_y:
            raise ValueError(
                f"'{declared.x}' and '{declared.y}' are columns of two tables: a curve "
                "is read along one table's rows"
            )
        xs = [_number(sample, declared.x, v) for v in table.values(column_x)]
        ys = [_number(sample, declared.y, v) for v in table.values(column_y)]
        x_errors = table.uncertainties(column_x)
        y_errors = table.uncertainties(column_y)
        keep = [at for at in range(len(xs)) if xs[at] is not None and ys[at] is not None]
        shown = [at for at in keep
                 if _on_the_axis(xs[at], declared.axes.get("x_scale"))
                 and _on_the_axis(ys[at], declared.axes.get("y_scale"))]
        points += len(keep) - len(shown)
        units[0].add(sample, _column_reading(table, column_x))
        units[1].add(sample, _column_reading(table, column_y))
        # With a group, a curve takes its group's colour and the legend names
        # each group once; without, each curve is its sample.
        label, colour = sample.name, (palette[drawn] if palette else None)
        if declared.group:
            key = _group_of(sample, declared.group)
            colour = groups.colour(key)
            if colour is None:
                if key not in colours:
                    colours[key] = f"C{len(colours) % 10}"
                    label = groups.label(key)
                else:
                    label = None
                colour = colours[key]
            else:
                # Named once per group: under a colour bar, only the
                # samples without a value.
                label = groups.label(key) if key not in colours else None
                colours[key] = colour
        _series(
            ax,
            declared.kind,
            [xs[at] for at in shown],
            [ys[at] for at in shown],
            [x_errors[at] for at in shown],
            [y_errors[at] for at in shown],
            label,
            colour,
        )
        drawn += 1
    if not drawn:
        raise _nothing_drawn(samples, omitted,
                             f"the columns '{declared.x}' and '{declared.y}'")
    if points:
        _warn(f"{points} points not drawn: at or below zero on a log axis")
    return units[0].reading, units[1].reading, omitted


def _palette(count):
    """Colours for ``count`` curves, none repeating: matplotlib's ten where
    they are enough, twenty, then evenly along a scale."""
    import matplotlib

    if count <= 10:
        return None
    if count <= 20:
        return [matplotlib.colormaps["tab20"](at) for at in range(count)]
    scale = matplotlib.colormaps["viridis"]
    return [scale(at / (count - 1)) for at in range(count)]


def _series(ax, kind, xs, ys, x_errors, y_errors, label, colour=None):
    """One series in its kind, with error bars wherever an uncertainty is."""
    xerr = _errors(x_errors)
    yerr = _errors(y_errors)
    style = {"scatter": "o", "line": "o-", "step": "o"}[kind]
    if kind == "step":
        (line,) = ax.step(xs, ys, where="mid", label=label, color=colour)
        ax.errorbar(xs, ys, xerr=xerr, yerr=yerr, fmt="none", capsize=3,
                    ecolor=line.get_color())
        return
    ax.errorbar(xs, ys, xerr=xerr, yerr=yerr, fmt=style, capsize=3, label=label,
                color=colour)


def _errors(uncertainties):
    """Error bars, or none where no value has an uncertainty; a missing one in
    a series that has others is drawn as zero, never invented."""
    if all(u is None for u in uncertainties):
        return None
    return [0.0 if u is None else u for u in uncertainties]


# ------------------------------------------------------------------ reading


def _table_of(samples, field):
    """Whether some sample holds ``field`` as a table's column."""
    for sample in samples:
        if _table_in(sample, field) is not None:
            return True
    return None


def _table_in(sample, field):
    """``(table name, table, column)`` when ``field`` is a column of a table
    this sample holds, else ``None``."""
    import samplekit

    head, _, column = field.partition(".")
    if not column or head not in sample:
        return None
    held = sample[head]
    if isinstance(held, samplekit.Table) and column in held.column_names:
        return head, held, column
    return None


def _scalar(sample, field, missing, text=False):
    """``(value, uncertainty, (unit, symbol))`` of one field of a sample — a
    number it holds, a table's cell included, or with ``text`` a category, as
    a bar's x may be — or ``None`` where the sample holds no value of it. A
    field the sample does not know is kept in ``missing``, to be raised if no
    sample knows it."""
    import samplekit

    try:
        held = _read(sample, field)
    except KeyError as error:
        missing.append(error)
        return None
    if isinstance(held, samplekit.Property):
        # Read once each, a warning said at the caller's line.
        value = _heard(lambda: held.value)
        # Never entered, or not applicable: no point.
        if value is None or value is samplekit.NA:
            return None
        if text and isinstance(value, str):
            return value, None, (None, held.symbol)
        uncertainty = _heard(lambda: held.uncertainty)
        return _number(sample, field, value), uncertainty, (held.unit, held.symbol)
    if isinstance(held, samplekit.Table):
        raise ValueError(f"'{field}' is a table: name one of its columns, {field}.<column>")
    if held is None or held is samplekit.NA:
        return None
    if text and isinstance(held, str):
        return held, None, (None, None)
    return (
        _number(sample, field, held),
        _channel(sample, field, "u"),
        _reading_of(sample, field),
    )


def _reading_of(sample, field):
    """``(unit, symbol)`` of a number read by path: a cell's own, or — for a
    channel, ``malt.u``, ``malt.stats.mean`` — its quantity's."""
    import samplekit

    unit = _channel(sample, field, "unit")
    if unit is not None:
        return unit, _channel(sample, field, "symbol")
    head = field
    while "." in head:
        head = head.rsplit(".", 1)[0]
        try:
            held = _read(sample, head)
        except (KeyError, ValueError):
            continue
        if isinstance(held, samplekit.Property):
            return held.unit, held.symbol
        if not isinstance(held, (list, dict)) and held is not None:
            return _channel(sample, head, "unit"), _channel(sample, head, "symbol")
    return None, None


def _channel(sample, field, channel):
    """A field's other channel — a cell's uncertainty or unit — or ``None``
    where it has none, or the field already names a channel."""
    try:
        return _read(sample, f"{field}.{channel}")
    except (KeyError, ValueError):
        return None


def _number(sample, field, value):
    from samplekit import NA

    # Not applicable is left out, as a value never entered is.
    if value is None or value is NA:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        if hasattr(value, "isoformat"):
            return value  # a date orders an axis as matplotlib reads it
        raise TypeError(
            f"'{field}' of {sample.name or sample.path} is {value!r}, not a number to draw"
        )
    return value


def _check_group(samples, group):
    """A group no sample knows is a misspelling, refused with the nearest
    field, as an axis is; a table's column is refused, since it is not one
    value per sample. Each field of several is checked so."""
    import samplekit

    for field in _fields_of(group):
        if field == "tags":
            continue
        refused = None
        for sample in samples:
            try:
                held = _read(sample, field)
            except KeyError as error:
                refused = refused or error
                continue
            # One value per sample or nothing: a table's repr was a legend
            # entry, and a group per sample of them was refused for its
            # count. A column is refused by the read itself, naming its cells.
            if isinstance(held, samplekit.Table):
                raise ValueError(
                    f"'{field}' is a table: group by one of its cells, "
                    f"{field}.<column>[<index>], or by a field each sample holds one value of"
                )
            break
        else:
            said = refused.args[0] if refused.args else str(refused)
            raise LookupFailed(f"{said}\n  tags and name group samples too")


def _group_of(sample, group):
    """The group a sample falls in: its value of the field, or over several
    fields the combination of its values, one per field."""
    fields = _fields_of(group)
    if len(fields) > 1:
        return tuple(_group_of(sample, field) for field in fields)
    if group == "tags":
        return ", ".join(sample.tags) or "(no tag)"
    try:
        held = _read(sample, group)
    except KeyError:
        return NONE
    import samplekit

    if isinstance(held, samplekit.Property):
        held = _heard(lambda: held.value)
    if isinstance(held, list):
        return ", ".join(str(item) for item in held)
    if held is samplekit.NA:
        return NOT_APPLICABLE
    return NONE if held is None else held


# The groups of the samples holding no value — never entered, or not
# applicable: neither is a value of the field, so a numeric group stays one.
NONE = "(none)"
NOT_APPLICABLE = "n/a"
NO_VALUE = (NONE, NOT_APPLICABLE)


def _read(sample, field):
    """``sample[field]``, its warnings — an outdated value — said at the caller's
    line, the first outside SampleKit, rather than at this module's."""
    return _heard(lambda: sample[field])


def _heard(read):
    """What ``read`` returns, the warnings it gave said at the caller's line —
    or, while a figure is drawn, what they say of values not current held to
    be summed up once it is drawn."""
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        held = read()
    for warning in caught:
        if _HELD is not None and _HELD.take(warning.message):
            continue
        _warn(warning.message, warning.category)
    return held


# What a declared figure's reads said of values not current, while it is drawn:
# one summary once it is, rather than a warning per value.
_HELD = None


class _NotCurrent:
    """The values not current a figure read, by sample, said as one summary
    — as an export says what it was written from."""

    READ = re.compile(
        r"^(?:(?P<who>.*?): )?(?:the uncertainty of )?'(?P<field>[^']+)' (?P<said>.+)$",
        re.DOTALL,
    )
    STATES = (("is outdated", "outdated"), ("failed when last computed", "failed"),
              ("waits for", "waiting"), ("was never computed", "never computed"))

    def __init__(self):
        self.read = {}

    def take(self, message):
        """Whether ``message`` says a value is not current, kept if so."""
        found = self.READ.match(str(message))
        if not found:
            return False
        for words, state in self.STATES:
            if found["said"].startswith(words):
                who = found["who"] or ""
                fields = self.read.setdefault(who, {})
                fields.setdefault(found["field"], state)
                return True
        return False

    def say(self, omitted):
        """One warning: how many values of which states, in how many samples,
        which, and what answers it. A value waiting, or never computed, of a
        sample left out is the left-out summary's, and not said twice."""
        read = {}
        for who, fields in self.read.items():
            kept = {field: state for field, state in fields.items()
                    if not (who in omitted.names and state in ("waiting", "never computed"))}
            if kept:
                read[who] = kept
        if not read:
            return
        counted = {}
        for fields in read.values():
            for state in fields.values():
                counted[state] = counted.get(state, 0) + 1
        states = ", ".join(f"{counted[state]} {state}" for _, state in self.STATES
                           if state in counted)
        listed = [f"{who} {', '.join(fields)}".strip() for who, fields in read.items()]
        if len(listed) > 5:
            listed = listed[:5] + [f"and {len(listed) - 5} more"]
        samples = f"{len(read)} sample" + ("" if len(read) == 1 else "s")
        _warn(
            f"drawn from values that are not current: {states} in {samples} — "
            f"{'; '.join(listed)}\n  "
            + _option("samplekit status says why, and samplekit compute runs them again",
                      "not_current() says why, and compute() runs them again")
        )


def _warn(message, category=UserWarning):
    package = os.path.dirname(os.path.abspath(__file__)) + os.sep
    frame = sys._getframe(1)
    while frame is not None and os.path.abspath(frame.f_code.co_filename).startswith(package):
        frame = frame.f_back
    if frame is None:
        warnings.warn(message, category, stacklevel=2)
        return
    warnings.warn_explicit(
        message, category, frame.f_code.co_filename, frame.f_lineno,
        module=frame.f_globals.get("__name__"),
        registry=frame.f_globals.setdefault("__warningregistry__", {}),
    )


class _Units:
    """The unit and symbol an axis is labelled with: every sample's unit is the
    same, or the points would share an axis in different units."""

    def __init__(self, field, what="axis"):
        self.field = field
        self.what = what
        self.unit = self.symbol = self.whose = None

    def add(self, sample, reading):
        unit, symbol = reading or (None, None)
        if unit is not None:
            if self.unit is None:
                self.unit, self.whose = unit, sample
            elif unit != self.unit:
                raise _in_the_data(ValueError(
                    f"'{self.field}' is in {self.unit} in {_named(self.whose)} and in "
                    f"{unit} in {_named(sample)}: one {self.what} holds one unit"
                ))
        if self.symbol is None:
            self.symbol = symbol

    @property
    def reading(self):
        return self.unit, self.symbol


def _named(sample):
    return sample.name or str(sample.path)


def _column_reading(table, column):
    """``(unit, symbol)`` of a table's column, read from its first row."""
    try:
        cell = table[0][column]
        return cell.unit, cell.symbol
    except Exception:
        return None


def _project_of(samples):
    """The project a figure's labels, style and settings are read from: the
    samples', or over several projects the first sample's."""
    try:
        return samples.project
    except ValueError:
        return _projects_of(samples)[0]


def _figure_shape(figure):
    """What a declared figure draws and how, to tell two declarations apart."""
    if figure is None:
        return None
    return tuple(getattr(figure, key) for key in (
        "kind", "x", "y", "group", "query", "title", "x_label", "y_label", "style",
        *AXIS_KEYS))


def _projects_of(samples):
    """Each project the samples come from, in the order met, by root."""
    found = {}
    for sample in samples:
        try:
            project = sample.project
        except (ValueError, AttributeError):
            continue
        if project is not None:
            found.setdefault(str(project.root), project)
    return list(found.values()) or [None]


def _said_across(samples):
    """Over several projects, a word when the others declare their figures'
    settings or style differently from the first's, which the figure
    follows."""
    projects = [project for project in _projects_of(samples) if project is not None]
    if len(projects) < 2:
        return
    lead = projects[0]
    differing = [
        str(project.root) for project in projects[1:]
        if project.matplotlib != lead.matplotlib
        or project.render.figure_style != lead.render.figure_style
    ]
    if differing:
        _warn(
            f"these samples come from {len(projects)} projects: the figure follows "
            f"{lead.root}'s [matplotlib] and figure style, which "
            f"{', '.join(differing)} declare differently"
        )


def _axis(project, field, read, style, row=None):
    """An axis's label: the field's symbol in the style, else the field, and
    its unit in the style in brackets. A cell's symbol is its column's, so
    its row is said after it — ``Specific gravity, day 3`` — where it read
    as the whole column."""
    unit, symbol = read if read else (None, None)
    if project is not None:
        unit, symbol = project._axis_label(field, unit, symbol, style)
    elif style not in (None, "plain"):
        raise ValueError(f"no style '{style}' is declared: no .samplekitrc was found")
    text = f"{symbol}, {row}" if symbol and row else symbol or field
    return f"{text} [{unit}]" if unit else text


def _row_of(samples, field):
    """The row a cell's field names, in words — ``day 3``, the index column
    and its value — or ``None`` for a field that is no cell."""
    import re

    import samplekit

    found = re.match(r"^([A-Za-z_]\w*)\.[A-Za-z_]\w*\[([^\]]*)\]", field or "")
    if not found:
        return None
    table, written = found.groups()
    for sample in samples:
        try:
            held = sample[table] if table in sample else None
        except (KeyError, ValueError):
            continue
        if not isinstance(held, samplekit.Table):
            continue
        index = held.index
        names = [index] if isinstance(index, str) else list(index)
        values = [value.strip() for value in written.split(",")]
        if len(names) == len(values):
            return ", ".join(f"{name} {value}" for name, value in zip(names, values))
        return f"{', '.join(names)} {written}"
    return None


def _typeset(text, style, what="label", given=False):
    """Refuses a label matplotlib would not typeset — LaTeX outside ``$…$``,
    which it writes out backslash by backslash, or mathtext it cannot parse —
    naming it, rather than a figure that shows the source or fails in
    matplotlib's words when it is drawn. Under ``text.usetex`` LaTeX typesets
    the label, and none of this applies."""
    if not text:
        return
    import re

    import matplotlib

    if matplotlib.rcParams["text.usetex"]:
        return
    where = f" (style '{style}')" if style and not given else ""
    remedy = "write it in mathtext" if given else "declare a style for figures, or write the label yourself in mathtext"
    # An escaped dollar is a dollar sign, not the edge of mathtext.
    plain = re.sub(r"\$[^$]*\$", "", text.replace(r"\$", ""))
    if "\\" in plain:
        raise ValueError(
            f"the {what} '{text}'{where} is LaTeX outside $…$, which matplotlib writes "
            f"as it is\n  a figure's text is matplotlib's mathtext, $\\mathrm{{brix}}$ and "
            f"not \\si{{\\litre}}, % and not \\%: {remedy}"
        )
    if "$" not in text.replace(r"\$", ""):
        return
    from matplotlib.mathtext import MathTextParser

    try:
        MathTextParser("path").parse(text)
    except ValueError as error:
        said = str(error).strip().splitlines()
        raise ValueError(
            f"matplotlib cannot typeset the {what} '{text}'{where}: "
            f"{said[-1] if said else error}"
        ) from None
