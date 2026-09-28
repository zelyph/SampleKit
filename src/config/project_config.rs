//! Owns `.samplekitrc`: what a project may declare, how the file is found, and
//! how a declaration composes with what a sample already says.
//!
//! This module **declares**; the modules named beside each section execute.

use std::fmt;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::core::formatting::{Presentation, Resolved};
use crate::format::schema::PrecisionSchema;

/// The file this module owns, by name.
pub const FILENAME: &str = ".samplekitrc";

const SUPPORTED: RangeInclusive<u32> = 1..=1;

// ------------------------------------------------------------- declarations

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    root: PathBuf,
    schema_version: u32,
    model: Option<ModelDeclaration>,
    render: RenderSettings,
    collection: CollectionRules,
    units: IndexMap<String, IndexMap<String, String>>,
    properties: IndexMap<String, PropertyDeclaration>,
    styles: IndexMap<String, StyleSettings>,
    queries: IndexMap<String, NamedQuery>,
    profiles: IndexMap<String, Profile>,
    exports: IndexMap<String, ExportTarget>,
    figures: IndexMap<String, FigureDeclaration>,
    /// `[matplotlib]`: matplotlib's own settings, by their dotted names.
    /// Carried, not interpreted: matplotlib checks them when it draws.
    matplotlib: IndexMap<String, toml::Value>,
    /// `[workbench.colors]`: the workbench's colour for each role it names,
    /// where the project changes one. Checked here, drawn there.
    workbench_colors: IndexMap<String, String>,
    /// The files imported, nearest first, each once.
    imports: Vec<PathBuf>,
    /// The files declaring each query, profile, export and figure, nearest
    /// first, where an import is involved: absent where only this file does.
    origins: IndexMap<(DeclarationKind, String), Vec<PathBuf>>,
    /// This file's declarations identical to what it imports.
    copies: Vec<ImportedCopy>,
}

/// The four kinds of named declaration a listing offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Query,
    Profile,
    Export,
    Figure,
}

impl DeclarationKind {
    /// The section's word in the file: `query` for `[query.*]`.
    fn section(self) -> &'static str {
        match self {
            DeclarationKind::Query => "query",
            DeclarationKind::Profile => "profile",
            DeclarationKind::Export => "export",
            DeclarationKind::Figure => "figure",
        }
    }

    const ALL: [DeclarationKind; 4] = [
        DeclarationKind::Query,
        DeclarationKind::Profile,
        DeclarationKind::Export,
        DeclarationKind::Figure,
    ];
}

/// Where a named declaration comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredIn {
    /// The file read declares it, and nothing it imports does.
    Local,
    /// Only an imported file declares it: that file, the nearest one.
    Imported(PathBuf),
    /// Both: the file read writes over the imported file's.
    Over(PathBuf),
}

/// A declaration a file writes as the file it imports already does: the
/// section, or the section and the key — `[property.height]`,
/// `[property.height] precision` — and the imported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCopy {
    pub key: String,
    pub from: PathBuf,
}

/// The roles `[workbench.colors]` names, each with the colour the workbench
/// gives it where the project does not: the terminal's own sixteen, so that its
/// theme decides the shades.
pub const WORKBENCH_COLORS: &[(&str, &str)] = &[
    ("failed", "red"),
    ("outdated", "yellow"),
    ("edited", "magenta"),
    ("current", "green"),
    ("selected", "cyan"),
    ("attribute", "blue"),
    ("table", "magenta"),
    ("index", "cyan"),
    ("key", "cyan"),
    ("muted", "dark-grey"),
    ("message", "yellow"),
    ("error", "red"),
    ("accent", "cyan"),
];

/// The colours a role takes: the terminal's sixteen by name, `default` for
/// the terminal's own foreground, or `#rrggbb`.
pub const COLOR_NAMES: &[&str] = &[
    "default",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "grey",
    "dark-grey",
    // The American spellings, taken as the same colours.
    "gray",
    "dark-gray",
    "light-red",
    "light-green",
    "light-yellow",
    "light-blue",
    "light-magenta",
    "light-cyan",
    "white",
];

/// Whether `text` is a colour `[workbench.colors]` takes.
pub fn is_color(text: &str) -> bool {
    COLOR_NAMES.contains(&text)
        || (text.len() == 7
            && text.starts_with('#')
            && text[1..].chars().all(|digit| digit.is_ascii_hexdigit()))
}

/// `[model]`: a path relative to the configuration file, never an import path.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelDeclaration {
    pub path: PathBuf,
    pub class: Option<String>,
    /// `[model] python`: the interpreter that runs it, resolved like `path`.
    pub python: Option<PathBuf>,
}

/// `[render]`: how a value is written when nothing overrides it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderSettings {
    pub style: Option<String>,
    pub precision: Option<PrecisionSchema>,
    /// A `terminal_rendering::Style` name. Carried, not interpreted: the list
    /// of styles belongs to whoever draws, and `--table-style` overrides this.
    pub table: Option<String>,
    /// What a terminal table puts first to say whose each row is.
    pub identify: Identify,
    /// The style a figure's axes read in when it names none.
    pub figure_style: Option<String>,
}

/// `[render] identify`: the column a terminal table adds, never an export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Identify {
    /// The sample's name, with its file where two share it.
    #[default]
    Name,
    /// The sample's file name.
    Filename,
    /// No column added.
    None,
}

/// `[collection]`: which files in a directory are samples.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionRules {
    pub recursive: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Where a sample's files are looked for, relative to the configuration.
    pub files: Vec<String>,
}

impl Default for CollectionRules {
    /// `*.md`, `*.MD` and `*.markdown` in the given directory, not recursive.
    /// Non-recursive because a sample directory commonly sits next to
    /// `README.md` and `generated/`.
    fn default() -> CollectionRules {
        CollectionRules {
            recursive: false,
            include: vec![
                "*.md".to_string(),
                "*.MD".to_string(),
                "*.markdown".to_string(),
            ],
            exclude: Vec::new(),
            files: Vec::new(),
        }
    }
}

/// `[style.<name>]`: the separator, the only style-dependent literal that is
/// neither a unit nor a symbol.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleSettings {
    pub separator: Option<String>,
}

/// What a project declares about one quantity, so that no file repeats it.
/// `unit` is declared here *and* written into every file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropertyDeclaration {
    pub unit: Option<String>,
    pub symbol: Option<String>,
    pub symbol_variants: IndexMap<String, String>,
    pub precision: Option<PrecisionSchema>,
}

/// `[query.*]`: which samples. Evaluated by `named-queries`.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedQuery {
    pub name: String,
    pub filter: String,
    /// The directory it applies to when the command line names no target :
    /// named for what it is, where an earlier key said `base`.
    pub directory: Option<PathBuf>,
}

/// `[profile.*]`: a grid where a sample is a row. Rendered by `profiles`.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    pub columns: Vec<ColumnSpec>,
    /// Sort keys in order, `-` reversing one.
    pub sort: Vec<String>,
    /// The fields it groups by, in order, as `--group` names them. Held as
    /// written, as the sort keys are: the parser is a layer up.
    pub group: Vec<String>,
}

/// One field with a display name, in a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    pub field: String,
    pub label: Option<String>,
    pub header: Option<String>,
    pub precision: Option<PrecisionSchema>,
    pub template: Option<String>,
}

/// `[export.*]`: where it goes. Written by `exports`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTarget {
    pub name: String,
    pub profile: String,
    pub format: Format,
    pub output: PathBuf,
    /// The field of that name, added as a column. Spelt as the field it adds
    /// rather than `include_*`; `output` is the destination.
    pub filename: bool,
    pub path: bool,
    /// A declared query the export selects with, so that it can be run without
    /// remembering which selection it was written for.
    pub query: Option<String>,
}

