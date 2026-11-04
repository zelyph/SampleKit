---
schema_version: 1
name: aluminium-c1
material: aluminium
grade: A356-T6
direction: L
tested: 2026-04-14
properties:
  width:
    v: 12.504666666666667
    readings: [12.512, 12.497, 12.505]
    u: 0.004333333333333506
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: aa5e229faaa3}
    fingerprint: 15479971897b
  thickness:
    v: 2.0033333333333334
    readings: [2.011, 1.996, 2.003]
    u: 0.004333333333333304
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 2ab07a28dd29}
    fingerprint: 5df0809756b4
  area:
    v: 25.05101555555556
    u: 0.054877870016794686
    unit: mm²
    computed: {thickness: 052daa7897b7, width: 3ba59c0b1ae7}
    fingerprint: 6524d22da14d
  young_modulus:
    v: 71.29451482871795
    u: 0.13828188352105747
    unit: GPa
    computed: {curve.strain: 473106bbfe7e, curve.stress: f07723f32e6a}
    fingerprint: 3dfb70396c92
  yield_strength:
    v: 217.7071853366061
    unit: MPa
    computed: {young_modulus: 3dfb70396c92, curve.strain: 473106bbfe7e, curve.stress: f07723f32e6a}
    fingerprint: 111c2137ad31
  tensile_strength:
    v: 273.24241545496886
    unit: MPa
    computed: {curve.stress: f07723f32e6a}
    fingerprint: fb2c15eb4cc7
  elongation:
    v: 6
    unit: "%"
    computed: {curve.strain: 473106bbfe7e}
    fingerprint: 9ea81452acd7
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
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: 6524d22da14d}, fingerprint: d3e1083d25fb}
      - strain: 0.025
        force: 0.448
        stress: {v: 17.88350651918569, computed: {row.force: 5f1ee76d7534, area: 6524d22da14d}, fingerprint: 35f9eb8e451b}
      - strain: 0.05
        force: 0.893
        stress: {v: 35.64725741435898, computed: {row.force: fdfe0f153558, area: 6524d22da14d}, fingerprint: 97705a8c45e2}
      - strain: 0.2
        force: 3.649
        stress: {v: 145.66275734042097, computed: {row.force: a1b6fe946818, area: 6524d22da14d}, fingerprint: cda3b33d1ce5}
      - strain: 0.3
        force: 5.158
        stress: {v: 205.89983621866025, computed: {row.force: c631f4240796, area: 6524d22da14d}, fingerprint: c515aa10fce1}
      - strain: 0.4
        force: 5.291
        stress: {v: 211.20900221654352, computed: {row.force: ad3e79ac9eca, area: 6524d22da14d}, fingerprint: 7416a8e882b9}
      - strain: 0.6
        force: 5.6
        stress: {v: 223.54383148982114, computed: {row.force: deda136b1f52, area: 6524d22da14d}, fingerprint: 27723a0611de}
      - strain: 1
        force: 5.969
        stress: {v: 238.27377324334685, computed: {row.force: 5f35c0b97de3, area: 6524d22da14d}, fingerprint: f36e81d6bacc}
      - strain: 2
        force: 6.52
        stress: {v: 260.26888952029174, computed: {row.force: cfde1f23ad61, area: 6524d22da14d}, fingerprint: 3532e836ffc2}
      - strain: 5
        force: 6.845
        stress: {v: 273.24241545496886, computed: {row.force: 121d5253f4df, area: 6524d22da14d}, fingerprint: fb2c15eb4cc7}
      - strain: 6
        force: 5.854
        stress: {v: 233.68314098953803, computed: {row.force: 15031bcfef25, area: 6524d22da14d}, fingerprint: 5c04f6cad093}
---
Tested at 2 mm/min, along the rolling direction.
