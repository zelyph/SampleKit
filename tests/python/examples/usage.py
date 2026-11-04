"""Realistic usage, type-checked against the stub by test_type_stubs."""

from pathlib import Path

from samplekit import Property, Sample, SampleList, Table


class Piece(Sample):
    def __init__(self, path: str | Path | None = None, name: str | None = None) -> None:
        super().__init__(path, name=name)
        self.diameter = Property(unit="g")
        self.volume = Property(unit="L")
        self.ductility = Property(compute_value=self._ductility, unit="g/L")
        self.set_dependencies("ductility", depends_on=["diameter", "volume"])
        self.preheating = Table(
            {
                "T": Property(unit="degC"),
                "R": Property(unit="lintner"),
                "G": Property(unit="NTU", compute=lambda row: 1 / float(row.R), depends_on=["row.R"]),
            },
            index="T",
        )

    def _ductility(self) -> float:
        return float(self.diameter) / float(self.volume)


def report(directory: Path) -> str:
    samples = SampleList(directory, model=Piece)
    heavy = samples.filter("diameter > 12").sorted("-diameter")
    first: Sample = heavy[0]
    rest: SampleList = heavy[1:]
    named: Sample = samples["C42"]
    summary = samples.stats("diameter")
    lines = [f"{first.name}: {first.diameter:.2f}", f"{len(rest)} more", str(named.path)]
    lines.append(f"mean {summary.mean:.3f} over {summary.count}")
    table = samples.to_dict(columns=["name", "diameter"])
    lines.append(", ".join(table))
    csv = samples.to_csv(profile=samples.project.profiles.ductilities)
    return "\n".join(lines) + (csv or "")
