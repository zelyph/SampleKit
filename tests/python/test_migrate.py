"""The migration tool, samplekit-migrate, on projects the earlier versions
wrote themselves: tests/data/migrate holds a tensile project made by
SampleKit 0.2.0's own package, and one made by 1.0.0-rc.1's."""

import importlib.util
import os
import pathlib
import shutil
import subprocess
import sys
import tomllib
import zipfile

import pytest

import samplekit as sk
from conftest import BINARY, ROOT

DATA = ROOT / "tests" / "data" / "migrate"
TOOL = ROOT / "migrate"


def run(*arguments, cwd=None):
    """The tool, as `python -m samplekit_migrate`, with this build's command line
    first on the PATH."""
    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(TOOL)
    environment["PATH"] = f"{BINARY.parent}{os.pathsep}{environment.get('PATH', '')}"
    return subprocess.run(
        [sys.executable, "-m", "samplekit_migrate", *map(str, arguments)],
        capture_output=True,
        text=True,
        env=environment,
        cwd=cwd,
        check=False,
    )


@pytest.fixture
def old(tmp_path):
    """The 0.2 project: its samples, and its model beside them."""
    source = tmp_path / "old"
    shutil.copytree(DATA / "0.2" / "samples", source / "samples")
    shutil.copy2(DATA / "0.2" / "specimen.py", source / "specimen.py")
    return source


def migrated(old, tmp_path, *more):
    target = tmp_path / "new"
    done = run(old, "--to", target, "--model", old / "specimen.py", "--write", *more)
    return target, done


def frontmatter(path):
    import yaml

    text = path.read_text()
    return yaml.safe_load(text[4 : text.index("\n---", 3)])


def command(*arguments):
    return subprocess.run([BINARY, *map(str, arguments)], capture_output=True, text=True, check=False)


def with_environment(target):
    """The project's environment, this repository's, as step 1 makes one."""
    (target / ".venv").symlink_to(ROOT / ".venv")


def test_a_0x_project_is_carried_to_a_copy_this_version_reads(old, tmp_path):
    target, done = migrated(old, tmp_path)
    assert done.returncode == 0, done.stderr
    with_environment(target)
    validated = command(target, "validate")
    assert validated.returncode == 0, validated.stdout + validated.stderr
    assert "3 samples · 0 defects" in validated.stdout
    steel = frontmatter(target / "samples" / "steel-01.md")
    assert steel["properties"]["width"]["readings"] == [12.49, 12.51, 12.5]
    assert steel["properties"]["gauge_length"] == {"v": 50, "u": 0.1, "unit": "mm"}
    curve = steel["tables"]["curve"]
    assert curve["index"] == "strain" and curve["title"] == "Tensile curve"
    assert [row["force"] for row in curve["rows"]] == [0, 9.512, 11.204, 12.93]
    # What a formula computed is not carried; the note is kept as it was,
    # what 0.x wrote there included.
    assert "area" not in steel["properties"]
    assert "stress" not in curve["columns"]
    text = (target / "samples" / "steel-01.md").read_text()
    old_text = (old / "samples" / "steel-01.md").read_text()
    body = old_text[old_text.index("\n---", 3) + 4 :].lstrip("\n")
    assert body.strip() and text.endswith(body), text
    report = (target / "MIGRATION.md").read_text()
    assert "the text below each frontmatter, as the note, as it is" in report


def test_without_write_nothing_is_written(old, tmp_path):
    target = tmp_path / "new"
    done = run(old, "--to", target, "--model", old / "specimen.py")
    assert done.returncode == 0, done.stderr
    assert "Would write" in done.stdout and "samples/steel-01.md" in done.stdout
    assert "Nothing written: --write writes it." in done.stdout
    assert not target.exists()


def test_the_source_is_never_written_and_the_target_must_be_new(old, tmp_path):
    before = {p: p.read_bytes() for p in old.rglob("*") if p.is_file()}
    busy = tmp_path / "busy"
    busy.mkdir()
    (busy / "kept.txt").write_text("kept")
    for target in (busy, old, old / "inside"):
        done = run(old, "--to", target, "--model", old / "specimen.py", "--write")
        assert done.returncode == 2, (target, done.stdout)
        assert "error:" in done.stderr
    assert not (old / "inside").exists()
    migrated(old, tmp_path)
    after = {p: p.read_bytes() for p in old.rglob("*") if p.is_file()}
    assert after == before


