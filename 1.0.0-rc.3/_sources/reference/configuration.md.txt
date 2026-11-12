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
  ones to the file that declares them. Write them with `/`, which every system
  reads; a `\` written on Windows is read as `/` on Linux and macOS too, so
  that the project reads wherever it is copied.
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
| `schema_version` | integer | required, in every file | The version of the configuration's format: `1`. A file importing another writes its own | `schema_version = 1` |
| `import` | folder or file, or a list | none | What this file takes, merged beneath it: a folder's `.samplekitrc`, or a file of any name; relative to this file, absolute, or starting with `~`. A list is merged in order, a later one over an earlier one | `import = ["..", "figures.samplekitrc"]` |

The sections below are all optional.

### `import`

- **What is imported**: everything but `[model] path` and `class`, which stay
  each file's own. `[model] python` is imported.
- **The merge rule**: key by key, this file winning. A table (a section, a
  `[property.NAME]`, a profile) merges key by key, recursively; any other value
  (a number, a text, a list) replaces the imported one whole.
- **A list**: each is merged in order, a later one over an earlier one, and
  this file over all. A file listed twice is merged once.
- **Chains**: the imported file may import another. A file importing itself,
  directly or through others, is an error naming the files.
- **Errors**: a folder without a `.samplekitrc`, or a path naming nothing, is
  an error naming the importing file. The imported file must be valid on its
  own, its `schema_version` written.
- **Paths**: a relative `[collection] files`, `[export.*] output`, `[query.*]
  directory`, `[model] python`, or `[matplotlib]` style file or font reads
  from the file declaring it.
  `[collection] include` and `exclude` are patterns on the collection's own
  files and apply as written. Each project's own decide which files are its
  samples, whichever folder a command names: the patterns of a folder above
  never reach into a project inside it.
- **`{project}`**, in `[collection] files` and `[export.*] output`, is the
  folder of the project a sample follows, written from the file declaring
  it (`data/ana` from the root; `.` in the project's own file):
  `files = ["{project}/photos/{name}*"]`, `output =
  "exports/{project}/overview.csv"`. `{project}.tsv` in the project's
  own file writes `..tsv`, which `validate` notes.
- **Where each comes from**: `samplekit list queries` (profiles, exports,
  figures) marks each name `local`, with the imported file, or `local over`
  it; `--show-rc` names the files imported; `samplekit validate` notes a
  declaration identical to the imported one.

## `[model]`

The project's Python model. Reading a configuration never runs it.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `path` | path | none | The model's file; without it, `[model]` names only the Python that draws the figures | `path = "model/specimen.py"` |
| `class` | text | the file's one class | The class to use, when the file defines several | `class = "Specimen"` |
| `python` | path | required to compute or draw | The Python that runs the model and draws the figures: an environment folder, whose `bin/python` (`Scripts\python.exe` on Windows) runs, or an interpreter. Declared without `path` too, for the figures of a project without a model. Nothing is searched for | `python = ".venv"` |

## `[render]`

How values are written when nothing else says.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `style` | text | `"plain"` | The style tables use: `plain`, or a `[style.*]` declared. `--style` overrides it | `style = "math"` |
| `precision` | precision | none | The precision of every number no `[property.*]` or column gives one | `precision = ".3f"` |
| `table` | text | `"plain"` | How a terminal table is drawn: `plain` or `boxed`, any other name an error. `--table-style` overrides it | `table = "boxed"` |
| `identify` | text | `"name"` | The column added first to a table shown in a terminal, unless the columns name it: `name`, `filename` or `none`; a table piped or written to a file has only the columns asked for | `identify = "filename"` |
| `figure_style` | text | none | The style a figure's labels use when the figure names none | `figure_style = "figure"` |

A **precision** is a format specifier, written as in Python: `".3f"` (three
decimals), `".2e"` (scientific notation), `".4g"` (four significant digits),
or a list of two, `[".3f", ".4f"]`: the value's, then the uncertainty's.
`".17g"` writes every digit a value carries, and an uncertainty's too:
`0.8787328442349305` is written as it is.

## `[collection]`

Which files are samples, and where a sample's own files are.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `recursive` | boolean | `true` in a project | Read the folders below the folder named too. A folder outside any project is read alone | `recursive = false` |
| `include` | list of patterns | `["*.md", "*.MD", "*.markdown"]` | The files read | `include = ["*.sample.md"]` |
| `exclude` | list of patterns | `[]` | The files skipped. `samplekit list skipped` lists them | `exclude = ["draft-*"]` |
| `files` | list of folders or patterns | `[]` | Where a sample's own files are: a folder, searched for files whose name starts with the sample's; or a pattern holding `{name}`, the sample's file name without `.md`; `{project}` is its project's folder (see `import`) | `files = ["photos", "raw/{name}/**"]` |

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

A spelling no section names is shown as written: the sections are display
forms, never a closed list of units. What `validate` reports is a file whose
unit contradicts the one `[property.*]` declares.

## `[style.NAME]`

A style says how a kind of document writes: named by `[render] style`,
`--style`, an export's and a figure's `style`, and the variants of
`[unit.*]` and `[property.*]`, `[unit.g] latex = '\gram'`. `plain` is built
in; `latex`, `markdown` and `html` are given, and a `[style.latex]` changes
the given one key by key. A style of units and symbols alone may be declared
empty, `[style.figure]`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `property` | text or `false` | `"{value} ± {uncertainty} {unit}"` | How a property is written, with `{value}`, `{uncertainty}`, `{unit}` and `{symbol}` in the style's forms; in a document, `false` writes its two columns. A unit placed inside it, `\SI{…}{{unit}}`, is written in each cell and not in the column's header | `property = '\SI{{value} \pm {uncertainty}}{{unit}}'` |
| `uncertainty` | text | the number | The piece `{uncertainty}` stands for in `property` and `unitless`, with `{uncertainty}` the number in it; written only where a value has an uncertainty | `uncertainty = ' \pm {uncertainty}'` |
| `unitless` | text | `property`, its unit empty | How a property with no unit is written, instead of `property` | `unitless = '\num{{value}{uncertainty}}'` |
| `missing` | text | `""` in a document | An absent value | `missing = "--"` |
| `escape` | text, table or `false` | `false`; the given styles' own | What text is escaped for: `latex`, `html`, `markdown`, or a table of replacements, `escape = { "<" = "&lt;" }`. A template and the style's own lines and forms are written as they are | `escape = "latex"` |

**A style writing documents** writes a table as a document holds it: `-o
table.tex`, `--style latex -o -` to the standard output, an export naming it
as its `style`, and Python's `to_text("latex")`. The given three write
documents; a style of your own does once it declares one of these keys:

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `extension` | text | the name; `tex`, `md`, `html` | The file's extension, which `-o` recognises for the given three | `extension = "txt"` |
| `before` | text | `""` | The lines before the table; `{align}` is the columns' alignment | `before = '\begin{tabular}{{align}}'` |
| `after` | text | `""` | The lines after it | `after = '\end{tabular}'` |
| `header` | text or `false` | `"{cells}"` | The header line, `{cells}` its cells joined; `false` writes none | `header = '{cells} \\'` |
| `header_cells` | text | `cells` | Between two cells of the header line | `header_cells = "</th><th>"` |
| `rule` | text | none | A line of this cell under each column, after the header, as Markdown's `---` | `rule = "---"` |
| `row` | text | `"{cells}"` | Each row's line | `row = '{cells} \\'` |
| `cells` | text | `" "` | Between two cells | `cells = " & "` |
| `between_groups` | text | none; LaTeX's `\midrule`, Markdown's `\| **{group}** \|`, HTML's `</tbody><tbody>` | A line where a column the profile groups by changes value. Holding `{group}`, it heads each group, the first included: `{group}` is its values, `{columns}` the number of columns | `between_groups = '\midrule \multicolumn{{columns}}{l}{{group}} \\'` |
| `header_unit` | text or `false` | `" ({unit})"` | Written after a header cell holding a unit; `false` writes none | `header_unit = " [{unit}]"` |
| `align_text`, `align_number` | text | `"l"`, `"r"` | `{align}`'s letter for a column of text, and of numbers | `align_number = "S"` |

## `[property.NAME]`

What a property's files need not repeat. The name is the property's; a
table's column is `[property."table.column"]`, quoted.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `unit` | text | none | The unit, for files that give none | `unit = "mm"` |
| `symbol` | text | none | The symbol, which labels a figure's axis | `symbol = "Rm"` |
| `symbol_STYLE` | text | `symbol` | The symbol in a style | `symbol_math = 'R_\mathrm{m}'` |
| `precision` | precision | `[render] precision` | How the number is written. Files hold none | `precision = [".3f", ".4f"]` |
| `range` | two numbers | none | The values it may take, in its declared unit, `inf` leaving a side open: `validate` reports one outside as a defect, and `set` and `compute` write it and warn | `range = [0, 50]` |

A unit or a symbol a file writes wins over these. An attribute's
`symbol_figure` titles a figure's legend when the figure is grouped by it.

## `[query.NAME]`

A named filter, used with `--query NAME`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `filter` | text | required | The filter, in the language of `-f` | `filter = 'material == steel'` |
| `directory` | path | none | The folder it applies to when a command names no target | `directory = "samples"` |

## `[profile.NAME]`

A named table shape, used with `--profile NAME`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `columns` | list of columns | required, not empty | The columns, in order | `columns = [{ field = "width" }]` |
| `sort` | list of fields | `[]` | The order; a `-` before a field sorts it from high to low | `sort = ["material", "-width"]` |
| `group` | list of fields | `[]` | A table per group of samples sharing these fields' values, as `--group` | `group = ["material"]` |

Each column is an inline table:

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `field` | field | required | What the column shows, as `-c` names it | `field = "width.u"` |
| `label` | text | the field | The column's title on the terminal and in a document (`--style latex`, an export's `style`) | `label = "Tensile strength"` |
| `header` | text | the field | The column's name in a CSV, TSV or JSON file: a property's two columns are `Rm_value [MPa]` and `Rm_uncertainty [MPa]`, the suffixes and the unit saying what each holds | `header = "Rm"` |
| `precision` | precision | `[property.*]`'s | How the column's numbers are written | `precision = ".4f"` |
| `template` | text | none | The cell as text, with `{value}`, `{uncertainty}`, `{unit}` and `{symbol}` filled in; `{value:.3f}` takes a precision. A CSV or TSV writes the text, in one column named by `header`, its unit and symbol spelled in `[render] style`; JSON writes the numbers | `template = '\num{{value:.3f}}'` |

A field listed twice, two columns sharing a `header`, and an empty `label` or
`header` are errors. `sort` and `group` are lists even for one field.

## `[export.NAME]`

A named file, written by `samplekit export NAME --write`: a table, from a
profile, or a figure, drawn as `samplekit plot` draws it.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `profile` | text | one of the two | The profile giving the columns of a table | `profile = "overview"` |
| `figure` | text | one of the two | The figure drawn, declared in `[figure.*]` or by the model; its file's format is its `output`'s extension | `figure = "curves"` |
| `format` | text | none | A table's: `csv`, `tsv` or `json`; without it, `style` names a style writing documents | `format = "csv"` |
| `output` | path | required | The file written; its folder is created if needed; `{project}` names the project's folder, so that projects importing it write apart; for a figure of one sample, `{name}` writes a file per sample | `output = "out/overview.csv"` |
| `query` | text | none | The query selecting the samples | `query = "ductile"` |
| `filename` | boolean | `false` | A table's: add a column holding each sample's file name | `filename = true` |
| `path` | boolean | `false` | A table's: add a column holding each sample's path, from its project's folder | `path = true` |
| `style` | text | `files`; a figure's, `[render] figure_style` | How units are spelled in a data file: `files`, as the samples write them (`degC`); `plain`, the forms `[unit.*]` declares for the screen (`°C`); or a style. With no `format`, the style writing the document. For a figure, the style its labels use | `style = "latex"` |

An export names a `profile` or a `figure`, never both. `format`, `filename`
and `path` are a table's, and refused beside a figure; `--precision` and
`--status` likewise, on the command line.

## `[figure.NAME]`

A named figure, drawn by `samplekit plot NAME`. Drawing it runs no code from
the project.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `kind` | text | `"scatter"` | `scatter`, `line`, `step`, `bar` or `box` | `kind = "line"` |
| `x` | field | required | The horizontal axis | `x = "curve.strain"` |
| `y` | field | required | The vertical axis | `y = "curve.stress"` |
| `group` | field | none | One series per value of this field | `group = "material"` |
| `query` | text | none | The query selecting the samples | `query = "ductile"` |
| `title` | text | none | The title drawn above the axes; none by default, a caption in the document saying what the figure shows | `title = "Stress against strain"` |
| `x_label`, `y_label` | text | the field's symbol and unit | An axis's label | `y_label = '$\sigma$ [MPa]'` |
| `style` | text | `[render] figure_style` | The style the labels use | `style = "figure"` |
| `x_limits`, `y_limits` | two bounds | matplotlib's | An axis's range; `"auto"` leaves a bound free; a first bound above the second turns the axis over | `x_limits = [0, "auto"]` |
| `x_scale`, `y_scale` | text | `"linear"` | `linear`, `log` or `symlog` | `y_scale = "log"` |
| `aspect` | text | `"auto"` | `equal` draws a unit the same length on both axes, `auto` does not | `aspect = "equal"` |
| `legend` | text | `"best"`; `"outside"` past six series | `best`, `outside`, `none`, or a place: `upper right`, `upper left`, `lower left`, `lower right`, `right`, `center left`, `center right`, `lower center`, `upper center` | `legend = "outside"` |
| `figsize` | two numbers | `[matplotlib] "figure.figsize"` | Width and height, in centimetres | `figsize = [18, 12]` |
| `matplotlib` | table | none | matplotlib's settings for this figure alone, over `[matplotlib]`; `style` and `fonts` are the project's | `matplotlib = { "lines.marker" = "s" }` |

Limits are also written as one text, `"0,auto"`. A bound is finite, two equal
bounds are refused, and a log axis takes no bound at or below zero.

## `[matplotlib]`

matplotlib's own settings, by their matplotlib names, for every figure of the
project. Quote a dotted name.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| any matplotlib setting | as matplotlib takes it | matplotlib's | That setting | `"axes.grid" = true` |
| `"figure.figsize"` | two numbers | matplotlib's | A figure's size, in centimetres | `"figure.figsize" = [15, 10]` |
| `style` | text or list | none | matplotlib's style sheets, by name or as `.mplstyle` files, applied in order before the section's own settings, which win | `style = ["seaborn-v0_8-paper", "figures.mplstyle"]` |
| `fonts` | text or list | none | Font files, or patterns of them, that matplotlib learns before it draws: a font that travels with the project | `fonts = ["fonts/*.otf"]` |

Other sizes keep matplotlib's units: points for fonts and lines. A name
matplotlib does not know, a value it refuses and a date are errors;
`backend` and `interactive` are refused. A style sheet matplotlib does not
have, a file that is not there and a pattern that finds none are errors too.
A `.mplstyle` file and a font are read from the file declaring them.

SampleKit makes a few choices of its own, and gives way where the project
declares the setting, in this section or in a style sheet:

| Setting | SampleKit's choice |
| --- | --- |
| `lines.marker`, or `marker` in `axes.prop_cycle` | a round marker on each point of a `scatter` and a `line` |
| `lines.markersize` | the points of a `box` drawn at size 3 |
| `errorbar.capsize` | caps of 3 points on the error bars |
| `legend.fontsize` | a small legend, where it is placed outside |

A `scatter` draws no line between its points, whatever `lines.linestyle` says.
`plot --no-project-style` ignores the section, and a figure's own
`matplotlib`.

## `[colors]`

The colours of the TUI and of the command line, by role. The command line
wears them only on a terminal, and never with `--no-color`, `NO_COLOR` or
`TERM=dumb`; a file it writes, and a pipe, carry none. A colour is `default` (the terminal's own), one
of the terminal's sixteen by name (`black`, `red`, `green`, `yellow`, `blue`,
`magenta`, `cyan`, `grey`, `dark-grey`, `light-red`, `light-green`,
`light-yellow`, `light-blue`, `light-magenta`, `light-cyan`, `white`; `gray`
and `dark-gray` too), or `#rrggbb`.

