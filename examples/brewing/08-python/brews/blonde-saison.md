---
schema_version: 1
name: blonde-saison
style: saison
yeast: Belle Saison
hops: [Hallertau]
brewer: ana
status: conditioning
brewed: 2026-05-09
properties:
  og:
    v: 1.0486666666666666
    readings: [1.049, 1.049, 1.048]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 04baee468af7}
    fingerprint: caf1c10818ca
  fg:
    v: 1.0023333333333333
    readings: [1.002, 1.003, 1.002]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: df5df798000b}
    fingerprint: 319efb14cc31
  volume:
    v: 21
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 4.3, unit: kg, fingerprint: 24be04b9eefb}
  mash_temperature: {v: 64, unit: degC}
  abv:
    v: 6.081250000000001
    u: 0.12374368670764242
    unit: "%"
    computed: {fg: 3070740c7734, og: d5a5df63de5b}
    fingerprint: 98d18052d9d1
  attenuation:
    v: 95.20547945205486
    unit: "%"
    computed: {v: {fg: 3070740c7734, og: d5a5df63de5b}}
    fingerprint: c895a1dc1855
  efficiency:
    v: 77.1670190274841
    unit: "%"
    computed: {v: {grain_mass: 24be04b9eefb, og: d5a5df63de5b, volume: a9af74209b8e}}
    fingerprint: b6ad0354f7b4
  drop:
    v: 10.9
    u: 1.2124355652982226
    unit: pt/d
    computed: {og: d5a5df63de5b, fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11}
    fingerprint: 0211268f626d
  score:
    v: 44.75
    u: 0.47871355387816905
    computed: {tasting.score: 3c1fda0da3c6}
    fingerprint: 822ae339de63
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
        gravity: 1.034
        temperature: 27.6
        apparent: {v: 30.136986301369756, computed: {row.gravity: 153bb295112c, og: d5a5df63de5b}, fingerprint: 297d9f99d37d}
        rate: {v: 14.666666666666607, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: 4d7f18974113}
      - day: 2
        gravity: 1.023
        temperature: 28.1
        apparent: {v: 52.73972602739742, computed: {row.gravity: 44669390fc43, og: d5a5df63de5b}, fingerprint: 5f81f46e12ed}
        rate: {v: 11.00000000000012, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: a522d271969a}
      - day: 3
        gravity: 1.016
        temperature: 27.2
        apparent: {v: 67.12328767123283, computed: {row.gravity: 2b99b910dcad, og: d5a5df63de5b}, fingerprint: 777963ae64a6}
        rate: {v: 6.999999999999893, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: 29449efd71db}
      - day: 4
        gravity: 1.013
        temperature: 26.4
        apparent: {v: 73.2876712328769, computed: {row.gravity: c70bb521ed7d, og: d5a5df63de5b}, fingerprint: c7c0b6a8e53b}
        rate: {v: 3.0000000000001137, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: b786283bb5ce}
      - day: 6
        gravity: 1.008
        temperature: 25.8
        apparent: {v: 83.56164383561641, computed: {row.gravity: b8414a3fafbc, og: d5a5df63de5b}, fingerprint: 1d4503369385}
        rate: {v: 2.4999999999999467, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: 6ba27bd08e18}
      - day: 8
        gravity: 1.006
        temperature: 26.5
        apparent: {v: 87.67123287671231, computed: {row.gravity: 0a50f56d6e4b, og: d5a5df63de5b}, fingerprint: "668254882212"}
        rate: {v: 1.0000000000000009, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: a171cf00710f}
      - day: 11
        gravity: 1.003
        temperature: 25.5
        apparent: {v: 93.83561643835638, computed: {row.gravity: d87ec315b0f4, og: d5a5df63de5b}, fingerprint: ec72d3737e8a}
        rate: {v: 1.000000000000038, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: ee590b8bd52f}
      - day: 14
        gravity: 1.003
        temperature: 25.7
        apparent: {v: 93.83561643835638, computed: {row.gravity: d87ec315b0f4, og: d5a5df63de5b}, fingerprint: ec72d3737e8a}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: c2be5aa57a11, og: d5a5df63de5b}, fingerprint: d3e1083d25fb}
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
        bitterness: 3
      - taster: Sam
        score: 44
        bitterness: 2
      - taster: Ivo
        score: 45
        bitterness: 2
      - taster: Mia
        score: 46
        bitterness: 2
---
# Blonde saison

A lighter saison for summer.
