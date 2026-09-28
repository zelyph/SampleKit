"""The brews of the home-brewing demo: invented, and meant to read as a
home brewer's notebook — gravities around 1.040–1.080, fermentations of one to
two weeks, a little noise in every reading.

Everything is drawn from one seeded generator, so a build is the same brews
every time. `generate.py` writes them into each step of the demo.
"""

import math
import random
from dataclasses import dataclass, field

SEED = 20260924

# How fast each yeast takes the gravity down (per day), and where it ferments.
YEASTS = {
    "US-05": (0.50, 19.0),
    "S-04": (0.60, 19.0),
    "Nottingham": (0.55, 17.5),
    "Belle Saison": (0.45, 26.0),
    "WB-06": (0.65, 20.5),
    "Verdant": (0.50, 20.0),
}

# How bitter each style tastes, 1 to 5, around which tasters scatter.
BITTERNESS = {"ipa": 4.3, "pale_ale": 3.2, "stout": 2.8, "porter": 2.6, "saison": 2.2, "wheat": 1.4}

# Brew day is day 0: its gravity is the original gravity, read apart.
DAYS = [1, 2, 3, 4, 6, 8, 11, 14]
TASTERS = ["Lea", "Sam", "Noor", "Ivo", "Mia"]


@dataclass
class Brew:
    name: str
    title: str
    brewer: str
    style: str
    yeast: str
    hops: list
    brewed: str
    og: float
    fg: float
    volume: float
    grain_mass: float
    mash_temperature: float
    status: str
    tags: list = field(default_factory=list)
    note: str = ""
    # Filled by `draw`.
    og_readings: list = field(default_factory=list)
    fg_readings: list = field(default_factory=list)
    fermentation: list = field(default_factory=list)
    tasting: list = field(default_factory=list)

    @property
    def fermenting(self):
        return self.status == "fermenting"


BREWS = [
    Brew("oatmeal-stout", "Oatmeal stout", "ana", "stout", "S-04", ["Fuggles"], "2026-01-10",
         1.058, 1.016, 19.5, 5.6, 68, "ready", ["medal"],
         "Flaked oats in the mash for the body. Took silver at the spring club contest."),
    Brew("citra-ipa", "Citra IPA", "tom", "ipa", "US-05", ["Citra", "Mosaic"], "2026-01-24",
         1.064, 1.011, 20.5, 6.1, 65, "ready", ["dry_hopped"],
         "Dry hopped on day 4 with 100 g of Citra."),
    Brew("farmhouse-saison", "Farmhouse saison", "ana", "saison", "Belle Saison", ["Saaz"],
         "2026-02-07", 1.052, 1.004, 21.0, 4.6, 64, "ready", [],
         "Let free-rise to 27 °C; very dry, peppery."),
    Brew("robust-porter", "Robust porter", "tom", "porter", "S-04", ["East Kent Goldings"],
         "2026-02-21", 1.060, 1.017, 20.0, 5.9, 67, "ready", ["gift"],
         "Bottled twenty for the neighbours."),
    Brew("hazy-pale", "Hazy pale ale", "ana", "pale_ale", "Verdant", ["Mosaic", "Galaxy"],
         "2026-03-07", 1.054, 1.012, 20.0, 5.0, 66, "ready", ["dry_hopped"],
         "Oats and wheat for the haze."),
    Brew("hefeweizen", "Hefeweizen", "tom", "wheat", "WB-06", ["Hallertau"], "2026-03-14",
         1.050, 1.011, 21.5, 4.5, 65, "ready", [],
         "Fermented warm for banana."),
    Brew("session-ipa", "Session IPA", "tom", "ipa", "US-05", ["Citra"], "2026-03-28",
         1.042, 1.009, 20.0, 3.9, 66, "conditioning", ["dry_hopped"],
         "Low gravity, same hops as the Citra IPA."),
    Brew("dry-stout", "Dry stout", "ana", "stout", "Nottingham", ["Fuggles"], "2026-04-11",
         1.044, 1.010, 20.5, 4.1, 66, "conditioning", ["infected"],
         "A film on the surface on day 10: sour, kept to see."),
    Brew("double-ipa", "Double IPA", "tom", "ipa", "US-05", ["Simcoe", "Citra"], "2026-04-25",
         1.078, 1.014, 19.0, 7.8, 64, "ready", ["medal", "dry_hopped"],
         "Gold at the spring club contest."),
    Brew("blonde-saison", "Blonde saison", "ana", "saison", "Belle Saison", ["Hallertau"],
         "2026-05-09", 1.048, 1.003, 21.0, 4.3, 64, "conditioning", [],
         "A lighter saison for summer."),
    Brew("english-pale", "English pale ale", "tom", "pale_ale", "Nottingham",
         ["East Kent Goldings"], "2026-05-23", 1.048, 1.011, 20.0, 4.5, 67, "conditioning", [],
         "Bottle conditioned."),
    Brew("smoked-porter", "Smoked porter", "ana", "porter", "S-04", ["Fuggles"], "2026-06-06",
         1.066, 1.019, 19.5, 6.6, 68, "fermenting", [],
         "A third of the malt smoked over beech. Still fermenting: no final gravity yet."),
]

# The first three are the demo's first brews, written by hand.
FIRST = BREWS[:3]


def _readings(rng, value):
    """Three hydrometer readings: the true gravity, each off by up to a
    graduation — never all three alike, or the standard error would be nil."""
    while True:
        readings = [round(value + rng.choice([-0.001, 0.0, 0.001]), 3) for _ in range(3)]
        if len(set(readings)) > 1:
            return readings


def draw():
    """Every brew's readings, fermentation and tasting, from the seed."""
    rng = random.Random(SEED)
    for brew in BREWS:
        brew.og_readings = _readings(rng, brew.og)
        brew.fg_readings = [] if brew.fermenting else _readings(rng, brew.fg)
        rate, temperature = YEASTS[brew.yeast]
        rate *= rng.uniform(0.85, 1.15)
        days = DAYS[:4] if brew.fermenting else DAYS
        brew.fermentation = []
        start = sum(brew.og_readings) / len(brew.og_readings)
        for day in days:
            gravity = brew.fg + (start - brew.fg) * math.exp(-rate * day)
            gravity += rng.gauss(0, 0.0006)
            # Warmer while it ferments hard, then back to the room.
            heat = 2.0 * math.exp(-((day - 2) ** 2) / 4)
            brew.fermentation.append(
                (day, round(gravity, 3), round(temperature + heat + rng.gauss(0, 0.3), 1))
            )
        brew.tasting = []
        if not brew.fermenting:
            # Lea hosts the club's tastings and tastes every brew.
            for taster in ["Lea"] + rng.sample(TASTERS[1:], rng.choice([2, 3])):
                score = rng.randint(34, 46) + (2 if "medal" in brew.tags else 0)
                bitterness = min(5, max(1, round(BITTERNESS[brew.style] + rng.gauss(0, 0.5))))
                brew.tasting.append((taster, score, bitterness))
    return BREWS
