"""Converters — transform Samples to/from dicts, DataFrames, and other formats."""

from __future__ import annotations

from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from .sample import Sample
    from .sample_list import SampleList


# ── Sample converters ───────────────────────────────────


def sample_to_dict(sample: Sample) -> dict[str, Any]:
    """Export all sample data as a nested dict.

    Parameters
    ----------
    sample : Sample
        The sample to export.

    Returns
    -------
    dict
        ``{"name": ..., "prop_name": {"value": ..., "uncertainty": ..., ...}, ...}``
    """
    result: dict[str, Any] = {"name": sample.name}

    for key, prop in sample.props.items():
        entry: dict[str, Any] = {}
        if prop.value is not None:
            entry["value"] = prop.value
        if prop.uncertainty is not None:
            entry["uncertainty"] = prop.uncertainty
        if prop.unit:
            entry["unit"] = prop.unit
        if prop.data is not None:
            entry["data"] = prop.data
        if entry:
            result[key] = entry

    for key, table in sample.tables.items():
        result[key] = table.to_yaml()

    return result


def sample_to_dataframe(sample: Sample):
    """Export scalar properties as a single-row pandas DataFrame.

    Parameters
    ----------
    sample : Sample
        The sample to export.

    Returns
    -------
    pandas.DataFrame
        One row indexed by sample name, columns are numeric properties.
    """
    import pandas as pd
    data: dict[str, Any] = {}
    for name, prop in sample.props.items():
        v = prop.value
        if v is not None and not isinstance(v, str):
            data[name] = v
            if prop.uncertainty is not None:
                data[f"{name}_unc"] = prop.uncertainty
    return pd.DataFrame(data, index=[sample.name])


# ── SampleList converters ───────────────────────────────


def samplelist_to_dataframe(sample_list: SampleList):
    """Concatenate all samples into a single DataFrame (samples as rows).

    Parameters
    ----------
    sample_list : SampleList
        The collection to export.

    Returns
    -------
    pandas.DataFrame
    """
    import pandas as pd
    frames = [sample_to_dataframe(s) for s in sample_list]
    return pd.concat(frames) if frames else pd.DataFrame()


def samplelist_stats(sample_list: SampleList, prop_name: str):
    """Descriptive statistics for a property across all samples.

    Parameters
    ----------
    sample_list : SampleList
        The collection to analyze.
    prop_name : str
        Name of the property to gather statistics on.

    Returns
    -------
    pandas.Series
        Output of ``pandas.Series.describe()``.
    """
    import pandas as pd
    values = []
    for s in sample_list:
        try:
            v = s[prop_name].value
            if v is not None and not isinstance(v, str):
                values.append(float(v))
        except (KeyError, TypeError, ValueError):
            pass
    return pd.Series(values, name=prop_name).describe()
