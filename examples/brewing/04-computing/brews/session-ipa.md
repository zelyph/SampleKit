---
schema_version: 1
name: session-ipa
tags: [dry_hopped]
style: ipa
yeast: US-05
hops: [Citra]
brewer: tom
status: conditioning
brewed: 2026-03-28
properties:
  og:
    v: 1.042
    readings: [1.043, 1.042, 1.041]
    u: 5.773502691896424e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 5f34a644712c}
    fingerprint: c8b9322409f2
  fg:
    v: 1.009
    readings: [1.008, 1.01, 1.009]
    u: 5.773502691896583e-4
    statistics: {v: mean, u: standard_error}
    computed: {readings: 78cb57f23f22}
    fingerprint: 506eb7f5cf63
  volume:
    v: 20
    u: 0.2886751345948129
    unit: L
    computed: {u: {}}
    fingerprint: b32e7af7d4d6
  grain_mass: {v: 3.9, unit: kg, fingerprint: a690791b0792}
  mash_temperature: {v: 66, unit: degC}
  abv:
    v: 4.3312500000000185
    u: 0.15155444566228
    unit: "%"
    computed: {fg: 75b06f4e614f, og: 4654f38baaa0}
    fingerprint: 32f32002cb73
  attenuation:
    v: 78.57142857142884
    unit: "%"
    computed: {v: {fg: 75b06f4e614f, og: 4654f38baaa0}}
    fingerprint: 260f1a3beba6
  efficiency:
    v: 69.93006993006999
    unit: "%"
    computed: {v: {grain_mass: a690791b0792, og: 4654f38baaa0, volume: e2cdeaea1df9}}
    fingerprint: 9b9751a38f8e
---
# Session IPA

Low gravity, same hops as the Citra IPA.
