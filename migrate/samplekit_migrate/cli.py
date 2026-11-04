"""`python samplekit-migrate.pyz SOURCE --to TARGET`: a project of an earlier
SampleKit, carried into a copy this version reads."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

from . import FOR_VERSION, PUBLISHED
from . import description as d
from . import read_0x, read_rc1, write
from .model_0x import ModelError
from .walk import left_out

NOTICE = """\
SampleKit migration tool, for SampleKit {version}.

Nothing guarantees that this tool carries your data correctly: it cannot fit
every use, and it will miss some. It works on a copy, so that nothing is lost:
it never writes in the folder it reads.

Before anything, keep a backup of your project: three copies, on two kinds of
media, one of them elsewhere (3-2-1), and, better still, a git repository.
"""

HEADINGS = [
    read_0x.CARRIED,
    write.KEPT,
    write.NOT_CARRIED,
    read_0x.LEFT,
    read_0x.CHECK,
]


def parser() -> argparse.ArgumentParser:
    made = argparse.ArgumentParser(
        prog="samplekit-migrate",
        description=(
            "Carry a project of an earlier SampleKit (0.1.0, 0.1.1, 0.1.2, 0.2.0 or 1.0.0-rc.1) "
            f"into a new folder SampleKit {FOR_VERSION} reads. Run it with the Python of an "
            f"environment where samplekit {FOR_VERSION} is installed. It writes nothing without --write."
        ),
    )
    made.add_argument("source", type=Path, nargs="?", help="the project's folder, read and never written")
    made.add_argument("--to", type=Path, dest="target", help="the new folder to write: it must not exist, or be empty")
    made.add_argument("--from", dest="version", choices=list(PUBLISHED), help="the version that wrote the project, where it is not recognised")
    made.add_argument("--model", type=Path, help="0.x: the module declaring the class of the samples")
    made.add_argument("--class", dest="class_name", help="the class of the samples, where the module holds several")
    made.add_argument("--no-model", action="store_true", help="0.x without its model: every value carried as entered")
    made.add_argument("--keep-computed", action="store_true", help="0.x: carry the values a formula computed, as values typed over it (edited)")
    made.add_argument("--write", action="store_true", help="write the target; without it, say what would be written")
    made.add_argument("--clean-state", action="store_true", help="list what earlier versions left in this computer's state folder; with --write, remove it")
    made.add_argument("--verify", action="store_true", help="0.x, with --write: compute the copy and compare with the values 0.x computed")
    return made


def say(text: str = "") -> None:
    print(text)


def refuse(text: str) -> int:
    sys.stdout.flush()
    print(f"error: {text}", file=sys.stderr)
    return 2


def installed_samplekit() -> str | None:
    try:
        import samplekit
    except ImportError:
        return None
    return getattr(samplekit, "__version__", "")


def recognised(source: Path) -> str | None:
    if read_rc1.is_rc1_project(source):
        return "1.0.0-rc.1"
    for path in sorted(source.rglob("*.md")):
        try:
            data, _ = read_0x.frontmatter(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError):
            continue
        if read_0x.is_0x_sample(data):
            return "0.x"
    return None


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    # A model loaded here leaves no __pycache__ in the copy.
    sys.dont_write_bytecode = True
    say(NOTICE.format(version=FOR_VERSION))
    version = installed_samplekit()
    if version != FOR_VERSION:
        found = "no samplekit" if version is None else f"samplekit {version}"
        return refuse(
            f"this Python, {sys.executable}, has {found}: the tool writes through samplekit "
            f"{FOR_VERSION}. Run it with an environment's Python where it is installed:\n"
            f"  python -m venv .venv\n  .venv/bin/python -m pip install samplekit=={FOR_VERSION}\n"
            f"  .venv/bin/python samplekit-migrate.pyz SOURCE --to TARGET"
        )
    if arguments.clean_state:
        from .state import clean

        code = clean(arguments.write)
        if arguments.source is None:
            return code
    if arguments.source is None or arguments.target is None:
        return refuse("name the project and the new folder: samplekit-migrate.pyz OLD --to NEW")
    source, target = arguments.source, arguments.target
    if not source.is_dir():
        return refuse(f"{source} is not a folder")
    resolved_source, resolved_target = source.resolve(), target.resolve()
    if resolved_target == resolved_source or resolved_source in resolved_target.parents:
        return refuse(f"{target} is inside {source}: the target is a new folder, beside the project")
    if target.exists() and not target.is_dir():
        return refuse(f"{target} is a file: name a new folder")
    if target.exists() and any(target.iterdir()):
        return refuse(f"{target} exists and is not empty: name a new folder")

    if arguments.verify and not arguments.write:
        return refuse("--verify compares what --write wrote: give both")
    family = PUBLISHED.get(arguments.version) if arguments.version else recognised(source)
    if family is None:
        return refuse(
            f"{source} holds nothing an earlier SampleKit wrote: no .samplekitrc, and no file of "
            "0.x's samples. --from names the version"
        )
    try:
        if family == "1.0.0-rc.1":
            project = read_rc1.read(source, arguments.version or "1.0.0-rc.1")
        else:
            if arguments.model is None and not arguments.no_model:
                return refuse(
                    "a file of 0.x does not say which of its values a formula computed: --model names "
                    "the module of your 0.x class, or --no-model carries every value as entered"
                )
            if arguments.model is not None and not arguments.model.is_file():
                return refuse(f"{arguments.model} is not a file")
            project = read_0x.read(source, arguments.model, arguments.class_name, arguments.version or "0.x")
    except ModelError as error:
        return refuse(str(error))
    for folder in left_out(source):
        project.say(read_0x.LEFT, f"{folder}: not copied; make the environment, or the repository, again")

    files = write.plan(project)
    say(f"{'Writing' if arguments.write else 'Would write'} {len(files)} files to {target}:")
    for path in files[:12]:
        say(f"  {path}")
    if len(files) > 12:
        say(f"  … and {len(files) - 12} more")
    say()
    if not arguments.write:
        computed = sorted(
            {
                name
                for sample in project.samples
                for name, quantity in sample.properties.items()
                if quantity.origin == d.COMPUTED
            }
            | {
                f"{name}.{column}"
                for sample in project.samples
                for name, table in sample.tables.items()
                for row in table.rows
                for column, cell in row.items()
                if cell.origin == d.COMPUTED
            }
        )
        if computed and not arguments.keep_computed:
            project.say(write.NOT_CARRIED, ", ".join(computed))
        report(project, None)
        say("Nothing written: --write writes it.")
        return 0

    failures = write.write(project, source, target, arguments.keep_computed)
    for failure in failures:
        project.say(read_0x.CHECK, f"not written, {failure}")
    for refused in loaded(project, target):
        project.say(read_0x.CHECK, refused)
    checked = after(target)
    text = report(project, checked, (target / ".venv").exists())
    (target / "MIGRATION.md").write_text(text, encoding="utf-8")
    say(f"written: {target}, and what it carried in {target / 'MIGRATION.md'}")
    if arguments.verify:
        from .verify import verify

        return verify(project, target)
    return 1 if failures else 0


def loaded(project: d.Project, target: Path) -> list[str]:
    """Each sample written, read with the migrated model, as compute will:
    `validate` reads the files and not the model, and said nothing of a model
    that cannot load them."""
    if project.model is None or not project.samples:
        return []
    import samplekit as sk

    refused = []
    for sample in project.samples:
        try:
            sk.Sample(target / sample.path)
        except Exception as error:  # noqa: BLE001 - said, the next sample goes on
            first = str(error).splitlines()[0] if str(error) else ""
            refused.append(
                f"{sample.path}: the migrated model does not read it, {type(error).__name__}: {first}"
            )
    return refused


def after(target: Path) -> list[str]:
    """What this version's own `validate` and `status` say of the copy."""
    binary = shutil.which("samplekit")
    if binary is None:
        return ["samplekit, the command line, is not found: run `samplekit TARGET validate` yourself"]
    version = subprocess.run([binary, "--version"], capture_output=True, text=True, check=False)
    if version.stdout.split()[-1:] != [FOR_VERSION]:
        return [
            f"{binary} is {version.stdout.strip() or 'not this version'}: run SampleKit "
            f"{FOR_VERSION}'s `samplekit TARGET validate` yourself"
        ]
    said = []
    # The folder written with its `/`: a folder named as a command, `new`,
    # would be read as that command.
    folder = f"{target}{os.sep}"
    for command in (["validate"], ["status"]):
        done = subprocess.run(
            [binary, folder, *command],
            capture_output=True,
            text=True,
            check=False,
            stdin=subprocess.DEVNULL,
        )
        output = (done.stdout + done.stderr).strip()
        said.append(f"$ samplekit {target.name}/ {' '.join(command)}\n{output}")
    return said


