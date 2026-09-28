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
from conftest import BINARY, MODEL


def quoted(path):
    """A path as a history's message writes it: quoted where a shell would
    read one of its characters, as Windows' backslashes."""
    import shlex

    return shlex.quote(str(path))


def names(samples):
    return [sample.name for sample in samples]


def boil(**derivations):
    """A temperature-indexed table whose rows are not in index order."""
    return sk.Table(
        {"T": sk.Column(unit="degC"), "srm": sk.Column(unit="-"), "R": sk.Column(unit="lintner")},
        "T",
        title="Boil",
        rows=[
            {"T": 30, "srm": sk.Property(0.3, uncertainty=0.03), "R": 3.0},
            {"T": 10, "srm": sk.Property(0.1, uncertainty=0.01), "R": 1.0},
            {"T": 20, "srm": sk.Property(0.2, uncertainty=0.02), "R": 2.0},
        ],
        **derivations,
    )


def quantity_sample(project):
    project.config('schema_version = 1\n\n[property.malt]\nprecision = ".3f"\n')
    return sk.Sample(
        project.sample(
            "q.md",
            name="ph",
            properties={"malt": "{v: 3.1551234, u: 0.00123, unit: g}"},
        )
    )


# ------------------------------------------------------------ ordering


def test_sort_mutates_and_returns_none(collection):
    samples = sk.SampleList(collection.root)
    assert samples.sort("malt") is None
    assert names(samples) == ["C", "A", "B"]


def test_sort_actually_reorders(collection):
    samples = sk.SampleList(collection.root)
    before = names(samples)
    samples.sort("-malt")
    assert names(samples) == ["B", "A", "C"]
    assert names(samples) != before


def test_sorted_returns_a_new_collection(collection):
    samples = sk.SampleList(collection.root)
    ordered = samples.sorted("-malt")
    assert ordered is not samples
    assert names(ordered) == ["B", "A", "C"]
    assert names(samples) == ["A", "B", "C"]


def test_unknown_sort_key_raises_key_error(collection):
    samples = sk.SampleList(collection.root)
    with pytest.raises(KeyError):
        samples.sort("plto")
    assert names(samples) == ["A", "B", "C"]


# -------------------------------------------------------------- tables


def test_at_addresses_by_index_value():
    table = boil()
    assert table.at(20, "srm").value == 0.2
    assert table[0]["T"].value == 30


def test_table_metadata_is_attribute_access():
    table = boil()
    assert table.column_names == ["T", "srm", "R"]
    assert table.index_values == [30, 10, 20]
    assert table.data_columns == ["srm", "R"]
    assert table.index == "T"
    assert table.index_unit == "degC"
    assert table.title == "Boil"


def test_property_carries_value_and_uncertainty():
    table = boil()
    cell = table.at(20, "srm")
    assert (cell.value, cell.uncertainty) == (0.2, 0.02)
    assert table.values("srm") == [0.3, 0.1, 0.2]
    assert table.uncertainties("srm") == [0.03, 0.01, 0.02]


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
    assert str(quantity_sample(project).malt) == "3.155 ± 0.001 g"


def test_an_empty_format_specifier_equals_str(project):
    malt = quantity_sample(project).malt
    assert f"{malt}" == str(malt)
    assert format(malt, "") == str(malt)


def test_a_format_specifier_renders_like_a_command_line_column(project, cli):
    sample = quantity_sample(project)
    shown = f"{sample.malt:.2f}"
    # An uncertainty the precision rounds away keeps two significant figures.
    assert shown.startswith("3.16 ± 0.00") and "<" not in shown
    assert shown.removesuffix(" g") in cli(sample.path, "-c", "malt:.2f", "--width", "120")


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
    prop = sk.Property("Pilsner 350")
    assert str(prop) == "Pilsner 350"
    assert f"{prop:.2f}" == "Pilsner 350"


def test_arithmetic_uses_the_value_and_returns_a_bare_number():
    ebc = sk.Property(9.5, uncertainty=0.1)
    srm = sk.Property(0.001)
    result = ebc * (1 - 1j * srm)
    assert type(result) is complex
    assert result == 9.5 * (1 - 1j * 0.001)
    assert ebc + 1 == 10.5
    assert 2 ** sk.Property(3) == 8
    assert -ebc == -9.5
    assert abs(sk.Property(-2.0)) == 2.0
    assert round(sk.Property(2.567), 1) == 2.6
    assert type(ebc / 2) is float


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
        sample["mal"]
    assert "did you mean: malt?" in str(caught.value)


def test_unknown_attribute_raises_attribute_error_with_a_suggestion(collection):
    sample = sk.Sample(collection.root / "A.md")
    with pytest.raises(AttributeError) as caught:
        sample.mal
    assert "did you mean: malt?" in str(caught.value)
    assert hasattr(sample, "mal") is False
    assert getattr(sample, "mal", 7) == 7


def test_a_property_without_a_value_is_returned(project):
    sample = sk.Sample(project.sample("e.md", properties={"energy": "{unit: J}"}))
    assert sample.energy.value is None
    assert sample.energy.unit == "J"


def test_membership_tests_a_name():
    sample = sk.Sample()
    sample.malt = sk.Property(1.0)
    sample.mashing = boil()
    assert "malt" in sample
    assert "mashing" in sample
    assert "nothing" not in sample


def test_deleting_by_attribute_or_item_removes_the_property(project):
    sample = sk.Sample(
        project.sample(
            "d.md",
            properties={"malt": "1.0", "plato": "2.0", "volume": "3.0"},
        )
    )
    del sample.malt
    del sample["plato"]
    sample.save()
    text = sample.path.read_text()
    assert "malt" not in text
    assert "plato" not in text
    assert "volume" in text


def test_iterating_a_sample_yields_names_in_declaration_order(project):
    sample = sk.Sample(
        project.sample(
            "o.md",
            attributes={"batch": 7, "maltster": "jo"},
            properties={"volume": "3.0", "malt": "1.0"},
        )
    )
    assert list(sample) == ["batch", "maltster", "volume", "malt"]
    assert list(sample.keys()) == list(sample)
    assert [name for name, _ in sample.items()] == list(sample)


# ----------------------------------------------------------- assignment


def test_assigning_a_property_or_a_table_registers_it(tmp_path):
    sample = sk.Sample()
    sample.malt = sk.Property(1.5, unit="g")
    sample.mashing = boil()
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "properties:\n  malt:" in text
    assert "tables:\n  mashing:" in text


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
    sample.malt = sk.Property(3.1)
    with pytest.raises(TypeError) as caught:
        sample.malt = 3.2
    assert "s.malt.value" in str(caught.value)
    assert sample.malt.value == 3.1


def test_an_attribute_without_a_value_is_kept_and_not_written(tmp_path):
    sample = sk.Sample(name="E")
    sample.operator = None
    sample.temperatures = []
    assert sample.operator is None
    assert sample.temperatures == []
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "maltster" not in text
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
    sample.tags.append("keg")
    with pytest.raises(ValueError):
        sample.tags.append("not a tag")
    assert sample.tags == ["reference", "keg"]
    path = tmp_path / "s.md"
    sample.save(path)
    again = sk.Sample(path)
    assert again.name == "C43"
    assert again.note == "  verbatim\n\ttext  \n"
    assert again.tags == ["reference", "keg"]


def test_brackets_never_reach_private_state():
    sample = sk.Sample()
    with pytest.raises(ValueError):
        sample["_x"] = 5
    sample._x = 5
    with pytest.raises(KeyError):
        sample["_x"]


def test_removed_accessors_name_their_replacement():
    sample = sk.Sample()
    with pytest.raises(AttributeError) as caught:
        sample.get_property("malt")
    assert 's["name"]' in str(caught.value)


# ------------------------------------------------------------- filtering


def test_filter_chains(collection):
    samples = sk.SampleList(collection.root)
    chained = samples.filter("period == 6").filter(lambda sample: sample.beer == "Pilsner")
    assert names(chained) == ["A"]


def test_filter_shares_samples_with_the_parent(collection):
    samples = sk.SampleList(collection.root)
    subset = samples.filter("beer == Pilsner")
    subset[0].malt.value = 99.0
    assert samples["A"].malt.value == 99.0


# ---------------------------------------------------------------- loading


def test_constructing_with_a_path_loads_the_file(collection):
    sample = sk.Sample(collection.root / "A.md")
    assert sample.malt.value == 12.5
    assert sample.name == "A"


def test_the_file_is_loaded_after_the_class_init(project):
    project.model()
    sample_path = project.sample("s.md", properties={"malt": "{v: 12.5, unit: g}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(MODEL, namespace)
    sample = namespace["Model"](sample_path)
    assert sample.malt.value == 12.5
    assert sample.malt.unit == "g"
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
    assert sample.malt.value is None


def test_the_project_model_applies_without_consent(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("s.md", properties={"malt": "2.0"}))
    assert type(sample).__name__ == "Model"
    assert isinstance(sample, sk.Sample)
    assert project.imported()


def test_model_false_loads_the_data_alone(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("s.md", properties={"malt": "2.0"}), model=False)
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
    sample = sk.Sample(project.sample("s.md", properties={"malt": "2.0"}), model=Override)
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
    assert samples.to_csv(profile="platos") == cli(root, "--profile", "platos", "--csv")
    assert samples.to_tsv(profile="platos") == cli(root, "--profile", "platos", "--tsv")
    assert samples.to_json(profile="platos") == cli(root, "--profile", "platos", "--json")
    assert samples.to_csv(columns=["name", "malt:.2f"]) == cli(root, "-c", "name,malt:.2f", "--csv")


def test_to_dict_keys_are_the_export_columns_without_their_unit(collection):
    samples = sk.SampleList(collection.root)
    header = samples.to_csv(profile="platos").splitlines()[0].split(",")
    assert list(samples.to_dict(profile="platos")) == [name.split(" [")[0] for name in header]


def test_to_dict_keeps_full_precision(collection):
    samples = sk.SampleList(collection.root)
    assert samples.to_dict(columns=["malt:.3f"])["malt_value"] == [12.5, 13.1234, 11.0]


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
    assert "platos" in str(caught.value)


def test_export_regenerates_a_declared_export(collection, cli):
    written = sk.SampleList(collection.root).export("malts")
    assert written == collection.root / "malts.csv"
    by_command = collection.root / "by-command.csv"
    # The command line previews and writes on --write; the Python `export` is
    # the call itself, so it writes.
    cli("export", "malts", collection.root, "-o", by_command, "--write")
    assert written.read_bytes() == by_command.read_bytes()


def test_a_list_over_two_projects_writes_each_row_at_its_own_precision(tmp_path):
    # Python wrote raw numbers over two projects, where the command line
    # writes each row as its own project declares.
    for project, precision, malt in (("one", ".1f", 1.23456), ("two", ".3f", 2.34567)):
        (tmp_path / project).mkdir()
        (tmp_path / project / ".samplekitrc").write_text(
            f'schema_version = 1\n[property.malt]\nprecision = "{precision}"\n'
        )
        (tmp_path / project / "s.md").write_text(
            f"---\nschema_version: 1\nname: {project}\nproperties:\n  malt: {malt}\n---\n"
        )
    both = sk.load(tmp_path / "two") + sk.load(tmp_path / "one")
    text = both.to_csv(columns=["name", "malt"])
    # No uncertainty column: no sample fills one.
    assert "one,1.2\n" in text, text
    assert "two,2.346\n" in text, text


def test_a_figure_over_two_projects_draws_both(tmp_path):
    # Python refused a list spanning two projects; it draws them all.
    for project, malt in (("one", 1.0), ("two", 2.0)):
        (tmp_path / project).mkdir()
        (tmp_path / project / ".samplekitrc").write_text("schema_version = 1\n")
        for n in (1, 2):
            (tmp_path / project / f"s{n}.md").write_text(
                f"---\nschema_version: 1\nname: {project}{n}\nproperties:\n"
                f"  malt: {{v: {malt * n}, unit: g}}\n  volume: {{v: {n}.0, unit: L}}\n---\n"
            )
    both = sk.load(tmp_path / "one") + sk.load(tmp_path / "two")
    (drawn,) = sk.plot(both, x="volume", y="malt", output=tmp_path / "f.png")
    # The second project's heaviest, 4 g, is on the figure.
    ys = [y for line in drawn.axes[0].lines for y in line.get_ydata()]
    assert max(ys) == 4.0 and min(ys) == 1.0, ys


def test_an_output_makes_its_folder(project, tmp_path):
    # Every output's folder is made, declared or given, as the command line
    # makes it.
    from conftest import PLATOS

    project.config(PLATOS + '\n[export.nested]\nprofile = "platos"\nformat = "csv"\noutput = "out/malts.csv"\n')
    project.sample("A.md", name="A", properties={"malt": "{v: 15.0, unit: g}"})
    samples = sk.SampleList(project.root)
    written = samples.export("nested")
    assert written == project.root / "out" / "malts.csv"
    assert written.exists()
    elsewhere = samples.export("nested", output=project.root / "elsewhere" / "malts.csv")
    assert elsewhere.read_text() == written.read_text()
    samples.to_csv(project.root / "csv" / "deeper" / "malts.csv", columns=["malt"])
    assert (project.root / "csv" / "deeper" / "malts.csv").exists()
    figures = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    sk.plot(figures, "malt_volume", output=tmp_path / "figs" / "fig.png")
    assert (tmp_path / "figs" / "fig.png").read_bytes().startswith(b"\x89PNG")


def test_export_applies_the_query_it_declares(project):
    # An export may carry its own selection, in Python as on the command line.
    from conftest import PLATOS

    project.config(PLATOS + '\n[export.heavy]\nprofile = "platos"\nformat = "csv"\noutput = "heavy.csv"\nquery = "heavy"\n')
    project.sample("A.md", name="A", properties={"malt": "{v: 15.0, unit: g}"})
    project.sample("B.md", name="B", properties={"malt": "{v: 5.0, unit: g}"})
    samples = sk.SampleList(project.root)
    assert samples.project.exports.heavy.query == "heavy"
    written = samples.export("heavy")
    rows = written.read_text().splitlines()
    assert len(rows) == 2, rows
    assert rows[1].startswith("A,"), rows
    everything = samples.export("malts", output=project.root / "all.csv")
    assert len(everything.read_text().splitlines()) == 3


# ----------------------------------------------------------------- project


def test_project_entries_are_accepted_wherever_a_name_is(collection, tmp_path):
    samples = sk.SampleList(collection.root)
    project = samples.project
    assert samples.to_csv(profile=project.profiles.platos) == samples.to_csv(profile="platos")
    assert names(samples.query(project.queries.heavy)) == names(samples.query("heavy"))
    out = samples.export(project.exports.malts, output=tmp_path / "entry.csv")
    assert out.read_text() == samples.to_csv(profile="platos")


def test_project_is_read_only(collection):
    config = (collection.root / ".samplekitrc").read_text()
    project = sk.SampleList(collection.root).project
    with pytest.raises(AttributeError) as caught:
        project.profiles = None
    assert ".samplekitrc" in str(caught.value)
    with pytest.raises(AttributeError):
        project.profiles.platos = None
    assert (collection.root / ".samplekitrc").read_text() == config


def test_project_names_complete_in_a_console(collection):
    profiles = sk.SampleList(collection.root).project.profiles
    assert "platos" in dir(profiles)
    assert profiles._ipython_key_completions_() == ["platos"]


def test_unknown_project_name_suggests_the_nearest(collection):
    profiles = sk.SampleList(collection.root).project.profiles
    with pytest.raises(AttributeError) as caught:
        profiles.pltaos
    assert "did you mean: platos?" in str(caught.value)
    with pytest.raises(KeyError) as caught:
        profiles["pltaos"]
    assert "did you mean: platos?" in str(caught.value)


def test_outside_a_project_a_lookup_says_no_configuration_was_found(project):
    sample = sk.Sample(project.sample("s.md", properties={"malt": "1.0"}))
    with pytest.raises(AttributeError) as caught:
        sample.project.profiles.platos
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


def test_removed_list_members_name_their_replacement(collection):
    samples = sk.SampleList(collection.root)
    with pytest.raises(AttributeError) as caught:
        samples.append(sk.Sample())
    message = str(caught.value)
    assert "+" in message
    assert "SampleList([...])" in message
    assert "list(samples)" in message


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
    joined[0].malt.value = 1.0
    assert first[0].malt.value == 1.0
    assert samples[0].malt.value == 1.0


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
    from conftest import PLATOS, Project

    lists = []
    for name in ("first", "second"):
        root = tmp_path / name
        root.mkdir()
        project = Project(root)
        project.config(PLATOS)
        project.sample(f"{name}.md", name=name, properties={"malt": "1.0"})
        lists.append(sk.SampleList(root))
    return lists


def test_a_list_spanning_two_projects_has_no_single_project(tmp_path):
    first, second = two_projects(tmp_path)
    with pytest.raises(ValueError) as caught:
        (first + second).project
    assert "first" in str(caught.value)
    assert "second" in str(caught.value)


def test_a_declared_name_on_a_list_spanning_two_projects_is_refused(tmp_path):
    first, second = two_projects(tmp_path)
    mixed = first + second
    with pytest.raises(ValueError):
        mixed.to_csv(profile="platos")
    text = mixed.to_csv(profile=first.project.profiles.platos)
    assert text.startswith("name,")


# ------------------------------------------------------------------ saving


def test_mutation_through_a_property_reaches_the_sample(collection):
    sample = sk.Sample(collection.root / "A.md")
    malt = sample.malt
    malt.value = 20.0
    sample.save()
    assert "v: 20.0" in sample.path.read_text()


def test_saving_preserves_the_note(project):
    note = "# Notes  \n\n\tIndented, trailing spaces   \r\nno final newline"
    path = project.root / "n.md"
    path.write_bytes(
        b"---\nschema_version: 1\nproperties:\n  malt: 1.0\n---\n" + note.encode()
    )
    sample = sk.Sample(path)
    sample.malt.value = 2.0
    sample.save()
    assert path.read_bytes().endswith(note.encode())


def test_a_loaded_sample_remembers_its_path(collection):
    sample = sk.Sample(collection.root / "A.md")
    assert sample.path == collection.root / "A.md"
    assert sample.filename == "A.md"


def test_save_without_a_path_rewrites_the_file_read(collection):
    other = (collection.root / "B.md").read_text()
    sample = sk.Sample(collection.root / "A.md")
    sample.malt.value = 21.0
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
    sample.malt.value = 1.0
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
    assert "malt" in listing
    assert "beer" in listing


# ------------------------------------------------------------ computation


class Shapes(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.malt = sk.Property(12.0)
        self.volume = sk.Property(4.0)
        self.plato = sk.Property(compute=lambda: self.malt.value / self.volume.value)
        self.malt_tolerance = sk.Property(12.0, compute_uncertainty=lambda: 0.1, depends_on=[])
        self.joint = sk.Property(compute_quantity=lambda: (self.malt.value * 2, 0.5), depends_on=["malt"])
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column(), "N": sk.Column()},
            "T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
            compute_rows=[("G", ["row.R"], lambda row: 1 / row.R.value)],
            compute_columns=[
                (
                    "N",
                    ["mashing.R"],
                    lambda columns: [value / max(columns["R"].values) for value in columns["R"].values],
                )
            ],
        )


def test_all_five_computation_shapes_are_expressible(tmp_path):
    sample = Shapes()
    sample.compute()
    assert sample.plato.value == 3.0
    assert sample.malt_tolerance.uncertainty == 0.1
    assert (sample.joint.value, sample.joint.uncertainty) == (24.0, 0.5)
    assert sample.mashing.values("G") == [0.5, 0.25]
    assert sample.mashing.values("N") == [0.5, 1.0]
    path = tmp_path / "shapes.md"
    sample.save(path)
    text = path.read_text()
    # A record names the channel its formula produced.
    assert "computed: {v: {malt:" in text or "computed: {malt:" in text, text
    assert "u: 0.5" in text


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
        {name: sk.Column() for name in ("T", "R", "ph", "pitch", "low", "high")},
        "T",
        rows=[{"T": 1, "R": 1.0}, {"T": 2, "R": 2.0}],
        compute_rows=[(["ph", "pitch"], ["row.R"], fit)],
        compute_columns=[(["low", "high"], ["t.R"], normalize)],
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
        {name: sk.Column() for name in ("T", "R", "ph", "pitch")},
        "T",
        rows=[{"T": 1, "R": 1.0}],
        compute_rows=[(["ph", "pitch"], ["row.R"], lambda row: {"ph": 1.0})],
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
        {name: sk.Column() for name in ("T", "R", "N")},
        "T",
        rows=[{"T": 1, "R": 1.0}, {"T": 2, "R": 2.0}],
        compute_columns=[("N", ["u.R"], lambda columns: [1.0])],
    )
    with pytest.raises(ValueError):
        sample.compute("u")


