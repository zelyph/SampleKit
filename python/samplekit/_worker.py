"""The command line's worker.

``samplekit compute`` starts this module in the project's environment and speaks
to it in JSON, one object per line. It names a sample and its model; the worker
loads the sample through the Python API, computes value by value in dependency
order, and saves the file. The values never cross the pipe: both sides read the
file.
"""

import functools
import importlib.machinery
import hashlib
import inspect
import io
import json
import linecache
import logging
import os
import pathlib
import signal
import sys
import textwrap
import threading
import time
import tokenize
import traceback
import types
import warnings

# What `compute --try` computed, by sample, for a `write` that may follow. The
# tracebacks of what failed meanwhile wait beside it, by sample and value: a
# failure log is written when the failure is written to the file, and not
# before.
TRIED = {}
FAILURES = {}
# What a `write` records beside what `try` computed: each formula's digest, and
# the values computed whole.
TRIED_FORMULAS = {}

# The value in progress, which a logged record is sent beside.
CURRENT = {"value": None}

# The models this process has described, by file and class: once each.
DESCRIBED = {}

# What this worker and the command line say to each other, numbered as the
# command line's `PROTOCOL` is: raised with any change to the requests.
PROTOCOL = 2


class Forward(logging.Handler):
    """A record the model logs, sent to the command line beside the value in
    progress: a warning is shown there, the rest kept in the run's log."""

    def __init__(self, send):
        super().__init__(logging.INFO)
        self.send = send
        self.setFormatter(logging.Formatter("%(message)s"))

    def emit(self, record):
        try:
            self.send({"log": {
                "level": record.levelname,
                "value": CURRENT["value"],
                "message": self.format(record),
            }})
        except Exception:
            self.handleError(record)


class Lines:
    """What the model writes to ``sys.stdout`` or ``sys.stderr``, sent line by
    line beside the value in progress, so that the command line knows whose it
    is and receives it in order. A C library or a subprocess writing to
    the file descriptors themselves still reaches the stderr pipe."""

    encoding = "utf-8"
    errors = "replace"

    def __init__(self, send):
        self.send = send
        self.pending = ""
        self.lock = threading.Lock()

    def write(self, text):
        with self.lock:
            self.pending += text
            while "\n" in self.pending:
                line, self.pending = self.pending.split("\n", 1)
                self.send({"output": {"value": CURRENT["value"], "text": line}})
        return len(text)

    def flush(self):
        with self.lock:
            if self.pending:
                self.send({"output": {"value": CURRENT["value"], "text": self.pending}})
                self.pending = ""

    def isatty(self):
        return False

    def writable(self):
        return True

    def fileno(self):
        return 2

    @property
    def buffer(self):
        """The bytes a model writes, as a script's `sys.stdout.buffer` takes
        them: decoded and sent as text, so that a model computes from the command
        line exactly as it computes in a script."""
        return _Bytes(self)


class _Bytes:
    def __init__(self, lines):
        self.lines = lines

    def write(self, data):
        self.lines.write(bytes(data).decode("utf-8", "replace"))
        return len(data)

    def flush(self):
        self.lines.flush()


class _SourceOnly(importlib.machinery.SourceFileLoader):
    """Compiles a module from its source, never from a `__pycache__`."""

    def get_code(self, fullname):
        source = self.get_data(self.path)
        return compile(source, self.path, "exec", dont_inherit=True)


class _UserCodeFromSource:
    """The model's own code compiled from its source: a `__pycache__` the
    user's Python wrote, of the same second and size as an edit, ran the old
    code while the digest read the new text. The installed libraries keep
    their caches."""

    installed = tuple(os.path.realpath(prefix) for prefix in {
        sys.prefix, sys.base_prefix, sys.exec_prefix, sys.base_exec_prefix})

    def find_spec(self, name, path=None, target=None):
        spec = importlib.machinery.PathFinder.find_spec(name, path, target)
        origin = getattr(spec, "origin", None) if spec is not None else None
        if not origin or not origin.endswith(".py"):
            return None
        real = os.path.realpath(origin)
        if "site-packages" in real or real.startswith(self.installed):
            return None
        spec.loader = _SourceOnly(name, origin)
        return spec


def main():
    sys.dont_write_bytecode = True
    sys.meta_path.insert(0, _UserCodeFromSource())
    # The protocol keeps a private copy of stdout, and file descriptor 1 becomes
    # stderr before anything is imported: a model's print, or a C library
    # writing to stdout, cannot corrupt a line.
    protocol = os.fdopen(os.dup(1), "w", encoding="utf-8", buffering=1)
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    # The requests likewise, on a private copy of stdin, and file descriptor 0
    # reads nothing: a `breakpoint()` or an `input()` left in a model read the
    # next request as its answer, or waited on a line that never came.
    requests = os.fdopen(os.dup(0), "r", encoding="utf-8")
    null = os.open(os.devnull, os.O_RDONLY)
    os.dup2(null, 0)
    os.close(null)
    sys.stdin = open(0, "r", closefd=False)

    # A model's own threads may log: one line is written whole.
    lock = threading.Lock()

    def send(message):
        line = json.dumps(readable(message)) + "\n"
        with lock:
            protocol.write(line)
            protocol.flush()

    # The command line gone — killed, or interrupted — interrupts the value in
    # progress, which is then neither saved nor stamped.
    threading.Thread(target=watch_parent, args=(os.getppid(),), daemon=True).start()

    import samplekit
    from samplekit import _native

    version = ".".join(str(part) for part in sys.version_info[:3])
    # The protocol the command line checks beside the version: two builds of
    # one version may speak differently.
    send({"ready": {"version": samplekit.__version__, "python": version,
                    "protocol": PROTOCOL}})
    # After the announcement, which is the first line the command line reads.
    sys.stdout = sys.stderr = Lines(send)
    root = logging.getLogger()
    root.handlers = [Forward(send)]
    root.setLevel(logging.INFO)
    logging.captureWarnings(True)
    try:
        for line in requests:
            if not line.strip():
                continue
            try:
                handle(json.loads(line), send, samplekit, _native)
            except Exception:
                send({"error": {"kind": "sample", "traceback": traceback.format_exc()}})
            send({"done": True})
    except KeyboardInterrupt:
        # What finished is in the file already; what was running is dropped.
        # A second interrupt while the interpreter flushes its streams at exit
        # printed "Exception ignored while flushing sys.stdout", which the
        # command line then counted as lines the model printed: interrupts are
        # ignored from here, what is pending is flushed, and the interpreter
        # is left its own stderr to flush.
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        try:
            sys.stdout.flush()
        except BaseException:
            pass
        sys.stdout = sys.stderr = sys.__stderr__
        return