In the terminal interface, a role the table leaves out is its **theme's**:
SampleKit's own, `samplekit-dark` or `samplekit-light`, or `terminal` where
the terminal does not say it draws every colour, unless one is chosen with `T`
or named by `theme`, and the roles the table names are laid over it.
The command line has no theme: a role left out there is the default below,
one of the terminal's sixteen, which is also the theme `terminal`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `theme` | text | none | The terminal interface's theme for this project: one SampleKit has, or one of your own; a theme chosen with `T` in this folder comes first. A name no theme bears is said, and passed over | `theme = "nord"` |
| `background` | colour | `default` | The background of the terminal interface's whole screen; the command line leaves the terminal's | `background = "#1e1e2e"` |
| `foreground` | colour | `default` | The terminal interface's text, where nothing else colours it | `foreground = "#cdd6f4"` |
| `failed` | colour | `red` | A value whose formula failed | `failed = "light-red"` |
| `outdated` | colour | `yellow` | A value that is not current | `outdated = "#ff8800"` |
| `edited` | colour | `magenta` | A value written by hand over its formula | `edited = "light-magenta"` |
| `current` | colour | `green` | A computed value that is current | `current = "light-green"` |
| `selected` | colour | `cyan` | The line or item under the cursor | `selected = "light-cyan"` |
| `attribute` | colour | `blue` | Attributes, and an attribute's value | `attribute = "light-blue"` |
| `link` | colour | `blue` | A link, underlined; one leading nowhere takes `error`'s colour | `link = "light-blue"` |
| `value` | colour | `default` | A value and its uncertainty, drawn bold | `value = "white"` |
| `unit` | colour | `cyan` | A unit, after a number and in a header | `unit = "grey"` |
| `table` | colour | `magenta` | Tables | `table = "magenta"` |
| `index` | colour | `cyan` | A table's index column | `index = "cyan"` |
| `key` | colour | `cyan` | Keys, in the help and at the foot | `key = "yellow"` |
| `muted` | colour | `dark-grey` | Secondary text | `muted = "grey"` |
| `message` | colour | `yellow` | Messages | `message = "default"` |
| `error` | colour | `red` | Errors | `error = "light-red"` |
| `accent` | colour | `cyan` | The name in the title bar, a computation in progress | `accent = "#5fafd7"` |
| `waiting` | colour | `light-magenta` | What waits to be written in the terminal interface, in italics | `waiting = "light-yellow"` |
| `line` | colour | `dark-grey` | The background of the cursor's line on the collection, a sample and a table; in the typing mode the cell under the cursor reversed on it | `line = "#313244"` |

