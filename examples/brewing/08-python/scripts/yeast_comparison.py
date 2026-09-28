"""Which yeast ferments fastest, and how far: a figure the project does not
declare, drawn from a script.

    python scripts/yeast_comparison.py
"""

import statistics

import samplekit as sk

brews = sk.load("brews").filter("attenuation is present")

by_yeast = {}
for brew in brews:
    by_yeast.setdefault(brew.yeast, []).append(brew)

print(f"{'yeast':<14} {'brews':>5} {'attenuation':>12} {'drop':>6}")
for yeast, own in sorted(by_yeast.items()):
    attenuation = statistics.fmean(brew.attenuation.value for brew in own)
    drop = statistics.fmean(brew.drop.value for brew in own)
    print(f"{yeast:<14} {len(own):>5} {attenuation:>11.0f}% {drop:>6.1f}")

# The project's figure, redrawn over these brews with other axes: its styles,
# symbols and matplotlib settings still apply.
sk.plot(brews, x="drop", y="attenuation", group="yeast",
        title="Fast starters finish drier?", output="yeasts.png", overwrite=True)
print("wrote yeasts.png")
