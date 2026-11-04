"""The questions of the guide to selecting samples, from Python."""

import samplekit as sk

specimens = sk.load("samples")
print([specimen.name for specimen in specimens.filter("curve where (strain > 30)")])
for specimen in specimens.query("ductile").sorted("-elongation"):
    print(specimen.name, specimen.elongation)
print(specimens.stats("elongation").mean)
