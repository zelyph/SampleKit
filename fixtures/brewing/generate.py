"""Write the home-brewing demo, a folder per step, from simple to
complex:

  01-first-brews   three brews written by hand, nothing else
  02-measuring     units, uncertainties, tags, what the configuration presents
  03-selecting     twelve brews: queries, profiles, exports
  04-computing     a model: alcohol, attenuation, efficiency from the readings
  05-fermentation  tables: the fermentation day by day, the tasting
  06-figures       figures, and the styles that label them
  07-two-brewers   two projects side by side, one model between them
  08-python        the brews read from Python

    python3 fixtures/brewing/generate.py DESTINATION

Only writes files: `build.sh` runs it, then computes what the steps with a
model show computed. `readme/README.md` is the demo's own, above the steps;
`readme/NN-step.md` each step's.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from brews import BREWS, FIRST, draw  # noqa: E402

HERE = Path(__file__).parent
STEPS = [
    "01-first-brews",
    "02-measuring",
    "03-selecting",
    "04-computing",
    "05-fermentation",
    "06-figures",
    "07-two-brewers",
    "08-python",
]


# ------------------------------------------------------------------ samples


def _number(value):
    text = f"{value:.4f}".rstrip("0").rstrip(".")
    return text if "." in text or "e" in text else text


def _list(items):
    return "[" + ", ".join(items) + "]"


def sample(brew, step):
    """A brew's file as the step shows it."""
    lines = ["---", "schema_version: 1", f"name: {brew.name}"]
    if step >= 2 and brew.tags:
        lines.append(f"tags: {_list(brew.tags)}")
    lines.append(f"style: {brew.style}")
    lines.append(f"yeast: {brew.yeast}")
    if step >= 2:
        lines.append(f"hops: {_list(brew.hops)}")
        lines.append(f"brewer: {brew.brewer}")
        lines.append(f"status: {brew.status}")
    lines.append(f"brewed: {brew.brewed}")
    lines.append("properties:")
    if step == 1:
        lines.append(f"  og: {_number(brew.og)}")
        if not brew.fermenting:
            lines.append(f"  fg: {_number(brew.fg)}")
        lines.append(f"  volume: {{v: {_number(brew.volume)}, unit: L}}")
    else:
        if step < 4:
            # Read once, to a graduation of the hydrometer.
            lines.append(f"  og: {{v: {_number(brew.og)}, u: 0.001}}")
            if not brew.fermenting:
                lines.append(f"  fg: {{v: {_number(brew.fg)}, u: 0.001}}")
        else:
            # Read three times: the model makes a value and an uncertainty of them.
            lines.append(f"  og: {{readings: {_list(_number(r) for r in brew.og_readings)}}}")
            if brew.fg_readings:
                lines.append(f"  fg: {{readings: {_list(_number(r) for r in brew.fg_readings)}}}")
        lines.append(f"  volume: {{v: {_number(brew.volume)}, unit: L}}")
        lines.append(f"  grain_mass: {{v: {_number(brew.grain_mass)}, unit: kg}}")
        lines.append(f"  mash_temperature: {{v: {_number(brew.mash_temperature)}, unit: degC}}")
    if step >= 5:
        lines.append("tables:")
        lines.append("  fermentation:")
        lines.append("    title: Gravity and temperature, day by day")
        lines.append("    index: day")
        lines.append("    columns:")
        lines.append("      day: {unit: d}")
        lines.append("      gravity: {}")
        lines.append("      temperature: {unit: degC}")
        lines.append("    rows:")
        for day, gravity, temperature in brew.fermentation:
            lines.append(
                f"      - {{day: {day}, gravity: {_number(gravity)}, "
                f"temperature: {_number(temperature)}}}"
            )
        if brew.tasting:
            lines.append("  tasting:")
            lines.append("    title: Scores out of 50, bitterness from 1 to 5")
            lines.append("    index: taster")
            lines.append("    columns:")
            lines.append("      taster: {}")
            lines.append("      score: {}")
            lines.append("      bitterness: {}")
            lines.append("    rows:")
            for taster, score, bitterness in brew.tasting:
                lines.append(
                    f"      - {{taster: {taster}, score: {score}, bitterness: {bitterness}}}"
                )
    lines.append("---")
    lines.append(f"# {brew.title}")
    lines.append("")
    lines.append(brew.note)
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------ configuration


def by_step(source, step):
    """What of `source` a step shows: lines between `# >>> step N` and its
    `# <<<` appear from step N on; the markers nest."""
    kept, hidden = [], []
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("# >>> step "):
            hidden.append(int(stripped.split()[-1]) > step)
            continue
        if stripped == "# <<<":
            hidden.pop()
            continue
        if not any(hidden):
            kept.append(line)
    return "\n".join(kept) + "\n"


def configuration(step, model_path="model/brew.py"):
    """The `.samplekitrc` of a step."""
    text = by_step((HERE / "samplekitrc.toml").read_text(), step)
    return text.replace('"model/brew.py"', f'"{model_path}"')


def model(step):
    """The model of a step: the formulas, then the tables, then a figure."""
    return by_step((HERE / "model" / "brew.py").read_text(), step)


def helper(step):
    """The module the model imports, as the step needs it."""
    return by_step((HERE / "model" / "hydrometer.py").read_text(), step)


# ------------------------------------------------------------------ writing


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def project(folder, step, brews, model_path="model/brew.py", with_model=True):
    write(folder / ".samplekitrc", configuration(step, model_path))
    for brew in brews:
        write(folder / "brews" / f"{brew.name}.md", sample(brew, step))
    if step >= 4 and with_model:
        write(folder / model_path, model(step))
        write(folder / "model" / "hydrometer.py", helper(step))


def main(destination):
    draw()
    destination = Path(destination)
    # The demo's own README, above the steps: what a copy of it says first,
    # wherever it was written.
    if (destination / "README.md").exists():
        sys.exit(f"{destination / 'README.md'} exists; remove it first")
    write(destination / "README.md", (HERE / "readme" / "README.md").read_text())
    for number, name in enumerate(STEPS, start=1):
        folder = destination / name
        if folder.exists():
            sys.exit(f"{folder} exists; remove it first")
        readme = HERE / "readme" / f"{name}.md"
        if number == 7:
            # Two projects and a model beside them, in a folder that is none.
            write(folder / "shared" / "brew.py", model(6))
            write(folder / "shared" / "hydrometer.py", helper(6))
            for brewer in ("ana", "tom"):
                own = [brew for brew in BREWS if brew.brewer == brewer]
                project(folder / brewer, 6, own, "../shared/brew.py", with_model=False)
        else:
            brews = FIRST if number <= 2 else BREWS
            project(folder, min(number, 6), brews)
        if number == 8:
            for script in sorted((HERE / "scripts").glob("*.py")):
                write(folder / "scripts" / script.name, script.read_text())
        if readme.exists():
            write(folder / "README.md", readme.read_text())


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
