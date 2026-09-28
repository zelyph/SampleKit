#!/usr/bin/env python3
"""Write docs/reference/cli.md from the command line's own help.

Every command the help lists, and every command under one, is run with
`--help`, and its page is turned into Markdown: what it does, its usage, its
arguments and options by section, its examples. The help is the one source:
this page is never edited by hand.

    tools/generate-cli-reference.py            write the page
    tools/generate-cli-reference.py --check    fail when the page is stale

The binary is target/debug/samplekit or target/release/samplekit, whichever
was built last; `--binary PATH` names another. It runs in an empty folder, so
that no project's configuration shows in the help.
"""

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs" / "reference" / "cli.md"
# The option groups every command shares: listed once, at the top.
SHARED_SECTION = "General"
SECTION = re.compile(r"^([A-Z][\w ]*):(?: (.*))?$")
ENTRY = re.compile(r"^  (\S.*)$")
# An option with no short form starts where a short one's long form does.
LONG_ONLY = re.compile(r"^ {6}(--\S.*)$")
TEXT = re.compile(r"^ {6,}(\S.*)$")


def binary():
    if "--binary" in sys.argv:
        return Path(sys.argv[sys.argv.index("--binary") + 1]).resolve()
    built = [
        path
        for path in (ROOT / "target" / "debug" / "samplekit", ROOT / "target" / "release" / "samplekit")
        if path.exists()
    ]
    if not built:
        sys.exit("no samplekit built: run cargo build first")
    return max(built, key=lambda path: path.stat().st_mtime)


def help_of(program, words, folder):
    env = dict(os.environ, NO_COLOR="1", COLUMNS="100", TERM="dumb")
    ran = subprocess.run(
        [program, *words, "--help"],
        cwd=folder,
        env=env,
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
    )
    if ran.returncode != 0:
        sys.exit(f"samplekit {' '.join(words)} --help failed:\n{ran.stderr}")
    lines = ran.stdout.rstrip("\n").splitlines()
    # Where the help was run, which says nothing about the command.
    return [line for line in lines if not line.startswith("Configuration here:")]


def parse(lines):
    """A help page as (about, usage, sections): about is its paragraphs, usage
    its lines, and each section a name and its entries, an entry being its
    head and its paragraphs."""
    about, usage, sections = [], [], []
    index = 0
    paragraph = []
    while index < len(lines) and not lines[index].startswith("Usage:"):
        if lines[index].strip():
            paragraph.append(lines[index].strip())
        elif paragraph:
            about.append(" ".join(paragraph))
            paragraph = []
        index += 1
    if paragraph:
        about.append(" ".join(paragraph))
    usage.append(lines[index][len("Usage: "):].strip())
    index += 1
    while index < len(lines) and lines[index].startswith(" "):
        usage.append(lines[index].strip())
        index += 1
    current = None
    entry = None
    for line in lines[index:]:
        section = SECTION.match(line)
        if section:
            current = (section.group(1), [])
            sections.append(current)
            entry = None
            continue
        if current is None or not line.strip():
            if entry is not None:
                entry[1].append("")
            continue
        head = ENTRY.match(line) or (current[0] != "Examples" and LONG_ONLY.match(line))
        text = TEXT.match(line)
        if text and not head and entry is not None:
            entry[1].append(text.group(1))
            continue
        if head:
            # A command list writes its head and its text on one line.
            name, *said = re.split(r"\s{2,}", head.group(1).strip(), maxsplit=1)
            entry = (name, said)
            current[1].append(entry)
    return about, usage, [(name, [(head, paragraphs(body)) for head, body in entries])
                          for name, entries in sections]


def paragraphs(lines):
    found, paragraph = [], []
    for line in lines + [""]:
        if line:
            paragraph.append(line)
        elif paragraph:
            found.append(" ".join(paragraph))
            paragraph = []
    return found


CODE = re.compile(r"(`[^`]*`)")


def escaped(text):
    """Help text as Markdown shows it as written: code spans kept, the
    characters Markdown reads as markup escaped elsewhere."""
    parts = CODE.split(text)
    return "".join(
        part if part.startswith("`") else re.sub(r"([\\*_<>\[\]|#])", r"\\\1", part)
        for part in parts
    )


