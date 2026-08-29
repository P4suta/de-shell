#!/usr/bin/env python3
"""Enforce the de-shell 0.1.0 release-only line coverage contract."""

from __future__ import annotations

import json
import math
import pathlib
import sys
from typing import Any


MINIMUM = 90.0
REQUIRED_MODULES = (
    "scanner.rs",
    "frontend.rs",
    "runner.rs",
    "protocol.rs",
    "lab.rs",
    "patch.rs",
)


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON key: {key}")
        value[key] = item
    return value


def line_percent(summary: Any, label: str) -> float:
    if not isinstance(summary, dict):
        raise ValueError(f"{label} summary is not an object")
    lines = summary.get("lines")
    if not isinstance(lines, dict):
        raise ValueError(f"{label} lines summary is missing")
    percent = lines.get("percent")
    if not isinstance(percent, (int, float)) or isinstance(percent, bool):
        raise ValueError(f"{label} line percent is not numeric")
    percent = float(percent)
    if not math.isfinite(percent) or not 0.0 <= percent <= 100.0:
        raise ValueError(f"{label} line percent is outside 0..100")
    return percent


def validate(document: Any) -> list[tuple[str, float]]:
    if not isinstance(document, dict) or document.get("type") != "llvm.coverage.json.export":
        raise ValueError("input is not an LLVM coverage JSON export")
    data = document.get("data")
    if not isinstance(data, list) or len(data) != 1 or not isinstance(data[0], dict):
        raise ValueError("coverage export must contain exactly one aggregate")
    aggregate = data[0]
    results = [("overall", line_percent(aggregate.get("totals"), "overall"))]
    files = aggregate.get("files")
    if not isinstance(files, list):
        raise ValueError("coverage export files are missing")
    by_name: dict[str, float] = {}
    for item in files:
        if not isinstance(item, dict) or not isinstance(item.get("filename"), str):
            raise ValueError("coverage file entry is invalid")
        name = item["filename"].replace("\\", "/")
        for module in REQUIRED_MODULES:
            suffix = f"/crates/deshell/src/{module}"
            if name.endswith(suffix):
                if module in by_name:
                    raise ValueError(f"coverage export repeats {module}")
                by_name[module] = line_percent(item.get("summary"), module)
    missing = [module for module in REQUIRED_MODULES if module not in by_name]
    if missing:
        raise ValueError(f"coverage export omitted required modules: {', '.join(missing)}")
    results.extend((module, by_name[module]) for module in REQUIRED_MODULES)
    return results


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check-release-coverage.py LLVM_COVERAGE_JSON", file=sys.stderr)
        return 2
    path = pathlib.Path(sys.argv[1])
    try:
        document = json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
        results = validate(document)
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        print(f"release coverage input is invalid: {error}", file=sys.stderr)
        return 2
    failures = []
    for label, percent in results:
        print(f"{label}: {percent:.2f}%")
        if percent < MINIMUM:
            failures.append(f"{label}={percent:.2f}%")
    if failures:
        print(
            f"release coverage requires at least {MINIMUM:.0f}%: {', '.join(failures)}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
