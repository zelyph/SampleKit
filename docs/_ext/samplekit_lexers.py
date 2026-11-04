"""Pygments lexers for the documentation's examples.

`console` blocks are SampleKit sessions. The command after `$ ` is coloured as
a command line; what SampleKit prints is coloured as SampleKit colours it on a
terminal, by the same rules and in the default colours of `[colors]`: a value
bold and its unit cyan, an attribute's value blue, a header bold, a state in
its colour, `✓` and `written:` green, advice and what says nothing happened
grey. `tools/check-docs.py` holds these roles to what a terminal draws.

`sample` blocks are sample files: their frontmatter as YAML, their note as
Markdown.
"""

import re

from pygments.lexer import Lexer, RegexLexer, bygroups, using
from pygments.lexers.data import YamlLexer
from pygments.lexers.markup import MarkdownLexer
from pygments.token import (
    Comment,
    Keyword,
    Name,
    Punctuation,
    String,
    Text,
    Token,
    Whitespace,
)

# What SampleKit paints, one token type per role of `[colors]`.
SampleKit = Token.SampleKit
VALUE = SampleKit.Value
UNIT = SampleKit.Unit
ATTRIBUTE = SampleKit.Attribute
HEADER = SampleKit.Header
OUTDATED = SampleKit.Outdated
FAILED = SampleKit.Failed
EDITED = SampleKit.Edited
CURRENT = SampleKit.Current
MUTED = SampleKit.Muted
# The column a terminal table adds first to say whose each row is, faint.
IDENTITY = SampleKit.Identity

COMMANDS = (
    "compute|status|set|new|tag|export|plot|log|diff|restore|explain|open|"
    "list|validate|view|init|run|tui|help|completions"
)

# A number, bracketed where it waits, and a count out of a total, `3/3`.
NUMBER = re.compile(r"\(?-?\d+(?:\.\d+)?(?:e[-+]?\d+)?\)?$|\d+/\d+$")
# Headers of the tables SampleKit draws about samples rather than of them:
# their text cells are names, never an attribute's value.
OWN_TABLES = ("sample ", "value ", "# ")
COUNTS = ("new", "changed", "cells", "cell", "rows")
# A state's mark beside a value, in its state's colour.
MARKS = {"⚠": None, "✎": None, "✗": None}


class CommandLexer(RegexLexer):
    """The command after `$ `."""

    tokens = {
        "root": [
            (r"\\\n", Text),
            (r"\bsamplekit\b", Name.Builtin),
            (r"\bpython\b|\bgit\b|\bpip\b|\bcd\b", Name.Builtin),
            (rf"(?<= )(?:{COMMANDS})(?=[ \n]|$)", Keyword),
            (r"--?[A-Za-z][\w-]*", Name.Attribute),
            (r"'[^'\n]*'|\"[^\"\n]*\"", String),
            (r"#[^\n]*", Comment),
            (r"[^\s'\"#-]+|-", Text),
            (r"\s+", Whitespace),
        ],
    }


def is_number(word):
    return bool(NUMBER.match(word.rstrip(",")))


def state_of(text):
    """A state's colour, as the command line gives it: a failure red, a hand's
    value magenta, a value current green, and any other state, a value to
    compute again whatever its words, yellow."""
    text = text.strip()
    # A value waiting for an input is said plain: nothing is wrong with it.
    if not text or text in ("entered", "—", "-") or text.startswith("waits"):
        return None
    if text == "current":
        return CURRENT
    if text.startswith("failed"):
        return FAILED
    if text.startswith(("edited", "record missing")):
        return EDITED
    return OUTDATED


def advises(text):
    """Whether a line's end proposes a command or an option."""
    return "samplekit " in text or any(
        word.startswith("--") and len(word) > 2 and word[2].isalpha()
        for word in text.split()
    )


def words(text, at):
    """`text`'s words and the spaces between them, from column `at`."""
    for match in re.finditer(r"\S+|\s+", text):
        yield at + match.start(), match.group()