def test_stats_returns_a_typed_summary(collection):
    summary = sk.SampleList(collection.root).stats("malt")
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


# ---------------------------------------------------------------- inventory


V1_KEPT = {
    "Property": ["value", "uncertainty", "readings", "unit", "symbol", "text",
                 "is_computed", "format", "invalidate"],
    "Column": ["unit"],
    "Table": ["add", "extend", "update", "at", "values", "uncertainties", "column_names",
              "index_values", "data_columns", "index", "index_unit", "title"],
    "Sample": ["save", "set_dependencies", "dependencies", "dependents", "affected_by_change",
               "name", "tags", "note", "path", "filename", "project"],
    "SampleList": ["filter", "sort", "sorted", "query", "stats", "save_all", "index", "count"],
}

V1_DIAGNOSED = {
    "Property": {"data": "readings"},
    "Table": {"cell": "at(index, column)", "column": "values(column)"},
    "Sample": {"get_property": 's["name"]', "set_property": "Property(...)",
               "set_value": ".value", "set_uncertainty": ".uncertainty", "set_unit": ".unit",
               "set_symbol": ".symbol", "has_property": "in s", "remove_property": "del s",
               "get_table": 's["name"]', "set_table": "Table(...)", "remove_table": "del s",
               "property_names": "list(s)", "table_names": "list(s)", "notes": "note",
               "load_data": "Sample(path)", "render_view": "samplekit view",
               "render_report": "samplekit view", "view_names": "samplekit view",
               "view_definition": "samplekit view"},
    "SampleList": {"append": "SampleList([...])", "extend": "SampleList([...])",
                   "insert": "SampleList([...])", "pop": "SampleList([...])",
                   "clear": "SampleList([...])", "remove": "SampleList([...])",
                   "copy": "SampleList([...])", "names": "sample.name",
                   "query_names": "project.queries", "sort_multi": "sort([",
                   "to_records": "to_dict", "to_dataframe": "to_dict"},
}

V1_ABSENT = {
    # A precision is the project's to declare, and a column's symbol too.
    "Property": ["unit_math", "symbol_math", "precision_unc", "is_valid", "precision"],
    "Column": ["unit_math", "symbol_math", "precision_unc", "precision", "symbol"],
}


def inventory_instances(collection):
    sample = sk.Sample(collection.root / "A.md")
    return {
        "Property": sample.malt,
        "Column": sk.Column(unit="degC"),
        "Table": boil(),
        "Sample": sample,
        "SampleList": sk.SampleList(collection.root),
    }


def test_the_v1_public_inventory_is_either_present_or_diagnosed(collection):
    instances = inventory_instances(collection)
    for owner, members in V1_KEPT.items():
        for member in members:
            assert hasattr(instances[owner], member), f"{owner}.{member}"
    for owner, members in V1_DIAGNOSED.items():
        for member, replacement in members.items():
            with pytest.raises(AttributeError) as caught:
                getattr(instances[owner], member)
            assert replacement in str(caught.value), f"{owner}.{member}: {caught.value}"
    for owner, members in V1_ABSENT.items():
        for member in members:
            assert not hasattr(instances[owner], member), f"{owner}.{member}"


# ------------------------------------------------------------------ added


def test_filter_takes_an_expression_or_a_function(collection):
    samples = sk.SampleList(collection.root)
    by_expression = samples.filter("beer == Pilsner")
    by_function = samples.filter(lambda sample: sample.beer == "Pilsner")
    assert names(by_expression) == names(by_function) == ["A", "C"]
    with pytest.raises(KeyError):
        samples.filter("bere == Pilsner")


def test_a_list_of_paths_is_refused_naming_the_constructor(collection):
    with pytest.raises(TypeError) as caught:
        sk.SampleList([collection.root / "A.md"])
    assert "Sample(p)" in str(caught.value)


def test_a_table_assigned_under_another_name_than_its_column_inputs_is_refused():
    table = sk.Table(
        {"T": sk.Column(), "R": sk.Column(), "N": sk.Column()},
        "T",
        compute_columns=[("N", ["boil.R"], lambda columns: columns["R"].values)],
    )
    sample = sk.Sample()
    with pytest.raises(ValueError) as caught:
        sample.mashing = table
    assert "boil" in str(caught.value)
    assert "mashing" in str(caught.value)




# ------------------------------------------------------ recomputing

RUNS = {"plato": 0, "haze": 0, "clarity": 0, "spread": 0, "flatness": 0}


def reset_runs():
    for name in RUNS:
        RUNS[name] = 0


