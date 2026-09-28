#!/usr/bin/env python3
"""Write the package's docstrings into its stub, python/samplekit/__init__.pyi.

An editor reading the stub shows what the stub says, and the stub said the
signatures alone. The docstrings are the package's, written once in
its source: this script imports the package installed in .venv and gives each
class, method, property, attribute and function the stub declares the
docstring the package gives it, inserted where there is none and refreshed
where there is. The signatures and the types stay the stub's own, written by
hand; a name that starts with `_` gets none, as a docstring of a dunder is
Python's.

    .venv/bin/python tools/generate-stub-docstrings.py           write them
    .venv/bin/python tools/generate-stub-docstrings.py --check   exit 1 when
                                                                 the stub is
                                                                 stale

It reads the extension `maturin develop` built, as pytest does: a stale build
gives stale docstrings.
"""

import ast
import inspect
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STUB = ROOT / "python" / "samplekit" / "__init__.pyi"


def own_doc(value):
    """The docstring of what a class or the module holds, cleaned, or None."""
    if isinstance(value, (classmethod, staticmethod)):
        value = value.__func__
    if not (inspect.isroutine(value) or inspect.isdatadescriptor(value)):
        # A constant's `__doc__` is its type's.
        return None
    doc = getattr(value, "__doc__", None)
    if not isinstance(doc, str) or not doc.strip():
        return None
    return inspect.cleandoc(doc)


def member_doc(cls, name):
    """The docstring of `cls.name`, where a class of the package defines it:
    a method `list` gives a bound list is not the package's to document."""
    for owner in inspect.getmro(cls):
        if name in vars(owner):
            if not owner.__module__.startswith("samplekit"):
                return None
            return own_doc(vars(owner)[name])
    return None


def class_doc(cls):
    doc = cls.__dict__.get("__doc__")
    if not isinstance(doc, str) or not doc.strip():
        return None
    return inspect.cleandoc(doc)


def rendered(doc, indent):
    """A docstring as written at `indent`, its first line at the cursor."""
    if '"""' in doc:
        raise SystemExit(f'a docstring holds """, which a stub cannot quote: {doc[:60]!r}')
    prefix = "r" if "\\" in doc else ""
    lines = doc.split("\n")
    if len(lines) == 1:
        # A closing quote of its own would run into the three.
        return f'{prefix}"""{doc}"""' if not doc.endswith('"') else f'{prefix}"""{doc}\n{indent}"""'
    body = "\n".join(indent + line if line else "" for line in lines[1:])
    return f'{prefix}"""{lines[0]}\n{body}\n{indent}"""'


def is_docstring(node):
    return (
        isinstance(node, ast.Expr)
        and isinstance(node.value, ast.Constant)
        and isinstance(node.value.value, str)
    )


def is_ellipsis(node):
    return (
        isinstance(node, ast.Expr)
        and isinstance(node.value, ast.Constant)
        and node.value.value is Ellipsis
    )


class Edits:
    """Replacements by (line, column) spans, applied from the end so that the
    positions of those before stay true."""

    def __init__(self, text):
        self.lines = text.splitlines(keepends=True)
        self.edits = []

    def offset(self, line, column):
        # `ast` counts lines from 1 and columns in UTF-8 bytes.
        before = "".join(self.lines[: line - 1])
        text = self.lines[line - 1].encode()[:column].decode()
        return len(before) + len(text)

    def replace(self, start, end, text):
        self.edits.append((self.offset(*start), self.offset(*end), text))

    def insert(self, at, text):
        self.replace(at, at, text)

    def line_end(self, line):
        return (line, len(self.lines[line - 1].rstrip("\n").encode()))

    def applied(self):
        text = "".join(self.lines)
        for start, end, replacement in sorted(self.edits, reverse=True):
            text = text[:start] + replacement + text[end:]
        return text


