"""A project of SampleKit 0.x: its sample files, read with its model.

The four 0.x versions published, 0.1.0 to 0.2.0, wrote the same files: a
frontmatter holding `name`, each property as a number or a mapping of
`value`, `data`, `uncertainty`, `unit` and how it is shown, each table as
`_title`, `_index`, `_columns` and `_rows`; and below it a body written from
`template()`. They differ in the model's `__init__` alone, which the model's
reader takes either way.
"""

from __future__ import annotations

import datetime
import math
import statistics
from pathlib import Path, PurePosixPath

import yaml

from . import description as d
from .model_0x import ModelError, Reader
from .source import method
from .walk import project_files

LEFT = "Left behind"
CHECK = "To check by hand"
CARRIED = "Carried"
CONFIGURATION = "Declared once in .samplekitrc"


def frontmatter(text: str) -> tuple[dict | None, str]:
    """0.x's own split: a frontmatter between `---` lines, and the rest."""
    if not text.startswith("---"):
        return None, text
    end = text.find("\n---", 3)
    if end == -1:
        return None, text
    try:
        data = yaml.safe_load(text[4:end])
    except yaml.YAMLError:
        return None, text
    return (data if isinstance(data, dict) else None), text[end + 4 :].lstrip("\n")


def is_0x_sample(data: dict | None) -> bool:
    return isinstance(data, dict) and "name" in data and "schema_version" not in data


def unit_of(value) -> str | None:
    if not isinstance(value, str) or value in ("", "-"):
        return None
    return value


def number(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def close(a: float, b: float) -> bool:
    return math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-12)


def read(source: Path, model_path: Path | None, class_name: str | None, version: str) -> d.Project:
    project = d.Project(version=version)
    reader = None
    if model_path is not None:
        try:
            relative = model_path.resolve().relative_to(source.resolve())
            placed = PurePosixPath(relative.as_posix())
        except ValueError:
            placed = PurePosixPath("model") / model_path.name
        reader = Reader(model_path.read_text(encoding="utf-8"), placed, class_name)
        project.model = reader.model
        if reader.defaults:
            project.say(
                LEFT,
                "the model's default values, "
                + ", ".join(reader.defaults)
                + ": 0.x wrote them in every file it saved, and they are carried from there; "
                "this version's model declares none",
            )

    readings: list[str] = []
    values_seen: dict[str, list] = {}
    for path in project_files(source):
        relative = PurePosixPath(path.relative_to(source).as_posix())
        if model_path is not None and path.resolve() == model_path.resolve():
            continue
        if path.suffix == ".md":
            data, body = frontmatter(path.read_text(encoding="utf-8"))
            if is_0x_sample(data):
                sample = read_sample(relative, data, body, project, reader, readings, values_seen)
                project.samples.append(sample)
                continue
        project.copied.append(relative)

    if not project.samples:
        raise ModelError(f"{source} holds no sample file of SampleKit 0.x")

    # A property whose values are no numbers, text, dates, flags, is an
    # attribute; one where they mix with numbers is an attribute too, said.
    attributes: set[str] = set()
    for name, values in values_seen.items():
        kinds = {number(v) for v in values if v is not None}
        if kinds == {False}:
            attributes.add(name)
        elif kinds == {True, False}:
            attributes.add(name)
            project.say(
                CHECK,
                f"{name}: numbers in some files and text in others: carried as an attribute in each, "
                "its unit and uncertainty, if any, left behind",
            )
    if reader is not None:
        attributes |= {n for n in reader.attribute_defaults if n in values_seen}
    for sample in project.samples:
        for name in list(sample.properties):
            if name in attributes:
                sample.attributes[name] = sample.properties.pop(name).value
    if reader is not None:
        reader.write(readings, attributes)
        declared = {n for c in reader.declared.values() for n in c.properties}
        for name in sorted(attributes & declared):
            project.say(CARRIED, f"{name}: no number, written as an attribute of each sample, and out of the model")
        for line in reader.unread:
            project.say(CHECK, f"{reader.model.path}: {line}")
        if reader.others:
            project.say(
                CHECK,
                f"{reader.model.path}: the classes {', '.join(reader.others)} are rewritten too, and the "
                f"project takes {reader.cls.name}: samples saved with another class are read as "
                f"{reader.cls.name}'s; move them to a project of their own and migrate it apart",
            )
        if reader.model.copied:
            project.say(
                CHECK,
                f"{reader.model.path}: the methods {', '.join(reader.model.copied)}, copied as 0.x "
                "wrote them, each marked # TODO(migrate). A formula is too varied to be translated: "
                "read each against this version's API (a table's columns are read "
                "self.curve.values(\"stress\"), a row's cells row.force.value)",
            )
        if any(method(c, "template") is not None for c in reader.classes):
            project.say(LEFT, f"{reader.model.path}: template(), taken out of the model")
        for name in readings:
            project.say(
                CARRIED,
                f"{name}: readings, given 0.x's statistic, their mean and sample standard deviation",
            )
    else:
        for name in readings:
            project.say(
                CARRIED,
                f"{name}: readings, and beside them the mean and spread 0.x wrote, as values typed: "
                "no model gives them a statistic",
            )
    scripts = [str(p) for p in project.copied if p.suffix == ".py"]
    if scripts:
        project.say(
            CHECK,
            f"copied as they were, written for 0.x: {', '.join(scripts)}; the upgrade guide's "
            "section on scripts says what changed",
        )
    project.shown = merged_shown(project, reader)
    return project


