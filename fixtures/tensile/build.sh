#!/usr/bin/env sh
# Build the tensile example of the home page: a folder per step, from three
# specimens written by hand to the results their curves give. The home page
# walks through it.
#
#   fixtures/tensile/build.sh [DESTINATION]
#
# DESTINATION is target/tensile by default; examples/tensile is the copy
# committed, which tools/regenerate-demo.sh writes with this script. It
# refuses to write over a step that exists. It needs the repository's .venv
# with the package built by `maturin develop`.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
destination="${1:-$root/target/tensile}"
case "$destination" in
    /*) ;;
    *) destination="$(pwd)/$destination" ;;
esac
bin="$root/target/release/samplekit"
python="$root/.venv/bin/python"
if ! "$python" -c "import samplekit._worker" 2>/dev/null; then
    echo "the repository's .venv has no samplekit: run 'maturin develop --release' with it active" >&2
    exit 1
fi
cargo build --release --quiet --manifest-path "$root/Cargo.toml"
"$python" "$root/fixtures/tensile/generate.py" "$destination"

# The steps name the .venv beside them: the repository's, linked.
[ -e "$destination/.venv" ] || ln -s "$root/.venv" "$destination/.venv"

# Every step with a model, computed and written: the files as the page leaves
# them at the end of each section.
for step in 02-readings 03-curve 04-results; do
    (cd "$destination/$step" && "$bin" samples compute --write > /dev/null)
done
for project in rolled cast; do
    (cd "$destination/05-projects/$project" && "$bin" samples compute --write > /dev/null)
done

echo "tensile example ready in $destination"
