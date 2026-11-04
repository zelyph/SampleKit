#!/usr/bin/env sh
# Write examples/tensile again from fixtures/tensile: the demo the binary
# embeds for the start page, and the example the documentation walks through.
# Run it after changing anything under the fixture, then commit what it wrote;
# tools/check-docs.py fails until they agree.
#
#   tools/regenerate-demo.sh
#
# It needs the repository's .venv with the package built by `maturin develop`,
# as fixtures/tensile/build.sh does.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
demo="$root/examples/tensile"
# Its computations record their formulas' digests in a state folder of their
# own, never in the machine's: those records name the demo's samples, which
# are removed and written again.
state="$(mktemp -d)"
# Built beside it, then put in its place: the binary build.sh builds embeds
# examples/tensile, and refuses to build without it.
built="$root/target/tensile-regenerated"
trap 'rm -rf "$state" "$built"' EXIT
rm -rf "$built"
SAMPLEKIT_STATE_DIR="$state" "$root/fixtures/tensile/build.sh" "$built"
# What the computation left beside the steps is no part of the demo.
find "$built" \( -name .samplekit -o -name __pycache__ \) -prune -exec rm -rf {} +
rm -f "$built/.venv"
rm -rf "$demo"
mv "$built" "$demo"
echo "examples/tensile written again: commit it"