def computed_names(reader: Reader | None) -> tuple[set[str], set[str], dict[str, set[str]]]:
    """What the model computes: values, uncertainties, and table columns."""
    if reader is None:
        return set(), set(), {}
    values = {n for n, p in reader.model.properties.items() if p.value is not None}
    uncertainties = {n for n, p in reader.model.properties.items() if p.uncertainty is not None}
    columns = {
        t: {c for c, col in table.columns.items() if col.value is not None or col.uncertainty is not None}
        for t, table in reader.model.tables.items()
    }
    return values, uncertainties, columns


def read_sample(
    relative: PurePosixPath,
    data: dict,
    body: str,
    project: d.Project,
    reader: Reader | None,
    readings: list[str],
    values_seen: dict[str, list],
) -> d.Sample:
    name = data.get("name")
    sample = d.Sample(
        path=relative,
        name=str(name) if name is not None and str(name) != relative.stem else None,
        note=body,
    )
    if body.strip():
        project.say(
            CARRIED,
            "the text below each frontmatter, as the note, as it is: what 0.x wrote there "
            "from template() is a copy of the values then, to remove where it is one",
        )
    values, uncertainties, columns = computed_names(reader)
    for key, raw in data.items():
        if key == "name":
            continue
        if isinstance(raw, dict) and ("_rows" in raw or "_columns" in raw):
            sample.tables[key] = read_table(key, raw, project, columns.get(key, set()))
            continue
        if isinstance(raw, dict):
            value = raw.get("value")
            held = raw.get("data")
            uncertainty = raw.get("uncertainty")
            unit = unit_of(raw.get("unit"))
            note_shown(project, key, raw)
        else:
            value, held, uncertainty, unit = raw, None, None, None
        if value is not None and not number(value) and held is None:
            values_seen.setdefault(key, []).append(value)
            sample.properties[key] = d.Quantity(value=value)
            continue
        values_seen.setdefault(key, []).append(value)
        quantity = d.Quantity(value=value, uncertainty=uncertainty, unit=unit)
        if isinstance(held, list) and held:
            quantity.readings = [float(x) if number(x) else x for x in held]
            quantity.origin = d.READINGS
            spread = statistics.stdev(held) if len(held) > 1 else None
            if reader is not None and uncertainty is not None and (spread is None or not close(uncertainty, spread)):
                quantity.typed_uncertainty = True
                project.say(
                    CHECK,
                    f"{relative}: {key}, an uncertainty typed over its readings, {uncertainty:g}: "
                    "carried beside them, and compute writes the statistic's in its place; "
                    "to keep it, set it again after computing",
                )
            elif reader is not None:
                quantity.uncertainty = None
            if reader is not None:
                quantity.value = None
            if key not in readings:
                readings.append(key)
        elif key in values:
            quantity.origin = d.COMPUTED
        if key in uncertainties and quantity.origin != d.READINGS:
            quantity.origin = d.COMPUTED
        sample.properties[key] = quantity
    return sample


