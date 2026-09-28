---
schema_version: 1
name: robust-porter
tags: [gift]
style: porter
yeast: S-04
hops: [East Kent Goldings]
brewer: tom
status: ready
brewed: 2026-02-21
properties:
  og:
    v: 1.0603333333333333
    readings: [1.059, 1.061, 1.061]
    u: 6.66666666666621e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: bd1da9a02f10}
    fingerprint: 9ac14759abcd
  fg:
    v: 1.0170000000000001
    readings: [1.017, 1.018, 1.016]
    u: 5.773502691896583e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: fba2a0f9305e}
    fingerprint: eabca8932d20
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 5.9, unit: kg, fingerprint: c72bebe9495e}
  mash_temperature: {v: 67, unit: degC}
  abv:
    v: 5.687499999999986
    u: 0.15774286830154827
    unit: "%"
    computed: {fg: acf1ac4fe592, og: 67a4156c66bd}
    fingerprint: 1fb96827e148
  attenuation:
    v: 71.82320441988931
    unit: "%"
    computed: {v: {fg: acf1ac4fe592, og: 67a4156c66bd}}
    fingerprint: 3e0b386ff333
  efficiency:
    v: 66.40252402964269
    unit: "%"
    computed: {v: {grain_mass: c72bebe9495e, og: 67a4156c66bd, volume: e2cdeaea1df9}}
    fingerprint: 6e29dd4e7a9b
  drop:
    v: 11.399999999999988
    u: 1.7944358444926487
    unit: pt/d
    computed: {og: 67a4156c66bd, fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1}
    fingerprint: d68c8f820988
  score:
    v: 35.333333333333336
    u: 0.3333333333333333
    computed: {tasting.score: 9d0383889dc8}
    fingerprint: 7e3004497b98
tables:
  fermentation:
    title: "Gravity and temperature, day by day"
    index: day
    columns:
      day: {unit: d}
      gravity: {}
      temperature: {unit: degC}
      apparent: {unit: "%"}
      rate: {unit: pt/d}
    rows:
      - day: 1
        gravity: 1.043
        temperature: 19.9
        apparent: {v: 28.729281767955943, computed: {row.gravity: 36724455f9d0, og: 67a4156c66bd}, fingerprint: 9a683b9b851d}
        rate: {v: 17.33333333333342, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: 096df99ddb00}
      - day: 2
        gravity: 1.032
        temperature: 21
        apparent: {v: 46.961325966850794, computed: {row.gravity: 6174cde63ce7, og: 67a4156c66bd}, fingerprint: 8353df739864}
        rate: {v: 10.9999999999999, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: d1b2b7685f36}
      - day: 3
        gravity: 1.026
        temperature: 20.6
        apparent: {v: 56.90607734806627, computed: {row.gravity: 37aba3e5a21c, og: 67a4156c66bd}, fingerprint: e7339b92661d}
        rate: {v: 6.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: f51378cc159f}
      - day: 4
        gravity: 1.022
        temperature: 19.4
        apparent: {v: 63.53591160220992, computed: {row.gravity: e4effdb21f25, og: 67a4156c66bd}, fingerprint: 3f4dff3e792c}
        rate: {v: 4.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: 8babce7201f4}
      - day: 6
        gravity: 1.018
        temperature: 19.1
        apparent: {v: 70.16574585635357, computed: {row.gravity: d3b982f7f86d, og: 67a4156c66bd}, fingerprint: 213ccbb5766f}
        rate: {v: 2.0000000000000036, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: 10f1fb7ae2a6}
      - day: 8
        gravity: 1.018
        temperature: 18.8
        apparent: {v: 70.16574585635357, computed: {row.gravity: d3b982f7f86d, og: 67a4156c66bd}, fingerprint: 213ccbb5766f}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: d3e1083d25fb}
      - day: 11
        gravity: 1.017
        temperature: 18.9
        apparent: {v: 71.82320441988966, computed: {row.gravity: ef63930342dd, og: 67a4156c66bd}, fingerprint: aae850d16b15}
        rate: {v: 0.33333333333337006, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: 8b1644243d4f}
      - day: 14
        gravity: 1.016
        temperature: 19.1
        apparent: {v: 73.4806629834254, computed: {row.gravity: 2b99b910dcad, og: 67a4156c66bd}, fingerprint: d5801d87aa75}
        rate: {v: 0.3333333333332966, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: d940f753b0e1, og: 67a4156c66bd}, fingerprint: 6d743c154639}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 35
        bitterness: 2
      - taster: Ivo
        score: 35
        bitterness: 2
      - taster: Noor
        score: 36
        bitterness: 1
---
# Robust porter

Bottled twenty for the neighbours.