def code(text):
    ticks = "``" if "`" in text else "`"
    pad = " " if "`" in text else ""
    return f"{ticks}{pad}{text}{pad}{ticks}"


def anchor(words):
    return "-".join(["samplekit", *words])


def command_entries(sections):
    for name, entries in sections:
        if name == "Commands":
            return [(head, said) for head, said in entries if head != "help"]
    return []


def render(words, page, shared, out):
    about, usage, sections = page
    level = "#" * min(2 + len(words) - (1 if words else 0), 4)
    title = " ".join(["samplekit", *words])
    out.append(f"{level} {title}")
    out.append("")
    for paragraph in about:
        out.append(escaped(paragraph))
        out.append("")
    out.append("```text")
    out.extend(usage)
    out.append("```")
    out.append("")
    for name, entries in sections:
        if name == "Commands":
            out.append("Commands:")
            out.append("")
            for head, said in entries:
                if head == "help":
                    continue
                out.append(f"- [{code(' '.join([*words, head]))}](#{anchor([*words, head])}): "
                           f"{escaped(' '.join(said))}")
            out.append("")
            continue
        if name == "Examples":
            out.append("Examples:")
            out.append("")
            out.append("```sh")
            for head, said in entries:
                for paragraph in said:
                    out.append(f"# {paragraph}")
                out.append(head)
            out.append("```")
            out.append("")
            continue
        if name == SHARED_SECTION and shared is not None:
            own = [(head, said) for head, said in entries if shared.get(head) != said]
            common = [head for head, said in entries if shared.get(head) == said]
            if common:
                listed = ", ".join(code(head.split(" <")[0]) for head in common)
                out.append(f"General options ([described once](#general-options)): "
                           f"{listed}.")
                out.append("")
            entries = own
            if not entries:
                continue
        out.append(f"{name}:")
        out.append("")
        for head, said in entries:
            first, *rest = said or [""]
            out.append(f"- {code(head)}: {escaped(first)}".rstrip())
            for paragraph in rest:
                out.append("")
                out.append(f"  {escaped(paragraph)}")
        out.append("")


def main():
    program = binary()
    with tempfile.TemporaryDirectory() as folder:
        pages = {}

        def walk(words):
            page = parse(help_of(program, words, folder))
            pages[tuple(words)] = page
            for head, _ in command_entries(page[2]):
                walk([*words, head])

        walk([])

    top = pages[()]
    shared = {}
    for name, entries in top[2]:
        if name == SHARED_SECTION:
            shared = {head: said for head, said in entries}

    out = [
        "<!-- Generated by tools/generate-cli-reference.py from `samplekit --help`:",
        "     do not edit; change the help in the code and run the script again. -->",
        "",
        "# The command line",
        "",
        "Every command of `samplekit`, as its `--help` gives it. "
        "`samplekit COMMAND -h` prints a shorter summary in the terminal.",
        "",
        "## General options",
        "",
        "The options most commands share. Each command below says which of them it takes.",
        "",
    ]
    for head, said in shared.items():
        first, *rest = said or [""]
        out.append(f"- {code(head)}: {escaped(first)}".rstrip())
        for paragraph in rest:
            out.append("")
            out.append(f"  {escaped(paragraph)}")
    out.append("")
    for words, page in pages.items():
        render(list(words), page, shared, out)
    text = "\n".join(out).rstrip("\n") + "\n"

    if "--check" in sys.argv:
        if not PAGE.exists() or PAGE.read_text() != text:
            sys.exit(f"{PAGE.relative_to(ROOT)} is stale: run tools/generate-cli-reference.py "
                     "and commit it")
        print(f"{PAGE.relative_to(ROOT)} is current")
        return
    PAGE.parent.mkdir(parents=True, exist_ok=True)
    PAGE.write_text(text)
    print(f"wrote {PAGE.relative_to(ROOT)}: {len(pages)} commands")


if __name__ == "__main__":
    main()
