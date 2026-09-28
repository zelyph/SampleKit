# The Python API

Everything `import samplekit` offers, read from the package itself: its
docstrings are the source of this page. Examples use `import samplekit as sk`.

```{eval-rst}
.. module:: samplekit
```

## Loading and saving

`load` reads samples from files and folders, as the command line does, the
project's model applied. A `Sample` is one file; a `SampleList` is several,
and filters, sorts and tabulates them. `keep` records what a script saved as
one snapshot of the project's history.

```{eval-rst}
.. autofunction:: samplekit.load

.. autoclass:: samplekit.Sample
   :members:
   :inherited-members:

.. autoclass:: samplekit.SampleList
   :members:

.. autofunction:: samplekit.keep
```

## Values

A sample's quantities are `Property` objects: a value, an uncertainty, a unit,
readings. A value that does not apply to a sample is `NA`. An attribute that
holds a list is a `BoundList`, which writes its changes through to the
sample.

```{eval-rst}
.. autoclass:: samplekit.Property
   :members:

.. autodata:: samplekit.NA
   :annotation:

.. autoclass:: samplekit.NotApplicable

.. autoclass:: samplekit.BoundList
   :members: append, extend, insert, pop, remove, clear, sort, reverse, copy
```

## Tables

A table of a sample holds rows of measurements, by column. `Column` declares
one column of a new `Table`: its unit and the statistics of its readings;
its symbol, like any symbol, is the project's. `ColumnView` and `RowView` read a table by column and by row.

```{eval-rst}
.. autoclass:: samplekit.Table
   :members:

.. autoclass:: samplekit.Column
   :members:

.. autoclass:: samplekit.ColumnView
   :members:

.. autoclass:: samplekit.RowView
   :members:
```

## Statistics

`stats` names the statistics a model can choose for readings: which one is
the value, which one the uncertainty. `Summary` is the statistics of a set of
readings.

```{eval-rst}
.. autoclass:: samplekit.stats
   :members:

.. autoclass:: samplekit.Statistic
   :members:

.. autoclass:: samplekit.Summary
   :members:
```

## Figures

`plot` draws a figure declared in `.samplekitrc`, or one given by its axes.
`figure` marks a model's method as a figure of its own.

```{eval-rst}
.. autofunction:: samplekit.plot

.. autofunction:: samplekit.figure
```

## The project

What a project declares in its `.samplekitrc`, as a script reads it: its
model, its queries, profiles, exports and figures, its units and quantities.
`project`, on a sample or a list of samples, gives it.

```{eval-rst}
.. autoclass:: samplekit.Project
   :members:

.. autoclass:: samplekit.Model
   :members:

.. autoclass:: samplekit.Collection
   :members:

.. autoclass:: samplekit.Query
   :members:

.. autoclass:: samplekit.Profile
   :members:

.. autoclass:: samplekit.ColumnSpec
   :members:

.. autoclass:: samplekit.Export
   :members:

.. autoclass:: samplekit.FigureDeclaration
   :members:

.. autoclass:: samplekit.Render
   :members:

.. autoclass:: samplekit.Unit
   :members:

.. autoclass:: samplekit.PropertyDeclaration
   :members:

.. autoclass:: samplekit.Names
   :members:

.. autoclass:: samplekit.Field
   :members:
```
