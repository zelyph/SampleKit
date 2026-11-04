"""Write the tensile example of the home page, SampleKit's demo, a folder per step, its samples
as a hand writes them: built by build.sh, which computes them.

    python fixtures/tensile/generate.py DESTINATION

  01-measured  three specimens, their width and thickness one number each; no model
  02-readings  the width and the thickness read three times, a model taking their mean
  03-curve     the test itself: a curve of force against strain, and the stress
  04-results   what the curve gives, a profile, exports, figures, scripts, a sample's own files
  05-projects  the rolled specimens and cast ones, two projects under one root

It is SampleKit's demo too: build.rs embeds examples/tensile, which
tools/regenerate-demo.sh writes with build.sh, and each step's README.md,
from readme/, says what the step holds.

The numbers are fixed: the page shows them, and a run of this script writes the
same files byte for byte.
"""

import math
import pathlib
import random
import shutil
import sys

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent

# Name, material, grade, rolling direction, E [GPa], Rp0.2, Rm [MPa], A [%].
SPECIMENS = [
    ("aluminium-01", "aluminium", "6061-T6", "L", 69, 281, 316, 12),
    ("copper-01", "copper", "C11000", "T", 117, 72, 224, 44),
    ("steel-01", "steel", "S355", "L", 205, 380, 520, 24),
]
# The micrometer's readings: width, then thickness, three each.
READINGS = {
    "aluminium-01": ([12.496, 12.509, 12.503], [1.993, 1.98, 2.007]),
    "copper-01": ([12.509, 12.498, 12.506], [1.995, 1.988, 2.007]),
    "steel-01": ([12.494, 12.528, 12.518], [1.995, 2.004, 2.008]),
}
DATES = ["2026-03-02", "2026-03-03", "2026-03-04"]
# Cast specimens, a project of their own beside the rolled ones: the guide to
# several projects reads them together.
CAST = [
    ("aluminium-c1", "aluminium", "A356-T6", "L", 72, 205, 275, 6),
    ("iron-c1", "iron", "EN-GJS-400", "L", 169, 265, 425, 18),
]
CAST_READINGS = {
    "aluminium-c1": ([12.512, 12.497, 12.505], [2.011, 1.996, 2.003]),
    "iron-c1": ([12.488, 12.502, 12.495], [1.992, 2.006, 1.999]),
}
CAST_DATES = ["2026-04-14", "2026-04-15"]
SHARED = """schema_version = 1

[model]
python = "../.venv"
"""
PROJECT = """schema_version = 1
import = ".."

[model]
path = "../model/specimen.py"
class = "Specimen"
"""

MODEL = """schema_version = 1

[model]
path = "model/specimen.py"
class = "Specimen"
python = "../.venv"
"""
PRECISIONS = """
[property."curve.stress"]
precision = ".1f"
"""
SHARED_RESULTS = """
[property.yield_strength]
precision = ".0f"

[property.tensile_strength]
precision = ".0f"

[property.elongation]
precision = ".1f"

[query.ductile]
filter = "elongation > 20"

[profile.overview]
columns = [
  {field = "name"},
  {field = "material"},
  {field = "young_modulus", label = "E", precision = ".1f"},
  {field = "yield_strength", label = "Rp0.2"},
  {field = "tensile_strength", label = "Rm"},
  {field = "elongation", label = "A"},
]
sort = ["material", "name"]
"""
RESULTS = """
[property.yield_strength]
precision = ".0f"

[property.tensile_strength]
precision = ".0f"

[property.elongation]
precision = ".1f"

[query.ductile]
filter = "elongation > 20"

[profile.overview]
columns = [
  {field = "name"},
  {field = "material"},
  {field = "young_modulus", label = "E", precision = ".1f"},
  {field = "yield_strength", label = "Rp0.2"},
  {field = "tensile_strength", label = "Rm"},
  {field = "elongation", label = "A"},
]
sort = ["material", "name"]

[profile.data]
columns = [
  {field = "name"},
  {field = "young_modulus", header = "E", precision = ".17g"},
  {field = "yield_strength", header = "Rp0.2", precision = ".17g"},
]

[export.paper]
profile = "overview"
style = "latex"
output = "paper/results.tex"

[export.curves]
figure = "curves"
output = "paper/curves.pdf"

[figure.curves]
kind = "line"
x = "curve.strain"
y = "curve.stress"
group = "material"

[style.figure]

[render]
figure_style = "figure"

[property."curve.strain"]
symbol_figure = '$\\varepsilon$'

[matplotlib]
"font.family" = "serif"
"axes.grid" = true
"figure.figsize" = [14, 9]
"lines.marker" = "None"
"""


