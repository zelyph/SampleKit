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
    /// `[model] python`, with a model or without one: the Python that runs the
    /// model and draws the figures.
    python: Option<PathBuf>,
    render: RenderSettings,
    collection: CollectionRules,
    units: IndexMap<String, IndexMap<String, String>>,
    properties: IndexMap<String, PropertyDeclaration>,
    /// `[style.*]`, those SampleKit gives beside them: how each kind of
    /// document writes, a table as a document included where it says so.
    styles: IndexMap<String, StyleSettings>,
    /// The styles the file declares, apart from those given it does not name.
    declared_styles: Vec<String>,
    queries: IndexMap<String, NamedQuery>,
    profiles: IndexMap<String, Profile>,
    exports: IndexMap<String, Export>,
    figures: IndexMap<String, FigureDeclaration>,
    /// `[matplotlib]`: matplotlib's own settings, by their dotted names.
    /// Carried, not interpreted: matplotlib checks them when it draws.
    matplotlib: IndexMap<String, toml::Value>,
    /// `[colors]`: the colour for each role the TUI and the command line
    /// draw, where the project changes one. Checked here, drawn there.
    colors: IndexMap<String, String>,
    /// `[history] shared`: whether git carries the history, where something
    /// declares it.
    history_shared: Option<bool>,
    /// The files imported, nearest first, each once.
    imports: Vec<PathBuf>,
    /// The files declaring each query, profile, export and figure, nearest
    /// first, where an import is involved: absent where only this file does.
    origins: IndexMap<(DeclarationKind, String), Vec<PathBuf>>,
    /// This file's declarations identical to what it imports.
    copies: Vec<ImportedCopy>,
    /// `[collection] files` as the file declaring them wrote them, where an
    /// imported file does: that file, and each entry with its
    /// `{project}`.
    files_declared: Option<(PathBuf, Vec<String>)>,
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

/// The roles `[colors]` names, each with the colour the TUI and the command
/// line give it where the project does not: the terminal's own sixteen, so that its
/// theme decides the shades.
pub const COLOR_ROLES: &[(&str, &str)] = &[
    ("failed", "red"),
    ("outdated", "yellow"),
    ("edited", "magenta"),
    ("current", "green"),
    ("selected", "cyan"),
    ("attribute", "blue"),
    // A link, underlined in this colour; one leading nowhere in `error`'s.
    ("link", "blue"),
    // A value and its uncertainty, drawn bold in this colour; a unit.
    ("value", "default"),
    ("unit", "cyan"),
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

/// Whether `text` is a colour `[colors]` takes.
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
    /// `*.md`, `*.MD` and `*.markdown` in the given directory, not recursive:
    /// the rules of a folder no project describes, which may be anything. A
    /// project's own are recursive unless it says otherwise.
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

/// `[style.<name>]`: how a kind of document writes — a property, `{value}`,
/// `{uncertainty}`, `{unit}` and `{symbol}` placed, and one with no
/// uncertainty; an absent value; what text is escaped for; and, where the
/// style says it or is given, how a table is a document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleSettings {
    pub property: Option<String>,
    /// The piece `{uncertainty}` stands for in `property` and `unitless`,
    /// written only where there is an uncertainty: ` \pm {uncertainty}`.
    pub uncertainty: Option<String>,
    /// The form of a property with no unit, instead of `property`.
    pub unitless: Option<String>,
    pub missing: Option<String>,
    pub escape: Escape,
    pub document: Option<TextFormat>,
}

/// What a project declares about one quantity, so that no file repeats it.
/// `unit` is declared here *and* written into every file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropertyDeclaration {
    pub unit: Option<String>,
    pub symbol: Option<String>,
    pub symbol_variants: IndexMap<String, String>,
    pub precision: Option<PrecisionSchema>,
    /// `range = [MIN, MAX]`, in the declared unit: what a value outside is
    /// said of by `validate`, `set` and `compute`. `inf` leaves a side open.
    pub range: Option<(f64, f64)>,
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

/// `[export.*]`: a table or a figure, and where it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Export {
    Table(ExportTarget),
    Figure(FigureExport),
}

impl Export {
    pub fn name(&self) -> &str {
        match self {
            Export::Table(target) => &target.name,
            Export::Figure(figure) => &figure.name,
        }
    }

    /// The file it writes, as declared: from the configuration's folder.
    pub fn output(&self) -> &Path {
        match self {
            Export::Table(target) => &target.output,
            Export::Figure(figure) => &figure.output,
        }
    }

    pub fn query(&self) -> Option<&str> {
        match self {
            Export::Table(target) => target.query.as_deref(),
            Export::Figure(figure) => figure.query.as_deref(),
        }
    }
}

/// `[export.*]` naming a `figure`: drawn by `plotting` to `output`, as
/// `plot -o` draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureExport {
    pub name: String,
    /// A `[figure.*]`, or a figure the model declares: the file loads
    /// without knowing which, and `export` and `validate` judge it.
    pub figure: String,
    pub output: PathBuf,
    pub query: Option<String>,
    /// The style its labels use, as `plot --style` gives it.
    pub style: Option<String>,
}

/// What matplotlib writes, by extension: a figure's file names its format.
pub const FIGURE_FORMATS: [&str; 14] = [
    "eps", "jpeg", "jpg", "pdf", "pgf", "png", "ps", "raw", "rgba", "svg", "svgz", "tif", "tiff",
    "webp",
];

/// `[export.*]` naming a `profile`: where a table goes. Written by `exports`.
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
    /// The style its units are spelled in; `None`, as the files spell them
    /// ([`FILES_STYLE`]): a program reading the file finds `degC`, whatever
    /// the screen shows.
    pub style: Option<String>,
}

/// How `[render] table` may draw a terminal table.
pub const TABLE_STYLES: [&str; 2] = ["plain", "boxed"];

/// The style a data export spells units in unless told otherwise: as the
/// sample files write them, `degC` rather than `°C` — the spelling a program
/// reads back and a person can type. It needs no declaration.
pub const FILES_STYLE: &str = "files";

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
    /// Its own `[matplotlib]` settings, over the section's, by matplotlib's
    /// dotted names.
    pub matplotlib: IndexMap<String, toml::Value>,
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
            matplotlib: IndexMap::new(),
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

/// An export's `format`, refused at load when it is none of the three; one
/// naming a style that writes documents, and no format, is `Text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Csv,
    Tsv,
    Json,
    /// A table written as a document, as a `[style.NAME]` says.
    Text(Box<TextFormat>),
}

/// A style's document keys: how a table is written as text — a LaTeX
/// tabular, a Markdown table. Each line is its cells joined by `cells` and placed
/// where `{cells}` stands; `{align}`, in the text before and after, is each
/// column's alignment; a brace that opens no such name is text, so that
/// `\begin{tabular}{{align}}` is LaTeX around a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFormat {
    pub name: String,
    /// The file's extension, without its dot: the style's name unless said.
    pub extension: String,
    /// Before the table, and after it.
    pub before: String,
    pub after: String,
    /// The header line, `{cells}` its cells; `None`, no header.
    pub header: Option<String>,
    /// Between two cells of the header line; `cells` where none is said:
    /// HTML's `</th><th>` beside its rows' `</td><td>`.
    pub header_cells: Option<String>,
    /// A cell written for every column on a line of its own after the header:
    /// Markdown's `---`.
    pub rule: Option<String>,
    /// A row's line, `{cells}` its cells.
    pub row: String,
    /// Between two cells of a line.
    pub cells: String,
    /// A line between two groups of a profile that groups.
    pub between_groups: Option<String>,
    /// A cell with no value.
    pub missing: String,
    /// How a text cell is made safe for the document.
    pub escape: Escape,
    /// A property's value and uncertainty in one cell, `{value}`,
    /// `{uncertainty}` and `{unit}` placed; `None`, two columns, as a CSV
    /// writes them.
    pub property: Option<String>,
    /// The piece `{uncertainty}` stands for, where there is one.
    pub uncertainty: Option<String>,
    /// The form of a property with no unit.
    pub unitless: Option<String>,
    /// After a header cell whose column has a unit, `{unit}` placed; `None`,
    /// no unit in the header — each cell writes its own.
    pub header_unit: Option<String>,
    /// `{align}`'s letter for a column of text, and for one of numbers.
    pub align_text: String,
    pub align_number: String,
}

