---
schema_version: 1
name: session-ipa
tags: [dry_hopped]
style: ipa
yeast: US-05
hops: [Citra]
brewer: tom
status: conditioning
brewed: 2026-03-28
properties:
  og:
    v: 1.042
    readings: [1.043, 1.042, 1.041]
    u: 5.773502691896424e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 5f34a644712c}
    fingerprint: c8b9322409f2
  fg:
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
  grain_mass: {v: 3.9, unit: kg, fingerprint: a690791b0792}
  mash_temperature: {v: 66, unit: degC}
  abv:
    v: 4.3312500000000185
    u: 0.15155444566228
    unit: "%"
    computed: {fg: 75b06f4e614f, og: 4654f38baaa0}
    fingerprint: 32f32002cb73
  attenuation:
    v: 78.57142857142884
    unit: "%"
    computed: {v: {fg: 75b06f4e614f, og: 4654f38baaa0}}
    fingerprint: 260f1a3beba6
  efficiency:
    v: 69.93006993006999
    unit: "%"
    computed: {v: {grain_mass: a690791b0792, og: 4654f38baaa0, volume: e2cdeaea1df9}}
    fingerprint: 9b9751a38f8e
  drop:
    v: 9.000000000000052
    u: 1.581138830084191
    unit: pt/d
    computed: {og: 4654f38baaa0, fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216}
    fingerprint: c97e9ce49f87
  score:
    v: 37.25
    u: 0.8539125638299665
    computed: {tasting.score: 65e262e6f21f}
    fingerprint: dc51975ac254
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
        gravity: 1.028
        temperature: 20.5
        apparent: {v: 33.333333333333336, computed: {row.gravity: ed054d5cba9b, og: 4654f38baaa0}, fingerprint: df4c394acd92}
        rate: {v: 14.00000000000001, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: ed285ef09b13}
      - day: 2
        gravity: 1.019
        temperature: 20.7
        apparent: {v: 54.76190476190503, computed: {row.gravity: 17ed405c8a30, og: 4654f38baaa0}, fingerprint: fe9a49c9dc8d}
        rate: {v: 9.000000000000117, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: 613c70416987}
      - day: 3
        gravity: 1.015
        temperature: 20.7
        apparent: {v: 64.28571428571455, computed: {row.gravity: bd4cedc4baa5, og: 4654f38baaa0}, fingerprint: 9922f4db8090}
        rate: {v: 4.000000000000005, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: a32a92487a35}
      - day: 4
        gravity: 1.012
        temperature: 19.6
        apparent: {v: 71.42857142857143, computed: {row.gravity: 3ede97ec1bdd, og: 4654f38baaa0}, fingerprint: bb2b3233343f}
        rate: {v: 2.9999999999998916, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: c57828caba55}
      - day: 6
        gravity: 1.009
        temperature: 19.3
        apparent: {v: 78.57142857142884, computed: {row.gravity: 506eb7f5cf63, og: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 1.5000000000000568, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: 5fb58f376c26}
      - day: 8
        gravity: 1.01
        temperature: 18.8
        apparent: {v: 76.19047619047619, computed: {row.gravity: 50dc83923eca, og: 4654f38baaa0}, fingerprint: 1cc0ae3ba989}
        rate: {v: -0.500000000000056, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: 975e062a00c1}
      - day: 11
        gravity: 1.009
        temperature: 19.1
        apparent: {v: 78.57142857142884, computed: {row.gravity: 506eb7f5cf63, og: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 0.3333333333333706, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: d8d585c65a0d}
      - day: 14
        gravity: 1.009
        temperature: 19.3
        apparent: {v: 78.57142857142884, computed: {row.gravity: 506eb7f5cf63, og: 4654f38baaa0}, fingerprint: 260f1a3beba6}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 393ccf2bb216, og: 4654f38baaa0}, fingerprint: d3e1083d25fb}
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
        bitterness: 5
      - taster: Ivo
        score: 38
        bitterness: 4
      - taster: Sam
        score: 35
        bitterness: 4
      - taster: Mia
        score: 37
        bitterness: 4
---
# Session IPA

Low gravity, same hops as the Citra IPA.