/// `[figure.*]`: what a figure draws, and nothing about how it looks. Drawn by
/// `plotting`, through Python.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureDeclaration {
    pub name: String,
    pub kind: FigureKind,
    pub x: String,
    pub y: String,
    pub group: Option<String>,
    pub query: Option<String>,
    /// Written over the figure; none when not given.
    pub title: Option<String>,
    /// An axis's label, replacing the one resolved from the field.
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    /// The `[style.*]` whose unit and symbol variants label the axes; `plain`
    /// when not given, never `[render] style`, whose variants are written for
    /// documents rather than for matplotlib.
    pub style: Option<String>,
    /// An axis's bounds, either left free (`None`).
    pub x_limits: Option<Limits>,
    pub y_limits: Option<Limits>,
    /// `linear`, `log` or `symlog`, checked as the file loads.
    pub x_scale: Option<String>,
    pub y_scale: Option<String>,
    /// `equal` or `auto`.
    pub aspect: Option<String>,
    /// `best`, `outside`, `none`, or one of matplotlib's places.
    pub legend: Option<String>,
    /// Width and height in centimetres.
    pub figsize: Option<(f64, f64)>,
}

impl FigureDeclaration {
    /// A figure nobody declared: `plot -x -y` and the workbench's own.
    pub fn ad_hoc(x: &str, y: &str, kind: FigureKind, group: Option<String>) -> FigureDeclaration {
        FigureDeclaration {
            name: String::new(),
            kind,
            x: x.to_string(),
            y: y.to_string(),
            group,
            query: None,
            title: None,
            x_label: None,
            y_label: None,
            style: None,
            x_limits: None,
            y_limits: None,
            x_scale: None,
            y_scale: None,
            aspect: None,
            legend: None,
            figsize: None,
        }
    }
}

/// Where a legend may go: `best`, `outside`, `none`, and matplotlib's own
/// places.
pub const LEGENDS: [&str; 12] = [
    "best",
    "outside",
    "none",
    "upper right",
    "upper left",
    "lower left",
    "lower right",
    "right",
    "center left",
    "center right",
    "lower center",
    "upper center",
];

/// A figure's size, `[18, 12]` or `"18,12"`: two positive numbers, in
/// centimetres.
pub fn parse_figsize(written: &toml::Value) -> Result<(f64, f64), String> {
    let number = |value: &toml::Value| -> Option<f64> {
        match value {
            toml::Value::Integer(integer) => Some(*integer as f64),
            toml::Value::Float(number) => Some(*number),
            toml::Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    };
    let parts: Vec<toml::Value> = match written {
        toml::Value::Array(items) => items.clone(),
        toml::Value::String(text) => text
            .split(',')
            .map(|part| toml::Value::String(part.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    match parts.as_slice() {
        [width, height] => match (number(width), number(height)) {
            (Some(width), Some(height))
                if width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite() =>
            {
                Ok((width, height))
            }
            _ => Err(format!(
                "{written} is not a size: a width and a height in centimetres, both positive"
            )),
        },
        _ => Err(format!(
            "{written} is not a size: [width, height] in centimetres, as [18, 12]"
        )),
    }
}

/// An axis's lower and upper bounds; `None` is left to matplotlib.
pub type Limits = (Option<f64>, Option<f64>);

/// The scales a figure's axis takes.
pub const SCALES: [&str; 3] = ["linear", "log", "symlog"];
/// The aspects a figure takes.
pub const ASPECTS: [&str; 2] = ["equal", "auto"];

/// `[0, 100]`, `[0, "auto"]`, `"0,100"` or `"0,auto"`: two bounds, a free one
/// written `auto`. The refusal says what was written.
pub fn parse_limits(written: &toml::Value) -> Result<Limits, String> {
    let finite = |number: f64| -> Result<Option<f64>, String> {
        if number.is_finite() {
            Ok(Some(number))
        } else {
            Err(format!("{number} is not a bound: a finite number, or auto"))
        }
    };
    let bound = |value: &toml::Value| -> Result<Option<f64>, String> {
        match value {
            toml::Value::Integer(integer) => Ok(Some(*integer as f64)),
            toml::Value::Float(number) => finite(*number),
            toml::Value::String(text) if text.trim() == "auto" || text.trim().is_empty() => {
                Ok(None)
            }
            toml::Value::String(text) => text
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("'{text}' is not a bound: a number, or auto"))
                .and_then(finite),
            other => Err(format!("{other} is not a bound: a number, or auto")),
        }
    };
    let parts: Vec<toml::Value> = match written {
        toml::Value::Array(items) => items.clone(),
        toml::Value::String(text) => text
            .split(',')
            .map(|part| toml::Value::String(part.to_string()))
            .collect(),
        other => {
            return Err(format!(
                "{other} is not two bounds: [low, high], a free one written auto"
            ));
        }
    };
    let [low, high] = parts.as_slice() else {
        return Err(format!(
            "limits are two bounds, [low, high], a free one written auto — {} {} written",
            parts.len(),
            if parts.len() == 1 { "was" } else { "were" }
        ));
    };
    let limits = (bound(low)?, bound(high)?);
    if let (Some(low), Some(high)) = limits
        && low == high
    {
        return Err(format!("{low} to {high} is no range: two different bounds"));
    }
    Ok(limits)
}

/// matplotlib's ordinary kinds: named in the file, and refused at load when not
/// one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureKind {
    Scatter,
    Line,
    Step,
    Bar,
    /// The values of `y` over the samples sharing a value of `x`.
    Box,
}

impl FigureKind {
    pub const ALL: [FigureKind; 5] = [
        FigureKind::Scatter,
        FigureKind::Line,
        FigureKind::Step,
        FigureKind::Bar,
        FigureKind::Box,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FigureKind::Scatter => "scatter",
            FigureKind::Line => "line",
            FigureKind::Step => "step",
            FigureKind::Bar => "bar",
            FigureKind::Box => "box",
        }
    }

    /// The kind a word names, or the refusal naming the four.
    pub fn parse(word: &str) -> Result<FigureKind, String> {
        FigureKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == word)
            .ok_or_else(|| {
                format!("'{word}' is not a figure kind: scatter, line, step, bar or box")
            })
    }
}

/// Named in the file, so refused at load when it is not one of the three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Tsv,
    Json,
}

/// Which kind of thing a name was looked up as, for the error that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Query,
    Profile,
    Export,
}

impl ProfileKind {
    fn as_str(self) -> &'static str {
        match self {
            ProfileKind::Query => "query",
            ProfileKind::Profile => "profile",
            ProfileKind::Export => "export",
        }
    }
}

// ------------------------------------------------------------------ reading

