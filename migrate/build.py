"""Builds samplekit-migrate.pyz: the tool, and a pure-Python copy of PyYAML
with its licence, in one Python zip application.

    python migrate/build.py dist/samplekit-migrate.pyz

The Python running it must have PyYAML installed: its pure-Python modules are
copied, its compiled accelerator left out, which PyYAML does without.
"""

from __future__ import annotations

import importlib.metadata
import shutil
import sys
import tempfile
import zipapp
from pathlib import Path

HERE = Path(__file__).resolve().parent


def build(output: Path) -> Path:
    import yaml

    with tempfile.TemporaryDirectory() as scratch:
        staged = Path(scratch) / "app"
        shutil.copytree(
            HERE / "samplekit_migrate",
            staged / "samplekit_migrate",
            ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
        )
        vendored = staged / "yaml"
        vendored.mkdir()
        for module in Path(yaml.__file__).parent.glob("*.py"):
            shutil.copy2(module, vendored / module.name)
        licence = next(
            (
                file
                for file in importlib.metadata.distribution("PyYAML").files or []
                if file.name.upper().startswith("LICENSE")
            ),
            None,
        )
        if licence is None:
            raise SystemExit("PyYAML's licence is not found beside it: it must be shipped with it")
        shutil.copy2(licence.locate(), vendored / "LICENSE")
        shutil.copy2(HERE / "README.md", staged / "README.md")
        (staged / "__main__.py").write_text(
            "import sys\n\nfrom samplekit_migrate.cli import main\n\nsys.exit(main())\n",
            encoding="utf-8",
        )
        output.parent.mkdir(parents=True, exist_ok=True)
        zipapp.create_archive(staged, output, interpreter="/usr/bin/env python3")
    return output


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: python migrate/build.py OUTPUT.pyz")
    print(build(Path(sys.argv[1])))
