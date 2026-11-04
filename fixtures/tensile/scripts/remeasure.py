import samplekit as sk

steel = sk.Sample("samples/steel-01.md")
steel.width.readings = [12.49, 12.53, 12.52]
print(steel.not_current())
steel.compute()
print(steel.area, steel.tensile_strength)
steel.save()
