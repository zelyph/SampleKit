"""`--clean-state`: what earlier versions left in this computer's state
folder, and this one reads nothing of. Listed, then removed with --write;
nothing else of the folder is touched."""

from __future__ import annotations

import os
import re
import shutil
import sys
from pathlib import Path

# What rc.1 and the builds before it kept there, by name.
LEFT_BY_EARLIER = ["computed.json", "computed", "consent", "accepted"]
# Their run logs, named by the run: `compute-2026-09-12T10-04-05Z-4242.log`.
# This version names its own by project, `compute-PROJECT-….log`.
RUN_LOG = re.compile(r"^compute-\d{4}-\d{2}-\d{2}T[0-9:\-\.]+Z-\d+\.log$")


def folder() -> Path:
    """This version's state folder, as the program finds it."""
    named = os.environ.get("SAMPLEKIT_STATE_DIR")
    if named:
        return Path(named)
    home = Path.home()
    if sys.platform == "win32":
        return Path(os.environ.get("LOCALAPPDATA", home / "AppData" / "Local")) / "samplekit"
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "samplekit"
    state = os.environ.get("XDG_STATE_HOME")
    return (Path(state) if state else home / ".local" / "state") / "samplekit"


def leftovers(at: Path) -> list[Path]:
    found = [at / name for name in LEFT_BY_EARLIER if (at / name).exists()]
    logs = at / "logs"
    if logs.is_dir():
        found += sorted(p for p in logs.iterdir() if p.is_file() and RUN_LOG.match(p.name))
    return found


def clean(write: bool) -> int:
    at = folder()
    found = leftovers(at)
    if not found:
        print(f"{at}: nothing an earlier version left, nothing to remove")
        return 0
    print(f"{at}: what earlier versions left, which this one reads nothing of:")
    for path in found:
        print(f"  {path.relative_to(at)}{'/' if path.is_dir() else ''}")
    if not write:
        print("Nothing removed: --write removes them.")
        return 0
    for path in found:
        if path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()
    print(f"removed: {len(found)}")
    return 0
