"""How the files of samples/ were made: by SampleKit 0.2.0 itself, its package
taken from the tag v0.2.0 and run with PyYAML, `python make.py samples`.

Run with 0.1.0's package and 0.1/specimen.py, the files are the same."""
import sys
from pathlib import Path
sys.path.insert(0, ".")
from specimen import Specimen

out = Path(sys.argv[1]); out.mkdir(exist_ok=True)
specimens = {
    "steel-01": ("steel", "ana", [12.49, 12.51, 12.50], [2.004, 1.996, 2.001],
                 [(0.0, 0.0), (0.2, 9.512), (0.5, 11.204), (2.0, 12.930)]),
    "aluminium-01": ("aluminium", "tom", [12.52, 12.50, 12.49], [2.010, 2.006, 2.002],
                     [(0.0, 0.0), (0.2, 5.531), (0.5, 6.420), (2.0, 7.968)]),
}
for name, (material, operator, width, thickness, curve) in specimens.items():
    s = Specimen(name=name)
    s.material.value = material
    s.operator.value = operator
    s.width.value = width
    s.thickness.value = thickness
    for strain, force in curve:
        s.curve.add(strain=strain, force=force)
    s.curve._run_row_compute(s.curve.index_values)
    s.save(out / f"{name}.md")
# One the operator typed a value into by hand, a statistic set aside.
s = Specimen(name="copper-01")
s.material.value = "copper"
s.width.value = 12.47
s.width.uncertainty = 0.02
s.thickness.value = [2.003, 1.999]
s.thickness.uncertainty = 0.003
for strain, force in [(0.0, 0.0), (0.5, 4.810), (2.0, 5.522)]:
    s.curve.add(strain=strain, force=force)
s.curve._run_row_compute(s.curve.index_values)
s.save(out / "copper-01.md")
