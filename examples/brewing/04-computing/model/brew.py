"""A brew: a batch of beer, from the wort in the kettle to the bottle.

Invented for SampleKit's documentation. Each quantity a brewer writes down is
declared here, and what derives from them is computed: the alcohol, how much of
the sugar the yeast ate, how much the mash gave of what the grain held.
"""

import math
import statistics

import samplekit as sk

from hydrometer import POTENTIAL, RESOLUTION, points


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

        # What each formula reads. A value is out of date when one of these
        # changed after it was computed — and only then.
        self.set_dependencies("abv", depends_on=["og", "fg"])
        self.set_dependencies("attenuation", depends_on=["og", "fg"])
        self.set_dependencies("efficiency", depends_on=["og", "volume", "grain_mass"])

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
