---
schema_version: 1
name: specimen-01
material: steel
tested: 2026-11-11
properties:
  length: {readings: [20.02, 20.05, 19.98], unit: mm}
  width: {readings: [10.01, 9.98, 10.03], unit: mm}
tables:
  curve:
    index: strain
    columns:
      strain: {unit: "%"}
      force: {unit: N}
    rows:
      - strain: 0
        force: 0
      - strain: 0.1
        force: 4180
      - strain: 0.5
        force: 7210
      - strain: 2
        force: 9050
      - strain: 10
        force: 9840
---
A sample of the model template: save it in `samples/`, named as its `name`,
`specimen-01.md`. Its attributes and readings are yours to change; the area
and the table's stress are computed by `samplekit compute --write`, which
writes them here. This note, below the second `---`, is yours: SampleKit
never changes it.
