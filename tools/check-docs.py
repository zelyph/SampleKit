#!/usr/bin/env python3
"""Check the documentation in docs/ against the demo it walks through, the
tensile example.

Four checks:

1. **Links.** Every relative link between the pages, in every folder of
   docs/, leads to a file that exists, and every `#anchor` to a heading of
   that page. The demo's READMEs link to the documentation by its address on
   the forge, since they are also read where no documentation lies beside
   them: each such address must name a page of docs/, and each relative link
   a file of the demo.
2. **Commands.** A console block marked `<!-- run: STEP -->` just above it,
   `STEP` a step of the demo as `tensile/04-results`, runs line by line in a
   fresh copy of that step, and every command must exit 0; so does every
   console block of a step's own README. Lines that open a window or a
   terminal screen, `samplekit` alone, `samplekit tui`, `plot` without `-o`,
   `open`, are skipped.
3. **What they print.** Each `samplekit` command runs in a terminal
   (`script`): a line the block shows must be one it draws, in the colours
   the docs' lexer gives it, character by character. A Python script's lines
   must be ones it prints.
4. **The demo committed.** `examples/tensile/`, which the binary embeds for
   the start page, is file for file what fixtures/tensile generates now;
   `tools/regenerate-demo.sh` writes it again.

The demo is built from fixtures/tensile into target/docs-tensile, and its
commands run with a state directory of their own: what the owner's machine
records is never touched.

    tools/check-docs.py            links and commands
    tools/check-docs.py --links    links only, without building anything
"""

import fnmatch
import os
import re
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile
from importlib import util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
# The demo, the tensile example: committed, embedded in the binary for the
# start page, and the steps every runnable block of the docs runs in.
BUILT = ROOT / "target" / "docs-tensile"
RUNS = ROOT / "target" / "docs-runs"
EXAMPLE = ROOT / "examples" / "tensile"
# What running a step leaves beside it, never part of the demo: build.rs
# leaves the same out, and examples/.gitignore ignores it.
LEFT_BY_RUNS = {".samplekit", "__pycache__", ".venv", "out"}
BINARY = ROOT / "target" / "release" / "samplekit"
PYTHON = ROOT / ".venv" / "bin" / "python"
MARKER = re.compile(r"<!--\s*run:\s*([\w/-]+)\s*-->\s*\n```console\n(.*?)```", re.S)
LINK = re.compile(r"\]\(([^)\s]+)\)")
HEADING = re.compile(r"^#{1,6}\s+(.*)$", re.M)
FENCE = re.compile(r"^```.*?^```", re.M | re.S)
READMES = ROOT / "fixtures" / "tensile" / "readme"
# The docs' lexer, and a terminal's screen, to hold the colours to the
# command line's.
sys.path.insert(0, str(DOCS / "_ext"))
sys.path.insert(0, str(ROOT / "tools"))
import samplekit_lexers  # noqa: E402
from terminal_screen import screen  # noqa: E402

LEXED = {
    samplekit_lexers.VALUE: "bold",
    samplekit_lexers.HEADER: "bold",
    samplekit_lexers.UNIT: "cyan",
    samplekit_lexers.ATTRIBUTE: "blue",
    samplekit_lexers.OUTDATED: "yellow",
    samplekit_lexers.FAILED: "red",
    samplekit_lexers.EDITED: "magenta",
    samplekit_lexers.CURRENT: "green",
    samplekit_lexers.MUTED: "dark-grey",
    samplekit_lexers.IDENTITY: "dim",
}
# How many lines a terminal's colours were held to, said with the commands.
COMPARED = [0]
TERMINAL = shutil.which("script") is not None and sys.platform.startswith("linux")


# The site and the repository, as the demo's READMEs link to them.
SITE = "https://zelyph.github.io/SampleKit/latest"
RAW = "https://raw.githubusercontent.com/zelyph/SampleKit/main"
PAGE_ADDRESS = re.compile(re.escape(SITE) + r"/([^)\s#]*)\.html(#[^)\s]*)?")
IMAGE_ADDRESS = re.compile(re.escape(RAW) + r"/docs/([^)\s#]*)")