def report(project: d.Project, checked: list[str] | None, environment: bool = False) -> str:
    """What was carried, left and to check, and what to run next: printed, and
    with --write the target's MIGRATION.md."""
    lines = ["# Migration", "", NOTICE.format(version=FOR_VERSION).strip(), ""]
    named = "0.x (0.1.0 to 0.2.0)" if project.version == "0.x" else project.version
    lines += [f"Read as SampleKit {named}.", ""]
    for heading in HEADINGS:
        items = project.said.get(heading)
        if not items:
            continue
        lines += [f"## {heading}", ""]
        lines += [f"- {item}" for item in items]
        lines.append("")
    lines += ["## What to do next", ""]
    lines.append(
        "1. Make the project's environment, in the new folder: "
        f"`python -m venv .venv`, then `.venv/bin/python -m pip install samplekit=={FOR_VERSION}`."
    )
    lines.append("2. Read the model and every line marked `# TODO(migrate)`.")
    if project.version == "0.x":
        lines.append(
            "3. `samplekit . validate`, then `samplekit . compute --write`, which computes again "
            "what was not carried."
        )
    else:
        lines.append(
            "3. `samplekit . validate`, then `samplekit . status`: the files' computed values are "
            "taken as computed by the model as it is. A formula changed since rc.1 last computed "
            "goes unseen: `samplekit . compute -p NAME --rerun --write` computes its values again."
        )
    lines += [
        "4. Compare a few samples with the earlier version's by hand before trusting the rest.",
        "",
    ]
    if checked:
        lines += ["## What this version says of the copy", ""]
        if not environment:
            lines += [
                "Until step 1 is done, `validate` says that the environment `[model] python` "
                "names does not exist, and `status` that the model was not read.",
                "",
            ]
        for block in checked:
            lines += ["```text", block, "```", ""]
    text = "\n".join(lines)
    for line in lines[4:]:
        say(line)
    return text
