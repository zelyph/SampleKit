"""A model: what a sample of this project holds, and what is computed from what.

Save it where `.samplekitrc` names it, `[model] path`, and name its class
there, `[model] class`. Every name marked `← change` is one of your own:
a property, a unit, a formula. The sample template is a sample of this model.
"""

import math

import samplekit as sk


class Specimen(sk.Sample):  # ← your class, as [model] class names it
    """A specimen: what it holds, and in which units."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Measured several times: the value is the mean of the readings, and
        # the uncertainty their standard error.
        self.length = sk.Property(  # ← your measured value
            unit="mm",  # ← its unit
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )
        self.width = sk.Property(
            unit="mm",
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )

        # Computed from others: it says what it reads, so that a reading
        # changed makes it outdated.
        self.area = sk.Property(  # ← your computed value
            unit="mm²",
            compute=self._area,
            depends_on=["length", "width"],
        )

        # A table: its index, a column measured, and a column computed in
        # each row from that row's force and the area.
        self.curve = sk.Table(  # ← your table
            {
                "strain": sk.Property(unit="%"),  # ← its index
                "force": sk.Property(unit="N"),  # ← a column measured
                "stress": sk.Property(  # ← a column computed
                    unit="MPa",
                    compute=self._stress,
                    depends_on=["row.force", "area"],
                ),
            },
            index="strain",
        )

    def _area(self):
        """The length times the width, and its uncertainty from theirs."""
        length = self.length
        width = self.width
        value = length.value * width.value
        uncertainty = math.sqrt(
            (width.value * length.uncertainty) ** 2
            + (length.value * width.uncertainty) ** 2
        )
        return value, uncertainty

    def _stress(self, row):
        """The force over the area: N over mm² is MPa."""
        return row.force.value / self.area.value

    @sk.script
    def classify(self, limit: float = 100.0):  # ← its options, with defaults
        """Class the specimen by its area: large from the limit up."""
        self.size = "large" if self.area.value >= limit else "small"
