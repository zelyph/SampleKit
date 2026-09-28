"""The command line's figure.

``samplekit plot`` starts this module in the project's environment, writes one
JSON request on its standard input, and waits: the window, or the file, and
every diagnostic are this process's. The exit code is the command's.

    {"figure": "vfi", "model": false, "samples": [...], "output": null}
    {"axes": {"kind": "scatter", "x": "period", "y": "vfi", "group": null}, ...}
"""

import contextlib
import json
import sys
import traceback
import warnings

REFUSED, FAILED, UNWRITTEN, INTERRUPTED = 1, 2, 3, 130


def main():
    request = json.loads(sys.stdin.read())
    warnings.showwarning = _said
    try:
        return _run(request)
    except KeyboardInterrupt:
        return INTERRUPTED
    except ModuleNotFoundError as error:
        return refuse(
            f"this environment ({sys.executable}) has no {error.name}: a figure is drawn by "
            "matplotlib, installed with samplekit",
            code=UNWRITTEN,
        )


def _run(request):
    import samplekit
    from samplekit import _figures

    if request.get("version") not in (None, samplekit.__version__):
        return refuse(
            f"this environment holds samplekit {samplekit.__version__}, and the command "
            f"is {request['version']}: install the same version in both"
        )
    model = None if request.get("model") else False
    # What the model prints as it loads is not a figure's name.
    with contextlib.redirect_stdout(sys.stderr):
        try:
            samples = samplekit.SampleList(
                [samplekit.Sample(path, model=model) for path in request["samples"]]
            )
        except Exception:
            traceback.print_exc()
            return FAILED

    if model is None:
        _describe(samples)

    axes = request.get("axes")
    output = request.get("output")
    # What the history says wrote a file drawn here: the command line.
    _figures.SAID = request.get("said")
    # A refusal names the command's options, `-o`, rather than `output=`.
    _figures.COMMAND_LINE = True
    # Drawn from the project as it was: the snapshot it carries.
    _figures.CARRY = request.get("snapshot")
    overrides = {key: value for key, value in (request.get("overrides") or {}).items()
                 if value is not None}
    overrides["project_style"] = request.get("project_style", True)
    try:
        if axes is not None:
            # Given axes carry their group and kind; the overrides say the rest.
            overrides.update(
                {key: axes[key] for key in ("group", "kind") if axes.get(key) is not None}
            )
            _figures.plot(samples, x=axes["x"], y=axes["y"], output=output, overwrite=True,
                          **overrides)
        else:
            _figures.plot(samples, request["figure"], output=output, overwrite=True,
                          **overrides)
    except KeyboardInterrupt:
        raise
    except Exception as error:
        if isinstance(error, ModuleNotFoundError) and not _figures.raised_by_the_figure(error):
            raise
        if _figures.raised_by_the_figure(error):
            traceback.print_exc()
            return FAILED
        if isinstance(error, OSError):
            return refuse(f"the figure could not be written: {error}", code=UNWRITTEN)
        if isinstance(error, RuntimeError) and "LaTeX" in str(error):
            # LaTeX's whole log said nothing a reader could use: its errors,
            # the lines starting with `!`, and the label they were in.
            lines = str(error).splitlines()
            said = [line for line in lines if line.startswith("!")] or lines[:1]
            return refuse("LaTeX could not typeset a label: " + " ".join(said[:3]))
        # The samples disagreeing — two units on one axis, two bars at one x —
        # is a data error, 2, as the table's column of two units is; a name or
        # an option the command does not know is a refusal, 1.
        if isinstance(error, (RuntimeError, ValueError)) and _figures.raised_by_the_data(error):
            return refuse(error.args[0] if error.args else str(error), code=FAILED)
        if isinstance(error, (KeyError, RuntimeError, ValueError)):
            return refuse(error.args[0] if error.args else str(error))
        if isinstance(error, TypeError):
            return refuse(str(error), code=FAILED)
        traceback.print_exc()
        return FAILED
    return 0


def _describe(samples):
    """Writes the description of each model drawn from, as the worker does
    when it imports one. A figure is not refused for it: a model that
    loaded its samples draws, and the next command that needs the description
    has it written."""
    from samplekit import _native, _worker

    seen = set()
    for sample in samples:
        model = type(sample)
        if model in seen or model.__name__ == "Sample" and model.__module__ == "samplekit":
            continue
        seen.add(model)
        try:
            project = sample.project
            declared = project.model
            if declared is None or project.root is None:
                continue
            _worker.describe(model, str(declared.path), str(project.root), _native)
        except Exception:
            continue


def _said(message, category, filename, lineno, file=None, line=None):
    """A warning as the command line says one: its words, without the source
    line of this package it was raised on."""
    text = str(message).replace("compute() runs it", "samplekit compute runs it")
    print(f"warning: {text}", file=sys.stderr)


def refuse(message, code=REFUSED):
    print(f"error: {message}", file=sys.stderr)
    return code


if __name__ == "__main__":
    sys.exit(main())
