---
schema_version: 1
name: english-bronze
style: bronze_alloy
process: Normalised
carbon: [Electrode Graphite]
tester: tom
status: tempering
poured: 2026-05-23
properties:
  radius: {readings: [1.047, 1.049, 1.048]}
  notch: {readings: [1.012, 1.011, 1.012]}
  volume: {v: 20, unit: L}
  ore_mass: {v: 4.5, unit: kg}
  preheat_temperature: {v: 67, unit: degC}
tables:
  ageing:
    title: Hardness and temperature, day by day
    index: day
    columns:
      day: {unit: d}
      hardness: {}
      temperature: {unit: degC}
    rows:
      - {day: 1, hardness: 1.033, temperature: 19}
      - {day: 2, hardness: 1.025, temperature: 20}
      - {day: 3, hardness: 1.02, temperature: 19}
      - {day: 4, hardness: 1.016, temperature: 17.9}
      - {day: 6, hardness: 1.013, temperature: 17.8}
      - {day: 8, hardness: 1.011, temperature: 17.8}
      - {day: 11, hardness: 1.012, temperature: 17.6}
      - {day: 14, hardness: 1.011, temperature: 17.8}
  inspection:
    title: Scores out of 50, brittleness from 1 to 5
    index: inspector
    columns:
      inspector: {}
      score: {}
      brittleness: {}
    rows:
      - {inspector: Lea, score: 45, brittleness: 4}
      - {inspector: Sam, score: 46, brittleness: 3}
      - {inspector: Mia, score: 43, brittleness: 4}
---
# English bronze alloy

Mould annealed.
