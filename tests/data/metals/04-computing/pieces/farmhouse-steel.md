---
schema_version: 1
name: farmhouse-steel
style: steel
process: Brine Quench
carbon: [Pitch]
tester: ana
status: ready
poured: 2026-02-07
properties:
  radius:
    v: 1.0513333333333332
    readings: [1.051, 1.052, 1.051]
    u: 3.3333333333337994e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: b5cbda3f5a19}
    fingerprint: d83cc6adf73d
  notch:
    v: 1.0043333333333333
    readings: [1.004, 1.004, 1.005]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 84b18b802121}
    fingerprint: a6a13f83fc3a
  volume:
    v: 21
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 4.6, unit: kg, fingerprint: 9861c97eacd8}
  preheat_temperature: {v: 64, unit: degC}
  modulus:
    v: 6.168749999999991
    u: 0.12374368670764628
    unit: "%"
    computed: {notch: e6e6d9a96acf, radius: 4aad22608719}
    fingerprint: d858fa354ffd
  reduction:
    v: 91.5584415584416
    unit: "%"
    computed: {notch: e6e6d9a96acf, radius: 4aad22608719}
    fingerprint: 94032d90e50c
  recovery:
    v: 76.08695652173897
    unit: "%"
    computed: {ore_mass: 9861c97eacd8, radius: 4aad22608719, volume: a9af74209b8e}
    fingerprint: c9a56fb6db2a
---
# Farmhouse steel

Let free-rise to 27 °C; very dry, brittle.
