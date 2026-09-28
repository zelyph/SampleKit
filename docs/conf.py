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

project = "SampleKit"
author = "zelyph"
copyright = "2026, zelyph"
release = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
version = release

extensions = [
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

myst_enable_extensions = ["colon_fence", "deflist"]
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
html_css_files = ["versions.css"]
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
