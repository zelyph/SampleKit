#!/usr/bin/env python3
"""Write THIRD-PARTY-LICENSES.md: the licence of every crate the command line
and the extension are built from, which a copy of either must carry.

The crates are those `cargo metadata` resolves for every feature and every
system, as normal and build dependencies — the tests' are in no copy. Each
crate's text is the licence file it ships, read from Cargo's registry, which
`cargo fetch` fills; a crate whose licence is a choice (`MIT OR Apache-2.0`)
is given under the first of its files. A crate that ships no licence file is
listed with its licence's name alone, and said on the standard error.

    tools/generate-third-party-licenses.py           write the file
    tools/generate-third-party-licenses.py --check   exit 1 when it is not
                                                     what the lock makes
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "THIRD-PARTY-LICENSES.md"
# The file names a crate's licence goes by, the most specific first.
NAMES = ("LICENSE-MIT", "LICENSE-APACHE", "LICENSE", "LICENCE", "COPYING",
         "LICENSE-BSD", "LICENSE-ZLIB", "UNLICENSE", "LICENSE-UNICODE")


def metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--all-features", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def shipped(meta):
    """The packages reached from the root by normal and build dependencies."""
    packages = {package["id"]: package for package in meta["packages"]}
    nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        for dep in nodes[current]["deps"]:
            kinds = {kind["kind"] for kind in dep["dep_kinds"]}
            if kinds <= {"dev"}:
                continue
            if dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    return sorted((packages[id] for id in seen),
                  key=lambda package: (package["name"].lower(), package["version"]))


def licence_files(package):
    folder = Path(package["manifest_path"]).parent
    found = []
    for path in sorted(folder.iterdir()):
        stem = path.name.upper().split(".")[0]
        if path.is_file() and any(stem.startswith(name) for name in NAMES):
            found.append(path)
    order = {name: index for index, name in enumerate(NAMES)}
    return sorted(found, key=lambda path: min(
        (index for name, index in order.items() if path.name.upper().startswith(name)),
        default=len(NAMES)))


COPYRIGHT = re.compile(r"^\s*(?:copyright\b|\(c\)\s|©)(?!.*\[yyyy\])", re.I)
KINDS = [
    ("Apache-2.0", "Apache License"),
    ("MPL-2.0", "Mozilla Public License"),
    ("BSL-1.0", "Boost Software License"),
    ("Unlicense", "This is free and unencumbered software"),
    ("Unicode-3.0", "UNICODE LICENSE"),
    ("Zlib", "This software is provided 'as-is'"),
    ("BSD-3-Clause", "Neither the name"),
    ("BSD-2-Clause", "Redistribution and use in source and binary forms"),
    ("ISC", "Permission to use, copy, modify, and/or distribute"),
    ("MIT", "Permission is hereby granted"),
]


def kind(body, declared):
    for name, words in KINDS:
        if words.lower() in body.lower():
            return name
    return declared


def split(body):
    """A licence file's copyright lines, and its text without them."""
    lines = body.strip().splitlines()
    notices = [line.strip() for line in lines if COPYRIGHT.match(line)]
    rest = "\n".join(line for line in lines if not COPYRIGHT.match(line)).strip()
    return notices, re.sub(r"\n{3,}", "\n\n", rest)


def text():
    """The notice: one section per distinct licence text, naming every crate
    under it with its own copyright lines, as cargo-about writes it."""
    groups = {}
    missing = []
    for package in shipped(metadata()):
        name = f"{package['name']} {package['version']}"
        declared = package.get("license") or "unstated"
        files = licence_files(package)
        if not files:
            missing.append((name, declared))
            continue
        notices, body = split(files[0].read_text(errors="replace"))
        key = re.sub(r"\s+", " ", body)
        group = groups.setdefault(key, {"kind": kind(body, declared), "text": body, "crates": []})
        group["crates"].append((name, notices))
    ordered = sorted(groups.values(), key=lambda group: (-len(group["crates"]), group["kind"]))
    lines = [
        "# Third-party licences",
        "",
        "SampleKit's command line and its Python extension are built from the",
        "crates below, each under its own licence. The crates that share a",
        "licence's text are listed together, each with its copyright lines, and",
        "the text follows once. Written by tools/generate-third-party-licenses.py",
        "from Cargo.lock.",
        "",
        "The source of every one of them, those under the Mozilla Public License",
        "2.0 included, is published on crates.io, at",
        "`https://crates.io/crates/NAME/VERSION`.",
        "",
        "| Licence | Crates |",
        "| --- | --- |",
    ]
    counts = {}
    for group in ordered:
        counts[group["kind"]] = counts.get(group["kind"], 0) + len(group["crates"])
    for name, count in sorted(counts.items(), key=lambda item: -item[1]):
        lines.append(f"| {name} | {count} |")
    if missing:
        lines.append(f"| declared only | {len(missing)} |")
    for number, group in enumerate(ordered, 1):
        lines += ["", f"## {number}. {group['kind']}", "", "Used by:", ""]
        for name, notices in group["crates"]:
            said = "; ".join(notices)
            lines.append(f"- {name}" + (f" — {said}" if said else ""))
        lines += ["", "```text", group["text"].replace("```", "\'\'\'"), "```"]
    if missing:
        lines += ["", "## Crates that ship no licence file", "",
                  "Their text is the standard one of the licence their manifest",
                  "declares, at https://spdx.org/licenses/.", ""]
        lines += [f"- {name} — {declared}" for name, declared in missing]
    return "\n".join(lines) + "\n", [name for name, _ in missing]


def main():
    written, missing = text()
    if "--check" in sys.argv[1:]:
        if not OUTPUT.exists() or OUTPUT.read_text() != written:
            print("THIRD-PARTY-LICENSES.md is not what Cargo.lock makes: "
                  "run tools/generate-third-party-licenses.py")
            sys.exit(1)
        return
    for name in missing:
        print(f"no licence file in {name}", file=sys.stderr)
    OUTPUT.write_text(written)
    print(f"wrote {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