/// What a text cell is escaped for: a language given, a table of
/// replacements a style declares, or nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Escape {
    #[default]
    None,
    /// As LaTeX reads text: `v_latexescape`'s.
    Latex,
    /// As HTML reads text: `html-escape`'s.
    Html,
    /// `|`, which ends a Markdown table's cell.
    Markdown,
    /// Each text replaced by its replacement, in the order written.
    Replace(Vec<(String, String)>),
}

impl Escape {
    /// `text` made safe for the document this escapes for.
    pub fn apply(&self, text: &str) -> String {
        match self {
            Escape::None => text.to_string(),
            Escape::Markdown => text.replace('|', "\\|"),
            Escape::Latex => v_latexescape::escape_fmt(text).to_string(),
            Escape::Html => html_escape::encode_text(text).into_owned(),
            Escape::Replace(replacements) => {
                // Character by character, the first replacement matching where
                // it stands: `&` replaced first never touches the `&` of an
                // `&lt;`.
                let mut out = String::with_capacity(text.len());
                let mut rest = text;
                'walk: while !rest.is_empty() {
                    for (from, to) in replacements {
                        if !from.is_empty() && rest.starts_with(from.as_str()) {
                            out.push_str(to);
                            rest = &rest[from.len()..];
                            continue 'walk;
                        }
                    }
                    let next = rest.chars().next().expect("not empty");
                    out.push(next);
                    rest = &rest[next.len_utf8()..];
                }
                out
            }
        }
    }
}

impl TextFormat {
    /// The documents SampleKit gives without a declaration, which a style of
    /// the same name changes key by key.
    pub fn given(name: &str) -> Option<TextFormat> {
        match name {
            "latex" => Some(TextFormat {
                name: "latex".to_string(),
                extension: "tex".to_string(),
                before: "\\begin{tabular}{{align}}\n\\toprule".to_string(),
                after: "\\bottomrule\n\\end{tabular}".to_string(),
                header: Some("{cells} \\\\\n\\midrule".to_string()),
                header_cells: None,
                rule: None,
                row: "{cells} \\\\".to_string(),
                cells: " & ".to_string(),
                between_groups: Some("\\midrule".to_string()),
                missing: "--".to_string(),
                escape: Escape::Latex,
                property: Some("${value} \\pm {uncertainty}$".to_string()),
                uncertainty: None,
                unitless: None,
                header_unit: Some(" ({unit})".to_string()),
                align_text: "l".to_string(),
                align_number: "r".to_string(),
            }),
            "markdown" => Some(TextFormat {
                name: "markdown".to_string(),
                extension: "md".to_string(),
                before: String::new(),
                after: String::new(),
                header: Some("| {cells} |".to_string()),
                header_cells: None,
                rule: Some("---".to_string()),
                row: "| {cells} |".to_string(),
                cells: " | ".to_string(),
                // Markdown has no rule between rows: a group is headed.
                between_groups: Some("| **{group}** |".to_string()),
                missing: String::new(),
                escape: Escape::Markdown,
                property: Some("{value} ± {uncertainty}".to_string()),
                uncertainty: None,
                unitless: None,
                header_unit: Some(" ({unit})".to_string()),
                align_text: "l".to_string(),
                align_number: "r".to_string(),
            }),
            "html" => Some(TextFormat {
                name: "html".to_string(),
                extension: "html".to_string(),
                before: "<table>".to_string(),
                after: "</tbody>\n</table>".to_string(),
                // Its rows in a body, a group's in one of their own.
                header: Some("<thead><tr><th>{cells}</th></tr></thead>\n<tbody>".to_string()),
                header_cells: Some("</th><th>".to_string()),
                rule: None,
                row: "<tr><td>{cells}</td></tr>".to_string(),
                cells: "</td><td>".to_string(),
                between_groups: Some("</tbody>\n<tbody>".to_string()),
                missing: String::new(),
                escape: Escape::Html,
                property: Some("{value} ± {uncertainty}".to_string()),
                uncertainty: None,
                unitless: None,
                header_unit: Some(" ({unit})".to_string()),
                align_text: "l".to_string(),
                align_number: "r".to_string(),
            }),
            _ => None,
        }
    }

