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
            compute_quantity=self._area,
            depends_on=["width", "thickness"],
        )

        # The test: the force read at each strain, and the stress computed
        # in each row from that row's force and the cross-section.
        self.curve = sk.Table(
            {
                "strain": sk.Column(unit="%"),
                "force": sk.Column(unit="kN"),
                "stress": sk.Column(unit="MPa"),
            },
            "strain",
            compute_rows=[("stress", ["row.force", "area"], self._stress)],
        )

        # What the curve gives.
        self.young_modulus = sk.Property(
            unit="GPa",
            compute_quantity=self._young_modulus,
            depends_on=["curve.strain", "curve.stress"],
        )
        self.yield_strength = sk.Property(
            unit="MPa",
            compute=self._yield_strength,
            depends_on=["curve.strain", "curve.stress", "young_modulus"],
        )
        self.tensile_strength = sk.Property(
            unit="MPa",
            compute=self._tensile_strength,
            depends_on=["curve.stress"],
        )
        self.elongation = sk.Property(
            unit="%",
            compute=self._elongation,
            depends_on=["curve.strain"],
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

    def _young_modulus(self):
        """The slope of a straight line fitted to the rows up to 0.05 %, and
        the uncertainty of that slope."""
        points = [
            (strain, stress)
            for strain, stress in zip(self.curve.values("strain"), self.curve.values("stress"))
            if strain <= 0.05
        ]
        n = len(points)
        mean_strain = sum(strain for strain, _ in points) / n
        mean_stress = sum(stress for _, stress in points) / n
        spread = sum((strain - mean_strain) ** 2 for strain, _ in points)
        slope = sum(
            (strain - mean_strain) * (stress - mean_stress) for strain, stress in points
        ) / spread
        residuals = [
            stress - mean_stress - slope * (strain - mean_strain) for strain, stress in points
        ]
        uncertainty = math.sqrt(sum(r * r for r in residuals) / (n - 2) / spread)
        # MPa per % of strain, to GPa: times 100, then divided by 1000.
        return slope / 10, uncertainty / 10

    def _yield_strength(self):
        """Where the curve crosses the straight line of slope E, moved 0.2 %
        to the right: interpolated between the two rows on either side."""
        slope = self.young_modulus.value * 10  # GPa to MPa per %
        rows = list(zip(self.curve.values("strain"), self.curve.values("stress")))
        for (e0, s0), (e1, s1) in zip(rows, rows[1:]):
            above0 = s0 - slope * (e0 - 0.2)
            above1 = s1 - slope * (e1 - 0.2)
            if above0 > 0 >= above1:
                return s0 + above0 / (above0 - above1) * (s1 - s0)
        return None

    def _tensile_strength(self):
        """The highest stress."""
        return max(self.curve.values("stress"))

    def _elongation(self):
        """The strain at the break: the last row."""
        return self.curve.values("strain")[-1]
