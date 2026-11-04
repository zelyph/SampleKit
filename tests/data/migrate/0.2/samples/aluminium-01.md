---
name: aluminium-01
material: aluminium
operator: tom
width: {value: 12.50333333, data: [12.52, 12.5, 12.49], uncertainty: 0.01527525232,
  unit: mm, symbol: w, precision: .3f}
thickness: {value: 2.006, data: [2.01, 2.006, 2.002], uncertainty: 0.004, unit: mm,
  symbol: t, precision: .3f, precision_unc: .4f}
gauge_length: {value: 50, uncertainty: 0.1, unit: mm, symbol: L_0}
area: {value: 25.08168667, uncertainty: 0.05865385959, unit: mm², unit_math: mm^2,
  symbol: A}
tensile_strength: {value: 317.6819847, unit: MPa, symbol_math: '\sigma_{UTS}', precision: .0f}
curve:
  _title: Tensile curve
  _index: strain
  _columns:
  - {name: strain, unit: '%'}
  - {name: force, unit: kN, precision: .3f}
  - {name: stress, unit: MPa, symbol_math: \sigma, precision: .1f}
  _rows:
  - {strain: 0, force: 0, stress: 0}
  - {strain: 0.2, force: 5.531, stress: 220.51946}
  - {strain: 0.5, force: 6.42, stress: 255.9636473}
  - {strain: 2, force: 7.968, stress: 317.6819847}
---

## Specimen aluminium-01


| Property | Value | Unit |
| :---: | :---: | :---: |
| $material$ | aluminium |  |
| $w$ | $12.503 \pm 0.015$ | $mm$ |
| $t$ | $2.006 \pm 0.0040$ | $mm$ |
| $A$ | $25.081686666666666 \pm 0.05865385958693945$ | $mm^2$ |
| $\sigma_{UTS}$ | $318$ | $MPa$ |
