---
schema_version: 1
name: alpha-titanium
tags: [dry_treated]
style: titanium
process: UT-05
carbon: [Alpha, Mesophase]
tester: tom
status: ready
poured: 2026-01-24
properties:
  radius:
    v: 1.0636666666666668
    readings: [1.063, 1.065, 1.063]
    u: 6.666666666666673e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 688071fe6cd4}
    fingerprint: 8cfd5936bf29
  notch:
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
  ore_mass: {v: 6.1, unit: kg, fingerprint: 3241734749c6}
  preheat_temperature: {v: 65, unit: degC}
  modulus:
    v: 6.825000000000035
    u: 0.14510233457805027
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: 72fc715330d5}
    fingerprint: 547caec0d47e
  reduction:
    v: 81.67539267015736
    unit: "%"
    computed: {notch: 78d6a47f1b63, radius: 72fc715330d5}
    fingerprint: daf453927567
  recovery:
    v: 69.46810020580523
    unit: "%"
    computed: {ore_mass: 3241734749c6, radius: 72fc715330d5, volume: 34b353a78c5c}
    fingerprint: c12bf29cdebc
  drop:
    v: 14.000000000000057
    u: 2.366431913239872
    unit: pt/d
    computed: {radius: 72fc715330d5, ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2}
    fingerprint: 6ec164e703d6
  score:
    v: 42.666666666666664
    u: 0.881917103688197
    computed: {inspection.score: 41024f148078}
    fingerprint: d8fd553a89f3
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
        hardness: 1.041
        temperature: 20.7
        nominal: {v: 35.60209424083791, computed: {row.hardness: f830b9b209a2, radius: 72fc715330d5}, fingerprint: fb8a99fb05fa}
        rate: {v: 22.666666666666828, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: d0cc232fd2b6}
      - day: 2
        hardness: 1.029
        temperature: 20.7
        nominal: {v: 54.45026178010491, computed: {row.hardness: f801ead50a79, radius: 72fc715330d5}, fingerprint: 44858af791ee}
        rate: {v: 12.000000000000014, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: e8232e353d90}
      - day: 3
        hardness: 1.021
        temperature: 20.5
        nominal: {v: 67.01570680628292, computed: {row.hardness: c4581bd9b38c, radius: 72fc715330d5}, fingerprint: 35735d93a964}
        rate: {v: 8.000000000000007, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: 01bbd5dbd1cb}
      - day: 4
        hardness: 1.017
        temperature: 19.9
        nominal: {v: 73.29842931937192, computed: {row.hardness: ef63930342dd, radius: 72fc715330d5}, fingerprint: e18d401db1da}
        rate: {v: 4.0000000000000036, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: 65f12f57c6c2}
      - day: 6
        hardness: 1.014
        temperature: 19.1
        nominal: {v: 78.01047120418849, computed: {row.hardness: ddc8d2b525d6, radius: 72fc715330d5}, fingerprint: 819e7daf1374}
        rate: {v: 1.4999999999999458, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: 1f452d91a2c7}
      - day: 8
        hardness: 1.011
        temperature: 18.9
        nominal: {v: 82.72251308900542, computed: {row.hardness: 21fe04bbedf7, radius: 72fc715330d5}, fingerprint: 796d071a03ba}
        rate: {v: 1.5000000000000568, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: 5fb58f376c26}
      - day: 11
        hardness: 1.012
        temperature: 19.2
        nominal: {v: 81.15183246073299, computed: {row.hardness: 3ede97ec1bdd, radius: 72fc715330d5}, fingerprint: 5b78ad100a27}
        rate: {v: -0.3333333333333706, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: 8801b9b2393b}
      - day: 14
        hardness: 1.012
        temperature: 19
        nominal: {v: 81.15183246073299, computed: {row.hardness: 3ede97ec1bdd, radius: 72fc715330d5}, fingerprint: 5b78ad100a27}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: 20506da427d2, radius: 72fc715330d5}, fingerprint: d3e1083d25fb}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 44
        brittleness: 4
      - inspector: Mia
        score: 43
        brittleness: 4
      - inspector: Ivo
        score: 41
        brittleness: 4
---
# Alpha TITANIUM

Dry treated on day 4 with 100 g of Alpha.
