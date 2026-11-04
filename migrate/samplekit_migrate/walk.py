"""The files of a project, those of Python's own and of git left out."""

from __future__ import annotations

import os
from pathlib import Path

# Made again where they are needed, never copied: an environment holds the
# earlier version's package, and a repository its own history.
LEFT_OUT = {".git", "__pycache__", ".mypy_cache", ".pytest_cache", ".ruff_cache"}


def is_environment(folder: Path) -> bool:
    return (folder / "pyvenv.cfg").is_file()


def project_files(source: Path) -> list[Path]:
    """Every file under `source`, in a fixed order."""
    found: list[Path] = []
    for folder, folders, files in os.walk(source):
        here = Path(folder)
        folders[:] = sorted(
            f for f in folders if f not in LEFT_OUT and not is_environment(here / f)
        )
        for name in sorted(files):
            path = here / name
            if path.is_file():
                found.append(path)
    return found


def left_out(source: Path) -> list[str]:
    """The folders `project_files` leaves out, to be said."""
    said: list[str] = []
    for folder, folders, _ in os.walk(source):
        here = Path(folder)
        for name in sorted(folders):
            if name == ".git" or is_environment(here / name):
                said.append((here / name).relative_to(source).as_posix())
        folders[:] = [f for f in folders if f not in LEFT_OUT and not is_environment(here / f)]
    return said
