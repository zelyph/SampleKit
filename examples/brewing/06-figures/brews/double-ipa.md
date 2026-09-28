---
schema_version: 1
name: double-ipa
tags: [medal, dry_hopped]
style: ipa
yeast: US-05
hops: [Simcoe, Citra]
brewer: tom
status: ready
brewed: 2026-04-25
properties:
  og:
    v: 1.0783333333333331
    readings: [1.077, 1.079, 1.079]
    u: 6.666666666667228e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 25966adcab8a}
    fingerprint: b82a60031e3d
  fg:
    v: 1.014
    readings: [1.014, 1.015, 1.013]
    u: 5.773502691896424e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: b67bee2c3f8d}
    fingerprint: ddc8d2b525d6
  volume:
    v: 19
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 7.8, unit: kg, fingerprint: 2f2a93f5ca16}
  mash_temperature: {v: 64, unit: degC}
  abv:
    v: 8.443749999999973
    u: 0.15774286830155468
    unit: "%"
    computed: {fg: 5858fc435f7f, og: 4661dd4ab6e1}
    fingerprint: 7348900a7073
  attenuation:
    v: 82.12765957446803
    unit: "%"
    computed: {v: {fg: 5858fc435f7f, og: 4661dd4ab6e1}}
    fingerprint: aa6e43d59e49
  efficiency:
    v: 61.95193695193681
    unit: "%"
    computed: {v: {grain_mass: 2f2a93f5ca16, og: 4661dd4ab6e1, volume: bfd1de0af92e}}
    fingerprint: 57eb8f28712b
  drop:
    v: 16.99999999999997
    u: 2.750757471436979
    unit: pt/d
    computed: {og: 4661dd4ab6e1, fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725}
    fingerprint: b62a878f0895
  score:
    v: 40.333333333333336
    u: 2.96273147243853
    computed: {tasting.score: 9dd21caefce7}
    fingerprint: 0fa0e9ca8c4e
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
        gravity: 1.052
        temperature: 20.5
        apparent: {v: 33.61702127659552, computed: {row.gravity: 98c0480c1eee, og: 4661dd4ab6e1}, fingerprint: b6bb6f7758dc}
        rate: {v: 26.3333333333331, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: e824a4726fd0}
      - day: 2
        gravity: 1.036
        temperature: 21.1
        apparent: {v: 54.04255319148921, computed: {row.gravity: 892e83fdac90, og: 4661dd4ab6e1}, fingerprint: c6d692454214}
        rate: {v: 16.000000000000014, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: ae3a446ceb56}
      - day: 3
        gravity: 1.027
        temperature: 20.4
        apparent: {v: 65.53191489361704, computed: {row.gravity: 55e9d001cd92, og: 4661dd4ab6e1}, fingerprint: aa2920075957}
        rate: {v: 9.000000000000114, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: 450bef271585}
      - day: 4
        gravity: 1.021
        temperature: 19.4
        apparent: {v: 73.19148936170218, computed: {row.gravity: c4581bd9b38c, og: 4661dd4ab6e1}, fingerprint: a95325b26d6d}
        rate: {v: 6.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: f51378cc159f}
      - day: 6
        gravity: 1.017
        temperature: 19
        apparent: {v: 78.2978723404256, computed: {row.gravity: ef63930342dd, og: 4661dd4ab6e1}, fingerprint: 6f15bc62188a}
        rate: {v: 2.0000000000000018, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: c6de6b019a51}
      - day: 8
        gravity: 1.014
        temperature: 18.8
        apparent: {v: 82.12765957446803, computed: {row.gravity: ddc8d2b525d6, og: 4661dd4ab6e1}, fingerprint: aa6e43d59e49}
        rate: {v: 1.4999999999999458, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: 1f452d91a2c7}
      - day: 11
        gravity: 1.013
        temperature: 19.1
        apparent: {v: 83.40425531914903, computed: {row.gravity: c70bb521ed7d, og: 4661dd4ab6e1}, fingerprint: 253b6465fcc5}
        rate: {v: 0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: d8d585c65a0d}
      - day: 14
        gravity: 1.014
        temperature: 19.4
        apparent: {v: 82.12765957446803, computed: {row.gravity: ddc8d2b525d6, og: 4661dd4ab6e1}, fingerprint: aa6e43d59e49}
        rate: {v: -0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 1c43e2833725, og: 4661dd4ab6e1}, fingerprint: 8801b9b2393b}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 36
        bitterness: 4
      - taster: Noor
        score: 39
        bitterness: 4
      - taster: Sam
        score: 46
        bitterness: 4
---
# Double IPA

Gold at the spring club contest.
