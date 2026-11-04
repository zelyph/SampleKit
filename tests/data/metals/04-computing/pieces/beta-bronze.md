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
---
# Beta bronze alloy

Flux and tungsten for the roughness.
