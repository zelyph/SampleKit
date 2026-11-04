---
name: copper-01
material: copper
width: {value: 12.47, uncertainty: 0.02, unit: mm, symbol: w, precision: .3f}
thickness: {value: 2.001, data: [2.003, 1.999], uncertainty: 0.003, unit: mm, symbol: t,
  precision: .3f, precision_unc: .4f}
gauge_length: {value: 50, uncertainty: 0.1, unit: mm, symbol: L_0}
area: {value: 24.95247, uncertainty: 0.05478237399, unit: mm², unit_math: mm^2, symbol: A}
tensile_strength: {value: 221.300737, unit: MPa, symbol_math: '\sigma_{UTS}', precision: .0f}
curve:
  _title: Tensile curve
  _index: strain
  _columns:
  - {name: strain, unit: '%'}
  - {name: force, unit: kN, precision: .3f}
  - {name: stress, unit: MPa, symbol_math: \sigma, precision: .1f}
  _rows:
  - {strain: 0, force: 0, stress: 0}
  - {strain: 0.5, force: 4.81, stress: 192.7664876}
  - {strain: 2, force: 5.522, stress: 221.300737}
---

## Specimen copper-01


| Property | Value | Unit |
| :---: | :---: | :---: |
| $material$ | copper |  |
| $w$ | $12.470 \pm 0.020$ | $mm$ |
| $t$ | $2.001 \pm 0.0030$ | $mm$ |
| $A$ | $24.952470000000005 \pm 0.05478237399018046$ | $mm^2$ |
| $\sigma_{UTS}$ | $221$ | $MPa$ |
