"""What a sample of this project has, and what is derived from what.

The class exists and runs as it is: a sample needs nothing declared here to be
read. Declare in `__init__` each property you measure and each one derived from
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
        #   self.gauge_length = sk.Property(unit="mm")
        #
        # measured several times: value and uncertainty are statistics of
        # the readings a file holds
        #   self.width = sk.Property(unit="mm", value=sk.stats.mean,
        #                            uncertainty=sk.stats.standard_error)
        #
        # a text each file gives, a material or a supplier, is read by a
        # formula naming it in its depends_on; declared here, it is the value a
        # file without it takes
        #   self.material = "steel"

        # derived from other values, by a formula below
        #   self.area = sk.Property(unit="mm²", compute=self._area)
        #
        # a table: its columns here, its rows in each file, the index named
        #   self.curve = sk.Table({"strain": sk.Property(unit="%"),
        #                          "force": sk.Property(unit="kN")}, index="strain")

        # What each formula reads: how SampleKit knows a value is not current.
        # A formula reading what it did not declare is never reported outdated.
        #   self.set_dependencies("area", depends_on=["width", "thickness"])

    # ------------------------------------------------------------- formulas
    #
    # A number, or a value and its uncertainty as a pair:
    #
    #   def _area(self):
    #       return self.width.value * self.thickness.value
    #
    # ------------------------------------------------------------- figures
    #
    # A figure the model draws, of one sample (ax) or of several:
    #
    #   @sk.figure
    #   def curve_plot(self, ax):
    #       ax.plot(self.curve.values("strain"), self.curve.values("force"))
