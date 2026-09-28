#!/usr/bin/env bash
# Build the documentation site of one version into the tree of every version
# kept.
#
#   tools/build-docs-version.sh VERSION [--latest | --auto-latest] [--into DIR]
#
# The tree, target/site-versions/ unless --into names another:
#
#   index.html      sends a visitor to latest/
#   versions.json   {"latest": "1.0.0", "versions": ["1.0.0", "1.0.0-rc.1", …]},
#                   newest first: what the selector of every page reads
#   latest/         a copy of the latest version
#   1.0.0/  1.0.0-rc.1/  …
#   .nojekyll       so that GitHub Pages serves _static/ as it is
#
# --latest makes VERSION the latest. --auto-latest applies the rule of the
# published site: the latest is the newest release that is not a release
# candidate, or, while there is none, the newest release candidate. Without
# either, `latest` is left as it was.
#
# VERSION is the folder's name, and the list's; the pages say the version of
# Cargo.toml, which a different VERSION is warned about. The site is built
# as tools/check.sh builds it, warnings counted as errors, with the Sphinx of
# .venv unless SPHINX_BUILD names another; the Python reference is read from
# the extension installed there (`maturin develop --release`).
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
    sed -n '5p' "$0" | sed 's/^# *//' >&2
    exit 1
}

version=""
latest="keep"
into="target/site-versions"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --latest) latest="yes" ;;
        --auto-latest) latest="auto" ;;
        --into)
            [[ $# -ge 2 ]] || usage
            into="$2"
            shift
            ;;
        -h | --help) usage ;;
        -*)
            echo "error: unknown option '$1'" >&2
            usage
            ;;
        *)
            [[ -z "$version" ]] || usage
            version="$1"
            ;;
    esac
    shift
done
[[ -n "$version" ]] || usage

# A version is a semantic version, 1.0.0 or 1.0.0-rc.1: it names a folder
# of the published site, and the list is ordered by it.
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
    echo "error: '$version' is not a version such as 1.0.0 or 1.0.0-rc.1" >&2
    exit 1
fi

cargo_version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
if [[ "$version" != "$cargo_version" ]]; then
    echo "warning: building $version from sources that say $cargo_version: its pages will say $cargo_version" >&2
fi

sphinx="${SPHINX_BUILD:-.venv/bin/sphinx-build}"
if ! command -v "$sphinx" >/dev/null; then
    echo "error: no Sphinx at $sphinx" >&2
    echo "  .venv/bin/pip install -r docs/requirements.txt, or set SPHINX_BUILD" >&2
    exit 1
fi

mkdir -p "$into"
staging="$into/.building-$version"
rm -rf "$staging"
trap 'rm -rf "$staging"' EXIT
"$sphinx" -E -W --keep-going -q docs "$staging"
rm -rf "$staging/.doctrees" "$staging/.buildinfo"

rm -rf "${into:?}/$version"
mv "$staging" "$into/$version"
echo "built $into/$version"

# The list, and which version is the latest.
python3 - "$into" "$version" "$latest" <<'PY'
import json
import re
import shutil
import sys
from pathlib import Path

into, version, latest_rule = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
listing_path = into / "versions.json"


def key(v):
    """Newest first: 1.0.0 after 1.0.0-rc.2, after 1.0.0-rc.1."""
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-rc\.(\d+))?", v)
    if m is None:
        sys.exit(f"error: {listing_path} lists '{v}', which is not a version")
    major, minor, patch, rc = m.groups()
    return (int(major), int(minor), int(patch), rc is None, int(rc or 0))


listing = json.loads(listing_path.read_text()) if listing_path.exists() else {}
versions = set(listing.get("versions", [])) | {version}
# A version listed whose folder is gone would be offered and not found.
versions = sorted((v for v in versions if (into / v).is_dir()), key=key, reverse=True)
latest = listing.get("latest")
if latest_rule == "yes":
    latest = version
elif latest_rule == "auto":
    releases = [v for v in versions if "-rc." not in v]
    latest = (releases or versions)[0]
if latest not in versions:
    latest = None

listing = {"latest": latest, "versions": versions}
listing_path.write_text(json.dumps(listing, indent=2) + "\n")
print(f"versions   {', '.join(versions)}")
print(f"latest     {latest or '(none)'}")

if latest is not None:
    target = into / "latest"
    staging = into / ".latest"
    shutil.rmtree(staging, ignore_errors=True)
    shutil.copytree(into / latest, staging)
    shutil.rmtree(target, ignore_errors=True)
    staging.rename(target)
PY

cat >"$into/index.html" <<'HTML'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>SampleKit documentation</title>
<meta http-equiv="refresh" content="0; url=latest/">
<link rel="canonical" href="latest/">
</head>
<body>
<p>The documentation of the latest version is at <a href="latest/">latest/</a>.</p>
</body>
</html>
HTML
touch "$into/.nojekyll"
