# Step 1 · Measured

Three specimens, a file each in `samples/`: their material, their grade, the day they were tested, and their width and thickness, one number each, written by hand. No model yet: `.samplekitrc` holds one line.

Run every command from this folder.

```console
$ samplekit .
$ samplekit -c material,grade,width,thickness
$ samplekit -f 'material == aluminium || direction == T' -c material,direction
$ samplekit validate
```

The home page explains this step: [samples and projects](https://zelyph.github.io/SampleKit/latest/index.html#samples-and-projects).