def document_body(edits, node, doc, indent):
    """Give a def or a class body `doc` as its first statement."""
    first = node.body[0]
    if is_docstring(first):
        replacement = rendered(doc, indent) if doc else "..."
        if len(node.body) > 1 and not doc:
            # Other statements follow: the docstring's line goes with it.
            start = (first.lineno, 0)
            end = (first.end_lineno + 1, 0)
            edits.replace(start, end, "")
            return
        edits.replace((first.lineno, first.col_offset),
                      (first.end_lineno, first.end_col_offset), replacement)
        return
    if not doc:
        return
    line = edits.lines[first.lineno - 1]
    before = line.encode()[: first.col_offset].decode()
    if before.strip():
        # `def f() -> int: ...` — the body on the signature's line: the
        # docstring takes its place, on a line of its own.
        colon = len(before.rstrip().encode())
        if is_ellipsis(first) and len(node.body) == 1:
            edits.replace((first.lineno, colon), (first.end_lineno, first.end_col_offset),
                          "\n" + indent + rendered(doc, indent))
        else:
            edits.insert((first.lineno, colon), "\n" + indent + rendered(doc, indent))
        return
    if is_ellipsis(first) and len(node.body) == 1:
        edits.replace((first.lineno, first.col_offset),
                      (first.end_lineno, first.end_col_offset), rendered(doc, indent))
        return
    # A class's first member: the docstring before it — its decorators
    # included — a blank line between.
    start = min([first.lineno, *(d.lineno for d in getattr(first, "decorator_list", []))])
    edits.insert((start, 0), indent + rendered(doc, indent) + "\n\n")


def is_setter(node):
    return any(
        isinstance(decorator, ast.Attribute) and decorator.attr in ("setter", "deleter")
        for decorator in node.decorator_list
    )


def document_class(edits, node, cls):
    indent = " " * (node.col_offset + 4)
    document_body(edits, node, class_doc(cls), indent)
    body = node.body
    for at, item in enumerate(body):
        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if item.name.startswith("_") or is_setter(item):
                continue
            document_body(edits, item, member_doc(cls, item.name), indent + "    ")
        elif isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
            name = item.target.id
            if name.startswith("_"):
                continue
            doc = member_doc(cls, name)
            following = body[at + 1] if at + 1 < len(body) else None
            if following is not None and is_docstring(following):
                if doc:
                    edits.replace((following.lineno, following.col_offset),
                                  (following.end_lineno, following.end_col_offset),
                                  rendered(doc, indent))
                else:
                    edits.replace((following.lineno, 0), (following.end_lineno + 1, 0), "")
            elif doc:
                # An attribute's docstring is the string after it, which
                # editors show as they show a method's.
                edits.insert(edits.line_end(item.end_lineno), "\n" + indent + rendered(doc, indent))


def generated(text, package):
    edits = Edits(text)
    for node in ast.parse(text).body:
        if isinstance(node, ast.ClassDef):
            cls = getattr(package, node.name, None)
            if inspect.isclass(cls):
                document_class(edits, node, cls)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if node.name.startswith("_"):
                continue
            function = getattr(package, node.name, None)
            document_body(edits, node, own_doc(function) if function else None, "    ")
    return edits.applied()


def main(arguments):
    check = arguments == ["--check"]
    if arguments and not check:
        raise SystemExit(__doc__)
    import samplekit

    text = STUB.read_text()
    written = generated(text, samplekit)
    # Written twice, it must say the same: a docstring the first pass placed
    # is one the second refreshes, never one it adds again.
    if generated(written, samplekit) != written:
        raise SystemExit("the stub's docstrings do not settle: a second pass changes them")
    if check:
        if written != text:
            print(
                f"{STUB.relative_to(ROOT)} does not carry the package's docstrings: run\n"
                "  .venv/bin/python tools/generate-stub-docstrings.py",
                file=sys.stderr,
            )
            return 1
        return 0
    if written != text:
        STUB.write_text(written)
        print(f"wrote {STUB.relative_to(ROOT)}")
    else:
        print(f"{STUB.relative_to(ROOT)} carries the package's docstrings")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
