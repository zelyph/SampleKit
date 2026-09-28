#!/usr/bin/env sh
# Build the home-brewing demo: a folder per step, from three brews written by
# hand to two projects, figures and Python. Each step's README.md says what it
# shows; the documentation walks through them.
#
#   fixtures/brewing/build.sh [DESTINATION]
#
# DESTINATION is target/brewing by default; examples/brewing is the copy
# committed and embedded in the binary, which tools/regenerate-demo.sh writes
# with this script. It refuses to write over a step that exists. It needs the
# repository's .venv with the package built by `maturin develop`.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
destination="${1:-$root/target/brewing}"
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
"$python" "$root/fixtures/brewing/generate.py" "$destination"


# Inside the repository its .venv is found above the steps; outside, linked.
case "$destination/" in
    "$root"/*) ;;
    *) [ -e "$destination/.venv" ] || ln -s "$root/.venv" "$destination/.venv" ;;
esac

compute() {
    step="$1"
    shift
    (cd "$destination/$step" && "$bin" compute brews "$@" --write > /dev/null)
}

# Step 4: everything computed but the blonde saison, which never was; the
# smoked porter waits for its final gravity.
compute 04-computing -f 'name != blonde-saison'
# Steps 5, 6, 8: all computed.
compute 05-fermentation
compute 06-figures
compute 08-python
# Step 7: Tom's English pale never computed; the rest computed.
compute 07-two-brewers/ana
compute 07-two-brewers/tom -f 'name != english-pale'

# After the computation: the dry stout's grain weighed again (its efficiency
# is outdated), and the double IPA's alcohol copied from its label by hand (an
# override).
"$python" - "$destination" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])

def edit(file, pattern, replacement):
    path = root / file
    text, count = re.subn(pattern, replacement, path.read_text())
    assert count == 1, f"{file}: {pattern}"
    path.write_text(text)

for step in ("04-computing", "07-two-brewers/ana"):
    edit(f"{step}/brews/dry-stout.md", r"grain_mass: \{v: 4\.1,", "grain_mass: {v: 4.3,")
for step in ("04-computing", "07-two-brewers/tom"):
    edit(f"{step}/brews/double-ipa.md", r"(  abv:\n    v: )[0-9.]+", r"\g<1>8.5")
PY

echo "brewing demo ready in $destination"