/// The file's shape as serde sees it. Separate from the declarations above so
/// that validation happens once, between the two, rather than being scattered
/// through accessors.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    schema_version: Option<u32>,
    /// The folder whose `.samplekitrc` this one takes.
    #[serde(default)]
    import: Option<String>,
    #[serde(default)]
    model: Option<WrittenModel>,
    #[serde(default)]
    render: WrittenRender,
    #[serde(default)]
    collection: WrittenCollection,
    #[serde(default)]
    unit: IndexMap<String, IndexMap<String, String>>,
    #[serde(default)]
    property: IndexMap<String, WrittenProperty>,
    #[serde(default)]
    style: IndexMap<String, WrittenStyle>,
    #[serde(default)]
    query: IndexMap<String, WrittenQuery>,
    #[serde(default)]
    profile: IndexMap<String, WrittenProfile>,
    #[serde(default)]
    export: IndexMap<String, WrittenExport>,
    #[serde(default)]
    figure: IndexMap<String, WrittenFigure>,
    #[serde(default)]
    matplotlib: toml::Table,
    /// `[tui.colors]`, and `[workbench.colors]`, its name before, still read so
    /// that a project's colours survive the word's change.
    #[serde(default)]
    tui: WrittenWorkbench,
    #[serde(default)]
    workbench: WrittenWorkbench,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WrittenWorkbench {
    #[serde(default)]
    colors: IndexMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenModel {
    path: String,
    class: Option<String>,
    python: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WrittenRender {
    style: Option<String>,
    precision: Option<PrecisionSchema>,
    table: Option<String>,
    identify: Option<String>,
    figure_style: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WrittenCollection {
    recursive: Option<bool>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    files: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WrittenStyle {
    separator: Option<String>,
}

/// `symbol` and `precision` are named; every other key is a style variant of
/// the symbol, which is what lets a project add one without a format change.
#[derive(Deserialize, Default)]
struct WrittenProperty {
    unit: Option<String>,
    symbol: Option<String>,
    precision: Option<PrecisionSchema>,
    #[serde(flatten)]
    variants: IndexMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenQuery {
    filter: String,
    directory: Option<String>,
    /// The key `directory` replaced: read so that it can be refused by name,
    /// with the migration that rewrites it.
    base: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenProfile {
    #[serde(default)]
    columns: Vec<WrittenColumn>,
    #[serde(default)]
    sort: Vec<String>,
    /// A list only, as `sort` is: one spelling for a list of fields.
    #[serde(default)]
    group: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenColumn {
    field: String,
    label: Option<String>,
    header: Option<String>,
    precision: Option<PrecisionSchema>,
    template: Option<String>,
}

/// What a figure draws and how its axes read, and no more: a fit or a reference
/// line is the model's to draw, and refused here as any key the section does
/// not have.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenFigure {
    kind: Option<String>,
    x: Option<String>,
    y: Option<String>,
    group: Option<String>,
    query: Option<String>,
    title: Option<String>,
    x_label: Option<String>,
    y_label: Option<String>,
    style: Option<String>,
    x_limits: Option<toml::Value>,
    y_limits: Option<toml::Value>,
    x_scale: Option<String>,
    y_scale: Option<String>,
    aspect: Option<String>,
    legend: Option<String>,
    figsize: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenExport {
    profile: String,
    format: String,
    output: String,
    filename: Option<bool>,
    path: Option<bool>,
    query: Option<String>,
}

// ---------------------------------------------------------------- functions

/// The nearest `.samplekitrc`, searching upward from a file or directory. The
/// nearest one wins and nothing else is merged but what it imports: a
/// configuration inherited implicitly means a query behaves differently
/// depending on which directory it was launched from, and the reason is
/// invisible; an import is written where the reader looks.
pub fn find(start: &Path) -> Option<PathBuf> {
    // **Absolute first.** The search walks upward to the filesystem root, and
    // `.` has no parent to pop: a relative start would stop at the first
    // directory, so `cd` into a sample directory and the project above it
    // becomes invisible.
    let start = &dunce::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let mut at = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        let candidate = at.join(FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !at.pop() {
            return None;
        }
    }
}

/// Absence is not an error: a bare directory of samples is a valid project
/// with no configuration.
pub fn load_for(start: &Path) -> Result<Option<ProjectConfig>, ConfigError> {
    match find(start) {
        None => Ok(None),
        Some(path) => load(&path).map(Some),
    }
}

/// The whole file, validated before anything is returned: an error anywhere
/// means no configuration rather than a partial one.
pub fn load(path: &Path) -> Result<ProjectConfig, ConfigError> {
    let text = read(path)?;
    parse(path, &text)
}

fn read(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// A configuration's text, read as `load` reads the file at `path`: what an
/// edit is checked with before it is written. What it
/// imports is read from the files it names.
pub fn parse(path: &Path, text: &str) -> Result<ProjectConfig, ConfigError> {
    parse_within(path, text, &mut Vec::new())
}

/// `parse`, `importing` holding the files whose imports are being read, so
/// that one met twice is a loop rather than a recursion without end.
fn parse_within(
    path: &Path,
    text: &str,
    importing: &mut Vec<PathBuf>,
) -> Result<ProjectConfig, ConfigError> {
    let mut written = written_of(path, text)?;
    let root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    if written.import.is_none() {
        // What this file says of its own collection, `{collection}` being
        // its own folder.
        place_collection_in_written(&mut written);
        return build(path, root, written);
    }
    let head = folder_of(path);
    let gathered = gather(path, text, &head, importing)?;
    let merged: Written =
        toml::Value::Table(gathered.table)
            .try_into()
            .map_err(|error: toml::de::Error| ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: format!("{}, with what it imports", withdrawn_key(error.message())),
            })?;
    let mut config = build(path, root, merged)?;
    config.imports = gathered.imports;
    config.copies = gathered.copies;
    config.origins = gathered
        .declared
        .into_iter()
        .filter(|(_, files)| files.len() > 1 || files.first() != Some(&head.join(FILENAME)))
        .collect();
    Ok(config)
}

/// The file's shape as serde reads it, with the line of what is wrong.
fn written_of(path: &Path, text: &str) -> Result<Written, ConfigError> {
    toml::from_str(text).map_err(|error| ConfigError::Parse {
        path: path.to_path_buf(),
        line: error.span().map(|span| line_of(text, span.start)),
        message: withdrawn_key(error.message()),
    })
}

// ---------------------------------------------------------------- imports

/// A file's declarations with what it imports merged beneath them, its paths
/// read from its own folder.
struct Gathered {
    table: toml::Table,
    /// The files imported, nearest first.
    imports: Vec<PathBuf>,
    /// The files declaring each named declaration, nearest first.
    declared: IndexMap<(DeclarationKind, String), Vec<PathBuf>>,
    /// This file's declarations identical to what it imports.
    copies: Vec<ImportedCopy>,
}

/// The folder a configuration file is in, absolute and resolved, so that two
/// spellings of one file are one file.
fn folder_of(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let folder = absolute
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    dunce::canonicalize(&folder).unwrap_or(folder)
}

/// `path`'s declarations merged over what it imports, for the collection
/// whose folder is `head`: `{collection}` is written from each file to it.
fn gather(
    path: &Path,
    text: &str,
    head: &Path,
    importing: &mut Vec<PathBuf>,
) -> Result<Gathered, ConfigError> {
    let here = folder_of(path);
    let file = here.join(FILENAME);
    if let Some(at) = importing.iter().position(|held| *held == file) {
        let mut files = importing[at..].to_vec();
        files.push(file);
        return Err(ConfigError::ImportLoop { files });
    }
    let mut own: toml::Table = toml::from_str(text).map_err(|error| ConfigError::Parse {
        path: path.to_path_buf(),
        line: error.span().map(|span| line_of(text, span.start)),
        message: error.message().to_string(),
    })?;
    let import = own.remove("import");
    place_collection(&mut own, &relative_path(&here, head));
    let mut declared: IndexMap<(DeclarationKind, String), Vec<PathBuf>> = IndexMap::new();
    let Some(import) = import else {
        for key in declared_in(&own) {
            declared.entry(key).or_default().push(file.clone());
        }
        return Ok(Gathered {
            table: own,
            imports: Vec::new(),
            declared,
            copies: Vec::new(),
        });
    };
    let written = import.as_str().unwrap_or_default().to_string();
    let imported = imported_file(path, &here, &written)?;
    importing.push(file.clone());
    // The file imported is a project for the samples beside it: valid on its
    // own, its errors naming it.
    let result = read(&imported).and_then(|text| {
        parse_within(&imported, &text, importing)?;
        gather(&imported, &text, head, importing)
    });
    importing.pop();
    let mut inner = result?;
    // The model is each file's own: its path and its class are not imported.
    if let Some(toml::Value::Table(model)) = inner.table.get_mut("model") {
        model.remove("path");
        model.remove("class");
        if model.is_empty() {
            inner.table.remove("model");
        }
    }
    let there = folder_of(&imported);
    rebase(&mut inner.table, &here, &there);
    let copies = copies_of(&own, &inner.table, &imported);
    for key in declared_in(&own) {
        declared.entry(key).or_default().push(file.clone());
    }
    for (key, files) in inner.declared {
        declared.entry(key).or_default().extend(files);
    }
    let mut table = inner.table;
    merge(&mut table, own);
    // `[model] python` alone, its model not declared here, runs nothing.
    if let Some(toml::Value::Table(model)) = table.get("model")
        && !model.contains_key("path")
    {
        table.remove("model");
    }
    let mut imports = vec![imported];
    imports.extend(inner.imports);
    Ok(Gathered {
        table,
        imports,
        declared,
        copies,
    })
}

/// The `.samplekitrc` an `import` names: a folder, relative to the file
/// importing it, absolute, or from the home folder.
fn imported_file(path: &Path, here: &Path, written: &str) -> Result<PathBuf, ConfigError> {
    let refused = |reason: String| ConfigError::ImportNotFound {
        path: path.to_path_buf(),
        import: written.to_string(),
        reason,
    };
    if written.trim().is_empty() {
        return Err(refused(
            "it names no folder: import = \"..\" takes the .samplekitrc of the folder above"
                .to_string(),
        ));
    }
    let folder = if written == "~" || written.starts_with("~/") {
        let home = etcetera::home_dir()
            .map_err(|_| refused("the home folder is not known here".to_string()))?;
        home.join(written.trim_start_matches('~').trim_start_matches('/'))
    } else {
        here.join(written)
    };
    if folder.is_file() {
        let parent = Path::new(written)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| ".".to_string(), |parent| parent.display().to_string());
        return Err(refused(format!(
            "it names a file, and import names the folder holding one: import = \"{parent}\""
        )));
    }
    if !folder.is_dir() {
        return Err(refused(format!("{} is no folder", folder.display())));
    }
    let file = folder.join(FILENAME);
    if !file.is_file() {
        return Err(refused(format!(
            "{} holds no {FILENAME}",
            dunce::canonicalize(&folder).unwrap_or(folder).display()
        )));
    }
    Ok(dunce::canonicalize(&file).unwrap_or(file))
}

/// The named declarations a file's table holds, in its order.
fn declared_in(table: &toml::Table) -> Vec<(DeclarationKind, String)> {
    let mut found = Vec::new();
    for kind in DeclarationKind::ALL {
        if let Some(toml::Value::Table(section)) = table.get(kind.section()) {
            found.extend(section.keys().map(|name| (kind, name.clone())));
        }
    }
    found
}

/// `over` merged into `base` key by key: a table into a table, recursively;
/// anything else replaces. `over` wins, and what `base` holds keeps its place.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(held)), toml::Value::Table(value)) => merge(held, value),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// The keys `own` writes as `imported` already does: a table named whole
/// where every key of it repeats, else each key that does. The sections of
/// declarations — `[property]`, `[profile]` — are gone into, never named.
fn copies_of(own: &toml::Table, imported: &toml::Table, from: &Path) -> Vec<ImportedCopy> {
    fn within(
        own: &toml::Table,
        imported: &toml::Table,
        section: &str,
        from: &Path,
        found: &mut Vec<ImportedCopy>,
    ) {
        for (key, value) in own {
            let Some(held) = imported.get(key) else {
                continue;
            };
            let named = if section.is_empty() {
                key.clone()
            } else {
                format!("{section}.{key}")
            };
            match (value, held) {
                (toml::Value::Table(value), toml::Value::Table(held)) => {
                    if repeats(value, held) {
                        found.push(ImportedCopy {
                            key: format!("[{named}]"),
                            from: from.to_path_buf(),
                        });
                    } else {
                        within(value, held, &named, from, found);
                    }
                }
                (value, held) if value == held => found.push(ImportedCopy {
                    key: if section.is_empty() {
                        key.clone()
                    } else {
                        format!("[{section}] {key}")
                    },
                    from: from.to_path_buf(),
                }),
                _ => {}
            }
        }
    }
    fn repeats(own: &toml::Table, imported: &toml::Table) -> bool {
        own.iter()
            .all(|(key, value)| match (value, imported.get(key)) {
                (toml::Value::Table(value), Some(toml::Value::Table(held))) => repeats(value, held),
                (value, Some(held)) => value == held,
                (_, None) => false,
            })
    }
    let mut found = Vec::new();
    for (key, value) in own {
        if matches!(key.as_str(), "schema_version" | "import" | "model") {
            continue;
        }
        if let (toml::Value::Table(value), Some(toml::Value::Table(held))) =
            (value, imported.get(key))
        {
            within(value, held, key, from, &mut found);
        } else if imported.get(key) == Some(value) {
            found.push(ImportedCopy {
                key: key.clone(),
                from: from.to_path_buf(),
            });
        }
    }
    found
}

/// The paths a file declares, each read from the folder of the file declaring
/// it: `[collection] files`, `[export.*] output`, `[query.*] directory` and
/// `[model] python`. `include` and `exclude` are patterns on the collection's
/// own files, not paths, and `[matplotlib]` is matplotlib's.
fn path_values(table: &mut toml::Table, mut each: impl FnMut(&mut String, bool)) {
    if let Some(toml::Value::Table(collection)) = table.get_mut("collection")
        && let Some(toml::Value::Array(files)) = collection.get_mut("files")
    {
        for file in files {
            if let toml::Value::String(file) = file {
                each(file, true);
            }
        }
    }
    if let Some(toml::Value::Table(exports)) = table.get_mut("export") {
        for (_, export) in exports.iter_mut() {
            if let toml::Value::Table(export) = export
                && let Some(toml::Value::String(output)) = export.get_mut("output")
                // `-` is the standard output, no path.
                && output != "-"
            {
                each(output, true);
            }
        }
    }
    if let Some(toml::Value::Table(queries)) = table.get_mut("query") {
        for (_, query) in queries.iter_mut() {
            if let toml::Value::Table(query) = query
                && let Some(toml::Value::String(directory)) = query.get_mut("directory")
            {
                each(directory, false);
            }
        }
    }
    if let Some(toml::Value::Table(model)) = table.get_mut("model")
        && let Some(toml::Value::String(python)) = model.get_mut("python")
    {
        each(python, false);
    }
}

/// The placeholder for the collection's folder.
const COLLECTION: &str = "{collection}";

/// `{collection}` written as the collection's folder, seen from the file
/// declaring it: `data/c1` from the root, `.` in the collection's own.
fn place_collection(table: &mut toml::Table, collection: &Path) {
    let collection = slashed(collection);
    path_values(table, |value, placed| {
        if placed && value.contains(COLLECTION) {
            *value = normalised(&value.replace(COLLECTION, &collection));
        }
    });
}

/// The same, for a file that imports nothing: `{collection}` is its folder.
fn place_collection_in_written(written: &mut Written) {
    let place = |value: &mut String| {
        if value.contains(COLLECTION) {
            *value = normalised(&value.replace(COLLECTION, "."));
        }
    };
    if let Some(files) = written.collection.files.as_mut() {
        files.iter_mut().for_each(place);
    }
    for export in written.export.values_mut() {
        place(&mut export.output);
    }
}

/// Each relative path of an imported file, declared in `there`, written as
/// seen from `here`, the importing file's folder: the place it names, by the
/// words — a glob's parts are words too.
fn rebase(table: &mut toml::Table, here: &Path, there: &Path) {
    if here == there {
        return;
    }
    let words = |path: &Path| -> Vec<String> {
        path.components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect()
    };
    let (here, there) = (words(here), words(there));
    path_values(table, |value, _| {
        if Path::new(value.as_str()).is_absolute() || value.starts_with('~') {
            return;
        }
        // The place, from the root of the system: `there` and the value.
        let mut place = there.clone();
        for part in value.split('/') {
            match part {
                "" | "." => {}
                // Never above the root of the system, which `there` begins with.
                ".." if place.len() > 1 => {
                    place.pop();
                }
                ".." => {}
                part => place.push(part.to_string()),
            }
        }
        let common = here
            .iter()
            .zip(&place)
            .take_while(|(one, other)| one == other)
            .count();
        let mut seen: Vec<String> = vec!["..".to_string(); here.len() - common];
        seen.extend(place[common..].iter().cloned());
        *value = if seen.is_empty() {
            ".".to_string()
        } else {
            seen.join("/")
        };
    });
}

/// `to` seen from `from`, both absolute: `../..`, `data/c1`, or `.`.
fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    if from.first() != to.first() {
        // Another drive: nothing leads from one to the other.
        return to.iter().collect();
    }
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(one, other)| one == other)
        .count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for part in &to[common..] {
        path.push(part.as_os_str());
    }
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path
    }
}

/// A path as a pattern writes it, `/` between its parts on every system.
fn slashed(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `a/./b` as `a/b`, and `a/../b` as `b`, by the words alone: what a path
/// rebased or a placeholder replaced writes, as a person would have.
fn normalised(written: &str) -> String {
    let absolute = written.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in written.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

/// A key a later decision withdrew, said as withdrawn rather than listed
/// against the keys that remain, which says what to delete and not why.
fn withdrawn_key(message: &str) -> String {
    if message.starts_with("unknown field `bounds`") {
        return "[render] bounds was withdrawn: a declared precision is applied as written \
                on every surface, and an uncertainty it rounds to zero is written so \
                — delete the key; the upgrading guide lists what changed"
            .to_string();
    }
    message.to_string()
}

/// The line a byte offset falls on, one-based, so that a message points where
/// the reader can look: the newlines before it, and one. Counting the lines
/// before it named the line above an error that starts a line.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

// --------------------------------------------------------------- validation

/// Every profile is validated at load, including ones this run will never use.
/// A typo in an export used once a month should be reported the next time
/// *anything* in the project is loaded, not thirty days later.
fn build(path: &Path, root: PathBuf, written: Written) -> Result<ProjectConfig, ConfigError> {
    let schema_version = written.schema_version.ok_or(ConfigError::MissingVersion {
        path: path.to_path_buf(),
    })?;
    if !SUPPORTED.contains(&schema_version) {
        return Err(ConfigError::UnsupportedVersion {
            path: path.to_path_buf(),
            found: schema_version,
            supported: SUPPORTED,
        });
    }

    let model = written.model.map(|model| ModelDeclaration {
        path: PathBuf::from(model.path),
        class: model.class,
        python: model.python.map(PathBuf::from),
    });
    let defaults = CollectionRules::default();
    let collection = CollectionRules {
        recursive: written.collection.recursive.unwrap_or(defaults.recursive),
        include: written.collection.include.unwrap_or(defaults.include),
        exclude: written.collection.exclude.unwrap_or(defaults.exclude),
        files: written.collection.files.unwrap_or_default(),
    };

    let mut properties = IndexMap::new();
    for (name, declared) in written.property {
        // Checked as the file loads, as a column's precision is: a specifier
        // that cannot apply would otherwise be ignored without a word.
        if let Some(precision) = &declared.precision {
            check_precision(path, &format!("property.{name}"), None, precision)?;
        }
        // Every other key is a style's symbol, `symbol_<style>`: `precison` is
        // a typo, not a variant.
        if let Some(key) = declared
            .variants
            .keys()
            .find(|key| !key.starts_with("symbol_"))
        {
            return Err(ConfigError::UnknownKey {
                path: path.to_path_buf(),
                section: format!("property.{name}"),
                suggestion: crate::core::identifier::nearest(key, ["unit", "symbol", "precision"]),
                key: key.clone(),
            });
        }
        properties.insert(
            name,
            PropertyDeclaration {
                unit: declared.unit,
                symbol: declared.symbol,
                symbol_variants: declared.variants,
                precision: declared.precision,
            },
        );
    }

    let mut queries = IndexMap::new();
    for (name, query) in written.query {
        // A key that said the wrong thing is refused by name, with the way
        // forward, rather than accepted as a spelling.
        if query.base.is_some() {
            return Err(ConfigError::RenamedKey {
                path: path.to_path_buf(),
                section: format!("query.{name}"),
                old: "base",
                new: "directory",
            });
        }
        queries.insert(
            name.clone(),
            NamedQuery {
                name,
                filter: query.filter,
                directory: query.directory.map(PathBuf::from),
            },
        );
    }

    let mut profiles = IndexMap::new();
    for (name, profile) in written.profile {
        let columns = columns(path, "profile", &name, profile.columns)?;
        // A profile of no column renders nothing and says nothing.
        if columns.is_empty() {
            return Err(ConfigError::InvalidProfile {
                path: path.to_path_buf(),
                profile: name.clone(),
                reason: "it declares no column".to_string(),
            });
        }
        check_headers(&name, &columns)?;
        // A field left empty groups by nothing anyone can name.
        if profile.group.iter().any(|field| field.trim().is_empty()) {
            return Err(ConfigError::InvalidProfile {
                path: path.to_path_buf(),
                profile: name.clone(),
                reason: "group names an empty field: group = [\"style\"] names one".to_string(),
            });
        }
        profiles.insert(
            name.clone(),
            Profile {
                name,
                columns,
                sort: profile.sort,
                group: profile.group,
            },
        );
    }

    let mut exports = IndexMap::new();
    for (name, export) in written.export {
        let format = match export.format.as_str() {
            "csv" => Format::Csv,
            "tsv" => Format::Tsv,
            "json" => Format::Json,
            other => {
                return Err(ConfigError::InvalidProfile {
                    path: path.to_path_buf(),
                    profile: name.clone(),
                    reason: format!("'{other}' is not a format: csv, tsv or json"),
                });
            }
        };
        // An export naming an undeclared profile is caught here, at load,
        // rather than on the day the export runs.
        if !profiles.contains_key(&export.profile) {
            let available: Vec<String> = profiles.keys().cloned().collect();
            return Err(ConfigError::UnknownProfile {
                kind: ProfileKind::Profile,
                suggestion: crate::core::identifier::nearest(
                    &export.profile,
                    available.iter().map(String::as_str),
                ),
                name: export.profile.clone(),
                available,
            });
        }
        // A query it names must be declared too.
        if let Some(query) = &export.query
            && !queries.contains_key(query)
        {
            let available: Vec<String> = queries.keys().cloned().collect();
            return Err(ConfigError::UnknownProfile {
                kind: ProfileKind::Query,
                suggestion: crate::core::identifier::nearest(
                    query,
                    available.iter().map(String::as_str),
                ),
                name: query.clone(),
                available,
            });
        }
        exports.insert(
            name.clone(),
            ExportTarget {
                name,
                profile: export.profile,
                format,
                output: PathBuf::from(export.output),
                filename: export.filename.unwrap_or(false),
                path: export.path.unwrap_or(false),
                query: export.query,
            },
        );
    }

    let mut figures = IndexMap::new();
    for (name, figure) in written.figure {
        let refused = |reason: String| ConfigError::InvalidProfile {
            path: path.to_path_buf(),
            profile: format!("figure.{name}"),
            reason,
        };
        let kind = match figure.kind.as_deref() {
            None => FigureKind::Scatter,
            Some(word) => FigureKind::parse(word).map_err(refused)?,
        };
        let (Some(x), Some(y)) = (figure.x, figure.y) else {
            return Err(refused("a figure names both its axes, x and y".to_string()));
        };
        // A query it names must be declared, as an export's must.
        if let Some(query) = &figure.query
            && !queries.contains_key(query)
        {
            let available: Vec<String> = queries.keys().cloned().collect();
            return Err(ConfigError::UnknownProfile {
                kind: ProfileKind::Query,
                suggestion: crate::core::identifier::nearest(
                    query,
                    available.iter().map(String::as_str),
                ),
                name: query.clone(),
                available,
            });
        }
        // A style naming nothing is refused as the file loads, as `[render]
        // style` is.
        if let Some(style) = &figure.style
            && style != "plain"
            && !written.style.contains_key(style)
        {
            return Err(refused(
                ConfigError::UnknownStyle {
                    name: style.clone(),
                    available: written.style.keys().cloned().collect(),
                }
                .to_string(),
            ));
        }
        let limits = |key: &str, written: Option<toml::Value>| {
            written
                .map(|value| {
                    parse_limits(&value).map_err(|reason| refused(format!("{key}: {reason}")))
                })
                .transpose()
        };
        let x_limits = limits("x_limits", figure.x_limits)?;
        let y_limits = limits("y_limits", figure.y_limits)?;
        for (key, value, accepted) in [
            ("x_scale", &figure.x_scale, &SCALES[..]),
            ("y_scale", &figure.y_scale, &SCALES[..]),
            ("aspect", &figure.aspect, &ASPECTS[..]),
            ("legend", &figure.legend, &LEGENDS[..]),
        ] {
            if let Some(value) = value
                && !accepted.contains(&value.as_str())
            {
                return Err(refused(format!(
                    "{key} = '{value}' is not one of {}",
                    accepted.join(", ")
                )));
            }
        }
        // A log axis holds no bound at or below zero, which matplotlib would
        // ignore with a warning.
        for (axis, scale, limits) in [
            ("x", &figure.x_scale, &x_limits),
            ("y", &figure.y_scale, &y_limits),
        ] {
            let bounds = limits.map(|(low, high)| [low, high]).unwrap_or_default();
            if scale.as_deref() == Some("log")
                && let Some(bound) = bounds.into_iter().flatten().find(|bound| *bound <= 0.0)
            {
                return Err(refused(format!(
                    "{axis}_limits has {bound}, and a log scale holds no bound at or below \
                     zero: give a positive one, or auto"
                )));
            }
        }
        let figsize = figure
            .figsize
            .as_ref()
            .map(|written| {
                parse_figsize(written).map_err(|reason| refused(format!("figsize: {reason}")))
            })
            .transpose()?;
        figures.insert(
            name.clone(),
            FigureDeclaration {
                name,
                kind,
                x,
                y,
                group: figure.group,
                query: figure.query,
                title: figure.title,
                x_label: figure.x_label,
                y_label: figure.y_label,
                style: figure.style,
                x_limits,
                y_limits,
                x_scale: figure.x_scale,
                y_scale: figure.y_scale,
                aspect: figure.aspect,
                legend: figure.legend,
                figsize,
            },
        );
    }

    // Refused as the file loads, as `--style` is refused: a style naming
    // nothing would otherwise pass until the first value is rendered.
    if let Some(style) = &written.render.style
        && !style.is_empty()
        && style != "plain"
        && !written.style.contains_key(style)
    {
        // Named with its file, as every other error of a loading file is.
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: ConfigError::UnknownStyle {
                name: style.clone(),
                available: written.style.keys().cloned().collect(),
            }
            .to_string(),
        });
    }

    if let Some(style) = &written.render.figure_style
        && style != "plain"
        && !written.style.contains_key(style)
    {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[render] figure_style: {}",
                ConfigError::UnknownStyle {
                    name: style.clone(),
                    available: written.style.keys().cloned().collect(),
                }
            ),
        });
    }

    let identify = match written.render.identify.as_deref() {
        None | Some("name") => Identify::Name,
        Some("filename") => Identify::Filename,
        Some("none") => Identify::None,
        Some(other) => {
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: format!("[render] identify is name, filename or none, not '{other}'"),
            });
        }
    };
    Ok(ProjectConfig {
        root,
        schema_version,
        model,
        render: RenderSettings {
            style: written.render.style,
            precision: written.render.precision,
            table: written.render.table,
            identify,
            figure_style: written.render.figure_style,
        },
        collection,
        units: written.unit,
        properties,
        styles: written
            .style
            .into_iter()
            .map(|(name, style)| {
                (
                    name,
                    StyleSettings {
                        separator: style.separator,
                    },
                )
            })
            .collect(),
        queries,
        profiles,
        exports,
        figures,
        matplotlib: matplotlib_settings(path, written.matplotlib)?,
        workbench_colors: tui_colors(path, written.workbench.colors, written.tui.colors)?,
        imports: Vec::new(),
        origins: IndexMap::new(),
        copies: Vec::new(),
    })
}