    /// The names given without a declaration.
    pub const GIVEN: [&'static str; 3] = ["html", "latex", "markdown"];

    /// A document a style declares from nothing given: a plain line of cells.
    pub fn named(name: &str) -> TextFormat {
        TextFormat {
            name: name.to_string(),
            extension: name.to_string(),
            before: String::new(),
            after: String::new(),
            header: Some("{cells}".to_string()),
            header_cells: None,
            rule: None,
            row: "{cells}".to_string(),
            cells: " ".to_string(),
            between_groups: None,
            missing: String::new(),
            escape: Escape::None,
            property: Some("{value} ± {uncertainty}".to_string()),
            uncertainty: None,
            unitless: None,
            header_unit: Some(" ({unit})".to_string()),
            align_text: "l".to_string(),
            align_number: "r".to_string(),
        }
    }
}

impl StyleSettings {
    /// A style given without a declaration: its document's property and
    /// escape, and the document.
    fn given(name: &str) -> Option<StyleSettings> {
        TextFormat::given(name).map(|document| StyleSettings {
            property: document.property.clone(),
            uncertainty: document.uncertainty.clone(),
            unitless: document.unitless.clone(),
            missing: Some(document.missing.clone()),
            escape: document.escape.clone(),
            document: Some(document),
        })
    }
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
    /// The folders whose `.samplekitrc` this one takes, or the files: one or a
    /// list, read by `gather`.
    #[serde(default)]
    import: Option<toml::Value>,
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
    /// `[colors]`.
    #[serde(default)]
    colors: IndexMap<String, String>,
    #[serde(default)]
    history: WrittenHistory,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WrittenHistory {
    /// Whether git carries the history with the samples.
    shared: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenModel {
    /// Absent in a file declaring only `python`, for the projects that import
    /// it: a `[model]` without a path is no model.
    path: Option<String>,
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

/// `[style.NAME]` as written: every key optional, what a style given of its
/// name does not say taken from that one, else from a plain line of cells.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct WrittenStyle {
    property: Option<toml::Value>,
    uncertainty: Option<String>,
    unitless: Option<String>,
    missing: Option<String>,
    escape: Option<toml::Value>,
    extension: Option<String>,
    before: Option<String>,
    after: Option<String>,
    header: Option<toml::Value>,
    header_cells: Option<String>,
    rule: Option<String>,
    row: Option<String>,
    cells: Option<String>,
    between_groups: Option<String>,
    header_unit: Option<toml::Value>,
    align_text: Option<String>,
    align_number: Option<String>,
}

impl WrittenStyle {
    /// Whether it says how a table is a document.
    fn documents(&self) -> bool {
        self.extension.is_some()
            || self.before.is_some()
            || self.after.is_some()
            || self.header.is_some()
            || self.header_cells.is_some()
            || self.rule.is_some()
            || self.row.is_some()
            || self.cells.is_some()
            || self.between_groups.is_some()
            || self.header_unit.is_some()
            || self.align_text.is_some()
            || self.align_number.is_some()
    }
}

/// `symbol` and `precision` are named; every other key is a style variant of
/// the symbol, which is what lets a project add one without a format change.
#[derive(Deserialize, Default)]
struct WrittenProperty {
    unit: Option<String>,
    symbol: Option<String>,
    precision: Option<PrecisionSchema>,
    range: Option<Vec<f64>>,
    #[serde(flatten)]
    variants: IndexMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenQuery {
    filter: String,
    directory: Option<String>,
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
    matplotlib: Option<toml::Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenExport {
    profile: Option<String>,
    figure: Option<String>,
    format: Option<String>,
    output: String,
    filename: Option<bool>,
    path: Option<bool>,
    query: Option<String>,
    style: Option<String>,
}

// ---------------------------------------------------------------- functions

/// A `[style.NAME]` read: what it does not say taken from the style of that
/// name SampleKit gives, else from nothing — a style of units and symbols
/// alone, unless it says how a table is a document.
fn style_settings(name: &str, written: &WrittenStyle) -> Result<StyleSettings, String> {
    let given = StyleSettings::given(name);
    // `header = false` and `property = false` say there is none; a text is
    // the line or the cell.
    let optional = |key: &str,
                    value: &Option<toml::Value>,
                    held: Option<String>|
     -> Result<Option<String>, String> {
        match value {
            None => Ok(held),
            Some(toml::Value::String(text)) => Ok(Some(text.clone())),
            Some(toml::Value::Boolean(false)) => Ok(None),
            Some(other) => Err(format!(
                "{key} is a text, or false for none: {other} is neither"
            )),
        }
    };
    let escape = match &written.escape {
        None => given
            .as_ref()
            .map(|given| given.escape.clone())
            .unwrap_or_default(),
        Some(toml::Value::Boolean(false)) => Escape::None,
        Some(toml::Value::String(language)) => match language.as_str() {
            "none" => Escape::None,
            "latex" => Escape::Latex,
            "html" => Escape::Html,
            "markdown" => Escape::Markdown,
            other => {
                return Err(format!(
                    "escape = '{other}' is none of latex, html or markdown: a table of \
                     replacements escapes for any other language, escape = {{ \"<\" = \"&lt;\" }}"
                ));
            }
        },
        Some(toml::Value::Table(replacements)) => Escape::Replace(
            replacements
                .iter()
                .map(|(text, by)| match by {
                    toml::Value::String(by) => Ok((text.clone(), by.clone())),
                    other => Err(format!(
                        "escape: '{text}' is replaced by a text, and {other} is none"
                    )),
                })
                .collect::<Result<_, _>>()?,
        ),
        Some(other) => {
            return Err(format!(
                "escape is a language — latex, html, markdown — a table of replacements, \
                 or false: {other} is none"
            ));
        }
    };
    let property = optional(
        "property",
        &written.property,
        given.as_ref().and_then(|given| given.property.clone()),
    )?;
    let uncertainty = written.uncertainty.clone();
    let unitless = written.unitless.clone();
    let missing = written
        .missing
        .clone()
        .or_else(|| given.as_ref().and_then(|given| given.missing.clone()));
    let document = match given.as_ref().and_then(|given| given.document.clone()) {
        Some(base) => Some(base),
        None if written.documents() => Some(TextFormat::named(name)),
        None => None,
    };
    let document = match document {
        None => None,
        Some(base) => Some(TextFormat {
            name: name.to_string(),
            extension: written.extension.clone().unwrap_or(base.extension),
            before: written.before.clone().unwrap_or(base.before),
            after: written.after.clone().unwrap_or(base.after),
            header: optional("header", &written.header, base.header)?,
            header_cells: written.header_cells.clone().or(base.header_cells),
            rule: written.rule.clone().or(base.rule),
            row: written.row.clone().unwrap_or(base.row),
            cells: written.cells.clone().unwrap_or(base.cells),
            between_groups: written.between_groups.clone().or(base.between_groups),
            missing: missing.clone().unwrap_or(base.missing),
            escape: escape.clone(),
            property: if written.property.is_some() {
                property.clone()
            } else {
                base.property
            },
            uncertainty: uncertainty.clone().or(base.uncertainty),
            unitless: unitless.clone().or(base.unitless),
            header_unit: optional("header_unit", &written.header_unit, base.header_unit)?,
            align_text: written.align_text.clone().unwrap_or(base.align_text),
            align_number: written.align_number.clone().unwrap_or(base.align_number),
        }),
    };
    Ok(StyleSettings {
        property,
        uncertainty,
        unitless,
        missing,
        escape,
        document,
    })
}

/// Whether a style of this name is known: `plain` and `files` built in, one
/// SampleKit gives, or one the file declares.
fn style_known(declared: &IndexMap<String, WrittenStyle>, name: &str) -> bool {
    name == "plain"
        || name == FILES_STYLE
        || TextFormat::GIVEN.contains(&name)
        || declared.contains_key(name)
}

/// The styles a message names: those given, then those declared.
fn style_names(declared: &IndexMap<String, WrittenStyle>) -> Vec<String> {
    let mut names: Vec<String> = TextFormat::GIVEN
        .iter()
        .map(|name| name.to_string())
        .collect();
    for name in declared.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

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
    //
    // **By the path as written, then as resolved.** A folder linked into the
    // project — data on another disk — is the project's: resolved first, its
    // samples were found in no project, and written with no history.
    let resolved = dunce::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let written = std::path::absolute(start)
        .ok()
        .map(|path| lexically_normal(&path))
        .filter(|path| *path != resolved);
    if let Some(written) = written
        && let Some(found) = find_from(&written)
    {
        // The one found by the resolved path where both find the same file.
        return Some(match find_from(&resolved) {
            Some(by_resolved)
                if dunce::canonicalize(&by_resolved).ok() == dunce::canonicalize(&found).ok() =>
            {
                by_resolved
            }
            _ => found,
        });
    }
    find_from(&resolved)
}

fn find_from(start: &Path) -> Option<PathBuf> {
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

/// `a/b/../c` as `a/c`, `.` taken out, without reading the disk.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

/// The configuration of a project whose file cannot be read: its folder, and
/// nothing declared. What the history is read with, so that a file broken by
/// an edit can still be logged, compared and restored.
pub fn unread(path: &Path) -> ProjectConfig {
    parse(path, "schema_version = 1").expect("a bare configuration always parses")
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
    // The project's folder was written `{collection}` until 1.0.0-rc.2: a
    // path holding it would make a folder of that name.
    if let Some(line) = text.lines().position(|line| line.contains("{collection}")) {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: Some(line + 1),
            message: "{collection} is written {project} now: the folder of the project a \
                      sample follows"
                .to_string(),
        });
    }
    let mut written = written_of(path, text)?;
    // Every file says which version it was written for, one importing
    // another too: the version it imports is the other file's.
    if written.schema_version.is_none() {
        return Err(ConfigError::MissingVersion {
            path: path.to_path_buf(),
        });
    }
    let root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    if written.import.is_none() {
        // What this file says of its own collection, `{project}` being
        // its own folder.
        place_collection_in_written(&mut written);
        return build(path, root, written).map_err(|error| located(path, text, error));
    }
    let head = folder_of(path);
    let gathered = gather(path, text, &head, importing)?;
    let merged: Written =
        toml::Value::Table(gathered.table)
            .try_into()
            .map_err(|error: toml::de::Error| ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: format!("{}, with what it imports", error.message()),
            })?;
    let mut config = build(path, root, merged).map_err(|error| located(path, text, error))?;
    config.imports = gathered.imports;
    config.copies = gathered.copies;
    config.files_declared = gathered
        .files_declared
        .filter(|(file, _)| *file != folder_of(path).join(FILENAME));
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
        message: said_of(
            error.message(),
            error.span().map(|span| line_text(text, span.start)),
        ),
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
    /// The file whose `[collection] files` holds, and its entries as written.
    files_declared: Option<(PathBuf, Vec<String>)>,
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
/// whose folder is `head`: `{project}` is written from each file to it.
fn gather(
    path: &Path,
    text: &str,
    head: &Path,
    importing: &mut Vec<PathBuf>,
) -> Result<Gathered, ConfigError> {
    let here = folder_of(path);
    let file = here.join(path.file_name().unwrap_or(FILENAME.as_ref()));
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
    // As written, before `{project}` is placed: what a note about an
    // entry quotes, in the file that wrote it.
    let own_files: Option<(PathBuf, Vec<String>)> = own
        .get("collection")
        .and_then(|collection| collection.get("files"))
        .and_then(toml::Value::as_array)
        .map(|files| {
            (
                file.clone(),
                files
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_string))
                    .collect(),
            )
        });
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
            files_declared: own_files,
        });
    };
    // One or a list: each merged in order, a later one over an earlier one.
    let listed: Vec<String> = match &import {
        toml::Value::String(one) => vec![one.clone()],
        toml::Value::Array(items) if items.iter().all(toml::Value::is_str) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => {
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: "import names a folder or a file, or a list of them: import = \
                          [\"..\", \"figures.samplekitrc\"]"
                    .to_string(),
            });
        }
    };
    let mut table = toml::Table::new();
    // Nearest first: the last listed and what it imports, then the one before.
    let mut imports: Vec<PathBuf> = Vec::new();
    let mut inner_declared: Vec<IndexMap<(DeclarationKind, String), Vec<PathBuf>>> = Vec::new();
    let mut files_declared = None;
    for written in listed {
        let imported = imported_file(path, &here, &written)?;
        // A file met twice is merged once, where it is first met.
        if imports.contains(&imported) {
            continue;
        }
        importing.push(file.clone());
        // The file imported is a project for the samples beside it: valid on
        // its own, its errors naming it.
        let result = read(&imported).and_then(|text| {
            parse_within(&imported, &text, importing)?;
            gather(&imported, &text, head, importing)
        });
        importing.pop();
        // Said as this file's too: the file read is set aside with the one it
        // imports, and the message named only the one imported.
        let mut inner = result.map_err(|error| match error {
            ConfigError::ImportLoop { .. } => error,
            error => ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: format!("it imports a configuration that is not read — {error}"),
            },
        })?;
        // The model is each file's own: its path and its class are not
        // imported.
        if let Some(toml::Value::Table(model)) = inner.table.get_mut("model") {
            model.remove("path");
            model.remove("class");
            if model.is_empty() {
                inner.table.remove("model");
            }
        }
        let there = folder_of(&imported);
        rebase(&mut inner.table, &here, &there);
        merge(&mut table, inner.table);
        let mut chain = vec![imported];
        chain.extend(inner.imports);
        chain.retain(|file| !imports.contains(file));
        imports.splice(0..0, chain);
        inner_declared.push(inner.declared);
        files_declared = inner.files_declared.or(files_declared);
    }
    let copies = copies_of(&own, &table, imports.first().map_or(path, PathBuf::as_path))
        .into_iter()
        .map(|copy| ImportedCopy {
            from: declaring(&imports, &copy.key).unwrap_or(copy.from),
            key: copy.key,
        })
        .collect();
    for key in declared_in(&own) {
        declared.entry(key).or_default().push(file.clone());
    }
    // The nearer import's files first, as a nearer file is.
    for inner in inner_declared.into_iter().rev() {
        for (key, files) in inner {
            declared.entry(key).or_default().extend(files);
        }
    }
    merge(&mut table, own);
    // `[model] python` alone, its model not declared here, runs no model; the
    // interpreter it names still draws the project's figures.
    if let Some(toml::Value::Table(model)) = table.get_mut("model")
        && !model.contains_key("path")
    {
        model.retain(|key, _| key == "python");
        if model.is_empty() {
            table.remove("model");
        }
    }
    Ok(Gathered {
        table,
        imports,
        declared,
        copies,
        files_declared: own_files.or(files_declared),
    })
}