def quantity(text, at, attribute=True):
    """A value as SampleKit writes it: `12.5 ± 0.01 mm` bold and its unit
    cyan, a list of readings bold, a word an attribute's value; a state's mark
    after it, `⚠ outdated`, in its state's colour."""
    marked = re.search(r"\s+[⚠✎✗](?:\s.*)?$", text)
    if marked:
        yield from quantity(text[: marked.start()], at, attribute)
        gap = len(marked.group()) - len(marked.group().lstrip())
        mark = marked.group().lstrip()
        role = {"⚠": OUTDATED, "✎": EDITED, "✗": FAILED}[mark[0]]
        yield at + marked.start(), Whitespace, marked.group()[:gap]
        yield at + marked.start() + gap, role, mark
        return
    parts = text.split()
    if not parts or text.strip() in ("—", "-", "n/a"):
        yield at, Text, text
        return
    numeric = is_number(parts[0])
    if numeric and len(parts) == 2 and parts[1] in COUNTS:
        yield at, Text, text
        return
    if numeric:
        unit = len(parts) > 1 and not is_number(parts[-1]) and parts[-1] != "±"
        last = text.rstrip().rfind(parts[-1]) if unit else len(text)
        for start, word in words(text[:last], at):
            yield start, (Whitespace if word.isspace() else VALUE), word
        if unit:
            yield at + last, UNIT, text[last:]
        return
    # A sentence, as a note is, is no attribute's word: plain, as the terminal
    # prints it.
    if " " in text.strip() and not re.search(r"[⚠✎✗]", text):
        yield at, Text, text
        return
    yield at, (ATTRIBUTE if attribute else Text), text


def stated(text, at):
    """A state, `(8 rows)` after it plain."""
    rows = re.search(r" \(\d+ rows?\)$", text)
    end = rows.start() if rows else len(text)
    yield at, state_of(text[:end]) or Text, text[:end]
    if rows:
        yield at + end, Text, text[end:]


def identity_added(command, heads):
    """Whether a table's first column is the one SampleKit adds in a terminal
    to say whose each row is: `name` or `file`, the columns asked for with
    `-c` not naming it."""
    if not heads or heads[0] not in ("name", "file") or " --summary" in command:
        return False
    match = re.search(r"(?:-c|--columns)[ =](\S+)", command)
    if not match:
        return False
    asked = [column.split("=")[0] for column in match.group(1).split(",")]
    return heads[0] not in asked


def header(line, at=0):
    """A header: its words bold, a unit in brackets cyan."""
    for start, word in words(line, at):
        if word.isspace():
            yield start, Whitespace, word
        elif word.startswith("[") and word.endswith("]"):
            yield start, UNIT, word
        else:
            yield start, HEADER, word