def readable(item):
    """A message whose text the command line can decode. A lone surrogate — a
    file name printed after `os.fsdecode`, bytes decoded with
    `surrogateescape` — is valid Python and no JSON reader accepts it: the line
    was refused, and the run reported as a dead worker while the worker went
    on saving. Each such character is replaced, as undecodable bytes are."""
    if isinstance(item, str):
        try:
            item.encode("utf-8")
        except UnicodeEncodeError:
            try:
                raw = item.encode("utf-8", "surrogateescape")
            except UnicodeEncodeError:
                raw = item.encode("utf-8", "replace")
            return raw.decode("utf-8", "replace")
        return item
    if isinstance(item, dict):
        return {readable(key): readable(value) for key, value in item.items()}
    if isinstance(item, (list, tuple)):
        return [readable(value) for value in item]
    return item


def watch_parent(parent):
    while os.getppid() == parent:
        time.sleep(0.2)
    if hasattr(signal, "pthread_kill"):
        # Aimed at the main thread, so that a formula asleep or waiting wakes.
        signal.pthread_kill(threading.main_thread().ident, signal.SIGINT)
    else:
        import _thread

        _thread.interrupt_main()


def shown(sample, value):
    """A value as a terminal renders it, or a column's cells, read without
    warnings: before and after are what the command line shows."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            if "." in value:
                # Each cell's two numbers: a formula giving the uncertainty
                # alone changes no value.
                table, column = value.split(".", 1)
                held = getattr(sample, table)
                return list(zip(held.values(column), held.uncertainties(column)))
            return str(sample[value])
        except Exception:
            return "—"


def exact(sample, value):
    """A property's two numbers as they are held, for telling a change from a
    rendering that hides it; `None` where they cannot be read."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            if "." in value:
                return None
            quantity = getattr(sample, value)
            return (quantity.value, quantity.uncertainty, quantity.unit)
        except Exception:
            return None


