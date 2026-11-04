import samplekit as sk


class Specimen(sk.Sample):
    """A tensile specimen: what it holds, and in which units."""

    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # Measured three times with a micrometer: the value is the mean of the
        # readings, and the uncertainty their standard error.
        self.width = sk.Property(
            unit="mm",
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )
        self.thickness = sk.Property(
            unit="mm",
            value=sk.stats.mean,
            uncertainty=sk.stats.standard_error,
        )
