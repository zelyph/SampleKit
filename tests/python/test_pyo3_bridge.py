"""The bridge between the Rust core and Python."""

import datetime
import sys
import threading
import time
import traceback

import pytest

import samplekit as sk


def plato_sample(project):
    return sk.Sample(
        project.sample(
            "s.md",
            name="S",
            properties={
                "malt": "{v: 12.5, unit: g}",
                "plato": "{v: 3.05, unit: g/L}",
            },
        )
    )


class Doubling(sk.Sample):
    def __init__(self, path=None):
        super().__init__(path)
        self.malt = sk.Property(2.0)
        self.double = sk.Property(compute=lambda: self.malt.value * 2)
        self.set_dependencies("double", depends_on=["malt"])


def test_wrapper_shares_the_underlying_property(project):
    sample = plato_sample(project)
    first, second = sample.malt, sample["malt"]
    first.value = 13.0
    assert second.value == 13.0


def test_reconstructed_wrapper_keeps_the_handle(project):
    sample = plato_sample(project)
    kept = sample.malt
    dict(sample.items())["malt"].value = 14.0
    assert kept.value == 14.0
    sample.save()
    assert "v: 14.0" in sample.path.read_text()


def test_mutation_through_a_wrapper_invalidates_dependents():
    sample = Doubling()
    sample.compute()
    assert sample.double.value == 4.0
    wrapper = sample.malt
    wrapper.value = 5.0
    assert sample.double.is_outdated
    sample.compute()
    assert sample.double.value == 10.0


def test_callback_exception_propagates_unchanged():
    def formula():
        raise KeyError("missing measurement")

    prop = sk.Property(compute=formula)
    with pytest.raises(KeyError) as caught:
        prop.compute()
    assert caught.value.args == ("missing measurement",)


def test_callback_exception_keeps_its_traceback():
    def failing_formula():
        return {}["absent"]

    prop = sk.Property(compute=failing_formula)
    with pytest.raises(KeyError) as caught:
        prop.compute()
    frames = [frame.name for frame in traceback.extract_tb(caught.value.__traceback__)]
    assert "failing_formula" in frames


def test_unknown_property_raises_key_error(project):
    sample = plato_sample(project)
    with pytest.raises(KeyError) as caught:
        sample["plto"]
    message = str(caught.value)
    assert "available" in message
    assert "plato" in message


def test_incomparable_comparison_raises_type_error():
    label = sk.Property("Dunkel 57")
    assert label == "Dunkel 57"
    with pytest.raises(TypeError):
        label < 3


def test_missing_file_raises_file_not_found_error(tmp_path):
    with pytest.raises(FileNotFoundError):
        sk.Sample(tmp_path / "absent.md")
    with pytest.raises(FileNotFoundError):
        sk.SampleList(tmp_path / "nowhere")


def test_batch_conflict_raises_file_exists_error(collection, tmp_path):
    out = tmp_path / "out"
    out.mkdir()
    (out / "A.md").write_text("kept")
    with pytest.raises(FileExistsError):
        sk.SampleList(collection.root).save_all(out)
    assert (out / "A.md").read_text() == "kept"
    assert not (out / "B.md").exists()