def read_table(name: str, raw: dict, project: d.Project, computed: set[str]) -> d.Table:
    columns: dict[str, str | None] = {}
    for entry in raw.get("_columns") or []:
        if isinstance(entry, dict) and entry.get("name"):
            column = entry["name"]
            columns[column] = unit_of(entry.get("unit"))
            note_shown(project, f"{name}.{column}", entry)
    index = raw.get("_index") or next(iter(columns), "")
    rows: list[dict[str, d.Quantity]] = []
    for row in raw.get("_rows") or []:
        if not isinstance(row, dict):
            continue
        cells: dict[str, d.Quantity] = {}
        for column, cell in row.items():
            if column not in columns:
                columns[column] = None
            if isinstance(cell, dict):
                quantity = d.Quantity(value=cell.get("value"), uncertainty=cell.get("uncertainty"))
            else:
                quantity = d.Quantity(value=cell)
            if column in computed:
                quantity.origin = d.COMPUTED
            cells[column] = quantity
        rows.append(cells)
    title = raw.get("_title")
    return d.Table(index=[index], columns=columns, rows=rows, title=title if isinstance(title, str) else None)


SHOWN_KEYS = ("unit", "unit_math", "symbol", "symbol_math", "precision", "precision_unc")


def note_shown(project: d.Project, field: str, raw: dict) -> None:
    """How a file shows a property, gathered to be declared once."""
    held = project.shown.get(field)
    found = d.Shown(
        unit=unit_of(raw.get("unit")),
        symbol=raw.get("symbol") if isinstance(raw.get("symbol"), str) else None,
        symbol_math=raw.get("symbol_math") if isinstance(raw.get("symbol_math"), str) else None,
        precision=raw.get("precision") if isinstance(raw.get("precision"), str) and raw.get("precision") else None,
        precision_uncertainty=raw.get("precision_unc") if isinstance(raw.get("precision_unc"), str) else None,
    )
    unit_math = raw.get("unit_math")
    if isinstance(unit_math, str) and found.unit and unit_math != found.unit:
        if project.unit_math.setdefault(found.unit, unit_math) != unit_math:
            project.say(
                CHECK,
                f"the unit {found.unit}: written {project.unit_math[found.unit]} and {unit_math} "
                "in math; the first is declared",
            )
    if held is None:
        project.shown[field] = found
        return
    for key in ("symbol", "symbol_math", "precision", "precision_uncertainty"):
        mine, theirs = getattr(held, key), getattr(found, key)
        if theirs and not mine:
            setattr(held, key, theirs)
        elif theirs and mine != theirs:
            project.say(CHECK, f"{field}: its {key.replace('_', ' ')} is {mine} in a file and {theirs} in another; {mine} is declared")


def merged_shown(project: d.Project, reader: Reader | None) -> dict[str, d.Shown]:
    """The model's forms first, then the files'."""
    shown = dict(project.shown)
    if reader is not None:
        for field, from_model in reader.shown.items():
            held = shown.setdefault(field, d.Shown())
            for key in ("symbol", "symbol_math", "precision", "precision_uncertainty"):
                if getattr(from_model, key) and not getattr(held, key):
                    setattr(held, key, getattr(from_model, key))
    # 0.x wrote the name as the symbol where none was given: no symbol.
    for field, held in shown.items():
        last = field.rsplit(".", 1)[-1]
        if held.symbol == last:
            held.symbol = None
        if held.symbol_math in (last, held.symbol):
            held.symbol_math = None if held.symbol_math == last else held.symbol_math
        if held.precision_uncertainty == held.precision:
            held.precision_uncertainty = None
    return shown
