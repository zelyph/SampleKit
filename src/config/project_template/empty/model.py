"""What a sample of this project has, and what is derived from what.

The class exists and runs as it is: a sample needs nothing declared here to be
read. Declare in `__init__` each quantity you measure and each one derived from
them, then write the formulas below. `samplekit init --example` in an empty folder
writes a model with one formula of each kind, working, to read beside this one.
"""

import samplekit as sk


class Model(sk.Sample):
    def __init__(self, path=None, name=None):
        super().__init__(path, name=name)

        # What a sample has. Each kind, to copy and fill in:
        #
        # measured, as typed in each file
        #   self.volume = sk.Property(unit="L")
        #
        # measured several times: value and uncertainty are statistics of
        # the readings a file holds
        #   self.og = sk.Property(value=sk.stats.mean,
        #                         uncertainty=sk.stats.standard_error)
        #
        # derived from other values, by a formula below
        #   self.abv = sk.Property(unit="%", compute=self._abv)
        #
        # a value and its uncertainty from one call
        #   self.drop = sk.Property(unit="pt/d", compute_quantity=self._drop)
        #
        # a table: its columns here, its rows in each file, the index named
        #   self.fermentation = sk.Table({"day": sk.Column(unit="d"),
        #                                 "gravity": sk.Column()}, "day")

        # What each formula reads — how SampleKit knows a value is not current.
        # A formula reading what it did not declare is never reported outdated.
        #   self.set_dependencies("abv", depends_on=["og", "fg"])

    # ------------------------------------------------------------- formulas
    #
    #   def _abv(self):
    #       return (self.og.value - self.fg.value) * 131.25
    #
    # ------------------------------------------------------------- figures
    #
    # A figure the model draws, of one sample (ax) or of several:
    #
    #   @sk.figure
    #   def curve(self, ax):
    #       ax.plot(self.fermentation.values("day"),
    #               self.fermentation.values("gravity"))
