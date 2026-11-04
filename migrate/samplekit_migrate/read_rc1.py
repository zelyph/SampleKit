"""A project of SampleKit 1.0.0-rc.1.

Its sample files are this version's, schema 1, and the records they hold of
what computed each value are read as current: they are copied as they are,
and so is the history. Its configuration and its model are this version's
but for a few names, renamed here.
"""

from __future__ import annotations

import ast
import posixpath
import re
import tomllib
from pathlib import Path, PurePosixPath

import yaml

from . import description as d
from .model_0x import TODO, ModelError
from .read_0x import CHECK, CARRIED, LEFT
from .source import Source, arguments, called, find_class, literal, method, quoted
from .walk import project_files

CONFIGURATION = ".samplekitrc"


def is_rc1_project(source: Path) -> bool:
    return (source / CONFIGURATION).is_file()


def read(source: Path, version: str) -> d.Project:
    """Every `.samplekitrc` of the tree renamed, the root's and those of the
    projects below it, and the model each names; the rest copied."""
    project = d.Project(version=version)
    models: dict[PurePosixPath, object] = {}
    files = project_files(source)
    for path in files:
        if path.name != CONFIGURATION:
            continue
        relative = PurePosixPath(path.relative_to(source).as_posix())
        text = path.read_text(encoding="utf-8")
        try:
            declared = tomllib.loads(text).get("model", {})
        except tomllib.TOMLDecodeError as error:
            raise ModelError(f"{path} is not TOML: {error}") from error
        project.rewritten[relative] = configuration(text, project, relative)
        if isinstance(declared, dict) and isinstance(declared.get("path"), str):
            named = PurePosixPath(posixpath.normpath((relative.parent / declared["path"]).as_posix()))
            if (source / named).is_file():
                models[named] = declared.get("class")
            else:
                project.say(CHECK, f"{relative}: its model, {declared['path']}, is not in the project: not renamed")
    for path in files:
        relative = PurePosixPath(path.relative_to(source).as_posix())
        if relative in project.rewritten:
            continue
        if relative in models:
            described = model(path.read_text(encoding="utf-8"), relative, models[relative], project)
            project.rewritten[relative] = described.text
            if project.model is None:
                project.model = described
            continue
        if path.suffix == ".md" and is_sample(path):
            project.samples_as_they_are.append(relative)
            continue
        project.copied.append(relative)
    project.say(
        CARRIED,
        f"{len(project.samples_as_they_are)} sample files, copied as they are: this version "
        "reads rc.1's files and the records of what computed each value",
    )
    return project


def is_sample(path: Path) -> bool:
    text = path.read_text(encoding="utf-8", errors="replace")
    if not text.startswith("---"):
        return False
    end = text.find("\n---", 3)
    try:
        data = yaml.safe_load(text[4:end]) if end != -1 else None
    except yaml.YAMLError:
        return False
    return isinstance(data, dict) and "schema_version" in data


# ── The configuration ────────────────────────────────────

HEADER = re.compile(r"^\s*\[\s*([^\]]+?)\s*\]\s*(#.*)?$")
KEY = re.compile(r"^(\s*)([A-Za-z0-9_\-\"\.]+)(\s*=\s*)(.*)$")


def configuration(text: str, project: d.Project, relative: PurePosixPath = PurePosixPath(CONFIGURATION)) -> str:
    """rc.1's `.samplekitrc` with this version's names, line by line, so that
    its comments and its order stay."""
    parsed = tomllib.loads(text)
    lines = text.splitlines(keepends=True)
    section = ""
    out: list[str] = []
    has_python = isinstance(parsed.get("model"), dict) and "python" in parsed["model"]
    for line in lines:
        header = HEADER.match(line)
        if header:
            section = header.group(1).strip()
            if section == "workbench.colors":
                line = line.replace("workbench.colors", "colors", 1)
                project.say(CARRIED, f"{relative}: [workbench.colors] is [colors]")
                section = "colors"
            out.append(line)
            if section == "model" and not has_python:
                out.append('python = ".venv"\n')
                project.say(
                    CARRIED,
                    f'{relative}: [model] python = ".venv", the environment this version runs the model in',
                )
            continue
        key = KEY.match(line)
        if key and section == "colors" and key.group(2) == "stale":
            line = f"{key.group(1)}outdated{key.group(3)}{key.group(4)}\n"
            project.say(CARRIED, f"{relative}: the colour stale is outdated")
        elif key and section.startswith("style.") and key.group(2) == "separator":
            style = section[len("style."):]
            separator = parsed.get("style", {}).get(style, {}).get("separator", "±")
            template = f"{{value}} {separator} {{uncertainty}} {{unit}}"
            written = f"'{template}'" if "'" not in template else quoted(template)
            line = f"{key.group(1)}property{key.group(3)}{written}\n"
            project.say(CARRIED, f"{relative}: [{section}] separator is property = {written}")
        if "{collection}" in line:
            line = line.replace("{collection}", "{project}")
            project.say(CARRIED, f"{relative}: {{collection}} is {{project}}")
        out.append(line)
    return "".join(out)


# ── The model ────────────────────────────────────────────

RENAMED = {"compute": "compute_value", "compute_quantity": "compute"}


