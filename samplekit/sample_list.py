"""SampleList — collection of samples with filtering, sorting, and conversion."""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Callable

from .sample import Sample


class SampleList:
    """
    Collection of Sample objects loaded from a directory, file list, or built
    programmatically.

    Parameters
    ----------
    source : list[Sample] | list[Path|str] | Path | str | None
        - None: empty collection
        - list[Sample]: direct objects
        - list[Path|str]: paths to .md files
        - Path/str: directory to glob for .md files
    sample_class : type
        Sample subclass for loading from paths (default: Sample).
    pattern : str
        Glob pattern when loading from a directory (default: "*.md").

    Examples
    --------
    >>> samples = SampleList("data/", sample_class=MySample)
    >>> high = samples.filter(lambda s: s["temperature"].value > 30)
    >>> sorted_s = high.sort("temperature")
    >>> df = sorted_s.to_dataframe()
    """

    def __init__(
        self,
        source=None,
        sample_class: type = Sample,
        pattern: str = "*.md",
    ):
        self._samples: list[Sample] = []
        self._sample_class = sample_class

        if source is None:
            pass
        elif isinstance(source, (str, Path)):
            self._load_directory(Path(source), pattern)
        elif isinstance(source, list):
            for item in source:
                if isinstance(item, Sample):
                    self._samples.append(item)
                elif isinstance(item, (str, Path)):
                    self._load_file(Path(item))
        else:
            raise TypeError(f"Unsupported source type: {type(source)}")

    def _load_directory(self, directory: Path, pattern: str):
        for f in sorted(directory.glob(pattern)):
            self._load_file(f)

    def _load_file(self, filepath: Path):
        try:
            self._samples.append(self._sample_class.load(filepath))
        except Exception as e:
            print(f"Warning: {filepath}: {e}", file=sys.stderr)

    # ── Mutation ────────────────────────────────────────

    def append(self, sample: Sample | str | Path):
        """Add a sample (object or file path)."""
        if isinstance(sample, Sample):
            self._samples.append(sample)
        else:
            self._load_file(Path(sample))

    # ── Filtering & sorting ─────────────────────────────

    def filter(self, func: Callable[[Sample], bool]) -> SampleList:
        """Return a new SampleList with samples matching the predicate."""
        result = SampleList(sample_class=self._sample_class)
        result._samples = [s for s in self._samples if func(s)]
        return result

    def sort(
        self,
        key: str | Callable | list,
        reverse: bool | list[bool] = False,
    ) -> SampleList:
        """Return a sorted SampleList.

        Parameters
        ----------
        key : str, callable, or list
            - str: property name (sorts by value)
            - callable: sort key function
            - list: multi-key stable sort (applied in reverse order)
        reverse : bool or list[bool]
        """
        result = SampleList(sample_class=self._sample_class)

        if isinstance(key, list):
            # Multi-key stable sort
            reverse_list = reverse if isinstance(reverse, list) else [reverse] * len(key)
            samples = list(self._samples)
            for k, rev in zip(reversed(key), reversed(reverse_list)):
                sort_fn = self._make_sort_key(k)
                samples = sorted(samples, key=sort_fn, reverse=rev)
            result._samples = samples
        else:
            sort_fn = self._make_sort_key(key)
            rev = reverse[0] if isinstance(reverse, list) else reverse
            result._samples = sorted(self._samples, key=sort_fn, reverse=rev)

        return result

    @staticmethod
    def _make_sort_key(key) -> Callable:
        if isinstance(key, str):
            prop_name = key
            def _key(s: Sample):
                try:
                    v = s[prop_name].value
                    return v if v is not None else float('-inf')
                except (KeyError, AttributeError):
                    return float('-inf')
            return _key
        return key

    # ── Access ──────────────────────────────────────────

    def __getitem__(self, index):
        if isinstance(index, int):
            return self._samples[index]
        if isinstance(index, slice):
            result = SampleList(sample_class=self._sample_class)
            result._samples = self._samples[index]
            return result
        if isinstance(index, str):
            for s in self._samples:
                if s.name == index:
                    return s
            raise KeyError(index)
        raise TypeError(f"Invalid index type: {type(index)}")

    def __len__(self) -> int:
        return len(self._samples)

    def __iter__(self):
        return iter(self._samples)

    def __bool__(self) -> bool:
        return len(self._samples) > 0

    # ── Converters (delegated to samplekit.converters) ──

    def __getattr__(self, name: str):
        if name.startswith('_'):
            raise AttributeError(name)
        from . import converters
        fn = getattr(converters, f"samplelist_{name}", None)
        if fn is not None:
            return lambda *args, **kw: fn(self, *args, **kw)
        raise AttributeError(f"'{type(self).__name__}' has no attribute {name!r}")

    # ── I/O ─────────────────────────────────────────────

    def save_all(
        self,
        directory: str | Path,
        style: str = "math",
        overwrite: bool = False,
    ) -> list[Path]:
        """Save all samples to a directory as individual .md files."""
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True)
        paths = []
        for s in self._samples:
            fp = directory / f"{s.name}.md"
            if fp.exists() and not overwrite:
                raise FileExistsError(f"{fp} already exists")
            s.save(fp, style=style)
            paths.append(fp)
        return paths

    # ── Display ─────────────────────────────────────────

    def __repr__(self):
        return f"<SampleList with {len(self._samples)} samples>"
