# SampleKit

Sample data in plain Markdown: computed, current, tracked.

SampleKit keeps each measured sample as one Markdown file. At the top of the
file are its values, with their uncertainties and units; below them is a note
that is yours, which SampleKit never changes. A folder of these files is a
collection. Any text editor and git can read it, and there is no database to
set up.

With these files, SampleKit:

- selects and compares samples, and shows them as tables;
- computes values from the ones you measured, with formulas you write in
  Python;
- tells you which computed values are no longer current, and why;
- keeps every change it writes, so that you can see an earlier state of the
  project and go back to it;
- draws figures and writes tables for a paper.

You use it from the command line, from a full-screen terminal interface (the
TUI), or from Python.

## The documentation

- **[Tutorial](tutorial/index.md)**, for a first contact. It follows a demo
  project, a home brewer's notebook, in eight steps, from three brews written
  by hand to a model, figures and two projects.
- **[Explanations](explanations/index.md)**, for understanding how SampleKit
  works: what a sample is, how values and uncertainties are held, why a value
  is current or not, what the history keeps.
- **[How-to guides](how-to/index.md)**, for a task you already have in mind:
  moving existing data in, checking a project in continuous integration,
  sharing a project, exporting for a paper.
- **[Reference](reference/index.md)**, for looking something up: every command
  and its options, the Python API, the TUI's keys, every key of
  `.samplekitrc`, the sample file format, the exit codes.
- **[Changelog](changelog.md)**: what changed from one version to the next.

```{toctree}
:hidden:
:maxdepth: 2

tutorial/index
explanations/index
how-to/index
reference/index
changelog
```
