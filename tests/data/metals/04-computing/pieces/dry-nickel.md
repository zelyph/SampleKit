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
  ore_mass: {v: 4.3, unit: kg, fingerprint: 7bbe214853ed}
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
---
# Dry nickel

A film on the surface on day 10: pitted, kept to see.