/// The file an `import` names: a folder's `.samplekitrc`, or a file of any
/// name; relative to the file importing it, absolute, or from the home folder.
fn imported_file(path: &Path, here: &Path, written: &str) -> Result<PathBuf, ConfigError> {
    let refused = |reason: String| ConfigError::ImportNotFound {
        path: path.to_path_buf(),
        import: written.to_string(),
        reason,
    };
    if written.trim().is_empty() {
        return Err(refused(
            "it names no folder nor file: import = \"..\" takes the .samplekitrc of the folder \
             above"
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
    // A file is taken as it is, whatever its name.
    if folder.is_file() {
        return Ok(dunce::canonicalize(&folder).unwrap_or(folder));
    }
    if !folder.is_dir() {
        return Err(refused(format!(
            "{} is no folder nor file",
            folder.display()
        )));
    }
    let file = folder.join(FILENAME);
    if !file.is_file() {
        // A folder that cannot be read may hold one: said so, not *holds none*.
        if let Err(error) = std::fs::read_dir(&folder)
            && error.kind() == std::io::ErrorKind::PermissionDenied
        {
            return Err(refused(format!(
                "{} cannot be read: {error}",
                dunce::canonicalize(&folder).unwrap_or(folder).display()
            )));
        }
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
/// The nearest file of an import chain that writes `key` itself — a copy's
/// key as `copies_of` names it, `[property.volume]` or `[render] style`: the
/// file imported may hold it only because it imports it in turn.
fn declaring(chain: &[PathBuf], key: &str) -> Option<PathBuf> {
    let (section, last) = match key.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((section, "")) => (section.to_string(), None),
            Some((section, key)) => (section.to_string(), Some(key.trim().to_string())),
            None => return None,
        },
        None => (String::new(), Some(key.to_string())),
    };
    let mut segments: Vec<String> = section
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches('"').to_string())
        .collect();
    segments.extend(last.map(|key| key.trim_matches('"').to_string()));
    chain
        .iter()
        .find(|file| {
            std::fs::read_to_string(file)
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .is_some_and(|table| {
                    let mut at = Some(&toml::Value::Table(table.clone()));
                    for segment in &segments {
                        at = at.and_then(|value| value.get(segment));
                    }
                    at.is_some()
                })
        })
        .cloned()
}

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
            // As the file writes it: a unit's spelling, and a name that is
            // no bare key, quoted — `[unit."degC"]`.
            let quoted = section == "unit"
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            let key_written = if quoted {
                format!("\"{key}\"")
            } else {
                key.clone()
            };
            let named = if section.is_empty() {
                key_written.clone()
            } else {
                format!("{section}.{key_written}")
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
                        key_written.clone()
                    } else {
                        format!("[{section}] {key_written}")
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
/// it: `[collection] files`, `[export.*] output`, `[query.*] directory`,
/// `[model] python`, and `[matplotlib]`'s style files and fonts. `include` and
/// `exclude` are patterns on the collection's own files, not paths, and the
/// rest of `[matplotlib]` is matplotlib's.
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
    // `[matplotlib]`'s style sheets as files, and its fonts.
    if let Some(toml::Value::Table(matplotlib)) = table.get_mut("matplotlib") {
        for key in ["style", "fonts"] {
            let file =
                |entry: &str| key == "fonts" || entry.to_ascii_lowercase().ends_with(".mplstyle");
            match matplotlib.get_mut(key) {
                Some(toml::Value::String(entry)) if file(entry) => each(entry, false),
                Some(toml::Value::Array(entries)) => {
                    for entry in entries {
                        if let toml::Value::String(entry) = entry
                            && file(entry)
                        {
                            each(entry, false);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The placeholder for the project's folder.
const COLLECTION: &str = "{project}";

/// `{project}` written as the project's folder, seen from the file
/// declaring it: `data/c1` from the root, `.` in the collection's own.
fn place_collection(table: &mut toml::Table, collection: &Path) {
    let collection = slashed(collection);
    path_values(table, |value, placed| {
        if placed && value.contains(COLLECTION) {
            *value = normalised(&value.replace(COLLECTION, &collection));
        }
    });
}

/// The same, for a file that imports nothing: `{project}` is its folder.
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

/// The TOML reader's message in the words of the file: a nearer name for an
/// unknown key, Rust's `u32` as a whole number, and where it names no key —
/// *duplicate key*, *invalid type* — the line it read, so that the key is
/// named.
fn said_of(message: &str, line: Option<String>) -> String {
    let mut said = message
        .replace("expected u32", "expected a whole number")
        .replace("expected u64", "expected a whole number")
        .replace("expected i64", "expected a whole number")
        .replace("expected f64", "expected a number");
    if let Some(rest) = said.strip_prefix("unknown field `")
        && let Some((unknown, expected)) = rest.split_once("`, expected ")
    {
        let candidates: Vec<&str> = expected
            .split('`')
            .enumerate()
            .filter(|(at, _)| at % 2 == 1)
            .map(|(_, name)| name)
            .collect();
        // One near a known key with it; the keys listed only where none is
        // near.
        said = match crate::core::identifier::nearest(unknown, candidates.clone()) {
            Some(near) => format!("unknown key `{unknown}` — did you mean `{near}`?"),
            None => format!(
                "unknown key `{unknown}` — the keys are {}",
                candidates.join(", ")
            ),
        };
    }
    let names_no_key = said.starts_with("duplicate key") || said.starts_with("invalid type");
    let said = match line {
        Some(line) if names_no_key && !line.trim().is_empty() => {
            format!("{said}, in `{}`", line.trim())
        }
        _ => said,
    };
    crate::format::document::plain_type_error(&said)
}

/// The text of the line a byte offset falls on.
fn line_text(text: &str, offset: usize) -> String {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at);
    text[start..end].to_string()
}

/// An error of a declaration with its file and line where it has none: a
/// message naming neither left the reader of three imports to guess which
/// file to correct. The line is the key's where the message names a section
/// and a key the file writes, else the section's; an error of what the file
/// imports is the imported file's, and keeps no line of this one.
fn located(path: &Path, text: &str, error: ConfigError) -> ConfigError {
    let (message, section, key) = match error {
        ConfigError::Parse {
            path,
            line: None,
            message,
        } => {
            let (section, key) = named_in(&message);
            let line = section.and_then(|section| line_of_key(text, &section, key.as_deref()));
            return ConfigError::Parse {
                path,
                line,
                message,
            };
        }
        ConfigError::DuplicateField { ref profile, .. }
        | ConfigError::DuplicateHeader { ref profile, .. }
        | ConfigError::EmptyHeader { ref profile, .. } => {
            let section = format!("profile.{profile}");
            (error.to_string(), section, Some("columns".to_string()))
        }
        ConfigError::InvalidProfile {
            path,
            profile,
            reason,
        } => {
            let line = if profile.contains('.') {
                line_of_key(text, &profile, None)
            } else {
                line_of_key(text, &format!("profile.{profile}"), None)
                    .or_else(|| line_of_key(text, &format!("export.{profile}"), None))
            };
            return match line {
                Some(line) => ConfigError::Parse {
                    path,
                    line: Some(line),
                    message: format!("'{profile}': {reason}"),
                },
                None => ConfigError::InvalidProfile {
                    path,
                    profile,
                    reason,
                },
            };
        }
        ConfigError::UnknownStyle { .. } => {
            let message = format!("[render] style: {error}");
            (message, "render".to_string(), Some("style".to_string()))
        }
        error => return error,
    };
    ConfigError::Parse {
        path: path.to_path_buf(),
        line: line_of_key(text, &section, key.as_deref()),
        message,
    }
}

/// The section and key a message begins by naming — `[render] identify …`,
/// `[matplotlib] font.size: …` — the section without its brackets.
fn named_in(message: &str) -> (Option<String>, Option<String>) {
    let Some(rest) = message.strip_prefix('[') else {
        return (None, None);
    };
    let Some(end) = rest.find(']') else {
        return (None, None);
    };
    let section = rest[..end].to_string();
    let key = rest[end + 1..]
        .trim_start()
        .split(|c: char| c.is_whitespace() || c == ':' || c == '=')
        .next()
        .filter(|key| !key.is_empty())
        .map(str::to_string);
    (Some(section), key)
}

/// The line of `key` in `[section]` as the file writes it, else of the
/// section's header; `None` where the file writes neither — a value it
/// imports. A section is found written as a header, `[profile.p]` or
/// `[unit."degC"]`, quotes aside.
fn line_of_key(text: &str, section: &str, key: Option<&str>) -> Option<usize> {
    let bare = |written: &str| written.replace(['"', '\'', ' '], "");
    let wanted = bare(section);
    let mut within = false;
    let mut header = None;
    for (at, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let name = trimmed
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or_default();
            within = bare(name) == wanted;
            if within && header.is_none() {
                header = Some(at + 1);
            }
            continue;
        }
        if within
            && let Some(key) = key
            && bare(trimmed.split('=').next().unwrap_or_default()) == bare(key)
        {
            return Some(at + 1);
        }
    }
    header
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

    let python = written
        .model
        .as_ref()
        .and_then(|model| model.python.clone())
        .map(PathBuf::from);
    let model = written.model.and_then(|model| {
        Some(ModelDeclaration {
            path: PathBuf::from(model.path?),
            class: model.class,
            python: model.python.map(PathBuf::from),
        })
    });
    let defaults = CollectionRules::default();
    let collection = CollectionRules {
        // A project is its folder and every folder below it, unless it says
        // otherwise: its samples commonly sit in a folder of their own.
        recursive: written.collection.recursive.unwrap_or(true),
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
                suggestion: crate::core::identifier::nearest(
                    key,
                    ["unit", "symbol", "precision", "range"],
                ),
                key: key.clone(),
            });
        }
        let range = match declared.range.as_deref() {
            None => None,
            Some(&[low, high]) if !low.is_nan() && !high.is_nan() && low <= high => {
                Some((low, high))
            }
            Some(written) => {
                return Err(ConfigError::InvalidRange {
                    path: path.to_path_buf(),
                    section: format!("property.{name}"),
                    reason: if written.len() == 2 {
                        format!(
                            "{} is above {}: write the lower bound first",
                            written[0], written[1]
                        )
                    } else {
                        format!(
                            "{} numbers, where it takes two, [lowest, highest] — inf leaves a \
                             side open",
                            written.len()
                        )
                    },
                });
            }
        };
        properties.insert(
            name,
            PropertyDeclaration {
                unit: declared.unit,
                symbol: declared.symbol,
                symbol_variants: declared.variants,
                precision: declared.precision,
                range,
            },
        );
    }

    let mut queries = IndexMap::new();
    for (name, query) in written.query {
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
        // A field grouped by or sorted by twice — `sort = ["radius", "-radius"]` —
        // is two answers to one question, and two columns of one label are
        // two columns a reader cannot tell apart.
        let refused = |reason: String| ConfigError::InvalidProfile {
            path: path.to_path_buf(),
            profile: name.clone(),
            reason,
        };
        if let Some(field) = twice(profile.group.iter().map(|field| field.trim())) {
            return Err(refused(format!("group names '{field}' twice")));
        }
        if let Some(field) = twice(
            profile
                .sort
                .iter()
                .map(|key| key.trim().trim_start_matches(['-', '+'])),
        ) {
            return Err(refused(format!(
                "sort names '{field}' twice, and the first decides: name it once"
            )));
        }
        if let Some(label) = twice(columns.iter().filter_map(|column| column.label.as_deref())) {
            return Err(refused(format!(
                "two columns are labelled '{label}', and a table would show two columns \
                 nobody could tell apart"
            )));
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

    // The styles: SampleKit's, then the file's, each changing the one of its
    // name key by key.
    let mut styles: IndexMap<String, StyleSettings> = TextFormat::GIVEN
        .iter()
        .filter_map(|name| StyleSettings::given(name).map(|style| (name.to_string(), style)))
        .collect();
    for (name, style) in &written.style {
        let settings =
            style_settings(name, style).map_err(|reason| ConfigError::InvalidProfile {
                path: path.to_path_buf(),
                profile: format!("style.{name}"),
                reason,
            })?;
        styles.insert(name.clone(), settings);
    }

    let mut exports = IndexMap::new();
    for (name, export) in written.export {
        let refused = |reason: String| ConfigError::InvalidProfile {
            path: path.to_path_buf(),
            profile: name.clone(),
            reason,
        };
        // A table's export or a figure's: one of the two, named.
        let profile = match (export.profile, &export.figure) {
            (Some(_), Some(_)) => {
                return Err(refused(
                    "an export writes a profile or a figure, not both: profile = … for a \
                     table, figure = … for a figure"
                        .to_string(),
                ));
            }
            (None, None) => {
                return Err(refused(
                    "an export names what it writes: profile = … for a table, figure = … \
                     for a figure"
                        .to_string(),
                ));
            }
            (profile, _) => profile,
        };
        if let Some(figure) = export.figure {
            let table_keys: Vec<&str> = [
                ("format", export.format.is_some()),
                ("filename", export.filename.is_some()),
                ("path", export.path.is_some()),
            ]
            .into_iter()
            .filter_map(|(key, given)| given.then_some(key))
            .collect();
            if !table_keys.is_empty() {
                return Err(refused(format!(
                    "{} {} a table's: a figure's file is in the format its output's \
                     extension names, output = \"{name}.pdf\"",
                    table_keys.join(", "),
                    if table_keys.len() == 1 { "is" } else { "are" }
                )));
            }
            let extension = Path::new(&export.output)
                .extension()
                .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
            if !extension
                .as_deref()
                .is_some_and(|extension| FIGURE_FORMATS.contains(&extension))
            {
                return Err(refused(format!(
                    "output = \"{}\": a figure's file is one matplotlib writes, by its \
                     extension: {}",
                    export.output,
                    FIGURE_FORMATS.map(|format| format!(".{format}")).join(", ")
                )));
            }
            if let Some(query) = &export.query
                && !queries.contains_key(query)
            {
                let available: Vec<String> = queries.keys().cloned().collect();
                return Err(ConfigError::UnknownProfile {
                    within: Some(format!("{} · 'export.{name}'", path.display())),
                    kind: ProfileKind::Query,
                    suggestion: crate::core::identifier::nearest(
                        query,
                        available.iter().map(String::as_str),
                    ),
                    name: query.clone(),
                    available,
                });
            }
            if let Some(style) = &export.style
                && style != "plain"
                && !style_known(&written.style, style)
            {
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    line: None,
                    message: format!(
                        "[export.{name}] style: {}",
                        ConfigError::UnknownStyle {
                            name: style.clone(),
                            available: style_names(&written.style),
                        }
                    ),
                });
            }
            exports.insert(
                name.clone(),
                Export::Figure(FigureExport {
                    name,
                    figure,
                    output: PathBuf::from(export.output),
                    query: export.query,
                    style: export.style,
                }),
            );
            continue;
        }
        let profile = profile.expect("a table's export names its profile");
        // A data format, or a style writing documents: `format = "latex"`
        // was a text format's name, which a style says now.
        let document = |style: &str| {
            styles
                .get(style)
                .and_then(|settings| settings.document.clone())
        };
        let format = match (export.format.as_deref(), export.style.as_deref()) {
            (Some("csv"), _) => Format::Csv,
            (Some("tsv"), _) => Format::Tsv,
            (Some("json"), _) => Format::Json,
            (Some(other), _) if document(other).is_some() => {
                return Err(ConfigError::InvalidProfile {
                    path: path.to_path_buf(),
                    profile: name.clone(),
                    reason: format!(
                        "format = \"{other}\" is written style = \"{other}\" now, with no format: \
                         the style writes the document"
                    ),
                });
            }
            (Some(other), _) => {
                let known = ["csv", "tsv", "json"];
                return Err(ConfigError::InvalidProfile {
                    path: path.to_path_buf(),
                    profile: name.clone(),
                    reason: format!(
                        "'{other}' is not a format: {}{}",
                        known.join(", "),
                        crate::core::identifier::nearest(other, known.iter().copied())
                            .map(|near| format!(" — did you mean {near}?"))
                            .unwrap_or_default()
                    ),
                });
            }
            (None, Some(style)) if document(style).is_some() => {
                Format::Text(Box::new(document(style).expect("just found")))
            }
            (None, _) => {
                return Err(ConfigError::InvalidProfile {
                    path: path.to_path_buf(),
                    profile: name.clone(),
                    reason: "an export names its format — csv, tsv or json — or a style \
                             writing documents, style = \"latex\""
                        .to_string(),
                });
            }
        };
        // An export naming an undeclared profile is caught here, at load,
        // rather than on the day the export runs.
        if !profiles.contains_key(&profile) {
            let available: Vec<String> = profiles.keys().cloned().collect();
            return Err(ConfigError::UnknownProfile {
                within: Some(format!("{} · 'export.{name}'", path.display())),
                kind: ProfileKind::Profile,
                suggestion: crate::core::identifier::nearest(
                    &profile,
                    available.iter().map(String::as_str),
                ),
                name: profile.clone(),
                available,
            });
        }
        // A query it names must be declared too.
        if let Some(query) = &export.query
            && !queries.contains_key(query)
        {
            let available: Vec<String> = queries.keys().cloned().collect();
            return Err(ConfigError::UnknownProfile {
                within: Some(format!("{} · 'export.{name}'", path.display())),
                kind: ProfileKind::Query,
                suggestion: crate::core::identifier::nearest(
                    query,
                    available.iter().map(String::as_str),
                ),
                name: query.clone(),
                available,
            });
        }
        // A style naming nothing is refused as the file loads: resolved later,
        // it fell back to the plain forms without a word.
        if let Some(style) = &export.style
            && style != "plain"
            && style != FILES_STYLE
            && !style_known(&written.style, style)
        {
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                line: None,
                message: format!(
                    "[export.{name}] style: {}",
                    ConfigError::UnknownStyle {
                        name: style.clone(),
                        available: style_names(&written.style),
                    }
                ),
            });
        }
        exports.insert(
            name.clone(),
            Export::Table(ExportTarget {
                name,
                profile,
                format,
                output: PathBuf::from(export.output),
                filename: export.filename.unwrap_or(false),
                path: export.path.unwrap_or(false),
                query: export.query,
                style: export.style,
            }),
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
                within: Some(format!("{} · 'figure.{name}'", path.display())),
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
            && !style_known(&written.style, style)
        {
            return Err(refused(
                ConfigError::UnknownStyle {
                    name: style.clone(),
                    available: style_names(&written.style),
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
        // Its own settings: matplotlib's, read as the section's are; the
        // style sheets and the fonts are the project's.
        let own = matplotlib_settings(
            path,
            figure.matplotlib.unwrap_or_default(),
            &format!("[figure.{name}] matplotlib"),
        )?;
        if let Some(key) = ["style", "fonts"]
            .into_iter()
            .find(|key| own.contains_key(*key))
        {
            return Err(refused(format!(
                "matplotlib: {key} is the project's, under [matplotlib]: a figure's own \
                 settings are matplotlib's"
            )));
        }
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
                matplotlib: own,
            },
        );
    }

    // Refused as the file loads, as `--style` is refused: a style naming
    // nothing would otherwise pass until the first value is rendered.
    if let Some(style) = &written.render.style
        && !style.is_empty()
        && style != "plain"
        && !style_known(&written.style, style)
    {
        // Named with its file, as every other error of a loading file is.
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[render] style: {}",
                ConfigError::UnknownStyle {
                    name: style.clone(),
                    available: style_names(&written.style),
                }
            ),
        });
    }

    // A table's drawing, refused as the file loads: it stopped the first
    // table drawn, named in the file importing it, while every other command
    // used the configuration as valid.
    if let Some(table) = &written.render.table
        && !TABLE_STYLES.contains(&table.as_str())
    {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[render] table is {}, not '{table}'",
                TABLE_STYLES.join(" or ")
            ),
        });
    }

    if let Some(style) = &written.render.figure_style
        && style != "plain"
        && !style_known(&written.style, style)
    {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            line: None,
            message: format!(
                "[render] figure_style: {}",
                ConfigError::UnknownStyle {
                    name: style.clone(),
                    available: style_names(&written.style),
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
    if let Some(precision) = &written.render.precision {
        check_precision(path, "render", None, precision)?;
    }
    Ok(ProjectConfig {
        root,
        schema_version,
        model,
        python,
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
        styles,
        declared_styles: written.style.keys().cloned().collect(),
        queries,
        profiles,
        exports,
        figures,
        matplotlib: sheets_and_fonts(
            path,
            matplotlib_settings(path, written.matplotlib, "[matplotlib]")?,
        )?,
        colors: section_colors(path, "colors", written.colors)?,
        history_shared: written.history.shared,
        imports: Vec::new(),
        origins: IndexMap::new(),
        copies: Vec::new(),
        files_declared: None,
    })
}

/// One colours section, each role one the TUI names and each colour one it
/// can draw: refused at load, as any key a section does not have, naming the
/// section it was written in.
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
        if !COLOR_ROLES.iter().any(|(known, _)| known == role) {
            let roles: Vec<&str> = COLOR_ROLES.iter().map(|(role, _)| *role).collect();
            let near = crate::core::identifier::nearest(role, roles.iter().copied())
                .map(|near| format!(" — did you mean '{near}'?"))
                .unwrap_or_default();
            return Err(refused(format!(
                "[{section}] has no role '{role}'{near}\n  the roles: {}",
                roles.join(", ")
            )));
        }
        if !is_color(color) {
            let near = crate::core::identifier::nearest(color, COLOR_NAMES.iter().copied())
                .map(|near| format!(" — did you mean '{near}'?"))
                .unwrap_or_default();
            return Err(refused(format!(
                "[{section}] {role} = \"{color}\" is not a colour{near}\n  \
                 one of {}, or #rrggbb",
                COLOR_NAMES.join(", ")
            )));
        }
    }
    Ok(written)
}

