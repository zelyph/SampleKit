"""A script of your own: the samples read, the model's script called, what it
gives printed.

Save it in the project's `scripts/` folder and run it from the project's
folder with the project's Python:

    .venv/bin/python scripts/script.py --limit 150

Every line marked `← change` is one of your own choices.
"""

import argparse

import samplekit as sk

options = argparse.ArgumentParser(description=__doc__.splitlines()[0])
options.add_argument("--limit", type=float, default=100.0)  # ← your options
options.add_argument("--save", action="store_true", help="write what changed")
given = options.parse_args()

specimens = sk.load("samples").sorted("-area")  # ← which samples, in which order
for specimen in specimens:
    # The model's script, called as a method: its option is a keyword.
    specimen.classify(limit=given.limit)
    print(f"{specimen.name:14} {specimen.area}  {specimen.size}")  # ← what it prints
    if given.save:
        specimen.save()
