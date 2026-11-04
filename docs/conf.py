"""Sphinx configuration for SampleKit's documentation.

Build the site from the repository's root:

    .venv/bin/pip install -r docs/requirements.txt
    .venv/bin/maturin develop --release
    .venv/bin/sphinx-build -E -W --keep-going docs target/site

The Python reference (reference/python.md) is read from the package itself,
so `maturin develop` must have been run into the same environment first: the
package's native half, `samplekit._native`, exists only once it is built. An
extension older than the code documents the previous version.

tools/check.sh builds it this way, warnings counted as errors.
tools/build-docs-version.sh builds it the same way into the folder of one
version, beside the others the published site keeps.
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# The package's Python half, and the native module `maturin develop` puts
# beside it.
sys.path.insert(0, str(ROOT / "python"))
# The documentation's own extensions: its lexers.
sys.path.insert(0, str(Path(__file__).resolve().parent / "_ext"))

project = "SampleKit"
author = "zelyph"
copyright = "2026, zelyph"
release = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
version = release

extensions = [
    "samplekit_lexers",
    "myst_parser",
    "sphinx.ext.autodoc",
    "sphinx.ext.napoleon",
]

source_suffix = {".md": "markdown"}
root_doc = "index"

# The guide written before the documentation took its four parts. Its pages
# stay in docs/ until the new ones have taken what they hold, and are not part
# of the site.
exclude_patterns = [
    ".obsidian",
    "_build",
]

# Written as typed: `--write` outside a code span is an option, never an
# en dash, and a quote stays a straight quote.
smartquotes = False

myst_enable_extensions = ["colon_fence", "deflist", "attrs_block", "dollarmath"]
# Headings down to ### get an anchor a link can name: `page.md#a-heading`.
myst_heading_anchors = 3

autodoc_member_order = "bysource"
autodoc_typehints = "description"
autodoc_default_options = {"members": True}
napoleon_google_docstring = True
napoleon_numpy_docstring = True

html_theme = "furo"
html_title = f"SampleKit {release}"
templates_path = ["_templates"]
html_static_path = ["_static"]
html_css_files = ["versions.css", "samplekit.css"]
# The TUI's cyan as the brand colour; code coloured as on GitHub.
html_theme_options = {
    "light_css_variables": {
        "color-brand-primary": "#0e7490",
        "color-brand-content": "#0e7490",
        "color-brand-visited": "#0e7490",
        "color-inline-code-background": "#ecfeff",
    },
    "dark_css_variables": {
        "color-brand-primary": "#22d3ee",
        "color-brand-content": "#22d3ee",
        "color-brand-visited": "#22d3ee",
        "color-inline-code-background": "#16323a",
    },
}
pygments_style = "friendly"
pygments_dark_style = "github-dark"
html_js_files = ["versions.js"]
# Furo's sidebar, with the version selector below the title. It shows only on
# a site built by tools/build-docs-version.sh, which keeps a folder per
# version and the list of them the selector reads.
html_sidebars = {
    "**": [
        "sidebar/brand.html",
        "sidebar/versions.html",
        "sidebar/search.html",
        "sidebar/scroll-start.html",
        "sidebar/navigation.html",
        "sidebar/scroll-end.html",
    ]
}


# ------------------------------------------------------------------ moved pages
#
# Pages published at another address before: each old address is a page that
# sends its reader on, so that a link kept from 1.0.0-rc.1, or printed by it,
# still arrives. The guides were `how-to/`, the concepts `explanations/`, and
# the tutorial, retired, is the home page's walkthrough now.
MOVED = {
    **{
        f"how-to/{page}": f"guides/{page}"
        for page in (
            "index", "continuous-integration", "export-for-a-paper", "first-model",
            "move-existing-data", "run-a-script", "share-a-project", "upgrade",
        )
    },
    **{
        f"explanations/{page}": f"concepts/{page}"
        for page in (
            "index", "current-and-outdated", "history", "models-and-formulas",
            "projects", "readings-and-statistics", "samples-and-files",
            "values-and-uncertainty",
        )
    },
    **{
        f"tutorial/{page}": "index"
        for page in (
            "index", "01-first-brews", "02-measuring", "03-selecting", "04-computing",
            "05-fermentation", "06-figures", "07-two-brewers", "08-python",
        )
    },
}


def _write_moved(app, exception):
    if exception is not None or app.builder.format != "html":
        return
    out = Path(app.outdir)
    for old, new in MOVED.items():
        depth = old.count("/")
        target = "../" * depth + f"{new}.html"
        page = out / f"{old}.html"
        page.parent.mkdir(parents=True, exist_ok=True)
        page.write_text(
            "<!doctype html>\n<meta charset=\"utf-8\">\n"
            f"<title>Moved: {new}</title>\n"
            f"<meta http-equiv=\"refresh\" content=\"0; url={target}\">\n"
            f"<link rel=\"canonical\" href=\"{target}\">\n"
            f"<p>This page has moved: <a href=\"{target}\">{new}</a>.</p>\n",
            encoding="utf-8",
        )


def setup(app):
    app.connect("build-finished", _write_moved)
