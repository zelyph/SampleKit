"""Whatever the model needs and is not the model.

A package rather than a loose file, so that the model imports its names from
one place, `from helpers import linear_fit`, and so that this can grow into
several modules without the model's imports changing.

Everything the model imports is code its formulas run: change a function here
and the values of every formula that calls it become outdated, as when the
formula itself changes.
"""

from .measuring import CALLIPER, linear_fit

__all__ = ["CALLIPER", "linear_fit"]