def test_every_output_begins_with_the_notice(old, tmp_path):
    for more in ([], ["--write"]):
        done = run(old, "--to", tmp_path / f"new{len(more)}", "--model", old / "specimen.py", *more)
        assert done.stdout.startswith("SampleKit migration tool"), done.stdout[:200]
        head = done.stdout[:600]
        assert "Nothing guarantees that this tool carries your data correctly" in head
        assert "3-2-1" in head and "git repository" in head


def test_a_0x_model_is_required_or_refused_explicitly(old, tmp_path):
    refused = run(old, "--to", tmp_path / "new")
    assert refused.returncode == 2
    assert "--model" in refused.stderr and "--no-model" in refused.stderr
    target = tmp_path / "plain"
    done = run(old, "--to", target, "--no-model", "--write")
    assert done.returncode == 0, done.stderr
    steel = frontmatter(target / "samples" / "steel-01.md")
    assert steel["properties"]["area"]["v"] == pytest.approx(25.00416667)
    # Named as no model, the module is a file of the project, copied as it was.
    assert "[model]" not in (target / ".samplekitrc").read_text()
    assert (target / "specimen.py").read_bytes() == (old / "specimen.py").read_bytes()


def model_of(target):
    spec = importlib.util.spec_from_file_location("migrated_specimen", target / "specimen.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_the_0x_model_is_written_in_the_current_api(old, tmp_path):
    target, done = migrated(old, tmp_path)
    assert done.returncode == 0, done.stderr
    text = (target / "specimen.py").read_text()
    assert "import samplekit as sk" in text and "class Specimen(sk.Sample):" in text
    assert "def __init__(self, path=None, name=None):" in text
    assert "super().__init__(path, name=name)" in text
    assert "compute_value=self._area," in text
    assert "compute_uncertainty=self._area_unc," in text
    assert 'depends_on=["width", "thickness"],' in text
    # 0.x named no input of tensile_strength: what its body reads, to check.
    at = text.index("self.tensile_strength")
    assert "# TODO(migrate): 0.x named no input" in text[text.rindex("\n", 0, at - 1) - 120 : at]
    assert 'depends_on=["curve.stress"],' in text
    assert "compute_value=lambda row: self._stress(row.force.value)," in text
    assert 'depends_on=["row.force", "area"],' in text
    assert 'index="strain",' in text
    # It imports, and builds a sample.
    module = model_of(target)
    specimen = module.Specimen()
    assert specimen.area.is_computed


def test_the_0x_statistic_is_written_as_it_was(old, tmp_path):
    target, _ = migrated(old, tmp_path)
    text = (target / "specimen.py").read_text()
    for name in ("width", "thickness"):
        at = text.index(f"self.{name} = sk.Property(")
        block = text[at : text.index(")", at)]
        assert "value=sk.stats.mean," in block and "uncertainty=sk.stats.sample_stdev," in block
        assert "sk.stats.standard_error" in text[at - 200 : at]


def test_user_functions_are_copied_and_marked(old, tmp_path):
    target, _ = migrated(old, tmp_path)
    text = (target / "specimen.py").read_text()
    for name in ("_area", "_area_unc", "_tensile_strength", "_stress"):
        at = text.index(f"def {name}(")
        assert text[:at].rstrip().endswith("check it against this version's API"), name
    assert "def template(" not in text
    report = (target / "MIGRATION.md").read_text()
    assert "_area, _area_unc, _tensile_strength, _stress" in report
    assert "too varied to be translated" in report


def test_presentation_goes_to_the_configuration_once(old, tmp_path):
    target, _ = migrated(old, tmp_path)
    configuration = tomllib.loads((target / ".samplekitrc").read_text())
    properties = configuration["property"]
    assert properties["thickness"] == {"symbol": "t", "precision": [".3f", ".4f"]}
    assert properties["tensile_strength"] == {"symbol_math": r"\sigma_{UTS}", "precision": ".0f"}
    assert properties["curve.stress"] == {"symbol_math": r"\sigma", "precision": ".1f"}
    assert configuration["unit"]["mm²"] == {"math": "mm^2"}
    assert "math" in configuration["style"]
    for sample in (target / "samples").glob("*.md"):
        text = sample.read_text()
        for key in ("precision", "symbol", "unit_math"):
            assert key not in text, (sample, key)


def test_a_typed_uncertainty_over_readings_is_said(old, tmp_path):
    target, _ = migrated(old, tmp_path)
    copper = frontmatter(target / "samples" / "copper-01.md")
    assert copper["properties"]["thickness"] == {"readings": [2.003, 1.999], "u": 0.003, "unit": "mm"}
    report = (target / "MIGRATION.md").read_text()
    assert "samples/copper-01.md: thickness, an uncertainty typed over its readings, 0.003" in report


def test_keep_computed_carries_them_as_edited(old, tmp_path):
    target, done = migrated(old, tmp_path, "--keep-computed")
    assert done.returncode == 0, done.stderr
    steel = frontmatter(target / "samples" / "steel-01.md")
    assert steel["properties"]["area"]["v"] == pytest.approx(25.00416667)
    with_environment(target)
    status = command(target, "status")
    assert "area" in status.stdout and "--force gives it back to its formula" in status.stdout
    report = (target / "MIGRATION.md").read_text()
    assert "## Computed by 0.x, carried as typed over the formula" in report
    assert "- area" in report


def test_text_properties_become_attributes(old, tmp_path):
    target, _ = migrated(old, tmp_path)
    steel = frontmatter(target / "samples" / "steel-01.md")
    assert steel["material"] == "steel" and steel["operator"] == "ana"
    text = (target / "specimen.py").read_text()
    assert "self.material" not in text and "self.operator" not in text
    assert "# material: an attribute of each sample now, declared by none." in text


def test_the_four_0x_versions_read_the_same(old, tmp_path):
    made = {}
    for version in ("0.1.0", "0.1.2", "0.2.0"):
        target = tmp_path / version
        done = run(old, "--to", target, "--model", old / "specimen.py", "--from", version, "--write")
        assert done.returncode == 0, done.stderr
        made[version] = {
            p.relative_to(target): p.read_bytes()
            for p in target.rglob("*")
            if p.is_file() and ".samplekit" not in p.parts and p.name != "MIGRATION.md"
        }
    assert made["0.1.0"] == made["0.1.2"] == made["0.2.0"]
    # 0.1's own model, its __init__(name, filepath), is read the same.
    shutil.copy2(DATA / "0.1" / "specimen.py", old / "specimen.py")
    target, done = migrated(old, tmp_path)
    assert "def __init__(self, path=None, name=None):" in (target / "specimen.py").read_text()
    refused = run(old, "--to", tmp_path / "other", "--from", "0.3.0")
    assert refused.returncode == 2
    assert "1.0.0-rc.1" in refused.stderr and "0.1.0" in refused.stderr


@pytest.fixture
def lab(tmp_path):
    """The other 0.2 project: a base class and a class deriving from it, a
    date, a flag, lambdas, and a row formula of the module."""
    source = tmp_path / "lab"
    shutil.copytree(DATA / "0.2-lab", source, ignore=shutil.ignore_patterns("make.py"))
    return source


def migrated_lab(lab, tmp_path, *more):
    target = tmp_path / "labnew"
    done = run(lab, "--to", target, "--model", lab / "lab" / "coupon.py", "--write", *more)
    return target, done


def test_dates_and_flags_become_attributes_and_leave_the_model(lab, tmp_path):
    target, done = migrated_lab(lab, tmp_path)
    assert done.returncode == 0, done.stderr
    import datetime

    brass = frontmatter(target / "coupons" / "2025-05" / "brass-01.md")
    assert brass["received"] == datetime.date(2025, 5, 12) and brass["polished"] is True
    text = (target / "lab" / "coupon.py").read_text()
    for name in ("material", "received", "polished"):
        assert f"self.{name} = " not in text
    report = (target / "MIGRATION.md").read_text()
    assert "received: no number, written as an attribute of each sample, and out of the model" in report
    # Read with the migrated model, as compute reads them.
    assert "does not read it" not in report


def test_a_base_class_and_its_subclass_are_both_rewritten(lab, tmp_path):
    target, done = migrated_lab(lab, tmp_path, "--verify")
    text = (target / "lab" / "coupon.py").read_text()
    assert "class Specimen(sk.Sample):" in text and "class Coupon(Specimen):" in text
    assert text.count("super().__init__(path, name=name)") == 2
    assert 'class = "Coupon"' in (target / ".samplekitrc").read_text()
    # What the base declares computes in the subclass's samples, as 0.x did.
    assert "12 values agree with 0.x's, 0 differ or failed" in done.stdout, done.stdout
    assert done.returncode == 0


def test_two_classes_of_samples_need_class(lab, tmp_path):
    module = lab / "lab" / "coupon.py"
    module.write_text(module.read_text() + "\n\nclass Bar(Specimen):\n    pass\n")
    refused = run(lab, "--to", tmp_path / "x", "--model", module)
    assert refused.returncode == 2
    assert "several classes of samples, Coupon, Bar" in refused.stderr and "--class" in refused.stderr
    done = run(lab, "--to", tmp_path / "y", "--model", module, "--class", "Coupon", "--write")
    assert done.returncode == 0, done.stderr
    assert "the classes Bar are rewritten too, and the project takes Coupon" in done.stdout


def test_lambdas_and_functions_of_the_module_are_called_and_read(lab, tmp_path):
    target, _ = migrated_lab(lab, tmp_path)
    text = (target / "lab" / "coupon.py").read_text()
    # A lambda is called in parentheses, never its body.
    assert "(lambda loads: [0.01 * l * G for l in loads])(columns[\"load\"].values)" in text
    # A row formula of the module keeps the cells 0.x named.
    assert 'depends_on=["row.load", "row.d1", "row.d2"],' in text
    # A lambda reading an attribute: `.value` gone, the attribute named.
    assert "REFERENCE[self.material]," in text
    assert 'depends_on=["hardness", "material"],' in text
    assert "# TODO(migrate): a formula copied as SampleKit 0.x wrote it; check it" in text
    # The import of 0.x kept for the code copied, and sk beside it.
    assert "import samplekit\nimport samplekit as sk\n" in text


def test_verify_needs_write(old, tmp_path):
    refused = run(old, "--to", tmp_path / "new", "--model", old / "specimen.py", "--verify")
    assert refused.returncode == 2 and "--verify" in refused.stderr


@pytest.fixture
def rc1(tmp_path):
    source = tmp_path / "rc1"
    shutil.copytree(DATA / "rc.1", source)
    return source


def test_an_rc1_project_is_copied_and_its_configuration_renamed(rc1, tmp_path):
    target = tmp_path / "new"
    done = run(rc1, "--to", target, "--write")
    assert done.returncode == 0, done.stderr
    assert "Read as SampleKit 1.0.0-rc.1" in done.stdout
    for sample in (rc1 / "samples").glob("*.md"):
        assert (target / "samples" / sample.name).read_bytes() == sample.read_bytes()
    text = (target / ".samplekitrc").read_text()
    configuration = tomllib.loads(text)
    assert configuration["style"]["paper"] == {"property": r"{value} \pm {uncertainty} {unit}"}
    assert configuration["colors"] == {"outdated": "yellow"}
    assert "workbench" not in configuration
    assert configuration["collection"]["files"] == ["photos/{project}-*"]
    assert configuration["model"]["python"] == ".venv"
    with_environment(target)
    status = command(target, "status")
    assert "every computed value in 3 samples is current" in status.stdout, status.stdout + status.stderr


def test_an_rc1_model_is_renamed_in_its_init(rc1, tmp_path):
    target = tmp_path / "new"
    run(rc1, "--to", target, "--write")
    before = (rc1 / "model" / "specimen.py").read_text()
    text = (target / "model" / "specimen.py").read_text()
    assert "compute_quantity" not in text and "sk.Column" not in text and "compute_rows" not in text
    assert "compute=self._area," in text
    assert "compute_value=self._tensile_strength," in text
    assert '"stress": sk.Property(unit="MPa", compute=self._stress, depends_on=["row.force", "area"]),' in text
    assert 'index="strain",' in text
    # Its comments stay.
    for line in before.splitlines():
        if line.strip().startswith("#"):
            assert line in text, line
    module = importlib.util.spec_from_file_location("rc1_specimen", target / "model" / "specimen.py")
    loaded = importlib.util.module_from_spec(module)
    module.loader.exec_module(loaded)
    assert loaded.Specimen().area.is_computed


def test_each_project_of_an_rc1_tree_is_renamed(tmp_path):
    source = tmp_path / "rc1"
    shutil.copytree(DATA / "rc.1", source / "tensile")
    (source / ".samplekitrc").write_text('schema_version = 1\n\n[workbench.colors]\nstale = "red"\n')
    target = tmp_path / "new"
    target.mkdir()
    done = run(source, "--to", target, "--write")
    assert done.returncode == 0, done.stderr
    assert tomllib.loads((target / ".samplekitrc").read_text())["colors"] == {"outdated": "red"}
    inner = tomllib.loads((target / "tensile" / ".samplekitrc").read_text())
    assert inner["colors"] == {"outdated": "yellow"} and inner["model"]["python"] == ".venv"
    assert "sk.Column" not in (target / "tensile" / "model" / "specimen.py").read_text()


def test_the_report_says_what_to_do_next(old, rc1, tmp_path):
    target, _ = migrated(old, tmp_path)
    report = (target / "MIGRATION.md").read_text()
    assert "Read as SampleKit 0.x (0.1.0 to 0.2.0)." in report
    assert "## Not carried: computed again" in report and "area" in report
    assert "## To check by hand" in report
    assert "`python -m venv .venv`" in report
    assert "`samplekit . compute --write`" in report
    # A target named as a command, `new`, is still read as a folder.
    assert "$ samplekit new/ validate" in report
    assert "3 samples · " in report and "new sample" not in report
    other = tmp_path / "from-rc1"
    run(rc1, "--to", other, "--write")
    assert "--rerun --write" in (other / "MIGRATION.md").read_text()


def test_a_python_without_this_samplekit_is_refused(old, tmp_path, monkeypatch, capsys):
    sys.path.insert(0, str(TOOL))
    try:
        from samplekit_migrate import cli

        monkeypatch.setattr(cli, "installed_samplekit", lambda: "0.2.0")
        code = cli.main([str(old), "--to", str(tmp_path / "new"), "--model", str(old / "specimen.py")])
    finally:
        sys.path.remove(str(TOOL))
    said = capsys.readouterr()
    assert code == 2
    assert "has samplekit 0.2.0" in said.err
    assert "pip install samplekit==" in said.err
    assert "Read as" not in said.out


def test_the_archive_runs_on_its_own(tmp_path):
    archive = tmp_path / "samplekit-migrate.pyz"
    built = subprocess.run(
        [sys.executable, TOOL / "build.py", archive], capture_output=True, text=True, check=False
    )
    assert built.returncode == 0, built.stderr
    names = zipfile.ZipFile(archive).namelist()
    assert "yaml/__init__.py" in names and "yaml/LICENSE" in names and "README.md" in names
    assert not any(name.endswith((".so", ".pyd")) for name in names)
    helped = subprocess.run(
        [sys.executable, "-I", archive, "--help"], capture_output=True, text=True, check=False
    )
    assert helped.returncode == 0, helped.stderr
    assert "--keep-computed" in helped.stdout


def test_verify_compares_with_what_the_old_version_computed(old, tmp_path):
    target, done = migrated(old, tmp_path, "--verify")
    assert "Each value the migrated model computes, beside the one 0.x wrote:" in done.stdout
    assert "16 values agree with 0.x's, 1 differ or failed" in done.stdout
    # copper's thickness, its uncertainty typed over its readings, gives
    # another area's uncertainty: listed, never corrected.
    assert "samples/copper-01.md  area: 0.x 24.95247 ± 0.05478237399" in done.stdout
    assert done.returncode == 1


def test_the_tool_writes_for_this_version():
    sys.path.insert(0, str(TOOL))
    try:
        import samplekit_migrate
    finally:
        sys.path.remove(str(TOOL))
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    assert samplekit_migrate.FOR_VERSION == cargo["package"]["version"] == sk.__version__


def test_clean_state_removes_what_earlier_versions_left_and_nothing_else(tmp_path):
    state = tmp_path / "state"
    (state / "logs").mkdir(parents=True)
    (state / "accepted").mkdir()
    for name in ("computed.json", "consent", "machine", "logs/compute-2026-09-12T10-04-05Z-4242.log",
                 "logs/compute-tensile-20261006.log"):
        (state / name).write_text("kept")
    environment = dict(os.environ, PYTHONPATH=str(TOOL), SAMPLEKIT_STATE_DIR=str(state))
    listed = subprocess.run([sys.executable, "-m", "samplekit_migrate", "--clean-state"],
                            capture_output=True, text=True, env=environment, check=False)
    assert listed.returncode == 0, listed.stderr
    assert "computed.json" in listed.stdout and "accepted/" in listed.stdout
    assert "Nothing removed: --write removes them." in listed.stdout
    assert (state / "computed.json").exists()
    removed = subprocess.run([sys.executable, "-m", "samplekit_migrate", "--clean-state", "--write"],
                             capture_output=True, text=True, env=environment, check=False)
    assert removed.returncode == 0, removed.stderr
    left = sorted(p.relative_to(state).as_posix() for p in state.rglob("*"))
    assert left == ["logs", "logs/compute-tensile-20261006.log", "machine"]
