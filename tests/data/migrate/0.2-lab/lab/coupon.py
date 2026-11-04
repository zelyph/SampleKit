"""Hardness coupons, as SampleKit 0.2 declared them: a base class of every
specimen of the lab, and a class for coupons deriving from it."""
import datetime

import samplekit
from samplekit import Column, report

G = 9.80665
REFERENCE = {"brass": 120.0, "steel": 200.0}


def hv_from_diagonals(load, d1, d2):
    """Vickers hardness from the load in kgf and the diagonals in mm."""
    d = (d1 + d2) / 2
    return 1.8544 * load / d ** 2


class Specimen(samplekit.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name)
        self.material = samplekit.Property("steel")
        self.received = samplekit.Property()
        self.polished = samplekit.Property(False)
        self.thickness = samplekit.Property([], None, "mm", precision=".3f")


class Coupon(Specimen):
    def __init__(self, path=None, name=None):
        super().__init__(path, name)
        self.indents = samplekit.Table(
            columns={
                "load": Column("kgf"),
                "d1": Column("mm", precision=".4f"),
                "d2": Column("mm", precision=".4f"),
                "load_N": Column("N"),
                "hv": Column("HV", precision=".0f"),
            },
            compute={"load_N": lambda loads: [l * G for l in loads]},
            compute_unc={"load_N": lambda loads: [0.01 * l * G for l in loads]},
            compute_row={"hv": (hv_from_diagonals, ["load", "d1", "d2"])},
            title="Indents",
        )
        self.hardness = samplekit.Property(unit="HV", compute=self._hardness)
        self.ratio = samplekit.Property(
            compute=lambda: self.hardness.value / REFERENCE[self.material.value],
            depends_on=[self.hardness],
        )

    def _hardness(self):
        return sum(self.indents.hv.values) / len(self.indents.hv.values)

    def template(self, style="math"):
        return report.heading(f"Coupon {self.name}")
