---
schema_version: 1
name: gamma-zinc
style: zinc
process: T-04
carbon: [Fullerene]
tester: ana
status: ageing
poured: 2026-06-06
properties:
  radius:
    v: 1.0663333333333334
    readings: [1.066, 1.067, 1.066]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 9ad75c355ccd}
    fingerprint: eb8b1e90e9d7
  notch: {fingerprint: 44136fa355b3}
  volume:
    v: 19.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 6.6, unit: kg, fingerprint: 0f8baf0b4342}
  preheat_temperature: {v: 68, unit: degC}
  modulus: {unit: "%"}
  reduction: {unit: "%"}
  recovery:
    v: 63.63144431326252
    unit: "%"
    computed: {ore_mass: 0f8baf0b4342, radius: e27e7af8c5f1, volume: 13eb4e36096f}
    fingerprint: 58d70ee2e848
  drop:
    v: 13.000000000000034
    u: 2.7507574714370127
    unit: pt/d
    computed: {radius: e27e7af8c5f1, ageing.day: d4b98a401692, ageing.hardness: 48e447b2b80a}
    fingerprint: d76c0b41d70a
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
        hardness: 1.044
        temperature: 20.1
        nominal: {v: 33.66834170854268, computed: {row.hardness: 53f6e12b9563, radius: e27e7af8c5f1}, fingerprint: 4c4e3d0a2b53}
        rate: {v: 22.333333333333314, computed: {ageing.day: d4b98a401692, ageing.hardness: 48e447b2b80a, radius: e27e7af8c5f1}, fingerprint: debd6122b19a}
      - day: 2
        hardness: 1.032
        temperature: 20.6
        nominal: {v: 51.75879396984922, computed: {row.hardness: 6174cde63ce7, radius: e27e7af8c5f1}, fingerprint: d4311b4b438d}
        rate: {v: 12.000000000000014, computed: {ageing.day: d4b98a401692, ageing.hardness: 48e447b2b80a, radius: e27e7af8c5f1}, fingerprint: e8232e353d90}
      - day: 3
        hardness: 1.027
        temperature: 20.3
        nominal: {v: 59.29648241206045, computed: {row.hardness: 55e9d001cd92, radius: e27e7af8c5f1}, fingerprint: a3ad4f4f1429}
        rate: {v: 5.000000000000114, computed: {ageing.day: d4b98a401692, ageing.hardness: 48e447b2b80a, radius: e27e7af8c5f1}, fingerprint: 21e1a69c7423}
      - day: 4
        hardness: 1.024
        temperature: 20
        nominal: {v: 63.81909547738692, computed: {row.hardness: e26a346dd09a, radius: e27e7af8c5f1}, fingerprint: 5321e919815b}
        rate: {v: 2.9999999999998934, computed: {ageing.day: d4b98a401692, ageing.hardness: 48e447b2b80a, radius: e27e7af8c5f1}, fingerprint: c98e5a61a5de}
---
# Gamma zinc

A third of the diameter gamma over beech. Still ageing: no final hardness yet.
