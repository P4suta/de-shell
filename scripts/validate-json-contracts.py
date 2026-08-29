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


def report_command(
    arguments: list[str], *, exits: tuple[int, ...] = (0,)
) -> tuple[subprocess.CompletedProcess[str], dict[str, object]]:
    completed = subprocess.run(arguments, text=True, capture_output=True, check=False)
    if completed.returncode not in exits or completed.stderr:
        raise RuntimeError(
            f"report command failed ({completed.returncode}): {' '.join(arguments)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed, json.loads(completed.stdout)


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
        (root / "entry.sh").write_bytes(b"#!/bin/sh\n/usr/bin/printf schema-validation\n")
        generated = root / "generated"
        generated.mkdir()
        _, init_report = report_command(
            [
                str(binary),
                "init",
                "--root",
                str(root),
                "--entry",
                "entry.sh",
                "--target",
                "rust",
                "--format",
                "json",
            ]
        )
        write_json(generated / "init-report.json", init_report)
        _, scan_report = report_command(
            [str(binary), "scan", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "scan-report.json", scan_report)
        _, analyze_report = report_command(
            [str(binary), "analyze", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "analyze-report.json", analyze_report)

        plans = list((root / ".deshell" / "artifacts").glob("*/*/plan.json"))
        evidence = list((root / ".deshell" / "artifacts").glob("*/*/evidence.json"))
        if len(plans) != 1 or len(evidence) != 1:
            raise RuntimeError("analysis did not create exactly one plan/evidence pair")

        write_json(
            generated / "project.json",
            tomllib.loads((root / ".deshell" / "project.toml").read_text(encoding="utf-8")),
        )
        scenario_path = next((root / ".deshell" / "scenarios").glob("*.toml"))
        write_json(generated / "scenario.json", tomllib.loads(scenario_path.read_text(encoding="utf-8")))
        write_json(
            generated / "lock.json",
            tomllib.loads((root / "deshell.lock").read_text(encoding="utf-8")),
        )

        write_json(
            generated / "generator-protocol.json",
            {
                "schema_version": 1,
                "protocol": "deshell.generator.v1",
                "generator": {
                    "name": "deshell-official",
                    "version": "0.1.0",
                    "digest": "sha256:" + "0" * 64,
                    "capabilities": ["rust", "go", "host"],
                },
                "max_frame_bytes": 4 * 1024 * 1024,
            },
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

        _, scenario_report = report_command(
            [str(binary), "scenario", "list", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "scenario-report.json", scenario_report)
        scenario_line = next(
            line for line in scenario_report["details"]["output"] if "\tdraft\t" in line
        )
        scenario_name, _, scenario_digest, _ = scenario_line.split("\t")
        report_command(
            [
                str(binary), "scenario", "approve", "--root", str(root),
                "--name", scenario_name, "--digest", scenario_digest, "--format", "json",
            ]
        )
        _, matrix_report = report_command(
            [str(binary), "matrix", "list", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "matrix-report.json", matrix_report)
        matrix_line = next(
            line for line in matrix_report["details"]["output"] if "\tdraft\t" in line
        )
        cell, _, matrix_digest = matrix_line.split("\t")
        report_command(
            [
                str(binary), "matrix", "approve", "--root", str(root),
                "--cell", cell, "--digest", matrix_digest, "--format", "json",
            ]
        )
        _, check_report = report_command(
            [str(binary), "check", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "check-report.json", check_report)
        _, verify_report = report_command(
            [str(binary), "verify", "--root", str(root), "--format", "json"],
            exits=(3, 6),
        )
        write_json(generated / "verify-report.json", verify_report)
        _, observe_report = report_command(
            [str(binary), "observe", "--root", str(root), "--format", "json"],
            exits=(6,),
        )
        write_json(generated / "observe-report.json", observe_report)
        _, explain_report = report_command(
            [str(binary), "explain", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "explain-report.json", explain_report)
        _, rewrite_report = report_command(
            [
                str(binary), "rewrite", "--root", str(root), "--equivalent",
                "--format", "json",
            ]
        )
        write_json(generated / "rewrite-report.json", rewrite_report)
        _, modernize_report = report_command(
            [
                str(binary), "modernize", "--root", str(root), "--profile", "secure",
                "--format", "json",
            ]
        )
        write_json(generated / "modernize-report.json", modernize_report)
        _, plan_report = report_command(
            [str(binary), "migrate", "plan", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "migrate-report.json", plan_report)
        plan_output = plan_report["details"]["output"]
        plan_digest = next(
            line.removeprefix("plan ")
            for line in plan_output
            if line.startswith("plan ")
        )
        migration = root / ".deshell" / "migrations" / "sha256" / plan_digest
        requests = list((migration / "requests").glob("*.json"))
        proposals = list((migration / "proposals").glob("*.json"))
        if len(requests) != 1 or len(proposals) != 1:
            raise RuntimeError("migration did not create exactly one request/proposal pair")
        migration_evidence = generated / "migration-evidence.json"
        _, verify_migration_report = report_command(
            [
                str(binary),
                "migrate",
                "verify",
                "--root",
                str(root),
                "--plan",
                plan_digest,
                "--cell",
                cell,
                "--output",
                str(migration_evidence),
                "--format",
                "json",
            ]
        )
        command(
            [
                str(binary),
                "migrate",
                "evidence",
                "import",
                "--root",
                str(root),
                "--plan",
                plan_digest,
                str(migration_evidence),
            ]
        )

        approval_artifact = next((root / ".deshell" / "approvals" / "sha256").glob("*.json"))
        command(
            [
                str(binary),
                "migrate",
                "apply",
                "--root",
                str(root),
                "--plan",
                plan_digest,
            ]
        )
        _, retired_scan = report_command(
            [str(binary), "scan", "--root", str(root), "--format", "json"]
        )
        if retired_scan["details"]["counts"].get("findings") != 0:
            raise RuntimeError("retirement did not produce a zero-finding scan")
        _, shell_free = report_command(
            [
                str(binary),
                "verify",
                "--root",
                str(root),
                "--require",
                "shell-free",
                "--format",
                "json",
            ]
        )
        if shell_free["status"] != "ok":
            raise RuntimeError("retirement did not satisfy the shell-free gate")

        (root / "risk.sh").write_text('eval "$command"\n', encoding="utf-8")
        audit = subprocess.run(
            [str(binary), "audit", "--root", str(root), "--format", "jsonl"],
            text=True,
            capture_output=True,
            check=False,
        )
        audit_lines = [line for line in audit.stdout.splitlines() if line]
        if audit.returncode == 0 or not audit_lines:
            raise RuntimeError("could not generate an audit finding")
        (generated / "audit-finding.json").write_text(
            audit_lines[0] + "\n", encoding="utf-8"
        )
        _, audit_report = report_command(
            [str(binary), "audit", "--root", str(root), "--format", "json"],
            exits=(4,),
        )
        write_json(generated / "audit-report.json", audit_report)

        _, doctor_report = report_command(
            [str(binary), "doctor", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "doctor-report.json", doctor_report)

        _, harden_report = report_command(
            [str(binary), "harden", "plan", "--root", str(root), "--format", "json"]
        )
        write_json(generated / "harden-report.json", harden_report)
        harden_output = harden_report["details"]["output"]
        harden_digest = next(
            line.removeprefix("harden plan ")
            for line in harden_output
            if line.startswith("harden plan ")
        )
        hardening = root / ".deshell" / "hardening"
        harden_plan = hardening / "sha256" / harden_digest / "plan.json"
        harden_approval = hardening / "approvals" / f"{harden_digest}.json"
        approval_value = json.loads(harden_approval.read_text(encoding="utf-8"))
        approval_value.update(
            {
                "approval": "approved",
                "owner": "schema-validator",
                "reason": "validate the independent hardening Evidence contract",
            }
        )
        write_json(harden_approval, approval_value)
        command(
            [
                str(binary),
                "harden",
                "verify",
                "--root",
                str(root),
                "--plan",
                harden_digest,
            ]
        )
        harden_evidence = hardening / "sha256" / harden_digest / "evidence.json"

        instances = [
            ("approval-v1.schema.json", approval_artifact),
            ("migration-index-v1.schema.json", root / ".deshell" / "migrations" / "active.json"),
            ("init-report-v1.schema.json", generated / "init-report.json"),
            ("scan-report-v1.schema.json", generated / "scan-report.json"),
            ("scenario-report-v1.schema.json", generated / "scenario-report.json"),
            ("matrix-report-v1.schema.json", generated / "matrix-report.json"),
            ("audit-report-v1.schema.json", generated / "audit-report.json"),
            ("analyze-report-v1.schema.json", generated / "analyze-report.json"),
            ("check-report-v1.schema.json", generated / "check-report.json"),
            ("verify-report-v1.schema.json", generated / "verify-report.json"),
            ("observe-report-v1.schema.json", generated / "observe-report.json"),
            ("doctor-report-v1.schema.json", generated / "doctor-report.json"),
            ("explain-report-v1.schema.json", generated / "explain-report.json"),
            ("rewrite-report-v1.schema.json", generated / "rewrite-report.json"),
            ("modernize-report-v1.schema.json", generated / "modernize-report.json"),
            ("harden-report-v1.schema.json", generated / "harden-report.json"),
            ("migrate-report-v1.schema.json", generated / "migrate-report.json"),
            ("manifest-v1.schema.json", root / ".deshell" / "manifest.json"),
            ("effect-ir-v1.schema.json", plans[0]),
            ("evidence-v1.schema.json", evidence[0]),
            ("project-v1.schema.json", generated / "project.json"),
            ("scenario-v1.schema.json", generated / "scenario.json"),
            ("lock-v1.schema.json", generated / "lock.json"),
            ("protocol-v1.schema.json", generated / "protocol.json"),
            ("diagnostic-v1.schema.json", generated / "diagnostic.json"),
            ("generator-protocol-v1.schema.json", generated / "generator-protocol.json"),
            ("migration-request-v1.schema.json", requests[0]),
            ("proposal-v1.schema.json", proposals[0]),
            ("migration-plan-v1.schema.json", migration / "plan.json"),
            ("migration-evidence-v1.schema.json", migration_evidence),
            ("archive-manifest-v1.schema.json", root / ".deshell" / "archive" / "manifest.json"),
            ("audit-finding-v1.schema.json", generated / "audit-finding.json"),
            ("harden-plan-v1.schema.json", harden_plan),
            ("harden-approval-v1.schema.json", harden_approval),
            ("harden-evidence-v1.schema.json", harden_evidence),
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
