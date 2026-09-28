"""A brew: a batch of beer, from the wort in the kettle to the bottle.

Invented for SampleKit's documentation. Each quantity a brewer writes down is
declared here, and what derives from them is computed: the alcohol, how much of
the sugar the yeast ate, how much the mash gave of what the grain held.
"""

import math
import statistics

import samplekit as sk

from hydrometer import POTENTIAL, RESOLUTION, points
# >>> step 5
from hydrometer import linear_fit
# <<<


class Brew(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times with the hydrometer: the value is the mean of the
        # readings and the uncertainty their standard error, so a reading
        # corrected corrects both. `samplekit set FILE og.readings=…` writes
        # them. Without these statistics the readings would have no value:
        # SampleKit takes no mean on its own.
        self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)

        # Written once. The volume is read on the fermenter's scale, marked
        # every litre: its uncertainty is a formula of that scale alone, and
        # `depends_on=[]` says it reads nothing of the brew.
        self.volume = sk.Property(unit="L", compute_uncertainty=self._scale, depends_on=[])
        self.grain_mass = sk.Property(unit="kg")
        self.mash_temperature = sk.Property(unit="degC")

        # Computed from them.
        self.abv = sk.Property(unit="%", compute_quantity=self._abv)
        self.attenuation = sk.Property(unit="%", compute=self._attenuation)
        self.efficiency = sk.Property(unit="%", compute=self._efficiency)
# >>> step 5

        # The fermentation, a row a day. `compute_rows` fills `apparent` in
        # each row, from that row's gravity and the brew's original gravity.
        # `compute_columns` fills `rate` once for the whole column, since each
        # row's rate needs the row before it.
        self.fermentation = sk.Table(
            {
                "day": sk.Column(unit="d"),
                "gravity": sk.Column(),
                "temperature": sk.Column(unit="degC"),
                "apparent": sk.Column(unit="%"),
                "rate": sk.Column(unit="pt/d"),
            },
            "day",
            title="Gravity and temperature, day by day",
            compute_rows=[(["apparent"], ["row.gravity", "og"], self._apparent)],
            compute_columns=[
                ("rate", ["fermentation.day", "fermentation.gravity", "og"], self._rate)
            ],
        )
        # The tasting, a row per taster: written, nothing computed in it.
        self.tasting = sk.Table(
            {"taster": sk.Column(), "score": sk.Column(), "bitterness": sk.Column()},
            "taster",
            title="Scores out of 50, bitterness from 1 to 5",
        )

        # Read over a whole table: how fast the gravity fell in the first
        # three days, and the tasters' mean score.
        self.drop = sk.Property(unit="pt/d", compute_quantity=self._drop)
        self.score = sk.Property(compute_quantity=self._score)
# <<<

        # What each formula reads. A value is out of date when one of these
        # changed after it was computed — and only then.
        self.set_dependencies("abv", depends_on=["og", "fg"])
        self.set_dependencies("attenuation", depends_on=["og", "fg"])
        self.set_dependencies("efficiency", depends_on=["og", "volume", "grain_mass"])
# >>> step 5
        self.set_dependencies(
            "drop", depends_on=["og", "fermentation.day", "fermentation.gravity"]
        )
        self.set_dependencies("score", depends_on=["tasting.score"])
# <<<

    # ------------------------------------------------------------ formulas

    def _scale(self):
        # Marked every litre, read to the nearest mark.
        return 1.0 / math.sqrt(12)

    def _abv(self):
        # The usual home-brewing rule: 131.25 per unit of gravity lost. Its
        # uncertainty combines the two gravities' own.
        value = (self.og.value - self.fg.value) * 131.25
        og = math.hypot(self.og.uncertainty or 0.0, RESOLUTION)
        fg = math.hypot(self.fg.uncertainty or 0.0, RESOLUTION)
        return value, math.hypot(og, fg) * 131.25

    def _attenuation(self):
        return 100.0 * (self.og.value - self.fg.value) / (self.og.value - 1.0)

    def _efficiency(self):
        extracted = points(self.og.value) * self.volume.value
        return 100.0 * extracted / (self.grain_mass.value * POTENTIAL)
# >>> step 5

    def _apparent(self, row):
        # One run per row: `row` is that row, `self` the brew.
        return {"apparent": 100.0 * (self.og.value - row.gravity.value) / (self.og.value - 1.0)}

    def _rate(self, columns):
        # One run for the whole column: `columns` holds this table's inputs.
        # Each row's rate is measured from the row before it, the first from
        # brew day, when the gravity was the original gravity.
        days = [0] + columns["day"].values
        gravity = [self.og.value] + columns["gravity"].values
        return [
            (points(gravity[i - 1]) - points(gravity[i])) / (days[i] - days[i - 1])
            for i in range(1, len(days))
        ]

    def _drop(self):
        # Brew day, day 0, is the original gravity; then the table's rows.
        days = self.fermentation.values("day")
        gravity = self.fermentation.values("gravity")
        early = [(0, points(self.og.value))]
        early += [(d, points(g)) for d, g in zip(days, gravity) if d <= 3]
        slope, _, standard_error = linear_fit([d for d, _ in early], [p for _, p in early])
        return -slope, standard_error

    def _score(self):
        scores = self.tasting.values("score")
        spread = statistics.stdev(scores) / math.sqrt(len(scores)) if len(scores) > 1 else None
        return statistics.fmean(scores), spread
# <<<
# >>> step 6

    # ------------------------------------------------------------- figures
    # `samplekit plot attenuation_curve brews/citra-ipa.md` draws one brew.

    @sk.figure
    def attenuation_curve(self, ax):
        """How much of the sugar was eaten, day by day, against where it ended."""
        ax.plot(self.fermentation.values("day"), self.fermentation.values("apparent"), "o-")
        if self.attenuation.value is not None:
            ax.axhline(self.attenuation.value, color="0.5", linestyle="--")
        ax.set_xlabel("day")
        ax.set_ylabel("apparent attenuation [%]")
# <<<
