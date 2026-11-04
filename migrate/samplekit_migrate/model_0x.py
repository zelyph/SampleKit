"""A model of SampleKit 0.x, read by its syntax tree and written in this
version's API.

It is never run: the earlier package is not installed beside this one. Every
class of samples in the module is understood, a base class and the classes
deriving from it alike: their `__init__`, `Property`, `Table` and `Column` by
their arguments, written again; every other method is copied as it is, marked
to check.
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass, field
from pathlib import PurePosixPath

from . import description as d
from .source import Source, arguments, called, first_line, literal, method, on_self, quoted

# 0.x's positional order of each constructor's arguments.
PROPERTY_ARGS = [
    "value", "uncertainty", "unit", "unit_math", "symbol", "symbol_math",
    "precision", "precision_unc", "compute", "compute_unc", "depends_on",
]
TABLE_ARGS = ["columns", "index", "data", "compute", "compute_unc", "compute_row", "title"]
COLUMN_ARGS = ["unit", "unit_math", "symbol", "symbol_math", "precision", "precision_unc"]

TODO = "# TODO(migrate)"
CHECK_METHOD = f"{TODO}: copied as SampleKit 0.x wrote it; check it against this version's API"
CHECK_LAMBDA = f"{TODO}: a formula copied as SampleKit 0.x wrote it; check it"
STATISTIC_NOTE = (
    "# 0.x gave readings their mean and their sample standard deviation: kept so.\n"
    "# The documentation advises sk.stats.standard_error, the uncertainty of their mean."
)
# What 0.x exported that this version exports by the same name.
CURRENT_NAMES = {"Sample", "Property", "Table", "SampleList"}
SIMPLE_CALLABLE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$")


class ModelError(Exception):
    pass


def unit_of(node: ast.AST | None) -> str | None:
    unit = literal(node)
    if not isinstance(unit, str) or unit in ("", "-"):
        return None
    return unit


def shown_of(given: dict[str, ast.AST]) -> d.Shown:
    def text(key: str) -> str | None:
        value = literal(given.get(key))
        return value if isinstance(value, str) and value else None

    return d.Shown(
        unit=unit_of(given.get("unit")),
        symbol=text("symbol"),
        symbol_math=text("symbol_math"),
        precision=text("precision"),
        precision_uncertainty=text("precision_unc"),
    )


def call_of(source: str, arguments_: str) -> str:
    """A callable written in the source, called: a lambda in parentheses, or
    its body would be the thing called."""
    if SIMPLE_CALLABLE.match(source):
        return f"{source}({arguments_})"
    return f"({source})({arguments_})"


def listed(names: list[str]) -> str:
    return "[" + ", ".join(f'"{n}"' for n in names) + "]"


@dataclass
class Declared:
    """What one class's `__init__` declares, and where."""

    cls: ast.ClassDef
    init: ast.FunctionDef | None
    properties: dict[str, d.Declaration] = field(default_factory=dict)
    tables: dict[str, d.TableDeclaration] = field(default_factory=dict)
    statements: dict[str, ast.Assign] = field(default_factory=dict)


def sample_classes(tree: ast.Module) -> list[ast.ClassDef]:
    """Every class of samples in the module, in its order: those deriving from
    `Sample`, and from one of them, at any depth."""
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
    found: list[ast.ClassDef] = []
    names: set[str] = set()
    changed = True
    while changed:
        changed = False
        for cls in classes:
            if cls in found:
                continue
            for base in cls.bases:
                if (isinstance(base, ast.Name) and (base.id == "Sample" or base.id in names)) or (
                    isinstance(base, ast.Attribute) and base.attr == "Sample"
                ):
                    found.append(cls)
                    names.add(cls.name)
                    changed = True
                    break
    return [cls for cls in classes if cls in found]


