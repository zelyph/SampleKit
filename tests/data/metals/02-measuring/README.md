# Step 2 · Measuring

In this step the same three pieces are written down as a tester would: each
hardness with its uncertainty, the ore and the preheat temperature with their
units, who poured it, and tags. You then change a value and a tag from the
command line, and see how SampleKit shows each change before it writes it.

Run every command from this folder, `02-measuring`.

## What changed

A piece now holds more. Here is `pieces/oxidised-nickel.md`:

```text
---
schema_version: 1
name: oxidised-nickel
tags: [prize]
style: nickel
process: T-04
carbon: [Fullerene]
tester: ana
status: ready
poured: 2026-01-10
properties:
  radius: {v: 1.058, u: 0.001}
  notch: {v: 1.016, u: 0.001}
  volume: {v: 19.5, unit: L}
  ore_mass: {v: 5.6, unit: kg}
  preheat_temperature: {v: 68, unit: degC}
---
# Oxidised nickel

Powdered flux in the preheat for the body. Took silver at the spring trade fair.
```

- `u` is the **uncertainty** of the value, in the same unit. A durometer is
  read to its graduation, so each hardness is ±0.001.
- `unit` is the unit, written as you like: `L`, `kg`, `degC`. SampleKit never
  converts units; it shows them, and reports two samples that disagree.
- `tags` are labels, such as `prize`. A tag is one word: letters, digits and
  `_`.
- `carbon` is an attribute holding a list.

`.samplekitrc` now says how each property is shown: its unit, how many digits
(its **precision**: the value's, then the uncertainty's), and `°C` for the
files' `degC`:

```toml
[unit."degC"]
plain = "°C"

[property.radius]
symbol = "RADIUS"
precision = [".3f", ".4f"]

[property.ore_mass]
unit = "kg"
symbol = "m_ore"
precision = ".2f"
```

## Try it

Read first:

```console
$ samplekit pieces -c radius,notch,ore_mass,preheat_temperature
$ samplekit pieces -f 'tags has prize'
$ samplekit pieces list fields           # every name a filter or a column can use
$ samplekit pieces validate              # checks the files, changes nothing
```

Then change things. Every command that changes a file first shows what it
would do, and writes nothing; `--write` makes the change.

```console
$ samplekit pieces/alpha-titanium.md set notch=1.012             # a preview
$ samplekit pieces/alpha-titanium.md set notch=1.012 --write     # written
$ samplekit pieces/farmhouse-steel.md tag add gift --write
$ samplekit new inconel --keep pieces/alpha-titanium.md style,process   # a new piece, two fields of another
```

## What you see

The table shows each uncertainty after its value, with the digits the
configuration declares. When every row of a column has the same unit, the
unit moves to the header:

```text
            radius               notch   ore_mass [kg]   preheat_temperature [°C]
─────────────────────────────────────────────────────────────────────────
1.064 ± 0.0010   1.011 ± 0.0010              6.10                      65
1.052 ± 0.0010   1.004 ± 0.0010              4.60                      64
1.058 ± 0.0010   1.016 ± 0.0010              5.60                      68
```

`set` shows each change as *before → after*, and what the change makes not
current. Nothing is computed yet in this step, so nothing depends on `notch`:

```text
pieces/alpha-titanium.md · 1 change

  notch   1.011 ± 0.0010  →  1.012 ± 0.0010

  nothing becomes not current

apply these changes? [y/N] n
nothing written
```

`y` writes it, and the last line becomes `written: pieces/alpha-titanium.md`;
`--write` answers yes beforehand. A few
things to know about `set`:

- A name the configuration declares, or that the other pieces hold as a
  property, is written as a property, with its unit.
- A name nothing knows is written as a new attribute, and the preview marks
  it `new`: check the spelling. `samplekit pieces/alpha-titanium.md set
  colour=matte` would create `colour`.
- Write decimals with a point: `notch=1.012`, never `notch=1,012`.
- An empty value removes what you name: `notch.u=` removes the uncertainty.

`tag` changes a tag on every piece you select — a file, a folder, or what a
filter keeps (`samplekit pieces tag add gift -f 'style == nickel'`). `new
--keep` takes from another piece the fields you name, and nothing else — not
its tester, not its dates: the new piece is written beside it, and its
measurements are yours to set:

```text
pieces/inconel.md · new sample

  taken from pieces/alpha-titanium.md   style = titanium, process = UT-05

apply these changes? [y/N] n
nothing written
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
pieces.
