# Step 4 · Results

What the curve gives: the Young's modulus, the yield and tensile strengths, the elongation. `.samplekitrc` declares a profile, a query, exports for a paper, a figure and its look; the model draws two figures of its own and holds two scripts; `photos/` and `machine/` hold each specimen's own files, and `scripts/` Python scripts that read them. The aluminium and copper specimens link to the steel one they are compared with, `compared_with: "[[steel-01]]"`.

Run every command from this folder.

```console
$ samplekit --profile overview
$ samplekit --query ductile --profile overview
$ samplekit export paper --write
$ samplekit plot curves -o curves.png --write
$ samplekit run ranking
$ samplekit list files
$ samplekit -f 'links to steel-01'
$ python scripts/report.py
```

The home page explains this step: [results computed from a table](https://zelyph.github.io/SampleKit/latest/index.html#results-computed-from-a-table); its links have a guide, [link samples](https://zelyph.github.io/SampleKit/latest/guides/link-samples.html).