class SampleKitConsoleLexer(Lexer):
    """A terminal session with SampleKit: commands, and what they print."""

    name = "SampleKit console"
    aliases = ["console", "samplekit-console"]

    def get_tokens_unprocessed(self, text):
        lines = text.split("\n")
        at = 0
        self.command = ""
        self.table = None  # (kind, the column of states) inside a table
        self.field_width = None  # where a view's or a preview's values begin
        self.in_notes = False  # a preview's notes beneath its change
        self.in_states = False  # the values a change makes not current
        for number, line in enumerate(lines):
            following = lines[number + 1] if number + 1 < len(lines) else ""
            yield from self.line(line, following, at)
            stripped = line.strip()
            if line.startswith("$ "):
                self.command = line
                self.table, self.field_width = None, None
                self.in_notes = self.in_states = False
            elif not stripped:
                self.table, self.in_notes, self.in_states = None, False, False
            elif following.strip().startswith("─") and not stripped.startswith("─"):
                if " view" in self.command and line.startswith("  "):
                    kind = "view"
                elif stripped.startswith("# "):
                    kind = "log"
                elif stripped.startswith(OWN_TABLES):
                    kind = "own"
                elif " sem " in line and " mean" in line:
                    kind = "summary"
                else:
                    kind = "samples"
                heads = re.findall(r"\S+(?: \S+)*", line)
                self.table = (kind, heads.index("state") if "state" in heads else None)
                # Tags are words, drawn plain where an attribute's value is
                # blue.
                self.plain_columns = {at for at, head in enumerate(heads) if head == "tags"}
                self.identity = kind == "samples" and identity_added(self.command, heads)
            at += len(line)
            if number + 1 < len(lines):
                yield at, Whitespace, "\n"
                at += 1

    def line(self, line, following, at):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip())
        lead = (at, Whitespace, line[:indent])
        if line.startswith("$ "):
            yield at, Name.Builtin, "$"
            yield at + 1, Whitespace, " "
            for start, kind, part in CommandLexer().get_tokens_unprocessed(line[2:]):
                yield at + 2 + start, kind, part
            return
        if not stripped:
            yield at, Whitespace, line
            return
        if stripped.startswith("─"):
            yield at, Text, line
            return
        if following.strip().startswith("─"):
            heads = re.findall(r"\S+(?: \S+)*", line)
            if identity_added(self.command, heads) and not stripped.startswith(OWN_TABLES):
                first = re.match(r"\s*\S+", line)
                yield at, IDENTITY, first.group()
                yield from header(line[first.end() :], at + first.end())
                return
            yield from header(line, at)
            return
        if stripped.startswith("✓ "):
            yield lead
            yield at + indent, CURRENT, "✓"
            yield at + indent + 1, Text, line[indent + 1 :]
            return
        # `tag`: the tag added or taken out, `+reference`, as an attribute.
        if " tag " in self.command:
            tagged = re.search(r"(?<!\S)[+-](\w+)", line)
            if tagged and not stripped.startswith(("written:", "nothing")):
                yield at, Text, line[: tagged.start(1)]
                yield at + tagged.start(1), ATTRIBUTE, tagged.group(1)
                yield at + tagged.end(), Text, line[tagged.end() :]
                return
        if stripped.startswith("written:"):
            yield lead
            yield at + indent, CURRENT, "written:"
            yield at + indent + 8, Text, line[indent + 8 :]
            return
        if stripped.startswith(("nothing written", "note:")) or (
            stripped.startswith("samplekit ") and not stripped.startswith("samplekit [")
        ):
            yield lead
            yield at + indent, MUTED, line[indent:]
            return
        if self.table is not None:
            yield from self.row(line, at)
            return
        if self.in_notes and self.field_width and indent < self.field_width:
            self.in_notes = False
        # The list's head, `this makes 6 values not current`: a state of
        # its own may say `(itself not current)`, further in.
        if "not current" in stripped and indent < 4 and not self.in_notes:
            self.in_states = True
        elif self.in_states and indent >= 4:
            # The state is the last of the line's cells: a value's name may be
            # its file's and its own, `pieces/a.md  modulus`.
            match = re.match(r"(\s+.*\S\s{2,})(\S+(?: \S+)*)$", line)
            if match:
                yield at, Text, match.group(1)
                yield from stated(match.group(2), at + len(match.group(1)))
                return
        # A preview: `field   before  →  after`, then its notes.
        if "  →  " in line:
            match = re.match(r"(\s*\S+\s+)(.*?)(  →  )(.*)$", line)
            if match:
                head, before, arrow, after = match.groups()
                yield at, Text, head
                yield from quantity(before, at + len(head))
                yield at + len(head) + len(before), Text, arrow
                yield from quantity(after, at + len(head) + len(before) + len(arrow))
                self.field_width, self.in_notes, self.in_states = len(head), True, False
                return
        if self.in_notes and self.field_width and indent >= self.field_width:
            yield lead
            yield at + indent, MUTED, line[indent:]
            return
        self.in_notes = False
        # `explain`: an input, its value and its state.
        if " explain" in self.command and indent == 4:
            match = re.match(r"(\s+\S+\s+)(.*?)(\s{2,})(\S.*)$", line)
            if match:
                name, value, gap, state = match.groups()
                yield at, Text, name
                yield from quantity(value, at + len(name))
                yield at + len(name) + len(value), Whitespace, gap
                yield from stated(state, at + len(name) + len(value) + len(gap))
                return
        if " explain" in self.command and " = " in line and " · " in line:
            head, value = line.split(" = ", 1)
            yield at, Text, head + " = "
            yield from quantity(value, at + len(head) + 3)
            return
        if stripped.startswith("this value is "):
            yield lead
            yield at + indent, Text, "this value is "
            yield from stated(stripped[14:], at + indent + 14)
            return
        # A sample shown whole, `view`: a field, then its value.
        match = re.match(r"(\s+\S+\s{2,})(\S.*)$", line)
        if " view" in self.command and indent == 2 and match:
            label, value = match.groups()
            yield at, Text, label
            yield from quantity(value, at + len(label))
            self.field_width = len(label)
            return
        if (
            " view" in self.command
            and self.field_width
            and indent >= self.field_width
            and stripped.startswith("readings ")
        ):
            yield lead
            yield at + indent, MUTED, "readings"
            yield at + indent + 8, Whitespace, " "
            for start, word in words(line[indent + 9 :], at + indent + 9):
                yield start, (Whitespace if word.isspace() else VALUE), word
            return
        # An error: the word red, then what it says.
        start = indent
        if stripped.startswith("error:"):
            yield lead
            yield at + indent, FAILED, "error:"
            start = indent + len("error:")
        rest = line[start:]
        # A line that ends on advice: what it says, then what to do, grey.
        for mark in (" — ", ": "):
            if mark in rest:
                said, tip = rest.rsplit(mark, 1)
                if (mark == " — " and advises(tip)) or (
                    mark == ": " and tip.startswith("samplekit ")
                ):
                    if start == indent:
                        yield lead
                    yield at + start, Text, said + mark
                    yield at + start + len(said) + len(mark), MUTED, tip
                    return
        if start == indent:
            yield at, Text, line
        else:
            yield at + start, Text, rest

    def row(self, line, at):
        """A table's row, its cells apart where two spaces or more part them:
        a number right-aligned stands under its header, not after it."""
        assert self.table is not None
        kind, states = self.table
        cells = list(re.finditer(r"\S+(?: \S+)*", line))
        # A summary's label stands before its count, `radius  3/3`: the groups'
        # values before it, as attributes are.
        label = next(
            (at_ - 1 for at_, cell in enumerate(cells) if re.fullmatch(r"\d+/\d+", cell.group())),
            0,
        )
        consumed = 0
        for column, cell in enumerate(cells):
            start, text = cell.start(), cell.group()
            if start > consumed:
                yield at + consumed, Whitespace, line[consumed:start]
            place = at + start
            if column == 0 and getattr(self, "identity", False):
                yield place, IDENTITY, text
            elif kind == "summary" and column < label:
                yield place, ATTRIBUTE, text
            elif kind == "summary" and column == label:
                yield from header(text, place)
            elif kind == "log":
                yield place, Text, text
            elif column == states:
                yield from stated(text, place)
            elif is_number(text.split()[0]) or text in ("—", "-"):
                yield from quantity(text, place)
            elif column in getattr(self, "plain_columns", set()):
                yield place, Text, text
            elif kind == "view" or (kind == "samples" and column > 0):
                yield place, ATTRIBUTE, text
            else:
                yield place, Text, text
            consumed = cell.end()
        if consumed < len(line):
            yield at + consumed, Whitespace, line[consumed:]


class SampleFileLexer(RegexLexer):
    """A sample file: a YAML frontmatter between `---` lines, then a note."""

    name = "SampleKit sample file"
    aliases = ["sample"]

    tokens = {
        "root": [
            (
                r"(---\n)((?:.*\n)*?)(---\n?)((?:.|\n)*)",
                bygroups(
                    Punctuation, using(YamlLexer), Punctuation, using(MarkdownLexer)
                ),
            ),
            (r"(?:.|\n)+", using(YamlLexer)),
        ],
    }


def setup(app):
    app.add_lexer("console", SampleKitConsoleLexer)
    app.add_lexer("sample", SampleFileLexer)
    return {"parallel_read_safe": True, "parallel_write_safe": True}
