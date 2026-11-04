---
schema_version: 1
name: aluminium-01
material: aluminium
grade: 6061-T6
direction: L
tested: 2026-03-02
compared_with: "[[steel-01]]"
properties:
  width:
    v: 12.502666666666668
    readings: [12.496, 12.509, 12.503]
    u: 0.00375647588986124
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: c20377fd5af0}
    fingerprint: 5264d8249e4c
  thickness:
    v: 1.9933333333333334
    readings: [1.993, 1.98, 2.007]
    u: 0.007796010375684373
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: aff798d46ea4}
    fingerprint: 5ffe8389ed6e
  area:
    v: 24.921982222222226
    u: 0.09775811391969412
    unit: mm²
    computed: {thickness: 2497606b9235, width: c0f6752aed0d}
    fingerprint: 64305e5732d5
  young_modulus:
    v: 69.17587793087974
    u: 0.27799567339783
    unit: GPa
    computed: {curve.strain: cbb2a776806e, curve.stress: b9586cd67bef}
    fingerprint: e63161690d38
  yield_strength:
    v: 285.0245062817296
    unit: MPa
    computed: {young_modulus: e63161690d38, curve.strain: cbb2a776806e, curve.stress: b9586cd67bef}
    fingerprint: 2ae131461142
  tensile_strength:
    v: 318.0726127367075
    unit: MPa
    computed: {curve.stress: b9586cd67bef}
    fingerprint: 33de2eaa0626
  elongation:
    v: 12
    unit: "%"
    computed: {curve.strain: cbb2a776806e}
    fingerprint: eb6217ea1354
tables:
  curve:
    index: strain
    columns:
      strain: {unit: "%"}
      force: {unit: kN}
      stress: {unit: MPa}
    rows:
      - strain: 0
        force: 0.0
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: 64305e5732d5}, fingerprint: d3e1083d25fb}
      - strain: 0.025
        force: 0.434
        stress: {v: 17.41434514037228, computed: {row.force: d1a328fbcbc4, area: 64305e5732d5}, fingerprint: 6b2d484f5be8}
      - strain: 0.05
        force: 0.862
        stress: {v: 34.587938965439875, computed: {row.force: 2605851af0d9, area: 64305e5732d5}, fingerprint: 19cdf4176f25}
      - strain: 0.2
        force: 3.402
        stress: {v: 136.5059957777569, computed: {row.force: c3932f348df8, area: 64305e5732d5}, fingerprint: d9af79c981a7}
      - strain: 0.3
        force: 5.005
        stress: {v: 200.8267221833255, computed: {row.force: 9d84e43daf4d, area: 64305e5732d5}, fingerprint: 87b92bf49ba0}
      - strain: 0.4
        force: 6.954
        stress: {v: 279.03077443813095, computed: {row.force: 24e13a446136, area: 64305e5732d5}, fingerprint: ce47a4d69926}
      - strain: 0.6
        force: 7.101
        stress: {v: 284.9291816630958, computed: {row.force: 4bf9aca3f661, area: 64305e5732d5}, fingerprint: 98f9fedb169a}
      - strain: 1
        force: 7.18
        stress: {v: 288.09907398127416, computed: {row.force: e8da3824c94c, area: 64305e5732d5}, fingerprint: 67cead1e84ea}
      - strain: 2
        force: 7.495
        stress: {v: 300.73851803477015, computed: {row.force: bcc7e623413b, area: 64305e5732d5}, fingerprint: 03f28cf27f38}
      - strain: 5
        force: 7.761
        stress: {v: 311.41182634661124, computed: {row.force: 31a474564dd5, area: 64305e5732d5}, fingerprint: 2ff8a1a46956}
      - strain: 10
        force: 7.927
        stress: {v: 318.0726127367075, computed: {row.force: fbc0851547b1, area: 64305e5732d5}, fingerprint: 33de2eaa0626}
      - strain: 12
        force: 6.703
        stress: {v: 268.9593444145516, computed: {row.force: 00ef8c31716e, area: 64305e5732d5}, fingerprint: 11df87be196b}
---
Tested at 2 mm/min, along the rolling direction.
