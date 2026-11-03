# Guides

Each guide is one task in your own project, its steps in order, its commands
shown with what they print. The [home page](../index.md) walks through a
whole project first, on a tensile test; a guide goes further on one subject,
and the [concepts](../concepts/index.md) say why SampleKit works as it
does.

**The guides' examples run in the demo.** SampleKit carries the home page's
tensile test as a demo in five steps, each a project of its own: a guide's
*last step* is the fourth, with the results, tables, figures and scripts; the
fifth holds two projects. `samplekit` alone opens the start page, where `g`,
then *The demo*, writes it, with a Python environment for its model, and
opens a step. A guide's `python scripts/…` runs with that environment's
Python ([the Python to use](work-from-python.md#the-python-to-use)).

## Getting started

| Guide | For |
| --- | --- |
| [Install SampleKit](install.md) | the command line and the Python package, on Linux, macOS and Windows |
| [Start a project](start-a-project.md) | a folder set up for SampleKit: its configuration, its environment, a first sample |
| [Bring existing data in](move-existing-data.md) | measurements in a spreadsheet, CSV files or a notebook, turned into sample files |
| [Write a model](first-model.md) | the values your Python formulas compute, and how SampleKit keeps them current |

## Working with samples

| Guide | For |
| --- | --- |
| [Edit samples](edit-samples.md) | a value measured again, an attribute, a tag, a table's row, a new sample |
| [Find and select samples](find-and-select.md) | which samples, which values, in which order, grouped how, and the table written to a file |
| [Link samples](link-samples.md) | a sample naming another, a file or a web page: shown, followed both ways, checked, kept when renamed |
| [Work in the terminal interface](terminal-interface.md) | the same work from one full-screen view, by the keyboard |
| [Work from Python](work-from-python.md) | your samples read, analysed and changed from a script or a notebook |
| [Run a script of the model](run-a-script.md) | a task written once in the model, run on the samples selected |

## Publishing and sharing

| Guide | For |
| --- | --- |
| [Tables and figures for a paper](export-for-a-paper.md) | a paper's tables and figures, written again by one command, made again as they were |
| [Several projects](several-projects.md) | series kept apart, read together, what they share declared once |
| [Organise a project's settings](organise-settings.md) | `.samplekitrc` in several files, each imported where it is needed |
| [Share a project](share-a-project.md) | one project on two computers, or with a colleague, through git or a synchronised folder |
| [Check a project in continuous integration](continuous-integration.md) | a job that fails when a file is wrong or a value is not current |

## When something goes wrong

| Guide | For |
| --- | --- |
| [When something goes wrong](when-something-goes-wrong.md) | a refusal, a warning, a value you do not trust: what it means, what to do |
| [Upgrade from an older SampleKit](upgrade.md) | a project written by an earlier version, with the migration tool or by hand |

```{toctree}
:hidden:

install
start-a-project
move-existing-data
first-model
edit-samples
find-and-select
link-samples
terminal-interface
work-from-python
run-a-script
export-for-a-paper
several-projects
organise-settings
share-a-project
continuous-integration
when-something-goes-wrong
upgrade
```
