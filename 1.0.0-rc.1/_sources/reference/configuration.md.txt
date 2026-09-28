# Configuration

`.samplekitrc` is a project's configuration, in TOML. It is optional: a
folder of samples without one is read with the defaults below.

## Where it applies

- A sample follows the nearest `.samplekitrc`: in its own folder, else in the
  closest folder above, with what that file imports, wherever the command runs
  from. Configurations are combined only by `import`.
- A folder below a project with a `.samplekitrc` of its own is a separate
  project.
- Paths written in `.samplekitrc` are relative to the file itself, imported
  ones to the file that declares them.
- What `list`, the completion and the TUI's `f` offer is the configuration of
  the folder a command runs from, or of its target.
- `--rc PATH` uses another configuration for one command; `--show-rc` prints
  the configuration, model and Python a command would use.
- The file is checked each time it is read. An unknown section or key, a
  value of the wrong type, or a name that refers to nothing declared is an
  error naming the line and the nearest valid name. A configuration with an
  error is set aside and said; the samples it covers are read without it.

## Top level

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `schema_version` | integer | required | The version of the configuration's format: `1` | `schema_version = 1` |
| `import` | folder | none | The folder whose `.samplekitrc` this one takes, merged beneath it: relative to this file, absolute, or starting with `~` | `import = ".."` |

The sections below are all optional.

### `import`

- **What is imported**: everything but `[model] path` and `class`, which stay
  each file's own. `[model] python` is imported.
- **The merge rule**: key by key, this file winning. A table (a section, a
  `[property.NAME]`, a profile) merges key by key, recursively; any other value
  (a number, a text, a list) replaces the imported one whole.
- **Chains**: the imported file may import another. A file importing itself,
  directly or through others, is an error naming the files.
- **Errors**: a folder without a `.samplekitrc`, a path naming a file, or a
  folder that does not exist is an error naming the importing file. The
  imported file must be valid on its own.
- **Paths**: a relative `[collection] files`, `[export.*] output`, `[query.*]
  directory` or `[model] python` reads from the file declaring it.
  `[collection] include` and `exclude` are patterns on the collection's own
  files and apply as written.
