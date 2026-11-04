---
schema_version: 1
name: beta-bronze
tags: [dry_treated]
style: bronze_alloy
process: Vacuum
carbon: [Mesophase, Glassy]
tester: ana
status: ready
poured: 2026-03-07
properties:
  radius:
    v: 1.0539999999999998
    readings: [1.055, 1.053, 1.054]
    u: 5.773502691895942e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 29404fdd8b4a}
    fingerprint: 91a5f93f346f
  notch:
    v: 1.0123333333333333
    readings: [1.012, 1.013, 1.012]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: e6619f8a04d5}
    fingerprint: 126eb670f3dc
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 5, unit: kg, fingerprint: ff7f3288e42d}
  preheat_temperature: {v: 66, unit: degC}
  modulus:
    v: 5.4687499999999805
    u: 0.1383496476323628
    unit: "%"
    computed: {notch: b8be79bd79f7, radius: 76b4b198c9ed}
    fingerprint: 23e690bb67dc
  reduction:
    v: 77.16049382716047
    unit: "%"
    computed: {notch: b8be79bd79f7, radius: 76b4b198c9ed}
    fingerprint: 3259683d3c24
  recovery:
    v: 70.12987012986991
    unit: "%"
    computed: {ore_mass: ff7f3288e42d, radius: 76b4b198c9ed, volume: e2cdeaea1df9}
    fingerprint: f3c7d0072a79
  drop:
    v: 10.599999999999943
    u: 1.5874507866387193
    unit: pt/d
    computed: {radius: 76b4b198c9ed, ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066}
    fingerprint: a394333c53ef
  score:
    v: 40.75
    u: 2.780137886268713
    computed: {inspection.score: b111d483d8ee}
    fingerprint: 8d408576c8a4
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
        hardness: 1.038
        temperature: 21.7
        nominal: {v: 29.629629629629342, computed: {row.hardness: fb88df114fc9, radius: 76b4b198c9ed}, fingerprint: d83bb4f4a532}
        rate: {v: 15.999999999999794, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: f06d9b4526f2}
      - day: 2
        hardness: 1.028
        temperature: 22.6
        nominal: {v: 48.14814814814793, computed: {row.hardness: ed054d5cba9b, radius: 76b4b198c9ed}, fingerprint: 29a1db375355}
        rate: {v: 10.00000000000001, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: ccb1c6a508a3}
      - day: 3
        hardness: 1.022
        temperature: 22
        nominal: {v: 59.25925925925909, computed: {row.hardness: e4effdb21f25, radius: 76b4b198c9ed}, fingerprint: 8c0567fa8076}
        rate: {v: 6.0000000000000036, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: 80d515e958e4}
      - day: 4
        hardness: 1.019
        temperature: 20.6
        nominal: {v: 64.81481481481488, computed: {row.hardness: 17ed405c8a30, radius: 76b4b198c9ed}, fingerprint: 6938fc0739a2}
        rate: {v: 3.0000000000001137, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: b786283bb5ce}
      - day: 6
        hardness: 1.014
        temperature: 19.9
        nominal: {v: 74.07407407407396, computed: {row.hardness: ddc8d2b525d6, radius: 76b4b198c9ed}, fingerprint: 38ca225b5f1d}
        rate: {v: 2.4999999999999476, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: fe294989d129}
      - day: 8
        hardness: 1.014
        temperature: 19.9
        nominal: {v: 74.07407407407396, computed: {row.hardness: ddc8d2b525d6, radius: 76b4b198c9ed}, fingerprint: 38ca225b5f1d}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: d3e1083d25fb}
      - day: 11
        hardness: 1.012
        temperature: 20.1
        nominal: {v: 77.77777777777769, computed: {row.hardness: 3ede97ec1bdd, radius: 76b4b198c9ed}, fingerprint: 01579f4a2324}
        rate: {v: 0.6666666666666673, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: 9cb86828eb5c}
      - day: 14
        hardness: 1.012
        temperature: 20.1
        nominal: {v: 77.77777777777769, computed: {row.hardness: 3ede97ec1bdd, radius: 76b4b198c9ed}, fingerprint: 01579f4a2324}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: 639d0e8b6066, radius: 76b4b198c9ed}, fingerprint: d3e1083d25fb}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 37
        brittleness: 4
      - inspector: Noor
        score: 35
        brittleness: 3
      - inspector: Sam
        score: 46
        brittleness: 4
      - inspector: Mia
        score: 45
        brittleness: 3
---
# Beta bronze alloy

Flux and tungsten for the roughness.
