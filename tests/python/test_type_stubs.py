"""The type stubs."""

import ast
import pathlib
import subprocess
import sys
import tomllib
import zipfile

import samplekit as sk
from conftest import ROOT

STUB = ROOT / "python" / "samplekit" / "__init__.pyi"
EXAMPLES = pathlib.Path(__file__).resolve().parent / "examples"


def stub_tree():
    return ast.parse(STUB.read_text())


def stub_classes():
    """Each class the stub declares, with the public names its body declares."""
    classes = {}
    for node in stub_tree().body:
        if not isinstance(node, ast.ClassDef):
            continue
        members = set()
        for item in node.body:
            if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                members.add(item.name)
            elif isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                members.add(item.target.id)
        classes[node.name] = {member for member in members if not member.startswith("_")}
    return classes


def stub_module_names():
    """The functions and the constants the stub declares at the top level; a
    type alias is the checker's alone, and exists nowhere at runtime."""
    names = set()
    for node in stub_tree().body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            names.add(node.name)
        elif (
            isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and not (isinstance(node.annotation, ast.Name) and node.annotation.id == "TypeAlias")
        ):
            names.add(node.target.id)
    return {name for name in names if not name.startswith("_")}


def runtime_members(cls):
    """A class's public names beyond what its builtin base already has."""
    base = list if issubclass(cls, list) else object
    return {name for name in dir(cls) if not name.startswith("_") and name not in dir(base)}


def mypy(*arguments):
    return subprocess.run(
        [sys.executable, "-m", "mypy", "--no-incremental", *map(str, arguments)],
        capture_output=True,
        text=True,
        check=False,
        cwd=ROOT,
    )


def test_every_runtime_attribute_is_in_the_stub():
    classes = stub_classes()
    declared = set(classes) | stub_module_names()
    assert set(sk.__all__) <= declared, set(sk.__all__) - declared
    for name in sk.__all__:
        # A function or a constant — `load`, `NA` — has no members to compare.
        if name in classes:
            undeclared = runtime_members(getattr(sk, name)) - classes[name]
            assert not undeclared, f"{name}: {sorted(undeclared)}"


def test_every_stub_entry_exists_at_runtime():
    for name, members in stub_classes().items():
        assert name in sk.__all__, name
        missing = members - set(dir(getattr(sk, name)))
        assert not missing, f"{name}: {sorted(missing)}"
    for name in stub_module_names():
        assert name in sk.__all__, name
        assert hasattr(sk, name), name


def test_stub_passes_a_type_checker():
    result = mypy("--strict", STUB)
    assert result.returncode == 0, result.stdout + result.stderr


def test_example_scripts_type_check():
    result = mypy("--strict", EXAMPLES / "usage.py")
    assert result.returncode == 0, result.stdout + result.stderr


def test_py_typed_is_installed(tmp_path):
    subprocess.run(
        [sys.executable, "-m", "maturin", "build", "--out", str(tmp_path)],
        capture_output=True,
        check=True,
        cwd=ROOT,
    )
    wheel = next(tmp_path.glob("samplekit-*.whl"))
    names = zipfile.ZipFile(wheel).namelist()
    assert "samplekit/py.typed" in names
    assert "samplekit/__init__.pyi" in names


def test_overloads_resolve_to_distinct_types(tmp_path):
    script = tmp_path / "overloads.py"
    script.write_text(
        "from samplekit import SampleList\n"
        "samples = SampleList('.')\n"
        "reveal_type(samples[0])\n"
        "reveal_type(samples[:2])\n"
        "reveal_type(samples['name'])\n"
    )
    result = mypy(script)
    revealed = [line for line in result.stdout.splitlines() if "Revealed type" in line]
    assert len(revealed) == 3, result.stdout
    assert '"samplekit.Sample"' in revealed[0]
    assert '"samplekit.SampleList"' in revealed[1]
    assert '"samplekit.Sample"' in revealed[2]
    assert revealed[0] != revealed[1]


def test_scalar_and_attribute_aliases_cover_the_storage_domain(tmp_path):
    script = tmp_path / "aliases.py"
    script.write_text(
        "from datetime import date, datetime\n"
        "from typing import TYPE_CHECKING\n"
        "if TYPE_CHECKING:\n"
        "    from samplekit import Attribute, AttributeScalar, BoundList, Scalar\n"
        "def check(tags: 'BoundList[str]') -> None:\n"
        "    values: 'list[Scalar]' = [True, 1, 1.5, 'text', date(2026, 1, 1),\n"
        "                             datetime(2026, 1, 1, 10, 30), None]\n"
        "    items: 'list[AttributeScalar]' = [True, 1, 1.5, 'text', date(2026, 1, 1)]\n"
        "    listed: 'Attribute' = tags\n"
        "    scalar: 'Attribute' = None\n"
    )
    result = mypy("--strict", script)
    assert result.returncode == 0, result.stdout + result.stderr


def test_a_declaration_by_channel_type_checks(tmp_path):
    script = tmp_path / "declared.py"
    script.write_text(
        "import samplekit as sk\n"
        "def build(sample: sk.Sample) -> None:\n"
        "    sample.p = sk.Property(compute=lambda: 1.0,\n"
        "                           depends_on={'v': ['a'], 'u': ['a', 'p.v']})\n"
        "    sample.set_dependencies('p', depends_on={'v': ['a']})\n"
        "    sample.set_dependencies('p', depends_on=['a'])\n"
    )
    result = mypy("--strict", script)
    assert result.returncode == 0, result.stdout + result.stderr


def test_several_paths_load_a_list(tmp_path):
    script = tmp_path / "loads.py"
    script.write_text(
        "import samplekit as sk\n"
        "reveal_type(sk.load('a', 'b'))\n"
        "reveal_type(sk.load('a'))\n"
    )
    result = mypy(script)
    revealed = [line for line in result.stdout.splitlines() if "Revealed type" in line]
    assert len(revealed) == 2, result.stdout
    assert revealed[0].endswith('"samplekit.SampleList"'), revealed[0]
    # One path is a file or a folder, which no type tells apart: the script
    # knows which it named.
    assert revealed[1].endswith('"Any"'), revealed[1]


def test_a_models_arithmetic_type_checks(tmp_path):
    # The demo's model and its scripts, bodies checked: a property's value,
    # a column's values and a folder loaded were unions no arithmetic, and no
    # loop over the samples, got past.
    brewing = ROOT / "fixtures" / "brewing"
    sources = [*brewing.joinpath("model").glob("*.py"), *brewing.joinpath("scripts").glob("*.py")]
    assert len(sources) == 4, sources
    for source in sources:
        (tmp_path / source.name).write_text(source.read_text())
    # Untyped globals: a script's own `by_yeast = {}` is no question of the
    # stub's.
    result = mypy("--check-untyped-defs", "--allow-untyped-globals",
                  *sorted(tmp_path.glob("*.py")))
    assert result.returncode == 0, result.stdout + result.stderr


def test_the_stub_carries_the_packages_docstrings():
    # An editor shows the stub's docstrings: they are the package's, written
    # into it by the tool, never by hand.
    result = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "generate-stub-docstrings.py"), "--check"],
        capture_output=True,
        text=True,
        check=False,
        cwd=ROOT,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    stub = STUB.read_text()
    assert '"""Write the sample to its file.' in stub, "Sample.save carries no docstring"


def test_python_311_is_the_declared_minimum():
    metadata = tomllib.loads((ROOT / "pyproject.toml").read_text())
    assert metadata["project"]["requires-python"] == ">=3.11"
    ast.parse(STUB.read_text(), feature_version=(3, 11))
