"""The project described, written as this version writes it.

The samples are written by the `samplekit` package installed in the Python
that runs this tool: what it writes is that version's by construction. The
configuration and the model are text, written here.
"""

from __future__ import annotations

import shutil
from pathlib import Path, PurePosixPath

from . import description as d

NOT_CARRIED = "Not carried: computed again"
KEPT = "Computed by 0.x, carried as typed over the formula (compute --force gives it back to it)"


def plan(project: d.Project) -> list[PurePosixPath]:
    """The files the target will hold, in the order they are written."""
    if project.rewritten:
        files = list(project.rewritten)
    else:
        files = [PurePosixPath(".samplekitrc")]
        if project.model is not None:
            files.append(project.model.path)
    files += [sample.path for sample in project.samples]
    files += project.samples_as_they_are
    files += project.copied
    return files


def configuration_0x(project: d.Project) -> str:
    """A `.samplekitrc` for a project 0.x kept none of: its model, and how
    its values are shown, which 0.x wrote in every file."""
    lines = ["schema_version = 1", ""]
    if project.model is not None:
        lines += [
            "[model]",
            f'path = "{project.model.path}"',
            f'class = "{project.model.class_name}"',
            'python = ".venv"',
            "",
        ]
    math = any(shown.symbol_math for shown in project.shown.values()) or bool(project.unit_math)
    if math:
        lines += [
            "# The forms 0.x wrote as symbol_math and unit_math: --style math writes them.",
            "[style.math]",
            "",
        ]
    for field in sorted(project.shown):
        shown = project.shown[field]
        keys = []
        if shown.symbol:
            keys.append(f"symbol = {toml_string(shown.symbol)}")
        if shown.symbol_math:
            keys.append(f"symbol_math = {toml_string(shown.symbol_math)}")
        if shown.precision and shown.precision_uncertainty:
            keys.append(
                f"precision = [{toml_string(shown.precision)}, {toml_string(shown.precision_uncertainty)}]"
            )
        elif shown.precision:
            keys.append(f"precision = {toml_string(shown.precision)}")
        elif shown.precision_uncertainty:
            keys.append(f'precision = ["", {toml_string(shown.precision_uncertainty)}]')
        if not keys:
            continue
        section = f'[property."{field}"]' if "." in field else f"[property.{field}]"
        lines += [section, *keys, ""]
    for unit in sorted(project.unit_math):
        lines += [f"[unit.{toml_string(unit)}]", f"math = {toml_string(project.unit_math[unit])}", ""]
    return "\n".join(lines).rstrip("\n") + "\n"


def toml_string(text: str) -> str:
    """A TOML string: literal, as a backslash of LaTeX stays one, where it can."""
    if "'" not in text and "\n" not in text:
        return f"'{text}'"
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def write(project: d.Project, source: Path, target: Path, keep_computed: bool) -> list[str]:
    """Writes the target; returns what it says of each sample left out."""
    import samplekit as sk

    # Refused where it holds anything; an empty folder is taken.
    target.mkdir(parents=True, exist_ok=True)
    written = dict(project.rewritten)
    if not written:
        written[PurePosixPath(".samplekitrc")] = configuration_0x(project)
        if project.model is not None:
            written[project.model.path] = project.model.text
    for relative, text in written.items():
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8")
    for relative in [*project.copied, *project.samples_as_they_are]:
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source / relative, destination)
    history = source / ".samplekit" / "history"
    if project.samples_as_they_are and history.is_dir() and not (target / ".samplekit" / "history").exists():
        shutil.copytree(history, target / ".samplekit" / "history", symlinks=True)
    failures: list[str] = []
    for sample in project.samples:
        path = target / sample.path
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            write_sample(sk, sample, path, keep_computed, project)
        except Exception as error:  # noqa: BLE001 - one sample's refusal is said, the others go on
            failures.append(f"{sample.path}: {type(error).__name__}: {error}")
    return failures


def carried(quantity: d.Quantity, keep_computed: bool) -> bool:
    return quantity.origin != d.COMPUTED or keep_computed


def write_sample(sk, sample: d.Sample, path: Path, keep_computed: bool, project: d.Project) -> None:
    written = sk.Sample.new(path, name=sample.name, model=False)
    for name, value in sample.attributes.items():
        setattr(written, name, value)
    for name, quantity in sample.properties.items():
        if not carried(quantity, keep_computed):
            project.say(NOT_CARRIED, name)
            continue
        if quantity.origin == d.COMPUTED:
            project.say(KEPT, name)
        if quantity.readings is not None:
            setattr(written, name, sk.Property(quantity.readings, quantity.uncertainty, unit=quantity.unit))
        elif quantity.value is not None:
            setattr(written, name, sk.Property(quantity.value, quantity.uncertainty, unit=quantity.unit))
    for name, table in sample.tables.items():
        columns = {}
        kept = []
        for column, unit in table.columns.items():
            whole = [row.get(column) for row in table.rows]
            if any(cell is not None and not carried(cell, keep_computed) for cell in whole):
                project.say(NOT_CARRIED, f"{name}.{column}")
                continue
            if any(cell is not None and cell.origin == d.COMPUTED for cell in whole):
                project.say(KEPT, f"{name}.{column}")
            columns[column] = sk.Property(unit=unit)
            kept.append(column)
        rows = []
        for row in table.rows:
            cells = {}
            for column in kept:
                cell = row.get(column)
                if cell is None or cell.value is None:
                    continue
                cells[column] = (
                    sk.Property(cell.value, cell.uncertainty) if cell.uncertainty is not None else cell.value
                )
            rows.append(cells)
        setattr(
            written,
            name,
            sk.Table(columns, index=table.index if len(table.index) > 1 else table.index[0], title=table.title, rows=rows),
        )
    written.note = sample.note
    written.save()
