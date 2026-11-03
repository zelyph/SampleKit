# Organise a project's settings

A `.samplekitrc` grows with a project: properties and units, queries and
profiles, exports, figures and their look. This guide spreads it over files of
its own, each holding one concern, and has each project import what it needs.
The rules it relies on are in [importing a
configuration](../reference/configuration.md#import).

## 1. A file per concern

Keep together what changes together. Here, two files at the root of a
laboratory's folder, beside the projects that use them:

```text
lab/
├── .samplekitrc
├── .venv/                   one Python environment, for every project's model
├── quantities.samplekitrc   what each quantity is: units, symbols, digits
├── figures.samplekitrc      how every figure looks
├── tensile/
│   ├── .samplekitrc
│   ├── model/
│   └── samples/
└── bending/
    ├── .samplekitrc
    ├── model/
    └── samples/
```

A file of settings is a `.samplekitrc` like any other, whatever its name. It
writes its own `schema_version`, and must be valid on its own:

```{code-block} toml
:caption: quantities.samplekitrc

schema_version = 1

[unit."MPa"]
figure = '$\mathrm{MPa}$'

[property.yield_strength]
symbol = "Rp0.2"
precision = ".0f"

[property.tensile_strength]
symbol = "Rm"
precision = ".0f"
```

```{code-block} toml
:caption: figures.samplekitrc

schema_version = 1

[style.figure]

[render]
figure_style = "figure"

[matplotlib]
"font.family" = "serif"
"axes.grid" = true
"figure.figsize" = [14, 9]
"lines.marker" = "None"
```

A style sheet or a font named in `figures.samplekitrc`, `style =
"paper.mplstyle"` or `fonts = ["fonts/*.otf"]`, is read from that file's
folder, wherever the file importing it is.

## 2. The root imports them

The root's `.samplekitrc` takes both, as a list, and names the Python
environment every project's model runs in:

```{code-block} toml
:caption: .samplekitrc

schema_version = 1
import = ["quantities.samplekitrc", "figures.samplekitrc"]

[model]
python = ".venv"
```

Each file listed is merged in order: a later one wins over an earlier one
where both write the same key, and the importing file wins over all. What the
root declares of its own, a profile or a query that every project uses, goes
below the `import` line.

## 3. Each project imports the root

A project imports the folder above, and with it everything the root imports.
It adds what is its own, its model first, and changes only what differs:

```{code-block} toml
:caption: tensile/.samplekitrc

schema_version = 1
import = ".."

[model]
path = "model/specimen.py"
class = "Specimen"

[matplotlib]
"figure.figsize" = [8.5, 6]
```

Here the tensile figures are one column of a journal wide; every other setting
of `figures.samplekitrc` still applies. A project may also import a file of
its own beside the root, `import = ["..", "poster.samplekitrc"]`, for a look
it alone uses.

The model is never imported: `[model] path` and `class` are each project's.
`[model] python` is imported, so that the environment the root names,
`lab/.venv/`, runs every project's model; a project that needs another names
its own.

## 4. Check what applies

From a project's folder, `samplekit --show-rc` names the configuration read
and every file it imports, nearest first. With `-v`, it lists each setting,
its value, and the file it comes from: `matplotlib."figure.figsize" = [8.5,
6]` from `tensile/.samplekitrc`, `matplotlib."font.family" = "serif"` from
`figures.samplekitrc`.

`samplekit validate` notes a declaration a file repeats from what it imports,
word for word: a copy to remove, so that a change made in one place is not
undone by the copy.

## See also

- [Projects and their configuration](../concepts/projects.md#importing-what-several-projects-share),
  why a configuration is merged only by `import`.
- [Every key of `.samplekitrc`](../reference/configuration.md).
