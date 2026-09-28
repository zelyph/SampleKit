---
schema_version: 1
name: citra-ipa
tags: [dry_hopped]
style: ipa
yeast: US-05
hops: [Citra, Mosaic]
brewer: tom
status: ready
brewed: 2026-01-24
properties:
  og:
    v: 1.0636666666666668
    readings: [1.063, 1.065, 1.063]
    u: 6.666666666666673e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 688071fe6cd4}
    fingerprint: 8cfd5936bf29
  fg:
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
  grain_mass: {v: 6.1, unit: kg, fingerprint: 3241734749c6}
  mash_temperature: {v: 65, unit: degC}
  abv:
    v: 6.825000000000035
    u: 0.14510233457805027
    unit: "%"
    computed: {fg: 78d6a47f1b63, og: 72fc715330d5}
    fingerprint: 547caec0d47e
  attenuation:
    v: 81.67539267015736
    unit: "%"
    computed: {v: {fg: 78d6a47f1b63, og: 72fc715330d5}}
    fingerprint: daf453927567
  efficiency:
    v: 69.46810020580523
    unit: "%"
    computed: {v: {grain_mass: 3241734749c6, og: 72fc715330d5, volume: 34b353a78c5c}}
    fingerprint: c12bf29cdebc
  drop:
    v: 14.000000000000057
    u: 2.366431913239872
    unit: pt/d
    computed: {og: 72fc715330d5, fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2}
    fingerprint: 6ec164e703d6
  score:
    v: 42.666666666666664
    u: 0.881917103688197
    computed: {tasting.score: 41024f148078}
    fingerprint: d8fd553a89f3
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
        gravity: 1.041
        temperature: 20.7
        apparent: {v: 35.60209424083791, computed: {row.gravity: f830b9b209a2, og: 72fc715330d5}, fingerprint: fb8a99fb05fa}
        rate: {v: 22.666666666666828, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: d0cc232fd2b6}
      - day: 2
        gravity: 1.029
        temperature: 20.7
        apparent: {v: 54.45026178010491, computed: {row.gravity: f801ead50a79, og: 72fc715330d5}, fingerprint: 44858af791ee}
        rate: {v: 12.000000000000014, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: e8232e353d90}
      - day: 3
        gravity: 1.021
        temperature: 20.5
        apparent: {v: 67.01570680628292, computed: {row.gravity: c4581bd9b38c, og: 72fc715330d5}, fingerprint: 35735d93a964}
        rate: {v: 8.000000000000007, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: 01bbd5dbd1cb}
      - day: 4
        gravity: 1.017
        temperature: 19.9
        apparent: {v: 73.29842931937192, computed: {row.gravity: ef63930342dd, og: 72fc715330d5}, fingerprint: e18d401db1da}
        rate: {v: 4.0000000000000036, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: 65f12f57c6c2}
      - day: 6
        gravity: 1.014
        temperature: 19.1
        apparent: {v: 78.01047120418849, computed: {row.gravity: ddc8d2b525d6, og: 72fc715330d5}, fingerprint: 819e7daf1374}
        rate: {v: 1.4999999999999458, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: 1f452d91a2c7}
      - day: 8
        gravity: 1.011
        temperature: 18.9
        apparent: {v: 82.72251308900542, computed: {row.gravity: 21fe04bbedf7, og: 72fc715330d5}, fingerprint: 796d071a03ba}
        rate: {v: 1.5000000000000568, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: 5fb58f376c26}
      - day: 11
        gravity: 1.012
        temperature: 19.2
        apparent: {v: 81.15183246073299, computed: {row.gravity: 3ede97ec1bdd, og: 72fc715330d5}, fingerprint: 5b78ad100a27}
        rate: {v: -0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: 8801b9b2393b}
      - day: 14
        gravity: 1.012
        temperature: 19
        apparent: {v: 81.15183246073299, computed: {row.gravity: 3ede97ec1bdd, og: 72fc715330d5}, fingerprint: 5b78ad100a27}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 20506da427d2, og: 72fc715330d5}, fingerprint: d3e1083d25fb}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 44
        bitterness: 4
      - taster: Mia
        score: 43
        bitterness: 4
      - taster: Ivo
        score: 41
        bitterness: 4
---
# Citra IPA

Dry hopped on day 4 with 100 g of Citra.
