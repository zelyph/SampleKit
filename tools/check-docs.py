#!/usr/bin/env python3
"""Check the documentation in docs/ against the demo it walks through.

Four checks:

1. **Links.** Every relative link between the pages, in every folder of
   docs/, leads to a file that exists, and every `#anchor` to a heading of
   that page. The demo's READMEs link to the documentation by its address on
   the forge, since they are also read where no documentation lies beside them
: each such address must name a page of docs/, and each relative
   link a file of the demo.
2. **Commands.** A console block marked `<!-- run: STEP -->` just above it runs,
   line by line, in a fresh copy of that step of the demo, and every command
   must exit 0 — and so does every console block of a step's own README.
   Lines that open a window or a terminal screen — `samplekit` alone,
   `samplekit tui`, `plot` without `-o`, `open` — are skipped.
3. **The tutorial.** docs/tutorial/NN-step.md is what
   tools/generate-tutorial.py makes of that step's README.
4. **The demo committed.** `examples/brewing/`, which the guide points to and
   the binary embeds, is file for file what fixtures/brewing generates now
; `tools/regenerate-demo.sh` writes it again.

The demo is built from fixtures/brewing into target/docs-demo, and its commands
run with a state directory of their own: what the owner's machine records is
never touched.

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
BUILT = ROOT / "target" / "docs-demo"
RUNS = ROOT / "target" / "docs-runs"
# The demo committed, and embedded in the binary for the start page.
EXAMPLE = ROOT / "examples" / "brewing"
# What running a step leaves beside it, never part of the demo: build.rs
# leaves the same out, and examples/.gitignore ignores it.
LEFT_BY_RUNS = {".samplekit", "__pycache__", ".venv", "out"}
BINARY = ROOT / "target" / "release" / "samplekit"
PYTHON = ROOT / ".venv" / "bin" / "python"
MARKER = re.compile(r"<!--\s*run:\s*([\w/-]+)\s*-->\s*\n```console\n(.*?)```", re.S)
LINK = re.compile(r"\]\(([^)\s]+)\)")
HEADING = re.compile(r"^#{1,6}\s+(.*)$", re.M)
FENCE = re.compile(r"^```.*?^```", re.M | re.S)
READMES = ROOT / "fixtures" / "brewing" / "readme"


def _tutorial():
    """tools/generate-tutorial.py, whose name is no module's."""
    spec = util.spec_from_file_location("generate_tutorial",
                                        ROOT / "tools" / "generate-tutorial.py")
    module = util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


TUTORIAL = _tutorial()


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
        text = FENCE.sub("", page.read_text())
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
            page = TUTORIAL.PAGE_ADDRESS.fullmatch(target)
            image = TUTORIAL.IMAGE_ADDRESS.fullmatch(target)
            if page:
                failures.append(check_link(readme, page.group(1) + ".md"
                                           + (page.group(2) or ""), DOCS))
            elif image:
                failures.append(check_link(readme, image.group(1), DOCS))
            elif target.startswith((TUTORIAL.SITE, TUTORIAL.RAW)):
                failures.append(f"{shown(readme)}: {target} is neither a page of the "
                                "site nor an image of docs/")
            elif not re.match(r"[a-z]+://", target):
                failures.append(check_link(readme, target, step))
    return [failure for failure in failures if failure]


def check_tutorial():
    """docs/tutorial/NN-step.md, each what its README makes."""
    return [f"{shown(path)} is not what fixtures/brewing/readme/{path.name} makes: "
            "run tools/generate-tutorial.py"
            for path, text in TUTORIAL.pages().items()
            if not path.exists() or path.read_text() != text]


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
    subprocess.run([ROOT / "fixtures" / "brewing" / "build.sh", BUILT], check=True,
                   env=environment(state), stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)


def skipped(arguments):
    if not arguments:
        return True
    if arguments[0] in ("tui", "open"):
        return True
    return arguments[0] == "plot" and "-o" not in arguments


def blocks():
    """Every block to run: the pages' marked ones, then each step's README."""
    for page in pages():
        run = page.relative_to(DOCS).with_suffix("").as_posix().replace("/", "-")
        for number, (step, block) in enumerate(MARKER.findall(page.read_text())):
            yield shown(page), f"{run}-{number}", step, block
    # Each step's README; the demo's own, above them, makes an environment.
    for readme in sorted((ROOT / "fixtures" / "brewing" / "readme").glob("[0-9]*.md")):
        found = re.findall(r"```console\n(.*?)```", readme.read_text(), re.S)
        for number, block in enumerate(found):
            yield f"{readme.stem}/README.md", f"readme-{readme.stem}-{number}", readme.stem, block


def check_commands(state):
    failures = []
    if RUNS.exists():
        shutil.rmtree(RUNS)
    counted = 0
    for name, run, step, block in blocks():
        copy = RUNS / run
        shutil.copytree(BUILT / step, copy, symlinks=True)
        cwd = copy
        for line in block.splitlines():
            if not line.startswith("$ "):
                continue
            command = re.split(r"\s{2,}#", line[2:])[0].strip()
            words = shlex.split(command)
            if words[0] == "cd":
                cwd = (cwd / words[1]).resolve()
                continue
            if words[0] == "samplekit":
                if skipped(words[1:]):
                    continue
                words[0] = str(BINARY)
            elif words[0] == "python":
                words[0] = str(PYTHON)
            counted += 1
            ran = subprocess.run(words, cwd=cwd, env=environment(state),
                                 stdin=subprocess.DEVNULL, capture_output=True, text=True)
            if ran.returncode != 0:
                said = (ran.stderr or ran.stdout).strip().splitlines()[:4]
                failures.append(f"{name} ({step}): {command}\n    exit "
                                f"{ran.returncode}: " + "\n    ".join(said))
    print(f"{counted} commands run")
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


def check_example():
    """examples/brewing, committed, is what fixtures/brewing generates now:
    generation is deterministic, so file for file."""
    if not EXAMPLE.is_dir():
        return [f"{EXAMPLE.relative_to(ROOT)} is missing: run tools/regenerate-demo.sh"]
    built, committed = demo_files(BUILT), demo_files(EXAMPLE)
    differing = [
        *(f"  {name}: not in examples/brewing" for name in built.keys() - committed.keys()),
        *(f"  {name}: in examples/brewing, not generated" for name in committed.keys() - built.keys()),
        *(f"  {name}: differs" for name in built.keys() & committed.keys()
          if built[name] != committed[name]),
    ]
    if not differing:
        return []
    return ["examples/brewing is not what fixtures/brewing generates:\n"
            + "\n".join(sorted(differing))
            + "\n  run tools/regenerate-demo.sh, then commit examples/brewing"]


def main():
    failures = check_links() + check_tutorial()
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
