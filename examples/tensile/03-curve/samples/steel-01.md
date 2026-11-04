---
schema_version: 1
name: steel-01
material: steel
grade: S355
direction: L
tested: 2026-03-04
properties:
  width:
    v: 12.513333333333334
    readings: [12.494, 12.528, 12.518]
    u: 0.01008849730028156
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: b1b4d9b56ab1}
    fingerprint: f099c3bf0938
  thickness:
    v: 2.0023333333333335
    readings: [1.995, 2.004, 2.008]
    u: 0.003844187531556873
    unit: mm
    statistics: {v: mean, u: standard_error}
    computed: {readings: 3160016f6c6e}
    fingerprint: 1d88e7bb89d3
  area:
    v: 25.05586444444445
    u: 0.05217296159913354
    unit: mm²
    computed: {thickness: 2b1079ab857e, width: 25920047999d}
    fingerprint: 17b7fdf04105
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
        stress: {v: 0.0, computed: {row.force: d3e1083d25fb, area: 17b7fdf04105}, fingerprint: d3e1083d25fb}
      - strain: 0.025
        force: 1.281
        stress: {v: 51.12575552283656, computed: {row.force: ac703dc2866b, area: 17b7fdf04105}, fingerprint: 04e7b66e0cb3}
      - strain: 0.05
        force: 2.585
        stress: {v: 103.1694598177459, computed: {row.force: 5c05bfe4640b, area: 17b7fdf04105}, fingerprint: bf03c8945016}
      - strain: 0.2
        force: 9.516
        stress: {v: 379.7913267410716, computed: {row.force: 3d1b6c5c1082, area: 17b7fdf04105}, fingerprint: 855a4385f8fe}
      - strain: 0.3
        force: 9.57
        stress: {v: 381.94651081463377, computed: {row.force: 849d95622ae4, area: 17b7fdf04105}, fingerprint: 98d3b3a4e410}
      - strain: 0.4
        force: 9.716
        stress: {v: 387.77348997648716, computed: {row.force: 360f5467ee11, area: 17b7fdf04105}, fingerprint: 0d0237fe75ab}
      - strain: 0.6
        force: 9.87
        stress: {v: 393.9197556677571, computed: {row.force: 74f322edc076, area: 17b7fdf04105}, fingerprint: 6e006bd86f0a}
      - strain: 1
        force: 10.128
        stress: {v: 404.21674624144316, computed: {row.force: 7f31f1ca1514, area: 17b7fdf04105}, fingerprint: 41e75c0ea0b5}
      - strain: 2
        force: 10.724
        stress: {v: 428.0035926829815, computed: {row.force: 631850524c18, area: 17b7fdf04105}, fingerprint: 94d5427b18ff}
      - strain: 5
        force: 11.802
        stress: {v: 471.0274525218713, computed: {row.force: 0bff9097ad64, area: 17b7fdf04105}, fingerprint: 03ca91456691}
      - strain: 10
        force: 12.636
        stress: {v: 504.3130732135541, computed: {row.force: 8b07e9349d05, area: 17b7fdf04105}, fingerprint: 7592370df3ce}
      - strain: 20
        force: 12.975
        stress: {v: 517.8428398975834, computed: {row.force: d9b1a96d40e9, area: 17b7fdf04105}, fingerprint: e5049fae9a08}
      - strain: 24
        force: 11.07
        stress: {v: 441.8127350802504, computed: {row.force: 97f69e6ed8c2, area: 17b7fdf04105}, fingerprint: c02e64cd3310}
---
Tested at 2 mm/min, along the rolling direction.
