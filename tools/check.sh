#!/usr/bin/env bash
# Everything a commit must pass, in the order that fails fastest.
#
#   tools/check.sh           check what is built, the documentation site included
#   tools/check.sh --build   rebuild the extension and the binary first
#
# The Python tests and the model_runtime tests run against the extension
# `maturin develop` installed in .venv, not against the source: they refuse a
# build older than the code, and --build is how that is answered. It reinstalls
# the extension, which the owner may be using — say so before running it.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$*"; }

if [[ "${1:-}" == "--build" ]]; then
    step "maturin develop --release"
    .venv/bin/maturin develop --release
fi

step "cargo fmt --check"
cargo fmt --check

step "cargo clippy"
cargo clippy --all-targets --features python -- -D warnings

step "cargo doc"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --quiet

step "cargo test"
cargo build --quiet
cargo test --quiet

step "pytest"
.venv/bin/python -m pytest -q

step "docs and the demo"
# Every link of the user guide, and every command it and the demo's steps
# show, run against the demo built afresh; and examples/brewing,
# committed and embedded in the binary, is that demo file for file.
tools/check-docs.py

step "the reference, generated"
# docs/reference/cli.md is the help of the binary just built; the TUI's page,
# docs/reference/tui.md, is checked by a test of cargo test above.
tools/generate-cli-reference.py --check --binary target/debug/samplekit

step "the third-party licences, generated"
# THIRD-PARTY-LICENSES.md goes with every binary and wheel: the licences of
# the crates Cargo.lock names, read from Cargo's registry.
tools/generate-third-party-licenses.py --check

step "the stub's docstrings, generated"
# python/samplekit/__init__.pyi carries the package's docstrings, so that an
# editor shows them: written by the tool from the extension in .venv.
.venv/bin/python tools/generate-stub-docstrings.py --check

step "the documentation site"
# Warnings are errors: a broken link, a page no table of contents reaches, a
# docstring autodoc cannot read; -E reads every page again, so that a warning
# of a page unchanged is not lost. The Python reference is read from the
# extension installed in .venv, as pytest is.
if [[ -x .venv/bin/sphinx-build ]]; then
    .venv/bin/sphinx-build -E -W --keep-going -q docs target/site
else
    echo "skipped: Sphinx is not installed in .venv"
    echo "  .venv/bin/pip install -r docs/requirements.txt"
fi

printf '\nall checks passed\n'
