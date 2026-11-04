"""One specimen and the collection, read as the command line reads them."""

import samplekit as sk

steel = sk.Sample("samples/steel-01.md")
print(steel.name, steel.material)
print(steel.area)
print(steel.area.value, steel.area.uncertainty, steel.area.unit)
print(steel.thickness.readings, steel.thickness.stats.mean)
print(steel["curve.stress[10]"])
print(steel.curve.values("strain")[:4])

specimens = sk.load("samples")
ductile = specimens.query("ductile").sorted("-elongation")
print([specimen.name for specimen in ductile])
print(specimens.stats("elongation").mean)
print(specimens.to_dict(columns=["name", "tensile_strength"]))
