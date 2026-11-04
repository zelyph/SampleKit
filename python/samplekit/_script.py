"""The command line's script.

``samplekit run`` starts this module in the model's environment, writes one
JSON request in a file whose path ``SAMPLEKIT_SCRIPT_REQUEST`` holds, and
waits. The script prints as it runs, and its standard input is the
terminal's, or the TUI's line: ``input()`` asks there, each question and its
answer named in the reply. What it returns is printed after it, and what it
would save is held, not written, and named in the reply file for the command
to show and ask about. The exit code is the command's.

    {"script": "dilute", "samples": [...], "options": {"factor": "0.8"},
     "style": null, "reply": "/tmp/…/reply.json"}
"""

import builtins
import json
import os
import sys
import traceback
import warnings

REFUSED, FAILED, INTERRUPTED = 1, 2, 130


def main():
    with open(os.environ["SAMPLEKIT_SCRIPT_REQUEST"], encoding="utf-8") as asked:
        request = json.load(asked)
    warnings.showwarning = _said
    builtins.input = _asking
    try:
        return _run(request)
    except KeyboardInterrupt:
        return INTERRUPTED


def _run(request):
    import samplekit
    from samplekit import _native, _scripts

    if request.get("version") not in (None, samplekit.__version__):
        return refuse(
            f"this environment holds samplekit {samplekit.__version__}, and the command "
            f"is {request['version']}: install the same version in both"
        )
    name = request["script"]
    try:
        samples = [samplekit.Sample(path) for path in request["samples"]]
    except Exception:
        traceback.print_exc()
        return FAILED
    if not samples:
        return refuse("no sample is selected: nothing to run", code=FAILED)
    models = {type(sample) for sample in samples}
    model = type(samples[0])
    try:
        scripts = _scripts.scripts_of(model)
    except TypeError as error:
        return refuse(str(error))
    script = scripts.get(name)
    if script is None:
        return refuse(
            f"no script named '{name}' in {model.__name__}: "
            + (", ".join(scripts) if scripts else "the model marks none with @sk.script")
        )
    if script.collection and len(models) > 1:
        return refuse(
            f"'{name}' runs on one model's samples, and the selection holds "
            + ", ".join(sorted(kind.__name__ for kind in models))
        )
    for other in models - {model}:
        if name not in _scripts.scripts_of(other):
            return refuse(f"{other.__name__}, the model of some samples selected, declares "
                          f"no script '{name}'")
    try:
        arguments = _scripts.arguments(script, request.get("options") or {})
    except ValueError as error:
        return refuse(str(error))

    # From here, a save is held for the command to show.
    _native._hold_saves()
    results = []
    running = None
    try:
        if script.collection:
            results.append((None, getattr(model, name)(samplekit.SampleList(samples),
                                                       **arguments)))
        else:
            for sample in samples:
                running = sample
                results.append((sample, getattr(sample, name)(**arguments)))
            running = None
        # What the script changed and did not save is saved as it would be.
        for sample in samples:
            if sample._changed:
                sample.save()
    except KeyboardInterrupt:
        raise
    except Exception:
        _said_failure()
        # On which sample, and that nothing is written: the traceback alone
        # left the reader to guess which of the selection raised.
        if running is not None:
            print(f"error: '{name}' raised on {_path(running)}: nothing is written",
                  file=sys.stderr)
        else:
            print(f"error: '{name}' raised: nothing is written", file=sys.stderr)
        return FAILED

    style = request.get("style")
    several = sum(1 for _, result in results if result is not None) > 1
    for sample, result in results:
        text = shown(result, style)
        if text is None:
            continue
        if several and sample is not None:
            if "\n" in text:
                print(f"{sample.name}\n{_indented(text)}")
            else:
                print(f"{sample.name}  {text}")
        else:
            print(text)
    sys.stdout.flush()

    held = [{"path": path, "before": before, "after": after}
            for path, before, after in _native._held_saves()]
    with open(request["reply"], "w", encoding="utf-8") as reply:
        json.dump({"held": held, "answers": _ANSWERS}, reply)
    return 0


_ANSWERS = []
_INPUT = builtins.input


def _asking(prompt=""):
    """``input()`` as the script calls it: answered from the terminal, or the
    TUI's line, and kept for the preview and the history. With nothing to
    read, the question is named, and how a script is given a value."""
    try:
        answer = _INPUT(prompt)
    except EOFError:
        raise EOFError(
            f"the script asked {str(prompt).strip()!r}, and no terminal can answer it: "
            "--set NAME=VALUE gives a script its values"
        ) from None
    _ANSWERS.append({"prompt": str(prompt), "answer": answer})
    return answer


def shown(result, style=None):
    """What a script returned, as text: None for nothing."""
    import samplekit

    if result is None:
        return None
    if isinstance(result, str):
        return result
    if isinstance(result, samplekit.SampleList):
        return "\n".join(_path(sample) for sample in result)
    if isinstance(result, (list, tuple)):
        parts = [shown(item, style) for item in result]
        return "\n".join(part for part in parts if part is not None)
    if isinstance(result, (samplekit.Property, samplekit.Table, samplekit.Sample,
                           samplekit.Summary)):
        return result.to_text(style)
    return str(result)


def _path(sample):
    path = sample.path
    if path is None:
        return sample.name or "(no file)"
    try:
        return os.path.relpath(path)
    except ValueError:
        return str(path)


def _indented(text):
    return "\n".join(f"  {line}" if line else line for line in text.splitlines())


def _said(message, category, filename, lineno, file=None, line=None):
    """A warning as the command line says one: its words, without the source
    line it was raised on."""
    text = str(message).replace("compute() runs it", "samplekit compute runs it")
    # Said once: a value read twice warned twice.
    if text in _SAID:
        return
    _SAID.add(text)
    print(_painted("warning:", text), file=sys.stderr)


_SAID = set()


def _said_failure():
    """The script's traceback, from its own code: what printed before it
    first, and none of this module's frames."""
    kind, error, trace = sys.exc_info()
    while trace is not None and trace.tb_frame.f_code.co_filename == __file__:
        trace = trace.tb_next
    sys.stdout.flush()
    traceback.print_exception(kind, error, trace)


def refuse(message, code=REFUSED):
    print(_painted("error:", message), file=sys.stderr)
    return code

def _painted(prefix, text):
    """`prefix` and `text` as the command line paints them where it gave its
    colours: the prefix in its own, advice at the end muted."""
    import os
    reset = os.environ.get("SAMPLEKIT_PAINT_RESET")
    if not reset:
        return f"{prefix} {text}"
    style = os.environ.get(
        "SAMPLEKIT_PAINT_WARNING" if prefix == "warning:" else "SAMPLEKIT_PAINT_ERROR", ""
    )
    muted = os.environ.get("SAMPLEKIT_PAINT_MUTED", "")
    fact, mark, tip = text.rpartition(" — ")
    if mark and ("samplekit " in tip or any(
        word.startswith("--") and len(word) > 2 and word[2].isalpha() for word in tip.split()
    )):
        text = f"{fact} — {muted}{tip}{reset}"
    return f"{style}{prefix}{reset} {text}"


if __name__ == "__main__":
    sys.exit(main())
