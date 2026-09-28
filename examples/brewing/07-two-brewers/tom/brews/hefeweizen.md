---
schema_version: 1
name: hefeweizen
style: wheat
yeast: WB-06
hops: [Hallertau]
brewer: tom
status: ready
brewed: 2026-03-14
properties:
  og:
    v: 1.0503333333333333
    readings: [1.05, 1.05, 1.051]
    u: 3.333333333332966e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 90b06bbe1192}
    fingerprint: 161065338e7d
  fg:
    v: 1.0113333333333332
    readings: [1.011, 1.011, 1.012]
    u: 3.3333333333338916e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 49ea3c201d11}
    fingerprint: ee70e8e34b6b
  volume:
    v: 21.5
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 4.5, unit: kg, fingerprint: b9c0a17d7902}
  mash_temperature: {v: 65, unit: degC}
  abv:
    v: 5.118750000000019
    u: 0.1237436867076467
    unit: "%"
    computed: {fg: 88d0a6870033, og: 3c8f0ee1e0b4}
    fingerprint: 1066e0fd3a05
  attenuation:
    v: 77.48344370860954
    unit: "%"
    computed: {v: {fg: 88d0a6870033, og: 3c8f0ee1e0b4}}
    fingerprint: 1b1c6b0217cf
  efficiency:
    v: 78.0784030784031
    unit: "%"
    computed: {v: {grain_mass: b9c0a17d7902, og: 3c8f0ee1e0b4, volume: f43fab5bdc60}}
    fingerprint: 1be60972c1e4
  drop:
    v: 11.600000000000033
    u: 2.4535688292770392
    unit: pt/d
    computed: {og: 3c8f0ee1e0b4, fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7}
    fingerprint: f96b34bd1f00
  score:
    v: 39.25
    u: 2.212653007891959
    computed: {tasting.score: 90ca6b7f4a3f}
    fingerprint: 2a39976f937e
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
        gravity: 1.03
        temperature: 22.1
        apparent: {v: 40.39735099337744, computed: {row.gravity: f381f8eb8599, og: 3c8f0ee1e0b4}, fingerprint: c0a8f4d0baa1}
        rate: {v: 20.333333333333314, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: e1a1d8f85887}
      - day: 2
        gravity: 1.02
        temperature: 22.4
        apparent: {v: 60.26490066225163, computed: {row.gravity: e1baf5ef5d9a, og: 3c8f0ee1e0b4}, fingerprint: 0da84161fc34}
        rate: {v: 10.00000000000001, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: ccb1c6a508a3}
      - day: 3
        gravity: 1.015
        temperature: 21.8
        apparent: {v: 70.19867549668894, computed: {row.gravity: bd4cedc4baa5, og: 3c8f0ee1e0b4}, fingerprint: a83e2cfffd3c}
        rate: {v: 5.0000000000001155, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: ed75c9f642f2}
      - day: 4
        gravity: 1.013
        temperature: 20.9
        apparent: {v: 74.17218543046378, computed: {row.gravity: c70bb521ed7d, og: 3c8f0ee1e0b4}, fingerprint: 069e205b8efc}
        rate: {v: 2.0000000000000018, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: c6de6b019a51}
      - day: 6
        gravity: 1.011
        temperature: 20.9
        apparent: {v: 78.14569536423862, computed: {row.gravity: 21fe04bbedf7, og: 3c8f0ee1e0b4}, fingerprint: 187fe24f3bab}
        rate: {v: 1.0000000000000009, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: a171cf00710f}
      - day: 8
        gravity: 1.011
        temperature: 20.1
        apparent: {v: 78.14569536423862, computed: {row.gravity: 21fe04bbedf7, og: 3c8f0ee1e0b4}, fingerprint: 187fe24f3bab}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: d3e1083d25fb}
      - day: 11
        gravity: 1.01
        temperature: 21
        apparent: {v: 80.13245033112581, computed: {row.gravity: 50dc83923eca, og: 3c8f0ee1e0b4}, fingerprint: cdb77019217e}
        rate: {v: 0.3333333333332966, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: 6d743c154639}
      - day: 14
        gravity: 1.01
        temperature: 20.7
        apparent: {v: 80.13245033112581, computed: {row.gravity: 50dc83923eca, og: 3c8f0ee1e0b4}, fingerprint: cdb77019217e}
        rate: {v: 0.0, computed: {fermentation.day: add6d3b4afed, fermentation.gravity: 8e82a116f4c7, og: 3c8f0ee1e0b4}, fingerprint: d3e1083d25fb}
  tasting:
    title: "Scores out of 50, bitterness from 1 to 5"
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - taster: Lea
        score: 42
        bitterness: 1
      - taster: Mia
        score: 36
        bitterness: 1
      - taster: Sam
        score: 44
        bitterness: 1
      - taster: Noor
        score: 35
        bitterness: 2
---
# Hefeweizen

Fermented warm for banana.
