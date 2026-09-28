"""Fixtures for the tests of the Python notes.

They run against the installed extension: `maturin develop` must have built the
code under test, and the command line they compare with is `target/debug`.
"""

import pathlib
import sys
import subprocess

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[2]
# `samplekit.exe` on Windows.
BINARY = ROOT / "target" / "debug" / ("samplekit.exe" if sys.platform == "win32" else "samplekit")


def _newest_source(binary):
    """The most recently changed file a build is made from.

    Each build is compared with what it is actually made of. `src/interfaces/`
    is the binary's alone, so a change there leaves the extension exactly as
    current as it was; `src/python/` is behind the `python` feature, which the
    binary is not built with, so a change there leaves the binary current.
    """
    sources = [ROOT / "Cargo.toml", *ROOT.joinpath("src").rglob("*.rs")]
    excluded = ROOT / "src" / ("python" if binary else "interfaces")
    sources = [path for path in sources if excluded not in path.parents]
    return max(sources, key=lambda path: path.stat().st_mtime)


def pytest_sessionstart(session):
    """Refuses to test a build older than the code.

    A stale extension tests the previous version, which is worse than not
    testing: on 2026-09-19 a whole decision's Python half ran green against an
    extension built before the code it was meant to test. A changed timestamp
    with unchanged content costs one rebuild; the other mistake costs a defect.
    """
    import os

    import samplekit._native as native

    if os.environ.get("SAMPLEKIT_ALLOW_STALE_BUILD"):
        return
    builds = [
        ("the extension", pathlib.Path(native.__file__), "maturin develop --release", _newest_source(False)),
        ("the command line these tests compare with", BINARY, "cargo build", _newest_source(True)),
    ]
    stale = [
        (what, built, how, newest)
        for what, built, how, newest in builds
        if not built.is_file() or built.stat().st_mtime < newest.stat().st_mtime
    ]
    if stale:
        lines = [
            f"{what} ({built.relative_to(ROOT) if built.is_relative_to(ROOT) else built}) "
            f"is older than {newest.relative_to(ROOT)}: run `{how}`"
            for what, built, how, newest in stale
        ]
        pytest.exit(
            "refusing to test a stale build\n  " + "\n  ".join(lines)
            + "\n  SAMPLEKIT_ALLOW_STALE_BUILD=1 runs anyway",
            returncode=2,
        )

PLATOS = """\
schema_version = 1

[property.malt]
unit = "g"
precision = ".3f"

[query.heavy]
filter = "malt > 12"

[profile.platos]
sort = ["-malt"]
columns = [
  {field = "name"},
  {field = "malt", precision = ".2f"},
]

[export.malts]
profile = "platos"
format = "csv"
output = "malts.csv"
"""

MODEL = """\
import pathlib

from samplekit import Property, Sample

pathlib.Path(__file__).with_name("imported.txt").write_text("imported")


class Model(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = Property(unit="kg")
        self.double = Property(compute=lambda: self.malt.value * 2)
        self.set_dependencies("double", depends_on=["malt"])
"""


def sample_text(name=None, attributes=None, properties=None, tags=None, note="# Notes\n"):
    """A sample file, its fields written as YAML text."""
    lines = ["---", "schema_version: 1"]
    if name is not None:
        lines.append(f"name: {name}")
    if tags:
        lines.append("tags: [" + ", ".join(tags) + "]")
    for key, value in (attributes or {}).items():
        lines.append(f"{key}: {value}")
    if properties:
        lines.append("properties:")
        for key, value in properties.items():
            lines.append(f"  {key}: {value}")
    lines.append("---")
    return "\n".join(lines) + "\n" + note


class Project:
    """A directory that may hold a `.samplekitrc`, samples and a model."""

    def __init__(self, root):
        self.root = root

    def config(self, text=PLATOS):
        path = self.root / ".samplekitrc"
        path.write_text(text)
        return path

    def model(self, text=MODEL):
        path = self.root / "model.py"
        path.write_text(text)
        return path

    def sample(self, file, **fields):
        path = self.root / file
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(sample_text(**fields))
        return path

    def imported(self):
        return (self.root / "imported.txt").exists()


@pytest.fixture
def project(tmp_path):
    root = tmp_path / "project"
    root.mkdir()
    return Project(root)


@pytest.fixture
def collection(project):
    """Three samples in a project declaring a query, a profile and an export."""
    project.config()
    project.sample(
        "A.md",
        name="A",
        attributes={"beer": "Pilsner", "period": 6},
        properties={"malt": "{v: 12.5, u: 0.05, unit: g}"},
    )
    project.sample(
        "B.md",
        name="B",
        attributes={"beer": "Weizen", "period": 6},
        properties={"malt": "{v: 13.1234, u: 0.02, unit: g}"},
    )
    project.sample(
        "C.md",
        name="C",
        attributes={"beer": "Pilsner", "period": 5},
        properties={"malt": "{v: 11.0, unit: g}"},
    )
    return project


@pytest.fixture
def cli():
    """The command line's standard output, which must succeed."""

    def run(*arguments):
        completed = subprocess.run(
            [str(BINARY), *map(str, arguments)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr
        return completed.stdout

    return run
