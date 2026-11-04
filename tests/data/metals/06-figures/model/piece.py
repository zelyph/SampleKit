"""A piece: a batch of alloy, from the tension in the crucible to the mould.

Invented for SampleKit's documentation. Each quantity a tester writes down is
declared here, and what derives from them is computed: the modulus, how much of
the sugar the process ate, how much the preheat gave of what the ore held.
"""

import math
import statistics

import samplekit as sk

from durometer import POTENTIAL, RESOLUTION, points
from durometer import linear_fit


class Piece(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times with the durometer: the value is the mean of the
        # readings and the uncertainty their standard error, so a reading
        # corrected corrects both. `samplekit FILE set radius.readings=…` writes
        # them. Without these statistics the readings would have no value:
        # SampleKit takes no mean on its own.
        self.radius = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.notch = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)

        # Written once. The volume is read on the furnace's scale, marked
        # every litre: its uncertainty is a formula of that scale alone, and
        # `depends_on=[]` says it reads nothing of the piece.
        self.volume = sk.Property(unit="L", compute_uncertainty=self._scale, depends_on=[])
        self.ore_mass = sk.Property(unit="kg")
        self.preheat_temperature = sk.Property(unit="degC")

        # Computed from them.
        self.modulus = sk.Property(unit="%", compute=self._modulus)
        self.reduction = sk.Property(unit="%", compute=self._reduction)
        self.recovery = sk.Property(unit="%", compute=self._recovery)

        # The ageing, a row a day. Each column is declared as a property
        # is: `nominal` is computed in each row, from that row's hardness and
        # the piece's original hardness; `compute_column` fills `rate` once for
        # the whole column, since each row's rate needs the row before it.
        self.ageing = sk.Table(
            {
                "day": sk.Property(unit="d"),
                "hardness": sk.Property(),
                "temperature": sk.Property(unit="degC"),
                "nominal": sk.Property(
                    unit="%", compute=self._nominal, depends_on=["row.hardness", "radius"]
                ),
                "rate": sk.Property(
                    unit="pt/d",
                    compute_column=self._rate,
                    depends_on=["ageing.day", "ageing.hardness", "radius"],
                ),
            },
            index="day",
            title="Hardness and temperature, day by day",
        )
        # The inspection, a row per inspector: written, nothing computed in it.
        self.inspection = sk.Table(
            {"inspector": sk.Property(), "score": sk.Property(), "brittleness": sk.Property()},
            index="inspector",
            title="Scores out of 50, brittleness from 1 to 5",
        )

        # Read over a whole table: how fast the hardness fell in the first
        # three days, and the inspectors' mean score.
        self.drop = sk.Property(unit="pt/d", compute=self._drop)
        self.score = sk.Property(compute=self._score)

        # What each formula reads. A value is out of date when one of these
        # changed after it was computed — and only then.
        self.set_dependencies("modulus", depends_on=["radius", "notch"])
        self.set_dependencies("reduction", depends_on=["radius", "notch"])
        self.set_dependencies("recovery", depends_on=["radius", "volume", "ore_mass"])
        self.set_dependencies(
            "drop", depends_on=["radius", "ageing.day", "ageing.hardness"]
        )
        self.set_dependencies("score", depends_on=["inspection.score"])

    # ------------------------------------------------------------ formulas

    def _scale(self):
        # Marked every litre, read to the nearest mark.
        return 1.0 / math.sqrt(12)

    def _modulus(self):
        # The usual home-metals rule: 131.25 per unit of hardness lost. Its
        # uncertainty combines the two hardnesses' own.
        value = (self.radius.value - self.notch.value) * 131.25
        radius = math.hypot(self.radius.uncertainty or 0.0, RESOLUTION)
        notch = math.hypot(self.notch.uncertainty or 0.0, RESOLUTION)
        return value, math.hypot(radius, notch) * 131.25

    def _reduction(self):
        return 100.0 * (self.radius.value - self.notch.value) / (self.radius.value - 1.0)

    def _recovery(self):
        extracted = points(self.radius.value) * self.volume.value
        return 100.0 * extracted / (self.ore_mass.value * POTENTIAL)

    def _nominal(self, row):
        # One run per row: `row` is that row, `self` the piece.
        return 100.0 * (self.radius.value - row.hardness.value) / (self.radius.value - 1.0)

    def _rate(self, columns):
        # One run for the whole column: `columns` holds this table's inputs.
        # Each row's rate is measured from the row before it, the first from
        # piece day, when the hardness was the original hardness.
        days = [0] + columns["day"].values
        hardness = [self.radius.value] + columns["hardness"].values
        return [
            (points(hardness[i - 1]) - points(hardness[i])) / (days[i] - days[i - 1])
            for i in range(1, len(days))
        ]

    def _drop(self):
        # Piece day, day 0, is the original hardness; then the table's rows.
        days = self.ageing.values("day")
        hardness = self.ageing.values("hardness")
        early = [(0, points(self.radius.value))]
        early += [(d, points(g)) for d, g in zip(days, hardness) if d <= 3]
        slope, _, standard_error = linear_fit([d for d, _ in early], [p for _, p in early])
        return -slope, standard_error

    def _score(self):
        scores = self.inspection.values("score")
        spread = statistics.stdev(scores) / math.sqrt(len(scores)) if len(scores) > 1 else None
        return statistics.fmean(scores), spread

    # ------------------------------------------------------------- figures
    # `samplekit pieces/alpha-titanium.md plot reduction_curve` draws one piece.

    @sk.figure
    def reduction_curve(self, ax):
        """How much of the sugar was eaten, day by day, against where it ended."""
        ax.plot(self.ageing.values("day"), self.ageing.values("nominal"), "o-")
        if self.reduction.value is not None:
            ax.axhline(self.reduction.value, color="0.5", linestyle="--")
        ax.set_xlabel("day")
        ax.set_ylabel("nominal reduction [%]")
