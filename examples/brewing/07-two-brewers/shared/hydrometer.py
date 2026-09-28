"""What a hydrometer's readings give. A helper the model imports: the value of
a formula calling it becomes outdated when it changes, since it is code the
formula runs."""

import math

# A hydrometer graduated every 0.002, read to half a graduation: the standard
# uncertainty of a rectangular distribution that wide.
RESOLUTION = 0.001 / math.sqrt(3)

# What a kilogram of malt gives a litre of wort at best, in gravity points
# (1.001 is one point): about 37 points per pound per US gallon.
POTENTIAL = 308.0


def points(gravity):
    """A gravity in points: 1.052 is 52."""
    return (gravity - 1.0) * 1000.0


def linear_fit(xs, ys):
    """Least squares `y = slope * x + intercept`, with the slope's standard error."""
    n = len(xs)
    mean_x = sum(xs) / n
    mean_y = sum(ys) / n
    sxx = sum((x - mean_x) ** 2 for x in xs)
    sxy = sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys))
    slope = sxy / sxx
    intercept = mean_y - slope * mean_x
    if n < 3:
        return slope, intercept, None
    residuals = sum((y - (slope * x + intercept)) ** 2 for x, y in zip(xs, ys))
    return slope, intercept, math.sqrt(residuals / (n - 2) / sxx)
