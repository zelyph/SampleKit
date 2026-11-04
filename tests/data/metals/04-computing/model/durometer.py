"""What a durometer's readings give. A helper the model imports: the value of
a formula calling it becomes outdated when it changes, since it is code the
formula runs."""

import math

# A durometer graduated every 0.002, read to half a graduation: the standard
# uncertainty of a rectangular distribution that wide.
RESOLUTION = 0.001 / math.sqrt(3)

# What a kilogram of ore gives a litre of the melt at best, in hardness points
# (1.001 is one point).
POTENTIAL = 308.0


def points(hardness):
    """A hardness in points: 1.052 is 52."""
    return (hardness - 1.0) * 1000.0
