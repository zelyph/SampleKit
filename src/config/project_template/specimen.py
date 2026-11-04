"""What a specimen of this project has, and what is derived from what.

The example is a tensile test, as in SampleKit's demo: a strip of metal pulled
until it breaks, the force read as it stretches. One of each kind of formula,
so that this file can be copied from:

  * a property read several times, whose value and uncertainty are
    statistics of those readings              (width, thickness)
  * an uncertainty computed beside a value    (gauge_length)
  * a value and its uncertainty from one call (area)
  * a table filled row by row                 (curve.strain, curve.stress)
  * a column filled once, whole               (curve.energy)
  * a value read over a whole table           (young_modulus, tensile_strength)
  * a value computed from a computed value    (elongation, from curve.strain)

**A formula must declare what it reads**, in `depends_on=` or with
`set_dependencies` at the end of `__init__`. SampleKit compares those inputs
against what it recorded, and that is how it knows a value is not current. A
formula that reads what it did not declare computes a number nothing will ever
mark outdated.
"""

import math

import samplekit as sk

from helpers import CALLIPER, linear_fit


class Specimen(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times with a micrometer: the value is the mean of the
        # readings and the uncertainty their standard error, so correcting a
        # reading corrects both. A file holds `readings: [...]`, and
        # `samplekit FILE set width.readings=12.51,12.52,12.51` writes them.
        self.width = sk.Property(
            unit="mm", value=sk.stats.mean, uncertainty=sk.stats.standard_error
        )
        self.thickness = sk.Property(
            unit="mm", value=sk.stats.mean, uncertainty=sk.stats.standard_error
        )

        # Typed once, as marked on the specimen; its uncertainty is the
        # calliper's, a formula rather than a number typed in each file.
        # `depends_on=[]` says the formula reads nothing of the specimen: a
        # computed value that declares neither its inputs nor this is refused.
        self.gauge_length = sk.Property(
            unit="mm", compute_uncertainty=self._calliper, depends_on=[]
        )

        # The cross-section: one call gives its value and its uncertainty.
        self.area = sk.Property(unit="mm²", compute=self._area)

        # A table: its columns are declared here, its rows live in each file.
        # A column computed row by row runs its formula once per row, reading
        # that row through `row` and the specimen through `self`;
        # `compute_column` runs once for the whole column, since each row's
        # energy adds to the row before it.
        self.curve = sk.Table(
            {
                "extension": sk.Property(unit="mm"),
                "force": sk.Property(unit="kN"),
                "strain": sk.Property(
                    unit="%",
                    compute=self._strain,
                    depends_on=["row.extension", "gauge_length"],
                ),
                "stress": sk.Property(
                    unit="MPa", compute=self._stress, depends_on=["row.force", "area"]
                ),
                "energy": sk.Property(
                    unit="J",
                    compute_column=self._energy,
                    depends_on=["curve.extension", "curve.force"],
                ),
            },
            index="extension",
            title="The force read as the specimen stretches",
        )

        # Read over the whole table.
        self.young_modulus = sk.Property(unit="GPa", compute=self._young_modulus)
        self.tensile_strength = sk.Property(unit="MPa", compute=self._tensile_strength)
        # Read from a computed column: outdated whenever it is.
        self.elongation = sk.Property(unit="%", compute=self._elongation)

        # What each formula reads. Miss one and the value it gives will never
        # be reported outdated when that input changes.
        self.set_dependencies("area", depends_on=["width", "thickness"])
        self.set_dependencies("young_modulus", depends_on=["curve.strain", "curve.stress"])
        self.set_dependencies("tensile_strength", depends_on=["curve.stress"])
        self.set_dependencies("elongation", depends_on=["curve.strain"])

    # ------------------------------------------------------------- formulas

    def _calliper(self):
        # A type B uncertainty is a formula like any other.
        return CALLIPER

    def _area(self):
        # The width times the thickness, and its uncertainty from theirs. A
        # size typed without its readings has no uncertainty: nor has the area.
        w, t = self.width, self.thickness
        value = w.value * t.value
        if w.uncertainty is None or t.uncertainty is None:
            return value
        return value, math.hypot(t.value * w.uncertainty, w.value * t.uncertainty)

    def _strain(self, row):
        # One run per row: `row` is that row, `self` the specimen.
        return 100 * row.extension.value / self.gauge_length.value

    def _stress(self, row):
        # kN over mm², times 1000, is MPa.
        return 1000 * row.force.value / self.area.value

    def _energy(self, columns):
        # One run for the whole column: `columns` holds the table's inputs.
        # The work done up to each row, the area under the force: kN × mm is J.
        extension = columns["extension"].values
        force = columns["force"].values
        energy = [0.0]
        for i in range(1, len(extension)):
            step = (force[i] + force[i - 1]) / 2 * (extension[i] - extension[i - 1])
            energy.append(energy[-1] + step)
        return energy

    def _young_modulus(self):
        # The slope of the rows up to 0.05 % of strain, and its standard
        # error: MPa per % of strain is 0.1 GPa.
        rows = [
            (strain, stress)
            for strain, stress in zip(self.curve.values("strain"), self.curve.values("stress"))
            if strain <= 0.05
        ]
        slope, _, error = linear_fit([e for e, _ in rows], [s for _, s in rows])
        if error is None:
            return slope / 10
        return slope / 10, error / 10

    def _tensile_strength(self):
        return max(self.curve.values("stress"))

    def _elongation(self):
        # The strain at the break: the last row.
        return self.curve.values("strain")[-1]

    # -------------------------------------------------------------- figures

    @sk.figure
    def stress_strain(self, ax):
        """The specimen's curve: stress against strain."""
        ax.plot(self.curve.values("strain"), self.curve.values("stress"), "o-")
        ax.set_xlabel("strain [%]")
        ax.set_ylabel("stress [MPa]")

    # -------------------------------------------------------------- scripts

    @sk.script
    def classify(self, limit: float = 20.0):
        """Class the specimen's ductility by its elongation at break."""
        self.ductility = "high" if self.elongation.value >= limit else "low"
