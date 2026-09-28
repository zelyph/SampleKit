"""Realistic usage, type-checked against the stub by test_type_stubs."""

from pathlib import Path

from samplekit import Column, Property, Sample, SampleList, Table


class Brew(Sample):
    def __init__(self, path: str | Path | None = None, name: str | None = None) -> None:
        super().__init__(path, name=name)
        self.malt = Property(unit="g")
        self.volume = Property(unit="L")
        self.plato = Property(compute=self._plato, unit="g/L")
        self.set_dependencies("plato", depends_on=["malt", "volume"])
        self.mashing = Table(
            {"T": Column(unit="degC"), "R": Column(unit="lintner"), "G": Column(unit="NTU")},
            "T",
            compute_rows=[("G", ["row.R"], lambda row: 1 / float(row.R))],
        )

    def _plato(self) -> float:
        return float(self.malt) / float(self.volume)


def report(directory: Path) -> str:
    samples = SampleList(directory, model=Brew)
    heavy = samples.filter("malt > 12").sorted("-malt")
    first: Sample = heavy[0]
    rest: SampleList = heavy[1:]
    named: Sample = samples["C42"]
    summary = samples.stats("malt")
    lines = [f"{first.name}: {first.malt:.2f}", f"{len(rest)} more", str(named.path)]
    lines.append(f"mean {summary.mean:.3f} over {summary.count}")
    table = samples.to_dict(columns=["name", "malt"])
    lines.append(", ".join(table))
    csv = samples.to_csv(profile=samples.project.profiles.platos)
    return "\n".join(lines) + (csv or "")
