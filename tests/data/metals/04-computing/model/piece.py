"""A piece: a batch of alloy, from the tension in the crucible to the mould.

Invented for SampleKit's documentation. Each quantity a tester writes down is
declared here, and what derives from them is computed: the modulus, how much of
the sugar the process ate, how much the preheat gave of what the ore held.
"""

import math
import statistics

import samplekit as sk

from durometer import POTENTIAL, RESOLUTION, points


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

        # What each formula reads. A value is out of date when one of these
        # changed after it was computed — and only then.
        self.set_dependencies("modulus", depends_on=["radius", "notch"])
        self.set_dependencies("reduction", depends_on=["radius", "notch"])
        self.set_dependencies("recovery", depends_on=["radius", "volume", "ore_mass"])

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
