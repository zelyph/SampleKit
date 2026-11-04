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
---
# Robust zinc

Cast twenty for the neighbours.