def missing_inputs(sample, stored, value, waiting, made):
    """What `value` needs and nobody has given: a value it is declared to read
    that is absent from the file and was not computed in this run, one that is
    itself waiting, a table's column with no row — and, for an uncertainty
    formula, the value it is the uncertainty of, named as `value` itself. A
    formula over nothing is not run: it waits, and is no failure.

    Only looks: an input read here while it is stale would warn that it is,
    in the middle of the run that is about to compute it."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        return _missing_inputs(sample, stored, value, waiting, made)


def _missing_inputs(sample, stored, value, waiting, made):
    if "[" in value:
        return []
    missing = []
    try:
        inputs = sample.dependencies(value)
    except Exception:
        inputs = []

    def held(name):
        if name in made:
            return True
        # A value no formula gives is read from the model — a default counts —
        # which computes nothing; one a formula gives, from the file.
        try:
            held = sample[name]
            # An attribute: given where it holds something.
            if not hasattr(held, "is_computed"):
                return held is not None
            if not held.is_computed:
                # A statistic of readings not taken yet: this computation takes
                # it, running nothing.
                return sample[name].value is not None or sample[name]._statistic_owed
        except Exception:
            return True
        try:
            return stored is not None and name in stored and stored[name].value is not None
        except Exception:
            return True

    def column_hole(name):
        """What of a column read whole nobody entered: the column, where no
        row holds a value; else its first empty cell, `hardness[20]`; `None`
        where every row holds one. The model's own rows count as the file's:
        a table it declares with `rows=` holds them before any file does."""
        table, _, column = name.partition(".")
        column = column.split("[")[0]
        for source in (sample, stored):
            try:
                if source is None or table not in source:
                    continue
                values = source[table].values(column)
                if not any(item is not None for item in values):
                    continue
                indexes = source[table].index_values
                for index, item in zip(indexes, values):
                    if item is None:
                        shown = ", ".join(str(part) for part in index) \
                            if isinstance(index, tuple) else str(index)
                        return f"{name}[{shown}]"
                return None
            except Exception:
                return None
        return name

    own_table = value.split(".")[0] if "." in value else None

    def row_by_row(name):
        """Whether `value` is a column its table's formula gives row by row,
        reading `name` of the same table one row at a time."""
        if own_table is None or name.split(".")[0] != own_table:
            return False
        try:
            return bool(sample[own_table]._derived_by_rows(value.split(".", 1)[1]))
        except Exception:
            return True

    for name in inputs:
        if name.startswith("row."):
            continue
        if "." in name:
            if name in waiting:
                missing.append(name)
                continue
            if name in made or row_by_row(name):
                continue
            hole = column_hole(name)
            if hole is not None:
                missing.append(hole)
            continue
        if name in waiting or not held(name):
            missing.append(name)
    # An uncertainty computed beside a value that nobody entered: a property's.
    if "." in value:
        return missing
    try:
        own = sample[value]
        if not own.is_computed and not held(value):
            missing.append(value)
    except Exception:
        pass
    return missing


def numeric(held):
    """Whether `exact` read a number to spell: text, n/a and nothing are
    shown as the terminal renders them."""
    return (
        held is not None
        and isinstance(held[0], (int, float))
        and not isinstance(held[0], bool)
    )


def empty(cell):
    """A cell nothing was written in: no value nor uncertainty, or a float's
    NaN; a cell is read as its two numbers."""
    if isinstance(cell, tuple):
        return all(empty(number) for number in cell)
    return cell is None or (isinstance(cell, float) and cell != cell)


def exaggerated(before, after, old, new):
    """Whether two values as shown, `207 → 208`, say a change far larger than
    the one between their numbers, 207.48 → 207.53: less than half a unit of
    the last digit shown. Said whole then, as values shown alike are."""
    def first(said):
        for word in str(said).split(" "):
            try:
                float(word)
            except ValueError:
                continue
            return word
        return None

    def unit(shown):
        mantissa, _, exponent = shown.lower().partition("e")
        decimals = len(mantissa.partition(".")[2])
        return 10.0 ** ((int(exponent) if exponent else 0) - decimals)

    was, shown = first(before), first(after)
    if was is None or shown is None or not numeric(old) or not numeric(new):
        return False
    # The finer of the two: `20.1235 → 20` shows its move.
    step = min(unit(was), unit(shown))
    difference = abs(float(old[0]) - float(new[0]))
    # Less than half a step, which rounding shows as a whole one.
    return 0 < difference < step / 2


def spelled(held):
    """Both numbers in full, and the unit they are in, which the rounded
    form carried and the digits alone had lost."""
    value, uncertainty, unit = held
    number = f"{value!r}" if uncertainty is None else f"{value!r} ± {uncertainty!r}"
    return f"{number} {unit}" if unit else number


def handle(request, send, samplekit, native):
    if request["op"] == "write":
        sample = TRIED.pop(request["sample"], None)
        digests, whole = TRIED_FORMULAS.pop(request["sample"], ({}, []))
        if sample is None:
            FAILURES.pop(request["sample"], None)
            send({"error": {"kind": "save", "message": "nothing was computed for it to write"}})
            return
        try:
            write_failures(request["sample"], sample)
            sample.save()
        except Exception as error:
            send({"error": {"kind": "save", "message": str(error)}})
            return
        send({"formulas": digests})
        send({"computed": whole})
        send({"saved": request["sample"]})
        return
    try:
        model = native._model_at(request["model"], request.get("class"), request["root"])
    except Exception as error:
        # A description of a model that cannot be read would be trusted by
        # nobody, and read by a person: it goes.
        forget_description(request["root"])
        # With its traceback: a model raising as it is imported was reported by
        # its message alone — no file, no line, no exception type.
        send({"error": {"kind": "model", "message": str(error),
                        "traceback": model_traceback(error)}})
        return
    # Described as it is imported, whatever was asked. A model that cannot be
    # described is said where the description was asked for; a computation goes
    # on, and finds for itself whether a sample loads.
    try:
        described = describe(model, request["model"], request["root"], native)
    except Exception as error:
        forget_description(request["root"])
        if request["op"] == "describe":
            send({"error": {"kind": "model", "message": str(error),
                            "traceback": model_traceback(error)}})
            return
    if request["op"] == "describe":
        send({"described": described})
        return
    try:
        sample = samplekit.Sample(request["sample"], model=model)
    except Exception as error:
        send({"error": {"kind": "sample", "traceback": model_traceback(error)}})
        return
    names = request.get("names") or []
    rerun = bool(request.get("rerun"))
    force = bool(request.get("force"))
    timeout = request.get("timeout")
    # Each formula's digest, taken before any formula runs, and those that
    # differ from what computed the sample's values: planned though current.
    digests = formula_digests(sample, request["model"])
    recorded = request.get("recorded") or {}

    def known_to(name, known):
        known = [known] if isinstance(known, str) else known
        return digests[name] in known

    changed = sorted(name for name, known in recorded.items()
                     if name in digests and not known_to(name, known))
    send({"formulas": digests})
    try:
        plan = sample._plan(names, rerun, force, changed)
    except (KeyError, ValueError) as error:
        # A value this sample does not have, another model's in a selection
        # spanning two projects, or one without a formula: reported for the
        # sample, never raised.
        send({"error": {"kind": "names", "message": error.args[0] if error.args else str(error)}})
        return
    if request["op"] == "plan":
        # Said as status says it: a value the file records failed is
        # *failed*, and one whose inputs nobody entered *waits for* them —
        # not *never computed*, which the run then contradicted.
        try:
            stored = samplekit.Sample(request["sample"], model=False)
        except Exception:
            stored = None
        # What reads a value whose formula changed is said with it: it is
        # computed again if that value moves, as a stale input's reader is.
        planned = {value for value, _ in plan}
        for value, reason in list(plan):
            # Values named alone are what runs: what reads them is left
            # outdated, as `-p` says, and the list promised it would run.
            if reason != "formula changed" or names:
                continue
            try:
                readers = sample.affected_by_change(value)
            except Exception:
                readers = []
            for reader in readers:
                # An override stands whatever its formula's inputs do.
                if reader not in planned and not edited(sample, reader):
                    planned.add(reader)
                    plan.append((reader, f"reads {value}, whose formula changed"))
        # A value planned before this one is computed first: it counts as held.
        said, waiting, made, held = [], set(), set(), set()
        for value, reason in plan:
            # What failed holds back what reads it, as what waits does.
            missing = missing_inputs(sample, stored, value, waiting | held, made)
            if missing:
                waiting.add(value)
                # An input that waits itself said so, as `compute` says one
                # that failed.
                def named(name):
                    if name in held:
                        return f"{name}, which failed"
                    if name in waiting and name != value:
                        return f"{name}, which waits"
                    return name

                if any(named(name) != name for name in missing):
                    reason = "waits for " + "; ".join(named(name) for name in missing)
                else:
                    reason = "waits for " + ", ".join(missing)
            else:
                made.add(value)
                if failed_before(sample, value):
                    reason = "failed"
                    held.add(value)
            said.append([value, reason])
        send({"plan": said})
        return

    # Before is what the file holds: the modelled sample may already answer
    # with what its formulas would give. Read only by a computation, never by a
    # plan, which would otherwise read every file twice.
    try:
        stored = samplekit.Sample(request["sample"], model=False)
    except Exception:
        stored = None

    # An input held as text where numbers belong fails what reads it by name,
    # before any formula runs.
    blocked = {}
    for entry in request.get("refused") or []:
        reason = f'{entry["name"]} is text: "{entry["text"]}"'
        try:
            dependents = sample.affected_by_change(entry["name"])
        except Exception:
            dependents = []
        for dependent in dependents:
            blocked.setdefault(dependent, reason)

    # A formula that ran past --timeout and did not stop, its worker stopped
    # for it: failed `TimedOut`, never run again in this run, the rest of the
    # sample computed by this fresh worker.
    overran = set(request.get("overran") or [])
    if overran:
        LIMIT["seconds"] = request.get("timeout")
    for value in overran:
        stopped = TimedOut(
            f"the formula ran past --timeout {LIMIT['seconds']:g} s and did not stop: "
            "its worker was stopped"
        )
        try:
            sample._fail_planned(value, stopped)
        except Exception:
            pass
        blocked[value] = f"TimedOut: {stopped}\nwhile computing '{value}'"

    stopped_tables = {value.split(".")[0] for value in overran if "." in value}
    writing = request["op"] == "compute"
    computed = 0
    done = set()
    # Values not run for want of an input, and those computed in this run.
    waiting = set()
    made = set()
    failed = set()
    # The values computed whole, whose formula is now the one that computed
    # them: a property, or a column every cell of which ran.
    whole = []
    FAILURES.pop(request["sample"], None)
    # Without names a computed value can stale what reads it, so the plan is
    # asked again — for what is pending — until only failures, or nothing, remain.
    while True:
        plan = [entry for entry in plan if entry[0] not in done]
        if not plan:
            break
        send({"plan": [list(entry) for entry in plan]})
        for value, reason in plan:
            done.add(value)
            if value in blocked:
                failed.add(value)
                send({"failed": value, "traceback": blocked[value] + "\n"})
                # Its failure written, as a formula's that raised: the file
                # says `TimedOut`, and the log why.
                if writing and value in overran:
                    try:
                        sample.save()
                    except Exception as error:
                        send({"error": {"kind": "save", "message": str(error)}})
                        return
                    record_failure(sample, value, blocked[value])
                continue
            # What ran past --timeout is missing to what reads it: it waits
            # *for area, which failed*. Run, it would bring that input up to
            # date first, past --timeout again. A value that raised is read
            # with its last number, as before.
            missing = missing_inputs(sample, stored, value, waiting | (failed & overran), made)
            # A table one column of which ran past --timeout is not read in
            # this run: reading any of its columns resolves the table, which
            # ran that formula again, and stopped another worker for it.
            if not missing and stopped_tables:
                try:
                    reads = sample.dependencies(value)
                except Exception:
                    reads = []
                missing = [
                    stopped
                    for stopped in overran
                    if "." in stopped
                    and any(read.split(".")[0] == stopped.split(".")[0] for read in reads if "." in read and not read.startswith("row."))
                ]
            if missing:
                waiting.add(value)
                # A failure an earlier model gave is not what this one does:
                # it waits. Left, the file went on saying failed.
                if writing and failed_before(sample, value):
                    try:
                        sample[value]._invalidate()
                        sample.save()
                    except Exception as error:
                        send({"error": {"kind": "save", "message": str(error)}})
                        return
                    forget_failure(sample, value)
                send({"waiting": value, "inputs": missing,
                      "failed": [name for name in missing if name in failed]})
                continue
            CURRENT["value"] = value
            send({"started": value})
            started = time.perf_counter()
            before = shown(stored, value) if stored is not None else "—"
            again = rerun or value in changed
            try:
                within(timeout, lambda: sample._compute_planned(value, again, force))
            except Exception as error:
                sys.stdout.flush()
                failed.add(value)
                trace = model_traceback(error, formula_of(sample, value))
                send({"failed": value, "traceback": trace})
                FAILURES.setdefault(request["sample"], {})[value] = trace
                # The failure is written, and cleared by the next success; its
                # traceback goes to the log at the same moment, so that a
                # rehearsal leaves nothing behind.
                if writing:
                    try:
                        write_failures(request["sample"], sample)
                        sample.save()
                    except Exception as save_error:
                        send({"error": {"kind": "save", "message": str(save_error)}})
                        return
            else:
                sys.stdout.flush()
                # Counted by cells, a column by its rows, and a value and its
                # uncertainty of two formulas as two.
                cells = cells_of(sample, value)
                computed += cells
                made.add(value)
                if "." not in value or again:
                    whole.append(value)
                # Computed by the formula as it now is: no longer changed.
                changed = [name for name in changed if name != value]
                # Saved as each value finishes, which is when it is stamped: an
                # interruption keeps every value finished before it.
                if writing:
                    try:
                        sample.save()
                    except Exception as error:
                        # Not the model's failure: said as a save, and the sample stops.
                        send({"error": {"kind": "save", "message": str(error)}})
                        return
                    forget_failure(sample, value)
                after = shown(sample, value)
                # As a table shows it: at the precision the project declares,
                # or at its rule where it declares none — every digit said the
                # noise a computation leaves, 6.081250000000001. Two values
                # shown alike — 0.01842 → 0.01824, both `0.018` — are said
                # whole: the one change a reader looks for.
                new = exact(sample, value)
                if isinstance(after, str) and numeric(new):
                    old = exact(stored, value) if stored is not None else None
                    if (after == before or exaggerated(before, after, old, new)) \
                            and numeric(old) and old[:2] != new[:2]:
                        before, after = spelled(old), spelled(new)
                if isinstance(after, list) and isinstance(before, list):
                    moved = sum(1 for old, new in zip(before, after) if old != new)
                    moved += abs(len(after) - len(before))
                    # Cells nobody wrote yet are said to be empty: `5 cells`
                    # before a first compute read as five values replaced.
                    held = "cells" if any(not empty(cell) for cell in before) else "empty cells"
                    before, after = f"{len(before)} {held}", f"{moved} changed"
                elif isinstance(after, list):
                    before, after = "—", f"{len(after)} new"
                send({
                    "finished": value,
                    "cells": cells,
                    "seconds": time.perf_counter() - started,
                    "before": before,
                    "after": after,
                })
        CURRENT["value"] = None
        if names:
            # What reads the named values is left outdated, and said.
            # What waits for an input nobody entered is not left: no run
            # would compute it.
            left = [entry for entry in sample._plan([], False, False, changed)
                    if entry[1] in ("outdated", "never computed", "formula changed")
                    and not missing_inputs(sample, stored, entry[0], waiting, made)]
            send({"pending": len(left)})
            break
        rerun = force = False
        plan = sample._plan([], False, False, changed)
    if writing:
        send({"computed": whole})
    if computed and writing:
        send({"saved": request["sample"]})
    elif computed:
        TRIED[request["sample"]] = sample
        TRIED_FORMULAS[request["sample"]] = (digests, whole)


def cells_of(sample, value):
    """The values a computation of `value` gave: a column's cells, or one; two
    where a value and its uncertainty have a formula each."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            if "." in value:
                table, column = value.split(".", 1)
                return max(1, len(sample[table].values(column)))
            return sample[value]._formulas
        except Exception:
            return 1


