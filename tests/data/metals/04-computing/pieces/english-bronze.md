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
---
# English bronze alloy

Mould annealed.
