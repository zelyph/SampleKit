"""What a calliper gives, and a least-squares fit, with nothing to install."""

import math

# A calliper read to 0.02 mm: the standard uncertainty of a rectangular
# distribution that wide.
CALLIPER = 0.02 / math.sqrt(12)


def linear_fit(xs, ys):
    """`y = slope * x + intercept`, with the slope's standard error.

    Returns (slope, intercept, standard_error). The error is None for fewer
    than three points, where it has no meaning.
    """
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
