---
schema_version: 1
name: english-pale
style: pale_ale
yeast: Nottingham
hops: [East Kent Goldings]
brewer: tom
status: conditioning
brewed: 2026-05-23
properties:
  og:
    v: 1.048
    readings: [1.047, 1.049, 1.048]
    u: 5.773502691895942e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 1fec585c2145}
    fingerprint: eeb1e9379427
  fg:
    v: 1.0116666666666665
    readings: [1.012, 1.011, 1.012]
    u: 3.333333333333707e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 73c71b19edcc}
    fingerprint: 94c117bbcc48
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 4.5, unit: kg, fingerprint: b9c0a17d7902}
  mash_temperature: {v: 67, unit: degC}
  abv:
    v: 4.768750000000028
    u: 0.1383496476323659
    unit: "%"
    computed: {fg: 78d6a47f1b63, og: b7e0f80ac817}
    fingerprint: 0b113903c626
  attenuation:
    v: 75.69444444444483
    unit: "%"
    computed: {v: {fg: 78d6a47f1b63, og: b7e0f80ac817}}
    fingerprint: ee3127f78b3e
  efficiency:
    v: 69.26406926406932
    unit: "%"
    computed: {v: {grain_mass: b9c0a17d7902, og: b7e0f80ac817, volume: e2cdeaea1df9}}
    fingerprint: f7e6b8f98b69
  drop:
    v: 9.200000000000006
    u: 1.6062378404209383
    unit: pt/d
    computed: {og: b7e0f80ac817, fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc}
    fingerprint: 3c1bfe073a4b
  score:
    v: 44.666666666666664
    u: 0.881917103688197
    computed: {tasting.score: 5cdea24b25a3}
    fingerprint: 7f62d0e73668
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
        gravity: 1.033
        temperature: 19
        apparent: {v: 31.25000000000023, computed: {row.gravity: d27bb26b73a4, og: b7e0f80ac817}, fingerprint: a7f81706d418}
        rate: {v: 15.000000000000128, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 9ef8e33dddb4}
      - day: 2
        gravity: 1.025
        temperature: 20
        apparent: {v: 47.9166666666669, computed: {row.gravity: 4a35019a38ae, og: b7e0f80ac817}, fingerprint: 2638b8ce73e7}
        rate: {v: 8.000000000000004, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 642f92539239}
      - day: 3
        gravity: 1.02
        temperature: 19
        apparent: {v: 58.333333333333336, computed: {row.gravity: e1baf5ef5d9a, og: b7e0f80ac817}, fingerprint: adac99f8ecd2}
        rate: {v: 4.999999999999893, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 6cb30ccfc134}
      - day: 4
        gravity: 1.016
        temperature: 17.9
        apparent: {v: 66.66666666666667, computed: {row.gravity: 2b99b910dcad, og: b7e0f80ac817}, fingerprint: 838b89e11ad3}
        rate: {v: 4.0000000000000036, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 65f12f57c6c2}
      - day: 6
        gravity: 1.013
        temperature: 17.8
        apparent: {v: 72.9166666666669, computed: {row.gravity: c70bb521ed7d, og: b7e0f80ac817}, fingerprint: 0c5df4d435bb}
        rate: {v: 1.5000000000000568, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 5fb58f376c26}
      - day: 8
        gravity: 1.011
        temperature: 17.8
        apparent: {v: 77.08333333333357, computed: {row.gravity: 21fe04bbedf7, og: b7e0f80ac817}, fingerprint: ea827d232895}
        rate: {v: 1.0000000000000009, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: a171cf00710f}
      - day: 11
        gravity: 1.012
        temperature: 17.6
        apparent: {v: 75.0, computed: {row.gravity: 3ede97ec1bdd, og: b7e0f80ac817}, fingerprint: 59ab8774aa36}
        rate: {v: -0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: 8801b9b2393b}
      - day: 14
        gravity: 1.011
        temperature: 17.8
        apparent: {v: 77.08333333333357, computed: {row.gravity: 21fe04bbedf7, og: b7e0f80ac817}, fingerprint: ea827d232895}
        rate: {v: 0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 28bf091dacdc, og: b7e0f80ac817}, fingerprint: d8d585c65a0d}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 45
        bitterness: 4
      - taster: Sam
        score: 46
        bitterness: 3
      - taster: Mia
        score: 43
        bitterness: 4
---
# English pale ale

Bottle conditioned.
