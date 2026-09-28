# Projects

A **project** is a folder with a `.samplekitrc` file: its samples, the rules
for reading them, and optionally a model. This page says what a project
decides, how SampleKit finds the project a sample belongs to, and what
happens when one command reads samples from several projects.

## Why a project

The sample files hold the data. What they do not hold is said once for all of
them, in the project:

- **how to show** the quantities: their units' display forms, symbols and
  precisions ([Values and uncertainty](values-and-uncertainty.md));
- **which files** are samples, and where each sample's own files are
  ([Samples and their files](samples-and-files.md));
- **the model** that computes derived values
  ([Models and formulas](models-and-formulas.md));
- **named selections and layouts**: queries, profiles, exports, figures;
- the TUI's colours.

A project is also the unit of **one kind of sample**: one set of
quantities and one model. When the quantities or the formulas change from one
series to the next, the next series is a new project.

A project costs nothing to the person with ten samples. **A folder of samples
with no `.samplekitrc` works**: every command reads it with the default
settings. `samplekit init` makes the file when you want one, and the
[history](history.md) needs one.

## The configuration file

`.samplekitrc` is written in TOML, a plain text format of sections and
`key = value` lines:

```toml
schema_version = 1

[collection]
recursive = true
exclude = ["README.md"]

[model]
path = "model/brew.py"
class = "Brew"

[property.og]
symbol = "OG"
precision = [".3f", ".4f"]

[query.strong]
filter = 'og > 1.060'
```

Paths inside it are relative to the file itself, so a project can be moved or
copied as a whole. You can edit it in any text editor, or section by section
in the TUI (`P`, or `c` on the start page), which keeps its comments.
The [configuration reference](../reference/configuration.md) lists every
section and key.

**A configuration is checked every time it is read.** An unknown key, a
misspelled section or a precision that is not valid is an error that names
where it is and suggests the nearest valid name. A configuration with an error
is set aside, and the command says so: the samples it covers are then read
with the default settings, never with a half-read configuration.

## The nearest configuration

**A sample follows the nearest `.samplekitrc` above it**: in its own folder
first, then in each folder above, up to the first one found. That one applies,
with what it imports (below), and no other.

So a project's configuration covers its subfolders too, unless a subfolder
has a `.samplekitrc` of its own; that subfolder is then a **separate
project**, with its own units, model and queries. A new project made inside
another one is a project of its own; the TUI's setup says which project is
above it.

**A sample follows its project wherever a command runs from.** What it shows,
computes, filters, exports and draws is the same whether you run the command
in its folder, from the folder above several projects, or name its file alone.

## Importing what several projects share

Several collections often share their quantities, their units and a few
tables: the same `[property.*]`, the same profiles. Rather than copying them
into each `.samplekitrc`, **a configuration can import another**:

```toml
# samples/ana/.samplekitrc
schema_version = 1
import = ".."             # the .samplekitrc of the folder above

[model]
path = "../model/ana.py"

[property.og]
precision = ".4f"         # finer than the shared one; its unit and symbol stay
```

- **`import` names a folder**, relative to the file (or absolute, or starting
  with `~`), and takes that folder's `.samplekitrc`. The folder above keeps
  its own file for its own samples, and holds what is common.
- **Imports chain**: a collection can import `data/`, which imports the root.
  A loop is refused, naming the files.
- **Everything is imported but the model**: `[model] path` and `class` stay
  each project's own. The rest (`[render]`, `[collection]`, units, properties,
  styles, queries, profiles, exports, figures, `[matplotlib]`, the TUI's
  colours, and `[model] python`) is imported.
- **Merged key by key**, the importing file winning. Redeclaring
  `[property.og] precision` changes that precision and keeps the imported unit
  and symbol; redeclaring a profile's `sort` keeps its imported columns. A
  list, such as `exclude = [...]`, replaces the imported one whole.
