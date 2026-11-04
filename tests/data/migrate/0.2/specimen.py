"""A tensile specimen, as SampleKit 0.2 declared it."""
import math

from samplekit import Sample, Property, Table, Column, report


class Specimen(Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name)
        self.material = Property(value="steel")
        self.operator = Property()
        self.width = Property(unit="mm", symbol="w", precision=".3f")
        self.thickness = Property(unit="mm", symbol="t", precision=".3f", precision_unc=".4f")
        self.gauge_length = Property(value=50.0, uncertainty=0.1, unit="mm", symbol="L_0")
        self.area = Property(
            unit="mm²", unit_math="mm^2", symbol="A",
            compute=self._area, compute_unc=self._area_unc,
            depends_on=[self.width, self.thickness],
        )
        self.tensile_strength = Property(
            unit="MPa", symbol_math=r"\sigma_{UTS}", precision=".0f",
            compute=self._tensile_strength,
        )
        self.curve = Table(
            title="Tensile curve",
            columns={
                "strain": Column(unit="%"),
                "force": Column(unit="kN", precision=".3f"),
                "stress": Column(unit="MPa", symbol_math=r"\sigma", precision=".1f"),
            },
            compute_row={"stress": (self._stress, ["force"])},
        )

    def _area(self):
        return self.width.value * self.thickness.value

    def _area_unc(self):
        w, t = self.width, self.thickness
        return math.hypot(t.value * w.uncertainty, w.value * t.uncertainty)

    def _tensile_strength(self):
        return max(self.curve.stress.values)

    def _stress(self, force):
        return force * 1000 / self.area.value

    def template(self, style="math"):
        parts = [report.heading(f"Specimen {self.name}")]
        parts.append(report.properties_table(
            self, ["material", "width", "thickness", "area", "tensile_strength"], style=style))
        return "\n\n".join(parts)
