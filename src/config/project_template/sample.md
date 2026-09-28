---
schema_version: 1
name: EXAMPLE
tags: [example]
style: pale ale
brewed: 2026-09-16
properties:
  og: {readings: [1.052, 1.053, 1.052]}
  fg: {readings: [1.011, 1.012, 1.011]}
  volume: {v: 21.0, unit: L}
  grain_mass: {v: 5.2, unit: kg}
tables:
  fermentation:
    index: day
    columns:
      day: {unit: d}
      gravity: {}
      temperature: {unit: degC}
    rows:
      - {day: 1, gravity: 1.046, temperature: 19.0}
      - {day: 2, gravity: 1.034, temperature: 19.5}
      - {day: 3, gravity: 1.024, temperature: 20.0}
      - {day: 5, gravity: 1.015, temperature: 19.5}
      - {day: 7, gravity: 1.012, temperature: 19.0}
---

# EXAMPLE

Everything above the closing `---` is data SampleKit reads and writes. Everything
below it is yours: SampleKit never touches a word of it, and it is where remarks
belong — a comment inside the frontmatter is dropped by the next write.

Note that `og` and `fg` hold three hydrometer readings each rather than one
number. The model turns them into a value and an uncertainty, so correcting a
reading corrects both.

Nothing here was brewed — it is an example, so that the model has something to
run on. Delete this file once your own samples are in.