/// The name `outdated` had as a role before it was renamed: still read, as the same
/// colour, so that a project's colours survive the word's change.
const OUTDATED_FORMERLY: &str = "stale";

/// `[tui.colors]` over `[workbench.colors]`, the section's name before Each
/// read as its own section, and a role both write taking the `[tui.colors]`
/// colour. The file is not rewritten: a project's colours keep colouring under
/// either name.
fn tui_colors(
    path: &Path,
    former: IndexMap<String, String>,
    written: IndexMap<String, String>,
) -> Result<IndexMap<String, String>, ConfigError> {
    let mut colors = section_colors(path, "workbench.colors", former)?;
    for (role, color) in section_colors(path, "tui.colors", written)? {
        colors.insert(role, color);
    }
    Ok(colors)
}

/// One colours section, each role one the TUI names and each colour one it
/// can draw: refused at load, as any key a section does not have, naming the
/// section it was written in. `stale` is read as `outdated`, which wins where
/// both are written.
fn section_colors(
    path: &Path,
    section: &str,
    written: IndexMap<String, String>,
) -> Result<IndexMap<String, String>, ConfigError> {
    for (role, color) in &written {
        let refused = |message: String| ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message,
        };
        if role != OUTDATED_FORMERLY && !WORKBENCH_COLORS.iter().any(|(known, _)| known == role) {
            let roles: Vec<&str> = WORKBENCH_COLORS.iter().map(|(role, _)| *role).collect();
            return Err(refused(format!(
                "[{section}] has no role '{role}'\n  the roles: {}",
                roles.join(", ")
            )));
        }
        if !is_color(color) {
            return Err(refused(format!(
                "[{section}] {role} = \"{color}\" is not a colour\n  \
                 one of {}, or #rrggbb",
                COLOR_NAMES.join(", ")
            )));
        }
    }
    let written_outdated = written.contains_key("outdated");
    Ok(written
        .into_iter()
        .filter_map(|(role, color)| match role.as_str() {
            OUTDATED_FORMERLY if written_outdated => None,
            OUTDATED_FORMERLY => Some(("outdated".to_string(), color)),
            _ => Some((role, color)),
        })
        .collect())
}

