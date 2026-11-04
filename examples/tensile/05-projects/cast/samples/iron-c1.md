---
schema_version: 1
name: iron-c1
material: iron
grade: EN-GJS-400
direction: L
tested: 2026-04-15
properties:
  width:
    v: 12.495000000000001
    readings: [12.488, 12.502, 12.495]
    u: 0.004041451884327449
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 1b9207397513}
    fingerprint: 7aa27e039a0e
  thickness:
    v: 1.9989999999999999
    readings: [1.992, 2.006, 1.999]
    u: 0.00404145188432732
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 2c7f94d77ba3}
    fingerprint: 76b3dad56b7e
  area:
    v: 24.977505
    u: 0.05114010257452814
    unit: mm²
    computed: {thickness: e97643226d31, width: d0bfc9d7a2dd}
    fingerprint: d80ffcf3ced1
  young_modulus:
    v: 169.8328155674476
    u: 0.8783627599796651
    unit: GPa
    computed: {curve.strain: 959e8bd4cfb5, curve.stress: 0436379f50a3}
    fingerprint: 5e4cde8a48df
  yield_strength:
    v: 276.3165968394566
    unit: MPa
    computed: {young_modulus: 5e4cde8a48df, curve.strain: 959e8bd4cfb5, curve.stress: 0436379f50a3}
    fingerprint: 9d0e58cf9891
  tensile_strength:
    v: 416.37465391359143
    unit: MPa
    computed: {curve.stress: 0436379f50a3}
    fingerprint: 30d5d4d49e9e
  elongation:
    v: 18
    unit: "%"
    computed: {curve.strain: 959e8bd4cfb5}
    fingerprint: d591766a59f7
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
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: d80ffcf3ced1}, fingerprint: d3e1083d25fb}
      - strain: 0.025
        force: 1.051
        stress: {v: 42.077861659921595, computed: {row.force: ce1d642a8d5f, area: d80ffcf3ced1}, fingerprint: cefdb9a68aa6}
      - strain: 0.05
        force: 2.121
        stress: {v: 84.9164077837238, computed: {row.force: bb02d21a83ad, area: d80ffcf3ced1}, fingerprint: 0e86e34895e1}
      - strain: 0.2
        force: 6.663
        stress: {v: 266.7600306756019, computed: {row.force: 07805484c700, area: d80ffcf3ced1}, fingerprint: 267871c19f1a}
      - strain: 0.3
        force: 6.839
        stress: {v: 273.80637097260114, computed: {row.force: 0f74d037259b, area: d80ffcf3ced1}, fingerprint: cdc82a0d50af}
      - strain: 0.4
        force: 6.939
        stress: {v: 277.809973414078, computed: {row.force: 88e69011adab, area: d80ffcf3ced1}, fingerprint: d0267053ccfe}
      - strain: 0.6
        force: 7.117
        stress: {v: 284.9363857599067, computed: {row.force: c48cf2a2a0fa, area: d80ffcf3ced1}, fingerprint: b3d22e588e60}
      - strain: 1
        force: 7.547
        stress: {v: 302.1518762582572, computed: {row.force: f28b4cdc919c, area: d80ffcf3ced1}, fingerprint: bc6e286815c6}
      - strain: 2
        force: 8.376
        stress: {v: 335.3417404981002, computed: {row.force: 7b961e7e653c, area: d80ffcf3ced1}, fingerprint: f80c9d71fd91}
      - strain: 5
        force: 9.732
        stress: {v: 389.63058960452616, computed: {row.force: 74aa50adf1b1, area: d80ffcf3ced1}, fingerprint: 73ac53e93424}
      - strain: 10
        force: 10.4
        stress: {v: 416.37465391359143, computed: {row.force: 9672e7c5932b, area: d80ffcf3ced1}, fingerprint: 30d5d4d49e9e}
      - strain: 18
        force: 9.005
        stress: {v: 360.5243998549895, computed: {row.force: 1964561066aa, area: d80ffcf3ced1}, fingerprint: 1482de8b8ac8}
---
Tested at 2 mm/min, along the rolling direction.
