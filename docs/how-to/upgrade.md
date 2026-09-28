# Upgrade from an older SampleKit

This version of SampleKit reads sample files of **schema 1**, and refuses any
other. This recipe moves samples written by SampleKit 0.2 into files this
version reads.

No command converts files. Whatever a tool did for the files, the model would
still have to be rewritten by hand — its class, its properties, its formulas —
so a converting command would save part of the work and take all the risk of
writing to your files. Nothing obliges you to upgrade: take your time.

## Find the files to move

A file from an older version is not read, and every command that meets it says
so, with the reason, and exits with code 2. The rest of the collection is read
as usual, and nothing is written:

```text
$ samplekit list samples/
warning: samples/S-00.md was not read: malformed frontmatter: line 3: 'data' is a mapping, and an attribute is one scalar or one homogeneous list
  it predates schema 1, and no command converts it: load it with the samplekit that wrote it, hand its values to this one and save
0 samples
…
1 file not read — samplekit list skipped
```

`samplekit validate samples/` reports each such file as a defect: one run
counts them.

## Move samples from SampleKit 0.2

1. **Install both versions**, each in its own Python environment, since two
   versions of one package cannot share one: `python -m venv old` and
   `python -m venv new`. SampleKit 0.2 is on the Python Package Index:
   `old/bin/pip install samplekit==0.2.0`.
2. **Write the new model**: its class, its properties and its formulas, as
   this version declares them ([A first model](first-model.md)).
3. **Load the samples with the old version** and its old model, and write what
   they hold to a JSON file, which both versions read.
4. **Read the JSON with the new version**, set the values on new samples, and
   **save** them — into a new folder, never over the originals.

**Carry everything a sample holds**: each quantity's value, uncertainty,
readings and unit, the tags, the attributes, the tables and the note. A copy
that keeps only the values gives files in which `validate` finds nothing
wrong, since every number it kept is right. Compare a few files by hand before
trusting the rest.

```python
# ── in the old environment, with the old SampleKit and the old model ──
import json
from pathlib import Path

import old_model

carried = []
for path in sorted(Path("samples").glob("*.md")):
    sample = old_model.Brew(path)
    carried.append({
        "file": path.name,
        "name": sample.name,
        "tags": list(sample.tags),
        "note": sample.note,
        # Adapt to your old model: its quantities and attributes by name.
        "quantities": {
            name: {"v": getattr(sample, name).value,
                   "u": getattr(sample, name).uncertainty,
                   "unit": getattr(sample, name).unit}
            for name in ("og", "volume")
        },
        "attributes": {name: getattr(sample, name) for name in ("brewer", "style")},
    })
Path("carried.json").write_text(json.dumps(carried, indent=1, default=str))
```

```python
# ── in the new environment, with the new SampleKit and the new model ──
import json
from pathlib import Path

import new_model

for held in json.loads(Path("carried.json").read_text()):
    fresh = new_model.Brew.new(Path("new-samples") / held["file"])
    fresh.name = held["name"]
    for tag in held["tags"]:
        fresh.tags.append(tag)
    fresh.note = held["note"]
    for name, numbers in held["quantities"].items():
        quantity = getattr(fresh, name)
        quantity.value = numbers["v"]
        quantity.uncertainty = numbers["u"]
    for name, value in held["attributes"].items():
        setattr(fresh, name, value)
    fresh.save()
```

Readings and tables are carried the same way — a list per quantity, a list of
rows per table — and are left out here to keep the example short. Then:

```console
$ samplekit validate new-samples/
$ samplekit compute new-samples/ --write
```

## Keys renamed since 0.2

A 0.2 file spelt a quantity's numbers `value` and `uncertainty`, and its
repeated measurements `data`. They are `v`, `u` and `readings` now, and the
old spellings are not read. The recipe above needs no change for it:
`save()` writes the new ones.

A file that already says `schema_version: 1` and still holds a long
spelling needs only the rename. `validate` names the key and what it is
written now:

```text
$ samplekit validate samples/
  S-01.md                not read: line 4: properties.og: 'value' is written 'v' since schema 1: rename it
```

## What needs nothing

**A file written differently from SampleKit's own form** is valid, and is
read. The next command that writes it rewrites it in the fixed form;
`validate` notes it, and there is nothing to run.
