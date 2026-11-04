"""How the files of coupons/ were made: by SampleKit 0.2.0 itself, its package
taken from the tag v0.2.0 and run with PyYAML, `python make.py coupons`."""
import datetime
import sys
from pathlib import Path

sys.path.insert(0, "lab")
from coupon import Coupon

out = Path(sys.argv[1])
for name, material, received, polished, thickness, indents in [
    ("brass-01", "brass", datetime.date(2025, 5, 12), True, [3.002, 2.998, 3.001],
     [(10, 0.4190, 0.4210), (20, 0.5930, 0.5950)]),
    ("steel-01", "steel", datetime.date(2025, 6, 2), False, [2.501, 2.499],
     [(10, 0.3040, 0.3050), (20, 0.4300, 0.4320)]),
]:
    c = Coupon(name=name)
    c.material.value = material
    c.received.value = received
    c.polished.value = polished
    c.thickness.value = thickness
    for load, d1, d2 in indents:
        c.indents.add(load=load, d1=d1, d2=d2)
    c.indents._run_column_compute(c.indents.index_values)
    c.indents._run_row_compute(c.indents.index_values)
    folder = out / received.strftime("%Y-%m")
    c.save(folder / f"{name}.md")
