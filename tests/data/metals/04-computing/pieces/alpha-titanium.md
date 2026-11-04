---
schema_version: 1
name: alpha-titanium
tags: [dry_treated]
style: titanium
process: UT-05
carbon: [Alpha, Mesophase]
tester: tom
status: ready
poured: 2026-01-24
properties:
  radius:
    v: 1.0636666666666668
    readings: [1.063, 1.065, 1.063]
    u: 6.666666666666673e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 688071fe6cd4}
    fingerprint: 8cfd5936bf29
  notch:
    v: 1.0116666666666665
    readings: [1.012, 1.011, 1.012]
    u: 3.333333333333707e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 73c71b19edcc}
    fingerprint: 94c117bbcc48
  volume:
    v: 20.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 6.1, unit: kg, fingerprint: 3241734749c6}
  preheat_temperature: {v: 65, unit: degC}
  modulus:
    v: 6.825000000000035
    u: 0.14510233457805027
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: 72fc715330d5}
    fingerprint: 547caec0d47e
  reduction:
    v: 81.67539267015736
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: 72fc715330d5}
    fingerprint: daf453927567
  recovery:
    v: 69.46810020580523
    unit: "%"
    computed: {ore_mass: 3241734749c6, radius: 72fc715330d5, volume: 34b353a78c5c}
    fingerprint: c12bf29cdebc
---
# Alpha TITANIUM

Dry treated on day 4 with 100 g of Alpha.