def model(text: str, path: PurePosixPath, class_name, project: d.Project) -> d.Model:
    try:
        source = Source(text)
    except SyntaxError as error:
        raise ModelError(f"{path} is not Python this tool can read: {error}") from error
    classes = find_class(source.tree, class_name if isinstance(class_name, str) else None)
    if len(classes) != 1:
        raise ModelError(f"{path}: the model's class is not found; --class names it")
    cls = classes[0]
    described = d.Model(path=path, class_name=cls.name)
    init = method(cls, "__init__")
    tables: set[int] = set()
    if init is not None:
        for node in ast.walk(init):
            if called(node, "Table") and table(node, source, project):
                tables.add(id(node))
        for node in ast.walk(init):
            if not isinstance(node, ast.Call) or any(id(t) in tables for t in _enclosing(init, node)):
                continue
            if called(node, "Property"):
                for keyword in node.keywords:
                    if keyword.arg in RENAMED:
                        start = source.offset(keyword.lineno, keyword.col_offset)
                        source.replace(start, start + len(keyword.arg), RENAMED[keyword.arg])
                        project.say(CARRIED, f"{path}: {keyword.arg}= is {RENAMED[keyword.arg]}=")
            elif called(node, "Column"):
                func = node.func
                end = source.offset(func.end_lineno, func.end_col_offset)
                source.replace(end - len("Column"), end, "Property")
                project.say(CARRIED, f"{path}: a table's Column is a Property")
    # What rc.1 had and this version has not, wherever it is used.
    marked: set[int] = set()
    for node in ast.walk(source.tree):
        if isinstance(node, ast.Attribute) and node.attr in ("stale", "is_stale", "invalidate"):
            if node.lineno in marked:
                continue
            marked.add(node.lineno)
            said = {
                "stale": "stale() is outdated()",
                "is_stale": "is_stale is is_outdated",
                "invalidate": "invalidate() is gone: compute(force=True) gives a value back to its formula",
            }[node.attr]
            source.insert_line_before(node.lineno, f"{TODO}: {said}")
            project.say(CHECK, f"{path}, line {node.lineno}: {said}")
    described.text = source.result()
    return described


def _enclosing(root: ast.AST, target: ast.AST) -> list[ast.AST]:
    """The calls enclosing `target` in `root`."""
    found: list[ast.AST] = []

    def visit(node: ast.AST, path: list[ast.AST]) -> bool:
        if node is target:
            found.extend(path)
            return True
        return any(visit(child, [*path, node]) for child in ast.iter_child_nodes(node))

    visit(root, [])
    return found


def table(call: ast.Call, source: Source, project: d.Project) -> bool:
    """rc.1's `Table(columns, index, compute_rows=…, compute_columns=…)`
    written as this version declares it; False where it is already so."""
    given = arguments(call, ["columns", "index"])
    rows, columns_whole = given.get("compute_rows"), given.get("compute_columns")
    positional_index = len(call.args) > 1
    columns = given.get("columns")
    has_column = isinstance(columns, ast.Dict) and any(called(v, "Column") for v in columns.values)
    if rows is None and columns_whole is None and not positional_index and not has_column:
        return False
    if not isinstance(columns, ast.Dict):
        project.say(CHECK, f"line {call.lineno}: a table whose columns are not a dict, left as it was")
        return False
    indent = source.indent_of(call.lineno)
    inner = indent + "    "
    column_indent = inner + "    "
    extra: dict[str, list[str]] = {}
    computed: list[str] = []

    def entries(node, kind: str) -> None:
        for entry in getattr(node, "elts", []):
            if not isinstance(entry, (ast.Tuple, ast.List)) or len(entry.elts) != 3:
                project.say(CHECK, f"line {entry.lineno}: an entry of {kind} this tool cannot read")
                continue
            outputs, inputs, function = entry.elts
            names = literal(outputs)
            reads = source.segment(inputs)
            formula = source.segment(function)
            keyword = "compute" if kind == "compute_rows" else "compute_column"
            if isinstance(names, str):
                extra.setdefault(names, []).extend([f"{keyword}={formula}", f"depends_on={reads}"])
            else:
                listed = ", ".join(quoted(n) for n in names or [])
                computed.append(
                    f"sk.ComputedColumns([{listed}], {keyword}={formula}, depends_on={reads})"
                )
            project.say(CARRIED, f"a table's {kind} entry is its column's formula")

    entries(rows, "compute_rows")
    entries(columns_whole, "compute_columns")
    parts = []
    for key, value in zip(columns.keys, columns.values):
        name = literal(key)
        if called(value, "Column"):
            inside = [source.segment(a) for a in value.args] + [
                source.segment(k) if k.arg is None else f"{k.arg}={source.segment(k.value)}"
                for k in value.keywords
            ]
            inside += extra.get(name, [])
            written = f"sk.Property({', '.join(inside)})"
        else:
            written = source.segment(value)
            if name in extra:
                project.say(CHECK, f"line {value.lineno}: {name}'s formula could not be joined to its column")
        parts.append(f"{column_indent}{source.segment(key)}: {written},")
    lines = [f"{source.segment(call.func)}(\n{inner}{{"] + parts + [f"{inner}}},"]
    index = given.get("index")
    if index is not None:
        lines.append(f"{inner}index={source.segment(index)},")
    for keyword in call.keywords:
        if keyword.arg in ("columns", "index", "compute_rows", "compute_columns"):
            continue
        lines.append(f"{inner}{keyword.arg}={source.segment(keyword.value)},")
    if computed:
        lines.append(f"{inner}computed=[")
        lines += [f"{column_indent}{c}," for c in computed]
        lines.append(f"{inner}],")
    lines.append(f"{indent})")
    source.replace_node(call, "\n".join(lines))
    return True