/// `[matplotlib]` as matplotlib names its settings: a key written unquoted,
/// `font.size = 13`, nests in TOML and is read back as the name it spells. A
/// table left over is a setting nobody names.
fn matplotlib_settings(
    path: &Path,
    written: toml::Table,
) -> Result<IndexMap<String, toml::Value>, ConfigError> {
    fn flatten(
        prefix: &str,
        table: toml::Table,
        into: &mut IndexMap<String, toml::Value>,
        twice: &mut Option<String>,
    ) {
        for (key, value) in table {
            let name = if prefix.is_empty() {
                key
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                toml::Value::Table(nested) => flatten(&name, nested, into, twice),
                other => {
                    if into.insert(name.clone(), other).is_some() && twice.is_none() {
                        *twice = Some(name);
                    }
                }
            }
        }
    }
    let mut settings = IndexMap::new();
    let mut twice = None;
    flatten("", written, &mut settings, &mut twice);
    if let Some(name) = twice {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[matplotlib] {name} is written twice, once dotted and once as a table: \
                 one of them would be dropped"
            ),
        });
    }
    if let Some(name) = ["backend", "interactive"]
        .into_iter()
        .find(|name| settings.contains_key(*name))
    {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[matplotlib] {name} is not a figure's: samplekit chooses between a window \
                 and a file, and MPLBACKEND chooses the window's backend"
            ),
        });
    }
    if let Some((name, _)) = settings
        .iter()
        .find(|(_, value)| matches!(value, toml::Value::Datetime(_)))
    {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!("[matplotlib] {name}: a date is not a matplotlib setting"),
        });
    }
    Ok(settings)
}

