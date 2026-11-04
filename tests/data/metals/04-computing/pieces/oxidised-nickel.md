---
schema_version: 1
name: oxidised-nickel
tags: [prize]
style: nickel
process: T-04
carbon: [Fullerene]
tester: ana
status: ready
poured: 2026-01-10
properties:
  radius:
    v: 1.0583333333333331
    readings: [1.057, 1.059, 1.059]
    u: 6.666666666667228e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 80a914b37c29}
    fingerprint: 033aa51566ae
  notch:
    v: 1.0163333333333333
    readings: [1.017, 1.017, 1.015]
    u: 6.666666666666857e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 60f652146f69}
    fingerprint: 23eb579c5ea5
  volume:
    v: 19.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 5.6, unit: kg, fingerprint: deda136b1f52}
  preheat_temperature: {v: 68, unit: degC}
  modulus:
    v: 5.512499999999976
    u: 0.1636975106713652
    unit: "%"
    computed: {notch: 14ea62102a16, radius: df88d98c1cd5}
    fingerprint: dd2d31dba16d
  reduction:
    v: 71.99999999999994
    unit: "%"
    computed: {notch: 14ea62102a16, radius: df88d98c1cd5}
    fingerprint: ecc372f60b69
  recovery:
    v: 65.9496753246751
    unit: "%"
    computed: {ore_mass: deda136b1f52, radius: df88d98c1cd5, volume: 13eb4e36096f}
    fingerprint: eedb765ec345
---
# Oxidised nickel

Powdered flux in the preheat for the body. Took silver at the spring trade fair.
