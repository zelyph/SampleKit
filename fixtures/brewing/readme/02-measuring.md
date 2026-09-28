# Step 2 · Measuring

In this step the same three brews are written down as a brewer would: each
gravity with its uncertainty, the grain and the mash temperature with their
units, who brewed it, and tags. You then change a value and a tag from the
command line, and see how SampleKit shows each change before it writes it.

Run every command from this folder, `02-measuring`.

## What changed

A brew now holds more. Here is `brews/oatmeal-stout.md`:

```text
---
schema_version: 1
name: oatmeal-stout
tags: [medal]
style: stout
yeast: S-04
hops: [Fuggles]
brewer: ana
status: ready
brewed: 2026-01-10
properties:
  og: {v: 1.058, u: 0.001}
  fg: {v: 1.016, u: 0.001}
  volume: {v: 19.5, unit: L}
  grain_mass: {v: 5.6, unit: kg}
  mash_temperature: {v: 68, unit: degC}
---
# Oatmeal stout

Flaked oats in the mash for the body. Took silver at the spring club contest.
```

- `u` is the **uncertainty** of the value, in the same unit. A hydrometer is
  read to its graduation, so each gravity is ±0.001.
- `unit` is the unit, written as you like: `L`, `kg`, `degC`. SampleKit never
  converts units; it shows them, and reports two samples that disagree.
- `tags` are labels, such as `medal`. A tag is one word: letters, digits and
  `_`.
- `hops` is an attribute holding a list.

`.samplekitrc` now says how each quantity is shown: its unit, how many digits
(its **precision**: the value's, then the uncertainty's), and `°C` for the
files' `degC`:

```toml
[unit."degC"]
plain = "°C"

[property.og]
symbol = "OG"
precision = [".3f", ".4f"]

[property.grain_mass]
unit = "kg"
symbol = "m_grain"
precision = ".2f"
```

## Try it

Read first:

```console
$ samplekit brews -c og,fg,grain_mass,mash_temperature
$ samplekit brews -f 'tags has medal'
$ samplekit list fields brews           # every name a filter or a column can use
$ samplekit validate brews              # checks the files, changes nothing
```

Then change things. Every command that changes a file first shows what it
would do, and writes nothing; `--write` makes the change.

```console
$ samplekit set brews/citra-ipa.md fg=1.012             # a preview
$ samplekit set brews/citra-ipa.md fg=1.012 --write     # written
$ samplekit tag add gift brews/farmhouse-saison.md --write
$ samplekit new pilsner --like brews/citra-ipa.md       # a new brew, shaped like another
```

## What you see

The table shows each uncertainty after its value, with the digits the
configuration declares. When every row of a column has the same unit, the
unit moves to the header:

```text
            og               fg   grain_mass [kg]   mash_temperature [°C]
─────────────────────────────────────────────────────────────────────────
1.064 ± 0.0010   1.011 ± 0.0010              6.10                      65
1.052 ± 0.0010   1.004 ± 0.0010              4.60                      64
1.058 ± 0.0010   1.016 ± 0.0010              5.60                      68
```

`set` shows each change as *before → after*, and what the change makes not
current. Nothing is computed yet in this step, so nothing depends on `fg`:

```text
brews/citra-ipa.md · 1 change

  fg                           1.011  →  1.012

  nothing derived rests on this

nothing written — pass --write to apply
```

With `--write`, the last line becomes `written: brews/citra-ipa.md`. A few
things to know about `set`:

- A name the configuration declares, or that the other brews hold as a
  property, is written as a property, with its unit.
- A name nothing knows is written as a new attribute, and the preview marks
  it `new`: check the spelling. `set brews/citra-ipa.md colour=amber` would
  create `colour`.
- Write decimals with a point: `fg=1.012`, never `fg=1,012`.
- An empty value removes what you name: `fg.u=` removes the uncertainty.

`tag` changes a tag on every brew you select — a file, a folder, or what a
filter keeps (`samplekit tag add gift brews -f 'style == stout'`). `new
--like` copies the other brew's attributes, and leaves its measured values
empty for you to fill in:

```text
would write brews/pilsner.md, shaped like brews/citra-ipa.md

  measured, for you to fill in   og, fg, volume, grain_mass, mash_temperature
    og, fg have no unit, so the file holds no line for them until samplekit set writes a value
  copied from the pattern        style = ipa, yeast = US-05, hops = [Citra, Mosaic], brewer = tom
  not carried                    status, brewed: the new sample's own to write

nothing written — pass --write to apply
```

## What SampleKit writes

When SampleKit writes a file, it rewrites the frontmatter in a fixed form:
keys in a fixed order, numbers written one way. So the first write to a file
you wrote by hand can change more lines than the value. A `# comment` inside
the frontmatter is lost; the note below it is never touched. Every write is
also kept in the project's history, `.samplekit/`, which step 4 shows.

## Why

A number without its uncertainty cannot be compared with another. SampleKit
carries the uncertainty with the value everywhere — tables, exports, formulas —
and the configuration decides how many digits are shown while the file keeps
the full number.

- To understand: [values, units and uncertainty](https://zelyph.github.io/SampleKit/latest/explanations/values-and-uncertainty.html),
  [a sample and its file](https://zelyph.github.io/SampleKit/latest/explanations/samples-and-files.html).
- To look up: [`set`, `tag`, `new`, `validate`](https://zelyph.github.io/SampleKit/latest/reference/cli.html),
  [the configuration's keys](https://zelyph.github.io/SampleKit/latest/reference/configuration.html),
  [what a sample file can hold](https://zelyph.github.io/SampleKit/latest/reference/sample-files.html).

Next: [Step 3 · Selecting](../03-selecting/README.md), with a whole season of
brews.
