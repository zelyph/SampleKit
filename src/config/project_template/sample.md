---
schema_version: 1
name: EXAMPLE
tags: [example]
material: steel
tested: 2026-09-16
properties:
  width: {readings: [12.51, 12.52, 12.50], unit: mm}
  thickness: {readings: [2.003, 1.998, 2.001], unit: mm}
  gauge_length: {v: 50.0, unit: mm}
tables:
  curve:
    index: extension
    columns:
      extension: {unit: mm}
      force: {unit: kN}
    rows:
      - {extension: 0.0, force: 0.0}
      - {extension: 0.01, force: 1.03}
      - {extension: 0.02, force: 2.07}
      - {extension: 0.025, force: 2.57}
      - {extension: 0.05, force: 5.1}
      - {extension: 0.1, force: 8.7}
      - {extension: 0.2, force: 9.8}
      - {extension: 1.0, force: 11.2}
      - {extension: 5.0, force: 12.9}
      - {extension: 10.0, force: 12.95}
      - {extension: 12.0, force: 12.1}
---

# EXAMPLE

Everything above the closing `---` is data SampleKit reads and writes. Everything
below it is yours: SampleKit never touches a word of it, and it is where remarks
belong. A comment inside the frontmatter is dropped by the next write.

Note that `width` and `thickness` hold three micrometer readings each rather
than one number. The model turns them into a value and an uncertainty, so
correcting a reading corrects both.

Nothing here was tested: it is an example, so that the model has something to
run on. Delete this file once your own samples are in.
