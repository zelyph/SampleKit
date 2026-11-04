"""`--verify`: the copy computed with the migrated model, each value beside the
one the earlier version computed. Nothing is written: what differs is listed,
never corrected."""

from __future__ import annotations

import math
from pathlib import Path

from . import description as d


def same(now, then) -> bool:
    if then is None:
        return now is None
    return isinstance(now, (int, float)) and math.isclose(now, then, rel_tol=1e-9, abs_tol=1e-12)


def verify(project: d.Project, target: Path) -> int:
    import samplekit as sk

    if project.version != "0.x":
        print("verify: the files of rc.1 are copied as they are; there is nothing to compare")
        return 0
    counts = {"agree": 0, "differ": 0}
    print()
    print("Each value the migrated model computes, beside the one 0.x wrote:")

    def compare(sample: d.Sample, field: str, read, then: d.Quantity) -> None:
        try:
            now = read()
        except Exception as error:  # noqa: BLE001 - said, the next goes on
            counts["differ"] += 1
            print(f"  {sample.path}  {field}: {type(error).__name__}: {error}")
            return
        value = getattr(now, "value", now)
        uncertainty = getattr(now, "uncertainty", None)
        if same(value, then.value) and (then.uncertainty is None or same(uncertainty, then.uncertainty)):
            counts["agree"] += 1
            return
        counts["differ"] += 1
        print(
            f"  {sample.path}  {field}: 0.x {then.value!r} ± {then.uncertainty!r}, "
            f"now {value!r} ± {uncertainty!r}"
        )

    for sample in project.samples:
        try:
            computed = sk.Sample(target / sample.path)
            computed.compute()
        except Exception as error:  # noqa: BLE001 - said, the next sample goes on
            print(f"  {sample.path}: not computed, {type(error).__name__}: {error}")
            counts["differ"] += 1
            continue
        for name, quantity in sample.properties.items():
            if quantity.origin == d.COMPUTED and quantity.value is not None:
                compare(sample, name, lambda n=name: computed[n], quantity)
        for name, table in sample.tables.items():
            index = table.index[0]
            for row in table.rows:
                at = row.get(index)
                for column, cell in row.items():
                    if cell.origin != d.COMPUTED or cell.value is None or at is None:
                        continue
                    compare(
                        sample,
                        f"{name}.{column}[{at.value}]",
                        lambda t=name, i=at.value, c=column: computed[t].at(i, c),
                        cell,
                    )
    print(f"{counts['agree']} values agree with 0.x's, {counts['differ']} differ or failed")
    return 1 if counts["differ"] else 0