class Reader:
    """A module: its classes of samples, the one the project takes, and what
    their `__init__` declare."""

    def __init__(self, text: str, path: PurePosixPath, class_name: str | None):
        try:
            self.source = Source(text)
        except SyntaxError as error:
            raise ModelError(f"{path} is not Python this tool can read: {error}") from error
        self.path = path
        self.classes = sample_classes(self.source.tree)
        if not self.classes:
            raise ModelError(f"{path} holds no class deriving from Sample")
        by_name = {cls.name: cls for cls in self.classes}
        derived = {
            base.id
            for cls in self.classes
            for base in cls.bases
            if isinstance(base, ast.Name) and base.id in by_name
        }
        leaves = [cls.name for cls in self.classes if cls.name not in derived]
        if class_name is not None:
            if class_name not in by_name:
                raise ModelError(
                    f"{path} holds no class of samples named {class_name}: "
                    + ", ".join(by_name)
                )
            chosen = by_name[class_name]
        elif len(leaves) == 1:
            chosen = by_name[leaves[0]]
        else:
            raise ModelError(
                f"{path} holds several classes of samples, {', '.join(leaves)}: a project of this "
                "version has one class. --class names the one these samples were saved with; "
                "samples of another class go to a project of their own, migrated apart"
            )
        self.cls = chosen
        # The chosen class and its bases in the module, the base first.
        self.ancestry: list[ast.ClassDef] = []
        at: ast.ClassDef | None = chosen
        while at is not None:
            self.ancestry.insert(0, at)
            parent = next(
                (b.id for b in at.bases if isinstance(b, ast.Name) and b.id in by_name), None
            )
            at = by_name.get(parent) if parent else None
        self.others = [cls.name for cls in self.classes if cls not in self.ancestry]
        self.model = d.Model(path=path, class_name=chosen.name)
        self.shown: dict[str, d.Shown] = {}
        self.defaults: list[str] = []
        self.unread: list[str] = []
        # Names a declaration gives a value that is no number: an attribute.
        self.attribute_defaults: set[str] = set()
        self.declared: dict[str, Declared] = {}
        for cls in self.classes:
            declared = Declared(cls=cls, init=method(cls, "__init__"))
            if declared.init is not None:
                self._read_init(declared)
            self.declared[cls.name] = declared
        for cls in self.ancestry:
            self.model.properties.update(self.declared[cls.name].properties)
            self.model.tables.update(self.declared[cls.name].tables)
        self.attributes: set[str] = set()

    # ── Understanding ────────────────────────────────────

    def _formula(self, node: ast.AST | None) -> d.Formula | None:
        if node is None or isinstance(node, ast.Constant):
            return None
        return d.Formula(source=self.source.segment(node))

    def _read_init(self, declared: Declared) -> None:
        for statement in declared.init.body:
            if not (isinstance(statement, ast.Assign) and len(statement.targets) == 1):
                continue
            name = on_self(statement.targets[0])
            if name is None:
                continue
            if called(statement.value, "Property"):
                self._read_property(declared, name, statement)
            elif called(statement.value, "Table"):
                self._read_table(declared, name, statement)

    def _read_property(self, declared: Declared, name: str, statement: ast.Assign) -> None:
        given = arguments(statement.value, PROPERTY_ARGS)
        declaration = d.Declaration(
            name=name,
            unit=unit_of(given.get("unit")),
            value=self._formula(given.get("compute")),
            uncertainty=self._formula(given.get("compute_unc")),
        )
        depends = given.get("depends_on")
        if depends is not None:
            names = []
            for element in getattr(depends, "elts", []):
                read = on_self(element)
                if read is None:
                    self.unread.append(f"{name}: depends_on names {self.source.segment(element)}")
                else:
                    names.append(read)
            for formula in (declaration.value, declaration.uncertainty):
                if formula is not None:
                    formula.depends_on = names
        default = given.get("value")
        if default is not None and declaration.value is None:
            value = literal(default)
            if isinstance(value, (str, bool)):
                self.attribute_defaults.add(name)
            elif isinstance(value, (int, float)):
                self.defaults.append(f"{name} = {self.source.segment(default)}")
        self.shown.setdefault(name, shown_of(given))
        declared.properties[name] = declaration
        declared.statements[name] = statement

    def _read_table(self, declared: Declared, name: str, statement: ast.Assign) -> None:
        given = arguments(statement.value, TABLE_ARGS)
        columns_node = given.get("columns")
        if not isinstance(columns_node, ast.Dict):
            self.unread.append(f"{name}: its columns are not written as a dict")
            return
        columns: dict[str, d.Declaration] = {}
        for key, value in zip(columns_node.keys, columns_node.values):
            column = literal(key)
            if not isinstance(column, str):
                continue
            column_given = arguments(value, COLUMN_ARGS) if called(value, "Column") else {}
            columns[column] = d.Declaration(name=column, unit=unit_of(column_given.get("unit")))
            self.shown.setdefault(f"{name}.{column}", shown_of(column_given))
        index = next(iter(columns), "")
        # Column by column: the index values in, a list out.
        for formulas, slot in ((given.get("compute"), "value"), (given.get("compute_unc"), "uncertainty")):
            if isinstance(formulas, ast.Dict):
                for key, value in zip(formulas.keys, formulas.values):
                    column = literal(key)
                    if column in columns:
                        setattr(columns[column], slot, d.Formula(self.source.segment(value)))
                        columns[column].column = True
        # Row by row: (function, [the columns of the row it reads]).
        rows = given.get("compute_row")
        if isinstance(rows, ast.Dict):
            for key, value in zip(rows.keys, rows.values):
                column = literal(key)
                if column not in columns or not isinstance(value, (ast.Tuple, ast.List)):
                    continue
                function, reads = value.elts[0], literal(value.elts[1]) or []
                columns[column].value = d.Formula(
                    self.source.segment(function), depends_on=[f"row.{r}" for r in reads]
                )
                columns[column].row = True
        title = literal(given.get("title"))
        if given.get("index") is not None or given.get("data") is not None:
            self.unread.append(
                f"{name}: the index values or rows its declaration gave, which this version's "
                "declaration does not hold; the files hold the rows"
            )
        declared.tables[name] = d.TableDeclaration(
            name=name, index=[index], columns=columns, title=title if isinstance(title, str) else None
        )
        declared.statements[name] = statement

    # ── What a formula reads, where 0.x did not say ──────

    def _body_of(self, formula: d.Formula) -> ast.AST | None:
        """The code a formula runs: a method of the class or its bases, a
        function of the module, or a lambda itself."""
        try:
            node = ast.parse(formula.source, mode="eval").body
        except SyntaxError:
            return None
        name = on_self(node)
        if name is not None:
            for cls in reversed(self.ancestry):
                found = method(cls, name)
                if found is not None:
                    return found
            return None
        if isinstance(node, ast.Name):
            for top in self.source.tree.body:
                if isinstance(top, (ast.FunctionDef, ast.AsyncFunctionDef)) and top.name == node.id:
                    return top
            return None
        if isinstance(node, ast.Lambda):
            return node
        return None

    def reads_of(self, formula: d.Formula, row_reads: list[str] | None = None) -> list[str]:
        """What a formula reads of the sample: `self.width` is `width`,
        `self.curve.stress` is `curve.stress`, an attribute by its name; after
        the cells of its row it was given."""
        found: list[str] = list(row_reads or [])
        body = self._body_of(formula)
        if body is None:
            return found
        for node in ast.walk(body):
            if isinstance(node, ast.Attribute):
                table = on_self(node.value)
                if table in self.model.tables and node.attr in self.model.tables[table].columns:
                    read = f"{table}.{node.attr}"
                    if read not in found:
                        found.append(read)
            read = on_self(node)
            if read is not None and (read in self.model.properties or read in self.attributes):
                if read not in found:
                    found.append(read)
        return found

    def _depends(self, formula: d.Formula) -> tuple[list[str], str | None]:
        """Its `depends_on`, and a comment where 0.x did not say it."""
        if formula.depends_on is not None:
            read = list(formula.depends_on)
            more = [r for r in self.reads_of(formula) if r not in read and r in self.attributes]
            if more:
                return read + more, (
                    f"{TODO}: {', '.join(more)}, read by the formula, is an attribute now: named in depends_on"
                )
            return read, None
        found = self.reads_of(formula)
        if found:
            return found, f"{TODO}: 0.x named no input; depends_on lists what the formula was found to read: check it"
        return [], (
            f"{TODO}: 0.x named no input and none was found in the formula: "
            "name in depends_on what it reads"
        )

    # ── Writing it again ─────────────────────────────────

    def write(self, readings: list[str], attributes: set[str]) -> str:
        """The module in this version's API. `readings`: the properties a file
        held readings of; `attributes`: those whose values are no numbers."""
        source = self.source
        self.attributes = set(attributes) | self.attribute_defaults
        self._imports()
        union = {n for c in self.declared.values() for n in (*c.properties, *c.tables)}
        for cls in self.classes:
            declared = self.declared[cls.name]
            for base in cls.bases:
                if (isinstance(base, ast.Name) and base.id == "Sample") or (
                    isinstance(base, ast.Attribute) and base.attr == "Sample"
                ):
                    source.replace_node(base, "sk.Sample")
            init = declared.init
            if init is not None:
                self._signature(init)
                indent = source.indent_of(init.body[0].lineno)
                for statement in init.body:
                    if isinstance(statement, ast.Expr) and called(statement.value, "__init__"):
                        source.replace_node(statement, "super().__init__(path, name=name)")
                for name, statement in declared.statements.items():
                    if name in self.attributes:
                        written = f"{indent}# {name}: an attribute of each sample now, declared by none."
                    elif name in declared.tables:
                        written = self._table(declared.tables[name], indent)
                    else:
                        declaration = declared.properties[name]
                        declaration.readings_statistic = name in readings and declaration.value is None
                        written = self._property(declaration, indent)
                    source.replace(
                        source.line_start(statement.lineno),
                        source.offset(statement.end_lineno, statement.end_col_offset),
                        written,
                    )
                # Readings of a property no class declares: 0.x gave them its
                # statistic all the same. Declared once, in the class taken.
                if cls is self.cls:
                    lines = []
                    for name in readings:
                        if name in union or name in self.attributes:
                            continue
                        declaration = d.Declaration(name=name, readings_statistic=True)
                        self.model.properties[name] = declaration
                        lines.append(self._property(declaration, indent))
                    if lines:
                        last = init.body[-1]
                        at = source.offset(last.end_lineno, last.end_col_offset)
                        source.replace(at, at, "\n" + "\n".join(lines))
            for node in cls.body:
                if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) or node.name == "__init__":
                    continue
                if node.name == "template":
                    source.remove_lines(first_line(node), node.end_lineno)
                    continue
                self.model.copied.append(f"{cls.name}.{node.name}" if len(self.classes) > 1 else node.name)
                source.insert_line_before(first_line(node), CHECK_METHOD)
                self._attributes_read(node)
        for top in self.source.tree.body:
            if isinstance(top, (ast.FunctionDef, ast.AsyncFunctionDef)):
                self._attributes_read(top)
        self.model.text = source.result()
        return self.model.text

    def _attributes_read(self, node: ast.AST) -> None:
        """`self.material.value`, where `material` is an attribute now:
        `self.material`, said in the report."""
        for found in ast.walk(node):
            if (
                isinstance(found, ast.Attribute)
                and found.attr == "value"
                and on_self(found.value) in self.attributes
            ):
                name = on_self(found.value)
                self.source.replace_node(found, f"self.{name}")
                self.unread.append(
                    f"line {found.lineno}: self.{name}.value is self.{name}, {name} being an attribute now"
                )

    def _formula_text(self, formula: d.Formula) -> str:
        """A formula's source, `self.X.value` of an attribute made `self.X`."""
        text = formula.source
        for name in self.attributes:
            text = re.sub(rf"\bself\.{re.escape(name)}\.value\b", f"self.{name}", text)
        return text

    def _imports(self) -> None:
        """`import samplekit as sk` once; an import of 0.x's kept where this
        version has its names, so that the code copied still finds them, and
        what it has not said."""
        source = self.source
        added = False
        for node in source.tree.body:
            if isinstance(node, ast.Import) and any(
                alias.name.split(".")[0] == "samplekit" for alias in node.names
            ):
                kept = source.segment(node)
                if not added and kept != "import samplekit as sk":
                    source.replace_node(node, f"{kept}\nimport samplekit as sk")
                added = True
            elif isinstance(node, ast.ImportFrom) and (node.module or "").split(".")[0] == "samplekit":
                names = [alias.name for alias in node.names]
                kept = [n for n in names if n in CURRENT_NAMES]
                gone = [n for n in names if n not in CURRENT_NAMES]
                lines = [] if added else ["import samplekit as sk"]
                added = True
                if kept and node.module == "samplekit":
                    lines.append(f"from samplekit import {', '.join(kept)}")
                if gone or node.module != "samplekit":
                    lines.append(
                        f"{TODO}: 0.x's {', '.join(gone) or node.module} is not in this version: "
                        "the code using it is to rewrite"
                    )
                source.remove_lines(node.lineno, node.end_lineno, "".join(f"{l}\n" for l in lines))
        if not added:
            body = source.tree.body
            at = body[1].lineno if body and isinstance(body[0], ast.Expr) and len(body) > 1 else 1
            start = source.line_start(at)
            source.replace(start, start, "import samplekit as sk\n")

    def _signature(self, init: ast.FunctionDef) -> None:
        """`def __init__(self, name=None, filepath=None)` and 0.2's
        `(self, path=None, name=None)` alike, as this version calls it."""
        source = self.source
        start = source.offset(init.lineno, init.col_offset)
        data = source.text.encode("utf-8")
        opening = data.index(b"(", start)
        depth, at = 0, opening
        while True:
            char = data[at : at + 1]
            if char == b"(":
                depth += 1
            elif char == b")":
                depth -= 1
                if depth == 0:
                    break
            at += 1
        source.replace(opening, at + 1, "(self, path=None, name=None)")

    def _property(self, declaration: d.Declaration, indent: str) -> str:
        inner = indent + "    "
        lines: list[str] = []
        comments: list[str] = []
        if declaration.unit:
            lines.append(f"unit={quoted(declaration.unit)},")
        if declaration.readings_statistic:
            comments.append(STATISTIC_NOTE)
            lines.append("value=sk.stats.mean,")
            lines.append("uncertainty=sk.stats.sample_stdev,")
        depends: list[str] = []
        formulas = [f for f in (declaration.value, declaration.uncertainty) if f is not None]
        if declaration.value is not None:
            lines.append(f"compute_value={self._formula_text(declaration.value)},")
        if declaration.uncertainty is not None:
            lines.append(f"compute_uncertainty={self._formula_text(declaration.uncertainty)},")
        for formula in formulas:
            found, said = self._depends(formula)
            depends += [r for r in found if r not in depends]
            if said and said not in comments:
                comments.append(said)
            if not SIMPLE_CALLABLE.match(formula.source) and CHECK_LAMBDA not in comments:
                comments.append(CHECK_LAMBDA)
        if formulas:
            lines.append(f"depends_on={listed(depends)},")
        head = "".join(f"{indent}{line}\n" for comment in comments for line in comment.split("\n"))
        if not lines:
            return f"{head}{indent}self.{declaration.name} = sk.Property()"
        body = "".join(f"{inner}{line}\n" for line in lines)
        return f"{head}{indent}self.{declaration.name} = sk.Property(\n{body}{indent})"

    def _table(self, table: d.TableDeclaration, indent: str) -> str:
        inner, column_indent = indent + "    ", indent + "        "
        deeper = column_indent + "    "
        index = table.index[0]
        parts = []
        comments: list[str] = []
        for name, column in table.columns.items():
            arguments_ = []
            if column.unit:
                arguments_.append(f"unit={quoted(column.unit)}")
            if column.row and column.value is not None:
                reads = column.value.depends_on or []
                cells = ", ".join(f"row.{r[len('row.'):]}.value" for r in reads)
                arguments_.append(
                    f"compute_value=lambda row: {call_of(self._formula_text(column.value), cells)}"
                )
                depends = self.reads_of(column.value, reads)
                if len(depends) > len(reads):
                    comments.append(
                        f"{TODO}: {name}'s formula reads more than 0.x named; depends_on lists it: check it"
                    )
                arguments_.append(f"depends_on={listed(depends)}")
            elif column.column and (column.value is not None or column.uncertainty is not None):
                at = f'columns["{index}"].values'
                value = call_of(self._formula_text(column.value), at) if column.value is not None else None
                spread = (
                    call_of(self._formula_text(column.uncertainty), at)
                    if column.uncertainty is not None
                    else None
                )
                if value and spread:
                    formula = f"lambda columns: list(zip({value}, {spread}))"
                elif value:
                    formula = f"lambda columns: {value}"
                else:
                    formula = f"lambda columns: [(None, u) for u in {spread}]"
                arguments_.append(f"compute_column={formula}")
                depends = [f"{table.name}.{index}"]
                for formula_ in (column.value, column.uncertainty):
                    if formula_ is not None:
                        depends += [r for r in self.reads_of(formula_) if r not in depends]
                if len(depends) > 1:
                    comments.append(
                        f"{TODO}: {name}'s formula reads more than the index; depends_on lists it: check it"
                    )
                arguments_.append(f"depends_on={listed(depends)}")
            for formula_ in (column.value, column.uncertainty):
                if formula_ is not None and not SIMPLE_CALLABLE.match(formula_.source):
                    if CHECK_LAMBDA not in comments:
                        comments.append(CHECK_LAMBDA)
            if len(arguments_) > 1:
                listed_ = "".join(f"{deeper}{a},\n" for a in arguments_)
                parts.append(f'{column_indent}"{name}": sk.Property(\n{listed_}{column_indent}),')
            else:
                parts.append(f'{column_indent}"{name}": sk.Property({", ".join(arguments_)}),')
        head = "".join(f"{indent}{comment}\n" for comment in comments)
        title = f"{inner}title={quoted(table.title)},\n" if table.title else ""
        return (
            f"{head}{indent}self.{table.name} = sk.Table(\n"
            f"{inner}{{\n" + "\n".join(parts) + f"\n{inner}}},\n"
            f"{inner}index={quoted(index)},\n{title}{indent})"
        )
