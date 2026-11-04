"""The changes of the guide to editing samples, from Python."""

import samplekit as sk

steel = sk.Sample("samples/steel-01.md")
steel.grade = "S355J2"
steel.thickness.readings = [1.991, 1.994, 1.989]
steel.tags.append("reference")
print(steel.not_current())
steel.save()

aluminium = sk.Sample("samples/aluminium-01.md")
aluminium.curve.add(strain=11, force=7.8)
aluminium.compute()
print(aluminium.elongation, aluminium.tensile_strength)
aluminium.save()
