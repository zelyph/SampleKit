---
schema_version: 1
name: farmhouse-saison
style: saison
yeast: Belle Saison
hops: [Saaz]
brewer: ana
status: ready
brewed: 2026-02-07
properties:
  og:
    v: 1.0513333333333332
    readings: [1.051, 1.052, 1.051]
    u: 3.3333333333337994e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: b5cbda3f5a19}
    fingerprint: d83cc6adf73d
  fg:
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
  grain_mass: {v: 4.6, unit: kg, fingerprint: 9861c97eacd8}
  mash_temperature: {v: 64, unit: degC}
  abv:
    v: 6.168749999999991
    u: 0.12374368670764628
    unit: "%"
    computed: {fg: e6e6d9a96acf, og: 4aad22608719}
    fingerprint: d858fa354ffd
  attenuation:
    v: 91.5584415584416
    unit: "%"
    computed: {v: {fg: e6e6d9a96acf, og: 4aad22608719}}
    fingerprint: 94032d90e50c
  efficiency:
    v: 76.08695652173897
    unit: "%"
    computed: {v: {grain_mass: 9861c97eacd8, og: 4aad22608719, volume: a9af74209b8e}}
    fingerprint: c9a56fb6db2a
  drop:
    v: 10.899999999999997
    u: 1.342882471898908
    unit: pt/d
    computed: {og: 4aad22608719, fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db}
    fingerprint: 537231fd6e8b
  score:
    v: 40.0
    u: 3.2145502536643185
    computed: {tasting.score: 9fc694a0f0a7}
    fingerprint: 30e20ba11ea9
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
        gravity: 1.037
        temperature: 27.7
        apparent: {v: 27.922077922077932, computed: {row.gravity: b564ffea14b2, og: 4aad22608719}, fingerprint: 7c0c43f4a2f3}
        rate: {v: 14.333333333333307, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: a7277a1e91ed}
      - day: 2
        gravity: 1.025
        temperature: 28
        apparent: {v: 51.298701298701374, computed: {row.gravity: 4a35019a38ae, og: 4aad22608719}, fingerprint: 25582dda7b0e}
        rate: {v: 12.00000000000001, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: 1dea52624b7f}
      - day: 3
        gravity: 1.019
        temperature: 27.4
        apparent: {v: 62.987012987013095, computed: {row.gravity: 17ed405c8a30, og: 4aad22608719}, fingerprint: 926e3d90e336}
        rate: {v: 6.0000000000000036, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: 80d515e958e4}
      - day: 4
        gravity: 1.013
        temperature: 26.7
        apparent: {v: 74.67532467532482, computed: {row.gravity: c70bb521ed7d, og: 4aad22608719}, fingerprint: ac7b6fe3735b}
        rate: {v: 6.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: f51378cc159f}
      - day: 6
        gravity: 1.009
        temperature: 26
        apparent: {v: 82.46753246753264, computed: {row.gravity: 506eb7f5cf63, og: 4aad22608719}, fingerprint: f878bb86fbf4}
        rate: {v: 2.0000000000000018, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: c6de6b019a51}
      - day: 8
        gravity: 1.006
        temperature: 25.9
        apparent: {v: 88.31168831168827, computed: {row.gravity: 0a50f56d6e4b, og: 4aad22608719}, fingerprint: 1cea6bfc56a9}
        rate: {v: 1.4999999999999458, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: 1f452d91a2c7}
      - day: 11
        gravity: 1.005
        temperature: 26.4
        apparent: {v: 90.25974025974045, computed: {row.gravity: 7c7742691baf, og: 4aad22608719}, fingerprint: cce00c16882f}
        rate: {v: 0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: d8d585c65a0d}
      - day: 14
        gravity: 1.004
        temperature: 25.8
        apparent: {v: 92.20779220779218, computed: {row.gravity: d9a798f9256b, og: 4aad22608719}, fingerprint: 55868076ffd7}
        rate: {v: 0.3333333333332966, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 45780d4419db, og: 4aad22608719}, fingerprint: 6d743c154639}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 39
        bitterness: 2
      - taster: Ivo
        score: 46
        bitterness: 2
      - taster: Mia
        score: 35
        bitterness: 2
---
# Farmhouse saison

Let free-rise to 27 °C; very dry, peppery.