def edited(sample, value):
    """Whether `value` holds an override."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            return sample[value].state == "edited"
        except Exception:
            return False


def failed_before(sample, value):
    """Whether the file records a failure for `value`."""
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            return sample[value].state == "failed"
        except Exception:
            return False


def write_failures(key, sample):
    """Writes the tracebacks waiting for this sample, as its file is written."""
    for value, trace in FAILURES.pop(key, {}).items():
        record_failure(sample, value, trace)


def record_failure(sample, value, text):
    """Keeps a formula's whole traceback beside the project, not in the data.

    The sample file records the exception's type alone, so that a file stays
    data and stays portable — one real sample carried an absolute path sixteen
    times over. The rest lives in `.samplekit/failures/<sample>.log`, one block
    per value, replaced each time that value fails again, and `samplekit
    explain` reads it.

    A log that cannot be written never turns a computed value into a failed
    run: it is said, as a warning. Read and written in UTF-8, whatever the
    system's locale — under another, a traceback naming a file with an accent
    was lost, and the log left empty.
    """
    try:
        root = sample.project.root
        path = sample.path
        if root is None or path is None:
            return
        folder = pathlib.Path(root) / ".samplekit" / "failures"
        log = folder / failure_log_name(root, path)
        log.parent.mkdir(parents=True, exist_ok=True)
        blocks = {}
        if log.exists():
            current = None
            for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("## "):
                    current = line[3:].strip()
                    blocks[current] = []
                elif current is not None:
                    blocks[current].append(line)
        if text is None:
            blocks.pop(value, None)
        else:
            blocks[value] = text.rstrip().splitlines()
        written = []
        for name, lines in blocks.items():
            if not any(line.strip() for line in lines):
                continue
            written.append(f"## {name}\n" + "\n".join(lines).strip("\n") + "\n")
        if written:
            log.write_text("\n".join(written), encoding="utf-8")
        elif log.exists():
            log.unlink()
    except Exception as error:
        logging.getLogger("samplekit").warning(
            "the traceback of %s could not be kept: %s", value, error)


def forget_failure(sample, value):
    """A value's traceback taken out of its sample's log, once the value no
    longer fails: a log left behind named a failure that `status` no longer
    reported. The log goes with its last block."""
    try:
        root = sample.project.root
        if root is None or sample.path is None:
            return
        log = pathlib.Path(root) / ".samplekit" / "failures" / failure_log_name(root, sample.path)
        if log.exists():
            record_failure(sample, value, None)
    except Exception:
        pass


def failure_log_name(root, path):
    """A sample's failure log, named after its file **where it lies in the
    project** — `a/S1.md` logs to `a/S1.log` — so that two samples of one file
    name in two directories never share one, and `explain` never shows one's
    traceback for the other. A sample at the project's root keeps the name it
    always had. The command line names it the same way."""
    try:
        relative = pathlib.Path(path).resolve().relative_to(pathlib.Path(root).resolve())
    except ValueError:
        relative = pathlib.Path(pathlib.Path(path).name)
    return relative.with_suffix(".log")


class TimedOut(TimeoutError):
    """A formula `compute --timeout` stopped: its value is failed, and the
    others run."""

    def __str__(self):
        if self.args:
            return str(self.args[0])
        return f"the formula ran past --timeout {LIMIT['seconds']:g} s"


LIMIT = {"seconds": None}


def within(seconds, call):
    """`call()`, stopped by `TimedOut` raised in it past `seconds`: the thread
    that runs formulas is told from a timer's, as Ctrl-C would be, on every
    system. A formula inside one long call of a compiled library is stopped when
    that call returns."""
    if not seconds:
        return call()
    import ctypes

    main = ctypes.c_ulong(threading.get_ident())
    raise_in = ctypes.pythonapi.PyThreadState_SetAsyncExc
    timer = threading.Timer(seconds, lambda: raise_in(main, ctypes.py_object(TimedOut)))
    timer.daemon = True
    LIMIT["seconds"] = seconds
    timer.start()
    try:
        return call()
    finally:
        timer.cancel()
        timer.join()
        # One raised as the formula returned belongs to nothing: withdrawn.
        raise_in(main, None)


def model_traceback(error, formula=None):
    """The traceback from the model's own frames: SampleKit's are not the
    researcher's code, and would be the first thing read. Where none is the
    model's — a value refused as the formula returned it, a formula called
    with arguments it does not take — the formula's own definition stands for
    them: `_worker.py` was all the traceback named."""
    package = os.path.dirname(os.path.abspath(__file__)) + os.sep
    frames = [
        frame for frame in traceback.extract_tb(error.__traceback__)
        if not os.path.abspath(frame.filename).startswith(package)
    ]
    if not frames:
        place = formula_frame(formula)
        if place is None:
            return "".join(traceback.format_exception_only(type(error), error))
        frames = [place]
    return (
        "Traceback (most recent call last):\n"
        + "".join(traceback.format_list(frames))
        + "".join(traceback.format_exception_only(type(error), error))
    )


def formula_frame(formula):
    """Where a formula is defined, as a traceback's frame: its first line."""
    function = getattr(formula, "__func__", formula)
    code = getattr(function, "__code__", None)
    if code is None:
        return None
    line = linecache.getline(code.co_filename, code.co_firstlineno).strip()
    return traceback.FrameSummary(
        code.co_filename, code.co_firstlineno, getattr(function, "__qualname__", code.co_name),
        line=line or None,
    )