def curve(e_modulus, yield_strength, tensile_strength, elongation, area):
    """The force read at each strain, from a curve of that shape with a little
    noise: three points in the elastic part, a few around the yield, then
    wider steps to the break."""
    modulus = e_modulus * 10  # MPa per % of strain
    at_yield = yield_strength / modulus
    points = [0, 0.025, 0.05, 0.2, 0.3, 0.4, 0.6, 1, 2, 5, 10, 20, 30, 40]
    points = sorted(set(p for p in points if p < elongation) | {elongation})
    rows = []
    for strain in points:
        if strain <= at_yield:
            stress = modulus * strain
        else:
            plastic = strain - at_yield
            stress = yield_strength + (tensile_strength - yield_strength) * (
                1 - math.exp(-plastic / (elongation * 0.18))
            )
            if strain > elongation * 0.85:
                stress *= 1 - 0.15 * ((strain - elongation * 0.85) / (elongation * 0.15)) ** 2
        stress *= 1 + np.random.normal(0, 0.012 if strain <= at_yield else 0.003)
        rows.append((strain, round(stress * area / 1000, 3)))
    return rows


def front(name, material, grade, direction, date):
    return [
        "---",
        "schema_version: 1",
        f"name: {name}",
        f"material: {material}",
        f"grade: {grade}",
        f"direction: {direction}",
        f"tested: {date}",
        "properties:",
    ]


def note(direction):
    along = "along" if direction == "L" else "across"
    return f"Tested at 2 mm/min, {along} the rolling direction."