/// `[matplotlib]` as matplotlib names its settings: a key written unquoted,
/// `font.size = 13`, nests in TOML and is read back as the name it spells. A
/// table left over is a setting nobody names.
fn matplotlib_settings(
    path: &Path,
    written: toml::Table,
    within: &str,
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
                "{within} {name} is written twice, once dotted and once as a table: \
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
                "{within} {name} is not a figure's: samplekit chooses between a window \
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
            message: format!("{within} {name}: a date is not a matplotlib setting"),
        });
    }
    Ok(settings)
}

/// `[matplotlib] style` and `fonts`, SampleKit's keys beside matplotlib's: a
/// style sheet by name or as a `.mplstyle` file, one or a list; font files or
/// patterns, one or a list. A path is read from the file declaring it, and
/// held whole. Their existence is matplotlib's to judge, as it draws.
fn sheets_and_fonts(
    path: &Path,
    mut settings: IndexMap<String, toml::Value>,
) -> Result<IndexMap<String, toml::Value>, ConfigError> {
    let folder = path.parent().unwrap_or(Path::new("."));
    for key in ["style", "fonts"] {
        let Some(value) = settings.get(key) else {
            continue;
        };
        let written: Vec<String> = match value {
            toml::Value::String(one) => vec![one.clone()],
            toml::Value::Array(items) if items.iter().all(toml::Value::is_str) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
            _ => {
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    line: None,
                    message: format!(
                        "[matplotlib] {key}: {}",
                        if key == "style" {
                            "a style sheet's name or a .mplstyle file, or a list of them: \
                             style = [\"seaborn-v0_8-paper\", \"figures.mplstyle\"]"
                        } else {
                            "a font file or a pattern of them, or a list: fonts = \
                             [\"fonts/*.otf\"]"
                        }
                    ),
                });
            }
        };
        let resolved: Vec<toml::Value> = written
            .into_iter()
            .map(|entry| {
                let a_path = key == "fonts" || entry.to_ascii_lowercase().ends_with(".mplstyle");
                if a_path && Path::new(&entry).is_relative() {
                    // By the path's parts, which know Windows's separators:
                    // by the words of `/`, `sub\../paper.mplstyle` kept its `..`.
                    toml::Value::String(
                        lexically_normal(&folder.join(&entry))
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    toml::Value::String(entry)
                }
            })
            .collect();
        settings.insert(key.to_string(), toml::Value::Array(resolved));
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

/// The first item written twice.
fn twice<'a>(items: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for item in items {
        if seen.contains(&item) {
            return Some(item);
        }
        seen.push(item);
    }
    None
}