def shown(page):
    return page.relative_to(ROOT).as_posix()


def pages():
    """The pages of the site: every page under docs/ but those conf.py leaves
    out of the build — the guide written before the four parts."""
    excluded = runpy.run_path(str(DOCS / "conf.py"))["exclude_patterns"]
    for page in sorted(DOCS.rglob("*.md")):
        relative = page.relative_to(DOCS)
        # A hidden folder (an editor's, Obsidian's) holds no page.
        if any(part.startswith(".") for part in relative.parts):
            continue
        if not any(fnmatch.fnmatch(relative.as_posix(), pattern) or relative.parts[0] == pattern
                   for pattern in excluded):
            yield page


def name(page):
    return page.relative_to(DOCS).as_posix()


def slug(heading):
    """A heading's anchor, as GitHub and most Markdown viewers make it."""
    text = heading.strip().lower().replace("`", "")
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s", "-", text)


def anchors(page):
    text = FENCE.sub("", page.read_text())
    return {slug(heading) for heading in HEADING.findall(text)}


def check_link(page, target, base):
    """What is wrong with `target`, a link of `page` resolved from `base`."""
    path, _, anchor = target.partition("#")
    linked = (base / path).resolve() if path else page
    if not linked.exists():
        return f"{shown(page)}: '{target}' leads to no file"
    if anchor and linked.suffix == ".md" and anchor not in anchors(linked):
        return f"{shown(page)}: '{target}' leads to no heading of {linked.name}"
    return None


def check_links():
    failures = []
    for page in pages():
        # A link written in code is an example of one, not a link.
        text = re.sub(r"`[^`]*`", "", FENCE.sub("", page.read_text()))
        for target in LINK.findall(text):
            if re.match(r"[a-z]+://", target) or target.startswith("mailto:"):
                continue
            failures.append(check_link(page, target, page.parent))
    # The demo's READMEs: the documentation by its address, the demo's files
    # by their path beside the README as the demo writes it.
    for readme in sorted(READMES.glob("*.md")):
        step = EXAMPLE if readme.stem == "README" else EXAMPLE / readme.stem
        text = FENCE.sub("", readme.read_text())
        for target in LINK.findall(text):
            page = PAGE_ADDRESS.fullmatch(target)
            image = IMAGE_ADDRESS.fullmatch(target)
            if page:
                failures.append(check_link(readme, page.group(1) + ".md"
                                           + (page.group(2) or ""), DOCS))
            elif image:
                failures.append(check_link(readme, image.group(1), DOCS))
            elif target.startswith((SITE, RAW)):
                failures.append(f"{shown(readme)}: {target} is neither a page of the "
                                "site nor an image of docs/")
            elif not re.match(r"[a-z]+://", target):
                failures.append(check_link(readme, target, step))
    return [failure for failure in failures if failure]


def environment(state):
    env = dict(os.environ)
    env.update(
        SAMPLEKIT_STATE_DIR=str(state),
        MPLBACKEND="Agg",
        COLUMNS="100",
        NO_COLOR="1",
        PYTHONDONTWRITEBYTECODE="1",
        PATH=f"{BINARY.parent}:{PYTHON.parent}:{env.get('PATH', '')}",
    )
    return env


def build(state):
    if BUILT.exists():
        shutil.rmtree(BUILT)
    # In the same state folder as the commands: the demo's computations record
    # their formulas' digests there, never in the machine's own.
    subprocess.run([ROOT / "fixtures" / "tensile" / "build.sh", BUILT], check=True,
                   env=environment(state), stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)


COMMANDS = {"status", "compute", "explain", "tag", "export", "plot", "view",
            "validate", "restore", "remove", "run", "diff", "list", "new", "set", "log",
            "tui", "init", "open", "help", "rename"}


def split_command(arguments):
    """The paths written before the command, the command, and what follows:
    `samplekit pieces status` as `samplekit status pieces` was."""
    for at, word in enumerate(arguments):
        if word in COMMANDS:
            return arguments[:at], word, arguments[at + 1:]
        if word.startswith("-"):
            break
    return arguments, None, []


