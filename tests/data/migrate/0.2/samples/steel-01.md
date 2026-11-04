---
name: steel-01
material: steel
operator: ana
width: {value: 12.5, data: [12.49, 12.51, 12.5], uncertainty: 0.01, unit: mm, symbol: w,
  precision: .3f}
thickness: {value: 2.000333333, data: [2.004, 1.996, 2.001], uncertainty: 0.004041451884,
  unit: mm, symbol: t, precision: .3f, precision_unc: .4f}
gauge_length: {value: 50, uncertainty: 0.1, unit: mm, symbol: L_0}
area: {value: 25.00416667, uncertainty: 0.0543343048, unit: mm², unit_math: mm^2,
  symbol: A}
tensile_strength: {value: 517.1138144, unit: MPa, symbol_math: '\sigma_{UTS}', precision: .0f}
curve:
  _title: Tensile curve
  _index: strain
  _columns:
  - {name: strain, unit: '%'}
  - {name: force, unit: kN, precision: .3f}
  - {name: stress, unit: MPa, symbol_math: \sigma, precision: .1f}
  _rows:
  - {strain: 0, force: 0, stress: 0}
  - {strain: 0.2, force: 9.512, stress: 380.4165972}
  - {strain: 0.5, force: 11.204, stress: 448.0853191}
  - {strain: 2, force: 12.93, stress: 517.1138144}
---

## Specimen steel-01


| Property | Value | Unit |
| :---: | :---: | :---: |
| $material$ | steel |  |
| $w$ | $12.500 \pm 0.010$ | $mm$ |
| $t$ | $2.000 \pm 0.0040$ | $mm$ |
| $A$ | $25.004166666666666 \pm 0.05433430479704101$ | $mm^2$ |
| $\sigma_{UTS}$ | $517$ | $MPa$ |
