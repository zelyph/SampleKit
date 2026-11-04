"""Edits of a Python source at the places its syntax tree names.

A model is rewritten where it must change and kept as it was elsewhere, its
comments included: each edit replaces a span the tree gives, and the edits are
made from the end, so that one does not move the next.
"""

from __future__ import annotations

import ast


class Source:
    """A module's text, its tree, and the edits asked of it."""

    def __init__(self, text: str):
        self.text = text
        self.tree = ast.parse(text)
        self._lines = text.encode("utf-8").splitlines(keepends=True)
        self._starts = [0]
        for line in self._lines:
            self._starts.append(self._starts[-1] + len(line))
        self._edits: list[tuple[int, int, str]] = []

    # The tree counts columns in bytes of UTF-8: offsets are taken in the
    # bytes, then the text cut there.
    def offset(self, line: int, column: int) -> int:
        return self._starts[line - 1] + column

    def span(self, node: ast.AST) -> tuple[int, int]:
        return (
            self.offset(node.lineno, node.col_offset),
            self.offset(node.end_lineno, node.end_col_offset),
        )

    def segment(self, node: ast.AST) -> str:
        start, end = self.span(node)
        return self.text.encode("utf-8")[start:end].decode("utf-8")

    def line_start(self, line: int) -> int:
        return self._starts[line - 1]

    def indent_of(self, line: int) -> str:
        raw = self._lines[line - 1].decode("utf-8")
        return raw[: len(raw) - len(raw.lstrip(" \t"))]

    def replace(self, start: int, end: int, text: str) -> None:
        self._edits.append((start, end, text))

    def replace_node(self, node: ast.AST, text: str) -> None:
        self.replace(*self.span(node), text)

    def insert_line_before(self, line: int, text: str) -> None:
        """A line of its own above `line`, at that line's indent."""
        at = self.line_start(line)
        self._edits.append((at, at, f"{self.indent_of(line)}{text}\n"))

    def remove_lines(self, first: int, last: int, leaving: str = "") -> None:
        """Lines `first` to `last` taken out whole, `leaving` in their place."""
        self.replace(self.line_start(first), self._starts[last], leaving)

    def result(self) -> str:
        data = self.text.encode("utf-8")
        # From the end; at one place, in the order asked.
        ordered = sorted(enumerate(self._edits), key=lambda e: (e[1][0], e[0]), reverse=True)
        for _, (start, end, text) in ordered:
            data = data[:start] + text.encode("utf-8") + data[end:]
        return data.decode("utf-8")


def called(node: ast.AST, *names: str) -> bool:
    """Whether `node` calls one of `names`, as `Name(...)` or `module.Name(...)`."""
    if not isinstance(node, ast.Call):
        return False
    func = node.func
    if isinstance(func, ast.Name):
        return func.id in names
    if isinstance(func, ast.Attribute):
        return func.attr in names
    return False


def on_self(node: ast.AST) -> str | None:
    """`self.NAME` → NAME."""
    if (
        isinstance(node, ast.Attribute)
        and isinstance(node.value, ast.Name)
        and node.value.id == "self"
    ):
        return node.attr
    return None


def literal(node: ast.AST | None):
    """A literal's value, or None where the source computes it."""
    if node is None:
        return None
    try:
        return ast.literal_eval(node)
    except (ValueError, SyntaxError, TypeError):
        return None


def arguments(call: ast.Call, positional: list[str]) -> dict[str, ast.AST]:
    """A call's arguments by name, its positional ones named as `positional`
    names them."""
    given: dict[str, ast.AST] = {}
    for name, node in zip(positional, call.args):
        given[name] = node
    for keyword in call.keywords:
        if keyword.arg is not None:
            given[keyword.arg] = keyword.value
    return given


def find_class(tree: ast.Module, name: str | None) -> list[ast.ClassDef]:
    """The classes of a sample: those named `name`, or deriving from `Sample`."""
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
    if name is not None:
        return [node for node in classes if node.name == name]

    def from_sample(node: ast.ClassDef) -> bool:
        for base in node.bases:
            if isinstance(base, ast.Name) and base.id == "Sample":
                return True
            if isinstance(base, ast.Attribute) and base.attr == "Sample":
                return True
        return False

    return [node for node in classes if from_sample(node)]


def method(cls: ast.ClassDef, name: str) -> ast.FunctionDef | None:
    for node in cls.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == name:
            return node
    return None


def first_line(node: ast.AST) -> int:
    """A definition's first line, its decorators included."""
    decorators = getattr(node, "decorator_list", [])
    return min([node.lineno, *(d.lineno for d in decorators)])


def quoted(text: str) -> str:
    """A Python string literal, double-quoted where it can be."""
    if '"' not in text and "\\" not in text:
        return f'"{text}"'
    return repr(text)
