import math
import samplekit as sk


class Specimen(sk.Sample):
    """A tensile specimen: what it holds, and in which units."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Measured three times with a micrometer: the value is the mean of the
        # readings, and the uncertainty their standard error.
        self.width = sk.Property(
            unit="mm",
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )
        self.thickness = sk.Property(
            unit="mm",
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )

        # The cross-section, computed from the width and the thickness.
        self.area = sk.Property(
            unit="mm²",
            compute=self._area,
            depends_on=["width", "thickness"],
        )

        # The test: the force read at each strain, and the stress computed
        # in each row from that row's force and the cross-section.
        self.curve = sk.Table(
            {
                "strain": sk.Property(unit="%"),
                "force": sk.Property(unit="kN"),
                "stress": sk.Property(
                    unit="MPa",
                    compute=self._stress,
                    depends_on=["row.force", "area"],
                ),
            },
            index="strain",
        )

    def _area(self):
        """The width times the thickness, and its uncertainty from theirs."""
        w = self.width
        t = self.thickness
        value = w.value * t.value
        # A size typed without its readings has no uncertainty: nor has the area.
        if w.uncertainty is None or t.uncertainty is None:
            return value
        uncertainty = math.sqrt(
            (t.value * w.uncertainty) ** 2
            + (w.value * t.uncertainty) ** 2
        )
        return value, uncertainty

    def _stress(self, row):
        """The force over the cross-section: kN over mm², times 1000, is MPa."""
        return 1000 * row.force.value / self.area.value