def formula_of(sample, value):
    """The function that computes `value`, as the model declares it: the first
    callable among its declaration's parts."""
    try:
        parts = sample._formula_code().get(value, [])
    except Exception:
        return None
    for _role, declared in parts:
        if callable(declared):
            return declared
        if isinstance(declared, (tuple, list)):
            for item in declared:
                if callable(item):
                    return item
    return None


# ------------------------------------------------------------ formula digests
#
# A value goes stale when its formula changed, and only then: each formula's
# digest is taken here, of its declaration and of the source it reaches, and the
# command line keeps it in the project's history. A figure, a comment, a
# docstring or a helper no formula calls change no digest.

# Each file's lines, read once per worker: the model is imported once per
# process, so the text read first is the text that runs, and a model edited
# during a run is recorded as the version that ran.
SOURCES = {}
# The normalised tokens of each block of source, by file and first line.
BLOCKS = {}
# Whether a file is of the model's own code, by directory and file.
OWN = {}
# What a value is when it names nothing plain.
NOT_PLAIN = object()


def source_lines(filename):
    lines = SOURCES.get(filename)
    if lines is None:
        try:
            with tokenize.open(filename) as handle:
                lines = handle.read().splitlines(keepends=True)
        except (OSError, SyntaxError, UnicodeDecodeError):
            lines = []
        SOURCES[filename] = lines
    return lines


