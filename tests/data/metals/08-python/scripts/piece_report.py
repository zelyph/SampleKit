"""What a tester runs after a piece day: what is not up to date, the best
pieces, and a CSV of the season for a spreadsheet.

    python scripts/piece_report.py
"""

import samplekit as sk

pieces = sk.load("pieces")

# What status reports, piece by piece.
for piece in pieces:
    waiting = piece.not_current()
    if waiting:
        print(f"{piece.name}: {', '.join(waiting)}")

# The best of the inspected, by their mean score.
inspected = pieces.filter("score is present").sorted("-score")
print()
for piece in inspected[:3]:
    print(f"{piece.name:<18} {piece.score.value:5.1f} ± {piece.score.uncertainty:.1f}"
          f"   {piece.modulus.value:.1f} %")

# Every piece of the season, as the "modulus" profile shows them — written by
# SampleKit, so that `samplekit season.csv explain` says what it was made from.
pieces.to_csv("season.csv", profile="modulus", overwrite=True)
print("\nwrote season.csv")
