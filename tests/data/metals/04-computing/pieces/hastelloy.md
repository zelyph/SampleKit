---
schema_version: 1
name: hastelloy
style: tungsten
process: WT-06
carbon: [Hard coke]
tester: tom
status: ready
poured: 2026-03-14
properties:
  radius:
    v: 1.0503333333333333
    readings: [1.05, 1.05, 1.051]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 90b06bbe1192}
    fingerprint: 161065338e7d
  notch:
    v: 1.0113333333333332
    readings: [1.011, 1.011, 1.012]
    u: 3.3333333333338916e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 49ea3c201d11}
    fingerprint: ee70e8e34b6b
  volume:
    v: 21.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 4.5, unit: kg, fingerprint: b9c0a17d7902}
  preheat_temperature: {v: 65, unit: degC}
  modulus:
    v: 5.118750000000019
    u: 0.1237436867076467
    unit: "%"
    computed: {notch: 88d0a6870033, radius: 3c8f0ee1e0b4}
    fingerprint: 1066e0fd3a05
  reduction:
    v: 77.48344370860954
    unit: "%"
    computed: {notch: 88d0a6870033, radius: 3c8f0ee1e0b4}
    fingerprint: 1b1c6b0217cf
  recovery:
    v: 78.0784030784031
    unit: "%"
    computed: {ore_mass: b9c0a17d7902, radius: 3c8f0ee1e0b4, volume: f43fab5bdc60}
    fingerprint: 1be60972c1e4
---
# Hastelloy

Aged warm for hardness.