/// A specifier [`crate::core::formatting`] refuses, named with where it was
/// written. Accepted, it would be dropped at the first rendering and the
/// default precision shown in its place, without a word.
fn check_precision(
    path: &Path,
    section: &str,
    field: Option<&str>,
    precision: &PrecisionSchema,
) -> Result<(), ConfigError> {
    use crate::core::formatting::Precision;
    let checked = match precision {
        PrecisionSchema::Both(spec) => Precision::both(spec).map(|_| ()),
        PrecisionSchema::Split(value, uncertainty) => {
            Precision::split(value, uncertainty).map(|_| ())
        }
    };
    checked.map_err(|error| ConfigError::InvalidPrecision {
        path: path.to_path_buf(),
        section: section.to_string(),
        reason: match field {
            Some(field) => format!("column '{field}': {error}"),
            None => error.to_string(),
        },
    })
}

/// The specifiers a template writes, `{value:.3f}`, checked as a column's
/// `precision` is. The grammar is `terminal_rendering::fill`'s: a brace whose
/// name is not a channel is LaTeX's and is left alone.
fn check_template(
    path: &Path,
    section: &str,
    field: &str,
    template: &str,
) -> Result<(), ConfigError> {
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let placeholder = &after[..close];
        let Some((name, spec)) = placeholder.split_once(':') else {
            rest = after;
            continue;
        };
        let refused = |reason: String| ConfigError::InvalidPrecision {
            path: path.to_path_buf(),
            section: section.to_string(),
            reason: format!("column '{field}', template placeholder '{{{placeholder}}}': {reason}"),
        };
        match name {
            "value" | "uncertainty" | "u" => {
                crate::core::formatting::Precision::both(spec)
                    .map_err(|error| refused(error.to_string()))?;
            }
            "unit" | "symbol" => {
                return Err(refused(format!(
                    "'{name}' is text and takes no specifier: write {{{name}}}"
                )));
            }
            _ => {}
        }
        rest = after;
    }
    Ok(())
}