/// Two columns that would emit one header name collide in the exported file,
/// where nothing could tell them apart afterwards.
fn check_headers(profile: &str, columns: &[ColumnSpec]) -> Result<(), ConfigError> {
    // Compared **after flattening**, which is the only comparison that catches
    // the real case: `diameter` and `diameter.u` declare different headers and emit
    // `diameter_uncertainty` twice.
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

    /// `[model] python` as written, with a model declared or without one.
    pub fn python(&self) -> Option<&Path> {
        self.python.as_deref()
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
    /// `quantity` is the address — `modulus`, or `ageing.hardness` —
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
        // exception, and needs no declaration: it is the absence of a style;
        // so is `files`, the files' own spelling.
        if style != "plain" && style != FILES_STYLE && !self.styles.contains_key(style) {
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
        // The quantity's own declaration, then `[render] precision`, which
        // is every number's that no declaration gives one.
        let precision = presentation.precision.clone().or_else(|| {
            declared
                .and_then(|d| d.precision.as_ref())
                .or(self.render.precision.as_ref())
                .and_then(PrecisionSchema::precision)
        });

        // A lookup that finds nothing falls back to the spelling itself, which
        // is legible by construction.
        let unit = unit.map(|spelling| match self.units.get(&spelling) {
            Some(variants) if style != FILES_STYLE => {
                variants.get(style).cloned().unwrap_or(spelling)
            }
            _ => spelling,
        });
        // A variant is keyed as it is written: `symbol_math` for the `math`
        // style, which is what makes adding a style a declaration rather than
        // a format change.
        let symbol = match (style, declared) {
            ("plain" | FILES_STYLE, _) => symbol,
            (_, Some(declaration)) => declaration
                .symbol_variants
                .get(&format!("symbol_{style}"))
                .cloned()
                .or(symbol),
            _ => symbol,
        };
        let settings = self.styles.get(style);
        // A style that escapes its text writes a unit so: `%` began a LaTeX
        // comment, beside a value as in `\SI{…}{%}`. A LaTeX form — `\percent`,
        // `^\circ\mathrm{C}` — is written as it is in a form, and in math mode
        // alone.
        let escape = settings.map(|settings| &settings.escape);
        let (unit_alone, unit_in_form) = match (&unit, escape) {
            (Some(unit), Some(escape)) if *escape != Escape::None => {
                let math = unit.contains(['\\', '^', '_', '{']);
                let alone = match escape {
                    Escape::Latex if math => format!("${unit}$"),
                    _ => escape.apply(unit),
                };
                let in_form = if math {
                    unit.clone()
                } else {
                    escape.apply(unit)
                };
                (Some(alone), Some(in_form))
            }
            _ => (None, None),
        };
        Ok(Resolved {
            unit,
            symbol,
            property: settings.and_then(|settings| settings.property.clone()),
            uncertainty: settings.and_then(|settings| settings.uncertainty.clone()),
            unitless: settings.and_then(|settings| settings.unitless.clone()),
            unit_alone,
            unit_in_form,
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

    /// The styles the project declares, in its order: a figure's to choose
    /// from, where a document's given styles — `html` — draw nothing.
    pub fn declared_style_names(&self) -> Vec<&str> {
        self.declared_styles.iter().map(String::as_str).collect()
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

    /// A table's export or a figure's, by name.
    pub fn export(&self, name: &str) -> Result<&Export, ConfigError> {
        self.exports
            .get(name)
            .ok_or_else(|| self.unknown(ProfileKind::Export, name, self.export_names()))
    }

    /// A table's export; a figure's is refused, saying so.
    pub fn table_export(&self, name: &str) -> Result<&ExportTarget, ConfigError> {
        match self.export(name)? {
            Export::Table(target) => Ok(target),
            Export::Figure(figure) => Err(ConfigError::FigureExport {
                name: name.to_string(),
                figure: figure.figure.clone(),
            }),
        }
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
    /// A text format by its name: the project's declaration, else the one
    /// SampleKit gives.
    /// The document a style writes, where it writes one: given, or declared.
    pub fn text_format(&self, name: &str) -> Option<&TextFormat> {
        self.styles
            .get(name)
            .and_then(|settings| settings.document.as_ref())
    }

    /// Every style writing documents, by name: those given, then those
    /// declared.
    pub fn text_format_names(&self) -> Vec<&str> {
        self.styles
            .iter()
            .filter(|(_, settings)| settings.document.is_some())
            .map(|(name, _)| name.as_str())
            .collect()
    }

    pub fn matplotlib(&self) -> &IndexMap<String, toml::Value> {
        &self.matplotlib
    }

    /// The colour of a role: the project's, or the default
    /// [`COLOR_ROLES`] gives it.
    pub fn color(&self, role: &str) -> Option<&str> {
        self.colors.get(role).map(String::as_str).or_else(|| {
            COLOR_ROLES
                .iter()
                .find(|(known, _)| *known == role)
                .map(|(_, color)| *color)
        })
    }

    /// The files imported, nearest first.
    pub fn imports(&self) -> &[PathBuf] {
        &self.imports
    }

    /// `[history] shared`, where this file or one it imports declares it:
    /// the history's `.gitignore` written for git to carry it, or to hide it.
    pub fn history_shared(&self) -> Option<bool> {
        self.history_shared
    }

    /// Where `[collection] files` was declared, when an imported file
    /// declares it, and its entries as that file writes them, in the order of
    /// `collection().files`.
    pub fn files_declared(&self) -> Option<(&Path, &[String])> {
        self.files_declared
            .as_ref()
            .map(|(file, written)| (file.as_path(), written.as_slice()))
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
            within: None,
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
        /// The file and the declaration naming it, where one does: *no profile
        /// named 'nope'* alone left the reader to find which export wrote it.
        within: Option<String>,
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
    /// A figure's export asked for where a table's is read: its profile,
    /// its columns.
    FigureExport {
        name: String,
        figure: String,
    },
    /// A declared precision that is not a specifier.
    InvalidPrecision {
        path: PathBuf,
        section: String,
        reason: String,
    },
    /// A `range` that is not two numbers, the lower first.
    InvalidRange {
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

/// A file and the line of `key` in its `[section]`, `.samplekitrc:4`, as a
/// parse error names its line: a precision refused after the file was parsed
/// was said without one. The file alone where the line is not found.
fn at_line(path: &Path, section: &str, key: &str) -> String {
    let found = std::fs::read_to_string(path).ok().and_then(|text| {
        let mut inside = false;
        text.lines().enumerate().find_map(|(at, line)| {
            let line = line.trim();
            if let Some(header) = line.strip_prefix('[') {
                let header: String = header
                    .split(']')
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| !matches!(c, '"' | '\'' | ' '))
                    .collect();
                inside = header == section.replace(['"', '\'', ' '], "");
                return None;
            }
            (inside
                && line
                    .strip_prefix(key)
                    .is_some_and(|rest| rest.trim_start().starts_with('=')))
            .then_some(at + 1)
        })
    });
    match found {
        Some(line) => format!("{}:{line}", path.display()),
        None => path.display().to_string(),
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidPrecision {
                path,
                section,
                reason,
            } => write!(
                f,
                "{}: [{section}] precision: {reason}",
                at_line(path, section, "precision")
            ),
            ConfigError::InvalidRange {
                path,
                section,
                reason,
            } => write!(
                f,
                "{}: [{section}] range: {reason}",
                at_line(path, section, "range")
            ),
            ConfigError::UnknownKey {
                path,
                section,
                key,
                suggestion,
            } => write!(
                f,
                "{}: [{section}] takes no key '{key}'{}\n  a declaration takes unit, symbol, precision, range and symbol_<style>",
                path.display(),
                suggestion
                    .as_ref()
                    .map(|name| format!(" — did you mean '{name}'?"))
                    .unwrap_or_default()
            ),
            ConfigError::FigureExport { name, figure } => write!(
                f,
                "'{name}' exports the figure '{figure}', not a table: it has no profile \
                 nor columns"
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
                 version it was written for — write schema_version = 1 on its first line",
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
                within,
            } => {
                if let Some(within) = within {
                    write!(f, "{within}: ")?;
                }
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
