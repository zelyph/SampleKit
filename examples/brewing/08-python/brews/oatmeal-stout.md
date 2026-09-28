---
schema_version: 1
name: oatmeal-stout
tags: [medal]
style: stout
yeast: S-04
hops: [Fuggles]
brewer: ana
status: ready
brewed: 2026-01-10
properties:
  og:
    v: 1.0583333333333331
    readings: [1.057, 1.059, 1.059]
    u: 6.666666666667228e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 80a914b37c29}
    fingerprint: 033aa51566ae
  fg:
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
  grain_mass: {v: 5.6, unit: kg, fingerprint: deda136b1f52}
  mash_temperature: {v: 68, unit: degC}
  abv:
    v: 5.512499999999976
    u: 0.1636975106713652
    unit: "%"
    computed: {fg: 14ea62102a16, og: df88d98c1cd5}
    fingerprint: dd2d31dba16d
  attenuation:
    v: 71.99999999999994
    unit: "%"
    computed: {v: {fg: 14ea62102a16, og: df88d98c1cd5}}
    fingerprint: ecc372f60b69
  efficiency:
    v: 65.9496753246751
    unit: "%"
    computed: {v: {grain_mass: deda136b1f52, og: df88d98c1cd5, volume: 13eb4e36096f}}
    fingerprint: eedb765ec345
  drop:
    v: 11.599999999999968
    u: 2.141650453894513
    unit: pt/d
    computed: {og: df88d98c1cd5, fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76}
    fingerprint: 138bc0ced9a5
  score:
    v: 41.25
    u: 1.0307764064044151
    computed: {tasting.score: 9edae3319df8}
    fingerprint: 8e61c7a8bc3e
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
        gravity: 1.039
        temperature: 20
        apparent: {v: 33.14285714285704, computed: {row.gravity: 16007f041c0b, og: df88d98c1cd5}, fingerprint: 8f63edd4d6d8}
        rate: {v: 19.333333333333208, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: b9dccab9399c}
      - day: 2
        gravity: 1.029
        temperature: 20.8
        apparent: {v: 50.285714285714256, computed: {row.gravity: f801ead50a79, og: df88d98c1cd5}, fingerprint: 03a7b994ff69}
        rate: {v: 10.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: 96d97aa519f9}
      - day: 3
        gravity: 1.023
        temperature: 20.2
        apparent: {v: 60.571428571428584, computed: {row.gravity: 44669390fc43, og: df88d98c1cd5}, fingerprint: cdaf00ec8148}
        rate: {v: 6.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: f51378cc159f}
      - day: 4
        gravity: 1.019
        temperature: 19.7
        apparent: {v: 67.42857142857147, computed: {row.gravity: 17ed405c8a30, og: df88d98c1cd5}, fingerprint: a588a3a8119b}
        rate: {v: 4.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: 8babce7201f4}
      - day: 6
        gravity: 1.017
        temperature: 19.3
        apparent: {v: 70.85714285714292, computed: {row.gravity: ef63930342dd, og: df88d98c1cd5}, fingerprint: 334fb4a18a83}
        rate: {v: 1.0000000000000018, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: 64a71f73cb17}
      - day: 8
        gravity: 1.016
        temperature: 18.8
        apparent: {v: 72.57142857142846, computed: {row.gravity: 2b99b910dcad, og: df88d98c1cd5}, fingerprint: d25808af2dbc}
        rate: {v: 0.49999999999994493, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: 8ef55962b8ab}
      - day: 11
        gravity: 1.016
        temperature: 19.1
        apparent: {v: 72.57142857142846, computed: {row.gravity: 2b99b910dcad, og: df88d98c1cd5}, fingerprint: d25808af2dbc}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: d3e1083d25fb}
      - day: 14
        gravity: 1.015
        temperature: 18.7
        apparent: {v: 74.28571428571436, computed: {row.gravity: bd4cedc4baa5, og: df88d98c1cd5}, fingerprint: 201651b14dc9}
        rate: {v: 0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: f7ebe4098f76, og: df88d98c1cd5}, fingerprint: d8d585c65a0d}
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
      - taster: Sam
        score: 43
        bitterness: 3
      - taster: Noor
        score: 43
        bitterness: 2
      - taster: Ivo
        score: 40
        bitterness: 3
---
# Oatmeal stout

Flaked oats in the mash for the body. Took silver at the spring club contest.