fn columns(
    path: &Path,
    kind: &str,
    profile: &str,
    written: Vec<WrittenColumn>,
) -> Result<Vec<ColumnSpec>, ConfigError> {
    let mut columns = Vec::new();
    let section = format!("{kind}.{profile}");
    for column in written {
        if let Some(precision) = &column.precision {
            check_precision(path, &section, Some(&column.field), precision)?;
        }
        if let Some(template) = &column.template {
            check_template(path, &section, &column.field, template)?;
        }
        if column.label.as_deref() == Some("") {
            return Err(ConfigError::EmptyHeader {
                profile: profile.to_string(),
                field: column.field,
            });
        }
        if column.header.as_deref() == Some("") {
            return Err(ConfigError::EmptyHeader {
                profile: profile.to_string(),
                field: column.field,
            });
        }
        // With an entry per field there is no length to validate, but a field
        // declared twice is still a mistake: which of the two wins is a
        // question with no good answer.
        if columns
            .iter()
            .any(|existing: &ColumnSpec| existing.field == column.field)
        {
            return Err(ConfigError::DuplicateField {
                profile: profile.to_string(),
                field: column.field,
            });
        }
        columns.push(ColumnSpec {
            field: column.field,
            label: column.label,
            header: column.header,
            precision: column.precision,
            template: column.template,
        });
        let _ = path;
    }
    Ok(columns)
}

