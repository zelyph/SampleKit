"""The model's scripts, called from Python."""

import samplekit as sk

specimens = sk.load("samples")
for specimen in specimens:
    specimen.classify(limit=15)
    print(specimen.name, specimen.ductility)
type(specimens[0]).ranking(specimens, by="elongation")
