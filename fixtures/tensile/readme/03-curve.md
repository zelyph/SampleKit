# Step 3 · Curve

The test itself: each specimen holds a table, the force read at each strain, and the model computes the cross-section, then the stress in every row.

Run every command from this folder.

```console
$ samplekit samples/steel-01.md view
$ samplekit -c area,'curve.stress[10]'
$ samplekit -f 'curve where (strain > 30)' -c material
```

The home page explains this step: [a table inside a sample](https://zelyph.github.io/SampleKit/latest/index.html#a-table-inside-a-sample).
