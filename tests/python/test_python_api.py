"""The Python API."""

import collections.abc
import re
import datetime
import json
import os
import time
import pathlib
import subprocess
import sys
import warnings

import numpy
import pytest
import warnings

import samplekit as sk
from conftest import BINARY, MODEL, sample_text


def quoted(path):
    """A path as a history's message writes it: quoted where a shell would
    read one of its characters, as Windows' backslashes."""
    import shlex

    return shlex.quote(str(path))


def names(samples):
    return [sample.name for sample in samples]


def soak(**derivations):
    """A temperature-indexed table whose rows are not in index order."""
    return sk.Table(
        {"T": sk.Property(unit="degC"), "hrc": sk.Property(unit="-"), "R": sk.Property(unit="lintner")},
        index="T",
        title="Soak",
        rows=[
            {"T": 30, "hrc": sk.Property(0.3, uncertainty=0.03), "R": 3.0},
            {"T": 10, "hrc": sk.Property(0.1, uncertainty=0.01), "R": 1.0},
            {"T": 20, "hrc": sk.Property(0.2, uncertainty=0.02), "R": 2.0},
        ],
        **derivations,
    )


def quantity_sample(project):
    project.config('schema_version = 1\n\n[property.diameter]\nprecision = ".3f"\n')
    return sk.Sample(
        project.sample(
            "q.md",
            name="ph",
            properties={"diameter": "{v: 3.1551234, u: 0.00123, unit: g}"},
        )
    )


# ------------------------------------------------------------ ordering


def test_sort_mutates_and_returns_none(collection):
    samples = sk.SampleList(collection.root)
    assert samples.sort("diameter") is None
    assert names(samples) == ["C", "A", "B"]


def test_sort_actually_reorders(collection):
    samples = sk.SampleList(collection.root)
    before = names(samples)
    samples.sort("-diameter")
    assert names(samples) == ["B", "A", "C"]
    assert names(samples) != before


def test_sorted_returns_a_new_collection(collection):
    samples = sk.SampleList(collection.root)
    ordered = samples.sorted("-diameter")
    assert ordered is not samples
    assert names(ordered) == ["B", "A", "C"]
    assert names(samples) == ["A", "B", "C"]


def test_unknown_sort_key_raises_key_error(collection):
    samples = sk.SampleList(collection.root)
    with pytest.raises(KeyError):
        samples.sort("dutility")
    assert names(samples) == ["A", "B", "C"]


# -------------------------------------------------------------- tables


def test_at_addresses_by_index_value():
    table = soak()
    assert table.at(20, "hrc").value == 0.2
    assert table[0]["T"].value == 30


def test_table_metadata_is_attribute_access():
    table = soak()
    assert table.column_names == ["T", "hrc", "R"]
    assert table.index_values == [30, 10, 20]
    assert table.data_columns == ["hrc", "R"]
    assert table.index == "T"
    assert table.index_unit == "degC"
    assert table.title == "Soak"


def test_property_carries_value_and_uncertainty():
    table = soak()
    cell = table.at(20, "hrc")
    assert (cell.value, cell.uncertainty) == (0.2, 0.02)
    assert table.values("hrc") == [0.3, 0.1, 0.2]
    assert table.uncertainties("hrc") == [0.03, 0.01, 0.02]


# ----------------------------------------------------------- properties


def test_v_and_u_are_value_and_uncertainty():
    prop = sk.Property(1.5, uncertainty=0.1)
    assert prop.v == prop.value
    assert prop.u == prop.uncertainty
    prop.v = 2.0
    prop.u = 0.2
    assert (prop.value, prop.uncertainty) == (2.0, 0.2)


def test_float_and_int_forward_to_the_value():
    prop = sk.Property(3.5)
    assert float(prop) == 3.5
    assert int(prop) == 3
    assert bool(prop) is True


def test_str_renders_the_quantity_as_a_terminal_does(project):
    # The uncertainty rounded to the nearest, as the value is.
    assert str(quantity_sample(project).diameter) == "3.155 ± 0.002 g"


def test_an_empty_format_specifier_equals_str(project):
    diameter = quantity_sample(project).diameter
    assert f"{diameter}" == str(diameter)
    assert format(diameter, "") == str(diameter)


def test_a_format_specifier_renders_like_a_command_line_column(project, cli):
    sample = quantity_sample(project)
    shown = f"{sample.diameter:.2f}"
    # An uncertainty below the last place is rounded up to it, never written as
    # zero nor as a bound.
    assert shown.startswith("3.16 ± 0.01") and "<" not in shown
    assert shown.removesuffix(" g") in cli(sample.path, "-c", "diameter:.2f", "--width", "120")


def test_an_unsupported_format_specifier_raises_value_error():
    with pytest.raises(ValueError) as caught:
        f"{sk.Property(1.0):>10}"
    assert ">10" in str(caught.value)


def test_repr_shows_the_stored_values_unrounded():
    prop = sk.Property(3.1551234, uncertainty=0.00123, unit="g")
    assert repr(prop) == "Property(value=3.1551234, uncertainty=0.00123, unit='g')"
    assert repr(sk.Property()) == "Property(value=None)"


def test_an_absent_value_prints_as_a_dash():
    assert str(sk.Property()) == "—"
    assert f"{sk.Property():.2f}" == "—"


def test_a_text_property_prints_verbatim():
    prop = sk.Property("Inconel 350")
    assert str(prop) == "Inconel 350"
    assert f"{prop:.2f}" == "Inconel 350"


def test_arithmetic_uses_the_value_and_returns_a_bare_number():
    hv = sk.Property(9.5, uncertainty=0.1)
    hrc = sk.Property(0.001)
    result = hv * (1 - 1j * hrc)
    assert type(result) is complex
    assert result == 9.5 * (1 - 1j * 0.001)
    assert hv + 1 == 10.5
    assert 2 ** sk.Property(3) == 8
    assert -hv == -9.5
    assert abs(sk.Property(-2.0)) == 2.0
    assert round(sk.Property(2.567), 1) == 2.6
    assert type(hv / 2) is float


def test_arithmetic_on_an_absent_value_names_the_property():
    sample = sk.Sample()
    sample.energy = sk.Property()
    with pytest.raises(TypeError) as caught:
        sample.energy * 2
    assert "'energy'" in str(caught.value)
    assert "NoneType" not in str(caught.value)


def test_an_absent_property_equals_no_value():
    first, second = sk.Property(), sk.Property()
    assert (first == second) is False
    assert (first != second) is True
    assert (first == None) is False  # noqa: E711


# ------------------------------------------------------- sample access


def test_unknown_property_raises_key_error_with_a_suggestion(collection):
    sample = sk.Sample(collection.root / "A.md")
    with pytest.raises(KeyError) as caught:
        sample["diametr"]
    assert "did you mean: diameter?" in str(caught.value)


def test_unknown_attribute_raises_attribute_error_with_a_suggestion(collection):
    sample = sk.Sample(collection.root / "A.md")
    with pytest.raises(AttributeError) as caught:
        sample.diametr
    assert "did you mean: diameter?" in str(caught.value)
    assert hasattr(sample, "diametr") is False
    assert getattr(sample, "diametr", 7) == 7


def test_a_property_without_a_value_is_returned(project):
    sample = sk.Sample(project.sample("e.md", properties={"energy": "{unit: J}"}))
    assert sample.energy.value is None
    assert sample.energy.unit == "J"


def test_membership_tests_a_name():
    sample = sk.Sample()
    sample.diameter = sk.Property(1.0)
    sample.preheating = soak()
    assert "diameter" in sample
    assert "preheating" in sample
    assert "nothing" not in sample


def test_deleting_by_attribute_or_item_removes_the_property(project):
    sample = sk.Sample(
        project.sample(
            "d.md",
            properties={"diameter": "1.0", "ductility": "2.0", "volume": "3.0"},
        )
    )
    del sample.diameter
    del sample["ductility"]
    sample.save()
    text = sample.path.read_text()
    assert "diameter" not in text
    assert "ductility" not in text
    assert "volume" in text


def test_iterating_a_sample_yields_names_in_declaration_order(project):
    sample = sk.Sample(
        project.sample(
            "o.md",
            attributes={"batch": 7, "machinist": "jo"},
            properties={"volume": "3.0", "diameter": "1.0"},
        )
    )
    assert list(sample) == ["batch", "machinist", "volume", "diameter"]
    assert list(sample.keys()) == list(sample)
    assert [name for name, _ in sample.items()] == list(sample)


# ----------------------------------------------------------- assignment


def test_assigning_a_property_or_a_table_registers_it(tmp_path):
    sample = sk.Sample()
    sample.diameter = sk.Property(1.5, unit="g")
    sample.preheating = soak()
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "properties:\n  diameter:" in text
    assert "tables:\n  preheating:" in text


def test_assigning_a_plain_value_creates_an_attribute(tmp_path):
    sample = sk.Sample()
    sample.batch = 7
    path = tmp_path / "s.md"
    sample.save(path)
    assert "\nbatch: 7\n" in path.read_text()
    assert sk.Sample(path).batch == 7


def test_an_underscore_attribute_is_private_and_never_saved(tmp_path):
    sample = sk.Sample(name="P")
    sample._cache = {"a": 1}
    assert sample._cache == {"a": 1}
    assert "_cache" not in sample
    assert "_cache" not in list(sample.keys())
    path = tmp_path / "s.md"
    sample.save(path)
    assert "_cache" not in path.read_text()
    assert "_cache" not in sk.SampleList([sample]).to_dict(columns=["name"])


def test_assigning_an_unstorable_object_suggests_an_underscore():
    sample = sk.Sample()
    for unstorable in (len, (1, 2), {"a": 1}):
        with pytest.raises(TypeError) as caught:
            sample.thing = unstorable
        assert "s._thing" in str(caught.value)
    assert "thing" not in sample


def test_assigning_a_different_kind_to_a_name_is_refused():
    sample = sk.Sample()
    sample.diameter = sk.Property(3.1)
    with pytest.raises(TypeError) as caught:
        sample.diameter = 3.2
    assert "s.diameter.value" in str(caught.value)
    assert sample.diameter.value == 3.1


def test_an_attribute_without_a_value_is_kept_and_not_written(tmp_path):
    sample = sk.Sample(name="E")
    sample.operator = None
    sample.temperatures = []
    assert sample.operator is None
    assert sample.temperatures == []
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "machinist" not in text
    assert "temperatures" not in text


# ---------------------------------------------------------------- lists


def test_a_list_attribute_is_a_list():
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    assert isinstance(sample.temperatures, list)
    assert json.dumps(sample.temperatures) == "[20, 30]"
    assert sample.temperatures == [20, 30]
    assert numpy.array(sample.temperatures).sum() == 50


def test_mutating_a_list_attribute_reaches_the_sample(tmp_path):
    sample = sk.Sample()
    sample.temperatures = [30, 20]
    temperatures = sample.temperatures
    temperatures.append(40)
    temperatures += [50]
    temperatures[0:1] = [10]
    temperatures.sort()
    path = tmp_path / "s.md"
    sample.save(path)
    assert "temperatures: [10, 20, 40, 50]" in path.read_text()


def test_a_list_item_of_another_kind_is_refused():
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    with pytest.raises(ValueError) as caught:
        sample.temperatures.append("hot")
    assert "temperatures" in str(caught.value)
    assert sample.temperatures == [20, 30]


def test_a_nested_list_is_refused():
    sample = sk.Sample()
    with pytest.raises(TypeError):
        sample.grid = [[1, 2]]
    sample.temperatures = [20]
    with pytest.raises(TypeError):
        sample.temperatures.append([1])
    assert sample.temperatures == [20]


def test_a_tuple_is_refused_suggesting_a_list():
    sample = sk.Sample()
    with pytest.raises(TypeError) as caught:
        sample.pair = (1, 2)
    assert "list" in str(caught.value)


def test_reassigning_a_list_attribute_detaches_the_old_list():
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    old = sample.temperatures
    sample.temperatures = [1]
    old.append(99)
    assert sample.temperatures == [1]
    assert old == [20, 30, 99]


def test_a_copy_of_a_list_attribute_is_detached():
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    copied = list(sample.temperatures)
    copied.append(1)
    duplicate = sample.temperatures.copy()
    duplicate.append(2)
    assert type(duplicate) is list
    assert sample.temperatures == [20, 30]


# ---------------------------------------------------------- dates, kinds


def test_a_datetime_with_a_timezone_is_refused():
    sample = sk.Sample()
    with pytest.raises(ValueError) as caught:
        sample.moment = datetime.datetime(2026, 9, 14, 10, 30, tzinfo=datetime.timezone.utc)
    assert "timezone" in str(caught.value)


def test_a_naive_datetime_round_trips_with_its_precision(tmp_path):
    sample = sk.Sample()
    sample.minute = datetime.datetime(2026, 9, 14, 10, 30)
    sample.second = datetime.datetime(2026, 9, 14, 10, 30, 15)
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "minute: 2026-09-14T10:30\n" in text
    assert "second: 2026-09-14T10:30:15\n" in text
    again = sk.Sample(path)
    assert again.minute == datetime.datetime(2026, 9, 14, 10, 30)
    assert again.second == datetime.datetime(2026, 9, 14, 10, 30, 15)


def test_path_and_filename_are_read_only(collection):
    sample = sk.Sample(collection.root / "A.md")
    for name in ("path", "filename"):
        with pytest.raises(AttributeError) as caught:
            setattr(sample, name, "elsewhere.md")
        assert "save(path)" in str(caught.value)


def test_a_method_name_is_reachable_only_by_brackets():
    sample = sk.Sample()
    with pytest.raises(AttributeError):
        sample.save = 3
    sample["save"] = 3
    assert sample["save"] == 3
    assert callable(sample.save)


def test_a_boolean_is_not_stored_as_an_integer(tmp_path):
    sample = sk.Sample()
    sample.cold_crashed = True
    path = tmp_path / "s.md"
    sample.save(path)
    assert "cold_crashed: true" in path.read_text()


def test_name_note_and_tags_use_their_descriptors(tmp_path):
    sample = sk.Sample()
    sample.name = "C43"
    sample.note = "  verbatim\n\ttext  \n"
    sample.tags = ["reference"]
    sample.tags.append("reference")
    sample.tags.append("billet")
    with pytest.raises(ValueError):
        sample.tags.append("not a tag")
    assert sample.tags == ["reference", "billet"]
    path = tmp_path / "s.md"
    sample.save(path)
    again = sk.Sample(path)
    assert again.name == "C43"
    assert again.note == "  verbatim\n\ttext  \n"
    assert again.tags == ["reference", "billet"]


def test_brackets_never_reach_private_state():
    sample = sk.Sample()
    with pytest.raises(ValueError):
        sample["_x"] = 5
    sample._x = 5
    with pytest.raises(KeyError):
        sample["_x"]


# ------------------------------------------------------------- filtering


def test_filter_chains(collection):
    samples = sk.SampleList(collection.root)
    chained = samples.filter("period == 6").filter(lambda sample: sample.alloy == "Inconel")
    assert names(chained) == ["A"]


def test_filter_shares_samples_with_the_parent(collection):
    samples = sk.SampleList(collection.root)
    subset = samples.filter("alloy == Inconel")
    subset[0].diameter.value = 99.0
    assert samples["A"].diameter.value == 99.0


# ---------------------------------------------------------------- loading


def test_constructing_with_a_path_loads_the_file(collection):
    sample = sk.Sample(collection.root / "A.md")
    assert sample.diameter.value == 12.5
    assert sample.name == "A"


