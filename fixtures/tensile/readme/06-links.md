# Step 6 · Links

The specimens of step 4, their results computed, each linking to the others, to its own curve, to the method they were tested by and to the standard it follows: every form an attribute's link takes. `machine/` holds each specimen's files, as in step 4, and `docs/method.pdf` the method, two pages.

Their links, as each file writes them:

```yaml
# samples/aluminium-01.md
compared_with: "[[steel-01]]"                     # a sample, by its file's name
method: "[the method](../docs/method.pdf#page=2)"  # a PDF at a page, by its path from the sample's folder
standard: "[ISO 6892-1](https://www.iso.org/standard/78322.html)"  # a web page

# samples/copper-01.md
compared_with: "[[samples/steel-01]]"   # by the end of its path
tested_after: "[[aluminium-01.md]]"     # with its extension
raw: "[[copper-01/curve.csv]]"          # a file by its name: three are curve.csv, so with its folder
method: "[[method.pdf#page=2]]"         # the PDF by its name, at a page

# samples/steel-01.md
compared_with: ["[[aluminium-01]]", "[[copper-01]]"]  # a list of links
tested_after: "[[Copper-01]]"                         # in another case
raw: "[the curve](../machine/steel-01/curve.csv)"     # its own curve, by its path
standard: https://www.iso.org/standard/78322.html     # an address alone
repeated_by: "[[steel-02]]"                           # leads nowhere: no sample is steel-02
```

Another tool's address is a link too, opened by the application that answers to it, as `source: zotero://select/library/items/ABCD1234` would open a reference in Zotero; none is written here, since no library holds it.

Run every command from this folder.

```console
$ samplekit -c name,compared_with,tested_after
$ samplekit samples/steel-01.md view
$ samplekit samples/aluminium-01.md rename aluminium-02
$ samplekit list files
```

In the terminal interface, `samplekit` here, `Enter` or `o` on a link goes where it leads: on the copper specimen's screen, its `tested_after` opens the aluminium specimen's, and `Esc` comes back; its `raw` opens the curve in your spreadsheet, its `method` the PDF at its second page, in the browser. On the steel specimen's `compared_with`, a list, `Enter` lists the two samples, and `Enter` follows one. At the end of a sample's screen, *linked from* lists the samples whose links lead to it, however they write it: the steel specimen's lists the aluminium and copper ones.

`samplekit validate` reports the link that leads nowhere, the steel specimen's `repeated_by`, and exits with code 2. A web page, and another tool's address, are not checked.

`rename` previews the aluminium specimen renamed: its file, its `name`, its folder under `machine/`, and the two links leading to it, each written again in its own form, `[[aluminium-02.md]]` with its extension, `[[aluminium-02]]` in the list. `--write` makes it.

The home page explains this step: [links](https://zelyph.github.io/SampleKit/latest/index.html#links); they have a guide, [link samples](https://zelyph.github.io/SampleKit/latest/guides/link-samples.html), and every form is in [links in a sample file](https://zelyph.github.io/SampleKit/latest/reference/sample-files.html#links).