- **A relative path reads from the file that declares it.** An export
  imported from the root writes where the root's file says, and `[collection]
  files` imported from it looks in the root's folders. `{collection}` stands
  for the collection's folder: `files = ["{collection}/photos/{name}*"]`
  declared at the root finds each collection's own photos, and `output =
  "exports/{collection}/og.csv"` gives each collection its own file. Two
  collections that would write the same file are refused before either is
  written.

`samplekit --show-rc` names the files a configuration imports, and
`samplekit validate` notes a declaration that repeats the imported one: a
copy to remove.

Why only by `import`: a configuration merged without being asked is one
nobody wrote. If a precision could come from the folder above on its own, a
sample's table would change when someone edited a file two levels up, and
nothing in the project you are working on would say why. An `import` line is
in the file you are reading.

| To… | Use |
| --- | --- |
| see which configuration, model and Python a command would use | `samplekit --show-rc FOLDER` |
| name the configurations in use on any command | `-v` |
| use one configuration for every sample, instead of the nearest | `--rc PATH` |

## Which folders a command reads

A command reads **the folder you name, and only it**: by default, not its
subfolders. The scope is a question of what you are asking, so it is widened
on the command line, by naming more (`samplekit ana/brews tom/brews`), rather
than set once and forgotten.

A project can say that its subfolders are part of it:

```toml
[collection]
recursive = true
exclude = ["README.md", "draft-*"]
```

With `recursive = true`, naming the project's folder reads every sample below
it. `include` and `exclude` choose file names by pattern; `samplekit list
skipped` lists what an `exclude` left out, so that a pattern written too
widely is found.

When a recursive read meets a subfolder with its own `.samplekitrc`, it reads
that subfolder's samples too, but **each under its own project**: a collection
can span several projects, and SampleKit shows them apart.

## Several projects in one view

Comparing two series, or two people's notebooks, is a real need. So a
command can read samples from several projects at once:

```console
$ samplekit status ana/brews tom/brews
$ samplekit ana/brews tom/brews -c name,abv
```

**Each sample keeps its own project's rules.** Its units, precisions, model,
queries and profiles are its project's, whatever else the command reads. What
that looks like:

- **On the terminal, one table per project**, headed by the project's folder.
  A table mixing two projects' precisions and models in one column would
  compare numbers declared differently, silently. With `--group`, each project
  comes first and its groups follow: `ana · style = stout`.
- **A CSV or JSON is one table**, since a file has one header. Its rows each
  keep their project's precision. Add the field `project` (the project's
  folder name) or `path` to the columns to tell the rows apart. A profile
  written to a data format over several projects is refused, since each
  project's profile may differ: narrow the target to one project, or give
  `--rc`.
- **A query or a profile is looked up in each sample's own project.**
  `--query strong` keeps, in each project, the samples that project's `strong`
  selects. A query declared in a folder above reaches a sub-project's samples
  only when the sub-project imports it. Samples whose project lacks it are
  left out, said in one line; `-v` names each project.
- **A declared export** writes the samples of the project that declares it;
  over several projects, each writes its own.
- **Computing** runs each project's model on its own samples. When one project
  cannot run (its model is missing, say), the error names it and the others
  are still computed.
- **In the TUI**, the collection is split by project, each part headed by its
  folder's name.
- **In Python**, `sk.load("ana/brews", "tom/brews")` gives one list; each
  sample keeps its project's rules, and `to_csv()` writes each row at its own
  project's precision.

Two projects can share one model, by pointing their `[model] path` at the same
file (`path = "../shared/brew.py"`): the formulas are then the same, and each
project keeps its own configuration.

## What the current folder offers

Where you run a command decides **what is offered**, never how a sample is
read. `samplekit list profiles` (and `queries`, `exports`, `figures`), the
completion of `--profile` and `--query`, and `f` in the TUI show the
configuration of the folder you are in, or of the target you name, with what
it imports: at the root of several collections, what they have in common; in
a collection, what is common and its own. Where that configuration imports
another, each name says where it comes from:

```console
$ cd samples/ana && samplekit list profiles
overview  ../.samplekitrc
mash      local
strength  local over ../.samplekitrc
```

`local over` is a profile the collection redeclares in part. Naming several
targets lists each one's under its file.

A command that would warn several times of one thing, such as a profile that
several projects lack, says it once, in one line counting them and ending
`-v shows them`; `-v` lists each beneath it.

## Queries, profiles and groups

Three things shape what a command shows, and each answers one question:

| | Answers | Declared as | Used with |
| --- | --- | --- | --- |
| **Query** | *which samples?* | `[query.NAME] filter = '…'` | `--query NAME`, `f` in the TUI, `.query()` in Python |
| **Profile** | *which columns, in which order, sorted how, grouped how?* | `[profile.NAME]` | `--profile NAME`, `f` in the TUI, `profile=` in Python |
| **Group** | *split into which tables?* | `--group FIELD`, or a profile's `group` | `g` in the TUI, `group_by()` in Python |

A query names no columns and a profile names no samples, so one of each can be
combined: `--profile overview --query strong`. An option beside a profile
replaces one part of it: `-c` the columns, `-s` the sort, `--group` the
groups.

A group is **computed from the data** each time, never saved as a list of
samples. A set you choose by hand, the medal winners of a season, is
a **tag** on each of them, reached by a query (`tags has reference`): the tag
travels with the file, where a saved list of paths breaks when a file is
renamed.

In the TUI, `W` saves what the collection shows (its filter, columns, sort and
grouping) as a query, a profile or an export of the project, which the command
line can then use by name.

## Common mistakes

- **Expecting a folder's configuration to add to the one above by itself.**
  The nearest one applies alone, unless it says `import = ".."`.
- **Copying the shared sections into each collection.** Declare them once
  above and import them; `samplekit validate` notes the copies left.
- **Expecting a command to read subfolders by default.** Name them, or set
  `recursive = true` in the project.
- **Comparing two projects in one CSV without a `project` column.** Add it.
- **Declaring a query at the top of a tree of projects** and expecting it in
  each without an import. Import the top's configuration in each project that
  uses it.
- **An imported export writing one file for every collection.** Name the
  collection in its destination: `output = "exports/{collection}/og.csv"`.
- **Keeping a list of files as a "group".** Tag the samples instead.

## Where to go next

- Two projects side by side: tutorial step [7](../tutorial/07-two-brewers.md);
  queries and profiles: step [3](../tutorial/03-selecting.md).
- [Share a project](../how-to/share-a-project.md) with a colleague.
- Every key of `.samplekitrc`: [Configuration
  reference](../reference/configuration.md); `init`, `--rc`, `--show-rc`:
  [Command line reference](../reference/cli.md).
