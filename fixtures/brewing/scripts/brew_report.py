"""What a brewer runs after a brew day: what is not up to date, the best
brews, and a CSV of the season for a spreadsheet.

    python scripts/brew_report.py
"""

import samplekit as sk

brews = sk.load("brews")

# What status reports, brew by brew.
for brew in brews:
    waiting = brew.not_current()
    if waiting:
        print(f"{brew.name}: {', '.join(waiting)}")

# The best of the tasted, by their mean score.
tasted = brews.filter("score is present").sorted("-score")
print()
for brew in tasted[:3]:
    print(f"{brew.name:<18} {brew.score.value:5.1f} ± {brew.score.uncertainty:.1f}"
          f"   {brew.abv.value:.1f} %")

# Every brew of the season, as the "alcohol" profile shows them.
with open("season.csv", "w") as out:
    out.write(brews.to_csv(profile="alcohol"))
print("\nwrote season.csv")
