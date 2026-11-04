"""The paper's table and figure, from Python."""

import samplekit as sk

specimens = sk.load("samples")
print(specimens.to_text("markdown", profile="overview"))
# Written again at each draft: a file there already is replaced.
print(specimens.export("paper", overwrite=True).name)
print(specimens.export("curves", overwrite=True).name)
