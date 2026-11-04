"""The links of the guide to linking samples, from Python."""

import samplekit as sk

copper = sk.load("samples/copper-01.md")
steel = copper.linked("compared_with")
print(steel.name, steel.tensile_strength)
specimens = sk.load("samples")
print([specimen.name for specimen in specimens.filter("links to steel-01")])
