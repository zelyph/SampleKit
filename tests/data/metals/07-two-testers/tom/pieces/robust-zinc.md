---
schema_version: 1
name: robust-zinc
tags: [gift]
style: zinc
process: T-04
carbon: [Electrode Graphite]
tester: tom
status: ready
poured: 2026-02-21
properties:
  radius:
    v: 1.0603333333333333
    readings: [1.059, 1.061, 1.061]
    u: 6.66666666666621e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: bd1da9a02f10}
    fingerprint: 9ac14759abcd
  notch:
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
  ore_mass: {v: 5.9, unit: kg, fingerprint: c72bebe9495e}
  preheat_temperature: {v: 67, unit: degC}
  modulus:
    v: 5.687499999999986
    u: 0.15774286830154827
    unit: "%"
    computed: {notch: acf1ac4fe592, radius: 67a4156c66bd}
    fingerprint: 1fb96827e148
  reduction:
    v: 71.82320441988931
    unit: "%"
    computed: {notch: acf1ac4fe592, radius: 67a4156c66bd}
    fingerprint: 3e0b386ff333
  recovery:
    v: 66.40252402964269
    unit: "%"
    computed: {ore_mass: c72bebe9495e, radius: 67a4156c66bd, volume: e2cdeaea1df9}
    fingerprint: 6e29dd4e7a9b
  drop:
    v: 11.399999999999988
    u: 1.7944358444926487
    unit: pt/d
    computed: {radius: 67a4156c66bd, ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1}
    fingerprint: d68c8f820988
  score:
    v: 35.333333333333336
    u: 0.3333333333333333
    computed: {inspection.score: 9d0383889dc8}
    fingerprint: 7e3004497b98
tables:
  ageing:
    title: "Hardness and temperature, day by day"
    index: day
    columns:
      day: {unit: d}
      hardness: {}
      temperature: {unit: degC}
      nominal: {unit: "%"}
      rate: {unit: pt/d}
    rows:
      - day: 1
        hardness: 1.043
        temperature: 19.9
        nominal: {v: 28.729281767955943, computed: {row.hardness: 36724455f9d0, radius: 67a4156c66bd}, fingerprint: 9a683b9b851d}
        rate: {v: 17.33333333333342, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: 096df99ddb00}
      - day: 2
        hardness: 1.032
        temperature: 21
        nominal: {v: 46.961325966850794, computed: {row.hardness: 6174cde63ce7, radius: 67a4156c66bd}, fingerprint: 8353df739864}
        rate: {v: 10.9999999999999, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: d1b2b7685f36}
      - day: 3
        hardness: 1.026
        temperature: 20.6
        nominal: {v: 56.90607734806627, computed: {row.hardness: 37aba3e5a21c, radius: 67a4156c66bd}, fingerprint: e7339b92661d}
        rate: {v: 6.000000000000007, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: f51378cc159f}
      - day: 4
        hardness: 1.022
        temperature: 19.4
        nominal: {v: 63.53591160220992, computed: {row.hardness: e4effdb21f25, radius: 67a4156c66bd}, fingerprint: 3f4dff3e792c}
        rate: {v: 4.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: 8babce7201f4}
      - day: 6
        hardness: 1.018
        temperature: 19.1
        nominal: {v: 70.16574585635357, computed: {row.hardness: d3b982f7f86d, radius: 67a4156c66bd}, fingerprint: 213ccbb5766f}
        rate: {v: 2.0000000000000036, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: 10f1fb7ae2a6}
      - day: 8
        hardness: 1.018
        temperature: 18.8
        nominal: {v: 70.16574585635357, computed: {row.hardness: d3b982f7f86d, radius: 67a4156c66bd}, fingerprint: 213ccbb5766f}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: d3e1083d25fb}
      - day: 11
        hardness: 1.017
        temperature: 18.9
        nominal: {v: 71.82320441988966, computed: {row.hardness: ef63930342dd, radius: 67a4156c66bd}, fingerprint: aae850d16b15}
        rate: {v: 0.33333333333337006, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: 8b1644243d4f}
      - day: 14
        hardness: 1.016
        temperature: 19.1
        nominal: {v: 73.4806629834254, computed: {row.hardness: 2b99b910dcad, radius: 67a4156c66bd}, fingerprint: d5801d87aa75}
        rate: {v: 0.3333333333332966, computed: {ageing.day: add6d3b4afed, ageing.hardness: d940f753b0e1, radius: 67a4156c66bd}, fingerprint: 6d743c154639}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 35
        brittleness: 2
      - inspector: Ivo
        score: 35
        brittleness: 2
      - inspector: Noor
        score: 36
        brittleness: 1
---
# Robust zinc

Cast twenty for the neighbours.
