# Step 5 · Projects

Two projects under one root: `rolled/`, the specimens of the steps before, and `cast/`, two cast ones. The root's `.samplekitrc` declares what both share; each project imports it, and names the one model, `model/specimen.py`.

Run every command from this folder.

```console
$ samplekit --profile overview
$ samplekit --query ductile -c material,elongation
$ samplekit cast list profiles
```

The guide to several projects explains this step: [several projects](https://zelyph.github.io/SampleKit/latest/guides/several-projects.html).
