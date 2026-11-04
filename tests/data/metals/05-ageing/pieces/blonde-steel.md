---
schema_version: 1
name: blonde-steel
style: steel
process: Brine Quench
carbon: [Hard coke]
tester: ana
status: tempering
poured: 2026-05-09
properties:
  radius:
    v: 1.0486666666666666
    readings: [1.049, 1.049, 1.048]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 04baee468af7}
    fingerprint: caf1c10818ca
  notch:
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
  ore_mass: {v: 4.3, unit: kg, fingerprint: 24be04b9eefb}
  preheat_temperature: {v: 64, unit: degC}
  modulus:
    v: 6.081250000000001
    u: 0.12374368670764242
    unit: "%"
    computed: {notch: 3070740c7734, radius: d5a5df63de5b}
    fingerprint: 98d18052d9d1
  reduction:
    v: 95.20547945205486
    unit: "%"
    computed: {notch: 3070740c7734, radius: d5a5df63de5b}
    fingerprint: c895a1dc1855
  recovery:
    v: 77.1670190274841
    unit: "%"
    computed: {ore_mass: 24be04b9eefb, radius: d5a5df63de5b, volume: a9af74209b8e}
    fingerprint: b6ad0354f7b4
  drop:
    v: 10.9
    u: 1.2124355652982226
    unit: pt/d
    computed: {radius: d5a5df63de5b, ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11}
    fingerprint: 0211268f626d
  score:
    v: 44.75
    u: 0.47871355387816905
    computed: {inspection.score: 3c1fda0da3c6}
    fingerprint: 822ae339de63
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
        hardness: 1.034
        temperature: 27.6
        nominal: {v: 30.136986301369756, computed: {row.hardness: 153bb295112c, radius: d5a5df63de5b}, fingerprint: 297d9f99d37d}
        rate: {v: 14.666666666666607, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: 4d7f18974113}
      - day: 2
        hardness: 1.023
        temperature: 28.1
        nominal: {v: 52.73972602739742, computed: {row.hardness: 44669390fc43, radius: d5a5df63de5b}, fingerprint: 5f81f46e12ed}
        rate: {v: 11.00000000000012, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: a522d271969a}
      - day: 3
        hardness: 1.016
        temperature: 27.2
        nominal: {v: 67.12328767123283, computed: {row.hardness: 2b99b910dcad, radius: d5a5df63de5b}, fingerprint: 777963ae64a6}
        rate: {v: 6.999999999999893, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: 29449efd71db}
      - day: 4
        hardness: 1.013
        temperature: 26.4
        nominal: {v: 73.2876712328769, computed: {row.hardness: c70bb521ed7d, radius: d5a5df63de5b}, fingerprint: c7c0b6a8e53b}
        rate: {v: 3.0000000000001137, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: b786283bb5ce}
      - day: 6
        hardness: 1.008
        temperature: 25.8
        nominal: {v: 83.56164383561641, computed: {row.hardness: b8414a3fafbc, radius: d5a5df63de5b}, fingerprint: 1d4503369385}
        rate: {v: 2.4999999999999467, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: 6ba27bd08e18}
      - day: 8
        hardness: 1.006
        temperature: 26.5
        nominal: {v: 87.67123287671231, computed: {row.hardness: 0a50f56d6e4b, radius: d5a5df63de5b}, fingerprint: "668254882212"}
        rate: {v: 1.0000000000000009, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: a171cf00710f}
      - day: 11
        hardness: 1.003
        temperature: 25.5
        nominal: {v: 93.83561643835638, computed: {row.hardness: d87ec315b0f4, radius: d5a5df63de5b}, fingerprint: ec72d3737e8a}
        rate: {v: 1.000000000000038, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: ee590b8bd52f}
      - day: 14
        hardness: 1.003
        temperature: 25.7
        nominal: {v: 93.83561643835638, computed: {row.hardness: d87ec315b0f4, radius: d5a5df63de5b}, fingerprint: ec72d3737e8a}
        rate: {v: 0.0, computed: {ageing.day: add6d3b4afed, ageing.hardness: c2be5aa57a11, radius: d5a5df63de5b}, fingerprint: d3e1083d25fb}
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
        brittleness: 3
      - inspector: Sam
        score: 44
        brittleness: 2
      - inspector: Ivo
        score: 45
        brittleness: 2
      - inspector: Mia
        score: 46
        brittleness: 2
---
# Blonde steel

A lighter steel for summer.
