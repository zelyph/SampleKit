---
schema_version: 1
name: hazy-pale
tags: [dry_hopped]
style: pale_ale
yeast: Verdant
hops: [Mosaic, Galaxy]
brewer: ana
status: ready
brewed: 2026-03-07
properties:
  og:
    v: 1.0539999999999998
    readings: [1.055, 1.053, 1.054]
    u: 5.773502691895942e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 29404fdd8b4a}
    fingerprint: 91a5f93f346f
  fg:
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
  grain_mass: {v: 5, unit: kg, fingerprint: ff7f3288e42d}
  mash_temperature: {v: 66, unit: degC}
  abv:
    v: 5.4687499999999805
    u: 0.1383496476323628
    unit: "%"
    computed: {fg: b8be79bd79f7, og: 76b4b198c9ed}
    fingerprint: 23e690bb67dc
  attenuation:
    v: 77.16049382716047
    unit: "%"
    computed: {v: {fg: b8be79bd79f7, og: 76b4b198c9ed}}
    fingerprint: 3259683d3c24
  efficiency:
    v: 70.12987012986991
    unit: "%"
    computed: {v: {grain_mass: ff7f3288e42d, og: 76b4b198c9ed, volume: e2cdeaea1df9}}
    fingerprint: f3c7d0072a79
  drop:
    v: 10.599999999999943
    u: 1.5874507866387193
    unit: pt/d
    computed: {og: 76b4b198c9ed, fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066}
    fingerprint: a394333c53ef
  score:
    v: 40.75
    u: 2.780137886268713
    computed: {tasting.score: b111d483d8ee}
    fingerprint: 8d408576c8a4
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
        gravity: 1.038
        temperature: 21.7
        apparent: {v: 29.629629629629342, computed: {row.gravity: fb88df114fc9, og: 76b4b198c9ed}, fingerprint: d83bb4f4a532}
        rate: {v: 15.999999999999794, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: f06d9b4526f2}
      - day: 2
        gravity: 1.028
        temperature: 22.6
        apparent: {v: 48.14814814814793, computed: {row.gravity: ed054d5cba9b, og: 76b4b198c9ed}, fingerprint: 29a1db375355}
        rate: {v: 10.00000000000001, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: ccb1c6a508a3}
      - day: 3
        gravity: 1.022
        temperature: 22
        apparent: {v: 59.25925925925909, computed: {row.gravity: e4effdb21f25, og: 76b4b198c9ed}, fingerprint: 8c0567fa8076}
        rate: {v: 6.0000000000000036, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: 80d515e958e4}
      - day: 4
        gravity: 1.019
        temperature: 20.6
        apparent: {v: 64.81481481481488, computed: {row.gravity: 17ed405c8a30, og: 76b4b198c9ed}, fingerprint: 6938fc0739a2}
        rate: {v: 3.0000000000001137, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: b786283bb5ce}
      - day: 6
        gravity: 1.014
        temperature: 19.9
        apparent: {v: 74.07407407407396, computed: {row.gravity: ddc8d2b525d6, og: 76b4b198c9ed}, fingerprint: 38ca225b5f1d}
        rate: {v: 2.4999999999999476, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: fe294989d129}
      - day: 8
        gravity: 1.014
        temperature: 19.9
        apparent: {v: 74.07407407407396, computed: {row.gravity: ddc8d2b525d6, og: 76b4b198c9ed}, fingerprint: 38ca225b5f1d}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: d3e1083d25fb}
      - day: 11
        gravity: 1.012
        temperature: 20.1
        apparent: {v: 77.77777777777769, computed: {row.gravity: 3ede97ec1bdd, og: 76b4b198c9ed}, fingerprint: 01579f4a2324}
        rate: {v: 0.6666666666666673, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: 9cb86828eb5c}
      - day: 14
        gravity: 1.012
        temperature: 20.1
        apparent: {v: 77.77777777777769, computed: {row.gravity: 3ede97ec1bdd, og: 76b4b198c9ed}, fingerprint: 01579f4a2324}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 639d0e8b6066, og: 76b4b198c9ed}, fingerprint: d3e1083d25fb}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 37
        bitterness: 4
      - taster: Noor
        score: 35
        bitterness: 3
      - taster: Sam
        score: 46
        bitterness: 4
      - taster: Mia
        score: 45
        bitterness: 3
---
# Hazy pale ale

Oats and wheat for the haze.