def normalised(lines):
    """Source as its tokens: a comment, a blank line, a docstring, the
    indentation and where the block sits in its file change nothing, and any
    other change — a digit, a name, an operator — does. An f-string is one
    token, as it was before Python 3.12 split it, so that a newer interpreter
    stales nothing."""
    text = textwrap.dedent("".join(lines))
    rows = text.splitlines(keepends=True)
    tokens = []
    cut = False
    try:
        for token in tokenize.generate_tokens(io.StringIO(text).readline):
            tokens.append(token)
    except (tokenize.TokenError, IndentationError, SyntaxError):
        # A lambda's line cut from the call around it: what was read, and
        # the rest as text without its comments.
        cut = True
    significant = []
    depth, start = 0, None
    for token in tokens:
        kind = tokenize.tok_name.get(token.type, "")
        if kind in ("FSTRING_START", "TSTRING_START"):
            if depth == 0:
                start = token.start
            depth += 1
        elif kind in ("FSTRING_END", "TSTRING_END"):
            depth -= 1
            if depth == 0:
                significant.append(("STRING", spanned(rows, start, token.end)))
        elif depth or kind in ("COMMENT", "NL", "ENCODING", "ENDMARKER"):
            continue
        elif kind in ("INDENT", "DEDENT", "NEWLINE"):
            significant.append((kind, ""))
        else:
            significant.append((kind, token.string))
    out = []
    skip = False
    for at, (kind, string) in enumerate(significant):
        if skip:
            skip = False
            continue
        # A string standing alone first in a block, or in the file, is a
        # docstring: it documents, and computes nothing. Left out with the
        # line it ends, so that writing one changes nothing.
        before = significant[at - 1][0] if at else "INDENT"
        after = significant[at + 1][0] if at + 1 < len(significant) else "NEWLINE"
        if kind == "STRING" and before == "INDENT" and after == "NEWLINE":
            skip = True
        elif kind in ("INDENT", "DEDENT", "NEWLINE"):
            out.append("\x01" + kind)
        else:
            out.append(string)
    if cut:
        out.append("\x01rest")
        for line in rows:
            code = line.split("#", 1)[0].strip()
            if code:
                out.append(code)
    return "\x00".join(out)


def spanned(rows, start, end):
    """The text between two token positions, as the source spells it."""
    (first, first_column), (last, last_column) = start, end
    if first == last:
        return rows[first - 1][first_column:last_column]
    return (
        rows[first - 1][first_column:]
        + "".join(rows[first:last - 1])
        + rows[last - 1][:last_column]
    )


def unnamed(block):
    """A function's normalised source with its own name taken out of its
    `def`: `def (self):`, whatever the method is called."""
    if block is None:
        return None
    tokens = block.split("\x00")
    for at, token in enumerate(tokens):
        if token == "def" and at + 1 < len(tokens):
            return "\x00".join(tokens[:at + 1] + tokens[at + 2:])
    return block


def block_of(filename, first):
    """The normalised block of source starting on line `first` of a file: a
    function with its decorators, a class, or the statement holding a
    lambda."""
    key = (filename, first)
    if key not in BLOCKS:
        lines = source_lines(filename)
        if not lines or first < 1:
            BLOCKS[key] = None
        else:
            try:
                block = inspect.getblock(lines[first - 1:])
            except Exception:
                block = lines[first - 1:first]
            BLOCKS[key] = normalised(block)
    return BLOCKS[key]


def names_of(code):
    """Every name a code object uses, its nested functions and comprehensions
    included: globals, and attributes, since `self._helper()` reaches a
    method by an attribute's name."""
    names = set(code.co_names)
    for constant in code.co_consts:
        if isinstance(constant, types.CodeType):
            names |= names_of(constant)
    return names