[Choose the colours](../guides/colours.md) shows every theme SampleKit has,
says how one is chosen, and how to make your own.

## `[history]`

The project's [history](../concepts/history.md), kept in
`.samplekit/history/`.

| Key | Type | Default | Meaning | Example |
| --- | --- | --- | --- | --- |
| `shared` | boolean | not set | Whether git carries the history with the samples: `true` lets git see what must travel and keeps out what each computer makes for itself; `false` hides the history from git. Not set, a new history is hidden, and one shared by hand is left as it is. | `shared = true` |

Imported as any key: a root declaring it shares the history of every project
importing it. See [Share a project](../guides/share-a-project.md).

## A complete example

```toml
schema_version = 1

[model]
path = "model/specimen.py"
class = "Specimen"
python = ".venv"

[render]
table = "boxed"
figure_style = "figure"

[collection]
recursive = true
exclude = ["draft-*"]
files = ["photos", "machine/{name}/*"]

[unit."degC"]
plain = "°C"
math = '^\circ\mathrm{C}'
figure = "°C"

[style.math]
property = '${value} \pm {uncertainty}$ {unit}'

[style.figure]

[property.tensile_strength]
symbol = "Rm"
symbol_math = 'R_\mathrm{m}'
precision = ".0f"

[query.steels]
filter = 'material == steel'

[profile.overview]
columns = [
  { field = "name", label = "Specimen" },
  { field = "material", label = "Material" },
  { field = "tensile_strength", label = "Tensile strength", header = "Rm" },
]
sort = ["material", "-tensile_strength"]

[export.overview]
profile = "overview"
format = "csv"
output = "out/overview.csv"

[figure.curves]
kind = "line"
x = "curve.strain"
y = "curve.stress"
group = "material"
legend = "outside"

[matplotlib]
"axes.grid" = true

[colors]
outdated = "#ff8800"
```
