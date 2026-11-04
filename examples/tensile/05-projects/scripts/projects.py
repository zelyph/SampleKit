"""Both projects read together, from Python."""

import samplekit as sk

specimens = sk.load(".")
for specimen in specimens.sorted("-elongation"):
    print(specimen.project.root.name, specimen.name, specimen.elongation)