- **`{collection}`**, in `[collection] files` and `[export.*] output`, is the
  folder of the collection a sample follows, written from the file declaring
  it (`data/ana` from the root; `.` in the collection's own file):
  `files = ["{collection}/photos/{name}*"]`, `output =
  "exports/{collection}/overview.csv"`.
- **Where each comes from**: `samplekit list queries` (profiles, exports,
  figures) marks each name `local`, with the imported file, or `local over`
  it; `--show-rc` names the files imported; `samplekit validate` notes a
  declaration identical to the imported one.

## `[model]`

The project's Python model. Reading a configuration never runs it.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `path` | path | required | The model's file | `path = "model/brew.py"` |
| `class` | text | the file's one class | The class to use, when the file defines several | `class = "Brew"` |
| `python` | path | the nearest `.venv/` above the project | The Python interpreter that runs the model | `python = "../env/bin/python"` |

## `[render]`

How values are written when nothing else says.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `style` | text | `"plain"` | The style tables use: `plain`, or a `[style.*]` declared. `--style` overrides it | `style = "math"` |
| `precision` | precision | none | The precision of every number no `[property.*]` or column gives one | `precision = ".3f"` |
| `table` | text | `"plain"` | How a terminal table is drawn: `plain` or `boxed`. `--table-style` overrides it | `table = "boxed"` |
| `identify` | text | `"name"` | The first column of a terminal table: `name`, `filename` or `none` | `identify = "filename"` |
| `figure_style` | text | none | The style a figure's labels use when the figure names none | `figure_style = "figure"` |

A **precision** is a format specifier, `".3f"` (three decimals) or `".2e"`
(scientific notation), or a list of two, `[".3f", ".4f"]`: the value's, then
the uncertainty's.

## `[collection]`

Which files are samples, and where a sample's own files are.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `recursive` | boolean | `false` | Read the subfolders of the folder named too | `recursive = true` |
| `include` | list of patterns | `["*.md", "*.MD", "*.markdown"]` | The files read | `include = ["*.sample.md"]` |
| `exclude` | list of patterns | `[]` | The files skipped. `samplekit list skipped` lists them | `exclude = ["draft-*"]` |
| `files` | list of folders or patterns | `[]` | Where a sample's own files are: a folder, searched for files whose name starts with the sample's; or a pattern holding `{name}`, the sample's file name without `.md`; `{collection}` is its collection's folder (see `import`) | `files = ["photos", "raw/{name}/**"]` |

Patterns are globs: `*` any characters within a name, `?` one character,
`**` any number of folders, `[0-9]` one of a set, `{a,b}` either. A pattern
holding a `/` is matched against the path from the project's folder; without
one, against the file name.

## `[unit."SPELLING"]`

How a unit is shown, in each style. The key is the unit as the files write
it, quoted. SampleKit never converts units: a unit no section names is shown
as written.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `plain` | text | the spelling | The unit in the `plain` style | `plain = "°C"` |
| any style's name | text | the spelling | The unit in that style | `math = '^\circ\mathrm{C}'` |

The units the sections name are the project's vocabulary: `validate` reports
a spelling outside it.

## `[style.NAME]`

A style, named for `[render] style`, `--style`, a figure's `style` and the
variants of `[unit.*]` and `[property.*]`. `plain` is built in.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `separator` | text | `"±"` | What stands between a value and its uncertainty | `separator = '\pm'` |

A style may be declared empty, `[style.figure]`, to name its variants.

## `[property.NAME]`

What a quantity's files need not repeat. The name is the property's; a
table's column is `[property."table.column"]`, quoted.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `unit` | text | none | The unit, for files that give none | `unit = "L"` |
| `symbol` | text | none | The symbol, which labels a figure's axis | `symbol = "OG"` |
| `symbol_STYLE` | text | `symbol` | The symbol in a style | `symbol_math = '\mathrm{OG}'` |
| `precision` | precision | `[render] precision` | How the number is written. Files hold none | `precision = [".3f", ".4f"]` |

A unit or a symbol a file writes wins over these. An attribute's
`symbol_figure` titles a figure's legend when the figure is grouped by it.

## `[query.NAME]`

A named filter, used with `--query NAME`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `filter` | text | required | The filter, in the language of `-f` | `filter = 'style == ipa'` |
| `directory` | path | none | The folder it applies to when a command names no target | `directory = "brews"` |

## `[profile.NAME]`

A named table shape, used with `--profile NAME`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `columns` | list of columns | required, not empty | The columns, in order | `columns = [{ field = "og" }]` |
| `sort` | list of fields | `[]` | The order; a `-` before a field sorts it from high to low | `sort = ["style", "-og"]` |
| `group` | list of fields | `[]` | A table per group of samples sharing these fields' values, as `--group` | `group = ["style"]` |

Each column is an inline table:

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `field` | field | required | What the column shows, as `-c` names it | `field = "og.u"` |
| `label` | text | the field | The column's title in a terminal | `label = "Original gravity"` |
| `header` | text | the field | The column's name in a CSV, TSV or JSON file | `header = "og_sg"` |
| `precision` | precision | `[property.*]`'s | How the column's numbers are written | `precision = ".4f"` |
| `template` | text | none | The cell as text, with `{value}`, `{uncertainty}`, `{unit}` and `{symbol}` filled in; `{value:.3f}` takes a precision | `template = '\num{{value:.3f}}'` |

A field listed twice, two columns sharing a `header`, and an empty `label` or
`header` are errors. `sort` and `group` are lists even for one field.

## `[export.NAME]`

A named file, written by `samplekit export NAME --write`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `profile` | text | required | The profile giving the columns | `profile = "overview"` |
| `format` | text | required | `csv`, `tsv` or `json` | `format = "csv"` |
| `output` | path | required | The file written; its folder is created if needed; `{collection}` names the collection's folder, so that collections importing it write apart | `output = "out/overview.csv"` |
| `query` | text | none | The query selecting the samples | `query = "ipas"` |
| `filename` | boolean | `false` | Add a column holding each sample's file name | `filename = true` |
| `path` | boolean | `false` | Add a column holding each sample's path | `path = true` |

## `[figure.NAME]`

A named figure, drawn by `samplekit plot NAME`. Drawing it runs no code from
the project.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `kind` | text | `"scatter"` | `scatter`, `line`, `step`, `bar` or `box` | `kind = "line"` |
| `x` | field | required | The horizontal axis | `x = "fermentation.day"` |
| `y` | field | required | The vertical axis | `y = "fermentation.gravity"` |
| `group` | field | none | One series per value of this field | `group = "yeast"` |
| `query` | text | none | The query selecting the samples | `query = "ipas"` |
| `title` | text | the figure's name | The title | `title = "Fermentation"` |
| `x_label`, `y_label` | text | the field's symbol and unit | An axis's label | `y_label = '$V$ [L]'` |
| `style` | text | `[render] figure_style` | The style the labels use | `style = "figure"` |
| `x_limits`, `y_limits` | two bounds | matplotlib's | An axis's range; `"auto"` leaves a bound free; a first bound above the second turns the axis over | `x_limits = [0, "auto"]` |
| `x_scale`, `y_scale` | text | `"linear"` | `linear`, `log` or `symlog` | `y_scale = "log"` |
| `aspect` | text | `"auto"` | `equal` draws a unit the same length on both axes, `auto` does not | `aspect = "equal"` |
| `legend` | text | `"best"`; `"outside"` past six series | `best`, `outside`, `none`, or a place: `upper right`, `upper left`, `lower left`, `lower right`, `right`, `center left`, `center right`, `lower center`, `upper center` | `legend = "outside"` |
| `figsize` | two numbers | `[matplotlib] "figure.figsize"` | Width and height, in centimetres | `figsize = [18, 12]` |

Limits are also written as one text, `"0,auto"`. A bound is finite, two equal
bounds are refused, and a log axis takes no bound at or below zero.

## `[matplotlib]`

matplotlib's own settings, by their matplotlib names, for every figure of the
project. Quote a dotted name.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| any matplotlib setting | as matplotlib takes it | matplotlib's | That setting | `"axes.grid" = true` |
| `"figure.figsize"` | two numbers | matplotlib's | A figure's size, in centimetres | `"figure.figsize" = [15, 10]` |

Other sizes keep matplotlib's units: points for fonts and lines. A name
matplotlib does not know, a value it refuses and a date are errors;
`backend` and `interactive` are refused. `plot --no-project-style` ignores
the section.

## `[tui.colors]`

The TUI's colours, by role. A colour is `default` (the terminal's own), one
of the terminal's sixteen by name (`black`, `red`, `green`, `yellow`, `blue`,
`magenta`, `cyan`, `grey`, `dark-grey`, `light-red`, `light-green`,
`light-yellow`, `light-blue`, `light-magenta`, `light-cyan`, `white`; `gray`
and `dark-gray` too), or `#rrggbb`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `failed` | colour | `red` | A value whose formula failed | `failed = "light-red"` |
| `outdated` | colour | `yellow` | A value that is not current | `outdated = "#ff8800"` |
| `edited` | colour | `magenta` | A value written by hand over its formula | `edited = "light-magenta"` |
| `current` | colour | `green` | A computed value that is current | `current = "light-green"` |
| `selected` | colour | `cyan` | The line or item under the cursor | `selected = "light-cyan"` |
| `attribute` | colour | `blue` | Attributes | `attribute = "light-blue"` |
| `table` | colour | `magenta` | Tables | `table = "magenta"` |
| `index` | colour | `cyan` | A table's index column | `index = "cyan"` |
| `key` | colour | `cyan` | Keys, in the help and at the foot | `key = "yellow"` |
| `muted` | colour | `dark-grey` | Secondary text | `muted = "grey"` |
| `message` | colour | `yellow` | Messages | `message = "default"` |
| `error` | colour | `red` | Errors | `error = "light-red"` |
| `accent` | colour | `cyan` | The name in the title bar, a computation in progress | `accent = "#5fafd7"` |

`stale` is read as `outdated`, and `[workbench.colors]` as `[tui.colors]`:
the names these had before. Where both are written, the current name wins.

## A complete example

```toml
schema_version = 1

[model]
path = "model/brew.py"
class = "Brew"

[render]
table = "boxed"
figure_style = "figure"

[collection]
recursive = true
exclude = ["draft-*"]
files = ["photos", "labels/{name}*.png"]

[unit."degC"]
plain = "°C"
math = '^\circ\mathrm{C}'
figure = "°C"

[style.math]
separator = '\pm'

[style.figure]

[property.og]
symbol = "OG"
precision = [".3f", ".4f"]

[query.ipas]
filter = 'style == ipa'

[profile.overview]
columns = [
  { field = "name", label = "Brew" },
  { field = "style", label = "Style" },
  { field = "og", label = "Original gravity", header = "og" },
]
sort = ["style", "-og"]

[export.overview]
profile = "overview"
format = "csv"
output = "out/overview.csv"

[figure.score]
x = "abv"
y = "score"
group = "style"
legend = "outside"

[matplotlib]
"axes.grid" = true

[tui.colors]
outdated = "#ff8800"
```
