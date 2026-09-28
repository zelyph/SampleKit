"""What a brew of this project has, and what is derived from what.

The example is a home brewer's notebook, as in SampleKit's demo. One of each
kind of formula, so that this file can be copied from:

  * a quantity read several times, whose value and uncertainty are
    statistics of those readings            (og, fg)
  * an uncertainty computed beside a value  (volume)
  * a value computed from other values      (attenuation, efficiency)
  * a value and its uncertainty from one call  (abv)
  * a value computed from a computed value  (alcohol, from abv)
  * a table filled row by row               (fermentation.apparent)
  * a column filled once, whole             (fermentation.rate)
  * a value read over a whole table         (drop)

**A formula must declare what it reads**, with `set_dependencies` at the end of
`__init__`. SampleKit compares those inputs against what it recorded, and that
is how it knows a value is not current. A formula that reads what it did not
declare computes a number nothing will ever mark outdated.
"""

import math

import samplekit as sk

from helpers import POTENTIAL, RESOLUTION, linear_fit, points


class Brew(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Read three times with the hydrometer: the value is the mean of the
        # readings and the uncertainty their standard error, so correcting a
        # reading corrects both. A file supplies `readings: [...]`, and
        # `samplekit set <file> og.readings=1.052,1.053,1.052` writes them.
        self.og = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)
        self.fg = sk.Property(value=sk.stats.mean, uncertainty=sk.stats.standard_error)

        # Read once, on the fermenter's scale, marked every litre: the
        # uncertainty is a formula of that scale rather than a number typed in
        # each file.
        #
        # `depends_on=[]` is how a formula says it reads nothing of the brew —
        # this one only knows the scale. A computed value that declares
        # neither its inputs nor this is refused, because a formula reading
        # something it never declared produces a number nothing will ever mark
        # outdated.
        self.volume = sk.Property(unit="L", compute_uncertainty=self._scale, depends_on=[])
        # Typed in each file, as it was weighed.
        self.grain_mass = sk.Property(unit="kg")

        # Derived from the measured values.
        self.attenuation = sk.Property(unit="%", compute=self._attenuation)
        self.efficiency = sk.Property(unit="%", compute=self._efficiency)
        # One call gives both the value and its uncertainty.
        self.abv = sk.Property(unit="%", compute_quantity=self._abv)
        # Derived from a derived value: outdated whenever abv is.
        self.alcohol = sk.Property(unit="g", compute_quantity=self._alcohol)

        # A table: its columns are declared here, its rows live in each file.
        # `compute_rows` runs once per row and fills the columns it names; it
        # reads that row through `row`, and the brew through `self`.
        # `compute_columns` runs once for the whole column, since each row's
        # rate needs the row before it.
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
            compute_rows=[
                (
                    ["apparent"],                 # the columns it fills
                    ["row.gravity", "og"],        # what it reads
                    self._apparent,               # the function
                )
            ],
            compute_columns=[
                ("rate", ["fermentation.day", "fermentation.gravity", "og"], self._rate)
            ],
        )

        # Read over a whole table: how fast the gravity fell in the first
        # three days.
        self.drop = sk.Property(unit="pt/d", compute_quantity=self._drop)

        # What each formula reads. Miss one and the value it gives will never be
        # reported outdated when that input changes.
        self.set_dependencies("attenuation", depends_on=["og", "fg"])
        self.set_dependencies("efficiency", depends_on=["og", "volume", "grain_mass"])
        self.set_dependencies("abv", depends_on=["og", "fg"])
        self.set_dependencies("alcohol", depends_on=["abv", "volume"])
        self.set_dependencies(
            "drop", depends_on=["og", "fermentation.day", "fermentation.gravity"]
        )

    # ------------------------------------------------------------- formulas

    def _scale(self):
        # Marked every litre, read to the nearest mark: the standard
        # uncertainty of a rectangular distribution one litre wide. A type B
        # uncertainty is a formula like any other.
        return 1.0 / math.sqrt(12)

    def _attenuation(self):
        return 100.0 * (self.og.value - self.fg.value) / (self.og.value - 1.0)

    def _efficiency(self):
        extracted = points(self.og.value) * self.volume.value
        return 100.0 * extracted / (self.grain_mass.value * POTENTIAL)

    def _abv(self):
        # The usual home-brewing rule: 131.25 per unit of gravity lost. Its
        # uncertainty combines the two gravities' own with the hydrometer's.
        value = (self.og.value - self.fg.value) * 131.25
        og = math.hypot(self.og.uncertainty or 0.0, RESOLUTION)
        fg = math.hypot(self.fg.uncertainty or 0.0, RESOLUTION)
        return value, math.hypot(og, fg) * 131.25

    def _alcohol(self):
        # Grams of alcohol in the batch: ethanol weighs 0.789 g per mL.
        grams = 7.89 * self.abv.value * self.volume.value
        relative = math.hypot(
            (self.abv.uncertainty or 0.0) / self.abv.value,
            (self.volume.uncertainty or 0.0) / self.volume.value,
        )
        return grams, grams * relative

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