/// Two columns that would emit one header name collide in the exported file,
/// where nothing could tell them apart afterwards.
fn check_headers(profile: &str, columns: &[ColumnSpec]) -> Result<(), ConfigError> {
    // Compared **after flattening**, which is the only comparison that catches
    // the real case: `malt` and `malt.u` declare different headers and emit
    // `malt_uncertainty` twice.
    let shape = Profile {
        name: profile.to_string(),
        columns: columns.to_vec(),
        sort: Vec::new(),
        group: Vec::new(),
    };
    let mut seen: Vec<(String, String)> = Vec::new();
    for column in columns {
        for header in shape.headers_for(column, true) {
            if let Some((_, first)) = seen.iter().find(|(name, _)| *name == header) {
                return Err(ConfigError::DuplicateHeader {
                    profile: profile.to_string(),
                    header,
                    fields: vec![first.clone(), column.field.clone()],
                });
            }
            seen.push((header, column.field.clone()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- accessors

impl ProjectConfig {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn model(&self) -> Option<&ModelDeclaration> {
        self.model.as_ref()
    }

    pub fn render(&self) -> &RenderSettings {
        &self.render
    }

    pub fn collection(&self) -> &CollectionRules {
        &self.collection
    }

    /// A path relative to the configuration's directory, so that a project can
    /// be moved or copied whole.
    pub fn resolve(&self, relative: &str) -> PathBuf {
        let path = Path::new(relative);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    /// What a project declares about one quantity, by its address.
    pub fn property(&self, quantity: &str) -> Option<&PropertyDeclaration> {
        self.properties.get(quantity)
    }

    /// The quantities `[property.*]` declares, in declaration order.
    pub fn property_names(&self) -> Vec<&str> {
        self.properties.keys().map(String::as_str).collect()
    }

    /// How a unit is displayed, keyed by the spelling files write.
    pub fn unit(&self, spelling: &str) -> Option<&IndexMap<String, String>> {
        self.units.get(spelling)
    }

    /// The project's unit vocabulary, for a caller reporting a spelling outside
    /// it. Empty when the project declares none, which is measured against
    /// nothing.
    pub fn unit_vocabulary(&self) -> Vec<&str> {
        self.units.keys().map(String::as_str).collect()
    }

    /// The literals for one quantity, in the style in force.
    ///
    /// `quantity` is the address — `abv`, or `fermentation.gravity` —
    /// which is the key into `[property.*]`. The presentation comes in **as the
    /// file wrote it**, already composed with any column override, and what the
    /// file omits is filled from the declaration: that is the whole point,
    /// 50 quantities declared once instead of 39 090 times.
    ///
    /// **The `plain` style is a style.** `[unit."g/L"] plain = "g/L"`
    /// declares how that key displays when nothing more exotic is asked for, so
    /// the plain lookup happens like any other — skipping it would make the
    /// most common declaration in the table the one that never takes effect.
    pub fn resolved(
        &self,
        presentation: &Presentation,
        quantity: &str,
    ) -> Result<Resolved, ConfigError> {
        self.resolved_as(presentation, quantity, None)
    }

    /// Resolve one quantity under a transient semantic style. An explicit
    /// style belongs to the invocation and never mutates `[render] style`.
    pub fn resolved_as(
        &self,
        presentation: &Presentation,
        quantity: &str,
        style: Option<&str>,
    ) -> Result<Resolved, ConfigError> {
        let style = match style.or(self.render.style.as_deref()) {
            None | Some("") => "plain",
            Some(name) => name,
        };
        // A style naming nothing declared is an error. `plain` is the
        // exception, and needs no declaration: it is the absence of a style.
        if style != "plain" && !self.styles.contains_key(style) {
            return Err(ConfigError::UnknownStyle {
                name: style.to_string(),
                available: self.styles.keys().cloned().collect(),
            });
        }
        let declared = self.property(quantity);
        let unit = presentation
            .unit
            .clone()
            .or_else(|| declared.and_then(|d| d.unit.clone()));
        let symbol = presentation
            .symbol
            .clone()
            .or_else(|| declared.and_then(|d| d.symbol.clone()));
        let precision = presentation.precision.clone().or_else(|| {
            declared
                .and_then(|d| d.precision.as_ref())
                .and_then(PrecisionSchema::precision)
        });

        // A lookup that finds nothing falls back to the spelling itself, which
        // is legible by construction.
        let unit = unit.map(|spelling| match self.units.get(&spelling) {
            Some(variants) => variants.get(style).cloned().unwrap_or(spelling),
            None => spelling,
        });
        // A variant is keyed as it is written: `symbol_math` for the `math`
        // style, which is what makes adding a style a declaration rather than
        // a format change.
        let symbol = match (style, declared) {
            ("plain", _) => symbol,
            (_, Some(declaration)) => declaration
                .symbol_variants
                .get(&format!("symbol_{style}"))
                .cloned()
                .or(symbol),
            _ => symbol,
        };
        let separator = self
            .styles
            .get(style)
            .and_then(|settings| settings.separator.clone())
            .unwrap_or_else(|| "\u{b1}".to_string());
        Ok(Resolved {
            unit,
            symbol,
            separator,
            precision,
        })
    }

    /// The built-in style followed by project declarations in author order.
    pub fn render_style_names(&self) -> Vec<&str> {
        std::iter::once("plain")
            .chain(
                self.styles
                    .keys()
                    .map(String::as_str)
                    .filter(|name| *name != "plain"),
            )
            .collect()
    }

    pub fn style(&self, name: &str) -> Option<&StyleSettings> {
        self.styles.get(name)
    }

    pub fn query(&self, name: &str) -> Result<&NamedQuery, ConfigError> {
        self.queries
            .get(name)
            .ok_or_else(|| self.unknown(ProfileKind::Query, name, self.query_names()))
    }

    pub fn profile(&self, name: &str) -> Result<&Profile, ConfigError> {
        self.profiles
            .get(name)
            .ok_or_else(|| self.unknown(ProfileKind::Profile, name, self.profile_names()))
    }

    pub fn export(&self, name: &str) -> Result<&ExportTarget, ConfigError> {
        self.exports
            .get(name)
            .ok_or_else(|| self.unknown(ProfileKind::Export, name, self.export_names()))
    }

    pub fn query_names(&self) -> Vec<&str> {
        self.queries.keys().map(String::as_str).collect()
    }

    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.keys().map(String::as_str).collect()
    }

    pub fn export_names(&self) -> Vec<&str> {
        self.exports.keys().map(String::as_str).collect()
    }

    /// `[figure.<name>]`, or `None`: a name the project does not declare may be
    /// the model's, which is not this module's to know.
    pub fn figure(&self, name: &str) -> Option<&FigureDeclaration> {
        self.figures.get(name)
    }

    pub fn figure_names(&self) -> Vec<&str> {
        self.figures.keys().map(String::as_str).collect()
    }

    /// `[matplotlib]`, by matplotlib's dotted names, in declaration order.
    pub fn matplotlib(&self) -> &IndexMap<String, toml::Value> {
        &self.matplotlib
    }

    /// The colour of a workbench role: the project's, or the default
    /// [`WORKBENCH_COLORS`] gives it.
    pub fn workbench_color(&self, role: &str) -> Option<&str> {
        self.workbench_colors
            .get(role)
            .map(String::as_str)
            .or_else(|| {
                WORKBENCH_COLORS
                    .iter()
                    .find(|(known, _)| *known == role)
                    .map(|(_, color)| *color)
            })
    }

    /// The files imported, nearest first.
    pub fn imports(&self) -> &[PathBuf] {
        &self.imports
    }

    /// Where a query, a profile, an export or a figure comes from: `Local` for
    /// one this file alone declares, or none of that name.
    pub fn origin(&self, kind: DeclarationKind, name: &str) -> DeclaredIn {
        let Some(files) = self.origins.get(&(kind, name.to_string())) else {
            return DeclaredIn::Local;
        };
        let own = folder_of(&self.root.join(FILENAME)).join(FILENAME);
        match files.as_slice() {
            [first, next, ..] if *first == own => DeclaredIn::Over(next.clone()),
            [first, ..] if *first != own => DeclaredIn::Imported(first.clone()),
            _ => DeclaredIn::Local,
        }
    }

    /// This file's declarations identical to what it imports, for validate.
    pub fn copies(&self) -> &[ImportedCopy] {
        &self.copies
    }

    fn unknown(&self, kind: ProfileKind, name: &str, available: Vec<&str>) -> ConfigError {
        let available: Vec<String> = available.into_iter().map(str::to_string).collect();
        ConfigError::UnknownProfile {
            kind,
            suggestion: crate::core::identifier::nearest(
                name,
                available.iter().map(String::as_str),
            ),
            name: name.to_string(),
            available,
        }
    }
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum ConfigError {
    /// Malformed TOML, **and** a section or key this build does not know: the
    /// parser's message names the line and lists everything accepted, which is
    /// better than a hand-written check that could drift out of step.
    Parse {
        path: PathBuf,
        line: Option<usize>,
        message: String,
    },
    UnsupportedVersion {
        path: PathBuf,
        found: u32,
        supported: RangeInclusive<u32>,
    },
    MissingVersion {
        path: PathBuf,
    },
    InvalidProfile {
        path: PathBuf,
        profile: String,
        reason: String,
    },
    DuplicateField {
        profile: String,
        field: String,
    },
    DuplicateHeader {
        profile: String,
        header: String,
        fields: Vec<String>,
    },
    EmptyHeader {
        profile: String,
        field: String,
    },
    UnknownProfile {
        kind: ProfileKind,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A key renamed to say what it is, refused naming the new one: no command
    /// rewrites a configuration.
    RenamedKey {
        path: PathBuf,
        section: String,
        old: &'static str,
        new: &'static str,
    },
    /// `import` naming no folder holding a `.samplekitrc`.
    ImportNotFound {
        path: PathBuf,
        import: String,
        reason: String,
    },
    /// Files importing one another, in the order they do, the first again last.
    ImportLoop {
        files: Vec<PathBuf>,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// A `[render] style` naming nothing declared. **Not** a fallback to
    /// plain: that would render a whole manuscript table wrong without a word.
    UnknownStyle {
        name: String,
        available: Vec<String>,
    },
    /// A declared precision that is not a specifier.
    InvalidPrecision {
        path: PathBuf,
        section: String,
        reason: String,
    },
    /// A key a declaration does not take.
    UnknownKey {
        path: PathBuf,
        section: String,
        key: String,
        suggestion: Option<String>,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidPrecision {
                path,
                section,
                reason,
            } => write!(f, "{}: [{section}] precision: {reason}", path.display()),
            ConfigError::UnknownKey {
                path,
                section,
                key,
                suggestion,
            } => write!(
                f,
                "{}: [{section}] takes no key '{key}'{}\n  a declaration takes unit, symbol, precision and symbol_<style>",
                path.display(),
                suggestion
                    .as_ref()
                    .map(|name| format!(" — did you mean '{name}'?"))
                    .unwrap_or_default()
            ),
            ConfigError::RenamedKey {
                path,
                section,
                old,
                new,
            } => write!(
                f,
                "{}: [{section}] '{old}' is now '{new}' — it names a directory, not a parent query\n  \
                 rename the key: no command rewrites a configuration",
                path.display()
            ),
            ConfigError::UnknownStyle { name, available } => write!(
                f,
                "'{name}' is not a declared style{}",
                if available.is_empty() {
                    ", and this project declares none".to_string()
                } else {
                    format!("\n  available: plain, {}", available.join(", "))
                }
            ),
            ConfigError::Parse {
                path,
                line,
                message,
            } => match line {
                Some(line) => write!(f, "{}:{line}: {message}", path.display()),
                None => write!(f, "{}: {message}", path.display()),
            },
            ConfigError::UnsupportedVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} declares schema_version {found}, and this build reads {}",
                path.display(),
                crate::format::schema::readable_versions(supported)
            ),
            ConfigError::MissingVersion { path } => write!(
                f,
                "{} has no schema_version: a configuration declares which \
                 version it was written for",
                path.display()
            ),
            ConfigError::InvalidProfile {
                path,
                profile,
                reason,
            } => write!(f, "{} · '{profile}': {reason}", path.display()),
            ConfigError::DuplicateField { profile, field } => write!(
                f,
                "'{profile}' declares '{field}' twice, and which of the two wins \
                 is a question with no good answer"
            ),
            ConfigError::DuplicateHeader {
                profile,
                header,
                fields,
            } => write!(
                f,
                "'{profile}' would emit the header '{header}' twice, from {} — \
                 nothing could tell the two columns apart in the exported file",
                fields.join(" and ")
            ),
            ConfigError::EmptyHeader { profile, field } => write!(
                f,
                "'{profile}' declares an empty name for '{field}': omit the key \
                 to fall back to the field, rather than naming it nothing"
            ),
            ConfigError::UnknownProfile {
                kind,
                name,
                available,
                suggestion,
            } => {
                write!(f, "no {} named '{name}'", kind.as_str())?;
                if let Some(suggestion) = suggestion {
                    write!(f, "\n  did you mean: '{suggestion}'?")?;
                }
                if !available.is_empty() {
                    write!(f, "\n  available: {}", available.join(", "))?;
                }
                Ok(())
            }
            ConfigError::ImportNotFound {
                path,
                import,
                reason,
            } => write!(
                f,
                "{}: import = \"{import}\" takes nothing: {reason}",
                path.display()
            ),
            ConfigError::ImportLoop { files } => {
                let named: Vec<String> = files
                    .iter()
                    .map(|file| file.display().to_string())
                    .collect();
                write!(
                    f,
                    "{} imports itself: {}\n  remove one of these imports",
                    named.first().map(String::as_str).unwrap_or_default(),
                    named.join(" imports ")
                )
            }
            ConfigError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ConfigError {}