class Platos(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g")
        self.volume = sk.Property(unit="L")
        self.plato = sk.Property(compute=self._plato)
        self.haze = sk.Property(compute=self._haze)
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.set_dependencies("haze", depends_on=["plato"])

    def _plato(self):
        RUNS["plato"] += 1
        return self.malt.value / self.volume.value

    def _haze(self):
        RUNS["haze"] += 1
        return 1 - self.plato.value / 4.0


class Boil(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.foam = sk.Property(2.0)
        self.mashing = sk.Table(
            {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
            "T",
            rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
            compute_rows=[("G", ["row.R", "foam"], self._clarity)],
        )

    def _clarity(self, row):
        RUNS["clarity"] += 1
        return 1 / row.R.value / self.foam.value


def saved_platos(directory, file="d.md", malt=12.0):
    """Platos computed from malt and a volume of 4, saved, and runs reset."""
    sample = Platos(name=file)
    sample.malt.value = malt
    sample.volume.value = 4.0
    sample.compute()
    path = directory / file
    sample.save(path)
    reset_runs()
    return path


def edit_malt(path, before="12.0", after="16.0"):
    text = path.read_text()
    edited = text.replace(f"v: {before}", f"v: {after}", 1)
    assert edited != text
    path.write_text(edited)
    return edited


def test_a_value_stale_on_disk_is_served_and_reported_stale(tmp_path):
    path = saved_platos(tmp_path)
    edit_malt(path)
    sample = Platos(path)
    assert sample.plato.value == 3.0
    assert sample.plato.is_outdated
    assert sample.haze.is_outdated
    assert not sample.malt.is_outdated
    assert RUNS["plato"] == 0 and RUNS["haze"] == 0


def test_the_former_names_of_outdated_still_answer(tmp_path):
    """*stale* was renamed *outdated*: a script written against
    `stale()` and `is_stale` goes on getting the same answers."""
    path = saved_platos(tmp_path)
    edit_malt(path)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        sample = Platos(path)
        assert sample.stale() == sample.outdated() == ["plato", "haze"]
        assert sample.plato.is_stale and sample.plato.is_outdated
        assert not sample.malt.is_stale
        sample.compute()
        assert sample.stale() == sample.outdated() == []


def test_saving_runs_no_formula(tmp_path):
    path = saved_platos(tmp_path)
    edit_malt(path)
    sample = Platos(path)
    sample.save()
    assert RUNS["plato"] == 0 and RUNS["haze"] == 0
    reloaded = Platos(path)
    assert reloaded.plato.value == 3.0
    assert reloaded.plato.is_outdated

    fresh = Platos(name="fresh")
    fresh.malt.value = 1.0
    fresh.volume.value = 8.0
    fresh.save(tmp_path / "fresh.md")
    text = (tmp_path / "fresh.md").read_text()
    assert RUNS["plato"] == 0
    assert "0.125" not in text
    assert "computed" not in text




def test_compute_without_names_runs_stale_and_never_computed_values(tmp_path):
    path = saved_platos(tmp_path)
    edit_malt(path)
    sample = Platos(path)
    sample.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (1, 1)
    assert sample.plato.value == 4.0
    assert sample.outdated() == []
    sample.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (1, 1)
    sample.compute(rerun=True)
    assert (RUNS["plato"], RUNS["haze"]) == (2, 2)

    fresh = Platos(name="fresh")
    fresh.malt.value = 1.0
    fresh.volume.value = 8.0
    fresh.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (3, 3)
    fresh.save(tmp_path / "fresh.md")
    assert "0.125" in (tmp_path / "fresh.md").read_text()


def test_a_list_recomputes_every_sample(tmp_path):
    first = saved_platos(tmp_path, "a.md")
    second = saved_platos(tmp_path, "b.md", malt=8.0)
    edit_malt(first)
    edit_malt(second, "8.0", "20.0")
    samples = sk.SampleList([Platos(first), Platos(second)])
    samples.compute()
    assert RUNS["plato"] == 2
    assert [sample.plato.value for sample in samples] == [4.0, 5.0]
    samples.compute("plato")
    assert RUNS["plato"] == 2
    samples.compute("plato", rerun=True)
    assert RUNS["plato"] == 4


def test_stale_lists_properties_and_columns(tmp_path):
    boil = Boil()
    boil.compute()
    path = tmp_path / "boil.md"
    boil.save(path)
    text = path.read_text()
    edited = re.sub(r"(?m)^(  foam: (?:\{v: )?)2\.0", r"\g<1>4.0", text)
    assert edited != text
    path.write_text(edited)
    reset_runs()
    stale = Boil(path)
    assert stale.outdated() == ["mashing.G"]
    assert stale.mashing.at(10, "G").is_outdated
    stale.compute()
    assert RUNS["clarity"] == 2
    assert stale.outdated() == []


def test_a_recomputation_that_changed_nothing_reruns_nothing_downstream(tmp_path):
    sample = Platos(saved_platos(tmp_path))
    sample.plato.compute(rerun=True)
    assert RUNS["plato"] == 1
    assert sample.haze.value == 0.25
    assert RUNS["haze"] == 0


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
        self.malt = sk.Property(10.0)
        self.grist = sk.Property(compute=lambda: self.malt.value * 2)
        self.brix = sk.Property(compute=lambda: self.grist.value / 4)
        self.vfi = sk.Property(compute=lambda: self.brix.value + 1)
        self.set_dependencies("grist", depends_on=["malt"])
        self.set_dependencies("brix", depends_on=["grist"])
        self.set_dependencies("vfi", depends_on=["brix"])


def entered_platos():
    sample = Platos()
    sample.malt.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    assert sample.haze.value == 0.25
    reset_runs()
    return sample


def test_assigning_a_computed_value_overrides_it():
    sample = entered_platos()
    sample.plato.value = 2.0
    assert sample.plato.is_computed
    assert sample.plato.is_edited
    assert not sample.plato.is_outdated
    assert sample.edited() == ["plato"]
    sample.compute()
    assert sample.haze.value == 0.5
    sample.malt.value = 40.0
    sample.compute()
    assert sample.plato.value == 2.0
    assert sample.haze.value == 0.5
    assert (RUNS["plato"], RUNS["haze"]) == (0, 1)




def test_computing_a_named_value_computes_its_stale_inputs_first(tmp_path):
    path = saved_platos(tmp_path)
    edit_malt(path)
    sample = Platos(path)
    assert sample.plato.is_outdated and sample.haze.is_outdated
    sample.haze.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (1, 1)
    assert sample.plato.value == 4.0
    assert sample.haze.value == 0.0
    assert sample.outdated() == []
    reset_runs()
    sample.compute("haze", rerun=True)
    assert (RUNS["plato"], RUNS["haze"]) == (0, 1)


def test_an_override_round_trips_through_a_file(tmp_path):
    path = saved_platos(tmp_path)
    sample = Platos(path)
    sample.plato.value = 2.0
    sample.compute()
    assert sample.haze.value == 0.5
    sample.save()
    text = path.read_text()
    assert re.search(r"(?m)^    fingerprint: \{edited: [0-9a-f]{12}\}$", text), text
    assert re.search(r"computed: \{v: \{plato: \{edited: [0-9a-f]{12}\}\}\}", text), text
    reloaded = Platos(path)
    assert reloaded.plato.is_edited
    assert reloaded.plato.value == 2.0
    assert not reloaded.haze.is_outdated
    assert reloaded.haze.edited_upstream == ["plato"]
    assert RUNS["plato"] == 0

    hand = saved_platos(tmp_path, file="hand.md")
    text = hand.read_text()
    edited = re.sub(r"(?m)^(  plato:\n    v: )3\.0$", r"\g<1>3.5", text)
    assert edited != text, text
    hand.write_text(edited)
    held = Platos(hand)
    assert held.plato.is_edited
    held.compute(rerun=True)
    assert held.plato.value == 3.5
    assert RUNS["plato"] == 0


def test_edited_upstream_names_the_overrides_a_value_rests_on():
    sample = Chain()
    sample.grist.value = 8.0
    sample.compute()
    assert sample.vfi.value == 3.0
    assert sample.vfi.edited_upstream == ["grist"]
    assert sample.brix.edited_upstream == ["grist"]
    assert sample.grist.edited_upstream == []
    sample.grist.compute(force=True)
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


class Carbonation(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.conditioning = sk.Table(
            {"day": sk.Column(), "venting": sk.Column(), "carbonation": sk.Column()},
            "day",
            rows=[{"day": 1, "venting": 2.0}, {"day": 2, "venting": 1.5}],
            compute_columns=[
                (
                    "carbonation",
                    ["conditioning.venting"],
                    lambda columns: [v / columns["venting"].values[0] for v in columns["venting"].values],
                )
            ],
        )
        self.flatness = sk.Property(compute=self._flatness)
        self.set_dependencies("flatness", depends_on=["conditioning.carbonation"])

    def _flatness(self):
        RUNS["flatness"] += 1
        return self.conditioning.values("carbonation")[-1]




def test_compute_runs_what_is_named_when_it_is_pending(tmp_path):
    sample = Platos(saved_platos(tmp_path))
    sample.plato.compute()
    sample.compute("plato", "haze")
    assert (RUNS["plato"], RUNS["haze"]) == (0, 0)
    sample.plato.compute(rerun=True)
    assert RUNS["plato"] == 1
    sample.compute("plato", "haze", rerun=True)
    assert (RUNS["plato"], RUNS["haze"]) == (2, 1)
    with pytest.raises(ValueError):
        sample.compute("malt")
    with pytest.raises(KeyError) as caught:
        sample.compute("plto")
    assert "plato" in str(caught.value)

    stale = Platos(edit_and_return(saved_platos(tmp_path, "s.md")))
    reset_runs()
    stale.compute("haze")
    assert (RUNS["plato"], RUNS["haze"]) == (1, 1)

    boil = Boil()
    boil.compute()
    reset_runs()
    boil.compute("mashing.G")
    boil.mashing.at(10, "G").compute()
    boil.compute("mashing")
    assert RUNS["clarity"] == 0
    boil.compute("mashing.G", rerun=True)
    assert RUNS["clarity"] == 2
    boil.mashing.at(10, "G").compute(rerun=True)
    assert RUNS["clarity"] == 3
    boil.compute("mashing", rerun=True)
    assert RUNS["clarity"] == 5


def edit_and_return(path):
    edit_malt(path)
    return path


def test_compute_gives_an_override_back_only_when_forced():
    sample = entered_platos()
    sample.plato.value = 2.0
    sample.compute()
    sample.compute(rerun=True)
    sample.plato.compute()
    sample.compute("plato", rerun=True)
    assert sample.plato.value == 2.0
    assert sample.plato.is_edited
    assert RUNS["plato"] == 0
    sample.plato.compute(force=True)
    assert RUNS["plato"] == 1
    assert not sample.plato.is_edited
    assert sample.edited() == []
    assert sample.plato.value == 3.0
    sample.compute()
    assert sample.haze.value == 0.25


def test_property_state_names_where_a_value_stands(tmp_path):
    path = saved_platos(tmp_path)
    edit_malt(path)
    sample = Platos(path)
    assert sample.malt.state == "entered"
    assert sample.plato.state == "outdated"
    sample.compute()
    assert sample.plato.state == "current"
    sample.plato.value = 2.0
    assert sample.plato.state == "edited"

    fresh = Platos(name="fresh")
    assert fresh.plato.state == "never computed"
    fresh.malt.value = 1.0
    fresh.volume.value = 0.0
    with pytest.raises(ZeroDivisionError):
        fresh.plato.compute()
    assert fresh.plato.state == "failed"

    boil = Boil()
    assert boil.mashing.at(10, "G").state == "never computed"
    boil.compute()
    assert boil.mashing.at(10, "G").state == "current"
    assert boil.mashing.at(10, "R").state == "entered"


def test_a_list_warns_of_a_file_it_could_not_read(project):
    project.sample("A.md", name="A", properties={"malt": 12.0})
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
    sample.malt.value = 2.0
    sample.save()
    again = sk.Sample(path)
    assert again.name == "CC-10"
    assert again.malt.value == 2.0
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
    path = project.sample("s.md", properties={"malt": "2.0"})
    with pytest.raises(FileExistsError, match=r"Sample\(path\)"):
        sk.Sample.new(path)
    assert not project.imported()


def test_new_on_a_model_class_creates_that_class(project):
    namespace = {"__file__": str(project.root / "model.py")}
    exec(MODEL, namespace)
    cask = namespace["Model"]
    sample = cask.new(project.root / "B.md", name="Bee")
    assert type(sample) is cask
    assert sample.name == "Bee"
    with pytest.raises(FileNotFoundError, match="does not exist"):
        cask.new(project.root / "missing" / "C.md")


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
        self.conditioning = sk.Table(
            {"day": sk.Column(), "venting": sk.Column(), "carbonation": sk.Column()},
            "day",
            rows=[{"day": 1, "venting": first}, {"day": 2, "venting": 1.0}],
            compute_columns=[
                ("carbonation", ["conditioning.venting"],
                 lambda columns: [v / columns["venting"].values[0] for v in columns["venting"].values])
            ],
        )
        self.flatness = sk.Property(compute=lambda: self.conditioning.at(2, "carbonation").value)
        self.set_dependencies("flatness", depends_on=["conditioning.carbonation"])
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
        sk.Property: ["value", "uncertainty", "unit", "symbol", "v", "u", "text", "is_computed"],
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
    again.conditioning.update(2, venting=0.5)
    assert again.flatness.state == "outdated"
    assert "flatness" in again.outdated()




def test_an_export_includes_what_its_declaration_includes(project):
    project.config(
        'schema_version = 1\n\n[profile.malts]\ncolumns = [{field = "name"}, {field = "malt"}]\n\n'
        '[export.malts]\nprofile = "malts"\nformat = "csv"\noutput = "malts.csv"\nfilename = true\n'
    )
    project.sample("A.md", name="A", properties={"malt": "12.5"})
    written = sk.SampleList(project.root).export("malts", output=project.root / "out.csv")
    with open(written) as handle:
        assert "filename" in handle.readline()


def test_sorted_takes_a_comma_list(collection):
    samples = sk.SampleList(collection.root, model=False)
    written = [sample.name for sample in samples.sorted("-period,malt")]
    assert written == [sample.name for sample in samples.sorted(["-period", "malt"])]
    assert written == ["A", "B", "C"]


def test_a_table_not_yet_assigned_holds_no_derived_value_until_computed():
    table = sk.Table(
        {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
        "T",
        rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        compute_rows=[("G", ["row.R"], lambda row: 1 / row.R.value)],
    )
    assert table.values("G") == [None, None]
    # It is what it is: never computed, not current.
    assert table.at(10, "G").state == "never computed"
    assert table.at(10, "R").state == "entered"
    sample = sk.Sample()
    sample.mashing = table
    sample.compute()
    assert sample.mashing.values("G") == [0.5, 0.25]
    assert sample.mashing.at(10, "G").state == "current"



def test_reading_a_value_never_runs_its_formula():
    reset_runs()
    sample = Platos()
    sample.malt.value = 12.0
    sample.volume.value = 4.0
    with pytest.warns(UserWarning, match="never computed"):
        assert sample.plato.value is None
    assert len(sk.SampleList([sample]).filter("plato > 1")) == 0
    assert RUNS["plato"] == 0
    sample.compute()
    assert sample.plato.value == 3.0


def test_changing_an_input_leaves_a_dependent_stale_until_computed():
    sample = Platos()
    sample.malt.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    reset_runs()
    sample.malt.value = 16.0
    assert sample.plato.is_outdated
    with pytest.warns(UserWarning, match="outdated"):
        assert sample.plato.value == 3.0
    assert RUNS["plato"] == 0
    sample.compute()
    assert sample.plato.value == 4.0
    assert RUNS["plato"] == 1
    assert not sample.plato.is_outdated


def test_a_value_reading_a_derived_column_is_not_computed_twice():
    reset_runs()
    sample = Carbonation(name="r")
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


def test_to_csv_headers_use_the_project_unit_spelling(project, cli):
    from conftest import PLATOS

    project.config(PLATOS + '\n[unit.g]\nplain = "gram"\n')
    project.sample("A.md", name="A", properties={"malt": "{v: 12.5, u: 0.05, unit: g}"})
    samples = sk.SampleList(project.root)
    written = samples.to_csv(columns=["name", "malt"])
    assert "[gram]" in written.splitlines()[0]
    assert written == cli(project.root, "-c", "name,malt", "--csv")


CHAIN = """\
from samplekit import Property, Sample


class Chain(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = Property()
        self.double = Property(compute=lambda: self.malt.value + 1.0)
        self.quadruple = Property(compute=lambda: self.double.value * 2)
        self.set_dependencies("double", depends_on=["malt"])
        self.set_dependencies("quadruple", depends_on=["double"])
"""


def test_a_failed_compute_keeps_the_values_it_did_not_reach(project):
    sample_path = project.sample("s.md", properties={"malt": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CHAIN, namespace)
    sample = namespace["Chain"](sample_path)
    sample.compute()
    sample.save()
    sample = namespace["Chain"](sample_path)
    # A date, which the formula cannot add to: text is refused where a number
    # is held.
    sample.malt.value = datetime.date(2026, 9, 27)
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
    sample_path = project.sample("s.md", properties={"malt": "{v: 1.5}"})
    namespace = {"__file__": str(project.root / "model.py")}
    exec(CHAIN, namespace)
    sample = namespace["Chain"](sample_path)
    sample.compute()
    sample.save()

    fresh = namespace["Chain"](sample_path)
    assert fresh.not_current() == {}
    assert fresh.double.failure is None

    fresh.malt.value = datetime.date(2026, 9, 27)
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
    sk.SampleList(collection.root).export("malts", output="-")
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
            "malt": "{v: 1.0, fingerprint: aaaaaaaaaaaa}",
            "double": "{v: 2.0, computed: {malt: aaaaaaaaaaaa}, fingerprint: bbbbbbbbbbbb}",
        },
    )
    assert sk.Sample(path, model=False).double.state == "edited"


def test_a_text_is_refused_on_a_property_with_a_unit(project):
    sample = quantity_sample(project)
    with pytest.raises(ValueError) as caught:
        sample.malt.value = "abc"
    assert "is measured in g" in str(caught.value)


def test_a_model_declares_the_statistics_of_its_readings(project):
    sample_path = project.sample("s.md", properties={"malt": "{v: 5.0, readings: [1.0, 2.0, 9.0]}"})

    class Dosed(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.malt = sk.Property(unit="g", value=sk.stats.median, uncertainty=sk.stats.sample_stdev)

    # The written value outranks the declared statistic; the uncertainty is the
    # declared one, since the file writes none.
    dosed = Dosed(sample_path)
    assert dosed.malt.value == 5.0
    assert dosed.malt.uncertainty == pytest.approx(19**0.5)
    assert sk.Sample(sample_path, model=False).malt.value == 5.0
    # With no value written, the declared statistic stands for the readings.
    unwritten = project.sample("u.md", properties={"malt": "{readings: [1.0, 2.0, 9.0]}"})
    assert Dosed(unwritten).malt.value == 2.0
    # Without the model no statistic is declared, and no mean stands in.
    assert sk.Sample(unwritten, model=False).malt.value is None


class _Dosed(sk.Sample):
    """`malt` by declared statistics, and `twice` reading it."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.malt = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.twice = sk.Property(compute=lambda: 2 * self.malt.value, depends_on=["malt"])


def _dosed(project):
    path = project.sample("w.md", properties={"malt": "{readings: [1.0, 2.0, 3.0], unit: g}"})
    sample = _Dosed(path)
    sample.compute()
    sample.save()
    return path


def test_a_declared_statistic_reloaded_untouched_is_not_edited(project):
    # A file always writes a value beside its readings, so *written* is the
    # ordinary shape of a reloaded sample, not an override.
    sample = _Dosed(_dosed(project))
    assert sample.edited() == []
    assert sample.outdated() == []
    assert sample.malt.state == "current"
    assert sample._plan([], False, False) == []


def test_a_corrected_reading_is_stale_and_an_ordinary_compute_runs_it(project):
    # A dosing corrected by hand, from Python: the verdict the command line gives, and a
    # plain compute — no force — gives the value back to its statistic.
    path = _dosed(project)
    text = path.read_text().replace("[1.0, 2.0, 3.0]", "[1.0, 2.0, 6.0]")
    assert "6.0" in text
    path.write_text(text)
    with pytest.warns(UserWarning, match="outdated"):
        sample = _Dosed(path)
        assert sample.malt.state == "outdated"
        assert sample.malt.value == 2.0
    assert sample.edited() == []
    assert sample.outdated() == ["malt", "twice"]
    sample.compute()
    assert sample.malt.value == 3.0
    assert sample.twice.value == 6.0
    assert sample.outdated() == []
    sample.save()
    assert _Dosed(path).malt.state == "current"


class _Ratio(sk.Sample):
    """`ratio` fails where `b` is zero."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.a = sk.Property()
        self.b = sk.Property()
        self.ratio = sk.Property(compute=lambda: self.a.value / self.b.value, depends_on=["a", "b"])


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
    path = _dosed(project)
    sample = _Dosed(path)
    sample.malt.readings = [4.0, 5.0, 6.0]
    assert sample.malt.state == "outdated"
    assert sample.outdated() == ["malt", "twice"]
    sample.compute()
    assert sample.malt.value == 5.0
    assert sample.twice.value == 10.0
    sample.save()
    assert _Dosed(path).outdated() == []


def test_readings_changed_in_place_are_the_propertys(project):
    # `readings` was a copy: append changed nothing, without a word.
    path = _dosed(project)
    sample = _Dosed(path)
    readings = sample.malt.readings
    readings.append(6.0)
    assert list(sample.malt.readings) == [1.0, 2.0, 3.0, 6.0]
    sample.compute()
    assert sample.malt.value == 3.0
    with pytest.raises(ValueError):
        sample.malt.readings.clear()
    assert list(sample.malt.readings) == [1.0, 2.0, 3.0, 6.0]


def test_a_value_written_over_a_statistic_is_kept_until_forced(project):
    # The readings stand, the writing outranks them and survives a save, a
    # reload and an ordinary compute; force gives the statistic back.
    path = _dosed(project)
    sample = _Dosed(path)
    sample.malt.value = 4.75
    assert list(sample.malt.readings) == [1.0, 2.0, 3.0]
    assert sample.edited() == ["malt"]
    sample.compute()
    sample.save()
    reloaded = _Dosed(path)
    assert reloaded.malt.value == 4.75
    assert reloaded.malt.state == "edited"
    assert reloaded.edited() == ["malt"]
    reloaded.compute()
    assert reloaded.malt.value == 4.75
    reloaded.compute(force=True)
    assert reloaded.malt.value == 2.0
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
    table = sk.Table({"day": sk.Column(), "carbonation": sk.Column()}, "day")
    with pytest.raises(TypeError, match="values"):
        table["carbonation"]
    with pytest.raises(TypeError, match="tuple"):
        table.at([1, 2], "carbonation")


def test_a_sample_is_addressed_as_the_command_line_addresses_it(project):
    path = project.sample("s.md", name="S", properties={"malt": "{v: 1.5, u: 0.1}"})
    sample = sk.Sample(path)
    assert sample["name"] == "S"
    assert sample["malt.u"] == 0.1
    assert isinstance(sample["malt"], sk.Property)
    with pytest.raises(KeyError):
        sample["malt.nothing"]


def test_a_datetime_keeps_its_microseconds():
    import datetime

    moment = datetime.datetime(2025, 11, 13, 15, 10, 53, 815606)
    assert sk.Property(moment).value == moment


def test_a_table_formula_reads_a_table_it_declares(tmp_path):
    class Kettle(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.raw = sk.Table({"T": sk.Column(), "f": sk.Column()}, "T")
            self.refs = sk.Table({"T": sk.Column()}, "T")
            self.shifted = sk.Table(
                {"T": sk.Column(), "g": sk.Column(), "h": sk.Column(), "k": sk.Column()},
                "T",
                compute_columns=[("g", ["shifted.T", "raw.f"], self._doubled)],
                compute_rows=[
                    ("h", ["row.T", "raw.f"], self._peak),
                    # `refs.T` names a column `shifted` declares too: both
                    # qualifiers are its own until the table is assigned.
                    ("k", ["row.T", "refs.T"], lambda row: len(self.refs.values("T"))),
                ],
            )

        def _doubled(self, columns):
            f = dict(zip(self.raw.values("T"), self.raw.values("f")))
            return [2 * f[T] for T in columns["T"].values]

        def _peak(self, row):
            return max(self.raw.values("f")) + row.T.value

    sample = Kettle(name="c")
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
    assert Kettle(path).shifted.at(20, "g").state == "current"


def test_a_sample_saves_again_when_a_table_precedes_the_one_it_reads(tmp_path):
    import re

    class Kettle(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.shifted = sk.Table(
                {"T": sk.Column(), "g": sk.Column()},
                "T",
                compute_columns=[("g", ["shifted.T", "raw.f"], self._doubled)],
            )
            self.raw = sk.Table({"T": sk.Column(), "f": sk.Column()}, "T")

        def _doubled(self, columns):
            f = dict(zip(self.raw.values("T"), self.raw.values("f")))
            return [2 * f[T] for T in columns["T"].values]

    sample = Kettle(name="c")
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
    Kettle(path).save()
    assert Kettle(path).shifted.values("g") == [2.0, 4.0]


def test_a_cell_edited_by_hand_says_so(tmp_path):
    class Doubled(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.t = sk.Table(
                {"T": sk.Column(), "x": sk.Column(), "y": sk.Column()},
                "T",
                compute_rows=[("y", ["row.x"], self._y)],
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
        self.q = sk.Property(compute=self._q, depends_on=["p"])
        self.p = sk.Property(compute=self._p, depends_on=["a"])

    def _p(self):
        RUNS["plato"] += 1
        return self.a.value * 2

    def _q(self):
        RUNS["haze"] += 1
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
            self.r = sk.Property(compute=lambda: self.a.value)

    with pytest.raises(ValueError, match="does not say what it reads: r"):
        Undeclared(name="u")
    with pytest.raises(ValueError, match="does not say what it reads: r"):
        Undeclared.new(tmp_path / "u.md")

    # An input only a file could supply is named as such.
    class FileFed(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.r = sk.Property(compute=lambda: 1.0, depends_on=["malt"])

    with pytest.raises(KeyError, match="has no file to supply it"):
        FileFed(name="f")


def test_compute_runs_values_in_dependency_order(tmp_path):
    # Declared out of order, each formula runs once: the dependent would
    # otherwise run its input inside itself and the input then run again.
    reset_runs()
    sample = Declared(name="d")
    sample.a.value = 1.0
    sample.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (1, 1)
    sample.a.value = 2.0
    sample.compute()
    assert (RUNS["plato"], RUNS["haze"]) == (2, 2)
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
    assert (RUNS["plato"], RUNS["haze"]) == (0, 1)

    retained = Carbonation(name="r")
    retained.compute()
    retained.save(tmp_path / "r.md")
    reset_runs()
    again = Carbonation(tmp_path / "r.md")
    assert again.flatness.state == "current"
    again.compute()
    assert RUNS["flatness"] == 0


def test_a_row_added_with_a_derived_cell_holds_it():
    # The rule covers a cell supplied with its row as it covers one updated.
    sample = Shapes()
    sample.compute()
    sample.mashing.add(T=30, R=8.0, G=99.0)
    assert sample.mashing.at(30, "G").state == "edited"
    sample.compute()
    assert sample.mashing.at(30, "G").value == 99.0
    assert sample.edited() == ["mashing.G"]
    sample.compute(force=True)
    assert sample.mashing.at(30, "G").value == 0.125
    sample.mashing.add(T=40, R=16.0)
    assert sample.mashing.at(40, "G").state == "never computed"
    sample.compute()
    assert sample.mashing.at(40, "G").value == 0.0625


def test_replacing_a_property_replaces_its_declaration():
    # The new Property carries its own depends_on, and what read the name by
    # name keeps reading it.
    sample = Declared(name="d")
    sample.a.value = 1.0
    sample.b = sk.Property(2.0)
    sample.p = sk.Property(compute=lambda: sample.b.value * 3, depends_on=["b"])
    assert sample.dependencies("p") == ["b"]
    assert sample.dependents("a") == []
    assert sample.dependencies("q") == ["p"]
    sample.compute()
    sample.a.value = 9.0
    assert sample.p.state == "current"
    sample.b.value = 4.0
    assert sample.p.state == "outdated"
    sample.p = sk.Property(compute=lambda: 1.0)
    with pytest.raises(ValueError, match="does not say what it reads: p"):
        sample.compute()


def test_a_list_reads_stale_values_in_silence(tmp_path):
    # A read never raises, and a list filters, tabulates and summarises what it
    # holds without a warning per row; the sample reports on request.
    sample = Platos(name="s")
    sample.malt.value = 12.0
    sample.volume.value = 4.0
    sample.compute()
    sample.malt.value = 16.0
    samples = sk.SampleList([sample])
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert [s.name for s in samples.filter("plato > 1")] == ["s"]
        assert samples.to_dict(columns=["plato"])["plato_value"] == [3.0]
        assert samples.stats("plato").mean == 3.0
    assert sample.outdated() == ["plato", "haze"]
    with pytest.warns(UserWarning, match="outdated"):
        assert sample.plato.value == 3.0


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


def test_a_declaration_says_what_each_formula_reads(tmp_path):
    # One depends_on, a mapping keyed by channel where a property has two
    # formulas. Each channel names everything its own formula reads, repetition
    # included; the file is what factors it.
    class Cell(sk.Sample):
        def __init__(self, path=None, name=None):
            self.src = sk.Property(unit="mL")
            self.p = sk.Property(
                unit="mL",
                compute=lambda: self.src.value * 2,
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
                    compute=(lambda: 1.0) if computes_value else None,
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
                compute=lambda: self.src.value * 2,
                compute_uncertainty=lambda: abs(self.p.value) * 0.01,
                depends_on={"v": ["src"], "u": ["src", "p.v"]},
            )
            self.q = sk.Property(compute=lambda: self.p.value + 1, depends_on=["p"])
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
            self.p = sk.Property(compute=lambda: self.a.value * 2, depends_on=["a"])
            self.q = sk.Property(compute=lambda: 1.0, depends_on=[])
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
            self.p = sk.Property(compute=lambda: self.a.value * 2, depends_on=["a"])
            self.q = sk.Property(compute=lambda: 1.0, depends_on=[])
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
    wanted = sk.Property(compute=lambda: sample.src.value * 2, depends_on=["srcc"])
    with pytest.raises(KeyError, match="srcc"):
        sample.p = wanted
    assert "p" not in sample
    # The caller's object keeps its formula, to be assigned again once fixed.
    assert wanted.is_computed

    # The same over an existing property: it is left as it was.
    sample.p = sk.Property(compute=lambda: sample.src.value * 2, depends_on=["src"])
    with pytest.raises(ValueError, match="names one channel of another value"):
        sample.p = sk.Property(compute=lambda: 1.0, depends_on={"v": ["src.v"]})
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
                compute=lambda: self.a.value + self.b.value, depends_on=["a"]
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
        declare({"v": ["src"], "u": ["p.v"]}, compute=lambda: 1.0)
    with pytest.raises(ValueError, match="value of 'p' has no formula"):
        declare({"v": ["src"], "u": ["p.v"]}, compute_uncertainty=lambda: 0.1)
    # One list naming the value a value-only formula computes is a self-read,
    # and says so — not the advice to declare per channel.
    with pytest.raises(ValueError, match="cannot read what it produces"):
        declare(["p.v"], compute=lambda: 1.0)
    with pytest.raises(ValueError, match="in one formula"):
        declare(["p.v"], compute_quantity=lambda: (1.0, 0.1))
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
        declare(["nope.v"], compute=lambda: 1.0)


def test_reading_never_computes_on_any_thread():
    # The guard held only on the thread that imported samplekit: the flag was a
    # thread-local, and a worker thread read with the default — a filter or an
    # export there ran formulas that take minutes.
    import threading

    runs = []

    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.z = sk.Property(compute=self._z, depends_on=["a"])
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
    assert seen == {"value": None, "state": "never computed"}
    assert runs == []


def test_a_formula_cannot_change_the_value_it_is_producing():
    # Writing to its own property while computing panicked on the borrow: a
    # PanicException no `except Exception` catches, and one that left reading
    # computing for the rest of the process.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.z = sk.Property(compute=self._z, depends_on=["a"])
            super().__init__(path, name=name)

        def _z(self):
            self.z.unit = "mm"
            return 1.0

    sample = Model(name="s")
    with pytest.raises(RuntimeError, match="cannot change the value it is producing"):
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
        {"i": sk.Column(), "x": sk.Column(), "y": sk.Column()},
        "i",
        compute_rows=[(["y"], ["row.x"], y)],
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
                {"i": sk.Column(), "x": sk.Column(), "g": sk.Column()},
                "i",
                compute_rows=[(["g"], ["row.x"], lambda row: {"g": 1 / row["x"]})],
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
            self.inverse = sk.Property(compute=lambda: 1 / self.a.value, depends_on=["a"])
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
            self.one = sk.Property(compute=lambda: np.array([3.5]), depends_on=[])
            self.many = sk.Property(compute=lambda: np.array([1.0, 2.0]), depends_on=[])
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
        sk.Column(unit="degC", precision=".1f")
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
        sk.Column(unit="degC", symbol="T")
    # None says nothing, and is what a keyword left out passes.
    assert sk.Property(value=1.0, symbol=None).value == 1.0
    assert sk.Column(unit="degC", symbol=None).unit == "degC"
    assert not hasattr(sk.Column(unit="degC"), "symbol")


def test_a_list_carries_on_past_a_failure_and_raises_them_together(tmp_path):
    # As the command line does, every sample is computed and what failed is
    # raised at the end — one as itself, several as a group.
    class Model(sk.Sample):
        def __init__(self, path=None, name=None, a=1.0):
            self.a = sk.Property(a)
            self.inverse = sk.Property(compute=lambda: 1 / self.a.value, depends_on=["a"])
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
            self.slow = sk.Property(compute=self._slow, depends_on=["a"])
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
    "[figure.malt_volume]\nx = \"volume\"\ny = \"malt\"\ngroup = \"beer\"\n"
    "[figure.ebc]\nkind = \"line\"\nx = \"measurements.f\"\ny = \"measurements.ebc\"\n"
)


def figure_samples(tmp_path, rc=FIGURE_RC):
    (tmp_path / ".samplekitrc").write_text(rc)
    paths = []
    for name, malt, beer in [("C1", 12.0, "schwarz"), ("C2", 10.0, "bock"),
                                 ("C3", 11.0, "schwarz")]:
        path = tmp_path / f"{name}.md"
        path.write_text(
            f"---\nschema_version: 1\nname: {name}\nbeer: {beer}\nproperties:\n"
            f"  malt: {{v: {malt}, u: 0.1, unit: g}}\n  volume: {{v: {malt / 3}, unit: L}}\n"
            "tables:\n  measurements:\n    index: f\n    columns:\n      f: {unit: brix}\n"
            "      ebc: {}\n    rows:\n      - {f: 8.0, ebc: {v: 5.1, u: 0.05}}\n"
            "      - {f: 9.0, ebc: {v: 5.3, u: 0.05}}\n---\n"
        )
        paths.append(path)
    return paths


class Plotted(sk.Sample):
    @sk.figure
    def mass_bar(self, ax):
        ax.bar(["malt"], [self.malt.value])

    @sk.figure(subplots=(1, 2))
    def pair(self, axes):
        axes[0].plot([0, 1], [0, 1])
        axes[1].plot([0, 1], [1, 0])

    @sk.figure
    @classmethod
    def malts(cls, samples, ax):
        cls.received = [sample.name for sample in samples]
        ax.bar([sample.name for sample in samples], [sample.malt.value for sample in samples])


def test_a_figure_method_is_marked_as_one():
    from samplekit._figures import figures_of

    found = figures_of(Plotted)
    assert set(found) == {"mass_bar", "pair", "malts"}
    assert not found["mass_bar"].collection
    assert found["malts"].collection
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
    (drawn,) = sk.plot(samples, "malts", output=tmp_path / "all.png")
    assert Plotted.received == ["C1", "C2", "C3"]
    assert len(drawn.axes[0].patches) == 3


def test_plot_writes_the_format_its_extension_names(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    for name, magic in [("f.pdf", b"%PDF"), ("f.svg", b"<?xml"), ("f.png", b"\x89PNG")]:
        sk.plot(samples, "malt_volume", output=tmp_path / name)
        assert (tmp_path / name).read_bytes().startswith(magic), name


def test_plot_refuses_to_overwrite_unless_asked(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    target = tmp_path / "f.png"
    target.write_text("kept")
    with pytest.raises(FileExistsError):
        sk.plot(samples, "malt_volume", output=target)
    assert target.read_text() == "kept"
    sk.plot(samples, "malt_volume", output=target, overwrite=True)
    assert target.read_bytes().startswith(b"\x89PNG")


def test_a_declared_figure_draws_a_series_per_group_with_error_bars(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "malt_volume", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    labels = sorted(ax.get_legend_handles_labels()[1])
    assert labels == ["bock", "schwarz"]
    # Numbers group in their order, not as text: 3.3 before 12.0.
    (numeric,) = sk.plot(samples, x="volume", y="malt", group="malt",
                         output=tmp_path / "n.png")
    assert numeric.axes[0].get_legend_handles_labels()[1] == ["10", "11", "12"]
    # One errorbar container per series, each carrying its vertical bars.
    assert len(ax.containers) == 2
    assert all(container.has_yerr for container in ax.containers)
    assert ax.get_xlabel() == "volume [L]"
    assert ax.get_ylabel() == "malt [g]"
    # No title unless one is declared: a report gives the caption.
    assert ax.get_title() == ""


def test_a_declared_figure_over_a_table_draws_a_curve_per_sample(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "ebc", output=tmp_path / "f.png")
    ax = drawn.axes[0]
    assert sorted(ax.get_legend_handles_labels()[1]) == ["C1", "C2", "C3"]
    assert ax.get_xlabel().startswith("measurements.f")


def test_a_figure_name_both_declared_and_in_the_model_is_refused(tmp_path):
    rc = FIGURE_RC + "[figure.mass_bar]\nx = \"volume\"\ny = \"malt\"\n"
    samples = sk.SampleList([Plotted(p) for p in figure_samples(tmp_path, rc)])
    with pytest.raises(ValueError, match="both"):
        sk.plot(samples[0], "mass_bar", output=tmp_path / "f.png")
    with pytest.raises(KeyError, match="malt_volume"):
        sk.plot(samples, "malt_volum", output=tmp_path / "f.png")


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
        sk.plot(samples, "malt_volume")


def test_the_project_lists_its_figures(tmp_path):
    rc = FIGURE_RC.replace("group = \"beer\"\n", "group = \"beer\"\nquery = \"light\"\n")
    rc += "[query.light]\nfilter = \"malt < 12\"\n"
    sample = sk.Sample(figure_samples(tmp_path, rc)[0], model=False)
    declared = sample.project.figures.malt_volume
    assert (declared.kind, declared.x, declared.y) == ("scatter", "volume", "malt")
    assert (declared.group, declared.query) == ("beer", "light")
    assert list(sample.project.figures) == ["malt_volume", "ebc"]


STYLED_RC = FIGURE_RC + (
    "[style.figure]\n[style.math]\n"
    "[unit.g]\nfigure = '$\\mathrm{g}$'\nmath = '\\si{\\gram}'\n"
    "[property.malt]\nsymbol = \"m\"\nsymbol_figure = '$m$'\n"
)


def test_an_axis_is_labelled_in_the_figure_style(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False)
                             for p in figure_samples(tmp_path, STYLED_RC)])
    (plain,) = sk.plot(samples, "malt_volume", output=tmp_path / "p.png")
    assert plain.axes[0].get_ylabel() == "m [g]"
    (styled,) = sk.plot(samples, "malt_volume", style="figure", output=tmp_path / "s.png")
    assert styled.axes[0].get_ylabel() == "$m$ [$\\mathrm{g}$]"
    (given,) = sk.plot(samples, "malt_volume", y_label="Malt", title="T",
                       output=tmp_path / "g.png")
    assert given.axes[0].get_ylabel() == "Malt"
    assert given.axes[0].get_title() == "T"


def test_a_table_cell_is_an_axis(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="malt", y="measurements.ebc[8.0]", output=tmp_path / "c.png")
    ax = drawn.axes[0]
    (container,) = ax.containers
    assert container.has_yerr
    assert len(container.lines[0].get_xdata()) == 3


def test_a_label_with_latex_outside_math_is_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False)
                             for p in figure_samples(tmp_path, STYLED_RC)])
    with pytest.raises(ValueError, match=r"\\si\{\\gram\}"):
        sk.plot(samples, "malt_volume", style="math", output=tmp_path / "m.png")
    assert not (tmp_path / "m.png").exists()


def test_a_group_colours_the_curves_of_a_table(tmp_path):
    # Grouped, a curve takes its group's colour, and the legend names each group
    # once, in the groups' order.
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "ebc", group="beer", output=tmp_path / "g.png")
    ax = drawn.axes[0]
    assert ax.get_legend_handles_labels()[1] == ["bock", "schwarz"]
    # Three curves, two colours: the two schwarz samples share theirs.
    colours = [container.lines[0].get_color() for container in ax.containers]
    assert len(colours) == 3 and len(set(colours)) == 2


def test_the_projects_matplotlib_settings_shape_every_figure(tmp_path):
    import matplotlib

    rc = FIGURE_RC + '[matplotlib]\n"axes.labelsize" = 21\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    assert samples.project.matplotlib == {"axes.labelsize": 21}
    before = matplotlib.rcParams["axes.labelsize"]
    (drawn,) = sk.plot(samples, "malt_volume", output=tmp_path / "f.png")
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
            sk.plot(samples, "malt_volume", output=folder / "f.png")


def test_project_style_false_leaves_matplotlib_its_own(tmp_path):
    import matplotlib

    rc = FIGURE_RC + '[matplotlib]\n"axes.labelsize" = 21\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "malt_volume", output=tmp_path / "f.png", project_style=False)
    default = matplotlib.font_manager.FontProperties(
        size=matplotlib.rcParams["axes.labelsize"]).get_size_in_points()
    assert drawn.axes[0].xaxis.label.get_size() == default


def test_limits_scales_and_aspect_set_the_axes(tmp_path):
    rc = FIGURE_RC.replace('group = "beer"\n',
                           'group = "beer"\nx_limits = [0, 10]\naspect = "equal"\n')
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    declared = samples.project.figures.malt_volume
    assert declared.x_limits == (0.0, 10.0) and declared.aspect == "equal"
    (drawn,) = sk.plot(samples, "malt_volume", y_scale="log", y_limits=(1, None),
                       output=tmp_path / "f.png")
    ax = drawn.axes[0]
    assert ax.get_xlim() == (0.0, 10.0)
    assert ax.get_ylim()[0] == 1.0
    assert ax.get_yscale() == "log"
    assert ax.get_aspect() == 1.0


def test_a_bound_a_log_axis_cannot_take_is_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="at or below zero"):
        sk.plot(samples, "malt_volume", y_scale="log", y_limits=(0, 20),
                output=tmp_path / "f.png")
    assert not (tmp_path / "f.png").exists()


def test_a_group_no_sample_knows_is_refused_naming_the_nearest(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(KeyError, match="did you mean: beer"):
        sk.plot(samples, x="volume", y="malt", group="beet", output=tmp_path / "f.png")
    with pytest.raises(KeyError, match="names a column"):
        sk.plot(samples, "malt_volume", group="measurements.ebc", output=tmp_path / "f.png")
    assert not (tmp_path / "f.png").exists()


def test_an_output_without_a_format_matplotlib_writes_is_refused(tmp_path):
    # matplotlib gives a path without an extension one of its own: the file
    # named would not be the file written, nor the one overwrite= guards.
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (tmp_path / "fig.png").write_text("precious")
    with pytest.raises(ValueError, match="has none"):
        sk.plot(samples, "malt_volume", output=tmp_path / "fig")
    with pytest.raises(ValueError, match="writes no '.xyz'"):
        sk.plot(samples, "malt_volume", output=tmp_path / "fig.xyz")
    assert (tmp_path / "fig.png").read_text() == "precious"


def test_one_axis_holds_one_unit(tmp_path):
    figure_samples(tmp_path)
    (tmp_path / "C4.md").write_text(
        "---\nschema_version: 1\nname: C4\nbeer: schwarz\nproperties:\n"
        "  malt: {v: 0.011, unit: kg}\n  volume: {v: 4.0, unit: L}\n---\n"
    )
    samples = sk.SampleList([sk.Sample(p, model=False) for p in sorted(tmp_path.glob("*.md"))])
    with pytest.raises(ValueError, match="in g in C1 and in kg in C4"):
        sk.plot(samples, x="volume", y="malt", output=tmp_path / "f.png")


def test_an_uncertainty_axis_is_labelled_as_one(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="malt", y="malt.u", output=tmp_path / "f.png")
    assert drawn.axes[0].get_xlabel() == "malt [g]"
    assert drawn.axes[0].get_ylabel() == "u(malt) [g]"


def test_limits_are_finite_and_a_log_axis_takes_neither_bound_at_zero(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    for limits in [(float("nan"), 5), (0, float("inf")), (3, 3)]:
        with pytest.raises(ValueError):
            sk.plot(samples, "malt_volume", x_limits=limits, output=tmp_path / "f.png")
    with pytest.raises(ValueError, match="has -1"):
        sk.plot(samples, "malt_volume", y_scale="log", y_limits=(None, -1),
                output=tmp_path / "f.png")


def test_a_bar_carries_both_uncertainties(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, x="malt", y="volume", kind="bar", output=tmp_path / "f.png")
    from matplotlib.container import BarContainer

    (bars,) = [c for c in drawn.axes[0].containers if isinstance(c, BarContainer)]
    assert bars.errorbar is not None and bars.errorbar.has_xerr


def test_an_empty_title_or_label_draws_none(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (drawn,) = sk.plot(samples, "malt_volume", title="", y_label="",
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
            self.inverse = sk.Property(compute=self._inverse, depends_on=["a"])
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
    empty = samples.filter("malt > 1000")
    assert len(empty) == 0
    assert empty.project.root == samples.project.root
    assert "malt_volume" in empty.project.figures
    with pytest.raises(ValueError, match="no sample to draw"):
        sk.plot(empty, "malt_volume", output=tmp_path / "f.png")


def test_two_columns_writing_one_header_are_refused(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    with pytest.raises(ValueError, match="both write a column 'malt_uncertainty'"):
        samples.to_dict(columns=["malt", "malt.u"])
    cells = samples.to_dict(columns=["measurements.ebc[8.0].u", "measurements.f[8.0].unit"])
    assert list(cells) == ["measurements.ebc[8.0]_uncertainty", "measurements.f[8.0]_unit"]


def test_a_sample_whose_formula_is_its_own_method_is_freed():
    # A formula held strongly through Rust was a cycle the collector could not
    # see: the sample was never freed.
    import gc
    import weakref

    class Doubled(sk.Sample):
        def __init__(self, path=None, name=None):
            self.a = sk.Property(2.0)
            self.b = sk.Property(compute=self._double, depends_on=["a"])
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
            f"---\nschema_version: 1\nname: M{index:02}\nbeer: m{index % 3}\n"
            f"batch: b{index}\nproperties:\n  malt: {{v: {10 + index}.5, unit: g}}\n"
            f"  volume: {{v: {index}.0, unit: L}}\n---\n"
        )
    return sk.SampleList([sk.Sample(p, model=False) for p in sorted(tmp_path.glob("M*.md"))])


def test_what_a_figure_leaves_out_is_said(tmp_path):
    samples = many_samples(tmp_path, 4)
    with pytest.warns(UserWarning, match="1 of 4 samples not drawn: volume at or below zero"):
        (drawn,) = sk.plot(samples, x="volume", y="malt", x_scale="log",
                           output=tmp_path / "f.png")
    assert len(drawn.axes[0].lines) >= 1


def test_a_numeric_group_of_many_values_is_a_colour_scale(tmp_path):
    samples = many_samples(tmp_path)
    (drawn,) = sk.plot(samples, x="volume", y="malt", group="malt", output=tmp_path / "f.png")
    assert len(drawn.axes) == 2  # the axes and the colour bar
    assert drawn.axes[0].get_legend() is None
    with pytest.raises(ValueError, match="makes 12 groups"):
        sk.plot(samples, x="volume", y="malt", group="batch", output=tmp_path / "g.png")


def test_a_groups_legend_reads_in_its_precision_and_is_titled(tmp_path):
    rc = FIGURE_RC + '[property.volume]\nprecision = ".0f"\nsymbol = "V"\n'
    samples = many_samples(tmp_path, 3, rc)
    (drawn,) = sk.plot(samples, x="malt", y="malt", group="volume", output=tmp_path / "f.png")
    legend = drawn.axes[0].get_legend()
    assert [text.get_text() for text in legend.get_texts()] == ["0", "1", "2"]
    assert legend.get_title().get_text() == "V [L]"


def test_bars_stand_side_by_side_and_one_series_takes_one_bar_per_x(tmp_path):
    samples = many_samples(tmp_path, 6)
    (drawn,) = sk.plot(samples, x="beer", y="malt", group="batch", kind="bar",
                       output=tmp_path / "f.png")
    ticks = [label.get_text() for label in drawn.axes[0].get_xticklabels()]
    assert ticks == ["m0", "m1", "m2"]
    with pytest.raises(ValueError, match="two bars at beer = m0"):
        sk.plot(samples, x="beer", y="malt", kind="bar", output=tmp_path / "g.png")


def test_a_box_plot_draws_the_values_per_category(tmp_path):
    samples = many_samples(tmp_path, 6)
    (drawn,) = sk.plot(samples, x="beer", y="malt", kind="box", output=tmp_path / "f.png")
    ticks = [label.get_text() for label in drawn.axes[0].get_xticklabels()]
    # Each with how many samples its box holds.
    assert ticks == ["m0\n(2)", "m1\n(2)", "m2\n(2)"]
    with pytest.raises(ValueError, match="box plot"):
        sk.plot(samples, x="beer", y="malt", kind="box", group="batch",
                output=tmp_path / "g.png")


def test_a_legend_is_placed_and_a_figure_sized(tmp_path):
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    # Centimetres, converted for matplotlib.
    (drawn,) = sk.plot(samples, "malt_volume", legend="none", figsize=(10.16, 7.62),
                       output=tmp_path / "f.png")
    assert drawn.axes[0].get_legend() is None
    assert tuple(drawn.get_size_inches()) == pytest.approx((4.0, 3.0))
    with pytest.raises(ValueError, match="legend is one of"):
        sk.plot(samples, "malt_volume", legend="left-ish", output=tmp_path / "g.png")


def test_a_figure_size_written_as_text_is_in_centimetres_too(tmp_path):
    # `"figure.figsize" = "15, 10"` reached matplotlib as inches.
    rc = FIGURE_RC + '[matplotlib]\n"figure.figsize" = "10.16, 7.62"\n'
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    (drawn,) = sk.plot(samples, "malt_volume", output=tmp_path / "f.png")
    assert tuple(drawn.get_size_inches()) == pytest.approx((4.0, 3.0))
    # A size out of reason is said: centimetres are meant.
    with pytest.warns(UserWarning, match="sizes are in centimetres"):
        sk.plot(samples, "malt_volume", figsize=(160, 20), output=tmp_path / "g.png")


def test_a_project_names_the_style_its_figures_read_in(tmp_path):
    rc = (FIGURE_RC + "[render]\nfigure_style = \"figure\"\n[style.figure]\n"
          "[property.malt]\nsymbol_figure = '$m$'\n")
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path, rc)])
    assert samples.project.render.figure_style == "figure"
    (drawn,) = sk.plot(samples, "malt_volume", output=tmp_path / "f.png")
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
        sk.plot(samples, "malt_volume", output=tmp_path / "{name}.pdf")


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
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"
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
        "---\nschema_version: 1\nname: E\nproperties:\n  ibu:\n    v: 12.0\n"
        "    u: 0.005773502691896258\n    unit: mL\n    computed: {u: {}}\n"
        "    fingerprint: 18debc327c6f\n  headspace:\n    v: 1.1309733552923256\n"
        "    unit: hL\n    computed: {v: {ibu: fce15bfbafe1}}\n"
        "    fingerprint: 5bcbffb74128\n---\n"
    )
    sample = sk.Sample(path, model=False)
    sample.headspace.value = 1.5
    assert sample.edited() == ["headspace"]
    sample.save()
    text = path.read_text()
    assert "computed: {v: {ibu: fce15bfbafe1}}" in text
    assert "fingerprint: {edited:" in text


def test_a_column_never_computed_says_so_when_read():
    # Loose end 23: a property never computed warns when read, and a table's
    # column read as a column of None said nothing.
    table = sk.Table(
        {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
        "T",
        rows=[{"T": 10, "R": 2.0}, {"T": 20, "R": 4.0}],
        compute_rows=[("G", ["row.R"], lambda row: 1 / row.R.value)],
    )
    sample = sk.Sample()
    sample.mashing = table
    with pytest.warns(UserWarning, match="mashing.G was never computed"):
        assert sample.mashing.G.values == [None, None]
    with pytest.warns(UserWarning, match="never computed"):
        assert sample.mashing.values("G") == [None, None]
    sample.compute()
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert sample.mashing.G.values == [0.5, 0.25]
        assert sample.mashing.values("R") == [2.0, 4.0]


def test_a_sample_saved_from_python_is_a_snapshot(tmp_path):
    # A save is a snapshot naming the file, after one of what changed before it;
    # a save that changes nothing takes none.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    path = tmp_path / "c1.md"
    path.write_text("---\nschema_version: 1\nname: C1\nproperties:\n  malt: 1.0\n---\n")
    sample = sk.Sample(path, model=False)
    sample.malt.value = 2.0
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
    sk.keep("malt corrected")
    # The message given still names what ran — here, pytest.
    said = messages()
    assert len(said) == 2, said
    assert said[0].startswith("python ") and said[0].endswith(" · malt corrected"), said
    assert said[1] == "the project as SampleKit first kept it"


def test_a_script_is_one_snapshot_with_its_source_kept(tmp_path):
    # However many saves, one snapshot when the script ends, named by its
    # command line, its source kept beside it.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name in ("c1", "c2", "c3"):
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"
        )
    script = tmp_path / "raise.py"
    script.write_text(
        "import samplekit as sk\n"
        "for name in ('c1', 'c2', 'c3'):\n"
        f"    s = sk.Sample({str(tmp_path)!r} + '/' + name + '.md', model=False)\n"
        "    s.malt.value = 2.0\n"
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
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"
        )
    script = tmp_path / "fix.py"
    script.write_text(
        "import pathlib, sys\n"
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.malt.value = 2.0\n"
        "s.save()\n"
        "pathlib.Path(__file__).write_text('# rewritten after it ran\\n')\n"
        "sk.keep('first pass')\n"
        f"s = sk.Sample({str(tmp_path / 'c2.md')!r}, model=False)\n"
        "s.malt.value = 3.0\n"
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
        "s.malt.value = 4.0\n"
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
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"
        )
    other = tmp_path / "other.py"
    other.write_text(
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c2.md')!r}, model=False)\n"
        "s.malt.value = 3.0\n"
        "s.save()\n"
    )
    script = tmp_path / "outer.py"
    script.write_text(
        "import subprocess, sys\n"
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.malt.value = 2.0\n"
        "s.save()\n"
        f"subprocess.run([sys.executable, {str(other)!r}], check=True)\n"
    )
    subprocess.run([sys.executable, str(script)], check=True, cwd=tmp_path)
    git = ["git", "--git-dir", str(tmp_path / ".samplekit/history")]
    ids = subprocess.run(git + ["log", "--reverse", "--format=%H"], capture_output=True,
                         text=True, check=True).stdout.split()

    def malt(snapshot, name):
        import re
        text = subprocess.run(git + ["show", f"{snapshot}:{name}.md"], capture_output=True,
                              text=True, check=True).stdout
        return float(re.search(r"malt:\s*\{?\s*(?:v:\s*)?([-0-9.]+)", text).group(1))

    # Oldest first: once c2 holds 3.0, no later snapshot takes it back.
    seen = [malt(snapshot, "c2") for snapshot in ids]
    assert 3.0 in seen, seen
    assert 1.0 not in seen[seen.index(3.0):], seen
    assert malt(ids[-1], "c1") == 2.0 and malt(ids[-1], "c2") == 3.0


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
    for name, malt in [("c1", 1.0), ("c2", 2.0)]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {malt}\n  volume: {malt * 2}\n---\n")
    samples = sk.load(tmp_path)
    samples.to_csv(tmp_path / "table.csv", columns=["name", "malt"])
    sk.plot(samples, x="volume", y="malt", output=tmp_path / "figure.svg")

    def tag(path):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
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
    assert " · plot malt against volume" in tag(tmp_path / "figure.svg")
    assert "samplekit snapshot " in (tmp_path / "figure.svg").read_text()


def test_compute_goes_on_past_a_failure_and_waits_for_what_nobody_entered():
    # As samplekit compute does. A formula over an input nobody entered waits,
    # said as a warning; one that fails does not stop the others, and is raised
    # at the end, the others named.
    class Brew(sk.Sample):
        def __init__(self, path=None, name=None):
            self.og = sk.Property(1.05)
            self.fg = sk.Property()
            self.volume = sk.Property(20.0)
            self.abv = sk.Property(compute=lambda: (self.og.value - self.fg.value) * 131.25,
                                   depends_on=["og", "fg"])
            self.broken = sk.Property(compute=lambda: 1 / 0, depends_on=["og"])
            self.points = sk.Property(compute=lambda: (self.og.value - 1) * 1000,
                                      depends_on=["og"])
            self.total = sk.Property(compute=lambda: self.points.value * self.volume.value,
                                     depends_on=["points", "volume"])
            super().__init__(path, name=name)

    brew = Brew(name="b")
    with pytest.warns(UserWarning, match="'abv' waits for fg"):
        with pytest.raises(ZeroDivisionError):
            brew.compute()
    assert brew.points.value == pytest.approx(50.0)
    assert brew.total.value == pytest.approx(1000.0)
    assert brew.abv.value is None


def test_not_applicable_is_one_value_python_reads_and_writes(tmp_path):
    # Sk.NA in, `n/a` in the file, sk.NA out, and a formula over it gives it
    # without running.
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    ran = []

    class Brew(sk.Sample):
        def __init__(self, path=None, name=None):
            self.og = sk.Property(1.05)
            self.fg = sk.Property()
            self.abv = sk.Property(compute=lambda: ran.append(1) or (self.og.value - self.fg.value),
                                   depends_on=["og", "fg"])
            super().__init__(path, name=name)

    brew = Brew(name="b")
    brew.fg.value = sk.NA
    brew.compute()
    assert brew.abv.value is sk.NA
    assert ran == []
    path = tmp_path / "b.md"
    brew.save(path)
    assert "fg: n/a" in path.read_text() or "v: n/a" in path.read_text()
    assert Brew(path).fg.value is sk.NA
    assert str(sk.NA) == "n/a" and not sk.NA


def test_a_figure_leaves_out_a_value_not_applicable(tmp_path):
    # No point, as for a value never entered.
    import matplotlib
    matplotlib.use("Agg")
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    for name, malt in [("a", "1.0"), ("b", "n/a"), ("c", "3.0")]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: {malt}\n  volume: 2.0\n---\n")
    with pytest.warns(UserWarning, match="1 of 3 samples not drawn"):
        sk.plot(sk.load(tmp_path), x="volume", y="malt", output=tmp_path / "f.svg")


# ------------------------------------------------- review of 2026-09-27


class Tasted(sk.Sample):
    """A brew whose score is the mean of a tasting table, and whose alcohol
    reads a final gravity: either may be empty."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.og = sk.Property(1.05)
        self.fg = sk.Property()
        self.abv = sk.Property(compute=self._abv, depends_on=["og", "fg"])
        self.tasting = sk.Table({"taster": sk.Column(), "score": sk.Column()}, "taster")
        self.score = sk.Property(compute=self._score, depends_on=["tasting.score"])

    def _abv(self):
        return (self.og.value - self.fg.value) * 131.25

    def _score(self):
        import statistics
        return statistics.fmean(self.tasting.values("score"))


def test_a_formula_over_an_empty_column_waits():
    # A table without a row is an input nobody entered, as the command line
    # judges it: the mean of no score raised StatisticsError.
    brew = Tasted(name="b")
    with pytest.warns(UserWarning, match=r"b: .*'score' waits for tasting\.score"):
        brew.compute()
    assert brew.not_current()["score"] == "waits for tasting.score"
    brew.tasting.add(taster="Lea", score=40.0)
    brew.compute()
    assert brew.score.value == pytest.approx(40.0)
    assert "score" not in brew.not_current()


def test_not_current_says_what_a_value_waits_for():
    brew = Tasted(name="b")
    brew.tasting.add(taster="Lea", score=40.0)
    assert brew.not_current() == {"abv": "waits for fg", "score": "never computed"}
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        assert brew.abv.value is None
    said = [str(warning.message) for warning in caught]
    assert said == ["b: 'abv' waits for fg: nobody entered fg, so its formula does not run"]
    brew.fg.value = 1.01
    assert brew.not_current()["abv"] == "never computed"


def test_a_warning_points_at_the_scripts_line():
    brew = Tasted(name="b")
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        brew.abv.value
        brew.compute()
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
    brew = Tasted(name="b")
    brew.fg.value = 1.01
    brew.compute("abv")
    before = brew.abv.value
    for assign in (lambda: setattr(brew.abv, "value", [1.0, 2.0]),
                   lambda: setattr(brew.abv, "readings", [1.0, 2.0])):
        with pytest.raises(ValueError, match="override"):
            assign()
    assert brew.abv.value == before and brew.abv.readings is None
    assert brew.abv.is_computed


def test_replacing_a_formula_by_a_plain_property_is_refused():
    brew = Tasted(name="b")
    with pytest.raises(ValueError, match=r"s\.abv\.value = \.\.\. to override"):
        brew.abv = sk.Property(5.0)
    assert brew.abv.is_computed
    brew.abv = sk.Property(compute=lambda: 5.0, depends_on=[])
    assert brew.abv.is_computed
    del brew.abv
    brew.abv = sk.Property(5.0)
    assert brew.abv.value == 5.0 and not brew.abv.is_computed


def test_a_boolean_is_refused_on_a_property_with_a_unit(project):
    sample = quantity_sample(project)
    with pytest.raises(ValueError, match="boolean"):
        sample.malt.value = True
    with pytest.raises(ValueError, match="boolean"):
        sk.Property(True, unit="g")
    assert sample.malt.value == pytest.approx(3.1551234)
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
    table = sk.Table({"T": sk.Column(), "R": sk.Column()}, "T", rows=[{"T": 1, "R": 2.0}])
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
    table = sk.Table({"T": sk.Column(), "carbonation": sk.Column()}, "T",
                     rows=[{"T": 20, "carbonation": 2.0}])
    sample = sk.Sample(name="s")
    sample.malt = sk.Property(1.0)
    sample.conditioning = table
    assert sample["conditioning.carbonation[20]"] == 2.0
    with pytest.raises(KeyError, match="no row at"):
        sample["conditioning.carbonation[99]"]
    with pytest.raises(KeyError, match="did you mean: carbonation"):
        sample["conditioning.carbonatoin[20]"]
    with pytest.raises(KeyError, match="did you mean: malt"):
        sample["mal.u"]
    with pytest.raises(KeyError, match="did you mean: malt"):
        sample.dependencies("mal")
    with pytest.raises(KeyError, match="did you mean: malt"):
        sample.dependents("mal")
    with pytest.raises(KeyError, match="did you mean: malt"):
        sample.affected_by_change("mal")
    with pytest.raises(KeyError, match="did you mean: carbonation"):
        sample.dependencies("conditioning.carbonatoin")
    assert sample.dependencies("malt") == []


def test_a_table_method_writes_any_extension(collection):
    # The method names the format, the caller the file, as `--csv -o m.json`
    # writes CSV into m.json.
    samples = sk.SampleList(collection.root)
    samples.to_csv(collection.root / "m.json", columns=["name"])
    assert (collection.root / "m.json").read_text() == samples.to_csv(columns=["name"])
    samples.to_json(collection.root / "m.tsv", columns=["name"])
    assert (collection.root / "m.tsv").read_text() == samples.to_json(columns=["name"])


def test_a_declared_exports_other_extension_is_refused(collection):
    # As `samplekit export malts -o m.json` refuses it — the format is the
    # configuration's, and the path contradicts it.
    samples = sk.SampleList(collection.root)
    with pytest.raises(ValueError, match=r"'malts' writes CSV, and the extension names JSON"
                       r".*change format in \[export\.malts\]; nothing was written"):
        samples.export("malts", output=collection.root / "m.json")
    assert not list(collection.root.glob("m.*"))
    samples.export("malts", output=collection.root / "m.txt")
    assert (collection.root / "m.txt").is_file()


def test_stats_weights_its_mean_as_the_summary_does(collection, cli):
    # `mean (1/u²)` where every uncertainty is known, as --summary.
    samples = sk.SampleList(collection.root).sorted("name")
    measured = samples[:2]
    summary = measured.stats("malt")
    weights = [1 / 0.05**2, 1 / 0.02**2]
    assert summary.weighted
    assert summary.mean == pytest.approx(
        (12.5 * weights[0] + 13.1234 * weights[1]) / sum(weights))
    assert "(1/u²)" in repr(summary)
    said = cli(collection.root / "A.md", collection.root / "B.md", "-c", "name,malt", "--summary")
    assert "mean (1/u²)" in said and f"{summary.mean:.3f}" in said
    # C has no uncertainty: the plain mean stands, as the command line's.
    plain = samples.stats("malt")
    assert not plain.weighted
    assert plain.mean == pytest.approx((12.5 + 13.1234 + 11.0) / 3)
    assert not sk.Property([1.0, 2.0]).stats.weighted


def test_an_uncertainty_column_left_out_is_said(project):
    # Said as the command line says it on stderr.
    project.config()
    project.sample("A.md", name="A", properties={"malt": "{v: 12.5, unit: g}"})
    samples = sk.SampleList(project.root)
    with pytest.warns(UserWarning, match=r"malt_uncertainty.* is left out: it is empty for "
                      r"every sample"):
        written = samples.to_csv(columns=["name", "malt"])
    assert "uncertainty" not in written
    with pytest.warns(UserWarning, match="is left out"):
        samples.to_tsv(columns=["name", "malt"])
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        samples.to_json(columns=["name", "malt"])


def test_a_key_error_prints_its_message_on_its_lines(project):
    import traceback

    sample = quantity_sample(project)
    with pytest.raises(KeyError) as caught:
        sample["mal"]
    said = "".join(traceback.format_exception_only(caught.value))
    assert said.startswith("KeyError: unknown field 'mal'\n")
    assert "\\n" not in said
    assert isinstance(caught.value, KeyError) and caught.value.args[0].startswith("unknown")


def test_an_attribute_error_suggests_once(project):
    import traceback

    sample = quantity_sample(project)
    with pytest.raises(AttributeError) as caught:
        sample.mal
    said = "".join(traceback.format_exception_only(caught.value))
    assert said.lower().count("did you mean") == 1
    assert not hasattr(sample, "mal")


def test_a_type_is_named_with_its_article(project):
    sample = quantity_sample(project)
    with pytest.raises(TypeError) as caught:
        sample.tags.append(3)
    assert "an int" in str(caught.value) and "a int" not in str(caught.value)
    with pytest.raises(TypeError) as caught:
        sk.Property(compute=object())
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
        sk.plot(samples, "malt_volume")
    assert "output=" in str(caught.value) and "-o " not in str(caught.value)
    with pytest.raises(TypeError, match="x_label changes a declared figure"):
        sk.plot(Plotted(paths[0]), "mass_bar", x_label="m")


def test_auto_is_a_free_bound_of_an_axis(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.SampleList([sk.Sample(p, model=False) for p in figure_samples(tmp_path)])
    (figure,) = sk.plot(samples, "malt_volume", x_limits=[0, "auto"], output=tmp_path / "f.png")
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

    units = drawn("malt")
    assert units.returncode == 2, units.stderr
    assert "one axis holds one unit" in units.stderr
    unknown = drawn("maltt")
    assert unknown.returncode == 1, unknown.stderr


def test_an_empty_selection_writes_the_header(collection):
    samples = sk.SampleList(collection.root)
    nothing = samples.filter("malt > 100")
    assert len(nothing) == 0
    assert nothing.to_csv(columns=["name", "malt"]).startswith("name,malt")
    assert nothing.filter("malt > 1000").to_dict(columns=["name"]) == {"name": []}
    with pytest.raises(KeyError, match="did you mean: malt"):
        nothing.to_csv(columns=["name", "mal"])


# ------------------------------------------------- review of 2026-09-27, Python


def grouped_samples(tmp_path, values, rc="schema_version = 1\n"):
    """Samples of one numeric property each, `n/a` where the value is None."""
    tmp_path.mkdir(exist_ok=True)
    (tmp_path / ".samplekitrc").write_text(rc)
    for at, value in enumerate(values):
        written = "n/a" if value is None else value
        (tmp_path / f"s{at:02}.md").write_text(
            f"---\nschema_version: 1\nname: s{at:02}\nproperties:\n"
            f"  og: {1.040 + at / 1000}\n  abv: {4.0 + at / 10}\n  efficiency: {written}\n---\n"
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
        sk.plot(samples, x="volume", y="malt", group="measurements", output=output)
    with pytest.raises(KeyError, match="names a column"):
        sk.plot(samples, x="volume", y="malt", group="measurements.ebc", output=output)
    assert not output.exists()
    drawn = sk.plot(samples, x="volume", y="malt", group="measurements.ebc[8.0]", output=output)
    assert legend_of(drawn) == ["5.1"]


def test_a_value_not_applicable_leaves_a_numeric_group_numeric(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    few = grouped_samples(tmp_path / "few", [70, None, 62, 69])
    drawn = sk.plot(few, x="og", y="abv", group="efficiency", output=tmp_path / "few.png")
    assert legend_of(drawn) == ["62", "69", "70", "n/a"]
    legend = drawn[0].axes[0].get_legend()
    grey = [handle for handle, text in zip(legend.legend_handles, legend.get_texts())
            if text.get_text() == "n/a"][0]
    assert matplotlib.colors.to_hex(grey.get_color()) == matplotlib.colors.to_hex("0.6")
    many = grouped_samples(tmp_path / "many", [None] + list(range(60, 71)))
    drawn = sk.plot(many, x="og", y="abv", group="efficiency", output=tmp_path / "many.png")
    # A colour bar for the twelve values, and the sample without one named.
    assert len(drawn[0].axes) == 2
    assert legend_of(drawn) == ["n/a"]


def test_numbers_that_read_alike_are_told_apart_in_a_legend(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    rc = 'schema_version = 1\n[property.efficiency]\nprecision = ".0f"\n'
    samples = grouped_samples(tmp_path / "near", [66.2, 65.9, 70.0], rc)
    drawn = sk.plot(samples, x="og", y="abv", group="efficiency", output=tmp_path / "f.png")
    assert legend_of(drawn) == ["65.9", "66.2", "70.0"]
    apart = grouped_samples(tmp_path / "apart", [66.0, 70.0], rc)
    drawn = sk.plot(apart, x="og", y="abv", group="efficiency", output=tmp_path / "g.png")
    assert legend_of(drawn) == ["66", "70"]


def test_text_is_refused_where_a_number_is_held(tmp_path):
    path = tmp_path / "b.md"
    path.write_text(
        "---\nschema_version: 1\nname: b\nproperties:\n  og: {v: 1.048, readings: [1.047, 1.049]}\n"
        "  style: {v: ipa}\ntables:\n  t:\n    index: d\n    columns: {d: {}, g: {}}\n"
        "    rows:\n      - {d: 1, g: 1.03}\n---\n"
    )
    brew = sk.Sample(path, model=False)
    for wrong in ("1.05", True):
        with pytest.raises(ValueError, match="'og' holds a number"):
            brew.og.value = wrong
    with pytest.raises(ValueError, match="holds a number"):
        brew.t.at(1, "g").value = "1.03"
    assert brew.og.value == pytest.approx(1.048)
    # A property holding text takes text.
    brew.style.value = "stout"
    brew.og.value = 1.05
    assert brew.og.value == 1.05


class Brewed(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.og = sk.Property(1.05)
        self.volume = sk.Property(20.0)
        self.efficiency = sk.Property(compute=lambda: (self.og.value - 1) * self.volume.value,
                                      depends_on=["og", "volume"])


def test_a_value_waiting_says_so_when_read_as_compute_does():
    brew = Brewed(name="b")
    brew.compute()
    brew.volume.value = None

    def said(action):
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            action()
        return [str(warning.message) for warning in caught]

    # Computed before its input was taken away: read, it waits, as compute()
    # says it — not "stale: compute() runs it again", which compute declines.
    sentence = "b: 'efficiency' waits for volume: nobody entered volume, so its formula does not run"
    assert said(brew.compute) == [sentence]
    assert said(lambda: brew.efficiency.value) == [sentence]

    # An input that failed was entered: it is said to have failed.
    class Broken(sk.Sample):
        def __init__(self, path=None, name=None):
            super().__init__(path, name=name)
            self.og = sk.Property(1.05)
            self.double = sk.Property(compute=lambda: 1 / 0, depends_on=["og"])
            self.quad = sk.Property(compute=lambda: self.double.value * 2, depends_on=["double"])

    def computed():
        with pytest.raises(ZeroDivisionError):
            Broken(name="k").compute()

    assert said(computed) == [
        "k: 'quad' waits for double: double failed, so its formula does not run"]


def test_a_warning_names_an_unnamed_sample_by_its_file(tmp_path):
    path = tmp_path / "brew-7.md"
    path.write_text("---\nschema_version: 1\nproperties:\n  og: 1.05\n---\n")
    brew = Tasted(path)
    with pytest.warns(UserWarning, match=r"^brew-7: 'abv' waits for fg"):
        assert brew.abv.value is None


def test_an_empty_selection_writes_the_whole_header(collection, cli, tmp_path):
    samples = sk.SampleList(collection.root)
    nothing = samples.filter("malt > 100")
    root = collection.root
    assert nothing.to_csv(columns=["name", "malt"]) == cli(
        root, "-c", "name,malt", "-f", "malt > 100", "--csv")
    assert nothing.to_csv(columns=["name", "malt"]).startswith(
        "name,malt_value [g],malt_uncertainty [g]")
    assert nothing.to_tsv(columns=["name", "malt"]) == cli(
        root, "-c", "name,malt", "-f", "malt > 100", "--tsv")
    assert nothing.to_csv(profile="platos") == cli(
        root, "--profile", "platos", "-f", "malt > 100", "--csv")
    with pytest.warns(UserWarning, match="'malts' is written from 0 of 3 samples"):
        written = nothing.export("malts", output=tmp_path / "malts.csv")
    assert written.read_text().splitlines()[0] == samples.export(
        "malts", output=tmp_path / "all.csv").read_text().splitlines()[0]


def test_a_figure_of_samples_lacking_an_axis_is_refused_as_data(tmp_path):
    import matplotlib
    from samplekit import _figures
    matplotlib.use("Agg")
    paths = figure_samples(tmp_path)
    empty = tmp_path / "C4.md"
    empty.write_text("---\nschema_version: 1\nname: C4\nproperties:\n  malt: {unit: g}\n"
                     "  volume: 3.0\n---\n")
    with pytest.raises(ValueError, match="^the one sample selected holds no malt: nothing to draw") as one:
        sk.plot(sk.Sample(empty), x="volume", y="malt", output=tmp_path / "f.png")
    assert _figures.raised_by_the_data(one.value)
    other = tmp_path / "C5.md"
    other.write_text(empty.read_text().replace("C4", "C5"))
    with pytest.raises(ValueError, match="none of the 2 samples holds both 'volume' and 'malt'"):
        sk.plot(sk.load(empty, other), x="volume", y="malt", output=tmp_path / "f.png")
    assert paths


def test_a_figure_written_by_a_script_names_the_script_and_its_axes(tmp_path):
    figure_samples(tmp_path)
    script = tmp_path / "draw.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "sk.plot(sk.load(sys.argv[1]), x='volume', y='malt', output=sys.argv[2])\n"
    )
    output = tmp_path / "f.svg"
    subprocess.run([sys.executable, str(script), str(tmp_path), str(output)], check=True,
                   env={**os.environ, "MPLBACKEND": "Agg"}, cwd=tmp_path)
    import hashlib
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    history = tmp_path / ".samplekit/history"
    tag = subprocess.run(
        ["git", "--git-dir", str(history), "cat-file", "-p", output_record(history, digest)],
        capture_output=True, text=True, check=True).stdout
    assert "python " in tag and script.name in tag, tag
    assert " · plot malt against volume" in tag, tag


def test_keep_takes_one_line_of_text(tmp_path):
    with pytest.raises(TypeError, match="keep\\(\\) takes a message as text"):
        sk.keep(3)
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    (tmp_path / "c1.md").write_text("---\nschema_version: 1\nname: C1\nproperties:\n  malt: 1.0\n---\n")
    script = tmp_path / "keep.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "s = sk.Sample(sys.argv[1], model=False)\n"
        "s.malt.value = 2.0\ns.save()\nsk.keep('first\\nline')\n"
        "s.malt.value = 3.0\ns.save()\nsk.keep('   ')\n"
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
        sample.malt.value = 4.0
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
    table = sk.Table({"d": sk.Column(), "g": sk.Column()}, "d")
    with pytest.raises(AttributeError, match="did you mean: values\\?"):
        table.valuse


def test_an_auto_bound_leaves_a_margin(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    samples = sk.load(*figure_samples(tmp_path))
    drawn = sk.plot(samples, x="volume", y="malt", y_limits=(0, "auto"), output=tmp_path / "f.png")
    low, high = drawn[0].axes[0].get_ylim()
    top = 12.0 + 0.1
    assert low == 0
    # matplotlib's margin past the highest error bar, over the whole range.
    assert high == pytest.approx(top + matplotlib.rcParams["axes.ymargin"] * top)


def test_a_cell_axis_names_its_row(tmp_path):
    import matplotlib
    matplotlib.use("Agg")
    rc = FIGURE_RC + '[property."measurements.ebc"]\nsymbol = "ebc"\n'
    samples = sk.load(*figure_samples(tmp_path, rc))
    drawn = sk.plot(samples, x="volume", y="measurements.ebc[8.0]", output=tmp_path / "f.png")
    assert drawn[0].axes[0].get_ylabel() == "ebc, f 8.0"
    column = sk.plot(samples, "ebc", output=tmp_path / "g.png")
    assert column[0].axes[0].get_ylabel() == "ebc"


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
        sk.plot(samples, "malt_volum")
    assert str(unknown.value).splitlines()[1:] == [
        "  did you mean: malt_volume?", "  the figures: malt_volume, ebc"]
    sample = sk.SampleList(collection.root)[0]
    with pytest.raises(KeyError) as field:
        sample["mal"]
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
    table = sk.Table({"d": sk.Column(), "g": sk.Column()}, "d")
    table.add(d=1, g=2.0)
    with pytest.raises(KeyError) as row:
        table.at(99, "g")
    assert "\n\n" not in str(row.value) and not str(row.value).endswith("\n")
    with pytest.raises(KeyError, match="an empty field name"):
        sample[""]
    with pytest.raises(ValueError, match="the list holds no sample"):
        sk.SampleList([]).stats("malt")


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
        self.malt = sk.Property(unit="g", value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.twice = sk.Property(unit="g", compute=self._twice, depends_on=["malt"])
        self.mashing = sk.Table(
            {"T": sk.Column(), "G": sk.Column(unit="NTU")},
            "T",
            rows=[{"T": 10}],
            compute_rows=[("G", ["row.T"], lambda row: 1 / row.T.value)],
        )

    def _twice(self):
        return 2 * self.malt.value


def test_formula_code_names_each_formula_and_what_declares_it(project):
    path = project.sample("d.md", properties={"malt": "{readings: [1.0, 2.0, 3.0], unit: g}"})
    sample = _Declared(path)
    code = sample._formula_code()
    assert sorted(code) == ["malt", "mashing.G", "twice"]
    # A method as its function, which holds no sample; the statistics as
    # declared; a column's derivation with what it reads and fills.
    assert code["twice"] == [("compute", _Declared._twice), ("unit", "g")]
    assert [(role, repr(given)) for role, given in code["malt"]] == [
        ("value", "sk.stats.mean"),
        ("uncertainty", "sk.stats.standard_error"),
        ("unit", "'g'"),
    ]
    assert [role for role, _ in code["mashing.G"]] == ["compute_rows", "inputs", "outputs", "unit"]
    assert code["mashing.G"][1:] == [("inputs", "row.T"), ("outputs", "G"), ("unit", "NTU")]
    # A value current by its inputs is planned when its formula changed.
    sample.compute()
    assert sample._plan([], False, False) == []
    assert sample._plan([], False, False, ["twice"]) == [("twice", "formula changed")]


# ------------------------------------------- readings and their value


class _Viscous(sk.Sample):
    """A mouthfeel table whose `sweetness` column declares the statistics of its cells."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)
        self.mouthfeel = sk.Table(
            {
                "T": sk.Column(),
                "sweetness": sk.Column(
                    unit="cP", value=sk.stats.mean, uncertainty=sk.stats.standard_error
                ),
            },
            "T",
        )


def test_a_column_declares_the_statistics_of_its_cells(project):
    path = project.root / "v.md"
    sample = _Viscous(name="v")
    sample.mouthfeel.add(T=40, sweetness=[1.0, 2.0, 3.0])
    cell = sample.mouthfeel.at(40, "sweetness")
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
    assert corrected.outdated() == ["mouthfeel.sweetness"]
    assert corrected.mouthfeel.at(40, "sweetness").value == 2.0
    corrected.compute()
    assert corrected.mouthfeel.at(40, "sweetness").value == 3.0
    assert corrected.outdated() == []
    corrected.save()
    assert _Viscous(path).outdated() == []
    # Readings given to the cell in a session leave it not current in the
    # same way; a row replaced whole takes its statistic at once.
    again = _Viscous(path)
    again.mouthfeel.at(40, "sweetness").readings = [3.0, 5.0]
    assert again.outdated() == ["mouthfeel.sweetness"]
    again.mouthfeel.update(40, sweetness=[7.0, 9.0])
    assert again.mouthfeel.at(40, "sweetness").value == 8.0


def test_a_column_takes_only_statistics():
    with pytest.raises(TypeError, match="statistic of its cells' readings"):
        sk.Column(value=5)
    with pytest.raises(ValueError, match="standard_error"):
        sk.Column(value=sk.stats.standard_error)
    assert "value=sk.stats.median" in repr(sk.Column(value=sk.stats.median))


def test_a_list_assigned_with_no_statistic_gives_no_value(project):
    path = project.sample("s.md", name="s", properties={"fg": "1.01"})
    sample = sk.Sample(path, model=False)
    sample.og = sk.Property([1.05, 1.06])
    assert sample.og.readings == [1.05, 1.06]
    # No statistic is declared, so no mean stands in, and none is written.
    assert sample.og.value is None
    sample.save()
    text = path.read_text()
    assert "og: {readings: [1.05, 1.06]}" in text


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
        sk.plot(samples.filter("malt > 1000"), x="volume", y="malt",
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
    sample.malt = sk.Property(1.5, unit="g")
    for name in ("kept.txt", "kept", "kept.sample.yaml"):
        path = tmp_path / name
        sample.save(path)
        assert sk.Sample(path, model=False).malt.value == 1.5


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
         f"import samplekit as sk\nprint(sk.load({str(tmp_path)!r})[0].malt.value)\n"],
        check=True, env=env, cwd=tmp_path, capture_output=True)
    assert not (tmp_path / ".samplekit/history").exists()
    script = tmp_path / "draw.py"
    script.write_text(
        "import sys\nimport samplekit as sk\n"
        "samples = sk.load(sys.argv[1])\n"
        "samples.to_csv('table.csv', columns=['name', 'malt'], overwrite=True)\n"
        "sk.plot(samples, x='volume', y='malt', output='f.svg', overwrite=True)\n"
        "sk.plot(samples, x='volume', y='malt', output='g.png', overwrite=True)\n"
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
        digest = hashlib.sha256((tmp_path / name).read_bytes()).hexdigest()
        record = output_record(tmp_path / ".samplekit/history", digest)
        tied = subprocess.run(git + ["rev-parse", f"{record}^{{commit}}"],
                              capture_output=True, text=True, check=True).stdout.strip()
        assert tied == own, name
    # `log` over the project lists it, though it changed no kept file.
    binary = BINARY
    if binary.exists():
        shown = subprocess.run([str(binary), "log"], cwd=tmp_path, capture_output=True,
                               text=True, check=True).stdout
        assert "no kept file · log --script 1" in shown, shown
    # Run again, it is a step again.
    subprocess.run([sys.executable, str(script), str(tmp_path)], check=True, env=env,
                   cwd=tmp_path)
    count = subprocess.run(git + ["rev-list", "--count", "HEAD"], capture_output=True,
                           text=True, check=True).stdout.strip()
    assert count == "3"


def test_a_python_c_script_keeps_its_source(tmp_path):
    (tmp_path / ".samplekitrc").write_text("schema_version = 1\n")
    (tmp_path / "c1.md").write_text(
        "---\nschema_version: 1\nname: c1\nproperties:\n  malt: 1.0\n---\n")
    code = (
        "import samplekit as sk\n"
        f"s = sk.Sample({str(tmp_path / 'c1.md')!r}, model=False)\n"
        "s.malt.value = 4.0\n"
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
    for group in (["beer", "name"], "beer,name", "beer, name"):
        (drawn,) = sk.plot(samples, x="volume", y="malt", group=group,
                           output=tmp_path / "f.png", overwrite=True)
        legend = drawn.axes[0].get_legend()
        assert [text.get_text() for text in legend.get_texts()] == [
            "bock, C2", "schwarz, C1", "schwarz, C3"]
        assert legend.get_title().get_text() == "beer, name"
    # Each field is checked as one is: a misspelt one names the nearest.
    with pytest.raises(KeyError, match="name"):
        sk.plot(samples, x="volume", y="malt", group=["beer", "nmae"],
                output=tmp_path / "g.png")
    with pytest.raises(TypeError, match="group is a field, or several in a list"):
        sk.plot(samples, x="volume", y="malt", group=3, output=tmp_path / "g.png")
    assert not (tmp_path / "g.png").exists()


def test_a_list_is_grouped_by_its_fields(tmp_path):
    # As --group splits a table, a dict in the groups' order.
    figure_samples(tmp_path)
    (tmp_path / "C4.md").write_text(
        "---\nschema_version: 1\nname: C4\nproperties:\n  malt: {v: 9.0, unit: g}\n---\n")
    samples = sk.SampleList(tmp_path)
    groups = samples.group_by("beer")
    assert list(groups) == ["bock", "schwarz", None]
    assert [s.name for s in groups["schwarz"]] == ["C1", "C3"]
    assert isinstance(groups["bock"], sk.SampleList)
    # The samples are the list's own.
    assert groups["schwarz"][0] is samples["C1"]
    for fields in (["beer", "name"], "beer,name", ("beer", "name")):
        combined = samples.group_by(fields)
        assert list(combined) == [("bock", "C2"), ("schwarz", "C1"), ("schwarz", "C3"),
                                  (None, "C4")]
    # A list sorted first is sorted within each group.
    heaviest = samples.sorted("-malt").group_by("beer")
    assert [s.name for s in heaviest["schwarz"]] == ["C1", "C3"]
    with pytest.raises(KeyError, match="beer"):
        samples.group_by("beet")
    with pytest.raises(TypeError, match="group_by takes a field"):
        samples.group_by(3)
    # Checked against the list it came from, as a filter is.
    nothing = samples.filter("malt > 100")
    assert nothing.group_by("beer") == {}
    with pytest.raises(KeyError, match="beer"):
        nothing.group_by("beet")


def test_a_filter_reads_the_state_of_each_sample(tmp_path):
    # `state` reads as the command line reads it.
    (tmp_path / ".samplekitrc").write_text(
        'schema_version = 1\n[model]\npath = "model.py"\nclass = "Chain"\n'
    )
    (tmp_path / "model.py").write_text(CHAIN)
    for name, extra in [("a", ""), ("b", ""), ("c", "yeast_strain: US05\nyeast_strain: S04\n")]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\n{extra}properties:\n  malt: {{v: 1.5}}\n---\n"
        )
    namespace = {"__file__": str(tmp_path / "model.py")}
    exec(CHAIN, namespace)
    failing = namespace["Chain"](tmp_path / "a.md")
    failing.malt.value = datetime.date(2026, 9, 27)
    with pytest.raises(TypeError):
        failing.compute()
    failing.save()

    samples = sk.SampleList(tmp_path)
    names = lambda selected: sorted(sample.name for sample in selected)
    assert names(samples.filter("state == failed")) == ["a"]
    # a's `quadruple` never ran either: what it reads failed.
    assert names(samples.filter("state == never_computed")) == ["a", "b", "c"]
    # a's malt is now a date among numbers, which `validate` finds too.
    assert names(samples.filter("state == defective")) == ["a", "c"]
    assert names(samples.filter("state == defective && !(state == failed)")) == ["c"]
    assert names(samples.filter("state == defective && state == never_computed")) == ["a", "c"]
    with pytest.raises(ValueError, match="did you mean: 'outdated'"):
        samples.filter("state == stale")



def chain_project(root, model=True):
    """Three samples of the chain model: a failed, b current, c with a key written twice."""
    (root / ".samplekitrc").write_text(
        'schema_version = 1\n[model]\npath = "model.py"\nclass = "Chain"\n'
    )
    for name, extra in [("a", ""), ("b", ""), ("c", "yeast_strain: US05\nyeast_strain: S04\n")]:
        (root / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\n{extra}properties:\n  malt: {{v: 1.5}}\n---\n"
        )
    (root / "model.py").write_text(CHAIN)
    namespace = {"__file__": str(root / "model.py")}
    exec(CHAIN, namespace)
    failing = namespace["Chain"](root / "a.md")
    failing.malt.value = datetime.date(2026, 9, 27)
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
        assert names(without.filter("state == never_computed")) == ["a", "c"]
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
                "state": [["failed", "defective", "never_computed"], ["current"],
                          ["defective", "never_computed"]],
            }
            assert samples.to_csv(columns=["name", "state"]).splitlines() == [
                "name,state",
                "a,failed;defective;never_computed",
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
            f"---\nschema_version: 1\nname: {name}\nproperties:\n  malt: 1.0\n---\n"
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
    assert list(samples.group_by("beer")) == ["bock", "schwarz"]
    heaviest = samples.sorted("-malt")
    assert list(heaviest.group_by("beer")) == ["schwarz", "bock"]
    # Narrowed, the order is kept, and so is the list's being sorted.
    assert list(heaviest.filter("malt > 1").group_by("beer")) == ["schwarz", "bock"]
    assert list(heaviest[0:3].group_by("beer")) == ["schwarz", "bock"]
    # Sorted in place, the same.
    inplace = samples.filter("malt > 1")
    inplace.sort("-malt")
    assert list(inplace.group_by("beer")) == ["schwarz", "bock"]
    # `+` makes a list nobody sorted: by the values again.
    joined = heaviest[0:1] + heaviest[1:]
    assert [s.name for s in joined] == ["C1", "C3", "C2"]
    assert list(joined.group_by("beer")) == ["bock", "schwarz"]


def test_a_profile_that_groups_writes_one_table_carrying_its_groups(tmp_path, cli):
    # A declared profile brings its groups to a data format, as `--profile NAME
    # --csv` writes it, and to a declared export.
    (tmp_path / ".samplekitrc").write_text(
        "schema_version = 1\n"
        '[profile.by_status]\ncolumns = [{field = "name"}, {field = "malt"}]\n'
        'group = ["status"]\n'
        '[export.by_status]\nprofile = "by_status"\nformat = "csv"\n'
        'output = "out/by_status.csv"\n'
    )
    for name, status, malt in [("A", "approved", 12.5), ("B", "rejected", 9.0),
                               ("C", "approved", 10.0)]:
        (tmp_path / f"{name}.md").write_text(
            f"---\nschema_version: 1\nname: {name}\nstatus: {status}\n"
            f"properties:\n  malt: {{v: {malt}, u: 0.1, unit: g}}\n---\n"
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


def test_a_column_repr_names_its_unit_as_python_writes_it():
    # It printed `Column(unit=Some("degC"), symbol=None)`, Rust's spelling; a
    # column declares no symbol any more.
    assert repr(sk.Column(unit="degC")) == 'Column(unit="degC")'
    assert repr(sk.Column()) == 'Column(unit=None)'


def test_a_sample_without_a_name_is_named_by_its_file(tmp_path):
    # The file's name without its extension, where the file writes no `name:`;
    # assigned, the name is written and the file keeps its own; None takes it
    # away again. In memory with no file, there is none until it is saved.
    path = tmp_path / "s-1.md"
    path.write_text("---\nschema_version: 1\nproperties:\n  malt: {v: 2.0}\n---\n")
    sample = sk.Sample(path)
    assert sample.name == "s-1"
    assert sk.SampleList(tmp_path)["s-1"].path == sample.path
    sample.name = "Cask A"
    sample.save()
    assert "name: Cask A" in path.read_text()
    assert sk.Sample(path).name == "Cask A"
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
        'schema_version = 1\n[property.malt]\nsymbol = "m"\n')
    path = project.sample("s.md", name="s", properties={"malt": "{v: 1.0, unit: g}"})
    sample = sk.Sample(path, model=False)
    assert sample.malt.symbol == "m"
    with pytest.raises(AttributeError):
        sample.malt.symbol = "M"
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
            self.t = sk.Table({"N": sk.Column(), "who": sk.Column()}, "N",
                              compute_rows=[("who", ["row.N"],
                                             lambda row: f"{self.name}#{int(row.N.value)}")])

    sample = Named(path)
    sample.compute()
    assert sample.t.who.values == ["a-1#1"]
