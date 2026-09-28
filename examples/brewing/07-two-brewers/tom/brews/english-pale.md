---
schema_version: 1
name: english-pale
style: pale_ale
yeast: Nottingham
hops: [East Kent Goldings]
brewer: tom
status: conditioning
brewed: 2026-05-23
properties:
  og: {readings: [1.047, 1.049, 1.048]}
  fg: {readings: [1.012, 1.011, 1.012]}
  volume: {v: 20, unit: L}
  grain_mass: {v: 4.5, unit: kg}
  mash_temperature: {v: 67, unit: degC}
tables:
  fermentation:
    title: Gravity and temperature, day by day
    index: day
    columns:
      day: {unit: d}
      gravity: {}
      temperature: {unit: degC}
    rows:
      - {day: 1, gravity: 1.033, temperature: 19}
      - {day: 2, gravity: 1.025, temperature: 20}
      - {day: 3, gravity: 1.02, temperature: 19}
      - {day: 4, gravity: 1.016, temperature: 17.9}
      - {day: 6, gravity: 1.013, temperature: 17.8}
      - {day: 8, gravity: 1.011, temperature: 17.8}
      - {day: 11, gravity: 1.012, temperature: 17.6}
      - {day: 14, gravity: 1.011, temperature: 17.8}
  tasting:
    title: Scores out of 50, bitterness from 1 to 5
    index: taster
    columns:
      taster: {}
      score: {}
      bitterness: {}
    rows:
      - {taster: Lea, score: 45, bitterness: 4}
      - {taster: Sam, score: 46, bitterness: 3}
      - {taster: Mia, score: 43, bitterness: 4}
---
# English pale ale

Bottle conditioned.