def plain(value, depth=0, folder=None):
    """A value as its content, or NOT_PLAIN: a number, a text, a container of
    them, an array's bytes. A text naming a path under `folder` — the model's
    own, as `os.path.dirname(__file__)` gives it — is taken from that folder,
    so that a project moved or cloned elsewhere changes no formula."""
    if depth > 20:
        return NOT_PLAIN
    if isinstance(value, str) and folder is not None:
        base = str(folder)
        if value == base or value.startswith(base + os.sep):
            return repr("<model>" + value[len(base):].replace(os.sep, "/"))
    if value is None or isinstance(value, (bool, int, float, complex, str, bytes)):
        return repr(value)
    if isinstance(value, (tuple, list, set, frozenset)):
        items = [plain(item, depth + 1, folder) for item in value]
        if any(item is NOT_PLAIN for item in items):
            return NOT_PLAIN
        if isinstance(value, (set, frozenset)):
            items = sorted(items, key=repr)
        return [type(value).__name__, items]
    if isinstance(value, dict):
        pairs = [(plain(key, depth + 1, folder), plain(item, depth + 1, folder))
                 for key, item in value.items()]
        if any(key is NOT_PLAIN or item is NOT_PLAIN for key, item in pairs):
            return NOT_PLAIN
        return ["dict", sorted(pairs, key=repr)]
    if all(hasattr(value, name) for name in ("tobytes", "dtype", "shape")):
        try:
            return ["array", str(value.dtype), list(value.shape),
                    hashlib.sha256(value.tobytes()).hexdigest()]
        except Exception:
            return NOT_PLAIN
    return NOT_PLAIN


# What each value a formula reaches held the first time this process read it,
# kept with the value so that its id names no other while the process runs.
FIRST_READ = {}


def first_reading(value, folder, scope=None, name=None):
    """A value a formula reaches, as the process first read it: before any
    formula ran, as the model was imported. A cache a formula fills is read
    empty whatever it holds since, where its contents changed the digest from
    one sample to the next; a constant edited in the file is read anew by the
    next process. A module's global is known by its name in its module, so
    that one a formula binds anew — `global _DATA; _DATA = load()` — is still
    read as it first was."""
    key = (scope, name, folder) if scope is not None else (id(value), folder)
    if key not in FIRST_READ:
        FIRST_READ[key] = (value, plain(value, folder=folder))
    return FIRST_READ[key][1]


# The model classes whose digests were taken as they were first used.
PRIMED = set()


def prime(klass, model_path):
    """Takes a model's formula digests the first time it is used in this
    process, before any formula runs: the source is read then, as it runs, and
    each value its formulas reach as it was before them. Taken at the first
    save instead, a model edited meanwhile was recorded as the version that
    had not run, and a cache filled since was read full. One that cannot be
    taken now is taken at the save, which says why."""
    if klass in PRIMED:
        return
    PRIMED.add(klass)
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            sample = klass()
        formula_digests(sample, model_path, quiet=True)
    except Exception:
        pass