def skipped(arguments):
    if not arguments:
        return True
    _, command, rest = split_command(arguments)
    if command in ("tui", "open"):
        return True
    return command == "plot" and "-o" not in rest


def blocks():
    """Every block to run: the pages' marked ones, then each step's README."""
    for page in pages():
        run = page.relative_to(DOCS).with_suffix("").as_posix().replace("/", "-")
        for number, (step, block) in enumerate(MARKER.findall(page.read_text())):
            yield shown(page), f"{run}-{number}", step, block
    # Each step's README; the demo's own, above them, makes an environment.
    for readme in sorted(READMES.glob("[0-9]*.md")):
        found = re.findall(r"```console\n(.*?)```", readme.read_text(), re.S)
        for number, block in enumerate(found):
            yield f"{readme.stem}/README.md", f"readme-{readme.stem}-{number}", readme.stem, block


def lexed_styles(block):
    """The style the docs give each character of a block, by its offset."""
    styles = {}
    lexer = samplekit_lexers.SampleKitConsoleLexer()
    for at, kind, part in lexer.get_tokens_unprocessed(block):
        style = next((name for token, name in LEXED.items() if kind in token), "plain")
        for offset in range(len(part)):
            styles[at + offset] = style
    return styles


def colours_differ(name, block, styles, number, drawn):
    """The lines a block shows after its command at line `number`, held to
    what the terminal drew: the first character whose style differs."""
    lines = block.split("\n")
    offset = sum(len(line) + 1 for line in lines[: number + 1])
    held = {}
    for line, style in drawn:
        held.setdefault(line, style)
    failures = []
    for line in lines[number + 1 :]:
        if line.startswith("$ "):
            break
        style = held.get(line)
        # A line shown that the command did not print is an output made up,
        # or one the command no longer gives: `…` marks a line left out, and a
        # time of day changes at each run.
        # A question is drawn with the answer the closed input gave it.
        if style is None and line.endswith("]"):
            style = next((drawn_style for drawn_line, drawn_style in held.items()
                          if drawn_line.startswith(line)), None)
        if (line.strip() and style is None and "…" not in line
                and not re.search(r"\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}", line)):
            failures.append(f"{name}: shown, and not printed by the command: {line.strip()[:70]!r}")
        if line.strip() and style is not None:
            COMPARED[0] += 1
            for column, (character, got) in enumerate(zip(line, style)):
                want = styles.get(offset + column, "plain")
                if character != " " and want != got:
                    failures.append(
                        f"{name}: colours of {line.strip()[:60]!r}: {character!r} is "
                        f"{want} in the docs, {got} in a terminal"
                    )
                    break
        offset += len(line) + 1
    return failures