def test_the_file_is_loaded_after_the_class_init(project):
    project.model()
    sample_path = project.sample("s.md", properties={"diameter": "{v: 12.5, unit: g}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(MODEL, namespace)
    sample = namespace["Model"](sample_path)
    assert sample.diameter.value == 12.5
    assert sample.diameter.unit == "g"
    sample.compute()
    assert sample.double.value == 25.0


def test_a_missing_file_raises_file_not_found_with_the_nearest(collection):
    with pytest.raises(FileNotFoundError) as caught:
        sk.Sample(collection.root / "B2.md")
    assert "B.md" in str(caught.value)


def test_a_sample_without_a_path_is_new(project):
    namespace = {"__file__": str(project.root / "model.py")}
    exec(MODEL, namespace)
    sample = namespace["Model"](name="C43")
    assert sample.path is None
    assert sample.name == "C43"
    assert sample.diameter.value is None


def test_the_project_model_applies_without_consent(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("s.md", properties={"diameter": "2.0"}))
    assert type(sample).__name__ == "Model"
    assert isinstance(sample, sk.Sample)
    assert project.imported()


def test_explain_on_a_fresh_copy_says_what_the_model_reads(project, cli):
    # With no description written yet — a fresh copy, a snapshot read with
    # --at — explain borrowed what a value reads from another sample's file.
    project.model()
    project.config(
        'schema_version = 1\n\n[model]\npath = "model.py"\n'
        f"python = {json.dumps(sys.prefix)}\n"
    )
    sample = project.sample("s.md", name="s", properties={"diameter": "2.0"})
    said = cli(sample, "explain", "double")
    assert "the model computes it from diameter" in said, said
    assert "file records" not in said, said


def test_a_file_written_for_a_command_is_said_in_its_journal(project, tmp_path):
    # The model's worker writes what its command computes: those files went
    # unseen by that command's snapshot and by every other command meanwhile.
    project.config("schema_version = 1\n")
    sample = project.sample("s.md", name="s", properties={"diameter": "2.0"})
    journal = tmp_path / "writing-1.lock"
    journal.write_text("1\n")
    script = (
        "import samplekit as sk\n"
        f"s = sk.Sample({str(sample)!r})\n"
        "s.diameter.value = 3.0\n"
        "s.save()\n"
    )
    env = dict(os.environ, SAMPLEKIT_JOURNALS=str(journal), SAMPLEKIT_HISTORY="off")
    subprocess.run([sys.executable, "-c", script], check=True, env=env)
    assert f"wrote {sample.resolve()}" in journal.read_text().splitlines()


TABLED = """\
import samplekit as sk


class Model(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property()
        self.double = sk.Property(compute_value=lambda: self.diameter.value * 2)
        self.set_dependencies("double", depends_on=["diameter"])
        self.t = sk.Table(
            {
                "d": sk.Property(),
                "g": sk.Property(),
                "a": sk.Property(compute=self._a, depends_on=["row.g"]),
            },
            index="d",
        )

    def _a(self, row):
        return {"a": row.g.value * 10}
"""

TABLE_ROWS = (
    "tables:\n  t:\n    index: d\n    columns:\n      d: {}\n      g: {}\n      a: {}\n"
    "    rows:\n      - {d: 1, g: 1.0}\n      - {d: 2, g: 2.0}\n"
)


def _tabled(project):
    project.model(TABLED)
    project.config(
        'schema_version = 1\n\n[model]\npath = "model.py"\n'
        f"python = {json.dumps(sys.prefix)}\n"
    )


def test_a_cell_emptied_is_waited_for_on_every_surface(project, cli):
    # `status` said outdated — row.g, `compute` waits for t.g[2], and a table's
    # state column outdated: one state, said three ways.
    _tabled(project)
    path = project.root / "s.md"
    path.write_text(sample_text(name="s", properties={"diameter": "2.0"}) .replace("---\n# Notes", TABLE_ROWS + "---\n# Notes"))
    cli(path, "compute", "--write")
    cli(path, "set", "t.g[2]=", "--write")
    said = cli(path, "status")
    assert "waits for t.g[2]" in said, said
    assert "outdated" not in said, said
    assert "waits for t.g[2]" in cli(path, "explain", "t.a[2]")
    assert "waiting" in cli(path, "-c", "name,t.a[2]", "--status")
    assert sk.Sample(path).t.at(2, "a").state == "waiting"


def test_a_value_typed_where_its_formula_never_ran_is_an_override(project, cli):
    # Typed over a formula never run, it was written as a value entered: the
    # workbench said *record missing*, `status` *waits for diameter*, `explain`
    # *entered or measured*.
    _tabled(project)
    path = project.sample("s.md", name="s")
    assert "typed over its formula" in cli(path, "set", "double=5")
    cli(path, "set", "double=5", "--write")
    listed = json.loads(cli(path, "status", "--json"))
    assert [(entry["value"], entry["state"]) for entry in listed] == [("double", "edited")]
    assert sk.Sample(path).double.state == "edited"
    assert "by hand" in cli(path, "explain", "double")


def test_a_formula_returning_none_fails(project):
    # `None` was written as no value, and said never computed after each run.
    project.model(TABLED.replace("self.diameter.value * 2", "None"))
    project.config(
        'schema_version = 1\n\n[model]\npath = "model.py"\n'
        f"python = {json.dumps(sys.prefix)}\n"
    )
    path = project.sample("s.md", name="s", properties={"diameter": "2.0"})
    ran = subprocess.run(
        [str(BINARY), str(path), "compute", "--write"], capture_output=True, text=True
    )
    assert ran.returncode != 0, ran.stdout
    said = ran.stdout + ran.stderr
    assert "returned None" in said and "sk.NA" in said, said


def test_a_column_the_model_computes_is_said_so_before_it_ran(project, cli):
    # A column no formula has filled yet was said measured.
    _tabled(project)
    path = project.root / "s.md"
    path.write_text(sample_text(name="s").replace("---\n# Notes", TABLE_ROWS + "---\n# Notes"))
    said = cli(path, "explain", "t")
    assert "computed by the model from row.g, never computed yet" in said, said


def _exported(project):
    _tabled(project)
    with open(project.root / ".samplekitrc", "a") as rc:
        rc.write(
            '\n[profile.p]\ncolumns = [{field = "name"}, {field = "double"}]\n'
            '[export.e]\nprofile = "p"\nformat = "csv"\noutput = "out/e.csv"\n'
        )
    return project.sample("s.md", name="s", properties={"diameter": "2.0"})


def _export(project, *arguments):
    return subprocess.run(
        [str(BINARY), str(project.root), "export", "e", *arguments],
        capture_output=True,
        text=True,
    )


def test_an_export_of_values_never_computed_exits_2_unless_accepted(project):
    # Written empty, and said nowhere: the export exited 0.
    _exported(project)
    previewed = _export(project)
    said = previewed.stdout + previewed.stderr
    assert "1 never computed, written empty" in said, said
    written = _export(project, "--write")
    assert written.returncode == 2, written.stderr
    assert "--accept never_computed" in written.stderr, written.stderr
    assert _export(project, "--write", "--accept", "never-computed").returncode == 0
    refused = _export(project, "--write", "--accept", "stale")
    assert refused.returncode == 1, refused.stderr


def test_an_export_after_its_formula_changed_is_outdated(project, cli):
    # The formula changed since the values were computed: `status` said so,
    # and the export wrote them and exited 0.
    path = _exported(project)
    cli(path, "compute", "--write")
    assert _export(project, "--write").returncode == 0
    project.model(TABLED.replace("self.diameter.value * 2", "self.diameter.value * 3"))
    previewed = _export(project)
    said = previewed.stdout + previewed.stderr
    assert "1 outdated" in said, said
    written = _export(project, "--write")
    assert written.returncode == 2, written.stderr
    assert "--accept outdated" in written.stderr, written.stderr
    assert _export(project, "--write", "--accept", "outdated").returncode == 0


def test_model_false_loads_the_data_alone(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("s.md", properties={"diameter": "2.0"}), model=False)
    assert type(sample) is sk.Sample
    assert "double" not in sample
    assert not project.imported()


class Override(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.flagged = True


def test_an_explicit_model_overrides_the_project_model(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("s.md", properties={"diameter": "2.0"}), model=Override)
    assert type(sample) is Override
    assert not project.imported()


def test_a_sample_list_applies_the_model_to_every_sample(collection):
    samples = sk.SampleList(collection.root, model=Override)
    assert len(samples) == 3
    assert all(type(sample) is Override for sample in samples)


def test_removed_loaders_name_the_constructor():
    for name in ("load", "load_with_model", "load_data"):
        with pytest.raises(AttributeError) as caught:
            getattr(sk.Sample, name)
        assert "Sample(path)" in str(caught.value)


# ------------------------------------------------------------------ export


def test_to_csv_is_byte_identical_to_the_command_line(collection, cli):
    samples = sk.SampleList(collection.root)
    root = collection.root
    assert samples.to_csv(profile="ductilities") == cli(root, "--profile", "ductilities", "--csv")
    assert samples.to_tsv(profile="ductilities") == cli(root, "--profile", "ductilities", "--tsv")
    assert samples.to_json(profile="ductilities") == cli(root, "--profile", "ductilities", "--json")
    assert samples.to_csv(columns=["name", "diameter:.2f"]) == cli(root, "-c", "name,diameter:.2f", "--csv")


def test_to_dict_keys_are_the_export_columns_without_their_unit(collection):
    samples = sk.SampleList(collection.root)
    header = samples.to_csv(profile="ductilities").splitlines()[0].split(",")
    assert list(samples.to_dict(profile="ductilities")) == [name.split(" [")[0] for name in header]


def test_to_dict_keeps_full_precision(collection):
    samples = sk.SampleList(collection.root)
    assert samples.to_dict(columns=["diameter:.3f"])["diameter_value"] == [12.5, 13.1234, 11.0]


def test_to_csv_without_a_path_returns_the_text(collection):
    before = sorted(path.name for path in collection.root.iterdir())
    text = sk.SampleList(collection.root).to_csv(columns=["name"])
    assert text.startswith("name\n")
    assert sorted(path.name for path in collection.root.iterdir()) == before


def test_to_csv_refuses_an_existing_file_without_overwrite(collection, tmp_path):
    samples = sk.SampleList(collection.root)
    out = tmp_path / "out.csv"
    out.write_text("kept")
    with pytest.raises(FileExistsError):
        samples.to_csv(out, columns=["name"])
    assert out.read_text() == "kept"
    assert samples.to_csv(out, columns=["name"], overwrite=True) is None
    assert out.read_text().startswith("name\n")


def test_a_whole_list_is_json_and_joined_by_delimited_exports(project):
    project.sample("a.md", name="a", attributes={"temperatures": "[20, 30]"})
    project.sample("b.md", name="b", attributes={"temperatures": "[40]"})
    samples = sk.SampleList(project.root)
    assert samples.to_dict(columns=["temperatures"])["temperatures"] == [[20, 30], [40]]
    rows = json.loads(samples.to_json(columns=["temperatures"]))
    assert [row["temperatures"] for row in rows] == [[20, 30], [40]]
    # Joined by `;`, as tags are; an item alone, as before.
    assert samples.to_csv(columns=["temperatures"]) == "temperatures\n20;30\n40\n"
    assert samples.to_csv(columns=["temperatures[#0]"]) == "temperatures[#0]\n20\n40\n"


def test_an_export_without_columns_or_profile_lists_the_profiles(collection):
    with pytest.raises(ValueError) as caught:
        sk.SampleList(collection.root).to_csv()
    assert "ductilities" in str(caught.value)


def test_export_regenerates_a_declared_export(collection, cli):
    written = sk.SampleList(collection.root).export("diameters")
    assert written == collection.root / "diameters.csv"
    by_command = collection.root / "by-command.csv"
    # The command line previews and writes on --write; the Python `export` is
    # the call itself, so it writes.
    cli(collection.root, "export", "diameters", "-o", by_command, "--write")
    assert written.read_bytes() == by_command.read_bytes()


def test_a_list_over_two_projects_writes_each_row_at_its_own_precision(tmp_path):
    # Python wrote raw numbers over two projects, where the command line
    # writes each row as its own project declares.
    for project, precision, diameter in (("one", ".1f", 1.23456), ("two", ".3f", 2.34567)):
        (tmp_path / project).mkdir()
        (tmp_path / project / ".samplekitrc").write_text(
            f'schema_version = 1\n[property.diameter]\nprecision = "{precision}"\n'
        )
        (tmp_path / project / "s.md").write_text(
            f"---\nschema_version: 1\nname: {project}\nproperties:\n  diameter: {diameter}\n---\n"
        )
    both = sk.load(tmp_path / "two") + sk.load(tmp_path / "one")
    text = both.to_csv(columns=["name", "diameter"])
    # No uncertainty column: no sample fills one.
    assert "one,1.2\n" in text, text
    assert "two,2.346\n" in text, text


def test_a_figure_over_two_projects_draws_both(tmp_path):
    # Python refused a list spanning two projects; it draws them all.
    for project, diameter in (("one", 1.0), ("two", 2.0)):
        (tmp_path / project).mkdir()
        (tmp_path / project / ".samplekitrc").write_text("schema_version = 1\n")
        for n in (1, 2):
            (tmp_path / project / f"s{n}.md").write_text(
                f"---\nschema_version: 1\nname: {project}{n}\nproperties:\n"
                f"  diameter: {{v: {diameter * n}, unit: g}}\n  volume: {{v: {n}.0, unit: L}}\n---\n"
            )
    both = sk.load(tmp_path / "one") + sk.load(tmp_path / "two")
    (drawn,) = sk.plot(both, x="volume", y="diameter", output=tmp_path / "f.png")
    # The second project's heaviest, 4 g, is on the figure.
    ys = [y for line in drawn.axes[0].lines for y in line.get_ydata()]
    assert max(ys) == 4.0 and min(ys) == 1.0, ys


def test_an_output_makes_its_folder(project, tmp_path):
    # Every output's folder is made, declared or given, as the command line
    # makes it.
    from conftest import DUCTILITIES

    project.config(DUCTILITIES + '\n[export.nested]\nprofile = "ductilities"\nformat = "csv"\noutput = "out/diameters.csv"\n')
    project.sample("A.md", name="A", properties={"diameter": "{v: 15.0, unit: g}"})
    samples = sk.SampleList(project.root)
    written = samples.export("nested")
    assert written == project.root / "out" / "diameters.csv"
    assert written.exists()
    elsewhere = samples.export("nested", output=project.root / "elsewhere" / "diameters.csv")
    assert elsewhere.read_text() == written.read_text()
    samples.to_csv(project.root / "csv" / "deeper" / "diameters.csv", columns=["diameter"])
    assert (project.root / "csv" / "deeper" / "diameters.csv").exists()
    figures = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    sk.plot(figures, "diameter_volume", output=tmp_path / "figs" / "fig.png")
    assert (tmp_path / "figs" / "fig.png").read_bytes().startswith(b"\x89PNG")


def test_export_applies_the_query_it_declares(project):
    # An export may carry its own selection, in Python as on the command line.
    from conftest import DUCTILITIES

    project.config(DUCTILITIES + '\n[export.heavy]\nprofile = "ductilities"\nformat = "csv"\noutput = "heavy.csv"\nquery = "heavy"\n')
    project.sample("A.md", name="A", properties={"diameter": "{v: 15.0, unit: g}"})
    project.sample("B.md", name="B", properties={"diameter": "{v: 5.0, unit: g}"})
    samples = sk.SampleList(project.root)
    assert samples.project.exports.heavy.query == "heavy"
    written = samples.export("heavy")
    rows = written.read_text().splitlines()
    assert len(rows) == 2, rows
    assert rows[1].startswith("A,"), rows
    everything = samples.export("diameters", output=project.root / "all.csv")
    assert len(everything.read_text().splitlines()) == 3


# ----------------------------------------------------------------- project


def test_project_entries_are_accepted_wherever_a_name_is(collection, tmp_path):
    samples = sk.SampleList(collection.root)
    project = samples.project
    assert samples.to_csv(profile=project.profiles.ductilities) == samples.to_csv(profile="ductilities")
    assert names(samples.query(project.queries.heavy)) == names(samples.query("heavy"))
    out = samples.export(project.exports.diameters, output=tmp_path / "entry.csv")
    assert out.read_text() == samples.to_csv(profile="ductilities")


def test_project_is_read_only(collection):
    config = (collection.root / ".samplekitrc").read_text()
    project = sk.SampleList(collection.root).project
    with pytest.raises(AttributeError) as caught:
        project.profiles = None
    assert ".samplekitrc" in str(caught.value)
    with pytest.raises(AttributeError):
        project.profiles.ductilities = None
    assert (collection.root / ".samplekitrc").read_text() == config


def test_project_names_complete_in_a_console(collection):
    profiles = sk.SampleList(collection.root).project.profiles
    assert "ductilities" in dir(profiles)
    assert profiles._ipython_key_completions_() == ["ductilities"]


def test_unknown_project_name_suggests_the_nearest(collection):
    profiles = sk.SampleList(collection.root).project.profiles
    with pytest.raises(AttributeError) as caught:
        profiles.ductilites
    assert "did you mean: ductilities?" in str(caught.value)
    with pytest.raises(KeyError) as caught:
        profiles["ductilites"]
    assert "did you mean: ductilities?" in str(caught.value)


def test_outside_a_project_a_lookup_says_no_configuration_was_found(project):
    sample = sk.Sample(project.sample("s.md", properties={"diameter": "1.0"}))
    with pytest.raises(AttributeError) as caught:
        sample.project.profiles.ductilities
    assert "no .samplekitrc" in str(caught.value)
    assert sample.project.root is None


def test_importing_samplekit_does_not_import_pandas():
    completed = subprocess.run(
        [sys.executable, "-c", "import sys, samplekit; print('pandas' in sys.modules)"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert completed.stdout.strip() == "False"


# ------------------------------------------------------------- composition


def test_sample_list_is_a_read_only_sequence(collection):
    samples = sk.SampleList(collection.root)
    assert isinstance(samples, collections.abc.Sequence)
    assert len(samples) == 3
    first = samples[0]
    assert samples[-1].name == "C"
    assert isinstance(samples[:2], sk.SampleList)
    assert names(samples[:2]) == ["A", "B"]
    assert samples["B"].name == "B"
    assert first in samples
    assert names(reversed(samples)) == ["C", "B", "A"]
    assert samples.index(first) == 0
    assert samples.count(first) == 1


def test_item_assignment_and_deletion_are_refused(collection):
    samples = sk.SampleList(collection.root)
    with pytest.raises(TypeError):
        samples[0] = sk.Sample()
    with pytest.raises(TypeError):
        del samples[0]
    assert len(samples) == 3


def test_adding_lists_keeps_order_and_shares_samples(collection):
    samples = sk.SampleList(collection.root)
    first, rest = samples[:1], samples[1:]
    joined = first + rest
    assert names(joined) == ["A", "B", "C"]
    joined[0].diameter.value = 1.0
    assert first[0].diameter.value == 1.0
    assert samples[0].diameter.value == 1.0


def test_in_place_add_rebinds_and_leaves_the_original(collection):
    samples = sk.SampleList(collection.root)
    growing = samples[:1]
    original = growing
    growing += samples[1:2]
    assert growing is not original
    assert len(original) == 1
    assert names(growing) == ["A", "B"]


def test_adding_lists_refuses_a_sample_present_in_both(collection):
    samples = sk.SampleList(collection.root)
    with pytest.raises(ValueError) as caught:
        samples + samples[:1]
    assert "A.md" in str(caught.value)


def test_a_list_built_from_samples_refuses_a_duplicate():
    sample = sk.Sample()
    with pytest.raises(ValueError):
        sk.SampleList([sample, sample])


def two_projects(tmp_path):
    from conftest import DUCTILITIES, Project

    lists = []
    for name in ("first", "second"):
        root = tmp_path / name
        root.mkdir()
        project = Project(root)
        project.config(DUCTILITIES)
        project.sample(f"{name}.md", name=name, properties={"diameter": "1.0"})
        lists.append(sk.SampleList(root))
    return lists


def test_a_list_spanning_two_projects_has_no_single_project(tmp_path):
    first, second = two_projects(tmp_path)
    with pytest.raises(ValueError) as caught:
        (first + second).project
    assert "first" in str(caught.value)
    assert "second" in str(caught.value)


def test_a_folder_above_projects_reads_each_sample_with_its_own(tmp_path):
    # Loaded from the folder above, each sample was given that folder's
    # configuration: its project was the root's, and its model none.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    two_projects(tmp_path)
    (tmp_path / "first" / "model.py").write_text(
        "import samplekit as sk\n\nclass Poured(sk.Sample):\n    pass\n"
    )
    rc = tmp_path / "first" / ".samplekitrc"
    # A literal string: a Windows path's backslashes are no escapes there.
    rc.write_text(rc.read_text() + f"[model]\npath = \"model.py\"\npython = '{sys.executable}'\n")
    loaded = {sample.name: sample for sample in sk.load(tmp_path)}
    assert loaded["first"].project.root == tmp_path / "first"
    assert loaded["second"].project.root == tmp_path / "second"
    assert type(loaded["first"]).__name__ == "Poured"
    assert type(loaded["second"]) is sk.Sample


def test_a_folder_above_projects_reads_each_by_its_own_collection_rules(tmp_path):
    # From the folder above, a project's `exclude` was not applied: a draft the
    # command line left out was loaded.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    project = tmp_path / "p"
    project.mkdir()
    (project / ".samplekitrc").write_text(
        'schema_version = 1\n[collection]\nexclude = ["draft-*"]\n'
    )
    for name in ("a", "draft-b"):
        (project / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\n---\n"
        )
    assert [sample.name for sample in sk.load(tmp_path)] == ["a"]


def test_a_declared_name_on_a_list_spanning_two_projects_is_refused(tmp_path):
    first, second = two_projects(tmp_path)
    mixed = first + second
    with pytest.raises(ValueError):
        mixed.to_csv(profile="ductilities")
    text = mixed.to_csv(profile=first.project.profiles.ductilities)
    assert text.startswith("name,")


# ------------------------------------------------------------------ saving


def test_mutation_through_a_property_reaches_the_sample(collection):
    sample = sk.Sample(collection.root / "A.md")
    diameter = sample.diameter
    diameter.value = 20.0
    sample.save()
    assert "v: 20.0" in sample.path.read_text()


def test_saving_preserves_the_note(project):
    note = "# Notes  \n\n\tIndented, trailing spaces   \r\nno final newline"
    path = project.root / "n.md"
    path.write_bytes(
        b"---\nschema_version: 1\nproperties:\n  diameter: 1.0\n---\n" + note.encode()
    )
    sample = sk.Sample(path)
    sample.diameter.value = 2.0
    sample.save()
    assert path.read_bytes().endswith(note.encode())


def test_a_loaded_sample_remembers_its_path(collection):
    sample = sk.Sample(collection.root / "A.md")
    assert sample.path == collection.root / "A.md"
    assert sample.filename == "A.md"


def test_save_without_a_path_rewrites_the_file_read(collection):
    other = (collection.root / "B.md").read_text()
    sample = sk.Sample(collection.root / "A.md")
    sample.diameter.value = 21.0
    sample.save()
    assert "v: 21.0" in (collection.root / "A.md").read_text()
    assert (collection.root / "B.md").read_text() == other


def test_save_without_a_path_on_a_new_sample_suggests_one():
    with pytest.raises(ValueError) as caught:
        sk.Sample().save()
    assert "save(path)" in str(caught.value)


def test_save_to_a_path_moves_the_sample_there(collection, tmp_path):
    original = collection.root / "A.md"
    before = original.read_text()
    sample = sk.Sample(original)
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (elsewhere / ".samplekitrc").write_text("schema_version = 1\n")
    moved = elsewhere / "A-copy.md"
    sample.save(moved)
    assert sample.path == moved
    assert sample.project.root == elsewhere
    sample.diameter.value = 1.0
    sample.save()
    assert "v: 1.0" in moved.read_text()
    assert original.read_text() == before


def test_save_refuses_another_existing_file_without_overwrite(collection):
    sample = sk.Sample(collection.root / "A.md")
    target = collection.root / "B.md"
    before = target.read_text()
    with pytest.raises(FileExistsError):
        sample.save(target)
    assert target.read_text() == before
    sample.save(target, overwrite=True)
    assert "name: A" in target.read_text()


def test_save_refuses_a_file_changed_since_it_was_read(collection):
    path = collection.root / "A.md"
    sample = sk.Sample(path)
    path.write_text(path.read_text().replace("12.5", "12.6"))
    with pytest.raises(OSError) as caught:
        sample.save()
    assert type(caught.value) is OSError
    assert "A.md" in str(caught.value)
    assert "12.6" in path.read_text()
    sample.save(overwrite=True)
    assert "12.5" in path.read_text()


def test_save_refuses_a_file_removed_since_it_was_read(collection):
    path = collection.root / "A.md"
    sample = sk.Sample(path)
    path.unlink()
    with pytest.raises(FileNotFoundError) as caught:
        sample.save()
    assert "A.md" in str(caught.value)
    assert not path.exists()


def test_dir_lists_property_names(collection):
    listing = dir(sk.Sample(collection.root / "A.md"))
    assert "diameter" in listing
    assert "alloy" in listing


# ------------------------------------------------------------ computation


class Shapes(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.diameter = sk.Property(12.0)
        self.volume = sk.Property(4.0)
        self.ductility = sk.Property(compute_value=lambda: self.diameter.value / self.volume.value)
        self.diameter_tolerance = sk.Property(12.0, compute_uncertainty=lambda: 0.1, depends_on=[])
        self.joint = sk.Property(compute=lambda: (self.diameter.value * 2, 0.5), depends_on=["diameter"])
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=lambda row: 1 / row.R.value, depends_on=["row.R"]),
                "N": sk.Property(compute_column=lambda columns: [value / max(columns["R"].values) for value in columns["R"].values], depends_on=["preheating.R"]),
            },
            index="T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        )


def test_all_five_computation_shapes_are_expressible(tmp_path):
    sample = Shapes()
    sample.compute()
    assert sample.ductility.value == 3.0
    assert sample.diameter_tolerance.uncertainty == 0.1
    assert (sample.joint.value, sample.joint.uncertainty) == (24.0, 0.5)
    assert sample.preheating.values("G") == [0.5, 0.25]
    assert sample.preheating.values("N") == [0.5, 1.0]
    path = tmp_path / "shapes.md"
    sample.save(path)
    text = path.read_text()
    # A record names the channel its formula produced.
    assert "computed: {v: {diameter:" in text or "computed: {diameter:" in text, text
    assert "u: 0.5" in text




def test_a_column_declares_its_formula_as_a_property_does():
    # Three ways to give a cell: a value, a dict of them, a Property for one
    # with its uncertainty; and the column's formula declared on the table.
    class Pull(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.area = sk.Property(value=2.0)
            self.curve = sk.Table(
                {
                    "strain": sk.Property(),
                    "force": sk.Property(),
                    "stress": sk.Property(
                        compute=lambda row: row.force.value / self.area.value,
                        depends_on=["row.force", "area"],
                    ),
                    "spread": sk.Property(
                        compute=lambda row: (row.force.value, 0.1),
                        depends_on=["row.force"],
                    ),
                    "low": sk.Property(),
                    "high": sk.Property(),
                },
                index="strain",
                rows=[{"strain": 0, "force": 1.0}, {"strain": 1, "force": 3.0}],
                computed=[
                    sk.ComputedColumns(["low", "high"], compute=lambda row: {
                        "low": (row.force.value - 1, 0.5),
                        "high": row.force.value + 1,
                    }, depends_on=["row.force"]),
                ],
            )

    sample = Pull()
    sample.compute()
    assert sample.curve.values("stress") == [0.5, 1.5]
    assert sample.curve.values("spread") == [1.0, 3.0]
    assert sample.curve.uncertainties("spread") == [0.1, 0.1]
    assert sample.curve.uncertainties("low") == [0.5, 0.5]
    assert sample.curve.values("high") == [2.0, 4.0]
    # A whole column's formula takes pairs too.
    table = sk.Table(
        {
            "t": sk.Property(),
            "x": sk.Property(),
            "y": sk.Property(compute_column=lambda columns: [(v * 2, 0.2) for v in columns["x"].values], depends_on=["t.x"]),
        },
        index="t",
        rows=[{"t": 0, "x": 1.0}, {"t": 1, "x": 2.0}],
    )
    holder = sk.Sample()
    holder.t = table
    holder.compute()
    assert holder.t.uncertainties("y") == [0.2, 0.2]
    # Each half alone, beside the other entered; and both, each its formula's.
    halves = sk.Table(
        {
            "t": sk.Property(),
            "x": sk.Property(),
            "doubled": sk.Property(
                compute_value=lambda row: 2 * row.x.value, depends_on=["row.x"]
            ),
            "rated": sk.Property(
                compute_uncertainty=lambda row: 0.1 * row.x.value, depends_on=["row.x"]
            ),
            # None is no uncertainty, as a property's formula gives it.
            "plain": sk.Property(compute_uncertainty=lambda row: None, depends_on=[]),
            "both": sk.Property(
                compute_value=lambda row: 3 * row.x.value,
                compute_uncertainty=lambda row: 0.5,
                depends_on={"v": ["row.x"], "u": []},
            ),
            # In sequence, as a property's two: the uncertainty is given the
            # value just computed.
            "scaled": sk.Property(
                compute_value=lambda row: 10 * row.x.value,
                compute_uncertainty=lambda row: 0.01 * row.scaled.value,
                depends_on={"v": ["row.x"], "u": ["row.scaled.v"]},
            ),
        },
        index="t",
        rows=[{"t": 0, "x": 1.0, "doubled": sk.Property(None, 0.3), "rated": 7.0, "plain": 4.0}],
    )
    holder.h = halves
    holder.compute()
    doubled, rated, both = (holder.h.at(0, column) for column in ("doubled", "rated", "both"))
    assert (doubled.value, doubled.uncertainty) == (2.0, 0.3)
    assert (rated.value, rated.uncertainty) == (7.0, 0.1)
    assert (both.value, both.uncertainty) == (3.0, 0.5)
    assert holder.h.at(0, "scaled").uncertainty == pytest.approx(0.1)
    assert holder.h.at(0, "plain").uncertainty is None
    assert holder.h.at(0, "plain").state == "current"
    # What it reads is said, and one formula a column.
    with pytest.raises(ValueError, match="depends_on"):
        sk.Table({"t": sk.Property(), "y": sk.Property(compute=lambda row: 1.0)}, index="t")
    with pytest.raises(ValueError, match="keep one"):
        sk.Table(
            {"t": sk.Property(), "y": sk.Property(compute=lambda row: 1.0, depends_on=[])},
            index="t",
            computed=[
                sk.ComputedColumns("y", compute=lambda row: 2.0, depends_on=[]),
            ],
        )

def test_a_table_names_its_index():
    columns = {"day": sk.Property(unit="d"), "hour": sk.Property(unit="h"), "g": sk.Property()}
    # A bare second argument did not say what it was.
    with pytest.raises(TypeError, match="index="):
        sk.Table(columns, "day")
    with pytest.raises(TypeError, match="names its index"):
        sk.Table(columns)
    table = sk.Table(columns, index=["day", "hour"], rows=[{"day": 1, "hour": 6, "g": 1.05}])
    assert table.at((1, 6), "g").value == 1.05


def test_computed_columns_fill_several_columns_at_once():
    def bounds(row):
        return {"low": row.force.value - 1, "high": (row.force.value + 1, 0.5)}

    def spread(columns):
        values = columns["force"].values
        return {"least": [min(values)] * len(values), "most": [max(values)] * len(values)}

    table = sk.Table(
        {name: sk.Property() for name in ("strain", "force", "low", "high", "least", "most")},
        index="strain",
        rows=[{"strain": 0, "force": 1.0}, {"strain": 1, "force": 3.0}],
        computed=[
            sk.ComputedColumns(["low", "high"], compute=bounds, depends_on=["row.force"]),
            sk.ComputedColumns(["least", "most"], compute_column=spread, depends_on=["t.force"]),
        ],
    )
    holder = sk.Sample()
    holder.t = table
    holder.compute()
    assert holder.t.values("low") == [0.0, 2.0]
    assert holder.t.uncertainties("high") == [0.5, 0.5]
    assert holder.t.values("most") == [3.0, 3.0]
    with pytest.raises(ValueError, match="compute="):
        sk.ComputedColumns(["low"], depends_on=[])
    with pytest.raises(ValueError, match="not both"):
        sk.ComputedColumns(["low"], compute=bounds, compute_column=spread, depends_on=[])
    with pytest.raises(ValueError, match="depends_on"):
        sk.ComputedColumns(["low"], compute=bounds)
    with pytest.raises(ValueError, match="keep one"):
        sk.Table(
            {"t": sk.Property(), "y": sk.Property(compute=lambda row: 1.0, depends_on=[])},
            index="t",
            computed=[sk.ComputedColumns(["y"], compute=lambda row: {"y": 2.0}, depends_on=[])],
        )


def test_compute_column_fills_a_tables_column_only():
    sample = sk.Sample()
    with pytest.raises(ValueError, match="compute="):
        sample.x = sk.Property(compute_column=lambda columns: [1.0], depends_on=[])
    with pytest.raises(ValueError, match="compute_column"):
        sk.Property(compute=lambda: 1.0, compute_column=lambda columns: [1.0])
    with pytest.raises(ValueError, match="compute_column"):
        sk.Property(value=sk.stats.mean, compute_column=lambda columns: [1.0])


def test_compute_gives_the_property_whole(tmp_path):
    # `compute` was a value's formula; the property's, from one call, was
    # `compute_quantity`.
    class Bar(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.alone = sk.Property(unit="mm", compute=lambda: 2.5, depends_on=[])
            self.pair = sk.Property(unit="mm", compute=lambda: (2.5, 0.1), depends_on=[])
            self.half = sk.Property(
                unit="mm", compute_value=lambda: 2.5, uncertainty=0.2, depends_on=[]
            )

    sample = Bar()
    sample.compute()
    assert (sample.alone.value, sample.alone.uncertainty) == (2.5, None)
    assert (sample.pair.value, sample.pair.uncertainty) == (2.5, 0.1)
    # A value computed beside an uncertainty given apart keeps it.
    assert (sample.half.value, sample.half.uncertainty) == (2.5, 0.2)
    path = tmp_path / "bar.md"
    sample.save(path)
    text = path.read_text()
    # Each is one formula's: its record names no channel.
    assert "  alone:\n    v: 2.5\n    unit: mm\n    computed: {}\n" in text, text
    assert "  pair:\n    v: 2.5\n    u: 0.1\n    unit: mm\n    computed: {}\n" in text, text
    # The value alone is the formula's, the uncertainty the hand's.
    assert "u: 0.2\n    unit: mm\n    computed: {v: {}}\n" in text, text
    for extra in ({"uncertainty": 0.1}, {"compute_uncertainty": lambda: 0.1}):
        with pytest.raises(ValueError, match="compute_value"):
            sk.Property(compute=lambda: 2.5, **extra)
    with pytest.raises(ValueError, match="compute_value"):
        sk.Property(compute=lambda: 2.5, compute_value=lambda: 2.5)

def test_a_multi_output_table_callback_runs_once():
    calls = {"row": 0, "column": 0}

    def fit(row):
        calls["row"] += 1
        return {"ph": row.R.value * 2, "pitch": row.R.value * 3}

    def normalize(columns):
        calls["column"] += 1
        values = columns["R"].values
        return {"low": [v - 1 for v in values], "high": [v + 1 for v in values]}

    table = sk.Table(
        {name: sk.Property() for name in ("T", "R", "ph", "pitch", "low", "high")},
        index="T",
        rows=[{"T": 1, "R": 1.0}, {"T": 2, "R": 2.0}],
        computed=[
            sk.ComputedColumns(["ph", "pitch"], compute=fit, depends_on=["row.R"]),
            sk.ComputedColumns(["low", "high"], compute_column=normalize, depends_on=["t.R"]),
        ],
    )
    sample = sk.Sample()
    sample.t = table
    sample.compute()
    assert sample.t.values("pitch") == [3.0, 6.0]
    assert sample.t.values("high") == [2.0, 3.0]
    assert calls == {"row": 2, "column": 1}


def test_a_table_callback_refuses_an_output_mismatch():
    sample = sk.Sample()
    sample.t = sk.Table(
        {name: sk.Property() for name in ("T", "R", "ph", "pitch")},
        index="T",
        rows=[{"T": 1, "R": 1.0}],
        computed=[
            sk.ComputedColumns(["ph", "pitch"], compute=lambda row: {"ph": 1.0}, depends_on=["row.R"]),
        ],
    )
    with pytest.raises(ValueError) as caught:
        sample.compute("t")
    assert "pitch" in str(caught.value)
    # Nothing was written for the output the run did return: it holds no value,
    # and computing again refuses again.
    assert sample.t.at(1, "ph").value is None
    with pytest.raises(ValueError):
        sample.compute("t")

    sample.u = sk.Table(
        {name: sk.Property() for name in ("T", "R", "N")},
        index="T",
        rows=[{"T": 1, "R": 1.0}, {"T": 2, "R": 2.0}],
        computed=[
            sk.ComputedColumns("N", compute_column=lambda columns: [1.0], depends_on=["u.R"]),
        ],
    )
    with pytest.raises(ValueError):
        sample.compute("u")


def test_stats_returns_a_typed_summary(collection):
    summary = sk.SampleList(collection.root).stats("diameter")
    assert isinstance(summary, sk.Summary)
    assert not isinstance(summary, dict)
    assert summary.count == 3
    assert summary.minimum == 11.0
    assert summary.maximum == 13.1234
    assert summary.mean == pytest.approx((12.5 + 13.1234 + 11.0) / 3)
    for statistic in (
        "median",
        "first_quartile",
        "third_quartile",
        "sample_stdev",
        "population_stdev",
        "standard_error",
    ):
        assert getattr(summary, statistic) is not None


# ------------------------------------------------------------------ added


def test_filter_takes_an_expression_or_a_function(collection):
    samples = sk.SampleList(collection.root)
    by_expression = samples.filter("alloy == Inconel")
    by_function = samples.filter(lambda sample: sample.alloy == "Inconel")
    assert names(by_expression) == names(by_function) == ["A", "C"]
    with pytest.raises(KeyError):
        samples.filter("bere == Inconel")


def test_a_list_of_paths_is_refused_naming_the_constructor(collection):
    with pytest.raises(TypeError) as caught:
        sk.SampleList([collection.root / "A.md"])
    assert "Sample(p)" in str(caught.value)


def test_a_table_assigned_under_another_name_than_its_column_inputs_is_refused():
    table = sk.Table(
        {
            "T": sk.Property(),
            "R": sk.Property(),
            "N": sk.Property(compute_column=lambda columns: columns["R"].values, depends_on=["soak.R"]),
        },
        index="T",
    )
    sample = sk.Sample()
    with pytest.raises(ValueError) as caught:
        sample.preheating = table
    assert "soak" in str(caught.value)
    assert "preheating" in str(caught.value)




# ------------------------------------------------------ recomputing

RUNS = {"ductility": 0, "roughness": 0, "rigidity": 0, "spread": 0, "flatness": 0}


def reset_runs():
    for name in RUNS:
        RUNS[name] = 0


class Ductilities(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.ductility = sk.Property(compute_value=self._ductility)
        self.roughness = sk.Property(compute_value=self._roughness)
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.set_dependencies("roughness", depends_on=["ductility"])

    def _ductility(self):
        RUNS["ductility"] += 1
        return self.diameter.value / self.volume.value

    def _roughness(self):
        RUNS["roughness"] += 1
        return 1 - self.ductility.value / 4.0


class Soak(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.wear = sk.Property(2.0)
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=self._rigidity, depends_on=["row.R", "wear"]),
            },
            index="T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        )

    def _rigidity(self, row):
        RUNS["rigidity"] += 1
        return 1 / row.R.value / self.wear.value


def saved_ductilities(directory, file="d.md", diameter=12.0):
    """Ductilities computed from diameter and a volume of 4, saved, and runs reset."""
    sample = Ductilities(name=file)
    sample.diameter.value = diameter
    sample.volume.value = 4.0
    sample.compute()
    path = directory / file
    sample.save(path)
    reset_runs()
    return path


def edit_diameter(path, before="12.0", after="16.0"):
    text = path.read_text()
    edited = text.replace(f"v: {before}", f"v: {after}", 1)
    assert edited != text
    path.write_text(edited)
    return edited


def test_a_value_stale_on_disk_is_served_and_reported_stale(tmp_path):
    path = saved_ductilities(tmp_path)
    edit_diameter(path)
    sample = Ductilities(path)
    assert sample.ductility.value == 3.0
    assert sample.ductility.is_outdated
    assert sample.roughness.is_outdated
    assert not sample.diameter.is_outdated
    assert RUNS["ductility"] == 0 and RUNS["roughness"] == 0


def test_saving_runs_no_formula(tmp_path):
    path = saved_ductilities(tmp_path)
    edit_diameter(path)
    sample = Ductilities(path)
    sample.save()
    assert RUNS["ductility"] == 0 and RUNS["roughness"] == 0
    reloaded = Ductilities(path)
    assert reloaded.ductility.value == 3.0
    assert reloaded.ductility.is_outdated

    fresh = Ductilities(name="fresh")
    fresh.diameter.value = 1.0
    fresh.volume.value = 8.0
    fresh.save(tmp_path / "fresh.md")
    text = (tmp_path / "fresh.md").read_text()
    assert RUNS["ductility"] == 0
    assert "0.125" not in text
    assert "computed" not in text




def test_compute_without_names_runs_stale_and_never_computed_values(tmp_path):
    path = saved_ductilities(tmp_path)
    edit_diameter(path)
    sample = Ductilities(path)
    sample.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (1, 1)
    assert sample.ductility.value == 4.0
    assert sample.outdated() == []
    sample.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (1, 1)
    sample.compute(rerun=True)
    assert (RUNS["ductility"], RUNS["roughness"]) == (2, 2)

    fresh = Ductilities(name="fresh")
    fresh.diameter.value = 1.0
    fresh.volume.value = 8.0
    fresh.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (3, 3)
    fresh.save(tmp_path / "fresh.md")
    assert "0.125" in (tmp_path / "fresh.md").read_text()


def test_a_list_recomputes_every_sample(tmp_path):
    first = saved_ductilities(tmp_path, "a.md")
    second = saved_ductilities(tmp_path, "b.md", diameter=8.0)
    edit_diameter(first)
    edit_diameter(second, "8.0", "20.0")
    samples = sk.SampleList([Ductilities(first), Ductilities(second)])
    samples.compute()
    assert RUNS["ductility"] == 2
    assert [sample.ductility.value for sample in samples] == [4.0, 5.0]
    samples.compute("ductility")
    assert RUNS["ductility"] == 2
    samples.compute("ductility", rerun=True)
    assert RUNS["ductility"] == 4


def test_stale_lists_properties_and_columns(tmp_path):
    soak = Soak()
    soak.compute()
    path = tmp_path / "soak.md"
    soak.save(path)
    text = path.read_text()
    edited = re.sub(r"(?m)^(  wear: (?:\{v: )?)2\.0", r"\g<1>4.0", text)
    assert edited != text
    path.write_text(edited)
    reset_runs()
    stale = Soak(path)
    assert stale.outdated() == ["preheating.G"]
    assert stale.preheating.at(10, "G").is_outdated
    stale.compute()
    assert RUNS["rigidity"] == 2
    assert stale.outdated() == []


def test_a_recomputation_that_changed_nothing_reruns_nothing_downstream(tmp_path):
    sample = Ductilities(saved_ductilities(tmp_path))
    sample.ductility.compute(rerun=True)
    assert RUNS["ductility"] == 1
    assert sample.roughness.value == 0.25
    assert RUNS["roughness"] == 0


class ForgetsPath(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(name=name)


class SkipsInit(sk.Sample):
    def __init__(self, path=None):
        self.checked = True


def test_a_path_not_forwarded_to_super_init_is_refused(collection):
    path = collection.root / "A.md"
    with pytest.raises(TypeError) as caught:
        ForgetsPath(path)
    assert "super().__init__" in str(caught.value)
    assert "ForgetsPath" in str(caught.value)
    with pytest.raises(TypeError):
        SkipsInit(path)
    assert ForgetsPath(name="new").name == "new"


class Chain(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.diameter = sk.Property(10.0)
        self.swarf = sk.Property(compute_value=lambda: self.diameter.value * 2)
        self.porosity = sk.Property(compute_value=lambda: self.swarf.value / 4)
        self.vfi = sk.Property(compute_value=lambda: self.porosity.value + 1)
        self.set_dependencies("swarf", depends_on=["diameter"])
        self.set_dependencies("porosity", depends_on=["swarf"])
        self.set_dependencies("vfi", depends_on=["porosity"])


def entered_ductilities():
    sample = Ductilities()
    sample.diameter.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    assert sample.roughness.value == 0.25
    reset_runs()
    return sample


def test_assigning_a_computed_value_overrides_it():
    sample = entered_ductilities()
    sample.ductility.value = 2.0
    assert sample.ductility.is_computed
    assert sample.ductility.is_edited
    assert not sample.ductility.is_outdated
    assert sample.edited() == ["ductility"]
    sample.compute()
    assert sample.roughness.value == 0.5
    sample.diameter.value = 40.0
    sample.compute()
    assert sample.ductility.value == 2.0
    assert sample.roughness.value == 0.5
    assert (RUNS["ductility"], RUNS["roughness"]) == (0, 1)




def test_computing_a_named_value_computes_its_stale_inputs_first(tmp_path):
    path = saved_ductilities(tmp_path)
    edit_diameter(path)
    sample = Ductilities(path)
    assert sample.ductility.is_outdated and sample.roughness.is_outdated
    sample.roughness.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (1, 1)
    assert sample.ductility.value == 4.0
    assert sample.roughness.value == 0.0
    assert sample.outdated() == []
    reset_runs()
    sample.compute("roughness", rerun=True)
    assert (RUNS["ductility"], RUNS["roughness"]) == (0, 1)


def test_an_override_round_trips_through_a_file(tmp_path):
    path = saved_ductilities(tmp_path)
    sample = Ductilities(path)
    sample.ductility.value = 2.0
    sample.compute()
    assert sample.roughness.value == 0.5
    sample.save()
    text = path.read_text()
    assert re.search(r"(?m)^    fingerprint: \{edited: [0-9a-f]{12}\}$", text), text
    assert re.search(r"computed: \{v: \{ductility: \{edited: [0-9a-f]{12}\}\}\}", text), text
    reloaded = Ductilities(path)
    assert reloaded.ductility.is_edited
    assert reloaded.ductility.value == 2.0
    assert not reloaded.roughness.is_outdated
    assert reloaded.roughness.edited_upstream == ["ductility"]
    assert RUNS["ductility"] == 0

    hand = saved_ductilities(tmp_path, file="hand.md")
    text = hand.read_text()
    edited = re.sub(r"(?m)^(  ductility:\n    v: )3\.0$", r"\g<1>3.5", text)
    assert edited != text, text
    hand.write_text(edited)
    held = Ductilities(hand)
    assert held.ductility.is_edited
    held.compute(rerun=True)
    assert held.ductility.value == 3.5
    assert RUNS["ductility"] == 0


def test_edited_upstream_names_the_overrides_a_value_rests_on():
    sample = Chain()
    sample.swarf.value = 8.0
    sample.compute()
    assert sample.vfi.value == 3.0
    assert sample.vfi.edited_upstream == ["swarf"]
    assert sample.porosity.edited_upstream == ["swarf"]
    assert sample.swarf.edited_upstream == []
    sample.swarf.compute(force=True)
    sample.compute()
    assert sample.vfi.value == 6.0
    assert sample.vfi.edited_upstream == []


def test_the_package_carries_its_version():
    import pathlib
    import tomllib

    manifest = pathlib.Path(__file__).resolve().parents[2] / "Cargo.toml"
    assert sk.__version__ == tomllib.loads(manifest.read_text())["package"]["version"]


class Measured(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.age = sk.Property(2.0, compute_uncertainty=self._spread, depends_on=[])

    def _spread(self):
        RUNS["spread"] += 1
        return 0.01


def test_compute_runs_an_uncertainty_formula_beside_an_entered_value(tmp_path):
    reset_runs()
    sample = Measured(name="m")
    sample.compute()
    assert RUNS["spread"] == 1
    sample.save(tmp_path / "m.md")
    assert "u: 0.01" in (tmp_path / "m.md").read_text()
    sample.compute()
    assert RUNS["spread"] == 1
    sample.compute("age")
    assert RUNS["spread"] == 1
    sample.compute("age", rerun=True)
    assert RUNS["spread"] == 2
    sample.compute(rerun=True)
    assert RUNS["spread"] == 3


class Nitridation(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.tempering = sk.Table(
            {
                "day": sk.Property(),
                "venting": sk.Property(),
                "nitridation": sk.Property(compute_column=lambda columns: [v / columns["venting"].values[0] for v in columns["venting"].values], depends_on=["tempering.venting"]),
            },
            index="day",
            rows=[{"day": 1, "venting": 2.0}, {"day": 2, "venting": 1.5}],
        )
        self.flatness = sk.Property(compute_value=self._flatness)
        self.set_dependencies("flatness", depends_on=["tempering.nitridation"])

    def _flatness(self):
        RUNS["flatness"] += 1
        return self.tempering.values("nitridation")[-1]




def test_compute_runs_what_is_named_when_it_is_pending(tmp_path):
    sample = Ductilities(saved_ductilities(tmp_path))
    sample.ductility.compute()
    sample.compute("ductility", "roughness")
    assert (RUNS["ductility"], RUNS["roughness"]) == (0, 0)
    sample.ductility.compute(rerun=True)
    assert RUNS["ductility"] == 1
    sample.compute("ductility", "roughness", rerun=True)
    assert (RUNS["ductility"], RUNS["roughness"]) == (2, 1)
    with pytest.raises(ValueError):
        sample.compute("diameter")
    with pytest.raises(KeyError) as caught:
        sample.compute("dutility")
    assert "ductility" in str(caught.value)

    stale = Ductilities(edit_and_return(saved_ductilities(tmp_path, "s.md")))
    reset_runs()
    stale.compute("roughness")
    assert (RUNS["ductility"], RUNS["roughness"]) == (1, 1)

    soak = Soak()
    soak.compute()
    reset_runs()
    soak.compute("preheating.G")
    soak.preheating.at(10, "G").compute()
    soak.compute("preheating")
    assert RUNS["rigidity"] == 0
    soak.compute("preheating.G", rerun=True)
    assert RUNS["rigidity"] == 2
    soak.preheating.at(10, "G").compute(rerun=True)
    assert RUNS["rigidity"] == 3
    soak.compute("preheating", rerun=True)
    assert RUNS["rigidity"] == 5


def edit_and_return(path):
    edit_diameter(path)
    return path


def test_compute_gives_an_override_back_only_when_forced():
    sample = entered_ductilities()
    sample.ductility.value = 2.0
    sample.compute()
    sample.compute(rerun=True)
    sample.ductility.compute()
    sample.compute("ductility", rerun=True)
    assert sample.ductility.value == 2.0
    assert sample.ductility.is_edited
    assert RUNS["ductility"] == 0
    sample.ductility.compute(force=True)
    assert RUNS["ductility"] == 1
    assert not sample.ductility.is_edited
    assert sample.edited() == []
    assert sample.ductility.value == 3.0
    sample.compute()
    assert sample.roughness.value == 0.25


def test_property_state_names_where_a_value_stands(tmp_path):
    path = saved_ductilities(tmp_path)
    edit_diameter(path)
    sample = Ductilities(path)
    assert sample.diameter.state == "entered"
    assert sample.ductility.state == "outdated"
    sample.compute()
    assert sample.ductility.state == "current"
    sample.ductility.value = 2.0
    assert sample.ductility.state == "edited"

    fresh = Ductilities(name="fresh")
    assert fresh.ductility.state == "never_computed"
    fresh.diameter.value = 1.0
    fresh.volume.value = 0.0
    with pytest.raises(ZeroDivisionError):
        fresh.ductility.compute()
    assert fresh.ductility.state == "failed"

    soak = Soak()
    assert soak.preheating.at(10, "G").state == "never_computed"
    soak.compute()
    assert soak.preheating.at(10, "G").state == "current"
    assert soak.preheating.at(10, "R").state == "entered"


def test_a_list_warns_of_a_file_it_could_not_read(project):
    project.sample("A.md", name="A", properties={"diameter": 12.0})
    (project.root / "broken.md").write_text("---\nschema_version: 1\nname: {a: 1}\n---\n")
    with pytest.warns(UserWarning, match="broken.md was not read"):
        samples = sk.SampleList(project.root, model=False)
    assert len(samples) == 1


def test_new_creates_a_sample_of_the_project_model_at_a_path(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    (project.root / "cells").mkdir()
    path = project.root / "cells" / "CC-10.md"
    sample = sk.Sample.new(path)
    assert type(sample).__name__ == "Model"
    assert sample.path == path
    assert sample.name == "CC-10"
    assert not path.exists()
    sample.diameter.value = 2.0
    sample.save()
    again = sk.Sample(path)
    assert again.name == "CC-10"
    assert again.diameter.value == 2.0
    again.compute()
    assert again.double.value == 4.0


def test_new_writes_no_name(project):
    # The file's name is the sample's; a name given is written.
    path = project.root / "CC-11.md"
    sample = sk.Sample.new(path, model=False)
    sample.save()
    assert "name:" not in path.read_text()
    assert sk.Sample(path, model=False).name == "CC-11"
    named = project.root / "CC-12.md"
    sk.Sample.new(named, name="Bee", model=False).save()
    assert "name: Bee" in named.read_text()
    assert sk.Sample(named, model=False).name == "Bee"


def test_new_refuses_a_path_that_exists(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    path = project.sample("s.md", properties={"diameter": "2.0"})
    with pytest.raises(FileExistsError, match=r"Sample\(path\)"):
        sk.Sample.new(path)
    assert not project.imported()


def test_new_on_a_model_class_creates_that_class(project):
    namespace = {"__file__": str(project.root / "model.py")}
    exec(MODEL, namespace)
    slab = namespace["Model"]
    sample = slab.new(project.root / "B.md", name="Bee")
    assert type(sample) is slab
    assert sample.name == "Bee"
    with pytest.raises(FileNotFoundError, match="does not exist"):
        slab.new(project.root / "missing" / "C.md")


def test_a_created_file_written_meanwhile_is_not_overwritten(project):
    path = project.root / "D.md"
    sample = sk.Sample.new(path, model=False)
    path.write_text("someone else's\n")
    with pytest.raises(FileExistsError, match="Sample.new"):
        sample.save()
    assert path.read_text() == "someone else's\n"
    sample.save(overwrite=True)
    assert sk.Sample(path).name == "D"


def test_docstrings_name_no_internal_decisions():
    import inspect
    import re

    # A decision's number, the native module, and the story of how the
    # behaviour came to be: a docstring says what a thing does.
    pattern = re.compile(r"D-\d+|_native|BTreeMap|\bBefore this\b|\bused to\b|\bA plain copy\b")
    documented = [("samplekit", sk)]
    for name in sk.__all__:
        exported = getattr(sk, name)
        if not inspect.isclass(exported):
            # A function is its own docstring; `NA` is its class's.
            documented.append((name, exported))
            continue
        for klass in inspect.getmro(exported):
            if not klass.__module__.startswith("samplekit"):
                continue
            documented.append((klass.__name__, klass))
            for member, value in vars(klass).items():
                if not member.startswith("_") or member == "__init__":
                    documented.append((f"{klass.__name__}.{member}", value))
    citing = sorted(
        {name for name, value in documented
         if isinstance(getattr(value, "__doc__", None), str) and pattern.search(value.__doc__)}
    )
    assert citing == []


CYCLED = """\
import samplekit as sk


class Aged(sk.Sample):
    def __init__(self, path=None, name=None, first=2.0):
        super().__init__(path, name=name)
        self.tempering = sk.Table(
            {
                "day": sk.Property(),
                "venting": sk.Property(),
                "nitridation": sk.Property(compute_column=lambda columns: [v / columns["venting"].values[0] for v in columns["venting"].values], depends_on=["tempering.venting"]),
            },
            index="day",
            rows=[{"day": 1, "venting": first}, {"day": 2, "venting": 1.0}],
        )
        self.flatness = sk.Property(compute_value=lambda: self.tempering.at(2, "nitridation").value)
        self.set_dependencies("flatness", depends_on=["tempering.nitridation"])
"""


def cycled_class(project):
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CYCLED, namespace)
    return namespace["Aged"]


def test_every_public_class_and_method_has_a_docstring():
    import inspect

    undocumented = []
    for name in sk.__all__:
        exported = getattr(sk, name)
        if name == "BoundList" or exported is sk.NA:
            continue
        if not inspect.isclass(exported):
            if not (exported.__doc__ or "").strip():
                undocumented.append(name)
            continue
        for klass in inspect.getmro(exported):
            if not klass.__module__.startswith("samplekit"):
                continue
            if not (klass.__doc__ or "").strip():
                undocumented.append(klass.__name__)
            for member, value in vars(klass).items():
                if member.startswith("_") or not callable(value):
                    continue
                if not (getattr(value, "__doc__", None) or "").strip():
                    undocumented.append(f"{klass.__name__}.{member}")
    # The attributes a script reads first say what they hold too.
    attributes = {
        sk.Property: ["value", "uncertainty", "unit", "symbol", "v", "u", "is_computed"],
        sk.Sample: ["name", "path", "filename", "note", "tags", "project"],
        sk.Table: ["index", "title"],
        sk.Export: ["filename", "path"],
    }
    for klass, names in attributes.items():
        for member in names:
            if not (inspect.getattr_static(klass, member).__doc__ or "").strip():
                undocumented.append(f"{klass.__name__}.{member}")
    assert sorted(set(undocumented)) == []


def test_a_table_update_stales_what_reads_the_table(project):
    aged = cycled_class(project)
    path = project.root / "C.md"
    sample = aged(name="C")
    sample.compute()
    sample.save(path)
    again = aged(path)
    assert again.flatness.state == "current"
    again.tempering.update(2, venting=0.5)
    assert again.flatness.state == "outdated"
    assert "flatness" in again.outdated()




def test_an_export_includes_what_its_declaration_includes(project):
    project.config(
        'schema_version = 1\n\n[profile.diameters]\ncolumns = [{field = "name"}, {field = "diameter"}]\n\n'
        '[export.diameters]\nprofile = "diameters"\nformat = "csv"\noutput = "diameters.csv"\nfilename = true\n'
    )
    project.sample("A.md", name="A", properties={"diameter": "12.5"})
    written = sk.SampleList(project.root).export("diameters", output=project.root / "out.csv")
    with open(written) as handle:
        assert "filename" in handle.readline()


def test_sorted_takes_a_comma_list(collection):
    samples = sk.SampleList(collection.root, model=False)
    written = [sample.name for sample in samples.sorted("-period,diameter")]
    assert written == [sample.name for sample in samples.sorted(["-period", "diameter"])]
    assert written == ["A", "B", "C"]


def test_a_table_not_yet_assigned_holds_no_derived_value_until_computed():
    table = sk.Table(
        {
            "T": sk.Property(),
            "R": sk.Property(),
            "G": sk.Property(compute=lambda row: 1 / row.R.value, depends_on=["row.R"]),
        },
        index="T",
        rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
    )
    assert table.values("G") == [None, None]
    # It is what it is: never computed, not current.
    assert table.at(10, "G").state == "never_computed"
    assert table.at(10, "R").state == "entered"
    sample = sk.Sample()
    sample.preheating = table
    sample.compute()
    assert sample.preheating.values("G") == [0.5, 0.25]
    assert sample.preheating.at(10, "G").state == "current"



def test_reading_a_value_never_runs_its_formula():
    reset_runs()
    sample = Ductilities()
    sample.diameter.value = 12.0
    sample.volume.value = 4.0
    with pytest.warns(UserWarning, match="never computed"):
        assert sample.ductility.value is None
    assert len(sk.SampleList([sample]).filter("ductility > 1")) == 0
    assert RUNS["ductility"] == 0
    sample.compute()
    assert sample.ductility.value == 3.0


def test_changing_an_input_leaves_a_dependent_stale_until_computed():
    sample = Ductilities()
    sample.diameter.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    reset_runs()
    sample.diameter.value = 16.0
    assert sample.ductility.is_outdated
    with pytest.warns(UserWarning, match="outdated"):
        assert sample.ductility.value == 3.0
    assert RUNS["ductility"] == 0
    sample.compute()
    assert sample.ductility.value == 4.0
    assert RUNS["ductility"] == 1
    assert not sample.ductility.is_outdated


def test_a_value_reading_a_derived_column_is_not_computed_twice():
    reset_runs()
    sample = Nitridation(name="r")
    sample.compute()
    assert RUNS["flatness"] == 1
    assert sample.flatness.value == 0.75
    assert not sample.flatness.is_outdated
    sample.compute()
    assert RUNS["flatness"] == 1


def test_a_list_filter_runs_no_formula(project):
    aged = cycled_class(project)
    samples = sk.SampleList([aged(name="Good"), aged(name="Bad", first=0.0)])
    assert [sample.name for sample in samples.filter("name == Good")] == ["Good"]


def test_to_csv_headers_spell_units_as_the_files_do(project, cli):
    # `plain` is the screen's form: a CSV is read back by programs, and
    # writes the unit as the files spell it, as the command line does.
    from conftest import DUCTILITIES

    project.config(DUCTILITIES + '\n[unit.g]\nplain = "gram"\n')
    project.sample("A.md", name="A", properties={"diameter": "{v: 12.5, u: 0.05, unit: g}"})
    samples = sk.SampleList(project.root)
    written = samples.to_csv(columns=["name", "diameter"])
    assert "[g]" in written.splitlines()[0]
    assert "gram" not in written
    assert written == cli(project.root, "-c", "name,diameter", "--csv")


CHAIN = """\
from samplekit import Property, Sample


class Chain(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = Property()
        self.double = Property(compute_value=lambda: self.diameter.value + 1.0)
        self.quadruple = Property(compute_value=lambda: self.double.value * 2)
        self.set_dependencies("double", depends_on=["diameter"])
        self.set_dependencies("quadruple", depends_on=["double"])
"""


def test_a_failed_compute_keeps_the_values_it_did_not_reach(project):
    sample_path = project.sample("s.md", properties={"diameter": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CHAIN, namespace)
    sample = namespace["Chain"](sample_path)
    sample.compute()
    sample.save()
    sample = namespace["Chain"](sample_path)
    # A date, which the formula cannot add to: text is refused where a number
    # is held.
    sample.diameter.value = datetime.date(2026, 9, 27)
    with pytest.raises(TypeError):
        sample.compute()
    sample.save()
    written = sample_path.read_text()
    assert "v: 5.0" in written
    # The value that failed, an input of another, is written failed, beside the
    # last value its formula gave.
    double = written.split("double:")[1].split("quadruple:")[0]
    assert "failed:" in double and "v:" in double


def test_not_current_answers_what_status_answers(project):
    """`outdated()` and `edited()` are the narrow questions; neither sees a
    failure.

    A quality-control script following the guide to the letter used to miss
    every broken formula, because `outdated()` is documented as *every
    outdated value* and a failed value is not outdated.
    """
    sample_path = project.sample("s.md", properties={"diameter": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CHAIN, namespace)
    sample = namespace["Chain"](sample_path)
    sample.compute()
    sample.save()

    fresh = namespace["Chain"](sample_path)
    assert fresh.not_current() == {}
    assert fresh.double.failure is None

    fresh.diameter.value = datetime.date(2026, 9, 27)
    with pytest.raises(TypeError):
        fresh.compute()
    fresh.save()

    after = namespace["Chain"](sample_path)
    reported = after.not_current()
    assert reported.get("double") == "failed", reported
    # The narrow questions still do not see it, which is the point.
    assert "double" not in after.outdated()
    # And the reason is an attribute, not a UserWarning a script must catch.
    assert "TypeError" in (after.double.failure or "")


def test_export_to_a_dash_writes_to_standard_output(collection, capsys, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    sk.SampleList(collection.root).export("diameters", output="-")
    assert "name" in capsys.readouterr().out
    assert not (tmp_path / "-").exists()


def test_save_all_to_a_missing_directory_names_no_lock(collection):
    with pytest.raises(OSError) as caught:
        sk.SampleList(collection.root).save_all(collection.root / "missing")
    assert ".lock" not in str(caught.value)


def test_a_value_read_without_its_model_says_how_it_stands(project):
    project.config()
    path = project.sample(
        "s.md",
        name="S",
        properties={
            "diameter": "{v: 1.0, fingerprint: aaaaaaaaaaaa}",
            "double": "{v: 2.0, computed: {diameter: aaaaaaaaaaaa}, fingerprint: bbbbbbbbbbbb}",
        },
    )
    assert sk.Sample(path, model=False).double.state == "edited"


def test_a_text_is_refused_on_a_property_with_a_unit(project):
    sample = quantity_sample(project)
    with pytest.raises(ValueError) as caught:
        sample.diameter.value = "abc"
    assert "is measured in g" in str(caught.value)


def test_a_model_declares_the_statistics_of_its_readings(project):
    sample_path = project.sample("s.md", properties={"diameter": "{v: 5.0, readings: [1.0, 2.0, 9.0]}"})

    class Doped(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.diameter = sk.Property(unit="g", value=sk.stats.median, uncertainty=sk.stats.sample_stdev)

    # The written value outranks the declared statistic; the uncertainty is the
    # declared one, since the file writes none.
    doped = Doped(sample_path)
    assert doped.diameter.value == 5.0
    assert doped.diameter.uncertainty == pytest.approx(19**0.5)
    assert sk.Sample(sample_path, model=False).diameter.value == 5.0
    # With no value written, the declared statistic is owed: reading computes
    # nothing, as the command line says it, and compute() takes it.
    unwritten = project.sample("u.md", properties={"diameter": "{readings: [1.0, 2.0, 9.0]}"})
    owed = Doped(unwritten)
    assert owed.diameter.value is None
    assert owed.diameter.state == "never_computed"
    owed.compute()
    assert owed.diameter.value == 2.0
    # Without the model no statistic is declared, and no mean stands in.
    assert sk.Sample(unwritten, model=False).diameter.value is None


class _Doped(sk.Sample):
    """`diameter` by declared statistics, and `twice` reading it."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.twice = sk.Property(compute_value=lambda: 2 * self.diameter.value, depends_on=["diameter"])


def _doped(project):
    path = project.sample("w.md", properties={"diameter": "{readings: [1.0, 2.0, 3.0], unit: g}"})
    sample = _Doped(path)
    sample.compute()
    sample.save()
    return path


def test_a_declared_statistic_reloaded_untouched_is_not_edited(project):
    # A file always writes a value beside its readings, so *written* is the
    # ordinary shape of a reloaded sample, not an override.
    sample = _Doped(_doped(project))
    assert sample.edited() == []
    assert sample.outdated() == []
    assert sample.diameter.state == "current"
    assert sample._plan([], False, False) == []


def test_a_corrected_reading_is_stale_and_an_ordinary_compute_runs_it(project):
    # A doping corrected by hand, from Python: the verdict the command line gives, and a
    # plain compute — no force — gives the value back to its statistic.
    path = _doped(project)
    text = path.read_text().replace("[1.0, 2.0, 3.0]", "[1.0, 2.0, 6.0]")
    assert "6.0" in text
    path.write_text(text)
    with pytest.warns(UserWarning, match="outdated"):
        sample = _Doped(path)
        assert sample.diameter.state == "outdated"
        assert sample.diameter.value == 2.0
    assert sample.edited() == []
    assert sample.outdated() == ["diameter", "twice"]
    sample.compute()
    assert sample.diameter.value == 3.0
    assert sample.twice.value == 6.0
    assert sample.outdated() == []
    sample.save()
    assert _Doped(path).diameter.state == "current"


class _Ratio(sk.Sample):
    """`ratio` fails where `b` is zero."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property()
        self.b = sk.Property()
        self.ratio = sk.Property(compute_value=lambda: self.a.value / self.b.value, depends_on=["a", "b"])


def test_a_failure_computed_earlier_reads_as_no_value_over_a_list(project):
    # One sample's failed formula made sorting, filtering and statistics over
    # the whole list raise: read as the file holds it — no value — as the
    # command line reads it, and said by not_current().
    samples = sk.SampleList([
        _Ratio(project.sample("ok.md", name="ok", properties={"a": "6.0", "b": "2.0"})),
        _Ratio(project.sample("zero.md", name="zero", properties={"a": "1.0", "b": "0.0"})),
    ])
    with pytest.raises(ZeroDivisionError):
        samples.compute()
    assert [s.name for s in samples.sorted("-ratio")] == ["ok", "zero"]
    assert [s.name for s in samples.filter("ratio > 1")] == ["ok"]
    assert samples.stats("ratio").mean == 3.0
    assert samples[1].not_current()["ratio"] == "failed"


def test_readings_assigned_in_python_are_stale_and_compute_takes_them(project):
    # The readings assigned from Python kept the old mean as the value, read
    # as entered, and compute() never took the new one: a file saved so said
    # current over a value its readings did not give. As `set --readings`.
    path = _doped(project)
    sample = _Doped(path)
    sample.diameter.readings = [4.0, 5.0, 6.0]
    assert sample.diameter.state == "outdated"
    assert sample.outdated() == ["diameter", "twice"]
    sample.compute()
    assert sample.diameter.value == 5.0
    assert sample.twice.value == 10.0
    sample.save()
    assert _Doped(path).outdated() == []


def test_readings_changed_in_place_are_the_propertys(project):
    # `readings` was a copy: append changed nothing, without a word.
    path = _doped(project)
    sample = _Doped(path)
    readings = sample.diameter.readings
    readings.append(6.0)
    assert list(sample.diameter.readings) == [1.0, 2.0, 3.0, 6.0]
    sample.compute()
    assert sample.diameter.value == 3.0
    with pytest.raises(ValueError):
        sample.diameter.readings.clear()
    assert list(sample.diameter.readings) == [1.0, 2.0, 3.0, 6.0]


def test_a_value_written_over_a_statistic_is_kept_until_forced(project):
    # The readings stand, the writing outranks them and survives a save, a
    # reload and an ordinary compute; force gives the statistic back.
    path = _doped(project)
    sample = _Doped(path)
    sample.diameter.value = 4.75
    assert list(sample.diameter.readings) == [1.0, 2.0, 3.0]
    assert sample.edited() == ["diameter"]
    sample.compute()
    sample.save()
    reloaded = _Doped(path)
    assert reloaded.diameter.value == 4.75
    assert reloaded.diameter.state == "edited"
    assert reloaded.edited() == ["diameter"]
    reloaded.compute()
    assert reloaded.diameter.value == 4.75
    reloaded.compute(force=True)
    assert reloaded.diameter.value == 2.0
    assert reloaded.edited() == []


def test_a_statistic_stands_only_where_it_means_something():
    with pytest.raises(ValueError, match="uncertainty"):
        sk.Property(value=sk.stats.standard_error)
    with pytest.raises(ValueError, match="value"):
        sk.Property(uncertainty=sk.stats.median)


def test_a_property_carries_the_statistics_of_its_readings():
    assert sk.Property([1.0, 2.0, 9.0]).stats.median == 2.0
    assert sk.Property(3.0).stats is None


def test_sorted_takes_a_function_as_python_does(collection):
    samples = sk.SampleList(collection.root)
    ordered = samples.sorted(key=lambda sample: str(sample.path))
    assert [str(sample.path) for sample in ordered] == sorted(str(sample.path) for sample in samples)


def test_load_reads_a_directory_or_a_file(collection):
    samples = sk.load(collection.root)
    assert isinstance(samples, sk.SampleList)
    assert isinstance(sk.load(samples[0].path), sk.Sample)
    # Several, folders and files alike, are one list.
    both = sk.load(samples[0].path, samples[1].path)
    assert isinstance(both, sk.SampleList)
    assert [sample.path for sample in both] == [samples[0].path, samples[1].path]
    with pytest.raises(TypeError):
        sk.load()


def test_a_table_says_how_a_column_is_read():
    table = sk.Table({"day": sk.Property(), "nitridation": sk.Property()}, index="day")
    with pytest.raises(TypeError, match="values"):
        table["nitridation"]
    with pytest.raises(TypeError, match="tuple"):
        table.at([1, 2], "nitridation")


def test_a_sample_is_addressed_as_the_command_line_addresses_it(project):
    path = project.sample("s.md", name="S", properties={"diameter": "{v: 1.5, u: 0.1}"})
    sample = sk.Sample(path)
    assert sample["name"] == "S"
    assert sample["diameter.u"] == 0.1
    assert isinstance(sample["diameter"], sk.Property)
    with pytest.raises(KeyError):
        sample["diameter.nothing"]


def test_a_datetime_keeps_its_microseconds():
    import datetime

    moment = datetime.datetime(2025, 11, 13, 15, 10, 53, 815606)
    assert sk.Property(moment).value == moment


def test_a_table_formula_reads_a_table_it_declares(tmp_path):
    class Crucible(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.raw = sk.Table({"T": sk.Property(), "f": sk.Property()}, index="T")
            self.refs = sk.Table({"T": sk.Property()}, index="T")
            self.shifted = sk.Table(
                {
                    "T": sk.Property(),
                    "g": sk.Property(compute_column=self._doubled, depends_on=["shifted.T", "raw.f"]),
                    "h": sk.Property(compute=self._peak, depends_on=["row.T", "raw.f"]),
                    # `refs.T` names a column `shifted` declares too: both
                    # qualifiers are its own until the table is assigned.
                    "k": sk.Property(
                        compute=lambda row: len(self.refs.values("T")), depends_on=["row.T", "refs.T"]
                    ),
                },
                index="T",
            )

        def _doubled(self, columns):
            f = dict(zip(self.raw.values("T"), self.raw.values("f")))
            return [2 * f[T] for T in columns["T"].values]

        def _peak(self, row):
            return max(self.raw.values("f")) + row.T.value

    sample = Crucible(name="c")
    sample.raw.add(T=20, f=1.0)
    sample.raw.add(T=30, f=2.0)
    sample.refs.add(T=0)
    sample.shifted.add(T=20)
    sample.shifted.add(T=30)
    sample.compute()
    assert sample.shifted.values("g") == [2.0, 4.0]
    assert sample.shifted.values("h") == [22.0, 32.0]
    assert sample.shifted.values("k") == [1, 1]
    sample.raw.update(20, f=5.0)
    assert sample.shifted.at(20, "g").state == "outdated"
    sample.compute()
    assert sample.shifted.values("g") == [10.0, 4.0]
    assert sample.shifted.values("h") == [25.0, 35.0]
    path = tmp_path / "c.md"
    sample.save(path)
    assert Crucible(path).shifted.at(20, "g").state == "current"


def test_a_sample_saves_again_when_a_table_precedes_the_one_it_reads(tmp_path):
    import re

    class Crucible(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.shifted = sk.Table(
                {
                    "T": sk.Property(),
                    "g": sk.Property(compute_column=self._doubled, depends_on=["shifted.T", "raw.f"]),
                },
                index="T",
            )
            self.raw = sk.Table({"T": sk.Property(), "f": sk.Property()}, index="T")

        def _doubled(self, columns):
            f = dict(zip(self.raw.values("T"), self.raw.values("f")))
            return [2 * f[T] for T in columns["T"].values]

    sample = Crucible(name="c")
    sample.raw.add(T=20, f=1.0)
    sample.raw.add(T=30, f=2.0)
    sample.shifted.add(T=20)
    sample.shifted.add(T=30)
    sample.compute()
    path = tmp_path / "c.md"
    sample.save(path)
    # A migrated file: the reading table first, its cells without records.
    text = path.read_text()
    assert text.index("shifted:") < text.index("raw:")
    path.write_text(re.sub(r", computed: \{[^}]*\}, fingerprint: [0-9a-f]+", "", text))
    assert "computed:" not in path.read_text()
    Crucible(path).save()
    assert Crucible(path).shifted.values("g") == [2.0, 4.0]


def test_a_cell_edited_by_hand_says_so(tmp_path):
    class Doubled(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.t = sk.Table(
                {
                    "T": sk.Property(),
                    "x": sk.Property(),
                    "y": sk.Property(compute=self._y, depends_on=["row.x"]),
                },
                index="T",
            )

        def _y(self, row):
            return 2 * row.x.value

    sample = Doubled(name="d")
    sample.t.add(T=20, x=1.0)
    sample.t.add(T=30, x=3.0)
    sample.compute()
    path = tmp_path / "d.md"
    sample.save(path)
    again = Doubled(path)
    again.t.update(20, y=77.0)
    assert again.t.at(20, "y").state == "edited"
    assert again.t.at(20, "y").is_edited
    assert again.edited() == ["t.y"]
    again.compute()
    assert again.t.at(20, "y").value == 77.0
    again.compute(force=True)
    assert again.t.at(20, "y").value == 2.0


class Declared(sk.Sample):
    """A dependent declared before its input, with `depends_on=` alone."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property(unit="g")
        self.q = sk.Property(compute_value=self._q, depends_on=["p"])
        self.p = sk.Property(compute_value=self._p, depends_on=["a"])

    def _p(self):
        RUNS["ductility"] += 1
        return self.a.value * 2

    def _q(self):
        RUNS["roughness"] += 1
        return self.p.value + 1


def test_declarations_are_settled_when_a_sample_is_made(tmp_path):
    # In memory, by `Sample.new`, or from a file, the edges exist and an
    # undeclared formula is refused at the same moment.
    sample = Declared(name="d")
    assert sample.dependencies("p") == ["a"]
    assert sample.dependents("a") == ["p"]
    sample.a.value = 1.0
    sample.compute()
    sample.a.value = 5.0
    assert sample.p.is_outdated
    assert sorted(sample.outdated()) == ["p", "q"]
    sample.save(tmp_path / "d.md")
    assert "computed: {v: {a: " in (tmp_path / "d.md").read_text()
    assert "computed: {}" not in (tmp_path / "d.md").read_text()

    created = Declared.new(tmp_path / "e.md")
    assert created.dependencies("p") == ["a"]

    class Undeclared(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.a = sk.Property(unit="g")
            self.r = sk.Property(compute_value=lambda: self.a.value)

    with pytest.raises(ValueError, match="does not say what it reads: r"):
        Undeclared(name="u")
    with pytest.raises(ValueError, match="does not say what it reads: r"):
        Undeclared.new(tmp_path / "u.md")


def test_compute_runs_values_in_dependency_order(tmp_path):
    # Declared out of order, each formula runs once: the dependent would
    # otherwise run its input inside itself and the input then run again.
    reset_runs()
    sample = Declared(name="d")
    sample.a.value = 1.0
    sample.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (1, 1)
    sample.a.value = 2.0
    sample.compute()
    assert (RUNS["ductility"], RUNS["roughness"]) == (2, 2)
    assert sample.q.value == 5.0


def test_a_loaded_value_is_current_by_either_declaration_route(tmp_path):
    # Every value the file supplied is current once everything is in place:
    # a `depends_on=` value, and a value reading a derived column, run again
    # for nothing when the declarations were settled after the fill.
    sample = Declared(name="d")
    sample.a.value = 1.0
    sample.compute()
    sample.save(tmp_path / "d.md")
    reset_runs()
    loaded = Declared(tmp_path / "d.md")
    assert loaded.p.state == "current"
    loaded.q.compute(rerun=True)
    assert (RUNS["ductility"], RUNS["roughness"]) == (0, 1)

    retained = Nitridation(name="r")
    retained.compute()
    retained.save(tmp_path / "r.md")
    reset_runs()
    again = Nitridation(tmp_path / "r.md")
    assert again.flatness.state == "current"
    again.compute()
    assert RUNS["flatness"] == 0


def test_a_row_added_with_a_derived_cell_holds_it():
    # The rule covers a cell supplied with its row as it covers one updated.
    sample = Shapes()
    sample.compute()
    sample.preheating.add(T=30, R=8.0, G=99.0)
    assert sample.preheating.at(30, "G").state == "edited"
    sample.compute()
    assert sample.preheating.at(30, "G").value == 99.0
    assert sample.edited() == ["preheating.G"]
    sample.compute(force=True)
    assert sample.preheating.at(30, "G").value == 0.125
    sample.preheating.add(T=40, R=16.0)
    assert sample.preheating.at(40, "G").state == "never_computed"
    sample.compute()
    assert sample.preheating.at(40, "G").value == 0.0625


def test_replacing_a_property_replaces_its_declaration():
    # The new Property carries its own depends_on, and what read the name by
    # name keeps reading it.
    sample = Declared(name="d")
    sample.a.value = 1.0
    sample.b = sk.Property(2.0)
    sample.p = sk.Property(compute_value=lambda: sample.b.value * 3, depends_on=["b"])
    assert sample.dependencies("p") == ["b"]
    assert sample.dependents("a") == []
    assert sample.dependencies("q") == ["p"]
    sample.compute()
    sample.a.value = 9.0
    assert sample.p.state == "current"
    sample.b.value = 4.0
    assert sample.p.state == "outdated"
    sample.p = sk.Property(compute_value=lambda: 1.0)
    with pytest.raises(ValueError, match="does not say what it reads: p"):
        sample.compute()


def test_a_list_reads_stale_values_in_silence(tmp_path):
    # A read never raises, and a list filters, tabulates and summarises what it
    # holds without a warning per row; the sample reports on request.
    sample = Ductilities(name="s")
    sample.diameter.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    sample.diameter.value = 16.0
    samples = sk.SampleList([sample])
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert [s.name for s in samples.filter("ductility > 1")] == ["s"]
        assert samples.to_dict(columns=["ductility"])["ductility_value"] == [3.0]
        assert samples.stats("ductility").mean == 3.0
    assert sample.outdated() == ["ductility", "roughness"]
    with pytest.warns(UserWarning, match="outdated"):
        assert sample.ductility.value == 3.0


def test_an_uncertainty_reads_its_own_value(tmp_path):
    # ±0.5 % of the reading, the most ordinary instrument specification there
    # is. The value is entered, the uncertainty is a formula's, and what it
    # reads is the value beside it.
    class Gauge(sk.Sample):
        def __init__(self, path=None, name=None):
            self.reading = sk.Property(
                unit="mL", compute_uncertainty=self._pct, depends_on=["reading.v"]
            )
            super().__init__(path, name=name)

        def _pct(self):
            return abs(self.reading.value) * 0.005

    sample = Gauge(name="g")
    sample.reading.value = 200.0
    sample.compute()
    assert sample.reading.uncertainty == 1.0
    written = tmp_path / "g.md"
    sample.save(written)
    assert "computed: {u: {reading.v: " in written.read_text()

    # Reloaded it is current, and a corrected reading stales the uncertainty
    # alone — which an ordinary compute then repairs, no force asked for.
    reloaded = Gauge(written)
    assert reloaded.reading.state == "current"
    assert reloaded.outdated() == []
    reloaded.reading.value = 250.0
    assert reloaded.outdated() == ["reading"]
    reloaded.compute()
    assert reloaded.reading.uncertainty == 1.25
    reloaded.save()
    assert Gauge(written).reading.state == "current"

    # Typed by hand, the uncertainty is an override, saved as one, and stays
    # one when the value beside it is corrected.
    typed = Gauge(written)
    typed.reading.uncertainty = 3.0
    typed.save()
    typed = Gauge(written)
    assert typed.reading.state == "edited"
    typed.reading.value = 260.0
    typed.compute()
    assert typed.reading.uncertainty == 3.0
    assert typed.reading.state == "edited"
    assert typed.outdated() == []


def test_a_cell_uncertainty_reads_the_value_beside_it(tmp_path):
    # The same instrument, its readings a column: each cell's value entered,
    # its uncertainty ±0.5 % of it.
    class Pull(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.curve = sk.Table(
                {
                    "strain": sk.Property(unit="%"),
                    "force": sk.Property(
                        unit="kN", compute_uncertainty=self._rated, depends_on=["row.force.v"]
                    ),
                },
                index="strain",
            )

        def _rated(self, row):
            return 0.005 * row.force.value

    sample = Pull(name="p")
    sample.curve.add(strain=0, force=10.0)
    sample.curve.add(strain=1, force=20.0)
    # Entered by hand, a value beside a computed uncertainty overrides nothing.
    assert sample.curve.at(0, "force").state == "never_computed"
    sample.compute()
    assert sample.curve.uncertainties("force") == [0.05, 0.1]
    written = tmp_path / "p.md"
    sample.save(written)
    assert "computed: {u: {row.force.v: " in written.read_text()

    # Reloaded it is current; a corrected value stales its uncertainty, which
    # an ordinary compute repairs.
    reloaded = Pull(written)
    assert reloaded.outdated() == []
    reloaded.curve.update(1, force=21.0)
    assert reloaded.outdated() == ["curve.force"]
    reloaded.compute()
    assert reloaded.curve.at(1, "force").uncertainty == pytest.approx(0.105)
    assert reloaded.curve.at(1, "force").value == 21.0
    reloaded.save()
    assert Pull(written).outdated() == []
    # A value assigned to the cell itself is no override either.
    again = Pull(written)
    again.curve.at(0, "force").value = 11.0
    assert again.curve.at(0, "force").state != "edited"
    again.compute()
    assert again.curve.at(0, "force").uncertainty == pytest.approx(0.055)
    # An uncertainty typed by hand is an override, and stays one when the
    # value beside it is corrected, until compute(force=True).
    again.curve.at(0, "force").uncertainty = 0.2
    again.curve.update(0, force=13.0)
    again.compute()
    assert again.curve.at(0, "force").uncertainty == 0.2
    assert again.curve.at(0, "force").state == "edited"
    # Saved and read again, it is still an override, its record kept, and no
    # outdated value.
    again.save()
    kept = Pull(written)
    assert kept.curve.at(0, "force").state == "edited"
    assert kept.outdated() == []
    assert "fingerprint: {edited: " in written.read_text()
    kept.compute(force=True)
    assert kept.curve.at(0, "force").uncertainty == pytest.approx(0.065)

    # Its own cell, read whole, is its own result: refused, naming the channel.
    with pytest.raises(ValueError, match=r"row\.force\.v"):
        sk.Table(
            {"strain": sk.Property(), "force": sk.Property(
                compute_uncertainty=lambda row: 0.1, depends_on=["row.force"])},
            index="strain",
        )


def test_a_cell_reading_a_stale_cell_of_its_row_is_outdated(tmp_path):
    # A true stress read from the stress beside it: the area corrected stales
    # the stress, and what rests on it with it, as a property reading another.
    class Pull(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.area = sk.Property(2.0)
            self.curve = sk.Table(
                {
                    "strain": sk.Property(),
                    "force": sk.Property(),
                    "stress": sk.Property(
                        compute=lambda row: row.force.value / self.area.value,
                        depends_on=["row.force", "area"],
                    ),
                    "true": sk.Property(
                        compute=lambda row: 2 * row.stress.value, depends_on=["row.stress"]
                    ),
                },
                index="strain",
            )

    sample = Pull(name="p")
    sample.curve.add(strain=0, force=10.0)
    sample.compute()
    written = tmp_path / "p.md"
    sample.save(written)
    reloaded = Pull(written)
    reloaded.area.value = 4.0
    assert reloaded.outdated() == ["curve.stress", "curve.true"]
    reloaded.save()
    assert Pull(written).curve.at(0, "true").state == "outdated"


def test_a_cell_typed_in_python_is_saved_as_an_override(tmp_path):
    # A computed cell written from a script was saved bare: the command line
    # read it *record missing*, and no ✎ said a hand had typed it.
    class Pull(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.curve = sk.Table(
                {
                    "strain": sk.Property(),
                    "force": sk.Property(),
                    "stress": sk.Property(
                        compute=lambda row: 2 * row.force.value, depends_on=["row.force"]
                    ),
                },
                index="strain",
            )

    sample = Pull(name="p")
    sample.curve.add(strain=0, force=1.0)
    sample.curve.add(strain=1, force=2.0)
    sample.compute()
    written = tmp_path / "p.md"
    sample.save(written)
    typed = Pull(written)
    typed.curve.update(0, stress=9.0)
    typed.curve.at(1, "stress").value = 8.0
    typed.save()
    assert written.read_text().count("fingerprint: {edited: ") == 2
    reloaded = Pull(written)
    assert [cell.state for cell in reloaded.curve.stress] == ["edited", "edited"]
    assert reloaded.outdated() == []
    reloaded.compute(force=True)
    assert reloaded.curve.values("stress") == [2.0, 4.0]


def test_a_declaration_says_what_each_formula_reads(tmp_path):
    # One depends_on, a mapping keyed by channel where a property has two
    # formulas. Each channel names everything its own formula reads, repetition
    # included; the file is what factors it.
    class Cell(sk.Sample):
        def __init__(self, path=None, name=None):
            self.src = sk.Property(unit="mL")
            self.p = sk.Property(
                unit="mL",
                compute_value=lambda: self.src.value * 2,
                compute_uncertainty=self._pct,
                depends_on={"v": ["src"], "u": ["src", "p.v"]},
            )
            super().__init__(path, name=name)

        def _pct(self):
            return abs(self.p.value) * 0.005 + self.src.value * 0.001

    sample = Cell(name="c")
    sample.src.value = 100.0
    sample.compute()
    assert (sample.p.value, sample.p.uncertainty) == (200.0, 1.1)
    written = tmp_path / "c.md"
    sample.save(written)
    # What both formulas read is stated once, at the quantity's level.
    assert "computed: {src: " in written.read_text()
    assert ", u: {p.v: " in written.read_text()

    reloaded = Cell(written)
    assert reloaded.p.state == "current"
    reloaded.src.value = 110.0
    assert reloaded.outdated() == ["p"]
    reloaded.compute()
    assert reloaded.p.value == 220.0

    def declare(depends_on, computes_value=True):
        class M(sk.Sample):
            def __init__(self, path=None, name=None):
                self.p = sk.Property(
                    compute_value=(lambda: 1.0) if computes_value else None,
                    compute_uncertainty=lambda: 0.1,
                    depends_on=depends_on,
                )
                super().__init__(path, name=name)

        return M(name="m")

    # A channel reading itself is a cycle of one; a value resting on its own
    # uncertainty is the other mistake, and the message says which.
    with pytest.raises(ValueError, match="value this formula is computing"):
        declare({"v": ["p.v"], "u": []})
    with pytest.raises(ValueError, match="uncertainty this formula is computing"):
        declare({"v": [], "u": ["p.u"]})
    with pytest.raises(ValueError, match="cannot be computed from it"):
        declare({"v": ["p.u"], "u": []})
    # Said once for both formulas where both compute, it does not say enough.
    with pytest.raises(ValueError, match="say which formula reads what"):
        declare(["p.v"])
    # And a formula on a channel the mapping leaves out is a formula that
    # declared nothing.
    with pytest.raises(ValueError, match="does not say what it reads"):
        declare({"u": ["p.v"]})
    with pytest.raises(ValueError, match="is not a channel"):
        declare({"value": []})
    with pytest.raises(ValueError, match="names no channel"):
        declare({})


def test_a_value_read_per_channel_stays_current_once_saved(tmp_path):
    # The documented per-channel pattern — common inputs, the uncertainty also
    # reading the value — with another value reading the property. The save
    # refreshed an input's own digest by a hand-made copy of the reader's rule
    # that lacked its case of common inputs, and the value read as edited the
    # moment it was written, then froze.
    class Cell(sk.Sample):
        def __init__(self, path=None, name=None):
            self.src = sk.Property()
            self.p = sk.Property(
                compute_value=lambda: self.src.value * 2,
                compute_uncertainty=lambda: abs(self.p.value) * 0.01,
                depends_on={"v": ["src"], "u": ["src", "p.v"]},
            )
            self.q = sk.Property(compute_value=lambda: self.p.value + 1, depends_on=["p"])
            super().__init__(path, name=name)

    sample = Cell(name="c")
    sample.src.value = 100.0
    sample.compute()
    written = tmp_path / "c.md"
    sample.save(written)

    reloaded = Cell(written)
    assert reloaded.not_current() == {}
    assert reloaded.edited() == []
    reloaded.src.value = 7.0
    assert sorted(reloaded.outdated()) == ["p", "q"]
    reloaded.compute()
    assert (reloaded.p.value, reloaded.q.value) == (14.0, 15.0)


def test_a_record_written_before_channels_is_not_frozen_by_a_save(tmp_path):
    # A file whose record names no channel, beside a model computing the value
    # alone and a file supplying the uncertainty. Naming the channel at the next
    # save changed how the stored digest is read without retaking it: the value
    # then read as edited, a load held it as an override, and nothing ever
    # recomputed it again.
    def digest(text):
        import hashlib

        return hashlib.sha256(text.encode()).hexdigest()[:12]

    written = tmp_path / "s.md"
    written.write_text(
        "---\nschema_version: 1\nname: S\nproperties:\n"
        f"  a: {{v: 100.0, fingerprint: {digest('{v: 100.0}')}}}\n"
        f"  p: {{v: 200.0, u: 3.0, computed: {{a: {digest('{v: 100.0}')}}}, "
        f"fingerprint: {digest('{v: 200.0, u: 3.0}')}}}\n"
        "---\n"
    )

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property()
            self.p = sk.Property(compute_value=lambda: self.a.value * 2, depends_on=["a"])
            self.q = sk.Property(compute_value=lambda: 1.0, depends_on=[])
            super().__init__(path, name=name)

    sample = Model(written)
    assert sample.p.state == "current"
    # A save the property takes no part in: something else is computed.
    sample.compute()
    sample.save()
    assert "computed: {v: {a: " in written.read_text()

    reloaded = Model(written)
    assert reloaded.edited() == []
    assert reloaded.p.state == "current"
    reloaded.a.value = 999.0
    assert reloaded.outdated() == ["p"]
    reloaded.compute()
    assert reloaded.p.value == 1998.0


def test_a_value_changed_by_hand_is_not_blessed_when_its_channel_is_named(tmp_path):
    # The other half of the same repair: the digest is retaken only where it
    # still vouches for what the file holds. A number a hand typed over a
    # record made before channels stays read as edited.
    def digest(text):
        import hashlib

        return hashlib.sha256(text.encode()).hexdigest()[:12]

    written = tmp_path / "s.md"
    written.write_text(
        "---\nschema_version: 1\nname: S\nproperties:\n"
        f"  a: {{v: 100.0, fingerprint: {digest('{v: 100.0}')}}}\n"
        # 250 is not what the digest was taken over: a hand wrote it.
        f"  p: {{v: 250.0, u: 3.0, computed: {{a: {digest('{v: 100.0}')}}}, "
        f"fingerprint: {digest('{v: 200.0, u: 3.0}')}}}\n"
        "---\n"
    )

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property()
            self.p = sk.Property(compute_value=lambda: self.a.value * 2, depends_on=["a"])
            self.q = sk.Property(compute_value=lambda: 1.0, depends_on=[])
            super().__init__(path, name=name)

    sample = Model(written)
    assert sample.p.state == "edited"
    sample.compute()
    sample.save()
    reloaded = Model(written)
    assert reloaded.p.state == "edited"
    assert reloaded.p.value == 250.0


def test_a_refused_declaration_changes_nothing():
    # Once a sample is made, a declaration applies as it is assigned. It was
    # judged after the property was installed, so a typo raised and left the
    # formula in place, marked declared, reading nothing.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.src = sk.Property(3.0)
            super().__init__(path, name=name)

    sample = Model(name="t")
    wanted = sk.Property(compute_value=lambda: sample.src.value * 2, depends_on=["srcc"])
    with pytest.raises(KeyError, match="srcc"):
        sample.p = wanted
    assert "p" not in sample
    # The caller's object keeps its formula, to be assigned again once fixed.
    assert wanted.is_computed

    # The same over an existing property: it is left as it was.
    sample.p = sk.Property(compute_value=lambda: sample.src.value * 2, depends_on=["src"])
    with pytest.raises(ValueError, match="names one channel of another value"):
        sample.p = sk.Property(compute_value=lambda: 1.0, depends_on={"v": ["src.v"]})
    assert sample.dependencies("p") == ["src"]
    sample.compute()
    assert sample.p.value == 6.0


def test_the_later_declaration_wins():
    # A constructor's depends_on waits until the sample is made; applied then,
    # it replaced a set_dependencies made after it in __init__ to correct it.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(1.0)
            self.b = sk.Property(2.0)
            self.c = sk.Property(
                compute_value=lambda: self.a.value + self.b.value, depends_on=["a"]
            )
            super().__init__(path, name=name)
            self.set_dependencies("c", depends_on=["a", "b"])

    sample = Model(name="t")
    assert sample.dependencies("c") == ["a", "b"]
    sample.compute()
    sample.b.value = 5.0
    assert sample.c.state == "outdated"


def test_a_channel_is_declared_only_where_a_formula_reads():
    # A key for a channel no formula produces was folded into the other
    # channel's record, and the file claimed a formula read what it never did.
    def declare(depends_on, **formulas):
        class Model(sk.Sample):
            def __init__(self, path=None, name=None):
                self.src = sk.Property(1.0)
                self.p = sk.Property(depends_on=depends_on, **formulas)
                super().__init__(path, name=name)

        return Model(name="m")

    with pytest.raises(ValueError, match="uncertainty of 'p' has no formula"):
        declare({"v": ["src"], "u": ["p.v"]}, compute_value=lambda: 1.0)
    with pytest.raises(ValueError, match="value of 'p' has no formula"):
        declare({"v": ["src"], "u": ["p.v"]}, compute_uncertainty=lambda: 0.1)
    # One list naming the value a value-only formula computes is a self-read,
    # and says so — not the advice to declare per channel.
    with pytest.raises(ValueError, match="cannot read what it produces"):
        declare(["p.v"], compute_value=lambda: 1.0)
    with pytest.raises(ValueError, match="in one formula"):
        declare(["p.v"], compute=lambda: (1.0, 0.1))
    # Any mapping reads as one, not only a dict.
    from types import MappingProxyType

    sample = declare(
        MappingProxyType({"u": ["p.v"]}), compute_uncertainty=lambda: 0.1
    )
    # An uncertainty beside a value nobody entered waits: entered.
    sample.p.value = 2.0
    sample.compute()
    assert sample.p.uncertainty == 0.1
    # A dotted name that is neither a value nor a table says both.
    with pytest.raises(KeyError, match="neither a value nor a table"):
        declare(["nope.v"], compute_value=lambda: 1.0)


def test_reading_never_computes_on_any_thread():
    # The guard held only on the thread that imported samplekit: the flag was a
    # thread-local, and a worker thread read with the default — a filter or an
    # export there ran formulas that take minutes.
    import threading

    runs = []

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.z = sk.Property(compute_value=self._z, depends_on=["a"])
            super().__init__(path, name=name)

        def _z(self):
            runs.append(threading.current_thread().name)
            return self.a.value * 2

    seen = {}

    def read():
        sample = Model(name="s")
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            seen["value"] = sample.z.value
        seen["state"] = sample.z.state

    worker = threading.Thread(target=read)
    worker.start()
    worker.join()
    assert seen == {"value": None, "state": "never_computed"}
    assert runs == []


def test_a_formula_cannot_change_the_value_it_is_producing():
    # Writing to its own property while computing panicked on the borrow: a
    # PanicException no `except Exception` catches, and one that left reading
    # computing for the rest of the process.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.z = sk.Property(compute_value=self._z, depends_on=["a"])
            super().__init__(path, name=name)

        def _z(self):
            self.z.unit = "mm"
            return 1.0

    sample = Model(name="s")
    with pytest.raises(ValueError, match="a formula writes only the value it returns"):
        sample.compute()
    assert sample.z.state == "failed"

    # And reading still computes nothing afterwards, in this sample or another.
    other = Model(name="t")
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        assert other.z.value is None


def test_a_row_formula_printing_its_table_raises_an_ordinary_error():
    # A table not yet in a sample, resolved by computing one of its cells: a
    # row formula printing it found the table's state held and panicked — a
    # PanicException, which no `except Exception` catches.
    def y(row):
        repr(table)
        return {"y": row["x"] * 2}

    table = sk.Table(
        {
            "i": sk.Property(),
            "x": sk.Property(),
            "y": sk.Property(compute=y, depends_on=["row.x"]),
        },
        index="i",
    )
    table.add(i=1, x=3.0)
    with pytest.raises(Exception) as raised:
        table.at(1, "y").compute()
    assert isinstance(raised.value, RuntimeError), type(raised.value)


def test_save_all_refuses_two_samples_bound_for_one_file(tmp_path):
    # Conflicts were looked for on disk, and a collision inside the list is
    # not on disk yet: the second sample was written over the first.
    first = sk.Sample(name="S1")
    second = sk.Sample(name="S1")
    first.operator = "A"
    second.operator = "B"
    samples = sk.SampleList([first, second])
    with pytest.raises(FileExistsError, match="one file"):
        samples.save_all(tmp_path)
    assert list(tmp_path.iterdir()) == []


def test_not_current_covers_tables_and_a_cell_says_it_failed():
    # The reason for this — a script seeing broken formulas — failed for
    # every table: not_current iterated properties only, and a cell's failure
    # always answered None.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.t = sk.Table(
                {
                    "i": sk.Property(),
                    "x": sk.Property(),
                    "g": sk.Property(compute=lambda row: {"g": 1 / row["x"]}, depends_on=["row.x"]),
                },
                index="i",
            )
            super().__init__(path, name=name)

    sample = Model(name="s")
    sample.t.add(i=1, x=0.0)
    assert sample.not_current() == {"t.g": "never computed"}
    with pytest.raises(ZeroDivisionError):
        sample.compute()
    assert sample.not_current() == {"t.g": "failed"}
    assert "ZeroDivisionError" in sample.t.at(1, "g").failure


def test_a_failed_computation_says_which_value_and_which_sample():
    # A bare ZeroDivisionError out of a computation of many values, or of a
    # list of many samples, named nothing a script could act on. The
    # exception keeps its type; a note says where.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(0.0)
            self.inverse = sk.Property(compute_value=lambda: 1 / self.a.value, depends_on=["a"])
            super().__init__(path, name=name)

    sample = Model(name="S2")
    with pytest.raises(ZeroDivisionError) as raised:
        sample.compute()
    assert any("'inverse'" in note for note in raised.value.__notes__)

    samples = sk.SampleList([Model(name="S2")])
    with pytest.raises(ZeroDivisionError) as raised:
        samples.compute()
    notes = raised.value.__notes__
    assert any("'inverse'" in note for note in notes)
    assert any("sample 'S2'" in note for note in notes)


def test_a_formula_returning_an_array_is_told_what_it_returned():
    # numpy's `__index__` refused it in numpy's words, naming nothing the
    # formula did. One element is its number; more is said as it is.
    np = pytest.importorskip("numpy")

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.one = sk.Property(compute_value=lambda: np.array([3.5]), depends_on=[])
            self.many = sk.Property(compute_value=lambda: np.array([1.0, 2.0]), depends_on=[])
            super().__init__(path, name=name)

    sample = Model(name="s")
    sample.compute("one")
    assert sample.one.value == 3.5
    with pytest.raises(TypeError, match="array of shape"):
        sample.compute("many")


def test_a_model_declares_no_precision():
    # A precision is the project's to declare: the keyword is refused with where
    # it went, rather than accepted and dropped.
    with pytest.raises(TypeError, match=r"\[property\.<name>\] precision.*\.samplekitrc"):
        sk.Property(value=1.0, precision=".3f")
    # A column's section quoted, as the configuration writes it: unquoted,
    # TOML reads `table.column` as a table inside another.
    with pytest.raises(TypeError,
                       match=r'\[property\."<table>\.<column>"\] precision.*\.samplekitrc'):
        sk.Property(unit="degC", precision=".1f")
    # None says nothing, and is what a keyword left out passes.
    assert sk.Property(value=1.0, precision=None).value == 1.0


def test_a_model_declares_no_symbol():
    # A symbol is the project's to declare, as a precision is: the keyword is
    # refused with where it went, a column's in the form the configuration gives
    # it.
    with pytest.raises(TypeError, match=r'\[property\.<name>\] symbol = "m" in \.samplekitrc'):
        sk.Property(value=1.0, symbol="m")
    with pytest.raises(TypeError,
                       match=r'\[property\."<table>\.<column>"\] symbol = "T" in \.samplekitrc'):
        sk.Property(unit="degC", symbol="T")
    # None says nothing, and is what a keyword left out passes.
    assert sk.Property(value=1.0, symbol=None).value == 1.0
    assert sk.Property(unit="degC", symbol=None).unit == "degC"


def test_a_list_carries_on_past_a_failure_and_raises_them_together(tmp_path):
    # As the command line does, every sample is computed and what failed is
    # raised at the end — one as itself, several as a group.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None, a=1.0):
            self.a = sk.Property(a)
            self.inverse = sk.Property(compute_value=lambda: 1 / self.a.value, depends_on=["a"])
            super().__init__(path, name=name)

    samples = sk.SampleList([Model(name="Z1", a=0.0), Model(name="OK", a=4.0),
                             Model(name="Z2", a=0.0)])
    with pytest.raises(ExceptionGroup) as raised:
        samples.compute()
    group = raised.value
    assert "2 samples" in str(group)
    assert [type(error) for error in group.exceptions] == [ZeroDivisionError] * 2
    assert any("sample 'Z1'" in note for note in group.exceptions[0].__notes__)
    assert any("sample 'Z2'" in note for note in group.exceptions[1].__notes__)
    # The one in between was computed, not skipped.
    assert samples["OK"].inverse.value == 0.25

    # One failure is itself, not a group of one.
    single = sk.SampleList([Model(name="Z3", a=0.0), Model(name="OK2", a=2.0)])
    with pytest.raises(ZeroDivisionError):
        single.compute()
    assert single["OK2"].inverse.value == 0.5


def test_an_interruption_is_not_a_failure(tmp_path):
    # Ctrl-C in a formula stops the list at once, and the value it interrupted
    # is never computed — not failed, and nothing saved says so.
    started = []

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(1.0)
            self.slow = sk.Property(compute_value=self._slow, depends_on=["a"])
            super().__init__(path, name=name)

        def _slow(self):
            started.append(self.name)
            raise KeyboardInterrupt

    first, second = Model(name="I1"), Model(name="I2")
    with pytest.raises(KeyboardInterrupt):
        sk.SampleList([first, second]).compute()
    assert first.not_current() == {"slow": "never computed"}
    assert first.slow.failure is None
    # The list stopped: the second was never reached.
    assert started == ["I1"]
    assert second.not_current() == {"slow": "never computed"}
    path = tmp_path / "i1.md"
    first.save(path)
    assert "failed" not in path.read_text()


# ------------------------------------------------------------------ figures

FIGURE_RC = (
    "schema_version = 1\n"
    "[figure.diameter_volume]\nx = \"volume\"\ny = \"diameter\"\ngroup = \"alloy\"\n"
    "[figure.hv]\nkind = \"line\"\nx = \"measurements.f\"\ny = \"measurements.hv\"\n"
)


def figure_samples(tmp_path, rc=FIGURE_RC):
    (tmp_path / ".samplekitrc").write_text(rc)
    paths = []
    for name, diameter, alloy in [("C1", 12.0, "cobalt"), ("C2", 10.0, "chrome"),
                                 ("C3", 11.0, "cobalt")]:
        path = tmp_path / f"{name}.md"
        path.write_text(
            f"---\nschema_version: 1\nname: {name}\nalloy: {alloy}\nproperties:\n"
            f"  diameter: {{v: {diameter}, u: 0.1, unit: g}}\n  volume: {{v: {diameter / 3}, unit: L}}\n"
            "tables:\n  measurements:\n    index: f\n    columns:\n      f: {unit: porosity}\n"
            "      hv: {}\n    rows:\n      - {f: 8.0, hv: {v: 5.1, u: 0.05}}\n"
            "      - {f: 9.0, hv: {v: 5.3, u: 0.05}}\n---\n"
        )
        paths.append(path)
    return paths


class Plotted(sk.Sample):
    @sk.figure
    def mass_bar(self, ax):
        ax.bar(["diameter"], [self.diameter.value])

    @sk.figure(subplots=(1, 2))
    def pair(self, axes):
        axes[0].plot([0, 1], [0, 1])
        axes[1].plot([0, 1], [1, 0])

    @sk.figure
    @classmethod
    def diameters(cls, samples, ax):
        cls.received = [sample.name for sample in samples]
        ax.bar([sample.name for sample in samples], [sample.diameter.value for sample in samples])


def test_a_figure_method_is_marked_as_one():
    from samplekit._figures import figures_of

    found = figures_of(Plotted)
    assert set(found) == {"mass_bar", "pair", "diameters"}
    assert not found["mass_bar"].collection
    assert found["diameters"].collection
    assert found["pair"].subplots == (1, 2)


def test_a_model_figure_draws_into_the_axes_it_is_given(tmp_path):
    path = figure_samples(tmp_path)[0]
    sample = Plotted(path)
    (one,) = sk.plot(sample, "mass_bar", output=tmp_path / "one.png")
    assert len(one.axes) == 1 and len(one.axes[0].patches) == 1
    (two,) = sk.plot(sample, "pair", output=tmp_path / "two.png")
    assert len(two.axes) == 2
    assert all(len(ax.lines) == 1 for ax in two.axes)


def test_a_collection_figure_receives_the_list(tmp_path):
    samples = sk.SampleList([Plotted(path) for path in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "diameters", output=tmp_path / "all.png")
    assert Plotted.received == ["C1", "C2", "C3"]
    assert len(drawn.axes[0].patches) == 3


def test_plot_writes_the_format_its_extension_names(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    for name, magic in [("f.pdf", b"%PDF"), ("f.svg", b"<?xml"), ("f.png", b"\x89PNG")]:
        sk.plot(samples, "diameter_volume", output=tmp_path / name)
        assert (tmp_path / name).read_bytes().startswith(magic), name


def test_plot_refuses_to_overwrite_unless_asked(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    target = tmp_path / "f.png"
    target.write_text("kept")
    with pytest.raises(FileExistsError):
        sk.plot(samples, "diameter_volume", output=target)
    assert target.read_text() == "kept"
    sk.plot(samples, "diameter_volume", output=target, overwrite=True)
    assert target.read_bytes().startswith(b"\x89PNG")


def test_a_declared_figure_draws_a_series_per_group_with_error_bars(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "diameter_volume", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    labels = sorted(ax.get_legend_handles_labels()[1])
    assert labels == ["chrome", "cobalt"]
    # Numbers group in their order, not as text: 3.3 before 12.0.
    (numeric,) = sk.plot(samples, x="volume", y="diameter", group="diameter",
                         output=tmp_path / "n.png")
    assert numeric.axes[0].get_legend_handles_labels()[1] == ["10", "11", "12"]
    # One errorbar container per series, each carrying its vertical bars.
    assert len(ax.containers) == 2
    assert all(container.has_yerr for container in ax.containers)
    assert ax.get_xlabel() == "volume [L]"
    assert ax.get_ylabel() == "diameter [g]"
    # No title unless one is declared: a report gives the caption.
    assert ax.get_title() == ""


def test_a_declared_figure_over_a_table_draws_a_curve_per_sample(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "hv", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    assert sorted(ax.get_legend_handles_labels()[1]) == ["C1", "C2", "C3"]
    assert ax.get_xlabel().startswith("measurements.f")


def test_an_export_of_a_figure_draws_it_to_its_file(tmp_path):
    # As samplekit export does: the figure drawn to the export's output, its
    # query narrowing the list.
    rc = FIGURE_RC + (
        "[query.dark]\nfilter = \"alloy == cobalt\"\n"
        "[export.curves]\nfigure = \"diameter_volume\"\noutput = \"figs/curves.png\"\n"
        "query = \"dark\"\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    entry = samples.project.exports.curves
    assert (entry.figure, entry.profile, entry.format) == ("diameter_volume", None, None)
    written = samples.export("curves")
    assert written == tmp_path / "figs" / "curves.png"
    assert written.read_bytes().startswith(b"\x89PNG")
    with pytest.raises(FileExistsError):
        samples.export("curves")
    samples.export(entry, output=tmp_path / "other.png")
    assert (tmp_path / "other.png").read_bytes().startswith(b"\x89PNG")


def _data_line(ax):
    """The line of a series drawn with its error bars: its points."""
    return ax.containers[0].lines[0]


def test_what_the_project_declares_wins_over_samplekits_choice(tmp_path):
    # Undeclared: a round marker, caps of 3 points.
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "hv", output=tmp_path / "a.png")
    assert _data_line(drawn.axes[0]).get_marker() == "o"
    assert drawn.axes[0].containers[0].lines[1][0].get_markersize() == 6.0
    # Declared: the project's marker, caps and outside legend's size.
    rc = FIGURE_RC + (
        "[matplotlib]\n\"lines.marker\" = \"None\"\n\"errorbar.capsize\" = 0\n"
        "\"legend.fontsize\" = 15\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "hv", output=tmp_path / "b.png", legend="outside")
    ax = drawn.axes[0]
    assert _data_line(ax).get_marker() == "None"
    assert ax.containers[0].lines[1] == ()
    assert ax.get_legend().get_texts()[0].get_fontsize() == 15
    # A scatter keeps no line between its points.
    (scatter,) = sk.plot(samples, "diameter_volume", output=tmp_path / "c.png")
    assert _data_line(scatter.axes[0]).get_linestyle() == "None"


def test_a_marker_in_the_property_cycle_is_the_projects(tmp_path):
    rc = FIGURE_RC + (
        "[matplotlib]\n\"axes.prop_cycle\" = \"cycler(color=['k', 'r', 'b'], "
        "marker=['s', '^', 'v'])\"\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "hv", output=tmp_path / "f.png")
    markers = [container.lines[0].get_marker() for container in drawn.axes[0].containers]
    assert markers == ["s", "^", "v"]


def test_style_sheets_and_fonts_are_the_projects(tmp_path):
    import matplotlib

    (tmp_path / "paper.mplstyle").write_text("lines.linewidth: 4\n")
    (tmp_path / "fonts").mkdir()
    shipped = pathlib.Path(matplotlib.get_data_path()) / "fonts" / "ttf" / "DejaVuSerif.ttf"
    (tmp_path / "fonts" / "Serif.ttf").write_bytes(shipped.read_bytes())
    rc = FIGURE_RC + (
        "[matplotlib]\nstyle = [\"ggplot\", \"paper.mplstyle\"]\n"
        "fonts = [\"fonts/*.ttf\"]\n\"lines.linewidth\" = 2\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    sheets = samples.project.matplotlib["style"]
    assert sheets[0] == "ggplot"
    assert pathlib.Path(sheets[1]).resolve() == (tmp_path / "paper.mplstyle").resolve()
    (drawn,) = sk.plot(samples, "hv", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    # The sheets in order, then the section's own keys, which win.
    assert matplotlib.colors.to_hex(ax.get_facecolor()) == "#e5e5e5"
    assert _data_line(ax).get_linewidth() == 2
    for written, said in [
        ("style = \"nonesuch\"", "matplotlib has no style sheet 'nonesuch'"),
        ("style = \"gone.mplstyle\"", "gone.mplstyle is not there"),
        ("fonts = [\"nothing/*.otf\"]", "finds no file"),
    ]:
        rc = FIGURE_RC + f"[matplotlib]\n{written}\n"
        samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
        with pytest.raises(ValueError, match=re.escape(said)):
            sk.plot(samples, "hv", output=tmp_path / "g.png", overwrite=True)


def test_a_figures_own_settings_apply_to_it_alone(tmp_path):
    rc = FIGURE_RC.replace(
        "y = \"measurements.hv\"\n",
        "y = \"measurements.hv\"\nmatplotlib = { \"lines.marker\" = \"s\" }\n",
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    assert samples.project.figures.hv.matplotlib == {"lines.marker": "s"}
    (own,) = sk.plot(samples, "hv", output=tmp_path / "a.png")
    (other,) = sk.plot(samples, "diameter_volume", output=tmp_path / "b.png")
    assert _data_line(own.axes[0]).get_marker() == "s"
    assert _data_line(other.axes[0]).get_marker() == "o"


def test_a_model_figures_own_settings_apply_to_it(tmp_path):
    class Thick(sk.Sample):
        @sk.figure(matplotlib={"lines.linewidth": 5})
        def thick(self, ax):
            ax.plot([0, 1], [0, 1])

    samples = sk.SampleList([Thick(p) for p in figure_samples(tmp_path)[:1]])
    (drawn,) = sk.plot(samples, "thick", output=tmp_path / "f.png")
    assert drawn.axes[0].lines[0].get_linewidth() == 5
    with pytest.raises(TypeError, match="a dict of matplotlib's settings"):
        sk.figure(matplotlib="lines.linewidth: 5")(lambda self, ax: None)


def test_a_figure_name_both_declared_and_in_the_model_is_refused(tmp_path):
    rc = FIGURE_RC + "[figure.mass_bar]\nx = \"volume\"\ny = \"diameter\"\n"
    samples = sk.SampleList([Plotted(p) for p in figure_samples(tmp_path, rc)])
    with pytest.raises(ValueError, match="both"):
        sk.plot(samples[0], "mass_bar", output=tmp_path / "f.png")
    with pytest.raises(KeyError, match="diameter_volume"):
        sk.plot(samples, "diameter_volum", output=tmp_path / "f.png")


def test_an_instance_figure_over_several_samples_with_an_output_is_refused(tmp_path):
    samples = sk.SampleList([Plotted(p) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="one sample"):
        sk.plot(samples, "mass_bar", output=tmp_path / "f.png")
    assert not (tmp_path / "f.png").exists()


def test_a_window_over_ssh_without_a_display_is_refused(tmp_path, monkeypatch):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    monkeypatch.setenv("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")
    monkeypatch.delenv("DISPLAY", raising=False)
    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    with pytest.raises(RuntimeError, match="output="):
        sk.plot(samples, "diameter_volume")


def test_a_window_the_workbench_cannot_open_says_its_file_is_asked(tmp_path, monkeypatch):
    # Drawn from the workbench, the advice named `-o`, an option of the command
    # line.
    from samplekit import _figures
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    monkeypatch.setenv("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")
    monkeypatch.delenv("DISPLAY", raising=False)
    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    monkeypatch.setattr(_figures, "COMMAND_LINE", True)
    monkeypatch.setattr(_figures, "WORKBENCH", True)
    with pytest.raises(RuntimeError, match="the workbench asks its file next"):
        sk.plot(samples, "diameter_volume")


def test_the_project_lists_its_figures(tmp_path):
    rc = FIGURE_RC.replace("group = \"alloy\"\n", "group = \"alloy\"\nquery = \"light\"\n")
    rc += "[query.light]\nfilter = \"diameter < 12\"\n"
    sample = sk.Sample(figure_samples(tmp_path, rc)[0], model=False)
    declared = sample.project.figures.diameter_volume
    assert (declared.kind, declared.x, declared.y) == ("scatter", "volume", "diameter")
    assert (declared.group, declared.query) == ("alloy", "light")
    assert list(sample.project.figures) == ["diameter_volume", "hv"]


STYLED_RC = FIGURE_RC + (
    "[style.figure]\n[style.math]\n"
    "[unit.g]\nfigure = '$\\mathrm{g}$'\nmath = '\\si{\\gram}'\n"
    "[property.diameter]\nsymbol = \"m\"\nsymbol_figure = '$m$'\n"
)


def test_an_axis_is_labelled_in_the_figure_style(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False)
                             for p in figure_samples(tmp_path, STYLED_RC)])
    (plain,) = sk.plot(samples, "diameter_volume", output=tmp_path / "p.png")
    assert plain.axes[0].get_ylabel() == "m [g]"
    (styled,) = sk.plot(samples, "diameter_volume", style="figure", output=tmp_path / "s.png")
    assert styled.axes[0].get_ylabel() == "$m$ [$\\mathrm{g}$]"
    (given,) = sk.plot(samples, "diameter_volume", y_label="Diameter", title="T",
                       output=tmp_path / "g.png")
    assert given.axes[0].get_ylabel() == "Diameter"
    assert given.axes[0].get_title() == "T"


def test_a_table_cell_is_an_axis(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="diameter", y="measurements.hv[8.0]", output=tmp_path / "c.png")
    ax = drawn.axes[0]
    (container,) = ax.containers
    assert container.has_yerr
    assert len(container.lines[0].get_xdata()) == 3


def test_a_label_with_latex_outside_math_is_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False)
                             for p in figure_samples(tmp_path, STYLED_RC)])
    with pytest.raises(ValueError, match=r"\\si\{\\gram\}"):
        sk.plot(samples, "diameter_volume", style="math", output=tmp_path / "m.png")
    assert not (tmp_path / "m.png").exists()


def test_a_group_colours_the_curves_of_a_table(tmp_path):
    # Grouped, a curve takes its group's colour, and the legend names each group
    # once, in the groups' order.
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "hv", group="alloy", output=tmp_path / "g.png")
    ax = drawn.axes[0]
    assert ax.get_legend_handles_labels()[1] == ["chrome", "cobalt"]
    # Three curves, two colours: the two cobalt samples share theirs.
    colours = [container.lines[0].get_color() for container in ax.containers]
    assert len(colours) == 3 and len(set(colours)) == 2


def test_the_projects_matplotlib_settings_shape_every_figure(tmp_path):
    import matplotlib

    rc = FIGURE_RC + '[matplotlib]\n"axes.labelsize" = 21\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    assert samples.project.matplotlib == {"axes.labelsize": 21}
    before = matplotlib.rcParams["axes.labelsize"]
    (drawn,) = sk.plot(samples, "diameter_volume", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    assert ax.xaxis.label.get_size() == 21 and ax.yaxis.label.get_size() == 21
    assert matplotlib.rcParams["axes.labelsize"] == before


def test_an_unknown_matplotlib_setting_is_refused_naming_the_nearest(tmp_path):
    for written, said in [('"font.sise" = 13', "font.size"), ('"axes.grid" = "maybe"', "axes.grid")]:
        folder = tmp_path / said
        folder.mkdir()
        rc = FIGURE_RC + f"[matplotlib]\n{written}\n"
        samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(folder, rc)])
        with pytest.raises(ValueError, match=re.escape(said)):
            sk.plot(samples, "diameter_volume", output=folder / "f.png")


def test_project_style_false_leaves_matplotlib_its_own(tmp_path):
    import matplotlib

    rc = FIGURE_RC + '[matplotlib]\n"axes.labelsize" = 21\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "diameter_volume", output=tmp_path / "f.png", project_style=False)
    default = matplotlib.font_manager.FontProperties(
        size=matplotlib.rcParams["axes.labelsize"]).get_size_in_points()
    assert drawn.axes[0].xaxis.label.get_size() == default


def test_limits_scales_and_aspect_set_the_axes(tmp_path):
    rc = FIGURE_RC.replace('group = "alloy"\n',
                           'group = "alloy"\nx_limits = [0, 10]\naspect = "equal"\n')
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    declared = samples.project.figures.diameter_volume
    assert declared.x_limits == (0.0, 10.0) and declared.aspect == "equal"
    (drawn,) = sk.plot(samples, "diameter_volume", y_scale="log", y_limits=(1, None),
                       output=tmp_path / "f.png")
    ax = drawn.axes[0]
    assert ax.get_xlim() == (0.0, 10.0)
    assert ax.get_ylim()[0] == 1.0
    assert ax.get_yscale() == "log"
    assert ax.get_aspect() == 1.0


def test_a_bound_a_log_axis_cannot_take_is_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="at or below zero"):
        sk.plot(samples, "diameter_volume", y_scale="log", y_limits=(0, 20),
                output=tmp_path / "f.png")
    assert not (tmp_path / "f.png").exists()


def test_a_group_no_sample_knows_is_refused_naming_the_nearest(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(KeyError, match="did you mean: alloy"):
        sk.plot(samples, x="volume", y="diameter", group="alloi", output=tmp_path / "f.png")
    with pytest.raises(KeyError, match="names a column"):
        sk.plot(samples, "diameter_volume", group="measurements.hv", output=tmp_path / "f.png")
    assert not (tmp_path / "f.png").exists()


def test_an_output_without_a_format_matplotlib_writes_is_refused(tmp_path):
    # matplotlib gives a path without an extension one of its own: the file
    # named would not be the file written, nor the one overwrite= guards.
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (tmp_path / "fig.png").write_text("precious")
    with pytest.raises(ValueError, match="has none"):
        sk.plot(samples, "diameter_volume", output=tmp_path / "fig")
    with pytest.raises(ValueError, match="writes no '.xyz'"):
        sk.plot(samples, "diameter_volume", output=tmp_path / "fig.xyz")
    assert (tmp_path / "fig.png").read_text() == "precious"


def test_one_axis_holds_one_unit(tmp_path):
    figure_samples(tmp_path)
    (tmp_path / "C4.md").write_text(
        "---\nschema_version: 1\nname: C4\nalloy: cobalt\nproperties:\n"
        "  diameter: {v: 0.011, unit: kg}\n  volume: {v: 4.0, unit: L}\n---\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in sorted(tmp_path.glob("*.md"))])
    with pytest.raises(ValueError, match="in g in C1 and in kg in C4"):
        sk.plot(samples, x="volume", y="diameter", output=tmp_path / "f.png")


def test_one_axis_holds_the_unit_a_project_declares(tmp_path):
    # A file writing no unit is in the one [property.*] declares: kilograms
    # beside grams on one axis are refused, as the table refuses them.
    figure_samples(tmp_path, FIGURE_RC + '\n[property.diameter]\nunit = "kg"\n')
    (tmp_path / "C4.md").write_text(
        "---\nschema_version: 1\nname: C4\nalloy: cobalt\nproperties:\n"
        "  diameter: {v: 0.011}\n  volume: {v: 4.0, unit: L}\n---\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in sorted(tmp_path.glob("*.md"))])
    with pytest.raises(ValueError, match="in g in C1 and in kg in C4"):
        sk.plot(samples, x="volume", y="diameter", output=tmp_path / "f.png")


def test_an_uncertainty_axis_is_labelled_as_one(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="diameter", y="diameter.u", output=tmp_path / "f.png")
    assert drawn.axes[0].get_xlabel() == "diameter [g]"
    assert drawn.axes[0].get_ylabel() == "u(diameter) [g]"


def test_limits_are_finite_and_a_log_axis_takes_neither_bound_at_zero(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    for limits in [(float("nan"), 5), (0, float("inf")), (3, 3)]:
        with pytest.raises(ValueError):
            sk.plot(samples, "diameter_volume", x_limits=limits, output=tmp_path / "f.png")
    with pytest.raises(ValueError, match="has -1"):
        sk.plot(samples, "diameter_volume", y_scale="log", y_limits=(None, -1),
                output=tmp_path / "f.png")


def test_a_bar_carries_both_uncertainties(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="diameter", y="volume", kind="bar", output=tmp_path / "f.png")
    from matplotlib.container import BarContainer

    (bars,) = [c for c in drawn.axes[0].containers if isinstance(c, BarContainer)]
    assert bars.errorbar is not None and bars.errorbar.has_xerr


def test_an_empty_title_or_label_draws_none(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "diameter_volume", title="", y_label="",
                       output=tmp_path / "f.png")
    assert drawn.axes[0].get_title() == ""
    assert drawn.axes[0].get_ylabel() == ""


def test_a_backend_in_the_project_settings_is_refused(tmp_path):
    rc = FIGURE_RC + '[matplotlib]\nbackend = "pdf"\n'
    (tmp_path / ".samplekitrc").write_text(rc)
    with pytest.raises(Exception, match="MPLBACKEND"):
        sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])


class Base(sk.Sample):
    @sk.figure
    def fig(self, ax):
        ax.set_title("Base.fig")


class Sub(Base):
    @sk.figure
    def fig(self, ax):
        ax.set_title("Sub.fig")


class Plain(Base):
    def fig(self, ax):
        ax.set_title("Plain.fig")


def test_each_sample_draws_its_own_models_figure(tmp_path):
    from samplekit._figures import figures_of

    assert "fig" not in figures_of(Plain)
    paths = figure_samples(tmp_path)
    samples = sk.SampleList([Base(paths[0]), Sub(paths[1])])
    titles = []
    for sample in samples:
        (figure_,) = sk.plot(sample, "fig", output=tmp_path / f"{sample.name}.png")
        titles.append(figure_.axes[0].get_title())
    assert titles == ["Base.fig", "Sub.fig"]
    with pytest.raises(ValueError, match="declares no figure 'fig'"):
        sk.plot(sk.SampleList([Base(paths[0]), sk.Sample(paths[1], model=False)]), "fig",
                output=tmp_path / "x.png")


def test_a_classmethod_under_the_decorator_is_refused():
    with pytest.raises(TypeError, match="above @classmethod"):
        class Reversed(sk.Sample):
            @classmethod
            @sk.figure
            def everything(cls, samples, ax):
                pass

        from samplekit._figures import figures_of

        figures_of(Reversed)
    with pytest.raises(TypeError, match="static method"):
        sk.figure(staticmethod(lambda ax: None))


class Pyplotted(sk.Sample):
    @sk.figure
    def habit(self, ax):
        import matplotlib.pyplot as plt

        plt.plot([0, 1], [0, 1])

    @sk.figure
    def broken(self, ax):
        raise OSError("reference-curve.csv")


def test_a_figure_drawn_with_pyplot_is_refused_and_leaves_nothing_open(tmp_path):
    # Drawn off screen, as the tests beside it are: a window of the desktop's
    # backend failed in Pillow before the refusal was reached.
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    path = figure_samples(tmp_path)[0]
    before = plt.get_fignums()
    with pytest.raises(RuntimeError, match="not into the axes it was given"):
        sk.plot(Pyplotted(path), "habit", output=tmp_path / "f.png")
    assert plt.get_fignums() == before
    assert not (tmp_path / "f.png").exists()


def test_what_a_figure_raises_is_marked_as_its_own(tmp_path):
    from samplekit._figures import raised_by_the_figure

    path = figure_samples(tmp_path)[0]
    with pytest.raises(OSError) as raised:
        sk.plot(Pyplotted(path), "broken", output=tmp_path / "f.png")
    assert raised_by_the_figure(raised.value)
    with pytest.raises(KeyError) as refused:
        sk.plot(Pyplotted(path), "nope", output=tmp_path / "f.png")
    assert not raised_by_the_figure(refused.value)


def test_a_saved_failure_keeps_its_traceback_beside_the_project(tmp_path):
    # A failure mark has its log beside it, whichever wrote the file.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")

    class Broken(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(0.0)
            self.inverse = sk.Property(compute_value=self._inverse, depends_on=["a"])
            super().__init__(path, name=name)

        def _inverse(self):
            return 1 / self.a.value

    sample = Broken(name="B1")
    with pytest.raises(ZeroDivisionError):
        sample.compute()
    path = tmp_path / "sub" / "b1.md"
    path.parent.mkdir()
    sample.save(path)
    assert "ZeroDivisionError" in path.read_text()
    log = tmp_path / ".samplekit" / "failures" / "sub" / "b1.log"
    text = log.read_text()
    assert text.startswith("## inverse")
    assert "1 / self.a.value" in text and "ZeroDivisionError" in text


def test_an_empty_selection_keeps_its_project(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    empty = samples.filter("diameter > 1000")
    assert len(empty) == 0
    assert empty.project.root == samples.project.root
    assert "diameter_volume" in empty.project.figures
    with pytest.raises(ValueError, match="no sample to draw"):
        sk.plot(empty, "diameter_volume", output=tmp_path / "f.png")


def test_two_columns_writing_one_header_are_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="'diameter' writes its uncertainty already, as 'diameter_uncertainty'"):
        samples.to_dict(columns=["diameter", "diameter.u"])
    cells = samples.to_dict(columns=["measurements.hv[8.0].u", "measurements.f[8.0].unit"])
    assert list(cells) == ["measurements.hv[8.0]_uncertainty", "measurements.f[8.0]_unit"]


def test_a_sample_whose_formula_is_its_own_method_is_freed():
    # A formula held strongly through Rust was a cycle the collector could not
    # see: the sample was never freed.
    import gc
    import weakref

    class Doubled(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.b = sk.Property(compute_value=self._double, depends_on=["a"])
            super().__init__(path, name=name)

        def _double(self):
            return self.a.value * 2

    sample = Doubled(name="D")
    sample.compute()
    assert sample.b.value == 4.0
    gone = weakref.ref(sample)
    del sample
    gc.collect()
    assert gone() is None


def many_samples(tmp_path, count=12, rc=FIGURE_RC):
    (tmp_path / ".samplekitrc").write_text(rc)
    for index in range(count):
        (tmp_path / f"M{index:02}.md").write_text(
            f"---\nschema_version: 1\nname: M{index:02}\nalloy: m{index % 3}\n"
            f"batch: b{index}\nproperties:\n  diameter: {{v: {10 + index}.5, unit: g}}\n"
            f"  volume: {{v: {index}.0, unit: L}}\n---\n"
        )
    return sk.SampleList([sk.Sample(p, model=False) for p in sorted(tmp_path.glob("M*.md"))])


def test_what_a_figure_leaves_out_is_said(tmp_path):
    samples = many_samples(tmp_path, 4)
    with pytest.warns(UserWarning, match="1 of 4 samples not drawn: volume at or below zero"):
        (drawn,) = sk.plot(samples, x="volume", y="diameter", x_scale="log",
                           output=tmp_path / "f.png")
    assert len(drawn.axes[0].lines) >= 1


def test_a_numeric_group_of_many_values_is_a_colour_scale(tmp_path):
    samples = many_samples(tmp_path)
    (drawn,) = sk.plot(samples, x="volume", y="diameter", group="diameter", output=tmp_path / "f.png")
    assert len(drawn.axes) == 2  # the axes and the colour bar
    assert drawn.axes[0].get_legend() is None
    with pytest.raises(ValueError, match="makes 12 groups"):
        sk.plot(samples, x="volume", y="diameter", group="batch", output=tmp_path / "g.png")


def test_a_groups_legend_reads_in_its_precision_and_is_titled(tmp_path):
    rc = FIGURE_RC + '[property.volume]\nprecision = ".0f"\nsymbol = "V"\n'
    samples = many_samples(tmp_path, 3, rc)
    (drawn,) = sk.plot(samples, x="diameter", y="diameter", group="volume", output=tmp_path / "f.png")
    legend = drawn.axes[0].get_legend()
    assert [text.get_text() for text in legend.get_texts()] == ["0", "1", "2"]
    assert legend.get_title().get_text() == "V [L]"


def test_bars_stand_side_by_side_and_one_series_takes_one_bar_per_x(tmp_path):
    samples = many_samples(tmp_path, 6)
    (drawn,) = sk.plot(samples, x="alloy", y="diameter", group="batch", kind="bar",
                       output=tmp_path / "f.png")
    ticks = [label.get_text() for label in drawn.axes[0].get_xticklabels()]
    assert ticks == ["m0", "m1", "m2"]
    with pytest.raises(ValueError, match="two bars at alloy = m0"):
        sk.plot(samples, x="alloy", y="diameter", kind="bar", output=tmp_path / "g.png")


def test_a_box_plot_draws_the_values_per_category(tmp_path):
    samples = many_samples(tmp_path, 6)
    (drawn,) = sk.plot(samples, x="alloy", y="diameter", kind="box", output=tmp_path / "f.png")
    ticks = [label.get_text() for label in drawn.axes[0].get_xticklabels()]
    # Each with how many samples its box holds.
    assert ticks == ["m0\n(2)", "m1\n(2)", "m2\n(2)"]
    with pytest.raises(ValueError, match="box plot"):
        sk.plot(samples, x="alloy", y="diameter", kind="box", group="batch",
                output=tmp_path / "g.png")


def test_a_legend_is_placed_and_a_figure_sized(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    # Centimetres, converted for matplotlib.
    (drawn,) = sk.plot(samples, "diameter_volume", legend="none", figsize=(10.16, 7.62),
                       output=tmp_path / "f.png")
    assert drawn.axes[0].get_legend() is None
    assert tuple(drawn.get_size_inches()) == pytest.approx((4.0, 3.0))
    with pytest.raises(ValueError, match="legend is one of"):
        sk.plot(samples, "diameter_volume", legend="left-ish", output=tmp_path / "g.png")


def test_a_figure_size_written_as_text_is_in_centimetres_too(tmp_path):
    # `"figure.figsize" = "15, 10"` reached matplotlib as inches.
    rc = FIGURE_RC + '[matplotlib]\n"figure.figsize" = "10.16, 7.62"\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "diameter_volume", output=tmp_path / "f.png")
    assert tuple(drawn.get_size_inches()) == pytest.approx((4.0, 3.0))
    # A size out of reason is said: centimetres are meant.
    with pytest.warns(UserWarning, match="sizes are in centimetres"):
        sk.plot(samples, "diameter_volume", figsize=(160, 20), output=tmp_path / "g.png")


def test_a_project_names_the_style_its_figures_read_in(tmp_path):
    rc = (FIGURE_RC + "[render]\nfigure_style = \"figure\"\n[style.figure]\n"
          "[property.diameter]\nsymbol_figure = '$m$'\n")
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    assert samples.project.render.figure_style == "figure"
    (drawn,) = sk.plot(samples, "diameter_volume", output=tmp_path / "f.png")
    assert drawn.axes[0].get_ylabel() == "$m$ [g]"


class Sized(sk.Sample):
    @sk.figure(figsize=(7.62, 5.08))    # centimetres: 3 by 2 inches
    def small(self, ax):
        ax.plot([0, 1], [0, 1])


def test_a_figure_of_one_sample_writes_a_file_per_sample(tmp_path):
    paths = figure_samples(tmp_path)
    samples = sk.SampleList([Sized(path) for path in paths])
    drawn = sk.plot(samples, "small", output=tmp_path / "{name}.png")
    assert len(drawn) == 3
    assert sorted(p.name for p in tmp_path.glob("C*.png")) == ["C1.png", "C2.png", "C3.png"]
    assert tuple(drawn[0].get_size_inches()) == pytest.approx((3.0, 2.0))
    with pytest.raises(FileExistsError):
        sk.plot(samples, "small", output=tmp_path / "{name}.png")
    with pytest.raises(ValueError, match="a file per sample"):
        sk.plot(samples, "diameter_volume", output=tmp_path / "{name}.pdf")


# The system's opener is replaced by a script named `xdg-open`, which only Linux calls.
@pytest.mark.skipif(sys.platform != "linux", reason="replaces xdg-open, Linux's opener")
def test_a_samples_files_are_found_by_its_name_and_opened_by_the_system(tmp_path, monkeypatch):
    # A file is a sample's when its name begins with the sample's, the longest
    # name owning a file several begin.
    (tmp_path / ".samplekitrc").write_text(
        'schema_version = 1\n[collection]\nfiles = ["../images"]\n'
    )
    for name in ["B2", "B24", "B24-2"]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"
        )
    images = tmp_path.parent / "images"
    (images / "reports").mkdir(parents=True, exist_ok=True)
    for name in ["B24_photo_x500.tif", "B24_photo_x2000.tif", "B24-2_photo.tif", "B2_x.png",
                 "B25_orphan.png", "reports/B24_report.pdf"]:
        (images / name).write_text("")
    b24 = sk.Sample(tmp_path / "B24.md", model=False)
    assert [p.name for p in b24.files()] == ["B24_photo_x2000.tif", "B24_photo_x500.tif",
                                           "B24_report.pdf"]
    assert [p.name for p in b24.files("*x500*")] == ["B24_photo_x500.tif"]
    assert [p.name for p in sk.Sample(tmp_path / "B24-2.md", model=False).files()] == [
        "B24-2_photo.tif"]
    assert [p.name for p in sk.Sample(tmp_path / "B2.md", model=False).files()] == ["B2_x.png"]

    opened = tmp_path / "opened"
    opener = tmp_path / "bin" / "xdg-open"
    opener.parent.mkdir()
    opener.write_text(f"#!/bin/sh\necho \"$1\" > {opened}\n")
    opener.chmod(0o755)
    monkeypatch.setenv("PATH", f"{opener.parent}:{os.environ['PATH']}")
    monkeypatch.delenv("SSH_CONNECTION", raising=False)
    monkeypatch.delenv("SSH_TTY", raising=False)
    [path] = b24.open("*x500*")
    assert path.name == "B24_photo_x500.tif"
    for _ in range(50):
        if opened.exists():
            break
        time.sleep(0.05)
    assert opened.read_text().strip().endswith("B24_photo_x500.tif")
    [folder] = b24.open("*x500*", navigate=True)
    assert folder.resolve() == images.resolve()
    # Every file a pattern names opens; files of one folder, one folder.
    assert len(b24.open()) == 3
    assert len(b24.open("*.tif", navigate=True)) == 1
    for index in range(4):
        (images / f"B24_extra{index}.png").write_text("")
    with pytest.raises(ValueError, match="more than 5 open only with many=True"):
        b24.open()
    assert len(b24.open(many=True)) == 7
    with pytest.raises(FileNotFoundError):
        b24.open("*nothing*")
    monkeypatch.setenv("SSH_CONNECTION", "1 2 3 4")
    monkeypatch.delenv("DISPLAY", raising=False)
    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    with pytest.raises(RuntimeError, match="no display"):
        b24.open("*x500*")


def test_an_override_without_the_model_keeps_its_record_as_set_does(tmp_path):
    # One rule for a value set by hand. From Python, a derived value on a sample
    # read without its model lost its record and read current.
    path = tmp_path / "e.md"
    path.write_text(
        "---\nschema_version: 1\nname: E\nproperties:\n  hb:\n    v: 12.0\n"
        "    u: 0.005773502691896258\n    unit: mL\n    computed: {u: {}}\n"
        "    fingerprint: 18debc327c6f\n  headspace:\n    v: 1.1309733552923256\n"
        "    unit: hL\n    computed: {v: {hb: fce15bfbafe1}}\n"
        "    fingerprint: 5bcbffb74128\n---\n"
    )
    sample = sk.Sample(path, model=False)
    sample.headspace.value = 1.5
    assert sample.edited() == ["headspace"]
    sample.save()
    text = path.read_text()
    assert "computed: {v: {hb: fce15bfbafe1}}" in text
    assert "fingerprint: {edited:" in text


def test_a_column_never_computed_says_so_when_read():
    # Loose end 23: a property never computed warns when read, and a table's
    # column read as a column of None said nothing.
    table = sk.Table(
        {
            "T": sk.Property(),
            "R": sk.Property(),
            "G": sk.Property(compute=lambda row: 1 / row.R.value, depends_on=["row.R"]),
        },
        index="T",
        rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
    )
    sample = sk.Sample()
    sample.preheating = table
    with pytest.warns(UserWarning, match="preheating.G was never computed"):
        assert sample.preheating.G.values == [None, None]
    with pytest.warns(UserWarning, match="never computed"):
        assert sample.preheating.values("G") == [None, None]
    sample.compute()
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert sample.preheating.G.values == [0.5, 0.25]
        assert sample.preheating.values("R") == [2.0, 4.0]


def test_a_sample_saved_from_python_is_a_snapshot(tmp_path):
    # A save is a snapshot naming the file, after one of what changed before it;
    # a save that changes nothing takes none.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    path = tmp_path / "c1.md"
    path.write_text("---\nschema_version: 1\nname: C1\nproperties:\n  diameter: 1.0\n---\n")
    sample = sk.Sample(path, model=False)
    sample.diameter.value = 2.0
    sample.save()
    sample.save()

    def messages():
        out = subprocess.run(
            ["git", "--git-dir", str(tmp_path / ".samplekit/history"), "log", "--format=%s"],
            capture_output=True, text=True, check=True,
        )
        return out.stdout.splitlines()

    # One snapshot for the script's saves, taken at keep() or when it ends.
    assert not (tmp_path / ".samplekit/history").exists()
    sk.keep("diameter corrected")
    # The message given still names what ran — here, pytest.
    said = messages()
    assert len(said) == 2, said
    assert said[0].startswith("python ") and said[0].endswith(" · diameter corrected"), said
    assert said[1] == "the project as SampleKit first kept it"


def test_a_save_into_a_folder_made_a_project_brings_its_history(tmp_path):
    # A save there was refused until a terminal answered the offer: the
    # snapshots the project above kept of the folder are brought, and said.
    (tmp_path / ".samplekitrc").write_text(
        "schema_version = 1\n[collection]\nrecursive = true\n"
    )
    (tmp_path / "sub").mkdir()
    path = tmp_path / "sub" / "s.md"
    path.write_text("---\nschema_version: 1\nname: S\nproperties:\n  diameter: 1.0\n---\n")
    sample = sk.Sample(path, model=False)
    sample.diameter.value = 2.0
    sample.save()
    sk.keep("from above")
    (tmp_path / "sub" / ".samplekitrc").write_text("schema_version = 1\n")
    sample = sk.Sample(path, model=False)
    sample.diameter.value = 3.0
    with pytest.warns(UserWarning, match="brought from the history of"):
        sample.save()
    sk.keep("from the folder")
    said = subprocess.run(
        ["git", "--git-dir", str(tmp_path / "sub/.samplekit/history"), "log", "--all",
         "--format=%s"],
        capture_output=True, text=True, check=True,
    ).stdout
    assert "from above" in said, said
    assert "from the folder" in said, said


def test_a_script_is_one_snapshot_with_its_source_kept(tmp_path):
    # However many saves, one snapshot when the script ends, named by its
    # command line, its source kept beside it.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name in ("c1", "c2", "c3"):
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"
        )
    script = tmp_path / "raise.py"
    script.write_text(
        "import samplekit as sk\n"
        "for name in ('c1', 'c2', 'c3'):\n"
        f"    s = sk.Sample({str(tmp_path)!r} + '/' + name + '.md', model=False)\n"
        "    s.diameter.value = 2.0\n"
        "    s.save()\n"
    )
    subprocess.run([sys.executable, str(script)], check=True, cwd=tmp_path)
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    log = subprocess.run(git + ["log", "--format=%H %s"], capture_output=True, text=True,
                         check=True).stdout.splitlines()
    assert len(log) == 2, log
    last, said = log[0].split(" ", 1)
    assert said.startswith("python ") and "raise.py" in said, said
    source = subprocess.run(git + ["cat-file", "-p", f"script/{last}^{{}}"],
                            capture_output=True, text=True, check=True).stdout
    assert "s.save()" in source
    # The command line reads it back, beside that snapshot.
    binary = BINARY
    if binary.exists():
        shown = subprocess.run([str(binary), "log", "--script", "1"], cwd=tmp_path,
                               capture_output=True, text=True, check=True)
        assert "s.save()" in shown.stdout, shown.stdout + shown.stderr
        assert "raise.py" in shown.stderr


def test_a_script_is_kept_as_it_ran_when_it_first_saved(tmp_path):
    # The source is read at the script's first save, not when it ends; its
    # arguments are quoted as a shell takes them; a message given to keep still
    # names the script; `python -c` is said so.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name in ("c1", "c2"):
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"
        )
    script = tmp_path / "fix.py"
    script.write_text(
        "import pathlib, sys\n"
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.diameter.value = 2.0\n"
        "s.save()\n"
        "pathlib.Path(__file__).write_text('# rewritten after it ran\\n')\n"
        "sk.keep('first pass')\n"
        f"s = sk.Sample({str(tmp_path / 'c2.md')!r}, model=False)\n"
        "s.diameter.value = 3.0\n"
        "s.save()\n"
    )
    subprocess.run([sys.executable, str(script), "two words"], check=True, cwd=tmp_path)
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    log = subprocess.run(git + ["log", "--format=%H %s"], capture_output=True, text=True,
                         check=True).stdout.splitlines()
    assert len(log) == 3, log
    last, said = log[0].split(" ", 1)
    # The script's path is quoted where it holds a character a shell reads, as
    # Windows' `\\` is.
    assert said.startswith("python ") and "fix.py" in said, said
    assert said.endswith(" 'two words'"), said
    kept, message = log[1].split(" ", 1)
    assert message.startswith("python ") and "fix.py" in message, message
    assert message.endswith(" · first pass"), message
    for snapshot in (last, kept):
        source = subprocess.run(git + ["cat-file", "-p", f"script/{snapshot}^{{}}"],
                                capture_output=True, text=True, check=True).stdout
        assert "s.save()" in source and not source.startswith("# rewritten"), source
    # No script file: said so.
    inline = (
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c2.md')!r}, model=False)\n"
        "s.diameter.value = 4.0\n"
        "s.save()\n"
    )
    subprocess.run([sys.executable, "-c", inline], check=True, cwd=tmp_path)
    said = subprocess.run(git + ["log", "-1", "--format=%s"], capture_output=True, text=True,
                          check=True).stdout.strip()
    assert said.startswith("python -c · saved "), said


def test_a_snapshot_taken_meanwhile_is_not_taken_back(tmp_path):
    # A write kept by another process while a script runs — a command in
    # another terminal, a subprocess — is not taken back by the script's
    # snapshot: no snapshot holds a state the project never held.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name in ("c1", "c2"):
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"
        )
    other = tmp_path / "other.py"
    other.write_text(
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c2.md')!r}, model=False)\n"
        "s.diameter.value = 3.0\n"
        "s.save()\n"
    )
    script = tmp_path / "outer.py"
    script.write_text(
        "import subprocess, sys\n"
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.diameter.value = 2.0\n"
        "s.save()\n"
        f"subprocess.run([sys.executable, {str(other)!r}], check=True)\n"
    )
    subprocess.run([sys.executable, str(script)], check=True, cwd=tmp_path)
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    ids = subprocess.run(git + ["log", "--reverse", "--format=%H"], capture_output=True,
                         text=True, check=True).stdout.split()

    def diameter(snapshot, name):
        import re
        text = subprocess.run(git + ["show", f"{snapshot}:{name}.md"], capture_output=True,
                              text=True, check=True).stdout
        return float(re.search(r"diameter:\s*\{?\s*(?:v:\s*)?([-0-9.]+)", text).group(1))

    # Oldest first: once c2 holds 3.0, no later snapshot takes it back.
    seen = [diameter(snapshot, "c2") for snapshot in ids]
    assert 3.0 in seen, seen
    assert 1.0 not in seen[seen.index(3.0):], seen
    assert diameter(ids[-1], "c1") == 2.0 and diameter(ids[-1], "c2") == 3.0


def kept_digest(path):
    """A written file's digest as the history takes it: in a file holding
    text, CRLF as LF, which a figure written on Windows holds."""
    import hashlib

    held = path.read_bytes()
    if b"\0" not in held:
        held = held.replace(b"\r\n", b"\n")
    return hashlib.sha256(held).hexdigest()


def output_record(history, digest):
    """The one record of a file written with these contents: a record names
    the machine that wrote it, `output/<digest>@<machine>`."""
    names = subprocess.run(
        ["git", "--git-dir", str(history), "for-each-ref", "--format=%(refname)",
         f"refs/tags/output/{digest}@*"],
        capture_output=True, text=True, check=True).stdout.split()
    assert len(names) == 1, names
    return names[0]


def test_a_file_written_from_python_is_tied_to_its_snapshot(tmp_path):
    # to_csv and plot record the file in the project's history, by its hash,
    # saying Python wrote it.
    import hashlib
    import matplotlib
    matplotlib.use("Agg")
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name, diameter in [("c1", 1.0), ("c2", 2.0)]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {diameter}\n  volume: {diameter * 2}\n---\n")
    samples = sk.load(tmp_path)
    samples.to_csv(tmp_path / "table.csv", columns=["name", "diameter"])
    sk.plot(samples, x="volume", y="diameter", output=tmp_path / "figure.svg")

    def tag(path):
        digest = kept_digest(path)
        history = tmp_path / ".samplekit/history"
        out = subprocess.run(
            ["git", "--git-dir", str(history), "cat-file", "-p",
             output_record(history, digest)],
            capture_output=True, text=True)
        return out.stdout

    # Named by what ran, as the script's own snapshot is.
    assert "python " in tag(tmp_path / "table.csv")
    assert " · to_csv" in tag(tmp_path / "table.csv")
    assert "written: table.csv" in tag(tmp_path / "table.csv")
    assert " · plot diameter against volume" in tag(tmp_path / "figure.svg")
    assert "samplekit snapshot " in (tmp_path / "figure.svg").read_text()


def test_compute_goes_on_past_a_failure_and_waits_for_what_nobody_entered():
    # As samplekit compute does. A formula over an input nobody entered waits,
    # said as a warning; one that fails does not stop the others, and is raised
    # at the end, the others named.
    class Piece(sk.Sample):
        def __init__(self, path=None, name=None):
            self.radius = sk.Property(1.05)
            self.notch = sk.Property()
            self.volume = sk.Property(20.0)
            self.modulus = sk.Property(compute_value=lambda: (self.radius.value - self.notch.value) * 131.25,
                                   depends_on=["radius", "notch"])
            self.broken = sk.Property(compute_value=lambda: 1 / 0, depends_on=["radius"])
            self.points = sk.Property(compute_value=lambda: (self.radius.value - 1) * 1000,
                                      depends_on=["radius"])
            self.total = sk.Property(compute_value=lambda: self.points.value * self.volume.value,
                                     depends_on=["points", "volume"])
            super().__init__(path, name=name)

    piece = Piece(name="b")
    with pytest.warns(UserWarning, match="'modulus' waits for notch"):
        with pytest.raises(ZeroDivisionError):
            piece.compute()
    assert piece.points.value == pytest.approx(50.0)
    assert piece.total.value == pytest.approx(1000.0)
    assert piece.modulus.value is None


def test_not_applicable_is_one_value_python_reads_and_writes(tmp_path):
    # Sk.NA in, `n/a` in the file, sk.NA out, and a formula over it gives it
    # without running.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    ran = []

    class Piece(sk.Sample):
        def __init__(self, path=None, name=None):
            self.radius = sk.Property(1.05)
            self.notch = sk.Property()
            self.modulus = sk.Property(compute_value=lambda: ran.append(1) or (self.radius.value - self.notch.value),
                                   depends_on=["radius", "notch"])
            super().__init__(path, name=name)

    piece = Piece(name="b")
    piece.notch.value = sk.NA
    piece.compute()
    assert piece.modulus.value is sk.NA
    assert ran == []
    path = tmp_path / "b.md"
    piece.save(path)
    assert "notch: n/a" in path.read_text() or "v: n/a" in path.read_text()
    assert Piece(path).notch.value is sk.NA
    assert str(sk.NA) == "n/a" and not sk.NA


def test_save_all_stops_at_ctrl_c(collection):
    # The KeyboardInterrupt raised inside a sample's save was lost there, and
    # the list went on to its end.
    import signal

    class Pressing(sk.Sample):
        def save(self, *arguments, **keywords):
            super().save(*arguments, **keywords)
            # Raised in this process as Ctrl-C raises it: os.kill would end
            # the process on Windows.
            signal.raise_signal(signal.SIGINT)

    samples = sk.SampleList([Pressing(path) for path in sorted(collection.root.glob("*.md"))])
    for sample in samples:
        sample.tags = list(sample.tags) + ["late"]
    with pytest.raises(KeyboardInterrupt, match="1 of the list written"):
        samples.save_all()
    assert sum("late" in path.read_text() for path in collection.root.glob("*.md")) == 1


def test_reading_a_value_whose_input_does_not_apply_computes_nothing(project):
    # Read, it was replaced by n/a and said current; saved, it was edited.
    _tabled(project)
    path = project.sample("s.md", name="s", properties={"diameter": "2.0"})
    subprocess.run([str(BINARY), str(path), "compute", "--write"], check=True, capture_output=True)
    subprocess.run(
        [str(BINARY), str(path), "set", "diameter=n/a", "--write"], check=True, capture_output=True
    )
    sample = sk.Sample(path)
    with pytest.warns(UserWarning):
        assert sample.double.value == 4.0
    assert sample.double.state == "outdated"
    sample.compute()
    assert sample.double.value is sk.NA


def test_python_raises_as_its_docstrings_say(project):
    # `#-1` of an empty table raised ValueError where `#0` raised IndexError;
    # two units compared raised ValueError where `filter` says TypeError; a
    # tag given twice was dropped without a word.
    project.config("schema_version = 1\n")
    path = project.root / "s.md"
    path.write_text(
        sample_text(name="s", properties={"volume": "{v: 20, unit: L}", "mass": "{v: 2, unit: kg}"})
        .replace("---\n# Notes", "tables:\n  t:\n    index: d\n    columns:\n      d: {}\n      g: {}\n    rows: []\n---\n# Notes")
    )
    sample = sk.Sample(path)
    for address in ("t.g[#0]", "t.g[#-1]"):
        with pytest.raises(IndexError):
            sample[address]
    with pytest.raises(TypeError, match="one unit"):
        sk.SampleList(project.root).filter("volume > mass")
    with pytest.warns(UserWarning, match="given more than once"):
        sample.tags = ["a", "a", "b"]
    assert list(sample.tags) == ["a", "b"]


def test_a_figure_leaves_out_a_value_not_applicable(tmp_path):
    # No point, as for a value never entered.
    import matplotlib
    matplotlib.use("Agg")
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name, diameter in [("a", "1.0"), ("b", "n/a"), ("c", "3.0")]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {diameter}\n  volume: 2.0\n---\n")
    with pytest.warns(UserWarning, match="1 of 3 samples not drawn"):
        sk.plot(sk.load(tmp_path), x="volume", y="diameter", output=tmp_path / "f.svg")


# ------------------------------------------------- review of 2026-09-27


class Inspected(sk.Sample):
    """A piece whose score is the mean of a inspection table, and whose modulus
    reads a final hardness: either may be empty."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = sk.Property(1.05)
        self.notch = sk.Property()
        self.modulus = sk.Property(compute_value=self._modulus, depends_on=["radius", "notch"])
        self.inspection = sk.Table({"inspector": sk.Property(), "score": sk.Property()}, index="inspector")
        self.score = sk.Property(compute_value=self._score, depends_on=["inspection.score"])

    def _modulus(self):
        return (self.radius.value - self.notch.value) * 131.25

    def _score(self):
        import statistics
        return statistics.fmean(self.inspection.values("score"))


def test_a_formula_over_an_empty_column_waits():
    # A table without a row is an input nobody entered, as the command line
    # judges it: the mean of no score raised StatisticsError.
    piece = Inspected(name="b")
    with pytest.warns(UserWarning, match=r"b: .*'score' waits for inspection\.score"):
        piece.compute()
    assert piece.not_current()["score"] == "waits for inspection.score"
    piece.inspection.add(inspector="Lea", score=40.0)
    piece.compute()
    assert piece.score.value == pytest.approx(40.0)
    assert "score" not in piece.not_current()


def test_not_current_says_what_a_value_waits_for():
    piece = Inspected(name="b")
    piece.inspection.add(inspector="Lea", score=40.0)
    assert piece.not_current() == {"modulus": "waits for notch", "score": "never computed"}
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        assert piece.modulus.value is None
    said = [str(warning.message) for warning in caught]
    assert said == ["b: 'modulus' waits for notch: nobody entered notch, so its formula does not run"]
    piece.notch.value = 1.01
    assert piece.not_current()["modulus"] == "never computed"


def test_a_warning_points_at_the_scripts_line():
    piece = Inspected(name="b")
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        piece.modulus.value
        piece.compute()
    assert caught and all(warning.filename == __file__ for warning in caught), [
        (warning.filename, warning.lineno) for warning in caught
    ]
    # Each warning about a value names its sample, so that two samples waiting
    # alike are two warnings, not one that Python's filter shows once.
    assert all(str(warning.message).startswith("b: ") for warning in caught)


def test_an_uncertainty_waiting_for_its_value_says_so():
    class Volume(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.volume = sk.Property(unit="L", compute_uncertainty=lambda: 0.3, depends_on=[])

    sample = Volume(name="v")
    with pytest.warns(UserWarning) as caught:
        sample.compute()
    said = " ".join(str(warning.message) for warning in caught)
    assert "the uncertainty of 'volume' waits for its value" in said
    assert "waits for volume" not in said


def test_readings_over_a_formula_are_refused():
    piece = Inspected(name="b")
    piece.notch.value = 1.01
    piece.compute("modulus")
    before = piece.modulus.value
    for assign in (lambda: setattr(piece.modulus, "value", [1.0, 2.0]),
                   lambda: setattr(piece.modulus, "readings", [1.0, 2.0])):
        with pytest.raises(ValueError, match="override"):
            assign()
    assert piece.modulus.value == before and piece.modulus.readings is None
    assert piece.modulus.is_computed


def test_replacing_a_formula_by_a_plain_property_is_refused():
    piece = Inspected(name="b")
    with pytest.raises(ValueError, match=r"s\.modulus\.value = \.\.\. to override"):
        piece.modulus = sk.Property(5.0)
    assert piece.modulus.is_computed
    piece.modulus = sk.Property(compute_value=lambda: 5.0, depends_on=[])
    assert piece.modulus.is_computed
    del piece.modulus
    piece.modulus = sk.Property(5.0)
    assert piece.modulus.value == 5.0 and not piece.modulus.is_computed


def test_a_boolean_is_refused_on_a_property_with_a_unit(project):
    sample = quantity_sample(project)
    with pytest.raises(ValueError, match="boolean"):
        sample.diameter.value = True
    with pytest.raises(ValueError, match="boolean"):
        sk.Property(True, unit="g")
    assert sample.diameter.value == pytest.approx(3.1551234)
    sample.flag = sk.Property(True)
    assert sample.flag.value is True


def test_a_blank_name_is_refused(project):
    sample = quantity_sample(project)
    for blank in ("", "  "):
        with pytest.raises(ValueError, match="cannot be empty"):
            sample.name = blank
        with pytest.raises(ValueError, match="cannot be empty"):
            sk.Sample(name=blank)
    assert sample.name == "ph"
    sample.name = None
    # No name written: the file's stands for it.
    assert sample.name == "q"


def test_a_missing_row_is_said_as_table_at_says_it():
    table = sk.Table({"T": sk.Property(), "R": sk.Property()}, index="T", rows=[{"T": 1, "R": 2.0}])
    with pytest.raises(KeyError) as caught:
        table.R[0]
    said = str(caught.value)
    assert said.startswith("no row at (0)")
    assert "available: 1" in said and "Integer" not in said


def test_save_all_to_a_missing_directory_says_so_and_writes_nothing(collection):
    missing = collection.root / "missing"
    with pytest.raises(FileNotFoundError) as caught:
        sk.SampleList(collection.root).save_all(missing)
    assert "does not exist" in str(caught.value)
    assert "os error" not in str(caught.value)
    assert not missing.exists()


def test_sample_list_index_takes_start_and_stop(collection):
    samples = sk.SampleList(collection.root).sorted("name")
    assert samples.index("B") == 1
    assert samples.index(samples[1], 1, 2) == 1
    assert samples.index("C", -1) == 2
    with pytest.raises(ValueError):
        samples.index("A", 1)
    assert samples.count("A") == 1


def test_a_field_path_to_a_missing_row_or_name_raises():
    table = sk.Table({"T": sk.Property(), "nitridation": sk.Property()}, index="T",
                     rows=[{"T": 20, "nitridation": 2.0}])
    sample = sk.Sample(name="s")
    sample.diameter = sk.Property(1.0)
    sample.tempering = table
    assert sample["tempering.nitridation[20]"] == 2.0
    with pytest.raises(KeyError, match="no row at"):
        sample["tempering.nitridation[99]"]
    with pytest.raises(KeyError, match="did you mean: nitridation"):
        sample["tempering.nitridatoin[20]"]
    with pytest.raises(KeyError, match="did you mean: diameter"):
        sample["diametr.u"]
    with pytest.raises(KeyError, match="did you mean: diameter"):
        sample.dependencies("diametr")
    with pytest.raises(KeyError, match="did you mean: diameter"):
        sample.dependents("diametr")
    with pytest.raises(KeyError, match="did you mean: diameter"):
        sample.affected_by_change("diametr")
    with pytest.raises(KeyError, match="did you mean: nitridation"):
        sample.dependencies("tempering.nitridatoin")
    assert sample.dependencies("diameter") == []


def test_a_table_method_writes_any_extension(collection):
    # The method names the format, the caller the file, as `--csv -o m.json`
    # writes CSV into m.json.
    samples = sk.SampleList(collection.root)
    samples.to_csv(collection.root / "m.json", columns=["name"])
    assert (collection.root / "m.json").read_text() == samples.to_csv(columns=["name"])
    samples.to_json(collection.root / "m.tsv", columns=["name"])
    assert (collection.root / "m.tsv").read_text() == samples.to_json(columns=["name"])


def test_a_declared_exports_other_extension_is_refused(collection):
    # As `samplekit export diameters -o m.json` refuses it — the format is the
    # configuration's, and the path contradicts it.
    samples = sk.SampleList(collection.root)
    with pytest.raises(ValueError, match=r"'diameters' writes CSV, and the extension names JSON"
                       r".*change format in \[export\.diameters\]; nothing was written"):
        samples.export("diameters", output=collection.root / "m.json")
    assert not list(collection.root.glob("m.*"))
    samples.export("diameters", output=collection.root / "m.txt")
    assert (collection.root / "m.txt").is_file()


def test_stats_weights_its_mean_as_the_summary_does(collection, cli):
    # `mean (1/u²)` where every uncertainty is known, as --summary.
    samples = sk.SampleList(collection.root).sorted("name")
    measured = samples[:2]
    summary = measured.stats("diameter")
    weights = [1 / 0.05**2, 1 / 0.02**2]
    assert summary.weighted
    assert summary.mean == pytest.approx(
        (12.5 * weights[0] + 13.1234 * weights[1]) / sum(weights))
    assert "(1/u²)" in repr(summary)
    said = cli(collection.root / "A.md", collection.root / "B.md", "-c", "name,diameter", "--summary")
    assert "mean (1/u²)" in said and f"{summary.mean:.3f}" in said
    # C has no uncertainty: the plain mean stands, as the command line's.
    plain = samples.stats("diameter")
    assert not plain.weighted
    assert plain.mean == pytest.approx((12.5 + 13.1234 + 11.0) / 3)
    assert not sk.Property([1.0, 2.0]).stats.weighted


def test_a_weighted_mean_carries_its_own_standard_error(collection):
    # 1/√Σw beside a weighted mean; the samples' own deviation over √n beside
    # the plain one.
    samples = sk.SampleList(collection.root).sorted("name")
    weighted = samples[:2].stats("diameter")
    weights = [1 / 0.05**2, 1 / 0.02**2]
    assert weighted.standard_error == pytest.approx(1 / sum(weights) ** 0.5)
    plain = samples.stats("diameter")
    assert plain.standard_error == pytest.approx(plain.sample_stdev / 3**0.5)


def test_an_uncertainty_column_left_out_is_said(project):
    # Said as the command line says it on stderr.
    project.config()
    project.sample("A.md", name="A", properties={"diameter": "{v: 12.5, unit: g}"})
    samples = sk.SampleList(project.root)
    with pytest.warns(UserWarning, match=r"diameter_uncertainty.* is left out: it is empty for "
                      r"every sample"):
        written = samples.to_csv(columns=["name", "diameter"])
    assert "uncertainty" not in written
    with pytest.warns(UserWarning, match="is left out"):
        samples.to_tsv(columns=["name", "diameter"])
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        samples.to_json(columns=["name", "diameter"])


def test_a_key_error_prints_its_message_on_its_lines(project):
    import traceback

    sample = quantity_sample(project)
    with pytest.raises(KeyError) as caught:
        sample["diametr"]
    said = "".join(traceback.format_exception_only(caught.value))
    assert said.startswith("KeyError: unknown field 'diametr'\n")
    assert "\\n" not in said
    assert isinstance(caught.value, KeyError) and caught.value.args[0].startswith("unknown")


def test_an_attribute_error_suggests_once(project):
    import traceback

    sample = quantity_sample(project)
    with pytest.raises(AttributeError) as caught:
        sample.diametr
    said = "".join(traceback.format_exception_only(caught.value))
    assert said.lower().count("did you mean") == 1
    assert not hasattr(sample, "diametr")


def test_a_type_is_named_with_its_article(project):
    sample = quantity_sample(project)
    with pytest.raises(TypeError) as caught:
        sample.tags.append(3)
    assert "an int" in str(caught.value) and "a int" not in str(caught.value)
    with pytest.raises(TypeError) as caught:
        sk.Property(compute_value=object())
    assert "an object" in str(caught.value)


def test_a_folder_given_as_a_sample_names_load(collection):
    with pytest.raises(IsADirectoryError, match=r"sk\.load"):
        sk.Sample(collection.root)
    note = collection.root / "note.md"
    note.write_text("# Just a note\n")
    with pytest.raises(ValueError) as caught:
        sk.load(note)
    assert "not a sample" in str(caught.value)
    assert "claimed" not in str(caught.value)


def test_a_refusal_names_the_options_of_python(tmp_path, monkeypatch):
    paths = figure_samples(tmp_path)
    samples = sk.SampleList([sk.Sample(p, model=False) for p in paths])
    monkeypatch.setenv("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")
    monkeypatch.delenv("DISPLAY", raising=False)
    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    with pytest.raises(RuntimeError) as caught:
        sk.plot(samples, "diameter_volume")
    assert "output=" in str(caught.value) and "-o " not in str(caught.value)
    with pytest.raises(TypeError, match="x_label changes a declared figure"):
        sk.plot(Plotted(paths[0]), "mass_bar", x_label="m")


def test_auto_is_a_free_bound_of_an_axis(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (figure,) = sk.plot(samples, "diameter_volume", x_limits=[0, "auto"], output=tmp_path / "f.png")
    low, high = figure.axes[0].get_xlim()
    assert low == 0 and high > 3


def test_samples_disagreeing_are_a_data_error_for_the_command_line(tmp_path):
    # `samplekit plot` exits with what its figure process returns: 2 for the
    # samples' own disagreement, as the table does for a column of two units,
    # and 1 for a name nobody declared.
    paths = figure_samples(tmp_path)
    paths[1].write_text(paths[1].read_text().replace("unit: g}", "unit: kg}"))

    def drawn(y):
        request = {"axes": {"x": "volume", "y": y}, "samples": [str(p) for p in paths],
                   "output": str(tmp_path / "f.png"), "model": False}
        return subprocess.run([sys.executable, "-m", "samplekit._figure"],
                              input=json.dumps(request), capture_output=True, text=True,
                              env={**os.environ, "MPLBACKEND": "Agg"})

    units = drawn("diameter")
    assert units.returncode == 2, units.stderr
    assert "one axis holds one unit" in units.stderr
    unknown = drawn("diameterr")
    assert unknown.returncode == 1, unknown.stderr


def test_an_empty_selection_writes_the_header(collection):
    samples = sk.SampleList(collection.root)
    nothing = samples.filter("diameter > 100")
    assert len(nothing) == 0
    assert nothing.to_csv(columns=["name", "diameter"]).startswith("name,diameter")
    assert nothing.filter("diameter > 1000").to_dict(columns=["name"]) == {"name": []}
    with pytest.raises(KeyError, match="did you mean: diameter"):
        nothing.to_csv(columns=["name", "diametr"])


# ------------------------------------------------- review of 2026-09-27, Python


def grouped_samples(tmp_path, values, rc="schema_version = 1\n"):
    """Samples of one numeric property each, `n/a` where the value is None."""
    tmp_path.mkdir(exist_ok=True)
    (tmp_path / ".samplekitrc").write_text(rc)
    for at, value in enumerate(values):
        written = "n/a" if value is None else value
        (tmp_path / f"s{at:02}.md").write_text(
            f"---\nschema_version: 1\nname: s{at:02}\nproperties:\n"
            f"  radius: {1.040 + at / 1000}\n  modulus: {4.0 + at / 10}\n  recovery: {written}\n---\n"
        )
    return sk.load(tmp_path)


def legend_of(figures):
    legend = figures[0].axes[0].get_legend()
    return [text.get_text() for text in legend.get_texts()] if legend else []


def test_a_table_is_no_group(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.load(*figure_samples(tmp_path))
    output = tmp_path / "f.png"
    # Its repr was each legend entry, and over twelve samples it was refused
    # for being twelve text groups.
    with pytest.raises(ValueError, match="'measurements' is a table"):
        sk.plot(samples, x="volume", y="diameter", group="measurements", output=output)
    with pytest.raises(KeyError, match="names a column"):
        sk.plot(samples, x="volume", y="diameter", group="measurements.hv", output=output)
    assert not output.exists()
    drawn = sk.plot(samples, x="volume", y="diameter", group="measurements.hv[8.0]", output=output)
    assert legend_of(drawn) == ["5.1"]


def test_a_value_not_applicable_leaves_a_numeric_group_numeric(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    few = grouped_samples(tmp_path / "few", [70, None, 62, 69])
    drawn = sk.plot(few, x="radius", y="modulus", group="recovery", output=tmp_path / "few.png")
    assert legend_of(drawn) == ["62", "69", "70", "n/a"]
    legend = drawn[0].axes[0].get_legend()
    grey = [handle for handle, text in zip(legend.legend_handles, legend.get_texts())
            if text.get_text() == "n/a"][0]
    assert matplotlib.colors.to_hex(grey.get_color()) == matplotlib.colors.to_hex("0.6")
    many = grouped_samples(tmp_path / "many", [None] + list(range(60, 71)))
    drawn = sk.plot(many, x="radius", y="modulus", group="recovery", output=tmp_path / "many.png")
    # A colour bar for the twelve values, and the sample without one named.
    assert len(drawn[0].axes) == 2
    assert legend_of(drawn) == ["n/a"]


def test_numbers_that_read_alike_are_told_apart_in_a_legend(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    rc = 'schema_version = 1\n[property.recovery]\nprecision = ".0f"\n'
    samples = grouped_samples(tmp_path / "near", [66.2, 65.9, 70.0], rc)
    drawn = sk.plot(samples, x="radius", y="modulus", group="recovery", output=tmp_path / "f.png")
    assert legend_of(drawn) == ["65.9", "66.2", "70.0"]
    apart = grouped_samples(tmp_path / "apart", [66.0, 70.0], rc)
    drawn = sk.plot(apart, x="radius", y="modulus", group="recovery", output=tmp_path / "g.png")
    assert legend_of(drawn) == ["66", "70"]


def test_text_is_refused_where_a_number_is_held(tmp_path):
    path = tmp_path / "b.md"
    path.write_text(
        "---\nschema_version: 1\nname: b\nproperties:\n  radius: {v: 1.048, readings: [1.047, 1.049]}\n"
        "  style: {v: titanium}\ntables:\n  t:\n    index: d\n    columns: {d: {}, g: {}}\n"
        "    rows:\n      - {d: 1, g: 1.03}\n---\n"
    )
    piece = sk.Sample(path, model=False)
    for wrong in ("1.05", True):
        with pytest.raises(ValueError, match="'radius' holds a number"):
            piece.radius.value = wrong
    with pytest.raises(ValueError, match="holds a number"):
        piece.t.at(1, "g").value = "1.03"
    assert piece.radius.value == pytest.approx(1.048)
    # A property holding text takes text.
    piece.style.value = "nickel"
    piece.radius.value = 1.05
    assert piece.radius.value == 1.05


class Poured(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = sk.Property(1.05)
        self.volume = sk.Property(20.0)
        self.recovery = sk.Property(compute_value=lambda: (self.radius.value - 1) * self.volume.value,
                                      depends_on=["radius", "volume"])


def test_a_value_waiting_says_so_when_read_as_compute_does():
    piece = Poured(name="b")
    piece.compute()
    piece.volume.value = None

    def said(action):
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            action()
        return [str(warning.message) for warning in caught]

    # Computed before its input was taken away: read, it waits, as compute()
    # says it — not "stale: compute() runs it again", which compute declines.
    sentence = "b: 'recovery' waits for volume: nobody entered volume, so its formula does not run"
    assert said(piece.compute) == [sentence]
    assert said(lambda: piece.recovery.value) == [sentence]

    # An input that failed was entered: it is said to have failed.
    class Broken(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.radius = sk.Property(1.05)
            self.double = sk.Property(compute_value=lambda: 1 / 0, depends_on=["radius"])
            self.quad = sk.Property(compute_value=lambda: self.double.value * 2, depends_on=["double"])

    def computed():
        with pytest.raises(ZeroDivisionError):
            Broken(name="k").compute()

    assert said(computed) == [
        "k: 'quad' waits for double: double failed, so its formula does not run"]


def test_a_warning_names_an_unnamed_sample_by_its_file(tmp_path):
    path = tmp_path / "piece-7.md"
    path.write_text("---\nschema_version: 1\nproperties:\n  radius: 1.05\n---\n")
    piece = Inspected(path)
    with pytest.warns(UserWarning, match=r"^piece-7: 'modulus' waits for notch"):
        assert piece.modulus.value is None


def test_an_empty_selection_writes_the_whole_header(collection, cli, tmp_path):
    samples = sk.SampleList(collection.root)
    nothing = samples.filter("diameter > 100")
    root = collection.root
    assert nothing.to_csv(columns=["name", "diameter"]) == cli(
        root, "-c", "name,diameter", "-f", "diameter > 100", "--csv")
    assert nothing.to_csv(columns=["name", "diameter"]).startswith(
        "name,diameter_value [g],diameter_uncertainty [g]")
    assert nothing.to_tsv(columns=["name", "diameter"]) == cli(
        root, "-c", "name,diameter", "-f", "diameter > 100", "--tsv")
    assert nothing.to_csv(profile="ductilities") == cli(
        root, "--profile", "ductilities", "-f", "diameter > 100", "--csv")
    with pytest.warns(UserWarning, match="'diameters' is written from 0 of 3 samples"):
        written = nothing.export("diameters", output=tmp_path / "diameters.csv")
    assert written.read_text().splitlines()[0] == samples.export(
        "diameters", output=tmp_path / "all.csv").read_text().splitlines()[0]


def test_a_figure_of_samples_lacking_an_axis_is_refused_as_data(tmp_path):
    import matplotlib
    from samplekit import _figures
    matplotlib.use("Agg")
    paths = figure_samples(tmp_path)
    empty = tmp_path / "C4.md"
    empty.write_text("---\nschema_version: 1\nname: C4\nproperties:\n  diameter: {unit: g}\n"
                     "  volume: 3.0\n---\n")
    with pytest.raises(ValueError, match="^the one sample selected holds no diameter: nothing to draw") as one:
        sk.plot(sk.Sample(empty), x="volume", y="diameter", output=tmp_path / "f.png")
    assert _figures.raised_by_the_data(one.value)
    other = tmp_path / "C5.md"
    other.write_text(empty.read_text().replace("C4", "C5"))
    with pytest.raises(ValueError, match="none of the 2 samples holds both 'volume' and 'diameter'"):
        sk.plot(sk.load(empty, other), x="volume", y="diameter", output=tmp_path / "f.png")
    assert paths


def test_a_figure_written_by_a_script_names_the_script_and_its_axes(tmp_path):
    figure_samples(tmp_path)
    script = tmp_path / "draw.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "sk.plot(sk.load(sys.argv[1]), x='volume', y='diameter', output=sys.argv[2])\n"
    )
    output = tmp_path / "f.svg"
    subprocess.run([sys.executable, str(script), str(tmp_path), str(output)], check=True,
                   env={**os.environ, "MPLBACKEND": "Agg"}, cwd=tmp_path)
    import hashlib
    digest = kept_digest(output)
    history = tmp_path / ".samplekit/history"
    tag = subprocess.run(
        ["git", "--git-dir", str(history), "cat-file", "-p", output_record(history, digest)],
        capture_output=True, text=True, check=True).stdout
    assert "python " in tag and script.name in tag, tag
    assert " · plot diameter against volume" in tag, tag


def test_keep_takes_one_line_of_text(tmp_path):
    with pytest.raises(TypeError, match="keep\\(\\) takes a message as text"):
        sk.keep(3)
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    (tmp_path / "c1.md").write_text("---\nschema_version: 1\nname: C1\nproperties:\n  diameter: 1.0\n---\n")
    script = tmp_path / "keep.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "s = sk.Sample(sys.argv[1], model=False)\n"
        "s.diameter.value = 2.0\ns.save()\nsk.keep('first\\nline')\n"
        "s.diameter.value = 3.0\ns.save()\nsk.keep('   ')\n"
    )
    subprocess.run([sys.executable, str(script), str(tmp_path / "c1.md")], check=True, cwd=tmp_path)
    said = subprocess.run(
        ["git", "--git-dir", str(tmp_path / ".samplekit/history"), "log", "--format=%s"],
        capture_output=True, text=True, check=True).stdout.splitlines()
    # Blank says nothing, as on the command line: the script's command line
    # is the message.
    assert said[0] == f"python {quoted(script)} {quoted(tmp_path / 'c1.md')}", said
    assert said[1] == f"python {quoted(script)} · first line", said


def test_a_save_refused_by_the_system_says_so_plainly(project, tmp_path):
    sample = quantity_sample(project)
    with pytest.raises(FileNotFoundError, match="does not exist, and nothing was written") as missing:
        sample.save(tmp_path / "nowhere" / "q.md")
    for overwrite in (False, True):
        with pytest.raises(IsADirectoryError, match="is a folder") as folder:
            sample.save(tmp_path, overwrite=overwrite)
        assert "overwrite=True" not in str(folder.value)
        assert "ph.md" in str(folder.value)
    for error in (missing.value, folder.value):
        assert "os error" not in str(error)


def test_overwrite_does_not_change_permissions(project):
    sample = quantity_sample(project)
    path = sample.path
    path.chmod(0o444)
    try:
        if os.access(path, os.W_OK):
            pytest.skip("this user writes read-only files")
        sample.diameter.value = 4.0
        with pytest.raises(PermissionError) as refused:
            sample.save(overwrite=True)
        assert "os error" not in str(refused.value)
    finally:
        path.chmod(0o644)


def test_a_name_with_a_path_is_refused(tmp_path):
    with pytest.raises(ValueError, match="'a/b' is a path"):
        sk.Sample(name="a/b")
    with pytest.raises(ValueError, match="is a path"):
        sk.Sample.new(tmp_path / "x.md", name="a/b")
    sample = sk.Sample(name="x")
    with pytest.raises(ValueError, match="'x/y' is a path"):
        sample.name = "x/y"
    with pytest.raises(ValueError, match="cannot be empty"):
        sample.name = " "
    assert sample.name == "x"


def test_a_tag_is_refused_in_a_tags_words():
    sample = sk.Sample(name="x")
    with pytest.raises(ValueError, match="a tag cannot be empty") as empty:
        sample.tags.append("")
    with pytest.raises(ValueError, match="a tag is one word. Try 'a_b'") as spaced:
        sample.tags = ["a b"]
    for error in (empty.value, spaced.value):
        assert "property" not in str(error) and "CLI" not in str(error)
    assert sample.tags == []


def test_a_mistyped_method_is_suggested(collection):
    samples = sk.SampleList(collection.root)
    sample = samples[0]
    for mistyped, meant in [(lambda: sample.sav, "save"), (lambda: sample.not_curent, "not_current"),
                            (lambda: sk.Sample.sav, "save"), (lambda: samples.filtr, "filter")]:
        with pytest.raises(AttributeError) as caught:
            mistyped()
        assert f"did you mean: {meant}?" in str(caught.value)
    table = sk.Table({"d": sk.Property(), "g": sk.Property()}, index="d")
    with pytest.raises(AttributeError, match="did you mean: values\\?"):
        table.valuse


def test_an_auto_bound_leaves_a_margin(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.load(*figure_samples(tmp_path))
    drawn = sk.plot(samples, x="volume", y="diameter", y_limits=(0, "auto"), output=tmp_path / "f.png")
    low, high = drawn[0].axes[0].get_ylim()
    top = 12.0 + 0.1
    assert low == 0
    # matplotlib's margin past the highest error bar, over the whole range.
    assert high == pytest.approx(top + matplotlib.rcParams["axes.ymargin"] * top)


def test_a_cell_axis_names_its_row(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    rc = FIGURE_RC + '[property."measurements.hv"]\nsymbol = "hv"\n'
    samples = sk.load(*figure_samples(tmp_path, rc))
    drawn = sk.plot(samples, x="volume", y="measurements.hv[8.0]", output=tmp_path / "f.png")
    assert drawn[0].axes[0].get_ylabel() == "hv, f 8.0"
    column = sk.plot(samples, "hv", output=tmp_path / "g.png")
    assert column[0].axes[0].get_ylabel() == "hv"


def test_messages_say_one_thing_one_way(collection, tmp_path):
    import matplotlib
    from samplekit import _figures
    matplotlib.use("Agg")
    # The command line's formats, not what this matplotlib happens to write.
    refused = _figures.output_refusal(pathlib.Path("f.gif"))
    assert refused.endswith(".eps, .jpeg, .jpg, .pdf, .pgf, .png, .ps, .raw, .rgba, .svg, "
                            ".svgz, .tif, .tiff, .webp")
    # Figures in the order declared, the suggestion before what exists.
    (tmp_path / "f").mkdir()
    samples = sk.load(*figure_samples(tmp_path / "f"))
    with pytest.raises(KeyError) as unknown:
        sk.plot(samples, "diameter_volum")
    assert str(unknown.value).splitlines()[1:] == [
        "  did you mean: diameter_volume?", "  the figures: diameter_volume, hv"]
    sample = sk.SampleList(collection.root)[0]
    with pytest.raises(KeyError) as field:
        sample["diametr"]
    lines = str(field.value).splitlines()
    assert lines[1].strip().startswith("did you mean") and lines[2].strip().startswith("available")
    # One wording for a file not replaced.
    for refusal in (lambda: sample.save(collection.root / "B.md"),
                    lambda: sk.SampleList(collection.root).to_csv(
                        collection.root / "B.md", columns=["name"])):
        with pytest.raises(FileExistsError, match="already exists, and nothing was written"):
            refusal()
    with pytest.raises(FileExistsError, match="^one destination already exists"):
        sk.SampleList([sample]).save_all(collection.root)
    with pytest.raises(FileExistsError, match="^3 destinations already exist"):
        sk.SampleList(collection.root).save_all(collection.root)
    table = sk.Table({"d": sk.Property(), "g": sk.Property()}, index="d")
    table.add(d=1, g=2.0)
    with pytest.raises(KeyError) as row:
        table.at(99, "g")
    assert "\n\n" not in str(row.value) and not str(row.value).endswith("\n")
    with pytest.raises(KeyError, match="an empty field name"):
        sample[""]
    with pytest.raises(ValueError, match="the list holds no sample"):
        sk.SampleList([]).stats("diameter")


def test_private_docstrings_name_no_internal_decisions():
    import inspect
    from samplekit import _figure, _figures, _native
    internal = re.compile(r"D-\d+|\[\[[a-z-]+|spec/")
    found = []

    def walk(item, where, depth=0):
        doc = getattr(item, "__doc__", None)
        if isinstance(doc, str) and internal.search(doc):
            found.append(where)
        if depth < 2 and (inspect.ismodule(item) or inspect.isclass(item)):
            for name, member in vars(item).items():
                # Each module is walked on its own, the model runtime's
                # worker aside.
                if not name.startswith("__") and not inspect.ismodule(member):
                    walk(member, f"{where}.{name}", depth + 1)

    for module in (sk, _native, _figures, _figure):
        walk(module, module.__name__)
    assert not found, found
    stub = (pathlib.Path(sk.__file__).parent / "__init__.pyi").read_text()
    assert not internal.search(stub), internal.findall(stub)


class _Declared(sk.Sample):
    """A formula of each shape, for the worker's digest of each."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.twice = sk.Property(unit="g", compute_value=self._twice, depends_on=["diameter"])
        self.preheating = sk.Table(
            {
                "T": sk.Property(),
                "G": sk.Property(unit="NTU", compute=lambda row: 1 / row.T.value, depends_on=["row.T"]),
            },
            index="T",
            rows=[{"T": 10}],
        )

    def _twice(self):
        return 2 * self.diameter.value


def test_formula_code_names_each_formula_and_what_declares_it(project):
    path = project.sample("d.md", properties={"diameter": "{readings: [1.0, 2.0, 3.0], unit: g}"})
    sample = _Declared(path)
    code = sample._formula_code()
    assert sorted(code) == ["diameter", "preheating.G", "twice"]
    # A method as its function, which holds no sample; the statistics as
    # declared; a column's derivation with what it reads and fills.
    assert code["twice"] == [("compute_value", _Declared._twice), ("unit", "g")]
    assert [(role, repr(given)) for role, given in code["diameter"]] == [
        ("value", "sk.stats.mean"),
        ("uncertainty", "sk.stats.standard_error"),
        ("unit", "'g'"),
    ]
    assert [role for role, _ in code["preheating.G"]] == ["compute", "inputs", "outputs", "unit"]
    assert code["preheating.G"][1:] == [("inputs", "row.T"), ("outputs", "G"), ("unit", "NTU")]
    # A value current by its inputs is planned when its formula changed.
    sample.compute()
    assert sample._plan([], False, False) == []
    assert sample._plan([], False, False, ["twice"]) == [("twice", "formula changed")]


# ------------------------------------------- readings and their value


class _Viscous(sk.Sample):
    """A toughness table whose `stiffness` column declares the statistics of its cells."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.toughness = sk.Table(
            {
                "T": sk.Property(),
                "stiffness": sk.Property(
                    unit="cP", value=sk.stats.mean, uncertainty=sk.stats.standard_error
                ),
            },
            index="T",
        )


def test_a_column_declares_the_statistics_of_its_cells(project):
    path = project.root / "v.md"
    sample = _Viscous(name="v")
    sample.toughness.add(T=40, stiffness=[1.0, 2.0, 3.0])
    cell = sample.toughness.at(40, "stiffness")
    assert cell.value == 2.0
    assert cell.uncertainty == pytest.approx(3**-0.5)
    sample.save(path)
    text = path.read_text()
    # Said once, by the column; the cell records the readings it was taken over.
    assert "statistics: {v: mean, u: standard_error}" in text
    assert "computed: {readings:" in text
    reloaded = _Viscous(path)
    assert reloaded.outdated() == []
    # A reading corrected in the file leaves the cell not current, serving the
    # statistic it recorded, and compute takes it again.
    path.write_text(text.replace("[1.0, 2.0, 3.0]", "[1.0, 2.0, 6.0]"))
    corrected = _Viscous(path)
    assert corrected.outdated() == ["toughness.stiffness"]
    assert corrected.toughness.at(40, "stiffness").value == 2.0
    corrected.compute()
    assert corrected.toughness.at(40, "stiffness").value == 3.0
    assert corrected.outdated() == []
    corrected.save()
    assert _Viscous(path).outdated() == []
    # Readings given to the cell in a session leave it not current in the
    # same way; a row replaced whole takes its statistic at once.
    again = _Viscous(path)
    again.toughness.at(40, "stiffness").readings = [3.0, 5.0]
    assert again.outdated() == ["toughness.stiffness"]
    again.toughness.update(40, stiffness=[7.0, 9.0])
    assert again.toughness.at(40, "stiffness").value == 8.0


def test_a_column_takes_only_statistics():
    # Each cell holds its own value, in the file: a column says how its
    # readings give it, never what it is.
    for given in (sk.Property(value=5), sk.Property(uncertainty=0.1), sk.Property([1.0, 2.0])):
        with pytest.raises(TypeError, match="each cell holds its own"):
            sk.Table({"T": sk.Property(), "x": given}, index="T")
    with pytest.raises(ValueError, match="standard_error"):
        sk.Property(value=sk.stats.standard_error)


def test_a_list_assigned_with_no_statistic_gives_no_value(project):
    path = project.sample("s.md", name="s", properties={"notch": "1.01"})
    sample = sk.Sample(path, model=False)
    sample.radius = sk.Property([1.05, 1.06])
    assert sample.radius.readings == [1.05, 1.06]
    # No statistic is declared, so no mean stands in, and none is written.
    assert sample.radius.value is None
    sample.save()
    text = path.read_text()
    assert "radius: {readings: [1.05, 1.06]}" in text


# ------------------------------------------------------------------


def test_a_figure_sums_up_what_it_read_not_current(tmp_path):
    # A warning per value read — each stale value, each value waiting — came
    # before the summary of what was left out: one summary each now.
    import matplotlib
    matplotlib.use("Agg")

    def ratio(name, a, b):
        sample = _Ratio(name=name)
        sample.a.value = a
        if b is not None:
            sample.b.value = b
        return sample

    current = ratio("current", 6.0, 2.0)
    outdated = ratio("outdated", 8.0, 2.0)
    waiting = ratio("waiting", 1.0, None)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        for sample in (current, outdated, waiting):
            sample.compute()
    outdated.a.value = 10.0
    samples = sk.SampleList([current, outdated, waiting])
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        sk.plot(samples, x="a", y="ratio", output=tmp_path / "f.png")
    said = [str(warning.message) for warning in caught]
    assert said == [
        "1 of 3 samples not drawn: no ratio",
        "drawn from values that are not current: 1 outdated in 1 sample — outdated ratio\n"
        "  not_current() says why, and compute() runs them again",
    ], said
    # At the script's line, as every warning of the package is.
    assert all(warning.filename == __file__ for warning in caught), caught


def test_an_empty_selection_is_refused_as_data(tmp_path):
    from samplekit import _figures

    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="no sample to draw: the selection is empty") as empty:
        sk.plot(samples.filter("diameter > 1000"), x="volume", y="diameter",
                output=tmp_path / "f.png")
    # The command line exits 2 for it, as for any figure of data it cannot draw.
    assert _figures.raised_by_the_data(empty.value)
    assert not (tmp_path / "f.png").exists()


def test_load_reads_a_duplicate_target_once(tmp_path):
    paths = figure_samples(tmp_path)
    with pytest.warns(UserWarning, match="^1 target was given more than once, read once$"):
        both = sk.load(paths[0], paths[0], paths[1])
    assert [sample.name for sample in both] == ["C1", "C2"]
    # A file and the folder holding it: one sample, where it was first met,
    # as the command line merges them.
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        whole = sk.load(paths[1], tmp_path)
    assert [sample.name for sample in whole][0] == "C2"
    assert sorted(sample.name for sample in whole) == ["C1", "C2", "C3"]


def test_a_failed_value_reads_as_its_last_value(tmp_path):
    # It read as None, where the file keeps the last value its formula gave.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    path = tmp_path / "r.md"
    path.write_text("---\nschema_version: 1\nname: r\nproperties:\n  a: 6.0\n  b: 2.0\n---\n")
    sample = _Ratio(path)
    sample.compute()
    sample.b.value = 0.0
    with pytest.raises(ZeroDivisionError):
        sample.compute()
    assert sample.ratio.state == "failed"
    with pytest.warns(UserWarning, match=r"^r: 'ratio' failed when last computed: .*; read as its last value$"):
        assert sample.ratio.value == 3.0
    sample.save()
    again = _Ratio(path)
    with pytest.warns(UserWarning, match="failed when last computed"):
        assert again.ratio.value == 3.0
    # A formula that never gave a value has none to read.
    fresh = _Ratio(name="fresh")
    fresh.a.value = 1.0
    fresh.b.value = 0.0
    with pytest.raises(ZeroDivisionError):
        fresh.compute()
    with pytest.warns(UserWarning, match="^fresh: 'ratio' failed when last computed: [^;]*$"):
        assert fresh.ratio.value is None


def test_save_takes_any_extension(tmp_path):
    # Where a user keeps a file is theirs: any name is written, and read back.
    sample = sk.Sample(name="kept")
    sample.diameter = sk.Property(1.5, unit="g")
    for name in ("kept.txt", "kept", "kept.sample.yaml"):
        path = tmp_path / name
        sample.save(path)
        assert sk.Sample(path, model=False).diameter.value == 1.5


def test_a_script_writing_only_a_figure_is_a_snapshot_of_its_own(tmp_path):
    # A script that saved nothing and wrote a figure was no step of the
    # history: its figure was tied to the last command's snapshot, and its
    # source was kept nowhere.
    figure_samples(tmp_path)
    env = {**os.environ, "MPLBACKEND": "Agg"}
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    # One that only prints is none.
    subprocess.run(
        [sys.executable, "-c",
         f"import samplekit as sk\nprint(sk.load({str(tmp_path)!r})[0].diameter.value)\n"],
        check=True, env=env, cwd=tmp_path, capture_output=True)
    assert not (tmp_path / ".samplekit/history").exists()
    script = tmp_path / "draw.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "samples = sk.load(sys.argv[1])\n"
        "samples.to_csv('table.csv', columns=['name', 'diameter'], overwrite=True)\n"
        "sk.plot(samples, x='volume', y='diameter', output='f.svg', overwrite=True)\n"
        "sk.plot(samples, x='volume', y='diameter', output='g.png', overwrite=True)\n"
    )
    subprocess.run([sys.executable, str(script), str(tmp_path)], check=True, env=env,
                   cwd=tmp_path)
    log = subprocess.run(git + ["log", "--format=%H %s"], capture_output=True, text=True,
                         check=True).stdout.splitlines()
    assert len(log) == 2, log
    own, said = log[0].split(" ", 1)
    assert said == f"python {quoted(script)} {quoted(tmp_path)}", said
    source = subprocess.run(git + ["cat-file", "-p", f"script/{own}^{{}}"],
                            capture_output=True, text=True, check=True).stdout
    assert source == script.read_text()
    # Each file it wrote is tied to that one snapshot.
    import hashlib
    for name in ("table.csv", "f.svg", "g.png"):
        digest = kept_digest(tmp_path / name)
        record = output_record(tmp_path / ".samplekit/history", digest)
        tied = subprocess.run(git + ["rev-parse", f"{record}^{{commit}}"],
                              capture_output=True, text=True, check=True).stdout.strip()
        assert tied == own, name
    # `log` over the project lists it, though it changed no kept file, and
    # names what it wrote.
    binary = BINARY
    if binary.exists():
        shown = subprocess.run([str(binary), "log"], cwd=tmp_path, capture_output=True,
                               text=True, check=True).stdout
        assert "no sample · log --script 1" in shown, shown
        assert "table.csv" in shown, shown
    # Run again, it is a step again.
    subprocess.run([sys.executable, str(script), str(tmp_path)], check=True, env=env,
                   cwd=tmp_path)
    count = subprocess.run(git + ["rev-list", "--count", "HEAD"], capture_output=True,
                           text=True, check=True).stdout.strip()
    assert count == "3"


def test_a_python_c_script_keeps_its_source(tmp_path):
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    (tmp_path / "c1.md").write_text(
        "---\nschema_version: 1\nname: c1\nproperties:\n  diameter: 1.0\n---\n")
    code = (
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.diameter.value = 4.0\n"
        "s.save()\n"
    )
    subprocess.run([sys.executable, "-c", code, "an argument"], check=True, cwd=tmp_path)
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    own, said = subprocess.run(git + ["log", "-1", "--format=%H %s"], capture_output=True,
                               text=True, check=True).stdout.strip().split(" ", 1)
    assert said == "python -c · saved c1.md", said
    kept = subprocess.run(git + ["cat-file", "-p", f"script/{own}^{{}}"],
                          capture_output=True, text=True, check=True).stdout
    assert kept == code
    binary = BINARY
    if binary.exists():
        shown = subprocess.run([str(binary), "log", "--script", "1"], cwd=tmp_path,
                               capture_output=True, text=True, check=True)
        assert shown.stdout == code, shown.stdout + shown.stderr
        assert "python -c" in shown.stderr


def test_a_group_over_several_fields_draws_their_combinations(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    for group in (["alloy", "name"], "alloy,name", "alloy, name"):
        (drawn,) = sk.plot(samples, x="volume", y="diameter", group=group,
                           output=tmp_path / "f.png", overwrite=True)
        legend = drawn.axes[0].get_legend()
        assert [text.get_text() for text in legend.get_texts()] == [
            "chrome, C2", "cobalt, C1", "cobalt, C3"]
        assert legend.get_title().get_text() == "alloy, name"
    # Each field is checked as one is: a misspelt one names the nearest.
    with pytest.raises(KeyError, match="name"):
        sk.plot(samples, x="volume", y="diameter", group=["alloy", "nmae"],
                output=tmp_path / "g.png")
    with pytest.raises(TypeError, match="group is a field, or several in a list"):
        sk.plot(samples, x="volume", y="diameter", group=3, output=tmp_path / "g.png")
    assert not (tmp_path / "g.png").exists()


def test_a_list_is_grouped_by_its_fields(tmp_path):
    # As --group splits a table, a dict in the groups' order.
    figure_samples(tmp_path)
    (tmp_path / "C4.md").write_text(
        "---\nschema_version: 1\nname: C4\nproperties:\n  diameter: {v: 9.0, unit: g}\n---\n")
    samples = sk.SampleList(tmp_path)
    groups = samples.group_by("alloy")
    assert list(groups) == ["chrome", "cobalt", None]
    assert [s.name for s in groups["cobalt"]] == ["C1", "C3"]
    assert isinstance(groups["chrome"], sk.SampleList)
    # The samples are the list's own.
    assert groups["cobalt"][0] is samples["C1"]
    for fields in (["alloy", "name"], "alloy,name", ("alloy", "name")):
        combined = samples.group_by(fields)
        assert list(combined) == [("chrome", "C2"), ("cobalt", "C1"), ("cobalt", "C3"),
                                  (None, "C4")]
    # A list sorted first is sorted within each group.
    heaviest = samples.sorted("-diameter").group_by("alloy")
    assert [s.name for s in heaviest["cobalt"]] == ["C1", "C3"]
    with pytest.raises(KeyError, match="alloy"):
        samples.group_by("alloi")
    with pytest.raises(TypeError, match="group_by takes a field"):
        samples.group_by(3)
    # Checked against the list it came from, as a filter is.
    nothing = samples.filter("diameter > 100")
    assert nothing.group_by("alloy") == {}
    with pytest.raises(KeyError, match="alloy"):
        nothing.group_by("alloi")


def test_a_filter_reads_the_state_of_each_sample(tmp_path):
    # `state` reads as the command line reads it.
    (tmp_path / ".samplekitrc").write_text(
        'schema_version = 1\n[model]\npath = "model.py"\nclass = "Chain"\n'
    )
    (tmp_path / "model.py").write_text(CHAIN)
    for name, extra in [("a", ""), ("b", ""), ("c", '  carbon: {v: "2.0"}\n')]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: 1.5}}\n{extra}---\n"
        )
    namespace = {"__file__": str(tmp_path / "model.py")}
    exec(CHAIN, namespace)
    failing = namespace["Chain"](tmp_path / "a.md")
    failing.diameter.value = datetime.date(2026, 9, 27)
    with pytest.raises(TypeError):
        failing.compute()
    failing.save()

    samples = sk.SampleList(tmp_path)
    names = lambda selected: sorted(sample.name for sample in selected)
    assert names(samples.filter("state == failed")) == ["a"]
    # a's `quadruple` never ran: what it reads failed.
    # Behind what failed, a's value waits, as status says it.
    assert names(samples.filter("state == never_computed")) == ["b", "c"]
    assert "a" in names(samples.filter("state == waiting"))
    # a's diameter is now a date among numbers, which `validate` finds too.
    assert names(samples.filter("state == defective")) == ["a", "c"]
    assert names(samples.filter("state == defective && !(state == failed)")) == ["c"]
    assert names(samples.filter("state == defective && state == never_computed")) == ["c"]
    with pytest.raises(ValueError, match="'stale' is not a state"):
        samples.filter("state == stale")



def chain_project(root, model=True):
    """Three samples of the chain model: a failed, b current, c holding a quoted number."""
    (root / ".samplekitrc").write_text(
        'schema_version = 1\n[model]\npath = "model.py"\nclass = "Chain"\n'
    )
    for name, extra in [("a", ""), ("b", ""), ("c", '  carbon: {v: "2.0"}\n')]:
        (root / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: {{v: 1.5}}\n{extra}---\n"
        )
    (root / "model.py").write_text(CHAIN)
    namespace = {"__file__": str(root / "model.py")}
    exec(CHAIN, namespace)
    failing = namespace["Chain"](root / "a.md")
    failing.diameter.value = datetime.date(2026, 9, 27)
    with pytest.raises(TypeError):
        failing.compute()
    failing.save()
    current = namespace["Chain"](root / "b.md")
    current.compute()
    current.save()
    if not model:
        # The project now names a model that is not there.
        (root / ".samplekitrc").write_text(
            'schema_version = 1\n[model]\npath = "gone.py"\nclass = "Chain"\n'
        )


def test_a_list_read_without_its_model_reads_it_for_state(tmp_path):
    # The model is read to answer `state`, as the command line reads it.
    chain_project(tmp_path)
    names = lambda selected: sorted(sample.name for sample in selected)
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        without = sk.SampleList(tmp_path, model=False)
        assert names(without.filter("state == never_computed")) == ["c"]
        assert names(without.filter("state == current")) == ["b"]
        assert names(without.filter("state == not_current")) == ["a", "c"]
        assert names(without.filter("state == failed")) == ["a"]
        bare = sk.SampleList([sk.Sample(tmp_path / "c.md", model=False)])
        assert names(bare.filter("state == never_computed")) == ["c"]
    # As read with the model.
    with_model = sk.SampleList(tmp_path)
    for word in ("never_computed", "current", "not_current", "failed", "defective"):
        assert names(without.filter(f"state == {word}")) == names(
            with_model.filter(f"state == {word}")), word


def test_a_state_without_a_readable_model_is_unknown_and_said(tmp_path):
    # Only a model that cannot be read leaves its words unknown.
    chain_project(tmp_path, model=False)
    names = lambda selected: sorted(sample.name for sample in selected)
    without = sk.SampleList(tmp_path, model=False)
    with pytest.warns(UserWarning, match=r"the model was not read for 3 samples \(.*gone\.py"):
        assert names(without.filter("state == never_computed")) == []
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert names(without.filter("state == failed")) == ["a"]


def test_state_is_a_column_worst_first(tmp_path):
    # `state` is a column, as `-c name,state` makes it.
    chain_project(tmp_path)
    for samples in (sk.SampleList(tmp_path), sk.SampleList(tmp_path, model=False)):
        samples = samples.sorted("name")
        with warnings.catch_warnings():
            warnings.simplefilter("error")
            table = samples.to_dict(columns=["name", "state"])
            assert table == {
                "name": ["a", "b", "c"],
                "state": [["failed", "defective", "waiting"], ["current"],
                          ["defective", "never_computed"]],
            }
            assert samples.to_csv(columns=["name", "state"]).splitlines() == [
                "name,state",
                "a,failed;defective;waiting",
                "b,current",
                "c,defective;never_computed",
            ]
        with pytest.raises(TypeError, match="'state' cannot be sorted by"):
            samples.sorted("state")
        with pytest.raises(TypeError, match="'state' cannot be sorted by"):
            samples.sorted(["name", "-state"])


def test_a_files_pattern_gives_a_sample_its_files(tmp_path):
    # A pattern naming the sample, beside the folders searched by name.
    (tmp_path / ".samplekitrc").write_text(
        'schema_version = 1\n[collection]\nfiles = ["images/{name}*.{jpg,png}", "raw/{name}/**"]\n'
    )
    for name in ["C2", "C24"]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"
        )
    for file in ["images/C2.jpg", "images/C2_top.png", "images/C2.tif", "images/C24.jpg",
                 "raw/C2/run/a.dat", "raw/C24/b.dat"]:
        (tmp_path / file).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / file).write_text("")
    c2 = sk.Sample(tmp_path / "C2.md", model=False)
    assert [p.name for p in c2.files()] == ["C2.jpg", "C2_top.png", "a.dat"]
    assert [p.name for p in c2.files("*.png")] == ["C2_top.png"]
    assert [p.name for p in sk.Sample(tmp_path / "C24.md", model=False).files()] == [
        "C24.jpg", "b.dat"]


def test_a_sorted_list_groups_in_its_own_order(tmp_path):
    # A sorted list groups in the order of its groups' first samples, as -s
    # orders --group's tables; unsorted, by the values.
    figure_samples(tmp_path)
    samples = sk.SampleList(tmp_path)
    assert list(samples.group_by("alloy")) == ["chrome", "cobalt"]
    heaviest = samples.sorted("-diameter")
    assert list(heaviest.group_by("alloy")) == ["cobalt", "chrome"]
    # Narrowed, the order is kept, and so is the list's being sorted.
    assert list(heaviest.filter("diameter > 1").group_by("alloy")) == ["cobalt", "chrome"]
    assert list(heaviest[0:3].group_by("alloy")) == ["cobalt", "chrome"]
    # Sorted in place, the same.
    inplace = samples.filter("diameter > 1")
    inplace.sort("-diameter")
    assert list(inplace.group_by("alloy")) == ["cobalt", "chrome"]
    # `+` makes a list nobody sorted: by the values again.
    joined = heaviest[0:1] + heaviest[1:]
    assert [s.name for s in joined] == ["C1", "C3", "C2"]
    assert list(joined.group_by("alloy")) == ["chrome", "cobalt"]


def test_a_profile_that_groups_writes_one_table_carrying_its_groups(tmp_path, cli):
    # A declared profile brings its groups to a data format, as `--profile NAME
    # --csv` writes it, and to a declared export.
    (tmp_path / ".samplekitrc").write_text(
        "schema_version = 1\n"
        '[profile.by_status]\ncolumns = [{field = "name"}, {field = "diameter"}]\n'
        'group = ["status"]\n'
        '[export.by_status]\nprofile = "by_status"\nformat = "csv"\n'
        'output = "out/by_status.csv"\n'
    )
    for name, status, diameter in [("A", "approved", 12.5), ("B", "rejected", 9.0),
                               ("C", "approved", 10.0)]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nstatus: {status}\n"
            f"properties:\n  diameter: {{v: {diameter}, u: 0.1, unit: g}}\n---\n"
        )
    samples = sk.SampleList(tmp_path)
    csv = samples.to_csv(profile="by_status")
    assert csv == cli(tmp_path, "--profile", "by_status", "--csv")
    lines = csv.splitlines()
    assert lines[0].startswith("status,name,")
    assert [line.split(",")[:2] for line in lines[1:]] == [
        ["approved", "A"], ["approved", "C"], ["rejected", "B"]]
    table = samples.to_dict(profile="by_status")
    assert list(table)[:2] == ["status", "name"]
    assert table["name"] == ["A", "C", "B"]
    written = samples.export("by_status")
    assert pathlib.Path(written).read_text().splitlines() == lines


def test_a_sample_without_a_name_is_named_by_its_file(tmp_path):
    # The file's name without its extension, where the file writes no `name:`;
    # assigned, the name is written and the file keeps its own; None takes it
    # away again. In memory with no file, there is none until it is saved.
    path = tmp_path / "s-1.md"
    path.write_text("---\nschema_version: 1\nproperties:\n  diameter: {v: 2.0}\n---\n")
    sample = sk.Sample(path)
    assert sample.name == "s-1"
    assert sk.SampleList(tmp_path)["s-1"].path == sample.path
    sample.name = "Slab A"
    sample.save()
    assert "name: Slab A" in path.read_text()
    assert sk.Sample(path).name == "Slab A"
    sample.name = None
    sample.save()
    assert "name:" not in path.read_text()
    assert sample.name == "s-1"
    with pytest.raises(ValueError, match="control character"):
        sample.name = "a\nb"
    unsaved = sk.Sample()
    assert unsaved.name is None
    unsaved.save(tmp_path / "s-2.md")
    assert unsaved.name == "s-2"
    assert "name:" not in (tmp_path / "s-2.md").read_text()


def test_a_symbol_is_read_and_never_set(project):
    # A setter let a model's __init__ give the symbol its constructor refuses,
    # and a save write it into the file.
    (project.root / ".samplekitrc").write_text(
        'schema_version = 1\n[property.diameter]\nsymbol = "m"\n')
    path = project.sample("s.md", name="s", properties={"diameter": "{v: 1.0, unit: g}"})
    sample = sk.Sample(path, model=False)
    assert sample.diameter.symbol == "m"
    with pytest.raises(AttributeError):
        sample.diameter.symbol = "M"
    assert sk.Property(value=1.0).symbol is None


def test_a_table_formula_reads_the_samples_name(tmp_path):
    # Refused while the tables resolved: a model rebuilt the name from the
    # file's path to find the reports a row reads.
    path = tmp_path / "a-1.md"
    path.write_text("---\nschema_version: 1\ntables:\n  t:\n    index: N\n"
                    "    columns:\n      N: {}\n      who: {}\n    rows:\n      - {N: 1}\n---\n")

    class Named(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.t = sk.Table({
                "N": sk.Property(),
                "who": sk.Property(compute=lambda row: f"{self.name}#{int(row.N.value)}", depends_on=["row.N"]),
            }, index="N")

    sample = Named(path)
    sample.compute()
    assert sample.t.who.values == ["a-1#1"]


def test_a_value_computed_from_python_is_kept_in_the_trace(project, tmp_path):
    # A script computes and saves: the formula that computed each value is kept
    # in the project's trace, as a computation of the command line keeps it, so
    # that a formula edited afterwards is said of what the script computed.
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\npython = ".venv"\n')
    (project.root / ".venv").symlink_to(pathlib.Path(sys.prefix))
    sample = sk.Sample(project.sample("s.md", properties={"diameter": "2.0"}))
    sample.compute()
    sample.save()
    sk.keep("doubled")
    tags = subprocess.run(
        ["git", "--git-dir", str(project.root / ".samplekit/history"), "tag", "--list", "trace/*"],
        capture_output=True, text=True, check=True,
    ).stdout.split()
    assert len(tags) == 1, tags
    project.model(MODEL.replace("self.diameter.value * 2", "self.diameter.value * 3"))
    if BINARY.exists():
        status = subprocess.run(
            [str(BINARY), ".", "status"], cwd=project.root, capture_output=True, text=True,
            env={**os.environ, "SAMPLEKIT_STATE_DIR": str(tmp_path / "state"), "NO_COLOR": "1"},
        )
        assert "formula changed" in status.stdout, status.stdout + status.stderr


HARDNESS = """\
import samplekit as sk


class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.double = sk.Property(compute_value=lambda: 2 * self.radius.value)
        self.set_dependencies("double", depends_on=["radius"])
"""


def _hardness(project):
    project.model(HARDNESS)
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    path = project.sample("s.md", properties={"radius": "{readings: [1.063, 1.065, 1.063]}"})
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    return path


def test_a_reading_added_keeps_its_value_and_uncertainty_together(project):
    # A reading appended leaves the value and its uncertainty the pair the
    # statistic last gave, outdated together, until it is taken again.
    path = _hardness(project)
    sample = sk.Sample(path)
    before = (sample.radius.value, sample.radius.uncertainty)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        sample.radius.readings.append(1.064)
        assert (sample.radius.value, sample.radius.uncertainty) == before
        assert sample.radius.state == "outdated"


def test_a_statistic_computed_by_name_is_current(project):
    # `Property.compute()` and `compute("radius")` take the statistic again and
    # record it, as a whole computation does: current, what reads it outdated.
    path = _hardness(project)
    for compute in (lambda s: s.radius.compute(), lambda s: s.compute("radius")):
        sample = sk.Sample(path)
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            sample.radius.readings.append(1.064)
            compute(sample)
            assert sample.radius.state == "current"
            assert sample.radius.value == pytest.approx(1.06375)
            assert sample.not_current() == {"double": "outdated"}


def test_a_statistic_or_an_export_over_two_units_is_refused(tmp_path):
    # `stats`, `to_csv` and `to_dict` refuse a column written in two units, as
    # a table does: a mean of litres and millilitres compares nothing.
    (tmp_path / ".samplekitrc").write_text('schema_version = 1\n[property.volume]\nunit = "L"\n')
    for name, volume, unit in (("a", 20, "L"), ("b", 21, "L"), ("c", 20000, "mL")):
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  volume: {{v: {volume}, unit: {unit}}}\n---\n"
        )
    pieces = sk.load(tmp_path)
    for call in (lambda: pieces.stats("volume"), lambda: pieces.to_csv(columns="name,volume"),
                 lambda: pieces.to_dict(columns="name,volume")):
        with pytest.raises(ValueError, match="volume is in L and mL|'volume' is in L and mL"):
            call()


def test_a_changed_formula_is_outdated_and_computed_again_from_python(project, tmp_path):
    # A formula edited since it computed a value: `.state` says outdated,
    # `not_current()` names it, and `compute()` runs it again, as the command
    # line does — the project's trace says which.
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\npython = ".venv"\n')
    (project.root / ".venv").symlink_to(pathlib.Path(sys.prefix))
    path = project.sample("s.md", properties={"diameter": "2.0"})
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    sk.keep("doubled")
    # Another length: an edit in the same second, the same size, is read from
    # the bytecode Python cached of the model, whatever SampleKit does.
    project.model(MODEL.replace("self.diameter.value * 2", "self.diameter.value * 3.0"))
    if BINARY.exists():
        # The description written again, as the first command after an edit does.
        subprocess.run([str(BINARY), ".", "status"], cwd=project.root, capture_output=True,
                       env={**os.environ, "SAMPLEKIT_STATE_DIR": str(tmp_path / "state")})
    # A script run after the edit, which imports the model as it now is.
    script = (
        "import samplekit as sk, sys\n"
        f"s = sk.Sample({str(path)!r})\n"
        "print(s.double.state, s.not_current())\n"
        "s.compute()\n"
        "print(s.double.value)\n"
    )
    ran = subprocess.run([sys.executable, "-c", script], capture_output=True, text=True,
                         cwd=project.root, check=True)
    said, value = ran.stdout.strip().splitlines()
    assert said == "outdated {'double': 'formula changed'}", ran.stdout + ran.stderr
    assert float(value) == 6.0


def test_a_text_no_sample_holds_is_warned_with_the_nearest(tmp_path):
    # `style == titaniumm` gave an empty list in silence, and `stats` or `to_csv`
    # then failed far from the cause.
    (tmp_path / "a.md").write_text("---\nschema_version: 1\nstyle: titanium\n---\n")
    (tmp_path / "b.md").write_text("---\nschema_version: 1\nstyle: nickel\n---\n")
    samples = sk.load(tmp_path)
    with pytest.warns(UserWarning, match="did you mean 'titanium'"):
        assert len(samples.filter("style == titaniumm")) == 0
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert len(samples.filter("style == titanium")) == 1


def test_a_sample_read_by_a_relative_path_is_saved_there_after_a_chdir(tmp_path, monkeypatch):
    # Held relative to the folder the script was in, a sample read then saved
    # after `os.chdir` was said *removed or moved*, and `overwrite=True` wrote
    # it into the other folder.
    (tmp_path / "pieces").mkdir()
    (tmp_path / "other").mkdir()
    (tmp_path / "pieces" / "a.md").write_text("---\nschema_version: 1\nstyle: titanium\n---\n")
    monkeypatch.chdir(tmp_path)
    sample = sk.Sample("pieces/a.md")
    assert sample.path.is_absolute()
    monkeypatch.chdir(tmp_path / "other")
    sample.style = "nickel"
    sample.save()
    assert "style: nickel" in (tmp_path / "pieces" / "a.md").read_text()
    assert not (tmp_path / "other" / "pieces").exists()


def test_sorting_by_a_row_no_sample_holds_is_refused(tmp_path):
    # `sorted("ageing.hardness[99]")` gave the list back as it was,
    # where `filter` and `to_csv` refuse a row no sample holds.
    for name, hardness in (("a", 1.04), ("b", 1.03)):
        (tmp_path / f"{name}.md").write_text(
            "---\nschema_version: 1\ntables:\n  ageing:\n    index: day\n    columns:\n"
            f"      day: {{}}\n      hardness: {{}}\n    rows:\n      - day: 1\n        hardness: {hardness}\n---\n"
        )
    samples = sk.load(tmp_path)
    assert names(samples.sorted("ageing.hardness[1]")) == ["b", "a"]
    with pytest.raises(KeyError, match="99"):
        samples.sorted("ageing.hardness[99]")


def test_a_text_shaped_as_a_date_is_refused_naming_its_field(tmp_path):
    # `poured = "2025-03-02"` refused the save as *does not read back*, with
    # nothing to say which field, nor why.
    path = tmp_path / "a.md"
    path.write_text("---\nschema_version: 1\nstyle: titanium\n---\n")
    sample = sk.Sample(path)
    sample.poured = "2025-03-02"
    with pytest.raises(Exception) as caught:
        sample.save()
    said = str(caught.value)
    assert "poured" in said, said
    assert "read as a date" in said, said
    sample.poured = datetime.date(2025, 3, 2)
    sample.save()
    assert "poured: 2025-03-02" in path.read_text()


def test_a_folder_misspelt_is_suggested_and_an_array_said_as_given(tmp_path, monkeypatch):
    # `sk.load("piece")` beside `pieces/` suggested nothing, and an array given
    # to a value was said to be *a formula's* return.
    (tmp_path / "pieces").mkdir()
    (tmp_path / "pieces" / "a.md").write_text("---\nschema_version: 1\nproperties:\n  radius: 1.05\n---\n")
    monkeypatch.chdir(tmp_path)
    with pytest.raises(FileNotFoundError, match="nearest: .*pieces"):
        sk.load("piece")
    sample = sk.load("pieces/a.md")
    with pytest.raises(TypeError) as caught:
        sample.radius.value = numpy.array([1.0, 2.0])
    assert "formula" not in str(caught.value)
    assert "array of shape (2,)" in str(caught.value)


TEXT_FORMULA = """\
from samplekit import Property, Sample


class Worded(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.diameter = Property()
        self.verdict = Property(compute_value=lambda: "strong")
        self.set_dependencies("verdict", depends_on=["diameter"])
"""


def test_a_formula_giving_text_for_a_quantity_fails(project):
    # A quantity holds a number: the text a formula gives is refused, never
    # written for what reads it next.
    sample_path = project.sample("s.md", properties={"diameter": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(TEXT_FORMULA, namespace)
    sample = namespace["Worded"](sample_path)
    with pytest.raises(TypeError, match="returned the text \"strong\": a computed property holds a number"):
        sample.compute()
    assert sample.verdict.value is None


def test_text_is_refused_in_a_column_with_a_unit(tmp_path):
    path = tmp_path / "b.md"
    path.write_text(
        "---\nschema_version: 1\nname: b\ntables:\n  t:\n    index: d\n"
        "    columns: {d: {}, g: {unit: P}, note: {}}\n    rows:\n      - {d: 1, g: 12.0}\n---\n"
    )
    piece = sk.Sample(path, model=False)
    with pytest.raises(ValueError, match="'g' is measured in P"):
        piece.t.add(d=2, g="warm")
    piece.t.add(d=3, g=11.5, note="worn")
    assert piece.t.at(3, "note").value == "worn"


def test_a_value_whose_input_was_emptied_says_it_waits(project):
    sample_path = project.sample("s.md", properties={"diameter": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CHAIN, namespace)
    sample = namespace["Chain"](sample_path)
    sample.compute()
    sample.save()
    # Emptied as `set diameter=` empties it: the key stays, without a value.
    emptied, count = re.subn(r"(\n  diameter:)[^\n]*", r"\1 {}", sample_path.read_text())
    assert count == 1
    sample_path.write_text(emptied)
    again = sk.Sample(sample_path, model=False)
    assert again.double.state == "waiting"
    assert again.double.value == 2.5


def test_save_all_without_a_folder_writes_each_back_with_its_check(collection):
    # save_all(its folder, overwrite=True) was the only way, and wrote over a
    # change made meanwhile unchecked.
    samples = sk.SampleList(collection.root)
    for sample in samples:
        sample.tags = list(sample.tags) + ["seen"]
    written = samples.save_all()
    assert len(written) == len(samples)
    assert all("seen" in path.read_text() for path in written)
    # Changed on disk since it was read: refused, as save() refuses it.
    again = sk.SampleList(collection.root)
    first = again[0]
    first.tags = list(first.tags) + ["twice"]
    first_path = written[[path.name for path in written].index(pathlib.Path(first.path).name)]
    first_path.write_text(first_path.read_text().replace("seen", "edited"))
    for sample in again:
        sample.tags = list(sample.tags) + ["late"]
    # Every sample checked first: the one changed since is named, and none of
    # the others is written — those before it were, and the error said
    # nothing was.
    with pytest.raises(OSError, match="changed since it was read"):
        again.save_all()
    assert not any("late" in path.read_text() for path in written)
    # A sample with no file of its own is named, and nothing written.
    loose = sk.SampleList([sk.Sample(name="loose")])
    with pytest.raises(ValueError, match="no file of its own"):
        loose.save_all()


def test_a_sort_and_a_filter_refuse_two_units(project):
    # Litres and millilitres: `sorted` put 21000 mL after 20 L, and a filter
    # compared 21000 with 20 of no unit, keeping it.
    project.config("schema_version = 1\n")
    project.sample("a.md", name="a", properties={"volume": "{v: 20, unit: L}"})
    project.sample("b.md", name="b", properties={"volume": "{v: 21000, unit: mL}"})
    samples = sk.SampleList(project.root)
    for asked in (lambda: samples.sorted("volume"), lambda: samples.filter("volume > 20")):
        with pytest.raises(ValueError, match="'volume' is in L and mL across these samples"):
            asked()
    # A field compared with no number is no comparison of units.
    assert len(samples.filter("volume is present")) == 2


def test_an_attribute_changing_kind_and_a_tag_twice_warn(project):
    project.config("schema_version = 1\n")
    path = project.sample(
        "a.md", name="a", tags=["inspected"], attributes={"style": "titanium", "poured": "2026-01-25"}
    )
    sample = sk.Sample(path)
    with pytest.warns(UserWarning, match="'style' was text and is now a number"):
        sample.style = 5
    with pytest.warns(UserWarning, match="'poured' was a date and is now text"):
        sample.poured = "25 January"
    with pytest.warns(UserWarning, match="'inspected' is a tag of this sample already"):
        sample.tags.append("inspected")
    assert sample.tags == ["inspected"]


def test_not_applicable_reads_as_nothing_in_a_dict_and_as_nan(project):
    import math

    project.config("schema_version = 1\n")
    project.sample("a.md", name="a", properties={"notch": "n/a"})
    project.sample("b.md", name="b", properties={"notch": "1.01"})
    columns = sk.SampleList(project.root).to_dict(columns=["name", "notch"])
    assert columns["notch_value"] == [None, 1.01]
    assert math.isnan(float(sk.NA))


def test_a_save_writes_only_what_the_script_changed(project):
    # A tag appended, and the save wrote the readings' statistics, their
    # records, and an empty key for every value the model declares.
    project.config('schema_version = 1\n[model]\npath = "model.py"\n')
    project.model(
        "import samplekit as sk\n\n\nclass Piece(sk.Sample):\n"
        "    def __init__(self, path=None, name=None):\n"
        "        super().__init__(path, name=name)\n"
        "        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)\n"
        "        self.modulus = sk.Property(unit='%', compute_value=lambda: self.radius.value * 100)\n"
        "        self.set_dependencies('modulus', depends_on=['radius'])\n"
    )
    path = project.sample("a.md", name="a", properties={"radius": "{readings: [1.05, 1.052]}"})
    sample = sk.Sample(path)
    sample.tags.append("py")
    sample.save()
    written = path.read_text()
    assert "tags: [py]" in written
    assert "statistics" not in written and "modulus" not in written and "v:" not in written
    # Computed, the statistic and what reads it are written.
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    written = path.read_text()
    assert "statistics: {v: mean, u: standard_error}" in written and "modulus:" in written


def test_to_text_is_the_command_lines_text_format(collection, cli):
    samples = sk.SampleList(collection.root)
    root = collection.root
    latex = samples.to_text("latex", profile="ductilities")
    assert latex == cli(root, "--profile", "ductilities", "--style", "latex", "-o", "-")
    assert latex.startswith("\\begin{tabular}")
    assert samples.to_text("markdown", columns="name").startswith("| name |\n| --- |\n")
    with pytest.raises(ValueError, match="latex"):
        samples.to_text("latx", columns="name")


def test_a_formula_changed_to_the_same_number_is_current_once_computed(project, tmp_path):
    # A formula rewritten without changing its number — `* 2` into `* 4.0 / 2`
    # — stayed *formula changed* for ever from Python: `compute(rerun=True)`
    # gave the very record it held, and the save kept no fact of the new
    # formula. `state`, `is_outdated` and `outdated()` disagreed meanwhile,
    # and reading the value warned nothing.
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\npython = ".venv"\n')
    (project.root / ".venv").symlink_to(pathlib.Path(sys.prefix))
    path = project.sample("s.md", properties={"diameter": "2.0"})
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    sk.keep("doubled")
    project.model(MODEL.replace("self.diameter.value * 2", "self.diameter.value * 4.0 / 2"))
    if BINARY.exists():
        subprocess.run([str(BINARY), ".", "status"], cwd=project.root, capture_output=True,
                       env={**os.environ, "SAMPLEKIT_STATE_DIR": str(tmp_path / "state")})
    script = (
        "import samplekit as sk, warnings\n"
        f"s = sk.Sample({str(path)!r})\n"
        "print(s.double.state, s.double.is_outdated, s.outdated())\n"
        "with warnings.catch_warnings(record=True) as heard:\n"
        "    warnings.simplefilter('always')\n"
        "    s.double.value\n"
        "print(any('formula changed' in str(w.message) for w in heard))\n"
        "s.compute(rerun=True)\n"
        "print(s.not_current(), s.double.state)\n"
        "s.save()\n"
        "sk.keep('again')\n"
        "print(sk.Sample(s.path).not_current())\n"
    )
    ran = subprocess.run([sys.executable, "-c", script], capture_output=True, text=True,
                         cwd=project.root, check=True)
    lines = ran.stdout.strip().splitlines()
    assert lines == [
        "outdated True ['double']",
        "True",
        "{} current",
        "{}",
    ], ran.stdout + ran.stderr


READINGS_MODEL = """\
from samplekit import Property, Sample, stats


class Model(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.radius = Property(value=stats.mean, uncertainty=stats.standard_error)
        self.notch = Property(value=stats.mean, uncertainty=stats.standard_error)
        self.modulus = Property(compute_value=lambda: (self.radius.value - self.notch.value) * 131.25)
        self.set_dependencies("modulus", depends_on=["radius", "notch"])
"""


def test_a_sample_begun_is_computed_as_far_as_it_goes(project):
    # A piece on its first day, its final hardness not measured: `compute()`
    # raised *after 11 rounds* and computed nothing, where the command line
    # takes the hardness and lets the modulus wait.
    project.model(READINGS_MODEL)
    project.config('schema_version = 1\n[model]\npath = "model.py"\n')
    sample = sk.Sample.new(project.root / "fresh.md")
    sample.radius.readings = [1.050, 1.051]
    with pytest.warns(UserWarning, match="'modulus' waits for notch"):
        sample.compute()
    assert sample.radius.value == pytest.approx(1.0505)
    assert sample.not_current() == {"modulus": "waits for notch"}


def test_readings_read_before_they_were_assigned_again_are_bound_to_nothing(project):
    # A list read before the readings were assigned again replaced them, in
    # silence, when it was appended to.
    path = project.sample("s.md", properties={"radius": "{readings: [1.052, 1.053]}"})
    sample = sk.Sample(path, model=False)
    old = sample.radius.readings
    sample.radius.readings = [1.060, 1.061, 1.062]
    old.append(1.070)
    assert sample.radius.readings == [1.060, 1.061, 1.062]
    assert old == [1.052, 1.053, 1.070]
    # The list read now is bound, as before.
    sample.radius.readings.append(1.063)
    assert sample.radius.readings == [1.060, 1.061, 1.062, 1.063]


def test_the_analysts_small_findings(project, tmp_path):
    # Each said wrongly, or not at all: a statistic `invalidate` left alone,
    # lists of names unsorted, a path assigned through an item advised as an
    # attribute `radius_v`, `s["tags"]` unknown, a name two samples carry indexed
    # in silence, a symbol's variants keyed by the file's key, the collection's
    # `files` missing, a query two projects declare alike refused, a file the
    # collection ignores created, one number taken for readings, a hand-made
    # BoundList broken, a format specifier said twice, `divmod` refused.
    project.model(READINGS_MODEL)
    project.config(
        'schema_version = 1\n[model]\npath = "model.py"\n[style.math]\n'
        '[property.radius]\nsymbol_math = "RADIUS"\n[collection]\nfiles = ["photos"]\n'
        '[query.strong]\nfilter = "radius > 1"\n'
    )
    path = project.sample(
        "a.md", name="EXAMPLE", tags=["t"],
        properties={"radius": "{readings: [1.05, 1.06]}", "notch": "{readings: [1.01, 1.012]}"},
    )
    project.sample("b.md", name="EXAMPLE", properties={"radius": "{readings: [1.04]}"})
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    again = sk.Sample(path)
    assert again.dependencies("modulus") == ["notch", "radius"]
    assert again.affected_by_change("radius") == sorted(again.affected_by_change("radius"))
    with pytest.raises(ValueError, match="s.radius.value = <value>"):
        again["radius.v"] = 1.07
    assert list(again["tags"]) == ["t"]
    pieces = sk.load(project.root)
    with pytest.raises(ValueError, match="2 samples are named 'EXAMPLE'"):
        pieces.index("EXAMPLE")
    assert pieces.project.properties["radius"].symbol_variants == {"math": "RADIUS"}
    assert pieces.project.collection.files == ("photos",)
    other = tmp_path / "other"
    other.mkdir()
    (other / ".samplekitrc").write_text('schema_version = 1\n[query.strong]\nfilter = "radius > 1"\n')
    (other / "c.md").write_text("---\nschema_version: 1\nname: C\nproperties:\n  radius: 1.2\n---\n")
    both = sk.load(project.root) + sk.load(other)
    assert len(both.query("strong")) >= 1
    with pytest.raises(ValueError, match="noext.md is one"):
        sk.Sample.new(project.root / "noext")
    with pytest.raises(TypeError, match="one number is not readings"):
        again.notch.readings = 1.011
    bound = sk.BoundList([1, 2])
    bound.append(3)
    assert bound == [1, 2, 3]
    with pytest.raises(ValueError) as refused:
        format(sample.modulus, ">10")
    assert str(refused.value).count("precision") <= 2
    assert "is not a precision the command line accepts" not in str(refused.value)
    assert divmod(sample.modulus, 1) == (sample.modulus // 1, sample.modulus % 1)


def test_an_export_of_values_not_current_says_so(project):
    # Written from values that are not current, the file for the paper left
    # with nothing said from Python, where `samplekit export --write` says it.
    project.model()
    project.config(
        'schema_version = 1\n[model]\npath = "model.py"\n'
        '[profile.p]\ncolumns = [{field = "diameter"}, {field = "double"}]\n'
        '[export.e]\nprofile = "p"\nformat = "csv"\noutput = "e.csv"\n'
    )
    path = project.sample("s.md", properties={"diameter": "{v: 2.0, unit: kg}"})
    sample = sk.Sample(path)
    sample.compute()
    sample.save()
    path.write_text(path.read_text().replace("v: 2.0", "v: 3.0"))
    pieces = sk.load(project.root)
    with pytest.warns(UserWarning, match="'e' is written from values that are not current: "
                                         "1 outdated in 1 sample"):
        pieces.export("e", overwrite=True)


def test_keep_warns_where_nothing_was_saved(tmp_path, monkeypatch):
    # `sk.keep("…")` did nothing without a word, nothing saved or outside
    # any project.
    monkeypatch.chdir(tmp_path)
    with pytest.warns(UserWarning, match="nothing was saved in a project since the last snapshot"):
        sk.keep("volumes corrected")


def test_a_value_typed_nowhere_is_absent_not_entered():
    # `state == "entered"` was said of a quantity holding nothing.
    sample = sk.Sample(name="S")
    sample.volume = sk.Property(unit="L")
    assert sample.volume.state == "absent"
    sample.volume.value = 20.0
    assert sample.volume.state == "entered"


def test_a_summary_counts_what_is_not_applicable_apart(collection):
    # `Summary` said nothing of the samples a value does not apply to.
    for path in sorted(collection.root.glob("*.md"))[:1]:
        text = path.read_text()
        path.write_text(text.replace("{v: 12.5, u: 0.05, unit: g}", "{v: n/a, unit: g}"))
    summary = sk.SampleList(collection.root).stats("diameter")
    assert summary.not_applicable == 1, repr(summary)
    assert summary.count == 2
    assert summary.considered == 2
    assert "not_applicable=1" in repr(summary)
    readings = sk.Sample(name="R")
    readings.x = sk.Property(value=sk.stats.mean)
    readings.x.readings = [1.0, 2.0]
    assert readings.x.stats.considered is None


def test_a_property_is_written_in_a_style(tmp_path):
    # A property alone could not be written in `\SI{…}{…}`: only a text
    # format's table could, and a style gave its units alone.
    (tmp_path / ".samplekitrc").write_text(
        "schema_version = 1\n[style.si]\n"
        "property = '\\SI{{value}{uncertainty}}{{unit}}'\n"
        "uncertainty = ' \\pm {uncertainty}'\n"
        "[unit.g]\nsi = '\\gram'\n")
    (tmp_path / "a.md").write_text(
        "---\nschema_version: 1\nname: A\nproperties:\n"
        "  diameter: {v: 12.1, u: 0.05, unit: g}\n  carbon: {v: 3.0, unit: g}\n---\n")
    sample = sk.Sample(tmp_path / "a.md")
    assert sample.diameter.to_text("si") == "\\SI{12.100 \\pm 0.050}{\\gram}"
    assert sample.carbon.to_text("si") == "\\SI{3}{\\gram}"
    assert sample.diameter.to_text() == str(sample.diameter)
    with pytest.raises(ValueError, match="no style named 'sx'"):
        sample.diameter.to_text("sx")


def test_a_script_is_marked_and_its_options_read_by_its_signature():
    from samplekit._scripts import arguments, described, scripts_of

    class Poured(sk.Sample):
        @sk.script
        def dilute(self, factor: float = 0.9, note="", loud=False):
            """Scale the hardness's excess.

            Not in the description.
            """
            return factor

        @sk.script
        @classmethod
        def report(cls, samples, count: int):
            return count

    found = scripts_of(Poured)
    assert list(found) == ["dilute", "report"]
    assert not found["dilute"].collection and found["report"].collection
    assert described(found["dilute"]) == {
        "name": "dilute",
        "acts": "sample",
        "description": "Scale the hardness's excess.",
        "options": [
            {"name": "factor", "type": "float", "default": "0.9"},
            {"name": "note", "type": "str", "default": ""},
            {"name": "loud", "type": "bool", "default": "false"},
        ],
    }
    assert described(found["report"])["options"] == [
        {"name": "count", "type": "int", "default": None}]
    assert arguments(found["dilute"], {"factor": "0.5", "loud": "yes"}) == {
        "factor": 0.5, "loud": True}
    with pytest.raises(ValueError, match="needs count"):
        arguments(found["report"], {})
    with pytest.raises(ValueError, match="takes no option 'fator'"):
        arguments(found["dilute"], {"fator": "1"})
    # Still the method it marks.
    assert Poured().dilute(2) == 2
    # A static method, *args, and the decorator beneath @classmethod refused.
    with pytest.raises(TypeError, match="static method"):
        sk.script(staticmethod(lambda: None))
    with pytest.raises(TypeError, match="named parameters"):
        sk.script(lambda self, *rest: None)

    class Misplaced(sk.Sample):
        @classmethod
        @sk.script
        def wrong(cls, samples):
            pass

    with pytest.raises(TypeError, match="goes above @classmethod"):
        scripts_of(Misplaced)


def test_a_table_a_sample_and_a_summary_are_written_as_text(tmp_path):
    (tmp_path / ".samplekitrc").write_text(
        "schema_version = 1\n[style.math]\n[unit.g]\nmath = '\\si{\\gram}'\n")
    (tmp_path / "a.md").write_text(
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 12.0, u: 0.1, unit: g}\n"
        "tables:\n  m:\n    index: T\n    columns:\n      T: {unit: C}\n      w: {unit: g}\n"
        "    rows:\n      - {T: 20.0, w: 1.5}\n---\n")
    sample = sk.Sample(tmp_path / "a.md", model=False)
    text = sample.to_text()
    assert text.startswith("A\n  tags       —\n  diameter   12.00 ± 0.10 g")
    assert "  m · 1 row" in text
    assert sample.m.to_text("math").splitlines()[1].split() == ["T", "[C]", "w", "[\\si{\\gram}]"]
    with pytest.raises(ValueError, match="no style named 'mth'"):
        sample.to_text("mth")
    summary = sk.SampleList([sample]).stats("diameter").to_text()
    assert summary.splitlines()[0].split()[:2] == ["n", "mean"]
    # The row `--summary` writes: its label and unit, then the count.
    assert summary.splitlines()[-1].split()[:3] == ["diameter", "[g]", "1/1"]


def test_readings_never_taken_give_no_value_nor_spread(project):
    # Reading computes nothing, in Python as on the command line: readings
    # whose statistic nothing took give neither a mean nor a standard error,
    # which `to_csv` wrote beside an empty value.
    project.model(HARDNESS)
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    path = project.sample("s.md", properties={"radius": "{readings: [1.063, 1.065, 1.063]}"})
    sample = sk.Sample(path)
    with pytest.warns(UserWarning, match="'radius' was never computed"):
        assert sample.radius.value is None
    assert sample.radius.uncertainty is None
    assert sample.radius.state == "never_computed"
    with pytest.warns(UserWarning, match="radius_uncertainty is left out"):
        rows = sk.SampleList([sample]).to_csv(columns="name,radius").splitlines()
    assert rows == ["name,radius_value", "s,"], rows


def test_readings_saved_before_a_computation_stay_owed(project, tmp_path):
    # Saved without `compute()`, readings were written with a record of a
    # statistic nobody took and no value: edited by hand for the command line,
    # and no computation would ever take them again.
    project.model(HARDNESS)
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\npython = ".venv"\n')
    (project.root / ".venv").symlink_to(pathlib.Path(sys.prefix))
    path = project.sample("s.md")
    sample = sk.Sample(path)
    sample.radius.readings = [1.063, 1.065, 1.063]
    sample.save()
    text = path.read_text()
    assert "computed:" not in text and "statistics:" not in text, text
    assert sk.Sample(path).radius.state == "never_computed"
    if BINARY.exists():
        env = {**os.environ, "SAMPLEKIT_STATE_DIR": str(tmp_path / "state"), "NO_COLOR": "1"}
        status = subprocess.run(
            [str(BINARY), ".", "status"], cwd=project.root, capture_output=True, text=True, env=env,
        )
        assert re.search(r"radius\s+never computed", status.stdout), status.stdout + status.stderr
        subprocess.run(
            [str(BINARY), ".", "compute", "--write"], cwd=project.root, capture_output=True,
            text=True, env=env, check=True,
        )
        assert sk.Sample(path).radius.value == pytest.approx(1.0636666, abs=1e-6)


class RowsOnSwarf(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.swarf = sk.Property(compute_value=lambda: 4.0, depends_on=[])
        self.preheat = sk.Table(
            {
                "T": sk.Property(),
                "R": sk.Property(),
                "G": sk.Property(compute=lambda row: self.swarf.value * row.R.value, depends_on=["swarf", "row.R"]),
            },
            index="T",
            rows=[{"T": 1, "R": 2.0}, {"T": 2, "R": 3.0}],
        )


def test_a_cells_edited_upstream_names_what_its_row_rests_on():
    # A cell answered `[]` whatever its formula read.
    sample = RowsOnSwarf(name="r")
    sample.compute()
    assert sample.preheat.at(1, "G").edited_upstream == []
    sample.swarf.value = 8.0
    sample.preheat.at(2, "R").value = 5.0
    sample.compute()
    assert sample.preheat.at(1, "G").edited_upstream == ["swarf"]
    assert sample.preheat.at(2, "G").edited_upstream == ["swarf"]


def test_save_all_refuses_a_read_only_file_before_writing_any(collection):
    # A read-only file stopped the list half written, the error saying
    # nothing was.
    samples = sk.SampleList(collection.root)
    for sample in samples:
        sample.tags = list(sample.tags) + ["seen"]
    locked = pathlib.Path(samples[-1].path)
    locked.chmod(0o444)
    try:
        with pytest.raises(OSError, match="read-only"):
            samples.save_all()
        assert not any(
            "seen" in pathlib.Path(sample.path).read_text() for sample in samples
        )
    finally:
        locked.chmod(0o644)


def test_a_save_refused_says_what_changed_and_how_to_keep_it(collection):
    # `overwrite=True` alone was offered, which wrote over the other change
    # without a word of what it lost.
    sample = sk.SampleList(collection.root)[0]
    path = pathlib.Path(sample.path)
    sample.tags = list(sample.tags) + ["mine"]
    path.write_text(path.read_text().replace("schema_version: 1\n", "schema_version: 1\ntester: them\n", 1))
    with pytest.raises(OSError) as refused:
        sample.save()
    said = str(refused.value)
    assert "tester" in said, said
    assert "read it again" in said and "what changed there is lost" in said, said
    assert "tester: them" in path.read_text()


class Graded(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.mass = sk.Property(unit="g")
        self.graded = sk.Property(compute_value=lambda: self.mass.value * {"A": 2.0}.get(self.batch, 1.0),
                                  depends_on=["mass", "batch"])


def test_an_input_its_files_give_is_held_empty_until_they_do(tmp_path):
    # A formula reading an attribute its files give, which the model does not
    # declare: the model was refused as made without a file, and so could
    # not be described.
    sample = Graded(name="g")
    assert sample.dependencies("graded") == ["batch", "mass"]
    sample.mass.value = 2.0
    sample.compute()
    assert "graded" in sample.not_current()
    sample.batch = "A"
    sample.compute()
    assert sample.graded.value == 4.0


def test_remove_deletes_the_file_and_the_history_gives_it_back(collection, cli):
    path = collection.root / "B.md"
    written = path.read_text()
    sample = sk.Sample(path)
    sample.remove()
    assert not path.exists()
    # Its values stay, and a save writes it again.
    assert sample.diameter.value == 13.1234
    sample.save()
    assert path.exists()
    sk.Sample(path).remove()
    sk.keep("B removed")
    assert "B removed" in cli(collection.root, "log")
    cli(collection.root, "restore", "--write")
    assert path.read_text() == written


def test_remove_refuses_a_file_changed_since_it_was_read(collection):
    path = collection.root / "A.md"
    sample = sk.Sample(path)
    path.write_text(path.read_text().replace("12.5", "12.6"))
    with pytest.raises(OSError, match="changed on disk since it was read, and nothing was removed"):
        sample.remove()
    assert path.exists()
    with pytest.raises(ValueError, match="this sample has no file to remove"):
        sk.Sample(name="x").remove()


def test_rename_moves_the_file_and_the_links_leading_to_it(collection):
    a = collection.root / "A.md"
    a.write_text(a.read_text().replace("alloy: Inconel\n", 'alloy: Inconel\ntwin: "[[B]]"\n'))
    sample = sk.Sample(collection.root / "B.md")
    moved = sample.rename("B2")
    assert moved == collection.root / "B2.md"
    assert not (collection.root / "B.md").exists()
    assert sk.Sample(a).twin == "[[B2]]"
    assert sk.Sample(a).linked("twin").name == "B2"
    # The sample is its new file's: a save writes there.
    sample.save()
    assert not (collection.root / "B.md").exists()
    with pytest.raises(FileExistsError, match="another sample has that name"):
        sample.rename("A")
    with pytest.raises(ValueError, match="no sample's name"):
        sample.rename("x/y")


def test_linked_reads_the_sample_an_attribute_links_to(collection):
    path = collection.root / "A.md"
    path.write_text(path.read_text().replace(
        "alloy: Inconel\n",
        'alloy: Inconel\ntwin: "[[B]]"\nsupplier: https://example.org/a\nlost: "[[Z]]"\n'))
    sample = sk.Sample(path)
    # The attribute stays the text written.
    assert sample.twin == "[[B]]"
    assert sample.linked("twin").name == "B"
    with pytest.raises(FileNotFoundError, match=r"lost: \[\[Z\]\] leads to no sample"):
        sample.linked("lost")
    with pytest.raises(ValueError, match="supplier is a web address"):
        sample.linked("supplier")
    with pytest.raises(ValueError, match="alloy is no link"):
        sample.linked("alloy")
    with pytest.raises(KeyError):
        sample.linked("nothing")


def test_linked_follows_a_markdown_link_and_a_list_of_links(collection):
    (collection.root / "reports").mkdir()
    (collection.root / "reports" / "a.pdf").write_text("%PDF")
    path = collection.root / "A.md"
    path.write_text(path.read_text().replace(
        "alloy: Inconel\n",
        'alloy: Inconel\ntwin: "[the twin](B.md)"\n'
        'report: "[report](reports/a.pdf)"\nparents: ["[[B]]", "[c](C.md)"]\n'))
    sample = sk.Sample(path)
    assert sample.linked("twin").name == "B"
    assert [parent.name for parent in sample.linked("parents")] == ["B", "C"]
    report = sample.linked("report")
    assert isinstance(report, pathlib.Path)
    assert report.resolve() == (collection.root / "reports" / "a.pdf").resolve()


def test_a_tables_csv_is_what_view_table_csv_prints(tmp_path, cli):
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    path = tmp_path / "p1.md"
    path.write_text(
        "---\nschema_version: 1\nname: p1\ntables:\n  heating:\n    index: [T]\n"
        "    columns:\n      T: {unit: degC}\n      hrc: {}\n    rows:\n"
        "      - {T: 20, hrc: 0.1}\n      - {T: 30, hrc: {v: 0.2, u: 0.01}}\n---\n"
    )
    with pytest.warns(UserWarning, match="T_uncertainty is left out: it is empty in every row"):
        text = sk.Sample(path).heating.to_csv()
    assert text == "T_value [degC],hrc_value,hrc_uncertainty\n20,0.1,\n30,0.2,0.01\n"
    assert text == cli(path, "view", "heating", "--csv")
    with pytest.raises(ValueError, match="no sample"):
        sk.Table(index="T", columns={"T": sk.Property()}).to_csv()