def main(destination):
    random.seed(6)
    np.random.seed(6)
    root = pathlib.Path(destination)
    steps = ["01-measured", "02-readings", "03-curve", "04-results"]
    # Each test as the machine exported it, kept beside the samples.
    exported = {}
    for step in steps:
        if (root / step).exists():
            sys.exit(f"{root / step} exists: generate into an empty folder")
        (root / step / "samples").mkdir(parents=True)
    for (name, material, grade, direction, e, ys, uts, el), date in zip(SPECIMENS, DATES):
        widths, thicknesses = READINGS[name]
        area = np.mean(widths) * np.mean(thicknesses)
        rows = curve(e, ys, uts, el, area)
        exported[name] = (rows, direction)
        head = front(name, material, grade, direction, date)
        # One number each, the readings' mean rounded: the first step.
        simple = head + [
            f"  width: {{v: {round(float(np.mean(widths)), 2)}, unit: mm}}",
            f"  thickness: {{v: {round(float(np.mean(thicknesses)), 2)}, unit: mm}}",
            "---",
            note(direction),
            "",
        ]
        (root / "01-measured" / "samples" / f"{name}.md").write_text("\n".join(simple))
        read = head + [
            f"  width: {{readings: [{', '.join(map(str, widths))}], unit: mm}}",
            f"  thickness: {{readings: [{', '.join(map(str, thicknesses))}], unit: mm}}",
        ]
        (root / "02-readings" / "samples" / f"{name}.md").write_text(
            "\n".join(read + ["---", note(direction), ""])
        )
        table = read + [
            "tables:",
            "  curve:",
            "    index: strain",
            "    columns:",
            '      strain: {unit: "%"}',
            "      force: {unit: kN}",
            "    rows:",
        ]
        table += [f"      - {{strain: {strain}, force: {force}}}" for strain, force in rows]
        table += ["---", note(direction), ""]
        (root / "03-curve" / "samples" / f"{name}.md").write_text("\n".join(table))
        # In the results, the specimens of other metals name the steel one
        # they are compared with: a link, followed from either side.
        if name != "steel-01":
            at = table.index("properties:")
            table = table[:at] + ['compared_with: "[[steel-01]]"'] + table[at:]
        (root / "04-results" / "samples" / f"{name}.md").write_text("\n".join(table))
    # Two projects under one root: the rolled specimens, and cast ones.
    projects = root / "05-projects"
    if projects.exists():
        sys.exit(f"{projects} exists: generate into an empty folder")
    (projects / "model").mkdir(parents=True)
    shutil.copy(HERE / "model" / "04-results.py", projects / "model" / "specimen.py")
    (projects / ".samplekitrc").write_text(SHARED + PRECISIONS + SHARED_RESULTS)
    for project, specimens, readings, dates in (
        ("rolled", SPECIMENS, READINGS, DATES),
        ("cast", CAST, CAST_READINGS, CAST_DATES),
    ):
        (projects / project / "samples").mkdir(parents=True)
        (projects / project / ".samplekitrc").write_text(PROJECT)
        for (name, material, grade, direction, e, ys, uts, el), date in zip(specimens, dates):
            widths, thicknesses = readings[name]
            area = np.mean(widths) * np.mean(thicknesses)
            rows = exported[name][0] if name in exported else curve(e, ys, uts, el, area)
            text = front(name, material, grade, direction, date) + [
                f"  width: {{readings: [{', '.join(map(str, widths))}], unit: mm}}",
                f"  thickness: {{readings: [{', '.join(map(str, thicknesses))}], unit: mm}}",
                "tables:",
                "  curve:",
                "    index: strain",
                "    columns:",
                '      strain: {unit: "%"}',
                "      force: {unit: kN}",
                "    rows:",
            ]
            text += [f"      - {{strain: {strain}, force: {force}}}" for strain, force in rows]
            text += ["---", note(direction), ""]
            (projects / project / "samples" / f"{name}.md").write_text("\n".join(text))
    (root / "01-measured" / ".samplekitrc").write_text("schema_version = 1\n")
    # The demo's pages: one above the steps, one in each.
    shutil.copy(HERE / "readme" / "README.md", root / "README.md")
    for step in steps + ["05-projects"]:
        shutil.copy(HERE / "readme" / f"{step}.md", root / step / "README.md")
    for step in steps[1:]:
        (root / step / "model").mkdir()
        shutil.copy(HERE / "model" / f"{step}.py", root / step / "model" / "specimen.py")
    (root / "02-readings" / ".samplekitrc").write_text(MODEL)
    (root / "03-curve" / ".samplekitrc").write_text(MODEL + PRECISIONS)
    results = MODEL.replace(
        "schema_version = 1\n", 'schema_version = 1\n\n[collection]\nfiles = ["photos", "machine/{name}/*"]\n'
    )
    # The stress's symbol in a figure, in the section that holds its
    # precision: TOML takes a section once.
    stress = PRECISIONS.replace(
        'precision = ".1f"\n', "precision = \".1f\"\nsymbol_figure = '$\\sigma$'\n"
    )
    (root / "04-results" / ".samplekitrc").write_text(results + stress + RESULTS)
    # The photograph the home page shows: Cdang's, Wikimedia Commons, public
    # domain.
    (root / "04-results" / "photos").mkdir()
    shutil.copy(
        HERE.parent.parent / "docs" / "images" / "tensile-fracture.jpg",
        root / "04-results" / "photos" / "steel-01-fracture.jpg",
    )
    # The scripts the documentation runs, beside the samples.
    shutil.copytree(HERE / "scripts", root / "04-results" / "scripts",
                    ignore=shutil.ignore_patterns("projects.py"))
    (root / "05-projects" / "scripts").mkdir()
    shutil.copy(HERE / "scripts" / "projects.py", root / "05-projects" / "scripts")
    # What the testing machine wrote of each test: a folder per specimen.
    for name, (rows, direction) in exported.items():
        folder = root / "04-results" / "machine" / name
        folder.mkdir(parents=True)
        lines = ["strain [%],force [kN]"] + [f"{strain},{force}" for strain, force in rows]
        (folder / "curve.csv").write_text("\n".join(lines) + "\n")
        (folder / "settings.txt").write_text(
            f"speed: 2 mm/min\ndirection: {direction}\ngauge length: 50 mm\n"
        )


if __name__ == "__main__":
    main(sys.argv[1])
