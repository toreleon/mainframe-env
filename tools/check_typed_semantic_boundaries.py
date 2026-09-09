#!/usr/bin/env python3
"""Fail when a migrated typed-HIR path regains runtime grammar parsing."""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path


class BoundaryError(RuntimeError):
    """A typed semantic boundary regressed."""


def read(root: Path, relative: str) -> str:
    path = root / relative
    if not path.is_file():
        raise BoundaryError(f"typed semantic boundary file is missing: {relative}")
    return path.read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise BoundaryError(message)


def production(source: str) -> str:
    return source.split("#[cfg(test)]", 1)[0]


def between(source: str, start: str, end: str) -> str:
    require(start in source and end in source, f"typed boundary markers drifted: {start} / {end}")
    return source.split(start, 1)[1].split(end, 1)[0]


def reject(source: str, patterns: list[str], scope: str) -> None:
    for pattern in patterns:
        require(pattern not in source, f"{scope} contains forbidden runtime grammar path {pattern}")


def check(root: Path) -> None:
    compiler_manifest = read(root, "crates/kernel/mainframe-env-compiler/Cargo.toml")
    require(
        "mainframe-env-host-api" not in compiler_manifest,
        "the language frontend must not depend on the host-provider contract",
    )

    decimal_hir = production(
        read(root, "crates/kernel/mainframe-env-compiler/src/hir/decimal.rs")
    )
    for required in [
        'DECIMAL_NAMESPACE: &str = "mainframe.decimal"',
        'ASSIGNMENT_PLAN_ATTRIBUTE: &str = "assignment_plan"',
        'CONDITION_STATUS_ATTRIBUTE: &str = "typed_condition_status"',
        'CONDITION_BRANCHES_ATTRIBUTE: &str = "typed_condition_branches"',
        '"cobol.add@1"',
        '"cobol.compute@1"',
        "encode_decimal_assignment_plan",
    ]:
        require(required in decimal_hir, f"typed decimal compiler boundary omits {required}")

    cics_hir = production(read(root, "crates/kernel/mainframe-env-compiler/src/hir/cics.rs"))
    for required in [
        'CICS_PLAN_ATTRIBUTE: &str = "cics_plan"',
        '("cics.file", "read")',
        '("cics.file", "rewrite")',
        '("cics.recovery", "syncpoint")',
        'schema.runtime_import = Some("host.cics".into())',
        "encode_cics_effect_plan",
    ]:
        require(required in cics_hir, f"typed CICS compiler boundary omits {required}")

    lower = production(read(root, "crates/kernel/mainframe-env-compiler/src/lower.rs"))
    for required in [
        "crate::hir::decimal::encode_statement",
        "crate::hir::cics::encode_statement",
        'operation_attributes.remove("control_text")',
        "typed_size_error_branch_polarities",
        "CONDITION_POLARITY_ATTRIBUTE",
    ]:
        require(required in lower, f"typed lowering boundary omits {required}")

    decimal_runtime = production(
        read(root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_decimal.rs")
    )
    reject(
        decimal_runtime,
        [
            "position(",
            "decode_arguments(",
            "has_condition_handler(",
            '"TO"',
            '"GIVING"',
            '"ROUNDED"',
            '"ON SIZE ERROR"',
            '"NOT ON SIZE ERROR"',
        ],
        "typed decimal runtime",
    )
    for required in [
        "decode_decimal_assignment_plan",
        "validate_declared_slots",
        "let mut staged",
        "cobol.add@1",
        "cobol.compute@1",
        "typed_condition_status",
        "typed_condition_branches",
        "typed_condition_polarity",
        "pub(super) fn control_branch",
    ]:
        require(required in decimal_runtime, f"typed decimal runtime omits {required}")
    decimal_condition = between(
        decimal_runtime,
        "pub(super) fn control_branch",
        "fn validate_declared_slots",
    )
    reject(
        decimal_condition,
        ['"control_text"', "has_condition_handler("],
        "typed decimal condition runtime",
    )

    cics_runtime = production(
        read(root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics.rs")
    )
    typed_execute = between(
        cics_runtime,
        "pub(super) fn execute(\n",
        "pub(super) fn execute_legacy(\n",
    )
    reject(
        typed_execute,
        [
            "CicsOperation::from_tokens",
            "legacy_arguments",
            "legacy_destination",
            "split_whitespace",
            "reference(",
            "arguments(",
        ],
        "typed CICS runtime",
    )
    validation = between(cics_runtime, "fn plan(operation: &Operation)", "pub(super) fn legacy_arguments")
    reject(
        validation,
        ["CicsOperation::from_tokens", "legacy_arguments(", "legacy_destination("],
        "typed CICS validation",
    )
    for required in [
        "decode_cics_effect_plan",
        '(FILE_NAMESPACE, "read", 1)',
        '(FILE_NAMESPACE, "rewrite", 1)',
        '(RECOVERY_NAMESPACE, "syncpoint", 1)',
        "expected_effects",
    ]:
        require(required in cics_runtime, f"typed CICS runtime omits {required}")

    machine = production(
        read(root, "crates/kernel/mainframe-env-interpreter/src/machine.rs")
    )
    require(
        machine.index("if typed_cics::is_typed(operation)")
        < machine.index("let name = operation.identity.name()"),
        "typed CICS dispatch must precede name-only legacy dispatch",
    )
    require(
        "operations.extend(typed_cics::operation_identities())" in machine
        and "operations.insert(typed_decimal::operation_identity())" in machine,
        "reference-machine dialect registry omits a typed operation family",
    )
    decimal_branch = between(machine, "    fn control_branch", "    fn prepare_search")
    require(
        decimal_branch.index("typed_decimal::control_branch(self, operation)")
        < decimal_branch.index('text_attribute(operation, "control_text")'),
        "typed decimal condition dispatch must precede legacy branch-text parsing",
    )
    reject(
        decimal_branch,
        ['Some("add" | "assign"', '"ON SIZE ERROR",\n                    Some("assign"'],
        "typed decimal branch dispatch",
    )

    artifact = read(root, "crates/contracts/mainframe-env-compiler-api/src/lib.rs")
    require(
        'ARTIFACT_CONTRACT: &str = "mainframe-env.artifact@3"' in artifact
        and 'LEGACY_ARTIFACT_CONTRACT: &str = "mainframe-env.artifact@2"' in artifact,
        "artifact compatibility versions are not explicit",
    )
    artifact_model = production(
        read(root, "crates/contracts/mainframe-env-compiler-api/src/artifact.rs")
    )
    require(
        "pub dialect_contracts: BTreeSet<String>" in artifact_model
        and "manifest.dialect_contracts != payload_dialects" in artifact_model,
        "artifact publication does not bind exact payload dialect versions",
    )
    for required in [
        "pub struct ArtifactManifestV2",
        "pub struct ValidatedArtifact",
        "VersionedArtifactManifest::V2",
        "legacy.migrate(payload_dialects)",
        "verify_legal(module, catalog, profile)",
    ]:
        require(required in artifact_model, f"artifact compatibility reader omits {required}")
    release = tomllib.loads(read(root, "release.toml"))
    versions = json.loads(read(root, "conformance/0.2/inventory/versions.json"))
    require(
        release.get("contracts", {}).get("artifact") == "mainframe-env.artifact@3"
        and versions.get("contracts", {}).get("artifact") == "mainframe-env.artifact@3",
        "current release and conformance artifact contract authorities must both declare @3",
    )

    jcl = read(root, "crates/apps/mainframe-env-batch/src/jcl_schema.rs")
    batch_manifest = read(root, "crates/apps/mainframe-env-batch/Cargo.toml")
    require(
        'JCL_PLAN_CONTRACT: &str = "mainframe-env.jcl-job-plan@1"' in jcl
        and "mainframe-env-compiler" not in batch_manifest,
        "the independent JCL typed-plan frontend is coupled to COBOL HIR",
    )

    resources = read(root, "crates/kernel/mainframe-env-application/src/package_v1.rs")
    application_manifest = read(root, "crates/kernel/mainframe-env-application/Cargo.toml")
    require(
        'APPLICATION_PACKAGE_CONTRACT: &str = "mainframe-env.application-package@1"'
        in resources
        and "pub fn parse_bms(" in resources
        and "pub fn parse_csd(" in resources
        and "mainframe-env-compiler" not in application_manifest
        and "mainframe-env-interpreter" not in application_manifest,
        "resource-definition parsing must remain outside COBOL HIR and the program VM",
    )


def main() -> None:
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]
    check(root)
    print("typed-semantic-boundaries: pass")


if __name__ == "__main__":
    main()
