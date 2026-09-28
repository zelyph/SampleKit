#!/usr/bin/env sh
# Write examples/brewing again from fixtures/brewing: the demo the guide points
# to and the binary embeds. Run it after changing anything under
# fixtures/brewing, then commit examples/brewing; tools/check-docs.py fails
# until the two agree. The tutorial's pages, docs/tutorial/NN-step.md, are the
# steps' READMEs: tools/generate-tutorial.py writes them again with the demo.
#
#   tools/regenerate-demo.sh
#
# It needs the repository's .venv with the package built by `maturin develop`,
# as fixtures/brewing/build.sh does.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
demo="$root/examples/brewing"
# Its computations record their formulas' digests in a state folder of their
# own, never in the machine's: those records name the demo's samples, which
# are removed and written again.
state="$(mktemp -d)"
# Built beside it, then put in its place: the binary build.sh builds embeds
# examples/brewing, and refuses to build without it.
built="$root/target/demo-regenerated"
trap 'rm -rf "$state" "$built"' EXIT
rm -rf "$built"
SAMPLEKIT_STATE_DIR="$state" "$root/fixtures/brewing/build.sh" "$built"
# What the computation left beside the steps is no part of the demo.
find "$built" \( -name .samplekit -o -name __pycache__ \) -prune -exec rm -rf {} +
rm -rf "$demo"
mv "$built" "$demo"
python3 "$root/tools/generate-tutorial.py"
echo "examples/brewing and docs/tutorial written again: commit them"