def test_a_declared_model_loads_without_consent(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    sample = sk.Sample(project.sample("a.md", properties={"malt": "3.0"}))
    assert project.imported()
    assert type(sample).__name__ == "Model"
    sample.compute()
    assert sample.double.value == 6.0


def test_model_false_does_not_import(project):
    project.model()
    project.config('schema_version = 1\n\n[model]\npath = "model.py"\n')
    path = project.sample("a.md", properties={"malt": "3.0"})
    sample = sk.Sample(path, model=False)
    samples = sk.SampleList(project.root, model=False)
    assert not project.imported()
    assert type(sample) is sk.Sample
    assert all(type(each) is sk.Sample for each in samples)


def test_error_messages_retain_suggestions(project):
    sample = plato_sample(project)
    with pytest.raises(AttributeError) as caught:
        sample.plto
    assert "did you mean: plato?" in str(caught.value)
    with pytest.raises(KeyError) as caught:
        sk.SampleList(project.root).sort("plto")
    assert "plato" in str(caught.value)


def test_scalars_round_trip_without_changing_kind(tmp_path):
    values = {
        "flag": True,
        "count": 7,
        "ratio": 0.5,
        "label": "Dunkel 57",
        "day": datetime.date(2026, 9, 14),
        "moment": datetime.datetime(2026, 9, 14, 10, 30, 5),
    }
    sample = sk.Sample(name="S")
    for key, value in values.items():
        setattr(sample, key, value)
    sample.nothing = None
    path = tmp_path / "s.md"
    sample.save(path)
    again = sk.Sample(path)
    for key, value in values.items():
        read = getattr(again, key)
        assert read == value
        assert type(read) is type(value)
    assert sample.nothing is None
    assert "nothing" not in again


def test_python_subclasses_use_the_specific_conversion_first(tmp_path):
    sample = sk.Sample()
    sample.flag = True
    sample.moment = datetime.datetime(2026, 1, 2, 3, 4)
    assert sample.flag is True
    assert type(sample.moment) is datetime.datetime
    path = tmp_path / "s.md"
    sample.save(path)
    text = path.read_text()
    assert "flag: true" in text
    assert "moment: 2026-01-02T03:04" in text


def test_timezone_aware_datetime_is_refused():
    sample = sk.Sample()
    with pytest.raises(ValueError) as caught:
        sample.moment = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
    assert "timezone" in str(caught.value)
    assert "moment" not in sample


def test_list_conversion_is_not_scalar_conversion():
    prop = sk.Property(compute=lambda: [1.0, 2.0])
    with pytest.raises(TypeError) as caught:
        prop.compute()
    assert "list" in str(caught.value)
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    assert sample.temperatures == [20, 30]


def test_rejected_list_mutation_changes_neither_side():
    sample = sk.Sample()
    sample.temperatures = [20, 30]
    bound = sample.temperatures
    with pytest.raises(ValueError):
        bound[0:1] = ["hot"]
    assert bound == [20, 30]
    assert sample.temperatures == [20, 30]


def test_rust_work_releases_the_gil(project):
    for position in range(1500):
        project.sample(f"s{position:04}.md", properties={"malt": f"{position}.5"})
    ticks = [0]
    stop = threading.Event()

    def spin():
        while not stop.is_set():
            ticks[0] += 1
            time.sleep(0)

    previous = sys.getswitchinterval()
    # Long enough that the spinner only runs when the GIL is released.
    sys.setswitchinterval(30)
    spinner = threading.Thread(target=spin, daemon=True)
    spinner.start()
    try:
        time.sleep(0.05)
        before = ticks[0]
        samples = sk.SampleList(project.root, model=False)
        loading = ticks[0] - before
        before = ticks[0]
        samples.sort("-malt")
        sorting = ticks[0] - before
    finally:
        stop.set()
        spinner.join()
        sys.setswitchinterval(previous)
    assert loading > 0
    assert sorting > 0


def test_error_families_map_to_standard_exceptions(collection, tmp_path):
    sample = sk.Sample()
    with pytest.raises(ValueError):
        sk.Property(float("nan"))
    with pytest.raises(ValueError):
        sample.moment = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
    with pytest.raises(OverflowError):
        sample.count = 2**70
    with pytest.raises(ValueError):
        sk.Property(1.0, uncertainty=-1.0)
    with pytest.raises(ValueError):
        sample["bad name"] = 1
    with pytest.raises(ValueError):
        sample.mixed = [1, "a"]
    with pytest.raises(KeyError):
        sample["nothing"]
    with pytest.raises(ValueError):
        sample["_private"] = 1
    with pytest.raises(ValueError):
        sample["path"] = 1

    table = sk.Table({"T": sk.Column(), "R": sk.Column()}, "T", rows=[{"T": 20, "R": 1.0}])
    with pytest.raises(KeyError):
        table.at(20, "nothing")
    with pytest.raises(KeyError):
        table.at(99, "R")
    with pytest.raises(IndexError):
        table[5]

    cyclic = sk.Sample()
    cyclic.a = sk.Property(compute=lambda: 1.0)
    cyclic.b = sk.Property(compute=lambda: 2.0)
    cyclic.set_dependencies("a", depends_on=["b"])
    with pytest.raises(ValueError):
        cyclic.set_dependencies("b", depends_on=["a"])

    malformed = tmp_path / "malformed.md"
    malformed.write_text("---\nschema_version: 1\nproperties: [1, 2]\n---\n")
    with pytest.raises(ValueError):
        sk.Sample(malformed)

    samples = sk.SampleList(collection.root)
    existing = tmp_path / "out.csv"
    existing.write_text("kept")
    with pytest.raises(FileExistsError):
        samples.to_csv(existing, columns=["name"])
    with pytest.raises(ValueError):
        samples.to_dict(columns=["malt:"])
    with pytest.raises(TypeError):
        samples.filter("beer > 3")

    # A whole list is joined in CSV, no longer refused.
    listing = sk.Sample(name="L")
    listing.temperatures = [20, 30]
    assert sk.SampleList([listing]).to_csv(columns=["temperatures"]) == "temperatures\n20;30\n"




def test_an_assigned_property_object_reaches_the_sample(tmp_path):
    sample = sk.Sample()
    prop = sk.Property(3.2, unit="g")
    sample.malt = prop
    prop.value = 3.3
    path = tmp_path / "s.md"
    sample.save(path)
    assert sample.malt.value == 3.3
    assert "v: 3.3" in path.read_text()


def test_a_value_is_recorded_while_its_inputs_are_the_ones_it_read(tmp_path):
    runs = [0]

    class Dense(sk.Sample):
        def __init__(self, path=None):
            super().__init__(path)
            self.malt = sk.Property(12.0)
            self.volume = sk.Property(4.0)
            self.plato = sk.Property(compute=self._plato)
            self.set_dependencies("plato", depends_on=["malt", "volume"])

        def _plato(self):
            runs[0] += 1
            return self.malt.value / self.volume.value

    sample = Dense()
    sample.compute()
    assert sample.plato.value == 3.0
    sample.malt.value = 16.0
    assert sample.plato.is_outdated
    path = tmp_path / "strong.md"
    sample.save(path)
    assert runs == [1]
    reloaded = sk.Sample(path, model=False)
    assert reloaded.plato.value == 3.0
    assert reloaded.plato.is_outdated


def test_a_formula_reads_its_sample_while_a_computation_resolves_tables(tmp_path):
    class Conducting(sk.Sample):
        def __init__(self, path=None):
            super().__init__(path)
            self.foam = sk.Property(2.0)
            self.mashing = sk.Table(
                {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
                "T",
                rows=[{"T": 65, "R": 10.0}],
                compute_rows=[
                    ("G", ["row.R", "foam"], lambda row: 1 / row.R.value / self.foam.value)
                ],
            )

    sample = Conducting()
    sample.compute()
    sample.save(tmp_path / "conducting.md")
    assert sample.mashing.at(65, "G").value == pytest.approx(0.05)

    class Reaching(sk.Sample):
        def __init__(self, path=None):
            super().__init__(path)
            self.other = sk.Table({"T": sk.Column()}, "T", rows=[{"T": 1}])
            self.mashing = sk.Table(
                {"T": sk.Column(), "G": sk.Column()},
                "T",
                rows=[{"T": 65}],
                compute_rows=[("G", [], lambda row: float(len(self.other)))],
            )

    with pytest.raises(RuntimeError) as caught:
        Reaching().compute()
    assert "other" in str(caught.value)


def test_a_sample_crossing_a_thread_is_refused():
    # Single-threading is a contract; this is what a violation produces:
    # PyO3's PanicException, a BaseException, and the sample is unharmed on its
    # own thread.
    import threading

    sample = sk.Sample(name="t")
    sample.malt = sk.Property(1.0)
    outcome = {}

    def read():
        try:
            outcome["value"] = sample.malt.value
        except BaseException as error:  # noqa: BLE001 - the point is its class
            outcome["error"] = error

    worker = threading.Thread(target=read)
    worker.start()
    worker.join()
    error = outcome["error"]
    assert type(error).__name__ == "PanicException"
    assert not isinstance(error, Exception)
    assert "unsendable" in str(error)
    assert sample.malt.value == 1.0


def test_a_date_python_cannot_hold_is_said(project):
    # The core holds a date of year 0; Python's dates begin in year 1, and its
    # own refusal named no date.
    path = project.sample("y.md", name="Y", attributes={"brewed": "0000-01-01"})
    sample = sk.Sample(path)
    with pytest.raises(ValueError, match="0000-01-01 is a date Python cannot hold"):
        sample.brewed
    assert sample.name == "Y"


def test_a_row_formula_returning_a_pair_is_told_the_form():
    # `(value, uncertainty)` is what a property's joint formula returns, and
    # the natural thing to try in a row: it was refused as "a list is not one
    # cell", with a traceback ending in the package and nothing of the model.
    def drop(row):
        return (row.R.value * 2, 0.1)

    class Pairing(sk.Sample):
        def __init__(self, path=None):
            super().__init__(path)
            self.mashing = sk.Table(
                {"T": sk.Column(), "R": sk.Column(), "G": sk.Column()},
                "T",
                rows=[{"T": 65, "R": 10.0}],
                compute_rows=[("G", ["row.R"], drop)],
            )

    with pytest.raises(TypeError) as caught:
        Pairing().compute()
    said = str(caught.value)
    assert "a row formula returns one value, or sk.Property(value=…, uncertainty=…)" in said
    assert "returned a tuple" in said
    # Named where the model writes it: the function and its file.
    assert "drop" in said and __file__ in said
