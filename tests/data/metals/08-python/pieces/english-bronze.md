---
schema_version: 1
name: english-bronze
style: bronze_alloy
process: Normalised
carbon: [Electrode Graphite]
tester: tom
status: tempering
poured: 2026-05-23
properties:
  radius:
    v: 1.048
    readings: [1.047, 1.049, 1.048]
    u: 5.773502691895942e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 1fec585c2145}
    fingerprint: eeb1e9379427
  notch:
    v: 1.0116666666666665
    readings: [1.012, 1.011, 1.012]
    u: 3.333333333333707e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 73c71b19edcc}
    fingerprint: 94c117bbcc48
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 4.5, unit: kg, fingerprint: b9c0a17d7902}
  preheat_temperature: {v: 67, unit: degC}
  modulus:
    v: 4.768750000000028
    u: 0.1383496476323659
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: b7e0f80ac817}
    fingerprint: 0b113903c626
  reduction:
    v: 75.69444444444483
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: b7e0f80ac817}
    fingerprint: ee3127f78b3e
  recovery:
    v: 69.26406926406932
    unit: "%"
    computed: {ore_mass: b9c0a17d7902, radius: b7e0f80ac817, volume: e2cdeaea1df9}
    fingerprint: f7e6b8f98b69
  drop:
    v: 9.200000000000006
    u: 1.6062378404209383
    unit: pt/d
    computed: {radius: b7e0f80ac817, ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc}
    fingerprint: 3c1bfe073a4b
  score:
    v: 44.666666666666664
    u: 0.881917103688197
    computed: {inspection.score: 5cdea24b25a3}
    fingerprint: 7f62d0e73668
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
        hardness: 1.033
        temperature: 19
        nominal: {v: 31.25000000000023, computed: {row.hardness: d27bb26b73a4, radius: b7e0f80ac817}, fingerprint: a7f81706d418}
        rate: {v: 15.000000000000128, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 9ef8e33dddb4}
      - day: 2
        hardness: 1.025
        temperature: 20
        nominal: {v: 47.9166666666669, computed: {row.hardness: 4a35019a38ae, radius: b7e0f80ac817}, fingerprint: 2638b8ce73e7}
        rate: {v: 8.000000000000004, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 642f92539239}
      - day: 3
        hardness: 1.02
        temperature: 19
        nominal: {v: 58.333333333333336, computed: {row.hardness: e1baf5ef5d9a, radius: b7e0f80ac817}, fingerprint: adac99f8ecd2}
        rate: {v: 4.999999999999893, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 6cb30ccfc134}
      - day: 4
        hardness: 1.016
        temperature: 17.9
        nominal: {v: 66.66666666666667, computed: {row.hardness: 2b99b910dcad, radius: b7e0f80ac817}, fingerprint: 838b89e11ad3}
        rate: {v: 4.0000000000000036, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 65f12f57c6c2}
      - day: 6
        hardness: 1.013
        temperature: 17.8
        nominal: {v: 72.9166666666669, computed: {row.hardness: c70bb521ed7d, radius: b7e0f80ac817}, fingerprint: 0c5df4d435bb}
        rate: {v: 1.5000000000000568, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 5fb58f376c26}
      - day: 8
        hardness: 1.011
        temperature: 17.8
        nominal: {v: 77.08333333333357, computed: {row.hardness: 21fe04bbedf7, radius: b7e0f80ac817}, fingerprint: ea827d232895}
        rate: {v: 1.0000000000000009, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: a171cf00710f}
      - day: 11
        hardness: 1.012
        temperature: 17.6
        nominal: {v: 75.0, computed: {row.hardness: 3ede97ec1bdd, radius: b7e0f80ac817}, fingerprint: 59ab8774aa36}
        rate: {v: -0.3333333333333706, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: 8801b9b2393b}
      - day: 14
        hardness: 1.011
        temperature: 17.8
        nominal: {v: 77.08333333333357, computed: {row.hardness: 21fe04bbedf7, radius: b7e0f80ac817}, fingerprint: ea827d232895}
        rate: {v: 0.3333333333333706, computed: {ageing.day: add6d3b4afed, ageing.hardness: 28bf091dacdc, radius: b7e0f80ac817}, fingerprint: d8d585c65a0d}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 45
        brittleness: 4
      - inspector: Sam
        score: 46
        brittleness: 3
      - inspector: Mia
        score: 43
        brittleness: 4
---
# English bronze alloy

Mould annealed.
