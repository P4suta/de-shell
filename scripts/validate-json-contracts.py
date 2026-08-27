#!/usr/bin/env python3
"""Validate generated de-shell v1 documents with an independent engine."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import tempfile
import tomllib


def command(arguments: list[str], *, stdin: str | None = None) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        arguments,
        input=stdin,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(arguments)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed


def write_json(path: pathlib.Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("--validator", default="check-jsonschema")
    arguments = parser.parse_args()

    repository = pathlib.Path(__file__).resolve().parent.parent
    binary = arguments.binary.resolve(strict=True)
    schema_root = repository / "contracts" / "schema"
    schemas = sorted(schema_root.glob("*-v1.schema.json"))
    command([arguments.validator, "--check-metaschema", *map(str, schemas)])
    for schema in schemas:
        name = schema.name.removesuffix("-v1.schema.json")
        embedded = command([str(binary), "schema", name]).stdout.encode()
        if embedded != schema.read_bytes():
            raise RuntimeError(f"embedded schema bytes are stale for {name}")

    with tempfile.TemporaryDirectory(prefix="deshell-schema-validation-") as raw_root:
        root = pathlib.Path(raw_root)
        (root / "entry.sh").write_bytes(b"#!/bin/sh\nprintf schema-validation\n")
        command([str(binary), "init", "--root", str(root), "--entry", "entry.sh"])

        generated = root / "generated"
        generated.mkdir()
        inventory = command(
            [str(binary), "scan", "--root", str(root), "--format", "json"]
        ).stdout
        (generated / "inventory.json").write_text(inventory, encoding="utf-8")
        command([str(binary), "analyze", "--root", str(root)])

        plans = list((root / ".deshell" / "artifacts").glob("*/*/plan.json"))
        evidence = list((root / ".deshell" / "artifacts").glob("*/*/evidence.json"))
        if len(plans) != 1 or len(evidence) != 1:
            raise RuntimeError("analysis did not create exactly one plan/evidence pair")

        write_json(
            generated / "project.json",
            tomllib.loads((root / ".deshell" / "project.toml").read_text(encoding="utf-8")),
        )
        write_json(
            generated / "scenario.json",
            tomllib.loads(
                (root / ".deshell" / "scenarios" / "default.toml").read_text(
                    encoding="utf-8"
                )
            ),
        )
        write_json(
            generated / "lock.json",
            tomllib.loads((root / "deshell.lock").read_text(encoding="utf-8")),
        )

        handshake = command(
            [str(binary), "__process-agent"],
            stdin=(
                '{"id":1,"jsonrpc":"2.0","method":"deshell.handshake",'
                '"params":{"protocol_version":1}}\n'
            ),
        ).stdout
        (generated / "protocol.json").write_text(handshake, encoding="utf-8")

        failed = subprocess.run(
            [
                str(binary),
                "check",
                "--root",
                str(root / "missing"),
                "--diagnostics",
                "jsonl",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        diagnostics = [line for line in failed.stderr.splitlines() if line]
        if failed.returncode == 0 or len(diagnostics) != 1:
            raise RuntimeError("could not generate exactly one failure diagnostic")
        (generated / "diagnostic.json").write_text(diagnostics[0] + "\n", encoding="utf-8")

        instances = [
            ("inventory-v1.schema.json", generated / "inventory.json"),
            ("manifest-v1.schema.json", root / ".deshell" / "manifest.json"),
            ("effect-ir-v1.schema.json", plans[0]),
            ("evidence-v1.schema.json", evidence[0]),
            ("project-v1.schema.json", generated / "project.json"),
            ("scenario-v1.schema.json", generated / "scenario.json"),
            ("lock-v1.schema.json", generated / "lock.json"),
            ("protocol-v1.schema.json", generated / "protocol.json"),
            ("diagnostic-v1.schema.json", generated / "diagnostic.json"),
        ]
        for schema, instance in instances:
            command(
                [
                    arguments.validator,
                    "--schemafile",
                    str(schema_root / schema),
                    str(instance),
                ]
            )

    print(f"validated {len(schemas)} schemas and {len(instances)} generated documents")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
