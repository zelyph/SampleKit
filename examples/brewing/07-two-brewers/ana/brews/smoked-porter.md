---
schema_version: 1
name: smoked-porter
style: porter
yeast: S-04
hops: [Fuggles]
brewer: ana
status: fermenting
brewed: 2026-06-06
properties:
  og:
    v: 1.0663333333333334
    readings: [1.066, 1.067, 1.066]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 9ad75c355ccd}
    fingerprint: eb8b1e90e9d7
  fg: {statistics: {v: mean, u: standard_error}, fingerprint: 44136fa355b3}
  volume:
    v: 19.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 6.6, unit: kg, fingerprint: 0f8baf0b4342}
  mash_temperature: {v: 68, unit: degC}
  abv: {unit: "%"}
  attenuation: {unit: "%"}
  efficiency:
    v: 63.63144431326252
    unit: "%"
    computed: {v: {grain_mass: 0f8baf0b4342, og: e27e7af8c5f1, volume: 13eb4e36096f}}
    fingerprint: 58d70ee2e848
  drop:
    v: 13.000000000000034
    u: 2.7507574714370127
    unit: pt/d
    computed: {og: e27e7af8c5f1, fermentation.day: d4b98a401692, fermentation.gravity: 48e447b2b80a}
    fingerprint: d76c0b41d70a
tables:
  fermentation:
    title: "Gravity and temperature, day by day"
    index: day
    columns:
      day: {unit: d}
      gravity: {}
      temperature: {unit: degC}
      apparent: {unit: "%"}
      rate: {unit: pt/d}
    rows:
      - day: 1
        gravity: 1.044
        temperature: 20.1
        apparent: {v: 33.66834170854268, computed: {row.gravity: 53f6e12b9563, og: e27e7af8c5f1}, fingerprint: 4c4e3d0a2b53}
        rate: {v: 22.333333333333314, computed: {fermentation.day: d4b98a401692, fermentation.gravity: 48e447b2b80a, og: e27e7af8c5f1}, fingerprint: debd6122b19a}
      - day: 2
        gravity: 1.032
        temperature: 20.6
        apparent: {v: 51.75879396984922, computed: {row.gravity: 6174cde63ce7, og: e27e7af8c5f1}, fingerprint: d4311b4b438d}
        rate: {v: 12.000000000000014, computed: {fermentation.day: d4b98a401692, fermentation.gravity: 48e447b2b80a, og: e27e7af8c5f1}, fingerprint: e8232e353d90}
      - day: 3
        gravity: 1.027
        temperature: 20.3
        apparent: {v: 59.29648241206045, computed: {row.gravity: 55e9d001cd92, og: e27e7af8c5f1}, fingerprint: a3ad4f4f1429}
        rate: {v: 5.000000000000114, computed: {fermentation.day: d4b98a401692, fermentation.gravity: 48e447b2b80a, og: e27e7af8c5f1}, fingerprint: 21e1a69c7423}
      - day: 4
        gravity: 1.024
        temperature: 20
        apparent: {v: 63.81909547738692, computed: {row.gravity: e26a346dd09a, og: e27e7af8c5f1}, fingerprint: 5321e919815b}
        rate: {v: 2.9999999999998934, computed: {fermentation.day: d4b98a401692, fermentation.gravity: 48e447b2b80a, og: e27e7af8c5f1}, fingerprint: c98e5a61a5de}
---
# Smoked porter

A third of the malt smoked over beech. Still fermenting: no final gravity yet.