def check_commands(state):
    failures = []
    if RUNS.exists():
        shutil.rmtree(RUNS)
    # Each run is a step's copy, and a step names the .venv beside it.
    RUNS.mkdir(parents=True)
    (RUNS / ".venv").symlink_to(ROOT / ".venv")
    counted = 0
    for name, run, step, block in blocks():
        copy = RUNS / run
        # `tensile/04-results`: a step of the tensile example; else the demo's.
        source = BUILT / step.split("/", 1)[-1]
        shutil.copytree(source, copy, symlinks=True)
        cwd = copy
        styles = lexed_styles(block)
        for number, line in enumerate(block.splitlines()):
            if not line.startswith("$ "):
                continue
            command = re.split(r"\s{2,}#", line[2:])[0].strip()
            words = shlex.split(command)
            if words[0] == "cd":
                cwd = (cwd / words[1]).resolve()
                continue
            made = None
            if words[0] == "samplekit":
                if skipped(words[1:]):
                    continue
                words[0] = str(BINARY)
                # An environment init would make is the repository's, linked:
                # nothing is installed from the network here.
                paths, command, rest = split_command(words[1:])
                if command == "init" and "--no-venv" not in words and "--write" in words:
                    positional = paths + [word for word in rest if not word.startswith("-")]
                    made = cwd / positional[-1] if positional else cwd
                    words.append("--no-venv")
            elif words[0] == "python":
                words[0] = str(PYTHON)
            counted += 1
            if TERMINAL and words[0] == str(BINARY):
                # In a terminal 100 columns wide, its colours on: its exit is
                # the command's, and what it drew is held to the docs.
                coloured = {key: value for key, value in environment(state).items()
                            if key != "NO_COLOR"}
                coloured["TERM"] = "xterm-256color"
                # Read as bytes: text mode would make each carriage return a
                # line, and a progress bar many.
                raw = subprocess.run(
                    ["script", "-q", "-e", "-c",
                     "stty cols 100 rows 60; exec " + shlex.join(words), "/dev/null"],
                    cwd=cwd, env=coloured, stdin=subprocess.DEVNULL, capture_output=True)
                ran = subprocess.CompletedProcess(
                    raw.args, raw.returncode,
                    raw.stdout.decode("utf-8", "replace"),
                    raw.stderr.decode("utf-8", "replace"))
                failures += colours_differ(name, block, styles, number, screen(ran.stdout))
            else:
                ran = subprocess.run(words, cwd=cwd, env=environment(state),
                                     stdin=subprocess.DEVNULL, capture_output=True, text=True)
            # What a Python script printed, shown after it, is held to what it
            # prints now, line by line.
            if words[0] == str(PYTHON) and ran.returncode == 0:
                printed = set(ran.stdout.splitlines())
                for shown_line in block.split("\n")[number + 1:]:
                    if shown_line.startswith("$ "):
                        break
                    if shown_line.strip() and shown_line not in printed and "…" not in shown_line:
                        failures.append(f"{name}: shown, and not printed by the script: "
                                        f"{shown_line.strip()[:70]!r}")
                    elif shown_line.strip():
                        COMPARED[0] += 1
            if made is not None and ran.returncode == 0 and not (made / ".venv").exists():
                (made / ".venv").symlink_to(ROOT / ".venv")
            if ran.returncode != 0:
                said = (ran.stderr or ran.stdout).strip().splitlines()[:4]
                failures.append(f"{name} ({step}): {command}\n    exit "
                                f"{ran.returncode}: " + "\n    ".join(said))
    print(f"{counted} commands run, {COMPARED[0]} lines of their output held to their colours")
    return failures


def demo_files(folder):
    """The demo's files under `folder`, by their path in it: what running its
    steps leaves aside."""
    found = {}
    for path in sorted(folder.rglob("*")):
        relative = path.relative_to(folder)
        if LEFT_BY_RUNS.intersection(relative.parts) or path.suffix == ".pyc":
            continue
        if path.is_file() and not path.is_symlink():
            found[relative.as_posix()] = path.read_bytes()
    return found


def check_example(example=EXAMPLE, built_at=BUILT, fixture="tensile"):
    """examples/tensile, committed, is what fixtures/tensile generates now:
    generation is deterministic, so file for file. The same of
    examples/tensile."""
    named = example.relative_to(ROOT).as_posix()
    if not example.is_dir():
        return [f"{named} is missing: run tools/regenerate-demo.sh"]
    built, committed = demo_files(built_at), demo_files(example)
    differing = [
        *(f"  {name}: not in {named}" for name in built.keys() - committed.keys()),
        *(f"  {name}: in {named}, not generated" for name in committed.keys() - built.keys()),
        *(f"  {name}: differs" for name in built.keys() & committed.keys()
          if built[name] != committed[name]),
    ]
    if not differing:
        return []
    return [f"{named} is not what fixtures/{fixture} generates:\n"
            + "\n".join(sorted(differing))
            + f"\n  run tools/regenerate-demo.sh, then commit {named}"]


def main():
    failures = check_links()
    if "--links" not in sys.argv:
        with tempfile.TemporaryDirectory() as state:
            build(Path(state))
            failures += check_example()
            failures += check_commands(Path(state))
    for failure in failures:
        print(failure)
    if failures:
        sys.exit(f"{len(failures)} problems in docs/")
    print("docs/ is consistent with the demo")


if __name__ == "__main__":
    main()
