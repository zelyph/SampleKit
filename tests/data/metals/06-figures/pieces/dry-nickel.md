---
schema_version: 1
name: dry-nickel
tags: [impure]
style: nickel
process: Normalised
carbon: [Fullerene]
tester: ana
status: tempering
poured: 2026-04-11
properties:
  radius:
    v: 1.0433333333333332
    readings: [1.044, 1.043, 1.043]
    u: 3.3333333333332443e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 0af8bd4decf0}
    fingerprint: 5962c1426074
  notch:
    v: 1.0099999999999998
    readings: [1.011, 1.009, 1.01]
    u: 5.773502691895942e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 603579a9f8e2}
    fingerprint: 715a3d07e482
  volume:
    v: 20.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 4.1, unit: kg, fingerprint: 7bbe214853ed}
  preheat_temperature: {v: 66, unit: degC}
  modulus:
    v: 4.375000000000013
    u: 0.13834964763236396
    unit: "%"
    computed: {notch: 100396ebe001, radius: a8ea7995583e}
    fingerprint: 56612a681e02
  reduction:
    v: 76.92307692307736
    unit: "%"
    computed: {notch: 100396ebe001, radius: a8ea7995583e}
    fingerprint: a7c535875e2c
  recovery:
    v: 70.34632034632017
    unit: "%"
    computed: {ore_mass: 7bbe214853ed, radius: a8ea7995583e, volume: 34b353a78c5c}
    fingerprint: d599adbb336c
  drop:
    v: 8.599999999999985
    u: 1.5231546211727645
    unit: pt/d
    computed: {radius: a8ea7995583e, ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59}
    fingerprint: a86b5c24407f
  score:
    v: 40.0
    u: 2.3094010767585034
    computed: {inspection.score: 224dc40aa603}
    fingerprint: ed696e922e59
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
        hardness: 1.029
        temperature: 19.2
        nominal: {v: 33.0769230769231, computed: {row.hardness: f801ead50a79, radius: a8ea7995583e}, fingerprint: 95f742caff28}
        rate: {v: 14.333333333333307, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: a7277a1e91ed}
      - day: 2
        hardness: 1.022
        temperature: 19.6
        nominal: {v: 49.230769230769056, computed: {row.hardness: e4effdb21f25, radius: a8ea7995583e}, fingerprint: b16d71c1b105}
        rate: {v: 6.999999999999893, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: 29449efd71db}
      - day: 3
        hardness: 1.017
        temperature: 18.6
        nominal: {v: 60.769230769230894, computed: {row.hardness: ef63930342dd, radius: a8ea7995583e}, fingerprint: e3a2ac50937a}
        rate: {v: 5.000000000000117, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: 636b7537e879}
      - day: 4
        hardness: 1.014
        temperature: 18.1
        nominal: {v: 67.69230769230758, computed: {row.hardness: ddc8d2b525d6, radius: a8ea7995583e}, fingerprint: 02b89388747c}
        rate: {v: 2.9999999999998916, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: c57828caba55}
      - day: 6
        hardness: 1.012
        temperature: 17.2
        nominal: {v: 72.30769230769221, computed: {row.hardness: 3ede97ec1bdd, radius: a8ea7995583e}, fingerprint: f4f43ba97600}
        rate: {v: 1.0000000000000009, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: a171cf00710f}
      - day: 8
        hardness: 1.011
        temperature: 18
        nominal: {v: 74.61538461538478, computed: {row.hardness: 21fe04bbedf7, radius: a8ea7995583e}, fingerprint: 766c0ed46a4a}
        rate: {v: 0.500000000000056, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: a89d5070d56d}
      - day: 11
        hardness: 1.01
        temperature: 17.2
        nominal: {v: 76.92307692307685, computed: {row.hardness: 50dc83923eca, radius: a8ea7995583e}, fingerprint: d47712fd9206}
        rate: {v: 0.3333333333332966, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: 6d743c154639}
      - day: 14
        hardness: 1.009
        temperature: 17.3
        nominal: {v: 79.23076923076941, computed: {row.hardness: 506eb7f5cf63, radius: a8ea7995583e}, fingerprint: 7fb8b1ac8ba6}
        rate: {v: 0.3333333333333706, computed: {ageing.day: add6d3b4afed, ageing.hardness: 1d0346f95a59, radius: a8ea7995583e}, fingerprint: d8d585c65a0d}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 36
        brittleness: 2
      - inspector: Mia
        score: 40
        brittleness: 2
      - inspector: Sam
        score: 44
        brittleness: 4
---
# Dry nickel

A film on the surface on day 10: pitted, kept to see.