class Digests:
    """Takes each formula's digest for one model: its declaration, and the
    source of every function it reaches in the model's own code — the files
    under the templates directory, as the model's digest counts them. Code from
    elsewhere is taken by its name: upgrading a library stales nothing."""

    def __init__(self, directory, model):
        self.directory = pathlib.Path(directory).resolve()
        self.model = model
        self.bases = set(model.__mro__)

    def own(self, filename):
        """Whether a file is the model's own code: under the templates
        directory, and in no hidden directory, `__pycache__` or virtual
        environment, which the model's digest skips too."""
        if not filename:
            return False
        key = (self.directory, filename)
        if key not in OWN:
            OWN[key] = self._own(filename)
        return OWN[key]

    def _own(self, filename):
        try:
            relative = pathlib.Path(filename).resolve().relative_to(self.directory)
        except (OSError, ValueError):
            return False
        folders = relative.parts[:-1]
        if any(part.startswith(".") or part in ("__pycache__", "site-packages") for part in folders):
            return False
        here = self.directory
        for part in folders:
            here = here / part
            if (here / "pyvenv.cfg").is_file():
                return False
        return True

    def relative(self, filename):
        """A file of the model's own code, from the templates directory."""
        try:
            return pathlib.Path(filename).resolve().relative_to(self.directory).as_posix()
        except (OSError, ValueError):
            return str(filename)

    def formula(self, name, parts, sample):
        """One formula's digest: each part of its declaration, what its
        functions reach, and the names it is declared to read."""
        described = []
        seen = set()
        for role, given in parts:
            if isinstance(given, str):
                described.append([role, given])
            elif role in ("value", "uncertainty"):
                # A statistic, which says what it is.
                described.append([role, repr(given)])
            else:
                reached = []
                self.reference(role, given, reached, seen)
                described.append([role, reached])
        try:
            reads = sorted(sample.dependencies(name))
        except Exception:
            reads = None
        described.append(["reads", reads])
        text = json.dumps(described, default=repr)
        return hashlib.sha256(text.encode("utf-8", "surrogatepass")).hexdigest()

    def function(self, function, out, seen):
        code = function.__code__
        label = f"{getattr(function, '__module__', None)}.{function.__qualname__}"
        own = self.own(code.co_filename)
        # The model's own functions are taken without their names: renaming
        # one changes no number, and recomputed everything it gave.
        if id(code) in seen:
            out.append(["reached"] if own else ["reached", label])
            return
        seen.add(id(code))
        if not self.own(code.co_filename):
            # A decorator's wrapper, `functools.wraps` kept: what it wraps is
            # the model's code.
            wrapped = getattr(function, "__wrapped__", None)
            if wrapped is not None and wrapped is not function:
                self.reference("wrapped", wrapped, out, seen)
                return
            out.append(["library", label])
            return
        out.append(["function", unnamed(block_of(code.co_filename, code.co_firstlineno))])
        for default in function.__defaults__ or ():
            self.reference("default", default, out, seen)
        for key, default in sorted((function.__kwdefaults__ or {}).items()):
            self.reference(key, default, out, seen)
        for key, cell in zip(code.co_freevars, function.__closure__ or ()):
            try:
                held = cell.cell_contents
            except ValueError:
                continue
            self.reference(key, held, out, seen)
        known = function.__globals__
        for key in sorted(names_of(code)):
            if key in known:
                self.reference(key, known[key], out, seen, scope=id(known))
            self.attribute(key, out, seen)

    def attribute(self, key, out, seen):
        """A name the model's class holds, reached as `self.<key>`: its own
        method or constant, and none SampleKit's `Sample` gives it."""
        for klass in self.model.__mro__:
            if key in vars(klass):
                if klass.__module__.startswith("samplekit") or klass is object:
                    return
                # By its name alone, the class's being none of the formula's.
                self.reference(key, vars(klass)[key], out, seen)
                return

    def reference(self, key, value, out, seen, scope=None):
        """What a name a formula uses stands for, as far as it changes what the
        formula computes."""
        import samplekit

        if isinstance(value, samplekit.Sample):
            out.append(["sample", key])
        elif isinstance(value, types.FunctionType):
            self.function(value, out, seen)
        elif callable(value) and not isinstance(value, type) \
                and getattr(value, "__wrapped__", None) is not None:
            # `functools.lru_cache`, `functools.cache` and their kind wrap a
            # function of the model: its source is what it computes.
            self.reference(key, value.__wrapped__, out, seen)
        elif isinstance(value, (types.MethodType, staticmethod, classmethod)):
            self.reference(key, value.__func__, out, seen)
        elif isinstance(value, property):
            for accessor in (value.fget, value.fset, value.fdel):
                if accessor is not None:
                    self.reference(key, accessor, out, seen)
        elif isinstance(value, functools.partial):
            self.reference(key, value.func, out, seen)
            for argument in value.args:
                self.reference("argument", argument, out, seen)
            for name, argument in sorted(value.keywords.items()):
                self.reference(name, argument, out, seen)
        elif isinstance(value, type):
            self.klass(value, out, seen)
        elif isinstance(value, types.ModuleType):
            filename = getattr(value, "__file__", None)
            if not (filename and self.own(filename)):
                out.append(["library", value.__name__])
                return
            # The model's own module by its file, never by the name it was
            # imported under: `model.piece` and `piece` are one source.
            named = self.relative(filename)
            if ("module", filename) in seen:
                out.append(["reached", named])
            else:
                seen.add(("module", filename))
                out.append(["module", named, normalised(source_lines(filename))])
        else:
            content = first_reading(value, self.directory, scope=scope, name=key)
            if content is not NOT_PLAIN:
                out.append(["value", key, content])
            else:
                # An object: its class says what it does, and its state is
                # not a formula's.
                self.klass(type(value), out, seen)

    def klass(self, value, out, seen):
        """A class reached by name: the model's own — its source, whole — or
        another's, by its name. The model's class is reached by its
        attributes, one by one, so that a figure stales nothing."""
        label = f"{value.__module__}.{value.__qualname__}"
        if value in self.bases:
            # The model's class, whatever it is called.
            out.append(["model"])
            return
        filename = getattr(inspect.getmodule(value), "__file__", None)
        if not self.own(filename):
            out.append(["library", label])
            return
        # The model's own class by its file and its name there, never by the
        # module name it was imported under.
        label = f"{self.relative(filename)}:{value.__qualname__}"
        if ("class", id(value)) in seen:
            out.append(["reached", label])
            return
        seen.add(("class", id(value)))
        first = getattr(value, "__firstlineno__", None)
        if first is None:
            try:
                first = inspect.findsource(value)[1] + 1
            except (OSError, TypeError):
                first = 0
        out.append(["class", label, block_of(filename, first)])


def describe(model, model_path, root, native):
    """Writes the model's description in the project, `.samplekit/model.json`,
    once per model in this process: what a sample of it made without
    a file declares — as `Sample.new` makes one, so that no file's own field
    is taken for the model's — each formula's digest, and its figures and
    scripts. The file is written only where its content changed. Returns its path."""
    key = (os.path.realpath(model_path), model.__name__)
    if key in DESCRIBED:
        return DESCRIBED[key]
    from samplekit import _figures, _scripts

    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        sample = model()
    digests = formula_digests(sample, model_path)
    figures = [(name, bool(figure.collection), _scripts.first_paragraph(figure.__doc__))
               for name, figure in _figures.figures_of(model).items()]
    scripts = json.dumps([_scripts.described(script)
                          for script in _scripts.scripts_of(model).values()])
    path = native._describe_model(sample, str(model_path), model.__name__, str(root),
                                  digests, figures, scripts)
    DESCRIBED[key] = path
    return path


def forget_description(root):
    """Removes the description of a model that could not be read."""
    try:
        os.remove(os.path.join(root, ".samplekit", "model.json"))
    except OSError:
        pass


def formula_digests(sample, model_path, quiet=False):
    """Each formula's digest, by the value it gives. One that cannot be taken
    is said, and left out: its value is then judged by its inputs alone —
    said unless `quiet`, where it is taken again later and said then."""
    digests = {}
    taker = Digests(os.path.dirname(os.path.abspath(model_path)), type(sample))
    logger = logging.getLogger("samplekit")
    try:
        declared = sample._formula_code()
    except Exception as error:
        if not quiet:
            logger.warning("the formulas' digests could not be taken: %s", error)
        return digests
    for name, parts in declared.items():
        try:
            digests[name] = taker.formula(name, parts, sample)
        except Exception as error:
            if not quiet:
                logger.warning("the digest of %s's formula could not be taken: %s",
                               name, error)
    return digests


if __name__ == "__main__":
    main()
