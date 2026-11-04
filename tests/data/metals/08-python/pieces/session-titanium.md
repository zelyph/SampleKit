---
schema_version: 1
name: session-titanium
tags: [dry_treated]
style: titanium
process: UT-05
carbon: [Alpha]
tester: tom
status: tempering
poured: 2026-03-28
properties:
  radius:
    v: 1.042
    readings: [1.043, 1.042, 1.041]
    u: 5.773502691896424e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 5f34a644712c}
    fingerprint: c8b9322409f2
  notch:
    v: 1.009
    readings: [1.008, 1.01, 1.009]
    u: 5.773502691896583e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 78cb57f23f22}
    fingerprint: 506eb7f5cf63
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  ore_mass: {v: 3.9, unit: kg, fingerprint: a690791b0792}
  preheat_temperature: {v: 66, unit: degC}
  modulus:
    v: 4.3312500000000185
    u: 0.15155444566228
    unit: "%"
    computed: {notch: 75b06f4e614f, radius: 4654f38baaa0}
    fingerprint: 32f32002cb73
  reduction:
    v: 78.57142857142884
    unit: "%"
    computed: {notch: 75b06f4e614f, radius: 4654f38baaa0}
    fingerprint: 260f1a3beba6
  recovery:
    v: 69.93006993006999
    unit: "%"
    computed: {ore_mass: a690791b0792, radius: 4654f38baaa0, volume: e2cdeaea1df9}
    fingerprint: 9b9751a38f8e
  drop:
    v: 9.000000000000052
    u: 1.581138830084191
    unit: pt/d
    computed: {radius: 4654f38baaa0, ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216}
    fingerprint: c97e9ce49f87
  score:
    v: 37.25
    u: 0.8539125638299665
    computed: {inspection.score: 65e262e6f21f}
    fingerprint: dc51975ac254
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
        hardness: 1.028
        temperature: 20.5
        nominal: {v: 33.333333333333336, computed: {row.hardness: ed054d5cba9b, radius: 4654f38baaa0}, fingerprint: df4c394acd92}
        rate: {v: 14.00000000000001, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: ed285ef09b13}
      - day: 2
        hardness: 1.019
        temperature: 20.7
        nominal: {v: 54.76190476190503, computed: {row.hardness: 17ed405c8a30, radius: 4654f38baaa0}, fingerprint: fe9a49c9dc8d}
        rate: {v: 9.000000000000117, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: 613c70416987}
      - day: 3
        hardness: 1.015
        temperature: 20.7
        nominal: {v: 64.28571428571455, computed: {row.hardness: bd4cedc4baa5, radius: 4654f38baaa0}, fingerprint: 9922f4db8090}
        rate: {v: 4.000000000000005, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: a32a92487a35}
      - day: 4
        hardness: 1.012
        temperature: 19.6
        nominal: {v: 71.42857142857143, computed: {row.hardness: 3ede97ec1bdd, radius: 4654f38baaa0}, fingerprint: bb2b3233343f}
        rate: {v: 2.9999999999998916, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: c57828caba55}
      - day: 6
        hardness: 1.009
        temperature: 19.3
        nominal: {v: 78.57142857142884, computed: {row.hardness: 506eb7f5cf63, radius: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 1.5000000000000568, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: 5fb58f376c26}
      - day: 8
        hardness: 1.01
        temperature: 18.8
        nominal: {v: 76.19047619047619, computed: {row.hardness: 50dc83923eca, radius: 4654f38baaa0}, fingerprint: 1cc0ae3ba989}
        rate: {v: -0.500000000000056, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: 975e062a00c1}
      - day: 11
        hardness: 1.009
        temperature: 19.1
        nominal: {v: 78.57142857142884, computed: {row.hardness: 506eb7f5cf63, radius: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 0.3333333333333706, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: d8d585c65a0d}
      - day: 14
        hardness: 1.009
        temperature: 19.3
        nominal: {v: 78.57142857142884, computed: {row.hardness: 506eb7f5cf63, radius: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: 393ccf2bb216, radius: 4654f38baaa0}, fingerprint: d3e1083d25fb}
  inspection:
    title: "Scores out of 50, brittleness from 1 to 5"
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - inspector: Lea
        score: 39
        brittleness: 5
      - inspector: Ivo
        score: 38
        brittleness: 4
      - inspector: Sam
        score: 35
        brittleness: 4
      - inspector: Mia
        score: 37
        brittleness: 4
---
# Session TITANIUM

Low hardness, same carbon as the Alpha TITANIUM.
