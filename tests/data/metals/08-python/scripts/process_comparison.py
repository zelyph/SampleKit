"""Which process ages fastest, and how far: a figure the project does not
declare, drawn from a script.

    python scripts/process_comparison.py
"""

import statistics

import samplekit as sk

pieces = sk.load("pieces").filter("reduction is present")

by_process = {}
for piece in pieces:
    by_process.setdefault(piece.process, []).append(piece)

print(f"{'process':<14} {'pieces':>5} {'reduction':>12} {'drop':>6}")
for process, own in sorted(by_process.items()):
    reduction = statistics.fmean(piece.reduction.value for piece in own)
    drop = statistics.fmean(piece.drop.value for piece in own)
    print(f"{process:<14} {len(own):>5} {reduction:>11.0f}% {drop:>6.1f}")

# The project's figure, redrawn over these pieces with other axes: its styles,
# symbols and matplotlib settings still apply.
sk.plot(pieces, x="drop", y="reduction", group="process",
        title="Fast starters finish drier?", output="processes.png", overwrite=True)
print("wrote processes.png")
