---
schema_version: 1
name: copper-01
material: copper
grade: C11000
direction: T
tested: 2026-03-03
compared_with: "[[samples/steel-01]]"
tested_after: "[[aluminium-01.md]]"
raw: "[[copper-01/curve.csv]]"
method: "[[method.pdf#page=2]]"
properties:
  width:
    v: 12.504333333333333
    readings: [12.509, 12.498, 12.506]
    u: 0.0032829526005988064
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 9e56dbfb43a6}
    fingerprint: 7a62a25ea91b
  thickness:
    v: 1.9966666666666668
    readings: [1.995, 1.988, 2.007]
    u: 0.005547772325697763
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 025dfef63790}
    fingerprint: d2bd09072f20
  area:
    v: 24.966985555555556
    u: 0.06968019906785453
    unit: mm²
    computed: {thickness: 85fe0145ef79, width: 3f655f8c437f}
    fingerprint: 1ea05ae9e70c
  young_modulus:
    v: 117.19476480206947
    u: 0.8787328442349305
    unit: GPa
    computed: {curve.strain: e181921cfe11, curve.stress: eb94b4c7b933}
    fingerprint: fb4b15e43a8a
  yield_strength:
    v: 75.8774024282937
    unit: MPa
    computed: {young_modulus: fb4b15e43a8a, curve.strain: e181921cfe11, curve.stress: eb94b4c7b933}
    fingerprint: 1254ee79b35a
  tensile_strength:
    v: 221.0919691412931
    unit: MPa
    computed: {curve.stress: eb94b4c7b933}
    fingerprint: 6d22a07a43f6
  elongation:
    v: 44
    unit: "%"
    computed: {curve.strain: e181921cfe11}
    fingerprint: d542d832a46d
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
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: 1ea05ae9e70c}, fingerprint: d3e1083d25fb}
      - strain: 0.025
        force: 0.741
        stress: {v: 29.679193683640978, computed: {row.force: d8ce55f5bf3e, area: 1ea05ae9e70c}, fingerprint: efa56d7b1c14}
      - strain: 0.05
        force: 1.463
        stress: {v: 58.59738240103475, computed: {row.force: 2ad60d1537ce, area: 1ea05ae9e70c}, fingerprint: 560036d18740}
      - strain: 0.2
        force: 1.864
        stress: {v: 74.6585924781468, computed: {row.force: a8e35536d36d, area: 1ea05ae9e70c}, fingerprint: 59bce3854893}
      - strain: 0.3
        force: 1.911
        stress: {v: 76.54107844728463, computed: {row.force: 935f3a1c553d, area: 1ea05ae9e70c}, fingerprint: b4a3e8f7d6d5}
      - strain: 0.4
        force: 1.955
        stress: {v: 78.30340573754131, computed: {row.force: ab28d88298ca, area: 1ea05ae9e70c}, fingerprint: b0666ca4c26a}
      - strain: 0.6
        force: 2.051
        stress: {v: 82.14848346173771, computed: {row.force: d90d05f902ae, area: 1ea05ae9e70c}, fingerprint: 0b44add4fb41}
      - strain: 1
        force: 2.227
        stress: {v: 89.19779262276445, computed: {row.force: cac38ca2db42, area: 1ea05ae9e70c}, fingerprint: 7b1feba143b2}
      - strain: 2
        force: 2.624
        stress: {v: 105.09879112803499, computed: {row.force: ac4705742580, area: 1ea05ae9e70c}, fingerprint: 51f689e57d0a}
      - strain: 5
        force: 3.578
        stress: {v: 143.30925101223673, computed: {row.force: 9758a9d5450b, area: 1ea05ae9e70c}, fingerprint: ff3dfc109542}
      - strain: 10
        force: 4.492
        stress: {v: 179.9175951780233, computed: {row.force: 3ecdd59bca70, area: 1ea05ae9e70c}, fingerprint: 8eabdab22858}
      - strain: 20
        force: 5.279
        stress: {v: 211.43922193784172, computed: {row.force: b1ab0215a008, area: 1ea05ae9e70c}, fingerprint: 5046f02eaa49}
      - strain: 30
        force: 5.52
        stress: {v: 221.0919691412931, computed: {row.force: a9aece5149af, area: 1ea05ae9e70c}, fingerprint: 6d22a07a43f6}
      - strain: 40
        force: 5.419
        stress: {v: 217.04662695229482, computed: {row.force: aba865f20a2f, area: 1ea05ae9e70c}, fingerprint: a84c67ac54fa}
      - strain: 44
        force: 4.763
        stress: {v: 190.77192917028606, computed: {row.force: be612a2255bd, area: 1ea05ae9e70c}, fingerprint: efb2d54e6590}
---
Tested at 2 mm/min, across the rolling direction.
